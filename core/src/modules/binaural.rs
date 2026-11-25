//! Binauralny – synteza binauralna na słuchawki: lewy i prawy kanał grają jak z dwóch głośników stojących przed słuchaczem
//! (±45°), a otoczenie (różnica kanałów) także z dwóch za nim (±110°).
//!
//! Filtry ucha: pomiary HRTF manekina KEMAR – Bill Gardner i Keith Martin, MIT Media Lab, © 1994 MIT Media Laboratory
//! (dane wolno używać bez ograniczeń z podaniem autorów). Tablice: `hrtf_kemar.rs`, generator `tools/hrtf/`.
//!
//! Praca w dziedzinie M/S (Blumlein 1933; „shuffler” – Cooper i Bauck 1989):
//!  * przód: suma kanałów (środek: wokal, bas, werbel) przechodzi BEZ filtra ucha – zachowuje dokładnie swoją barwę;
//!    tylko różnica kanałów dostaje filtr Hi − Hc (ucho bliższe minus dalsze, para ±45°, wyrównany do neutralnej barwy),
//!    czyli różnice czasu i głośności między uszami. Przy a = 0 samo opóźnienie; bas (< ok. 200 Hz) bez zmian;
//!  * tył: różnica kanałów (S, Hafler 1970) i odrobina środka, rozproszone rzadkimi ciągami impulsów (inny ciąg dla każdego
//!    głośnika, 9…30 ms), > 250 Hz. Bez pogłosu rekurencyjnego, bez symulacji ścian, bez wyboru „modelu słuchacza”,
//!    bez zmiany różnic głośności uszu w basie – źródła są w polu dalekim (pomiar z 1,4 m).

use super::hrtf_kemar::{DELAY_MS, FIR, RATES};
use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{dot, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK, MAX_BLOCK};
use std::f64::consts::FRAC_1_SQRT_2;

const TAPS: usize = 14;
const T0_MS: f64 = 9.0;
const T1_MS: f64 = 30.0;
/// głośność tyłu przy a = 1: z różnicy kanałów i ze środka
const G_SIDE: f64 = 0.65;
const G_MID: f64 = 0.28;

/// Splot FIR z historią (współczynniki odwrócone w czasie).
struct Conv {
    rev: Vec<f64>,
    hist: Vec<f64>,
}
impl Conv {
    fn new(h: &[f32]) -> Self {
        let rev: Vec<f64> = h.iter().rev().map(|&v| v as f64).collect();
        let hist = vec![0.0; rev.len() - 1 + MAX_BLOCK];
        Self { rev, hist }
    }
    /// y[i] = Σ h[k]·x[i−k]
    fn run(&mut self, x: &[f64], y: &mut [f64]) {
        let m = self.rev.len() - 1;
        let n = x.len();
        self.hist[m..m + n].copy_from_slice(x);
        for i in 0..n {
            y[i] = dot(&self.rev, &self.hist[i..i + m + 1]);
        }
        self.hist.copy_within(n..n + m, 0);
        for v in self.hist[..m].iter_mut() {
            if v.abs() < 1e-30 {
                *v = 0.0;
            }
        }
    }
}

/// Bufor kołowy z rzadkimi odczepami.
struct Ring {
    buf: Vec<f64>,
    mask: usize,
    pos: usize,
}
impl Ring {
    fn new(len: usize) -> Self {
        let n = len.next_power_of_two();
        Self { buf: vec![0.0; n], mask: n - 1, pos: 0 }
    }
    #[inline]
    fn push(&mut self, x: f64) {
        self.pos = (self.pos + 1) & self.mask;
        self.buf[self.pos] = x;
    }
    #[inline]
    fn tap(&self, t: &[(usize, f64)]) -> f64 {
        t.iter().map(|&(d, g)| g * self.buf[self.pos.wrapping_sub(d) & self.mask]).sum()
    }
}

pub struct Binaural {
    ctx: Ctx,
    a: Smoother,
    d: usize,
    dly: [Ring; 2],
    conv: [Conv; 3],
    amb_hp: [Svf; 2],
    amb: [Ring; 2],
    taps: [Vec<(usize, f64)>; 4],
    prev_a: f64,
    /// [nieużywane, różnica z przodu, tył M, tył S]
    y: [Vec<f64>; 4],
    x: [Vec<f64>; 4],
}

fn table(fs: f64) -> usize {
    (0..RATES.len()).min_by(|&i, &j| (RATES[i] / fs).ln().abs().total_cmp(&(RATES[j] / fs).ln().abs())).unwrap_or(0)
}

impl Binaural {
    pub fn new(ctx: Ctx) -> Self {
        let t = table(ctx.fs);
        let buf = || vec![0.0; MAX_BLOCK];
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            d: 0,
            dly: [Ring::new(1), Ring::new(1)],
            conv: [1, 2, 3].map(|i| Conv::new(FIR[t][i])),
            amb_hp: [Svf::default(); 2],
            amb: [Ring::new(1), Ring::new(1)],
            taps: [vec![], vec![], vec![], vec![]],
            prev_a: 0.0,
            y: [buf(), buf(), buf(), buf()],
            x: [buf(), buf(), buf(), buf()],
        };
        s.reset(ctx);
        s
    }
}

