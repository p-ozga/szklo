//! Odczyt danych ze Spotify: biblioteka, zawartość list, wyszukiwanie w katalogu.

use crate::tekst::{do_adresu, ocena, uprosc};
use futures_util::StreamExt;
use http::{header, HeaderMap, HeaderValue, Method};
use librespot_core::{session::Session, spotify_id::SpotifyId, SpotifyUri};
use librespot_metadata::{Metadata, Playlist};
use librespot_protocol::extended_metadata::{BatchedEntityRequest, EntityRequest, ExtensionQuery};
use librespot_protocol::extension_kind::ExtensionKind;
use librespot_protocol::metadata as md;
use librespot_protocol::playlist4_external::SelectedListContent;
use protobuf::{EnumOrUnknown, Message};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;

/// Listy, które nie są playlistami (w aplikacji Spotify to zakładki „Twojej biblioteki”).
pub const POLUBIONE: &str = "spotify:collection";
pub const ALBUMY: &str = "szklo:albumy";
pub const WYKONAWCY: &str = "szklo:wykonawcy";
const ODCINKI: &str = "szklo:odcinki";

/// Nazwa z odnośnikiem (wykonawca albo album przy utworze).
#[derive(Clone, Serialize, Deserialize, Default, Debug, PartialEq)]
pub struct Odn {
    pub nazwa: String,
    pub uri: String,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Utwor {
    pub uri: String,
    pub tytul: String,
    pub wykonawcy: Vec<Odn>,
    pub album: Odn,
    /// sekundy
    pub czas: f64,
}

impl Utwor {
    pub fn wykonawca(&self) -> String {
        self.wykonawcy.iter().map(|a| a.nazwa.as_str()).collect::<Vec<_>>().join(", ")
    }
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Album {
    pub uri: String,
    pub nazwa: String,
    pub wykonawca: String,
    pub rok: i32,
    /// liczba utworów (0 = nieznana)
    pub ile: usize,
}

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct Wykonawca {
    pub uri: String,
    pub nazwa: String,
}

#[derive(Clone, Serialize, Default, Debug)]
pub struct Lista {
    pub uri: String,
    pub nazwa: String,
    pub ile: i32,
    /// polubione | odcinki | albumy | wykonawcy | podcast | playlista
    pub rodzaj: &'static str,
    /// folder playlist w Spotify (pusty = bez folderu)
    pub folder: String,
}

#[derive(Clone, Default)]
pub struct Biblioteka {
    pub listy: Vec<Lista>,
    pub albumy: Vec<Album>,
    pub wykonawcy: Vec<Wykonawca>,
    /// czego nie udało się wczytać (reszta działa)
    pub braki: Vec<String>,
}

/// Zawartość listy, albumu albo strona wykonawcy.
#[derive(Clone, Default)]
pub struct Tresc {
    pub tytul: String,
    pub utwory: Vec<Utwor>,
    pub albumy: Vec<Album>,
}

#[derive(Clone, Serialize, Default, Debug)]
pub struct Wyniki {
    pub utwory: Vec<Utwor>,
    pub albumy: Vec<Album>,
    pub wykonawcy: Vec<Wykonawca>,
}

/// Opisy utworów już raz pobrane (szukanie w bibliotece bez ponownego pytania Spotify). Plik na dysku jest opcjonalny.
#[derive(Default)]
pub struct Pamiec {
    utwory: Mutex<HashMap<String, Utwor>>,
    plik: Option<PathBuf>,
}

impl Pamiec {
    pub fn nowa(plik: Option<PathBuf>) -> Self {
        let utwory = plik
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<Vec<Utwor>>(&b).ok())
            .map(|v| v.into_iter().map(|u| (u.uri.clone(), u)).collect())
            .unwrap_or_default();
        Self { utwory: Mutex::new(utwory), plik }
    }

    fn daj(&self, uri: &str) -> Option<Utwor> {
        self.utwory.lock().ok()?.get(uri).cloned()
    }

    fn dodaj(&self, v: &[Utwor]) {
        if let Ok(mut m) = self.utwory.lock() {
            for u in v {
                m.insert(u.uri.clone(), u.clone());
            }
        }
    }

