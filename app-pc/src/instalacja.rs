//! Składniki potrzebne do pełnego działania Szkła i ich instalacja.
//!
//! * Wtyczka Szkła w Equalizer APO: kopia `szklo_vst2.dll` w `C:\ProgramData\Szklo\` (folder tylko do odczytu dla
//!   zwykłych użytkowników – wtyczkę ładuje usługa dźwięku Windows) i wpis `VSTPlugin:` w `config.txt` Equalizer APO.
//!   Robi to `Szklo.exe --instaluj-wtyczke` (instalator MSI albo przycisk w programie, z prawami administratora).
//! * Equalizer APO (Jonas Thedering, GPL-2.0) – NIE jest dołączony: Szkło pobiera oficjalny instalator z SourceForge,
//!   gdy użytkownik kliknie „Pobierz i zainstaluj Equalizer APO”.
//! * WebView2 Runtime (Microsoft) – jest w Windows 11; gdy go brak, Szkło przy starcie proponuje pobranie
//!   oficjalnego instalatora Microsoft.
//!
//! Tryby wiersza poleceń (bez okna, kod wyjścia 0 = sukces):
//!   `--instaluj-wtyczke`   – skopiuj wtyczkę i dopisz ją do konfiguracji Equalizer APO,
//!   `--odinstaluj-wtyczke` – usuń wpis Szkła z konfiguracji i (jeśli się da) pliki wtyczki.

use std::path::{Path, PathBuf};

const ZNACZNIK: &str = "\\programdata\\szklo\\szklo_vst2"; // porównywane małymi literami
const KOMENTARZ: &str = "# Szkło – procesor dźwięku";

/// Obsługa trybów instalacyjnych. `Some(kod)` = program ma się zakończyć z tym kodem (bez okna).
pub fn obsluz_argumenty() -> Option<i32> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let wynik = if a.iter().any(|x| x == "--instaluj-wtyczke") {
        zarejestruj_wtyczke()
    } else if a.iter().any(|x| x == "--odinstaluj-wtyczke") {
        wyrejestruj_wtyczke()
    } else {
        return None;
    };
    match wynik {
        Ok(m) => {
            log::info!("instalacja: {m}");
            Some(0)
        }
        Err(e) => {
            log::error!("instalacja: {e}");
            Some(1)
        }
    }
}

// ───────────────────────────── Equalizer APO: stan ─────────────────────────────

/// (folder instalacji, folder konfiguracji) Equalizer APO z rejestru; `None` = nie zainstalowany.
pub fn apo_sciezki() -> Option<(PathBuf, PathBuf)> {
    let inst = rejestr_hklm(r"SOFTWARE\EqualizerAPO", "InstallPath")?;
    let konf = rejestr_hklm(r"SOFTWARE\EqualizerAPO", "ConfigPath").unwrap_or_else(|| format!("{inst}\\config"));
    let inst = PathBuf::from(inst);
    inst.join("EqualizerAPO.dll").exists().then(|| (inst, PathBuf::from(konf)))
}

pub fn apo_zainstalowany() -> bool {
    apo_sciezki().is_some()
}

/// Czy config.txt Equalizer APO ładuje wtyczkę Szkła (niezakomentowany wpis z istniejącym plikiem).
pub fn wtyczka_zarejestrowana() -> bool {
    let Some((_, konf)) = apo_sciezki() else { return false };
    let Ok(t) = std::fs::read_to_string(konf.join("config.txt")) else { return false };
    t.lines().any(|l| {
        let l = l.trim();
        if !l.to_lowercase().starts_with("vstplugin:") || !l.to_lowercase().contains(ZNACZNIK) {
            return false;
        }
        sciezka_biblioteki(l).map(|p| Path::new(&p).exists()).unwrap_or(false)
    })
}

/// Ścieżka z `Library "…"` w wierszu VSTPlugin.
fn sciezka_biblioteki(wiersz: &str) -> Option<String> {
    let i = wiersz.find("Library \"")? + "Library \"".len();
    let j = wiersz[i..].find('"')? + i;
    Some(wiersz[i..j].to_string())
}

fn folder_wtyczki() -> PathBuf {
    std::env::var_os("ProgramData").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\ProgramData")).join("Szklo")
}

// ───────────────────────────── wtyczka: rejestracja ─────────────────────────────

