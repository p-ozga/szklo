//! Szyna STFT A (spec v1 §2.1, §3.9): N = 2048·F, skok N/4. Kolejno: Szum i brum (§4.3) → Naprawa MP3 (§4.2 a, b),
//! a za syntezą Naprawa MP3 (c) – redukcja pre-echa w dziedzinie czasu z wyprzedzeniem danym przez szynę.
//! Każda pozycja ma własne włączanie z neutralizacją 50 ms (e: 0↔1); przy a·e = 0 szyna jest tożsamością.

use super::{lin, sstep, und};
use crate::catalog::ModuleId;
use crate::dsp::stft::{StftStereo, C64};
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module};
use std::f64::consts::{FRAC_1_SQRT_2, PI};

/// Stan jednej pozycji UI w module złożonym: moc (wygładzana) i zaangażowanie e (neutralizacja 50 ms).
pub(crate) struct Sub {
    pub on: bool,
    pub e: f64,
    pub a: Smoother,
}
impl Sub {
    pub fn new(fs: f64) -> Self {
        Self { on: false, e: 0.0, a: Smoother::new(0.0, 0.02, fs) }
    }
    /// Postęp o `n` próbek; zwraca efektywną moc a·e.
    pub fn advance(&mut self, n: usize, fs: f64) -> f64 {
        let step = n as f64 / (0.05 * fs);
        self.e = if self.on { (self.e + step).min(1.0) } else { (self.e - step).max(0.0) };
        self.a.advance(n) * self.e
    }
}

/// Prosty generator losowy (xorshift) – fazy szumu wypełniającego.
pub(crate) struct Rng(u64);
impl Rng {
    pub fn next_f64(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

pub struct BusA {
    ctx: Ctx,
    stft: Option<StftStereo>,
    nr: Sub,
    dl: Sub,
    // — Szum i brum —
    p: Vec<f64>,
    sub_min: Vec<f64>,
    stored: Vec<[f64; 8]>,
    sub_count: usize,
    sub_idx: usize,
    g_prev: Vec<f64>,
    gamma_prev: Vec<f64>,
    gtmp: Vec<f64>,
    n_raw: Vec<f64>,
    // — Naprawa MP3 (a, b) —
    bands: Vec<(usize, usize, f64)>, // (k0, k1, f_środka) – 1/6 okt. w całym paśmie
    p_lt: Vec<f64>,
    lt_spec: Vec<f64>,
    /// bufory robocze ramki (moc pasm, poziomy w dB) – prealokowane w `reset`, bez alokacji w callbacku STFT
    pb: Vec<f64>,
    lvl: Vec<f64>,
    cutoff_band: Option<usize>,
    cutoff_cand: Option<usize>,
    cutoff_frames: usize,
    rng: Rng,
    xin_l: Vec<C64>,
    xin_r: Vec<C64>,
    // — Naprawa MP3 (c): pre-echo —
    det_hp: Svf,
    out_hp: [Svf; 2],
    ef: f64,
    ef_hist: Vec<f64>,
    hist_pos: usize,
    gain_line: Vec<f64>,
    line_pos: usize,
    dead: usize,
    t_in: u64,
    a_att: f64,
    a_rel: f64,
}

impl BusA {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            stft: None, // tworzy `reset`
            nr: Sub::new(ctx.fs),
            dl: Sub::new(ctx.fs),
            p: Vec::new(),
            sub_min: Vec::new(),
            stored: Vec::new(),
            sub_count: 0,
            sub_idx: 0,
            g_prev: Vec::new(),
            gamma_prev: Vec::new(),
            gtmp: Vec::new(),
            n_raw: Vec::new(),
            bands: Vec::new(),
            p_lt: Vec::new(),
            lt_spec: Vec::new(),
            pb: Vec::new(),
            lvl: Vec::new(),
            cutoff_band: None,
            cutoff_cand: None,
            cutoff_frames: 0,
            rng: Rng(0x2545F4914F6CDD1D),
            xin_l: Vec::new(),
            xin_r: Vec::new(),
            det_hp: Svf::default(),
            out_hp: [Svf::default(); 2],
            ef: 0.0,
            ef_hist: Vec::new(),
            hist_pos: 0,
            gain_line: Vec::new(),
            line_pos: 0,
            dead: 0,
            t_in: 0,
            a_att: 0.0,
            a_rel: 0.0,
        };
        s.reset(ctx);
        s
    }

