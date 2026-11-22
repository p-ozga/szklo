//! Rotacja fazy (spec v1 §4.19): 4 all-passy 1. rzędu (TPT) w kaskadzie, ta sama f_r dla L i R.
//! Suwak: f_r = expo(a, 10, 200) Hz dla a ≥ 0,05, a poniżej płynnie do 2 Hz przy a = 0.
//! Odstępstwo od spec v1: 4 sekcje 1. rzędu przy 10 Hz dają przy 60 Hz 4·2·atan(10/60) ≈ 76° (nie < 20°),
//! więc neutralizacja schodzi do 2 Hz (≈ 15° przy 60 Hz) – przenikanie on/off bez grzebienia w basie.
//! Cel modułu: symetryczny przebieg (mowa) przed clipperem/limiterem. Zmiana f na TPT nie daje trzasków.

use super::{control_chunks, expo};

fn freq(a: f64) -> f64 {
    2.0 + (expo(a, 10.0, 200.0) - 2.0) * (a / 0.05).min(1.0)
}
use crate::catalog::ModuleId;
use crate::dsp::svf::Tpt1;
use crate::dsp::util::Smoother;
use crate::module::{Ctx, Module};

pub struct PhaseRotation {
    ctx: Ctx,
    a: Smoother,
    ap: [[Tpt1; 4]; 2],
}

impl PhaseRotation {
    pub fn new(ctx: Ctx) -> Self {
        Self { ctx, a: Smoother::new(0.0, 0.02, ctx.fs), ap: [[Tpt1::default(); 4]; 2] }
    }
}

impl Module for PhaseRotation {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::PhaseRotation]
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
        let f = freq(self.a.value);
        for ch in self.ap.iter_mut() {
            for a in ch.iter_mut() {
                *a = Tpt1::new(ctx.fs, f);
            }
        }
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let fs = self.ctx.fs;
        let (a_s, ap) = (&mut self.a, &mut self.ap);
        control_chunks(l, r, |cl, cr| {
            let settled = a_s.is_settled();
            let a = a_s.advance(cl.len());
            if !settled {
                let f = freq(a);
                for ch in ap.iter_mut() {
                    for x in ch.iter_mut() {
                        x.set(fs, f);
                    }
                }
            }
            for (buf, ch) in [cl, cr].into_iter().zip(ap.iter_mut()) {
                for v in buf.iter_mut() {
                    let mut y = *v;
                    for x in ch.iter_mut() {
                        y = x.ap(y);
                    }
                    *v = y;
                }
            }
        });
        for ch in self.ap.iter_mut() {
            for x in ch.iter_mut() {
                x.flush_denormals();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard() {
        // a = 0 to all-pass 2 Hz – nie bit-exact (≈ 15° przy 60 Hz)
        crate::modules::testkit::standard(|c| Box::new(PhaseRotation::new(c)), ModuleId::PhaseRotation, 0.15);
    }
}
