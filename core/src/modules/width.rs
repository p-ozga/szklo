//! Szerokość (spec v1 §4.18) – poszerzanie M/S z kompatybilnością mono.
//!
//! S_h = HP Butterworth 150 Hz (S), S_l = S − S_h (dokładne dopełnienie).
//! ρ = korelacja L/R (100 ms); straż = clamp(ρ/0,4, 0, 1)·clamp(E[M²]/E[S²], 0, 1)
//! S′ = (1 + w_s·straż)·S_h + (1 − m_s)·S_l;  L′ = M + S′, R′ = M − S′ (M nietknięte → suma mono bez zmian).
//! Suwak: w_s = 1,1·a (2026-11-25: było 0,6 – tylko 2,8 JND przy 100%), m_s = 0,5·a; a = 0 → S′ = S dokładnie.

use super::control_chunks;
use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module};

pub struct Width {
    ctx: Ctx,
    a: Smoother,
    hp: Svf,
    e_lr: f64,
    e_ll: f64,
    e_rr: f64,
    e_m: f64,
    e_s: f64,
    alpha: f64,
    guard_s: f64,
    a_guard: f64,
    k_hi_prev: f64,
    k_lo_prev: f64,
}

impl Width {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self { ctx, a: Smoother::new(0.0, 0.02, ctx.fs), hp: Svf::default(), e_lr: 0.0, e_ll: 0.0, e_rr: 0.0, e_m: 0.0, e_s: 0.0, alpha: 0.0, guard_s: 0.0, a_guard: 0.0, k_hi_prev: 1.0, k_lo_prev: 1.0 };
        s.reset(ctx);
        s
    }
}

impl Module for Width {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Width]
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
        self.hp = Svf::new(SvfCoeffs::new(ctx.fs, 150.0, std::f64::consts::FRAC_1_SQRT_2, Mode::Highpass));
        self.e_lr = 0.0;
        self.e_ll = 0.0;
        self.e_rr = 0.0;
        self.e_m = 0.0;
        self.e_s = 0.0;
        // uśrednianie 100 ms liczone na blokach sterujących
        self.alpha = tau_coeff(0.1, ctx.fs / crate::module::CONTROL_BLOCK as f64);
        self.a_guard = tau_coeff(0.05, ctx.fs / crate::module::CONTROL_BLOCK as f64);
        self.guard_s = 0.0;
        self.k_hi_prev = 1.0;
        self.k_lo_prev = 1.0 - 0.5 * self.a.value;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let s = &mut *self;
        control_chunks(l, r, |cl, cr| {
            let n = cl.len() as f64;
            let a = s.a.advance(cl.len());
            let (mut lr, mut ll, mut rr, mut mm, mut ss) = (0.0, 0.0, 0.0, 0.0, 0.0);
            for i in 0..cl.len() {
                lr += cl[i] * cr[i];
                ll += cl[i] * cl[i];
                rr += cr[i] * cr[i];
                let m = 0.5 * (cl[i] + cr[i]);
                let d = 0.5 * (cl[i] - cr[i]);
                mm += m * m;
                ss += d * d;
            }
            let al = s.alpha;
            s.e_lr = al * s.e_lr + (1.0 - al) * lr / n;
            s.e_ll = al * s.e_ll + (1.0 - al) * ll / n;
            s.e_rr = al * s.e_rr + (1.0 - al) * rr / n;
            s.e_m = al * s.e_m + (1.0 - al) * mm / n;
            s.e_s = al * s.e_s + (1.0 - al) * ss / n;
            let rho = s.e_lr / ((s.e_ll * s.e_rr).sqrt() + 1e-12);
            let guard = (rho / 0.4).clamp(0.0, 1.0) * (s.e_m / (s.e_s + 1e-12)).clamp(0.0, 1.0);
            // straż wygładzana dodatkowo (τ 50 ms), a wzmocnienia S interpolowane liniowo w bloku –
            // bez tego skoki k co 16 próbek modulowały S (test dymny: nieciągłości 15 → 88 na 4 fragmentach)
            s.guard_s = s.a_guard * s.guard_s + (1.0 - s.a_guard) * guard;
            let k_hi = 1.0 + 1.1 * a * s.guard_s;
            let k_lo = 1.0 - 0.5 * a;
            let (kh0, kl0) = (s.k_hi_prev, s.k_lo_prev);
            let inv = 1.0 / cl.len() as f64;
            for i in 0..cl.len() {
                let t = (i + 1) as f64 * inv;
                let kh = kh0 + (k_hi - kh0) * t;
                let kl = kl0 + (k_lo - kl0) * t;
                let m = 0.5 * (cl[i] + cr[i]);
                let d = 0.5 * (cl[i] - cr[i]);
                let sh = s.hp.tick(d);
                let sl = d - sh;
                let d2 = kh * sh + kl * sl;
                cl[i] = m + d2;
                cr[i] = m - d2;
            }
            s.k_hi_prev = k_hi;
            s.k_lo_prev = k_lo;
        });
        self.hp.flush_denormals();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard() {
        crate::modules::testkit::standard(|c| Box::new(Width::new(c)), ModuleId::Width, 1e-12);
    }
    #[test]
    fn mono_sum_unchanged() {
        let fs = 44_100.0;
        let mut w = Width::new(Ctx::new(fs));
        w.set_amount(ModuleId::Width, 1.0);
        w.reset(Ctx::new(fs));
        let (xl, xr) = crate::modules::testkit::music(20_000, fs);
        let (mut l, mut r) = (xl.clone(), xr.clone());
        crate::modules::testkit::run(&mut w, &mut l, &mut r);
        for i in 0..l.len() {
            assert!(((l[i] + r[i]) - (xl[i] + xr[i])).abs() < 1e-12);
        }
    }
}
