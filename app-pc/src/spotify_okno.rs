//! „Spotify w tle”: oficjalna aplikacja Spotify gra dalej, ale jej okno jest schowane – nie ma jej na pasku zadań
//! ani pod Alt+Tab. Szkło steruje nią przez multimedia Windows (media_win) i otwiera utwory linkami spotify:.
//!
//! Niczego w aplikacji Spotify nie zmieniamy: tylko chowamy/pokazujemy jej główne okno (to samo robi jej własna
//! opcja „minimalizuj do zasobnika”). Przy zamknięciu Szkła okno wraca na pasek (zminimalizowane), żeby program
//! nie został niewidoczny.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static UKRYWAJ: AtomicBool = AtomicBool::new(false);
static WATEK: AtomicBool = AtomicBool::new(false);
/// Ile najbliższych obiegów pętli ma być „szybkich” (25 ms zamiast 250 ms).
static SZYBKO: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Uchwyty okien, które schowaliśmy (żeby pokazać dokładnie te same).
fn schowane() -> &'static Mutex<Vec<isize>> {
    static S: Mutex<Vec<isize>> = Mutex::new(Vec::new());
    &S
}

pub fn wlaczone() -> bool {
    UKRYWAJ.load(Ordering::Relaxed)
}

/// Włącza/wyłącza chowanie. Wyłączenie od razu pokazuje okno Spotify.
pub fn ustaw(ukryj: bool) {
    UKRYWAJ.store(ukryj, Ordering::Relaxed);
    if ukryj {
        if !WATEK.swap(true, Ordering::Relaxed) {
            std::thread::Builder::new().name("spotify-okno".into()).spawn(petla).ok();
        }
    } else {
        pokaz(true);
    }
}

/// Przed zamknięciem Szkła: Spotify wraca na pasek zadań (zminimalizowany, bez zabierania fokusu).
pub fn przy_wyjsciu() {
    if UKRYWAJ.swap(false, Ordering::Relaxed) {
        pokaz(false);
    }
}

fn petla() {
    // Co 250 ms: tanie przejście po oknach najwyższego poziomu; nazwa procesu sprawdzana raz na PID.
    let mut znane: std::collections::HashMap<u32, bool> = std::collections::HashMap::new();
    loop {
        // zaraz po otwarciu linku spotify: aplikacja wyskakuje na wierzch – wtedy sprawdzaj gęściej, żeby mignęła jak najkrócej
        let szybko = SZYBKO.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1)).is_ok();
        std::thread::sleep(std::time::Duration::from_millis(if szybko { 25 } else { 250 }));
        if !UKRYWAJ.load(Ordering::Relaxed) {
            // pauza do ponownego włączenia – wątek zostaje, ale nic nie robi
            std::thread::sleep(std::time::Duration::from_millis(750));
            continue;
        }
        if znane.len() > 512 {
            znane.clear();
        }
        for h in okna_spotify(true, &mut znane) {
            schowaj(h);
        }
    }
}

/// Uruchamia aplikację Spotify, jeśli nie działa (link „spotify:” – działa dla wersji ze Sklepu i z instalatora).
pub fn uruchom_aplikacje(uri: &str) {
    #[cfg(windows)]
    {
        use windows::core::{w, HSTRING};
        use windows::Win32::UI::Shell::ShellExecuteW;
        use windows::Win32::UI::WindowsAndMessaging::{SW_SHOWMINNOACTIVE, SW_SHOWNORMAL};
        let jak = if wlaczone() { SW_SHOWMINNOACTIVE } else { SW_SHOWNORMAL };
        SZYBKO.store(240, Ordering::Relaxed); // ~6 s gęstego sprawdzania
        // SAFETY: łańcuchy żyją do końca wywołania.
        unsafe {
            ShellExecuteW(None, w!("open"), &HSTRING::from(uri), None, None, jak);
        }
    }
    #[cfg(not(windows))]
    let _ = uri;
}

#[cfg(windows)]
fn schowaj(h: isize) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};
    // SAFETY: uchwyt okna z EnumWindows; ShowWindow na nieistniejącym oknie po prostu zawiedzie.
    unsafe {
        let _ = ShowWindow(HWND(h as *mut core::ffi::c_void), SW_HIDE);
    }
    if let Ok(mut s) = schowane().lock() {
        if !s.contains(&h) {
            s.push(h);
            log::info!("Spotify w tle: okno schowane");
        }
    }
}
#[cfg(not(windows))]
fn schowaj(_: isize) {}

