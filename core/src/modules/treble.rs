//! Dynamiczna góra – „powietrze” powyżej ok. 5 kHz dokładane tylko tam, gdzie go brakuje (projekt własny).
//!
//! Półka wysokotonowa o stałych biegunach: wyjście = wejście + (A − 1)·HP + (√A − 1)·BP, więc przy A = 1 jest dokładnie
//! przezroczysta, a A można zmieniać szybko bez przeliczania filtrów. Jedno A dla obu kanałów (obraz stereo stoi).
//!
//! Ile podbić, mówi poziom góry WZGLĘDEM ŚRODKA tego samego nagrania (300–3000 Hz):
//!   chwilowo:  G = 0,8·(+1 dB − góra/środek), 0…15 dB – ciche wybrzmienia talerzy i oddech w górę, mocne „s” i werbel nie;
//!   średnio:   G ≤ +0,5 dB − (średnia góra/środek) – jasne nagrania nie robią się kłujące.
//! Nie ma tu pomiaru zapasu do pełnej skali ani przewidywania poziomu wyjścia – o przesterowanie dba limiter na końcu toru.
//! To kompresja w górę z torem bocznym, znana z dynamicznych korektorów i układów redukcji szumu od lat 60.–80.
//! Ochrona: szum tła (góra poniżej ok. −78 dBFS) zostaje bez zmian, pauzy zamrażają średnie.

use super::und;
use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};
use std::f64::consts::FRAC_1_SQRT_2;

const F0: f64 = 6500.0;
const Q0: f64 = 0.62;
const G_MAX: f64 = 15.0;
const GOAL_FAST: f64 = 1.0;
const GOAL_SLOW: f64 = 0.5;

pub struct DynamicTreble {
    ctx: Ctx,
    a: Smoother,
    shelf: [Svf; 2],
    ref_hp: [Svf; 2],
    ref_lp: [Svf; 2],
    p_ref: f64,
    hf_fast: f64,
    hf_slow: f64,
    g_db: f64,
    /// poprzednie (A − 1, √A − 1)
    prev: [f64; 2],
    c: [f64; 5],
    age: f64,
}

impl DynamicTreble {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            shelf: [Svf::default(); 2],
            ref_hp: [Svf::default(); 2],
            ref_lp: [Svf::default(); 2],
            p_ref: 0.0,
            hf_fast: 0.0,
            hf_slow: 0.0,
            g_db: 0.0,
            prev: [0.0; 2],
            c: [0.0; 5],
            age: 0.0,
        };
        s.reset(ctx);
        s
    }
}

#[inline]
fn db(p: f64) -> f64 {
    10.0 * (p + 1e-14).log10()
}

