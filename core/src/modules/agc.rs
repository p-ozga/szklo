//! AGC (spec v1 §4.7): wyrównanie głośności między utworami.
//!
//! Ważenie K obu kanałów → bloki 100 ms: z_i = Σ_kan mean(y_K²); L_M (4 bloki), L_S (30 bloków = 3 s).
//! Bramka: aktywne ⇔ L_M > max(−50 LUFS, L_est − 20 LU); wtedy L_est = L_S, inaczej zamrożone.
//! G_cel = clamp(k_a·(T − L_est), −12, +R_up), T = −18 LUFS; co 100 ms G += clamp(G_cel − G, −v_dn·0,1, +v_up·0,1);
//! awaryjnie (L_M + G > T + 8) v_dn × 3. Wzmocnienie: liniowa rampa und(G) w obrębie 100 ms.
//! Suwak: k_a = min(1, 2a), R_up = lin(a, 6, 12), v_up = lin(a, 0,5, 3) dB/s, v_dn = lin(a, 2, 8) dB/s.

use super::{lin, und};
use crate::catalog::ModuleId;
use crate::dsp::loudness::KWeighting;
use crate::dsp::util::Smoother;
use crate::module::{Ctx, Module};

const TARGET: f64 = -18.0;

pub struct Agc {
    ctx: Ctx,
    a: Smoother,
    kl: KWeighting,
    kr: KWeighting,
    block_len: usize,
    acc: f64,
    cnt: usize,
    z: [f64; 30],
    zpos: usize,
    zfill: usize,
    l_est: Option<f64>,
    g_db: f64,
    // rampa liniowa wzmocnienia w obrębie bloku 100 ms
    g_cur: f64,
    g_step: f64,
}

impl Agc {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            kl: KWeighting::new(ctx.fs),
            kr: KWeighting::new(ctx.fs),
            block_len: 1,
            acc: 0.0,
            cnt: 0,
            z: [0.0; 30],
            zpos: 0,
            zfill: 0,
            l_est: None,
            g_db: 0.0,
            g_cur: 1.0,
            g_step: 0.0,
        };
        s.reset(ctx);
        s
    }

    fn mean_last(&self, k: usize) -> f64 {
        let k = k.min(self.zfill).max(1);
        let mut s = 0.0;
        for j in 0..k {
            s += self.z[(self.zpos + 30 - 1 - j) % 30];
        }
        s / k as f64
    }

    fn end_block(&mut self) {
        let zi = self.acc / self.cnt.max(1) as f64;
        self.z[self.zpos] = zi;
        self.zpos = (self.zpos + 1) % 30;
        self.zfill = (self.zfill + 1).min(30);
        self.acc = 0.0;
        self.cnt = 0;
        let lufs = |ms: f64| -0.691 + 10.0 * (ms + 1e-18).log10();
        let l_m = lufs(self.mean_last(4));
        let l_s = lufs(self.mean_last(30));
        let gate = (-50.0f64).max(self.l_est.map(|e| e - 20.0).unwrap_or(-50.0));
        if l_m > gate {
            self.l_est = Some(l_s);
        }
        let a = self.a.value;
        let k_a = (2.0 * a).min(1.0);
        let r_up = lin(a, 6.0, 12.0);
        let v_up = lin(a, 0.5, 3.0);
        let mut v_dn = lin(a, 2.0, 8.0);
        let g_cel = match self.l_est {
            Some(e) => (k_a * (TARGET - e)).clamp(-12.0, r_up),
            None => 0.0,
        };
        if l_m + self.g_db > TARGET + 8.0 {
            v_dn *= 3.0;
        }
        self.g_db += (g_cel - self.g_db).clamp(-v_dn * 0.1, v_up * 0.1);
        if self.g_db.abs() < 1e-9 {
            self.g_db = 0.0;
        }
        let target = und(self.g_db);
        self.g_step = (target - self.g_cur) / self.block_len as f64;
    }
}

impl Module for Agc {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Agc]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn warmup_samples(&self) -> usize {
        // 3 s na zebranie pierwszej estymaty głośności – AGC startuje z G = 0 dB, więc to tylko 0,5 s
        self.ctx.ms(500.0)
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        self.kl = KWeighting::new(ctx.fs);
        self.kr = KWeighting::new(ctx.fs);
        self.block_len = ctx.ms(100.0);
        self.acc = 0.0;
        self.cnt = 0;
        self.z = [0.0; 30];
        self.zpos = 0;
        self.zfill = 0;
        self.l_est = None;
        self.g_db = 0.0;
        self.g_cur = 1.0;
        self.g_step = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        self.a.advance(l.len());
        for i in 0..l.len() {
            let (yl, yr) = (self.kl.tick(l[i]), self.kr.tick(r[i]));
            self.acc += yl * yl + yr * yr;
            self.cnt += 1;
            if self.g_step != 0.0 {
                self.g_cur += self.g_step;
            }
            if self.g_cur != 1.0 {
                l[i] *= self.g_cur;
                r[i] *= self.g_cur;
            }
            if self.cnt == self.block_len {
                // domknięcie rampy (bez dryfu)
                self.g_cur = und(self.g_db);
                self.end_block();
            }
        }
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        (-self.g_db).max(0.0) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard() {
        crate::modules::testkit::standard(|c| Box::new(Agc::new(c)), ModuleId::Agc, 1e-12);
    }
    #[test]
    fn quiet_signal_is_raised() {
        let fs = 44_100.0;
        let mut m = Agc::new(Ctx::new(fs));
        m.set_amount(ModuleId::Agc, 1.0);
        m.reset(Ctx::new(fs));
        let n = 44_100 * 8;
        let mut l: Vec<f64> = (0..n).map(|i| 0.03 * (i as f64 * 0.0713).sin()).collect();
        let mut r = l.clone();
        crate::modules::testkit::run(&mut m, &mut l, &mut r);
        let p = l[n - 4410..].iter().fold(0.0f64, |a, &v| a.max(v.abs()));
        assert!(p > 0.08, "AGC nie podniósł cichego sygnału: {p}");
    }
}
