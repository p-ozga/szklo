# Szkło

Domowy procesor dźwięku (PC + Android): 32 moduły (naprawa, dynamika, bas, barwa, stereo, wyjście), każdy z on/off i suwakiem
mocy 0–100%, sufit −1,0 dBTP pod słuchawki Bluetooth. Projekt filtrów: `spec/koncepcja_filtrow_v1.md`.

Na odwrocie okna (PC) jest wbudowany odtwarzacz: pliki z dysku, playlista, zegar i analizator widma, biblioteka Spotify
(polubione utwory, playlisty z folderami, albumy, wykonawcy, podcasty) z szukaniem w zaznaczonej liście, w bibliotece albo
w katalogu Spotify (muzyka gra w aplikacji Spotify, Szkło ją przetwarza) oraz wbudowana wizualizacja muzyki (silnik Butterchurn, własne presety); MilkDrop 3
można nadal uruchomić jako osobny program. Wizualizacja i biblioteka Spotify są też w aplikacji na Androida.

Sześć presetów, w tym **Skupienie** i **Wyciszenie** (filtry Mniej słów, Równy strumień, Szum tła – opis i źródła:
`docs/skupienie.md`). Filtry są strojone miarą słyszalności, nie samą zmianą sygnału: `tools/sluch/`.

| Katalog | Co |
|---|---|
| `core/` | rdzeń DSP w Rust (f64, bez `unsafe`): prymitywy `dsp/`, moduły `modules/`, tor `graph.rs`, silnik `engine.rs` |
| `cli/` | `szklo-cli` – przetwarzanie plików WAV (pomiary, regresja) |
| `plugin/` | `szklo_vst2.dll` – wtyczka VST2 dla Equalizer APO (cały dźwięk systemu) + kanał sterowania named pipe |
| `app-pc/` | `Szklo.exe` – aplikacja (WebView2, odwracanie okna, zasobnik, silnik w aplikacji albo sterowanie wtyczką, odtwarzacz) |
| `app-android/` | aplikacja Android (Jetpack Compose) + most JNI `app-android/rust` (w tym odbiornik Spotify Connect) |
| `tools/vsthost/` | test dymny wtyczki bez Equalizer APO |
| `spotify/` | biblioteka Spotify i wyszukiwanie (tylko odczyt) – wspólne dla PC i Androida |
| `wspolne/wizualizacja/` | wizualizacja wspólna dla PC i Androida: silnik Butterchurn (MIT) + własne presety |
| `tools/sluch/` | badanie, czy filtry słychać: model słuchu, 15 gatunków (utwory pobiera się osobno) |
| `tools/hrtf/` | generator tablic filtra „Binauralny” z pomiarów MIT KEMAR |
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
O – odwróć okno · spacja (trzymaj) – porównaj z oryginałem · E – włącz/wyłącz · 1–6 – presety · Ctrl+W – schowaj do zasobnika.
Suwaki: Shift = krok 10, Ctrl = krok 0,1, podwójny klik = wartość z presetu, strzałki ±1.

## Autorstwo

Twórcą programu jest **Claude** – model AI firmy Anthropic, pracujący w Claude Code: projekt filtrów, kod, testy, interfejs
i dokumentacja. Rola człowieka ([p-ozga](https://github.com/p-ozga)) sprowadziła się do pomysłu.

Firma Anthropic nie jest wydawcą Szkła, nie odpowiada za nie i go nie wspiera. „Claude” i „Anthropic” to znaki Anthropic PBC.

## Cudze elementy i znaki towarowe

Szkło korzysta z bibliotek open source, czcionek Inter i Space Grotesk (SIL OFL 1.1),
pomiarów HRTF manekina KEMAR (Bill Gardner i Keith Martin, MIT Media Lab, © 1994 MIT Media Laboratory) w filtrze
„Binauralny”, silnika wizualizacji Butterchurn (Jordan Berg, MIT) i współpracuje z Equalizer APO
(Jonas Thedering, GPL-2.0 – osobny program). Pełna lista z autorami i licencjami: [THIRD_PARTY.md](THIRD_PARTY.md),
w aplikacjach: odwrót → „O programie i licencje”.

Spotify i Spotify Connect to znaki towarowe Spotify AB, VST – Steinberg Media Technologies GmbH. Szkło nie jest powiązane
z nimi ani przez nie wspierane. Biblioteka i odtwarzanie Spotify korzystają z librespot – nieoficjalnego klienta; wymagają konta Premium
i odbywają się na ryzyko użytkownika. MilkDrop 3 (milkdrop2077) nie jest częścią Szkła.

## Licencja

Kod Szkła: [MIT](LICENSE). Biblioteki zewnętrzne zachowują swoje licencje (patrz `THIRD_PARTY.md`).
