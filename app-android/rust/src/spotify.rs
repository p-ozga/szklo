//! Odbiornik Spotify Connect „Szkło” (Android): Spotify → librespot → pełny silnik Szkła → wyjście audio (AAudio).
//!
//! - Logowanie: OAuth 2.0 z PKCE (przeglądarka telefonu → przekierowanie na http://127.0.0.1:5588/login, które
//!   odbiera mały serwer w tym procesie). Identyfikator klienta i zakresy jak w librespot (tryb „keymaster”).
//!   Po pierwszym logowaniu librespot zapisuje w pamięci podręcznej aplikacji poświadczenia wielokrotnego użytku –
//!   kolejne uruchomienia logują się same.
//! - Dźwięk: własny `Sink` librespot – każdy pakiet PCM (44,1 kHz, stereo, f64) przechodzi przez [`Engine`] ze
//!   WSPÓLNYMI parametrami i miernikami uchwytu JNI (suwaki w aplikacji działają na żywo), potem przez bufor
//!   pierścieniowy do strumienia wyjściowego cpal (AAudio).
//! - Stan dla Kotlina: tekst `idle | connecting | login <url> | ready | playing | error <opis>`.
//!
//! Uwaga prawna: librespot to nieoficjalny klient Spotify (wymaga Premium) – zgoda użytkownika 2026-11-22.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use librespot_connect::{ConnectConfig, Spirc};
use librespot_core::{authentication::Credentials, cache::Cache, config::DeviceType, session::Session, SessionConfig};
use librespot_playback::audio_backend::{Sink, SinkResult};
use librespot_playback::config::PlayerConfig;
use librespot_playback::convert::Converter;
use librespot_playback::decoder::AudioPacket;
use librespot_playback::mixer::softmixer::SoftMixer;
use librespot_playback::mixer::{Mixer, MixerConfig};
use librespot_playback::player::Player;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;
use szklo_core::{Engine, SharedMeters, SharedParams};

const CLIENT_ID: &str = "65b708073fc0480ea92a077233ca87bd";
const REDIRECT_PORT: u16 = 5588;
const SCOPES: &[&str] = &[
    "app-remote-control",
    "playlist-modify",
    "playlist-modify-private",
    "playlist-modify-public",
    "playlist-read",
    "playlist-read-collaborative",
    "playlist-read-private",
    "streaming",
    "ugc-image-upload",
    "user-follow-modify",
    "user-follow-read",
    "user-library-modify",
    "user-library-read",
    "user-modify",
    "user-modify-playback-state",
    "user-modify-private",
    "user-personalized",
    "user-read-birthdate",
    "user-read-currently-playing",
    "user-read-email",
    "user-read-play-history",
    "user-read-playback-position",
    "user-read-playback-state",
    "user-read-private",
    "user-read-recently-played",
    "user-top-read",
];
const FS: u32 = 44_100;

/// Liczniki próbek (diagnostyka): ile przyszło od librespot i ile oddał callback wyjścia.
static PROBKI_WE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static PROBKI_WY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

struct Runner {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn status_cell() -> &'static Mutex<String> {
    static S: OnceLock<Mutex<String>> = OnceLock::new();
    S.get_or_init(|| Mutex::new("idle".into()))
}
fn runner_cell() -> &'static Mutex<Option<Runner>> {
    static R: OnceLock<Mutex<Option<Runner>>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(None))
}
/// „playing”, chyba że Spotify odmówił klucza do bieżącego utworu (wtedy zostaje komunikat błędu).
fn status_gra() {
    if !crate::diag::KLUCZ_ODRZUCONY.load(Ordering::Relaxed) {
        set_status("playing");
    }
}

pub(crate) fn set_status(s: impl Into<String>) {
    let s = s.into();
    log::info!("Spotify: {s}");
    if let Ok(mut g) = status_cell().lock() {
        *g = s;
    }
}

pub fn status() -> String {
    status_cell().lock().map(|g| g.clone()).unwrap_or_else(|_| "idle".into())
}

/// Start odbiornika (idempotentny). `cache_dir` – katalog prywatny aplikacji.
pub fn start(params: Arc<SharedParams>, meters: Arc<SharedMeters>, cache_dir: String, name: String) {
    let mut r = match runner_cell().lock() {
        Ok(r) => r,
        Err(e) => e.into_inner(),
    };
    if let Some(run) = r.as_ref() {
        if run.thread.as_ref().map(|t| !t.is_finished()).unwrap_or(false) {
            return;
        }
    }
    let stop = Arc::new(AtomicBool::new(false));
    let s2 = stop.clone();
    set_status("connecting");
    let thread = std::thread::Builder::new()
        .name("szklo-spotify".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() {
                Ok(rt) => rt,
                Err(e) => {
                    set_status(format!("error tokio: {e}"));
                    return;
                }
            };
            let res = rt.block_on(run(params, meters, PathBuf::from(cache_dir), name, s2.clone()));
            match res {
                Ok(()) => set_status("idle"),
                Err(e) => set_status(format!("error {e}")),
            }
        })
        .ok();
    *r = Some(Runner { stop, thread });
}

