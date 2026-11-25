//! Stan współdzielony między wątkiem audio a UI/IPC – wyłącznie atomiki (bez blokad w wątku audio).

use crate::catalog::MODULE_COUNT;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

#[inline]
fn f2u(x: f32) -> u32 {
    x.to_bits()
}
#[inline]
fn u2f(x: u32) -> f32 {
    f32::from_bits(x)
}

/// Parametry sterowane z UI. Wątek audio czyta je na początku każdego bloku, gdy zmieni się `version`.
pub struct SharedParams {
    on: [AtomicBool; MODULE_COUNT],
    amount: [AtomicU32; MODULE_COUNT],
    master: AtomicU32,
    enabled: AtomicBool,
    compare: AtomicBool,
    ceiling_dbtp: AtomicU32,
    version: AtomicU64,
}

impl Default for SharedParams {
    fn default() -> Self {
        Self {
            on: std::array::from_fn(|_| AtomicBool::new(false)),
            amount: std::array::from_fn(|_| AtomicU32::new(f2u(0.5))),
            master: AtomicU32::new(f2u(1.0)),
            enabled: AtomicBool::new(true),
            compare: AtomicBool::new(false),
            ceiling_dbtp: AtomicU32::new(f2u(-1.0)),
            version: AtomicU64::new(1),
        }
    }
}

impl SharedParams {
    fn bump(&self) {
        self.version.fetch_add(1, Ordering::Release);
    }
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }
    pub fn set_module(&self, idx: usize, on: bool, amount: f32) {
        if idx >= MODULE_COUNT {
            return;
        }
        self.on[idx].store(on, Ordering::Relaxed);
        self.amount[idx].store(f2u(sanitize01(amount)), Ordering::Relaxed);
        self.bump();
    }
    pub fn set_module_on(&self, idx: usize, on: bool) {
        if idx < MODULE_COUNT {
            self.on[idx].store(on, Ordering::Relaxed);
            self.bump();
        }
    }
    pub fn set_module_amount(&self, idx: usize, amount: f32) {
        if idx < MODULE_COUNT {
            self.amount[idx].store(f2u(sanitize01(amount)), Ordering::Relaxed);
            self.bump();
        }
    }
    pub fn module(&self, idx: usize) -> (bool, f32) {
        (self.on[idx].load(Ordering::Relaxed), u2f(self.amount[idx].load(Ordering::Relaxed)))
    }
    /// Moc ogólna (pokrętło na Androidzie): mnożnik mocy wszystkich modułów, 0..1.
    pub fn set_master(&self, m: f32) {
        self.master.store(f2u(sanitize01(m)), Ordering::Relaxed);
        self.bump();
    }
    pub fn master(&self) -> f32 {
        u2f(self.master.load(Ordering::Relaxed))
    }
    /// Przycisk „Włączone/Wyłączone” (globalny bypass).
    pub fn set_enabled(&self, on: bool) {
        self.enabled.store(on, Ordering::Relaxed);
        self.bump();
    }
    pub fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
    /// „Porównaj z oryginałem” – trzymany = bypass.
    pub fn set_compare(&self, hold: bool) {
        self.compare.store(hold, Ordering::Relaxed);
        self.bump();
    }
    pub fn compare(&self) -> bool {
        self.compare.load(Ordering::Relaxed)
    }
    pub fn set_ceiling_dbtp(&self, db: f32) {
        let db = if db.is_finite() { db.clamp(-12.0, 0.0) } else { -1.0 };
        self.ceiling_dbtp.store(f2u(db), Ordering::Relaxed);
        self.bump();
    }
    pub fn ceiling_dbtp(&self) -> f32 {
        u2f(self.ceiling_dbtp.load(Ordering::Relaxed))
    }
    /// Wczytanie presetu (bez przejścia – przejście 150 ms robi UI, wysyłając pośrednie wartości).
    pub fn apply_preset(&self, p: &crate::presets::Preset) {
        for (i, s) in p.modules.iter().enumerate() {
            self.on[i].store(s.0, Ordering::Relaxed);
            self.amount[i].store(f2u(sanitize01(s.1)), Ordering::Relaxed);
        }
        self.bump();
    }
}

