//! Wbudowany odtwarzacz plików (karta odtwarzacza na odwrocie okna).
//!
//! Dekodowanie: Symphonia (MP3, FLAC, OGG/Vorbis, WAV, AAC/M4A, ALAC). Przeliczanie częstotliwości: rubato.
//! Wyjście – tak, żeby dźwięk ZAWSZE przechodził przez Szkło dokładnie raz:
//!  * tryb „Silnik w aplikacji” działa → próbki trafiają wprost do silnika (wstrzyknięcie, [`audio::wstrzyknij`]),
//!  * tryb Equalizer APO → własny strumień na domyślne wyjście Windows (przetwarza go wtyczka w APO).
//! Widmo (24 pasma) i pozycja są liczone tutaj i odczytywane przez UI.

use crate::audio;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::{FormatOptions, SeekMode, SeekTo};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTagKey};
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

pub const PASMA: usize = 24;
const ODMOWA: &str = "spotify: odmowa klucza";
pub const ROZSZERZENIA: &[&str] = &["mp3", "flac", "ogg", "oga", "wav", "m4a", "aac", "mp4", "alac", "aiff", "aif"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Pozycja {
    pub sciezka: String,
    pub tytul: String,
    pub wykonawca: String,
    pub album: String,
    /// sekundy (0 = nieznane)
    pub czas: f64,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Powtarzanie {
    #[default]
    Wyl,
    Wszystko,
    Jeden,
}

#[derive(Clone, Debug, Serialize, Default)]
pub struct Stan {
    pub lista: Vec<Pozycja>,
    pub biezacy: Option<usize>,
    pub gra: bool,
    pub pozycja: f64,
    pub czas: f64,
    pub losowo: bool,
    pub powtarzanie: Powtarzanie,
    pub blad: Option<String>,
    /// rośnie przy każdej zmianie LISTY – UI przebudowuje listę tylko wtedy (nie przy pauzie/przewijaniu)
    pub wersja: u64,
}

#[derive(Serialize, Deserialize, Default)]
struct Zapis {
    lista: Vec<Pozycja>,
    biezacy: Option<usize>,
    losowo: bool,
    powtarzanie: Powtarzanie,
}

pub enum Rozkaz {
    /// gotowe pozycje (pliki opisuje wątek okna wyboru – nie wątek odtwarzania, żeby dźwięk nie stawał)
    Dodaj(Vec<Pozycja>),
    Graj(Option<usize>),
    Pauza,
    PlayPauza,
    Nastepny,
    Poprzedni,
    Szukaj(f64),
    Losowo(bool),
    Powtarzanie(Powtarzanie),
    Usun(usize),
    Wyczysc,
    /// zastąp kolejkę (np. utworami playlisty Spotify) i graj od pozycji
    Zastap(Vec<Pozycja>, usize),
}

/// Uchwyt dla reszty programu.
pub struct Odtwarzacz {
    tx: Sender<Rozkaz>,
    pub stan: Arc<Mutex<Stan>>,
    pub widmo: Arc<Mutex<[f32; PASMA]>>,
    /// ustawiane przy każdej zmianie stanu (UI wysyła wtedy pełny stan)
    pub zmiana: Arc<AtomicBool>,
}

impl Odtwarzacz {
    pub fn rozkaz(&self, r: Rozkaz) {
        let _ = self.tx.send(r);
    }
}

fn plik_zapisu() -> PathBuf {
    crate::settings::dir().join("playlista.json")
}

pub fn uruchom() -> Odtwarzacz {
    let (tx, rx) = std::sync::mpsc::channel();
    let zapis: Zapis = std::fs::read_to_string(plik_zapisu()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    let stan = Arc::new(Mutex::new(Stan {
        lista: zapis.lista,
        biezacy: zapis.biezacy,
        losowo: zapis.losowo,
        powtarzanie: zapis.powtarzanie,
        ..Default::default()
    }));
    let widmo = Arc::new(Mutex::new([0.0f32; PASMA]));
    let zmiana = Arc::new(AtomicBool::new(true));
    let (s2, w2, z2) = (stan.clone(), widmo.clone(), zmiana.clone());
    std::thread::Builder::new()
        .name("szklo-odtwarzacz".into())
        .spawn(move || petla(rx, s2, w2, z2))
        .expect("wątek odtwarzacza");
    Odtwarzacz { tx, stan, widmo, zmiana }
}

// ───────────────────────────── metadane ─────────────────────────────

fn otworz(sciezka: &Path) -> Result<symphonia::core::probe::ProbeResult, String> {
    let f = std::fs::File::open(sciezka).map_err(|e| e.to_string())?;
    let mss = MediaSourceStream::new(Box::new(f), Default::default());
    let mut hint = Hint::new();
    if let Some(e) = sciezka.extension().and_then(|e| e.to_str()) {
        hint.with_extension(e);
    }
    symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions { enable_gapless: true, ..Default::default() }, &MetadataOptions::default())
        .map_err(|e| format!("nieobsługiwany plik: {e}"))
}

pub fn opisz(sciezka: &Path) -> Pozycja {
    let nazwa = sciezka.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let mut p = Pozycja { sciezka: sciezka.to_string_lossy().to_string(), tytul: nazwa, wykonawca: String::new(), album: String::new(), czas: 0.0 };
    if let Ok(mut probe) = otworz(sciezka) {
        let mut tagi = Vec::new();
        if let Some(m) = probe.metadata.get() {
            if let Some(r) = m.current() {
                tagi.extend(r.tags().to_vec());
            }
        }
        if let Some(r) = probe.format.metadata().current() {
            tagi.extend(r.tags().to_vec());
        }
        for t in tagi {
            match t.std_key {
                Some(StandardTagKey::TrackTitle) => p.tytul = t.value.to_string(),
                Some(StandardTagKey::Artist) => p.wykonawca = t.value.to_string(),
                Some(StandardTagKey::Album) => p.album = t.value.to_string(),
                _ => {}
            }
        }
        if let Some(tr) = probe.format.default_track() {
            if let (Some(n), Some(tb)) = (tr.codec_params.n_frames, tr.codec_params.time_base) {
                let t = tb.calc_time(n);
                p.czas = t.seconds as f64 + t.frac;
            }
        }
    }
    p
}

/// Rozwija foldery (rekurencyjnie) do listy plików audio, posortowanej.
pub fn rozwin(sciezki: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for s in sciezki {
        if s.is_dir() {
            let mut v: Vec<PathBuf> = walk(&s);
            v.sort();
            out.extend(v);
        } else if jest_audio(&s) {
            out.push(s);
        }
    }
    out
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Ok(rd) = std::fs::read_dir(d) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                v.extend(walk(&p));
            } else if jest_audio(&p) {
                v.push(p);
            }
        }
    }
    v
}

