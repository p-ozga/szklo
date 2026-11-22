//! Declipper (spec v1 §4.1) – latencja 512·F.
//!
//! Praca ramkowa na kanał: skok H = 256·F, okno N_d = 1024·F, obszar zatwierdzany [n−511F, n−256F].
//! Detekcja (2 s): tryb twardy – serie ≥ 3 próbek tego samego znaku z |x| ≥ 0,98·M (różnice < 1e−4·M), ≥ 5 serii
//! → θ̂ = mediana poziomów serii (osobno + i −); tryb miękki (a > 0,5) – ρ = #(|x| ≥ 0,89M)/#(|x| ≥ 0,71M) > 0,35 → θ̂ = 0,89M.
//! Rekonstrukcja AR (Janssen, Veldhuis, Vries 1986), rząd 32·F, 2 iteracje: autokorelacja okna (Hann), Levinson–Durbin,
//! b[l] = Σ a[k]a[k+|l|], na serię B·x = −z (Cholesky). Spójność: |s| ≥ θ̂ ze znakiem; ograniczenie przyrostu E·tanh(e/E).
//! Serie 64F < m ≤ 192F → Hermite; dłuższe – bez zmian; > 35 % obciętych w oknie – bez zmian; ≤ 48 serii na skok.
//! Suwak: c(a) = lin(a, 0,999, 0,97) (twardy), 1 − 0,2(a − 0,5) (miękki); G_max = lin(a, 2, 6) dB; y = x + min(1,4a)(s − x).

use super::{lin, und};
use crate::catalog::ModuleId;
use crate::dsp::util::Smoother;
use crate::module::{Ctx, Module};
use std::collections::VecDeque;

const BINS: usize = 1680; // −60…+24 dBFS co 0,05 dB
const BIN_LO: f64 = -60.0;
const BIN_STEP: f64 = 0.05;

fn bin_of(x: f64) -> Option<usize> {
    if x <= 0.0 {
        return None;
    }
    let d = 20.0 * x.log10();
    if d < BIN_LO {
        return None;
    }
    Some((((d - BIN_LO) / BIN_STEP) as usize).min(BINS - 1))
}

/// Najwyżej tyle ostatnich serii „twardych” pamiętamy (mediana z najnowszych wystarcza; stała pojemność = brak
/// alokacji w wątku audio).
const RUNS_CAP: usize = 2048;
const NO_BIN: u16 = u16::MAX;

/// Statystyki z ostatnich 2 s. Wszystkie bufory mają stały rozmiar (prealokowane w `new`) – `push_hop`
/// i `levels` nie alokują.
struct Detect {
    /// maks. |x| każdego skoku (pierścień `hops_2s`)
    hop_max: Vec<f64>,
    /// nr kubełka histogramu każdej próbki z ostatnich 2 s (pierścień `hops_2s`·`hop`; NO_BIN = poniżej zakresu)
    bins: Vec<u16>,
    hist: Vec<u32>,
    runs: VecDeque<(u64, f64, bool)>, // (nr skoku, poziom, znak +)
    med: Vec<f64>,                    // bufor roboczy mediany
    hop_no: u64,
    hops_2s: usize,
    hop: usize,
    pos: usize,
    filled: usize,
}