fn sanitize01(x: f32) -> f32 {
    if x.is_finite() {
        x.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// Mierniki zasilane z DSP (zapis: wątek audio, odczyt: UI 60 fps).
#[derive(Default)]
pub struct SharedMeters {
    in_peak_db: AtomicU32,
    out_peak_db: AtomicU32,
    gr_db: AtomicU32,
    lufs_short: AtomicU32,
    true_peak_db: AtomicU32,
    frames: AtomicU64,
    module_gr_db: [AtomicU32; MODULE_COUNT],
    /// podgląd fali wyjścia (wizualizacja)
    pub scope: Scope,
}

/// Długość podglądu fali (próbek na kanał) – tyle, ile potrzebuje wizualizacja.
pub const SCOPE_LEN: usize = 1024;

/// Podgląd ostatnich próbek wyjścia dla wizualizacji. Bez blokad: wątek dźwięku tylko zapisuje, okno tylko czyta
/// (pojedyncza niespójna próbka na granicy zapisu nie ma znaczenia dla obrazu). Wyłączony nic nie kosztuje.
pub struct Scope {
    on: AtomicBool,
    pos: AtomicU32,
    l: Vec<AtomicU32>,
    r: Vec<AtomicU32>,
    /// obwiednia szczytu do automatycznego wzmocnienia obrazu (bity f32)
    env: AtomicU32,
    /// pozycja zapisu przy ostatnim odczycie (czy od tamtej pory przyszedł dźwięk)
    seen: AtomicU32,
}

impl Default for Scope {
    fn default() -> Self {
        let z = || (0..SCOPE_LEN).map(|_| AtomicU32::new(0)).collect::<Vec<_>>();
        Self { on: AtomicBool::new(false), pos: AtomicU32::new(0), l: z(), r: z(), env: AtomicU32::new(0), seen: AtomicU32::new(u32::MAX) }
    }
}

impl Scope {
    pub fn set_enabled(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
    }
    pub fn enabled(&self) -> bool {
        self.on.load(Ordering::Relaxed)
    }
    /// Wątek dźwięku: dopisuje blok wyjścia (nic nie robi, gdy podgląd wyłączony).
    #[inline]
    pub(crate) fn push(&self, l: &[f64], r: &[f64]) {
        if !self.on.load(Ordering::Relaxed) {
            return;
        }
        let mut p = self.pos.load(Ordering::Relaxed) as usize;
        for (a, b) in l.iter().zip(r) {
            self.l[p].store(f2u(*a as f32), Ordering::Relaxed);
            self.r[p].store(f2u(*b as f32), Ordering::Relaxed);
            p = (p + 1) & (SCOPE_LEN - 1);
        }
        self.pos.store(p as u32, Ordering::Relaxed);
    }
    /// Ostatnie `SCOPE_LEN` próbek, od najstarszej.
    pub fn read(&self, l: &mut [f32; SCOPE_LEN], r: &mut [f32; SCOPE_LEN]) {
        let p = self.pos.load(Ordering::Relaxed) as usize;
        for i in 0..SCOPE_LEN {
            let k = (p + i) & (SCOPE_LEN - 1);
            l[i] = u2f(self.l[k].load(Ordering::Relaxed));
            r[i] = u2f(self.r[k].load(Ordering::Relaxed));
        }
    }
    /// Fala gotowa dla wizualizacji: `out[..1024]` lewy, `out[1024..]` prawy, 8 bitów ze środkiem 128.
    /// Zwraca false (i nic nie zapisuje), gdy od poprzedniego odczytu nie przyszedł żaden dźwięk.
    pub fn read_bytes(&self, out: &mut [u8; 2 * SCOPE_LEN]) -> bool {
        let p = self.pos.load(Ordering::Relaxed);
        if self.seen.swap(p, Ordering::Relaxed) == p {
            return false;
        }
        let (mut l, mut r) = ([0f32; SCOPE_LEN], [0f32; SCOPE_LEN]);
        self.read(&mut l, &mut r);
        let mut env = u2f(self.env.load(Ordering::Relaxed));
        scope_bytes(&l, &r, &mut env, out);
        self.env.store(f2u(env), Ordering::Relaxed);
        true
    }
}

/// Zamiana fali na 8 bitów z automatycznym wzmocnieniem (cicha muzyka też ma poruszać obrazem; cisza zostaje ciszą).
/// `env` – stan między wywołaniami (ok. 30 razy na sekundę): szczyt z opadaniem ok. 1 s.
pub fn scope_bytes(l: &[f32], r: &[f32], env: &mut f32, out: &mut [u8]) {
    let n = l.len().min(r.len()).min(out.len() / 2);
    let peak = l[..n].iter().chain(&r[..n]).fold(0f32, |m, v| m.max(v.abs()));
    *env = if peak.is_finite() { peak.max(*env * 0.97) } else { 0.0 };
    let gain = if *env > 1e-4 { (0.7 / *env).clamp(1.0, 12.0) } else { 0.0 };
    for i in 0..n {
        out[i] = (128.0 + (l[i] * gain).clamp(-1.0, 1.0) * 127.0) as u8;
        out[n + i] = (128.0 + (r[i] * gain).clamp(-1.0, 1.0) * 127.0) as u8;
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MeterSnapshot {
    /// Szczyt próbkowy wejścia w ostatnim bloku, dBFS.
    pub in_peak_db: f32,
    /// Szczyt próbkowy wyjścia w ostatnim bloku, dBFS.
    pub out_peak_db: f32,
    /// Łączna redukcja wzmocnienia (moduły dynamiki + wyjścia), dB ≥ 0.
    pub gr_db: f32,
    /// Głośność krótkoterminowa (okno 3 s, BS.1770), LUFS.
    pub lufs_short: f32,
    /// Szczyt true-peak wyjścia z ostatnich 3 s, dBTP.
    pub true_peak_db: f32,
    /// Licznik przetworzonych ramek (do wykrycia, że DSP żyje).
    pub frames: u64,
}

impl SharedMeters {
    pub fn new() -> Self {
        let m = Self::default();
        for a in [&m.in_peak_db, &m.out_peak_db, &m.lufs_short, &m.true_peak_db] {
            a.store(f2u(-120.0), Ordering::Relaxed);
        }
        m
    }
    pub(crate) fn publish(&self, s: &MeterSnapshot, module_gr: &[f32; MODULE_COUNT]) {
        self.in_peak_db.store(f2u(s.in_peak_db), Ordering::Relaxed);
        self.out_peak_db.store(f2u(s.out_peak_db), Ordering::Relaxed);
        self.gr_db.store(f2u(s.gr_db), Ordering::Relaxed);
        self.lufs_short.store(f2u(s.lufs_short), Ordering::Relaxed);
        self.true_peak_db.store(f2u(s.true_peak_db), Ordering::Relaxed);
        for (a, v) in self.module_gr_db.iter().zip(module_gr) {
            a.store(f2u(*v), Ordering::Relaxed);
        }
        self.frames.store(s.frames, Ordering::Release);
    }
    pub fn snapshot(&self) -> MeterSnapshot {
        MeterSnapshot {
            in_peak_db: u2f(self.in_peak_db.load(Ordering::Relaxed)),
            out_peak_db: u2f(self.out_peak_db.load(Ordering::Relaxed)),
            gr_db: u2f(self.gr_db.load(Ordering::Relaxed)),
            lufs_short: u2f(self.lufs_short.load(Ordering::Relaxed)),
            true_peak_db: u2f(self.true_peak_db.load(Ordering::Relaxed)),
            frames: self.frames.load(Ordering::Acquire),
        }
    }
    pub fn module_gr_db(&self, idx: usize) -> f32 {
        self.module_gr_db.get(idx).map(|a| u2f(a.load(Ordering::Relaxed))).unwrap_or(0.0)
    }
}

#[cfg(test)]
mod scope_tests {
    use super::*;

    /// Podgląd fali: wyłączony nic nie zapisuje; po włączeniu oddaje ostatnie próbki raz, potem czeka na nowy dźwięk.
    #[test]
    fn scope_returns_fresh_audio_once() {
        let s = Scope::default();
        let x: Vec<f64> = (0..300).map(|i| (i as f64 * 0.1).sin() * 0.5).collect();
        let mut out = [0u8; 2 * SCOPE_LEN];
        s.push(&x, &x);
        s.set_enabled(true);
        s.push(&x, &x);
        assert!(s.read_bytes(&mut out));
        assert!(!s.read_bytes(&mut out), "bez nowego dźwięku drugi odczyt ma być pusty");
        // ostatnia próbka trafiła na koniec, wzmocniona do ok. 0,7 szczytu; środek skali = 128
        let last = out[SCOPE_LEN - 1] as f64 - 128.0;
        assert!((last / 127.0 - x[299] / 0.5 * 0.7).abs() < 0.03, "{last}");
        assert_eq!(out[0], 128);
        s.push(&x, &x);
        assert!(s.read_bytes(&mut out));
    }
}
