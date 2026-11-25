//! Stan widoku biblioteki i wyszukiwania (dla interfejsu PC i Androida): polecenia wchodzą jako JSON, stan wychodzi jako
//! JSON z numerem wersji – interfejs pyta o wersję i przerysowuje się tylko po zmianie.
//!
//! Polecenia: `{"c":"odswiez","max_s":300}` · `{"c":"otworz","uri":"…"}` · `{"c":"wstecz"}`
//! `{"c":"szukaj","q":"…","zakres":"lista|biblioteka|katalog"}` (puste `q` wraca do zaznaczonej listy).

use crate::dane::*;
use crate::tekst::uprosc;
use futures_util::StreamExt;
use librespot_core::session::Session;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;

/// Lista w widoku jest ograniczona (interfejs i tak doczytuje przy przewijaniu).
const MAKS_UTWOROW: usize = 10_000;
const MAKS_WYNIKOW: usize = 1_000;
const CZAS: Duration = Duration::from_secs(45);

#[derive(Serialize, Default, Clone)]
struct Spis {
    /// pusto | wczytywanie | gotowe | blad
    stan: &'static str,
    info: String,
    listy: Vec<Lista>,
}

#[derive(Serialize, Default, Clone)]
struct Widok {
    /// "" | lista | szukaj
    rodzaj: &'static str,
    uri: String,
    tytul: String,
    q: String,
    zakres: String,
    /// nazwa zaznaczonej listy (do podpisu zakresu szukania)
    zaznaczona: String,
    wczytywanie: bool,
    info: String,
    wstecz: bool,
    #[serde(flatten)]
    wyniki: Wyniki,
}

#[derive(Default)]
struct Stan {
    spis: Spis,
    bib: Biblioteka,
    kiedy: Option<Instant>,
    widok: Widok,
    historia: Vec<String>,
    /// zaznaczona lista z biblioteki (zakres „lista”)
    zaznaczona: Option<String>,
    tresci: HashMap<String, Arc<Vec<Utwor>>>,
    /// utwory całej biblioteki (po pierwszym szukaniu)
    indeks: Option<Arc<Vec<Utwor>>>,
}

struct Wnetrze {
    polaczenie: Mutex<Option<(Handle, Session)>>,
    stan: Mutex<Stan>,
    pam: Pamiec,
    wersja_spisu: AtomicU64,
    wersja_widoku: AtomicU64,
    /// numer ostatniego polecenia widoku – spóźnione odpowiedzi są pomijane
    numer: AtomicU64,
    indeksowanie: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
pub struct Kontroler(Arc<Wnetrze>);

impl Kontroler {
    /// `katalog` – prywatny folder aplikacji na opisy utworów (None = tylko w pamięci).
    pub fn nowy(katalog: Option<PathBuf>) -> Self {
        Self(Arc::new(Wnetrze {
            polaczenie: Mutex::new(None),
            stan: Mutex::new(Stan { spis: Spis { stan: "pusto", ..Default::default() }, ..Default::default() }),
            pam: Pamiec::nowa(katalog.map(|k| k.join("utwory_spotify.json"))),
            wersja_spisu: AtomicU64::new(1),
            wersja_widoku: AtomicU64::new(1),
            numer: AtomicU64::new(0),
            indeksowanie: tokio::sync::Mutex::new(()),
        }))
    }