fn zarejestruj_wtyczke() -> Result<String, String> {
    use sha2::Digest;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let zrodlo = exe.with_file_name("szklo_vst2.dll");
    let bajty = std::fs::read(&zrodlo).map_err(|e| format!("brak {}: {e}", zrodlo.display()))?;
    let skrot: String = sha2::Sha256::digest(&bajty).iter().take(4).map(|b| format!("{b:02x}")).collect();
    let folder = folder_wtyczki();
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    // Wtyczkę ładuje usługa dźwięku (LOCAL SERVICE) – musi ją czytać; zwykli użytkownicy NIE mogą jej podmienić
    // (kod w procesie usługi systemowej). System i administratorzy: pełne prawa.
    // Folder mógł już istnieć (utworzony wcześniej przez zwykłego użytkownika): przejmujemy własność i ustawiamy prawa
    // od nowa; bez tego właściciel mógłby podmienić plik ładowany przez usługę systemową.
    let icacls = |args: &[&str]| -> Result<(), String> {
        let st = std::process::Command::new("icacls").arg(&folder).args(args).status().map_err(|e| format!("icacls: {e}"))?;
        if st.success() { Ok(()) } else { Err(format!("nie udało się zabezpieczyć {} (icacls {args:?})", folder.display())) }
    };
    icacls(&["/setowner", "*S-1-5-32-544", "/T", "/Q"])?;
    icacls(&["/inheritance:r", "/grant:r", "*S-1-5-18:(OI)(CI)F", "*S-1-5-32-544:(OI)(CI)F", "*S-1-5-19:(OI)(CI)RX", "*S-1-5-32-545:(OI)(CI)RX", "/T", "/Q"])?;
    // nazwa z sumą kontrolną: nowa wersja ma nowy plik (stary bywa zablokowany przez usługę dźwięku)
    let cel = folder.join(format!("szklo_vst2_{skrot}.dll"));
    // istniejący plik o tej nazwie musi być bajt w bajt nasz (nazwa to tylko 4 bajty skrótu – nie dowód)
    if std::fs::read(&cel).map(|b| b != bajty).unwrap_or(true) {
        std::fs::write(&cel, &bajty).map_err(|e| format!("{}: {e}", cel.display()))?;
    }
    let Some((_, konf)) = apo_sciezki() else {
        return Ok(format!("wtyczka skopiowana do {} (Equalizer APO jeszcze nie jest zainstalowany)", cel.display()));
    };
    let plik = konf.join("config.txt");
    let stary = std::fs::read(&plik).unwrap_or_default();
    let bom = stary.starts_with(&[0xEF, 0xBB, 0xBF]);
    let tekst = String::from_utf8_lossy(if bom { &stary[3..] } else { &stary[..] }).to_string();
    let nowa = cel.to_string_lossy().to_string();
    let mut zmieniono = false;
    let mut wiersze: Vec<String> = tekst
        .lines()
        .map(|l| {
            let t = l.trim_start();
            if t.to_lowercase().starts_with("vstplugin:") && t.to_lowercase().contains(ZNACZNIK) {
                if let Some(s) = sciezka_biblioteki(t) {
                    zmieniono = true;
                    // podmień tylko ścieżkę – reszta wiersza (zapisany stan wtyczki, ChunkData) zostaje
                    return l.replacen(&s, &nowa, 1);
                }
            }
            l.to_string()
        })
        .collect();
    if !zmieniono {
        let kopia = konf.join("config.przed_szklo.txt");
        if !kopia.exists() && plik.exists() {
            let _ = std::fs::copy(&plik, &kopia);
        }
        wiersze.push(format!("{KOMENTARZ} (dodane przez instalator Szkła; poprzednia konfiguracja: config.przed_szklo.txt)"));
        wiersze.push(format!("VSTPlugin: Library \"{nowa}\""));
    }
    let mut wynik = wiersze.join("\r\n");
    wynik.push_str("\r\n");
    let mut dane = if bom { vec![0xEF, 0xBB, 0xBF] } else { Vec::new() };
    dane.extend_from_slice(wynik.as_bytes());
    std::fs::write(&plik, dane).map_err(|e| format!("{}: {e}", plik.display()))?;
    usun_stare_kopie(&folder, Some(&cel));
    Ok(format!("wtyczka {} wpisana do {}", cel.display(), plik.display()))
}

