//! Protokół sterowania GUI ↔ wtyczka (lokalne named pipes `\\.\pipe\szklo-ctl-*`, bez sieci).
//! Linie tekstowe ASCII zakończone `\n`, łatwe do debugowania i bez zależności.
//!
//! Każdy serwer wtyczki (każda wersja DLL w audiodg.exe) ma WŁASNY potok o unikalnej nazwie; aplikacja łączy się
//! ze wszystkimi, wysyła stan do każdego, a mierniki bierze z tego, który faktycznie przetwarza dźwięk (rosnące
//! `frames`). Wcześniej jeden potok o stałej nazwie mogła zająć bezczynna instancja (np. stara wersja DLL).
//!
//! GUI → wtyczka:  `P <enabled> <compare> <master> <ceiling> <on>:<amount> ×29`
//! (starsze, krótsze linie są przyjmowane: brakujące moduły = wyłączone; nadmiarowe pozycje są pomijane)
//! wtyczka → GUI:  `M <in_db> <out_db> <gr_db> <lufs> <tp_db> <fs> <latency> [<frames>]`

use crate::catalog::MODULE_COUNT;
use crate::shared::{MeterSnapshot, SharedParams};

/// Najmniejsza przyjmowana liczba modułów w linii `P` (wersje do 0.3.0).
const MIN_MODULES: usize = 23;

/// Nazwa potoku starego typu (jeden na system) – aplikacja nadal ją obsługuje.
pub const PIPE_NAME: &str = r"\\.\pipe\szklo-ctl";
/// Prefiks nazw potoków (bez `\\.\pipe\`).
pub const PIPE_PREFIX: &str = "szklo-ctl";

/// Unikalna ścieżka potoku dla nowego serwera w tym procesie.
pub fn unique_pipe_path() -> String {
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    format!(r"\\.\pipe\{PIPE_PREFIX}-{}-{:x}-{}", std::process::id(), t, SEQ.fetch_add(1, Ordering::Relaxed))
}

pub fn encode_params(p: &SharedParams) -> String {
    let mut s = format!("P {} {} {:.4} {:.2}", p.enabled() as u8, p.compare() as u8, p.master(), p.ceiling_dbtp());
    for i in 0..MODULE_COUNT {
        let (on, a) = p.module(i);
        s.push_str(&format!(" {}:{:.4}", on as u8, a));
    }
    s.push('\n');
    s
}

/// Zastosuj linię `P …` do parametrów. Zwraca false przy błędzie formatu (nic nie zmienia).
pub fn apply_params_line(line: &str, p: &SharedParams) -> bool {
    let mut it = line.split_ascii_whitespace();
    if it.next() != Some("P") {
        return false;
    }
    let vals: Vec<&str> = it.collect();
    // zgodność wersji: linia/chunk sprzed dodania modułów (od 23 pozycji) i z nowszej aplikacji (więcej pozycji)
    if vals.len() < 4 + MIN_MODULES {
        return false;
    }
    let (Ok(en), Ok(cmp), Ok(master), Ok(ceil)) =
        (vals[0].parse::<u8>(), vals[1].parse::<u8>(), vals[2].parse::<f32>(), vals[3].parse::<f32>())
    else {
        return false;
    };
    let mut mods = [(false, 0.5f32); MODULE_COUNT];
    for (i, v) in vals[4..].iter().take(MODULE_COUNT).enumerate() {
        let Some((o, a)) = v.split_once(':') else { return false };
        let (Ok(o), Ok(a)) = (o.parse::<u8>(), a.parse::<f32>()) else { return false };
        mods[i] = (o != 0, a);
    }
    p.set_enabled(en != 0);
    p.set_compare(cmp != 0);
    p.set_master(master);
    p.set_ceiling_dbtp(ceil);
    for (i, (o, a)) in mods.iter().enumerate() {
        p.set_module(i, *o, *a);
    }
    true
}

pub fn encode_meters(m: &MeterSnapshot, fs: f64, latency: usize) -> String {
    format!(
        "M {:.2} {:.2} {:.2} {:.2} {:.2} {} {} {}\n",
        m.in_peak_db, m.out_peak_db, m.gr_db, m.lufs_short, m.true_peak_db, fs as u32, latency, m.frames
    )
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RemoteMeters {
    pub snap: MeterSnapshot,
    pub fs: u32,
    pub latency: usize,
}

pub fn decode_meters(line: &str) -> Option<RemoteMeters> {
    let mut it = line.split_ascii_whitespace();
    if it.next()? != "M" {
        return None;
    }
    let mut f = || it.next().and_then(|v| v.parse::<f32>().ok());
    let snap = MeterSnapshot {
        in_peak_db: f()?,
        out_peak_db: f()?,
        gr_db: f()?,
        lufs_short: f()?,
        true_peak_db: f()?,
        frames: 0,
    };
    let fs = f()? as u32;
    let latency = f()? as usize;
    // pole `frames` jest opcjonalne (starsze wtyczki go nie wysyłają)
    let mut snap = snap;
    snap.frames = it.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    Some(RemoteMeters { snap, fs, latency })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let a = SharedParams::default();
        a.set_module(3, true, 0.25);
        a.set_master(0.7);
        a.set_ceiling_dbtp(-1.5);
        let line = encode_params(&a);
        let b = SharedParams::default();
        assert!(apply_params_line(line.trim_end(), &b));
        assert_eq!(b.module(3), (true, 0.25));
        assert!((b.master() - 0.7).abs() < 1e-4);
        assert!((b.ceiling_dbtp() + 1.5).abs() < 1e-4);
        assert!(!apply_params_line("P 1 0", &b));
        // linia starszej wersji (23 moduły) – ostatni moduł wyłączony; linia nowszej (31) – nadmiar pominięty
        let stara: String = format!("P 1 0 1.0 -1.0{}", " 1:0.5".repeat(23));
        assert!(apply_params_line(&stara, &b));
        assert_eq!(b.module(22), (true, 0.5));
        assert!(!b.module(23).0);
        assert!(apply_params_line(&format!("P 1 0 1.0 -1.0{}", " 1:0.25".repeat(31)), &b));
        assert_eq!(b.module(23), (true, 0.25));
        let m = MeterSnapshot { in_peak_db: -3.0, out_peak_db: -1.0, gr_db: 2.5, lufs_short: -11.0, true_peak_db: -1.0, frames: 0 };
        let r = decode_meters(&encode_meters(&m, 48000.0, 4096)).unwrap();
        assert_eq!(r.fs, 48000);
        assert_eq!(r.latency, 4096);
        assert!((r.snap.gr_db - 2.5).abs() < 1e-3);
    }
}
