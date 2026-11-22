//! Zwrotnice Linkwitz–Riley 4. rzędu na SVF, drzewo zbalansowane z kompensacją fazy (spec v1 §3.3).
//!
//! Jedna zwrotnica = 3 SVF Butterwortha (k = √2): SVF_a daje LP i HP, SVF_b(LP) → LP4, SVF_c(HP) → HP4.
//! LP4 + HP4 = A_f (all-pass 2. rzędu, k = √2). Dla 5 pasm 4 all-passy, dla 4 – 2, dla 3 – 1.

use super::svf::{Mode, Svf, SvfCoeffs};
use std::f64::consts::SQRT_2;

#[derive(Clone, Copy, Debug)]
struct Lr4 {
    a: Svf,
    b: Svf,
    c: Svf,
}

impl Lr4 {
    fn new(fs: f64, f: f64) -> Self {
        let q = 1.0 / SQRT_2;
        // SVF_a: potrzebne jednocześnie LP i HP – liczymy ręcznie z v1/v2 (tryb LP, a HP z tożsamości)
        Self {
            a: Svf::new(SvfCoeffs::new(fs, f, q, Mode::Lowpass)),
            b: Svf::new(SvfCoeffs::new(fs, f, q, Mode::Lowpass)),
            c: Svf::new(SvfCoeffs::new(fs, f, q, Mode::Highpass)),
        }
    }
    #[inline]
    fn split(&mut self, x: f64) -> (f64, f64) {
        let (lp1, hp1) = self.a.tick_lp_hp(x);
        (self.b.tick(lp1), self.c.tick(hp1))
    }
    fn flush(&mut self) {
        self.a.flush_denormals();
        self.b.flush_denormals();
        self.c.flush_denormals();
    }
}

#[derive(Clone, Copy, Debug)]
struct Ap {
    s: Svf,
}
impl Ap {
    fn new(fs: f64, f: f64) -> Self {
        Self { s: Svf::new(SvfCoeffs::new(fs, f, 1.0 / SQRT_2, Mode::Allpass)) }
    }
    #[inline]
    fn tick(&mut self, x: f64) -> f64 {
        self.s.tick(x)
    }
}

#[derive(Clone, Debug)]
enum Tree {
    Two(Lr4),
    Three { x1: Lr4, x2: Lr4, a2: Ap },
    Four { x1: Lr4, x2: Lr4, x3: Lr4, a1: Ap, a3: Ap },
    Five { x1: Lr4, x2: Lr4, x3: Lr4, x4: Lr4, a1: Ap, a3: Ap, a4lo: Ap, a4mid: Ap },
}

/// Zwrotnica jednokanałowa na `freqs.len() + 1` pasm (2…5).
#[derive(Clone, Debug)]
pub struct Crossover {
    tree: Tree,
    freqs: Vec<f64>,
    fs: f64,
}

impl Crossover {
    pub fn new(fs: f64, freqs: &[f64]) -> Self {
        let f = freqs;
        let tree = match f.len() {
            1 => Tree::Two(Lr4::new(fs, f[0])),
            2 => Tree::Three { x1: Lr4::new(fs, f[0]), x2: Lr4::new(fs, f[1]), a2: Ap::new(fs, f[1]) },
            3 => Tree::Four { x1: Lr4::new(fs, f[0]), x2: Lr4::new(fs, f[1]), x3: Lr4::new(fs, f[2]), a1: Ap::new(fs, f[0]), a3: Ap::new(fs, f[2]) },
            4 => Tree::Five {
                x1: Lr4::new(fs, f[0]),
                x2: Lr4::new(fs, f[1]),
                x3: Lr4::new(fs, f[2]),
                x4: Lr4::new(fs, f[3]),
                a1: Ap::new(fs, f[0]),
                a3: Ap::new(fs, f[2]),
                a4lo: Ap::new(fs, f[3]),
                a4mid: Ap::new(fs, f[3]),
            },
            n => panic!("zwrotnica: nieobsługiwana liczba częstotliwości {n}"),
        };
        Self { tree, freqs: freqs.to_vec(), fs }
    }
    pub fn bands(&self) -> usize {
        self.freqs.len() + 1
    }
    pub fn reset(&mut self) {
        *self = Self::new(self.fs, &self.freqs.clone());
    }
    pub fn flush_denormals(&mut self) {
        match &mut self.tree {
            Tree::Two(x) => x.flush(),
            Tree::Three { x1, x2, a2 } => {
                x1.flush();
                x2.flush();
                a2.s.flush_denormals();
            }
            Tree::Four { x1, x2, x3, a1, a3 } => {
                for x in [x1, x2, x3] {
                    x.flush();
                }
                a1.s.flush_denormals();
                a3.s.flush_denormals();
            }
            Tree::Five { x1, x2, x3, x4, a1, a3, a4lo, a4mid } => {
                for x in [x1, x2, x3, x4] {
                    x.flush();
                }
                for a in [a1, a3, a4lo, a4mid] {
                    a.s.flush_denormals();
                }
            }
        }
    }
    /// Rozdziela próbkę na pasma (out.len() ≥ bands()).
    #[inline]
    pub fn split(&mut self, x: f64, out: &mut [f64]) {
        match &mut self.tree {
            Tree::Two(c) => {
                let (l, h) = c.split(x);
                out[0] = l;
                out[1] = h;
            }
            Tree::Three { x1, x2, a2 } => {
                let (l, h) = x1.split(x);
                out[0] = a2.tick(l);
                let (m, hh) = x2.split(h);
                out[1] = m;
                out[2] = hh;
            }
            Tree::Four { x1, x2, x3, a1, a3 } => {
                let (lo, hi) = x2.split(x);
                let lo = a3.tick(lo);
                let (p1, p2) = x1.split(lo);
                let hi = a1.tick(hi);
                let (p3, p4) = x3.split(hi);
                out[0] = p1;
                out[1] = p2;
                out[2] = p3;
                out[3] = p4;
            }
            Tree::Five { x1, x2, x3, x4, a1, a3, a4lo, a4mid } => {
                let (lo, hi) = x2.split(x);
                let lo = a4lo.tick(a3.tick(lo));
                let (p1, p2) = x1.split(lo);
                let hi = a1.tick(hi);
                let (mid, top) = x3.split(hi);
                let p3 = a4mid.tick(mid);
                let (p4, p5) = x4.split(top);
                out[0] = p1;
                out[1] = p2;
                out[2] = p3;
                out[3] = p4;
                out[4] = p5;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(freqs: &[f64]) {
        let fs = 44_100.0;
        let mut xo = Crossover::new(fs, freqs);
        let mut aps: Vec<Ap> = freqs.iter().map(|&f| Ap::new(fs, f)).collect();
        let mut b = [0.0; 5];
        let mut err = 0.0f64;
        for n in 0..20000 {
            let x = if n == 0 { 1.0 } else if n < 3000 { ((n * 7919) % 101) as f64 / 101.0 - 0.5 } else { 0.0 };
            xo.split(x, &mut b);
            let s: f64 = b[..xo.bands()].iter().sum();
            let mut r = x;
            for a in aps.iter_mut() {
                r = a.tick(r);
            }
            err = err.max((s - r).abs());
        }
        assert!(err < 1e-9, "{} pasm: suma ≠ all-pass, błąd {err}", freqs.len() + 1);
    }
    #[test]
    fn bands_sum_to_allpass_3_4_5() {
        check(&[150.0]);
        check(&[200.0, 3000.0]);
        check(&[150.0, 1000.0, 5000.0]);
        check(&[100.0, 400.0, 1600.0, 6400.0]);
    }
}