fn jest_audio(p: &Path) -> bool {
    p.extension().and_then(|e| e.to_str()).map(|e| ROZSZERZENIA.contains(&e.to_lowercase().as_str())).unwrap_or(false)
}

// ───────────────────────────── wyjście ─────────────────────────────

/// Własny strumień na domyślne wyjście (tryb APO).
struct WlasneWyjscie {
    _stream: cpal::Stream,
    prod: HeapProd<f32>,
    fs: u32,
    /// generacja domyślnego wyjścia Windows w chwili otwarcia – inna = urządzenie się zmieniło
    gen: u32,
    /// strumień zgłosił błąd (cpal kończy wtedy jego wątek na dobre) → trzeba otworzyć nowy
    martwy: Arc<AtomicBool>,
    /// pauza: wywołanie zwrotne oddaje ciszę i nie pobiera z bufora (natychmiastowa, bez „ogona” 0,5 s)
    pauza: Arc<AtomicBool>,
}

fn otworz_wyjscie() -> Result<WlasneWyjscie, String> {
    // generacja PRZED wyborem urządzenia: zmiana wyjścia w trakcie otwierania da różnicę i ponowne otwarcie
    let gen = crate::apo_check::generacja();
    let martwy = Arc::new(AtomicBool::new(false));
    let pauza = Arc::new(AtomicBool::new(false));
    let (martwy2, pauza2) = (martwy.clone(), pauza.clone());
    let dev = cpal::default_host().default_output_device().ok_or("brak domyślnego wyjścia")?;
    let cfg = dev.default_output_config().map_err(|e| e.to_string())?;
    if cfg.sample_format() != cpal::SampleFormat::F32 {
        return Err("wyjście nie jest f32".into());
    }
    let fs = cfg.sample_rate().0;
    let ch = cfg.channels() as usize;
    let (prod, mut cons): (HeapProd<f32>, HeapCons<f32>) = HeapRb::<f32>::new(fs as usize).split(); // 0,5 s stereo
    let mut tmp = Vec::<f32>::with_capacity(8192);
    let stream = dev
        .build_output_stream(
            &cfg.config(),
            move |data: &mut [f32], _| {
                let frames = data.len() / ch;
                tmp.clear();
                tmp.resize(frames * 2, 0.0);
                if !pauza2.load(Ordering::Relaxed) {
                    let _ = cons.pop_slice(&mut tmp);
                }
                for (f, fr) in data.chunks_mut(ch).enumerate() {
                    if ch == 1 {
                        fr[0] = 0.5 * (tmp[2 * f] + tmp[2 * f + 1]);
                    } else {
                        fr[0] = tmp[2 * f];
                        fr[1] = tmp[2 * f + 1];
                        for x in fr.iter_mut().skip(2) {
                            *x = 0.0;
                        }
                    }
                }
            },
            move |e| {
                log::error!("odtwarzacz – strumień wyjścia: {e}");
                martwy2.store(true, Ordering::Relaxed);
            },
            None,
        )
        .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    Ok(WlasneWyjscie { _stream: stream, prod, fs, gen, martwy, pauza })
}

