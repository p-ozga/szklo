//! Silnik: cały tor + globalny bypass + mierniki. Jedyny typ, którego używają wtyczka, aplikacja PC i JNI.

use crate::catalog::{ModuleId, MODULE_COUNT};
use crate::dsp::loudness::ShortTermLoudness;
use crate::dsp::truepeak::TruePeakMeter;
use crate::dsp::svf::Tpt1;
use crate::graph::{Slot, StereoDelay, CHAIN_ORDER};
use crate::module::{Ctx, OutputCtx, MAX_BLOCK};
use crate::shared::{MeterSnapshot, SharedMeters, SharedParams};
use std::sync::Arc;

pub struct Engine {
    ctx: Ctx,
    slots: Vec<Slot>,
    /// Indeks gniazda w `slots` dla danego `ModuleId`.
    slot_of: [usize; MODULE_COUNT],
    params: Arc<SharedParams>,
    meters: Arc<SharedMeters>,
    seen_version: u64,
    /// DC-blok (HPF 1. rz. 5 Hz) za Dekwantyzerem – spec v1 §2.1.
    dc: [Tpt1; 2],
    dc_after: usize,
    /// DC-blok działa tylko, gdy cokolwiek jest włączone (przenikanie 10 ms) – wszystko wyłączone = bit-exact.
    dc_mix: f64,
    /// Linia wyrównująca do 4096·F próbek (spec v1 §2.2).
    align: StereoDelay,
    out_ctx: OutputCtx,
    /// Bypass globalny: wejście opóźnione o łączną latencję.
    bypass_delay: StereoDelay,
    bypass_mix: f64, // 1 = przetworzone, 0 = oryginał
    bypass_step: f64,
    ceiling_db: f64,
    // Bufory robocze
    l: Vec<f64>,
    r: Vec<f64>,
    bl: Vec<f64>,
    br: Vec<f64>,
    // Mierniki
    lufs: ShortTermLoudness,
    tp: TruePeakMeter,
    tp_hold: Vec<f32>,
    tp_hold_pos: usize,
    tp_hold_acc: f32,
    tp_hold_cnt: usize,
    frames: u64,
    nan_resets: u32,
}

impl Engine {
    pub fn new(fs: f64, params: Arc<SharedParams>, meters: Arc<SharedMeters>) -> Self {
        Self::with_ctx(Ctx::new(fs), params, meters)
    }