impl Module for DynamicTreble {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::DynamicTreble]
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
        let k = fs / CONTROL_BLOCK as f64;
        self.a = Smoother::new(self.a.target(), 0.02, fs);
        self.shelf = [Svf::new(SvfCoeffs::new(fs, F0, Q0, Mode::Lowpass)); 2];
        self.ref_hp = [Svf::new(SvfCoeffs::new(fs, 300.0, FRAC_1_SQRT_2, Mode::Highpass)); 2];
        self.ref_lp = [Svf::new(SvfCoeffs::new(fs, 3000.0, FRAC_1_SQRT_2, Mode::Lowpass)); 2];
        self.p_ref = 0.0;
        self.hf_fast = 0.0;
        self.hf_slow = 0.0;
        self.g_db = 0.0;
        self.prev = [0.0; 2];
        // szybka: narastanie, opadanie; G: w dół, w górę; wolna
        self.c = [tau_coeff(0.0012, k), tau_coeff(0.09, k), tau_coeff(0.0015, k), tau_coeff(0.05, k), tau_coeff(2.0, k)];
        self.age = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = l.len();
        let kfs = self.ctx.fs / CONTROL_BLOCK as f64;
        let mut off = 0;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let inv = 1.0 / k as f64;
            let a = self.a.advance(k);
            let (xl, xr) = (&mut l[off..off + k], &mut r[off..off + k]);
            let mut hp = [[0.0f64; CONTROL_BLOCK]; 2];
            let mut bp = [[0.0f64; CONTROL_BLOCK]; 2];
            let (mut ph, mut pm) = (0.0, 0.0);
            for j in 0..k {
                for (c, x) in [xl[j], xr[j]].into_iter().enumerate() {
                    let (lo, hi) = self.shelf[c].tick_lp_hp(x);
                    hp[c][j] = hi;
                    bp[c][j] = x - lo - hi;
                    ph += hi * hi;
                    let t = self.ref_hp[c].tick(x);
                    let m = self.ref_lp[c].tick(t);
                    pm += m * m;
                }
            }
            ph *= 0.5 * inv;
            pm *= 0.5 * inv;
            let quiet = pm < 1e-8 && ph < 1e-8;
            self.age += k as f64 / self.ctx.fs;
            let cs = if self.age < 2.0 { tau_coeff(self.age.max(0.1), kfs) } else { self.c[4] };
            if !quiet {
                self.p_ref = pm + cs * (self.p_ref - pm);
                self.hf_slow = ph + cs * (self.hf_slow - ph);
            }
            let cf = if ph > self.hf_fast { self.c[0] } else { self.c[1] };
            self.hf_fast = ph + cf * (self.hf_fast - ph);
            let l_ref = db(self.p_ref);
            let l_fast = db(self.hf_fast);
            let room = (GOAL_SLOW - (db(self.hf_slow) - l_ref)).max(0.0);
            let mut g = (0.8 * (GOAL_FAST - (l_fast - l_ref))).clamp(0.0, G_MAX).min(room);
            g *= ((l_fast + 78.0) / 12.0).clamp(0.0, 1.0);
            let cg = if g < self.g_db { self.c[2] } else { self.c[3] };
            self.g_db = g + cg * (self.g_db - g);
            let tgt = if a > 0.0 {
                let h = und(0.5 * a * self.g_db); // √A
                [h * h - 1.0, h - 1.0]
            } else {
                [0.0; 2]
            };
            if tgt != [0.0; 2] || self.prev != [0.0; 2] {
                for j in 0..k {
                    let t = (j + 1) as f64 * inv;
                    let g0 = self.prev[0] + (tgt[0] - self.prev[0]) * t;
                    let g1 = self.prev[1] + (tgt[1] - self.prev[1]) * t;
                    xl[j] += g0 * hp[0][j] + g1 * bp[0][j];
                    xr[j] += g0 * hp[1][j] + g1 * bp[1][j];
                }
            }
            self.prev = tgt;
            off += k;
        }
        for c in 0..2 {
            self.shelf[c].flush_denormals();
            self.ref_hp[c].flush_denormals();
            self.ref_lp[c].flush_denormals();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::testkit;
    use std::f64::consts::PI;

    #[test]
    fn standard() {
        testkit::standard(|c| Box::new(DynamicTreble::new(c)), ModuleId::DynamicTreble, 0.0);
    }

    /// środek 1 kHz o amplitudzie 0,2 + ton 12 kHz o amplitudzie `ah`: przyrost 12 kHz [dB]
    fn gain(ah: f64) -> f64 {
        let fs = 48_000.0;
        let n = 48_000 * 8;
        let x: Vec<f64> =
            (0..n).map(|i| 0.2 * (2.0 * PI * 1000.0 * i as f64 / fs).sin() + ah * (2.0 * PI * 12_000.0 * i as f64 / fs).sin()).collect();
        let ctx = Ctx::new(fs);
        let mut m = DynamicTreble::new(ctx);
        m.set_amount(ModuleId::DynamicTreble, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (x.clone(), x);
        testkit::run(&mut m, &mut l, &mut r);
        let (mut a, mut b) = (0.0, 0.0);
        for (i, v) in l.iter().enumerate().skip(n / 2) {
            let p = 2.0 * PI * 12_000.0 * i as f64 / fs;
            a += v * p.cos();
            b += v * p.sin();
        }
        20.0 * (2.0 * (a * a + b * b).sqrt() / (n / 2) as f64 / ah).log10()
    }

    #[test]
    fn dark_gets_air_bright_does_not() {
        let (dark, medium, bright) = (gain(0.006), gain(0.03), gain(0.2));
        assert!(dark > 8.0, "ciemne {dark}");
        assert!(medium > 1.0 && medium < dark, "średnie {medium}");
        assert!(bright.abs() < 1.5, "jasne {bright}");
    }

    #[test]
    fn background_hiss_stays() {
        let fs = 48_000.0;
        let x = testkit::noise(48_000 * 5, 1.2e-4, 4); // ≈ −83 dBFS
        let ctx = Ctx::new(fs);
        let mut m = DynamicTreble::new(ctx);
        m.set_amount(ModuleId::DynamicTreble, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (x.clone(), x.clone());
        testkit::run(&mut m, &mut l, &mut r);
        let e = |v: &[f64]| v.iter().map(|q| q * q).sum::<f64>();
        assert!(10.0 * (e(&l) / e(&x)).log10() < 0.5);
    }
}
