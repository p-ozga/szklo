//! Równy strumień – muzyka jako spokojne, jednostajne tło do pracy (projekt własny z literatury).
//!
//! Skąd pomysł: dźwięk przeszkadza w pracy umysłowej tym bardziej, im mocniej się ZMIENIA („changing-state”, Jones i Macken
//! 1993); miarą jest siła fluktuacji głośności, największa przy ok. 4 zmianach na sekundę (Schlittmeier i in. 2012), a nagłe
//! zdarzenia wywołują odruch orientacyjny. Filtr nie zmienia barwy ani średniej głośności – zmniejsza wahania wokół średniej.
//!
//! Tor jak w Krysztale, ale w przeciwną stronę: wyjście = wejście + Σ (g_b − 1)·BP_b(x), sześć pasm. W każdym paśmie
//! obwiednia szybka i bardzo wolna (ok. 0,4 s); r = szybka/wolna w dB:
//!   wzmocnienie = −0,8·a·r, w granicach −9a … +5a dB (uderzenia ciszej, dołki między nimi głośniej).
//! Osłony: cisza i szum tła nie są podnoszone; wolne wyrównanie średniego poziomu pasma. Każde pasmo steruje tylko sobą.
//! Nie jest to modulacja dźwięku w żadnym rytmie – przeciwnie, filtr usuwa część istniejących wahań.

use super::und;
use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};

const N: usize = 6;
const F: [f64; N] = [80.0, 200.0, 500.0, 1200.0, 3000.0, 7500.0];
const Q: f64 = 0.75;
// obwiednia „szybka” prawie symetryczna: przy opadaniu dużo wolniejszym niż narastanie filtr przy pełnym suwaku sam
// dodawał wahania (pomiar 2026-11-25: fluktuacja −3,1 dB przy 50%, ale tylko −1,3 dB przy 100%)
const T_FA: [f64; N] = [0.012, 0.008, 0.005, 0.004, 0.003, 0.003];
const T_FR: [f64; N] = [0.030, 0.022, 0.015, 0.012, 0.010, 0.010];
const T_S: [f64; N] = [0.45, 0.40, 0.35, 0.30, 0.30, 0.30];
const T_GD: [f64; N] = [0.006, 0.004, 0.003, 0.002, 0.0015, 0.001];
const T_GU: [f64; N] = [0.020, 0.015, 0.010, 0.008, 0.006, 0.006];
const KAPPA: f64 = 0.8;
const DOWN_MAX: f64 = 9.0;
const UP_MAX: f64 = 5.0;

#[derive(Clone, Copy, Default)]
struct Band {
    f: [Svf; 2],
    fast: f64,
    slow: f64,
    g_db: f64,
    d: f64,
    p_in: f64,
    p_out: f64,
    c: [f64; 5],
}

pub struct EvenStream {
    ctx: Ctx,
    a: Smoother,
    b: [Band; N],
    a_long: f64,
    meter: f32,
}

impl EvenStream {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self { ctx, a: Smoother::new(0.0, 0.02, ctx.fs), b: [Band::default(); N], a_long: 0.0, meter: 0.0 };
        s.reset(ctx);
        s
    }
}

#[inline]
fn db(p: f64) -> f64 {
    10.0 * (p + 1e-14).log10()
}

