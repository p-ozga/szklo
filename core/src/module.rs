//! Wspólny interfejs modułów DSP.
//!
//! Różnice względem szkicu ze scenariusza (sekcja 4), zgodnie ze spec v1 §0:
//! - on/off obsługuje tor (`graph::Slot`): neutralizacja (moc → 0 przez 100 ms; dla wszystkich modułów a = 0 jest
//!   przezroczyste albo all-pass), potem przenikanie 10 ms; wyłączony = linia opóźniająca (bit-exact);
//! - `process` pracuje na f64 (cały tor w f64; f32 tylko na wejściu/wyjściu silnika);
//! - jeden obiekt może obsługiwać kilka pozycji UI (szyny STFT: Szum+Naprawa MP3, Auto EQ+Balans widma) –
//!   stąd `ids()` i `set_sub_enabled`.

use crate::catalog::ModuleId;

/// Konfiguracja wspólna dla wszystkich modułów.
#[derive(Clone, Copy, Debug)]
pub struct Ctx {
    pub fs: f64,
    /// Tryb niskiej latencji (wideo) – zarys w spec v1 §2.2; na razie tylko flaga.
    pub low_latency: bool,
}

impl Ctx {
    pub fn new(fs: f64) -> Self {
        Self { fs, low_latency: false }
    }
    /// Mnożnik rodziny częstotliwości: 1 dla 44,1/48 kHz, 2 dla 88,2/96 kHz (spec v1 §0).
    pub fn f(&self) -> usize {
        if self.fs > 60_000.0 {
            2
        } else {
            1
        }
    }
    pub fn ms(&self, ms: f64) -> usize {
        (ms * 1e-3 * self.fs).round().max(1.0) as usize
    }
}

/// Kontekst stopnia wyjściowego (spec v1 §4 „Wyjście”): sufit C i dosterowanie Clippera HD g_d.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutputCtx {
    /// Sufit, dBTP.
    pub ceiling_db: f64,
    /// Dosterowanie Clippera HD w dB (0, gdy wyłączony).
    pub clipper_drive_db: f64,
}

impl Default for OutputCtx {
    fn default() -> Self {
        Self { ceiling_db: -1.0, clipper_drive_db: 0.0 }
    }
}

pub trait Module: Send {
    /// Pozycje UI obsługiwane przez ten obiekt (zwykle jedna).
    fn ids(&self) -> &'static [ModuleId];
    /// Moc 0..1 (już przemnożona przez moc ogólną) dla danej pozycji. Moduł sam wygładza zmianę.
    fn set_amount(&mut self, id: ModuleId, amount: f32);
    /// Tylko moduły złożone: włączenie/wyłączenie pozycji wewnątrz (z własną neutralizacją).
    fn set_sub_enabled(&mut self, _id: ModuleId, _on: bool) {}
    /// Stała latencja dla bieżącego `Ctx` (nie zależy od mocy ani od on/off).
    fn latency_samples(&self) -> usize;
    /// Pełny reset stanu (zmiana fs albo ponowne włączenie). Wygładzane parametry przyjmują od razu wartość docelową.
    fn reset(&mut self, ctx: Ctx);
    /// Przetwarzanie bloku stereo w miejscu (długość ≤ `MAX_BLOCK`).
    fn process(&mut self, l: &mut [f64], r: &mut [f64]);
    /// Bieżąca redukcja wzmocnienia danej pozycji (miernik GR), dB ≥ 0.
    fn gain_reduction_db(&self, _id: ModuleId) -> f32 {
        0.0
    }
    fn set_output_ctx(&mut self, _o: OutputCtx) {}
    /// Ile próbek moduł potrzebuje „rozgrzewać się” po resecie (detektory, okna), zanim wejdzie przenikanie.
    fn warmup_samples(&self) -> usize {
        self.latency_samples() + 2048
    }
}

/// Maksymalny blok przetwarzania wewnątrz toru (wielokrotność bloku sterującego 16).
pub const MAX_BLOCK: usize = 256;
/// Blok sterujący dynamiki (spec v1 §0): K = 16·F.
pub const CONTROL_BLOCK: usize = 16;
