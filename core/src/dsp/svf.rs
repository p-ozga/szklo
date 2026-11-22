//! Filtr stanowy Simpera/Cytomic (trapezoidalny, „zero-delay feedback”).
//! Stabilny przy szybkiej modulacji parametrów – do filtrów zmiennych w czasie (EQ dynamiczne, auto-EQ).
//!
//! g = tan(π f0 / fs), k = 1/Q
//! a1 = 1/(1 + g(g + k)), a2 = g·a1, a3 = g·a2
//! v3 = x − ic2;  v1 = a1·ic1 + a2·v3;  v2 = ic2 + a2·ic1 + a3·v3
//! ic1 = 2v1 − ic1;  ic2 = 2v2 − ic2
//! LP = v2, BP = v1, HP = x − k·v1 − v2
//! wyjście ogólne: y = m0·x + m1·v1 + m2·v2  (tabela w `Mode`).

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    Lowpass,
    Highpass,
    Bandpass,
    Allpass,
    /// Pik (dzwon), wzmocnienie w dB.
    Bell(f64),
    LowShelf(f64),
    HighShelf(f64),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SvfCoeffs {
    k: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    m0: f64,
    m1: f64,
    m2: f64,
}

impl SvfCoeffs {
    pub fn new(fs: f64, f0: f64, q: f64, mode: Mode) -> Self {
        let f0 = f0.clamp(1.0, 0.497 * fs);
        let q = q.max(0.05);
        let mut g = (PI * f0 / fs).tan();
        let mut k = 1.0 / q;
        let (m0, m1, m2);
        match mode {
            Mode::Lowpass => {
                m0 = 0.0;
                m1 = 0.0;
                m2 = 1.0;
            }
            Mode::Highpass => {
                m0 = 1.0;
                m1 = -k;
                m2 = -1.0;
            }
            Mode::Bandpass => {
                // pasmowo-przepustowy znormalizowany: wzmocnienie 0 dB w szczycie (v1 ma w szczycie Q)
                m0 = 0.0;
                m1 = k;
                m2 = 0.0;
            }
            Mode::Allpass => {
                m0 = 1.0;
                m1 = -2.0 * k;
                m2 = 0.0;
            }
            Mode::Bell(db) => {
                let a = 10f64.powf(db / 40.0);
                k = 1.0 / (q * a);
                m0 = 1.0;
                m1 = k * (a * a - 1.0);
                m2 = 0.0;
            }
            Mode::LowShelf(db) => {
                let a = 10f64.powf(db / 40.0);
                g /= a.sqrt();
                m0 = 1.0;
                m1 = k * (a - 1.0);
                m2 = a * a - 1.0;
            }
            Mode::HighShelf(db) => {
                let a = 10f64.powf(db / 40.0);
                g *= a.sqrt();
                m0 = a * a;
                m1 = k * (1.0 - a) * a;
                m2 = 1.0 - a * a;
            }
        }
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        Self { k, a1, a2, a3, m0, m1, m2 }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Svf {
    pub c: SvfCoeffs,
    ic1: f64,
    ic2: f64,
}

impl Svf {
    pub fn new(c: SvfCoeffs) -> Self {
        Self { c, ic1: 0.0, ic2: 0.0 }
    }
    pub fn reset(&mut self) {
        self.ic1 = 0.0;
        self.ic2 = 0.0;
    }
    #[inline(always)]
    pub fn tick(&mut self, x: f64) -> f64 {
        let c = &self.c;
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        c.m0 * x + c.m1 * v1 + c.m2 * v2
    }
    /// Jednocześnie LP i HP (zwrotnice): LP = v2, HP = x − k·v1 − v2.
    #[inline(always)]
    pub fn tick_lp_hp(&mut self, x: f64) -> (f64, f64) {
        let c = &self.c;
        let v3 = x - self.ic2;
        let v1 = c.a1 * self.ic1 + c.a2 * v3;
        let v2 = self.ic2 + c.a2 * self.ic1 + c.a3 * v3;
        self.ic1 = 2.0 * v1 - self.ic1;
        self.ic2 = 2.0 * v2 - self.ic2;
        (v2, x - c.k * v1 - v2)
    }
    pub fn flush_denormals(&mut self) {
        if self.ic1.abs() < 1e-30 {
            self.ic1 = 0.0;
        }
        if self.ic2.abs() < 1e-30 {
            self.ic2 = 0.0;
        }
    }
}

/// Filtr 1. rzędu TPT (spec v1 §3.1): DC-blok, all-pass rotatora.
/// G = g/(1+g), v = (x − s)·G, lp = v + s, s = lp + v; hp = x − lp; ap = 2·lp − x.
#[derive(Clone, Copy, Debug, Default)]
pub struct Tpt1 {
    gg: f64,
    s: f64,
}

impl Tpt1 {
    pub fn new(fs: f64, fc: f64) -> Self {
        let mut t = Self::default();
        t.set(fs, fc);
        t
    }
    pub fn set(&mut self, fs: f64, fc: f64) {
        let g = (PI * fc.clamp(0.1, 0.45 * fs) / fs).tan();
        self.gg = g / (1.0 + g);
    }
    pub fn reset(&mut self) {
        self.s = 0.0;
    }
    #[inline(always)]
    pub fn lp(&mut self, x: f64) -> f64 {
        let v = (x - self.s) * self.gg;
        let lp = v + self.s;
        self.s = lp + v;
        lp
    }
    #[inline(always)]
    pub fn hp(&mut self, x: f64) -> f64 {
        x - self.lp(x)
    }
    #[inline(always)]
    pub fn ap(&mut self, x: f64) -> f64 {
        2.0 * self.lp(x) - x
    }
    pub fn flush_denormals(&mut self) {
        if self.s.abs() < 1e-30 {
            self.s = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn gain_at(fs: f64, f: f64, c: SvfCoeffs) -> f64 {
        let mut s = Svf::new(c);
        let n = (fs * 0.5) as usize;
        let mut acc = 0.0;
        let mut acc_in = 0.0;
        for i in 0..n {
            let x = (2.0 * PI * f * i as f64 / fs).sin();
            let y = s.tick(x);
            if i > n / 2 {
                acc += y * y;
                acc_in += x * x;
            }
        }
        10.0 * (acc / acc_in).log10()
    }
    #[test]
    fn bell_gain_at_center() {
        let fs = 48_000.0;
        let g = gain_at(fs, 1000.0, SvfCoeffs::new(fs, 1000.0, 1.0, Mode::Bell(6.0)));
        assert!((g - 6.0).abs() < 0.1, "{g}");
    }
    #[test]
    fn shelves() {
        let fs = 48_000.0;
        let g = gain_at(fs, 30.0, SvfCoeffs::new(fs, 200.0, 0.707, Mode::LowShelf(4.0)));
        assert!((g - 4.0).abs() < 0.15, "{g}");
        let g = gain_at(fs, 15000.0, SvfCoeffs::new(fs, 3000.0, 0.707, Mode::HighShelf(-3.0)));
        assert!((g + 3.0).abs() < 0.15, "{g}");
        let g = gain_at(fs, 1000.0, SvfCoeffs::new(fs, 300.0, 0.707, Mode::Allpass));
        assert!(g.abs() < 0.05, "{g}");
    }
}
