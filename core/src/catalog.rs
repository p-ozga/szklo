//! Katalog modułów: identyfikatory, nazwy w UI (sekcja 11.4 scenariusza) i grupy.
//! Kolejność w tej tablicy = kolejność w UI, NIE kolejność w torze (tę definiuje `graph::CHAIN_ORDER`).

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ModuleId {
    Declipper = 0,
    Delossifier,
    NoiseReduction,
    Dehummer,
    Dequantizer,
    NoiseGate,
    Agc,
    Multiband1,
    Multiband2,
    Compressor,
    Punch,
    DeepBass,
    HarmonicBass,
    BassBoost,
    WarmBass,
    AutoEq,
    SpectrumBalance,
    Width,
    PhaseRotation,
    ClipperHd,
    PreLimiter,
    SimpleClipper,
    HardLimit,
    /// dodany 2026-11-24 – na końcu, żeby indeksy zapisanych ustawień się nie przesunęły
    Space,
    /// dodany 2026-11-24
    RadioBass,
    /// dodane 2026-11-25
    Crystal,
    DynamicTreble,
    Binaural,
    Hood,
    /// dodane 2026-11-25 (filtry skupienia)
    LessWords,
    EvenStream,
    Bed,
}

pub const MODULE_COUNT: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Group {
    Naprawa,
    Dynamika,
    Bas,
    Barwa,
    Stereo,
    Wyjscie,
}

impl Group {
    pub const ALL: [Group; 6] = [Group::Naprawa, Group::Dynamika, Group::Bas, Group::Barwa, Group::Stereo, Group::Wyjscie];
    pub fn name_pl(self) -> &'static str {
        match self {
            Group::Naprawa => "Naprawa",
            Group::Dynamika => "Dynamika",
            Group::Bas => "Bas",
            Group::Barwa => "Barwa",
            Group::Stereo => "Stereo",
            Group::Wyjscie => "Wyjście",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ModuleInfo {
    pub id: ModuleId,
    /// Nazwa w UI (polska, własna – bez nazw własnych wzorca).
    pub name_pl: &'static str,
    /// Stały identyfikator ASCII (pliki presetów, IPC, JNI, parametry VST).
    pub key: &'static str,
    pub group: Group,
}

pub const MODULES: [ModuleInfo; MODULE_COUNT] = [
    ModuleInfo { id: ModuleId::Declipper, name_pl: "Declipper", key: "declipper", group: Group::Naprawa },
    ModuleInfo { id: ModuleId::Delossifier, name_pl: "Naprawa MP3", key: "delossifier", group: Group::Naprawa },
    ModuleInfo { id: ModuleId::NoiseReduction, name_pl: "Szum i brum", key: "noise_reduction", group: Group::Naprawa },
    ModuleInfo { id: ModuleId::Dehummer, name_pl: "Dehummer", key: "dehummer", group: Group::Naprawa },
    ModuleInfo { id: ModuleId::Dequantizer, name_pl: "Dekwantyzer", key: "dequantizer", group: Group::Naprawa },
    ModuleInfo { id: ModuleId::NoiseGate, name_pl: "Bramka szumów", key: "noise_gate", group: Group::Naprawa },
    ModuleInfo { id: ModuleId::Agc, name_pl: "AGC", key: "agc", group: Group::Dynamika },
    ModuleInfo { id: ModuleId::Multiband1, name_pl: "Multiband 1", key: "multiband1", group: Group::Dynamika },
    ModuleInfo { id: ModuleId::Multiband2, name_pl: "Multiband 2", key: "multiband2", group: Group::Dynamika },
    ModuleInfo { id: ModuleId::Compressor, name_pl: "Kompresor", key: "compressor", group: Group::Dynamika },
    ModuleInfo { id: ModuleId::Punch, name_pl: "Punch", key: "punch", group: Group::Dynamika },
    ModuleInfo { id: ModuleId::DeepBass, name_pl: "Głęboki bas", key: "deep_bass", group: Group::Bas },
    ModuleInfo { id: ModuleId::HarmonicBass, name_pl: "Bas harmoniczny", key: "harmonic_bass", group: Group::Bas },
    ModuleInfo { id: ModuleId::BassBoost, name_pl: "Bass Boost", key: "bass_boost", group: Group::Bas },
    ModuleInfo { id: ModuleId::WarmBass, name_pl: "Ciepły bas", key: "warm_bass", group: Group::Bas },
    ModuleInfo { id: ModuleId::AutoEq, name_pl: "Auto EQ", key: "auto_eq", group: Group::Barwa },
    ModuleInfo { id: ModuleId::SpectrumBalance, name_pl: "Balans widma", key: "spectrum_balance", group: Group::Barwa },
    ModuleInfo { id: ModuleId::Width, name_pl: "Szerokość", key: "width", group: Group::Stereo },
    ModuleInfo { id: ModuleId::PhaseRotation, name_pl: "Rotacja fazy", key: "phase_rotation", group: Group::Stereo },
    ModuleInfo { id: ModuleId::ClipperHd, name_pl: "Clipper HD", key: "clipper_hd", group: Group::Wyjscie },
    ModuleInfo { id: ModuleId::PreLimiter, name_pl: "Pre-limiter", key: "pre_limiter", group: Group::Wyjscie },
    ModuleInfo { id: ModuleId::SimpleClipper, name_pl: "Prosty clipper", key: "simple_clipper", group: Group::Wyjscie },
    ModuleInfo { id: ModuleId::HardLimit, name_pl: "Hard Limit", key: "hard_limit", group: Group::Wyjscie },
    ModuleInfo { id: ModuleId::Space, name_pl: "Przestrzeń", key: "space", group: Group::Stereo },
    ModuleInfo { id: ModuleId::RadioBass, name_pl: "Radiowy bas", key: "radio_bass", group: Group::Bas },
    ModuleInfo { id: ModuleId::Crystal, name_pl: "Kryształ", key: "crystal", group: Group::Barwa },
    ModuleInfo { id: ModuleId::DynamicTreble, name_pl: "Dynamiczna góra", key: "dynamic_treble", group: Group::Barwa },
    ModuleInfo { id: ModuleId::Binaural, name_pl: "Binauralny", key: "binaural", group: Group::Stereo },
    ModuleInfo { id: ModuleId::Hood, name_pl: "Kaptur", key: "hood", group: Group::Stereo },
    ModuleInfo { id: ModuleId::LessWords, name_pl: "Mniej słów", key: "less_words", group: Group::Stereo },
    ModuleInfo { id: ModuleId::EvenStream, name_pl: "Równy strumień", key: "even_stream", group: Group::Dynamika },
    ModuleInfo { id: ModuleId::Bed, name_pl: "Szum tła", key: "bed", group: Group::Barwa },
];

impl ModuleId {
    pub fn index(self) -> usize {
        self as usize
    }
    pub fn from_index(i: usize) -> Option<ModuleId> {
        MODULES.get(i).map(|m| m.id)
    }
    pub fn from_key(key: &str) -> Option<ModuleId> {
        MODULES.iter().find(|m| m.key == key).map(|m| m.id)
    }
    pub fn info(self) -> &'static ModuleInfo {
        &MODULES[self.index()]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_indices_match() {
        for (i, m) in MODULES.iter().enumerate() {
            assert_eq!(m.id.index(), i, "{}", m.key);
            assert_eq!(ModuleId::from_key(m.key), Some(m.id));
        }
    }
}
