//! Szyna STFT B (spec v1 §3.9): N = 1024·F, skok N/4. Auto EQ (§4.16) → Balans widma (§4.17).
//! Wzmocnienia wspólne dla L i R (liczone z mocy L+R) – obraz stereo nietknięty.

use super::bus_a::Sub;
use super::{lin, und};
use crate::catalog::ModuleId;
use crate::dsp::stft::{StftStereo, C64};
use crate::dsp::util::Smoother;
use crate::module::{Ctx, Module};

pub struct BusB {
    ctx: Ctx,
    stft: Option<StftStereo>,
    ae: Sub,
    sb: Sub,
    // Auto EQ
    bands: Vec<(usize, usize, f64)>,
    lb: Vec<f64>,
    lb_init: bool,
    /// średnia moc pasm (liniowo) i liczba uśrednionych ramek (≤ 30 s)
    lb_pow: Vec<f64>,
    lb_frames: f64,
    eb: Vec<f64>,
    /// bufory robocze ramki (korekta pasm przed/po wygładzeniu) – prealokowane w `reset`
    e_raw: Vec<f64>,
    e_sm: Vec<f64>,
    gk_db: Vec<f64>,
    // Balans widma
    pw: Vec<f64>,
    cum: Vec<f64>,
    gr: Vec<f64>,
    gr_tmp: Vec<f64>,
    gr_max: f32,
    lo12: Vec<(usize, usize)>,
    lo2: Vec<(usize, usize)>,
}

fn target_db(f: f64) -> f64 {
    let base = -1.5 * (f.clamp(60.0, 8000.0) / 1000.0).log2();
    let extra = if f > 8000.0 {
        -3.0 * (f / 8000.0).log2()
    } else if f < 60.0 {
        -6.0 * (60.0 / f).log2()
    } else {
        0.0
    };
    base + extra
}

impl BusB {
    pub fn new(ctx: Ctx) -> Self {
        let mut s = Self {
            ctx,
            stft: None,
            ae: Sub::new(ctx.fs),
            sb: Sub::new(ctx.fs),
            bands: Vec::new(),
            lb: Vec::new(),
            lb_init: false,
            lb_pow: Vec::new(),
            lb_frames: 0.0,
            eb: Vec::new(),
            e_raw: Vec::new(),
            e_sm: Vec::new(),
            gk_db: Vec::new(),
            pw: Vec::new(),
            cum: Vec::new(),
            gr: Vec::new(),
            gr_tmp: Vec::new(),
            gr_max: 0.0,
            lo12: Vec::new(),
            lo2: Vec::new(),
        };
        s.reset(ctx);
        s
    }
    fn sub(&mut self, id: ModuleId) -> &mut Sub {
        if id == ModuleId::AutoEq {
            &mut self.ae
        } else {
            &mut self.sb
        }
    }

