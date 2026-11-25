//! Próba na żywo (Windows, zapisane logowanie Szkła). Wypisuje liczby i czasy – bez tytułów z biblioteki i bez tokenów.
//! `cargo run -p szklo-spotify --example proba`
use librespot_core::{cache::Cache, session::Session, SessionConfig};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use szklo_spotify::Kontroler;

async fn czekaj(k: &Kontroler, co: &str, r: Value) -> Value {
    let t = Instant::now();
    k.rozkaz(&r);
    loop {
        tokio::time::sleep(Duration::from_millis(20)).await;
        let w: Value = serde_json::from_str(&k.widok_json()).unwrap();
        if w["wczytywanie"] == false || t.elapsed() > Duration::from_secs(120) {
            let n = |p: &str| w[p].as_array().map(|a| a.len()).unwrap_or(0);
            println!("{co}: {} utworów, {} albumów, {} wykonawców, {:.2} s, wstecz={} {}", n("utwory"), n("albumy"), n("wykonawcy"), t.elapsed().as_secs_f32(), w["wstecz"], w["info"]);
            return w;
        }
    }
}

#[tokio::main]
async fn main() {
    let d = std::path::PathBuf::from(std::env::var("APPDATA").unwrap()).join("Szklo").join("spotify");
    let cr = Cache::new(Some(d), None, None, None).unwrap().credentials().expect("brak logowania w Szkle");
    let s = Session::new(SessionConfig { client_id: "65b708073fc0480ea92a077233ca87bd".into(), ..Default::default() }, None);
    s.connect(cr, false).await.expect("połączenie");
    let k = Kontroler::nowy(None);
    let t = Instant::now();
    k.polacz(s);
    let spis = loop {
        tokio::time::sleep(Duration::from_millis(50)).await;
        let v: Value = serde_json::from_str(&k.spis_json()).unwrap();
        if v["stan"] != "wczytywanie" || t.elapsed() > Duration::from_secs(60) {
            break v;
        }
    };
    let listy = spis["listy"].as_array().unwrap();
    println!("spis: {} {} w {:.2} s", spis["stan"], spis["info"], t.elapsed().as_secs_f32());
    for r in ["polubione", "albumy", "wykonawcy", "odcinki", "podcast", "playlista"] {
        let x: Vec<_> = listy.iter().filter(|l| l["rodzaj"] == r).collect();
        println!("  {r}: {} list, pozycji {}, w folderach {}, bez nazwy {}", x.len(), x.iter().map(|l| l["ile"].as_i64().unwrap_or(0)).sum::<i64>(), x.iter().filter(|l| l["folder"] != "").count(), x.iter().filter(|l| l["nazwa"] == "").count());
    }
    let pierwsza = |r: &str| listy.iter().filter(|l| l["rodzaj"] == r).max_by_key(|l| l["ile"].as_i64()).map(|l| l["uri"].clone()).unwrap_or_default();
    for r in ["polubione", "albumy", "wykonawcy", "odcinki", "podcast", "playlista"] {
        czekaj(&k, &format!("otwórz {r}"), json!({"c":"otworz","uri":pierwsza(r)})).await;
    }
    czekaj(&k, "otwórz polubione (2. raz)", json!({"c":"otworz","uri":"spotify:collection"})).await;
    czekaj(&k, "szukaj w zaznaczonej „love”", json!({"c":"szukaj","q":"love","zakres":"lista"})).await;
    czekaj(&k, "szukaj w bibliotece „love”", json!({"c":"szukaj","q":"love","zakres":"biblioteka"})).await;
    czekaj(&k, "szukaj w bibliotece (2. raz)", json!({"c":"szukaj","q":"live","zakres":"biblioteka"})).await;
    // zapytania publiczne – tu tytuły można pokazać
    for q in ["boards of canada", "geogaddi", "chopin nocturne", "kult", "zażółć", "xqzvwkj"] {
        let w = czekaj(&k, &format!("katalog „{q}”"), json!({"c":"szukaj","q":q,"zakres":"katalog"})).await;
        let naz = |p: &str, f: &str| w[p].as_array().unwrap().iter().take(4).map(|x| x[f].as_str().unwrap_or("").to_string()).collect::<Vec<_>>();
        println!("   utwory {:?}\n   albumy {:?}\n   wykonawcy {:?}", naz("utwory", "tytul"), naz("albumy", "nazwa"), naz("wykonawcy", "nazwa"));
    }
    let w = czekaj(&k, "katalog „kult”", json!({"c":"szukaj","q":"kult","zakres":"katalog"})).await;
    czekaj(&k, "strona wykonawcy", json!({"c":"otworz","uri":w["wykonawcy"][0]["uri"]})).await;
    czekaj(&k, "album", json!({"c":"otworz","uri":w["albumy"][0]["uri"]})).await;
    czekaj(&k, "wstecz", json!({"c":"wstecz"})).await;
    czekaj(&k, "wstecz", json!({"c":"wstecz"})).await;
}