impl Detect {
    fn new(hops_2s: usize, hop: usize) -> Self {
        let hops_2s = hops_2s.max(1);
        Self {
            hop_max: vec![0.0; hops_2s],
            bins: vec![NO_BIN; hops_2s * hop],
            hist: vec![0; BINS],
            runs: VecDeque::with_capacity(RUNS_CAP),
            med: Vec::with_capacity(RUNS_CAP),
            hop_no: 0,
            hops_2s,
            hop,
            pos: 0,
            filled: 0,
        }
    }
    fn m(&self) -> f64 {
        self.hop_max.iter().copied().fold(0.0, f64::max)
    }
    fn count_above(&self, x: f64) -> u64 {
        let b = match bin_of(x) {
            Some(b) => b,
            None => 0,
        };
        self.hist[b..].iter().map(|&c| c as u64).sum()
    }
    /// Dodaje nowy skok (próbki oryginalne, dokładnie `hop` sztuk) do statystyk. Bez alokacji.
    fn push_hop(&mut self, xs: &[f64]) {
        debug_assert_eq!(xs.len(), self.hop);
        let mx = xs.iter().fold(0.0f64, |a, &v| a.max(v.abs()));
        // pierścień: najstarszy skok (sprzed 2 s) wypada z histogramu, nowy wchodzi na jego miejsce
        let slot = &mut self.bins[self.pos * self.hop..(self.pos + 1) * self.hop];
        if self.filled == self.hops_2s {
            for &b in slot.iter() {
                if b != NO_BIN {
                    self.hist[b as usize] -= 1;
                }
            }
        }
        for (dst, &v) in slot.iter_mut().zip(xs) {
            *dst = match bin_of(v.abs()) {
                Some(b) => {
                    self.hist[b] += 1;
                    b as u16
                }
                None => NO_BIN,
            };
        }
        self.hop_max[self.pos] = mx;
        self.pos = (self.pos + 1) % self.hops_2s;
        self.filled = (self.filled + 1).min(self.hops_2s);
        // serie „twarde” liczone względem bieżącego M
        let m = self.m().max(mx);
        if m > 1e-6 {
            let thr = 0.98 * m;
            let mut i = 0;
            while i < xs.len() {
                if xs[i].abs() >= thr {
                    let sign = xs[i] > 0.0;
                    let mut j = i + 1;
                    while j < xs.len() && xs[j].abs() >= thr && (xs[j] > 0.0) == sign && (xs[j] - xs[i]).abs() < 1e-4 * m {
                        j += 1;
                    }
                    if j - i >= 3 {
                        if self.runs.len() >= RUNS_CAP {
                            self.runs.pop_front();
                        }
                        self.runs.push_back((self.hop_no, xs[i].abs(), sign));
                    }
                    i = j;
                } else {
                    i += 1;
                }
            }
        }
        self.hop_no += 1;
        let min_hop = self.hop_no.saturating_sub(self.hops_2s as u64);
        while self.runs.front().map(|r| r.0 < min_hop).unwrap_or(false) {
            self.runs.pop_front();
        }
    }
    /// (θ̂+, θ̂−) albo None.
    fn levels(&mut self, soft_allowed: bool) -> Option<(f64, f64)> {
        let m = self.m();
        if m < 1e-4 {
            return None;
        }
        if self.runs.len() >= 5 {
            // mediana przez wybór k-tego elementu w buforze roboczym (bez alokacji, bez paniki na NaN)
            let (runs, buf) = (&self.runs, &mut self.med);
            let mut med = |pos: bool| -> Option<f64> {
                buf.clear();
                buf.extend(runs.iter().filter(|r| r.2 == pos).map(|r| r.1));
                if buf.is_empty() {
                    return None;
                }
                let mid = buf.len() / 2;
                let (_, v, _) = buf.select_nth_unstable_by(mid, |a, b| a.total_cmp(b));
                Some(*v)
            };
            let p = med(true);
            let n = med(false);
            match (p, n) {
                (Some(p), Some(n)) => return Some((p, n)),
                (Some(p), None) => return Some((p, p)),
                (None, Some(n)) => return Some((n, n)),
                _ => {}
            }
        }
        if soft_allowed {
            let hi = self.count_above(0.89 * m) as f64;
            let lo = self.count_above(0.71 * m) as f64;
            if lo > 0.0 && hi / lo > 0.35 {
                return Some((0.89 * m, 0.89 * m));
            }
        }
        None
    }
}

/// Levinson–Durbin: r[0..=p] → a[0..=p] (a[0] = 1). `tmp` – bufor roboczy długości ≥ p+1 (bez alokacji).
fn levinson(r: &[f64], p: usize, a: &mut [f64], tmp: &mut [f64]) -> bool {
    a.iter_mut().for_each(|v| *v = 0.0);
    a[0] = 1.0;
    let mut err = r[0];
    if !(err > 0.0) || !err.is_finite() {
        return false;
    }
    for i in 1..=p {
        let mut acc = r[i];
        for j in 1..i {
            acc += a[j] * r[i - j];
        }
        let k = -acc / err;
        tmp[..=i].copy_from_slice(&a[..=i]);
        for j in 1..i {
            a[j] = tmp[j] + k * tmp[i - j];
        }
        a[i] = k;
        err *= 1.0 - k * k;
        if !(err > 1e-300) {
            return false;
        }
    }
    true
}

