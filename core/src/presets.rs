//! Własne presety (nie kopiujemy presetów wzorca). Wartości wstępne z spec/koncepcja_filtrow_v1.md §5 (Fable),
//! do strojenia odsłuchem i pomiarami (faza 5).

use crate::catalog::{ModuleId, MODULE_COUNT};

#[derive(Clone, Debug)]
pub struct Preset {
    pub key: &'static str,
    pub name_pl: &'static str,
    /// (włączony, moc 0..1) w kolejności `catalog::MODULES`.
    pub modules: [(bool, f32); MODULE_COUNT],
}

const fn p(on: bool, pct: u8) -> (bool, f32) {
    (on, pct as f32 / 100.0)
}

use ModuleId as M;

const fn build(list: &[(ModuleId, bool, u8)]) -> [(bool, f32); MODULE_COUNT] {
    let mut out = [(false, 0.5f32); MODULE_COUNT];
    let mut i = 0;
    while i < list.len() {
        let (id, on, pct) = list[i];
        out[id as usize] = p(on, pct);
        i += 1;
    }
    out
}

pub const PRESETS: [Preset; 4] = [
    Preset {
        key: "codzienny",
        name_pl: "Codzienny",
        modules: build(&[
            (M::Declipper, true, 40),
            (M::Delossifier, true, 40),
            (M::NoiseReduction, false, 0),
            (M::Dehummer, false, 0),
            (M::Dequantizer, true, 50),
            (M::NoiseGate, false, 0),
            (M::Agc, true, 50),
            (M::Multiband1, true, 30),
            (M::Multiband2, true, 35),
            (M::Compressor, false, 0),
            (M::Punch, true, 25),
            (M::DeepBass, true, 35),
            (M::HarmonicBass, true, 30),
            (M::BassBoost, false, 0),
            (M::WarmBass, true, 50),
            (M::AutoEq, true, 40),
            (M::SpectrumBalance, true, 25),
            (M::Width, true, 20),
            (M::PhaseRotation, false, 0),
            (M::ClipperHd, true, 25),
            (M::PreLimiter, true, 30),
            (M::SimpleClipper, false, 0),
            (M::HardLimit, true, 0),
        ]),
    },
    Preset {
        key: "tramwaj",
        name_pl: "Głośno w tramwaju",
        modules: build(&[
            (M::Declipper, true, 30),
            (M::Delossifier, true, 40),
            (M::NoiseReduction, false, 0),
            (M::Dehummer, false, 0),
            (M::Dequantizer, false, 0),
            (M::NoiseGate, false, 0),
            (M::Agc, true, 90),
            (M::Multiband1, true, 75),
            (M::Multiband2, true, 70),
            (M::Compressor, true, 50),
            (M::Punch, true, 20),
            (M::DeepBass, false, 0),
            (M::HarmonicBass, true, 70),
            (M::BassBoost, true, 30),
            (M::WarmBass, true, 40),
            (M::AutoEq, true, 50),
            (M::SpectrumBalance, true, 40),
            (M::Width, true, 10),
            (M::PhaseRotation, true, 60),
            (M::ClipperHd, true, 75),
            (M::PreLimiter, true, 60),
            (M::SimpleClipper, false, 0),
            (M::HardLimit, true, 30),
        ]),
    },
    Preset {
        key: "stare",
        name_pl: "Stare nagrania",
        modules: build(&[
            (M::Declipper, true, 70),
            (M::Delossifier, true, 60),
            (M::NoiseReduction, true, 55),
            (M::Dehummer, true, 70),
            (M::Dequantizer, true, 70),
            (M::NoiseGate, true, 35),
            (M::Agc, true, 60),
            (M::Multiband1, true, 25),
            (M::Multiband2, true, 45),
            (M::Compressor, false, 0),
            (M::Punch, true, 40),
            (M::DeepBass, true, 30),
            (M::HarmonicBass, true, 45),
            (M::BassBoost, true, 20),
            (M::WarmBass, true, 55),
            (M::AutoEq, true, 80),
            (M::SpectrumBalance, true, 45),
            (M::Width, true, 15),
            (M::PhaseRotation, false, 0),
            (M::ClipperHd, true, 20),
            (M::PreLimiter, true, 30),
            (M::SimpleClipper, false, 0),
            (M::HardLimit, true, 0),
        ]),
    },
    Preset {
        key: "noc",
        name_pl: "Noc",
        modules: build(&[
            (M::Declipper, true, 40),
            (M::Delossifier, true, 40),
            (M::NoiseReduction, false, 0),
            (M::Dehummer, false, 0),
            (M::Dequantizer, true, 60),
            (M::NoiseGate, false, 0),
            (M::Agc, true, 80),
            (M::Multiband1, true, 40),
            (M::Multiband2, true, 60),
            (M::Compressor, true, 60),
            (M::Punch, false, 0),
            (M::DeepBass, false, 0),
            (M::HarmonicBass, true, 30),
            (M::BassBoost, true, 25),
            (M::WarmBass, true, 45),
            (M::AutoEq, true, 40),
            (M::SpectrumBalance, true, 50),
            (M::Width, true, 10),
            (M::PhaseRotation, false, 0),
            (M::ClipperHd, false, 0),
            (M::PreLimiter, true, 50),
            (M::SimpleClipper, true, 20),
            (M::HardLimit, true, 0),
        ]),
    },
];

pub fn by_key(key: &str) -> Option<&'static Preset> {
    PRESETS.iter().find(|p| p.key == key)
}
