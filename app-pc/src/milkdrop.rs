//! MilkDrop 3 w karcie odtwarzacza: uruchamia program wskazany przez użytkownika (MilkDrop3.exe – osobny program
//! autorstwa milkdrop2077, oparty na BeatDrop/MilkDrop 2; Szkło go NIE zawiera) i osadza jego okno w oknie Szkła
//! (okno potomne w obszarze karty). Zmniejszenie karty zamyka MilkDrop.

use std::path::PathBuf;
use std::sync::Mutex;

pub struct Prostokat {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

struct Dziala {
    proces: std::process::Child,
    hwnd: isize,
}

fn stan() -> &'static Mutex<Option<Dziala>> {
    static S: Mutex<Option<Dziala>> = Mutex::new(None);
    &S
}

fn plik_sciezki() -> PathBuf {
    crate::settings::dir().join("milkdrop.txt")
}

pub fn sciezka() -> Option<PathBuf> {
    let p = PathBuf::from(std::fs::read_to_string(plik_sciezki()).ok()?.trim());
    p.is_file().then_some(p)
}

/// Gdzie leży pobrany instalator (nie może zostać pomylony z zainstalowanym programem).
fn plik_instalatora() -> PathBuf {
    crate::settings::dir().join("pobrane").join("MilkDrop3-instalator.exe")
}

/// Pobiera instalator z oficjalnego wydania autora (github.com/milkdrop2077/MilkDrop3, najnowsze wydanie),
/// uruchamia go (okno instalatora i ewentualna zgoda administratora – po stronie użytkownika), czeka na koniec
/// i szuka zainstalowanego MilkDrop3.exe. `postep(tekst)` dostaje komunikaty do karty.
pub fn pobierz_i_zainstaluj(postep: &dyn Fn(String)) -> Result<PathBuf, String> {
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    // jedna instalacja naraz (drugie kliknięcie pisałoby do tego samego pliku)
    static TRWA: AtomicBool = AtomicBool::new(false);
    if TRWA.swap(true, Ordering::SeqCst) {
        return Err("pobieranie już trwa".into());
    }
    struct Koniec;
    impl Drop for Koniec {
        fn drop(&mut self) {
            TRWA.store(false, Ordering::SeqCst);
        }
    }
    let _koniec = Koniec;
    postep("Sprawdzam najnowsze wydanie MilkDrop 3…".into());
    let klient = reqwest::blocking::Client::builder()
        .user_agent("Szklo-DSP")
        // hojny limit na całość (90 MB na wolnym łączu), ale zerwane połączenie nie wisi w nieskończoność
        .timeout(Some(std::time::Duration::from_secs(30 * 60)))
        .connect_timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| e.to_string())?;
    let wyd: serde_json::Value = klient
        .get("https://api.github.com/repos/milkdrop2077/MilkDrop3/releases/latest")
        .send()
        .and_then(|r| r.error_for_status())
        .and_then(|r| r.json())
        .map_err(|e| format!("nie udało się odczytać wydania z GitHuba: {e}"))?;
    let zasob = wyd["assets"]
        .as_array()
        .and_then(|a| a.iter().find(|x| x["name"].as_str().is_some_and(|n| n.to_lowercase().ends_with(".exe"))))
        .ok_or("w wydaniu MilkDrop 3 nie ma pliku .exe")?;
    let url = zasob["browser_download_url"].as_str().ok_or("brak adresu pobierania")?.to_string();
    let rozmiar = zasob["size"].as_u64().unwrap_or(0);
    log::info!("MilkDrop: pobieram {url} ({rozmiar} B)");
    let cel = plik_instalatora();
    let _ = std::fs::create_dir_all(cel.parent().unwrap());
    let tymcz = cel.with_extension("czesc");
    let mut odp = klient.get(&url).send().and_then(|r| r.error_for_status()).map_err(|e| format!("pobieranie: {e}"))?;
    let mut plik = std::fs::File::create(&tymcz).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 1 << 16];
    let (mut ile, mut ost) = (0u64, 0u64);
    loop {
        let n = odp.read(&mut buf).map_err(|e| format!("pobieranie przerwane: {e}"))?;
        if n == 0 {
            break;
        }
        plik.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        ile += n as u64;
        if ile - ost > 1_000_000 {
            ost = ile;
            let proc = if rozmiar > 0 { format!(" ({} %)", ile * 100 / rozmiar) } else { String::new() };
            postep(format!("Pobieram MilkDrop 3: {:.0} z {:.0} MB{proc}", ile as f64 / 1e6, rozmiar as f64 / 1e6));
        }
    }
    drop(plik);
    if rozmiar > 0 && ile != rozmiar {
        let _ = std::fs::remove_file(&tymcz);
        return Err(format!("pobrano {ile} z {rozmiar} B – spróbuj ponownie"));
    }
    let _ = std::fs::remove_file(&cel);
    std::fs::rename(&tymcz, &cel).map_err(|e| e.to_string())?;
    log::info!("MilkDrop: pobrano instalator ({ile} B), uruchamiam");
    postep("Instalator MilkDrop 3 jest otwarty – przejdź przez jego kroki (Dalej → Zainstaluj).".into());
    uruchom_i_czekaj(&cel)?;
    postep("Szukam zainstalowanego MilkDrop 3…".into());
    let exe = znajdz().ok_or("instalacja zakończona, ale nie znalazłem MilkDrop3.exe – wskaż go przyciskiem „Wskaż MilkDrop3.exe”")?;
    ustaw_sciezke(&exe);
    log::info!("MilkDrop: zainstalowany w {}", exe.display());
    Ok(exe)
}

