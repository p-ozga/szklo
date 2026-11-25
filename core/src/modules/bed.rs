//! Szum tła – cichy, stały szum pod muzyką (projekt własny z literatury). Domyślnie wyłączony.
//!
//! Skąd pomysł: u osób z ADHD i z dużą nieuwagą stały szum poprawia wyniki zadań (metaanaliza Nigg i in. 2024: g ≈ 0,25),
//! a u pozostałych lekko je pogarsza (g ≈ −0,21) – dlatego to opcja, nie ustawienie dla wszystkich. Działa raczej samo stałe
//! pobudzenie niż losowość (Rijmen i in. 2024: ton 100 Hz pomagał jak szum różowy), a barwa szumu nie jest krytyczna.
//! Badań szumu zmieszanego z muzyką nie ma – poziom jest więc do ustawienia suwakiem. Szum dodatkowo wypełnia pauzy
//! i przykrywa słowa (niższa zrozumiałość tekstu piosenek).
//!
//! Tor: wyjście = wejście + g·szum. Dwa niezależne generatory (lewy/prawy – szum jest szeroki, nie „w głowie”),
//! szum różowy (filtr Kelleta) bez dudnienia poniżej 80 Hz i złagodzony powyżej 7 kHz.
//! Poziom idzie za średnim poziomem muzyki (narasta 2 s, opada 6 s): −27 dB + 19 dB·a względem muzyki (a = 1: −8 dB).
//! Gdy muzyka milknie na dłużej niż 3 s, szum sam wygasa w kilka sekund – filtr nie szumi w ciszy. Nic nie jest modulowane.

use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs, Tpt1};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};
use std::f64::consts::FRAC_1_SQRT_2;

/// skuteczna wartość szumu po filtrach przy białym wejściu ±1 (zmierzona w `reset`)
struct Gen {
    seed: u64,
    b: [f64; 3],
    hp: Tpt1,
    lp: Svf,
}

impl Gen {
    fn new(fs: f64, seed: u64) -> Self {
        Self { seed, b: [0.0; 3], hp: Tpt1::new(fs, 80.0), lp: Svf::new(SvfCoeffs::new(fs, 7000.0, FRAC_1_SQRT_2, Mode::Lowpass)) }
    }
    #[inline]
    fn tick(&mut self) -> f64 {
        self.seed ^= self.seed << 13;
        self.seed ^= self.seed >> 7;
        self.seed ^= self.seed << 17;
        let w = (self.seed >> 11) as f64 / (1u64 << 52) as f64 - 1.0;
        // Paul Kellet, wersja oszczędna: −3 dB/oktawę z dokładnością ok. ±0,5 dB
        self.b[0] = 0.99765 * self.b[0] + w * 0.0990460;
        self.b[1] = 0.96300 * self.b[1] + w * 0.2965164;
        self.b[2] = 0.57000 * self.b[2] + w * 1.0526913;
        let pink = self.b[0] + self.b[1] + self.b[2] + w * 0.1848;
        self.lp.tick(self.hp.hp(pink))
    }
}

pub struct Bed {
    ctx: Ctx,
    a: Smoother,
    gen: [Gen; 2],
    /// 1 / RMS generatora (policzone dla `norm_fs`)
    norm: f64,
    norm_fs: f64,
    ms: f64,
    c_up: f64,
    c_dn: f64,
    c_fade: f64,
    quiet_s: f64,
    g: f64,
}

impl Bed {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            gen: [Gen::new(ctx.fs, 1), Gen::new(ctx.fs, 2)],
            norm: 1.0,
            norm_fs: 0.0,
            ms: 0.0,
            c_up: 0.0,
            c_dn: 0.0,
            c_fade: 0.0,
            quiet_s: 0.0,
            g: 0.0,
        };
        s.reset(ctx);
        s
    }
}

