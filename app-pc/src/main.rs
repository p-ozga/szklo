//! Szkło – aplikacja PC.
//!
//! UI: jeden plik HTML (ui/index.html) w WebView2 – odwzorowanie 1:1 makiety „PC 1 · Panel modułów”,
//! z odwracaniem okna na stronę tylną (jak w VVS-Kopia). Okno bez ramki, zaokrąglenie DWM, ikona w zasobniku.
//! Most: JS → Rust `window.ipc.postMessage(JSON)`, Rust → JS `evaluate_script("szklo.…")`.
#![windows_subsystem = "windows"]

mod apo;
mod apo_check;
mod audio;
mod dziennik;
mod instalacja;
mod odtwarzacz;
mod media_win;
mod milkdrop;
mod monitor;
mod spotify_okno;
mod spotify_pc;
mod presety;
mod settings;

use serde_json::{json, Value};
use std::borrow::Cow;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use szklo_core::{SharedMeters, SharedParams, MODULE_COUNT, PRESETS};
use tao::dpi::LogicalSize;
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::{Icon, WindowBuilder};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, TrayIconBuilder, TrayIconEvent};
use wry::http::{header::CONTENT_TYPE, Request, Response};
use wry::{WebContext, WebViewBuilder};
#[cfg(windows)]
use wry::WebViewBuilderExtWindows;

const INDEX: &str = include_str!("../ui/index.html");
const FONT_INTER: &[u8] = include_bytes!("../ui/fonts/Inter.ttf");
const FONT_SG: &[u8] = include_bytes!("../ui/fonts/SpaceGrotesk.ttf");
const LICENCJE: &str = include_str!("../ui/licencje.txt");
const OFL_INTER: &str = include_str!("../ui/fonts/OFL-Inter.txt");
const OFL_SG: &str = include_str!("../ui/fonts/OFL-SpaceGrotesk.txt");

