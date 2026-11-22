//! Stopień wyjściowy (spec v1 §4.20–23): Pre-limiter → Clipper HD → Prosty clipper → Hard Limit.
//! Wspólny kontekst: sufit C (domyślnie −1,0 dBTP), próg clipperów θ = C·und(−0,3), dosterowanie Clippera HD g_d.

use super::{lin, und};
use crate::catalog::ModuleId;
use crate::dsp::limiter::LookaheadGain;
use crate::dsp::oversample::Oversampler4;
use crate::dsp::softclip::clip_knee;
use crate::dsp::truepeak::TpInterp;
use crate::dsp::util::{tau_coeff, Delay, Smoother};
use crate::module::{Ctx, Module, OutputCtx};

fn theta(o: &OutputCtx) -> f64 {
    und(o.ceiling_db) * und(-0.3)
}

/// Dosterowanie (wzmocnienie liniowe) interpolowane liniowo w obrębie bloku. Wygładzacz suwaka przesuwa się raz
/// na blok (do 256 próbek), więc bez rampy wzmocnienie zmieniało się schodkami do ~1,5 dB co blok („zipper”
/// przy ruchu suwaka / przejściu presetu). Gdy cel = wartość bieżąca, mnożnik jest dokładnie stały (bit-exact).
#[derive(Clone, Copy)]
struct DriveRamp {
    cur: f64,
}
impl DriveRamp {
    fn new(v: f64) -> Self {
        Self { cur: v }
    }
    /// Zwraca (wartość początkową, przyrost na próbkę); próbka i (0-based) dostaje g0 + d·(i + 1).
    #[inline]
    fn begin(&mut self, target: f64, n: usize) -> (f64, f64) {
        let g0 = self.cur;
        self.cur = target;
        (g0, (target - g0) / n.max(1) as f64)
    }
}

// ───────────────────────────── Pre-limiter ─────────────────────────────

/// Limiter 3.10 na szczycie próbkowym (D = 0), L = 64·F, τ0 = 50 ms; cel T = θ·und(X)/g_d – ogranicza głębokość
/// obcinania clippera do X dB. Suwak: X = lin(a, 12, 2) dB.
pub struct PreLimiter {
    ctx: Ctx,
    a: Smoother,
    o: OutputCtx,
    lim: LookaheadGain,
    dl: Delay,
    dr: Delay,
    gr: f32,
}

impl PreLimiter {
    pub fn new(ctx: Ctx) -> Self {
        let l = 64 * ctx.f();
        Self { ctx, a: Smoother::new(0.0, 0.02, ctx.fs), o: OutputCtx::default(), lim: LookaheadGain::new(l, 0.05, ctx.fs), dl: Delay::new(l), dr: Delay::new(l), gr: 0.0 }
    }
}

impl Module for PreLimiter {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::PreLimiter]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn set_output_ctx(&mut self, o: OutputCtx) {
        self.o = o;
    }
    fn latency_samples(&self) -> usize {
        64 * self.ctx.f()
    }
    fn reset(&mut self, ctx: Ctx) {
        let l = 64 * ctx.f();
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        self.lim = LookaheadGain::new(l, 0.05, ctx.fs);
        self.dl = Delay::new(l);
        self.dr = Delay::new(l);
        self.gr = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let a = self.a.advance(l.len());
        let x_db = lin(a, 12.0, 2.0);
        let t = theta(&self.o) * und(x_db) / und(self.o.clipper_drive_db);
        let mut gmin = 1.0f64;
        for i in 0..l.len() {
            let p = l[i].abs().max(r[i].abs());
            let g = self.lim.push(p, t);
            gmin = gmin.min(g);
            l[i] = self.dl.tick(l[i]) * g;
            r[i] = self.dr.tick(r[i]) * g;
        }
        self.gr = (-20.0 * gmin.log10()).max(0.0) as f32;
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        self.gr
    }
}

// ───────────────────────────── Clipper HD ─────────────────────────────

struct BoxAvg {
    buf: Vec<f64>,
    pos: usize,
    sum: f64,
    cnt: usize,
}
impl BoxAvg {
    fn new(n: usize) -> Self {
        Self { buf: vec![0.0; n], pos: 0, sum: 0.0, cnt: 0 }
    }
    #[inline]
    fn push(&mut self, x: f64) -> f64 {
        self.sum += x - self.buf[self.pos];
        self.buf[self.pos] = x;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
        self.cnt += 1;
        if self.cnt >= 4096 {
            self.sum = self.buf.iter().sum();
            self.cnt = 0;
        }
        self.sum / self.buf.len() as f64
    }
}