/// Dokąd idą próbki bieżącego utworu.
enum Cel {
    Silnik(u32),
    Wlasne(WlasneWyjscie),
}

impl Cel {
    fn wybierz() -> Result<Cel, String> {
        match audio::wstrzykniecie_fs() {
            Some(fs) => Ok(Cel::Silnik(fs)),
            None => otworz_wyjscie().map(Cel::Wlasne),
        }
    }
    fn fs(&self) -> u32 {
        match self {
            Cel::Silnik(fs) => *fs,
            Cel::Wlasne(w) => w.fs,
        }
    }
    /// Wolne miejsce (próbki), None = cel zniknął (np. zmiana trybu).
    fn wolne(&mut self) -> Option<usize> {
        match self {
            Cel::Silnik(fs) => (audio::wstrzykniecie_fs() == Some(*fs)).then(audio::wstrzyknij_wolne).flatten(),
            // nowe domyślne wyjście (słuchawki, Bluetooth) → None = otwórz utwór na nowym urządzeniu
            // …tak samo, gdy strumień padł albo ruszył silnik w aplikacji (wtedy próbki mają iść przez niego)
            Cel::Wlasne(w) => (w.gen == crate::apo_check::generacja() && !w.martwy.load(Ordering::Relaxed) && audio::wstrzykniecie_fs().is_none())
                .then(|| w.prod.vacant_len()),
        }
    }
    /// Ile próbek czeka jeszcze na zagranie (tylko własny strumień; silnik – nieznane, 0).
    fn zalega(&self) -> usize {
        match self {
            Cel::Silnik(_) => 0,
            Cel::Wlasne(w) => w.prod.occupied_len(),
        }
    }
    fn pauza(&self, b: bool) {
        match self {
            Cel::Silnik(_) => audio::wstrzyknij_pauza(b),
            Cel::Wlasne(w) => w.pauza.store(b, Ordering::Relaxed),
        }
    }
    fn wypchnij(&mut self, s: &[f32]) -> usize {
        match self {
            Cel::Silnik(_) => audio::wstrzyknij(s),
            Cel::Wlasne(w) => w.prod.push_slice(s),
        }
    }
    fn wyczysc(&mut self) {
        if let Cel::Wlasne(w) = self {
            // producent nie może opróżnić bufora – nowy strumień
            if let Ok(n) = otworz_wyjscie() {
                *w = n;
            }
        } else {
            audio::wstrzyknij_wyczysc();
        }
    }
}

// ───────────────────────────── dekodowanie ─────────────────────────────

/// Plik z dysku (Symphonia).
struct Plik {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    dekoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    tb: Option<symphonia::core::units::TimeBase>,
}

/// Bieżący utwór: plik albo strumień Spotify; wspólne przeliczanie częstotliwości na częstotliwość wyjścia.
struct Utwor {
    plik: Option<Plik>,
    fs_pliku: u32,
    resampler: Option<rubato::FftFixedIn<f32>>,
    we: [Vec<f32>; 2],
    pozycja: f64,
}

fn jest_spotify(sciezka: &str) -> bool {
    sciezka.starts_with("spotify:")
}

fn otworz_utwor(sciezka: &str, fs_wy: u32) -> Result<Utwor, String> {
    let (plik, fs_pliku) = if jest_spotify(sciezka) {
        crate::spotify_pc::get().graj(sciezka, 0);
        (None, crate::spotify_pc::FS)
    } else {
        crate::spotify_pc::stop_jesli_aktywny();
        let probe = otworz(Path::new(sciezka))?;
        let format = probe.format;
        let tr = format.default_track().ok_or("brak ścieżki audio")?;
        let track_id = tr.id;
        let tb = tr.codec_params.time_base;
        let fs_pliku = tr.codec_params.sample_rate.ok_or("nieznana częstotliwość")?;
        let dekoder = symphonia::default::get_codecs().make(&tr.codec_params, &DecoderOptions::default()).map_err(|e| format!("kodek: {e}"))?;
        (Some(Plik { format, dekoder, track_id, tb }), fs_pliku)
    };
    let resampler = if fs_pliku != fs_wy {
        Some(rubato::FftFixedIn::<f32>::new(fs_pliku as usize, fs_wy as usize, 1024, 2, 2).map_err(|e| format!("resampler: {e}"))?)
    } else {
        None
    };
    Ok(Utwor { plik, fs_pliku, resampler, we: [Vec::new(), Vec::new()], pozycja: 0.0 })
}

impl Utwor {
    fn spotify(&self) -> bool {
        self.plik.is_none()
    }