#[derive(Debug)]
enum Zd {
    Js(String),
    Okno(&'static str),
    Tryb,
    Tick,
    /// Strona UI załadowana od nowa – wyślij ponownie wszystko, co pamiętamy jako „już wysłane”.
    Odswiez,
}

struct Wspolne {
    proxy: EventLoopProxy<Zd>,
    params: Arc<SharedParams>,
    ust: Mutex<settings::Settings>,
    zmiana: Mutex<Option<Instant>>,
    na_tyle: std::sync::atomic::AtomicBool,
    /// Presety użytkownika (presety.json).
    presety: Mutex<Vec<presety::Preset>>,
    /// Wbudowany odtwarzacz (karta na odwrocie).
    odtw: odtwarzacz::Odtwarzacz,
    /// Karta odtwarzacza powiększona (UI wysyła wtedy widmo ~30 razy na sekundę).
    odtw_duzy: std::sync::atomic::AtomicBool,
    /// HWND okna i skala DPI (dla osadzenia MilkDrop w obszarze karty).
    okno_hwnd: std::sync::atomic::AtomicIsize,
    okno_skala: Mutex<f64>,
}

fn md_prostokat(w: &Wspolne, m: &Value) -> milkdrop::Prostokat {
    let s = *w.okno_skala.lock().unwrap();
    let f = |k: &str| (m[k].as_f64().unwrap_or(0.0) * s).round() as i32;
    milkdrop::Prostokat { x: f("x"), y: f("y"), w: f("w").max(1), h: f("h").max(1) }
}

fn md_start(w: &Arc<Wspolne>, m: &Value) {
    let r = md_prostokat(w, m);
    let w = w.clone();
    let wskaz = m["wskaz"].as_bool().unwrap_or(false);
    std::thread::spawn(move || {
        if milkdrop::sciezka().is_none() {
            if let Some(p) = milkdrop::znajdz() {
                log::info!("MilkDrop: znaleziono {}", p.display());
                milkdrop::ustaw_sciezke(&p);
            }
        }
        if milkdrop::sciezka().is_none() {
            if !wskaz {
                // nie ma programu na dysku – karta wyjaśni, skąd go wziąć (zamiast samego okna „Otwórz”)
                log::info!("MilkDrop: brak MilkDrop3.exe – pokazuję instrukcję");
                js(&w.proxy, "szklo.milkBrak()".into());
                return;
            }
            let wybor = rfd::FileDialog::new()
                .set_title("Wskaż MilkDrop3.exe (pobierz z github.com/milkdrop2077/MilkDrop3)")
                .add_filter("Program", &["exe"])
                .pick_file();
            match wybor {
                Some(p) => milkdrop::ustaw_sciezke(&p),
                None => {
                    js(&w.proxy, "szklo.milkStop()".into());
                    return;
                }
            }
        }
        let rodzic = w.okno_hwnd.load(Ordering::Relaxed);
        if let Err(e) = milkdrop::uruchom(rodzic, r) {
            log::error!("MilkDrop: {e}");
            js(&w.proxy, format!("szklo.milkStop({})", json!(e)));
        }
    });
}

/// Stan odtwarzacza dla UI. Lista (bywa długa) idzie tylko, gdy się zmieniła – pauza, przewijanie i zmiana
/// utworu wysyłają mały obiekt bez listy, a UI nie przebudowuje wtedy tysięcy wierszy.
fn odtw_json(w: &Wspolne, z_lista: bool) -> Value {
    if z_lista {
        let s = w.odtw.stan.lock().map(|s| s.clone()).unwrap_or_default();
        return serde_json::to_value(&s).unwrap_or(Value::Null);
    }
    let Ok(s) = w.odtw.stan.lock() else { return Value::Null };
    json!({ "biezacy": s.biezacy, "gra": s.gra, "pozycja": s.pozycja, "czas": s.czas, "losowo": s.losowo,
            "powtarzanie": s.powtarzanie, "blad": s.blad, "wersja": s.wersja })
}

/// Okno schowane/zminimalizowane: zegar UI i odpytywanie multimediów zwalniają.
static W_TLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Okno wyboru plików/folderu (w osobnym wątku – nie blokuje UI) → dopisanie do playlisty.
fn odtw_dodaj(w: &Arc<Wspolne>, folder: bool) {
    let w = w.clone();
    std::thread::spawn(move || {
        let d = rfd::FileDialog::new().set_title(if folder { "Dodaj folder z muzyką" } else { "Dodaj pliki muzyczne" });
        let wybor: Vec<std::path::PathBuf> = if folder {
            d.pick_folder().into_iter().collect()
        } else {
            d.add_filter("Muzyka", odtwarzacz::ROZSZERZENIA).pick_files().unwrap_or_default()
        };
        // Czytanie tagów tutaj (wątek okna wyboru), paczkami: odtwarzanie nie staje, a lista rośnie na bieżąco.
        let pliki = odtwarzacz::rozwin(wybor);
        for paczka in pliki.chunks(100) {
            w.odtw.rozkaz(odtwarzacz::Rozkaz::Dodaj(paczka.iter().map(|p| odtwarzacz::opisz(p)).collect()));
        }
    });
}

fn presets_json(w: &Wspolne) -> Value {
    // kolejność chipów: Codzienny, Noc, Stare nagrania, Głośno w tramwaju, potem presety użytkownika
    let order = ["codzienny", "noc", "stare", "tramwaj"];
    let mut v: Vec<Value> = order
        .iter()
        .filter_map(|k| szklo_core::presets::by_key(k))
        .map(|p| {
            json!({ "key": p.key, "name": p.name_pl,
                    "mods": p.modules.iter().map(|m| json!([m.0, m.1])).collect::<Vec<_>>() })
        })
        .collect();
    for p in w.presety.lock().unwrap().iter() {
        v.push(json!({ "key": p.key, "name": p.name, "user": true,
                       "mods": p.on.iter().zip(&p.amount).map(|(o, a)| json!([o, a])).collect::<Vec<_>>() }));
    }
    Value::Array(v)
}

/// Odpowiedź na akcję presetu: nowa lista + wybrany preset + komunikat na karcie.
fn presety_odpowiedz(w: &Wspolne, wynik: Result<String, String>) {
    let preset = w.ust.lock().unwrap().preset.clone();
    js(&w.proxy, format!("szklo.stan({})", json!({ "presets": presets_json(w), "preset": preset })));
    let (ok, msg) = match wynik {
        Ok(m) => (true, m),
        Err(m) => (false, m),
    };
    js(&w.proxy, format!("szklo.presetKomunikat({},{})", json!(msg), ok));
}

fn stan_json(w: &Wspolne) -> Value {
    let presets = presets_json(w);
    let u = w.ust.lock().unwrap();
    json!({ "presets": presets, "on": u.on, "a": u.amount, "preset": u.preset,
            "enabled": u.enabled, "ceiling": u.ceiling_dbtp })
}

fn urzadzenia_json(w: &Wspolne) -> Value {
    let d = audio::list_devices();
    let u = w.ust.lock().unwrap();
    let output = if u.output.is_empty() { d.default_output.clone() } else { u.output.clone() };
    json!({ "mode": u.mode, "inputs": d.inputs, "outputs": d.outputs, "input": u.input, "output": output })
}

fn js(p: &EventLoopProxy<Zd>, s: String) {
    let _ = p.send_event(Zd::Js(s));
}

fn oznacz_zmiane(w: &Wspolne) {
    *w.zmiana.lock().unwrap() = Some(Instant::now());
}

fn obsluz(w: &Arc<Wspolne>, body: &str) {
    let Ok(m) = serde_json::from_str::<Value>(body) else { return };
    let t = m["t"].as_str().unwrap_or("");
    match t {
        "ready" => {
            let _ = w.proxy.send_event(Zd::Odswiez);
            js(&w.proxy, format!("szklo.stan({})", stan_json(w)));
            js(&w.proxy, format!("szklo.urzadzenia({})", urzadzenia_json(w)));
            let _ = w.proxy.send_event(Zd::Tryb);
            js(&w.proxy, format!("szklo.odtw({})", odtw_json(w, true)));
            js(&w.proxy, format!("szklo.spotify({})", spotify_pc::stan_json()));
            js(&w.proxy, format!("szklo.spUkryte({})", spotify_okno::wlaczone()));
            // pierwsze uruchomienie z instalatora z zaznaczonym „zainstaluj Equalizer APO”
            static PO_INSTALACJI: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
            if std::env::args().any(|a| a == "--instaluj-apo") && !instalacja::apo_zainstalowany() && !PO_INSTALACJI.swap(true, Ordering::Relaxed) {
                js(&w.proxy, "setTimeout(()=>{ if(!naTyle) szklo.obroc(); },400)".into());
                apo_instaluj(w);
            }
            // test wizualny strony tylnej: Szklo.exe --tyl
            if std::env::args().any(|a| a == "--tyl") {
                js(&w.proxy, "setTimeout(()=>szklo.obroc(),300)".into());
            }
        }
        "mod" => {
            let (Some(i), Some(on), Some(a)) = (m["i"].as_u64(), m["on"].as_bool(), m["a"].as_f64()) else { return };
            let i = i as usize;
            if i < MODULE_COUNT {
                w.params.set_module(i, on, a as f32);
                let mut u = w.ust.lock().unwrap();
                u.on[i] = on;
                u.amount[i] = a as f32;
                oznacz_zmiane(w);
            }
        }
        "all" => {
            let (Some(on), Some(a)) = (m["on"].as_array(), m["a"].as_array()) else { return };
            if on.len() != MODULE_COUNT || a.len() != MODULE_COUNT {
                return;
            }
            let mut u = w.ust.lock().unwrap();
            for i in 0..MODULE_COUNT {
                let o = on[i].as_bool().unwrap_or(false);
                let v = a[i].as_f64().unwrap_or(0.0) as f32;
                w.params.set_module(i, o, v);
                u.on[i] = o;
                u.amount[i] = v;
            }
            if let Some(k) = m["preset"].as_str() {
                u.preset = k.to_string();
            }
            oznacz_zmiane(w);
        }
        "enabled" => {
            let on = m["on"].as_bool().unwrap_or(true);
            w.params.set_enabled(on);
            w.ust.lock().unwrap().enabled = on;
            oznacz_zmiane(w);
        }
        "compare" => w.params.set_compare(m["on"].as_bool().unwrap_or(false)),
        "mode" => {
            if let Some(md) = m["mode"].as_str().filter(|x| *x == "apo" || *x == "app") {
                w.ust.lock().unwrap().mode = md.to_string();
                oznacz_zmiane(w);
                let _ = w.proxy.send_event(Zd::Tryb);
            }
        }
        "devices" => {
            {
                let mut u = w.ust.lock().unwrap();
                u.input = m["input"].as_str().unwrap_or("").to_string();
                u.output = m["output"].as_str().unwrap_or("").to_string();
            }
            oznacz_zmiane(w);
            let _ = w.proxy.send_event(Zd::Tryb);
        }
        "preset_save_as" => {
            let nazwa = m["name"].as_str().unwrap_or("");
            let wynik = {
                let mut u = w.ust.lock().unwrap();
                u.capture_from(&w.params);
                let r = presety::zapisz_jako(&mut w.presety.lock().unwrap(), nazwa, &u.on, &u.amount);
                r.map(|(key, msg)| {
                    u.preset = key;
                    msg
                })
            };
            oznacz_zmiane(w);
            presety_odpowiedz(w, wynik);
        }
        "preset_save" => {
            let wynik = {
                let mut u = w.ust.lock().unwrap();
                u.capture_from(&w.params);
                let key = u.preset.clone();
                presety::zapisz(&mut w.presety.lock().unwrap(), &key, &u.on, &u.amount)
            };
            presety_odpowiedz(w, wynik);
        }
        "preset_delete" => {
            let key = m["key"].as_str().unwrap_or("").to_string();
            let wynik = presety::usun(&mut w.presety.lock().unwrap(), &key);
            if wynik.is_ok() {
                let mut u = w.ust.lock().unwrap();
                if u.preset == key {
                    u.preset = PRESETS[0].key.to_string(); // ustawienia zostają, preset oznaczony jako zmieniony
                }
                drop(u);
                oznacz_zmiane(w);
            }
            presety_odpowiedz(w, wynik);
        }
        // ── wbudowany odtwarzacz ──
        "p_dodaj" => odtw_dodaj(w, false),
        "p_folder" => odtw_dodaj(w, true),
        "p_graj" => w.odtw.rozkaz(odtwarzacz::Rozkaz::Graj(m["i"].as_u64().map(|i| i as usize))),
        "p_pp" => w.odtw.rozkaz(odtwarzacz::Rozkaz::PlayPauza),
        "p_nast" => w.odtw.rozkaz(odtwarzacz::Rozkaz::Nastepny),
        "p_poprz" => w.odtw.rozkaz(odtwarzacz::Rozkaz::Poprzedni),
        "p_szukaj" => w.odtw.rozkaz(odtwarzacz::Rozkaz::Szukaj(m["s"].as_f64().unwrap_or(0.0))),
        "p_losowo" => w.odtw.rozkaz(odtwarzacz::Rozkaz::Losowo(m["b"].as_bool().unwrap_or(false))),
        "p_powt" => {
            let p = match m["v"].as_str().unwrap_or("") {
                "wszystko" => odtwarzacz::Powtarzanie::Wszystko,
                "jeden" => odtwarzacz::Powtarzanie::Jeden,
                _ => odtwarzacz::Powtarzanie::Wyl,
            };
            w.odtw.rozkaz(odtwarzacz::Rozkaz::Powtarzanie(p));
        }
        "p_usun" => {
            if let Some(i) = m["i"].as_u64() {
                w.odtw.rozkaz(odtwarzacz::Rozkaz::Usun(i as usize));
            }
        }
        "p_wyczysc" => w.odtw.rozkaz(odtwarzacz::Rozkaz::Wyczysc),
        "apo_dodaj" => {
            log::info!("Equalizer APO: otwieram wybór urządzeń (DeviceSelector)");
            if let Err(e) = apo_check::otworz_device_selector() {
                log::error!("Equalizer APO: {e}");
                js(&w.proxy, format!("szklo.status({})", json!(e)));
            }
        }
        "apo_instaluj" => apo_instaluj(w),
        "apo_podlacz" => {
            let w2 = w.clone();
            std::thread::spawn(move || {
                log::info!("Equalizer APO: podłączam wtyczkę Szkła (na prośbę użytkownika)");
                match instalacja::podlacz_wtyczke() {
                    Ok(()) => js(&w2.proxy, format!("szklo.apoPostep({},false)", json!("Szkło jest podłączone do Equalizer APO."))),
                    Err(e) => {
                        log::error!("Equalizer APO: {e}");
                        js(&w2.proxy, format!("szklo.apoPostep({},true)", json!(e)));
                    }
                }
            });
        }
        "apo_restart" => {
            log::info!("Equalizer APO: restart usługi dźwięku Windows (na prośbę użytkownika)");
            if let Err(e) = apo_check::restart_dzwieku() {
                log::error!("Equalizer APO: {e}");
                js(&w.proxy, format!("szklo.status({})", json!(e)));
            }
        }
        // ── aplikacja Spotify / inny odtwarzacz przez sterowanie multimediami Windows ──
        "z_pp" => media_win::get().rozkaz(media_win::Rozkaz::PlayPauza),
        "z_nast" => media_win::get().rozkaz(media_win::Rozkaz::Nastepny),
        "z_poprz" => media_win::get().rozkaz(media_win::Rozkaz::Poprzedni),
        "z_szukaj" => media_win::get().rozkaz(media_win::Rozkaz::Szukaj(m["s"].as_f64().unwrap_or(0.0))),
        "z_losowo" => media_win::get().rozkaz(media_win::Rozkaz::Losowo(m["b"].as_bool().unwrap_or(false))),
        "z_powt" => media_win::get().rozkaz(media_win::Rozkaz::Powtarzanie(m["v"].as_str().unwrap_or("wyl").to_string())),
        "z_otworz" => {
            // utwór/playlista w aplikacji Spotify (link spotify:…) – aplikacja sama zaczyna grać
            if let Some(uri) = m["uri"].as_str().filter(|u| u.starts_with("spotify:")) {
                spotify_okno::uruchom_aplikacje(uri);
            }
        }
        "z_start" => {
            // uruchom aplikację Spotify, jeśli nie działa (przy „Spotify w tle” – od razu schowaną)
            if !spotify_okno::aplikacja_dziala() {
                log::info!("Spotify: uruchamiam aplikację");
                spotify_okno::uruchom_aplikacje("spotify:");
            }
        }
        "z_ukryj" => {
            let b = m["b"].as_bool().unwrap_or(false);
            log::info!("Spotify w tle: {}", if b { "włączone" } else { "wyłączone" });
            w.ust.lock().unwrap().hide_spotify = b;
            oznacz_zmiane(w);
            spotify_okno::ustaw(b);
            js(&w.proxy, format!("szklo.spUkryte({b})"));
        }
        // ── Spotify ──
        "sp_zaloguj" => spotify_pc::get().zaloguj(),
        "sp_wyloguj" => spotify_pc::get().wyloguj(),
        "sp_playlista" => {
            if let Some(uri) = m["uri"].as_str() {
                spotify_pc::get().utwory(uri);
            }
        }
        "sp_graj" => {
            // cała otwarta playlista do kolejki odtwarzacza, start od wskazanego utworu
            let i = m["i"].as_u64().unwrap_or(0) as usize;
            let lista = spotify_pc::get().stan.lock().map(|s| s.utwory.clone()).unwrap_or_default();
            if !lista.is_empty() {
                w.odtw.rozkaz(odtwarzacz::Rozkaz::Zastap(lista, i));
            }
        }
        // ── MilkDrop 3 w karcie ──
        "md_start" => md_start(w, &m),
        "md_rect" => milkdrop::przesun(md_prostokat(w, &m)),
        "md_stop" => milkdrop::zamknij(),
        // wizualizacja na pełnym ekranie (wyjście: Esc albo zmniejszenie karty)
        "md_pelny" => {
            let _ = w.proxy.send_event(Zd::Okno(if m["b"].as_bool().unwrap_or(false) { "pelny_wl" } else { "pelny_wyl" }));
        }
        "md_pobierz" => milkdrop::otworz_strone(),
        "md_instaluj" => {
            let w2 = w.clone();
            std::thread::spawn(move || {
                let p = w2.proxy.clone();
                let wynik = milkdrop::pobierz_i_zainstaluj(&|t| js(&p, format!("szklo.milkPostep({})", json!(t))));
                match wynik {
                    Ok(_) => js(&w2.proxy, "szklo.milkGotowy()".into()),
                    Err(e) => {
                        log::error!("MilkDrop: {e}");
                        js(&w2.proxy, format!("szklo.milkPostep({},true)", json!(e)));
                    }
                }
            });
        }
        "md_zmien" => {
            // wskaż inny MilkDrop3.exe
            let _ = std::fs::remove_file(settings::dir().join("milkdrop.txt"));
        }
        "p_widok" => w.odtw_duzy.store(m["duzy"].as_bool().unwrap_or(false), Ordering::Relaxed),
        "side" => w.na_tyle.store(m["back"].as_bool().unwrap_or(false), Ordering::Relaxed),
        "drag" => {
            let _ = w.proxy.send_event(Zd::Okno("drag"));
        }
        "maximize" => {
            let _ = w.proxy.send_event(Zd::Okno("max"));
        }
        "hide" => {
            // zamknięcie okna = od razu zapis ustawień (program zostaje w zasobniku, ale może zostać zabity)
            {
                let mut u = w.ust.lock().unwrap();
                u.capture_from(&w.params);
                u.save();
            }
            let _ = w.proxy.send_event(Zd::Okno("hide"));
        }
        "min" => {
            let _ = w.proxy.send_event(Zd::Okno("min"));
        }
        _ => {}
    }
}

fn protokol(req: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let path = req.uri().path();
    let (body, mime): (Cow<'static, [u8]>, &str) = match path {
        "/" | "/index.html" => (Cow::Borrowed(INDEX.as_bytes()), "text/html; charset=utf-8"),
        "/fonts/Inter.ttf" => (Cow::Borrowed(FONT_INTER), "font/ttf"),
        "/fonts/SpaceGrotesk.ttf" => (Cow::Borrowed(FONT_SG), "font/ttf"),
        // cudze elementy i ich licencje (karta „O programie i licencje”)
        "/licencje.txt" => (Cow::Borrowed(LICENCJE.as_bytes()), "text/plain; charset=utf-8"),
        "/dziennik.txt" => (Cow::Owned(dziennik::ogon().into_bytes()), "text/plain; charset=utf-8"),
        "/fonts/OFL-Inter.txt" => (Cow::Borrowed(OFL_INTER.as_bytes()), "text/plain; charset=utf-8"),
        "/fonts/OFL-SpaceGrotesk.txt" => (Cow::Borrowed(OFL_SG.as_bytes()), "text/plain; charset=utf-8"),
        _ => {
            return Response::builder().status(404).body(Cow::Borrowed(&b"404"[..])).unwrap();
        }
    };
    Response::builder().header(CONTENT_TYPE, mime).body(body).unwrap()
}

/// Ikona: pięć słupków jak w logo (#25E2F7 na #061214), generowana w kodzie.
fn ikona_rgba(n: u32) -> Vec<u8> {
    let mut px = vec![0u8; (n * n * 4) as usize];
    let bars = [(3.0, 12.0, 2.0), (7.0, 8.0, 16.0), (11.0, 5.0, 19.0), (15.0, 8.0, 16.0), (19.0, 11.0, 13.0)];
    let s = n as f32 / 24.0;
    for y in 0..n {
        for x in 0..n {
            let (fx, fy) = ((x as f32 + 0.5) / s, (y as f32 + 0.5) / s);
            // tło: zaokrąglony kwadrat
            let (dx, dy) = ((fx - 12.0).abs() - 7.0, (fy - 12.0).abs() - 7.0);
            let d = (dx.max(0.0).hypot(dy.max(0.0))) + dx.max(dy).min(0.0) - 5.0;
            let i = ((y * n + x) * 4) as usize;
            if d <= 0.0 {
                px[i..i + 4].copy_from_slice(&[0x06, 0x12, 0x14, 255]);
            }
            for (bx, y0, y1) in bars {
                let (y0, y1) = if y1 < y0 { (y1, y0) } else if (bx - 3.0f32).abs() < 0.1 { (11.0, 13.0) } else { (y0, y1) };
                if (fx - bx).abs() <= 0.95 && fy >= y0 - 0.9 && fy <= y1 + 0.9 {
                    px[i..i + 4].copy_from_slice(&[0x25, 0xE2, 0xF7, 255]);
                }
            }
        }
    }
    px
}

/// Skala rasteryzacji WebView2 = skala DPI okna (bez mnożnika „powiększenie tekstu” Windows).
#[cfg(windows)]
fn przypnij_skale(webview: &wry::WebView, skala: f64) {
    use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Controller3;
    use windows_core::Interface;
    use wry::WebViewExtWindows;
    if let Ok(k3) = webview.controller().cast::<ICoreWebView2Controller3>() {
        // SAFETY: wywołania COM na żywym kontrolerze WebView2 naszego okna.
        unsafe {
            let _ = k3.SetShouldDetectMonitorScaleChanges(false);
            let _ = k3.SetRasterizationScale(skala);
        }
    }
}
#[cfg(not(windows))]
fn przypnij_skale(_: &wry::WebView, _: f64) {}

/// Okno jest przezroczyste, a jego kształt (zaokrąglony panel 22 px) rysuje CSS – systemowe zaokrąglanie DWM
/// (8 px) wycinałoby rogi i dorysowywało obramowanie, więc je wyłączamy.
#[cfg(windows)]
fn bez_rogow_dwm(okno: &tao::window::Window) {
    use tao::platform::windows::WindowExtWindows;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND};
    let hwnd = HWND(okno.hwnd() as *mut core::ffi::c_void);
    let pref = DWMWCP_DONOTROUND;
    // DWMWA_COLOR_NONE – bez systemowej ramki Windows 11 (jasna linia na krawędziach i w rogach)
    let bez_ramki: u32 = 0xFFFF_FFFE;
    // SAFETY: poprawny HWND naszego okna, wskaźniki na lokalne zmienne o podanych rozmiarach.
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&pref) as u32,
        );
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &bez_ramki as *const _ as *const core::ffi::c_void,
            std::mem::size_of_val(&bez_ramki) as u32,
        );
    }
}
#[cfg(not(windows))]
fn bez_rogow_dwm(_: &tao::window::Window) {}

