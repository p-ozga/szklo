//! Tryb „silnik w aplikacji”: wejście (urządzenie nagrywające albo przechwyt wyjścia WASAPI loopback,
//! np. wirtualny kabel ustawiony jako domyślne wyjście systemu) → DSP → wybrane wyjście (słuchawki).
//!
//! Wątek wejścia wrzuca próbki do bufora SPSC, wywołanie zwrotne wyjścia je pobiera, przetwarza
//! silnikiem i oddaje. Dryf zegarów: bufor celuje w ~20 ms; nadmiar jest odrzucany, niedobór = cisza.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapProd, HeapRb};
use std::sync::Arc;
use szklo_core::{Engine, SharedMeters, SharedParams};

pub const LOOPBACK_PREFIX: &str = "Przechwyć: ";

pub struct DeviceLists {
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub default_output: String,
}

pub fn list_devices() -> DeviceLists {
    let host = cpal::default_host();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    if let Ok(devs) = host.input_devices() {
        for d in devs {
            if let Ok(n) = d.name() {
                inputs.push(n);
            }
        }
    }
    if let Ok(devs) = host.output_devices() {
        for d in devs {
            if let Ok(n) = d.name() {
                inputs.push(format!("{LOOPBACK_PREFIX}{n}"));
                outputs.push(n);
            }
        }
    }
    let default_output = host.default_output_device().and_then(|d| d.name().ok()).unwrap_or_default();
    DeviceLists { inputs, outputs, default_output }
}

fn find_output(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    if name.is_empty() {
        return host.default_output_device();
    }
    host.output_devices().ok()?.find(|d| d.name().map(|n| n == name).unwrap_or(false))
}

fn find_input(host: &cpal::Host, name: &str) -> Option<cpal::Device> {
    if let Some(out) = name.strip_prefix(LOOPBACK_PREFIX) {
        // WASAPI: strumień wejściowy na urządzeniu wyjściowym = przechwyt (loopback)
        return host.output_devices().ok()?.find(|d| d.name().map(|n| n == out).unwrap_or(false));
    }
    if name.is_empty() {
        return host.default_input_device();
    }
    host.input_devices().ok()?.find(|d| d.name().map(|n| n == name).unwrap_or(false))
}

pub struct Running {
    _input: cpal::Stream,
    _output: cpal::Stream,
    pub description: String,
}

