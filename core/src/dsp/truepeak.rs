//! Interpolator true-peak 4× (spec v1 §3.6) – detekcja w limiterze i miernik WY.
//!
//! h[m] = sinc(m/4)·kaiser(m), m = −80…80 (161 współczynników), β = 5,5; faza p ∈ {1,2,3}:
//! y_p[n] = Σ_{k=−19}^{20} c_p[k]·x[n+k], c_p[k] = h[4k − p] (40 współczynników, Σ = 1); faza 0 = x[n].
//! Przyczynowo: wynik dla próbki n−20 (opóźnienie D_tp = 20). p[n] = max |y_0..3| → szczyt w [n, n+1).

pub const TH: usize = 20;
const NC: usize = 2 * TH; // współczynników na fazę

pub(crate) fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    let y = x * x / 4.0;
    for k in 1..80 {
        term *= y / (k as f64 * k as f64);
        sum += term;
        if term < 1e-17 * sum {
            break;
        }
    }
    sum
}

pub(crate) fn sinc(x: f64) -> f64 {
    if x.abs() < 1e-12 {
        1.0
    } else {
        (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x)
    }
}

fn design(th: usize, beta: f64) -> [Vec<f64>; 3] {
    let half = 4 * th as i64;
    let h = |m: i64| -> f64 {
        if m.abs() > half {
            return 0.0;
        }
        let r = m as f64 / half as f64;
        sinc(m as f64 / 4.0) * bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / bessel_i0(beta)
    };
    let mut out: [Vec<f64>; 3] = Default::default();
    for p in 1..=3i64 {
        // k = −(th−1) … th  → indeks w wektorze j = k + th − 1
        let mut c: Vec<f64> = (-(th as i64) + 1..=th as i64).map(|k| h(4 * k - p)).collect();
        let s: f64 = c.iter().sum();
        for v in c.iter_mut() {
            *v /= s;
        }
        out[(p - 1) as usize] = c;
    }
    out
}

/// Interpolator jednego kanału.
#[derive(Clone)]
pub struct TpInterp {
    c: [Vec<f64>; 3],
    hist: Vec<f64>, // bufor podwójny 2·NC
    pos: usize,
}

impl Default for TpInterp {
    fn default() -> Self {
        Self::new()
    }
}

impl TpInterp {
    pub const LATENCY: usize = TH;

    pub fn new() -> Self {
        Self { c: design(TH, 5.5), hist: vec![0.0; 2 * NC], pos: 0 }
    }
    pub fn reset(&mut self) {
        self.hist.fill(0.0);
        self.pos = 0;
    }
    /// Wpycha x[n]; zwraca 4 wartości (y_0..y_3) dla chwili n − 20.
    #[inline]
    pub fn push(&mut self, x: f64) -> [f64; 4] {
        self.hist[self.pos] = x;
        self.hist[self.pos + NC] = x;
        self.pos += 1;
        if self.pos == NC {
            self.pos = 0;
        }
        // okno: x[n−39] … x[n]  (najstarsza … najnowsza); środek c = n − 20 → indeks TH − 1 w oknie
        let w = &self.hist[self.pos..self.pos + NC];
        let mut out = [w[TH - 1], 0.0, 0.0, 0.0];
        for p in 0..3 {
            out[p + 1] = super::util::dot(&self.c[p], w);
        }
        out
    }
    #[inline]
    pub fn push_peak(&mut self, x: f64) -> f64 {
        let o = self.push(x);
        o[0].abs().max(o[1].abs()).max(o[2].abs()).max(o[3].abs())
    }
}

/// Miernik true-peak stereo (maks. z bloku, liniowo).
#[derive(Default)]
pub struct TruePeakMeter {
    l: TpInterp,
    r: TpInterp,
}

impl TruePeakMeter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn process(&mut self, l: &[f64], r: &[f64]) -> f64 {
        let mut m = 0.0f64;
        for (&a, &b) in l.iter().zip(r) {
            m = m.max(self.l.push_peak(a)).max(self.r.push_peak(b));
        }
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intersample_peak_of_fs4_sine_detected() {
        let mut ip = TpInterp::new();
        let mut m = 0.0f64;
        for n in 0..4000 {
            let x = (std::f64::consts::FRAC_PI_2 * n as f64 + std::f64::consts::FRAC_PI_4).sin();
            let p = ip.push_peak(x);
            if n > 100 {
                m = m.max(p);
            }
        }
        let db = 20.0 * m.log10();
        assert!(db.abs() < 0.02, "szczyt {db} dB");
    }
    #[test]
    fn sines_up_to_20k_within_002_db() {
        let fs = 44_100.0;
        for &f in &[1000.0, 5000.0, 10000.0, 15000.0, 19000.0, 20000.0] {
            let mut ip = TpInterp::new();
            let mut m = 0.0f64;
            for n in 0..20000 {
                let x = (2.0 * std::f64::consts::PI * f * n as f64 / fs + 0.3).sin();
                let p = ip.push_peak(x);
                if n > 200 {
                    m = m.max(p);
                }
            }
            let db = 20.0 * m.log10();
            assert!(db.abs() < 0.03, "f={f}: {db} dB");
        }
    }
    #[test]
    fn phase0_is_delayed_input() {
        let mut ip = TpInterp::new();
        let xs: Vec<f64> = (0..100).map(|i| (i as f64 * 0.37).sin()).collect();
        for (n, &x) in xs.iter().enumerate() {
            let o = ip.push(x);
            if n >= TH {
                assert_eq!(o[0], xs[n - TH]);
            }
        }
    }
}