    pub fn with_ctx(ctx: Ctx, params: Arc<SharedParams>, meters: Arc<SharedMeters>) -> Self {
        let mut slot_of = [usize::MAX; MODULE_COUNT];
        let slots: Vec<Slot> = CHAIN_ORDER
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let s = Slot::new(crate::modules::create(*id, ctx), ctx);
                for sub in s.ids() {
                    slot_of[sub.index()] = i;
                }
                s
            })
            .collect();
        debug_assert!(slot_of.iter().all(|&i| i != usize::MAX), "każda pozycja UI musi mieć gniazdo");
        let sum: usize = slots.iter().map(|s| s.latency()).sum();
        let target = if ctx.low_latency { 1024 } else { 4096 } * ctx.f();
        let align = target.saturating_sub(sum);
        let total = sum + align;
        let dc_after = CHAIN_ORDER.iter().position(|&m| m == ModuleId::Dequantizer).unwrap_or(0);
        let mut e = Self {
            ctx,
            slots,
            slot_of,
            params,
            meters,
            seen_version: 0,
            dc: [Tpt1::new(ctx.fs, 5.0); 2],
            dc_after,
            dc_mix: 0.0,
            align: StereoDelay::new(align),
            out_ctx: OutputCtx::default(),
            bypass_delay: StereoDelay::new(total),
            bypass_mix: 1.0,
            bypass_step: 1.0 / (0.010 * ctx.fs),
            ceiling_db: -1.0,
            l: vec![0.0; MAX_BLOCK],
            r: vec![0.0; MAX_BLOCK],
            bl: vec![0.0; MAX_BLOCK],
            br: vec![0.0; MAX_BLOCK],
            lufs: ShortTermLoudness::new(ctx.fs),
            tp: TruePeakMeter::new(),
            tp_hold: vec![-120.0; 30],
            tp_hold_pos: 0,
            tp_hold_acc: -120.0,
            tp_hold_cnt: 0,
            frames: 0,
            nan_resets: 0,
        };
        e.sync_params(true);
        e
    }

    pub fn params(&self) -> &Arc<SharedParams> {
        &self.params
    }
    pub fn meters(&self) -> &Arc<SharedMeters> {
        &self.meters
    }
    pub fn sample_rate(&self) -> f64 {
        self.ctx.fs
    }
    /// Łączna latencja toru w próbkach (stała).
    pub fn latency(&self) -> usize {
        self.bypass_delay.len()
    }

    /// Zmiana częstotliwości próbkowania / trybu – pełny reset.
    pub fn reset(&mut self, ctx: Ctx) {
        let params = self.params.clone();
        let meters = self.meters.clone();
        *self = Self::with_ctx(ctx, params, meters);
    }

    /// Render offline: stany modułów ustawiane natychmiast (bez rozgrzewania i przenikania).
    pub fn force_current_state(&mut self) {
        self.sync_params(true);
        for s in self.slots.iter_mut() {
            s.force();
        }
        self.dc_mix = if crate::catalog::MODULES.iter().any(|m| self.params.module(m.id.index()).0) { 1.0 } else { 0.0 };
        self.bypass_mix = if self.params.enabled() && !self.params.compare() { 1.0 } else { 0.0 };
    }

    fn sync_params(&mut self, force: bool) {
        let v = self.params.version();
        if !force && v == self.seen_version {
            return;
        }
        self.seen_version = v;
        let master = self.params.master();
        let ceiling = self.params.ceiling_dbtp() as f64;
        for m in crate::catalog::MODULES.iter() {
            let id = m.id;
            let (on, amount) = self.params.module(id.index());
            let slot = &mut self.slots[self.slot_of[id.index()]];
            // Moc ogólna nie dotyczy Hard Limit (bezpieczeństwo, a = dosterowanie).
            let eff = if id == ModuleId::HardLimit { amount } else { amount * master };
            slot.set_amount(id, eff);
            slot.set_on(id, on);
        }
        let (c_on, c_a) = self.params.module(ModuleId::ClipperHd.index());
        let o = OutputCtx { ceiling_db: ceiling, clipper_drive_db: if c_on { 6.0 * (c_a * master) as f64 } else { 0.0 } };
        if force || o != self.out_ctx {
            self.out_ctx = o;
            for s in self.slots.iter_mut() {
                s.set_output_ctx(o);
            }
        }
        self.ceiling_db = ceiling;
    }

    /// Przetwarzanie w miejscu (kanały rozdzielone, f32).
    pub fn process(&mut self, left: &mut [f32], right: &mut [f32]) {
        let n = left.len().min(right.len());
        let mut off = 0;
        while off < n {
            let m = (n - off).min(MAX_BLOCK);
            for i in 0..m {
                self.l[i] = sanitize(left[off + i]);
                self.r[i] = sanitize(right[off + i]);
            }
            self.process_block(m);
            for i in 0..m {
                left[off + i] = self.l[i] as f32;
                right[off + i] = self.r[i] as f32;
            }
            off += m;
        }
    }

    /// Przetwarzanie w miejscu, przeplot L R L R…, f32. Mono/wielokanał: przetwarzane są 2 pierwsze kanały.
    pub fn process_interleaved(&mut self, buf: &mut [f32], channels: usize) {
        let ch = channels.max(1);
        let frames = buf.len() / ch;
        let mut off = 0;
        while off < frames {
            let m = (frames - off).min(MAX_BLOCK);
            for i in 0..m {
                let b = (off + i) * ch;
                self.l[i] = sanitize(buf[b]);
                self.r[i] = sanitize(if ch > 1 { buf[b + 1] } else { buf[b] });
            }
            self.process_block(m);
            for i in 0..m {
                let b = (off + i) * ch;
                buf[b] = self.l[i] as f32;
                if ch > 1 {
                    buf[b + 1] = self.r[i] as f32;
                }
            }
            off += m;
        }
    }

    fn process_block(&mut self, n: usize) {
        self.sync_params(false);
        let in_peak = peak(&self.l[..n], &self.r[..n]);

        // Wejście opóźnione dla bypassu globalnego.
        {
            let (bl, br) = (&mut self.bl[..n], &mut self.br[..n]);
            self.bypass_delay.process(&self.l[..n], &self.r[..n], bl, br);
        }

        let want = self.params.enabled() && !self.params.compare();
        // Tor liczy zawsze (także przy bypassie): stany filtrów i obwiedni pozostają aktualne,
        // więc „Porównaj z oryginałem” przełącza bez trzasków i bez ponownego rozgrzewania.
        {
            let (l, r) = (&mut self.l[..n], &mut self.r[..n]);
            for (i, s) in self.slots.iter_mut().enumerate() {
                s.process(l, r);
                if i == self.dc_after {
                    let any_on = crate::catalog::MODULES.iter().any(|m| self.params.module(m.id.index()).0);
                    let step = self.bypass_step;
                    for k in 0..n {
                        let (hl, hr) = (self.dc[0].hp(l[k]), self.dc[1].hp(r[k]));
                        self.dc_mix = if any_on { (self.dc_mix + step).min(1.0) } else { (self.dc_mix - step).max(0.0) };
                        if self.dc_mix >= 1.0 {
                            l[k] = hl;
                            r[k] = hr;
                        } else if self.dc_mix > 0.0 {
                            l[k] += self.dc_mix * (hl - l[k]);
                            r[k] += self.dc_mix * (hr - r[k]);
                        }
                    }
                    self.dc[0].flush_denormals();
                    self.dc[1].flush_denormals();
                }
            }
            self.align.process_inplace(l, r);
        }

        // Bezpiecznik: NaN/Inf w torze (np. niestabilność numeryczna modułu) zostałby w stanach filtrów na zawsze
        // i trafił do urządzenia. Wtedy: ten blok = oryginał opóźniony, a stany całego toru są odtwarzane od zera.
        if !self.l[..n].iter().chain(self.r[..n].iter()).all(|v| v.is_finite()) {
            self.recover_from_nan();
            self.l[..n].copy_from_slice(&self.bl[..n]);
            self.r[..n].copy_from_slice(&self.br[..n]);
        }

        if !(want && self.bypass_mix >= 1.0) {
            let dir = if want { self.bypass_step } else { -self.bypass_step };
            for i in 0..n {
                self.bypass_mix = (self.bypass_mix + dir).clamp(0.0, 1.0);
                let m = self.bypass_mix;
                if m <= 0.0 {
                    // pełny bypass = dokładnie oryginał (bit-exact)
                    self.l[i] = self.bl[i];
                    self.r[i] = self.br[i];
                } else {
                    self.l[i] = self.bl[i] + m * (self.l[i] - self.bl[i]);
                    self.r[i] = self.br[i] + m * (self.r[i] - self.br[i]);
                }
            }
        }

        self.meters.scope.push(&self.l[..n], &self.r[..n]);

        // Mierniki
        let out_peak = peak(&self.l[..n], &self.r[..n]);
        self.lufs.process(&self.l[..n], &self.r[..n]);
        let tp_block = self.tp.process(&self.l[..n], &self.r[..n]);
        self.update_tp_hold(tp_block, n);
        self.frames += n as u64;

        let mut module_gr = [0f32; MODULE_COUNT];
        let mut gr_total = 0f32;
        for m in crate::catalog::MODULES.iter() {
            let s = &self.slots[self.slot_of[m.id.index()]];
            if s.is_active() {
                let g = s.module.gain_reduction_db(m.id).max(0.0);
                module_gr[m.id.index()] = g;
                gr_total += g;
            }
        }
        if self.bypass_mix <= 0.0 {
            gr_total = 0.0;
        }
        let snap = MeterSnapshot {
            in_peak_db: lin_to_db(in_peak),
            out_peak_db: lin_to_db(out_peak),
            gr_db: gr_total,
            lufs_short: self.lufs.lufs() as f32,
            true_peak_db: self.tp_hold.iter().copied().fold(self.tp_hold_acc, f32::max),
            frames: self.frames,
        };
        self.meters.publish(&snap, &module_gr);
    }

    /// Awaryjny reset stanów toru (wywoływany tylko po wykryciu NaN/Inf; alokuje – sytuacja wyjątkowa).
    #[cold]
    fn recover_from_nan(&mut self) {
        for s in self.slots.iter_mut() {
            s.recover();
        }
        self.dc = [Tpt1::new(self.ctx.fs, 5.0); 2];
        self.align.clear();
        self.nan_resets = self.nan_resets.wrapping_add(1);
    }

    /// Ile razy zadziałał bezpiecznik NaN/Inf (diagnostyka; 0 w poprawnie działającym torze).
    pub fn nan_resets(&self) -> u32 {
        self.nan_resets
    }

    /// Szczyt true-peak z ostatnich 3 s (30 kubełków po 100 ms).
    fn update_tp_hold(&mut self, tp_lin: f64, n: usize) {
        self.tp_hold_acc = self.tp_hold_acc.max(lin_to_db(tp_lin));
        self.tp_hold_cnt += n;
        let bucket = (0.1 * self.ctx.fs) as usize;
        if self.tp_hold_cnt >= bucket {
            self.tp_hold[self.tp_hold_pos] = self.tp_hold_acc;
            self.tp_hold_pos = (self.tp_hold_pos + 1) % self.tp_hold.len();
            self.tp_hold_acc = -120.0;
            self.tp_hold_cnt = 0;
        }
    }
}