    fn frame(&mut self, xl: &mut [C64], xr: &mut [C64], a_ae: f64, a_sb: f64) {
        let nb = xl.len();
        let n = 1024 * self.ctx.f();
        let fs = self.ctx.fs;
        let hop = (n / 4) as f64;
        let bin_hz = fs / n as f64;
        for k in 0..nb {
            self.pw[k] = xl[k].norm_sqr() + xr[k].norm_sqr();
        }
        // poziom ramki (MS z Parsevala, okno Hanna Σw² = 0,375N)
        let tot: f64 = self.pw.iter().sum::<f64>() * 2.0;
        let frame_ms = tot / (n as f64 * 0.375 * n as f64) / 2.0;
        let frame_db = 10.0 * (frame_ms + 1e-30).log10();

        // ── Auto EQ ── (estymata liczona zawsze, gdy ramka jest głośna)
        // Długa analiza (zmiana 2026-11-22 po odsłuchu: korekta „przeskakiwała” razem z aranżacją utworu):
        // średnia MOCY pasm z ok. 30 s (na starcie średnia narastająca – bez skoku), korekta wygładzana τ = 6 s
        // i ograniczona do 0,3 dB/s.
        let frames_30s = (30.0 * fs / hop).max(1.0);
        let a_e = (-hop / (6.0 * fs)).exp();
        let max_step = 0.3 * hop / fs;
        let nbands = self.bands.len();
        if frame_db > -50.0 {
            self.lb_frames = (self.lb_frames + 1.0).min(frames_30s);
            let k_avg = 1.0 / self.lb_frames;
            for (b, &(k0, k1, _)) in self.bands.iter().enumerate() {
                let p: f64 = self.pw[k0..k1].iter().sum();
                self.lb_pow[b] += k_avg * (p - self.lb_pow[b]);
                self.lb[b] = 10.0 * (self.lb_pow[b] + 1e-30).log10();
            }
            self.lb_init = true;
        }
        let g_ae = 6.0 * a_ae;
        if self.lb_init {
            let o = (0..nbands).map(|b| self.lb[b] - target_db(self.bands[b].2)).sum::<f64>() / nbands as f64;
            // bez alokacji: e_raw = korekta pasm, e_sm = po wygładzeniu [¼, ½, ¼]
            for b in 0..nbands {
                self.e_raw[b] = (0.7 * (target_db(self.bands[b].2) - (self.lb[b] - o))).clamp(-g_ae, g_ae);
            }
            for b in 0..nbands {
                self.e_sm[b] = if nbands >= 3 {
                    let p = self.e_raw[b.saturating_sub(1)];
                    let q = self.e_raw[(b + 1).min(nbands - 1)];
                    0.25 * p + 0.5 * self.e_raw[b] + 0.25 * q
                } else {
                    self.e_raw[b]
                };
            }
            for b in 0..nbands {
                let target = a_e * self.eb[b] + (1.0 - a_e) * self.e_sm[b];
                self.eb[b] += (target - self.eb[b]).clamp(-max_step, max_step);
            }
        }
        // interpolacja liniowa w log f → prążki
        for k in 0..nb {
            let f = (k as f64 * bin_hz).max(1.0);
            let lf = f.log2();
            let g = if f <= self.bands[0].2 {
                self.eb[0]
            } else if f >= self.bands[nbands - 1].2 {
                self.eb[nbands - 1]
            } else {
                let mut b = 0;
                while b + 1 < nbands && self.bands[b + 1].2 < f {
                    b += 1;
                }
                let (f0, f1) = (self.bands[b].2.log2(), self.bands[b + 1].2.log2());
                let t = (lf - f0) / (f1 - f0);
                self.eb[b] + t * (self.eb[b + 1] - self.eb[b])
            };
            // Ograniczenie do ±G_ae także przy zastosowaniu: E_b jest wygładzane τ = 1 s, a moc (z neutralizacją
            // 50 ms) schodzi do 0 szybciej – bez tego korekta kilku dB znikała skokiem w chwili a = 0.
            self.gk_db[k] = if a_ae > 0.0 { g.clamp(-g_ae, g_ae) } else { 0.0 };
        }

        // ── Balans widma ──
        // 2026-11-25: było 0,67 / 9 dB / próg 3 dB, teraz 1,0 / 12 dB / 1 dB – 0,7 JND przy 100% (niesłyszalne)
        let s_b = lin(a_sb, 0.0, 1.0);
        let d_b = 12.0 * a_sb;
        self.cum[0] = 0.0;
        for k in 0..nb {
            self.cum[k + 1] = self.cum[k] + self.pw[k];
        }
        let att = (-hop / (0.020 * fs)).exp();
        let rel = (-hop / (0.200 * fs)).exp();
        let mut gr_max = 0.0f64;
        for k in 0..nb {
            let f = k as f64 * bin_hz;
            let (a0, a1) = self.lo12[k];
            let (c0, c1) = self.lo2[k];
            let mf = (self.cum[a1] - self.cum[a0]) / (a1 - a0) as f64;
            let mc = (self.cum[c1] - self.cum[c0]) / (c1 - c0) as f64;
            let x = 10.0 * ((mf + 1e-30) / (mc + 1e-30)).log10() - 1.0;
            let wf = if f < 200.0 {
                0.0
            } else if f < 400.0 {
                (f - 200.0) / 200.0
            } else if f <= 16_000.0 {
                1.0
            } else if f < 18_000.0 {
                (18_000.0 - f) / 2000.0
            } else {
                0.0
            };
            let target = (s_b * x.max(0.0)).min(d_b) * wf;
            let c = if target > self.gr[k] { att } else { rel };
            self.gr[k] = c * self.gr[k] + (1.0 - c) * target;
            if target == 0.0 && self.gr[k] < 1e-6 {
                self.gr[k] = 0.0;
            }
        }
        for k in 0..nb {
            let p = self.gr[k.saturating_sub(1)];
            let q = self.gr[(k + 1).min(nb - 1)];
            self.gr_tmp[k] = 0.25 * p + 0.5 * self.gr[k] + 0.25 * q;
            gr_max = gr_max.max(self.gr_tmp[k]);
        }
        self.gr_max = gr_max as f32;
        // zastosowanie
        for k in 0..nb {
            let db = self.gk_db[k] - self.gr_tmp[k];
            if db != 0.0 {
                let g = und(db);
                xl[k] *= g;
                xr[k] *= g;
            }
        }
    }
}

