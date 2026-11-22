//! Dehummer (spec v1 §4.4).
//!
//! Detekcja na mono zdecymowanym ×8 (po LP 1 kHz): demodulacja kwadraturowa przy k·f0 (k = 1…8, f0 ∈ {50, 60} Hz),
//! I/Q przez LP 1. rzędu 2 Hz → amplituda A_k, faza φ_k. Odniesienie: RMS pasm k·f0·2^(±1/4) (BP Q = 8), τ = 1 s.
//! nadwyżka_k = max(0, dB(A_k/(√2·R_k))). Brum ⇔ przez ≥ 3 s istnieje k ≤ 3 z nadwyżką > 10 dB i wahaniem A_k < 3 dB.
//! Strojenie: Δf z nachylenia fazy φ_1 (uśrednione 2 s), ±2 %. Usuwanie: 8 dzwonów SVF przy k·f0′, Q = 40,
//! G_k = −e_h·min(D_h, nadwyżka_k) dB; e_h: 0→1 w 1 s po wykryciu, 1→0 w 3 s po zaniku. Gdy e_h = 0 – bez filtrów (bit-exact).
//! Suwak: D_h = 30a dB.

use crate::catalog::ModuleId;
use crate::dsp::svf::{Mode, Svf, SvfCoeffs};
use crate::dsp::util::{tau_coeff, Smoother};
use crate::module::{Ctx, Module, CONTROL_BLOCK};
use std::f64::consts::{FRAC_1_SQRT_2, PI};

const DEC: usize = 8;
/// Odstępstwo od spec v1 (Q = 8): przy odstępie ±1/4 okt. pasmo odniesienia Q = 8 łapie sam brum (−9 dB),
/// przez co nadwyżka nigdy nie przekracza 10 dB. Q = 30 → przeciek ≈ −20 dB.
const REF_Q: f64 = 30.0;
const H: usize = 8;

struct Detector {
    f0: f64,
    ph: [f64; H],
    i: [f64; H],
    q: [f64; H],
    bp_lo: [Svf; H],
    bp_hi: [Svf; H],
    r_ms: [f64; H],
    amp_db: [f64; H],
    excess: [f64; H],
    // statystyki co 100 ms
    hist_db: Vec<[f64; 3]>, // A_1..A_3 w dB z ostatnich 3 s (30 wpisów)
    hist_pos: usize,
    ok_ticks: usize,
    prev_phase: f64,
    df_avg: f64,
    pub hum: bool,
}