/// Uruchamia instalator przez powłokę (obsługuje UAC) i czeka na jego zakończenie.
#[cfg(windows)]
fn uruchom_i_czekaj(exe: &std::path::Path) -> Result<(), String> {
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let plik = HSTRING::from(exe.as_os_str());
    let katalog = HSTRING::from(exe.parent().unwrap_or(std::path::Path::new(".")).as_os_str());
    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS,
        lpVerb: w!("open"),
        lpFile: PCWSTR(plik.as_ptr()),
        lpDirectory: PCWSTR(katalog.as_ptr()),
        nShow: SW_SHOWNORMAL.0,
        ..Default::default()
    };
    // SAFETY: struktura wypełniona poprawnie, łańcuchy żyją do końca funkcji; uchwyt procesu zamykany.
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
fn uruchom_i_czekaj(_: &std::path::Path) -> Result<(), String> {
    Err("tylko Windows".into())
}

/// Ścieżki instalacji MilkDrop z listy „Programy i funkcje” (rejestr Uninstall, HKLM/HKCU, 64/32 bit).
#[cfg(windows)]
fn z_rejestru() -> Vec<PathBuf> {
    use windows::core::{HSTRING, PWSTR};
    use windows::Win32::System::Registry::*;
    let mut wynik = Vec::new();
    let galezie = [
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_LOCAL_MACHINE, r"SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall"),
        (HKEY_CURRENT_USER, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"),
    ];
    let tekst = |root: HKEY, klucz: &str, nazwa: &str| -> Option<String> {
        let mut buf = [0u16; 1024];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: bufor lokalny, długość w bajtach.
        let r = unsafe {
            RegGetValueW(root, &HSTRING::from(klucz), &HSTRING::from(nazwa), RRF_RT_REG_SZ, None, Some(buf.as_mut_ptr() as *mut _), Some(&mut len))
        };
        r.is_ok().then(|| String::from_utf16_lossy(&buf[..(len as usize / 2).saturating_sub(1).min(buf.len())]))
    };
    for (root, sciezka) in galezie {
        let mut h = HKEY::default();
        // SAFETY: poprawny HSTRING i wskaźnik na lokalny HKEY.
        if unsafe { RegOpenKeyExW(root, &HSTRING::from(sciezka), 0, KEY_READ, &mut h) }.is_err() {
            continue;
        }
        let mut i = 0u32;
        loop {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            // SAFETY: bufor i długość lokalne, klucz otwarty.
            if unsafe { RegEnumKeyExW(h, i, PWSTR(buf.as_mut_ptr()), &mut len, None, PWSTR::null(), None, None) }.is_err() {
                break;
            }
            i += 1;
            let klucz = format!(r"{sciezka}\{}", String::from_utf16_lossy(&buf[..len as usize]));
            let nazwa = tekst(root, &klucz, "DisplayName").unwrap_or_default();
            if !nazwa.to_lowercase().contains("milkdrop") {
                continue;
            }
            if let Some(m) = tekst(root, &klucz, "InstallLocation").filter(|s| !s.is_empty()) {
                wynik.push(PathBuf::from(m.trim_matches('"')));
            }
            if let Some(ik) = tekst(root, &klucz, "DisplayIcon") {
                let p = PathBuf::from(ik.split(',').next().unwrap_or("").trim_matches('"'));
                if let Some(k) = p.parent() {
                    wynik.push(k.to_path_buf());
                }
            }
        }
        // SAFETY: klucz otwarty powyżej.
        unsafe {
            let _ = RegCloseKey(h);
        }
    }
    wynik
}
#[cfg(not(windows))]
fn z_rejestru() -> Vec<PathBuf> {
    Vec::new()
}

