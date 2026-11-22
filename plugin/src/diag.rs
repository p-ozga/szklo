//! Dziennik diagnostyczny wtyczki (np. w audiodg.exe, gdzie nie ma konsoli ani debuggera).
//! Włączony tylko, gdy istnieje katalog `C:\ProgramData\Szklo\log` (usługa audio musi mieć w nim prawo zapisu);
//! zapis do `szklo_diag.txt`. Poza tym żadnych operacji plikowych.

use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// Katalog dziennika; obecność sprawdzana najwyżej co 3 s (można włączyć/wyłączyć bez przeładowania wtyczki).
fn dir() -> Option<std::path::PathBuf> {
    static LAST: AtomicU64 = AtomicU64::new(0);
    static ON: AtomicBool = AtomicBool::new(false);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if now.saturating_sub(LAST.load(Ordering::Relaxed)) >= 3 {
        LAST.store(now, Ordering::Relaxed);
        ON.store(std::path::Path::new(DIR).is_dir(), Ordering::Relaxed);
    }
    if ON.load(Ordering::Relaxed) {
        Some(std::path::PathBuf::from(DIR))
    } else {
        None
    }
}

const DIR: &str = r"C:\ProgramData\Szklo\log";
/// Znacznik wersji DLL w dzienniku (rozróżnia kolejne wgrane wersje w jednym audiodg.exe).
pub const TAG: &str = concat!("v", env!("CARGO_PKG_VERSION"), "-", env!("SZKLO_BUILD_TAG"));

pub fn enabled() -> bool {
    dir().is_some()
}

pub fn log(msg: &str) {
    let Some(d) = dir() else { return };
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
    let exe = std::env::current_exe().ok().and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_default();
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(d.join("szklo_diag.txt")) {
        let _ = writeln!(f, "{t:.3} [{exe} pid {} {TAG}] {msg}", std::process::id());
    }
}