    /// Przelicza zebrane próbki na częstotliwość wyjścia (przeplecione stereo); pusty wektor = za mało danych.
    fn wyjmij(resampler: &mut Option<rubato::FftFixedIn<f32>>, we: &mut [Vec<f32>; 2]) -> Result<Vec<f32>, String> {
        let mut out = Vec::new();
        match resampler {
            None => {
                out.reserve(we[0].len() * 2);
                for i in 0..we[0].len() {
                    out.push(we[0][i]);
                    out.push(we[1][i]);
                }
                we[0].clear();
                we[1].clear();
            }
            Some(rs) => {
                use rubato::Resampler;
                while we[0].len() >= rs.input_frames_next() {
                    let n = rs.input_frames_next();
                    let blok = [&we[0][..n], &we[1][..n]];
                    let wy = rs.process(&blok, None).map_err(|e| e.to_string())?;
                    for i in 0..wy[0].len() {
                        out.push(wy[0][i]);
                        out.push(wy[1][i]);
                    }
                    we[0].drain(..n);
                    we[1].drain(..n);
                }
            }
        }
        Ok(out)
    }

    /// Następna porcja przeplecionych próbek stereo w częstotliwości wyjścia.
    /// None = koniec utworu; pusty wektor = chwilowo brak danych (Spotify jeszcze buforuje).
    fn porcja(&mut self) -> Result<Option<Vec<f32>>, String> {
        let Some(p) = self.plik.as_mut() else {
            // Spotify: bierzemy, co librespot już zdekodował
            let st = crate::spotify_pc::strumien();
            let mut buf = Vec::with_capacity(8192);
            if crate::spotify_pc::byla_odmowa() {
                return Err(ODMOWA.into());
            }
            let n = crate::spotify_pc::pobierz(&mut buf, 8192);
            if n == 0 {
                if st.blad.load(Ordering::Relaxed) {
                    return Err("utwór Spotify niedostępny".into());
                }
                if st.koniec.load(Ordering::Relaxed) && crate::spotify_pc::bufor_pusty() {
                    return Ok(None);
                }
                return Ok(Some(Vec::new()));
            }
            for fr in buf[..n].chunks(2) {
                self.we[0].push(fr[0]);
                self.we[1].push(fr[1]);
            }
            self.pozycja += (n / 2) as f64 / self.fs_pliku as f64;
            return Self::wyjmij(&mut self.resampler, &mut self.we).map(Some);
        };
        loop {
            let pakiet = match p.format.next_packet() {
                Ok(p) => p,
                Err(symphonia::core::errors::Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
                Err(symphonia::core::errors::Error::ResetRequired) => return Ok(None),
                Err(e) => return Err(e.to_string()),
            };
            if pakiet.track_id() != p.track_id {
                continue;
            }
            if let Some(tb) = p.tb {
                let t = tb.calc_time(pakiet.ts());
                self.pozycja = t.seconds as f64 + t.frac;
            }
            let dek = match p.dekoder.decode(&pakiet) {
                Ok(d) => d,
                Err(symphonia::core::errors::Error::DecodeError(e)) => {
                    log::warn!("odtwarzacz: uszkodzony pakiet ({e}) – pomijam");
                    continue;
                }
                Err(e) => return Err(e.to_string()),
            };
            let spec = *dek.spec();
            let ch = spec.channels.count().max(1);
            let mut sb = SampleBuffer::<f32>::new(dek.capacity() as u64, spec);
            sb.copy_interleaved_ref(dek);
            for fr in sb.samples().chunks(ch) {
                self.we[0].push(fr[0]);
                self.we[1].push(if ch > 1 { fr[1] } else { fr[0] });
            }
            let out = Self::wyjmij(&mut self.resampler, &mut self.we)?;
            if !out.is_empty() {
                return Ok(Some(out));
            }
        }
    }

    fn szukaj(&mut self, sek: f64) {
        let sek = sek.max(0.0);
        self.we[0].clear();
        self.we[1].clear();
        match self.plik.as_mut() {
            None => {
                crate::spotify_pc::get().szukaj((sek * 1000.0) as u32);
                self.pozycja = sek;
            }
            Some(p) => {
                let t = Time::new(sek as u64, sek.fract());
                if p.format.seek(SeekMode::Coarse, SeekTo::Time { time: t, track_id: Some(p.track_id) }).is_ok() {
                    p.dekoder.reset();
                    self.pozycja = sek;
                }
            }
        }
    }

    fn pauza(&self, pauza: bool) {
        if self.spotify() {
            if pauza {
                crate::spotify_pc::get().pauza();
            } else {
                crate::spotify_pc::get().wznow();
            }
        }
    }
}

// ───────────────────────────── fala dla wizualizacji ─────────────────────────────

/// Ostatnie 1024 próbki stereo tego, co słychać (odtwarzacz albo podsłuch wyjścia) – źródło wbudowanej wizualizacji.
struct Fala {
    l: [f32; szklo_core::SCOPE_LEN],
    r: [f32; szklo_core::SCOPE_LEN],
    pos: usize,
    licznik: u64,
    wyslano: u64,
    env: f32,
}

static FALA: Mutex<Fala> = Mutex::new(Fala { l: [0.0; szklo_core::SCOPE_LEN], r: [0.0; szklo_core::SCOPE_LEN], pos: 0, licznik: 0, wyslano: 0, env: 0.0 });

fn fala_dopisz(stereo: &[f32]) {
    let Ok(mut f) = FALA.lock() else { return };
    let ost = stereo.len().saturating_sub(2 * szklo_core::SCOPE_LEN) & !1;
    for fr in stereo[ost..].chunks_exact(2) {
        let p = f.pos;
        f.l[p] = fr[0];
        f.r[p] = fr[1];
        f.pos = (p + 1) % szklo_core::SCOPE_LEN;
    }
    f.licznik += 1;
}

/// Fala od ostatniego wywołania jako base64 (2048 bajtów: lewy, prawy); `None`, gdy nie przyszedł nowy dźwięk.
pub fn fala_b64() -> Option<String> {
    use base64::Engine;
    let mut f = FALA.lock().ok()?;
    if f.licznik == f.wyslano {
        return None;
    }
    f.wyslano = f.licznik;
    let n = szklo_core::SCOPE_LEN;
    let (mut l, mut r) = ([0f32; szklo_core::SCOPE_LEN], [0f32; szklo_core::SCOPE_LEN]);
    for i in 0..n {
        let k = (f.pos + i) % n;
        l[i] = f.l[k];
        r[i] = f.r[k];
    }
    let mut env = f.env;
    let mut out = [0u8; 2 * szklo_core::SCOPE_LEN];
    szklo_core::scope_bytes(&l, &r, &mut env, &mut out);
    f.env = env;
    Some(base64::engine::general_purpose::STANDARD.encode(out))
}

// ───────────────────────────── widmo ─────────────────────────────

pub(crate) struct Analizator {
    fft: Arc<dyn rustfft::Fft<f32>>,
    okno: Vec<f32>,
    bufor: Vec<f32>,
    robocze: Vec<rustfft::num_complex::Complex<f32>>,
    fs: u32,
}

const N_FFT: usize = 2048;

impl Analizator {
    pub(crate) fn new(fs: u32) -> Self {
        let mut pl = rustfft::FftPlanner::new();
        let okno = (0..N_FFT).map(|i| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / N_FFT as f32).cos()).collect();
        Self { fft: pl.plan_fft_forward(N_FFT), okno, bufor: Vec::with_capacity(N_FFT * 2), robocze: Vec::with_capacity(N_FFT), fs: fs.max(1) }
    }
    pub(crate) fn dodaj(&mut self, stereo: &[f32], wynik: &Mutex<[f32; PASMA]>) {
        fala_dopisz(stereo);
        for fr in stereo.chunks(2) {
            self.bufor.push(0.5 * (fr[0] + fr.get(1).copied().unwrap_or(fr[0])));
        }
        // jedna analiza na ~1/30 s
        let krok = ((self.fs / 30) as usize).max(1);
        while self.bufor.len() >= N_FFT.max(krok) {
            // bufor roboczy trzymany w strukturze – bez alokacji przy każdej analizie
            let mut c = std::mem::take(&mut self.robocze);
            c.clear();
            c.extend(self.bufor[..N_FFT].iter().zip(&self.okno).map(|(x, w)| rustfft::num_complex::Complex::new(x * w, 0.0)));
            self.fft.process(&mut c);
            let mut pasma = [0.0f32; PASMA];
            // pasma logarytmiczne 40 Hz – 16 kHz
            let (f0, f1) = (40.0f32, 16_000.0f32);
            for (b, p) in pasma.iter_mut().enumerate() {
                let lo = f0 * (f1 / f0).powf(b as f32 / PASMA as f32);
                let hi = f0 * (f1 / f0).powf((b + 1) as f32 / PASMA as f32);
                // indeksy przycięte do 1…N/2−1: przy niskiej częstotliwości urządzenia (np. Bluetooth Hands-Free 8 kHz)
                // górne pasma wypadają poza widmo – bez przycięcia byłby odczyt poza tablicą i koniec wątku
                let max_k = N_FFT / 2 - 1;
                let ilo = ((lo * N_FFT as f32 / self.fs as f32) as usize).clamp(1, max_k);
                let ihi = ((hi * N_FFT as f32 / self.fs as f32) as usize).clamp(ilo, max_k);
                let mut m = 0.0f32;
                if hi < self.fs as f32 / 2.0 || lo < self.fs as f32 / 2.0 {
                    for x in &c[ilo..=ihi] {
                        m = m.max(x.norm());
                    }
                }
                let db = 20.0 * (m * 4.0 / N_FFT as f32 + 1e-9).log10();
                *p = ((db + 66.0) / 66.0).clamp(0.0, 1.0); // −66…0 dBFS → 0…1
            }
            if let Ok(mut w) = wynik.lock() {
                *w = pasma;
            }
            self.robocze = c;
            self.bufor.drain(..krok.min(self.bufor.len()));
        }
    }
}

