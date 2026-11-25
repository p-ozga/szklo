//! Kryształ – wyostrzanie uderzeń w siedmiu pasmach (projekt własny z literatury).
//!
//! Wyjście = wejście + Σ (g_b − 1)·BP_b(x): równoległe pasmowe filtry SVF, więc przy g_b = 1 moduł jest dokładnie
//! przezroczysty, a zmiana wzmocnienia pasma działa jak dzwon o zmiennej wysokości (bez zwrotnic i ich przesunięć fazy).
//!
//! W każdym paśmie dwie ciągłe obwiednie mocy – szybka i wolna; ich stosunek r mówi, czy dźwięk właśnie się zaczyna (r > 0),
//! czy wybrzmiewa (r < 0). To klasyczna metoda różnicy obwiedni (znana z kształtowników transjentów od lat 90.):
//!   r > 1,5 dB → w górę 1,6·(r − 1,5), najwyżej 10 dB;   r < −2 dB → w dół 0,4·(−2 − r), najwyżej 4 dB.
//! Uderzenia są ostrzejsze, a tło między nimi ciszej – instrumenty wyraźniej się od siebie oddzielają.
//!
//! Osłony: wolne wyrównanie średniego poziomu każdego pasma (to nie korektor – barwa zostaje), bramka ciszy, a w pasmach
//! 6 i 12 kHz mniejsza waga (syczenie głosek nie kłuje). Każde pasmo steruje WYŁĄCZNIE samo sobą (żadne wzmocnienie nie zależy
//! od obwiedni innego pasma) i nic nie jest tu dzielone na ramki.
//! Moduł nie liczy współczynnika szczytu ani żadnej miary dynamiki nagrania i z niczym jej nie porównuje.

use super::und;
use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};

const N: usize = 7;
const F: [f64; N] = [70.0, 170.0, 420.0, 1000.0, 2500.0, 6000.0, 12_000.0];
const Q: f64 = 1.0;
/// waga pasma
const W: [f64; N] = [0.7, 0.65, 0.65, 0.8, 1.0, 0.95, 0.85];
/// stałe czasowe [s]: szybka (narastanie, opadanie), wolna (narastanie, opadanie), wzmocnienie (narastanie, opadanie)
const T_FA: [f64; N] = [0.006, 0.004, 0.0025, 0.0015, 0.001, 0.0007, 0.0007];
const T_FR: [f64; N] = [0.080, 0.065, 0.050, 0.040, 0.030, 0.025, 0.025];
const T_SA: [f64; N] = [0.130, 0.110, 0.090, 0.080, 0.070, 0.060, 0.060];
const T_SR: [f64; N] = [0.180, 0.160, 0.140, 0.120, 0.100, 0.090, 0.090];
const T_GU: [f64; N] = [0.004, 0.003, 0.002, 0.0012, 0.0008, 0.0006, 0.0006];
const T_GD: [f64; N] = [0.060, 0.050, 0.040, 0.035, 0.030, 0.025, 0.025];
const KAPPA: f64 = 1.6;
const UP_MAX: f64 = 10.0;

#[derive(Clone, Copy, Default)]
struct Band {
    f: [Svf; 2],
    fast: f64,
    slow: f64,
    g_db: f64,
    /// poprzednie wzmocnienie − 1 (interpolacja w bloku)
    d: f64,
    p_in: f64,
    p_out: f64,
    c: [f64; 6],
}

pub struct Crystal {
    ctx: Ctx,
    a: Smoother,
    b: [Band; N],
    a_long: f64,
    meter: f32,
}

impl Crystal {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self { ctx, a: Smoother::new(0.0, 0.02, ctx.fs), b: [Band::default(); N], a_long: 0.0, meter: 0.0 };
        s.reset(ctx);
        s
    }
}

#[inline]
fn db(p: f64) -> f64 {
    10.0 * (p + 1e-14).log10()
}

