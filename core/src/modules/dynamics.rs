//! Kompresory: Multiband 2 (§4.8, 3 pasma, wolny), Kompresor (§4.9, szerokopasmowy), Multiband 1 (§4.10, 5 pasm, szybki).
//! Wspólny silnik: zwrotnica LR4 (drzewo), detektor co blok sterujący (K = 16), komputer wzmocnienia z kolanem
//! (Giannoulis 2012), balistyka na redukcji w dB, makijaż automatyczny, rampa wzmocnienia w bloku.

use super::{und, GainRamp};
use crate::catalog::ModuleId;
use crate::dsp::crossover::Crossover;
use crate::dsp::dynamics::GainComputer;
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK, MAX_BLOCK};

#[derive(Clone, Copy, Debug)]
struct BandSpec {
    t0: f64,
    l_ref: f64,
    attack: f64,
    release: f64,
    /// druga gałąź zwolnienia (0 = brak), atak gałęzi wolnej = 10·attack
    release2: f64,
    /// stała czasowa detektora pasma [s]; 0 = wspólna z `Config::det`
    det: f64,
}

#[derive(Clone, Copy, Debug)]
enum Detector {
    Rms(f64),
    /// max(RMS τ, szczyt − 9 dB)
    RmsPeak(f64),
}

struct Config {
    id: ModuleId,
    freqs: &'static [f64],
    bands: &'static [BandSpec],
    det: Detector,
    knee: f64,
    max_red: f64,
    r_max: f64,
    t_drop: f64,
    makeup_k: f64,
    /// poziom zamrażania zwolnienia (MB2): poniżej – redukcja nie maleje
    freeze_db: Option<f64>,
    /// sprzężenie pasm Δ [dB] (MB1)
    link: Option<f64>,
}

const MB2_BANDS: [BandSpec; 3] = [
    BandSpec { t0: -22.0, l_ref: -20.0, attack: 0.3, release: 2.0, release2: 0.0, det: 0.0 },
    BandSpec { t0: -22.0, l_ref: -20.0, attack: 0.3, release: 2.0, release2: 0.0, det: 0.0 },
    BandSpec { t0: -30.0, l_ref: -28.0, attack: 0.3, release: 2.0, release2: 0.0, det: 0.0 },
];
const COMP_BANDS: [BandSpec; 1] = [BandSpec { t0: -20.0, l_ref: -19.0, attack: 0.01, release: 0.08, release2: 1.0, det: 0.0 }];
const MB1_BANDS: [BandSpec; 5] = [
    // det: detektor musi być wolniejszy niż okres najniższych dźwięków pasma. Wspólne 5 ms i chwilowy szczyt (do 0.5.0)
    // sprawiały, że w basie wzmocnienie falowało w takt samej fali – zniekształcenia ok. 1,3 % na stałym sygnale.
    BandSpec { t0: -22.0, l_ref: -20.0, attack: 0.020, release: 0.250, release2: 0.0, det: 0.045 },
    BandSpec { t0: -23.0, l_ref: -21.0, attack: 0.015, release: 0.200, release2: 0.0, det: 0.018 },
    BandSpec { t0: -25.0, l_ref: -23.0, attack: 0.010, release: 0.150, release2: 0.0, det: 0.008 },
    BandSpec { t0: -30.0, l_ref: -28.0, attack: 0.005, release: 0.100, release2: 0.0, det: 0.005 },
    BandSpec { t0: -36.0, l_ref: -34.0, attack: 0.003, release: 0.080, release2: 0.0, det: 0.005 },
];

#[derive(Clone, Copy, Default)]
struct BandState {
    ms: f64,
    /// szczyt z opadaniem (zamiast chwilowego)
    pk: f64,
    s_f: f64,
    s_s: f64,
    ramp: GainRampState,
}

#[derive(Clone, Copy)]
struct GainRampState(GainRamp);
impl Default for GainRampState {
    fn default() -> Self {
        Self(GainRamp::default())
    }
}

pub struct MultibandComp {
    cfg: Config,
    ctx: Ctx,
    a: Smoother,
    xl: Option<Crossover>,
    xr: Option<Crossover>,
    st: Vec<BandState>,
    // współczynniki na blok sterujący
    a_det: Vec<f64>,
    a_att: Vec<f64>,
    a_rel: Vec<f64>,
    a_att2: Vec<f64>,
    a_rel2: Vec<f64>,
    // bufory pasm [pasmo][próbka]
    bl: Vec<Vec<f64>>,
    br: Vec<Vec<f64>>,
    gr: f32,
}

