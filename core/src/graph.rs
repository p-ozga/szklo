//! Tor przetwarzania: kolejność, on/off bez trzasków, bit-exact bypass (spec v1 §0, §2).
//!
//! Każdy moduł ma stałą latencję L. Gniazdo (`Slot`) zawsze opóźnia sygnał „na sucho” o L, więc wyłączenie
//! modułu nie przesuwa czasu, a wyłączony moduł = czyste opóźnienie (bit-exact).
//! Włączanie: reset z mocą 0 (przezroczysty) → rozgrzewanie (słychać sucho) → przenikanie 10 ms → moc docelowa
//! (wygładzana w module). Wyłączanie: moc → 0 (neutralizacja 100 ms) → przenikanie 10 ms → moduł przestaje liczyć.
//! Neutralizacja trwa 5 stałych czasowych wygładzacza mocy (τ = 20 ms): po 60 ms zostawało jeszcze 5 % mocy
//! (np. Rotacja fazy z a = 0,6 miała wtedy ~7 Hz zamiast 2 Hz, czyli ~110° różnicy faz przy 30 Hz w chwili
//! przenikania); po 100 ms zostaje < 0,7 %.

use crate::catalog::{ModuleId, MODULE_COUNT};
use crate::module::{Ctx, Module, OutputCtx, MAX_BLOCK};

/// Kolejność jednostek w torze (spec v1 §2.1). Szyny STFT obsługują po dwie pozycje UI.
pub const CHAIN_ORDER: [ModuleId; 23] = [
    ModuleId::Declipper,
    ModuleId::Dequantizer,
    ModuleId::Dehummer,
    ModuleId::NoiseReduction, // szyna A: Szum i brum + Naprawa MP3
    ModuleId::NoiseGate,
    ModuleId::Agc,
    ModuleId::AutoEq, // szyna B: Auto EQ + Balans widma
    ModuleId::Width,
    ModuleId::Space,
    ModuleId::DeepBass,
    ModuleId::BassBoost,
    ModuleId::WarmBass,
    ModuleId::RadioBass,
    ModuleId::HarmonicBass,
    ModuleId::Multiband2,
    ModuleId::Compressor,
    ModuleId::Multiband1,
    ModuleId::Punch,
    ModuleId::PhaseRotation,
    ModuleId::PreLimiter,
    ModuleId::ClipperHd,
    ModuleId::SimpleClipper,
    ModuleId::HardLimit,
];

/// Stereofoniczna linia opóźniająca (bufor kołowy).
pub struct StereoDelay {
    l: Vec<f64>,
    r: Vec<f64>,
    pos: usize,
}