fn wyrejestruj_wtyczke() -> Result<String, String> {
    if let Some((_, konf)) = apo_sciezki() {
        let plik = konf.join("config.txt");
        if let Ok(stary) = std::fs::read(&plik) {
            let bom = stary.starts_with(&[0xEF, 0xBB, 0xBF]);
            let tekst = String::from_utf8_lossy(if bom { &stary[3..] } else { &stary[..] }).to_string();
            let wiersze: Vec<&str> = tekst
                .lines()
                .filter(|l| {
                    let t = l.trim_start().to_lowercase();
                    !(t.starts_with("vstplugin:") && t.contains(ZNACZNIK)) && !l.trim_start().starts_with(KOMENTARZ)
                })
                .collect();
            let mut dane = if bom { vec![0xEF, 0xBB, 0xBF] } else { Vec::new() };
            dane.extend_from_slice((wiersze.join("\r\n") + "\r\n").as_bytes());
            std::fs::write(&plik, dane).map_err(|e| format!("{}: {e}", plik.display()))?;
        }
    }
    // pliki wtyczki: usuń, co się da (wczytana przez usługę dźwięku zniknie po jej restarcie/ponownym uruchomieniu)
    usun_stare_kopie(&folder_wtyczki(), None);
    Ok("wpis Szkła usunięty z konfiguracji Equalizer APO".into())
}

fn usun_stare_kopie(folder: &Path, zostaw: Option<&Path>) {
    let Ok(rd) = std::fs::read_dir(folder) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let n = e.file_name().to_string_lossy().to_lowercase();
        if n.starts_with("szklo_vst2") && n.ends_with(".dll") && Some(p.as_path()) != zostaw {
            let _ = std::fs::remove_file(&p);
        }
    }
}

// ───────────────────────────── z programu (prawa administratora) ─────────────────────────────

/// Uruchamia `Szklo.exe <arg>` jako administrator (okno zgody Windows) i czeka. `Ok(true)` = sukces.
#[cfg(windows)]
pub fn jako_administrator(exe: &Path, arg: &str) -> Result<bool, String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;
    let plik = HSTRING::from(exe.as_os_str());
    let arg = HSTRING::from(arg);
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("runas"),
        lpFile: PCWSTR(plik.as_ptr()),
        lpParameters: PCWSTR(arg.as_ptr()),
        nShow: SW_HIDE.0,
        ..Default::default()
    };
    // SAFETY: struktura wypełniona poprawnie, łańcuchy żyją do końca funkcji; uchwyt procesu zamykany.
    unsafe {
        ShellExecuteExW(&mut info).map_err(|_| "brak zgody administratora".to_string())?;
        let mut kod = 1u32;
        if !info.hProcess.is_invalid() {
            WaitForSingleObject(info.hProcess, INFINITE);
            let _ = GetExitCodeProcess(info.hProcess, &mut kod);
            let _ = CloseHandle(info.hProcess);
        }
        Ok(kod == 0)
    }
}
#[cfg(not(windows))]
pub fn jako_administrator(_: &Path, _: &str) -> Result<bool, String> {
    Err("tylko Windows".into())
}

/// Podłącza wtyczkę do Equalizer APO z programu (okno zgody administratora).
pub fn podlacz_wtyczke() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    if !exe.with_file_name("szklo_vst2.dll").exists() {
        return Err("brak pliku wtyczki szklo_vst2.dll obok Szklo.exe – zainstaluj Szkło instalatorem".into());
    }
    match jako_administrator(&exe, "--instaluj-wtyczke")? {
        true if wtyczka_zarejestrowana() => Ok(()),
        true => Err("wtyczka skopiowana, ale Equalizer APO nie jest zainstalowany".into()),
        false => Err("nie udało się wpisać wtyczki do Equalizer APO (szczegóły w dzienniku)".into()),
    }
}