impl Module for Bed {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Bed]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn warmup_samples(&self) -> usize {
        // poziom muzyki musi być znany, zanim szum wejdzie
        (0.5 * self.ctx.fs) as usize
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        let k = ctx.fs / CONTROL_BLOCK as f64;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        // pomiar RMS generatora (stałe ziarno → zawsze ta sama wartość)
        // (raz na częstotliwość próbkowania: reset bywa wołany w wątku dźwięku przy włączaniu modułu)
        if self.norm_fs != ctx.fs {
            let mut g = Gen::new(ctx.fs, 0x5EED_0000_0000_0001);
            for _ in 0..4000 {
                g.tick();
            }
            let n = 40_000;
            let e: f64 = (0..n).map(|_| g.tick().powi(2)).sum::<f64>() / n as f64;
            self.norm = 1.0 / e.sqrt();
            self.norm_fs = ctx.fs;
        }
        self.gen = [Gen::new(ctx.fs, 0x9E37_79B9_7F4A_7C15), Gen::new(ctx.fs, 0xD1B5_4A32_D192_ED03)];
        self.ms = 0.0;
        self.c_up = tau_coeff(2.0, k);
        self.c_dn = tau_coeff(6.0, k);
        self.c_fade = tau_coeff(1.5, k);
        self.quiet_s = 0.0;
        self.g = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = l.len();
        let mut off = 0;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let a = self.a.advance(k);
            let mut p = 0.0;
            for i in off..off + k {
                p += l[i] * l[i] + r[i] * r[i];
            }
            p /= 2.0 * k as f64;
            if p < 1e-7 {
                // cisza (< −70 dBFS): przez 3 s trzymamy poziom, potem szum wygasa
                self.quiet_s += k as f64 / self.ctx.fs;
                if self.quiet_s > 3.0 {
                    self.ms *= self.c_fade;
                }
            } else {
                self.quiet_s = 0.0;
                // pierwsze dźwięki po ciszy: szybki start pomiaru
                let c = if self.ms < 1e-9 { 0.0 } else if p > self.ms { self.c_up } else { self.c_dn };
                self.ms = p + c * (self.ms - p);
            }
            let rel = 10f64.powf((-27.0 + 19.0 * a) / 20.0) * (a / 0.03).min(1.0);
            let target = self.ms.sqrt() * rel * self.norm;
            if target > 0.0 || self.g > 0.0 {
                let st = (target - self.g) / k as f64;
                let mut g = self.g;
                for i in off..off + k {
                    g += st;
                    l[i] += g * self.gen[0].tick();
                    r[i] += g * self.gen[1].tick();
                }
            }
            self.g = if target < 1e-12 { 0.0 } else { target };
            if self.ms < 1e-12 {
                self.ms = 0.0;
            }
            off += k;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::testkit;
    use std::f64::consts::PI;

    #[test]
    fn standard() {
        testkit::standard(|c| Box::new(Bed::new(c)), ModuleId::Bed, 0.0);
    }

    fn run(a: f32, x: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let ctx = Ctx::new(48_000.0);
        let mut m = Bed::new(ctx);
        m.set_amount(ModuleId::Bed, a);
        m.reset(ctx);
        let (mut l, mut r) = (x.to_vec(), x.to_vec());
        testkit::run(&mut m, &mut l, &mut r);
        (l, r)
    }
    fn rms_db(v: &[f64]) -> f64 {
        10.0 * (v.iter().map(|q| q * q).sum::<f64>() / v.len() as f64 + 1e-30).log10()
    }

    /// Szum 8 dB pod muzyką przy pełnym suwaku, 17,5 dB w połowie; lewy i prawy niezależne.
    #[test]
    fn level_follows_music() {
        let fs = 48_000.0;
        let n = 48_000 * 12;
        let x: Vec<f64> = (0..n).map(|i| 0.2 * (2.0 * PI * 440.0 * i as f64 / fs).sin()).collect();
        for (a, want) in [(1.0f32, -8.0), (0.5, -17.5)] {
            let (l, r) = run(a, &x);
            let nl: Vec<f64> = (n / 2..n).map(|i| l[i] - x[i]).collect();
            let nr: Vec<f64> = (n / 2..n).map(|i| r[i] - x[i]).collect();
            let d = rms_db(&nl) - rms_db(&x[n / 2..]);
            assert!((d - want).abs() < 1.5, "a={a}: {d} dB");
            let kor = nl.iter().zip(&nr).map(|(p, q)| p * q).sum::<f64>() / (nl.len() as f64 * 10f64.powf(rms_db(&nl) / 10.0));
            assert!(kor.abs() < 0.1, "korelacja kanałów szumu {kor}");
        }
    }

    /// Po 3 s ciszy szum wygasa; w ciszy od początku nie pojawia się wcale.
    #[test]
    fn fades_out_in_silence() {
        let fs = 48_000.0;
        let n = 48_000 * 30;
        let x: Vec<f64> = (0..n).map(|i| if i < 48_000 * 5 { 0.2 * (2.0 * PI * 440.0 * i as f64 / fs).sin() } else { 0.0 }).collect();
        let (l, _) = run(1.0, &x);
        assert!(rms_db(&l[48_000 * 6..48_000 * 7]) > -40.0, "szum ma jeszcze trwać w krótkiej pauzie");
        assert!(rms_db(&l[n - 48_000..]) < -75.0, "po długiej ciszy: {}", rms_db(&l[n - 48_000..]));
        let (l, _) = run(1.0, &vec![0.0; 48_000 * 3]);
        assert!(l.iter().all(|v| *v == 0.0));
    }
}
