//! Fabryka modułów (jeden plik = jeden moduł albo jedna szyna STFT). Specyfikacja: spec/koncepcja_filtrow_v1.md §4.
use crate::catalog::ModuleId;
use crate::module::{Ctx, Module, CONTROL_BLOCK};

pub mod agc;
pub mod bass;
pub mod bus_a;
pub mod bus_b;
pub mod declipper;
pub mod dehummer;
pub mod dequantizer;
pub mod dynamics;
pub mod noise_gate;
pub mod output;
pub mod phase_rotation;
pub mod punch;
pub mod warm_bass;
pub mod width;

pub fn create(id: ModuleId, ctx: Ctx) -> Box<dyn Module> {
    use ModuleId as M;
    match id {
        M::Declipper => Box::new(declipper::Declipper::new(ctx)),
        M::Dequantizer => Box::new(dequantizer::Dequantizer::new(ctx)),
        M::Dehummer => Box::new(dehummer::Dehummer::new(ctx)),
        M::NoiseReduction | M::Delossifier => Box::new(bus_a::BusA::new(ctx)),
        M::NoiseGate => Box::new(noise_gate::NoiseGate::new(ctx)),
        M::Agc => Box::new(agc::Agc::new(ctx)),
        M::WarmBass => Box::new(warm_bass::WarmBass::new(ctx)),
        M::AutoEq | M::SpectrumBalance => Box::new(bus_b::BusB::new(ctx)),
        M::Width => Box::new(width::Width::new(ctx)),
        M::DeepBass => Box::new(bass::DeepBass::new(ctx)),
        M::BassBoost => Box::new(bass::BassBoost::new(ctx)),
        M::HarmonicBass => Box::new(bass::HarmonicBass::new(ctx)),
        M::Multiband2 => Box::new(dynamics::MultibandComp::multiband2(ctx)),
        M::Compressor => Box::new(dynamics::MultibandComp::wideband(ctx)),
        M::Multiband1 => Box::new(dynamics::MultibandComp::multiband1(ctx)),
        M::Punch => Box::new(punch::Punch::new(ctx)),
        M::PhaseRotation => Box::new(phase_rotation::PhaseRotation::new(ctx)),
        M::PreLimiter => Box::new(output::PreLimiter::new(ctx)),
        M::ClipperHd => Box::new(output::ClipperHd::new(ctx)),
        M::SimpleClipper => Box::new(output::SimpleClipper::new(ctx)),
        M::HardLimit => Box::new(output::HardLimit::new(ctx)),
    }
}

/// Wywołuje `f` na kolejnych kawałkach bloku o długości ≤ `CONTROL_BLOCK` (tempo sterowania, spec v1 §0).
#[inline]
pub(crate) fn control_chunks(l: &mut [f64], r: &mut [f64], mut f: impl FnMut(&mut [f64], &mut [f64])) {
    for (cl, cr) in l.chunks_mut(CONTROL_BLOCK).zip(r.chunks_mut(CONTROL_BLOCK)) {
        f(cl, cr);
    }
}

/// Funkcje mapowania suwaka (spec v1 §0).
#[inline]
pub(crate) fn lin(a: f64, lo: f64, hi: f64) -> f64 {
    lo + (hi - lo) * a
}
#[inline]
pub(crate) fn expo(a: f64, lo: f64, hi: f64) -> f64 {
    lo * (hi / lo).powf(a)
}
#[inline]
pub(crate) fn sstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}
#[inline]
pub(crate) fn und(db: f64) -> f64 {
    10f64.powf(db / 20.0)
}

/// Rampa wzmocnienia liniowego w obrębie bloku sterującego.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GainRamp {
    pub cur: f64,
}
impl Default for GainRamp {
    fn default() -> Self {
        Self { cur: 1.0 }
    }
}
impl GainRamp {
    /// Mnoży próbki przez wzmocnienie narastające liniowo od bieżącego do `target`.
    #[inline]
    pub fn apply(&mut self, target: f64, l: &mut [f64], r: &mut [f64]) {
        let n = l.len() as f64;
        let d = (target - self.cur) / n;
        let mut g = self.cur;
        for i in 0..l.len() {
            g += d;
            l[i] *= g;
            r[i] *= g;
        }
        self.cur = target;
    }
}

/// Wspólny test zachowania modułów: brak NaN, a = 0 ≈ przezroczyste, ciągłość przy przemiataniu suwaka.
#[cfg(test)]
pub(crate) mod testkit {
    use super::*;
    use crate::module::MAX_BLOCK;

