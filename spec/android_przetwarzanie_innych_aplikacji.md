# Android: jak legalnie przetwarzać dźwięk innych aplikacji (Spotify) – raport (Faza 9, pkt 3)

Data: 2026-11-21. Status: **[STOP] – do decyzji Pawła** (rekomendacja w §7).
Źródła: dokumentacja Android SDK (`android.media.audiofx`, `AudioPlaybackCaptureConfiguration`,
`MediaProjection`, zachowanie usług pierwszoplanowych i rozgłoszeń), kod AOSP (efekt DynamicsProcessing),
praktyka aplikacji-korektorów. Punkty oznaczone **(sprawdzić na telefonie)** zależą od wersji
aplikacji/producenta i trzeba je potwierdzić pomiarem – sposób w §8.

---

## 1. Teza w jednym akapicie

Bez roota Android **nie pozwala wstawić własnego kodu DSP w strumień innej aplikacji**. Można jedynie
(a) dołączyć do jej sesji audio **wbudowane efekty systemowe** (`Equalizer`, `BassBoost`, `LoudnessEnhancer`,
`DynamicsProcessing`…) i sterować ich parametrami, albo (b) **przechwycić kopię** dźwięku
(`AudioPlaybackCapture`), której nie da się podmienić za oryginał. Dlatego pełny rdzeń Szkła (Rust) działa
na Androidzie tylko dla dźwięku, który odtwarzamy sami; dla Spotify mamy przybliżenie przez
`DynamicsProcessing`.

## 2. Sesje efektów i rozgłoszenia

- Każdy `AudioTrack`/`MediaPlayer` ma **numer sesji audio**. Efekt `android.media.audiofx.*` tworzony z tym
  numerem trafia do łańcucha efektów tej sesji w `audioserver` (AudioFlinger), przed mikserem.
- Odtwarzacze „współpracujące z korektorami” rozgłaszają:
  - `AudioEffect.ACTION_OPEN_AUDIO_EFFECT_CONTROL_SESSION` z `EXTRA_AUDIO_SESSION`, `EXTRA_PACKAGE_NAME`,
    `EXTRA_CONTENT_TYPE` – na starcie odtwarzania,
  - `AudioEffect.ACTION_CLOSE_AUDIO_EFFECT_CONTROL_SESSION` – na końcu,
  - przycisk „Korektor” w odtwarzaczu uruchamia aktywność z `ACTION_DISPLAY_AUDIO_EFFECT_CONTROL_PANEL`
    (z numerem sesji) – Szkło deklaruje ten filtr intencji.
- Spotify wysyła te rozgłoszenia (ustawienie „Korektor”) **(sprawdzić na telefonie – zachowanie zależy od wersji)**.
  Wiele aplikacji (przeglądarki, część odtwarzaczy wideo) ich nie wysyła – wtedy ta ścieżka nie działa.
- Uprawnienie: `MODIFY_AUDIO_SETTINGS` (zwykłe, bez zgody użytkownika).
- **Android 8+ (ograniczenia rozgłoszeń niejawnych):** rozgłoszenia OPEN/CLOSE są niejawne, a z naszej wiedzy nie
  są na liście wyjątków – odbiornik z manifestu ich zwykle nie dostanie. Działa odbiornik **rejestrowany
  dynamicznie** w żyjącym procesie → potrzebna usługa pierwszoplanowa (tak robi `GlobalEffectsService`).
  Odbiornik z manifestu zostawiamy dla nadawców adresujących jawnie.
- Priorytet efektu: przy kilku aplikacjach z tym samym typem efektu na jednej sesji kontrolę ma wyższy
  priorytet (`hasControl()`, `setControlStatusListener`). Inne typy efektów współistnieją.

### Sesja 0 (globalny mikser)

- Tworzenie efektów na sesji 0 („output mix”) jest **oznaczone w dokumentacji jako przestarzałe**.
  Na części urządzeń nadal działa (globalny korektor bez współpracy odtwarzacza), na części jest blokowane
  lub działa niestabilnie (zależne od producenta i wersji). Nie da się na tym oprzeć produktu; ewentualnie
  jako opcja eksperymentalna z jasnym opisem. W obecnym kodzie **nie używamy** sesji 0.

## 3. DynamicsProcessing (API 28+) – co daje, czego nie

Etapy (każdy kanał osobno): wzmocnienie wejścia → **preEQ** (N pasm) → **MBC** – kompresor wielopasmowy
(atak, zwolnienie, ratio, próg, kolano, bramka szumów, ekspander, pre/post gain na pasmo) → **postEQ** →
**limiter** (atak, zwolnienie, ratio, próg, post gain, grupy łączenia kanałów). Wariant
„rozdzielczość częstotliwościowa” albo „czasowa”, preferowana długość ramki. Liczy go audioserver
(implementacja programowa AOSP), nie nasz proces.

Mapowanie modułów Szkła (plik `app-android/.../audio/DpMapping.kt`, wartości wstępne):