/// Przy akrylu tło okna jest prostokątem – zaokrąglenie robi DWM (gładkie, ~8 px), CSS dopasowuje promień panelu.
#[cfg(windows)]
fn zaokraglij_dwm(okno: &tao::window::Window) {
    use tao::platform::windows::WindowExtWindows;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
    let hwnd = HWND(okno.hwnd() as *mut core::ffi::c_void);
    let pref = DWMWCP_ROUND;
    // SAFETY: poprawny HWND naszego okna, wskaźnik na lokalną zmienną o podanym rozmiarze.
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const _ as *const core::ffi::c_void, std::mem::size_of_val(&pref) as u32);
    }
}
#[cfg(not(windows))]
fn zaokraglij_dwm(_: &tao::window::Window) {}

/// Jedna kopia programu: druga (np. ze skrótu, gdy pierwsza siedzi w zasobniku) pokazuje działające okno i kończy się.
/// Dwie kopie naraz walczyły o jeden kanał wtyczki i o wspólny profil WebView2 (białe tło w rogach).
#[cfg(windows)]
fn juz_dziala() -> bool {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW};
    // SAFETY: wywołania Win32 ze stałymi łańcuchami; uchwyt muteksu celowo nie jest zamykany (żyje do końca procesu).
    unsafe {
        let m = CreateMutexW(None, true, w!("Local\\Szklo.Aplikacja"));
        if m.is_ok() && GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(h) = FindWindowW(None, w!("Szkło")) {
                let _ = ShowWindow(h, SW_SHOW);
                let _ = ShowWindow(h, SW_RESTORE);
                let _ = SetForegroundWindow(h);
            }
            return true;
        }
    }
    false
}
#[cfg(not(windows))]
fn juz_dziala() -> bool {
    false
}

