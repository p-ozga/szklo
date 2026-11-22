//! Sterowanie aplikacją Spotify (i innymi odtwarzaczami) przez systemowe sterowanie multimediami Windows
//! (Global System Media Transport Controls). Szkło nie odtwarza tu dźwięku – gra aplikacja Spotify, a Szkło
//! przetwarza jej dźwięk (Equalizer APO albo silnik w aplikacji) i pokazuje tytuł, okładkę, postęp i przyciski.

use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

#[derive(Clone, Serialize, Default, PartialEq)]
pub struct Zewn {
    /// np. „Spotify”; pusty = brak sesji
    pub aplikacja: String,
    pub tytul: String,
    pub wykonawca: String,
    pub album: String,
    pub gra: bool,
    pub pozycja: f64,
    pub czas: f64,
    pub losowo: bool,
    /// wyl | wszystko | jeden
    pub powtarzanie: String,
    /// okładka jako data:image/…;base64 (pusta, gdy brak) – NIE idzie w zwykłym stanie (bywa wielka):
    /// UI dostaje ją osobno, tylko gdy zmieni się `okladka_nr`
    #[serde(skip)]
    pub okladka: String,
    pub okladka_nr: u32,
}

/// Okno schowane → odpytywanie multimediów co 2 s zamiast co 0,4 s.
static WOLNO: AtomicBool = AtomicBool::new(false);
pub fn zwolnij(b: bool) {
    WOLNO.store(b, Ordering::Relaxed);
}

pub enum Rozkaz {
    PlayPauza,
    Nastepny,
    Poprzedni,
    Szukaj(f64),
    Losowo(bool),
    Powtarzanie(String),
}

pub struct Media {
    tx: Sender<Rozkaz>,
    pub stan: Arc<Mutex<Zewn>>,
    pub zmiana: Arc<AtomicBool>,
}

impl Media {
    pub fn rozkaz(&self, r: Rozkaz) {
        let _ = self.tx.send(r);
    }
}

pub fn get() -> &'static Media {
    static M: OnceLock<Media> = OnceLock::new();
    M.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel();
        let stan = Arc::new(Mutex::new(Zewn::default()));
        let zmiana = Arc::new(AtomicBool::new(true));
        let (s2, z2) = (stan.clone(), zmiana.clone());
        std::thread::Builder::new()
            .name("szklo-media".into())
            .spawn(move || {
                #[cfg(windows)]
                petla(rx, s2, z2);
                #[cfg(not(windows))]
                let _ = (rx, s2, z2);
            })
            .expect("wątek multimediów");
        Media { tx, stan, zmiana }
    })
}