/// Cholesky in-place (dolna trójkątna) i rozwiązanie B·x = rhs.
fn cholesky_solve(b: &mut [f64], m: usize, rhs: &mut [f64]) -> bool {
    for j in 0..m {
        let mut d = b[j * m + j];
        for k in 0..j {
            d -= b[j * m + k] * b[j * m + k];
        }
        if !(d > 1e-300) {
            return false;
        }
        let d = d.sqrt();
        b[j * m + j] = d;
        for i in j + 1..m {
            let mut s = b[i * m + j];
            for k in 0..j {
                s -= b[i * m + k] * b[j * m + k];
            }
            b[i * m + j] = s / d;
        }
    }
    for i in 0..m {
        let mut s = rhs[i];
        for k in 0..i {
            s -= b[i * m + k] * rhs[k];
        }
        rhs[i] = s / b[i * m + i];
    }
    for i in (0..m).rev() {
        let mut s = rhs[i];
        for k in i + 1..m {
            s -= b[k * m + i] * rhs[k];
        }
        rhs[i] = s / b[i * m + i];
    }
    true
}

struct Chan {
    orig: Vec<f64>,
    s: Vec<f64>,
    out: Vec<f64>,
    det: Detect,
}

pub struct Declipper {
    ctx: Ctx,
    a: Smoother,
    nd: usize,
    hop: usize,
    p: usize,
    fill: usize,
    out_pos: usize,
    ch: [Chan; 2],
    hann: Vec<f64>,
    // bufory robocze
    r: Vec<f64>,
    ar: Vec<f64>,
    bcoef: Vec<f64>,
    mat: Vec<f64>,
    rhs: Vec<f64>,
    lev_tmp: Vec<f64>,
    runs_buf: Vec<(usize, usize)>,
    active: bool,
}

