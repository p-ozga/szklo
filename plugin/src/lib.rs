//! Szkło – wtyczka VST 2.4 (Equalizer APO hostuje tylko VST2).
//!
//! Parametry hosta: 23 × (on/off, moc) + globalnie „Włączone” + sufit dBTP = 48 parametrów.
//! Stan zapisywany w chunku (tekst linii protokołu IPC) – Equalizer APO trzyma go w config.txt.
//! Sterowanie na żywo z aplikacji Szkło: serwer named pipe (tylko w procesie audiodg.exe – instancja
//! w edytorze konfiguracji Equalizer APO nie przetwarza dźwięku systemu, więc nie zajmuje potoku).
//!
//! W audiodg.exe wszystkie instancje wtyczki (Equalizer APO tworzy osobną na każde urządzenie, a przy zmianie
//! konfiguracji nową PRZED usunięciem starej) dzielą jeden stan parametrów, jedne mierniki i jeden serwer potoku –
//! inaczej aplikacja sterowałaby tylko instancją, która pierwsza zajęła potok, a po przeładowaniu konfiguracji
//! traciłaby połączenie. Panika w `process` nie może przejść przez granicę FFI (zabiłaby usługę audio Windows):
//! jest łapana, a wtyczka przechodzi w tryb przelotowy.

#![allow(deprecated)] // crate `vst` jest oznaczony jako przestarzały, ale to jedyna czysta implementacja ABI VST2 w Ruście

mod diag;
#[cfg(windows)]
mod pipe;

use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Mutex};
use szklo_core::ipc::{apply_params_line, encode_params};
use szklo_core::{Ctx, Engine, SharedMeters, SharedParams, MODULES, MODULE_COUNT, PRESETS};
use vst::buffer::AudioBuffer;
use vst::plugin::{Category, HostCallback, Info, Plugin, PluginParameters};
use vst::plugin_main;

const N_PARAMS: i32 = (MODULE_COUNT * 2 + 2) as i32;

struct Params {
    shared: Arc<SharedParams>,
}

impl Params {
    fn idx(index: i32) -> Option<(usize, bool)> {
        let i = index as usize;
        if i < MODULE_COUNT * 2 {
            Some((i / 2, i % 2 == 0))
        } else {
            None
        }
    }
}

impl PluginParameters for Params {
    fn get_parameter(&self, index: i32) -> f32 {
        match Self::idx(index) {
            Some((m, true)) => self.shared.module(m).0 as u8 as f32,
            Some((m, false)) => self.shared.module(m).1,
            None if index == N_PARAMS - 2 => self.shared.enabled() as u8 as f32,
            None => (self.shared.ceiling_dbtp() + 6.0) / 6.0, // −6…0 dBTP → 0…1
        }
    }
    fn set_parameter(&self, index: i32, value: f32) {
        let v = if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
        match Self::idx(index) {
            Some((m, true)) => self.shared.set_module_on(m, v >= 0.5),
            Some((m, false)) => self.shared.set_module_amount(m, v),
            None if index == N_PARAMS - 2 => self.shared.set_enabled(v >= 0.5),
            None if index == N_PARAMS - 1 => self.shared.set_ceiling_dbtp(v * 6.0 - 6.0),
            None => {}
        }
    }
    fn get_parameter_name(&self, index: i32) -> String {
        match Self::idx(index) {
            Some((m, true)) => format!("{} wł.", MODULES[m].name_pl),
            Some((m, false)) => format!("{} moc", MODULES[m].name_pl),
            None if index == N_PARAMS - 2 => "Włączone".into(),
            None => "Sufit".into(),
        }
    }
    fn get_parameter_text(&self, index: i32) -> String {
        match Self::idx(index) {
            Some((m, true)) => if self.shared.module(m).0 { "wł." } else { "wył." }.into(),
            Some((m, false)) => format!("{:.0}", self.shared.module(m).1 * 100.0),
            None if index == N_PARAMS - 2 => if self.shared.enabled() { "tak" } else { "nie" }.into(),
            None => format!("{:.1}", self.shared.ceiling_dbtp()),
        }
    }
    fn get_parameter_label(&self, index: i32) -> String {
        match Self::idx(index) {
            Some((_, false)) => "%".into(),
            None if index == N_PARAMS - 1 => "dBTP".into(),
            _ => String::new(),
        }
    }
    fn can_be_automated(&self, _index: i32) -> bool {
        true
    }
    fn get_preset_data(&self) -> Vec<u8> {
        encode_params(&self.shared).into_bytes()
    }
    fn get_bank_data(&self) -> Vec<u8> {
        self.get_preset_data()
    }
    fn load_preset_data(&self, data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            apply_params_line(s.trim(), &self.shared);
            // „porównaj” nigdy nie jest stanem zapisanym
            self.shared.set_compare(false);
        }
    }
    fn load_bank_data(&self, data: &[u8]) {
        self.load_preset_data(data)
    }
}

