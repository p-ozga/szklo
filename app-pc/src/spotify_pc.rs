//! Spotify w Szkle na PC (Premium): logowanie OAuth w przeglądarce, biblioteka i szukanie ([`szklo_spotify`]), odtwarzanie przez librespot
//! (otwarty, nieoficjalny klient Spotify – MIT). Dźwięk (44,1 kHz) trafia do bufora, z którego bierze go wbudowany
//! odtwarzacz ([`crate::odtwarzacz`]) – ten sam tor co pliki z dysku: przeliczenie częstotliwości → silnik Szkła.
//!
//! Spotify i Spotify Connect to znaki towarowe Spotify AB; Szkło nie jest powiązane ze Spotify.

use crate::odtwarzacz::Pozycja;
use librespot_core::{authentication::Credentials, cache::Cache, session::Session, SessionConfig, SpotifyUri};
use librespot_playback::audio_backend::{Sink, SinkResult};
use librespot_playback::config::PlayerConfig;
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::NoOpVolume;
use librespot_playback::player::{Player, PlayerEvent};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use serde::Serialize;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT_PORT: u16 = 5588;
const SCOPES: &str = "streaming user-read-private user-read-email playlist-read-private playlist-read-collaborative user-library-read user-read-playback-state";
pub const FS: u32 = 44_100;

#[derive(Clone, Serialize, Default)]
pub struct SpStan {
    /// wylogowany | laczenie | login | gotowe | blad
    pub status: String,
    pub opis: String,
    pub konto: String,
}

enum Rozkaz {
    Zaloguj,
    Wyloguj,
    /// biblioteka i szukanie (polecenie dla [`szklo_spotify::Kontroler`])
    Biblioteka(serde_json::Value),
    Graj(String, u32),
    Pauza,
    Wznow,
    Szukaj(u32),
    Stop,
}

/// Bufor PCM (stereo f32, 44,1 kHz) dla odtwarzacza + znaczniki stanu utworu.
pub struct Strumien {
    pub cons: Mutex<Option<HeapCons<f32>>>,
    /// librespot skończył utwór (EndOfTrack) – po opróżnieniu bufora odtwarzacz przechodzi dalej
    pub koniec: AtomicBool,
    /// utwór niedostępny / błąd ładowania
    pub blad: AtomicBool,
    /// pozycja zgłoszona przez librespot (ms) – podstawa zegara
    pub pozycja_ms: AtomicU32,
    /// wyczyść bufor przy następnym odczycie (nowy utwór, przewinięcie)
    pub czysc: AtomicBool,
}

pub struct Spotify {
    tx: tokio::sync::mpsc::UnboundedSender<Rozkaz>,
    pub stan: Arc<Mutex<SpStan>>,
    pub zmiana: Arc<AtomicBool>,
}

fn spotify() -> &'static OnceLock<Spotify> {
    static S: OnceLock<Spotify> = OnceLock::new();
    &S
}

pub fn strumien() -> &'static Strumien {
    static S: OnceLock<Strumien> = OnceLock::new();
    S.get_or_init(|| Strumien {
        cons: Mutex::new(None),
        koniec: AtomicBool::new(false),
        blad: AtomicBool::new(false),
        pozycja_ms: AtomicU32::new(0),
        czysc: AtomicBool::new(false),
    })
}

pub fn get() -> &'static Spotify {
    spotify().get_or_init(|| {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let stan = Arc::new(Mutex::new(SpStan { status: "wylogowany".into(), ..Default::default() }));
        let zmiana = Arc::new(AtomicBool::new(true));
        let (s2, z2) = (stan.clone(), zmiana.clone());
        std::thread::Builder::new()
            .name("szklo-spotify".into())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio");
                rt.block_on(petla(rx, s2, z2));
            })
            .expect("wątek Spotify");
        // zapisane poświadczenia → logowanie bez przeglądarki
        if katalog().join("credentials.json").exists() {
            let _ = tx.send(Rozkaz::Zaloguj);
        }
        Spotify { tx, stan, zmiana }
    })
}

