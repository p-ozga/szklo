//! Radiowy bas – gęsty, miękki, zawsze obecny dół jak z radia FM (projekt własny; cel z pomiarów charakteru, nie z kodu).
//!
//! Obserwacja (10 utworów, przetwarzanie radiowe): ilość basu względem środka (400–3000 Hz) zbliża się do wspólnego
//! poziomu (chude utwory dostają, dudniące tracą – mniej więcej połowę odchyłki), najwięcej zyskuje 25–80 Hz,
//! a 120–300 Hz jest lekko ujęte; krótkookresowa dynamika basu maleje (szczyt/średnia −2,5 dB).
//!
//! Tor (wejście = wyjście przy a = 0): trzy szeregowe dzwony SVF o wzmocnieniu zmiennym w czasie + nasycenie:
//!  1. **Wyrównanie ilości** (wolne, τ 2,5 s, zmiana ≤ 4 dB/s): G = a·clamp(0,55·(cel − poziom_wzgl_środka)).
//!     pasma: 45 Hz (−5…+12 dB), 85 Hz (−6…+8), 200 Hz (−5…+0,5, ze stałym ujęciem 2,5 dB); cele w `BANDS` dobrane do miary poniżej.
//!  2. **Gęstość** (szybka, atak 4 ms, zwolnienie 220 ms, względem własnego poziomu z 2,5 s): uderzenia ponad średnią
//!     w dół (połowa nadwyżki, do 7 dB), wybrzmienia w górę (do 2,5 dB) – bas jest „miękki” i równy.
//!  3. **Zaokrąglenie** – miękki ogranicznik pasma < 240 Hz z progiem nad jego poziomem RMS (jak taśma/clipper basu).
//! Ochrona: bez podbijania ciszy i szumu (< −60 dBFS) i gdy pasmo po podbiciu przekroczyłoby −13 dBFS RMS.

use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};
use std::f64::consts::{FRAC_1_SQRT_2, SQRT_2};

const N: usize = 3;
/// środek i dobroć dzwonu, cel względem środka [dB], zakres wolnego wzmocnienia [dB], stałe przesunięcie [dB],
/// udział szybkiej regulacji
struct Band {
    f: f64,
    q: f64,
    goal: f64,
    lo: f64,
    hi: f64,
    bias: f64,
    fast: f64,
}
const BANDS: [Band; N] = [
    Band { f: 45.0, q: 0.70, goal: 13.0, lo: -5.0, hi: 12.0, bias: 0.0, fast: 0.8 },
    Band { f: 85.0, q: 0.80, goal: 11.0, lo: -6.0, hi: 8.0, bias: 0.0, fast: 1.0 },
    Band { f: 200.0, q: 0.80, goal: 4.0, lo: -5.0, hi: 0.5, bias: -2.5, fast: 0.4 },
];
/// Pasmo nasycenia: łagodny filtr 2. rzędu wysoko nad basem – małe przesunięcie fazy w basie, więc odjęcie szczytów
/// pasma naprawdę zmniejsza szczyty sygnału (LR4 130 Hz przesuwał 60 Hz o ~75° i szczyty rosły).
const F_SAT: f64 = 240.0;
/// asymetria (odrobina 2. harmonicznej) i zapas progu nad RMS pasma
const BETA: f64 = 0.10;
const HEAD: f64 = 1.4;

struct Chan {
    bells: [Svf; N],
    lp: Svf,
    sat_hp: Svf,
    sat_lp: Svf,
}

impl Chan {
    fn new(fs: f64) -> Self {
                Self {
            bells: [0, 1, 2].map(|i| Svf::new(SvfCoeffs::new(fs, BANDS[i].f, BANDS[i].q, Mode::Bell(0.0)))),
            lp: Svf::new(SvfCoeffs::new(fs, F_SAT, FRAC_1_SQRT_2, Mode::Lowpass)),
            sat_hp: Svf::new(SvfCoeffs::new(fs, 8.0, FRAC_1_SQRT_2, Mode::Highpass)),
            sat_lp: Svf::new(SvfCoeffs::new(fs, 600.0, FRAC_1_SQRT_2, Mode::Lowpass)),
        }
    }
}

/// Detektor pasma: pasmowoprzepustowy z kaskady HP+LP, moc szybka i wolna.
#[derive(Clone, Copy, Default)]
struct Det {
    hp: Svf,
    lp: Svf,
    fast: f64,
    slow: f64,
}

