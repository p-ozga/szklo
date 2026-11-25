//! Mniej słów – ścisza to, co jest na samym środku sceny w paśmie mowy, czyli zwykle śpiew (projekt własny).
//!
//! Skąd pomysł: przy czytaniu, pisaniu i uczeniu się rozpraszają przede wszystkim ZROZUMIAŁE słowa (Perham i Currie 2014;
//! Souza i Barbosa 2023), a szkoda rośnie ze zrozumiałością mowy (Hongisto 2005, Haapakangas i in. 2020: od STI ok. 0,2,
//! pełna przy ok. 0,45). Muzyka instrumentalna prawie nie szkodzi. Filtr obniża stosunek wokalu do reszty muzyki.
//!
//! Tor (wejście + Δ; przy a = 0 dokładnie przezroczysty; bas i wysoka góra bez zmian): sześć pasm 350 Hz…4,6 kHz
//! (filtry pasmowe SVF). W każdym paśmie co ok. 0,3 ms liczymy podobieństwo kanałów (korelacja × równość poziomów,
//! uśredniane 60 ms). Im bardziej pasmo jest „na środku”, tym więcej jego wspólnej części (L+R)/2 odejmujemy z obu kanałów:
//!   L′ = L − k·M_b,  R′ = R − k·M_b,  k = 0,84·a·środkowość  (do −16 dB środka; boki pasma zostają nietknięte).
//! Znane od dziesięcioleci odejmowanie kanałów („karaoke”) i sterowanie korelacją pasm (dekodery matrycowe lat 80.);
//! bez transformaty widmowej, bez rozpoznawania mowy i bez wydzielania ścieżek.

use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};

const N: usize = 6;
const F: [f64; N] = [350.0, 600.0, 1000.0, 1700.0, 2800.0, 4600.0];
const Q: f64 = 1.4;
const DEPTH: f64 = 0.84;

#[derive(Clone, Copy, Default)]
struct Band {
    f: [Svf; 2],
    ll: f64,
    rr: f64,
    lr: f64,
    /// poprzednie k (interpolacja w bloku)
    k: f64,
}

pub struct LessWords {
    ctx: Ctx,
    a: Smoother,
    b: [Band; N],
    c_avg: f64,
    c_k: f64,
    /// 1 / największa suma charakterystyk pasm (żeby nakładające się pasma nie odejmowały za dużo)
    norm: f64,
}

impl LessWords {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self { ctx, a: Smoother::new(0.0, 0.02, ctx.fs), b: [Band::default(); N], c_avg: 0.0, c_k: 0.0, norm: 1.0 };
        s.reset(ctx);
        s
    }
}

/// Największy moduł sumy charakterystyk filtrów pasmowych (prototyp analogowy) w paśmie pracy.
fn sum_peak() -> f64 {
    let mut peak = 0.0f64;
    for i in 0..200 {
        let f = F[0] * (F[N - 1] / F[0]).powf(i as f64 / 199.0);
        let (mut re, mut im) = (0.0, 0.0);
        for f0 in F {
            // H = (jx/Q) / (1 − x² + jx/Q), x = f/f0
            let x = f / f0;
            let (a, b) = (1.0 - x * x, x / Q);
            let den = a * a + b * b;
            re += b * b / den;
            im += a * b / den;
        }
        peak = peak.max((re * re + im * im).sqrt());
    }
    peak
}