/// Spotify odmówił klucza do odszyfrowania utworu (albo nie odpowiedział). Znany problem nieoficjalnych klientów
/// (librespot #1649) – zatrzymujemy odtwarzanie zamiast pytać o klucze kolejnych utworów.
static ODMOWA: AtomicBool = AtomicBool::new(false);
pub const KOMUNIKAT_ODMOWY: &str = "Spotify nie wydał klucza do odtworzenia utworu na tym komputerze (blokada nieoficjalnych odtwarzaczy po stronie Spotify). Playlisty działają, ale odtwarzanie w Szkle jest niemożliwe – graj w aplikacji Spotify, Szkło i tak przetworzy jej dźwięk.";

pub fn odmowa_klucza() {
    if !ODMOWA.swap(true, Ordering::Relaxed) {
        if let Some(s) = aktywny() {
            ustaw(&s.stan, &s.zmiana, |st| st.opis = KOMUNIKAT_ODMOWY.into());
        }
    }
}

pub fn byla_odmowa() -> bool {
    ODMOWA.load(Ordering::Relaxed)
}

/// Moduł, jeśli już uruchomiony.
pub fn aktywny() -> Option<&'static Spotify> {
    spotify().get()
}

/// Start przy uruchomieniu programu tylko wtedy, gdy są zapisane poświadczenia (automatyczne logowanie).
pub fn start_jesli_zalogowany() {
    if katalog().join("credentials.json").exists() {
        let _ = get();
    }
}

/// Biblioteka i szukanie (wspólne z Androidem). Opisy utworów zapisuje w prywatnym folderze programu.
pub fn biblioteka() -> &'static szklo_spotify::Kontroler {
    static K: OnceLock<szklo_spotify::Kontroler> = OnceLock::new();
    K.get_or_init(|| szklo_spotify::Kontroler::nowy(Some(katalog())))
}

pub fn stan_json() -> serde_json::Value {
    aktywny()
        .and_then(|s| s.stan.lock().ok().map(|g| serde_json::to_value(&*g).unwrap_or_default()))
        .unwrap_or_else(|| serde_json::json!({ "status": "wylogowany", "opis": "", "konto": "" }))
}

/// Zatrzymuje odtwarzanie Spotify, jeśli moduł był już uruchomiony (bez startowania go).
pub fn stop_jesli_aktywny() {
    if let Some(s) = spotify().get() {
        s.stop();
    }
}

impl Spotify {
    pub fn zaloguj(&self) {
        let _ = self.tx.send(Rozkaz::Zaloguj);
    }
    pub fn wyloguj(&self) {
        let _ = self.tx.send(Rozkaz::Wyloguj);
    }
    pub fn biblioteka(&self, polecenie: serde_json::Value) {
        let _ = self.tx.send(Rozkaz::Biblioteka(polecenie));
    }
    /// Utwory z bieżącego widoku biblioteki jako kolejka odtwarzacza.
    pub fn kolejka(&self) -> Vec<Pozycja> {
        biblioteka()
            .utwory()
            .into_iter()
            .map(|u| Pozycja { wykonawca: u.wykonawca(), sciezka: u.uri, tytul: u.tytul, album: u.album.nazwa, czas: u.czas })
            .collect()
    }
    pub fn graj(&self, uri: &str, ms: u32) {
        let _ = self.tx.send(Rozkaz::Graj(uri.to_string(), ms));
    }
    pub fn pauza(&self) {
        let _ = self.tx.send(Rozkaz::Pauza);
    }
    pub fn wznow(&self) {
        let _ = self.tx.send(Rozkaz::Wznow);
    }
    pub fn szukaj(&self, ms: u32) {
        let _ = self.tx.send(Rozkaz::Szukaj(ms));
    }
    pub fn stop(&self) {
        let _ = self.tx.send(Rozkaz::Stop);
    }
}

fn katalog() -> PathBuf {
    crate::settings::dir().join("spotify")
}

