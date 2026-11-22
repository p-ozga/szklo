//! Bas: Głęboki bas (§4.12), Bass Boost (§4.13), Bas harmoniczny (§4.14).

use super::control_chunks;
use crate::catalog::ModuleId;
use crate::dsp::crossover::Crossover;
use crate::dsp::dynamics::PeakDetector;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};
use std::f64::consts::FRAC_1_SQRT_2;

// ───────────────────────────── Głęboki bas ─────────────────────────────

/// HPF podsoniczny f_hp = 8 + 14·min(1, 2a) Hz + dzwon 55 Hz Q 0,9 o wzmocnieniu dynamicznym
/// G_dyn = 8a dB · clamp((−12 − L_lf)/18, 0, 1), L_lf = RMS (50 ms) pasma < 120 Hz; wygładzanie: w dół 20 ms, w górę 300 ms.
pub struct DeepBass {
    ctx: Ctx,
    a: Smoother,
    hp: [Svf; 2],
    bell: [Svf; 2],
    lp_det: Svf,
    ms: f64,
    a_ms: f64,
    g: f64,
    a_down: f64,
    a_up: f64,
    last_fhp: f64,
    last_g: f64,
}

impl DeepBass {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            hp: [Svf::default(); 2],
            bell: [Svf::default(); 2],
            lp_det: Svf::default(),
            ms: 0.0,
            a_ms: 0.0,
            g: 0.0,
            a_down: 0.0,
            a_up: 0.0,
            last_fhp: -1.0,
            last_g: f64::NAN,
        };
        s.reset(ctx);
        s
    }
}

impl Module for DeepBass {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::DeepBass]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        let kfs = ctx.fs / CONTROL_BLOCK as f64;
        self.a_ms = tau_coeff(0.05, kfs);
        self.a_down = tau_coeff(0.02, kfs);
        self.a_up = tau_coeff(0.3, kfs);
        self.lp_det = Svf::new(SvfCoeffs::new(ctx.fs, 120.0, FRAC_1_SQRT_2, Mode::Lowpass));
        self.ms = 0.0;
        self.g = 0.0;
        self.last_fhp = -1.0;
        self.last_g = f64::NAN;
        for f in self.hp.iter_mut().chain(self.bell.iter_mut()) {
            f.reset();
        }
        let a = self.a.value;
        let fhp = 8.0 + 14.0 * (2.0 * a).min(1.0);
        for h in self.hp.iter_mut() {
            h.c = SvfCoeffs::new(ctx.fs, fhp, FRAC_1_SQRT_2, Mode::Highpass);
        }
        for b in self.bell.iter_mut() {
            b.c = SvfCoeffs::new(ctx.fs, 55.0, 0.9, Mode::Bell(0.0));
        }
        self.last_fhp = fhp;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let s = &mut *self;
        let fs = s.ctx.fs;
        control_chunks(l, r, |cl, cr| {
            let a = s.a.advance(cl.len());
            let fhp = 8.0 + 14.0 * (2.0 * a).min(1.0);
            if (fhp - s.last_fhp).abs() > 1e-6 {
                for h in s.hp.iter_mut() {
                    h.c = SvfCoeffs::new(fs, fhp, FRAC_1_SQRT_2, Mode::Highpass);
                }
                s.last_fhp = fhp;
            }
            // detektor na wejściu
            let mut acc = 0.0;
            for i in 0..cl.len() {
                let m = 0.5 * (cl[i] + cr[i]);
                let y = s.lp_det.tick(m);
                acc += y * y;
            }
            s.ms = s.a_ms * s.ms + (1.0 - s.a_ms) * acc / cl.len() as f64;
            let l_lf = 10.0 * (s.ms + 1e-18).log10();
            let target = 8.0 * a * ((-12.0 - l_lf) / 18.0).clamp(0.0, 1.0);
            let al = if target < s.g { s.a_down } else { s.a_up };
            s.g = target + al * (s.g - target);
            if (s.g - s.last_g).abs() > 0.01 || (s.g == 0.0 && s.last_g != 0.0) {
                let gg = if s.g < 1e-4 { 0.0 } else { s.g };
                for b in s.bell.iter_mut() {
                    b.c = SvfCoeffs::new(fs, 55.0, 0.9, Mode::Bell(gg));
                }
                s.last_g = gg;
            }
            for i in 0..cl.len() {
                cl[i] = s.bell[0].tick(s.hp[0].tick(cl[i]));
                cr[i] = s.bell[1].tick(s.hp[1].tick(cr[i]));
            }
        });
        for f in self.hp.iter_mut().chain(self.bell.iter_mut()) {
            f.flush_denormals();
        }
        self.lp_det.flush_denormals();
    }
}

// ───────────────────────────── Bass Boost ─────────────────────────────

/// Statyczna półka niska SVF 100 Hz, Q 0,7, G = 8a dB (bez nasycania).
pub struct BassBoost {
    ctx: Ctx,
    a: Smoother,
    f: [Svf; 2],
    last: f64,
}

