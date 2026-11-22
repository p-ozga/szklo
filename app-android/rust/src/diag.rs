//! Dziennik diagnostyczny: logi trafiają do logcat (tag SzkloSpotify) ORAZ do małego bufora w pamięci,
//! który aplikacja pokazuje na karcie Spotify – żeby zobaczyć błąd bez kabla USB i adb.

use std::collections::VecDeque;
use std::sync::{Mutex, OnceLock};

const MAX_LINII: usize = 120;

/// Spotify odmówił klucza dla bieżącego utworu – wtedy nie udajemy „playing”.
pub static KLUCZ_ODRZUCONY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn bufor() -> &'static Mutex<VecDeque<String>> {
    static B: OnceLock<Mutex<VecDeque<String>>> = OnceLock::new();
    B.get_or_init(|| Mutex::new(VecDeque::with_capacity(MAX_LINII)))
}

/// Dopisuje linię do bufora (najstarsze wypadają).
pub fn dopisz(linia: String) {
    if let Ok(mut b) = bufor().lock() {
        if b.len() >= MAX_LINII {
            b.pop_front();
        }
        b.push_back(linia);
    }
}

/// Cały bufor jako tekst (linia po linii, od najstarszej).
pub fn tekst() -> String {
    bufor().lock().map(|b| b.iter().cloned().collect::<Vec<_>>().join("\n")).unwrap_or_default()
}

fn czas() -> String {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let s = (ms / 1000) % 86_400;
    format!("{:02}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60) // UTC
}

struct Logger {
    #[cfg(target_os = "android")]
    android: android_logger::AndroidLogger,
}

impl log::Log for Logger {
    fn enabled(&self, m: &log::Metadata) -> bool {
        // symphonia przy uszkodzonym/zaszyfrowanym strumieniu sypie tysiącami „skipping junk” – tylko błędy
        if m.target().starts_with("symphonia") {
            return m.level() <= log::Level::Error;
        }
        m.level() <= log::Level::Info || (m.level() <= log::Level::Debug && m.target().starts_with("librespot"))
    }

    fn log(&self, r: &log::Record) {
        if !self.enabled(r.metadata()) {
            return;
        }
        // Spotify odmawia klucza deszyfrującego (librespot #1649) – pokaż to wprost na karcie.
        // Tylko komunikaty librespot (nasz set_status loguje z innego celu – bez rekurencji).
        if r.target().starts_with("librespot") && r.level() <= log::Level::Warn {
            let t = r.args().to_string();
            if t.contains("audio key") || t.contains("Unable to load key") {
                KLUCZ_ODRZUCONY.store(true, std::sync::atomic::Ordering::Relaxed);
                crate::spotify::set_status("error Spotify odmówił klucza odtwarzania (audio key)");
            }
        }
        if r.target().starts_with("librespot_playback") && r.args().to_string().starts_with("Loading <") {
            KLUCZ_ODRZUCONY.store(false, std::sync::atomic::Ordering::Relaxed); // nowy utwór – nowa próba
        }
        #[cfg(target_os = "android")]
        self.android.log(r);
        // do bufora na karcie: wszystko od Info wzwyż (Debug tylko w logcat – za dużo szumu)
        if r.level() <= log::Level::Info {
            let modul = r.target().split("::").take(2).collect::<Vec<_>>().join("::");
            dopisz(format!("{} {} [{}] {}", czas(), r.level().as_str().chars().next().unwrap_or('?'), modul, r.args()));
        }
    }

    fn flush(&self) {}
}

/// Instaluje logger (raz na proces).
pub fn init() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let l = Logger {
            #[cfg(target_os = "android")]
            android: android_logger::AndroidLogger::new(
                android_logger::Config::default().with_max_level(log::LevelFilter::Debug).with_tag("SzkloSpotify"),
            ),
        };
        if log::set_boxed_logger(Box::new(l)).is_ok() {
            log::set_max_level(log::LevelFilter::Debug);
        }
    });
}