| Grupa | Moduł | W DynamicsProcessing |
|---|---|---|
| Naprawa | Declipper | **brak** (wymaga rekonstrukcji sygnału) |
| | Naprawa MP3 | postEQ: podbicie 8–20 kHz („powietrze”) – namiastka |
| | Szum i brum | bramka/ekspander w najwyższym paśmie MBC – namiastka |
| | Dehummer | ujęcie pasma < 60 Hz (brak filtra grzebieniowego) |
| | Dekwantyzer | **brak** |
| | Bramka szumów | bramka + ekspander MBC (wszystkie pasma) |
| Dynamika | AGC, Multiband 1/2, Kompresor | MBC: ratio/próg/zwolnienie łączone z mocy modułów, częściowa kompensacja |
| | Punch | dłuższy atak MBC (transjenty przechodzą) – brak ekspansji w górę |
| Bas | Głęboki bas, Bass Boost | preEQ: podbicie < 150 Hz |
| | Bas harmoniczny | **brak harmonicznych** – zastępczo podbicie 60–150 Hz |
| Barwa | Korektor, Auto EQ, Balans widma | preEQ – krzywe statyczne (Auto EQ nie jest adaptacyjny) |
| Stereo | Szerokość, Rotacja fazy | **brak** (brak M/S i all-passów) |
| Wyjście | Clipper HD, Prosty clipper, Pre-limiter | wzmocnienie wejścia „dociskające” limiter |
| | Hard Limit | limiter DP, próg = sufit − 0,3 dB (**nie true-peak**, bez oversamplingu) |

Wniosek: w trybie „inne aplikacje” dostępne jest ok. 60% „charakteru” (EQ, dynamika wielopasmowa, limiter),
bez modułów, które wyróżniają Szkło (declipper, delossifier, clipper z kasowaniem zniekształceń,
psychoakustyczny bas, stereo, true-peak −1 dBTP dla Bluetooth). Brak mierników z DSP (DP ich nie udostępnia).

## 4. AudioPlaybackCapture (API 29+) – dlaczego się nie nadaje do przetwarzania

- `AudioRecord` z `AudioPlaybackCaptureConfiguration` + **`MediaProjection`** – wymagana zgoda użytkownika
  w systemowym oknie; od Androida 14 zgoda jest **na każdą sesję** przechwytywania, a usługa musi mieć typ FGS
  `mediaProjection`; system pokazuje wskaźnik udostępniania/nagrywania.
- Przechwytywane są tylko użycia `USAGE_MEDIA`, `USAGE_GAME`, `USAGE_UNKNOWN`.
- **Opt-out aplikacji:** atrybut `android:allowAudioPlaybackCapture="false"` albo
  `AudioAttributes`/`AudioManager.setAllowedCapturePolicy(ALLOW_CAPTURE_BY_SYSTEM | ALLOW_CAPTURE_BY_NONE)`.
  Aplikacje z targetSdk < 29 są domyślnie wyłączone z przechwytywania. Usługi strumieniowe często blokują
  przechwytywanie (DRM/umowy licencyjne) **(sprawdzić na telefonie dla Spotify)**.
- **Podwójny dźwięk:** przechwytywanie tworzy **kopię**; oryginał gra dalej. Aplikacja bez uprawnień
  systemowych nie może wyciszyć strumienia innej aplikacji, więc nasza przetworzona kopia nałożyłaby się
  na oryginał (z opóźnieniem ~50–150 ms = efekt grzebieniowy). Wyciszenie całego systemu wycisza też nas.
- Dodatkowo: opóźnienie (bufor przechwytywania + bufor odtwarzania), ryzyko polityk Google Play (przechwytywanie
  cudzych treści), zużycie baterii (dwa strumienie + nasz DSP).
- Zastosowanie sensowne: **tylko pomiar/wizualizacja** (np. miernik LUFS dla Spotify), nie przetwarzanie.

## 5. Ograniczenia nowszych wersji Androida (dotyczące naszych mechanizmów)

| Wersja | Zmiana | Skutek dla Szkła |
|---|---|---|
| 8 (API 26) | ograniczenia rozgłoszeń niejawnych dla odbiorników z manifestu; limity usług w tle | odbiornik sesji musi być dynamiczny w usłudze pierwszoplanowej |
| 9 (API 28) | `DynamicsProcessing` | minimum dla trybu „inne aplikacje” (na 8.x tylko Equalizer/BassBoost/LoudnessEnhancer – nie zaimplementowano) |
| 10 (API 29) | AudioPlaybackCapture; typy FGS w manifeście | j.w. §4; `foregroundServiceType` |
| 12 (API 31) | zakaz startu FGS z tła | usługę startujemy wyłącznie z UI (i z panelu korektora Spotify); `START_NOT_STICKY` |
| 13 (API 33) | uprawnienie `POST_NOTIFICATIONS` | prośba przy pierwszym starcie odtwarzania/trybu; odmowa nie blokuje działania |
| 14 (API 34) | obowiązkowy typ FGS + uprawnienie per typ; zgoda MediaProjection na każdą sesję | odtwarzacz: `mediaPlayback`; most efektów: `specialUse` (Google Play wymaga uzasadnienia w deklaracji) |
| 15 (API 35) | limity czasu dla FGS `dataSync`/`mediaProcessing`; edge-to-edge wymuszone dla targetSdk 35 | nie dotyczy naszych typów; UI jest edge-to-edge |