struct SlidingMax {
    inner: crate::dsp::util::SlidingMin,
}
impl SlidingMax {
    fn new(n: usize) -> Self {
        Self { inner: crate::dsp::util::SlidingMin::new(n) }
    }
    #[inline]
    fn push(&mut self, x: f64) -> f64 {
        -self.inner.push(-x)
    }
}

struct HdChan {
    b1: BoxAvg,
    b2: BoxAvg,
    vdel: Delay,  // v opóźnione o B−1
    lwd: Delay,   // Lw opóźnione o 8F
    hwd: Delay,   // Hw opóźnione o 8F
    emax: SlidingMax,
    e: f64,
    os1: Oversampler4,
    os2: Oversampler4,
}

impl HdChan {
    fn new(f: usize) -> Self {
        let b = 64 * f;
        Self {
            b1: BoxAvg::new(b),
            b2: BoxAvg::new(b),
            vdel: Delay::new(b - 1),
            lwd: Delay::new(8 * f),
            hwd: Delay::new(8 * f),
            emax: SlidingMax::new(72 * f),
            e: 0.0,
            os1: Oversampler4::new(),
            os2: Oversampler4::new(),
        }
    }
}

/// Clipper z „basem robiącym miejsce” (spec v1 §4.21): liniowofazowy podział jądrem trójkątnym (2× średnia ruchoma B = 64),
/// próg basu θ_L = max(θ − μ·e, 0,25θ) zależny od obwiedni reszty pasma, potem 2 przebiegi clip z kolanem na 4×.
/// Suwak: g_d = und(6a). Latencja 63 + 8 + 2×70 = 211 (F = 1).
pub struct ClipperHd {
    ctx: Ctx,
    a: Smoother,
    o: OutputCtx,
    ch: [HdChan; 2],
    a_e: f64,
    drive: DriveRamp,
    gr: f32,
}

const MU: f64 = 0.6;

impl ClipperHd {
    pub fn new(ctx: Ctx) -> Self {
        let f = ctx.f();
        Self { ctx, a: Smoother::new(0.0, 0.02, ctx.fs), o: OutputCtx::default(), ch: [HdChan::new(f), HdChan::new(f)], a_e: tau_coeff(0.0003, ctx.fs), drive: DriveRamp::new(1.0), gr: 0.0 }
    }
}

impl Module for ClipperHd {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::ClipperHd]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn set_output_ctx(&mut self, o: OutputCtx) {
        // dosterowanie bierzemy z własnego suwaka (wygładzanego); z kontekstu tylko sufit
        self.o = o;
    }
    fn latency_samples(&self) -> usize {
        let f = self.ctx.f();
        (64 * f - 1) + 8 * f + 2 * Oversampler4::LATENCY
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        self.ch = [HdChan::new(ctx.f()), HdChan::new(ctx.f())];
        self.a_e = tau_coeff(0.0003, ctx.fs);
        self.drive = DriveRamp::new(und(6.0 * self.a.value));
        self.gr = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let a = self.a.advance(l.len());
        let (g0, dg) = self.drive.begin(und(6.0 * a), l.len());
        let th = theta(&self.o);
        let a_e = self.a_e;
        let mut over = 0.0f64;
        for (buf, ch) in [l, r].into_iter().zip(self.ch.iter_mut()) {
            for (i, x) in buf.iter_mut().enumerate() {
                let v = (g0 + dg * (i + 1) as f64) * *x;
                let lw = ch.b2.push(ch.b1.push(v));
                let hw = ch.vdel.tick(v) - lw;
                // obwiednia |Hw| z okna 72 próbek kończącego się 8 próbek „w przyszłości” względem opóźnionego Lw/Hw
                let m = ch.emax.push(hw.abs());
                ch.e = m + a_e * (ch.e - m);
                let lwd = ch.lwd.tick(lw);
                let hwd = ch.hwd.tick(hw);
                let th_l = (th - MU * ch.e).max(0.25 * th);
                let lc = clip_knee(lwd, th_l, 0.25);
                let v1 = lc + hwd;
                let y1 = ch.os1.process(v1, |u| clip_knee(u, th, 0.06));
                let y2 = ch.os2.process(y1, |u| clip_knee(u, th, 0.06));
                over = over.max(v1.abs() / th);
                *x = y2;
            }
        }
        self.gr = if over > 1.0 { (20.0 * over.log10()) as f32 } else { 0.0 };
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        self.gr
    }
}

// ───────────────────────────── Prosty clipper ─────────────────────────────