    pub fn zapisz(&self) {
        let Some(p) = &self.plik else { return };
        let Ok(b) = self.utwory.lock().map(|m| serde_json::to_vec(&m.values().collect::<Vec<_>>()).unwrap_or_default()) else { return };
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, b).and_then(|_| std::fs::rename(&tmp, p)).is_err() {
            log::warn!("Spotify: nie udało się zapisać opisów utworów");
        }
    }

    /// Wylogowanie: opisy znikają z pamięci i z dysku.
    pub fn wyczysc(&self) {
        if let Ok(mut m) = self.utwory.lock() {
            m.clear();
        }
        if let Some(p) = &self.plik {
            let _ = std::fs::remove_file(p);
        }
    }
}

type Wynik<T> = Result<T, String>;

fn uri_z_gid(rodzaj: &str, gid: &[u8]) -> Option<String> {
    Some(format!("spotify:{rodzaj}:{}", SpotifyId::from_raw(gid).ok()?.to_base62().ok()?))
}

fn odn(rodzaj: &str, gid: &[u8], nazwa: &str) -> Odn {
    Odn { nazwa: nazwa.to_string(), uri: uri_z_gid(rodzaj, gid).unwrap_or_default() }
}

/// Opisy wielu pozycji naraz (paczki po 250). Kolejność wg `uris`; brakujące pozycje są pomijane.
async fn opisy(s: &Session, uris: &[String], rodzaj: ExtensionKind) -> Wynik<Vec<(String, Vec<u8>)>> {
    let paczki: Vec<Vec<String>> = uris.chunks(250).map(|c| c.to_vec()).collect();
    let odp: Vec<Wynik<Vec<(String, Vec<u8>)>>> = futures_util::stream::iter(paczki)
        .map(|p| async move {
            let req = BatchedEntityRequest {
                entity_request: p
                    .into_iter()
                    .map(|entity_uri| EntityRequest {
                        entity_uri,
                        query: vec![ExtensionQuery { extension_kind: EnumOrUnknown::new(rodzaj), ..Default::default() }],
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            };
            let r = s.spclient().get_extended_metadata(req).await.map_err(|e| e.to_string())?;
            Ok(r.extended_metadata
                .into_iter()
                .flat_map(|a| a.extension_data)
                .filter_map(|mut d| Some((d.entity_uri.clone(), d.extension_data.take()?.value)))
                .filter(|(_, b)| !b.is_empty())
                .collect())
        })
        .buffered(3)
        .collect()
        .await;
    let mut mapa = HashMap::new();
    for o in odp {
        mapa.extend(o?);
    }
    Ok(uris.iter().filter_map(|u| Some((u.clone(), mapa.get(u)?.clone()))).collect())
}

fn utwor_z(uri: &str, b: &[u8]) -> Option<Utwor> {
    if uri.starts_with("spotify:episode:") {
        let e = md::Episode::parse_from_bytes(b).ok()?;
        let pod = odn("show", e.show.gid(), e.show.name());
        return Some(Utwor { uri: uri.into(), tytul: e.name().into(), wykonawcy: vec![pod.clone()], album: pod, czas: e.duration() as f64 / 1000.0 });
    }
    let t = md::Track::parse_from_bytes(b).ok()?;
    Some(Utwor {
        uri: uri.into(),
        tytul: t.name().into(),
        wykonawcy: t.artist.iter().map(|a| odn("artist", a.gid(), a.name())).collect(),
        album: odn("album", t.album.gid(), t.album.name()),
        czas: t.duration() as f64 / 1000.0,
    })
}

/// Utwory i odcinki z opisami, w podanej kolejności (najpierw z pamięci, reszta ze Spotify).
async fn utwory(s: &Session, pam: &Pamiec, uris: &[String]) -> Wynik<Vec<Utwor>> {
    let mut mapa: HashMap<&str, Utwor> = HashMap::new();
    let (mut brak_u, mut brak_o) = (Vec::new(), Vec::new());
    let mut byly = HashSet::new();
    for u in uris {
        if !byly.insert(u.as_str()) {
            continue;
        }
        match pam.daj(u) {
            Some(x) => {
                mapa.insert(u, x);
            }
            None if u.starts_with("spotify:track:") => brak_u.push(u.clone()),
            None if u.starts_with("spotify:episode:") => brak_o.push(u.clone()),
            None => {}
        }
    }
    let mut nowe = Vec::new();
    for (lista, rodzaj) in [(brak_u, ExtensionKind::TRACK_V4), (brak_o, ExtensionKind::EPISODE_V4)] {
        if !lista.is_empty() {
            nowe.extend(opisy(s, &lista, rodzaj).await?.iter().filter_map(|(u, b)| utwor_z(u, b)));
        }
    }
    pam.dodaj(&nowe);
    let nowe: HashMap<String, Utwor> = nowe.into_iter().map(|u| (u.uri.clone(), u)).collect();
    Ok(uris.iter().filter_map(|u| mapa.get(u.as_str()).or_else(|| nowe.get(u)).cloned()).collect())
}

async fn albumy(s: &Session, uris: &[String]) -> Wynik<Vec<Album>> {
    Ok(opisy(s, uris, ExtensionKind::ALBUM_V4)
        .await?
        .iter()
        .filter_map(|(u, b)| {
            let a = md::Album::parse_from_bytes(b).ok()?;
            Some(Album {
                uri: u.clone(),
                nazwa: a.name().into(),
                wykonawca: a.artist.iter().map(|x| x.name()).collect::<Vec<_>>().join(", "),
                rok: a.date.year(),
                ile: a.disc.iter().map(|d| d.track.len()).sum(),
            })
        })
        .collect())
}

async fn wykonawcy(s: &Session, uris: &[String]) -> Wynik<Vec<Wykonawca>> {
    Ok(opisy(s, uris, ExtensionKind::ARTIST_V4)
        .await?
        .iter()
        .filter_map(|(u, b)| Some(Wykonawca { uri: u.clone(), nazwa: md::Artist::parse_from_bytes(b).ok()?.name().into() }))
        .collect())
}

/// Adresy pozycji zbioru użytkownika („collection” = polubione utwory i albumy, „artist”, „show”), od najnowszych.
async fn zbior(s: &Session, nazwa: &str) -> Wynik<Vec<String>> {
    let uzytkownik = s.username();
    let mut v: Vec<(i32, String)> = Vec::new();
    let mut znacznik = String::new();
    for _ in 0..100 {
        // PageRequest { username = 1, set = 2, pagination_token = 3, limit = 4 }
        let mut tresc = Vec::new();
        {
            let mut o = protobuf::CodedOutputStream::vec(&mut tresc);
            let zapis = |o: &mut protobuf::CodedOutputStream| -> protobuf::Result<()> {
                o.write_string(1, &uzytkownik)?;
                o.write_string(2, nazwa)?;
                if !znacznik.is_empty() {
                    o.write_string(3, &znacznik)?;
                }
                o.write_int32(4, 300)?;
                o.flush()
            };
            zapis(&mut o).map_err(|e| e.to_string())?;
        }
        const TYP: HeaderValue = HeaderValue::from_static("application/vnd.collection-v2.spotify.proto");
        let mut h = HeaderMap::new();
        h.insert(header::CONTENT_TYPE, TYP);
        h.insert(header::ACCEPT, TYP);
        let b = s.spclient().request(&Method::POST, "/collection/v2/paging", Some(h), Some(&tresc)).await.map_err(|e| e.to_string())?;
        let (poz, dalej) = strona_zbioru(&b).map_err(|e| e.to_string())?;
        v.extend(poz);
        if dalej.is_empty() || dalej == znacznik {
            break;
        }
        znacznik = dalej;
    }
    v.sort_by_key(|(t, _)| std::cmp::Reverse(*t)); // sortowanie stabilne
    Ok(v.into_iter().map(|(_, u)| u).collect())
}

/// PageResponse { items = 1: CollectionItem { uri = 1, added_at = 2, is_removed = 3 }, next_page_token = 2 }
fn strona_zbioru(b: &[u8]) -> protobuf::Result<(Vec<(i32, String)>, String)> {
    use protobuf::rt::WireType;
    let mut is = protobuf::CodedInputStream::from_bytes(b);
    let (mut v, mut dalej) = (Vec::new(), String::new());
    while let Some(t) = is.read_raw_tag_or_eof()? {
        match t {
            10 => {
                let dane = is.read_bytes()?;
                let mut j = protobuf::CodedInputStream::from_bytes(&dane);
                let (mut uri, mut kiedy, mut usuniety) = (String::new(), 0, false);
                while let Some(t) = j.read_raw_tag_or_eof()? {
                    match t {
                        10 => uri = j.read_string()?,
                        16 => kiedy = j.read_int32()?,
                        24 => usuniety = j.read_bool()?,
                        t => protobuf::rt::skip_field_for_tag(t, &mut j)?,
                    }
                }
                if !uri.is_empty() && !usuniety {
                    v.push((kiedy, uri));
                }
            }
            18 => dalej = is.read_string()?,
            t => {
                debug_assert!(WireType::new(t & 7).is_some());
                protobuf::rt::skip_field_for_tag(t, &mut is)?
            }
        }
    }
    Ok((v, dalej))
}

/// Playlisty użytkownika razem z nazwami folderów.
async fn playlisty(s: &Session) -> Wynik<Vec<Lista>> {
    const STRONA: usize = 500;
    let mut v = Vec::new();
    let mut foldery: Vec<String> = Vec::new();
    let mut od = 0;
    loop {
        let b = s.spclient().get_rootlist(od, Some(STRONA)).await.map_err(|e| e.to_string())?;
        let m = SelectedListContent::parse_from_bytes(&b).map_err(|e| e.to_string())?;
        let n = m.contents.items.len();
        for (i, it) in m.contents.items.iter().enumerate() {
            let uri = it.uri();
            if let Some(r) = uri.strip_prefix("spotify:start-group:") {
                foldery.push(nazwa_folderu(r));
            } else if uri.starts_with("spotify:end-group:") {
                foldery.pop();
            } else if uri.starts_with("spotify:playlist:") {
                let meta = m.contents.meta_items.get(i);
                v.push(Lista {
                    uri: uri.to_string(),
                    nazwa: meta.and_then(|m| m.attributes.as_ref()).map(|a| a.name().to_string()).unwrap_or_default(),
                    ile: meta.map(|m| m.length()).unwrap_or(0),
                    rodzaj: "playlista",
                    folder: foldery.join(" / "),
                });
            }
        }
        od += n;
        if n == 0 || od >= m.length().max(0) as usize {
            return Ok(v);
        }
    }
}

/// „id:Nazwa+folderu” → „Nazwa folderu”
fn nazwa_folderu(r: &str) -> String {
    let r = r.split_once(':').map(|x| x.1).unwrap_or("");
    let mut o = Vec::new();
    let b = r.as_bytes();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => o.push(b' '),
            b'%' if i + 2 < b.len() && b[i + 1].is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() => {
                o.push(u8::from_str_radix(&r[i + 1..i + 3], 16).unwrap_or(b'?'));
                i += 2;
            }
            x => o.push(x),
        }
        i += 1;
    }
    String::from_utf8_lossy(&o).into_owned()
}