    fn sub(&mut self, id: ModuleId) -> &mut Sub {
        if id == ModuleId::NoiseReduction {
            &mut self.nr
        } else {
            &mut self.dl
        }
    }

    /// Przetwarzanie ramki widmowej (wywoływane z callbacku STFT).
    #[allow(clippy::too_many_arguments)]
    fn frame(&mut self, xl: &mut [C64], xr: &mut [C64], a_nr: f64, a_dl: f64) {
        let nb = xl.len();
        let n = 2048 * self.ctx.f();
        let fs = self.ctx.fs;
        let hop = (n / 4) as f64;
        // kopia widma wejściowego (detekcja dziur na wejściu szyny)
        self.xin_l.clear();
        self.xin_l.extend_from_slice(xl);
        self.xin_r.clear();
        self.xin_r.extend_from_slice(xr);

        // ── Szum i brum ──
        // estymata szumu liczona zawsze (żeby po włączeniu była gotowa)
        for k in 0..nb {
            let pw = xl[k].norm_sqr() + xr[k].norm_sqr();
            self.p[k] = 0.85 * self.p[k] + 0.15 * pw;
            self.sub_min[k] = self.sub_min[k].min(self.p[k]);
        }
        self.sub_count += 1;
        if self.sub_count >= 16 {
            self.sub_count = 0;
            for k in 0..nb {
                self.stored[k][self.sub_idx] = self.sub_min[k];
                self.sub_min[k] = f64::MAX;
            }
            self.sub_idx = (self.sub_idx + 1) % 8;
        }
        for k in 0..nb {
            let m = self.stored[k].iter().copied().fold(self.sub_min[k], f64::min);
            self.n_raw[k] = 1.66 * if m == f64::MAX { 0.0 } else { m };
        }
        if a_nr > 0.0 {
            // 2026-11-25: górna połowa suwaka zjadała muzykę (przy 100% czyste nagranie traciło 4–8 JND, przy 50% odszumianie
            // było najlepsze) – skala złagodzona: nowe 100% ≈ dawne 72%, 50% ≈ dawne 43%
            let a_nr = 0.72 * a_nr.powf(0.75);
            let g_min = und(-15.0 * a_nr);
            let beta = 1.0 + a_nr;
            let l_cap = lin(a_nr, -66.0, -42.0);
            let bin_hz = fs / n as f64;
            // stałe wyniesione przed pętlę (wcześniej 3× powf na prążek i ramkę)
            let (k_dn, k_up) = (2f64.powf(-1.0 / 6.0), 2f64.powf(1.0 / 6.0));
            let p_band = 10f64.powf(l_cap / 10.0);
            for k in 1..nb {
                let f = k as f64 * bin_hz;
                // mediana N_raw w ±1/6 okt. – przybliżona medianą z 5 punktów rozłożonych w tym zakresie
                let lo = ((k as f64 * k_dn).floor().max(1.0) as usize).min(nb - 1);
                let hi = ((k as f64 * k_up).ceil() as usize).clamp(lo, nb - 1);
                let mut pts = [0.0f64; 5];
                for (j, pt) in pts.iter_mut().enumerate() {
                    let idx = lo + (hi - lo) * j / 4;
                    *pt = self.n_raw[idx];
                }
                pts.sort_unstable_by(|a, b| a.total_cmp(b)); // bez paniki na NaN, bez alokacji
                let med = pts[2];
                // limit poziomu: szum równomierny w paśmie 1/3 okt. o mocy und(L_cap)² (MS względem pełnej skali)
                let bw = (f * 0.2316).max(bin_hz);
                // E|X|² szumu białego o wariancji σ² (okno Hanna) = σ²·0,375·N; σ² = P_pasma·(fs/2)/B; L+R → ×2
                let n_cap = 2.0 * 0.375 * n as f64 * p_band * (fs / 2.0) / bw;
                let nk = self.n_raw[k].min(2.0 * med).min(n_cap).max(1e-30);
                let pw = xl[k].norm_sqr() + xr[k].norm_sqr();
                let gamma = pw / nk;
                let xi = 0.92 * self.g_prev[k] * self.g_prev[k] * self.gamma_prev[k] + 0.08 * (gamma - 1.0).max(0.0);
                let gw = xi / (beta + xi);
                self.gtmp[k] = gw.max(g_min);
                self.gamma_prev[k] = gamma.min(1e6);
            }
            self.gtmp[0] = self.gtmp[1];
            for k in 0..nb {
                self.g_prev[k] = self.gtmp[k];
            }
            // wygładzenie po częstotliwości [¼, ½, ¼]
            let mut prev = self.gtmp[0];
            for k in 0..nb {
                let next = if k + 1 < nb { self.gtmp[k + 1] } else { self.gtmp[k] };
                let g = 0.25 * prev + 0.5 * self.gtmp[k] + 0.25 * next;
                prev = self.gtmp[k];
                xl[k] *= g;
                xr[k] *= g;
            }
        } else {
            for k in 0..nb {
                self.g_prev[k] = 1.0;
                let pw = self.xin_l[k].norm_sqr() + self.xin_r[k].norm_sqr();
                self.gamma_prev[k] = (pw / self.n_raw[k].max(1e-30)).min(1e6);
            }
        }

        // ── Naprawa MP3 (a) dziury i (b) pasmo ──
        let a_lt = (-hop / (0.3 * fs)).exp();
        let a_hole = (-hop / (1.0 * fs)).exp();
        let a_spec = (-hop / (2.0 * fs)).exp();
        let nbands = self.bands.len();
        // bufory robocze wyjęte z `self` na czas ramki (zamiana wskaźników, bez alokacji)
        let mut pb = std::mem::take(&mut self.pb);
        let mut lvl = std::mem::take(&mut self.lvl);
        for (b, &(k0, k1, _)) in self.bands.iter().enumerate() {
            let mut s = 0.0;
            for k in k0..k1 {
                s += self.xin_l[k].norm_sqr() + self.xin_r[k].norm_sqr();
            }
            pb[b] = s / (k1 - k0).max(1) as f64;
            self.lt_spec[b] = a_spec * self.lt_spec[b] + (1.0 - a_spec) * pb[b];
            lvl[b] = 10.0 * (self.lt_spec[b] + 1e-30).log10();
        }
        // odcięcie pasma (b): najwyższe pasmo z poziomem > maks − 70 dB, nad którym spadek > 20 dB
        let lmax = lvl.iter().copied().fold(f64::MIN, f64::max);
        let mut cand = None;
        for b in (0..nbands.saturating_sub(1)).rev() {
            let f = self.bands[b].2;
            if (7000.0..=19_000.0).contains(&f) && lvl[b] > lmax - 70.0 && lvl[b] - lvl[b + 1] > 20.0 {
                cand = Some(b);
                break;
            }
        }
        let frames_2s = (2.0 * fs / hop) as usize;
        if cand.is_some() && cand.map(|c| c as i64) .zip(self.cutoff_cand.map(|c| c as i64)).map(|(a, b)| (a - b).abs() <= 1).unwrap_or(false) {
            self.cutoff_frames += 1;
        } else {
            self.cutoff_cand = cand;
            self.cutoff_frames = 0;
        }
        if self.cutoff_frames >= frames_2s {
            self.cutoff_band = self.cutoff_cand;
        } else if cand.is_none() {
            self.cutoff_band = None;
        }
        let f_c = self.cutoff_band.map(|b| self.bands[b].2 * 2f64.powf(1.0 / 12.0)).unwrap_or(fs / 2.0);
        let f_hole_top = f_c.min(16_000.0);
        // (a) dziury: pasma od 3 kHz do min(f_c, 16 kHz)
        for b in 0..nbands {
            let (k0, k1, f) = self.bands[b];
            if f < 3000.0 || f > f_hole_top {
                self.p_lt[b] = a_lt * self.p_lt[b] + (1.0 - a_lt) * pb[b];
                continue;
            }
            let pn = if b > 0 && b + 1 < nbands { (pb[b - 1] * pb[b + 1]).sqrt() } else { pb[b] };
            let hole = pb[b] < 0.063 * self.p_lt[b] && pb[b] < 0.063 * pn;
            if hole {
                self.p_lt[b] *= a_hole;
                if a_dl > 0.0 {
                    let p_fill = a_dl * (0.25 * self.p_lt[b] - pb[b]).max(0.0);
                    // P_fill to moc L+R na prążek → na kanał P/2, amplituda wstawiana A = 2·√(P/2)
                    let amp = 2.0 * (0.5 * p_fill).sqrt();
                    for k in k0..k1 {
                        let (pl, pr) = (2.0 * PI * self.rng.next_f64(), 2.0 * PI * self.rng.next_f64());
                        xl[k] += C64::from_polar(amp, pl);
                        xr[k] += C64::from_polar(amp, pr);
                    }
                }
            } else {
                self.p_lt[b] = a_lt * self.p_lt[b] + (1.0 - a_lt) * pb[b];
            }
        }
        // (b) rozszerzenie pasma
        let a_b = sstep(a_dl);
        if a_b > 0.0 {
            if let Some(cb) = self.cutoff_band {
                let bin_hz = fs / n as f64;
                let k_c = (f_c / bin_hz).round() as usize;
                let delta = (f_c / 3.0).min(4000.0);
                let m = (((delta / bin_hz) / 4.0).round() as usize).max(1) * 4;
                let k20 = ((20_000.0 / bin_hz) as usize).min(nb - 1);
                // nachylenie z regresji w oktawie poniżej f_c
                let (mut sx, mut sy, mut sxx, mut sxy, mut cnt) = (0.0, 0.0, 0.0, 0.0, 0.0);
                for b in 0..=cb {
                    let f = self.bands[b].2;
                    if f >= f_c / 2.0 && f <= f_c {
                        let x = (f / f_c).log2();
                        sx += x;
                        sy += lvl[b];
                        sxx += x * x;
                        sxy += x * lvl[b];
                        cnt += 1.0;
                    }
                }
                let slope = if cnt >= 2.0 { (cnt * sxy - sx * sy) / (cnt * sxx - sx * sx) } else { -12.0 };
                let s = slope.clamp(-24.0, -6.0);
                for k in (k_c + 1)..=(k_c + m).min(k20) {
                    if k < m {
                        continue;
                    }
                    let fk = k as f64 * bin_hz;
                    let fs_ = (k - m) as f64 * bin_hz;
                    let g = a_b * und(s * (fk / fs_.max(1.0)).log2() - 3.0);
                    xl[k] += self.xin_l[k - m] * g;
                    xr[k] += self.xin_r[k - m] * g;
                }
            }
        }
        self.pb = pb;
        self.lvl = lvl;
    }
}