fn main() {
    // tryby instalatora (--instaluj-wtyczke / --odinstaluj-wtyczke): bez okna, także gdy Szkło już działa
    if std::env::args().any(|a| a.starts_with("--instaluj-wtyczke") || a.starts_with("--odinstaluj-wtyczke")) {
        dziennik::init();
        std::process::exit(instalacja::obsluz_argumenty().unwrap_or(2));
    }
    if juz_dziala() {
        return;
    }
    dziennik::init();
    // bez WebView2 (Microsoft) okno się nie otworzy – zaproponuj instalację
    if !instalacja::zapewnij_webview2() {
        return;
    }
    spotify_pc::start_jesli_zalogowany();
    let el = EventLoopBuilder::<Zd>::with_user_event().build();
    let proxy = el.create_proxy();

    let params = Arc::new(SharedParams::default());
    let local_meters = Arc::new(SharedMeters::new());
    let ust = settings::Settings::load();
    if ust.hide_spotify {
        spotify_okno::ustaw(true);
    }
    ust.apply_to(&params);

    let w = Arc::new(Wspolne {
        proxy: proxy.clone(),
        params: params.clone(),
        ust: Mutex::new(ust),
        zmiana: Mutex::new(None),
        na_tyle: Default::default(),
        presety: Mutex::new(presety::wczytaj()),
        odtw: odtwarzacz::uruchom(),
        odtw_duzy: Default::default(),
        okno_hwnd: Default::default(),
        okno_skala: Mutex::new(1.0),
    });
    let link = apo::spawn(params.clone());

    let ikona = Icon::from_rgba(ikona_rgba(32), 32, 32).ok();
    let okno = WindowBuilder::new()
        .with_title("Szkło")
        .with_decorations(false)
        // okno = sam zaokrąglony panel makiety (1200×740); tło przezroczyste, rogi rysuje CSS
        .with_transparent(true)
        .with_inner_size(LogicalSize::new(1200.0, 740.0))
        .with_min_inner_size(LogicalSize::new(1200.0, 740.0))
        .with_window_icon(ikona)
        .with_visible(false);
    // Cień okna bez ramki (tao: DwmExtendFrameIntoClientArea) rysuje linię u góry i psuje przezroczystość WebView2
    // – w rogach poza zaokrągleniem było białe tło. Okno przezroczyste: bez cienia systemowego.
    #[cfg(windows)]
    let okno = {
        use tao::platform::windows::WindowBuilderExtWindows;
        okno.with_undecorated_shadow(false)
    };
    let okno = okno.build(&el).expect("okno");
    bez_rogow_dwm(&okno);
    // Mrożone szkło: akryl Windows pod oknem (rozmywa to, co pod spodem). Tło panelu ma ~15% przezroczystości (CSS),
    // karty są nieprzezroczyste. Gdy akryl niedostępny (starszy Windows), panel zostaje jak dotąd.
    #[cfg(windows)]
    let akryl = window_vibrancy::apply_acrylic(&okno, Some((6, 18, 20, 90))).is_ok();
    #[cfg(not(windows))]
    let akryl = false;
    if akryl {
        zaokraglij_dwm(&okno);
    }

    let mut kontekst = WebContext::new(std::env::var_os("LOCALAPPDATA").map(|l| std::path::PathBuf::from(l).join("Szklo").join("WebView2")));
    let w_ipc = w.clone();
    let devtools = std::env::var_os("SZKLO_DEVTOOLS").is_some();
    let budowniczy = WebViewBuilder::new_with_web_context(&mut kontekst)
        .with_custom_protocol("szklo".into(), |_id, req| protokol(req))
        .with_url(if akryl { "http://szklo.localhost/index.html?akryl=1" } else { "http://szklo.localhost/index.html" })
        .with_transparent(true)
        .with_background_color((0, 0, 0, 0))
        .with_devtools(devtools)
        // bez powiększania strony (Ctrl+kółko, Ctrl+plus/minus) – układ 1280×800 ma być stały
        .with_hotkeys_zoom(false)
        .with_ipc_handler(move |req: Request<String>| obsluz(&w_ipc, req.body()));
    // bez skrótów przeglądarki (F5 przeładowałby UI i zrestartował silnik audio, Ctrl+P drukowanie, Ctrl+F szukanie…)
    #[cfg(windows)]
    let budowniczy = budowniczy.with_browser_accelerator_keys(devtools);
    let webview = budowniczy.build(&okno).expect("WebView2");
    przypnij_skale(&webview, okno.scale_factor());
    *w.okno_skala.lock().unwrap() = okno.scale_factor();
    #[cfg(windows)]
    {
        use tao::platform::windows::WindowExtWindows;
        w.okno_hwnd.store(okno.hwnd(), Ordering::Relaxed);
    }
    okno.set_visible(true);

    // zegar UI: 60 Hz, a gdy okno schowane – 4 Hz (zostaje tylko zasobnik, zapis ustawień i pilnowanie strumieni)
    {
        let p = proxy.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(if W_TLE.load(Ordering::Relaxed) { 250 } else { 16 }));
            if p.send_event(Zd::Tick).is_err() {
                break;
            }
        });
    }

    // co 3 s: czy domyślne wyjście Windows ma Equalizer APO (w tle – enumeracja urządzeń nie blokuje UI)
    let wyjscie_apo: Arc<Mutex<Option<(String, bool)>>> = Arc::new(Mutex::new(None));
    {
        let wa = wyjscie_apo.clone();
        std::thread::spawn(move || {
            let mut ost = String::new();
            loop {
                // czy Equalizer APO jest zainstalowany i czy ładuje wtyczkę Szkła (odczyt rejestru i małego pliku)
                instalacja_stan::ustaw(instalacja::apo_zainstalowany(), instalacja::wtyczka_zarejestrowana());
                let r = apo_check::domyslne_wyjscie();
                // zmiana domyślnego wyjścia (słuchawki, Bluetooth…) → odtwarzacz i widmo przechodzą na nowe urządzenie
                if r.is_none() && ost != "?" {
                    log::warn!("wyjście Windows: nie udało się ustalić domyślnego urządzenia (cpal: {:?})", apo_check::nazwa_domyslnego());
                    ost = "?".into();
                }
                if let Some((nazwa, ma_apo)) = r.as_ref() {
                    if *nazwa != ost {
                        log::info!("wyjście Windows: {nazwa} (Equalizer APO: {})", if *ma_apo { "tak" } else { "nie" });
                        ost = nazwa.clone();
                        apo_check::GENERACJA.fetch_add(1, Ordering::Relaxed);
                    }
                }
                if let Ok(mut g) = wa.lock() {
                    *g = r;
                }
                std::thread::sleep(Duration::from_millis(1500));
            }
        });
    }

    let mut tray: Option<tray_icon::TrayIcon> = None;
    let mi_pokaz = MenuItem::new("Pokaż Szkło", true, None);
    let mi_wl = CheckMenuItem::new("Przetwarzanie włączone", true, w.ust.lock().unwrap().enabled, None);
    let mi_koniec = MenuItem::new("Zakończ", true, None);
    let mut running: Option<audio::Running> = None;
    let mut opis_urz = String::from("Czekam na dźwięk w Equalizer APO");
    let mut ost_status = String::new();
    let mut ost_wl = w.ust.lock().unwrap().enabled;
    // ostatnio wysłane do UI: skrypt mierników i opis urządzenia (wysyłamy tylko zmiany)
    let mut ost_mierniki = String::new();
    let mut ost_opis = String::new();
    let mut ost_poz = String::new();
    let mut licznik_tick: u32 = 0;
    let mut monitor: Option<monitor::Monitor> = None;
    let mut monitor_gen = apo_check::generacja();
    let mut ost_wersja_listy = u64::MAX;
    let mut ost_okladka_nr = u32::MAX;
    let mut ost_widmo = String::new();
    // urządzenie ma wpis Equalizer APO, coś gra, a wtyczka się nie łączy → APO dodany, ale jeszcze niezaładowany
    let mut apo_czeka_od: Option<Instant> = None;

    el.run(move |zd, _, cf| {
        *cf = ControlFlow::Wait;
        let _ = &kontekst;
        match zd {
            Event::NewEvents(StartCause::Init) => {
                let menu = Menu::new();
                let _ = menu.append_items(&[&mi_pokaz, &mi_wl, &PredefinedMenuItem::separator(), &mi_koniec]);
                tray = TrayIconBuilder::new()
                    .with_tooltip("Szkło")
                    .with_menu(Box::new(menu))
                    .with_menu_on_left_click(false)
                    .with_icon(tray_icon::Icon::from_rgba(ikona_rgba(32), 32, 32).expect("ikona"))
                    .build()
                    .ok();
            }
            Event::UserEvent(Zd::Js(s)) => {
                let _ = webview.evaluate_script(&s);
            }
            Event::UserEvent(Zd::Odswiez) => {
                ost_mierniki.clear();
                ost_opis.clear();
                ost_status.clear();
                // strona wczytana (od nowa): wyślij jeszcze raz wszystko, co idzie „tylko przy zmianie”
                ost_poz.clear();
                ost_widmo.clear();
                ost_wersja_listy = u64::MAX;
                ost_okladka_nr = u32::MAX;
                w.odtw.zmiana.store(true, Ordering::Relaxed);
                media_win::get().zmiana.store(true, Ordering::Relaxed);
            }
            Event::UserEvent(Zd::Okno(a)) => match a {
                "drag" => {
                    let _ = okno.drag_window();
                }
                "max" => okno.set_maximized(!okno.is_maximized()),
                "min" => okno.set_minimized(true),
                "pelny_wl" => {
                    if okno.fullscreen().is_none() {
                        log::info!("MilkDrop: pełny ekran");
                        bez_rogow_dwm(&okno);
                        okno.set_fullscreen(Some(tao::window::Fullscreen::Borderless(None)));
                        let _ = webview.evaluate_script("window.szklo&&szklo.pelnyEkran(true)");
                        // Esc wychodzi z pełnego ekranu (skrót przechwytywany tylko na ten czas – nie trafia do MilkDrop)
                        let p = w.proxy.clone();
                        milkdrop::esc_start(move || {
                            let _ = p.send_event(Zd::Okno("pelny_wyl"));
                        });
                    }
                }
                "pelny_wyl" => {
                    milkdrop::esc_stop();
                    if okno.fullscreen().is_some() {
                        log::info!("MilkDrop: koniec pełnego ekranu");
                        okno.set_fullscreen(None);
                        if akryl {
                            zaokraglij_dwm(&okno);
                        }
                    }
                    let _ = webview.evaluate_script("window.szklo&&szklo.pelnyEkran(false)");
                }
                "hide" => {
                    milkdrop::zamknij();
                    let _ = webview.evaluate_script("window.szklo&&szklo.milkStop()");
                    okno.set_visible(false)
                }
                _ => {}
            },
            Event::UserEvent(Zd::Tryb) => {
                let (mode, input, output) = {
                    let u = w.ust.lock().unwrap();
                    (u.mode.clone(), u.input.clone(), u.output.clone())
                };
                running = None;
                if mode == "app" {
                    link.active.store(false, Ordering::Relaxed);
                    match audio::start(&input, &output, params.clone(), local_meters.clone()) {
                        Ok(r) => {
                            log::info!("silnik w aplikacji: {input} → {output}: {}", r.description);
                            opis_urz = r.description.clone();
                            js(&w.proxy, format!("szklo.status({})", json!(format!("silnik w aplikacji działa, {}", r.description))));
                            running = Some(r);
                        }
                        Err(e) => {
                            log::error!("silnik w aplikacji nie wystartował ({input} → {output}): {e}");
                            opis_urz = "Silnik w aplikacji zatrzymany".into();
                            js(&w.proxy, format!("szklo.status({})", json!(format!("błąd: {e}"))));
                        }
                    }
                    ost_status.clear();
                } else {
                    link.active.store(true, Ordering::Relaxed);
                    ost_status.clear();
                }
            }
            Event::UserEvent(Zd::Tick) => {
                // mierniki
                let app_mode = running.is_some();
                let snap = if app_mode {
                    Some(local_meters.snapshot())
                } else {
                    link.meters.lock().ok().and_then(|g| *g).map(|m| {
                        let nazwa = wyjscie_apo.lock().ok().and_then(|g| g.as_ref().map(|x| x.0.clone())).unwrap_or_default();
                        let st = if nazwa.is_empty() {
                            format!("Equalizer APO · {}", audio::fmt_khz(m.fs))
                        } else {
                            format!("{nazwa} · Equalizer APO · {}", audio::fmt_khz(m.fs))
                        };
                        if st != opis_urz {
                            opis_urz = st;
                        }
                        m.snap
                    })
                };
                // UI odświeżamy tylko, gdy okno jest widoczne, i tylko gdy coś się zmieniło (w ciszy / bez
                // połączenia nie ma 60 wywołań evaluate_script na sekundę); skrypt idzie wprost do WebView,
                // bez dodatkowego przejścia przez kolejkę zdarzeń.
                let widoczne = okno.is_visible() && !okno.is_minimized();
                if W_TLE.swap(!widoczne, Ordering::Relaxed) == widoczne {
                    media_win::zwolnij(!widoczne);
                }
                licznik_tick = licznik_tick.wrapping_add(1);
                // MilkDrop zamknięty od środka (np. jego własnym skrótem) → karta wraca do odtwarzacza
                if licznik_tick % 30 == 0 && milkdrop::zakonczyl_sie() {
                    log::info!("MilkDrop: program zakończył się sam");
                    milkdrop::zamknij();
                    let _ = webview.evaluate_script("window.szklo&&szklo.milkStop()");
                }
                // strumień silnika padł (odłączone urządzenie, zmiana formatu w Windows) → postaw silnik od nowa
                if running.is_some() && licznik_tick % 30 == 0 && audio::blad_strumienia() {
                    log::warn!("silnik w aplikacji: strumień zgłosił błąd – uruchamiam ponownie");
                    let _ = w.proxy.send_event(Zd::Tryb);
                }
                if widoczne {
                    let skrypt = match snap {
                        Some(s) => format!(
                            "szklo.mierniki({{we:{:.1},wy:{:.1},gr:{:.2},lufs:{:.1},tp:{:.1}}})",
                            s.in_peak_db, s.out_peak_db, s.gr_db, s.lufs_short, s.true_peak_db
                        ),
                        None => "szklo.mierniki({we:-120,wy:-120,gr:0,lufs:-120,tp:-120})".to_string(),
                    };
                    if skrypt != ost_mierniki {
                        let _ = webview.evaluate_script(&skrypt);
                        ost_mierniki = skrypt;
                    }
                }
                // opis urządzenia i stan
                // (4×/s wystarczy – składanie napisu stanu 60×/s było stratą)
                let apo = (licznik_tick % 15 == 0 || !widoczne) && w.ust.lock().unwrap().mode == "apo";
                if apo {
                    // połączenie z wtyczką na urządzeniu BEZ APO to resztka po poprzednim wyjściu – nie pokazuj „przetwarza”
                    let bez_apo = matches!(wyjscie_apo.lock().ok().and_then(|g| g.clone()), Some((_, false)));
                    let (apo_jest, wtyczka_jest) = instalacja_stan::odczyt();
                    let st = if !apo_jest {
                        opis_urz = "Brak Equalizer APO".into();
                        "Equalizer APO nie jest zainstalowany – bez niego Szkło nie przetwarza dźwięku całego systemu. \
                         Kliknij „Pobierz i zainstaluj Equalizer APO” (bezpłatny program innego autora)."
                            .to_string()
                    } else if !wtyczka_jest && !link.connected.load(Ordering::Relaxed) {
                        opis_urz = "Szkło niepodłączone do Equalizer APO".into();
                        "Equalizer APO jest zainstalowany, ale nie ładuje wtyczki Szkła. Kliknij „Podłącz Szkło do Equalizer APO”.".to_string()
                    } else if link.connected.load(Ordering::Relaxed) && !bez_apo {
                        apo_czeka_od = None;
                        match wyjscie_apo.lock().ok().and_then(|g| g.clone()) {
                            Some((nazwa, _)) => format!("Szkło przetwarza dźwięk na „{nazwa}” (Equalizer APO)"),
                            None => "wtyczka w Equalizer APO połączona".to_string(),
                        }
                    } else {
                        match wyjscie_apo.lock().ok().and_then(|g| g.clone()) {
                            Some((nazwa, false)) => {
                                opis_urz = format!("Brak Equalizer APO na: {nazwa}");
                                format!(
                                    "„{nazwa}” nie ma Equalizer APO – dźwięk omija Szkło. Kliknij „Dodaj Equalizer APO do tego urządzenia”, \
                                     zaznacz je w oknie i uruchom ponownie komputer (jednorazowo dla każdego urządzenia)."
                                )
                            }
                            Some((nazwa, true)) => {
                                let gra_cos = w.odtw.stan.lock().map(|s| s.gra).unwrap_or(false)
                                    || media_win::get().stan.lock().map(|z| z.gra).unwrap_or(false);
                                if !gra_cos {
                                    apo_czeka_od = None;
                                }
                                let od = *apo_czeka_od.get_or_insert_with(Instant::now);
                                if gra_cos && od.elapsed() > Duration::from_secs(6) {
                                    opis_urz = format!("{nazwa} · Equalizer APO niezaładowany");
                                    format!(
                                        "„{nazwa}” ma dodany Equalizer APO, ale Windows jeszcze go nie załadował – dźwięk omija Szkło. \
                                         Kliknij „Załaduj bez restartu komputera” (dźwięk przerwie się na kilka sekund) albo uruchom ponownie komputer."
                                    )
                                } else {
                                    opis_urz = format!("{nazwa} · Equalizer APO · czekam na dźwięk");
                                    format!("„{nazwa}” ma Equalizer APO – Szkło przetworzy dźwięk, gdy coś zacznie grać.")
                                }
                            }
                            None => {
                                opis_urz = "Czekam na dźwięk w Equalizer APO".into();
                                "czekam na dźwięk: wtyczka łączy się, gdy coś gra na urządzeniu z Equalizer APO (instrukcja: docs/equalizer_apo.md)".to_string()
                            }
                        }
                    };
                    if st != ost_status {
                        log::info!("stan: {st}");
                        ost_status = st.clone();
                        js(&w.proxy, format!("szklo.status({})", json!(st)));
                        let brak = wyjscie_apo.lock().ok().and_then(|g| g.clone()).filter(|x| !x.1 && apo_jest).map(|x| x.0);
                        js(&w.proxy, format!("szklo.apoInstalacja({{jest:{apo_jest},wtyczka:{wtyczka_jest}}})"));
                        js(&w.proxy, format!("szklo.brakApo({})", json!(brak)));
                        js(&w.proxy, format!("szklo.apoNiezaladowany({})", st.contains("jeszcze go nie załadował")));
                    }
                }
                // wbudowany odtwarzacz: pełny stan po zmianie, pozycja ~10×/s, widmo ~30×/s (tylko powiększona karta)
                // podsłuch widma: okno widoczne, powiększona karta, a wbudowany odtwarzacz nie gra
                // (poza „if widoczne”: po schowaniu okna podsłuch ma się WYŁĄCZYĆ, a nie działać dalej w tle)
                if licznik_tick % 30 == 0 || (!widoczne && monitor.is_some()) {
                    let wlasny_gra = w.odtw.stan.lock().map(|s| s.gra).unwrap_or(false);
                    let chce = widoczne && w.na_tyle.load(Ordering::Relaxed) && w.odtw_duzy.load(Ordering::Relaxed) && !wlasny_gra;
                    // nowe wyjście Windows albo błąd strumienia → podsłuch od nowa
                    if monitor_gen != apo_check::generacja() || monitor.as_ref().is_some_and(|m| m.martwy.load(Ordering::Relaxed)) {
                        monitor_gen = apo_check::generacja();
                        monitor = None;
                    }
                    {
                        if chce && monitor.is_none() {
                            let (tryb, wy) = {
                                let u = w.ust.lock().unwrap();
                                (u.mode.clone(), u.output.clone())
                            };
                            let urz = if tryb == "app" { wy } else { String::new() };
                            match monitor::start(&urz, w.odtw.widmo.clone()) {
                                Ok(m) => {
                                    log::info!("widmo: podsłuch {}", m.urzadzenie);
                                    monitor = Some(m);
                                }
                                Err(e) => log::warn!("widmo: podsłuch niedostępny: {e}"),
                            }
                        } else if !chce && monitor.is_some() {
                            monitor = None;
                        }
                    }
                }
                if widoczne {
                    if w.odtw.zmiana.swap(false, Ordering::Relaxed) {
                        let wersja = w.odtw.stan.lock().map(|s| s.wersja).unwrap_or(0);
                        let z_lista = wersja != ost_wersja_listy;
                        ost_wersja_listy = wersja;
                        let _ = webview.evaluate_script(&format!("szklo.odtw({})", odtw_json(&w, z_lista)));
                    }
                    let media = media_win::get();
                    if media.zmiana.swap(false, Ordering::Relaxed) {
                        // okładka (data URL, bywa duża) idzie osobno i tylko po zmianie
                        let (z, okl) = match media.stan.lock() {
                            Ok(z) => (json!(*z), (z.okladka_nr != ost_okladka_nr).then(|| (z.okladka_nr, z.okladka.clone()))),
                            Err(_) => (Value::Null, None),
                        };
                        if let Some((nr, dane)) = okl {
                            ost_okladka_nr = nr;
                            let _ = webview.evaluate_script(&format!("szklo.zewnOkladka({})", json!(dane)));
                        }
                        let _ = webview.evaluate_script(&format!("szklo.zewn({z})"));
                    }
                    if spotify_pc::aktywny().is_some_and(|s| s.zmiana.swap(false, Ordering::Relaxed)) {
                        let _ = webview.evaluate_script(&format!("szklo.spotify({})", spotify_pc::stan_json()));
                    }
                    if licznik_tick % 6 == 0 {
                        let (p, g) = w.odtw.stan.lock().map(|s| (s.pozycja, s.gra)).unwrap_or((0.0, false));
                        let sk = format!("szklo.odtwPoz({p:.1},{g})");
                        if sk != ost_poz {
                            let _ = webview.evaluate_script(&sk);
                            ost_poz = sk;
                        }
                    }
                    if licznik_tick % 2 == 0 && w.na_tyle.load(Ordering::Relaxed) && w.odtw_duzy.load(Ordering::Relaxed) {
                        let wd = w.odtw.widmo.lock().map(|x| *x).unwrap_or_default();
                        let lista: Vec<String> = wd.iter().map(|v| format!("{v:.3}")).collect();
                        let sk = format!("szklo.widmo([{}])", lista.join(","));
                        if sk != ost_widmo {
                            let _ = webview.evaluate_script(&sk);
                            ost_widmo = sk;
                        }
                    }
                }
                if widoczne && opis_urz != ost_opis {
                    let _ = webview.evaluate_script(&format!("document.getElementById('urzadzenie').textContent={}", json!(opis_urz)));
                    ost_opis = opis_urz.clone();
                }
                // menu zasobnika ↔ przycisk Włączone
                let wl = params.enabled();
                if wl != ost_wl {
                    ost_wl = wl;
                    mi_wl.set_checked(wl);
                }
                while let Ok(ev) = MenuEvent::receiver().try_recv() {
                    if ev.id == mi_pokaz.id() {
                        okno.set_visible(true);
                        okno.set_focus();
                    } else if ev.id == mi_wl.id() {
                        let on = mi_wl.is_checked();
                        params.set_enabled(on);
                        w.ust.lock().unwrap().enabled = on;
                        oznacz_zmiane(&w);
                        js(&w.proxy, format!("szklo.stan({{enabled:{on}}})"));
                    } else if ev.id == mi_koniec.id() {
                        w.ust.lock().unwrap().save();
                        // nie zostawiaj po sobie niewidocznych/osieroconych programów
                        spotify_okno::przy_wyjsciu();
                        milkdrop::zamknij();
                        tray.take();
                        *cf = ControlFlow::Exit;
                    }
                }
                while let Ok(ev) = TrayIconEvent::receiver().try_recv() {
                    if let TrayIconEvent::Click { button: MouseButton::Left, .. } = ev {
                        okno.set_visible(true);
                        okno.set_focus();
                    }
                }
                // zapis ustawień 1 s po ostatniej zmianie
                let due = w.zmiana.lock().unwrap().map(|t| t.elapsed() > Duration::from_secs(1)).unwrap_or(false);
                if due {
                    *w.zmiana.lock().unwrap() = None;
                    let mut u = w.ust.lock().unwrap();
                    u.capture_from(&params);
                    u.save();
                }
                let _ = PRESETS.len();
            }
            // maksymalizacja/przywrócenie: UI zdejmuje zaokrąglenie i obrys, gdy okno wypełnia ekran
            Event::WindowEvent { event: WindowEvent::Resized(_), .. } => {
                let _ = webview.evaluate_script(&format!("window.szklo&&szklo.okno({{max:{}}})", okno.is_maximized()));
                if milkdrop::dziala() {
                    let _ = webview.evaluate_script("window.szklo&&szklo.milkPrzesun()");
                }
            }
            Event::WindowEvent { event: WindowEvent::ScaleFactorChanged { scale_factor, .. }, .. } => {
                przypnij_skale(&webview, scale_factor);
                *w.okno_skala.lock().unwrap() = scale_factor;
            }
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                // zamknięcie chowa do zasobnika – przetwarzanie działa dalej
                milkdrop::zamknij();
                let _ = webview.evaluate_script("window.szklo&&szklo.milkStop()");
                okno.set_visible(false);
            }
            _ => {}
        }
    });
}