type Status = Arc<Mutex<(f64, usize)>>;

struct Szklo {
    params: Arc<Params>,
    meters: Arc<SharedMeters>,
    engine: Option<Engine>,
    fs: f64,
    l: Vec<f32>,
    r: Vec<f32>,
    status: Status,
    /// Po panice w przetwarzaniu: tryb przelotowy do wyładowania wtyczki.
    failed: bool,
    /// diagnostyka (tylko gdy włączony dziennik): liczba wywołań i szczyty wej./wyj.
    d_calls: u64,
    d_in: f32,
    d_out: f32,
    #[cfg(windows)]
    _server: Option<Arc<pipe::Server>>,
}

fn fresh_state() -> (Arc<SharedParams>, Arc<SharedMeters>, Status) {
    let shared = Arc::new(SharedParams::default());
    shared.apply_preset(&PRESETS[0]);
    (shared, Arc::new(SharedMeters::new()), Arc::new(Mutex::new((44_100.0, 0usize))))
}

/// Stan i serwer potoku wspólne dla wszystkich instancji w procesie usługi audio.
#[cfg(windows)]
fn service_state() -> (Arc<SharedParams>, Arc<SharedMeters>, Status, Option<Arc<pipe::Server>>) {
    use std::sync::{OnceLock, Weak};
    static STATE: OnceLock<(Arc<SharedParams>, Arc<SharedMeters>, Status)> = OnceLock::new();
    static SERVER: Mutex<Weak<pipe::Server>> = Mutex::new(Weak::new());
    let (p, m, s) = STATE.get_or_init(fresh_state).clone();
    // Serwer żyje, dopóki żyje choć jedna instancja (ostatnia przy `Drop` zatrzymuje wątek i czeka na niego).
    let server = SERVER.lock().ok().and_then(|mut w| {
        if let Some(live) = w.upgrade() {
            return Some(live);
        }
        let new = Arc::new(pipe::Server::start(p.clone(), m.clone(), s.clone())?);
        *w = Arc::downgrade(&new);
        Some(new)
    });
    (p, m, s, server)
}

impl Default for Szklo {
    fn default() -> Self {
        #[cfg(windows)]
        let (shared, meters, status, server) = if pipe::is_audio_service() {
            service_state()
        } else {
            let (p, m, s) = fresh_state();
            (p, m, s, None)
        };
        #[cfg(not(windows))]
        let (shared, meters, status) = fresh_state();
        Self {
            params: Arc::new(Params { shared }),
            meters,
            engine: None,
            fs: 44_100.0,
            l: Vec::new(),
            r: Vec::new(),
            status,
            failed: false,
            d_calls: 0,
            d_in: 0.0,
            d_out: 0.0,
            #[cfg(windows)]
            _server: server,
        }
    }
}