impl Detector {
    fn new(fd: f64, f0: f64) -> Self {
        let mk = |k: usize, m: f64| Svf::new(SvfCoeffs::new(fd, (k as f64 * f0 * m).min(0.45 * fd), REF_Q, Mode::Bandpass));
        Self {
            f0,
            ph: [0.0; H],
            i: [0.0; H],
            q: [0.0; H],
            bp_lo: std::array::from_fn(|k| mk(k + 1, 2f64.powf(-0.25))),
            bp_hi: std::array::from_fn(|k| mk(k + 1, 2f64.powf(0.25))),
            r_ms: [0.0; H],
            amp_db: [-120.0; H],
            excess: [0.0; H],
            hist_db: vec![[-120.0; 3]; 30],
            hist_pos: 0,
            ok_ticks: 0,
            prev_phase: 0.0,
            df_avg: 0.0,
            hum: false,
        }
    }
    fn tick(&mut self, x: f64, fd: f64, a_iq: f64, a_r: f64) {
        for k in 0..H {
            let w = 2.0 * PI * (k as f64 + 1.0) * self.f0 / fd;
            self.ph[k] += w;
            if self.ph[k] > 2.0 * PI {
                self.ph[k] -= 2.0 * PI;
            }
            let (s, c) = self.ph[k].sin_cos();
            self.i[k] = a_iq * self.i[k] + (1.0 - a_iq) * x * c;
            self.q[k] = a_iq * self.q[k] + (1.0 - a_iq) * x * s;
            let lo = self.bp_lo[k].tick(x);
            let hi = self.bp_hi[k].tick(x);
            let e = 0.5 * (lo * lo + hi * hi);
            self.r_ms[k] = a_r * self.r_ms[k] + (1.0 - a_r) * e;
        }
    }
    /// Co 100 ms: amplitudy, nadwyżki, decyzja, strojenie.
    fn decide(&mut self, dt: f64) {
        for k in 0..H {
            let a = 2.0 * (self.i[k].hypot(self.q[k]));
            self.amp_db[k] = 20.0 * (a + 1e-12).log10();
            let r = (2.0 * self.r_ms[k]).sqrt() + 1e-12; // amplituda sinusa o tym RMS
            self.excess[k] = (self.amp_db[k] - 20.0 * r.log10()).max(0.0);
        }
        self.hist_db[self.hist_pos] = [self.amp_db[0], self.amp_db[1], self.amp_db[2]];
        self.hist_pos = (self.hist_pos + 1) % self.hist_db.len();
        let mut cond = false;
        for k in 0..3 {
            let (mut lo, mut hi) = (f64::MAX, f64::MIN);
            for h in self.hist_db.iter() {
                lo = lo.min(h[k]);
                hi = hi.max(h[k]);
            }
            if self.excess[k] > 10.0 && hi - lo < 3.0 && self.amp_db[k] > -90.0 {
                cond = true;
            }
        }
        self.ok_ticks = if cond { self.ok_ticks + 1 } else { 0 };
        let was = self.hum;
        self.hum = self.ok_ticks >= 30;
        if was && !cond {
            self.hum = false;
        }
        // strojenie z nachylenia fazy podstawowej
        let phase = self.q[0].atan2(self.i[0]);
        let mut dphi = phase - self.prev_phase;
        while dphi > PI {
            dphi -= 2.0 * PI;
        }
        while dphi < -PI {
            dphi += 2.0 * PI;
        }
        self.prev_phase = phase;
        // faza rośnie, gdy rzeczywista f < f0 (demodulacja e^{-jωt}): Δf = −dφ/(2π dt)
        let df = -dphi / (2.0 * PI * dt);
        let a2 = (-dt / 2.0f64).exp();
        self.df_avg = a2 * self.df_avg + (1.0 - a2) * df;
    }
    fn f_tuned(&self) -> f64 {
        self.f0 + self.df_avg.clamp(-0.02 * self.f0, 0.02 * self.f0)
    }
}

pub struct Dehummer {
    ctx: Ctx,
    a: Smoother,
    dec_lp: [Svf; 2],
    dec_cnt: usize,
    tick_cnt: usize,
    dets: [Detector; 2],
    active: Option<usize>,
    e_h: f64,
    bells: [[Svf; H]; 2],
    a_iq: f64,
    a_r: f64,
    last_gain: [f64; H],
    last_f: f64,
}

impl Dehummer {
    pub fn new(ctx: Ctx) -> Self {
        let fd = ctx.fs / DEC as f64;
        let mut s = Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            dec_lp: [Svf::default(); 2],
            dec_cnt: 0,
            tick_cnt: 0,
            dets: [Detector::new(fd, 50.0), Detector::new(fd, 60.0)],
            active: None,
            e_h: 0.0,
            bells: [[Svf::default(); H]; 2],
            a_iq: 0.0,
            a_r: 0.0,
            last_gain: [0.0; H],
            last_f: 0.0,
        };
        s.reset(ctx);
        s
    }
}