/// „Pobierz i zainstaluj Equalizer APO” – w tle, postęp na karcie „Źródło dźwięku”.
fn apo_instaluj(w: &Arc<Wspolne>) {
    let w2 = w.clone();
    std::thread::spawn(move || {
        log::info!("Equalizer APO: pobieranie i instalacja (na prośbę użytkownika)");
        let p = w2.proxy.clone();
        match instalacja::pobierz_i_zainstaluj_apo(&|t| js(&p, format!("szklo.apoPostep({},false)", json!(t)))) {
            Ok(()) => js(
                &w2.proxy,
                format!(
                    "szklo.apoPostep({},false)",
                    json!("Equalizer APO zainstalowany, Szkło podłączone. Kliknij „Załaduj bez restartu komputera” albo uruchom ponownie komputer.")
                ),
            ),
            Err(e) => {
                log::error!("Equalizer APO: {e}");
                js(&w2.proxy, format!("szklo.apoPostep({},true)", json!(e)));
            }
        }
    });
}

/// Stan instalacji Equalizer APO / wtyczki – liczony w wątku pilnującym wyjścia, czytany przez pętlę UI.
mod instalacja_stan {
    use std::sync::atomic::{AtomicU8, Ordering};
    static S: AtomicU8 = AtomicU8::new(0b11); // do pierwszego sprawdzenia: zakładamy, że wszystko jest
    pub fn ustaw(apo: bool, wtyczka: bool) {
        S.store(apo as u8 | (wtyczka as u8) << 1, Ordering::Relaxed);
    }
    pub fn odczyt() -> (bool, bool) {
        let s = S.load(Ordering::Relaxed);
        (s & 1 != 0, s & 2 != 0)
    }
}
