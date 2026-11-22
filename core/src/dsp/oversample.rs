//! Oversampling ×2·×2 FIR półpasmowymi, liniowofazowymi (spec v1 §3.5).
//!
//! Półpasmowy: N = 4K+3, M = (N−1)/2, h[n] = 0,5·sinc((n−M)/2)·kaiser(n), h[M] = 0,5, Σ_{n≠M} h = 0,5.
//! Stopień 1: N = 127, β = 9 (pasmo −0,01 dB do 20,3 kHz @44,1); stopień 2: N = 27, β = 9.
//! Interpolacja 2×: y[2i] = Σ_j 2h[2j]·x[i−j],  y[2i+1] = x[i−K]  (opóźnienie M próbek 2×).
//! Decymacja 2×: z[i] = Σ_n h[n]·v[2i−n] (liczone po wejściu próbki parzystej).
//! Łącznie: 63 (stopień 1) + 6,5 (stopień 2) + 0,5 dopełnienia = **70 próbek 1×** na przebieg góra–dół.

use super::truepeak::{bessel_i0, sinc};

fn halfband(n: usize, beta: f64) -> Vec<f64> {
    assert!(n % 4 == 3);
    let m = (n - 1) / 2;
    let mut h: Vec<f64> = (0..n)
        .map(|i| {
            let t = i as f64 - m as f64;
            let r = t / m as f64;
            0.5 * sinc(t / 2.0) * bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta)
        })
        .collect();
    h[m] = 0.5;
    let s: f64 = h.iter().enumerate().filter(|(i, _)| *i != m).map(|(_, v)| v).sum();
    for (i, v) in h.iter_mut().enumerate() {
        if i != m {
            *v *= 0.5 / s;
        }
    }
    h
}

/// Interpolator 2× (jeden stopień).
#[derive(Clone)]
pub struct Up2 {
    k: usize,
    taps: Vec<f64>, // 2h[2j], j = 0..=2K+1
    hist: Vec<f64>, // bufor podwójny
    pos: usize,
}

impl Up2 {
    pub fn new(n: usize, beta: f64) -> Self {
        let h = halfband(n, beta);
        let k = (n - 3) / 4;
        let taps: Vec<f64> = (0..=(2 * k + 1)).map(|j| 2.0 * h[2 * j]).collect();
        let len = taps.len();
        Self { k, taps, hist: vec![0.0; 2 * len], pos: 0 }
    }
    pub fn reset(&mut self) {
        self.hist.fill(0.0);
        self.pos = 0;
    }
    #[inline]
    pub fn push(&mut self, x: f64) -> [f64; 2] {
        let len = self.taps.len();
        self.hist[self.pos] = x;
        self.hist[self.pos + len] = x;
        self.pos += 1;
        if self.pos == len {
            self.pos = 0;
        }
        // okno: x[i−len+1] … x[i]; najnowsza na końcu
        let w = &self.hist[self.pos..self.pos + len];
        // taps[j] = taps[len−1−j] (filtr liniowofazowy), więc Σ taps[j]·w[len−1−j] = Σ taps[j]·w[j]
        let even = super::util::dot(&self.taps, w);
        let odd = w[len - 1 - self.k];
        [even, odd]
    }
}

/// Decymator 2× (jeden stopień).
#[derive(Clone)]
pub struct Down2 {
    m: usize,
    center: f64,
    taps: Vec<f64>, // h[2j]
    even_hist: Vec<f64>,
    odd_hist: Vec<f64>,
    ep: usize,
    op: usize,
}