impl MultibandComp {
    fn with(cfg: Config, ctx: Ctx) -> Self {
        let nb = cfg.bands.len();
        let mut s = Self {
            cfg,
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            xl: None,
            xr: None,
            st: vec![BandState::default(); nb],
            a_det: vec![0.0; nb],
            a_att: vec![0.0; nb],
            a_rel: vec![0.0; nb],
            a_att2: vec![0.0; nb],
            a_rel2: vec![0.0; nb],
            bl: vec![vec![0.0; MAX_BLOCK]; nb],
            br: vec![vec![0.0; MAX_BLOCK]; nb],
            gr: 0.0,
        };
        s.reset(ctx);
        s
    }
    pub fn multiband2(ctx: Ctx) -> Self {
        Self::with(
            Config { id: ModuleId::Multiband2, freqs: &[200.0, 3000.0], bands: &MB2_BANDS, det: Detector::Rms(0.05), knee: 10.0, max_red: 9.0, r_max: 2.5, t_drop: 8.0, makeup_k: 0.6, freeze_db: Some(-55.0), link: None },
            ctx,
        )
    }
    pub fn wideband(ctx: Ctx) -> Self {
        Self::with(
            Config { id: ModuleId::Compressor, freqs: &[], bands: &COMP_BANDS, det: Detector::Rms(0.01), knee: 6.0, max_red: 24.0, r_max: 3.0, t_drop: 4.0, makeup_k: 0.7, freeze_db: None, link: None },
            ctx,
        )
    }
    pub fn multiband1(ctx: Ctx) -> Self {
        Self::with(
            Config {
                id: ModuleId::Multiband1,
                freqs: &[100.0, 400.0, 1600.0, 6400.0],
                bands: &MB1_BANDS,
                det: Detector::RmsPeak(0.005),
                knee: 6.0,
                max_red: 12.0,
                r_max: 4.0,
                t_drop: 10.0,
                makeup_k: 0.6,
                freeze_db: None,
                link: Some(6.0),
            },
            ctx,
        )
    }
}

