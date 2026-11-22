//! Dekwantyzer (spec v1 §4.5): skwantowana próbka to przedział [x − q/2, x + q/2]; szukamy gładszego sygnału,
//! który z niego nie wychodzi (błąd ≤ q/2 z konstrukcji).
//!
//! Wykrycie siatki (okna 0,5 s): ≥ 95 % próbek z |x·2^15 − round(x·2^15)| < 1e−3 → q = 2^−15 (analogicznie 8 bit).
//! y⁰ = x; 4×: y ← splot(y, [1,4,6,4,1]/16); y ← clamp(y, x − q/2, x + q/2).
//! Waga poziomu: w = 1 − sstep((L_pk − (L_on − 6))/12); wyjście x + m·w·(y − x).
//! Suwak: m = min(1, 2a); L_on = lin(a, −72, −54) dBFS. Latencja 8 (4 przebiegi × 2 próbki).

use super::{lin, sstep};
use crate::catalog::ModuleId;
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module};

const PASSES: usize = 4;
const LAT: usize = 2 * PASSES; // każdy splot 5-próbkowy symetryczny = 2 próbki opóźnienia
struct Chan {
    /// historia wejść każdego przebiegu (5 próbek)
    stages: [[f64; 5]; PASSES],
    /// x wyrównane z wejściem przebiegu (3 ostatnie: [n−2, n−1, n])
    xs: [[f64; 3]; PASSES],
}

impl Chan {
    fn new() -> Self {
        Self { stages: [[0.0; 5]; PASSES], xs: [[0.0; 3]; PASSES] }
    }
}

pub struct Dequantizer {
    ctx: Ctx,
    a: Smoother,
    ch: [Chan; 2],
    // detekcja siatki
    q: f64,
    on_grid: usize,
    count: usize,
    win_len: usize,
    fade: f64,
    fade_step: f64,
    // obwiednia szczytowa
    pk: f64,
    a_pk: f64,
}

impl Dequantizer {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            ch: [Chan::new(), Chan::new()],
            q: 0.0,
            on_grid: 0,
            count: 0,
            win_len: 1,
            fade: 0.0,
            fade_step: 0.0,
            pk: 0.0,
            a_pk: 0.0,
        };
        s.reset(ctx);
        s
    }
    fn grid_ok(x: f64, bits: i32) -> bool {
        let s = x * 2f64.powi(bits);
        (s - s.round()).abs() < 1e-3
    }
}

/// Jeden przebieg: splot [1,4,6,4,1]/16 (opóźnienie 2), potem ograniczenie do przedziału wokół x opóźnionego o 2.
#[inline]
fn pass(hist: &mut [f64; 5], xs: &mut [f64; 3], v: f64, xd: f64, q: f64) -> f64 {
    hist.copy_within(1.., 0);
    hist[4] = v;
    let y = (hist[0] + 4.0 * hist[1] + 6.0 * hist[2] + 4.0 * hist[3] + hist[4]) / 16.0;
    // x odpowiadające wyjściu (opóźnione o 2 względem v) – przechowywane w xs[0..3]
    xs.copy_within(1..3, 0);
    xs[2] = xd;
    let xc = xs[0];
    y.clamp(xc - 0.5 * q, xc + 0.5 * q)
}

impl Module for Dequantizer {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Dequantizer]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        LAT
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        self.ch = [Chan::new(), Chan::new()];
        self.q = 0.0;
        self.on_grid = 0;
        self.count = 0;
        self.win_len = ctx.ms(500.0);
        self.fade = 0.0;
        self.fade_step = 1.0 / (0.05 * ctx.fs);
        self.pk = 0.0;
        self.a_pk = tau_coeff(0.05, ctx.fs);
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let a = self.a.advance(l.len());
        let m = (2.0 * a).min(1.0);
        let l_on = lin(a, -72.0, -54.0);
        for i in 0..l.len() {
            let (xl, xr) = (l[i], r[i]);
            // detekcja siatki (16 bit, potem 8 bit)
            if Self::grid_ok(xl, 15) && Self::grid_ok(xr, 15) {
                self.on_grid += 1;
            }
            self.count += 1;
            if self.count >= self.win_len {
                let frac = self.on_grid as f64 / self.count as f64;
                self.q = if frac >= 0.95 { 2f64.powi(-15) } else { 0.0 };
                self.count = 0;
                self.on_grid = 0;
            }
            let target_fade = if self.q > 0.0 { 1.0 } else { 0.0 };
            self.fade = if self.fade < target_fade { (self.fade + self.fade_step).min(1.0) } else { (self.fade - self.fade_step).max(0.0) };
            // obwiednia szczytowa
            let p = xl.abs().max(xr.abs());
            self.pk = if p > self.pk { p } else { p + self.a_pk * (self.pk - p) };
            let lpk = 20.0 * (self.pk + 1e-12).log10();
            let w = 1.0 - sstep((lpk - (l_on - 6.0)) / 12.0);
            let q = if self.q > 0.0 { self.q } else { 2f64.powi(-15) };
            let mix = m * w * self.fade;
            for (c, x, out) in [(0usize, xl, &mut l[i]), (1usize, xr, &mut r[i])] {
                let ch = &mut self.ch[c];
                let mut v = x;
                let mut xd = x;
                for p in 0..PASSES {
                    let y = pass(&mut ch.stages[p], &mut ch.xs[p], v, xd, q);
                    // x opóźnione o 2(p+1)
                    xd = ch.xs[p][0];
                    v = y;
                }
                let x_lat = xd; // = x[n − LAT]
                *out = if mix > 0.0 { x_lat + mix * (v - x_lat) } else { x_lat };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard() {
        crate::modules::testkit::standard(|c| Box::new(Dequantizer::new(c)), ModuleId::Dequantizer, 1e-12);
    }
    #[test]
    fn error_bounded_by_half_lsb_and_latency_8() {
        let fs = 44_100.0;
        let mut m = Dequantizer::new(Ctx::new(fs));
        m.set_amount(ModuleId::Dequantizer, 1.0);
        m.reset(Ctx::new(fs));
        let q = 2f64.powi(-15);
        let n = 60_000;
        // cichy sinus skwantowany do 16 bitów
        let x: Vec<f64> = (0..n).map(|i| ((0.0005 * (i as f64 * 0.01).sin()) / q).round() * q).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        crate::modules::testkit::run(&mut m, &mut l, &mut r);
        let mut max_err = 0.0f64;
        let mut changed = 0;
        for i in (LAT + 30_000)..n {
            let e = (l[i] - x[i - LAT]).abs();
            max_err = max_err.max(e);
            if e > 0.0 {
                changed += 1;
            }
        }
        assert!(max_err <= 0.5 * q + 1e-15, "błąd {max_err} > q/2");
        assert!(changed > 1000, "moduł nic nie zmienił ({changed})");
    }
}