impl Module for Binaural {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Binaural]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        (DELAY_MS * 1e-3 * RATES[table(self.ctx.fs)]).round() as usize
    }
    fn reset(&mut self, ctx: Ctx) {
        let fs = ctx.fs;
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, fs);
        let t = table(fs);
        self.d = self.latency_samples();
        self.dly = [Ring::new(self.d + 1), Ring::new(self.d + 1)];
        self.conv = [1, 2, 3].map(|i| Conv::new(FIR[t][i]));
        self.amb_hp = [Svf::new(SvfCoeffs::new(fs, 250.0, FRAC_1_SQRT_2, Mode::Highpass)); 2];
        let len = (T1_MS * 1e-3 * fs).ceil() as usize + 4;
        self.amb = [Ring::new(len), Ring::new(len)];
        // rzadkie impulsy: po jednym w równych przedziałach, losowe położenie i znak (stałe ziarno), zanik 10 dB, energia 1
        let mut seed: u64 = 0xB1A0_5EED_2026_1125;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for v in self.taps.iter_mut() {
            v.clear();
            let mut e = 0.0;
            for i in 0..TAPS {
                let u = (i as f64 + rnd()) / TAPS as f64;
                let g = if rnd() < 0.5 { -1.0 } else { 1.0 } * 10f64.powf(-10.0 * u / 20.0);
                e += g * g;
                v.push((((T0_MS + (T1_MS - T0_MS) * u) * 1e-3 * fs).round() as usize, g));
            }
            let k = 1.0 / f64::sqrt(e);
            v.iter_mut().for_each(|t| t.1 *= k);
        }
        self.prev_a = self.a.value;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = l.len();
        for i in 0..n {
            let (m, s) = (0.5 * (l[i] + r[i]), 0.5 * (l[i] - r[i]));
            self.x[0][i] = m;
            self.x[1][i] = s;
            let (mh, sh) = (self.amb_hp[0].tick(m), self.amb_hp[1].tick(s));
            self.amb[0].push(mh);
            self.amb[1].push(sh);
            let bl = G_SIDE * self.amb[1].tap(&self.taps[0]) + G_MID * self.amb[0].tap(&self.taps[2]);
            let br = -G_SIDE * self.amb[1].tap(&self.taps[1]) + G_MID * self.amb[0].tap(&self.taps[3]);
            self.x[2][i] = 0.5 * (bl + br);
            self.x[3][i] = 0.5 * (bl - br);
        }
        for c in 0..3 {
            self.conv[c].run(&self.x[c + 1][..n], &mut self.y[c + 1][..n]);
        }
        let mut off = 0;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let a = self.a.advance(k);
            let st = (a - self.prev_a) / k as f64;
            let mut g = self.prev_a;
            for i in off..off + k {
                g += st;
                self.dly[0].push(self.x[0][i]);
                self.dly[1].push(self.x[1][i]);
                let (dm, ds) = (self.dly[0].tap(&[(self.d, 1.0)]), self.dly[1].tap(&[(self.d, 1.0)]));
                let m = dm + g * self.y[2][i];
                let s = ds + g * (self.y[1][i] - ds + self.y[3][i]);
                l[i] = m + s;
                r[i] = m - s;
            }
            self.prev_a = a;
            off += k;
        }
        self.amb_hp[0].flush_denormals();
        self.amb_hp[1].flush_denormals();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::testkit;
    use std::f64::consts::PI;

    #[test]
    fn standard() {
        testkit::standard(|c| Box::new(Binaural::new(c)), ModuleId::Binaural, 1e-12);
    }

    fn przetworz(fs: f64, l: &[f64], r: &[f64]) -> (Vec<f64>, Vec<f64>, usize) {
        let ctx = Ctx::new(fs);
        let mut m = Binaural::new(ctx);
        m.set_amount(ModuleId::Binaural, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = (l.to_vec(), r.to_vec());
        testkit::run(&mut m, &mut l, &mut r);
        (l, r, m.latency_samples())
    }
    fn e(x: &[f64]) -> f64 {
        x.iter().map(|v| v * v).sum::<f64>() + 1e-30
    }

    /// Dźwięk tylko w lewym kanale: prawe ucho słyszy go ciszej i później (różnica czasu i głośności między uszami).
    #[test]
    fn left_source_reaches_both_ears() {
        for fs in [44_100.0, 48_000.0, 96_000.0] {
            let mut x = vec![0.0; 8000];
            x[500] = 1.0;
            let z = vec![0.0; 8000];
            let (l, r, lat) = przetworz(fs, &x, &z);
            // bez tyłu: pierwsze 6 ms
            let w = 500..500 + (0.006 * fs) as usize;
            let ild = 10.0 * (e(&l[w.clone()]) / e(&r[w.clone()])).log10();
            assert!(ild > 3.0 && ild < 25.0, "fs {fs}: różnica głośności {ild}");
            let czas = |v: &[f64]| {
                let (mut a, mut b) = (0.0, 0.0);
                for i in w.clone() {
                    a += (i - 500) as f64 * v[i] * v[i];
                    b += v[i] * v[i];
                }
                a / b / fs * 1e3
            };
            assert!(czas(&r) > czas(&l) + 0.10, "fs {fs}: {} / {} ms", czas(&l), czas(&r));
            assert!((lat as f64 / fs * 1e3 - DELAY_MS).abs() < 0.02);
        }
    }

    /// Mono: bas i całość bez zmiany poziomu.
    #[test]
    fn mono_keeps_level() {
        let fs = 48_000.0;
        let bas: Vec<f64> = (0..96_000).map(|i| 0.3 * (2.0 * PI * 45.0 * i as f64 / fs).sin()).collect();
        let (l, _, _) = przetworz(fs, &bas, &bas);
        let d = 10.0 * (e(&l[48_000..]) / e(&bas[48_000..])).log10();
        assert!(d.abs() < 1.0, "bas: {d} dB");
        let x = testkit::noise(96_000, 0.2, 8);
        let (l, r, _) = przetworz(fs, &x, &x);
        for y in [l, r] {
            let d = 10.0 * (e(&y[48_000..]) / e(&x[48_000..])).log10();
            assert!(d.abs() < 1.5, "szum: {d} dB");
        }
    }
}
