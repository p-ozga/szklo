//! Szkło – rdzeń DSP.
//!
//! Cały DSP w f64, wejście/wyjście f32. Bez `unsafe` (FFI żyje w osobnych crate'ach: plugin, JNI).
#![forbid(unsafe_code)]

pub mod catalog;
pub mod dsp;
pub mod engine;
pub mod graph;
pub mod ipc;
pub mod macro_map;
pub mod module;
pub mod modules;
pub mod presets;
pub mod shared;

pub use catalog::{Group, ModuleId, ModuleInfo, MODULES, MODULE_COUNT};
pub use engine::Engine;
pub use module::{Ctx, Module};
pub use presets::{Preset, PRESETS};
pub use shared::{MeterSnapshot, SharedMeters, SharedParams};

/// Wygodny konstruktor: silnik + współdzielone parametry i mierniki.
pub fn new_engine(fs: f64) -> (Engine, std::sync::Arc<SharedParams>, std::sync::Arc<SharedMeters>) {
    let p = std::sync::Arc::new(SharedParams::default());
    let m = std::sync::Arc::new(SharedMeters::new());
    (Engine::new(fs, p.clone(), m.clone()), p, m)
}
