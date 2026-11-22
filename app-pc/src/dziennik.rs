//! Automatyczny dziennik: %LOCALAPPDATA%\Szklo\logi\szklo-RRRR-MM-DD.log (UTC).
//! Zapisuje komunikaty `log` (Info i wyżej), zmiany stanu silnika oraz paniki (z miejscem w kodzie).
//! Trzymamy 14 ostatnich plików. Podgląd: odwrót okna → „O programie i licencje” → „Dziennik”.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

const ZOSTAW_PLIKOW: usize = 14;

pub fn katalog() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Szklo").join("logi")
}

fn dzien_i_czas() -> (String, String) {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let (dni, sek) = ((s / 86_400) as i64, s % 86_400);
    // dni od 1970-01-01 → data (algorytm H. Hinnanta)
    let z = dni + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    (format!("{y:04}-{m:02}-{d:02}"), format!("{:02}:{:02}:{:02}", sek / 3600, (sek / 60) % 60, sek % 60))
}

fn plik_dnia() -> PathBuf {
    katalog().join(format!("szklo-{}.log", dzien_i_czas().0))
}

struct Dziennik {
    zapis: Mutex<()>,
}

fn dopisz(linia: &str) {
    let _ = std::fs::create_dir_all(katalog());
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(plik_dnia()) {
        let _ = writeln!(f, "{} UTC {linia}", dzien_i_czas().1);
    }
}

impl log::Log for Dziennik {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info && !m.target().starts_with("wry") && !m.target().starts_with("tao")
    }
    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            let _g = self.zapis.lock();
            dopisz(&format!("{:<5} [{}] {}", r.level(), r.target(), r.args()));
        }
    }
    fn flush(&self) {}
}

/// Start dziennika: logger, przechwytywanie panik, sprzątanie starych plików.
pub fn init() {
    static D: Dziennik = Dziennik { zapis: Mutex::new(()) };
    if log::set_logger(&D).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
    let poprzedni = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let gdzie = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let co = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "?".into());
        let watek = std::thread::current().name().unwrap_or("?").to_string();
        dopisz(&format!("PANIKA [{watek}] {co} ({gdzie})"));
        poprzedni(info);
    }));
    // sprzątanie: zostaw ZOSTAW_PLIKOW najnowszych
    if let Ok(rd) = std::fs::read_dir(katalog()) {
        let mut pliki: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.extension().is_some_and(|e| e == "log")).collect();
        pliki.sort();
        let nadmiar = pliki.len().saturating_sub(ZOSTAW_PLIKOW);
        for p in pliki.into_iter().take(nadmiar) {
            let _ = std::fs::remove_file(p);
        }
    }
    log::info!("Szkło {} start", env!("CARGO_PKG_VERSION"));
}

/// Ostatnie ~400 linii dzisiejszego (albo najnowszego) pliku – do podglądu w aplikacji.
pub fn ogon() -> String {
    let mut p = plik_dnia();
    if !p.exists() {
        if let Ok(rd) = std::fs::read_dir(katalog()) {
            let mut v: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
            v.sort();
            if let Some(n) = v.pop() {
                p = n;
            }
        }
    }
    let t = std::fs::read_to_string(&p).unwrap_or_default();
    let linie: Vec<&str> = t.lines().collect();
    let od = linie.len().saturating_sub(400);
    format!("Plik: {}\n\n{}", p.display(), linie[od..].join("\n"))
}