impl Module for BusB {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::AutoEq, ModuleId::SpectrumBalance]
    }
    fn set_amount(&mut self, id: ModuleId, a: f32) {
        self.sub(id).a.set_target(a as f64);
    }
    fn set_sub_enabled(&mut self, id: ModuleId, on: bool) {
        self.sub(id).on = on;
    }
    fn latency_samples(&self) -> usize {
        1024 * self.ctx.f()
    }
    fn warmup_samples(&self) -> usize {
        self.latency_samples() + self.ctx.ms(1000.0)
    }
    fn reset(&mut self, ctx: Ctx) {
        let fs = ctx.fs;
        let n = 1024 * ctx.f();
        let nb = n / 2 + 1;
        self.ctx = ctx;
        // `reset` bywa wołany z wątku audio (włączenie modułu) – plan FFT tworzymy tylko przy zmianie rozmiaru
        match self.stft.as_mut() {
            Some(s) if s.size() == n => s.reset(),
            _ => self.stft = Some(StftStereo::new(n)),
        }
        for sub in [&mut self.ae, &mut self.sb] {
            sub.a = Smoother::new(sub.a.target(), 0.02, fs);
            sub.e = if sub.on { 1.0 } else { 0.0 };
        }
        let bin_hz = fs / n as f64;
        // pasma 1/3 okt. 50 Hz … 16 kHz, nie węższe niż 2 prążki (najniższe łączone)
        self.bands.clear();
        let mut f_lo = 50.0 * 2f64.powf(-1.0 / 6.0);
        let mut k_start = (f_lo / bin_hz).round().max(1.0) as usize;
        while f_lo < 16_000.0 {
            let f_hi = f_lo * 2f64.powf(1.0 / 3.0);
            let k1 = ((f_hi / bin_hz).round() as usize).min(nb);
            if k1 >= k_start + 2 {
                let fc = ((k_start as f64 * bin_hz) * (k1 as f64 * bin_hz)).sqrt();
                self.bands.push((k_start, k1, fc));
                k_start = k1;
            }
            f_lo = f_hi;
        }
        let nbands = self.bands.len();
        self.lb = vec![0.0; nbands];
        self.lb_init = false;
        self.lb_pow = vec![0.0; nbands];
        self.lb_frames = 0.0;
        self.eb = vec![0.0; nbands];
        self.e_raw = vec![0.0; nbands];
        self.e_sm = vec![0.0; nbands];
        self.gk_db = vec![0.0; nb];
        self.pw = vec![0.0; nb];
        self.cum = vec![0.0; nb + 1];
        self.gr = vec![0.0; nb];
        self.gr_tmp = vec![0.0; nb];
        self.gr_max = 0.0;
        let span = |k: usize, oct: f64| -> (usize, usize) {
            let f = (k as f64 * bin_hz).max(bin_hz);
            let a = ((f * 2f64.powf(-oct)) / bin_hz).floor() as usize;
            let b = (((f * 2f64.powf(oct)) / bin_hz).ceil() as usize + 1).min(nb);
            (a.min(k), b.max(k + 1))
        };
        self.lo12 = (0..nb).map(|k| span(k, 1.0 / 12.0)).collect();
        self.lo2 = (0..nb).map(|k| span(k, 0.5)).collect();
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let fs = self.ctx.fs;
        let n = l.len();
        let a_ae = self.ae.advance(n, fs);
        let a_sb = self.sb.advance(n, fs);
        if let Some(mut stft) = self.stft.take() {
            stft.process(l, r, |xl, xr| self.frame(xl, xr, a_ae, a_sb));
            self.stft = Some(stft);
        }
    }
    fn gain_reduction_db(&self, id: ModuleId) -> f32 {
        if id == ModuleId::SpectrumBalance {
            self.gr_max
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transparent_when_amounts_zero() {
        crate::modules::testkit::standard(|c| Box::new(BusB::new(c)), ModuleId::AutoEq, 1e-9);
        crate::modules::testkit::standard(|c| Box::new(BusB::new(c)), ModuleId::SpectrumBalance, 1e-9);
    }
}