async fn adresy_kontekstu(s: &Session, uri: &str) -> Wynik<Vec<String>> {
    let c = s.spclient().get_context(uri).await.map_err(|e| e.to_string())?;
    Ok(c.pages.iter().flat_map(|p| p.tracks.iter().filter_map(|t| t.uri.clone())).collect())
}

/// Spis całej biblioteki: polubione, albumy, wykonawcy, odcinki, podcasty, playlisty. Każda część osobno – błąd jednej
/// nie zabiera reszty.
pub async fn biblioteka(s: &Session) -> Wynik<Biblioteka> {
    let kto = s.username();
    let odc = format!("spotify:user:{kto}:collection:your-episodes");
    let (pl, zb, wy, po, od) = futures_util::join!(playlisty(s), zbior(s, "collection"), zbior(s, "artist"), zbior(s, "show"), adresy_kontekstu(s, &odc));
    let mut b = Biblioteka::default();
    let mut blad = |co: &str, e: String| {
        log::warn!("Spotify: {co}: {e}");
        b.braki.push(co.to_string());
    };
    let zb = zb.map_err(|e| blad("polubione utwory i albumy", e)).ok();
    let wy = wy.map_err(|e| blad("wykonawcy", e)).unwrap_or_default();
    let po = po.map_err(|e| blad("podcasty", e)).unwrap_or_default();
    let od = od.map_err(|e| log::info!("Spotify: odcinki: {e}")).unwrap_or_default();
    let pl = match pl {
        Ok(v) => v,
        Err(e) if zb.is_none() => return Err(e),
        Err(e) => {
            blad("playlisty", e);
            Vec::new()
        }
    };
    let zb = zb.unwrap_or_default();
    let ile_pol = zb.iter().filter(|u| u.starts_with("spotify:track:")).count();
    let adr_alb: Vec<String> = zb.into_iter().filter(|u| u.starts_with("spotify:album:")).collect();
    let (al, ar, sh) = futures_util::join!(albumy(s, &adr_alb), wykonawcy(s, &wy), opisy(s, &po, ExtensionKind::SHOW_V4));
    b.albumy = al.unwrap_or_default();
    b.wykonawcy = ar.unwrap_or_default();
    let mut dodaj = |uri: &str, nazwa: &str, ile: usize, rodzaj| b.listy.push(Lista { uri: uri.into(), nazwa: nazwa.into(), ile: ile as i32, rodzaj, folder: String::new() });
    dodaj(POLUBIONE, "Polubione utwory", ile_pol, "polubione");
    dodaj(ALBUMY, "Albumy", b.albumy.len(), "albumy");
    dodaj(WYKONAWCY, "Wykonawcy", b.wykonawcy.len(), "wykonawcy");
    if !od.is_empty() {
        dodaj(ODCINKI, "Zapisane odcinki", od.len(), "odcinki");
    }
    for (uri, d) in sh.unwrap_or_default() {
        if let Ok(p) = md::Show::parse_from_bytes(&d) {
            dodaj(&uri, p.name(), p.episode.len(), "podcast");
        }
    }
    b.listy.extend(pl);
    Ok(b)
}