impl Declipper {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            nd: 1024,
            hop: 256,
            p: 32,
            fill: 0,
            out_pos: 0,
            ch: [Self::chan(16, 4, 1), Self::chan(16, 4, 1)], // atrapy – właściwe bufory tworzy `reset`
            hann: Vec::new(),
            r: Vec::new(),
            ar: Vec::new(),
            bcoef: Vec::new(),
            mat: Vec::new(),
            rhs: Vec::new(),
            lev_tmp: Vec::new(),
            runs_buf: Vec::new(),
            active: false,
        };
        s.reset(ctx);
        s
    }
    fn chan(nd: usize, hop: usize, hops_2s: usize) -> Chan {
        Chan { orig: vec![0.0; nd], s: vec![0.0; nd], out: vec![0.0; hop], det: Detect::new(hops_2s, hop) }
    }

    fn reconstruct(&mut self, c: usize, a: f64) {
        // lista serii w prealokowanym buforze (bez alokacji w wątku audio)
        let mut runs = std::mem::take(&mut self.runs_buf);
        runs.clear();
        self.reconstruct_runs(c, a, &mut runs);
        self.runs_buf = runs;
    }

    fn reconstruct_runs(&mut self, c: usize, a: f64, runs: &mut Vec<(usize, usize)>) {
        let (nd, hop, p) = (self.nd, self.hop, self.p);
        let f = self.ctx.f();
        let soft = a > 0.5;
        let Some((tp, tn)) = self.ch[c].det.levels(soft) else { return };
        let hard = self.ch[c].det.runs.len() >= 5;
        let cthr = if hard { lin(a, 0.999, 0.97) } else { 1.0 - 0.2 * (a - 0.5) };
        let gmax = und(lin(a, 2.0, 6.0));
        // obszar zatwierdzany w buforze: [nd − 512F, nd − 256F)
        let c0 = nd - 2 * hop;
        let c1 = nd - hop;
        let ch = &mut self.ch[c];
        let is_clipped = |x: f64| if x > 0.0 { x >= cthr * tp } else { -x >= cthr * tn };
        // udział obciętych w oknie
        let nclip = ch.orig.iter().filter(|&&x| is_clipped(x)).count();
        if nclip * 100 > nd * 35 {
            return;
        }
        // serie zaczynające się w obszarze zatwierdzanym
        let mut i = c0;
        // Seria rozpoczęta przed obszarem zatwierdzanym została obsłużona w całości w poprzednim skoku (albo celowo
        // pominięta: > 192·F, limit 48 serii, > 35 % obciętych). Jej ogona nie rekonstruujemy drugi raz – inaczej
        // koniec długiej „fali prostokątnej” byłby poprawiany, a początek (już oddany na wyjście) nie.
        if is_clipped(ch.orig[c0]) && is_clipped(ch.orig[c0 - 1]) && (ch.orig[c0] > 0.0) == (ch.orig[c0 - 1] > 0.0) {
            let sign = ch.orig[c0] > 0.0;
            while i < c1 && is_clipped(ch.orig[i]) && (ch.orig[i] > 0.0) == sign {
                i += 1;
            }
        }
        while i < c1 {
            if is_clipped(ch.orig[i]) {
                let sign = ch.orig[i] > 0.0;
                let mut j = i + 1;
                while j < nd && is_clipped(ch.orig[j]) && (ch.orig[j] > 0.0) == sign {
                    j += 1;
                }
                // rozszerzenie o 1 próbkę z każdej strony
                let (s0, s1) = (i.saturating_sub(1), (j + 1).min(nd));
                let ok_ctx = s0 >= p && s1 + p <= nd;
                if ok_ctx && (s1 - s0) <= 192 * f && runs.len() < runs.capacity() {
                    runs.push((s0, s1));
                }
                i = j;
            } else {
                i += 1;
            }
        }
        if runs.is_empty() {
            return;
        }
        runs.sort_unstable_by_key(|r| std::cmp::Reverse(r.1 - r.0)); // `sort_by_key` (stabilne) alokuje
        runs.truncate(48);
        let runs: &[(usize, usize)] = runs;
        // stan początkowy: oryginał w seriach (s zawiera już poprawki poprzednich skoków poza seriami)
        for &(s0, s1) in runs {
            ch.s[s0..s1].copy_from_slice(&ch.orig[s0..s1]);
        }
        let levels = |x: f64| if x > 0.0 { tp } else { tn };
        for _iter in 0..2 {
            // autokorelacja okna Hanna z bieżącymi estymatami
            self.r.resize(p + 1, 0.0);
            for lag in 0..=p {
                let mut acc = 0.0;
                for n in lag..nd {
                    acc += ch.s[n] * self.hann[n] * ch.s[n - lag] * self.hann[n - lag];
                }
                self.r[lag] = acc;
            }
            self.r[0] *= 1.0 + 1e-6;
            self.ar.resize(p + 1, 0.0);
            if !levinson(&self.r, p, &mut self.ar, &mut self.lev_tmp) {
                return;
            }
            self.bcoef.resize(p + 1, 0.0);
            for l in 0..=p {
                let mut acc = 0.0;
                for k in 0..=(p - l) {
                    acc += self.ar[k] * self.ar[k + l];
                }
                self.bcoef[l] = acc;
            }
            for &(s0, s1) in runs {
                let m = s1 - s0;
                if m > 64 * f {
                    // Hermite przez próbki brzegowe
                    let (y0, y1) = (ch.s[s0 - 1], ch.s[s1]);
                    let (d0, d1) = (ch.s[s0 - 1] - ch.s[s0 - 2], ch.s[s1 + 1] - ch.s[s1]);
                    let len = (m + 1) as f64;
                    for (k, t_idx) in (s0..s1).enumerate() {
                        let t = (k + 1) as f64 / len;
                        let (t2, t3) = (t * t, t * t * t);
                        ch.s[t_idx] = (2.0 * t3 - 3.0 * t2 + 1.0) * y0 + (t3 - 2.0 * t2 + t) * d0 * len + (-2.0 * t3 + 3.0 * t2) * y1 + (t3 - t2) * d1 * len;
                    }
                    continue;
                }
                self.mat.resize(m * m, 0.0);
                self.rhs.resize(m, 0.0);
                for ii in 0..m {
                    for jj in 0..m {
                        let d = (ii as isize - jj as isize).unsigned_abs();
                        self.mat[ii * m + jj] = if d <= p { self.bcoef[d] } else { 0.0 };
                    }
                    let ti = s0 + ii;
                    let mut z = 0.0;
                    let lo = ti.saturating_sub(p);
                    let hi = (ti + p).min(nd - 1);
                    for n in lo..=hi {
                        if n >= s0 && n < s1 {
                            continue;
                        }
                        z += self.bcoef[(ti as isize - n as isize).unsigned_abs()] * ch.s[n];
                    }
                    self.rhs[ii] = -z;
                }
                if cholesky_solve(&mut self.mat, m, &mut self.rhs) {
                    for ii in 0..m {
                        ch.s[s0 + ii] = self.rhs[ii];
                    }
                }
            }
            // spójność i ograniczenie przyrostu
            for &(s0, s1) in runs {
                for t in s0..s1 {
                    let x = ch.orig[t];
                    if !is_clipped(x) {
                        continue; // próbki rozszerzenia brzegowego – bez warunku spójności
                    }
                    let th = levels(x);
                    let sg = x.signum();
                    let mut v = ch.s[t] * sg;
                    if v < th {
                        v = th;
                    }
                    let e_cap = th * (gmax - 1.0);
                    let e = v - th;
                    if e_cap > 0.0 && e > 0.0 {
                        v = th + e_cap * (e / e_cap).tanh();
                    }
                    ch.s[t] = sg * v;
                }
            }
        }
        self.active = true;
    }
}

