//! Czy Equalizer APO jest zainstalowany na DOMYŚLNYM urządzeniu wyjściowym Windows?
//! Najczęstsza przyczyna „Czekam na dźwięk”: APO wybrano w Configuratorze dla słuchawek BT/innego wyjścia,
//! a muzyka gra teraz na urządzeniu bez APO – wtyczka nigdy się nie ładuje.
//!
//! Tylko odczyt rejestru: MMDevices\Audio\Render\{guid} (nazwa, stan) i EqualizerAPO\Child APOs\{guid}.

/// (przyjazna nazwa domyślnego wyjścia, czy ma Equalizer APO). `None`, gdy nie da się ustalić.
#[cfg(windows)]
pub fn domyslne_wyjscie() -> Option<(String, bool)> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let nazwa = cpal::default_host().default_output_device()?.name().ok()?;
    let mut znalezione: Option<bool> = None;
    for guid in rejestr::podklucze(r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render") {
        let baza = format!(r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render\{guid}");
        if rejestr::dword(&baza, "DeviceState") != Some(1) {
            continue; // tylko aktywne
        }
        let props = format!(r"{baza}\Properties");
        let (Some(a), Some(b)) = (
            rejestr::tekst(&props, "{a45c254e-df1c-4efd-8020-67d146a850e0},2"),
            rejestr::tekst(&props, "{b3f8fa53-0004-438e-9003-51a46e139bfc},6"),
        ) else {
            continue;
        };
        if format!("{a} ({b})") == nazwa {
            znalezione = Some(rejestr::istnieje(&format!(r"SOFTWARE\EqualizerAPO\Child APOs\{guid}")));
            break;
        }
    }
    znalezione.map(|ma| (nazwa, ma))
}

#[cfg(not(windows))]
pub fn domyslne_wyjscie() -> Option<(String, bool)> {
    None
}

#[cfg(windows)]
mod rejestr {
    use windows::core::HSTRING;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegGetValueW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY,
        RRF_RT_REG_DWORD, RRF_RT_REG_SZ,
    };

    fn otworz(sciezka: &str) -> Option<HKEY> {
        let mut h = HKEY::default();
        // SAFETY: poprawny HSTRING i wskaźnik na lokalny HKEY.
        let r = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, &HSTRING::from(sciezka), 0, KEY_READ | KEY_WOW64_64KEY, &mut h) };
        r.is_ok().then_some(h)
    }

    pub fn istnieje(sciezka: &str) -> bool {
        match otworz(sciezka) {
            Some(h) => {
                // SAFETY: klucz otwarty powyżej.
                unsafe {
                    let _ = RegCloseKey(h);
                }
                true
            }
            None => false,
        }
    }

    pub fn podklucze(sciezka: &str) -> Vec<String> {
        let Some(h) = otworz(sciezka) else { return Vec::new() };
        let mut v = Vec::new();
        let mut i = 0u32;
        loop {
            let mut buf = [0u16; 256];
            let mut len = buf.len() as u32;
            // SAFETY: bufor i długość lokalne, klucz otwarty.
            let r = unsafe { RegEnumKeyExW(h, i, windows::core::PWSTR(buf.as_mut_ptr()), &mut len, None, windows::core::PWSTR::null(), None, None) };
            if r.is_err() {
                break;
            }
            v.push(String::from_utf16_lossy(&buf[..len as usize]));
            i += 1;
        }
        // SAFETY: klucz otwarty powyżej.
        unsafe {
            let _ = RegCloseKey(h);
        }
        v
    }

    pub fn dword(sciezka: &str, nazwa: &str) -> Option<u32> {
        let mut v = 0u32;
        let mut len = 4u32;
        // SAFETY: wskaźniki na lokalne zmienne o zadeklarowanym rozmiarze.
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &HSTRING::from(sciezka),
                &HSTRING::from(nazwa),
                RRF_RT_REG_DWORD,
                None,
                Some(&mut v as *mut u32 as *mut _),
                Some(&mut len),
            )
        };
        r.is_ok().then_some(v)
    }

    pub fn tekst(sciezka: &str, nazwa: &str) -> Option<String> {
        let mut buf = [0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        // SAFETY: bufor lokalny, długość w bajtach.
        let r = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                &HSTRING::from(sciezka),
                &HSTRING::from(nazwa),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr() as *mut _),
                Some(&mut len),
            )
        };
        if r.is_err() {
            return None;
        }
        let n = (len as usize / 2).saturating_sub(1); // bez końcowego zera
        Some(String::from_utf16_lossy(&buf[..n.min(buf.len())]))
    }
}
