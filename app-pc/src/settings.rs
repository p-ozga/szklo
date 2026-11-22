//! Ustawienia użytkownika: %APPDATA%\Szklo\ustawienia.json (zapisywane z opóźnieniem po zmianie).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use szklo_core::{SharedParams, MODULE_COUNT, PRESETS};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    pub on: Vec<bool>,
    pub amount: Vec<f32>,
    pub preset: String,
    pub enabled: bool,
    pub ceiling_dbtp: f32,
    /// "apo" = wtyczka w Equalizer APO, "app" = silnik w aplikacji.
    pub mode: String,
    pub input: String,
    pub output: String,
    /// „Spotify w tle”: chowaj okno aplikacji Spotify (gra dalej, sterowanie z karty odtwarzacza).
    pub hide_spotify: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let p = &PRESETS[0];
        Self {
            on: p.modules.iter().map(|m| m.0).collect(),
            amount: p.modules.iter().map(|m| m.1).collect(),
            preset: p.key.to_string(),
            enabled: true,
            ceiling_dbtp: -1.0,
            mode: "apo".into(),
            input: String::new(),
            output: String::new(),
            hide_spotify: false,
        }
    }
}

pub fn dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Szklo")
}

impl Settings {
    pub fn load() -> Self {
        let mut s: Settings = std::fs::read_to_string(dir().join("ustawienia.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        let d = Settings::default();
        if s.on.len() != MODULE_COUNT || s.amount.len() != MODULE_COUNT {
            s.on = d.on;
            s.amount = d.amount;
        }
        for a in s.amount.iter_mut() {
            *a = if a.is_finite() { a.clamp(0.0, 1.0) } else { 0.0 };
        }
        if s.mode != "apo" && s.mode != "app" {
            s.mode = d.mode;
        }
        s
    }

    pub fn save(&self) {
        let d = dir();
        let _ = std::fs::create_dir_all(&d);
        if let Ok(t) = serde_json::to_string_pretty(self) {
            // zapis atomowy: plik tymczasowy + zamiana
            let tmp = d.join("ustawienia.json.tmp");
            if std::fs::write(&tmp, t).is_ok() {
                let _ = std::fs::rename(&tmp, d.join("ustawienia.json"));
            }
        }
    }

    pub fn apply_to(&self, p: &SharedParams) {
        for i in 0..MODULE_COUNT {
            p.set_module(i, self.on[i], self.amount[i]);
        }
        p.set_enabled(self.enabled);
        p.set_ceiling_dbtp(self.ceiling_dbtp);
    }

    pub fn capture_from(&mut self, p: &SharedParams) {
        for i in 0..MODULE_COUNT {
            let (on, a) = p.module(i);
            self.on[i] = on;
            self.amount[i] = a;
        }
        self.enabled = p.enabled();
        self.ceiling_dbtp = p.ceiling_dbtp();
    }
}