pub fn stop() {
    let run = runner_cell().lock().ok().and_then(|mut r| r.take());
    if let Some(mut run) = run {
        run.stop.store(true, Ordering::Relaxed);
        if let Some(t) = run.thread.take() {
            let _ = t.join();
        }
    }
    set_status("idle");
}

/// Usuwa zapisane poświadczenia (wylogowanie).
pub fn logout(cache_dir: String) {
    stop();
    let _ = std::fs::remove_file(PathBuf::from(cache_dir).join("spotify").join("credentials.json"));
}

async fn run(params: Arc<SharedParams>, meters: Arc<SharedMeters>, cache_dir: PathBuf, name: String, stop: Arc<AtomicBool>) -> Result<(), String> {
    let base = cache_dir.join("spotify");
    let cache = Cache::new(Some(base.clone()), Some(base.clone()), None, None).map_err(|e| format!("pamięć podręczna: {e}"))?;
    let credentials = match cache.credentials() {
        Some(c) => c,
        None => {
            let token = tokio::task::spawn_blocking({
                let stop = stop.clone();
                move || oauth_login(&stop)
            })
            .await
            .map_err(|e| e.to_string())??;
            Credentials::with_access_token(token)
        }
    };
    if stop.load(Ordering::Relaxed) {
        return Ok(());
    }
    set_status("connecting");
    let session_config = SessionConfig { client_id: CLIENT_ID.to_string(), ..SessionConfig::default() };
    let session = Session::new(session_config, Some(cache));
    let mixer: Arc<dyn Mixer> = Arc::new(SoftMixer::open(MixerConfig::default()).map_err(|e| format!("mikser: {e}"))?);
    let player_config = PlayerConfig::default();
    let (p2, m2) = (params.clone(), meters.clone());
    let player = Player::new(player_config, session.clone(), mixer.get_soft_volume(), move || {
        Box::new(SzkloSink::new(p2, m2)) as Box<dyn Sink>
    });
    // zdarzenia odtwarzacza (ładowanie, odtwarzanie, niedostępny utwór…) → dziennik na karcie
    let mut zdarzenia = player.get_player_event_channel();
    tokio::spawn(async move {
        while let Some(ev) = zdarzenia.recv().await {
            let t = format!("{ev:?}");
            // pozycja co sekundę zaśmieca dziennik – pomijamy
            if !t.starts_with("PositionCorrection") && !t.starts_with("PositionChanged") {
                log::info!("zdarzenie: {}", t.chars().take(220).collect::<String>());
            }
        }
    });
    tokio::spawn({
        let stop = stop.clone();
        async move {
            let (mut we0, mut wy0) = (0u64, 0u64);
            while !stop.load(Ordering::Relaxed) {
                tokio::time::sleep(Duration::from_secs(5)).await;
                let (we, wy) = (PROBKI_WE.load(Ordering::Relaxed), PROBKI_WY.load(Ordering::Relaxed));
                if we != we0 || wy != wy0 {
                    log::info!("audio: od Spotify {} s, do głośnika {} s (ostatnie 5 s)", (we - we0) / 2 / FS as u64, (wy - wy0) / 2 / FS as u64);
                }
                (we0, wy0) = (we, wy);
            }
        }
    });
    let connect_config = ConnectConfig {
        name,
        device_type: DeviceType::Smartphone,
        initial_volume: u16::MAX,
        ..ConnectConfig::default()
    };
    let (spirc, task) = Spirc::new(connect_config, session.clone(), credentials, player, mixer)
        .await
        .map_err(|e| format!("Spotify Connect: {e}"))?;
    set_status("ready");
    log::info!("konto: typ {}", session.get_user_attribute("type").unwrap_or_else(|| "?".into()));
    let mut task = Box::pin(task);
    loop {
        tokio::select! {
            _ = &mut task => {
                // sesja zerwana (sieć) – kończymy; Kotlin uruchomi ponownie
                return Err("połączenie ze Spotify zerwane".into());
            }
            _ = tokio::time::sleep(Duration::from_millis(250)) => {
                if stop.load(Ordering::Relaxed) {
                    let _ = spirc.shutdown();
                    let _ = tokio::time::timeout(Duration::from_secs(3), &mut task).await;
                    session.shutdown();
                    return Ok(());
                }
            }
        }
    }
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

/// Logowanie przez przeglądarkę: zwraca token dostępu.
fn oauth_login(stop: &AtomicBool) -> Result<String, String> {
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
        url_encode(&SCOPES.join(" "))
    );
    let listener = std::net::TcpListener::bind(("127.0.0.1", REDIRECT_PORT)).map_err(|e| format!("port logowania {REDIRECT_PORT}: {e}"))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    set_status(format!("login {url}"));
    let code = loop {
        if stop.load(Ordering::Relaxed) {
            return Err("logowanie przerwane".into());
        }
        match listener.accept() {
            Ok((mut sock, _)) => {
                let _ = sock.set_nonblocking(false);
                let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
                let mut buf = [0u8; 4096];
                let n = sock.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let path = req.split_whitespace().nth(1).unwrap_or("");
                let code = path
                    .split('?')
                    .nth(1)
                    .and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("code=")))
                    .map(|c| c.to_string());
                let body = if code.is_some() {
                    "<html><meta charset=utf-8><body style='font-family:sans-serif;background:#081113;color:#F1F7F5;text-align:center;padding-top:30vh'><h2>Zalogowano do Spotify</h2><p>Wróć do aplikacji Szkło.</p></body></html>"
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
    set_status("connecting");
    let client = reqwest::blocking::Client::new();
    let resp = client
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
    v.get("access_token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| format!("brak tokenu: {v}"))
}

// ───────────────────────────── Wyjście: silnik Szkła → AAudio ─────────────────────────────

struct SzkloSink {
    engine: Engine,
    prod: HeapProd<f32>,
    cons: Option<HeapCons<f32>>,
    stream: Option<cpal::Stream>,
    buf: Vec<f32>,
}

impl SzkloSink {
    fn new(params: Arc<SharedParams>, meters: Arc<SharedMeters>) -> Self {
        let (prod, cons) = HeapRb::<f32>::new(FS as usize).split(); // 0,5 s stereo
        Self { engine: Engine::new(FS as f64, params, meters), prod, cons: Some(cons), stream: None, buf: Vec::with_capacity(8192) }
    }
}

fn open_output(mut cons: HeapCons<f32>) -> Result<cpal::Stream, String> {
    let host = cpal::default_host();
    let dev = host.default_output_device().ok_or("brak urządzenia wyjściowego")?;
    log::info!("wyjście audio: {}", dev.name().unwrap_or_else(|_| "?".into()));
    let config = cpal::StreamConfig { channels: 2, sample_rate: cpal::SampleRate(FS), buffer_size: cpal::BufferSize::Default };
    dev.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            let got = cons.pop_slice(data);
            data[got..].fill(0.0);
            PROBKI_WY.fetch_add(got as u64, Ordering::Relaxed);
        },
        |e| log::warn!("strumień audio: {e}"),
        None,
    )
    .map_err(|e| e.to_string())
}

impl Sink for SzkloSink {
    fn start(&mut self) -> SinkResult<()> {
        if self.stream.is_some() {
            if let Some(s) = &self.stream {
                let _ = s.play();
            }
            status_gra();
            return Ok(());
        }
        let Some(cons) = self.cons.take() else { return Ok(()) };
        // panika w cpal (np. brak kontekstu Androida) nie może po cichu zabić wątku odtwarzacza
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| open_output(cons))) {
            Ok(Ok(s)) => {
                if let Err(e) = s.play() {
                    log::warn!("start strumienia: {e}");
                }
                self.stream = Some(s);
                status_gra();
            }
            Ok(Err(e)) => set_status(format!("error wyjście audio: {e}")),
            Err(_) => set_status("error wyjście audio: awaria sterownika"),
        }
        Ok(())
    }

    fn stop(&mut self) -> SinkResult<()> {
        if let Some(s) = &self.stream {
            let _ = s.pause();
        }
        set_status("ready");
        Ok(())
    }

    fn write(&mut self, packet: AudioPacket, _converter: &mut Converter) -> SinkResult<()> {
        let AudioPacket::Samples(samples) = packet else { return Ok(()) };
        self.buf.clear();
        self.buf.extend(samples.iter().map(|&v| v as f32));
        PROBKI_WE.fetch_add(self.buf.len() as u64, Ordering::Relaxed);
        self.engine.process_interleaved(&mut self.buf, 2);
        // przeciwciśnienie: czekamy, aż wyjście zrobi miejsce (librespot dekoduje szybciej niż czas rzeczywisty)
        let mut off = 0;
        let mut waited = 0;
        while off < self.buf.len() {
            let n = self.prod.push_slice(&self.buf[off..]);
            off += n;
            if off < self.buf.len() {
                std::thread::sleep(Duration::from_millis(5));
                waited += 5;
                if waited > 2000 {
                    break; // wyjście stoi (np. zatrzymane) – nie blokujemy odtwarzacza w nieskończoność
                }
            }
        }
        if self.prod.vacant_len() == 0 {
            log::debug!("bufor wyjścia pełny");
        }
        Ok(())
    }
}