impl StereoDelay {
    pub fn new(len: usize) -> Self {
        Self { l: vec![0.0; len], r: vec![0.0; len], pos: 0 }
    }
    pub fn len(&self) -> usize {
        self.l.len()
    }
    pub fn is_empty(&self) -> bool {
        self.l.is_empty()
    }
    /// Zeruje zawartość (bez zmiany długości).
    pub fn clear(&mut self) {
        self.l.fill(0.0);
        self.r.fill(0.0);
        self.pos = 0;
    }
    /// Wpisuje `inl/inr`, zwraca do `outl/outr` próbki sprzed `len` (dla len = 0 – kopia).
    pub fn process(&mut self, inl: &[f64], inr: &[f64], outl: &mut [f64], outr: &mut [f64]) {
        let n = self.l.len();
        if n == 0 {
            outl.copy_from_slice(inl);
            outr.copy_from_slice(inr);
            return;
        }
        for i in 0..inl.len() {
            outl[i] = self.l[self.pos];
            outr[i] = self.r[self.pos];
            self.l[self.pos] = inl[i];
            self.r[self.pos] = inr[i];
            self.pos += 1;
            if self.pos == n {
                self.pos = 0;
            }
        }
    }
    /// Opóźnienie w miejscu.
    pub fn process_inplace(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = self.l.len();
        if n == 0 {
            return;
        }
        for i in 0..l.len() {
            let (a, b) = (self.l[self.pos], self.r[self.pos]);
            self.l[self.pos] = l[i];
            self.r[self.pos] = r[i];
            l[i] = a;
            r[i] = b;
            self.pos += 1;
            if self.pos == n {
                self.pos = 0;
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum State {
    Off,
    WarmUp(usize),
    FadingIn,
    On,
    Neutralizing(usize),
    FadingOut,
}

pub struct Slot {
    pub module: Box<dyn Module>,
    ctx: Ctx,
    on: [bool; MODULE_COUNT],
    amount: [f32; MODULE_COUNT],
    state: State,
    /// postęp przenikania 0..1 (0 = sucho, 1 = mokro)
    t: f64,
    step: f64,
    dry: StereoDelay,
    dl: Vec<f64>,
    dr: Vec<f64>,
    wl: Vec<f64>,
    wr: Vec<f64>,
}

#[inline]
fn sstep(x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

impl Slot {
    pub fn new(mut module: Box<dyn Module>, ctx: Ctx) -> Self {
        for &id in module.ids() {
            module.set_amount(id, 0.0);
            module.set_sub_enabled(id, false);
        }
        module.reset(ctx);
        let lat = module.latency_samples();
        Self {
            module,
            ctx,
            on: [false; MODULE_COUNT],
            amount: [0.0; MODULE_COUNT],
            state: State::Off,
            t: 0.0,
            step: 1.0 / (0.010 * ctx.fs),
            dry: StereoDelay::new(lat),
            dl: vec![0.0; MAX_BLOCK],
            dr: vec![0.0; MAX_BLOCK],
            wl: vec![0.0; MAX_BLOCK],
            wr: vec![0.0; MAX_BLOCK],
        }
    }

    pub fn latency(&self) -> usize {
        self.dry.len()
    }
    pub fn is_active(&self) -> bool {
        self.state != State::Off
    }
    pub fn ids(&self) -> &'static [ModuleId] {
        self.module.ids()
    }
    fn unit_on(&self) -> bool {
        self.module.ids().iter().any(|id| self.on[id.index()])
    }
    fn forward_amounts(&mut self) {
        for &id in self.module.ids() {
            let a = if self.on[id.index()] { self.amount[id.index()] } else { 0.0 };
            self.module.set_amount(id, a);
        }
    }
    fn zero_amounts(&mut self) {
        for &id in self.module.ids() {
            self.module.set_amount(id, 0.0);
        }
    }
    fn start_on(&mut self) {
        self.zero_amounts();
        for &id in self.module.ids() {
            self.module.set_sub_enabled(id, self.on[id.index()]);
        }
        self.module.reset(self.ctx);
        self.t = 0.0;
        self.state = State::WarmUp(self.module.warmup_samples());
    }

    /// Ustawienie stanu bez przejść (render offline od pierwszej próbki).
    pub fn force(&mut self) {
        for &id in self.module.ids() {
            self.module.set_sub_enabled(id, self.on[id.index()]);
        }
        self.forward_amounts();
        self.module.reset(self.ctx);
        if self.unit_on() {
            self.state = State::On;
            self.t = 1.0;
        } else {
            self.state = State::Off;
            self.t = 0.0;
        }
    }

    /// Awaryjne przywrócenie po wykryciu NaN/Inf w torze: czyści linię „suchą” i stan modułu.
    pub fn recover(&mut self) {
        self.dry.clear();
        self.force();
    }

    pub fn set_output_ctx(&mut self, o: OutputCtx) {
        self.module.set_output_ctx(o);
    }

    pub fn set_amount(&mut self, id: ModuleId, a: f32) {
        self.amount[id.index()] = a;
        if self.state == State::On && self.on[id.index()] {
            self.module.set_amount(id, a);
        }
    }

    pub fn set_on(&mut self, id: ModuleId, on: bool) {
        if self.on[id.index()] == on {
            return;
        }
        let was = self.unit_on();
        self.on[id.index()] = on;
        let now = self.unit_on();
        if self.module.ids().len() > 1 && self.state != State::Off {
            // moduł złożony: pozycja wewnątrz włącza/wyłącza się sama (z neutralizacją)
            self.module.set_sub_enabled(id, on);
            if self.state == State::On {
                self.forward_amounts();
            }
        }
        if was == now {
            return;
        }
        match (now, self.state) {
            (true, State::Off) => self.start_on(),
            (true, State::Neutralizing(_)) => {
                self.forward_amounts();
                self.state = State::On;
            }
            (true, State::FadingOut) => self.state = State::FadingIn,
            (false, State::WarmUp(_)) => self.state = State::Off,
            (false, State::FadingIn) => self.state = State::FadingOut,
            (false, State::On) => {
                self.zero_amounts();
                self.state = State::Neutralizing(self.ctx.ms(100.0));
            }
            _ => {}
        }
    }

    pub fn process(&mut self, l: &mut [f64], r: &mut [f64]) {
        let n = l.len();
        debug_assert!(n <= MAX_BLOCK);
        {
            let (dl, dr) = (&mut self.dl[..n], &mut self.dr[..n]);
            self.dry.process(l, r, dl, dr);
        }
        match self.state {
            State::Off => {
                l.copy_from_slice(&self.dl[..n]);
                r.copy_from_slice(&self.dr[..n]);
                return;
            }
            State::On => {
                self.module.process(l, r);
                return;
            }
            State::Neutralizing(rem) => {
                self.module.process(l, r);
                let rem = rem.saturating_sub(n);
                self.state = if rem == 0 { State::FadingOut } else { State::Neutralizing(rem) };
                return;
            }
            _ => {}
        }
        let (wl, wr) = (&mut self.wl[..n], &mut self.wr[..n]);
        wl.copy_from_slice(l);
        wr.copy_from_slice(r);
        self.module.process(wl, wr);
        if let State::WarmUp(rem) = self.state {
            let rem = rem.saturating_sub(n);
            self.state = if rem == 0 { State::FadingIn } else { State::WarmUp(rem) };
            l.copy_from_slice(&self.dl[..n]);
            r.copy_from_slice(&self.dr[..n]);
            return;
        }
        let dir = if self.state == State::FadingIn { self.step } else { -self.step };
        for i in 0..n {
            self.t = (self.t + dir).clamp(0.0, 1.0);
            let m = sstep(self.t);
            l[i] = self.dl[i] + m * (self.wl[i] - self.dl[i]);
            r[i] = self.dr[i] + m * (self.wr[i] - self.dr[i]);
        }
        if self.state == State::FadingIn && self.t >= 1.0 {
            self.state = State::On;
            self.forward_amounts();
        } else if self.state == State::FadingOut && self.t <= 0.0 {
            self.state = State::Off;
            for &id in self.module.ids() {
                self.module.set_sub_enabled(id, false);
            }
        }
    }
}