/// Pobiera oficjalny instalator Equalizer APO (SourceForge), uruchamia go, a potem podłącza wtyczkę Szkła.
pub fn pobierz_i_zainstaluj_apo(postep: &dyn Fn(String)) -> Result<(), String> {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    static TRWA: AtomicBool = AtomicBool::new(false);
    if TRWA.swap(true, Ordering::SeqCst) {
        return Err("instalacja już trwa".into());
    }
    struct Koniec;
    impl Drop for Koniec {
        fn drop(&mut self) {
            TRWA.store(false, Ordering::SeqCst);
        }
    }
    let _k = Koniec;
    let klient = reqwest::blocking::Client::builder()
        .https_only(true)
        .user_agent("Szklo-DSP")
        .connect_timeout(std::time::Duration::from_secs(20))
        .timeout(Some(std::time::Duration::from_secs(15 * 60)))
        .build()
        .map_err(|e| e.to_string())?;
    postep("Sprawdzam najnowszą wersję Equalizer APO…".into());
    // najnowsze wydanie dla Windows według SourceForge; awaryjnie znana wersja 1.4.2
    let (plik, rozmiar) = klient
        .get("https://sourceforge.net/projects/equalizerapo/best_release.json")
        .send()
        .and_then(|r| r.json::<serde_json::Value>())
        .ok()
        .and_then(|v| {
            let w = &v["platform_releases"]["windows"];
            let f = w["filename"].as_str()?.to_string();
            (f.to_lowercase().contains("x64") && f.to_lowercase().ends_with(".exe")).then(|| (f, w["bytes"].as_u64().unwrap_or(0)))
        })
        .unwrap_or_else(|| ("/1.4.2/EqualizerAPO-x64-1.4.2.exe".into(), 0));
    let url = format!("https://downloads.sourceforge.net/project/equalizerapo{plik}");
    log::info!("Equalizer APO: pobieram {url}");
    let cel = crate::settings::dir().join("pobrane").join("EqualizerAPO-instalator.exe");
    let _ = std::fs::create_dir_all(cel.parent().unwrap());
    let tymcz = cel.with_extension("czesc");
    let mut odp = klient.get(&url).send().and_then(|r| r.error_for_status()).map_err(|e| format!("pobieranie: {e}"))?;
    let calosc = odp.content_length().unwrap_or(rozmiar);
    let mut f = std::fs::File::create(&tymcz).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 16];
    let (mut ile, mut ost) = (0u64, 0u64);
    loop {
        let n = odp.read(&mut buf).map_err(|e| format!("pobieranie przerwane: {e}"))?;
        if n == 0 {
            break;
        }
        f.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        ile += n as u64;
        if ile - ost > 500_000 {
            ost = ile;
            let proc = if calosc > 0 { format!(" ({} %)", ile * 100 / calosc) } else { String::new() };
            postep(format!("Pobieram Equalizer APO: {:.1} MB{proc}", ile as f64 / 1e6));
        }
    }
    drop(f);
    // to ma być program Windows (SourceForge potrafi zwrócić stronę HTML zamiast pliku)
    let naglowek = std::fs::read(&tymcz).map(|b| b.get(..2) == Some(b"MZ")).unwrap_or(false);
    if !naglowek || (calosc > 0 && ile != calosc) {
        let _ = std::fs::remove_file(&tymcz);
        return Err("pobrany plik nie jest instalatorem Equalizer APO – spróbuj ponownie później".into());
    }
    let _ = std::fs::remove_file(&cel);
    std::fs::rename(&tymcz, &cel).map_err(|e| e.to_string())?;
    log::info!("Equalizer APO: pobrano ({ile} B), uruchamiam instalator");
    postep("Instalator Equalizer APO jest otwarty – przejdź przez jego kroki. Na końcu zaznacz urządzenia, na których słuchasz (słuchawki, głośniki).".into());
    // instalator sam prosi o zgodę administratora; czekamy na jego koniec
    let _ = uruchom_i_czekaj(&cel);
    if !apo_zainstalowany() {
        return Err("instalacja Equalizer APO nie została dokończona".into());
    }
    postep("Podłączam Szkło do Equalizer APO…".into());
    podlacz_wtyczke()?;
    Ok(())
}

/// Uruchamia program (np. instalator) przez powłokę i czeka na jego zakończenie.
fn uruchom_i_czekaj(exe: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        use windows::core::{w, HSTRING, PCWSTR};
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
        use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        let plik = HSTRING::from(exe.as_os_str());
        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOCLOSEPROCESS,
            lpVerb: w!("open"),
            lpFile: PCWSTR(plik.as_ptr()),
            nShow: SW_SHOWNORMAL.0,
            ..Default::default()
        };
        // SAFETY: jak wyżej.
        unsafe {
            ShellExecuteExW(&mut info).map_err(|e| format!("nie udało się uruchomić instalatora: {e}"))?;
            if !info.hProcess.is_invalid() {
                WaitForSingleObject(info.hProcess, INFINITE);
                let _ = CloseHandle(info.hProcess);
            }
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        Err("tylko Windows".into())
    }
}

// ───────────────────────────── WebView2 (Microsoft) ─────────────────────────────

/// Czy jest WebView2 Runtime (bez niego okno Szkła się nie otworzy).
pub fn webview2_jest() -> bool {
    const KLUCZ: &str = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let dobra = |v: Option<String>| v.is_some_and(|s| !s.is_empty() && s != "0.0.0.0");
    dobra(rejestr_hklm(&format!(r"SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{KLUCZ}"), "pv"))
        || dobra(rejestr_hklm(&format!(r"SOFTWARE\Microsoft\EdgeUpdate\Clients\{KLUCZ}"), "pv"))
        || dobra(rejestr_hkcu(&format!(r"Software\Microsoft\EdgeUpdate\Clients\{KLUCZ}"), "pv"))
}

