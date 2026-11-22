//! Serwer named pipe `\\.\pipe\szklo-ctl` w procesie audiodg.exe (Equalizer APO).
//!
//! Tryb PIPE_NOWAIT: ConnectNamedPipe/ReadFile/WriteFile nie blokują, więc wątek co ~33 ms sprawdza
//! flagę zatrzymania – `Drop` może bezpiecznie poczekać na wątek (Equalizer APO wyładowuje DLL przy
//! zmianie konfiguracji; wątek nie może przeżyć kodu biblioteki).
//! Potok tworzy sam wątek i ponawia próbę co ~0,5 s: nazwa bywa chwilowo zajęta przez poprzednią instancję
//! (FILE_FLAG_FIRST_PIPE_INSTANCE), która jest właśnie usuwana.
//! DACL: odczyt/zapis dla zalogowanych użytkowników (audiodg działa jako LOCAL SERVICE, GUI jako użytkownik).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;
use szklo_core::ipc::{apply_params_line, encode_meters, unique_pipe_path};
use szklo_core::{SharedMeters, SharedParams};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, LocalFree, ERROR_NO_DATA, ERROR_PIPE_CONNECTED, ERROR_PIPE_LISTENING, HANDLE, HLOCAL};
use windows::Win32::Security::Authorization::{ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1};
use windows::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
use windows::Win32::Storage::FileSystem::{ReadFile, WriteFile, FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, DisconnectNamedPipe, PeekNamedPipe, PIPE_NOWAIT, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE,
};

/// Czy działamy w usłudze audio Windows (a nie np. w edytorze konfiguracji Equalizer APO)?
pub fn is_audio_service() -> bool {
    if std::env::var_os("SZKLO_PIPE_ANYWHERE").is_some() {
        return true;
    }
    std::env::current_exe()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()))
        .map(|n| n == "audiodg.exe")
        .unwrap_or(false)
}

pub struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

struct Pipe(HANDLE);
// SAFETY: uchwyt potoku jest używany wyłącznie z jednego wątku serwera.
unsafe impl Send for Pipe {}