// ───────────────────────────── pętla ─────────────────────────────

/// Zapis playlisty na dysk – wołany POZA blokadą stanu (UI czyta stan 10×/s) i tylko po zmianie listy/trybu.
fn zapisz(z: &Zapis) {
    let d = crate::settings::dir();
    let _ = std::fs::create_dir_all(&d);
    if let Ok(t) = serde_json::to_string_pretty(&z) {
        let tmp = d.join("playlista.json.tmp");
        if std::fs::write(&tmp, t).is_ok() {
            let _ = std::fs::rename(tmp, plik_zapisu());
        }
    }
}

fn losowy(n: usize, poza: Option<usize>) -> usize {
    let ns = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos() as usize).unwrap_or(7);
    if n <= 1 {
        return 0;
    }
    let mut i = (ns ^ (ns >> 7)) % n;
    if Some(i) == poza {
        i = (i + 1) % n;
    }
    i
}

fn petla(rx: Receiver<Rozkaz>, stan: Arc<Mutex<Stan>>, widmo: Arc<Mutex<[f32; PASMA]>>, zmiana: Arc<AtomicBool>) {
    let mut utwor: Option<(Utwor, Cel, Analizator)> = None;
    let mut zaleglosc: Vec<f32> = Vec::new();
    // „brudne” = lista/tryb/bieżący utwór do zapisania na dysk (robione w wolnej chwili, poza blokadą)
    let brudne = std::cell::Cell::new(false);
    let mut bledy_pod_rzad = 0usize;
    let oznacz = |_s: &Stan| {
        zmiana.store(true, Ordering::Relaxed);
    };

    // otwiera utwór o indeksie i (albo zgłasza błąd i zwraca false)
    let start = |i: usize, utwor: &mut Option<(Utwor, Cel, Analizator)>, zaleglosc: &mut Vec<f32>| -> bool {
        let sciezka = match stan.lock().ok().and_then(|s| s.lista.get(i).map(|p| p.sciezka.clone())) {
            Some(s) => s,
            None => return false,
        };
        // Ten sam strumień wyjścia dla kolejnego utworu (jeśli nadal ważny): końcówka poprzedniego dogrywa się
        // z bufora, a nowy zaczyna się zaraz po niej – bez ucinania ostatnich 0,5 s i bez przerwy między utworami.
        let stary = utwor.take().map(|(_, c, _)| c);
        zaleglosc.clear();
        brudne.set(true);
        let cel = match stary {
            Some(mut c) => {
                if c.wolne().is_some() {
                    Ok(c)
                } else {
                    drop(c);
                    Cel::wybierz()
                }
            }
            None => Cel::wybierz(),
        };
        let wynik = cel.and_then(|cel| {
            let u = otworz_utwor(&sciezka, cel.fs())?;
            let fs = cel.fs();
            Ok((u, cel, Analizator::new(fs)))
        });
        let mut s = stan.lock().unwrap();
        s.biezacy = Some(i);
        s.pozycja = 0.0;
        match wynik {
            Ok(u) => {
                log::info!("odtwarzacz: {sciezka} ({} Hz → {} Hz)", u.0.fs_pliku, u.1.fs());
                s.czas = s.lista[i].czas;
                s.gra = true;
                s.blad = None;
                *utwor = Some(u);
                drop(s);
                zmiana.store(true, Ordering::Relaxed);
                true
            }
            Err(e) => {
                log::error!("odtwarzacz: {sciezka}: {e}");
                s.gra = false;
                s.blad = Some(format!("{}: {e}", Path::new(&sciezka).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
                drop(s);
                zmiana.store(true, Ordering::Relaxed);
                false
            }
        }
    };

    let nastepny_idx = |s: &Stan, wymus: bool| -> Option<usize> {
        let n = s.lista.len();
        if n == 0 {
            return None;
        }
        let i = s.biezacy.unwrap_or(0);
        if !wymus && s.powtarzanie == Powtarzanie::Jeden {
            return Some(i.min(n - 1));
        }
        if s.losowo {
            return Some(losowy(n, Some(i)));
        }
        if i + 1 < n {
            Some(i + 1)
        } else if s.powtarzanie == Powtarzanie::Wszystko || wymus {
            Some(0)
        } else {
            None
        }
    };

    loop {
        // rozkazy (blokująco, gdy nic nie gra)
        let gra = utwor.is_some() && stan.lock().map(|s| s.gra).unwrap_or(false);
        let r = if gra {
            match rx.try_recv() {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(_) => return,
            }
        } else {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(r) => Some(r),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                Err(_) => return,
            }
        };
        if r.is_none() && brudne.replace(false) {
            let z = stan.lock().ok().map(|s| Zapis { lista: s.lista.clone(), biezacy: s.biezacy, losowo: s.losowo, powtarzanie: s.powtarzanie });
            if let Some(z) = z {
                zapisz(&z);
            }
        }
        if let Some(r) = r {
            match r {
                Rozkaz::Dodaj(nowe) => {
                    log::info!("odtwarzacz: dodano {} plików", nowe.len());
                    let mut s = stan.lock().unwrap();
                    let bylo_pusto = s.lista.is_empty();
                    s.lista.extend(nowe);
                    if bylo_pusto && !s.lista.is_empty() {
                        s.biezacy = Some(0);
                    }
                    s.wersja += 1;
                    brudne.set(true);
                    oznacz(&s);
                }
                Rozkaz::Graj(i) => {
                    let i = i.or_else(|| stan.lock().ok().and_then(|s| s.biezacy)).unwrap_or(0);
                    let wznow = utwor.is_some() && stan.lock().map(|s| s.biezacy == Some(i)).unwrap_or(false);
                    if wznow {
                        if let Some((u, _, _)) = utwor.as_ref() {
                            u.pauza(false);
                        }
                        let mut s = stan.lock().unwrap();
                        s.gra = true;
                        oznacz(&s);
                    } else {
                        // inny utwór wybrany ręcznie: bez dogrywania końcówki poprzedniego
                        if let Some((_, cel, _)) = utwor.as_mut() {
                            cel.wyczysc();
                        }
                        start(i, &mut utwor, &mut zaleglosc);
                        oznacz(&stan.lock().unwrap());
                    }
                }
                Rozkaz::Pauza => {
                    if let Some((u, _, _)) = utwor.as_ref() {
                        u.pauza(true);
                    }
                    let mut s = stan.lock().unwrap();
                    s.gra = false;
                    oznacz(&s);
                }
                Rozkaz::PlayPauza => {
                    let (gra, b) = stan.lock().map(|s| (s.gra, s.biezacy)).unwrap_or((false, None));
                    if gra {
                        if let Some((u, _, _)) = utwor.as_ref() {
                            u.pauza(true);
                        }
                        let mut s = stan.lock().unwrap();
                        s.gra = false;
                        oznacz(&s);
                    } else if utwor.is_some() {
                        if let Some((u, _, _)) = utwor.as_ref() {
                            u.pauza(false);
                        }
                        let mut s = stan.lock().unwrap();
                        s.gra = true;
                        oznacz(&s);
                    } else {
                        start(b.unwrap_or(0), &mut utwor, &mut zaleglosc);
                    }
                }
                Rozkaz::Nastepny | Rozkaz::Poprzedni => {
                    let dalej = matches!(r, Rozkaz::Nastepny);
                    let idx = {
                        let s = stan.lock().unwrap();
                        if dalej {
                            nastepny_idx(&s, true)
                        } else {
                            let n = s.lista.len();
                            // >3 s od początku → od początku tego samego
                            if s.pozycja > 3.0 {
                                s.biezacy
                            } else {
                                s.biezacy.map(|i| if i == 0 { n.saturating_sub(1) } else { i - 1 })
                            }
                        }
                    };
                    if let Some(i) = idx {
                        if let Some((_, cel, _)) = utwor.as_mut() {
                            cel.wyczysc();
                        }
                        start(i, &mut utwor, &mut zaleglosc);
                    }
                }
                Rozkaz::Szukaj(sek) => {
                    if let Some((u, cel, _)) = utwor.as_mut() {
                        u.szukaj(sek);
                        cel.wyczysc();
                        zaleglosc.clear();
                        let mut s = stan.lock().unwrap();
                        s.pozycja = sek;
                        zmiana.store(true, Ordering::Relaxed);
                    }
                }
                Rozkaz::Losowo(b) => {
                    let mut s = stan.lock().unwrap();
                    s.losowo = b;
                    brudne.set(true);
                    oznacz(&s);
                }
                Rozkaz::Powtarzanie(p) => {
                    let mut s = stan.lock().unwrap();
                    s.powtarzanie = p;
                    brudne.set(true);
                    oznacz(&s);
                }
                Rozkaz::Usun(i) => {
                    let mut s = stan.lock().unwrap();
                    if i < s.lista.len() {
                        s.lista.remove(i);
                        match s.biezacy {
                            Some(b) if b == i => {
                                utwor = None;
                                s.gra = false;
                                s.biezacy = if s.lista.is_empty() { None } else { Some(i.min(s.lista.len() - 1)) };
                            }
                            Some(b) if b > i => s.biezacy = Some(b - 1),
                            _ => {}
                        }
                        s.wersja += 1;
                        brudne.set(true);
                        oznacz(&s);
                    }
                }
                Rozkaz::Zastap(lista, i) => {
                    utwor = None;
                    crate::spotify_pc::stop_jesli_aktywny();
                    {
                        let mut s = stan.lock().unwrap();
                        s.lista = lista;
                        s.biezacy = None;
                        s.gra = false;
                        s.wersja += 1;
                        brudne.set(true);
                        oznacz(&s);
                    }
                    start(i, &mut utwor, &mut zaleglosc);
                }
                Rozkaz::Wyczysc => {
                    crate::spotify_pc::stop_jesli_aktywny();
                    utwor = None;
                    let mut s = stan.lock().unwrap();
                    s.lista.clear();
                    s.biezacy = None;
                    s.gra = false;
                    s.pozycja = 0.0;
                    s.czas = 0.0;
                    s.wersja += 1;
                    brudne.set(true);
                    oznacz(&s);
                }
            }
            // pauza/wznowienie działa od razu: wyjście oddaje ciszę i nie zjada bufora (zamiast grać jeszcze 0,5 s)
            if let Some((_, cel, _)) = utwor.as_ref() {
                cel.pauza(!stan.lock().map(|s| s.gra).unwrap_or(false));
            }
            bledy_pod_rzad = 0;
            continue;
        }

        // odtwarzanie
        let gra = stan.lock().map(|s| s.gra).unwrap_or(false);
        enum Akcja {
            Dalej,
            Ponow,
            Koniec,
            Blad(String),
        }
        let akcja = {
            let Some((u, cel, an)) = utwor.as_mut() else { continue };
            if !gra {
                if let Ok(mut w) = widmo.lock() {
                    for x in w.iter_mut() {
                        *x *= 0.8;
                    }
                }
                continue;
            }
            if cel.wolne().is_none() {
                Akcja::Ponow // zmiana trybu albo nowe wyjście Windows
            } else if !zaleglosc.is_empty() {
                // najpierw zaległe próbki (gdy cel był pełny)
                match cel.wolne() {
                    None => Akcja::Ponow, // cel zniknął (zmiana trybu)
                    Some(w) if w < 2 => {
                        std::thread::sleep(Duration::from_millis(5));
                        Akcja::Dalej
                    }
                    Some(_) => {
                        let n = cel.wypchnij(&zaleglosc);
                        zaleglosc.drain(..n);
                        Akcja::Dalej
                    }
                }
            } else {
                match u.porcja() {
                    Ok(Some(p)) if p.is_empty() => {
                        // Spotify buforuje – chwila przerwy
                        std::thread::sleep(Duration::from_millis(5));
                        Akcja::Dalej
                    }
                    Ok(Some(p)) => {
                        bledy_pod_rzad = 0;
                        an.dodaj(&p, &widmo);
                        let n = cel.wypchnij(&p);
                        if n < p.len() {
                            zaleglosc.extend_from_slice(&p[n..]);
                        }
                        if let Ok(mut s) = stan.lock() {
                            s.pozycja = u.pozycja;
                        }
                        Akcja::Dalej
                    }
                    Ok(None) => Akcja::Koniec,
                    Err(e) => Akcja::Blad(e),
                }
            }
        };
        match akcja {
            Akcja::Dalej => {}
            Akcja::Ponow => {
                // otwórz utwór ponownie od tej samej pozycji (nowy cel)
                let (i, poz) = stan.lock().map(|s| (s.biezacy, s.pozycja)).unwrap_or((None, 0.0));
                if let Some(i) = i {
                    if start(i, &mut utwor, &mut zaleglosc) {
                        if let Some((u, _, _)) = utwor.as_mut() {
                            u.szukaj(poz);
                        }
                    }
                }
            }
            Akcja::Koniec => {
                let nast = stan.lock().ok().and_then(|s| nastepny_idx(&s, false));
                match nast {
                    Some(i) => {
                        start(i, &mut utwor, &mut zaleglosc);
                    }
                    None => {
                        // ostatni utwór: daj wybrzmieć temu, co jeszcze czeka w buforze wyjścia
                        if let Some((_, cel, _)) = utwor.as_mut() {
                            let t0 = std::time::Instant::now();
                            while cel.zalega() > 0 && cel.wolne().is_some() && t0.elapsed() < Duration::from_millis(700) {
                                std::thread::sleep(Duration::from_millis(5));
                            }
                        }
                        utwor = None;
                        let mut s = stan.lock().unwrap();
                        s.gra = false;
                        s.pozycja = 0.0;
                        oznacz(&s);
                    }
                }
            }
            Akcja::Blad(e) if e == ODMOWA => {
                // Spotify odmówił klucza – stop, bez przechodzenia do kolejnych utworów (każdy byłby kolejną prośbą)
                log::warn!("odtwarzacz: Spotify odmówił klucza – zatrzymuję kolejkę");
                crate::spotify_pc::stop_jesli_aktywny();
                utwor = None;
                let mut s = stan.lock().unwrap();
                s.gra = false;
                s.blad = Some(crate::spotify_pc::KOMUNIKAT_ODMOWY.into());
                oznacz(&s);
            }
            Akcja::Blad(e) => {
                log::error!("odtwarzacz: błąd dekodowania: {e}");
                // następny utwór, ale bez zapętlania: po przejściu całej listy samych błędów (albo na jej końcu) – stop
                bledy_pod_rzad += 1;
                let (nast, n) = stan.lock().map(|s| (nastepny_idx(&s, false), s.lista.len())).unwrap_or((None, 0));
                utwor = None;
                match nast {
                    Some(i) if bledy_pod_rzad < n.max(1) => {
                        start(i, &mut utwor, &mut zaleglosc);
                    }
                    _ => {
                        bledy_pod_rzad = 0;
                        let mut s = stan.lock().unwrap();
                        s.gra = false;
                        s.blad = Some(format!("Nie udało się odtworzyć: {e}"));
                        oznacz(&s);
                    }
                }
            }
        }
    }
}
