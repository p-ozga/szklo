//! Ciepły bas – „miękki i ciepły” dół pasma (zastępuje Korektor; projekt clean-room z literatury).
//!
//! Wyjście = wejście + Δ (przy a = 0 Δ ≡ 0 → moduł dokładnie przezroczysty). Pasmo basu b: LP Linkwitz–Riley
//! 4. rzędu 150 Hz (2× Butterworth SVF).
//!
//! 1. **Nasycenie parzystymi harmonicznymi („lampowe”)** na sygnale znormalizowanym GŁADKIM poziomem RMS basu
//!    (τ 60 ms, sprzężone stereo), więc działa jednakowo przy każdej głośności, a poziom normalizacji nie faluje
//!    w rytmie pojedynczych okresów basu:
//!       u = b/(√2·rms + 1e−4),  y(u) = (tanh(d(u + β)) − tanh(dβ)) / (d·sech²(dβ)),  β = 0,30
//!    (nachylenie 1 w zerze; asymetria → 2. harmoniczna, zaokrąglenie szczytów → „miękkość”).
//!    Δ_sat = w·HP25·LP500(√2·rms·y(u) − b) – tylko niskie harmoniczne (ciepło, nie „brzęczenie”), bez DC.
//!    Źródła: modele nasycenia lampowego/taśmowego (asymetryczna nieliniowość → parzyste harmoniczne),
//!    np. Pakarinen & Yeh 2009, Zölzer „DAFX” (nieliniowości).
//! 2. **Zaokrąglenie ataków basu** – bardzo łagodna, wolna kompresja RMS pasma basu (atak 40 ms, zwolnienie 500 ms,
//!    kolano 10 dB, ratio ≤ 1,6), bez pompowania. Δ_comp = b·(g − 1). Wyrównanie = średnia redukcja z ~4 s,
//!    więc średni poziom basu się NIE obniża (v2 ściszała 30–60 Hz o 1,9 dB).
//! 3. **Pełnia i ciepło** – dwa szerokie dzwony: 70 Hz (Q 0,6) i 170 Hz (Q 0,7), zależnie od utworu: gdy bas dominuje
//!    (udział pasma < 150 Hz, średnia z 3 s) – mniej, gdy go brakuje – więcej.
//!    Przy 0 dB dzwon SVF jest dokładnie przezroczysty.
//!
//! Zmiana 2026-11-22 po odsłuchu („tonacja się nie zmienia, bas szarpany”): usunięty dzielnik subharmoniczny
//! (przy realnych liniach basu przełączał się nieregularnie – buczenie niezgodne z melodią) i szybka obwiednia
//! szczytowa (8/120 ms – falowała w rytmie basu i modulowała nasycenie).
//!
//! Zmiana 2026-11-24 („działa, jakby nie mógł”): pomiar v2 na 8 utworach przy a = 1: −1,9 / +1,3 / +2,4 dB
//! (30–60 / 60–120 / 120–250 Hz). v3: wyrównanie kompresji, dzwony zależne od udziału basu, nieco mocniejsze nasycenie.
//!
//! Suwak: w = a, d = 1 + 2a, ratio = 1 + 0,6a, dzwony a·(3…5,5) dB @ 70 Hz i a·(2,5…4) dB @ 170 Hz.

use super::und;
use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};
use std::f64::consts::{FRAC_1_SQRT_2, SQRT_2};

const BETA: f64 = 0.30;
const F_LOW: f64 = 70.0;
const Q_LOW: f64 = 0.6;
const F_WARM: f64 = 170.0;
const Q_WARM: f64 = 0.7;

struct Chan {
    lp: [Svf; 2],
    sat_hp: Svf,
    sat_lp: Svf,
    low: Svf,
    bell: Svf,
}

