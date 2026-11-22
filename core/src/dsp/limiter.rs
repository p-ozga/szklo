//! Limiter z lookahead (spec v1 §3.10) – wspólny dla Pre-limitera i Hard Limit.
//!
//! q[n] = max(p[n], p[n−1]);  g_req = min(1, T/q);  g_min = min(g_req[n−L … n]);
//! r = min(g_min, r + k_rel·(1 − r));  g_s = średnia r z L próbek;  y[n] = g_s[n]·x[n − D − L].
//! Każde r[m], m ∈ [n−L+1, n] ma w oknie minimum indeks n−L, więc g_s[n] ≤ g_req[n−L] (brak przeregulowania).
//! Zwolnienie adaptacyjne: τ_rel = τ0·(1 + 9·min(1, GR_avg/6 dB)), GR_avg – średnia (τ = 1 s) z −dB(g_s).

use super::util::{tau_coeff, MovingAverage, SlidingMin};

#[derive(Clone, Debug)]
pub struct LookaheadGain {
    l: usize,
    fs: f64,
    tau0: f64,
    prev_p: f64,
    min: SlidingMin,
    avg: MovingAverage,
    r: f64,
    gr_avg: f64,
    a_gr: f64,
    k_rel: f64,
    cnt: u32,
    pub last_gain: f64,
}

impl LookaheadGain {
    pub fn new(lookahead: usize, tau0_s: f64, fs: f64) -> Self {
        let l = lookahead.max(1);
        Self {
            l,
            fs,
            tau0: tau0_s,
            prev_p: 0.0,
            min: SlidingMin::new(l + 1),
            avg: MovingAverage::new(l, 1.0),
            r: 1.0,
            gr_avg: 0.0,
            a_gr: tau_coeff(1.0, fs),
            k_rel: 1.0 - (-1.0 / (tau0_s * fs)).exp(),
            cnt: 0,
            last_gain: 1.0,
        }
    }
    pub fn lookahead(&self) -> usize {
        self.l
    }
    pub fn reset(&mut self) {
        self.prev_p = 0.0;
        self.min.reset();
        self.avg.reset(1.0);
        self.r = 1.0;
        self.gr_avg = 0.0;
        self.k_rel = 1.0 - (-1.0 / (self.tau0 * self.fs)).exp();
        self.cnt = 0;
        self.last_gain = 1.0;
    }
    /// Wpycha szczyt p[n] (≥ 0) i cel T; zwraca g_s[n] dla próbki opóźnionej o L.
    #[inline]
    pub fn push(&mut self, p: f64, target: f64) -> f64 {
        let q = p.max(self.prev_p);
        self.prev_p = p;
        let g_req = if q > target { target / q.max(1e-12) } else { 1.0 };
        let g_min = self.min.push(g_req);
        // zwolnienie adaptacyjne (GR_avg ma τ = 1 s, więc k_rel wystarczy przeliczać co 16 próbek;
        // log10 tylko gdy jest redukcja – wcześniej exp + log10 na każdą próbkę)
        let gr_db = if self.last_gain < 1.0 { -20.0 * self.last_gain.max(1e-9).log10() } else { 0.0 };
        self.gr_avg = gr_db + self.a_gr * (self.gr_avg - gr_db);
        if self.gr_avg < 1e-12 {
            self.gr_avg = 0.0; // bez denormali w długiej ciszy
        }
        if self.cnt == 0 {
            let tau = self.tau0 * (1.0 + 9.0 * (self.gr_avg / 6.0).clamp(0.0, 1.0));
            self.k_rel = 1.0 - (-1.0 / (tau * self.fs)).exp();
        }
        self.cnt = (self.cnt + 1) & 15;
        let k_rel = self.k_rel;
        self.r = g_min.min(self.r + k_rel * (1.0 - self.r));
        let g = self.avg.push(self.r).min(1.0);
        self.last_gain = g;
        g
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn no_overshoot_and_transparent_below_target() {
        let fs = 44_100.0;
        let l = 64;
        let mut lim = LookaheadGain::new(l, 0.05, fs);
        let mut seed = 7u64;
        let mut rnd = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) - 1.0
        };
        let x: Vec<f64> = (0..50_000).map(|i| rnd() * if (i / 5000) % 2 == 0 { 3.0 } else { 0.3 }).collect();
        let t = 0.5;
        let mut worst = 0.0f64;
        for n in 0..x.len() {
            let g = lim.push(x[n].abs(), t);
            if n >= l {
                worst = worst.max(x[n - l].abs() * g);
            }
        }
        assert!(worst <= t * (1.0 + 1e-12), "przerost {worst}");
        // poniżej celu: wzmocnienie dokładnie 1
        let mut lim = LookaheadGain::new(l, 0.05, fs);
        for _ in 0..1000 {
            assert_eq!(lim.push(0.1, t), 1.0);
        }
    }
}
