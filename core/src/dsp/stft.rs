//! Strumieniowa STFT stereo (analiza → modyfikacja widma → synteza overlap-add).
//!
//! Okno Hann (okresowe) przy analizie i syntezie (spec v1 §3.9), skok N/4 → Σw² = 1,5, więc wynik × 1/(1,5·N).
//! Dwa kanały rzeczywiste liczone jednym zespolonym FFT (z = l + j·r) – połowa kosztu.
//! Latencja = N próbek. Callback dostaje półwidma 0..=N/2 obu kanałów (do modyfikacji w miejscu).

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

pub type C64 = Complex<f64>;

pub struct StftStereo {
    n: usize,
    hop: usize,
    win: Vec<f64>,
    fwd: Arc<dyn Fft<f64>>,
    inv: Arc<dyn Fft<f64>>,
    in_l: Vec<f64>,
    in_r: Vec<f64>,
    acc_l: Vec<f64>,
    acc_r: Vec<f64>,
    out_l: Vec<f64>,
    out_r: Vec<f64>,
    fill: usize,
    out_pos: usize,
    z: Vec<C64>,
    scratch: Vec<C64>,
    pub spec_l: Vec<C64>,
    pub spec_r: Vec<C64>,
}

impl StftStereo {
    pub fn new(n: usize) -> Self {
        assert!(n >= 16 && n.is_power_of_two());
        let hop = n / 4;
        let win: Vec<f64> = (0..n)
            .map(|i| 0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos())
            .collect();
        let mut planner = FftPlanner::<f64>::new();
        let fwd = planner.plan_fft_forward(n);
        let inv = planner.plan_fft_inverse(n);
        let sl = fwd.get_inplace_scratch_len().max(inv.get_inplace_scratch_len());
        Self {
            n,
            hop,
            win,
            fwd,
            inv,
            in_l: vec![0.0; n],
            in_r: vec![0.0; n],
            acc_l: vec![0.0; n],
            acc_r: vec![0.0; n],
            out_l: vec![0.0; hop],
            out_r: vec![0.0; hop],
            fill: 0,
            out_pos: 0,
            z: vec![C64::new(0.0, 0.0); n],
            scratch: vec![C64::new(0.0, 0.0); sl],
            spec_l: vec![C64::new(0.0, 0.0); n / 2 + 1],
            spec_r: vec![C64::new(0.0, 0.0); n / 2 + 1],
        }
    }
    pub fn size(&self) -> usize {
        self.n
    }
    pub fn hop(&self) -> usize {
        self.hop
    }
    pub fn latency(&self) -> usize {
        self.n
    }
    pub fn bins(&self) -> usize {
        self.n / 2 + 1
    }
    pub fn reset(&mut self) {
        for v in [&mut self.in_l, &mut self.in_r, &mut self.acc_l, &mut self.acc_r, &mut self.out_l, &mut self.out_r] {
            v.fill(0.0);
        }
        self.fill = 0;
        self.out_pos = 0;
    }

    /// Przetwarza blok w miejscu. `f(spec_l, spec_r)` wywoływane raz na ramkę (co `hop` próbek).
    pub fn process<F: FnMut(&mut [C64], &mut [C64])>(&mut self, l: &mut [f64], r: &mut [f64], mut f: F) {
        let (n, hop) = (self.n, self.hop);
        for i in 0..l.len() {
            // wejście do końcówki bufora analizy
            self.in_l[n - hop + self.fill] = l[i];
            self.in_r[n - hop + self.fill] = r[i];
            l[i] = self.out_l[self.out_pos];
            r[i] = self.out_r[self.out_pos];
            self.fill += 1;
            self.out_pos += 1;
            if self.fill == hop {
                self.frame(&mut f);
                self.fill = 0;
                self.out_pos = 0;
            }
        }
    }

    fn frame<F: FnMut(&mut [C64], &mut [C64])>(&mut self, f: &mut F) {
        let (n, hop) = (self.n, self.hop);
        for k in 0..n {
            self.z[k] = C64::new(self.in_l[k] * self.win[k], self.in_r[k] * self.win[k]);
        }
        self.fwd.process_with_scratch(&mut self.z, &mut self.scratch);
        // rozdział dwóch rzeczywistych widm
        let half = n / 2;
        for k in 0..=half {
            let a = self.z[k];
            let b = self.z[(n - k) % n].conj();
            self.spec_l[k] = (a + b) * 0.5;
            let d = (a - b) * 0.5; // = j·R
            self.spec_r[k] = C64::new(d.im, -d.re);
        }
        f(&mut self.spec_l, &mut self.spec_r);
        // złożenie z = L + jR (z zachowaniem symetrii hermitowskiej)
        let j = C64::new(0.0, 1.0);
        for k in 0..=half {
            self.z[k] = self.spec_l[k] + j * self.spec_r[k];
        }
        for k in half + 1..n {
            self.z[k] = self.spec_l[n - k].conj() + j * self.spec_r[n - k].conj();
        }
        self.inv.process_with_scratch(&mut self.z, &mut self.scratch);
        let g = 1.0 / (1.5 * n as f64); // 1/N (rustfft nie normalizuje) × 1/Σw²
        for k in 0..n {
            let w = self.win[k] * g;
            self.acc_l[k] += self.z[k].re * w;
            self.acc_r[k] += self.z[k].im * w;
        }
        // gotowe próbki wyjściowe = początek akumulatora
        self.out_l.copy_from_slice(&self.acc_l[..hop]);
        self.out_r.copy_from_slice(&self.acc_r[..hop]);
        self.acc_l.copy_within(hop.., 0);
        self.acc_r.copy_within(hop.., 0);
        self.acc_l[n - hop..].fill(0.0);
        self.acc_r[n - hop..].fill(0.0);
        self.in_l.copy_within(hop.., 0);
        self.in_r.copy_within(hop.., 0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_reconstruction_with_latency() {
        let n = 256;
        let mut s = StftStereo::new(n);
        let len = 4000;
        let xl: Vec<f64> = (0..len).map(|i| ((i * 37 % 101) as f64 / 50.0 - 1.0) * 0.5).collect();
        let xr: Vec<f64> = (0..len).map(|i| ((i * 13 % 97) as f64 / 48.0 - 1.0) * 0.3).collect();
        let (mut l, mut r) = (xl.clone(), xr.clone());
        for (cl, cr) in l.chunks_mut(100).zip(r.chunks_mut(100)) {
            s.process(cl, cr, |_, _| {});
        }
        let lat = s.latency();
        for i in lat + n..len {
            assert!((l[i] - xl[i - lat]).abs() < 1e-9, "L i={i}: {} vs {}", l[i], xl[i - lat]);
            assert!((r[i] - xr[i - lat]).abs() < 1e-9, "R i={i}");
        }
    }
}