    pub fn run(m: &mut dyn Module, l: &mut [f64], r: &mut [f64]) {
        for (cl, cr) in l.chunks_mut(MAX_BLOCK).zip(r.chunks_mut(MAX_BLOCK)) {
            m.process(cl, cr);
        }
    }
    pub fn noise(n: usize, amp: f64, seed: u64) -> Vec<f64> {
        let mut s = seed.wrapping_add(0x9E3779B97F4A7C15);
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                amp * (((s >> 11) as f64 / (1u64 << 53) as f64) * 2.0 - 1.0)
            })
            .collect()
    }
    /// Sygnał „muzyczny”: bas 60 Hz + 1 kHz + szum, z modulacją głośności.
    pub fn music(n: usize, fs: f64) -> (Vec<f64>, Vec<f64>) {
        let nz = noise(n, 0.05, 3);
        let nz2 = noise(n, 0.05, 4);
        let l = (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                let env = 0.5 + 0.5 * (2.0 * std::f64::consts::PI * 0.7 * t).sin().abs();
                env * (0.4 * (2.0 * std::f64::consts::PI * 60.0 * t).sin() + 0.2 * (2.0 * std::f64::consts::PI * 1000.0 * t).sin()) + nz[i]
            })
            .collect();
        let r = (0..n)
            .map(|i| {
                let t = i as f64 / fs;
                0.35 * (2.0 * std::f64::consts::PI * 60.0 * t + 0.3).sin() + 0.15 * (2.0 * std::f64::consts::PI * 1500.0 * t).sin() + nz2[i]
            })
            .collect();
        (l, r)
    }
    /// Maks. skok między próbkami ponad skok sygnału wejściowego (detektor nieciągłości).
    pub fn max_step(y: &[f64]) -> f64 {
        y.windows(2).map(|w| (w[1] - w[0]).abs()).fold(0.0, f64::max)
    }
    pub fn all_finite(y: &[f64]) -> bool {
        y.iter().all(|v| v.is_finite())
    }
    /// Standardowy zestaw testów dla modułu.
    pub fn standard(make: impl Fn(Ctx) -> Box<dyn Module>, id: ModuleId, transparent_tol: f64) {
        let fs = 44_100.0;
        let ctx = Ctx::new(fs);
        let n = 3 * 44_100;
        let (xl, xr) = music(n, fs);
        // 1) a = 0 → przezroczyste (z dokładnością do tolerancji, po opóźnieniu)
        let mut m = make(ctx);
        m.set_amount(id, 0.0);
        m.set_sub_enabled(id, true);
        m.reset(ctx);
        let lat = m.latency_samples();
        let (mut l, mut r) = (xl.clone(), xr.clone());
        run(m.as_mut(), &mut l, &mut r);
        let mut err = 0.0f64;
        for i in (lat + 30_000)..n {
            err = err.max((l[i] - xl[i - lat]).abs()).max((r[i] - xr[i - lat]).abs());
        }
        assert!(err <= transparent_tol, "{id:?}: a=0 nie jest przezroczyste, błąd {err}");
        // 2) przemiatanie 0→1 w 2 s – brak NaN i trzasków (skok ≤ 3× maks. skoku wejścia + zapas)
        let mut m = make(ctx);
        m.set_amount(id, 0.0);
        m.set_sub_enabled(id, true);
        m.reset(ctx);
        let (mut l, mut r) = (xl.clone(), xr.clone());
        let blocks = n / MAX_BLOCK;
        for (b, (cl, cr)) in l.chunks_mut(MAX_BLOCK).zip(r.chunks_mut(MAX_BLOCK)).enumerate() {
            let a = (b as f64 / (blocks as f64 * 2.0 / 3.0)).min(1.0);
            m.set_amount(id, a as f32);
            m.process(cl, cr);
        }
        assert!(all_finite(&l) && all_finite(&r), "{id:?}: NaN/Inf");
        let step_in = max_step(&xl).max(max_step(&xr));
        let step_out = max_step(&l[lat + 2000..]).max(max_step(&r[lat + 2000..]));
        assert!(step_out < 4.0 * step_in + 0.05, "{id:?}: możliwy trzask: skok {step_out} vs wejście {step_in}");
        // 3) sygnały skrajne: cisza, DC, impuls, +12 dBFS
        for kind in 0..4 {
            let mut m = make(ctx);
            m.set_amount(id, 1.0);
            m.set_sub_enabled(id, true);
            m.reset(ctx);
            let mut l: Vec<f64> = (0..20_000)
                .map(|i| match kind {
                    0 => 0.0,
                    1 => 0.5,
                    2 => if i == 1000 { 1.0 } else { 0.0 },
                    _ => 4.0 * (i as f64 * 0.05).sin(),
                })
                .collect();
            let mut r = l.clone();
            run(m.as_mut(), &mut l, &mut r);
            assert!(all_finite(&l) && all_finite(&r), "{id:?}: NaN dla sygnału {kind}");
        }
    }
}
