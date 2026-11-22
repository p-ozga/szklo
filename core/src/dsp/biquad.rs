//! Biquad TDF-II (filtry o stałych współczynnikach) + projekty RBJ (Audio EQ Cookbook).

use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, Default)]
pub struct Biquad {
    b0: f64,
    b1: f64,
    b2: f64,
    a1: f64,
    a2: f64,
    s1: f64,
    s2: f64,
}

impl Biquad {
    /// Współczynniki znormalizowane (a0 = 1): y = b0 x + b1 x[-1] + b2 x[-2] − a1 y[-1] − a2 y[-2].
    pub const fn from_coeffs(b0: f64, b1: f64, b2: f64, a1: f64, a2: f64) -> Self {
        Self { b0, b1, b2, a1, a2, s1: 0.0, s2: 0.0 }
    }
    pub fn identity() -> Self {
        Self::from_coeffs(1.0, 0.0, 0.0, 0.0, 0.0)
    }
    pub fn coeffs(&self) -> ([f64; 3], [f64; 2]) {
        ([self.b0, self.b1, self.b2], [self.a1, self.a2])
    }
    /// Zmiana współczynników bez zerowania stanu.
    pub fn set(&mut self, other: &Biquad) {
        self.b0 = other.b0;
        self.b1 = other.b1;
        self.b2 = other.b2;
        self.a1 = other.a1;
        self.a2 = other.a2;
    }
    #[inline(always)]
    pub fn tick(&mut self, x: f64) -> f64 {
        let y = self.b0 * x + self.s1;
        self.s1 = self.b1 * x - self.a1 * y + self.s2;
        self.s2 = self.b2 * x - self.a2 * y;
        y
    }
    pub fn reset(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
    pub fn flush_denormals(&mut self) {
        if self.s1.abs() < 1e-30 {
            self.s1 = 0.0;
        }
        if self.s2.abs() < 1e-30 {
            self.s2 = 0.0;
        }
    }

    fn norm(b0: f64, b1: f64, b2: f64, a0: f64, a1: f64, a2: f64) -> Self {
        Self::from_coeffs(b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0)
    }

    pub fn lowpass(fs: f64, f0: f64, q: f64) -> Self {
        let w = 2.0 * PI * f0.min(0.49 * fs) / fs;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        Self::norm((1.0 - c) / 2.0, 1.0 - c, (1.0 - c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    pub fn highpass(fs: f64, f0: f64, q: f64) -> Self {
        let w = 2.0 * PI * f0.min(0.49 * fs) / fs;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        Self::norm((1.0 + c) / 2.0, -(1.0 + c), (1.0 + c) / 2.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    /// Pasmowo-przepustowy, wzmocnienie 0 dB w szczycie.
    pub fn bandpass(fs: f64, f0: f64, q: f64) -> Self {
        let w = 2.0 * PI * f0.min(0.49 * fs) / fs;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        Self::norm(al, 0.0, -al, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    pub fn notch(fs: f64, f0: f64, q: f64) -> Self {
        let w = 2.0 * PI * f0.min(0.49 * fs) / fs;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        Self::norm(1.0, -2.0 * c, 1.0, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    pub fn allpass(fs: f64, f0: f64, q: f64) -> Self {
        let w = 2.0 * PI * f0.min(0.49 * fs) / fs;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        Self::norm(1.0 - al, -2.0 * c, 1.0 + al, 1.0 + al, -2.0 * c, 1.0 - al)
    }
    pub fn peaking(fs: f64, f0: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f0.min(0.49 * fs) / fs;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        Self::norm(1.0 + al * a, -2.0 * c, 1.0 - al * a, 1.0 + al / a, -2.0 * c, 1.0 - al / a)
    }
    pub fn low_shelf(fs: f64, f0: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f0.min(0.49 * fs) / fs;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        let sa = 2.0 * a.sqrt() * al;
        Self::norm(
            a * ((a + 1.0) - (a - 1.0) * c + sa),
            2.0 * a * ((a - 1.0) - (a + 1.0) * c),
            a * ((a + 1.0) - (a - 1.0) * c - sa),
            (a + 1.0) + (a - 1.0) * c + sa,
            -2.0 * ((a - 1.0) + (a + 1.0) * c),
            (a + 1.0) + (a - 1.0) * c - sa,
        )
    }
    pub fn high_shelf(fs: f64, f0: f64, q: f64, gain_db: f64) -> Self {
        let a = 10f64.powf(gain_db / 40.0);
        let w = 2.0 * PI * f0.min(0.49 * fs) / fs;
        let (s, c) = w.sin_cos();
        let al = s / (2.0 * q);
        let sa = 2.0 * a.sqrt() * al;
        Self::norm(
            a * ((a + 1.0) + (a - 1.0) * c + sa),
            -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
            a * ((a + 1.0) + (a - 1.0) * c - sa),
            (a + 1.0) - (a - 1.0) * c + sa,
            2.0 * ((a - 1.0) - (a + 1.0) * c),
            (a + 1.0) - (a - 1.0) * c - sa,
        )
    }

    /// Moduł odpowiedzi |H(e^jw)| w dB dla częstotliwości f (do testów i wykresów).
    pub fn magnitude_db(&self, fs: f64, f: f64) -> f64 {
        let w = 2.0 * PI * f / fs;
        let (c1, s1) = (w.cos(), -w.sin());
        let (c2, s2) = ((2.0 * w).cos(), -(2.0 * w).sin());
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = self.b1 * s1 + self.b2 * s2;
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = self.a1 * s1 + self.a2 * s2;
        10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
    }
}

/// Filtry „dopasowane” 2. rzędu (spec v1 §3.4): bieguny przez niezmienność odpowiedzi impulsowej,
/// licznik z dopasowania |H|² prototypu analogowego w DC, f_m i Nyquiście. Zakres: f0 ≤ fs/4.
pub mod matched {
    use super::Biquad;
    use std::f64::consts::PI;

    fn build(fs: f64, fp: f64, qp: f64, fm: f64, m: impl Fn(f64) -> f64) -> Biquad {
        let wp = 2.0 * PI * fp / fs;
        let q = 1.0 / (2.0 * qp);
        let a1 = if q <= 1.0 {
            -2.0 * (-q * wp).exp() * (wp * (1.0 - q * q).sqrt()).cos()
        } else {
            -2.0 * (-q * wp).exp() * (wp * (q * q - 1.0).sqrt()).cosh()
        };
        let a2 = (-2.0 * q * wp).exp();
        let a0_ = (1.0 + a1 + a2).powi(2);
        let a1_ = (1.0 - a1 + a2).powi(2);
        let a2_ = -4.0 * a2;
        let wm = 2.0 * PI * fm / fs;
        let phi1 = (wm / 2.0).sin().powi(2);
        let phi0 = 1.0 - phi1;
        let phi2 = 4.0 * phi0 * phi1;
        let b0_ = a0_ * m(0.0);
        let b1_ = a1_ * m(fs / 2.0);
        let b2_ = (m(fm) * (a0_ * phi0 + a1_ * phi1 + a2_ * phi2) - b0_ * phi0 - b1_ * phi1) / phi2;
        let w = (b0_.sqrt() + b1_.sqrt()) / 2.0;
        let b0 = (w + (w * w + b2_).max(0.0).sqrt()) / 2.0;
        let b1 = (b0_.sqrt() - b1_.sqrt()) / 2.0;
        let b2 = w - b0;
        Biquad::from_coeffs(b0, b1, b2, a1, a2)
    }

    pub fn peaking(fs: f64, f0: f64, q: f64, gain_db: f64) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let m = move |f: f64| {
            let o = f / f0;
            let x = 1.0 - o * o;
            (x * x + (o * a / q).powi(2)) / (x * x + (o / (a * q)).powi(2))
        };
        build(fs, f0, a * q, f0, m)
    }

    /// |H_a(jΩ)|² dla H_a = A·(c2·s² + c1·s + c0)/(d2·s² + d1·s + d0).
    fn mag2(a: f64, c: [f64; 3], d: [f64; 3], o: f64) -> f64 {
        let nr = c[0] - c[2] * o * o;
        let ni = c[1] * o;
        let dr = d[0] - d[2] * o * o;
        let di = d[1] * o;
        a * a * (nr * nr + ni * ni) / (dr * dr + di * di)
    }

    pub fn high_shelf(fs: f64, f0: f64, q: f64, gain_db: f64) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let sa = a.sqrt();
        // H_a = A·(A s² + (√A/Q) s + 1)/(s² + (√A/Q) s + A)
        let m = move |f: f64| mag2(a, [1.0, sa / q, a], [a, sa / q, 1.0], f / f0);
        build(fs, f0 * sa, q, f0, m)
    }

    pub fn low_shelf(fs: f64, f0: f64, q: f64, gain_db: f64) -> Biquad {
        let a = 10f64.powf(gain_db / 40.0);
        let sa = a.sqrt();
        // H_a = A·(s² + (√A/Q) s + A)/(A s² + (√A/Q) s + 1)
        let m = move |f: f64| mag2(a, [a, sa / q, 1.0], [1.0, sa / q, a], f / f0);
        build(fs, f0 / sa, q, f0, m)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matched_high_shelf_control_values() {
        let b = matched::high_shelf(44_100.0, 10_000.0, std::f64::consts::FRAC_1_SQRT_2, 2.0);
        let (bb, aa) = b.coeffs();
        let want_b = [1.12554823, -0.51124161, 0.17198491];
        let want_a = [-0.33203296, 0.11832450];
        for i in 0..3 {
            assert!((bb[i] - want_b[i]).abs() < 1e-5, "b{i} = {}", bb[i]);
        }
        for i in 0..2 {
            assert!((aa[i] - want_a[i]).abs() < 1e-5, "a{} = {}", i + 1, aa[i]);
        }
    }
    #[test]
    fn matched_peak_close_to_prototype() {
        let fs = 44_100.0;
        let b = matched::peaking(fs, 6500.0, 1.2, -1.5);
        let g = b.magnitude_db(fs, 6500.0);
        assert!((g + 1.5).abs() < 0.1, "{g}");
    }
}