    fn stan(&self) -> MutexGuard<'_, Stan> {
        self.0.stan.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn zmien_spis(&self, f: impl FnOnce(&mut Stan)) {
        f(&mut self.stan());
        self.0.wersja_spisu.fetch_add(1, Ordering::Release);
    }
    fn zmien_widok(&self, f: impl FnOnce(&mut Stan)) {
        f(&mut self.stan());
        self.0.wersja_widoku.fetch_add(1, Ordering::Release);
    }
    fn sesja_i_watek(&self) -> Option<(Handle, Session)> {
        self.0.polaczenie.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn aktualne(&self, nr: u64) -> bool {
        self.0.numer.load(Ordering::Relaxed) == nr
    }

    /// Nowe połączenie (zalogowano albo połączono ponownie). Wołać z wątku tokio.
    pub fn polacz(&self, s: Session) {
        *self.0.polaczenie.lock().unwrap_or_else(|e| e.into_inner()) = Some((Handle::current(), s));
        if self.stan().kiedy.is_none() {
            self.odswiez();
        }
    }

    pub fn polaczony(&self) -> bool {
        self.sesja_i_watek().is_some_and(|(_, s)| !s.is_invalid())
    }

    /// Koniec połączenia bez czyszczenia danych (np. wyłączony odbiornik).
    pub fn rozlacz(&self) {
        *self.0.polaczenie.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Wylogowanie: wszystko znika (także z dysku).
    pub fn wyloguj(&self) {
        self.rozlacz();
        self.0.numer.fetch_add(1, Ordering::Relaxed);
        self.0.pam.wyczysc();
        self.zmien_spis(|s| *s = Stan { spis: Spis { stan: "pusto", ..Default::default() }, ..Default::default() });
        self.0.wersja_widoku.fetch_add(1, Ordering::Release);
    }

    /// (wersja spisu, wersja widoku)
    pub fn wersje(&self) -> (u64, u64) {
        (self.0.wersja_spisu.load(Ordering::Acquire), self.0.wersja_widoku.load(Ordering::Acquire))
    }
    pub fn spis_json(&self) -> String {
        serde_json::to_string(&self.stan().spis).unwrap_or_else(|_| "null".into())
    }
    pub fn widok_json(&self) -> String {
        serde_json::to_string(&self.stan().widok).unwrap_or_else(|_| "null".into())
    }
    /// Utwory z bieżącego widoku (np. do kolejki odtwarzacza).
    pub fn utwory(&self) -> Vec<Utwor> {
        self.stan().widok.wyniki.utwory.clone()
    }

    pub fn rozkaz(&self, m: &serde_json::Value) {
        match m["c"].as_str().unwrap_or("") {
            "odswiez" => {
                let maks = Duration::from_secs(m["max_s"].as_u64().unwrap_or(0));
                let swiezy = { self.stan().kiedy.is_some_and(|t| t.elapsed() < maks) };
                if !swiezy {
                    self.odswiez();
                }
            }
            "otworz" => {
                if let Some(u) = m["uri"].as_str() {
                    self.otworz(u, true);
                }
            }
            "wstecz" => {
                let u = {
                    let mut s = self.stan();
                    s.historia.pop(); // bieżący
                    s.historia.pop()
                };
                match u {
                    Some(u) => match u.strip_prefix("szukaj:").and_then(|r| r.split_once(':')) {
                        Some((z, q)) => self.szukaj(q, z),
                        None => self.otworz(&u, true),
                    },
                    None => self.zmien_widok(|s| s.widok = Widok::default()),
                }
            }
            "szukaj" => self.szukaj(m["q"].as_str().unwrap_or(""), m["zakres"].as_str().unwrap_or("biblioteka")),
            _ => {}
        }
    }

    fn odswiez(&self) {
        let Some((h, ses)) = self.sesja_i_watek() else { return };
        {
            let mut s = self.stan();
            if s.spis.stan == "wczytywanie" {
                return;
            }
            s.spis.stan = "wczytywanie";
            s.spis.info = "Wczytywanie biblioteki…".into();
        }
        self.0.wersja_spisu.fetch_add(1, Ordering::Release);
        let k = self.clone();
        h.spawn(async move {
            let w = tokio::time::timeout(CZAS, biblioteka(&ses)).await.unwrap_or_else(|_| Err("Spotify nie odpowiada".into()));
            k.zmien_spis(|s| match w {
                Ok(b) => {
                    let pl = b.listy.iter().filter(|l| l.rodzaj == "playlista").count();
                    let pol = b.listy.iter().find(|l| l.rodzaj == "polubione").map(|l| l.ile).unwrap_or(0);
                    log::info!("Spotify: biblioteka: {pl} playlist, {pol} polubionych, {} albumów, {} wykonawców", b.albumy.len(), b.wykonawcy.len());
                    s.spis = Spis {
                        stan: "gotowe",
                        info: if b.braki.is_empty() { format!("{pl} playlist") } else { format!("{pl} playlist · nie wczytano: {}", b.braki.join(", ")) },
                        listy: b.listy.clone(),
                    };
                    s.bib = b;
                    s.kiedy = Some(Instant::now());
                    s.tresci.clear();
                    s.indeks = None;
                }
                Err(e) => {
                    log::error!("Spotify: biblioteka: {e}");
                    s.spis.stan = "blad";
                    s.spis.info = format!("Nie udało się wczytać biblioteki: {e}");
                }
            });
        });
    }

    fn otworz(&self, uri: &str, do_historii: bool) {
        let nr = self.0.numer.fetch_add(1, Ordering::Relaxed) + 1;
        let uri = uri.to_string();
        let mut gotowe = false;
        self.zmien_widok(|s| {
            let z_listy = s.spis.listy.iter().find(|l| l.uri == uri).map(|l| l.nazwa.clone());
            if z_listy.is_some() {
                // nowa lista z lewej kolumny zaczyna nową historię
                s.zaznaczona = Some(uri.clone());
                s.historia.clear();
            }
            if do_historii && s.historia.last() != Some(&uri) {
                s.historia.push(uri.clone());
            }
            let zazn = zaznaczona_nazwa(s);
            s.widok = Widok { rodzaj: "lista", uri: uri.clone(), tytul: z_listy.unwrap_or_default(), zaznaczona: zazn, wczytywanie: true, wstecz: s.historia.len() > 1, ..Default::default() };
            if uri == ALBUMY || uri == WYKONAWCY {
                s.widok.wczytywanie = false;
                if uri == ALBUMY {
                    s.widok.wyniki.albumy = s.bib.albumy.clone();
                } else {
                    s.widok.wyniki.wykonawcy = s.bib.wykonawcy.clone();
                }
                gotowe = true;
            } else if let Some(t) = s.tresci.get(&uri) {
                s.widok.wyniki.utwory = t.iter().take(MAKS_UTWOROW).cloned().collect();
                s.widok.wczytywanie = false;
                gotowe = true;
            }
        });
        if gotowe {
            return;
        }
        let Some((h, ses)) = self.sesja_i_watek() else {
            self.zmien_widok(|s| brak_polaczenia(&mut s.widok));
            return;
        };
        let k = self.clone();
        h.spawn(async move {
            let w = k.pobierz(&ses, &uri).await;
            if !k.aktualne(nr) {
                return;
            }
            k.zmien_widok(|s| {
                s.widok.wczytywanie = false;
                match w {
                    Ok(t) => {
                        if s.widok.tytul.is_empty() {
                            s.widok.tytul = t.tytul;
                        }
                        s.widok.wyniki = Wyniki { utwory: t.utwory.into_iter().take(MAKS_UTWOROW).collect(), albumy: t.albumy, wykonawcy: Vec::new() };
                    }
                    Err(e) => {
                        log::error!("Spotify: {uri}: {e}");
                        s.widok.info = format!("Nie udało się wczytać: {e}");
                    }
                }
            });
        });
    }

    async fn pobierz(&self, ses: &Session, uri: &str) -> Result<Tresc, String> {
        let t = tokio::time::timeout(CZAS, tresc(ses, &self.0.pam, uri)).await.map_err(|_| "Spotify nie odpowiada".to_string())??;
        if t.albumy.is_empty() {
            self.stan().tresci.insert(uri.to_string(), Arc::new(t.utwory.clone()));
        }
        Ok(t)
    }

    fn szukaj(&self, q: &str, zakres: &str) {
        let q = q.trim().chars().take(120).collect::<String>();
        let nr = self.0.numer.fetch_add(1, Ordering::Relaxed) + 1;
        if uprosc(&q).is_empty() {
            let z = { self.stan().zaznaczona.clone() };
            match z {
                Some(u) => self.otworz(&u, true),
                None => self.zmien_widok(|s| {
                    s.historia.clear();
                    s.widok = Widok::default()
                }),
            }
            return;
        }
        let zakres = zakres.to_string();
        let znak = format!("szukaj:{zakres}:{q}");
        self.zmien_widok(|s| {
            // kolejne litery tego samego szukania nie zapychają historii
            if s.historia.last().is_some_and(|h| h.starts_with("szukaj:")) {
                s.historia.pop();
            }
            s.historia.push(znak.clone());
            let zazn = zaznaczona_nazwa(s);
            let stare = std::mem::take(&mut s.widok);
            let wyniki = if stare.rodzaj == "szukaj" && stare.zakres == zakres { stare.wyniki } else { Wyniki::default() };
            s.widok = Widok { rodzaj: "szukaj", q: q.clone(), tytul: format!("Szukanie: {q}"), zakres: zakres.clone(), zaznaczona: zazn, wczytywanie: true, wstecz: s.historia.len() > 1, wyniki, ..Default::default() };
        });
        let polaczenie = self.sesja_i_watek();
        let Some((h, ses)) = polaczenie else {
            self.zmien_widok(|s| brak_polaczenia(&mut s.widok));
            return;
        };
        let k = self.clone();
        h.spawn(async move {
            let w = match zakres.as_str() {
                "katalog" => tokio::time::timeout(CZAS, katalog(&ses, &k.0.pam, &q)).await.unwrap_or_else(|_| Err("Spotify nie odpowiada".into())),
                "lista" => k.szukaj_w_liscie(&ses, &q).await,
                _ => k.szukaj_w_bibliotece(&ses, &q, nr).await,
            };
            if !k.aktualne(nr) {
                return;
            }
            k.zmien_widok(|s| {
                s.widok.wczytywanie = false;
                match w {
                    Ok(w) => {
                        s.widok.info = if w.utwory.is_empty() && w.albumy.is_empty() && w.wykonawcy.is_empty() { "Nic nie znaleziono.".into() } else { String::new() };
                        s.widok.wyniki = w;
                    }
                    Err(e) => {
                        log::error!("Spotify: szukanie: {e}");
                        s.widok.wyniki = Wyniki::default();
                        s.widok.info = e;
                    }
                }
            });
        });
    }

    async fn szukaj_w_liscie(&self, ses: &Session, q: &str) -> Result<Wyniki, String> {
        let (uri, gotowe) = {
            let s = self.stan();
            let uri = s.zaznaczona.clone().ok_or("Najpierw zaznacz listę.")?;
            if uri == ALBUMY {
                return Ok(szukaj_w(q, &[], &s.bib.albumy, &[]));
            }
            if uri == WYKONAWCY {
                return Ok(szukaj_w(q, &[], &[], &s.bib.wykonawcy));
            }
            let t = s.tresci.get(&uri).cloned();
            (uri, t)
        };
        let u = match gotowe {
            Some(u) => u,
            None => Arc::new(self.pobierz(ses, &uri).await?.utwory),
        };
        Ok(szukaj_w(q, &u, &[], &[]))
    }

    async fn szukaj_w_bibliotece(&self, ses: &Session, q: &str, nr: u64) -> Result<Wyniki, String> {
        let u = self.zbuduj_indeks(ses, nr).await?;
        let (a, w) = {
            let s = self.stan();
            (s.bib.albumy.clone(), s.bib.wykonawcy.clone())
        };
        Ok(szukaj_w(q, &u, &a, &w))
    }

    /// Utwory ze wszystkich list (raz na odświeżenie biblioteki). Listy, których nie da się wczytać, są pomijane.
    async fn zbuduj_indeks(&self, ses: &Session, nr: u64) -> Result<Arc<Vec<Utwor>>, String> {
        let _jeden = self.0.indeksowanie.lock().await;
        let (indeks, listy, albumy) = {
            let s = self.stan();
            (s.indeks.clone(), s.spis.listy.clone(), s.bib.albumy.clone())
        };
        if let Some(i) = indeks {
            return Ok(i);
        }
        if listy.is_empty() {
            return Err("Biblioteka jeszcze się wczytuje – spróbuj za chwilę.".into());
        }
        let t0 = Instant::now();
        let adr: Vec<String> =
            listy.iter().filter(|l| !l.uri.starts_with("szklo:") && l.rodzaj != "podcast").map(|l| l.uri.clone()).chain(albumy.into_iter().map(|a| a.uri)).collect();
        let ile = adr.len();
        let (mut zrobione, mut pominiete) = (0, 0);
        let mut v: Vec<Utwor> = Vec::new();
        let mut byly = HashSet::new();
        let mut strumien = futures_util::stream::iter(adr)
            .map(|u| async move {
                let gotowe = { self.stan().tresci.get(&u).cloned() };
                match gotowe {
                    Some(t) => Ok(t),
                    None => self.pobierz(ses, &u).await.map(|t| Arc::new(t.utwory)),
                }
            })
            .buffered(4);
        while let Some(t) = strumien.next().await {
            zrobione += 1;
            match t {
                Ok(t) => v.extend(t.iter().filter(|u| byly.insert(u.uri.clone())).cloned()),
                Err(_) => pominiete += 1,
            }
            if zrobione % 5 == 0 && self.aktualne(nr) {
                self.zmien_widok(|s| s.widok.info = format!("Wczytywanie biblioteki do szukania: {zrobione} z {ile} list…"));
            }
            if ses.is_invalid() {
                return Err("Połączenie ze Spotify zerwane.".into());
            }
        }
        drop(strumien);
        log::info!("Spotify: spis do szukania: {} utworów z {ile} list ({pominiete} pominiętych) w {:.1} s", v.len(), t0.elapsed().as_secs_f32());
        if pominiete == ile {
            return Err("Nie udało się wczytać biblioteki ze Spotify.".into());
        }
        self.0.pam.zapisz();
        let v = Arc::new(v);
        self.stan().indeks = Some(v.clone());
        Ok(v)
    }
}

fn brak_polaczenia(w: &mut Widok) {
    w.wczytywanie = false;
    w.info = "Brak połączenia ze Spotify.".into();
}

fn zaznaczona_nazwa(s: &Stan) -> String {
    s.zaznaczona.as_ref().and_then(|u| s.spis.listy.iter().find(|l| &l.uri == u)).map(|l| l.nazwa.clone()).unwrap_or_default()
}

/// Szukanie w podanych pozycjach: utwory, albumy (podane + z utworów), wykonawcy (podani + z utworów).
pub fn szukaj_w(q: &str, utwory: &[Utwor], albumy: &[Album], wykonawcy: &[Wykonawca]) -> Wyniki {
    let q = uprosc(q);
    if q.is_empty() {
        return Wyniki::default();
    }
    let mut u: Vec<(u32, &Utwor)> = utwory.iter().map(|u| (szuk_utwor(&q, u), u)).filter(|x| x.0 > 0).collect();
    u.sort_by_key(|x| std::cmp::Reverse(x.0));

    let mut byly: HashSet<&str> = HashSet::new();
    let mut a: Vec<(u32, Album)> = Vec::new();
    for x in albumy {
        let o = szuk_album(&q, x);
        if o > 0 && byly.insert(&x.uri) {
            a.push((o, x.clone()));
        }
    }
    for x in utwory.iter().filter(|x| x.album.uri.starts_with("spotify:album:")) {
        let al = Album { uri: x.album.uri.clone(), nazwa: x.album.nazwa.clone(), wykonawca: x.wykonawca(), ..Default::default() };
        let o = szuk_album(&q, &al);
        if o > 0 && byly.insert(&x.album.uri) {
            a.push((o, al));
        }
    }
    a.sort_by_key(|x| std::cmp::Reverse(x.0));

    let mut byly: HashSet<&str> = HashSet::new();
    let mut w: Vec<(u32, Wykonawca)> = Vec::new();
    let z_utworow = utwory.iter().flat_map(|x| &x.wykonawcy).filter(|o| o.uri.starts_with("spotify:artist:")).map(|o| (o.uri.as_str(), o.nazwa.as_str()));
    for (uri, nazwa) in wykonawcy.iter().map(|x| (x.uri.as_str(), x.nazwa.as_str())).chain(z_utworow) {
        let o = crate::tekst::ocena(&q, &uprosc(nazwa));
        if o > 0 && byly.insert(uri) {
            w.push((o, Wykonawca { uri: uri.into(), nazwa: nazwa.into() }));
        }
    }
    w.sort_by_key(|x| std::cmp::Reverse(x.0));

    Wyniki {
        utwory: u.into_iter().take(MAKS_WYNIKOW).map(|x| x.1.clone()).collect(),
        albumy: a.into_iter().take(MAKS_WYNIKOW).map(|x| x.1).collect(),
        wykonawcy: w.into_iter().take(MAKS_WYNIKOW).map(|x| x.1).collect(),
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    fn u(t: &str, w: &str, a: &str) -> Utwor {
        let o = |r: &str, n: &str| Odn { nazwa: n.into(), uri: format!("spotify:{r}:{n}") };
        Utwor { uri: format!("spotify:track:{t}"), tytul: t.into(), wykonawcy: vec![o("artist", w)], album: o("album", a), czas: 1.0 }
    }

    #[test]
    fn szukanie_dzieli_na_rodzaje_i_szereguje() {
        let v = [u("Arahja", "Kult", "Spokojnie"), u("Kult jednostki", "Inny", "Płyta"), u("Coś", "Okultyści", "Żółty album"), u("Nic", "Nikt", "Nigdzie")];
        let w = szukaj_w("KULT", &v, &[], &[]);
        assert_eq!(w.utwory.iter().map(|x| x.tytul.as_str()).collect::<Vec<_>>(), ["Arahja", "Kult jednostki", "Coś"]);
        assert_eq!(w.wykonawcy.iter().map(|x| x.nazwa.as_str()).collect::<Vec<_>>(), ["Kult", "Okultyści"]);
        assert_eq!(w.albumy.len(), 2); // albumy wykonawców pasujących do zapytania
        assert_eq!(szukaj_w("zolty", &v, &[], &[]).albumy[0].nazwa, "Żółty album");
        assert_eq!(szukaj_w("arahja kult", &v, &[], &[]).utwory.len(), 1);
        assert!(szukaj_w("  ", &v, &[], &[]).utwory.is_empty());
    }

    #[test]
    fn album_zapisany_i_z_utworu_nie_powtarza_sie() {
        let v = [u("A", "Kult", "Spokojnie")];
        let a = [Album { uri: "spotify:album:Spokojnie".into(), nazwa: "Spokojnie".into(), wykonawca: "Kult".into(), rok: 1988, ile: 10 }];
        let w = szukaj_w("spokojnie", &v, &a, &[]);
        assert_eq!(w.albumy.len(), 1);
        assert_eq!(w.albumy[0].rok, 1988);
    }

    #[test]
    fn bez_polaczenia_widok_mowi_co_jest() {
        let k = Kontroler::nowy(None);
        let v0 = k.wersje().1;
        k.rozkaz(&serde_json::json!({"c":"szukaj","q":"kult","zakres":"katalog"}));
        assert!(k.wersje().1 > v0);
        let w: serde_json::Value = serde_json::from_str(&k.widok_json()).unwrap();
        assert_eq!(w["wczytywanie"], false);
        assert_eq!(w["info"], "Brak połączenia ze Spotify.");
        assert!(w["utwory"].is_array() && w["albumy"].is_array() && w["wykonawcy"].is_array());
        k.rozkaz(&serde_json::json!({"c":"szukaj","q":""}));
        assert_eq!(serde_json::from_str::<serde_json::Value>(&k.widok_json()).unwrap()["rodzaj"], "");
        k.rozkaz(&serde_json::json!({"c":"wstecz"}));
    }
}