impl Down2 {
    pub fn new(n: usize, beta: f64) -> Self {
        let h = halfband(n, beta);
        let m = (n - 1) / 2;
        let taps: Vec<f64> = (0..n).step_by(2).map(|i| h[i]).collect();
        let le = taps.len();
        let lo = (m + 1) / 2 + 1;
        Self { m, center: h[m], taps, even_hist: vec![0.0; 2 * le], odd_hist: vec![0.0; lo], ep: 0, op: 0 }
    }
    pub fn reset(&mut self) {
        self.even_hist.fill(0.0);
        self.odd_hist.fill(0.0);
        self.ep = 0;
        self.op = 0;
    }
    /// v = (v[2i], v[2i+1]); zwraca z[i] = Σ h[n]·v[2i−n].
    #[inline]
    pub fn push(&mut self, v: [f64; 2]) -> f64 {
        let le = self.taps.len();
        self.even_hist[self.ep] = v[0];
        self.even_hist[self.ep + le] = v[0];
        self.ep += 1;
        if self.ep == le {
            self.ep = 0;
        }
        let w = &self.even_hist[self.ep..self.ep + le]; // v[2i−2(le−1)] … v[2i]
        // Σ h[2j]·v[2i−2j]; h[2j] symetryczne względem j ↔ le−1−j, więc kolejność w oknie nie ma znaczenia
        let mut s = super::util::dot(&self.taps, w);
        // środek: v[2i − M] (M nieparzyste) = v[2(i − (M+1)/2) + 1] – próbka nieparzysta sprzed (M+1)/2 par
        let lo = self.odd_hist.len();
        let back = (self.m + 1) / 2;
        let idx = (self.op + lo - back) % lo;
        s += self.center * self.odd_hist[idx];
        self.odd_hist[self.op] = v[1];
        self.op = (self.op + 1) % lo;
        s
    }
}

/// Pełny przebieg ×4: góra → nieliniowość → dół. Latencja 70 próbek 1×.
#[derive(Clone)]
pub struct Oversampler4 {
    u1: Up2,
    u2: Up2,
    d2: Down2,
    d1: Down2,
    pad_in: f64, // 1 próbka 2× opóźnienia przed stopniem 2 (parzystość dla stopnia 1)
}

impl Default for Oversampler4 {
    fn default() -> Self {
        Self::new()
    }
}

impl Oversampler4 {
    pub const LATENCY: usize = 70;

    pub fn new() -> Self {
        Self { u1: Up2::new(127, 9.0), u2: Up2::new(27, 9.0), d2: Down2::new(27, 9.0), d1: Down2::new(127, 9.0), pad_in: 0.0 }
    }
    pub fn reset(&mut self) {
        self.u1.reset();
        self.u2.reset();
        self.d2.reset();
        self.d1.reset();
        self.pad_in = 0.0;
    }
    /// Przetwarza jedną próbkę: `f` działa na 4 kolejnych próbkach nadpróbkowanych.
    #[inline]
    pub fn process<F: FnMut(f64) -> f64>(&mut self, x: f64, mut f: F) -> f64 {
        let a = self.u1.push(x);
        let mut mid = [0.0; 2];
        for (t, &s2) in a.iter().enumerate() {
            let s = std::mem::replace(&mut self.pad_in, s2);
            let b = self.u2.push(s);
            let c = [f(b[0]), f(b[1])];
            mid[t] = self.d2.push(c);
        }
        self.d1.push(mid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn impulse_peak_at_latency() {
        let mut os = Oversampler4::new();
        let y: Vec<f64> = (0..300).map(|i| os.process(if i == 10 { 1.0 } else { 0.0 }, |u| u)).collect();
        let imax = y.iter().enumerate().max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap()).unwrap().0;
        assert_eq!(imax, 10 + Oversampler4::LATENCY, "{:?}", &y[70..90]);
    }
    #[test]
    fn roundtrip_is_pure_delay_70() {
        let fs = 44_100.0;
        let mut os = Oversampler4::new();
        let n = 6000;
        for &f in &[100.0, 1000.0, 10000.0, 19000.0] {
            os.reset();
            let x: Vec<f64> = (0..n).map(|i| (2.0 * std::f64::consts::PI * f * i as f64 / fs + 0.2).sin()).collect();
            let y: Vec<f64> = x.iter().map(|&v| os.process(v, |u| u)).collect();
            let mut err = 0.0f64;
            for i in 1000..n {
                err = err.max((y[i] - x[i - Oversampler4::LATENCY]).abs());
            }
            assert!(err < 2e-4, "f={f}: maks. błąd {err}");
        }
    }
}
