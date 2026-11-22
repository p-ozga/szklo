//! Elementy kompresorów (Giannoulis, Massberg, Reiss 2012, „Digital Dynamic Range Compressor Design”).

use super::util::tau_coeff;

/// Statyczna krzywa: wejście [dB] → redukcja [dB] (≥ 0), próg T, ratio R, kolano W (kwadratowe).
#[derive(Clone, Copy, Debug)]
pub struct GainComputer {
    pub threshold_db: f64,
    pub ratio: f64,
    pub knee_db: f64,
}

impl GainComputer {
    #[inline]
    pub fn reduction_db(&self, x_db: f64) -> f64 {
        let (t, r, w) = (self.threshold_db, self.ratio.max(1.0), self.knee_db.max(0.0));
        let slope = 1.0 - 1.0 / r;
        let d = x_db - t;
        if 2.0 * d < -w {
            0.0
        } else if w > 0.0 && 2.0 * d.abs() <= w {
            slope * (d + w / 2.0).powi(2) / (2.0 * w)
        } else {
            slope * d
        }
    }
}

/// Ekspander w dół (bramka): poniżej progu redukcja = (T − x)(R − 1), z kolanem; ograniczona do `range_db`.
#[derive(Clone, Copy, Debug)]
pub struct Expander {
    pub threshold_db: f64,
    pub ratio: f64,
    pub knee_db: f64,
    pub range_db: f64,
}

impl Expander {
    #[inline]
    pub fn reduction_db(&self, x_db: f64) -> f64 {
        let (t, r, w) = (self.threshold_db, self.ratio.max(1.0), self.knee_db.max(0.0));
        let d = t - x_db; // >0 poniżej progu
        let red = if 2.0 * d < -w {
            0.0
        } else if w > 0.0 && 2.0 * d.abs() <= w {
            (r - 1.0) * (d + w / 2.0).powi(2) / (2.0 * w)
        } else {
            (r - 1.0) * d
        };
        red.min(self.range_db)
    }
}

/// Wygładzanie redukcji w dB: „smooth decoupled branching” – atak, gdy redukcja rośnie, zwolnienie, gdy maleje.
#[derive(Clone, Copy, Debug)]
pub struct Ballistics {
    a_att: f64,
    a_rel: f64,
    pub state: f64,
}

impl Ballistics {
    pub fn new(attack_s: f64, release_s: f64, fs: f64) -> Self {
        Self { a_att: tau_coeff(attack_s, fs), a_rel: tau_coeff(release_s, fs), state: 0.0 }
    }
    pub fn set_times(&mut self, attack_s: f64, release_s: f64, fs: f64) {
        self.a_att = tau_coeff(attack_s, fs);
        self.a_rel = tau_coeff(release_s, fs);
    }
    #[inline]
    pub fn tick(&mut self, target: f64) -> f64 {
        let a = if target > self.state { self.a_att } else { self.a_rel };
        self.state = target + a * (self.state - target);
        self.state
    }
    pub fn reset(&mut self) {
        self.state = 0.0;
    }
}

/// Detektor RMS (średnia kwadratu, jednobiegunowa) – zwraca dB.
#[derive(Clone, Copy, Debug)]
pub struct RmsDetector {
    a: f64,
    ms: f64,
}

impl RmsDetector {
    pub fn new(tau_s: f64, fs: f64) -> Self {
        Self { a: tau_coeff(tau_s, fs), ms: 0.0 }
    }
    #[inline]
    pub fn tick_db(&mut self, x2: f64) -> f64 {
        self.ms = x2 + self.a * (self.ms - x2);
        10.0 * self.ms.max(1e-14).log10()
    }
    pub fn reset(&mut self) {
        self.ms = 0.0;
    }
    pub fn value_db(&self) -> f64 {
        10.0 * self.ms.max(1e-14).log10()
    }
}

/// Detektor obwiedni szczytowej (natychmiastowy atak, zwolnienie wykładnicze) – liniowo.
#[derive(Clone, Copy, Debug)]
pub struct PeakDetector {
    a_rel: f64,
    a_att: f64,
    env: f64,
}

impl PeakDetector {
    pub fn new(attack_s: f64, release_s: f64, fs: f64) -> Self {
        Self { a_att: tau_coeff(attack_s, fs), a_rel: tau_coeff(release_s, fs), env: 0.0 }
    }
    #[inline]
    pub fn tick(&mut self, x: f64) -> f64 {
        let x = x.abs();
        let a = if x > self.env { self.a_att } else { self.a_rel };
        self.env = x + a * (self.env - x);
        self.env
    }
    pub fn reset(&mut self) {
        self.env = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn knee_is_continuous() {
        let g = GainComputer { threshold_db: -20.0, ratio: 4.0, knee_db: 6.0 };
        let mut prev = g.reduction_db(-40.0);
        for i in 0..400 {
            let x = -40.0 + i as f64 * 0.1;
            let y = g.reduction_db(x);
            assert!(y >= prev - 1e-12 && (y - prev) < 0.1);
            prev = y;
        }
        assert!((g.reduction_db(0.0) - 15.0).abs() < 1e-9);
    }
}