#[cfg(windows)]
fn petla(rx: Receiver<Rozkaz>, stan: Arc<Mutex<Zewn>>, zmiana: Arc<AtomicBool>) {
    use windows::Media::Control::*;
    // SAFETY: inicjalizacja WinRT dla tego wątku (MTA).
    let _ = unsafe { windows::Win32::System::WinRT::RoInitialize(windows::Win32::System::WinRT::RO_INIT_MULTITHREADED) };
    let menedzer = loop {
        match GlobalSystemMediaTransportControlsSessionManager::RequestAsync().and_then(|a| a.get()) {
            Ok(m) => break m,
            Err(e) => {
                log::warn!("multimedia Windows niedostępne: {e}");
                std::thread::sleep(Duration::from_secs(10));
            }
        }
    };
    let mut ost_okladka_klucz = String::new();
    let mut okladka = String::new();
    let mut okladka_nr = 0u32;
    // rozkaz odebrany w trakcie czekania – obsłużony jako pierwszy w następnym obiegu (kolejność zachowana)
    let mut oczekujacy: Option<Rozkaz> = None;
    loop {
        // sesja: najpierw Spotify, potem bieżąca
        let sesja = (|| -> Option<GlobalSystemMediaTransportControlsSession> {
            let lista = menedzer.GetSessions().ok()?;
            let n = lista.Size().ok()?;
            for i in 0..n {
                let s = lista.GetAt(i).ok()?;
                if s.SourceAppUserModelId().map(|a| a.to_string().to_lowercase().contains("spotify")).unwrap_or(false) {
                    return Some(s);
                }
            }
            menedzer.GetCurrentSession().ok()
        })();

        // rozkazy
        while let Some(r) = oczekujacy.take().or_else(|| rx.try_recv().ok()) {
            let Some(s) = sesja.as_ref() else { continue };
            let wynik = match r {
                Rozkaz::PlayPauza => s.TryTogglePlayPauseAsync().map(|_| ()),
                Rozkaz::Nastepny => s.TrySkipNextAsync().map(|_| ()),
                Rozkaz::Poprzedni => s.TrySkipPreviousAsync().map(|_| ()),
                Rozkaz::Szukaj(sek) => s.TryChangePlaybackPositionAsync((sek * 10_000_000.0) as i64).map(|_| ()),
                Rozkaz::Losowo(b) => s.TryChangeShuffleActiveAsync(b).map(|_| ()),
                Rozkaz::Powtarzanie(p) => {
                    let tryb = match p.as_str() {
                        "wszystko" => windows::Media::MediaPlaybackAutoRepeatMode::List,
                        "jeden" => windows::Media::MediaPlaybackAutoRepeatMode::Track,
                        _ => windows::Media::MediaPlaybackAutoRepeatMode::None,
                    };
                    s.TryChangeAutoRepeatModeAsync(tryb).map(|_| ())
                }
            };
            if let Err(e) = wynik {
                log::warn!("sterowanie odtwarzaczem: {e}");
            }
        }

        let mut z = Zewn::default();
        if let Some(s) = sesja.as_ref() {
            let app = s.SourceAppUserModelId().map(|a| a.to_string()).unwrap_or_default();
            z.aplikacja = if app.to_lowercase().contains("spotify") {
                "Spotify".into()
            } else {
                app.rsplit(['\\', '!']).next().unwrap_or(&app).trim_end_matches(".exe").to_string()
            };
            if let Ok(p) = s.TryGetMediaPropertiesAsync().and_then(|a| a.get()) {
                z.tytul = p.Title().map(|t| t.to_string()).unwrap_or_default();
                z.wykonawca = p.Artist().map(|t| t.to_string()).unwrap_or_default();
                z.album = p.AlbumTitle().map(|t| t.to_string()).unwrap_or_default();
                let klucz = format!("{}|{}|{}", z.tytul, z.wykonawca, z.album);
                if klucz != ost_okladka_klucz {
                    ost_okladka_klucz = klucz;
                    okladka = p.Thumbnail().ok().and_then(|t| czytaj_okladke(&t)).unwrap_or_default();
                    okladka_nr = okladka_nr.wrapping_add(1);
                }
                z.okladka_nr = okladka_nr;
            }
            if let Ok(t) = s.GetTimelineProperties() {
                let sek = |ts: windows::Foundation::TimeSpan| ts.Duration as f64 / 10_000_000.0;
                z.czas = t.EndTime().map(sek).unwrap_or(0.0);
                let poz = t.Position().map(sek).unwrap_or(0.0);
                // pozycja jest aktualizowana przez aplikację rzadko – doliczamy czas od ostatniej aktualizacji
                let gra = s.GetPlaybackInfo().and_then(|i| i.PlaybackStatus()).map(|st| st == GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing).unwrap_or(false);
                let od = t.LastUpdatedTime().ok().map(|u| {
                    // DateTime w 100 ns od 1601 r.
                    let teraz = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) + 11_644_473_600.0;
                    (teraz - u.UniversalTime as f64 / 10_000_000.0).max(0.0)
                });
                z.pozycja = if gra { (poz + od.unwrap_or(0.0)).min(z.czas.max(poz)) } else { poz };
                z.gra = gra;
            }
            if let Ok(i) = s.GetPlaybackInfo() {
                z.losowo = i.IsShuffleActive().and_then(|v| v.Value()).unwrap_or(false);
                z.powtarzanie = match i.AutoRepeatMode().and_then(|v| v.Value()) {
                    Ok(windows::Media::MediaPlaybackAutoRepeatMode::List) => "wszystko",
                    Ok(windows::Media::MediaPlaybackAutoRepeatMode::Track) => "jeden",
                    _ => "wyl",
                }
                .into();
            }
        }
        if let Ok(mut g) = stan.lock() {
            // okładka (duża) kopiowana tylko przy zmianie numeru; porównanie stanu jej nie obejmuje
            let nowa = g.okladka_nr != z.okladka_nr;
            let stara = std::mem::take(&mut g.okladka); // teraz obie strony porównania mają pustą okładkę
            let brak_sesji = z.aplikacja.is_empty();
            if *g != z {
                *g = z;
                zmiana.store(true, Ordering::Relaxed);
            }
            g.okladka = if brak_sesji {
                String::new()
            } else if nowa {
                okladka.clone()
            } else {
                stara
            };
        }
        // czekaj ~0,4 s (okno schowane: 2 s) albo do rozkazu
        let czekaj = if WOLNO.load(Ordering::Relaxed) { 2000 } else { 400 };
        oczekujacy = rx.recv_timeout(Duration::from_millis(czekaj)).ok();
    }
}

#[cfg(windows)]
fn czytaj_okladke(t: &windows::Storage::Streams::IRandomAccessStreamReference) -> Option<String> {
    use base64::Engine as _;
    use windows::Storage::Streams::{DataReader, InputStreamOptions};
    let s = t.OpenReadAsync().ok()?.get().ok()?;
    let typ = s.ContentType().map(|c| c.to_string()).unwrap_or_else(|_| "image/jpeg".into());
    let n = s.Size().ok()? as u32;
    if n == 0 || n > 4_000_000 {
        return None;
    }
    let czyt = DataReader::CreateDataReader(&s).ok()?;
    czyt.SetInputStreamOptions(InputStreamOptions::None).ok()?;
    czyt.LoadAsync(n).ok()?.get().ok()?;
    let mut bufor = vec![0u8; n as usize];
    czyt.ReadBytes(&mut bufor).ok()?;
    Some(format!("data:{};base64,{}", if typ.is_empty() { "image/jpeg" } else { &typ }, base64::engine::general_purpose::STANDARD.encode(bufor)))
}