/// Szuka zainstalowanego MilkDrop3.exe: rejestr „Programy i funkcje”, potem foldery „MilkDrop*” w typowych
/// miejscach instalacji. Pomija pobrany instalator.
pub fn znajdz() -> Option<PathBuf> {
    let instalator = plik_instalatora();
    let dobry = |p: &std::path::Path| p != instalator.as_path();
    fn szukaj(d: &std::path::Path, glebokosc: u32, dobry: &dyn Fn(&std::path::Path) -> bool) -> Option<PathBuf> {
        let wpisy = std::fs::read_dir(d).ok()?;
        let mut katalogi = Vec::new();
        for e in wpisy.flatten() {
            let p = e.path();
            let nazwa = e.file_name().to_string_lossy().to_lowercase();
            if p.is_file() && nazwa.starts_with("milkdrop") && nazwa.ends_with(".exe") && !nazwa.contains("unins") && dobry(&p) {
                return Some(p);
            }
            if glebokosc > 0 && p.is_dir() {
                katalogi.push(p);
            }
        }
        katalogi.into_iter().find_map(|k| szukaj(&k, glebokosc - 1, dobry))
    }
    if let Some(p) = z_rejestru().iter().find_map(|k| szukaj(k, 1, &dobry)) {
        return Some(p);
    }
    let mut bazy = vec![PathBuf::from(r"C:\Program Files"), PathBuf::from(r"C:\Program Files (x86)"), PathBuf::from(r"C:\")];
    if let Some(l) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
        bazy.push(l.join("Programs"));
    }
    if let Some(d) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        bazy.push(d.join("Desktop"));
        bazy.push(d.join("Documents"));
    }
    bazy.iter().find_map(|b| {
        std::fs::read_dir(b)
            .ok()?
            .flatten()
            .filter(|e| e.path().is_dir() && e.file_name().to_string_lossy().to_lowercase().contains("milkdrop"))
            .find_map(|e| szukaj(&e.path(), 2, &dobry))
    })
}

/// Strona wydań MilkDrop 3 w domyślnej przeglądarce (pobiera użytkownik).
pub fn otworz_strone() {
    #[cfg(windows)]
    {
        use windows::core::{w, HSTRING};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
        // SAFETY: stałe łańcuchy.
        unsafe {
            ShellExecuteW(None, w!("open"), &HSTRING::from("https://github.com/milkdrop2077/MilkDrop3/releases"), None, None, SW_SHOWNORMAL);
        }
    }
}

pub fn ustaw_sciezke(p: &std::path::Path) {
    let _ = std::fs::create_dir_all(crate::settings::dir());
    let _ = std::fs::write(plik_sciezki(), p.to_string_lossy().as_bytes());
}

pub fn dziala() -> bool {
    stan().lock().map(|g| g.is_some()).unwrap_or(false)
}