impl Module for Declipper {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Declipper]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        2 * self.hop
    }
    fn warmup_samples(&self) -> usize {
        self.latency_samples() + self.ctx.ms(300.0)
    }
    fn reset(&mut self, ctx: Ctx) {
        let f = ctx.f();
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        self.nd = 1024 * f;
        self.hop = 256 * f;
        self.p = 32 * f;
        let hops_2s = ((2.0 * ctx.fs) / self.hop as f64).round() as usize;
        self.ch = [Self::chan(self.nd, self.hop, hops_2s), Self::chan(self.nd, self.hop, hops_2s)];
        self.hann = (0..self.nd).map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / self.nd as f64).cos()).collect();
        // bufory robocze rekonstrukcji – pełna pojemność tutaj, żeby `resize` w wątku audio nigdy nie alokował
        self.r = vec![0.0; self.p + 1];
        self.ar = vec![0.0; self.p + 1];
        self.bcoef = vec![0.0; self.p + 1];
        self.lev_tmp = vec![0.0; self.p + 1];
        self.mat = vec![0.0; (64 * f) * (64 * f)];
        self.rhs = vec![0.0; 64 * f];
        self.runs_buf = Vec::with_capacity(self.hop);
        self.fill = 0;
        self.out_pos = 0;
        self.active = false;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let (nd, hop) = (self.nd, self.hop);
        for i in 0..l.len() {
            let idx = nd - hop + self.fill;
            self.ch[0].orig[idx] = l[i];
            self.ch[1].orig[idx] = r[i];
            self.ch[0].s[idx] = l[i];
            self.ch[1].s[idx] = r[i];
            l[i] = self.ch[0].out[self.out_pos];
            r[i] = self.ch[1].out[self.out_pos];
            self.fill += 1;
            self.out_pos += 1;
            if self.fill == hop {
                let a = self.a.advance(hop);
                let k = (4.0 * a).min(1.0);
                for c in 0..2 {
                    {
                        let Chan { orig, det, .. } = &mut self.ch[c];
                        det.push_hop(&orig[nd - hop..]);
                    }
                    if k > 0.0 {
                        self.reconstruct(c, a);
                    }
                    let ch = &mut self.ch[c];
                    // obszar zatwierdzany [nd − 2hop, nd − hop) → wyjście na następny skok
                    for j in 0..hop {
                        let t = nd - 2 * hop + j;
                        let x = ch.orig[t];
                        ch.out[j] = if k > 0.0 { x + k * (ch.s[t] - x) } else { x };
                    }
                    ch.orig.copy_within(hop.., 0);
                    ch.s.copy_within(hop.., 0);
                }
                self.fill = 0;
                self.out_pos = 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard() {
        crate::modules::testkit::standard(|c| Box::new(Declipper::new(c)), ModuleId::Declipper, 1e-12);
    }
    #[test]
    fn restores_hard_clipped_sine() {
        let fs = 44_100.0;
        let ctx = Ctx::new(fs);
        let mut m = Declipper::new(ctx);
        m.set_amount(ModuleId::Declipper, 1.0);
        m.reset(ctx);
        let lat = m.latency_samples();
        let n = 44_100 * 3;
        let orig: Vec<f64> = (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                0.9 * (2.0 * std::f64::consts::PI * 220.0 * t).sin() + 0.3 * (2.0 * std::f64::consts::PI * 330.0 * t + 0.4).sin()
            })
            .collect();
        let clip = 0.8;
        let x: Vec<f64> = orig.iter().map(|v| v.clamp(-clip, clip)).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        crate::modules::testkit::run(&mut m, &mut l, &mut r);
        let range = (n - 44_100)..(n - 1000);
        let err_clip: f64 = range.clone().map(|i| (x[i] - orig[i]).powi(2)).sum();
        let err_rest: f64 = range.clone().map(|i| (l[i + lat] - orig[i]).powi(2)).sum();
        let gain = 10.0 * (err_clip / err_rest).log10();
        assert!(gain > 6.0, "declipper poprawił tylko o {gain} dB");
    }
}