/// Zawartość: playlista, polubione, odcinki, podcast, album, wykonawca (najpopularniejsze utwory + dyskografia).
pub async fn tresc(s: &Session, pam: &Pamiec, uri: &str) -> Wynik<Tresc> {
    let kto = s.username();
    if uri == POLUBIONE {
        let adr: Vec<String> = match zbior(s, "collection").await {
            Ok(v) => v.into_iter().filter(|u| u.starts_with("spotify:track:")).collect(),
            Err(e) => {
                log::info!("Spotify: zbiór polubionych: {e} – pytam o kontekst");
                adresy_kontekstu(s, &format!("spotify:user:{kto}:collection")).await?
            }
        };
        return Ok(Tresc { tytul: "Polubione utwory".into(), utwory: utwory(s, pam, &adr).await?, ..Default::default() });
    }
    if uri == ODCINKI {
        let adr = adresy_kontekstu(s, &format!("spotify:user:{kto}:collection:your-episodes")).await?;
        return Ok(Tresc { tytul: "Zapisane odcinki".into(), utwory: utwory(s, pam, &adr).await?, ..Default::default() });
    }
    let jeden = [uri.to_string()];
    match uri.split(':').nth(1) {
        Some("playlist") => {
            let id = SpotifyUri::from_uri(uri).map_err(|e| e.to_string())?;
            let p = Playlist::get(s, &id).await.map_err(|e| e.to_string())?;
            let adr: Vec<String> = p.tracks().filter_map(|t| t.to_uri().ok()).collect();
            Ok(Tresc { tytul: p.name().to_string(), utwory: utwory(s, pam, &adr).await?, ..Default::default() })
        }
        Some("album") => {
            let o = opisy(s, &jeden, ExtensionKind::ALBUM_V4).await?;
            let a = md::Album::parse_from_bytes(&o.first().ok_or("album niedostępny")?.1).map_err(|e| e.to_string())?;
            let adr: Vec<String> = a.disc.iter().flat_map(|d| &d.track).filter_map(|t| uri_z_gid("track", t.gid())).collect();
            let kto = a.artist.iter().map(|x| x.name()).collect::<Vec<_>>().join(", ");
            let tytul = if a.date.year() > 0 { format!("{} – {} ({})", kto, a.name(), a.date.year()) } else { format!("{} – {}", kto, a.name()) };
            Ok(Tresc { tytul, utwory: utwory(s, pam, &adr).await?, ..Default::default() })
        }
        Some("artist") => {
            let o = opisy(s, &jeden, ExtensionKind::ARTIST_V4).await?;
            let a = md::Artist::parse_from_bytes(&o.first().ok_or("wykonawca niedostępny")?.1).map_err(|e| e.to_string())?;
            let (top, dysk) = futures_util::join!(top_utwory(s, pam, &a), dyskografia(s, &a));
            Ok(Tresc { tytul: a.name().into(), utwory: top?, albumy: dysk? })
        }
        Some("show") => {
            let o = opisy(s, &jeden, ExtensionKind::SHOW_V4).await?;
            let p = md::Show::parse_from_bytes(&o.first().ok_or("podcast niedostępny")?.1).map_err(|e| e.to_string())?;
            let adr: Vec<String> = p.episode.iter().filter_map(|e| uri_z_gid("episode", e.gid())).take(500).collect();
            Ok(Tresc { tytul: p.name().into(), utwory: utwory(s, pam, &adr).await?, ..Default::default() })
        }
        _ => Err(format!("nieobsługiwana lista: {uri}")),
    }
}