impl Module for Dehummer {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::Dehummer]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn latency_samples(&self) -> usize {
        0
    }
    fn reset(&mut self, ctx: Ctx) {
        let fd = ctx.fs / DEC as f64;
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        self.dec_lp = [Svf::new(SvfCoeffs::new(ctx.fs, 1000.0, FRAC_1_SQRT_2, Mode::Lowpass)); 2];
        self.dec_cnt = 0;
        self.tick_cnt = 0;
        self.dets = [Detector::new(fd, 50.0), Detector::new(fd, 60.0)];
        self.active = None;
        self.e_h = 0.0;
        self.bells = [[Svf::new(SvfCoeffs::new(ctx.fs, 1000.0, 1.0, Mode::Bell(0.0))); H]; 2];
        self.a_iq = tau_coeff(1.0 / (2.0 * PI * 2.0), fd);
        self.a_r = tau_coeff(1.0, fd);
        self.last_gain = [0.0; H];
        self.last_f = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let fs = self.ctx.fs;
        let fd = fs / DEC as f64;
        let decide_every = (0.1 * fd).round() as usize;
        let n = l.len();
        let mut off = 0;
        while off < n {
            let k = (n - off).min(CONTROL_BLOCK);
            let a = self.a.advance(k);
            // detekcja (na wejściu)
            for i in 0..k {
                let m = 0.5 * (l[off + i] + r[off + i]);
                let y0 = self.dec_lp[0].tick(m);
                let y = self.dec_lp[1].tick(y0);
                self.dec_cnt += 1;
                if self.dec_cnt == DEC {
                    self.dec_cnt = 0;
                    for d in self.dets.iter_mut() {
                        d.tick(y, fd, self.a_iq, self.a_r);
                    }
                    self.tick_cnt += 1;
                    if self.tick_cnt >= decide_every {
                        self.tick_cnt = 0;
                        for d in self.dets.iter_mut() {
                            d.decide(0.1);
                        }
                        // wybór: wykryty brum o większej nadwyżce podstawowej
                        let cand = (0..2).filter(|&j| self.dets[j].hum).max_by(|&x, &y| {
                            self.dets[x].excess[0].partial_cmp(&self.dets[y].excess[0]).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        if cand.is_some() {
                            self.active = cand;
                        }
                    }
                }
            }
            // e_h: 0→1 w 1 s po wykryciu, 1→0 w 3 s po zaniku
            let humming = self.active.map(|j| self.dets[j].hum).unwrap_or(false);
            let dt = k as f64 / fs;
            self.e_h = if humming { (self.e_h + dt / 1.0).min(1.0) } else { (self.e_h - dt / 3.0).max(0.0) };
            if self.e_h <= 0.0 || a <= 0.0 {
                if self.e_h <= 0.0 {
                    self.active = if humming { self.active } else { None };
                }
                // stany filtrów zerowane, żeby ponowne włączenie było czyste.
                // UWAGA: `Svf::default()` ma współczynniki zerowe (m0 = 0 → wyjście 0), dlatego zerujemy tylko stany,
                // a `last_f = 0` wymusza przeliczenie wszystkich dzwonów przy ponownym wejściu (inaczej harmoniczne
                // z |G| ≤ 0,05 dB zostawały z zerowymi współczynnikami i moduł wyciszał cały sygnał).
                if self.last_f != 0.0 {
                    for ch in self.bells.iter_mut() {
                        for b in ch.iter_mut() {
                            b.reset();
                        }
                    }
                    self.last_gain = [0.0; H];
                    self.last_f = 0.0;
                }
                off += k;
                continue;
            }
            let j = self.active.unwrap_or(0);
            let f = self.dets[j].f_tuned();
            let d_h = 30.0 * a;
            // `last_f` = częstotliwość, dla której policzono współczynniki (aktualizowana tylko przy przeliczeniu –
            // inaczej powolny dryf strojenia < 0,01 Hz na blok nigdy nie trafiał do filtrów przy stałym G).
            let f_changed = (f - self.last_f).abs() > 0.01;
            for h in 0..H {
                let g = -self.e_h * d_h.min(self.dets[j].excess[h]);
                if (g - self.last_gain[h]).abs() > 0.05 || f_changed {
                    let fk = (h as f64 + 1.0) * f;
                    let c = if fk < 0.45 * fs { SvfCoeffs::new(fs, fk, 40.0, Mode::Bell(g)) } else { SvfCoeffs::new(fs, 1000.0, 1.0, Mode::Bell(0.0)) };
                    self.bells[0][h].c = c;
                    self.bells[1][h].c = c;
                    self.last_gain[h] = g;
                }
            }
            if f_changed {
                self.last_f = f;
            }
            for i in 0..k {
                let (mut x, mut y) = (l[off + i], r[off + i]);
                for h in 0..H {
                    x = self.bells[0][h].tick(x);
                    y = self.bells[1][h].tick(y);
                }
                l[off + i] = x;
                r[off + i] = y;
            }
            off += k;
        }
        for ch in self.bells.iter_mut() {
            for b in ch.iter_mut() {
                b.flush_denormals();
            }
        }
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        if self.e_h > 0.0 {
            (-self.last_gain[0]).max(0.0) as f32
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn standard() {
        crate::modules::testkit::standard(|c| Box::new(Dehummer::new(c)), ModuleId::Dehummer, 1e-12);
    }
    #[test]
    fn removes_50hz_hum() {
        let fs = 44_100.0;
        let mut m = Dehummer::new(Ctx::new(fs));
        m.set_amount(ModuleId::Dehummer, 1.0);
        m.reset(Ctx::new(fs));
        let n = 44_100 * 8;
        let nz = crate::modules::testkit::noise(n, 0.02, 9);
        let hum = |i: usize| {
            let t = i as f64 / fs;
            0.05 * (2.0 * PI * 50.3 * t).sin() + 0.02 * (2.0 * PI * 100.6 * t).sin() + 0.01 * (2.0 * PI * 150.9 * t).sin()
        };
        let mut l: Vec<f64> = (0..n).map(|i| hum(i) + nz[i]).collect();
        let mut r = l.clone();
        crate::modules::testkit::run(&mut m, &mut l, &mut r);
        let d = &m.dets[0];
        assert!(d.hum, "brum 50 Hz nie wykryty: nadwyżki {:?}, A {:?}, ok_ticks {}", d.excess, d.amp_db, d.ok_ticks);
        // energia przy 50 Hz w ostatniej sekundzie wyraźnie mniejsza
        let tail = (n - 44_100)..n;
        let corr = |x: &[f64]| -> f64 {
            let (mut i, mut q) = (0.0, 0.0);
            for t in tail.clone() {
                let ph = 2.0 * PI * 50.3 * t as f64 / fs;
                i += x[t] * ph.cos();
                q += x[t] * ph.sin();
            }
            (i * i + q * q).sqrt() * 2.0 / 44_100.0
        };
        let before: Vec<f64> = (0..n).map(|i| hum(i) + nz[i]).collect();
        let red = 20.0 * (corr(&l) / corr(&before)).log10();
        assert!(red < -15.0, "brum zredukowany tylko o {red} dB");
    }
    /// Regresja: po zjechaniu suwakiem do 0 i powrocie (brum nadal wykryty) moduł wyciszał cały sygnał,
    /// bo dzwony harmonicznych bez nadwyżki zostawały z zerowymi współczynnikami (`Svf::default()`).
    #[test]
    fn amount_to_zero_and_back_does_not_mute() {
        let fs = 44_100.0;
        let mut m = Dehummer::new(Ctx::new(fs));
        m.set_amount(ModuleId::Dehummer, 1.0);
        m.reset(Ctx::new(fs));
        let n = 44_100 * 11;
        let nz = crate::modules::testkit::noise(n, 0.02, 9);
        let sig = |i: usize| {
            let t = i as f64 / fs;
            0.05 * (2.0 * PI * 50.0 * t).sin() + 0.1 * (2.0 * PI * 1000.0 * t).sin() + nz[i]
        };
        let mut l: Vec<f64> = (0..n).map(sig).collect();
        let mut r = l.clone();
        let (a, b) = (8 * 44_100, 9 * 44_100);
        crate::modules::testkit::run(&mut m, &mut l[..a], &mut r[..a]);
        assert!(m.dets[0].hum && m.e_h > 0.9, "brum niewykryty");
        m.set_amount(ModuleId::Dehummer, 0.0);
        crate::modules::testkit::run(&mut m, &mut l[a..b], &mut r[a..b]);
        m.set_amount(ModuleId::Dehummer, 1.0);
        crate::modules::testkit::run(&mut m, &mut l[b..], &mut r[b..]);
        assert!(m.e_h > 0.0, "test wymaga aktywnego e_h");
        // ton 1 kHz (0,1) musi przejść nietknięty – także tuż po powrocie suwaka
        let tail = (b + 441)..(b + 441 + 8820);
        let (mut i, mut q) = (0.0, 0.0);
        for t in tail.clone() {
            let ph = 2.0 * PI * 1000.0 * t as f64 / fs;
            i += l[t] * ph.cos();
            q += l[t] * ph.sin();
        }
        let amp = (i * i + q * q).sqrt() * 2.0 / tail.len() as f64;
        assert!((amp - 0.1).abs() < 0.01, "ton 1 kHz po powrocie suwaka: {amp} (oczekiwane 0,1)");
    }
}