pub struct RadioBass {
    ctx: Ctx,
    a: Smoother,
    ch: [Chan; 2],
    det: [Det; N],
    ref_hp: Svf,
    ref_lp: Svf,
    ref_ms: f64,
    a_slow: f64,
    a_att: f64,
    a_rel: f64,
    /// wolne wzmocnienie [dB] przy a = 1 (bez suwaka), szybkie [dB] przy a = 1
    g_slow: [f64; N],
    g_fast: [f64; N],
    a_gf: f64,
    max_step: f64,
    last: [f64; N],
    ms_sat: f64,
    a_sat: f64,
    /// długi poziom pasma nasycenia (1 s) – próg nie spada do zera między uderzeniami
    ms_long: f64,
    a_long: f64,
    age: f64,
    meter: f32,
}

impl RadioBass {
    pub fn new(ctx: Ctx) -> Self {
        let fs = ctx.fs;
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, fs),
            ch: [Chan::new(fs), Chan::new(fs)],
            det: [Det::default(); N],
            ref_hp: Svf::default(),
            ref_lp: Svf::default(),
            ref_ms: 0.0,
            a_slow: 0.0,
            a_att: 0.0,
            a_rel: 0.0,
            g_slow: [0.0; N],
            g_fast: [0.0; N],
            a_gf: 0.0,
            max_step: 0.0,
            last: [0.0; N],
            ms_sat: 0.0,
            a_sat: 0.0,
            ms_long: 0.0,
            a_long: 0.0,
            age: 0.0,
            meter: 0.0,
        };
        s.reset(ctx);
        s
    }
}

/// Miękki ogranicznik: ≈ liniowy dla |u| ≪ 1, asymptota ≈ ±1; nachylenie 1 w zerze.
#[inline]
fn shaper(u: f64) -> f64 {
    let t0 = BETA.tanh();
    ((u + BETA).tanh() - t0) / (1.0 - t0 * t0)
}

#[inline]
fn db(p: f64) -> f64 {
    10.0 * (p + 1e-18).log10()
}