async fn top_utwory(s: &Session, pam: &Pamiec, a: &md::Artist) -> Wynik<Vec<Utwor>> {
    let kraj = s.country();
    let t = a.top_track.iter().find(|t| t.country() == kraj).or_else(|| a.top_track.first());
    let adr: Vec<String> = t.map(|t| t.track.iter().filter_map(|x| uri_z_gid("track", x.gid())).collect()).unwrap_or_default();
    utwory(s, pam, &adr).await
}

/// Albumy, single i EP wykonawcy (od najnowszych, bez składanek, do 150).
async fn dyskografia(s: &Session, a: &md::Artist) -> Wynik<Vec<Album>> {
    let adr: Vec<String> = a
        .album_group
        .iter()
        .chain(&a.single_group)
        .filter_map(|g| uri_z_gid("album", g.album.first()?.gid()))
        .take(150)
        .collect();
    let mut v = albumy(s, &adr).await?;
    v.sort_by_key(|x| std::cmp::Reverse(x.rok));
    Ok(v)
}

/// Szukanie w całym katalogu Spotify. Spotify oddaje utwory (po 20 najtrafniejszych na zapytanie); albumy i wykonawców
/// bierzemy z tych utworów, a gdy zapytanie to nazwa wykonawcy – także jego najpopularniejsze utwory i dyskografię.
pub async fn katalog(s: &Session, pam: &Pamiec, q: &str) -> Wynik<Wyniki> {
    let uq = uprosc(q);
    if uq.is_empty() {
        return Ok(Wyniki::default());
    }
    let adresy: Vec<String> = [q.to_string(), format!("{q} album"), format!("{q} artist")].iter().map(|f| format!("spotify:search:{}", do_adresu(f))).collect();
    let (a, b, c) = futures_util::join!(adresy_kontekstu(s, &adresy[0]), adresy_kontekstu(s, &adresy[1]), adresy_kontekstu(s, &adresy[2]));
    let glowne = a?;
    let ile_glownych = glowne.len();
    let mut adr = glowne;
    let mut byly: HashSet<String> = adr.iter().cloned().collect();
    for u in b.unwrap_or_default().into_iter().chain(c.unwrap_or_default()) {
        if byly.insert(u.clone()) {
            adr.push(u);
        }
    }
    let glowne: HashSet<&String> = adr.iter().take(ile_glownych).collect();
    let znalezione = utwory(s, pam, &adr).await?;
    // z dodatkowych zapytań zostają tylko utwory, które naprawdę pasują do zapytania
    let pasuje = |u: &Utwor| szuk_utwor(&uq, u) > 0;
    let znalezione: Vec<Utwor> = znalezione.into_iter().filter(|u| glowne.contains(&u.uri) || pasuje(u)).collect();

    // wykonawcy: wg zgodności nazwy, potem wg liczby trafień
    let mut licz: HashMap<String, (Odn, usize, usize)> = HashMap::new();
    for (i, u) in znalezione.iter().enumerate() {
        for w in u.wykonawcy.iter().filter(|w| w.uri.starts_with("spotify:artist:")) {
            licz.entry(w.uri.clone()).or_insert_with(|| (w.clone(), 0, i)).1 += 1;
        }
    }
    let mut wyk: Vec<(u32, usize, usize, Odn)> = licz.into_values().map(|(o, n, i)| (ocena(&uq, &uprosc(&o.nazwa)), n, i, o)).collect();
    wyk.sort_by_key(|(oc, n, i, _)| (std::cmp::Reverse(*oc), std::cmp::Reverse(*n), *i));

    // zapytanie o wykonawcę → jego utwory i dyskografia
    let mut top = Vec::new();
    let mut dysk = Vec::new();
    if let Some((_, _, _, w)) = wyk.first().filter(|w| w.0 >= 80) {
        if let Ok(t) = tresc(s, pam, &w.uri).await {
            (top, dysk) = (t.utwory, t.albumy);
        }
    }

    // albumy z utworów, z opisami (rok, liczba utworów)
    let mut byly = HashSet::new();
    let adr_alb: Vec<String> =
        znalezione.iter().map(|u| u.album.uri.clone()).filter(|u| u.starts_with("spotify:album:") && byly.insert(u.clone())).collect();
    let mut alb = albumy(s, &adr_alb).await.unwrap_or_default();
    alb.extend(dysk.into_iter().filter(|a| byly.insert(a.uri.clone())));
    let mut alb: Vec<(u32, Album)> = alb.into_iter().map(|a| (szuk_album(&uq, &a), a)).collect();
    alb.sort_by_key(|(o, _)| std::cmp::Reverse(*o)); // stabilnie: w grupie zostaje kolejność trafności

    let mut byly = HashSet::new();
    let pierwszy_to_wykonawca = !top.is_empty() && wyk.first().is_some_and(|w| w.0 == 100);
    let kolejno: Vec<Utwor> = if pierwszy_to_wykonawca { top.into_iter().chain(znalezione).collect() } else { znalezione.into_iter().chain(top).collect() };
    Ok(Wyniki {
        utwory: kolejno.into_iter().filter(|u| byly.insert(u.uri.clone())).collect(),
        albumy: alb.into_iter().map(|x| x.1).collect(),
        wykonawcy: wyk.into_iter().map(|(_, _, _, o)| Wykonawca { uri: o.uri, nazwa: o.nazwa }).collect(),
    })
}

