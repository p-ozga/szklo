//! Kaptur – muzyka jak przez czapkę albo kaptur: miękko, blisko i bardzo przestrzennie (projekt własny z literatury).
//!
//! Co się dzieje pod materiałem (badania nakryć głowy i włosów: Riederer 2003, Treeby i in. 2007, Pörschmann i in. 2019;
//! przegląd eksternalizacji: Best i in. 2020):
//!  1. tkanina tłumi górę dźwięku bezpośredniego (powyżej ok. 3–5 kHz) – jak przy dźwięku z większej odległości;
//!  2. dźwięk obiega głowę: każde ucho słyszy też drugi kanał – później, ciszej i bez góry;
//!  3. komora pod materiałem daje gęste, bardzo krótkie odbicia, INNE dla każdego ucha – spada podobieństwo sygnałów
//!     w uszach (spójność międzyuszna), a to właśnie słyszymy jako „szeroko” i „na zewnątrz głowy”.
//!
//! Tor (Δ dodawana do wejścia, przy a = 0 dokładnie przezroczysty; bas < ok. 200 Hz bez zmian):
//!  * półka −5 dB powyżej ok. 4 kHz o stałych biegunach (jak w Dynamicznej górze, tu w dół);
//!  * przesłuch: drugi kanał przez LP 900 Hz z opóźnieniem 0,3 ms (Bauer 1961), z odjęciem własnego – mono się nie zmienia;
//!    przesłuch jest sam dla siebie – nic dalej go nie przetwarza;
//!  * odbicia: 22 odczepy 0,8…12 ms i 30 odczepów 12…48 ms na ucho, rzadkie ciągi ±1 z zanikiem, inne dla każdego ucha
//!    (Schroeder 1962 – odbicia linią opóźniającą z odczepami; Lauridsen – nieskorelowane kanały). Źródło: WŁASNY kanał
//!    w pełnym paśmie (bez samego basu), przyciemniony półką; sygnału nie dzielimy na dół i górę i nie mieszamy kanałów
//!    przed odbiciami. Bez sprzężeń zwrotnych, bez filtrów HRTF.

use super::control_chunks;
use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs, Tpt1};
use crate::dsp::util::Smoother;
use crate::module::{Ctx, Module};
use std::f64::consts::FRAC_1_SQRT_2;

const EARLY: usize = 22;
const LATE: usize = 30;
const G_SHELF_DB: f64 = -5.0;
const G_CROSS: f64 = 0.42;
const G_EARLY: f64 = 0.62;
const G_LATE: f64 = 0.50;

#[derive(Clone, Copy, Default)]
struct Tap {
    d: usize,
    g: f64,
}

pub struct Hood {
    ctx: Ctx,
    a: Smoother,
    shelf: [Svf; 2],
    cross_lp: [Tpt1; 2],
    hp: [Svf; 2],
    dark: [Svf; 2],
    buf: [Vec<f64>; 2],
    buf_c: [Vec<f64>; 2],
    mask: usize,
    pos: usize,
    d_cross: usize,
    early: [[Tap; EARLY]; 2],
    late: [[Tap; LATE]; 2],
    prev: [f64; 5],
}

impl Hood {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            shelf: [Svf::default(); 2],
            cross_lp: [Tpt1::new(ctx.fs, 900.0), Tpt1::new(ctx.fs, 900.0)],
            hp: [Svf::default(); 2],
            dark: [Svf::default(); 2],
            buf: [vec![], vec![]],
            buf_c: [vec![], vec![]],
            mask: 0,
            pos: 0,
            d_cross: 0,
            early: [[Tap::default(); EARLY]; 2],
            late: [[Tap::default(); LATE]; 2],
            prev: [0.0; 5],
        };
        s.reset(ctx);
        s
    }
}