Inne praktyczne ograniczenia:
- Efekty producenta (np. Dolby, „Adapt Sound”) działają równolegle i mogą się sumować – zalecić ich wyłączenie.
- **Odtwarzanie odciążone** (compressed offload, `AudioTrack` offload API 29+): efekt, który nie obsługuje offloadu,
  wymusza zwykłe PCM w procesorze aplikacyjnym – większe zużycie baterii po stronie odtwarzacza.
- Bluetooth: efekty sesji działają w audioserverze przed koderem A2DP (SBC/AAC/LDAC) – sprzętowy offload kodeka ich nie omija.

## 6. Bateria

| Ścieżka | Koszt (szacunek, do pomiaru) |
|---|---|
| Lokalny odtwarzacz + pełny rdzeń Rust | dekodowanie (sprzętowe/programowe) + nasz DSP: cel ≤ 25% jednego rdzenia średniego telefonu (sekcja 7 scenariusza); wyklucza offload dekodowania – telefon nie zasypia procesora podczas odtwarzania |
| Most `DynamicsProcessing` | DSP w audioserverze: kilka % jednego rdzenia; nasz proces prawie bezczynny (usługa + powiadomienie, brak pętli); może wyłączyć offload odtwarzacza |
| AudioPlaybackCapture + własne przetwarzanie | najdroższe: przechwytywanie + DSP + drugi strumień wyjściowy; do tego problemy z §4 |

Pomiar (Faza 9): `adb shell dumpsys batterystats --reset` → 30 min odtwarzania → `dumpsys batterystats`,
oraz `adb shell top -p $(pidof audioserver),$(pidof pl.szklo)`; porównanie z tym samym utworem bez efektów.

## 7. Rekomendacja

1. **Pełna jakość = własny odtwarzacz** (zrobione: `LocalPlayerService`): pliki z telefonu (MP3/FLAC/…) przez cały
   rdzeń Rust – identyczne brzmienie jak na PC, z miernikami. Docelowo uzupełnić o bibliotekę/kolejkę i MediaSession.
2. **Spotify i inne aplikacje = przybliżenie przez `DynamicsProcessing` na sesjach z rozgłoszeń** (zrobione:
   `GlobalEffectsService` + `GlobalEffectsBridge`). W UI uczciwie oznaczone jako „przybliżenie”. Do zrobienia:
   strojenie `DpMapping` odsłuchem (A/B: Szkło-lokalnie vs Szkło-DP na tym samym utworze), ewentualnie
   osobne presety dla trybu DP.
3. **Nie robić** przetwarzania przez AudioPlaybackCapture (podwójny dźwięk, opt-out, zgoda na każdą sesję,
   polityki Play). Ewentualnie później jako *miernik* LUFS dla innych aplikacji.
4. **Opcja eksperymentalna (do decyzji):** globalny `DynamicsProcessing` na sesji 0 dla aplikacji, które nie
   wysyłają rozgłoszeń – przestarzałe, zależne od urządzenia; tylko za wyraźnym przełącznikiem „eksperymentalne”.
5. **Opcja z rootem (później, do decyzji):** spakować rdzeń Rust jako bibliotekę efektu audio (C ABI efektu HAL)
   instalowaną modułem Magisk – wtedy pełny DSP Szkła działa systemowo dla wszystkich aplikacji. To nasz kod,
   więc licencyjnie bez przeszkód; wymaga roota u użytkownika.

## 8. Weryfikacja na telefonie Pawła (lista kontrolna)

- [ ] Które aplikacje wysyłają OPEN/CLOSE: `adb logcat | findstr /i "AUDIO_EFFECT_CONTROL_SESSION GlobalEffectsBridge"`
      przy starcie odtwarzania w Spotify, YouTube Music, przeglądarce.
- [ ] Czy efekt jest w łańcuchu sesji: `adb shell dumpsys media.audio_flinger` (sekcja efektów sesji – DynamicsProcessing).
- [ ] Słyszalność: przełącznik „Przetwarzanie” w menu presetów (bypass) podczas odtwarzania w Spotify.
- [ ] Konflikty z efektami producenta (Dolby/Samsung/Xiaomi) – wyłączone/włączone.
- [ ] Bateria wg §6.
- [ ] Czy Spotify blokuje AudioPlaybackCapture (tylko informacyjnie, do decyzji w pkt 3).