impl Chan {
    fn new(fs: f64) -> Self {
        let lp = Svf::new(SvfCoeffs::new(fs, 150.0, FRAC_1_SQRT_2, Mode::Lowpass));
        Self {
            lp: [lp, lp],
            sat_hp: Svf::new(SvfCoeffs::new(fs, 25.0, FRAC_1_SQRT_2, Mode::Highpass)),
            sat_lp: Svf::new(SvfCoeffs::new(fs, 500.0, FRAC_1_SQRT_2, Mode::Lowpass)),
            low: Svf::new(SvfCoeffs::new(fs, F_LOW, Q_LOW, Mode::Bell(0.0))),
            bell: Svf::new(SvfCoeffs::new(fs, F_WARM, Q_WARM, Mode::Bell(0.0))),
        }
    }
    fn set_bells(&mut self, fs: f64, g_low: f64, g_warm: f64) {
        self.low.c = SvfCoeffs::new(fs, F_LOW, Q_LOW, Mode::Bell(g_low));
        self.bell.c = SvfCoeffs::new(fs, F_WARM, Q_WARM, Mode::Bell(g_warm));
    }
    #[inline]
    fn band(&mut self, x: f64) -> f64 {
        let t = self.lp[0].tick(x);
        self.lp[1].tick(t)
    }
    fn flush(&mut self) {
        for f in self.lp.iter_mut() {
            f.flush_denormals();
        }
        self.sat_hp.flush_denormals();
        self.sat_lp.flush_denormals();
        self.low.flush_denormals();
        self.bell.flush_denormals();
    }
}

pub struct WarmBass {
    ctx: Ctx,
    a: Smoother,
    ch: [Chan; 2],
    /// gładki poziom (średni kwadrat) pasma basu do normalizacji nasycenia – na próbkę
    ms_sat: f64,
    a_sat: f64,
    // kompresja pasma (na blok sterujący)
    ms: f64,
    a_ms: f64,
    /// „nadmiar” ponad próg (dB, bez nachylenia suwaka) i jego wolna średnia (wyrównanie poziomu)
    gr: f64,
    gr_avg: f64,
    a_avg: f64,
    /// moce do udziału basu (3 s): pasmo < 150 Hz i całość
    p_lo: f64,
    p_all: f64,
    a_share: f64,
    /// 0 = bas dominuje … 1 = basu brakuje (wygładzone)
    need: f64,
    a_gr_att: f64,
    a_gr_rel: f64,
    g_prev: f64,
    last_bell: f64,
    gr_meter: f32,
}

impl WarmBass {
    pub fn new(ctx: Ctx) -> Self {
        let fs = ctx.fs;
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, fs),
            ch: [Chan::new(fs), Chan::new(fs)],
            ms_sat: 0.0,
            a_sat: 0.0,
            ms: 0.0,
            a_ms: 0.0,
            gr: 0.0,
            gr_avg: 0.0,
            a_avg: 0.0,
            p_lo: 0.0,
            p_all: 0.0,
            a_share: 0.0,
            need: 0.5,
            a_gr_att: 0.0,
            a_gr_rel: 0.0,
            g_prev: 1.0,
            last_bell: 0.0,
            gr_meter: 0.0,
        };
        s.reset(ctx);
        s
    }
}

/// Wzmocnienia dzwonów (70 Hz, 170 Hz) w dB; `need` 0 = bas dominuje, 1 = basu brakuje.
#[inline]
fn bells(a: f64, need: f64) -> (f64, f64) {
    (a * (3.0 + 2.5 * need), a * (2.5 + 1.5 * need))
}

#[inline]
fn shaper(u: f64, d: f64) -> f64 {
    let t0 = (d * BETA).tanh();
    let slope = d * (1.0 - t0 * t0);
    ((d * (u + BETA)).tanh() - t0) / slope
}