impl Module for RadioBass {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::RadioBass]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        let fs = ctx.fs;
        let kfs = fs / CONTROL_BLOCK as f64;
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, fs);
        self.ch = [Chan::new(fs), Chan::new(fs)];
        let edges = [(22.0, 65.0), (65.0, 120.0), (140.0, 280.0)];
        for (d, (lo, hi)) in self.det.iter_mut().zip(edges) {
            *d = Det {
                hp: Svf::new(SvfCoeffs::new(fs, lo, FRAC_1_SQRT_2, Mode::Highpass)),
                lp: Svf::new(SvfCoeffs::new(fs, hi, FRAC_1_SQRT_2, Mode::Lowpass)),
                fast: 0.0,
                slow: 0.0,
            };
        }
        self.ref_hp = Svf::new(SvfCoeffs::new(fs, 400.0, FRAC_1_SQRT_2, Mode::Highpass));
        self.ref_lp = Svf::new(SvfCoeffs::new(fs, 3000.0, FRAC_1_SQRT_2, Mode::Lowpass));
        self.ref_ms = 0.0;
        self.a_slow = tau_coeff(2.5, kfs);
        self.a_att = tau_coeff(0.004, kfs);
        self.a_rel = tau_coeff(0.220, kfs);
        self.a_gf = tau_coeff(0.003, kfs);
        self.max_step = 4.0 / kfs;
        self.g_slow = [0.0; N];
        self.g_fast = [0.0; N];
        self.last = [0.0; N];
        self.ms_sat = 0.0;
        self.a_sat = tau_coeff(0.060, fs);
        self.ms_long = 0.0;
        self.a_long = tau_coeff(0.7, fs);
        self.age = 0.0;
        self.meter = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let fs = self.ctx.fs;
        let n = l.len();
        let mut off = 0;
        let mut meter = 0.0f64;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let a = self.a.advance(k);
            // ── detektory (na wejściu, środek L+R; działają też przy a = 0, żeby włączenie było gotowe) ──
            let mut acc = [0.0f64; N];
            let mut acc_ref = 0.0;
            for i in off..off + k {
                let m = 0.5 * (l[i] + r[i]);
                for (d, s) in self.det.iter_mut().zip(acc.iter_mut()) {
                    let t = d.hp.tick(m);
                    let y = d.lp.tick(t);
                    *s += y * y;
                }
                let t = self.ref_hp.tick(m);
                let y = self.ref_lp.tick(t);
                acc_ref += y * y;
            }
            let inv = 1.0 / k as f64;
            let p_ref = acc_ref * inv;
            // pierwsze sekundy: krótsza średnia, żeby filtr nie „budził się” długo na początku utworu
            self.age += k as f64 / fs;
            let t_slow = self.age.clamp(0.25, 2.5);
            let al = if self.age < 2.5 { tau_coeff(t_slow, fs / CONTROL_BLOCK as f64) } else { self.a_slow };
            let quiet = p_ref < 1e-7 && acc.iter().all(|p| p * inv < 1e-7);
            if !quiet {
                self.ref_ms = p_ref + al * (self.ref_ms - p_ref);
            }
            let l_ref = db(self.ref_ms);
            for b in 0..N {
                let p = acc[b] * inv;
                let d = &mut self.det[b];
                let af = if p > d.fast { self.a_att } else { self.a_rel };
                d.fast = p + af * (d.fast - p);
                if !quiet {
                    d.slow = p + al * (d.slow - p);
                }
                let lev = db(d.slow);
                // wolne wyrównanie ilości
                let mut want = (0.55 * (BANDS[b].goal - (lev - l_ref)) + BANDS[b].bias).clamp(BANDS[b].lo, BANDS[b].hi);
                if want > 0.0 {
                    let gate = ((lev + 66.0) / 12.0).clamp(0.0, 1.0); // cisza/szum: bez podbijania
                    let room = (-13.0 - lev).max(0.0); // nie głośniej niż −13 dBFS RMS
                    want = (want * gate).min(room);
                }
                if quiet {
                    want = self.g_slow[b]; // pauza w utworze: zamrożone
                }
                self.g_slow[b] += (want - self.g_slow[b]).clamp(-self.max_step, self.max_step);
                // szybka gęstość względem własnej średniej (+3 dB zapasu na naturalny kształt)
                let over = db(d.fast) - lev;
                let gf = if over > 1.0 {
                    (-0.5 * (over - 1.0)).max(-7.0)
                } else if over < -2.0 && lev > -55.0 {
                    (0.25 * (-2.0 - over)).min(2.5)
                } else {
                    0.0
                } * BANDS[b].fast;
                let gf = if db(d.fast) < -62.0 { gf.min(0.0) } else { gf };
                // w górę wolniej niż w dół – wybrzmienia narastają łagodnie
                let ag = if gf < self.g_fast[b] { self.a_gf } else { self.a_rel };
                self.g_fast[b] = gf + ag * (self.g_fast[b] - gf);
            }
            if a <= 0.0 && self.last == [0.0; N] {
                for i in off..off + k {
                    for (c, x) in [l[i], r[i]].into_iter().enumerate() {
                        let b = self.ch[c].lp.tick(x);
                        self.ms_sat = b * b + self.a_sat * (self.ms_sat - b * b);
                    }
                }
                off += k;
                continue;
            }
            for b in 0..N {
                let g = a * (self.g_slow[b] + self.g_fast[b]);
                meter = meter.max(-a * self.g_fast[b]);
                if (g - self.last[b]).abs() > 0.005 || (g == 0.0 && self.last[b] != 0.0) {
                    let c = SvfCoeffs::new(fs, BANDS[b].f, BANDS[b].q, Mode::Bell(g));
                    self.ch[0].bells[b].c = c;
                    self.ch[1].bells[b].c = c;
                    self.last[b] = g;
                }
            }
            let w = a;
            for i in off..off + k {
                let mut x = [l[i], r[i]];
                let mut b = [0.0f64; 2];
                for c in 0..2 {
                    for f in self.ch[c].bells.iter_mut() {
                        x[c] = f.tick(x[c]);
                    }
                    b[c] = self.ch[c].lp.tick(x[c]);
                }
                let p = 0.5 * (b[0] * b[0] + b[1] * b[1]);
                self.ms_sat = p + self.a_sat * (self.ms_sat - p);
                self.ms_long = p + self.a_long * (self.ms_long - p);
                let amp = HEAD * SQRT_2 * self.ms_long.sqrt() + 1e-4;
                for c in 0..2 {
                    let ch = &mut self.ch[c];
                    let e = ch.sat_hp.tick(amp * shaper(b[c] / amp) - b[c]);
                    x[c] += w * ch.sat_lp.tick(e);
                }
                l[i] = x[0];
                r[i] = x[1];
            }
            off += k;
        }
        for c in self.ch.iter_mut() {
            for f in c.bells.iter_mut().chain(std::iter::once(&mut c.lp)) {
                f.flush_denormals();
            }
            c.sat_hp.flush_denormals();
            c.sat_lp.flush_denormals();
        }
        for d in self.det.iter_mut() {
            d.hp.flush_denormals();
            d.lp.flush_denormals();
        }
        self.ref_hp.flush_denormals();
        self.ref_lp.flush_denormals();
        if self.ms_sat < 1e-30 {
            self.ms_sat = 0.0;
        }
        self.meter = meter as f32;
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        self.meter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::testkit;
    use std::f64::consts::PI;

    #[test]
    fn standard() {
        testkit::standard(|c| Box::new(RadioBass::new(c)), ModuleId::RadioBass, 0.0);
    }

    fn rms(x: &[f64]) -> f64 {
        10.0 * (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64).log10()
    }
    /// bas 85 Hz o amplitudzie `ab` + szum środka; zwraca poziom składowej 85 Hz na wyjściu względem wejścia [dB]
    fn gain(ab: f64) -> f64 {
        let fs = 48_000.0;
        let n = 48_000 * 12;
        let nz = testkit::noise(n, 0.12, 5);
        let x: Vec<f64> = (0..n).map(|i| ab * (2.0 * PI * 85.0 * i as f64 / fs).sin() + nz[i]).collect();
        let ctx = Ctx::new(fs);
        let mut m = RadioBass::new(ctx);
        m.set_amount(ModuleId::RadioBass, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (x.clone(), x.clone());
        testkit::run(&mut m, &mut l, &mut r);
        let amp = |y: &[f64]| {
            let (mut a, mut b) = (0.0, 0.0);
            for (i, v) in y.iter().enumerate() {
                let p = 2.0 * PI * 85.0 * i as f64 / fs;
                a += v * p.cos();
                b += v * p.sin();
            }
            2.0 * (a * a + b * b).sqrt() / y.len() as f64
        };
        20.0 * (amp(&l[n - 96_000..]) / amp(&x[n - 96_000..])).log10()
    }

    #[test]
    fn thin_gets_more_and_boomy_gets_less() {
        let (thin, boomy) = (gain(0.01), gain(0.35));
        assert!(thin > 3.0, "chudy bas: {thin} dB");
        assert!(boomy < -1.5, "dudniący bas: {boomy} dB");
    }

    /// Gęstość: naprzemiennie mocna i słaba nuta basu (różnica 10 dB) – po filtrze różnica ma być wyraźnie mniejsza.
    /// (Na muzyce: rozrzut poziomu basu w oknach 50 ms 8,0 → 6,5 dB.)
    #[test]
    fn evens_out_bass_hits() {
        let fs = 48_000.0;
        let n = 48_000 * 12;
        let nz = testkit::noise(n, 0.10, 9);
        let half = 24_000; // nuta 0,5 s
        let x: Vec<f64> = (0..n)
            .map(|i| {
                let amp = if (i / half) % 2 == 0 { 0.30 } else { 0.095 };
                amp * (2.0 * PI * 70.0 * i as f64 / fs).sin() + nz[i]
            })
            .collect();
        let ctx = Ctx::new(fs);
        let mut m = RadioBass::new(ctx);
        m.set_amount(ModuleId::RadioBass, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (x.clone(), x.clone());
        testkit::run(&mut m, &mut l, &mut r);
        let diff = |y: &[f64]| {
            let mut f = Svf::new(SvfCoeffs::new(fs, 120.0, FRAC_1_SQRT_2, Mode::Lowpass));
            let z: Vec<f64> = y.iter().map(|v| f.tick(*v)).collect();
            // środek ostatniej mocnej i ostatniej słabej nuty
            rms(&z[n - 2 * half + 4800..n - half]) - rms(&z[n - half + 4800..n])
        };
        let (before, after) = (diff(&x), diff(&l));
        assert!(before > 9.0 && after < before - 2.0, "różnica nut: {before} → {after} dB");
    }

    #[test]
    fn silence_and_noise_are_not_pumped() {
        let fs = 48_000.0;
        let n = 48_000 * 6;
        let x = testkit::noise(n, 2e-4, 3); // ≈ −79 dBFS
        let ctx = Ctx::new(fs);
        let mut m = RadioBass::new(ctx);
        m.set_amount(ModuleId::RadioBass, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (x.clone(), x.clone());
        testkit::run(&mut m, &mut l, &mut r);
        assert!((rms(&l[n / 2..]) - rms(&x[n / 2..])).abs() < 0.5);
    }

    #[test]
    fn highs_untouched() {
        let fs = 48_000.0;
        let n = 48_000 * 6;
        let x: Vec<f64> = (0..n).map(|i| 0.2 * (2.0 * PI * 3000.0 * i as f64 / fs).sin()).collect();
        let ctx = Ctx::new(fs);
        let mut m = RadioBass::new(ctx);
        m.set_amount(ModuleId::RadioBass, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (x.clone(), x.clone());
        testkit::run(&mut m, &mut l, &mut r);
        assert!((rms(&l[n / 2..]) - rms(&x[n / 2..])).abs() < 0.2);
    }
}