fn ustaw(stan: &Mutex<SpStan>, zmiana: &AtomicBool, f: impl FnOnce(&mut SpStan)) {
    if let Ok(mut s) = stan.lock() {
        f(&mut s);
    }
    zmiana.store(true, Ordering::Relaxed);
}

struct Polaczenie {
    session: Session,
    player: Arc<Player>,
}

async fn petla(mut rx: tokio::sync::mpsc::UnboundedReceiver<Rozkaz>, stan: Arc<Mutex<SpStan>>, zmiana: Arc<AtomicBool>) {
    let mut pol: Option<Polaczenie> = None;
    while let Some(r) = rx.recv().await {
        match r {
            Rozkaz::Zaloguj => {
                if pol.as_ref().is_some_and(|p| !p.session.is_invalid()) {
                    continue;
                }
                ustaw(&stan, &zmiana, |s| {
                    s.status = "laczenie".into();
                    s.opis = "Łączenie ze Spotify…".into();
                });
                match polacz(&stan, &zmiana).await {
                    Ok(p) => {
                        zalogowano(&p, &stan, &zmiana);
                        pol = Some(p);
                    }
                    Err(e) => {
                        log::error!("Spotify: logowanie: {e}");
                        ustaw(&stan, &zmiana, |s| {
                            s.status = "blad".into();
                            s.opis = e;
                        });
                    }
                }
            }
            Rozkaz::Wyloguj => {
                if let Some(p) = pol.take() {
                    p.player.stop();
                    p.session.shutdown();
                }
                let _ = std::fs::remove_file(katalog().join("credentials.json"));
                biblioteka().wyloguj();
                log::info!("Spotify: wylogowano");
                ustaw(&stan, &zmiana, |s| *s = SpStan { status: "wylogowany".into(), ..Default::default() });
            }
            Rozkaz::Biblioteka(m) => {
                // połączenie bywa zrywane (uśpienie komputera, zmiana sieci) – łączymy ponownie zapisanym logowaniem
                if pol.as_ref().is_some_and(|p| p.session.is_invalid()) && katalog().join("credentials.json").exists() {
                    log::info!("Spotify: połączenie zerwane – łączę ponownie");
                    match polacz(&stan, &zmiana).await {
                        Ok(p) => {
                            if let Some(stare) = pol.take() {
                                stare.player.stop();
                            }
                            zalogowano(&p, &stan, &zmiana);
                            pol = Some(p);
                        }
                        Err(e) => {
                            log::error!("Spotify: ponowne łączenie: {e}");
                            ustaw(&stan, &zmiana, |s| s.opis = format!("Brak połączenia ze Spotify: {e}"));
                        }
                    }
                }
                biblioteka().rozkaz(&m);
            }
            Rozkaz::Graj(uri, ms) => {
                let Some(p) = pol.as_ref() else {
                    strumien().blad.store(true, Ordering::Relaxed);
                    continue;
                };
                match SpotifyUri::from_uri(&uri) {
                    Ok(id) => {
                        ODMOWA.store(false, Ordering::Relaxed);
                        let st = strumien();
                        st.koniec.store(false, Ordering::Relaxed);
                        st.blad.store(false, Ordering::Relaxed);
                        st.czysc.store(true, Ordering::Relaxed);
                        st.pozycja_ms.store(ms, Ordering::Relaxed);
                        p.player.load(id, true, ms);
                    }
                    Err(e) => {
                        log::error!("Spotify: zły identyfikator {uri}: {e}");
                        strumien().blad.store(true, Ordering::Relaxed);
                    }
                }
            }
            Rozkaz::Pauza => {
                if let Some(p) = &pol {
                    p.player.pause();
                }
            }
            Rozkaz::Wznow => {
                if let Some(p) = &pol {
                    p.player.play();
                }
            }
            Rozkaz::Szukaj(ms) => {
                if let Some(p) = &pol {
                    strumien().czysc.store(true, Ordering::Relaxed);
                    strumien().pozycja_ms.store(ms, Ordering::Relaxed);
                    p.player.seek(ms);
                }
            }
            Rozkaz::Stop => {
                if let Some(p) = &pol {
                    p.player.stop();
                }
            }
        }
    }
}

