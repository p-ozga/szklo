//! Przestrzeń – wyraźny, stopniowany efekt przestrzenny (projekt clean-room z literatury).
//!
//! Wyjście: L′ = L + Δ, R′ = R − Δ, więc suma mono L′ + R′ jest DOKŁADNIE taka jak na wejściu (kompatybilność mono),
//! a przy a = 0 Δ ≡ 0 (moduł przezroczysty).
//!
//! Δ = q·[ (k−1)·S_h + g_o·(S_h[n−τ1] − 0,7·S_h[n−τ2]) + g_d·V(M_h) ]
//!  * S_h, M_h – bok i środek powyżej 180 Hz (bas zostaje w środku), M_h dodatkowo LP 9 kHz,
//!  * (k−1)·S_h – mocniejszy bok: k = 1 + 1,1a (do +6,4 dB),
//!  * wczesne odbicia boku (τ1 = 13 ms, τ2 = 23 ms) – głębia pomieszczenia: g_o = 0,35a,
//!  * V – dekorelacja środka rzadkim filtrem „velvet noise” (24 impulsy ±1 w 2…32 ms z zanikiem, energia 1):
//!    nawet nagranie mono dostaje przestrzeń, bez filtra grzebieniowego. g_d = 0,6a.
//!    Źródła: Lauridsen/Schroeder (pseudostereo z sumą mono bez zmian), Alary, Politis & Välimäki 2017
//!    (Velvet-noise decorrelator).
//!  * q – straż: bok po obróbce (> 180 Hz) nie przekracza ok. poziomu środka (S′ ≤ M·(0,7 + 0,5a) w mocy); nagrania
//!    już szerokie dostają najwyżej +2 dB boku. Środka nie ruszamy, więc wokal nie znika.
//!
//! Suwak działa coraz mocniej na całym zakresie; latencja 0.

use super::control_chunks;
use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};
use std::f64::consts::FRAC_1_SQRT_2;

const TAPS: usize = 24;
const T0_MS: f64 = 2.0;
const T1_MS: f64 = 32.0;
const ECHO1_MS: f64 = 13.0;
const ECHO2_MS: f64 = 23.0;

pub struct Space {
    ctx: Ctx,
    a: Smoother,
    s_hp: Svf,
    m_hp: Svf,
    m_lp: Svf,
    /// bufory kołowe (długość = potęga 2)
    buf_m: Vec<f64>,
    buf_s: Vec<f64>,
    mask: usize,
    pos: usize,
    taps: [(usize, f64); TAPS],
    e1: usize,
    e2: usize,
    e_m: f64,
    e_s: f64,
    a_e: f64,
    q: f64,
    a_q: f64,
    /// poprzednie wzmocnienia (k−1, g_o, g_d) – interpolacja liniowa w bloku
    prev: [f64; 3],
    warm: bool,
}

impl Space {
    pub fn new(ctx: Ctx) -> Self {
        let len = ((0.001 * T1_MS * ctx.fs).ceil() as usize + 8).next_power_of_two();
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            s_hp: Svf::default(),
            m_hp: Svf::default(),
            m_lp: Svf::default(),
            buf_m: vec![0.0; len],
            buf_s: vec![0.0; len],
            mask: len - 1,
            pos: 0,
            taps: [(0, 0.0); TAPS],
            e1: 0,
            e2: 0,
            e_m: 0.0,
            e_s: 0.0,
            a_e: 0.0,
            q: 0.0,
            a_q: 0.0,
            prev: [0.0; 3],
            warm: false,
        };
        s.reset(ctx);
        s
    }
}

