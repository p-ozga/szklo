# Szkło

Domowy procesor dźwięku (PC + Android): 23 moduły (naprawa, dynamika, bas, barwa, stereo, wyjście), każdy z on/off i suwakiem
mocy 0–100%, sufit −1,0 dBTP pod słuchawki Bluetooth. Projekt filtrów: `spec/koncepcja_filtrow_v1.md`.

Na odwrocie okna (PC) jest wbudowany odtwarzacz: pliki z dysku, playlista, zegar i analizator widma, Spotify (Premium)
odtwarzane wprost przez silnik Szkła oraz opcjonalnie wizualizacja MilkDrop 3 (osobny program, pobierany przez użytkownika).

| Katalog | Co |
|---|---|
| `core/` | rdzeń DSP w Rust (f64, bez `unsafe`): prymitywy `dsp/`, moduły `modules/`, tor `graph.rs`, silnik `engine.rs` |
| `cli/` | `szklo-cli` – przetwarzanie plików WAV (pomiary, regresja) |
| `plugin/` | `szklo_vst2.dll` – wtyczka VST2 dla Equalizer APO (cały dźwięk systemu) + kanał sterowania named pipe |
| `app-pc/` | `Szklo.exe` – aplikacja (WebView2, odwracanie okna, zasobnik, silnik w aplikacji albo sterowanie wtyczką, odtwarzacz) |
| `app-android/` | aplikacja Android (Jetpack Compose) + most JNI `app-android/rust` (w tym odbiornik Spotify Connect) |
| `tools/vsthost/` | test dymny wtyczki bez Equalizer APO |
| `tools/licencje/` | generator listy cudzych elementów i licencji (`THIRD_PARTY.md`) |
| `third_party/` | dwie biblioteki z łatkami: `vst` 0.4.0 i `librespot-core` 0.8.0 (obie MIT) |
| `spec/`, `docs/` | specyfikacje i instrukcje |

## Budowa (Windows)

```powershell
cargo build --release --workspace
cargo test --release -p szklo-core
```

Wynik w `target\release\`: `Szklo.exe`, `szklo-cli.exe`, `szklo_vst2.dll`.
Instalacja w Equalizer APO: `docs/equalizer_apo.md`. Android: `app-android/README.md`.

## Skróty w aplikacji PC

Przyciski w lewym górnym rogu: zamknij (do zasobnika; całkowite wyjście – menu zasobnika → Zakończ), minimalizuj, maksymalizuj.
O – odwróć okno · spacja (trzymaj) – porównaj z oryginałem · E – włącz/wyłącz · 1–4 – presety · Ctrl+W – schowaj do zasobnika.
Suwaki: Shift = krok 10, Ctrl = krok 0,1, podwójny klik = wartość z presetu, strzałki ±1.

## Cudze elementy i znaki towarowe

Szkło korzysta z bibliotek open source, czcionek Inter i Space Grotesk (SIL OFL 1.1) i współpracuje z Equalizer APO
(Jonas Thedering, GPL-2.0 – osobny program). Pełna lista z autorami i licencjami: [THIRD_PARTY.md](THIRD_PARTY.md),
w aplikacjach: odwrót → „O programie i licencje”.

Spotify i Spotify Connect to znaki towarowe Spotify AB, VST – Steinberg Media Technologies GmbH. Szkło nie jest powiązane
z nimi ani przez nie wspierane. Odtwarzanie Spotify korzysta z librespot – nieoficjalnego klienta; wymaga konta Premium
i odbywa się na ryzyko użytkownika. MilkDrop 3 (milkdrop2077) nie jest częścią Szkła.

## Licencja

Kod Szkła: [MIT](LICENSE). Biblioteki zewnętrzne zachowują swoje licencje (patrz `THIRD_PARTY.md`).
