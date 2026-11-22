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
    }
    let mut proces = proces;
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

pub fn zamknij() {
    if let Some(mut d) = stan().lock().ok().and_then(|mut g| g.take()) {
        let _ = d.proces.kill();
        let _ = d.proces.wait();
        log::info!("MilkDrop: zamknięty");
    }
}