impl Module for MultibandComp {
    fn ids(&self) -> &'static [ModuleId] {
        match self.cfg.id {
            ModuleId::Multiband1 => &[ModuleId::Multiband1],
            ModuleId::Multiband2 => &[ModuleId::Multiband2],
            _ => &[ModuleId::Compressor],
        }
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        if self.cfg.freqs.is_empty() {
            self.xl = None;
            self.xr = None;
        } else {
            self.xl = Some(Crossover::new(ctx.fs, self.cfg.freqs));
            self.xr = Some(Crossover::new(ctx.fs, self.cfg.freqs));
        }
        let kfs = ctx.fs / CONTROL_BLOCK as f64;
        let det_common = match self.cfg.det {
            Detector::Rms(t) | Detector::RmsPeak(t) => t,
        };
        for (b, bs) in self.cfg.bands.iter().enumerate() {
            self.a_det[b] = tau_coeff(if bs.det > 0.0 { bs.det } else { det_common }, kfs);
            self.a_att[b] = tau_coeff(bs.attack, kfs);
            self.a_rel[b] = tau_coeff(bs.release, kfs);
            self.a_att2[b] = tau_coeff(10.0 * bs.attack, kfs);
            self.a_rel2[b] = tau_coeff(bs.release2.max(1e-3), kfs);
        }
        for s in self.st.iter_mut() {
            *s = BandState::default();
        }
        self.gr = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let nb = self.cfg.bands.len();
        let mut off = 0;
        let n = l.len();
        let mut buf = [0.0f64; 5];
        let mut gr_max = 0.0f64;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let a = self.a.advance(k);
            // 1) podział na pasma
            for i in 0..k {
                let (x, y) = (l[off + i], r[off + i]);
                if let (Some(xl), Some(xr)) = (self.xl.as_mut(), self.xr.as_mut()) {
                    xl.split(x, &mut buf);
                    for b in 0..nb {
                        self.bl[b][i] = buf[b];
                    }
                    xr.split(y, &mut buf);
                    for b in 0..nb {
                        self.br[b][i] = buf[b];
                    }
                } else {
                    self.bl[0][i] = x;
                    self.br[0][i] = y;
                }
            }
            // 2) detektor + komputer + balistyka
            let ratio = 1.0 + (self.cfg.r_max - 1.0) * a;
            let mut s_now = [0.0f64; 5];
            for b in 0..nb {
                let spec = self.cfg.bands[b];
                let (mut sq, mut pk) = (0.0, 0.0f64);
                for i in 0..k {
                    let (u, v) = (self.bl[b][i], self.br[b][i]);
                    sq += u * u + v * v;
                    pk = pk.max(u.abs()).max(v.abs());
                }
                let st = &mut self.st[b];
                let ad = self.a_det[b];
                st.ms = ad * st.ms + (1.0 - ad) * sq / (2.0 * k as f64);
                st.pk = pk.max(ad * st.pk);
                let rms_db = 10.0 * (st.ms + 1e-18).log10();
                let xg = match self.cfg.det {
                    Detector::Rms(_) => rms_db,
                    Detector::RmsPeak(_) => rms_db.max(20.0 * (st.pk + 1e-9).log10() - 9.0),
                };
                let gc = GainComputer { threshold_db: spec.t0 - self.cfg.t_drop * a, ratio, knee_db: self.cfg.knee };
                let c = gc.reduction_db(xg).min(self.cfg.max_red);
                let frozen = self.cfg.freeze_db.map(|f| xg < f).unwrap_or(false);
                // gałąź szybka
                if c > st.s_f {
                    st.s_f = self.a_att[b] * st.s_f + (1.0 - self.a_att[b]) * c;
                } else if !frozen {
                    st.s_f = self.a_rel[b] * st.s_f + (1.0 - self.a_rel[b]) * c;
                }
                let mut s = st.s_f;
                if spec.release2 > 0.0 {
                    if c > st.s_s {
                        st.s_s = self.a_att2[b] * st.s_s + (1.0 - self.a_att2[b]) * c;
                    } else if !frozen {
                        st.s_s = self.a_rel2[b] * st.s_s + (1.0 - self.a_rel2[b]) * c;
                    }
                    s = s.max(st.s_s);
                }
                s_now[b] = s;
            }
            if let Some(d) = self.cfg.link {
                for b in 1..nb {
                    s_now[b] = s_now[b].max(s_now[b - 1] - d);
                }
                for b in (0..nb - 1).rev() {
                    s_now[b] = s_now[b].max(s_now[b + 1] - d);
                }
            }
            // 3) wzmocnienie z makijażem, rampa, suma pasm
            for i in 0..k {
                l[off + i] = 0.0;
                r[off + i] = 0.0;
            }
            for b in 0..nb {
                let spec = self.cfg.bands[b];
                let t = spec.t0 - self.cfg.t_drop * a;
                let makeup = self.cfg.makeup_k * (1.0 - 1.0 / ratio) * (spec.l_ref - t);
                let g = und(makeup - s_now[b]);
                gr_max = gr_max.max(s_now[b] - makeup.max(0.0)).max(0.0);
                let (bl, br) = (&mut self.bl[b][..k], &mut self.br[b][..k]);
                self.st[b].ramp.0.apply(g, bl, br);
                for i in 0..k {
                    l[off + i] += bl[i];
                    r[off + i] += br[i];
                }
            }
            off += k;
        }
        if let (Some(xl), Some(xr)) = (self.xl.as_mut(), self.xr.as_mut()) {
            xl.flush_denormals();
            xr.flush_denormals();
        }
        self.gr = gr_max as f32;
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        self.gr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compressor_standard() {
        crate::modules::testkit::standard(|c| Box::new(MultibandComp::wideband(c)), ModuleId::Compressor, 1e-12);
    }
    #[test]
    fn multiband_standard() {
        // a = 0: suma pasm = all-pass (wyjątek ze spec) – nie porównujemy z wejściem próbka w próbkę
        crate::modules::testkit::standard(|c| Box::new(MultibandComp::multiband1(c)), ModuleId::Multiband1, 10.0);
        crate::modules::testkit::standard(|c| Box::new(MultibandComp::multiband2(c)), ModuleId::Multiband2, 10.0);
    }
    #[test]
    fn compressor_reduces_loud_signal() {
        let fs = 44_100.0;
        let mut m = MultibandComp::wideband(Ctx::new(fs));
        m.set_amount(ModuleId::Compressor, 1.0);
        m.reset(Ctx::new(fs));
        let n = 44_100;
        let mut l: Vec<f64> = (0..n).map(|i| 0.9 * (i as f64 * 0.06).sin()).collect();
        let mut r = l.clone();
        crate::modules::testkit::run(&mut m, &mut l, &mut r);
        let peak = l[n / 2..].iter().fold(0.0f64, |a, &v| a.max(v.abs()));
        assert!(peak < 0.75, "kompresor nie zredukował: {peak}");
        assert!(m.gain_reduction_db(ModuleId::Compressor) > 1.0);
    }
}