pub fn start(input: &str, output: &str, params: Arc<SharedParams>, meters: Arc<SharedMeters>) -> Result<Running, String> {
    let host = cpal::default_host();
    let out_dev = find_output(&host, output).ok_or_else(|| format!("nie znaleziono wyjścia „{output}”"))?;
    let in_dev = find_input(&host, input).ok_or_else(|| format!("nie znaleziono wejścia „{input}”"))?;
    let out_name = out_dev.name().unwrap_or_default();
    if input.strip_prefix(LOOPBACK_PREFIX) == Some(out_name.as_str()) {
        return Err("wejście i wyjście to to samo urządzenie – powstałoby sprzężenie zwrotne".into());
    }

    let out_cfg = out_dev.default_output_config().map_err(|e| format!("wyjście: {e}"))?;
    let in_cfg = if input.starts_with(LOOPBACK_PREFIX) {
        in_dev.default_output_config()
    } else {
        in_dev.default_input_config()
    }
    .map_err(|e| format!("wejście: {e}"))?;
    let fs = out_cfg.sample_rate().0;
    if in_cfg.sample_rate().0 != fs {
        return Err(format!(
            "różne częstotliwości: wejście {} Hz, wyjście {} Hz – ustaw oba urządzenia na tę samą w Panelu dźwięku Windows",
            in_cfg.sample_rate().0,
            fs
        ));
    }
    if in_cfg.sample_format() != cpal::SampleFormat::F32 || out_cfg.sample_format() != cpal::SampleFormat::F32 {
        return Err("obsługiwany jest tylko format f32 (tryb współdzielony WASAPI)".into());
    }
    let in_ch = in_cfg.channels() as usize;
    let out_ch = out_cfg.channels() as usize;

    // ~200 ms pojemności, cel napełnienia ~20 ms.
    let cap = (fs as usize / 5) * 2;
    let target = (fs as usize / 50) * 2;
    let (mut prod, mut cons) = HeapRb::<f32>::new(cap).split();

    // cpal (WASAPI) po błędzie kończy wątek strumienia na dobre – flaga każe pętli głównej postawić silnik od nowa
    BLAD_STRUMIENIA.store(false, std::sync::atomic::Ordering::Relaxed);
    let err_fn = |e| {
        log::error!("błąd strumienia audio: {e}");
        BLAD_STRUMIENIA.store(true, std::sync::atomic::Ordering::Relaxed);
    };
    let in_stream = in_dev
        .build_input_stream(
            &in_cfg.config(),
            move |data: &[f32], _| {
                for fr in data.chunks(in_ch) {
                    let l = fr[0];
                    let r = if in_ch > 1 { fr[1] } else { fr[0] };
                    if prod.vacant_len() >= 2 {
                        let _ = prod.try_push(l);
                        let _ = prod.try_push(r);
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| format!("wejście: {e}"))?;

    let mut engine = Engine::new(fs as f64, params, meters);
    let mut scratch: Vec<f32> = Vec::with_capacity(8192);
    let mut wstrz_tmp: Vec<f32> = Vec::with_capacity(8192);
    // ~0,5 s miejsca na próbki wbudowanego odtwarzacza (dosumowywane do wejścia przed silnikiem)
    let (w_prod, mut w_cons) = HeapRb::<f32>::new(fs as usize).split();
    let mut primed = false;
    let out_stream = out_dev
        .build_output_stream(
            &out_cfg.config(),
            move |data: &mut [f32], _| {
                let frames = data.len() / out_ch;
                // dryf: za dużo w buforze → odrzuć nadmiar (zachowaj cel)
                let avail = cons.occupied_len();
                if avail > target * 4 {
                    let drop = (avail - target) & !1;
                    cons.skip(drop);
                }
                scratch.clear();
                scratch.resize(frames * 2, 0.0);
                if !primed && cons.occupied_len() >= target {
                    primed = true;
                }
                if primed {
                    // zawsze parzysta liczba próbek: nieparzysta zamieniała kanały na stałe
                    let ile = cons.occupied_len().min(scratch.len()) & !1;
                    let got = cons.pop_slice(&mut scratch[..ile]);
                    if got < frames * 2 {
                        primed = false; // niedobór: cisza do ponownego napełnienia
                    }
                }
                // wbudowany odtwarzacz: dosumuj (gra także wtedy, gdy wejście milczy)
                if WSTRZYK_CZYSC.swap(false, std::sync::atomic::Ordering::Relaxed) {
                    let n = w_cons.occupied_len();
                    w_cons.skip(n);
                }
                wstrz_tmp.clear();
                wstrz_tmp.resize(frames * 2, 0.0);
                // pauza odtwarzacza: nie pobieraj (bufor czeka), zamiast grać jeszcze pół sekundy
                let wn = if WSTRZYK_PAUZA.load(std::sync::atomic::Ordering::Relaxed) { 0 } else { w_cons.pop_slice(&mut wstrz_tmp) };
                for (a, b) in scratch.iter_mut().zip(&wstrz_tmp[..wn]) {
                    *a += *b;
                }
                engine.process_interleaved(&mut scratch, 2);
                for (f, fr) in data.chunks_mut(out_ch).enumerate() {
                    let (l, r) = (scratch[2 * f], scratch[2 * f + 1]);
                    match out_ch {
                        1 => fr[0] = 0.5 * (l + r),
                        _ => {
                            fr[0] = l;
                            fr[1] = r;
                            for x in fr.iter_mut().skip(2) {
                                *x = 0.0;
                            }
                        }
                    }
                }
            },
            err_fn,
            None,
        )
        .map_err(|e| format!("wyjście: {e}"))?;

    in_stream.play().map_err(|e| format!("wejście: {e}"))?;
    out_stream.play().map_err(|e| format!("wyjście: {e}"))?;
    if let Ok(mut g) = WSTRZYK.lock() {
        *g = Some((w_prod, fs));
    }
    Ok(Running {
        _input: in_stream,
        _output: out_stream,
        description: format!("{out_name} · {}", fmt_khz(fs)),
    })
}

impl Drop for Running {
    fn drop(&mut self) {
        // silnik stoi → odtwarzacz przełączy się na własne wyjście
        if let Ok(mut g) = WSTRZYK.lock() {
            *g = None;
        }
        WSTRZYK_PAUZA.store(false, std::sync::atomic::Ordering::Relaxed);
        WSTRZYK_CZYSC.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

// ───────────── wstrzyknięcie: wbudowany odtwarzacz → wprost do silnika (tryb „Silnik w aplikacji”) ─────────────

static WSTRZYK: std::sync::Mutex<Option<(HeapProd<f32>, u32)>> = std::sync::Mutex::new(None);
static WSTRZYK_CZYSC: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WSTRZYK_PAUZA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static BLAD_STRUMIENIA: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Częstotliwość silnika, gdy tryb aplikacji działa (odtwarzacz wtedy podaje próbki tutaj).
pub fn wstrzykniecie_fs() -> Option<u32> {
    WSTRZYK.lock().ok().and_then(|g| g.as_ref().map(|(_, fs)| *fs))
}
pub fn wstrzyknij_wolne() -> Option<usize> {
    WSTRZYK.lock().ok().and_then(|g| g.as_ref().map(|(p, _)| p.vacant_len()))
}
pub fn wstrzyknij(s: &[f32]) -> usize {
    WSTRZYK.lock().ok().and_then(|mut g| g.as_mut().map(|(p, _)| p.push_slice(s))).unwrap_or(0)
}
pub fn wstrzyknij_wyczysc() {
    use std::sync::atomic::Ordering::Relaxed;
    WSTRZYK_CZYSC.store(true, Relaxed);
    // poczekaj (krótko), aż wywołanie zwrotne opróżni bufor – inaczej wyrzuciłoby początek NOWEGO utworu
    for _ in 0..25 {
        if !WSTRZYK_CZYSC.load(Relaxed) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
}
/// Pauza wbudowanego odtwarzacza w trybie silnika (natychmiastowa cisza, bufor zostaje).
pub fn wstrzyknij_pauza(b: bool) {
    WSTRZYK_PAUZA.store(b, std::sync::atomic::Ordering::Relaxed);
}
/// Czy strumień silnika zgłosił błąd od ostatniego sprawdzenia (urządzenie odłączone, zmiana formatu…).
pub fn blad_strumienia() -> bool {
    BLAD_STRUMIENIA.swap(false, std::sync::atomic::Ordering::Relaxed)
}

pub fn fmt_khz(fs: u32) -> String {
    let k = fs as f64 / 1000.0;
    if (k - k.round()).abs() < 1e-9 {
        format!("{} kHz", k as u32)
    } else {
        format!("{:.1} kHz", k).replace('.', ",")
    }
}
