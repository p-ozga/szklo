//! Tryb „wtyczka w Equalizer APO”: klient kanałów sterowania `\\.\pipe\szklo-ctl*` (serwery = wtyczka w audiodg.exe).
//!
//! Equalizer APO może mieć w audiodg.exe kilka instancji wtyczki naraz (kilka urządzeń, a po podmianie DLL także
//! stare wersje, które żyją dalej na nieużywanym urządzeniu). Każdy serwer ma własny potok o unikalnej nazwie
//! (`szklo-ctl-<pid>-…`, starsze wersje: `szklo-ctl`). Wątek zarządcy co 0,5 s przegląda `\\.\pipe\` i dla każdego
//! nowego potoku uruchamia wątek-klienta. Klient wysyła pełny stan po każdej zmianie i co 1 s, a odbiera mierniki.
//! Mierniki pokazywane w UI pochodzą od serwera, którego licznik `frames` ostatnio rósł (= faktycznie przetwarza dźwięk).
//!
//! Synchroniczne uchwyty Windows serializują I/O na jednym obiekcie pliku, więc klient działa w jednym wątku:
//! czyta linię mierników (serwer wysyła je co ~33 ms), potem ewentualnie wysyła stan. Bez `unsafe`.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use szklo_core::ipc::{decode_meters, encode_params, RemoteMeters, PIPE_PREFIX};
use szklo_core::SharedParams;

#[derive(Default)]
pub struct ApoLink {
    /// Czy jest choć jedno połączenie z wtyczką, która w ciągu ostatnich 2 s przetwarzała dźwięk.
    pub connected: AtomicBool,
    pub meters: Mutex<Option<RemoteMeters>>,
    pub active: AtomicBool,
}

struct Peer {
    meters: Option<RemoteMeters>,
    last_frames: u64,
    last_audio: Option<Instant>,
}

type Peers = Arc<Mutex<HashMap<String, Peer>>>;

pub fn spawn(params: Arc<SharedParams>) -> Arc<ApoLink> {
    let link = Arc::new(ApoLink::default());
    let l2 = link.clone();
    std::thread::Builder::new()
        .name("szklo-apo".into())
        .spawn(move || manager(l2, params))
        .expect("wątek APO");
    link
}

fn list_pipes() -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(r"\\.\pipe\") else { return Vec::new() };
    let z_kreska = format!("{PIPE_PREFIX}-");
    rd.filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| n == PIPE_PREFIX || n.starts_with(&z_kreska))
        .collect()
}

fn manager(link: Arc<ApoLink>, params: Arc<SharedParams>) {
    let peers: Peers = Arc::new(Mutex::new(HashMap::new()));
    let mut obieg = 0u32;
    loop {
        // lista WSZYSTKICH potoków systemu – raz na sekundę (mierniki niżej dalej co 200 ms)
        obieg = obieg.wrapping_add(1);
        if link.active.load(Ordering::Relaxed) && obieg % 5 == 1 {
            for name in list_pipes() {
                let new = peers.lock().map(|mut p| {
                    if p.contains_key(&name) {
                        false
                    } else {
                        p.insert(name.clone(), Peer { meters: None, last_frames: u64::MAX, last_audio: None });
                        true
                    }
                });
                if new.unwrap_or(false) {
                    let (pe, pa, li, nm) = (peers.clone(), params.clone(), link.clone(), name.clone());
                    let _ = std::thread::Builder::new().name(format!("szklo-apo-{name}")).spawn(move || client(nm, pe, pa, li));
                }
            }
        }
        // wybór aktywnego serwera: ostatnio rosnące `frames`, nie starsze niż 2 s
        let best = peers.lock().ok().and_then(|p| {
            p.values()
                .filter_map(|x| x.last_audio.filter(|t| t.elapsed() < Duration::from_secs(2)).map(|t| (t, x.meters)))
                .max_by_key(|(t, _)| *t)
                .and_then(|(_, m)| m)
        });
        link.connected.store(best.is_some(), Ordering::Relaxed);
        if let Ok(mut g) = link.meters.lock() {
            *g = best;
        }
        std::thread::sleep(Duration::from_millis(if link.active.load(Ordering::Relaxed) { 200 } else { 500 }));
    }
}

fn client(name: String, peers: Peers, params: Arc<SharedParams>, link: Arc<ApoLink>) {
    let path = format!(r"\\.\pipe\{name}");
    // Potok jednoinstancyjny może być zajęty (np. inna kopia aplikacji) – kilka prób, potem rezygnacja
    // (zarządca spróbuje ponownie, gdy usuniemy wpis).
    let mut pipe = None;
    for _ in 0..10 {
        if let Ok(f) = std::fs::OpenOptions::new().read(true).write(true).open(&path) {
            pipe = Some(f);
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    if let Some(f) = pipe {
        let mut br = BufReader::new(f);
        let mut sent = 0u64;
        let mut sent_at = Instant::now();
        let mut line = String::new();
        while link.active.load(Ordering::Relaxed) {
            // stan po każdej zmianie i co 1 s (nowa instancja wtyczki wczytuje stan z config.txt Equalizer APO)
            let v = params.version();
            if v != sent || sent_at.elapsed() >= Duration::from_secs(1) {
                if br.get_mut().write_all(encode_params(&params).as_bytes()).and_then(|_| br.get_mut().flush()).is_err() {
                    break;
                }
                sent = v;
                sent_at = Instant::now();
            }
            line.clear();
            match br.read_line(&mut line) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    if let Some(m) = decode_meters(&line) {
                        if let Ok(mut p) = peers.lock() {
                            if let Some(x) = p.get_mut(&name) {
                                // starsze wtyczki nie wysyłają `frames` (0) – wtedy aktywność = zmieniające się mierniki
                                let changed = match (m.snap.frames, x.meters) {
                                    (0, Some(o)) => o.snap.in_peak_db != m.snap.in_peak_db || o.snap.lufs_short != m.snap.lufs_short,
                                    (0, None) => false,
                                    (f, _) => f != x.last_frames,
                                };
                                if changed {
                                    x.last_frames = m.snap.frames;
                                    x.last_audio = Some(Instant::now());
                                }
                                x.meters = Some(m);
                            }
                        }
                    }
                }
            }
        }
    }
    // koniec: zwolnij wpis (po 1 s, żeby zarządca nie łączył się w kółko z martwą nazwą)
    std::thread::sleep(Duration::from_millis(1000));
    if let Ok(mut p) = peers.lock() {
        p.remove(&name);
    }
}