impl Module for LessWords {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::LessWords]
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
            *b = Band { f: [Svf::new(SvfCoeffs::new(ctx.fs, F[i], Q, Mode::Bandpass)); 2], ..Band::default() };
        }
        self.c_avg = tau_coeff(0.060, k);
        self.c_k = tau_coeff(0.030, k);
        self.norm = 1.0 / sum_peak();
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = l.len();
        let mut off = 0;
        while off < n {
            let kk = (n - off).min(CONTROL_BLOCK);
            let inv = 1.0 / kk as f64;
            let a = self.a.advance(kk);
            let (xl, xr) = (&mut l[off..off + kk], &mut r[off..off + kk]);
            let mut sub = [0.0f64; CONTROL_BLOCK];
            for b in self.b.iter_mut() {
                let mut m = [0.0f64; CONTROL_BLOCK];
                let (mut ll, mut rr, mut lr) = (0.0, 0.0, 0.0);
                for j in 0..kk {
                    let (yl, yr) = (b.f[0].tick(xl[j]), b.f[1].tick(xr[j]));
                    m[j] = 0.5 * (yl + yr);
                    ll += yl * yl;
                    rr += yr * yr;
                    lr += yl * yr;
                }
                b.ll = ll + self.c_avg * (b.ll - ll);
                b.rr = rr + self.c_avg * (b.rr - rr);
                b.lr = lr + self.c_avg * (b.lr - lr);
                let geo = (b.ll * b.rr).sqrt();
                // korelacja × równość poziomów: 1 = dokładnie na środku, 0 = nieskorelowane albo z jednej strony
                let center = if geo > 1e-14 { (b.lr / geo) * (2.0 * geo / (b.ll + b.rr)) } else { 0.0 };
                let want = a * DEPTH * self.norm * ((center - 0.35) / 0.4).clamp(0.0, 1.0);
                let k1 = want + self.c_k * (b.k - want);
                let k1 = if k1 < 1e-7 && want == 0.0 { 0.0 } else { k1 };
                if k1 != 0.0 || b.k != 0.0 {
                    let st = (k1 - b.k) * inv;
                    let mut g = b.k;
                    for j in 0..kk {
                        g += st;
                        sub[j] += g * m[j];
                    }
                }
                b.k = k1;
            }
            for j in 0..kk {
                xl[j] -= sub[j];
                xr[j] -= sub[j];
            }
            off += kk;
        }
        for b in self.b.iter_mut() {
            b.f[0].flush_denormals();
            b.f[1].flush_denormals();
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
        testkit::standard(|c| Box::new(LessWords::new(c)), ModuleId::LessWords, 0.0);
    }

    fn przetworz(l: &[f64], r: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let ctx = Ctx::new(48_000.0);
        let mut m = LessWords::new(ctx);
        m.set_amount(ModuleId::LessWords, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (l.to_vec(), r.to_vec());
        testkit::run(&mut m, &mut l, &mut r);
        (l, r)
    }
    fn db(a: &[f64], b: &[f64]) -> f64 {
        let e = |v: &[f64]| v[v.len() / 2..].iter().map(|q| q * q).sum::<f64>() + 1e-30;
        10.0 * (e(a) / e(b)).log10()
    }

    /// „Głos” na środku (1 kHz, to samo w obu kanałach) ciszej o 8–17 dB; ten sam ton tylko w lewym kanale – bez zmian.
    #[test]
    fn center_voice_down_side_instrument_stays() {
        let fs = 48_000.0;
        let x: Vec<f64> = (0..96_000).map(|i| 0.2 * (2.0 * PI * 1000.0 * i as f64 / fs).sin()).collect();
        let (l, _) = przetworz(&x, &x);
        let d = db(&l, &x);
        assert!(d < -8.0 && d > -17.0, "środek {d} dB");
        let z = vec![0.0; x.len()];
        let (l, r) = przetworz(&x, &z);
        assert!(db(&l, &x).abs() < 0.3, "bok {} dB", db(&l, &x));
        assert!(r.iter().all(|v| v.abs() < 1e-3));
    }

    /// Bas i najwyższa góra na środku zostają.
    #[test]
    fn bass_and_air_untouched() {
        let fs = 48_000.0;
        for f in [60.0, 12_000.0] {
            let x: Vec<f64> = (0..96_000).map(|i| 0.2 * (2.0 * PI * f * i as f64 / fs).sin()).collect();
            let (l, _) = przetworz(&x, &x);
            assert!(db(&l, &x).abs() < 1.0, "{f} Hz: {} dB", db(&l, &x));
        }
    }

    /// Szerokie, nieskorelowane brzmienie (np. gitary po bokach) bez zmian.
    #[test]
    fn wide_sound_untouched() {
        let (a, b) = (testkit::noise(96_000, 0.2, 1), testkit::noise(96_000, 0.2, 2));
        let (l, _) = przetworz(&a, &b);
        assert!(db(&l, &a).abs() < 0.3);
    }
}
