//! Miękki clip z kolanem parabolicznym (spec v1 §3.11). Próg θ, półszerokość kolana w ∈ (0,1).
//! x1 = θ(1−w), x2 = θ(1+w):  f(u) = u (u ≤ x1);  u − (u−x1)²/(4θw) (x1 < u < x2);  θ (u ≥ x2). f, f′ ciągłe.
//! Pierwotna F (do ewentualnego ADAA – w v1 nieużywane w torze).

#[inline]
pub fn clip_knee(x: f64, theta: f64, w: f64) -> f64 {
    let u = x.abs();
    let x1 = theta * (1.0 - w);
    let y = if u <= x1 {
        u
    } else {
        let x2 = theta * (1.0 + w);
        if u >= x2 {
            theta
        } else {
            u - (u - x1) * (u - x1) / (4.0 * theta * w)
        }
    };
    y.copysign(x)
}

/// Pierwotna F(|x|) (parzysta).
pub fn clip_knee_antiderivative(x: f64, theta: f64, w: f64) -> f64 {
    let u = x.abs();
    let x1 = theta * (1.0 - w);
    let x2 = theta * (1.0 + w);
    if u <= x1 {
        u * u / 2.0
    } else if u < x2 {
        u * u / 2.0 - (u - x1).powi(3) / (12.0 * theta * w)
    } else {
        x2 * x2 / 2.0 - (2.0 / 3.0) * theta * theta * w * w + theta * (u - x2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn continuity() {
        let (t, w) = (0.8, 0.25);
        let x1 = t * (1.0 - w);
        let x2 = t * (1.0 + w);
        for &p in &[x1, x2] {
            let e = 1e-9;
            assert!((clip_knee(p - e, t, w) - clip_knee(p + e, t, w)).abs() < 1e-8);
            assert!((clip_knee_antiderivative(p - e, t, w) - clip_knee_antiderivative(p + e, t, w)).abs() < 1e-8);
        }
        assert_eq!(clip_knee(5.0, t, w), t);
        assert_eq!(clip_knee(-5.0, t, w), -t);
        assert_eq!(clip_knee(0.3, t, w), 0.3);
    }
}