/// Jeden przebieg 4×: f(u; θ, w), wejście × g_s. Suwak: g_s = und(6a), w = lin(a, 0,25, 0,10). Latencja 70.
pub struct SimpleClipper {
    ctx: Ctx,
    a: Smoother,
    o: OutputCtx,
    os: [Oversampler4; 2],
    drive: DriveRamp,
    gr: f32,
}

impl SimpleClipper {
    pub fn new(ctx: Ctx) -> Self {
        Self { ctx, a: Smoother::new(0.0, 0.02, ctx.fs), o: OutputCtx::default(), os: [Oversampler4::new(), Oversampler4::new()], drive: DriveRamp::new(1.0), gr: 0.0 }
    }
}

impl Module for SimpleClipper {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::SimpleClipper]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn set_output_ctx(&mut self, o: OutputCtx) {
        self.o = o;
    }
    fn latency_samples(&self) -> usize {
        Oversampler4::LATENCY
    }
    fn reset(&mut self, ctx: Ctx) {
        self.ctx = ctx;
        self.a = Smoother::new(self.a.target(), 0.02, ctx.fs);
        for os in self.os.iter_mut() {
            os.reset(); // bez ponownego projektowania filtrów i alokacji (reset bywa wołany z wątku audio)
        }
        self.drive = DriveRamp::new(und(6.0 * self.a.value));
        self.gr = 0.0;
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let a = self.a.advance(l.len());
        let (g0, dg) = self.drive.begin(und(6.0 * a), l.len());
        let w = lin(a, 0.25, 0.10);
        let th = theta(&self.o);
        let mut over = 0.0f64;
        for (buf, os) in [l, r].into_iter().zip(self.os.iter_mut()) {
            for (i, x) in buf.iter_mut().enumerate() {
                let v = (g0 + dg * (i + 1) as f64) * *x;
                over = over.max(v.abs() / th);
                *x = os.process(v, |u| clip_knee(u, th, w));
            }
        }
        self.gr = if over > 1.0 { (20.0 * over.log10()) as f32 } else { 0.0 };
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        self.gr
    }
}

// ───────────────────────────── Hard Limit ─────────────────────────────

/// Limiter true-peak (spec v1 §4.23): x_d = g_h·x; p = maks. po kanałach szczytu 4× (interpolator §3.6, D_tp = 20);
/// limiter 3.10: T = C·und(−0,10), L = 128·F, τ0 = 30 ms. Bez końcowego clipa próbek.
/// Suwak: g_h = und(6a) (a = 0 – tylko pilnowanie sufitu).
pub struct HardLimit {
    ctx: Ctx,
    a: Smoother,
    o: OutputCtx,
    ipl: TpInterp,
    ipr: TpInterp,
    lim: LookaheadGain,
    dl: Delay,
    dr: Delay,
    drive: DriveRamp,
    gr: f32,
}

impl HardLimit {
    pub fn new(ctx: Ctx) -> Self {
        let l = 128 * ctx.f();
        let d = TpInterp::LATENCY + l;
        Self {
            ctx,
            a: Smoother::new(0.0, 0.02, ctx.fs),
            o: OutputCtx::default(),
            ipl: TpInterp::new(),
            ipr: TpInterp::new(),
            lim: LookaheadGain::new(l, 0.03, ctx.fs),
            dl: Delay::new(d),
            dr: Delay::new(d),
            drive: DriveRamp::new(1.0),
            gr: 0.0,
        }
    }
}