impl Module for EvenStream {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::EvenStream]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        let k = ctx.fs / CONTROL_BLOCK as f64;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        for (i, b) in self.b.iter_mut().enumerate() {
            let c = SvfCoeffs::new(ctx.fs, F[i], Q, Mode::Bandpass);
            *b = Band { f: [Svf::new(c); 2], ..Band::default() };
            b.c = [T_FA[i], T_FR[i], T_S[i], T_GD[i], T_GU[i]].map(|t| tau_coeff(t, k));
        }
        self.a_long = tau_coeff(3.0, k);
        self.meter = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = l.len();
        let mut off = 0;
        let mut meter = 0.0f64;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let inv = 1.0 / k as f64;
            let a = self.a.advance(k);
            let (xl, xr) = (&mut l[off..off + k], &mut r[off..off + k]);
            let mut add = [[0.0f64; CONTROL_BLOCK]; 2];
            for b in self.b.iter_mut() {
                let mut y = [[0.0f64; CONTROL_BLOCK]; 2];
                let mut p = 0.0;
                for j in 0..k {
                    y[0][j] = b.f[0].tick(xl[j]);
                    y[1][j] = b.f[1].tick(xr[j]);
                    p += y[0][j] * y[0][j] + y[1][j] * y[1][j];
                }
                p *= 0.5 * inv;
                let c = b.c;
                b.fast = p + if p > b.fast { c[0] } else { c[1] } * (b.fast - p);
                b.slow = p + c[2] * (b.slow - p);
                let ls = db(b.slow);
                let ratio = db(b.fast) - ls;
                let mut g = (-KAPPA * ratio).clamp(-DOWN_MAX, UP_MAX);
                if g > 0.0 {
                    g *= ((ls + 70.0) / 14.0).clamp(0.0, 1.0); // pauz i szumu nie podnosimy
                }
                b.g_db = g + if g < b.g_db { c[3] } else { c[4] } * (b.g_db - g);
                if b.g_db.abs() < 1e-6 && g == 0.0 {
                    b.g_db = 0.0;
                }
                let ge = und(a * b.g_db);
                b.p_in = p + self.a_long * (b.p_in - p);
                let po = p * ge * ge;
                b.p_out = po + self.a_long * (b.p_out - po);
                let mk = (db(b.p_in) - db(b.p_out)).clamp(-3.0, 3.0) * (a / 0.1).min(1.0);
                let d = if a > 0.0 { und(a * b.g_db + mk) - 1.0 } else { 0.0 };
                meter = meter.max(-a * b.g_db);
                if d != 0.0 || b.d != 0.0 {
                    let st = (d - b.d) * inv;
                    let mut gg = b.d;
                    for j in 0..k {
                        gg += st;
                        add[0][j] += gg * y[0][j];
                        add[1][j] += gg * y[1][j];
                    }
                }
                b.d = d;
            }
            for j in 0..k {
                xl[j] += add[0][j];
                xr[j] += add[1][j];
            }
            off += k;
        }
        for b in self.b.iter_mut() {
            b.f[0].flush_denormals();
            b.f[1].flush_denormals();
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
        testkit::standard(|c| Box::new(EvenStream::new(c)), ModuleId::EvenStream, 0.0);
    }

    /// Szum falujący 4 razy na sekundę (±6 dB): po filtrze wahania obwiedni wyraźnie mniejsze, średni poziom ten sam.
    #[test]
    fn four_hertz_fluctuation_is_reduced() {
        let fs = 48_000.0;
        let n = 48_000 * 10;
        let nz = testkit::noise(n, 0.2, 5);
        let x: Vec<f64> = (0..n).map(|i| nz[i] * 10f64.powf(6.0 * (2.0 * PI * 4.0 * i as f64 / fs).sin() / 20.0)).collect();
        let ctx = Ctx::new(fs);
        let mut m = EvenStream::new(ctx);
        m.set_amount(ModuleId::EvenStream, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (x.clone(), x.clone());
        testkit::run(&mut m, &mut l, &mut r);
        // obwiednia w oknach 25 ms, odchylenie standardowe w dB
        let wahania = |v: &[f64]| {
            let e: Vec<f64> = v[n / 2..].chunks(1200).map(|c| 10.0 * (c.iter().map(|q| q * q).sum::<f64>() / 1200.0 + 1e-12).log10()).collect();
            let m = e.iter().sum::<f64>() / e.len() as f64;
            ((e.iter().map(|q| (q - m).powi(2)).sum::<f64>() / e.len() as f64).sqrt(), m)
        };
        let ((s0, m0), (s1, m1)) = (wahania(&x), wahania(&l));
        assert!(s1 < 0.6 * s0, "wahania {s0} → {s1} dB");
        assert!((m1 - m0).abs() < 2.5, "poziom {m0} → {m1}");
    }

    #[test]
    fn silence_is_not_lifted() {
        let x = testkit::noise(48_000 * 4, 1e-4, 2);
        let ctx = Ctx::new(48_000.0);
        let mut m = EvenStream::new(ctx);
        m.set_amount(ModuleId::EvenStream, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (x.clone(), x.clone());
        testkit::run(&mut m, &mut l, &mut r);
        let e = |v: &[f64]| v.iter().map(|q| q * q).sum::<f64>();
        assert!(10.0 * (e(&l) / e(&x)).log10() < 0.5);
    }
}