/// Pokazuje schowane okna. `na_wierzch`: przywróć i aktywuj (kliknięcie użytkownika) albo tylko na pasek.
#[cfg(windows)]
fn pokaz(na_wierzch: bool) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{IsWindow, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOWMINNOACTIVE};
    let mut lista: Vec<isize> = schowane().lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default();
    // SAFETY: IsWindow sprawdza uchwyt przed użyciem.
    lista.retain(|h| unsafe { IsWindow(HWND(*h as *mut core::ffi::c_void)) }.as_bool());
    if lista.is_empty() {
        // np. Szkło uruchomione ponownie, gdy Spotify było już schowane: główne okno = niewidoczne, z tytułem
        lista = okna_spotify(false, &mut std::collections::HashMap::new());
    }
    for h in lista {
        let hw = HWND(h as *mut core::ffi::c_void);
        // SAFETY: uchwyt sprawdzony wyżej / świeżo z EnumWindows.
        unsafe {
            if na_wierzch {
                let _ = ShowWindow(hw, SW_RESTORE);
                let _ = SetForegroundWindow(hw);
            } else {
                let _ = ShowWindow(hw, SW_SHOWMINNOACTIVE);
            }
        }
        log::info!("Spotify w tle: okno pokazane");
    }
}
#[cfg(not(windows))]
fn pokaz(_: bool) {}

/// Główne okna aplikacji Spotify: proces Spotify.exe, klasa Chrome_WidgetWin_1, bez właściciela, z tytułem.
/// `widoczne`: szukaj widocznych (do schowania) albo niewidocznych (do pokazania).
#[cfg(windows)]
fn okna_spotify(widoczne: bool, znane: &mut std::collections::HashMap<u32, bool>) -> Vec<isize> {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::*;
    struct Stan<'a> {
        widoczne: bool,
        znane: &'a mut std::collections::HashMap<u32, bool>,
        wynik: Vec<isize>,
    }
    unsafe extern "system" fn cb(h: HWND, l: LPARAM) -> BOOL {
        // SAFETY: l wskazuje na Stan żyjący przez całe EnumWindows.
        let s = unsafe { &mut *(l.0 as *mut Stan) };
        if unsafe { IsWindowVisible(h) }.as_bool() != s.widoczne {
            return BOOL(1);
        }
        let mut klasa = [0u16; 32];
        let n = unsafe { GetClassNameW(h, &mut klasa) } as usize;
        if String::from_utf16_lossy(&klasa[..n]) != "Chrome_WidgetWin_1" {
            return BOOL(1);
        }
        if unsafe { GetWindow(h, GW_OWNER) }.is_ok_and(|o| !o.is_invalid()) || unsafe { GetWindowTextLengthW(h) } == 0 {
            return BOOL(1);
        }
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(h, Some(&mut pid)) };
        let spotify = *s.znane.entry(pid).or_insert_with(|| czy_spotify(pid));
        if spotify {
            s.wynik.push(h.0 as isize);
        }
        BOOL(1)
    }
    let mut s = Stan { widoczne, znane, wynik: Vec::new() };
    // SAFETY: callback dostaje wskaźnik na `s`, które żyje do końca wywołania.
    let _ = unsafe { EnumWindows(Some(cb), LPARAM(&mut s as *mut Stan as isize)) };
    s.wynik
}
#[cfg(not(windows))]
fn okna_spotify(_: bool, _: &mut std::collections::HashMap<u32, bool>) -> Vec<isize> {
    Vec::new()
}

#[cfg(windows)]
fn czy_spotify(pid: u32) -> bool {
    use windows::core::PWSTR;
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION};
    // SAFETY: uchwyt procesu zamykany; bufor lokalny.
    unsafe {
        let Ok(p) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
        let mut buf = [0u16; 520];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(p, PROCESS_NAME_FORMAT(0), PWSTR(buf.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(p);
        ok && String::from_utf16_lossy(&buf[..len as usize]).to_lowercase().ends_with("\\spotify.exe")
    }
}

/// Czy aplikacja Spotify ma jakiekolwiek główne okno (widoczne albo schowane) – czyli czy działa.
pub fn aplikacja_dziala() -> bool {
    #[cfg(windows)]
    {
        let mut z = std::collections::HashMap::new();
        !okna_spotify(true, &mut z).is_empty() || !okna_spotify(false, &mut z).is_empty()
    }
    #[cfg(not(windows))]
    false
}