impl BassBoost {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self { ctx, a: Smoother::new(0.0, 0.02, ctx.fs), f: [Svf::default(); 2], last: f64::NAN };
        s.reset(ctx);
        s
    }
}

impl Module for BassBoost {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::BassBoost]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        for f in self.f.iter_mut() {
            f.reset();
            f.c = SvfCoeffs::new(ctx.fs, 100.0, 0.7, Mode::LowShelf(8.0 * self.a.value));
        }
        self.last = self.a.value;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let s = &mut *self;
        let fs = s.ctx.fs;
        control_chunks(l, r, |cl, cr| {
            let a = s.a.advance(cl.len());
            if a != s.last {
                for f in s.f.iter_mut() {
                    f.c = SvfCoeffs::new(fs, 100.0, 0.7, Mode::LowShelf(8.0 * a));
                }
                s.last = a;
            }
            for i in 0..cl.len() {
                cl[i] = s.f[0].tick(cl[i]);
                cr[i] = s.f[1].tick(cr[i]);
            }
        });
        for f in self.f.iter_mut() {
            f.flush_denormals();
        }
    }
}

// ───────────────────────────── Bas harmoniczny ─────────────────────────────

/// Brakująca podstawowa (Larsen & Aarts; US 5,930,373 – wygasły). Tor boczny mono:
/// s = LP4(110)(HP2(40)(m)); e = szczyt |s| (5/60 ms); u = clamp(s/(e+1e−4)); h = T2 + 0,7·T3 + 0,35·T4;
/// v = LP2(400)·LP2(400)·HP2(100)(e·h); L,R += g_h·v, g_h = a^1,5.
pub struct HarmonicBass {
    ctx: Ctx,
    a: Smoother,
    hp40: Svf,
    lp110: Crossover, // tylko gałąź LP4 (2 pasma) – suma niepotrzebna
    env: PeakDetector,
    hp100: Svf,
    lp400a: Svf,
    lp400b: Svf,
    bands: [f64; 5],
}

impl HarmonicBass {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            hp40: Svf::default(),
            lp110: Crossover::new(ctx.fs, &[110.0]),
            env: PeakDetector::new(0.005, 0.06, ctx.fs),
            hp100: Svf::default(),
            lp400a: Svf::default(),
            lp400b: Svf::default(),
            bands: [0.0; 5],
        };
        s.reset(ctx);
        s
    }
}

impl Module for HarmonicBass {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::HarmonicBass]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        let fs = ctx.fs;
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, fs);
        self.hp40 = Svf::new(SvfCoeffs::new(fs, 40.0, FRAC_1_SQRT_2, Mode::Highpass));
        self.lp110 = Crossover::new(fs, &[110.0]);
        self.env = PeakDetector::new(0.005, 0.06, fs);
        self.hp100 = Svf::new(SvfCoeffs::new(fs, 100.0, FRAC_1_SQRT_2, Mode::Highpass));
        self.lp400a = Svf::new(SvfCoeffs::new(fs, 400.0, FRAC_1_SQRT_2, Mode::Lowpass));
        self.lp400b = Svf::new(SvfCoeffs::new(fs, 400.0, FRAC_1_SQRT_2, Mode::Lowpass));
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let s = &mut *self;
        control_chunks(l, r, |cl, cr| {
            let a = s.a.advance(cl.len());
            let gh = a.powf(1.5);
            for i in 0..cl.len() {
                let m = 0.5 * (cl[i] + cr[i]);
                let x = s.hp40.tick(m);
                s.lp110.split(x, &mut s.bands);
                let sb = s.bands[0];
                let e = s.env.tick(sb);
                let u = (sb / (e + 1e-4)).clamp(-1.0, 1.0);
                let u2 = u * u;
                let t2 = 2.0 * u2 - 1.0;
                let t3 = (4.0 * u2 - 3.0) * u;
                let t4 = 8.0 * u2 * u2 - 8.0 * u2 + 1.0;
                let h = t2 + 0.7 * t3 + 0.35 * t4;
                let v = s.lp400b.tick(s.lp400a.tick(s.hp100.tick(e * h)));
                if gh > 0.0 {
                    cl[i] += gh * v;
                    cr[i] += gh * v;
                }
            }
        });
        for f in [&mut self.hp40, &mut self.hp100, &mut self.lp400a, &mut self.lp400b] {
            f.flush_denormals();
        }
        self.lp110.flush_denormals();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deep_bass_standard() {
        // a = 0: HPF 8 Hz (spec: −0,1 dB przy 20 Hz) – nie bit-exact
        crate::modules::testkit::standard(|c| Box::new(DeepBass::new(c)), ModuleId::DeepBass, 0.12);
    }
    #[test]
    fn bass_boost_standard() {
        crate::modules::testkit::standard(|c| Box::new(BassBoost::new(c)), ModuleId::BassBoost, 1e-9);
    }
    #[test]
    fn harmonic_bass_standard() {
        crate::modules::testkit::standard(|c| Box::new(HarmonicBass::new(c)), ModuleId::HarmonicBass, 1e-12);
    }
}