impl Module for Space {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Space]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        let fs = ctx.fs;
        self.a = Smoother::new(self.a.target(), 0.02, fs);
        let len = ((0.001 * T1_MS * fs).ceil() as usize + 8).next_power_of_two();
        if len != self.buf_m.len() {
            self.buf_m = vec![0.0; len];
            self.buf_s = vec![0.0; len];
            self.mask = len - 1;
        }
        self.buf_m.fill(0.0);
        self.buf_s.fill(0.0);
        self.pos = 0;
        self.s_hp = Svf::new(SvfCoeffs::new(fs, 180.0, FRAC_1_SQRT_2, Mode::Highpass));
        self.m_hp = Svf::new(SvfCoeffs::new(fs, 180.0, FRAC_1_SQRT_2, Mode::Highpass));
        self.m_lp = Svf::new(SvfCoeffs::new(fs, 9000.0f64.min(0.4 * fs), FRAC_1_SQRT_2, Mode::Lowpass));
        // impulsy: po jednym w równych przedziałach, losowe położenie i znak (stałe ziarno), zanik ~ −12 dB na końcu
        let mut seed: u64 = 0x5A7C_1E00_2026_1124;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut e = 0.0;
        for (i, t) in self.taps.iter_mut().enumerate() {
            let u = (i as f64 + rnd()) / TAPS as f64;
            let d = ((T0_MS + (T1_MS - T0_MS) * u) * 0.001 * fs).round() as usize;
            let sign = if rnd() < 0.5 { -1.0 } else { 1.0 };
            let g = sign * 10f64.powf(-12.0 * u / 20.0);
            e += g * g;
            *t = (d.max(1), g);
        }
        let norm = 1.0 / e.sqrt();
        for t in self.taps.iter_mut() {
            t.1 *= norm;
        }
        self.e1 = (ECHO1_MS * 0.001 * fs).round() as usize;
        self.e2 = (ECHO2_MS * 0.001 * fs).round() as usize;
        self.e_m = 0.0;
        self.e_s = 0.0;
        let kfs = fs / CONTROL_BLOCK as f64;
        self.a_e = tau_coeff(0.4, kfs);
        self.a_q = tau_coeff(0.15, kfs);
        self.q = 0.0;
        self.prev = [0.0; 3];
        self.warm = false;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let s = &mut *self;
        control_chunks(l, r, |cl, cr| {
            let n = cl.len();
            let a = s.a.advance(n);
            let inv = 1.0 / n as f64;
            let (mut em, mut es) = (0.0, 0.0);
            let mut sh = [0.0f64; CONTROL_BLOCK];
            for i in 0..n {
                let m = 0.5 * (cl[i] + cr[i]);
                let d = 0.5 * (cl[i] - cr[i]);
                sh[i] = s.s_hp.tick(d);
                let mh = s.m_lp.tick(s.m_hp.tick(m));
                let p = (s.pos + i) & s.mask;
                s.buf_m[p] = mh;
                s.buf_s[p] = sh[i];
                em += mh * mh;
                es += sh[i] * sh[i];
            }
            em *= inv;
            es *= inv;
            if s.warm {
                s.e_m = s.a_e * s.e_m + (1.0 - s.a_e) * em;
                s.e_s = s.a_e * s.e_s + (1.0 - s.a_e) * es;
            } else if em + es > 1e-10 {
                s.e_m = em;
                s.e_s = es;
                s.warm = true;
            }
            // straż: przewidywana energia boku ≤ max(M + 0,8 dB, S + 2 dB) przy pełnym suwaku (mniej przy niższym)
            let (kb, go, gd) = (1.1 * a, 0.35 * a, 0.6 * a);
            // A·q² + B·q + E_s = cap  (bok główny dodaje się zgodnie w fazie, odbicia i dekorelacja – energetycznie)
            let cap = (s.e_m * (0.7 + 0.5 * a)).max(s.e_s * (1.0 + 0.6 * a));
            let aa = s.e_s * (kb * kb + 1.49 * go * go) + gd * gd * s.e_m;
            let bb = 2.0 * s.e_s * kb;
            let q = if aa + bb + s.e_s > cap {
                let room = (cap - s.e_s).max(0.0);
                (((bb * bb + 4.0 * aa * room).sqrt() - bb) / (2.0 * aa + 1e-30)).clamp(0.0, 1.0)
            } else {
                1.0
            };
            s.q = q + s.a_q * (s.q - q);
            let tgt = [kb * s.q, go * s.q, gd * s.q];
            if a > 0.0 || s.prev != [0.0; 3] {
                for i in 0..n {
                    let t = (i + 1) as f64 * inv;
                    let p = s.pos + i;
                    let g = [
                        s.prev[0] + (tgt[0] - s.prev[0]) * t,
                        s.prev[1] + (tgt[1] - s.prev[1]) * t,
                        s.prev[2] + (tgt[2] - s.prev[2]) * t,
                    ];
                    let echo = s.buf_s[p.wrapping_sub(s.e1) & s.mask] - 0.7 * s.buf_s[p.wrapping_sub(s.e2) & s.mask];
                    let mut v = 0.0;
                    for &(d, c) in s.taps.iter() {
                        v += c * s.buf_m[p.wrapping_sub(d) & s.mask];
                    }
                    let delta = g[0] * sh[i] + g[1] * echo + g[2] * v;
                    cl[i] += delta;
                    cr[i] -= delta;
                }
            }
            s.prev = tgt;
            s.pos = (s.pos + n) & s.mask;
        });
        self.s_hp.flush_denormals();
        self.m_hp.flush_denormals();
        self.m_lp.flush_denormals();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::testkit;
    use std::f64::consts::PI;

    #[test]
    fn standard() {
        testkit::standard(|c| Box::new(Space::new(c)), ModuleId::Space, 0.0);
    }

    fn przetworz(a: f32, l: &[f64], r: &[f64], fs: f64) -> (Vec<f64>, Vec<f64>) {
        let mut m = Space::new(Ctx::new(fs));
        m.set_amount(ModuleId::Space, a);
        m.reset(Ctx::new(fs));
        let (mut l, mut r) = (l.to_vec(), r.to_vec());
        testkit::run(&mut m, &mut l, &mut r);
        (l, r)
    }
    /// bok/środek w dB (bez pierwszej sekundy)
    fn sm(l: &[f64], r: &[f64], fs: f64) -> f64 {
        let o = fs as usize;
        let (mut m, mut s) = (1e-20, 1e-20);
        for i in o..l.len() {
            m += (l[i] + r[i]).powi(2);
            s += (l[i] - r[i]).powi(2);
        }
        10.0 * (s / m).log10()
    }
    fn wezsze(n: usize) -> (Vec<f64>, Vec<f64>) {
        // wspólny środek + niezależny bok −14 dB
        let m = testkit::noise(n, 0.2, 1);
        let s = testkit::noise(n, 0.04, 2);
        (0..n).map(|i| (m[i] + s[i], m[i] - s[i])).unzip()
    }

    #[test]
    fn mono_sum_unchanged() {
        let fs = 48_000.0;
        let (xl, xr) = testkit::music(60_000, fs);
        let (l, r) = przetworz(1.0, &xl, &xr, fs);
        for i in 0..l.len() {
            assert!(((l[i] + r[i]) - (xl[i] + xr[i])).abs() < 1e-12);
        }
    }

    #[test]
    fn wider_with_every_step_of_the_slider() {
        let fs = 48_000.0;
        let (xl, xr) = wezsze(4 * 48_000);
        let mut prev = sm(&xl, &xr, fs);
        let first = prev;
        for a in [0.25, 0.5, 0.75, 1.0] {
            let (l, r) = przetworz(a, &xl, &xr, fs);
            let w = sm(&l, &r, fs);
            assert!(w > prev + 0.8, "a = {a}: {w} dB, poprzednio {prev} dB");
            prev = w;
        }
        assert!(prev > first + 6.0, "pełny suwak za słaby: {first} → {prev} dB");
    }

    #[test]
    fn mono_recording_becomes_spacious_but_bass_stays_centered() {
        let fs = 48_000.0;
        let n = 3 * 48_000;
        let x = testkit::noise(n, 0.2, 7);
        let (l, r) = przetworz(1.0, &x, &x, fs);
        let w = sm(&l, &r, fs);
        assert!(w > -9.0 && w < -0.5, "mono → S/M = {w} dB");
        let bas: Vec<f64> = (0..n).map(|i| 0.4 * (2.0 * PI * 60.0 * i as f64 / fs).sin()).collect();
        let (l, r) = przetworz(1.0, &bas, &bas, fs);
        assert!(sm(&l, &r, fs) < -20.0, "bas ucieka na boki: {} dB", sm(&l, &r, fs));
    }

    #[test]
    fn already_wide_recording_keeps_its_center() {
        let fs = 48_000.0;
        let n = 4 * 48_000;
        let (xl, xr) = (testkit::noise(n, 0.2, 11), testkit::noise(n, 0.2, 12)); // S = M
        let (l, r) = przetworz(1.0, &xl, &xr, fs);
        let d = sm(&l, &r, fs) - sm(&xl, &xr, fs);
        assert!(d < 2.3, "bok rośnie o {d} dB");
    }
}