/// Przy starcie: brak WebView2 → zapytaj i zainstaluj oficjalnym instalatorem Microsoft. `false` = nie da się dalej.
pub fn zapewnij_webview2() -> bool {
    if webview2_jest() {
        return true;
    }
    log::warn!("brak WebView2 Runtime");
    let tak = pytanie(
        "Szkło – brak składnika Windows",
        "Do wyświetlenia okna Szkło potrzebuje „Microsoft Edge WebView2 Runtime” (bezpłatny składnik Windows firmy Microsoft).\n\n\
         Pobrać go teraz z serwera Microsoft i zainstalować?",
    );
    if !tak {
        return false;
    }
    let cel = std::env::temp_dir().join("Szklo-MicrosoftEdgeWebview2Setup.exe");
    let ok = reqwest::blocking::Client::builder()
        .user_agent("Szklo-DSP")
        .timeout(Some(std::time::Duration::from_secs(300)))
        .build()
        .ok()
        .and_then(|k| k.get("https://go.microsoft.com/fwlink/p/?LinkId=2124703").send().ok())
        .and_then(|r| r.error_for_status().ok())
        .and_then(|r| r.bytes().ok())
        .filter(|b| b.starts_with(b"MZ"))
        .and_then(|b| std::fs::write(&cel, &b).ok())
        .is_some();
    if !ok {
        komunikat("Szkło", "Nie udało się pobrać WebView2. Zainstaluj go ze strony Microsoft (wyszukaj „WebView2 Runtime”) i uruchom Szkło ponownie.");
        return false;
    }
    let _ = std::process::Command::new(&cel).args(["/install"]).status();
    let _ = std::fs::remove_file(&cel);
    if webview2_jest() {
        log::info!("WebView2 Runtime zainstalowany");
        true
    } else {
        komunikat("Szkło", "Instalacja WebView2 nie została dokończona. Uruchom Szkło ponownie, żeby spróbować jeszcze raz.");
        false
    }
}

#[cfg(windows)]
fn pytanie(tytul: &str, tresc: &str) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, IDYES, MB_ICONQUESTION, MB_YESNO};
    // SAFETY: łańcuchy żyją do końca wywołania.
    unsafe { MessageBoxW(None, &HSTRING::from(tresc), &HSTRING::from(tytul), MB_YESNO | MB_ICONQUESTION) == IDYES }
}
#[cfg(not(windows))]
fn pytanie(_: &str, _: &str) -> bool {
    false
}

#[cfg(windows)]
fn komunikat(tytul: &str, tresc: &str) {
    use windows::core::HSTRING;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONWARNING, MB_OK};
    // SAFETY: łańcuchy żyją do końca wywołania.
    unsafe {
        MessageBoxW(None, &HSTRING::from(tresc), &HSTRING::from(tytul), MB_OK | MB_ICONWARNING);
    }
}
#[cfg(not(windows))]
fn komunikat(_: &str, _: &str) {}

// ───────────────────────────── rejestr ─────────────────────────────

#[cfg(windows)]
fn rejestr(root: windows::Win32::System::Registry::HKEY, klucz: &str, nazwa: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{RegGetValueW, RRF_RT_REG_SZ, RRF_SUBKEY_WOW6464KEY};
    let mut buf = [0u16; 1024];
    let mut len = (buf.len() * 2) as u32;
    // SAFETY: bufor lokalny, długość w bajtach.
    let r = unsafe {
        RegGetValueW(root, &HSTRING::from(klucz), &HSTRING::from(nazwa), RRF_RT_REG_SZ | RRF_SUBKEY_WOW6464KEY, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len))
    };
    r.is_ok().then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1).min(buf.len())]))
}
#[cfg(windows)]
fn rejestr_hklm(klucz: &str, nazwa: &str) -> Option<String> {
    rejestr(windows::Win32::System::Registry::HKEY_LOCAL_MACHINE, klucz, nazwa)
}
#[cfg(windows)]
fn rejestr_hkcu(klucz: &str, nazwa: &str) -> Option<String> {
    rejestr(windows::Win32::System::Registry::HKEY_CURRENT_USER, klucz, nazwa)
}
#[cfg(not(windows))]
fn rejestr_hklm(_: &str, _: &str) -> Option<String> {
    None
}
#[cfg(not(windows))]
fn rejestr_hkcu(_: &str, _: &str) -> Option<String> {
    None
}