impl Module for Hood {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Hood]
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
        self.shelf = [Svf::new(SvfCoeffs::new(fs, 4200.0, 0.6, Mode::Lowpass)); 2];
        self.cross_lp = [Tpt1::new(fs, 900.0), Tpt1::new(fs, 900.0)];
        self.hp = [Svf::new(SvfCoeffs::new(fs, 230.0, FRAC_1_SQRT_2, Mode::Highpass)); 2];
        self.dark = [Svf::new(SvfCoeffs::new(fs, 3500.0, 0.6, Mode::HighShelf(-11.0))); 2];
        let len = ((0.050 * fs).ceil() as usize + 64).next_power_of_two();
        self.mask = len - 1;
        self.pos = 0;
        for b in self.buf.iter_mut().chain(self.buf_c.iter_mut()) {
            b.clear();
            b.resize(len, 0.0);
        }
        self.d_cross = (0.0003 * fs).round() as usize;
        let mut seed: u64 = 0xCA9_71B0_2026_1125;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        let mut fill = |t: &mut [Tap], t0: f64, t1: f64, decay: f64| {
            let n = t.len() as f64;
            let mut e = 0.0;
            for (i, x) in t.iter_mut().enumerate() {
                let u = (i as f64 + rnd()) / n;
                // gęściej na początku (odstępy rosną)
                x.d = (t0 * (t1 / t0).powf(u) * fs).round() as usize;
                x.g = if rnd() < 0.5 { -1.0 } else { 1.0 } * 10f64.powf(-decay * u / 20.0);
                e += x.g * x.g;
            }
            let k = 1.0 / f64::sqrt(e);
            t.iter_mut().for_each(|x| x.g *= k);
        };
        for ear in 0..2 {
            fill(&mut self.early[ear], 0.0008, 0.012, 9.0);
            fill(&mut self.late[ear], 0.012, 0.048, 10.0);
        }
        self.prev = [0.0; 5];
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let s = &mut *self;
        control_chunks(l, r, |cl, cr| {
            let n = cl.len();
            let a = s.a.advance(n);
            let inv = 1.0 / n as f64;
            let h = super::und(0.5 * a * G_SHELF_DB);
            // odbicia rosną szybciej od początku suwaka – w połowie skali efekt ma być już wyraźny
            let w = 1.0 - (1.0 - a).max(0.0).powf(1.8);
            let tgt = [h * h - 1.0, h - 1.0, G_CROSS * a, G_EARLY * w, G_LATE * w];
            let active = a > 0.0 || s.prev != [0.0; 5];
            let mut g = s.prev;
            let mut st = [0.0; 5];
            for c in 0..5 {
                st[c] = (tgt[c] - g[c]) * inv;
            }
            for i in 0..n {
                s.pos = (s.pos + 1) & s.mask;
                let p = s.pos;
                let x = [cl[i], cr[i]];
                let mut hi = [0.0; 2];
                let mut bp = [0.0; 2];
                let mut cx = [0.0; 2];
                for c in 0..2 {
                    let (lo, h) = s.shelf[c].tick_lp_hp(x[c]);
                    hi[c] = h;
                    bp[c] = x[c] - lo - h;
                    let b = s.hp[c].tick(x[c]);
                    s.buf[c][p] = s.dark[c].tick(b);
                    cx[c] = s.cross_lp[c].lp(b);
                    s.buf_c[c][p] = cx[c];
                }
                if !active {
                    continue;
                }
                for c in 0..5 {
                    g[c] += st[c];
                }
                let mut y = [0.0; 2];
                for c in 0..2 {
                    let o = 1 - c;
                    let mut e = 0.0;
                    for t in s.early[c].iter() {
                        e += t.g * s.buf[c][p.wrapping_sub(t.d) & s.mask];
                    }
                    let mut q = 0.0;
                    for t in s.late[c].iter() {
                        q += t.g * s.buf[c][p.wrapping_sub(t.d) & s.mask];
                    }
                    let cross = s.buf_c[o][p.wrapping_sub(s.d_cross) & s.mask] - cx[c];
                    y[c] = x[c] + g[0] * hi[c] + g[1] * bp[c] + g[2] * cross + g[3] * e + g[4] * q;
                }
                cl[i] = y[0];
                cr[i] = y[1];
            }
            s.prev = tgt;
        });
        for c in 0..2 {
            self.shelf[c].flush_denormals();
            self.hp[c].flush_denormals();
            self.dark[c].flush_denormals();
            self.cross_lp[c].flush_denormals();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::testkit;

    #[test]
    fn standard() {
        testkit::standard(|c| Box::new(Hood::new(c)), ModuleId::Hood, 0.0);
    }

    fn przetworz(l: &[f64], r: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let ctx = Ctx::new(48_000.0);
        let mut m = Hood::new(ctx);
        m.set_amount(ModuleId::Hood, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (l.to_vec(), r.to_vec());
        testkit::run(&mut m, &mut l, &mut r);
        (l, r)
    }
    fn pasmo(x: &[f64], lo: f64, hi: f64) -> Vec<f64> {
        let mut a = Svf::new(SvfCoeffs::new(48_000.0, lo, FRAC_1_SQRT_2, Mode::Highpass));
        let mut b = Svf::new(SvfCoeffs::new(48_000.0, hi, FRAC_1_SQRT_2, Mode::Lowpass));
        x.iter().map(|v| b.tick(a.tick(*v))).collect()
    }
    fn kor(a: &[f64], b: &[f64]) -> f64 {
        let (mut ab, mut aa, mut bb) = (0.0, 0.0, 0.0);
        for i in 9600..a.len() {
            ab += a[i] * b[i];
            aa += a[i] * a[i];
            bb += b[i] * b[i];
        }
        ab / (aa * bb + 1e-30).sqrt()
    }

    /// Nagranie mono: uszy przestają słyszeć to samo (spójność wyraźnie spada), basu to nie dotyczy.
    #[test]
    fn mono_becomes_spacious_bass_stays() {
        let x = testkit::noise(48_000 * 5, 0.2, 1);
        let (l, r) = przetworz(&x, &x);
        let sr = kor(&pasmo(&l, 500.0, 3000.0), &pasmo(&r, 500.0, 3000.0));
        assert!(sr < 0.75, "spójność środka {sr}");
        let bas = kor(&pasmo(&l, 30.0, 80.0), &pasmo(&r, 30.0, 80.0));
        assert!(bas > 0.95, "spójność basu {bas}");
    }

    /// Góra przyciemniona, poziom całości bez dużej zmiany.
    #[test]
    fn softer_top_similar_level() {
        let x = testkit::noise(48_000 * 4, 0.2, 5);
        let y = testkit::noise(48_000 * 4, 0.2, 6);
        let (l, _) = przetworz(&x, &y);
        let e = |v: &[f64]| 10.0 * v.iter().map(|q| q * q).sum::<f64>().log10();
        let top = e(&pasmo(&l, 8000.0, 16_000.0)) - e(&pasmo(&x, 8000.0, 16_000.0));
        assert!(top < -3.0 && top > -7.0, "góra {top}");
        let mid = e(&pasmo(&l, 300.0, 3000.0)) - e(&pasmo(&x, 300.0, 3000.0));
        assert!(mid.abs() < 3.5, "środek {mid}");
    }
}