impl Szklo {
    fn has_server(&self) -> bool {
        #[cfg(windows)]
        {
            self._server.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
    fn ensure_engine(&mut self) -> &mut Engine {
        if self.engine.is_none() {
            let e = Engine::with_ctx(Ctx::new(self.fs), self.params.shared.clone(), self.meters.clone());
            if let Ok(mut s) = self.status.lock() {
                *s = (self.fs, e.latency());
            }
            self.engine = Some(e);
        }
        self.engine.as_mut().unwrap()
    }

    fn process_inner(&mut self, buffer: &mut AudioBuffer<f32>) {
        let n = buffer.samples();
        let (inputs, mut outputs) = buffer.split();
        if inputs.len() < 1 || outputs.len() < 1 {
            return;
        }
        // Equalizer APO może podać mono – wtedy duplikujemy kanał.
        self.l.resize(n, 0.0);
        self.r.resize(n, 0.0);
        self.l.copy_from_slice(&inputs.get(0)[..n]);
        if inputs.len() > 1 {
            self.r.copy_from_slice(&inputs.get(1)[..n]);
        } else {
            self.r.copy_from_slice(&inputs.get(0)[..n]);
        }
        let (mut l, mut r) = (std::mem::take(&mut self.l), std::mem::take(&mut self.r));
        self.ensure_engine().process(&mut l, &mut r);
        outputs.get_mut(0)[..n].copy_from_slice(&l);
        if outputs.len() > 1 {
            outputs.get_mut(1)[..n].copy_from_slice(&r);
        }
        self.l = l;
        self.r = r;
    }
}

/// Tryb przelotowy: wejście → wyjście bez zmian (gdy host przetwarza w miejscu, nic nie kopiujemy).
fn passthrough(buffer: &mut AudioBuffer<f32>) {
    let n = buffer.samples();
    let (inputs, mut outputs) = buffer.split();
    if inputs.len() == 0 {
        return;
    }
    for c in 0..outputs.len() {
        let src = inputs.get(c.min(inputs.len() - 1));
        let dst = outputs.get_mut(c);
        if src.as_ptr() != dst.as_ptr() {
            dst[..n].copy_from_slice(&src[..n]);
        }
    }
}

impl Plugin for Szklo {
    fn new(_host: HostCallback) -> Self {
        let s = Self::default();
        diag::log(&format!("nowa instancja (serwer potoku: {})", if cfg!(windows) && s.has_server() { "tak" } else { "nie" }));
        s
    }

    fn get_info(&self) -> Info {
        Info {
            name: "Szklo".into(),
            vendor: "Pawel Ozga".into(),
            unique_id: 0x535a_4b4c, // 'SZKL'
            version: 10,
            inputs: 2,
            outputs: 2,
            parameters: N_PARAMS,
            category: Category::Mastering,
            preset_chunks: true,
            // host pyta o `Info` przed utworzeniem silnika – latencja toru jest stała: 4096·F (spec v1 §2.2)
            initial_delay: self.engine.as_ref().map(|e| e.latency() as i32).unwrap_or(4096 * Ctx::new(self.fs).f() as i32),
            silent_when_stopped: false,
            ..Default::default()
        }
    }

    fn set_sample_rate(&mut self, rate: f32) {
        let fs = rate as f64;
        // Zakres 8…384 kHz; poza nim (albo NaN) zostaje poprzednia wartość. Silnik dla nowej fs powstaje
        // w `resume` (poza wątkiem audio), awaryjnie przy pierwszym `process`.
        if fs.is_finite() && (8_000.0..=384_000.0).contains(&fs) && (fs - self.fs).abs() > 0.5 {
            self.fs = fs;
            self.engine = None;
        }
    }

    fn resume(&mut self) {
        diag::log(&format!("resume: fs {} Hz", self.fs));
        self.ensure_engine();
    }

    fn process(&mut self, buffer: &mut AudioBuffer<f32>) {
        if buffer.samples() == 0 {
            return;
        }
        if diag::enabled() {
            let n = buffer.samples();
            let (inputs, _) = buffer.split();
            for c in 0..inputs.len() {
                for &v in &inputs.get(c)[..n] {
                    self.d_in = self.d_in.max(v.abs());
                }
            }
        }
        if !self.failed {
            // Panika nie może wyjść poza `extern "C"` (przerwałaby audiodg.exe = dźwięk całego systemu).
            let res = catch_unwind(AssertUnwindSafe(|| self.process_inner(buffer)));
            if res.is_ok() {
                if diag::enabled() {
                    let n = buffer.samples();
                    let (_, outputs) = buffer.split();
                    for c in 0..outputs.len() {
                        for &v in &outputs.get(c)[..n] {
                            self.d_out = self.d_out.max(v.abs());
                        }
                    }
                    self.d_calls += 1;
                    if self.d_calls % 500 == 1 {
                        diag::log(&format!(
                            "process #{}: blok {} próbek, kanały we {} wy {}, szczyt we {:.4} wy {:.4}, fs {}",
                            self.d_calls,
                            n,
                            buffer.input_count(),
                            buffer.output_count(),
                            self.d_in,
                            self.d_out,
                            self.fs
                        ));
                        self.d_in = 0.0;
                        self.d_out = 0.0;
                    }
                }
                return;
            }
            let msg = res.err().and_then(|e| e.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| e.downcast_ref::<String>().cloned())).unwrap_or_default();
            diag::log(&format!("PANIKA w process: {msg}"));
            self.failed = true;
            // stan silnika po panice jest nieokreślony – nie uruchamiamy jego destruktorów
            if let Some(e) = self.engine.take() {
                std::mem::forget(e);
            }
        }
        passthrough(buffer);
    }

    fn get_parameter_object(&mut self) -> Arc<dyn PluginParameters> {
        self.params.clone()
    }
}

impl Drop for Szklo {
    fn drop(&mut self) {
        diag::log(&format!("instancja {:p} usunięta", self as *const _));
    }
}

plugin_main!(Szklo);
