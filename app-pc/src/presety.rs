//! Presety użytkownika: %APPDATA%\Szklo\presety.json (osobno od ustawień – uszkodzenie jednego pliku nie kasuje drugiego).
//! Klucze `u_…`; nazwy presetów wbudowanych są zarezerwowane. Zapis atomowy (plik tymczasowy + zamiana).

use serde::{Deserialize, Serialize};
use szklo_core::{MODULE_COUNT, PRESETS};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    pub key: String,
    pub name: String,
    pub on: Vec<bool>,
    pub amount: Vec<f32>,
}

const MAX_NAZWA: usize = 40;

fn sciezka() -> std::path::PathBuf {
    crate::settings::dir().join("presety.json")
}

pub fn wczytaj() -> Vec<Preset> {
    let lista: Vec<Preset> = std::fs::read_to_string(sciezka()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
    lista
        .into_iter()
        .filter(|p| p.key.starts_with("u_") && !p.name.trim().is_empty() && p.on.len() == MODULE_COUNT && p.amount.len() == MODULE_COUNT)
        .map(|mut p| {
            for a in p.amount.iter_mut() {
                *a = if a.is_finite() { a.clamp(0.0, 1.0) } else { 0.0 };
            }
            p
        })
        .collect()
}

fn zapisz_plik(lista: &[Preset]) -> Result<(), String> {
    let d = crate::settings::dir();
    std::fs::create_dir_all(&d).map_err(|e| e.to_string())?;
    let t = serde_json::to_string_pretty(lista).map_err(|e| e.to_string())?;
    let tmp = d.join("presety.json.tmp");
    std::fs::write(&tmp, t).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, sciezka()).map_err(|e| e.to_string())
}

/// „Zapisz jako”. Zwraca (klucz, komunikat) albo komunikat błędu.
pub fn zapisz_jako(lista: &mut Vec<Preset>, nazwa: &str, on: &[bool], amount: &[f32]) -> Result<(String, String), String> {
    let nazwa: String = nazwa.trim().chars().take(MAX_NAZWA).collect();
    if nazwa.is_empty() {
        return Err("Podaj nazwę presetu".into());
    }
    if PRESETS.iter().any(|p| p.name_pl.to_lowercase() == nazwa.to_lowercase()) {
        return Err(format!("„{nazwa}” to preset wbudowany – wybierz inną nazwę"));
    }
    let istniejacy = lista.iter().position(|p| p.name.to_lowercase() == nazwa.to_lowercase());
    let key = match istniejacy {
        Some(i) => lista[i].key.clone(),
        None => {
            let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            format!("u_{ms:x}")
        }
    };
    let p = Preset { key: key.clone(), name: nazwa.clone(), on: on.to_vec(), amount: amount.to_vec() };
    let mut nowa = lista.clone();
    match istniejacy {
        Some(i) => nowa[i] = p,
        None => nowa.push(p),
    }
    zapisz_plik(&nowa).map_err(|e| format!("Nie udało się zapisać: {e}"))?;
    *lista = nowa;
    let msg = if istniejacy.is_some() { format!("Nadpisano „{nazwa}”") } else { format!("Zapisano „{nazwa}”") };
    Ok((key, msg))
}

/// „Zapisz”: nadpisuje preset użytkownika o kluczu `key`.
pub fn zapisz(lista: &mut Vec<Preset>, key: &str, on: &[bool], amount: &[f32]) -> Result<String, String> {
    let Some(i) = lista.iter().position(|p| p.key == key) else {
        return Err("Preset wbudowany – użyj „Zapisz jako…”".into());
    };
    let mut nowa = lista.clone();
    nowa[i].on = on.to_vec();
    nowa[i].amount = amount.to_vec();
    zapisz_plik(&nowa).map_err(|e| format!("Nie udało się zapisać: {e}"))?;
    let msg = format!("Zapisano „{}”", nowa[i].name);
    *lista = nowa;
    Ok(msg)
}

/// „Usuń”.
pub fn usun(lista: &mut Vec<Preset>, key: &str) -> Result<String, String> {
    let Some(i) = lista.iter().position(|p| p.key == key) else {
        return Err("Presetów wbudowanych nie można usunąć".into());
    };
    let mut nowa = lista.clone();
    let p = nowa.remove(i);
    zapisz_plik(&nowa).map_err(|e| format!("Nie udało się usunąć: {e}"))?;
    *lista = nowa;
    Ok(format!("Usunięto „{}”", p.name))
}

#[cfg(test)]
mod testy {
    use super::*;

    #[test]
    fn zapisz_jako_zapisz_usun() {
        // katalog tymczasowy zamiast %APPDATA% – test nie dotyka prawdziwych presetów
        let tmp = std::env::temp_dir().join(format!("szklo_test_presety_{}", std::process::id()));
        std::env::set_var("APPDATA", &tmp);
        let on = vec![true; MODULE_COUNT];
        let a = vec![0.5f32; MODULE_COUNT];
        let mut l = Vec::new();

        assert!(zapisz_jako(&mut l, "  ", &on, &a).is_err());
        assert!(zapisz_jako(&mut l, "codzienny", &on, &a).is_err(), "nazwa wbudowanego zajęta");

        let (k, msg) = zapisz_jako(&mut l, "Mój bas", &on, &a).unwrap();
        assert!(k.starts_with("u_") && msg.starts_with("Zapisano"));
        assert_eq!(wczytaj(), l);

        // ta sama nazwa (inna wielkość liter) = nadpisanie, bez duplikatu
        let a2 = vec![0.25f32; MODULE_COUNT];
        let (k2, msg2) = zapisz_jako(&mut l, "MÓJ BAS", &on, &a2).unwrap();
        assert_eq!(k2, k);
        assert!(msg2.starts_with("Nadpisano"));
        assert_eq!(l.len(), 1);

        let a3 = vec![0.75f32; MODULE_COUNT];
        zapisz(&mut l, &k, &on, &a3).unwrap();
        assert_eq!(wczytaj()[0].amount, a3);
        assert!(zapisz(&mut l, "codzienny", &on, &a3).is_err());

        assert!(usun(&mut l, "codzienny").is_err());
        usun(&mut l, &k).unwrap();
        assert!(wczytaj().is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }
}
