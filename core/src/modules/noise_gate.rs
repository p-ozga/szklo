//! Bramka szumów (spec v1 §4.6): 4 pasma LR4 (150 Hz, 1 kHz, 5 kHz), w każdym ekspander w dół.
//! Poziom pasma: RMS τ = 10 ms; szum N_b = minimum (poziomu wygładzonego 100 ms) z ostatnich 10 s;
//! próg T_b = min(N_b + 6, T_max); c = (R_e − 1)·max(0, T_b − x_G) z kolanem 6 dB, c ≤ D_max;
//! histereza: otwarcie przy T_b + 3 dB. Balistyka: otwieranie 2 ms, przytrzymanie 50 ms, zamykanie 150 ms.
//! Suwak: R_e = lin(a, 1, 3); T_max = lin(a, −80, −50) dBFS; D_max = 18a dB.

use super::{lin, und};
use crate::catalog::ModuleId;
use crate::dsp::crossover::Crossover;
use crate::dsp::dynamics::Expander;
use crate::dsp::util::{tau_coeff, SlidingMin, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK, MAX_BLOCK};

pub struct NoiseGate {
    ctx: Ctx,
    a: Smoother,
    xl: Crossover,
    xr: Crossover,
    ms: [f64; 4],
    lvl100: [f64; 4],
    noise: Vec<SlidingMin>,
    open: [bool; 4],
    hold: [usize; 4],
    c: [f64; 4],
    ramp: [f64; 4],
    k: [f64; 4], // a_ms, a_100, a_open, a_close
    hold_blocks: usize,
    bl: Vec<[f64; 4]>,
    br: Vec<[f64; 4]>,
    gr: f32,
    dec_cnt: usize,
    noise_db: [f64; 4],
}

impl NoiseGate {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            xl: Crossover::new(ctx.fs, &[150.0, 1000.0, 5000.0]),
            xr: Crossover::new(ctx.fs, &[150.0, 1000.0, 5000.0]),
            ms: [0.0; 4],
            lvl100: [-120.0; 4],
            noise: Vec::new(),
            open: [true; 4],
            hold: [0; 4],
            c: [0.0; 4],
            ramp: [1.0; 4],
            k: [0.0; 4],
            hold_blocks: 1,
            bl: vec![[0.0; 4]; MAX_BLOCK],
            br: vec![[0.0; 4]; MAX_BLOCK],
            gr: 0.0,
            dec_cnt: 0,
            noise_db: [-120.0; 4],
        };
        s.reset(ctx);
        s
    }
}

impl Module for NoiseGate {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::NoiseGate]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn warmup_samples(&self) -> usize {
        self.ctx.ms(300.0)
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        self.xl = Crossover::new(ctx.fs, &[150.0, 1000.0, 5000.0]);
        self.xr = Crossover::new(ctx.fs, &[150.0, 1000.0, 5000.0]);
        let kfs = ctx.fs / CONTROL_BLOCK as f64;
        self.k = [tau_coeff(0.010, kfs), tau_coeff(0.100, kfs), tau_coeff(0.002, kfs), tau_coeff(0.150, kfs)];
        self.hold_blocks = (0.050 * kfs).round() as usize;
        // minimum z 10 s – na wartościach co 100 ms (100 próbek okna), żeby nie trzymać 27 tys. bloków
        self.noise = (0..4).map(|_| SlidingMin::new(100)).collect();
        self.ms = [0.0; 4];
        self.lvl100 = [-120.0; 4];
        self.open = [true; 4];
        self.hold = [0; 4];
        self.c = [0.0; 4];
        self.ramp = [1.0; 4];
        self.gr = 0.0;
        self.dec_cnt = 0;
        self.noise_db = [-120.0; 4];
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = l.len();
        let mut off = 0;
        let mut t5 = [0.0; 5];
        let kfs = self.ctx.fs / CONTROL_BLOCK as f64;
        let dec = (0.1 * kfs).round().max(1.0) as usize;
        let mut gr_max = 0.0f64;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let a = self.a.advance(k);
            let r_e = lin(a, 1.0, 3.0);
            let t_max = lin(a, -80.0, -50.0);
            let d_max = 18.0 * a;
            for i in 0..k {
                self.xl.split(l[off + i], &mut t5);
                self.bl[i] = [t5[0], t5[1], t5[2], t5[3]];
                self.xr.split(r[off + i], &mut t5);
                self.br[i] = [t5[0], t5[1], t5[2], t5[3]];
            }
            self.dec_cnt += 1;
            let push_noise = self.dec_cnt >= dec;
            if push_noise {
                self.dec_cnt = 0;
            }
            for b in 0..4 {
                let mut sq = 0.0;
                for i in 0..k {
                    sq += self.bl[i][b] * self.bl[i][b] + self.br[i][b] * self.br[i][b];
                }
                self.ms[b] = self.k[0] * self.ms[b] + (1.0 - self.k[0]) * sq / (2.0 * k as f64);
                let xg = 10.0 * (self.ms[b] + 1e-18).log10();
                self.lvl100[b] = self.k[1] * self.lvl100[b] + (1.0 - self.k[1]) * xg;
                if push_noise {
                    self.noise_db[b] = self.noise[b].push(self.lvl100[b]);
                }
                let t_b = (self.noise_db[b] + 6.0).min(t_max);
                if xg > t_b + 3.0 {
                    self.open[b] = true;
                    self.hold[b] = self.hold_blocks;
                } else if xg < t_b {
                    if self.hold[b] > 0 {
                        self.hold[b] -= 1;
                    } else {
                        self.open[b] = false;
                    }
                }
                let target = if self.open[b] || a <= 0.0 {
                    0.0
                } else {
                    Expander { threshold_db: t_b, ratio: r_e, knee_db: 6.0, range_db: d_max }.reduction_db(xg)
                };
                let al = if target < self.c[b] { self.k[2] } else { self.k[3] };
                self.c[b] = al * self.c[b] + (1.0 - al) * target;
                if self.c[b] < 1e-6 && target == 0.0 {
                    self.c[b] = 0.0;
                }
                gr_max = gr_max.max(self.c[b]);
                let g1 = und(-self.c[b]);
                let g0 = self.ramp[b];
                let d = (g1 - g0) / k as f64;
                let mut g = g0;
                for i in 0..k {
                    g += d;
                    self.bl[i][b] *= g;
                    self.br[i][b] *= g;
                }
                self.ramp[b] = g1;
            }
            for i in 0..k {
                l[off + i] = self.bl[i][0] + self.bl[i][1] + self.bl[i][2] + self.bl[i][3];
                r[off + i] = self.br[i][0] + self.br[i][1] + self.br[i][2] + self.br[i][3];
            }
            off += k;
        }
        self.xl.flush_denormals();
        self.xr.flush_denormals();
        self.gr = gr_max as f32;
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        self.gr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard() {
        crate::modules::testkit::standard(|c| Box::new(NoiseGate::new(c)), ModuleId::NoiseGate, 10.0);
    }
}