/// Start i osadzenie (blokuje do ~10 s, wołać z wątku pobocznego).
#[cfg(windows)]
pub fn uruchom(rodzic: isize, r: Prostokat) -> Result<(), String> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::*;
    zamknij();
    // numer tego uruchomienia: zamknij() w trakcie czekania na okno go zmienia → nie osadzamy, kończymy proces
    let moje = URUCHOMIENIE.load(std::sync::atomic::Ordering::SeqCst);
    let exe = sciezka().ok_or("nie wskazano MilkDrop3.exe")?;
    let proces = std::process::Command::new(&exe)
        .current_dir(exe.parent().unwrap_or(std::path::Path::new(".")))
        .spawn()
        .map_err(|e| format!("nie udało się uruchomić {}: {e}", exe.display()))?;
    let pid = proces.id();
    log::info!("MilkDrop: start {} (pid {pid})", exe.display());

    struct Szukaj {
        pid: u32,
        znaleziony: isize,
        pole: i64,
    }
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
        // SAFETY: l to wskaźnik na lokalną strukturę Szukaj (żyje przez EnumWindows).
        let s = unsafe { &mut *(l.0 as *mut Szukaj) };
        let mut p = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut p)) };
        if p == s.pid && unsafe { IsWindowVisible(h) }.as_bool() {
            let mut rc = windows::Win32::Foundation::RECT::default();
            let _ = unsafe { GetWindowRect(h, &mut rc) };
            let pole = ((rc.right - rc.left) as i64) * ((rc.bottom - rc.top) as i64);
            if pole > s.pole {
                s.pole = pole;
                s.znaleziony = h.0 as isize;
            }
        }
        BOOL(1)
    }
    let mut hwnd = 0isize;
    for _ in 0..100 {
        std::thread::sleep(std::time::Duration::from_millis(100));
        let mut s = Szukaj { pid, znaleziony: 0, pole: 0 };
        // SAFETY: callback dostaje wskaźnik na `s`, które żyje do końca wywołania.
        let _ = unsafe { EnumWindows(Some(cb), LPARAM(&mut s as *mut Szukaj as isize)) };
        if s.znaleziony != 0 {
            hwnd = s.znaleziony;
            break;
        }
        if URUCHOMIENIE.load(std::sync::atomic::Ordering::SeqCst) != moje {
            break;
        }
    }
    let mut proces = proces;
    if URUCHOMIENIE.load(std::sync::atomic::Ordering::SeqCst) != moje {
        // użytkownik zmniejszył kartę / schował okno, zanim MilkDrop zdążył wystartować
        let _ = proces.kill();
        let _ = proces.wait();
        log::info!("MilkDrop: uruchomienie przerwane");
        return Ok(());
    }
    if hwnd == 0 {
        let _ = proces.kill();
        return Err("MilkDrop nie pokazał okna w ciągu 10 s".into());
    }
    let h = HWND(hwnd as *mut core::ffi::c_void);
    // SAFETY: uchwyty okien z EnumWindows i okna Szkła; zmiana stylu na okno potomne bez ramki.
    unsafe {
        let styl = WS_CHILD.0 | WS_VISIBLE.0 | WS_CLIPSIBLINGS.0;
        SetWindowLongPtrW(h, GWL_STYLE, styl as isize);
        SetWindowLongPtrW(h, GWL_EXSTYLE, 0);
        let _ = SetParent(h, HWND(rodzic as *mut core::ffi::c_void));
        let _ = SetWindowPos(h, HWND_TOP, r.x, r.y, r.w, r.h, SWP_SHOWWINDOW | SWP_FRAMECHANGED);
    }
    if let Ok(mut g) = stan().lock() {
        *g = Some(Dziala { proces, hwnd });
    }
    if URUCHOMIENIE.load(std::sync::atomic::Ordering::SeqCst) != moje {
        zamknij(); // zamknięcie trafiło dokładnie między sprawdzeniem a zapisem stanu
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn uruchom(_: isize, _: Prostokat) -> Result<(), String> {
    Err("tylko Windows".into())
}

/// Nowe położenie (zmiana rozmiaru okna Szkła).
pub fn przesun(r: Prostokat) {
    #[cfg(windows)]
    if let Ok(g) = stan().lock() {
        if let Some(d) = g.as_ref() {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{SetWindowPos, HWND_TOP, SWP_SHOWWINDOW};
            // SAFETY: uchwyt osadzonego okna MilkDrop.
            unsafe {
                let _ = SetWindowPos(HWND(d.hwnd as *mut core::ffi::c_void), HWND_TOP, r.x, r.y, r.w, r.h, SWP_SHOWWINDOW);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = r;
}

/// Zmieniany przez `zamknij()` – przerywa uruchomienie, które jeszcze czeka na okno.
static URUCHOMIENIE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn zamknij() {
    URUCHOMIENIE.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    if let Some(mut d) = stan().lock().ok().and_then(|mut g| g.take()) {
        let _ = d.proces.kill();
        let _ = d.proces.wait();
        log::info!("MilkDrop: zamknięty");
    }
}