impl Drop for Pipe {
    fn drop(&mut self) {
        // SAFETY: uchwyt należy do nas i jest zamykany dokładnie raz.
        unsafe {
            let _ = DisconnectNamedPipe(self.0);
            let _ = CloseHandle(self.0);
        }
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn create_pipe(path: &str) -> Option<Pipe> {
    let name = wide(path);
    // GA dla SYSTEM i LOCAL SERVICE, RW dla uwierzytelnionych użytkowników (AU).
    let sddl = wide("D:(A;;GA;;;SY)(A;;GA;;;LS)(A;;GRGW;;;AU)");
    let mut sd = PSECURITY_DESCRIPTOR::default();
    // SAFETY: wywołania Win32 z poprawnymi, zakończonymi zerem łańcuchami; deskryptor zwalniany LocalFree.
    unsafe {
        if ConvertStringSecurityDescriptorToSecurityDescriptorW(PCWSTR(sddl.as_ptr()), SDDL_REVISION_1, &mut sd, None).is_err() {
            return None;
        }
        let sa = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd.0,
            bInheritHandle: false.into(),
        };
        let h = CreateNamedPipeW(
            PCWSTR(name.as_ptr()),
            PIPE_ACCESS_DUPLEX | FILE_FLAG_FIRST_PIPE_INSTANCE,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_NOWAIT,
            1,
            8192,
            8192,
            0,
            Some(&sa),
        );
        let _ = LocalFree(HLOCAL(sd.0));
        if h.is_invalid() {
            None
        } else {
            Some(Pipe(h))
        }
    }
}

impl Server {
    pub fn start(params: Arc<SharedParams>, meters: Arc<SharedMeters>, status: Arc<Mutex<(f64, usize)>>) -> Option<Server> {
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let thread = std::thread::Builder::new()
            .name("szklo-pipe".into())
            .spawn(move || serve(params, meters, status, s2))
            .ok()?;
        Some(Server { stop, thread: Some(thread) })
    }
}

/// Sen przerywany flagą zatrzymania (żeby `Drop` nie czekał dłużej niż ~50 ms).
fn nap(stop: &AtomicBool, ms: u64) {
    let mut left = ms;
    while left > 0 && !stop.load(Ordering::Relaxed) {
        let d = left.min(50);
        std::thread::sleep(Duration::from_millis(d));
        left -= d;
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn serve(params: Arc<SharedParams>, meters: Arc<SharedMeters>, status: Arc<Mutex<(f64, usize)>>, stop: Arc<AtomicBool>) {
    let mut pipe: Option<Pipe> = None;
    let mut connected = false;
    let mut fails: u64 = 0;
    // Unikalna nazwa potoku tego serwera: kilka wersji DLL / urządzeń w audiodg.exe nie zabiera sobie kanału,
    // a aplikacja łączy się ze wszystkimi (`szklo-ctl-*`) i sama wybiera ten, który przetwarza dźwięk.
    let path = unique_pipe_path();
    let mut acc: Vec<u8> = Vec::with_capacity(4096);
    let mut buf = [0u8; 4096];
    while !stop.load(Ordering::Relaxed) {
        let h = match pipe.as_ref() {
            Some(p) => p.0,
            None => match create_pipe(&path) {
                Some(p) => {
                    let h = p.0;
                    pipe = Some(p);
                    connected = false;
                    crate::diag::log(&format!("potok utworzony: {path}"));
                    h
                }
                None => {
                    if fails % 20 == 0 {
                        crate::diag::log(&format!("nie mogę utworzyć potoku (zajęty?) – próba {fails}"));
                    }
                    fails += 1;
                    nap(&stop, 500);
                    continue;
                }
            },
        };
        // SAFETY: wszystkie wywołania na naszym uchwycie potoku, bufory lokalne o znanym rozmiarze.
        unsafe {
            if !connected {
                match ConnectNamedPipe(h, None) {
                    Ok(()) => {
                        connected = true;
                        crate::diag::log("aplikacja połączona z potokiem");
                    }
                    Err(_) => {
                        let e = GetLastError();
                        if e == ERROR_PIPE_CONNECTED {
                            connected = true;
                        } else if e == ERROR_NO_DATA {
                            // klient się rozłączył – zwolnij instancję
                            let _ = DisconnectNamedPipe(h);
                        } else if e != ERROR_PIPE_LISTENING {
                            let _ = DisconnectNamedPipe(h);
                        }
                    }
                }
                if !connected {
                    nap(&stop, 100);
                    continue;
                }
                acc.clear();
            }
            // odbiór poleceń
            let mut avail = 0u32;
            if PeekNamedPipe(h, None, 0, None, Some(&mut avail), None).is_err() {
                let _ = DisconnectNamedPipe(h);
                connected = false;
                continue;
            }
            if avail > 0 {
                let mut read = 0u32;
                if ReadFile(h, Some(&mut buf[..]), Some(&mut read), None).is_ok() && read > 0 {
                    acc.extend_from_slice(&buf[..read as usize]);
                    while let Some(pos) = acc.iter().position(|&b| b == b'\n') {
                        let line: Vec<u8> = acc.drain(..=pos).collect();
                        if let Ok(s) = std::str::from_utf8(&line) {
                            apply_params_line(s.trim(), &params);
                        }
                    }
                    if acc.len() > 65536 {
                        acc.clear();
                    }
                }
            }
            // wysyłka mierników (~30 Hz, niezależnie od tego, czy płynie dźwięk)
            let (fs, lat) = status.lock().map(|g| *g).unwrap_or((0.0, 0));
            let line = encode_meters(&meters.snapshot(), fs, lat);
            let mut written = 0u32;
            if WriteFile(h, Some(line.as_bytes()), Some(&mut written), None).is_err() {
                let _ = DisconnectNamedPipe(h);
                connected = false;
                continue;
            }
        }
        std::thread::sleep(Duration::from_millis(33));
    }
    drop(pipe);
}
