//! Punch (spec v1 §4.11) – ekspansja w górę transjentów, 3 pasma LR4 (150 Hz, 3 kHz).
//! Na blok sterujący w paśmie: e_f (atak 0,3 ms, zwolnienie 30 ms), e_s (atak 25 ms, zwolnienie 30 ms);
//! d = dB((e_f + 1e−3)/(e_s + 1e−3)); G = min(κ·max(d − 1, 0), G_max) dB; wygładzenie G: narastanie 1 ms, opadanie 30 ms.
//! Wagi pasm 1,0 / 1,0 / 0,6. Suwak: κ = 0,8a, G_max = 6a dB.

use super::{und, GainRamp};
use crate::catalog::ModuleId;
use crate::dsp::crossover::Crossover;
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK, MAX_BLOCK};

const W: [f64; 3] = [1.0, 1.0, 0.6];

pub struct Punch {
    ctx: Ctx,
    a: Smoother,
    xl: Crossover,
    xr: Crossover,
    ef: [f64; 3],
    es: [f64; 3],
    g: [f64; 3],
    ramp: [GainRamp; 3],
    c: [f64; 6], // ef_att, ef_rel, es_att, es_rel, g_up, g_dn
    bl: Vec<[f64; 3]>,
    br: Vec<[f64; 3]>,
}

impl Punch {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            xl: Crossover::new(ctx.fs, &[150.0, 3000.0]),
            xr: Crossover::new(ctx.fs, &[150.0, 3000.0]),
            ef: [0.0; 3],
            es: [0.0; 3],
            g: [0.0; 3],
            ramp: [GainRamp::default(); 3],
            c: [0.0; 6],
            bl: vec![[0.0; 3]; MAX_BLOCK],
            br: vec![[0.0; 3]; MAX_BLOCK],
        };
        s.reset(ctx);
        s
    }
}

impl Module for Punch {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Punch]
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
        self.xl = Crossover::new(ctx.fs, &[150.0, 3000.0]);
        self.xr = Crossover::new(ctx.fs, &[150.0, 3000.0]);
        let k = ctx.fs / CONTROL_BLOCK as f64;
        self.c = [tau_coeff(0.0003, k), tau_coeff(0.03, k), tau_coeff(0.025, k), tau_coeff(0.03, k), tau_coeff(0.001, k), tau_coeff(0.03, k)];
        self.ef = [0.0; 3];
        self.es = [0.0; 3];
        self.g = [0.0; 3];
        self.ramp = [GainRamp::default(); 3];
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = l.len();
        let mut off = 0;
        let mut tmp = [0.0; 5];
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let a = self.a.advance(k);
            let kappa = 0.8 * a;
            let gmax = 6.0 * a;
            let mut pk = [0.0f64; 3];
            for i in 0..k {
                self.xl.split(l[off + i], &mut tmp);
                self.bl[i] = [tmp[0], tmp[1], tmp[2]];
                self.xr.split(r[off + i], &mut tmp);
                self.br[i] = [tmp[0], tmp[1], tmp[2]];
                for b in 0..3 {
                    pk[b] = pk[b].max(self.bl[i][b].abs()).max(self.br[i][b].abs());
                }
            }
            let c = self.c;
            let mut target = [1.0f64; 3];
            for b in 0..3 {
                let x = pk[b];
                self.ef[b] = if x > self.ef[b] { c[0] * self.ef[b] + (1.0 - c[0]) * x } else { c[1] * self.ef[b] + (1.0 - c[1]) * x };
                self.es[b] = if x > self.es[b] { c[2] * self.es[b] + (1.0 - c[2]) * x } else { c[3] * self.es[b] + (1.0 - c[3]) * x };
                let d = 20.0 * ((self.ef[b] + 1e-3) / (self.es[b] + 1e-3)).log10();
                let gt = (kappa * (d - 1.0).max(0.0)).min(gmax);
                self.g[b] = if gt > self.g[b] { c[4] * self.g[b] + (1.0 - c[4]) * gt } else { c[5] * self.g[b] + (1.0 - c[5]) * gt };
                if self.g[b] < 1e-6 && gt == 0.0 {
                    self.g[b] = 0.0;
                }
                target[b] = und(self.g[b] * W[b]);
            }
            for b in 0..3 {
                // rampa per pasmo: liczymy przyrost i stosujemy próbka po próbce
                let g0 = self.ramp[b].cur;
                let d = (target[b] - g0) / k as f64;
                let mut g = g0;
                for i in 0..k {
                    g += d;
                    self.bl[i][b] *= g;
                    self.br[i][b] *= g;
                }
                self.ramp[b].cur = target[b];
            }
            for i in 0..k {
                l[off + i] = self.bl[i][0] + self.bl[i][1] + self.bl[i][2];
                r[off + i] = self.br[i][0] + self.br[i][1] + self.br[i][2];
            }
            off += k;
        }
        self.xl.flush_denormals();
        self.xr.flush_denormals();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard() {
        crate::modules::testkit::standard(|c| Box::new(Punch::new(c)), ModuleId::Punch, 10.0);
    }
}