impl Module for BusA {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::NoiseReduction, ModuleId::Delossifier]
    }
    fn set_amount(&mut self, id: ModuleId, a: f32) {
        self.sub(id).a.set_target(a as f64);
    }
    fn set_sub_enabled(&mut self, id: ModuleId, on: bool) {
        self.sub(id).on = on;
    }
    fn latency_samples(&self) -> usize {
        2048 * self.ctx.f()
    }
    fn warmup_samples(&self) -> usize {
        self.latency_samples() + self.ctx.ms(1500.0)
    }
    fn reset(&mut self, ctx: Ctx) {
        let fs = ctx.fs;
        let n = 2048 * ctx.f();
        self.ctx = ctx;
        // `reset` bywa wołany z wątku audio (włączenie modułu) – plan FFT tworzymy tylko przy zmianie rozmiaru
        match self.stft.as_mut() {
            Some(s) if s.size() == n => s.reset(),
            _ => self.stft = Some(StftStereo::new(n)),
        }
        let nb = n / 2 + 1;
        for sub in [&mut self.nr, &mut self.dl] {
            sub.a = Smoother::new(sub.a.target(), 0.02, fs);
            sub.e = if sub.on { 1.0 } else { 0.0 };
        }
        self.p = vec![0.0; nb];
        self.sub_min = vec![f64::MAX; nb];
        self.stored = vec![[f64::MAX; 8]; nb];
        self.sub_count = 0;
        self.sub_idx = 0;
        self.g_prev = vec![1.0; nb];
        self.gamma_prev = vec![1.0; nb];
        self.gtmp = vec![1.0; nb];
        self.n_raw = vec![0.0; nb];
        // pasma 1/6 okt. od 200 Hz do Nyquista
        let bin_hz = fs / n as f64;
        self.bands.clear();
        let mut f_lo = 200.0;
        while f_lo * 2f64.powf(1.0 / 6.0) < fs / 2.0 {
            let f_hi = f_lo * 2f64.powf(1.0 / 6.0);
            let k0 = (f_lo / bin_hz).round() as usize;
            let k1 = ((f_hi / bin_hz).round() as usize).min(nb);
            if k1 > k0 {
                self.bands.push((k0, k1, (f_lo * f_hi).sqrt()));
            }
            f_lo = f_hi;
        }
        self.p_lt = vec![0.0; self.bands.len()];
        self.lt_spec = vec![0.0; self.bands.len()];
        self.pb = vec![0.0; self.bands.len()];
        self.lvl = vec![0.0; self.bands.len()];
        self.cutoff_band = None;
        self.cutoff_cand = None;
        self.cutoff_frames = 0;
        self.xin_l = Vec::with_capacity(nb);
        self.xin_r = Vec::with_capacity(nb);
        self.det_hp = Svf::new(SvfCoeffs::new(fs, 3000.0, FRAC_1_SQRT_2, Mode::Highpass));
        self.out_hp = [Svf::new(SvfCoeffs::new(fs, 3000.0, FRAC_1_SQRT_2, Mode::Highpass)); 2];
        self.ef = 0.0;
        self.ef_hist = vec![0.0; ctx.ms(40.0) + 1];
        self.hist_pos = 0;
        // zapas co najmniej na jeden blok (256 próbek): przy fs < 12,8 kHz 20 ms było krótsze i indeks się zawijał
        self.gain_line = vec![1.0; n + ctx.ms(20.0).max(crate::module::MAX_BLOCK) + 1];
        self.line_pos = 0;
        self.dead = 0;
        self.t_in = 0;
        self.a_att = tau_coeff(0.0003, fs);
        self.a_rel = tau_coeff(0.005, fs);
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let fs = self.ctx.fs;
        let n = l.len();
        let a_nr = self.nr.advance(n, fs);
        let a_dl = self.dl.advance(n, fs);
        // (c) detektor pre-echa na wejściu szyny
        let d_pe = 9.0 * a_dl;
        let ms = |x: f64| (x * 1e-3 * fs).round() as usize;
        let hl = self.ef_hist.len();
        let gl = self.gain_line.len();
        for i in 0..n {
            let h = self.det_hp.tick(0.5 * (l[i] + r[i]));
            let x = h.abs();
            self.ef = if x > self.ef { x + self.a_att * (self.ef - x) } else { x + self.a_rel * (self.ef - x) };
            self.ef_hist[self.hist_pos] = self.ef;
            self.hist_pos = (self.hist_pos + 1) % hl;
            self.t_in += 1;
            if self.dead > 0 {
                self.dead -= 1;
            }
            // sprawdzenie co 16 próbek
            if d_pe > 0.0 && self.dead == 0 && self.t_in % 16 == 0 {
                let at = |back: usize| self.ef_hist[(self.hist_pos + hl - 1 - back.min(hl - 1)) % hl];
                let mean = |b0: usize, b1: usize, sq: bool| -> f64 {
                    let mut s = 0.0;
                    for b in b0..b1 {
                        let v = at(b);
                        s += if sq { v * v } else { v };
                    }
                    s / (b1 - b0).max(1) as f64
                };
                if self.ef > 4.0 * mean(ms(1.0), ms(5.0), false) + 1e-6 && mean(ms(1.0), ms(12.0), true) > 4.0 * mean(ms(16.0), ms(40.0), true) {
                    self.dead = ms(30.0);
                    // krzywa g na osi czasu wejścia: t0 = teraz; zapis do linii (odczyt z opóźnieniem N)
                    let g_lo = und(-d_pe);
                    // Odstępstwo od spec v1 (powrót liniowy w 0,5 ms przed t0 − 0,5 ms): ostry powrót wzmocnienia
                    // pasma > 3 kHz tuż przed atakiem dawał w teście dymnym słyszalne „kliknięcia” (detektor
                    // nieciągłości 5 → 48 na fragmencie). Teraz: stałe do t0 − 3 ms, powrót półokresem cosinusa
                    // w 2,5 ms, koniec w t0 − 0,5 ms (nadal przed atakiem, w oknie maskowania wstecznego).
                    let (t16, t8, t3, t05) = (ms(16.0), ms(8.0), ms(3.0), ms(0.5));
                    for back in 0..=t16 {
                        // back = t0 − t
                        let g = if back > t8 {
                            let u = (t16 - back) as f64 / (t16 - t8) as f64; // 0→1
                            1.0 + (g_lo - 1.0) * 0.5 * (1.0 - (PI * u).cos())
                        } else if back > t3 {
                            g_lo
                        } else if back > t05 {
                            let u = (t3 - back) as f64 / (t3 - t05).max(1) as f64; // 0→1
                            g_lo + (1.0 - g_lo) * 0.5 * (1.0 - (PI * u).cos())
                        } else {
                            1.0
                        };
                        let idx = (self.line_pos + gl - back) % gl;
                        self.gain_line[idx] = self.gain_line[idx].min(g);
                    }
                }
            }
            self.line_pos = (self.line_pos + 1) % gl;
            // zapis neutralny „w przód” (pozycja, która za chwilę stanie się bieżącą, startuje od 1)
            self.gain_line[self.line_pos] = 1.0;
        }
        // STFT
        if let Some(mut stft) = self.stft.take() {
            stft.process(l, r, |xl, xr| self.frame(xl, xr, a_nr, a_dl));
            self.stft = Some(stft);
        }
        // (c) zastosowanie na wyjściu: y = x + (g − 1)·HP3k(x), g z chwili (teraz − N)
        let lat = self.latency_samples();
        let end = self.line_pos;
        for i in 0..n {
            // próbka wyjściowa i odpowiada chwili wejścia (start + i) − N, a end = start + n
            let back = lat + (n - i);
            let idx = (end + gl * 2 - back) % gl;
            let g = self.gain_line[idx];
            let hl_ = self.out_hp[0].tick(l[i]);
            let hr_ = self.out_hp[1].tick(r[i]);
            if g < 1.0 {
                l[i] += (g - 1.0) * hl_;
                r[i] += (g - 1.0) * hr_;
            }
        }
        self.det_hp.flush_denormals();
        self.out_hp[0].flush_denormals();
        self.out_hp[1].flush_denormals();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transparent_when_amounts_zero() {
        // a = 0 dla obu pozycji: szyna = opóźnienie N (błąd rekonstrukcji STFT ~1e−12)
        crate::modules::testkit::standard(|c| Box::new(BusA::new(c)), ModuleId::NoiseReduction, 1e-9);
        crate::modules::testkit::standard(|c| Box::new(BusA::new(c)), ModuleId::Delossifier, 1e-9);
    }
    #[test]
    fn noise_reduction_lowers_hiss() {
        let fs = 44_100.0;
        let ctx = Ctx::new(fs);
        let mut m = BusA::new(ctx);
        m.set_sub_enabled(ModuleId::NoiseReduction, true);
        m.set_amount(ModuleId::NoiseReduction, 1.0);
        m.reset(ctx);
        let n = 44_100 * 6;
        let nz = crate::modules::testkit::noise(n, 0.003, 5);
        let mut l: Vec<f64> = (0..n).map(|i| nz[i] + if (i / 22050) % 2 == 0 { 0.2 * (i as f64 * 0.0712).sin() } else { 0.0 }).collect();
        let mut r = l.clone();
        crate::modules::testkit::run(&mut m, &mut l, &mut r);
        // w przerwie (bez tonu) pod koniec szum powinien być wyraźnie słabszy
        let lat = m.latency_samples();
        let seg = (5 * 44_100 - 15_000 + lat)..(5 * 44_100 - 1000 + lat);
        let rms_out = (l[seg.clone()].iter().map(|v| v * v).sum::<f64>() / seg.len() as f64).sqrt();
        let rms_in = 0.003 / 3f64.sqrt();
        let red = 20.0 * (rms_out / rms_in).log10();
        assert!(red < -6.0, "odszumianie tylko {red} dB");
    }
}