/// Zgodność utworu z zapytaniem (po [`uprosc`]): tytuł > wykonawca > album > słowa rozrzucone po wszystkich polach.
pub(crate) fn szuk_utwor(q: &str, u: &Utwor) -> u32 {
    let (t, w, a) = (uprosc(&u.tytul), uprosc(&u.wykonawca()), uprosc(&u.album.nazwa));
    let o = (ocena(q, &t) * 10).max(ocena(q, &w) * 8).max(ocena(q, &a) * 6);
    if o > 0 {
        o
    } else {
        ocena(q, &format!("{t} {w} {a}"))
    }
}

pub(crate) fn szuk_album(q: &str, a: &Album) -> u32 {
    let (n, w) = (uprosc(&a.nazwa), uprosc(&a.wykonawca));
    let o = (ocena(q, &n) * 10).max(ocena(q, &w) * 8);
    if o > 0 {
        o
    } else {
        ocena(q, &format!("{n} {w}"))
    }
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn folder_ma_nazwe() {
        assert_eq!(nazwa_folderu("abc123:Moje+listy"), "Moje listy");
        assert_eq!(nazwa_folderu("abc123:%C5%BB%C3%B3%C5%82w"), "Żółw");
        assert_eq!(nazwa_folderu("abc123"), "");
        assert_eq!(nazwa_folderu("x:100%"), "100%");
        assert_eq!(nazwa_folderu("x:%zż"), "%zż");
    }

    #[test]
    fn strona_zbioru_pomija_usuniete() {
        let poz = |uri: &str, kiedy: i32, usun: bool| {
            let mut v = Vec::new();
            let mut o = protobuf::CodedOutputStream::vec(&mut v);
            o.write_string(1, uri).unwrap();
            o.write_int32(2, kiedy).unwrap();
            if usun {
                o.write_bool(3, true).unwrap();
            }
            o.write_string(4, "spotify:inne").unwrap(); // nieznane pole
            o.flush().unwrap();
            drop(o);
            v
        };
        let mut b = Vec::new();
        {
            let mut o = protobuf::CodedOutputStream::vec(&mut b);
            o.write_bytes(1, &poz("spotify:track:a", 5, false)).unwrap();
            o.write_bytes(1, &poz("spotify:track:b", 6, true)).unwrap();
            o.write_string(2, "dalej").unwrap();
            o.write_string(3, "sync").unwrap();
            o.flush().unwrap();
        }
        let (v, d) = strona_zbioru(&b).unwrap();
        assert_eq!(v, vec![(5, "spotify:track:a".to_string())]);
        assert_eq!(d, "dalej");
        assert!(strona_zbioru(&[0xff, 0xff]).is_err());
    }
}