fn zalogowano(p: &Polaczenie, stan: &Mutex<SpStan>, zmiana: &AtomicBool) {
    let konto = p.session.username();
    log::info!("Spotify: zalogowano ({konto})");
    ustaw(stan, zmiana, |s| {
        s.status = "gotowe".into();
        s.opis = String::new();
        s.konto = konto;
    });
    biblioteka().polacz(p.session.clone());
}

async fn polacz(stan: &Arc<Mutex<SpStan>>, zmiana: &Arc<AtomicBool>) -> Result<Polaczenie, String> {
    let k = katalog();
    let cache = Cache::new(Some(k.clone()), Some(k.clone()), None, None).map_err(|e| format!("pamięć podręczna: {e}"))?;
    let credentials = match cache.credentials() {
        Some(c) => c,
        None => {
            let (s2, z2) = (stan.clone(), zmiana.clone());
            let token = tokio::task::spawn_blocking(move || oauth_login(&s2, &z2)).await.map_err(|e| e.to_string())??;
            Credentials::with_access_token(token)
        }
    };
    let session = Session::new(SessionConfig { client_id: CLIENT_ID.to_string(), ..SessionConfig::default() }, Some(cache));
    session.connect(credentials, true).await.map_err(|e| format!("Spotify: {e}"))?;
    let (prod, cons) = HeapRb::<f32>::new(FS as usize * 4).split(); // 2 s stereo
    if let Ok(mut g) = strumien().cons.lock() {
        *g = Some(cons);
    }
    let prod = Arc::new(Mutex::new(Some(prod)));
    let player = Player::new(PlayerConfig::default(), session.clone(), Box::new(NoOpVolume), move || {
        Box::new(SzkloSink { prod: prod.clone() }) as Box<dyn Sink>
    });
    // zdarzenia: pozycja, koniec utworu, niedostępność
    let mut ev = player.get_player_event_channel();
    tokio::spawn(async move {
        while let Some(e) = ev.recv().await {
            let st = strumien();
            match e {
                PlayerEvent::EndOfTrack { .. } => st.koniec.store(true, Ordering::Relaxed),
                PlayerEvent::Unavailable { track_id, .. } => {
                    log::warn!("Spotify: utwór niedostępny: {track_id:?}");
                    st.blad.store(true, Ordering::Relaxed);
                }
                PlayerEvent::Playing { position_ms, .. } | PlayerEvent::Paused { position_ms, .. } | PlayerEvent::Seeked { position_ms, .. } => {
                    st.pozycja_ms.store(position_ms, Ordering::Relaxed)
                }
                _ => {}
            }
        }
    });
    Ok(Polaczenie { session, player })
}

// ───────────────────────────── wyjście librespot → bufor ─────────────────────────────

struct SzkloSink {
    prod: Arc<Mutex<Option<HeapProd<f32>>>>,
}

impl Sink for SzkloSink {
    fn start(&mut self) -> SinkResult<()> {
        Ok(())
    }
    fn stop(&mut self) -> SinkResult<()> {
        Ok(())
    }
    fn write(&mut self, packet: AudioPacket, _c: &mut Converter) -> SinkResult<()> {
        let AudioPacket::Samples(s) = packet else { return Ok(()) };
        let buf: Vec<f32> = s.iter().map(|&v| v as f32).collect();
        let mut off = 0;
        let mut czekano = 0;
        while off < buf.len() {
            let n = self.prod.lock().ok().and_then(|mut g| g.as_mut().map(|p| p.push_slice(&buf[off..]))).unwrap_or(buf.len());
            off += n;
            if off < buf.len() {
                // przeciwciśnienie: odtwarzacz jeszcze nie pobrał (librespot dekoduje szybciej niż czas rzeczywisty)
                std::thread::sleep(Duration::from_millis(5));
                czekano += 5;
                if czekano > 3000 {
                    break;
                }
            }
        }
        Ok(())
    }
}

