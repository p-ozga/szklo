//! Głośność wg ITU-R BS.1770: ważenie K dla dowolnego fs + miernik krótkoterminowy (okno 3 s).

use super::biquad::Biquad;

/// Dwa stopnie ważenia K (półka wysoka + HPF RLB). Współczynniki liczone z parametrów analogowych
/// (wzory jak w libebur128 – dla 48 kHz dają dokładnie współczynniki z BS.1770).
#[derive(Clone, Debug)]
pub struct KWeighting {
    shelf: Biquad,
    hpf: Biquad,
}

impl KWeighting {
    pub fn new(fs: f64) -> Self {
        // stopień 1: półka wysoka
        let f0 = 1681.974450955533;
        let g = 3.999843853973347;
        let q = 0.7071752369554196;
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let vh = 10f64.powf(g / 20.0);
        let vb = vh.powf(0.4996667741545416);
        let a0 = 1.0 + k / q + k * k;
        let shelf = Biquad::from_coeffs(
            (vh + vb * k / q + k * k) / a0,
            2.0 * (k * k - vh) / a0,
            (vh - vb * k / q + k * k) / a0,
            2.0 * (k * k - 1.0) / a0,
            (1.0 - k / q + k * k) / a0,
        );
        // stopień 2: HPF RLB
        let f0 = 38.13547087602444;
        let q = 0.5003270373238773;
        let k = (std::f64::consts::PI * f0 / fs).tan();
        let a0 = 1.0 + k / q + k * k;
        let hpf = Biquad::from_coeffs(1.0, -2.0, 1.0, 2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0);
        Self { shelf, hpf }
    }
    #[inline]
    pub fn tick(&mut self, x: f64) -> f64 {
        self.hpf.tick(self.shelf.tick(x))
    }
    pub fn reset(&mut self) {
        self.shelf.reset();
        self.hpf.reset();
    }
}

/// Miernik krótkoterminowy: średnia moc ważona K z ostatnich 3 s (30 kubełków po 100 ms).
pub struct ShortTermLoudness {
    kl: KWeighting,
    kr: KWeighting,
    bucket_len: usize,
    acc: f64,
    cnt: usize,
    buckets: [f64; 30],
    pos: usize,
    filled: usize,
}

impl ShortTermLoudness {
    pub fn new(fs: f64) -> Self {
        Self {
            kl: KWeighting::new(fs),
            kr: KWeighting::new(fs),
            bucket_len: (0.1 * fs).round() as usize,
            acc: 0.0,
            cnt: 0,
            buckets: [0.0; 30],
            pos: 0,
            filled: 0,
        }
    }
    pub fn process(&mut self, l: &[f64], r: &[f64]) {
        for (&a, &b) in l.iter().zip(r) {
            let (ya, yb) = (self.kl.tick(a), self.kr.tick(b));
            self.acc += ya * ya + yb * yb;
            self.cnt += 1;
            if self.cnt == self.bucket_len {
                self.buckets[self.pos] = self.acc / self.cnt as f64;
                self.pos = (self.pos + 1) % 30;
                self.filled = (self.filled + 1).min(30);
                self.acc = 0.0;
                self.cnt = 0;
            }
        }
    }
    /// LUFS z ostatnich 3 s (−120 przy ciszy).
    pub fn lufs(&self) -> f64 {
        if self.filled == 0 {
            return -120.0;
        }
        let ms: f64 = self.buckets.iter().take(self.filled.max(1)).sum::<f64>() / self.filled as f64;
        if ms <= 1e-13 {
            -120.0
        } else {
            -0.691 + 10.0 * ms.log10()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn k_weighting_48k_matches_bs1770() {
        let k = KWeighting::new(48_000.0);
        // BS.1770-4, tabela 1 i 2
        let (b, a) = k.shelf.coeffs();
        assert!((b[0] - 1.53512485958697).abs() < 1e-9);
        assert!((b[1] + 2.69169618940638).abs() < 1e-9);
        assert!((b[2] - 1.19839281085285).abs() < 1e-9);
        assert!((a[0] + 1.69065929318241).abs() < 1e-9);
        assert!((a[1] - 0.73248077421585).abs() < 1e-9);
        let (_, a) = k.hpf.coeffs();
        assert!((a[0] + 1.99004745483398).abs() < 1e-9);
        assert!((a[1] - 0.99007225036621).abs() < 1e-9);
    }
    #[test]
    fn sine_1k_minus_20_gives_about_minus_23_lufs_per_channel_pair() {
        // 1 kHz sinus −20 dBFS w obu kanałach: −20 − 3,01 + 3,01 (2 kanały) ≈ −20 + ważenie(1 kHz ≈ +0,7) − 0,691
        let fs = 48_000.0;
        let mut m = ShortTermLoudness::new(fs);
        let a = 10f64.powf(-20.0 / 20.0);
        let x: Vec<f64> = (0..(fs as usize * 4)).map(|n| a * (2.0 * std::f64::consts::PI * 1000.0 * n as f64 / fs).sin()).collect();
        m.process(&x, &x);
        let l = m.lufs();
        assert!((l - (-20.0)).abs() < 0.3, "{l}");
    }
}