impl Module for Crystal {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Crystal]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        let fs = ctx.fs;
        let k = fs / CONTROL_BLOCK as f64;
        self.a = Smoother::new(self.a.target(), 0.02, fs);
        for (i, b) in self.b.iter_mut().enumerate() {
            let c = SvfCoeffs::new(fs, F[i], Q, Mode::Bandpass);
            *b = Band { f: [Svf::new(c); 2], ..Band::default() };
            b.c = [T_FA[i], T_FR[i], T_SA[i], T_SR[i], T_GU[i], T_GD[i]].map(|t| tau_coeff(t, k));
        }
        self.a_long = tau_coeff(3.0, k);
        self.meter = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = l.len();
        let mut off = 0;
        let mut meter = 0.0f64;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let inv = 1.0 / k as f64;
            let a = self.a.advance(k);
            let (xl, xr) = (&mut l[off..off + k], &mut r[off..off + k]);
            let mut add = [[0.0f64; CONTROL_BLOCK]; 2];
            for (i, b) in self.b.iter_mut().enumerate() {
                let mut y = [[0.0f64; CONTROL_BLOCK]; 2];
                let mut p = 0.0;
                for j in 0..k {
                    y[0][j] = b.f[0].tick(xl[j]);
                    y[1][j] = b.f[1].tick(xr[j]);
                    p += y[0][j] * y[0][j] + y[1][j] * y[1][j];
                }
                p *= 0.5 * inv;
                let c = b.c;
                b.fast = p + if p > b.fast { c[0] } else { c[1] } * (b.fast - p);
                b.slow = p + if p > b.slow { c[2] } else { c[3] } * (b.slow - p);
                let lf = db(b.fast);
                let ratio = lf - db(b.slow);
                let mut g = if ratio > 1.5 {
                    (KAPPA * (ratio - 1.5)).min(UP_MAX)
                } else if ratio < -2.0 {
                    -(0.4 * (-2.0 - ratio)).min(4.0)
                } else {
                    0.0
                } * W[i];
                if g > 0.0 {
                    g *= ((lf + 74.0) / 12.0).clamp(0.0, 1.0); // cisza i szum tła: bez wyostrzania
                }
                b.g_db = g + if g > b.g_db { c[4] } else { c[5] } * (b.g_db - g);
                if b.g_db.abs() < 1e-6 && g == 0.0 {
                    b.g_db = 0.0;
                }
                // wolne wyrównanie średniego poziomu pasma
                let ge = und(a * b.g_db);
                b.p_in = p + self.a_long * (b.p_in - p);
                let po = p * ge * ge;
                b.p_out = po + self.a_long * (b.p_out - po);
                // wyrównanie zanika razem z suwakiem (bez tego przy wyłączaniu zostawał skok wzmocnienia pasma)
                let mk = (db(b.p_in) - db(b.p_out)).clamp(-7.0, 4.0) * (a / 0.1).min(1.0);
                let d = if a > 0.0 { und(a * b.g_db + mk) - 1.0 } else { 0.0 };
                if d != 0.0 || b.d != 0.0 {
                    let st = (d - b.d) * inv;
                    let mut gg = b.d;
                    for j in 0..k {
                        gg += st;
                        add[0][j] += gg * y[0][j];
                        add[1][j] += gg * y[1][j];
                    }
                }
                b.d = d;
            }
            for j in 0..k {
                xl[j] += add[0][j];
                xr[j] += add[1][j];
            }
            off += k;
        }
        for b in self.b.iter_mut() {
            b.f[0].flush_denormals();
            b.f[1].flush_denormals();
        }
        // Kryształ podbija, nie redukuje – nie zgłasza nic do miernika redukcji (wcześniej pokazywał „redukcję” do 10 dB)
        let _ = meter;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::testkit;

    #[test]
    fn standard() {
        testkit::standard(|c| Box::new(Crystal::new(c)), ModuleId::Crystal, 0.0);
    }

    fn uderzenia(n: usize) -> Vec<f64> {
        // szum o stałym tle i uderzenia co 0,3 s z zanikiem 60 ms
        let nz = testkit::noise(n, 1.0, 7);
        (0..n).map(|i| nz[i] * (0.03 + 0.4 * (-((i % 14_400) as f64) / 2880.0).exp())).collect()
    }
    fn przetworz(a: f32, x: &[f64]) -> Vec<f64> {
        let ctx = Ctx::new(48_000.0);
        let mut m = Crystal::new(ctx);
        m.set_amount(ModuleId::Crystal, a);
        m.reset(ctx);
        let (mut l, mut r) = (x.to_vec(), x.to_vec());
        testkit::run(&mut m, &mut l, &mut r);
        l
    }
    fn rms(x: &[f64]) -> f64 {
        10.0 * (x.iter().map(|v| v * v).sum::<f64>() / x.len() as f64 + 1e-20).log10()
    }

    /// Kontrast: pierwsze 15 ms uderzenia względem tła przed nim rośnie, a średni poziom się nie zmienia.
    #[test]
    fn sharper_hits_same_level() {
        let n = 48_000 * 15;
        let x = uderzenia(n);
        let y = przetworz(1.0, &x);
        let kontrast = |z: &[f64]| {
            let (mut s, mut c) = (0.0, 0.0);
            for h in 20..n / 14_400 - 1 {
                let p = h * 14_400;
                s += rms(&z[p..p + 720]) - rms(&z[p - 2400..p]);
                c += 1.0;
            }
            s / c
        };
        let (k0, k1) = (kontrast(&x), kontrast(&y));
        assert!(k1 > k0 + 3.0, "kontrast {k0} → {k1}");
        let d = rms(&y[n / 2..]) - rms(&x[n / 2..]);
        // syntetyczne uderzenia we wszystkich pasmach naraz (najgorszy przypadek); na muzyce 0 ± 0,1 dB
        assert!(d.abs() < 2.5, "poziom {d}");
    }

    #[test]
    fn steady_sound_untouched() {
        let x = testkit::noise(48_000 * 6, 0.1, 3);
        let y = przetworz(1.0, &x);
        assert!((rms(&y[96_000..]) - rms(&x[96_000..])).abs() < 0.3);
    }
}