/// Pobiera do `out` dostępne próbki (stereo). Zwraca liczbę.
pub fn pobierz(out: &mut Vec<f32>, max: usize) -> usize {
    let st = strumien();
    let Ok(mut g) = st.cons.lock() else { return 0 };
    let Some(c) = g.as_mut() else { return 0 };
    if st.czysc.swap(false, Ordering::Relaxed) {
        let n = c.occupied_len();
        c.skip(n);
    }
    let n = c.occupied_len().min(max) & !1;
    let start = out.len();
    out.resize(start + n, 0.0);
    c.pop_slice(&mut out[start..])
}

pub fn bufor_pusty() -> bool {
    strumien().cons.lock().ok().and_then(|g| g.as_ref().map(|c| c.occupied_len() < 2)).unwrap_or(true)
}

// ───────────────────────────── OAuth PKCE ─────────────────────────────

fn b64url(data: &[u8]) -> String {
    use base64::Engine as _;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

fn url_encode(s: &str) -> String {
    let mut o = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => o.push(b as char),
            _ => o.push_str(&format!("%{b:02X}")),
        }
    }
    o
}

fn otworz_przegladarke(url: &str) {
    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
}

/// Logowanie przez przeglądarkę (PKCE): zwraca token dostępu.
fn oauth_login(stan: &Mutex<SpStan>, zmiana: &AtomicBool) -> Result<String, String> {
    use rand::RngCore;
    use sha2::Digest;
    let mut raw = [0u8; 48];
    rand::thread_rng().fill_bytes(&mut raw);
    let verifier = b64url(&raw);
    let challenge = b64url(&sha2::Sha256::digest(verifier.as_bytes()));
    let redirect = format!("http://127.0.0.1:{REDIRECT_PORT}/login");
    let url = format!(
        "https://accounts.spotify.com/authorize?response_type=code&client_id={CLIENT_ID}&redirect_uri={}&scope={}&code_challenge_method=S256&code_challenge={challenge}",
        url_encode(&redirect),
        url_encode(SCOPES)
    );
    let listener = std::net::TcpListener::bind(("127.0.0.1", REDIRECT_PORT)).map_err(|e| format!("port logowania {REDIRECT_PORT}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    ustaw(stan, zmiana, |s| {
        s.status = "login".into();
        s.opis = "Zaloguj się do Spotify w otwartej przeglądarce, potem wróć do Szkła.".into();
    });
    otworz_przegladarke(&url);
    let koniec = std::time::Instant::now() + Duration::from_secs(300);
    let code = loop {
        if std::time::Instant::now() > koniec {
            return Err("logowanie przerwane (minęło 5 minut)".into());
        }
        match listener.accept() {
            Ok((mut sock, _)) => {
                let _ = sock.set_nonblocking(false);
                let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("");
                let code = path.split('?').nth(1).and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("code="))).map(|c| c.to_string());
                let body = if code.is_some() {
                    "<html><meta charset=utf-8><body style='font-family:sans-serif;background:#061214;color:#EEF9FA;text-align:center;padding-top:30vh'><h2>Zalogowano do Spotify</h2><p>Wróć do Szkła.</p></body></html>"
                } else {
                    "<html><meta charset=utf-8><body>Brak kodu logowania – spróbuj ponownie.</body></html>"
                };
                let _ = write!(sock, "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
                if let Some(c) = code {
                    break c;
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(e) => return Err(format!("logowanie: {e}")),
        }
    };
    ustaw(stan, zmiana, |s| {
        s.status = "laczenie".into();
        s.opis = "Łączenie ze Spotify…".into();
    });
    let resp = reqwest::blocking::Client::new()
        .post("https://accounts.spotify.com/api/token")
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", redirect.as_str()),
            ("client_id", CLIENT_ID),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .map_err(|e| format!("wymiana kodu: {e}"))?;
    let v: serde_json::Value = resp.json().map_err(|e| format!("odpowiedź logowania: {e}"))?;
    v.get("access_token").and_then(|t| t.as_str()).map(|s| s.to_string()).ok_or_else(|| format!("brak tokenu: {v}"))
}