impl Module for WarmBass {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::WarmBass]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        let fs = ctx.fs;
        let kfs = fs / CONTROL_BLOCK as f64;
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, fs);
        self.ch = [Chan::new(fs), Chan::new(fs)];
        self.ms_sat = 0.0;
        self.a_sat = tau_coeff(0.060, fs);
        self.ms = 0.0;
        self.a_ms = tau_coeff(0.050, kfs);
        self.gr = 0.0;
        self.gr_avg = 0.0;
        self.a_avg = tau_coeff(4.0, kfs);
        self.p_lo = 0.0;
        self.p_all = 0.0;
        self.a_share = tau_coeff(3.0, kfs);
        self.need = 0.5;
        self.a_gr_att = tau_coeff(0.040, kfs);
        self.a_gr_rel = tau_coeff(0.500, kfs);
        self.g_prev = 1.0;
        let (gl, gw) = bells(self.a.value, self.need);
        for c in self.ch.iter_mut() {
            c.set_bells(fs, gl, gw);
        }
        self.last_bell = gl;
        self.gr_meter = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let fs = self.ctx.fs;
        let n = l.len();
        let mut off = 0;
        let mut gr_max = 0.0f64;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let a = self.a.advance(k);
            if a <= 0.0 && self.last_bell == 0.0 && self.g_prev == 1.0 {
                // przezroczyście; filtry pasma i poziomy aktualizowane, żeby wejście w działanie było płynne
                let mut sq = 0.0;
                for i in off..off + k {
                    let bl = self.ch[0].band(l[i]);
                    let br = self.ch[1].band(r[i]);
                    let p = 0.5 * (bl * bl + br * br);
                    self.ms_sat = p + self.a_sat * (self.ms_sat - p);
                    sq += p;
                }
                self.ms = self.a_ms * self.ms + (1.0 - self.a_ms) * sq / k as f64;
                off += k;
                continue;
            }
            let w = 1.0 * a;
            let d = 1.0 + 2.0 * a;
            let ratio = 1.0 + 0.6 * a;
            let (gl, gw) = bells(a, self.need);
            if (gl - self.last_bell).abs() > 1e-3 || (gl == 0.0 && self.last_bell != 0.0) {
                for c in self.ch.iter_mut() {
                    c.set_bells(fs, gl, gw);
                }
                self.last_bell = gl;
            }
            // kompresja pasma basu (detektor z poprzedniego bloku – 16 próbek bez znaczenia przy ataku 40 ms)
            let thr = -20.0;
            let knee = 10.0;
            let lvl = 10.0 * (self.ms + 1e-18).log10();
            let dd = lvl - thr;
            let slope = 1.0 - 1.0 / ratio;
            let over = if 2.0 * dd < -knee {
                0.0
            } else if 2.0 * dd.abs() <= knee {
                (dd + knee / 2.0).powi(2) / (2.0 * knee)
            } else {
                dd
            };
            let al = if over > self.gr { self.a_gr_att } else { self.a_gr_rel };
            self.gr = al * self.gr + (1.0 - al) * over;
            if over == 0.0 && self.gr < 1e-6 {
                self.gr = 0.0; // dokładnie 1,0 wzmocnienia → przy a = 0 pełna przezroczystość
            }
            self.gr_avg = self.a_avg * self.gr_avg + (1.0 - self.a_avg) * self.gr;
            // ograniczenie: po długiej ciszy/zmianie utworu wyrównanie nie może zagrać za głośno
            let g_db = (slope * (self.gr_avg - self.gr)).clamp(-6.0, 1.5);
            let g_target = und(g_db);
            gr_max = gr_max.max(-g_db);
            let g0 = self.g_prev;
            let dg = (g_target - g0) / k as f64;
            let mut sq = 0.0;
            let mut sq_all = 0.0;
            for j in 0..k {
                let i = off + j;
                let g = g0 + dg * (j + 1) as f64;
                let (xl, xr) = (l[i], r[i]);
                let bl = self.ch[0].band(xl);
                let br = self.ch[1].band(xr);
                let p = 0.5 * (bl * bl + br * br);
                sq += p;
                sq_all += 0.5 * (xl * xl + xr * xr);
                self.ms_sat = p + self.a_sat * (self.ms_sat - p);
                let lvl_pk = SQRT_2 * self.ms_sat.sqrt(); // amplituda sinusa o tym RMS
                let mut outs = [0.0f64; 2];
                for (c, (x, b)) in [(xl, bl), (xr, br)].into_iter().enumerate() {
                    let ch = &mut self.ch[c];
                    let u = (b / (lvl_pk + 1e-4)).clamp(-2.0, 2.0);
                    let sat = lvl_pk * shaper(u, d);
                    let hp = ch.sat_hp.tick(sat - b);
                    let diff = ch.sat_lp.tick(hp);
                    let delta = w * diff + b * (g - 1.0);
                    let lo = ch.low.tick(x + delta);
                    outs[c] = ch.bell.tick(lo);
                }
                l[i] = outs[0];
                r[i] = outs[1];
            }
            self.g_prev = g_target;
            // udział basu: −2 dB całości i więcej → need 0; −12 dB i mniej → need 1 (w ciszy bez zmian)
            if sq_all / k as f64 > 1e-7 {
                self.p_lo = self.a_share * self.p_lo + (1.0 - self.a_share) * sq / k as f64;
                self.p_all = self.a_share * self.p_all + (1.0 - self.a_share) * sq_all / k as f64;
                let share = 10.0 * ((self.p_lo + 1e-18) / (self.p_all + 1e-18)).log10();
                let nd = ((-2.0 - share) / 10.0).clamp(0.0, 1.0);
                self.need = nd + self.a_share * (self.need - nd);
            }
            self.ms = self.a_ms * self.ms + (1.0 - self.a_ms) * sq / k as f64;
            off += k;
        }
        for c in self.ch.iter_mut() {
            c.flush();
        }
        if self.ms_sat < 1e-30 {
            self.ms_sat = 0.0;
        }
        self.gr_meter = gr_max as f32;
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        self.gr_meter
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    #[test]
    fn standard() {
        crate::modules::testkit::standard(|c| Box::new(WarmBass::new(c)), ModuleId::WarmBass, 0.0);
    }

    /// Amplituda składowej f w sygnale (korelacja z sin/cos).
    fn amp(x: &[f64], f: f64, fs: f64) -> f64 {
        let (mut i, mut q) = (0.0, 0.0);
        for (n, v) in x.iter().enumerate() {
            let ph = 2.0 * PI * f * n as f64 / fs;
            i += v * ph.cos();
            q += v * ph.sin();
        }
        2.0 * (i * i + q * q).sqrt() / x.len() as f64
    }

    #[test]
    fn adds_second_harmonic_without_subharmonic() {
        let fs = 48_000.0;
        let ctx = Ctx::new(fs);
        let mut m = WarmBass::new(ctx);
        m.set_amount(ModuleId::WarmBass, 1.0);
        m.reset(ctx);
        let n = 48_000 * 3;
        let x: Vec<f64> = (0..n).map(|i| 0.3 * (2.0 * PI * 80.0 * i as f64 / fs).sin()).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        crate::modules::testkit::run(&mut m, &mut l, &mut r);
        let tail = &l[n - 48_000..];
        let a1 = amp(tail, 80.0, fs);
        let h2 = 20.0 * (amp(tail, 160.0, fs) / a1).log10();
        let sub = 20.0 * (amp(tail, 40.0, fs) / a1).log10();
        assert!(h2 > -40.0 && h2 < -10.0, "2. harmoniczna: {h2} dB");
        assert!(sub < -60.0, "subharmoniczna nie powinna powstawać: {sub} dB");
        assert!(tail.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn steady_tone_has_steady_envelope() {
        // stały sinus 50 Hz: obwiednia wyjścia nie może falować (brak „szarpania”)
        let fs = 48_000.0;
        let ctx = Ctx::new(fs);
        let mut m = WarmBass::new(ctx);
        m.set_amount(ModuleId::WarmBass, 1.0);
        m.reset(ctx);
        let n = 48_000 * 25; // 25 s: wolne średnie (3–4 s) już ustalone, mierzymy samo falowanie
        let x: Vec<f64> = (0..n).map(|i| 0.4 * (2.0 * PI * 50.0 * i as f64 / fs).sin()).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        crate::modules::testkit::run(&mut m, &mut l, &mut r);
        // szczyt w kolejnych okresach (960 próbek) w ostatnich 2 s
        let per = 960;
        let peaks: Vec<f64> = l[n - 96_000..].chunks(per).map(|c| c.iter().fold(0.0f64, |a, &v| a.max(v.abs()))).collect();
        let (lo, hi) = peaks.iter().fold((f64::MAX, 0.0f64), |(a, b), &p| (a.min(p), b.max(p)));
        let ripple = 20.0 * (hi / lo).log10();
        assert!(ripple < 0.05, "obwiednia faluje o {ripple} dB");
    }

    #[test]
    fn leaves_highs_untouched() {
        let fs = 48_000.0;
        let ctx = Ctx::new(fs);
        let mut m = WarmBass::new(ctx);
        m.set_amount(ModuleId::WarmBass, 1.0);
        m.reset(ctx);
        let n = 48_000 * 2;
        let x: Vec<f64> = (0..n).map(|i| 0.3 * (2.0 * PI * 3000.0 * i as f64 / fs).sin()).collect();
        let (mut l, mut r) = (x.clone(), x.clone());
        crate::modules::testkit::run(&mut m, &mut l, &mut r);
        let g = 20.0 * (amp(&l[n / 2..], 3000.0, fs) / 0.3).log10();
        assert!(g.abs() < 0.3, "wpływ na 3 kHz: {g} dB");
    }
}