impl Module for HardLimit {
    fn ids(&self) -> &'static [ModuleId] {
        &[ModuleId::HardLimit]
    }
    fn set_amount(&mut self, _id: ModuleId, a: f32) {
        self.a.set_target(a as f64);
    }
    fn set_output_ctx(&mut self, o: OutputCtx) {
        self.o = o;
    }
    fn latency_samples(&self) -> usize {
        TpInterp::LATENCY + 128 * self.ctx.f()
    }
    fn reset(&mut self, ctx: Ctx) {
        let fresh = HardLimit::new(ctx);
        let (a, o) = (self.a.target(), self.o);
        *self = fresh;
        self.a = Smoother::new(a, 0.02, ctx.fs);
        self.o = o;
        self.drive = DriveRamp::new(und(6.0 * a));
    }
    fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let a = self.a.advance(l.len());
        let (g0, dg) = self.drive.begin(und(6.0 * a), l.len());
        let t = und(self.o.ceiling_db) * und(-0.10);
        let mut gmin = 1.0f64;
        for i in 0..l.len() {
            let gh = g0 + dg * (i + 1) as f64;
            let (xl, xr) = (gh * l[i], gh * r[i]);
            let p = self.ipl.push_peak(xl).max(self.ipr.push_peak(xr));
            let g = self.lim.push(p, t);
            gmin = gmin.min(g);
            l[i] = self.dl.tick(xl) * g;
            r[i] = self.dr.tick(xr) * g;
        }
        self.gr = (-20.0 * gmin.log10()).max(0.0) as f32;
    }
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        self.gr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::truepeak::TruePeakMeter;
    use crate::modules::testkit;

    #[test]
    fn hard_limit_never_exceeds_ceiling() {
        let fs = 44_100.0;
        let ctx = Ctx::new(fs);
        let mut m = HardLimit::new(ctx);
        m.set_amount(ModuleId::HardLimit, 1.0);
        m.reset(ctx);
        m.set_output_ctx(OutputCtx { ceiling_db: -1.0, clipper_drive_db: 0.0 });
        let n = 3 * 44_100;
        let nz = testkit::noise(n, 1.0, 21);
        let mut l: Vec<f64> = (0..n).map(|i| 1.5 * (2.0 * std::f64::consts::PI * 11_025.0 * i as f64 / fs + 0.7).sin() * if i % 9000 < 4000 { 1.0 } else { 0.2 }).collect();
        let mut r: Vec<f64> = nz.iter().map(|v| 1.2 * v).collect();
        testkit::run(&mut m, &mut l, &mut r);
        let mut tp = TruePeakMeter::new();
        let peak = tp.process(&l[1000..], &r[1000..]);
        let db = 20.0 * peak.log10();
        assert!(db <= -1.0 + 0.01, "true peak {db} dBTP > sufit");
    }
    #[test]
    fn hard_limit_transparent_below_ceiling() {
        let fs = 44_100.0;
        let ctx = Ctx::new(fs);
        let mut m = HardLimit::new(ctx);
        m.reset(ctx);
        let lat = m.latency_samples();
        let (xl, xr) = testkit::music(20_000, fs);
        let xl: Vec<f64> = xl.iter().map(|v| v * 0.5).collect();
        let xr: Vec<f64> = xr.iter().map(|v| v * 0.5).collect();
        let (mut l, mut r) = (xl.clone(), xr.clone());
        testkit::run(&mut m, &mut l, &mut r);
        for i in lat..l.len() {
            assert_eq!(l[i], xl[i - lat]);
            assert_eq!(r[i], xr[i - lat]);
        }
    }
    #[test]
    fn clipper_hd_bounds_and_transparency() {
        let fs = 44_100.0;
        let ctx = Ctx::new(fs);
        let mut m = ClipperHd::new(ctx);
        m.reset(ctx);
        assert_eq!(m.latency_samples(), 211);
        // cichy sygnał tonalny (bez treści > 20 kHz, którą filtry oversamplingu usuwają): dokładne opóźnienie
        let tone = |i: usize, f: f64| (2.0 * std::f64::consts::PI * f * i as f64 / fs).sin();
        let xl: Vec<f64> = (0..20_000).map(|i| 0.2 * tone(i, 60.0) + 0.1 * tone(i, 1000.0) + 0.05 * tone(i, 9000.0)).collect();
        let xr: Vec<f64> = (0..20_000).map(|i| 0.2 * tone(i, 80.0) + 0.1 * tone(i, 3000.0)).collect();
        let (mut l, mut r) = (xl.clone(), xr.clone());
        testkit::run(&mut m, &mut l, &mut r);
        let mut err = 0.0f64;
        for i in 2000..l.len() {
            err = err.max((l[i] - xl[i - 211]).abs());
        }
        assert!(err < 2e-3, "Clipper HD nieprzezroczysty poniżej progu: {err}");
        // głośny: wyjście ≈ ≤ θ + przerost
        let mut m = ClipperHd::new(ctx);
        m.set_amount(ModuleId::ClipperHd, 1.0);
        m.reset(ctx);
        let (mut l, mut r) = testkit::music(44_100, fs);
        testkit::run(&mut m, &mut l, &mut r);
        let th = und(-1.0) * und(-0.3);
        let pk = l[2000..].iter().chain(r[2000..].iter()).fold(0.0f64, |a, &v| a.max(v.abs()));
        assert!(pk < th * und(1.5), "przerost Clippera HD: {} dB ponad θ", 20.0 * (pk / th).log10());
        assert!(testkit::all_finite(&l));
    }
    #[test]
    fn simple_clipper_and_prelimiter_finite() {
        testkit::standard(|c| Box::new(SimpleClipper::new(c)), ModuleId::SimpleClipper, 0.02);
        testkit::standard(|c| Box::new(PreLimiter::new(c)), ModuleId::PreLimiter, 1e-12);
    }
}