#[inline]
fn sanitize(x: f32) -> f64 {
    if x.is_finite() {
        x as f64
    } else {
        0.0
    }
}

fn peak(l: &[f64], r: &[f64]) -> f64 {
    l.iter().chain(r.iter()).fold(0.0f64, |a, &x| a.max(x.abs()))
}

pub fn lin_to_db(x: f64) -> f32 {
    if x <= 1e-6 {
        -120.0
    } else {
        (20.0 * x.log10()) as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dsp::truepeak::TruePeakMeter;

    fn music(n: usize, fs: f64) -> (Vec<f32>, Vec<f32>) {
        let (l, r) = crate::modules::testkit::music(n, fs);
        (l.iter().map(|&v| (v * 0.9) as f32).collect(), r.iter().map(|&v| (v * 0.9) as f32).collect())
    }

    #[test]
    fn latency_is_4096_at_44k1_and_48k_and_8192_at_96k() {
        for (fs, want) in [(44_100.0, 4096), (48_000.0, 4096), (96_000.0, 8192)] {
            let (e, _, _) = crate::new_engine(fs);
            assert_eq!(e.latency(), want, "fs {fs}");
        }
    }

    #[test]
    fn all_off_is_bit_exact_delay() {
        let fs = 44_100.0;
        let (mut e, p, _) = crate::new_engine(fs);
        for m in crate::MODULES.iter() {
            p.set_module_on(m.id.index(), false);
        }
        e.force_current_state();
        let lat = e.latency();
        let n = 20_000;
        let (xl, xr) = music(n, fs);
        let (mut l, mut r) = (xl.clone(), xr.clone());
        e.process(&mut l, &mut r);
        let mut max = 0.0f32;
        for i in lat + 5000..n {
            max = max.max((l[i] - xl[i - lat]).abs()).max((r[i] - xr[i - lat]).abs());
        }
        assert_eq!(max, 0.0, "wszystko wyłączone ≠ opóźnienie (bit-exact): {max}");
    }

    #[test]
    fn presets_are_finite_and_respect_ceiling() {
        let fs = 44_100.0;
        for preset in crate::PRESETS.iter() {
            let (mut e, p, _) = crate::new_engine(fs);
            p.apply_preset(preset);
            e.force_current_state();
            let n = 44_100 * 4;
            let (mut l, mut r) = music(n, fs);
            for (cl, cr) in l.chunks_mut(441).zip(r.chunks_mut(441)) {
                e.process(cl, cr);
            }
            assert!(l.iter().chain(r.iter()).all(|v| v.is_finite()), "{}: NaN", preset.key);
            let lf: Vec<f64> = l[e.latency() + 1000..].iter().map(|&v| v as f64).collect();
            let rf: Vec<f64> = r[e.latency() + 1000..].iter().map(|&v| v as f64).collect();
            let tp = 20.0 * TruePeakMeter::new().process(&lf, &rf).log10();
            assert!(tp <= -1.0 + 0.02, "{}: true peak {tp} dBTP", preset.key);
        }
    }

    #[test]
    fn toggling_modules_live_has_no_clicks() {
        let fs = 44_100.0;
        let (mut e, p, _) = crate::new_engine(fs);
        p.apply_preset(&crate::PRESETS[0]);
        e.force_current_state();
        let n = 44_100 * 6;
        let tone: Vec<f32> = (0..n).map(|i| (0.3 * (2.0 * std::f64::consts::PI * 220.0 * i as f64 / fs).sin()) as f32).collect();
        let (mut l, mut r) = (tone.clone(), tone.clone());
        let mut k = 0usize;
        for (b, (cl, cr)) in l.chunks_mut(512).zip(r.chunks_mut(512)).enumerate() {
            // co ~200 ms przełącz kolejny moduł
            if b % 17 == 0 {
                let id = crate::MODULES[k % crate::MODULE_COUNT].id;
                let (on, a) = p.module(id.index());
                if id != crate::ModuleId::HardLimit {
                    p.set_module(id.index(), !on, if a > 0.0 { a } else { 0.5 });
                }
                k += 1;
            }
            e.process(cl, cr);
        }
        let step_in = tone.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        let step_out = l[e.latency()..].windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0f32, f32::max);
        assert!(step_out < 6.0 * step_in, "trzask przy przełączaniu: {step_out} vs {step_in}");
        assert_eq!(e.nan_resets(), 0);
    }

    /// Globalny bypass („Wyłączone”) = dokładnie wejście opóźnione o latencję, a w żadnym presecie
    /// nie działa bezpiecznik NaN.
    #[test]
    fn global_bypass_is_bit_exact_and_no_nan_resets() {
        let fs = 48_000.0;
        let (mut e, p, _) = crate::new_engine(fs);
        p.apply_preset(&crate::PRESETS[1]);
        p.set_enabled(false);
        e.force_current_state();
        let lat = e.latency();
        let n = 30_000;
        let (xl, xr) = music(n, fs);
        let (mut l, mut r) = (xl.clone(), xr.clone());
        for (cl, cr) in l.chunks_mut(480).zip(r.chunks_mut(480)) {
            e.process(cl, cr);
        }
        for i in lat..n {
            assert_eq!(l[i], xl[i - lat]);
            assert_eq!(r[i], xr[i - lat]);
        }
        assert_eq!(e.nan_resets(), 0);
    }

    /// Nietypowe częstotliwości próbkowania z urządzeń Windows (zestaw głośnomówiący BT 16 kHz, 32 kHz, 192 kHz):
    /// silnik nie może panikować ani produkować NaN.
    #[test]
    fn unusual_sample_rates_do_not_panic() {
        for fs in [8_000.0, 11_025.0, 16_000.0, 22_050.0, 32_000.0, 88_200.0, 176_400.0, 192_000.0] {
            for preset in crate::PRESETS.iter() {
                let (mut e, p, _) = crate::new_engine(fs);
                p.apply_preset(preset);
                for m in crate::MODULES.iter() {
                    p.set_module(m.id.index(), true, 0.8);
                }
                e.force_current_state();
                let n = (fs * 1.5) as usize;
                let (mut l, mut r) = music(n, fs);
                for (cl, cr) in l.chunks_mut(441).zip(r.chunks_mut(441)) {
                    e.process(cl, cr);
                }
                assert!(l.iter().chain(r.iter()).all(|v| v.is_finite()), "fs {fs}: NaN");
                assert_eq!(e.nan_resets(), 0, "fs {fs}");
            }
        }
    }
}
