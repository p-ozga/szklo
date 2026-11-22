# Szkło – wstępna koncepcja filtrów i algorytmów DSP (v0, Opus)

Status: szkic do dopracowania. Źródła: literatura DSP, wygasłe patenty, pomiary czarnej skrzynki
(CLI 8.00: opóźnienie toru domyślnego = 188 próbek @44,1 kHz; wynik niedeterministyczny tylko ±1 LSB = dither).
Klucze plików `.sts` (format danych, interoperacyjność) mówią, *jakie* moduły i parametry istnieją – nie *jak* działają.

## 0. Założenia wspólne

- Wewnątrz f64, I/O f32. Częstotliwości próbkowania 44,1/48/96 kHz (wszystkie współczynniki liczone z `fs`, nic nie jest stałą dla 44,1).
- Każdy moduł: `latency_samples()` stała dla danego `fs` (niezależna od on/off i od suwaka), żeby przełączanie nie przesuwało czasu.
  Wyłączony moduł = czysta linia opóźniająca o swoją latencję (bit-exact z opóźnieniem).
- On/off: przenikanie 10 ms (równa moc? nie – **równa amplituda**, bo sygnały są skorelowane) między wyjściem modułu a opóźnionym wejściem.
- Suwak „moc” `a ∈ [0,1]` wygładzany jednobiegunowo (τ = 20 ms) per próbka lub per blok 32 próbek z interpolacją liniową.
  Konwencja: `a = 0` ≈ moduł przezroczysty (o ile algorytm na to pozwala), `a = 1` = maksimum sensownego zakresu.
- Denormale: f64 + flush stanów `|x| < 1e-30 → 0` na końcu bloku (bez `unsafe` do FTZ/DAZ).
- Wspólne prymitywy (`core/src/dsp/`):
  - **Biquad TDF-II** (RBJ cookbook) – filtry stałe.
  - **SVF Simper/Cytomic (trapezoidal)** – filtry modulowane w czasie (EQ dynamiczne, auto-EQ); stabilne przy szybkich zmianach.
  - **Dopasowane (matched) filtry 2. rzędu Vicanka (2016)** dla półek/pików powyżej ~fs/8 – unikamy „ściskania” charakterystyki przy Nyquiście (problem bilinearnej).
  - **Zwrotnice Linkwitz–Riley 4. rzędu** (2× Butterworth Q=0,7071) + kompensacja fazy all-passem w strukturze drzewa
    (każde pasmo przechodzi przez all-passy pozostałych zwrotnic) → suma pasm = all-pass, płaska amplituda ±0,01 dB.
  - **Oversampling**: FIR półpasmowe liniowo-fazowe (okno Kaisera, β≈8, 2 stopnie ×2·×2), tłumienie ≥ 90 dB.
    Do pomiaru true-peak: interpolator 4× wg ITU-R BS.1770-4 (48 współczynników, 4 fazy).
  - **Detektory obwiedni**: szczytowy i RMS, jednobiegunowe `α = exp(−1/(τ·fs))`; wygładzanie w dziedzinie dB (Giannoulis, Massberg, Reiss 2012, „decoupled smooth branching”).
  - **Komputer wzmocnienia**: próg T, ratio R, kolano W (kwadratowe, Giannoulis 2012).
  - **Okno przesuwnego minimum** (kolejka monotoniczna, O(1)) – do limiterów z lookahead.
  - **STFT**: 2048 / skok 512, okno sqrt-Hann (analiza i synteza, COLA przy 75%), `rustfft` (czysty Rust – działa na Androidzie).
    Latencja 2048 − 1 + … → jeden „szyna widmowa” wspólna dla modułów widmowych w tym samym miejscu toru.
  - **Ważenie K** (BS.1770): półka wysoka +4 dB @1681 Hz + HPF RLB 38 Hz; współczynniki przeliczane dla dowolnego fs (bilinear z prewarpingiem z parametrów analogowych).

## 1. Kolejność toru (wstępna – do weryfikacji pomiarem z fazy 2)

```
WE → DC-blok (HPF 1. rz. 5 Hz)
   → [Naprawa] Declipper → (szyna STFT A: Naprawa MP3, Szum i brum) → Dehummer → Dekwantyzer → Bramka szumów
   → [Stereo]  Rotacja fazy → Szerokość
   → [Dynamika] AGC (wolne)  
   → [Barwa]   Korektor → (szyna STFT B: Auto EQ, Balans widma)
   → [Bas]     Głęboki bas → Bass Boost → Bas harmoniczny
   → [Dynamika] Multiband 2 (3 pasma, wolne) → Kompresor (szerokopasmowy) → Multiband 1 (5 pasm, szybkie) → Punch
   → [Wyjście] Pre-limiter → Clipper HD → Prosty clipper → Hard Limit (sufit −1,0 dBTP)
   → miernik WY
```

## 2. Moduły – algorytm i znaczenie suwaka

### Naprawa
1. **Declipper** – detekcja serii ≥2 próbek o |x| ≥ c·max_lokalne (histogram szczytów, okno 1 s, symetria +/−), rekonstrukcja
   interpolacją AR (Janssen–Veldhuis–Vries 1986), rząd 24, kontekst 256 próbek z obu stron, z warunkiem spójności
   |y| ≥ poziom obcięcia i znak jak oryginał (consistent declipping, Záviška i in. 2021). Po rekonstrukcji −3 dB zapasu
   (kompensowane dalej przez AGC). Suwak: czułość detekcji (margines 0,99→0,90) i maks. podniesienie szczytu (+1…+6 dB).
   Latencja: 512 próbek.
2. **Naprawa MP3** (delossifier, STFT A) – (a) wypełnianie dziur widmowych: bin > 18 dB pod medianą 1/6 okt. powyżej 600 Hz
   → szum o obwiedni mediany × siła, losowa faza, ciągłość między ramkami; (b) rozszerzenie pasma powyżej wykrytego odcięcia
   (spadek widma długoterminowego > 30 dB na < 1/6 okt.): replikacja oktawy poniżej odcięcia z nachyleniem −6 dB/okt i
   wygładzoną obwiednią; (c) redukcja pre-echa: detekcja ataku w pasmach > 2 kHz, tłumienie ramek poprzedzających.
   Suwak: siła (a), poziom (b), głębokość (c).
3. **Szum i brum** (PNR, STFT A) – estymacja szumu metodą minimum statistics (Martin 2001), wzmocnienie Wienera
   z decyzją kierowaną (Ephraim–Malah 1984) przeciw „muzycznemu szumowi”, podłoga wzmocnienia −6…−18 dB wg suwaka.
4. **Dehummer** – detekcja 50/60 Hz (energia w wąskich pasmach przez 2 s, trwałość tonalna), kaskada 8 notchy (k·f0, Q=30…60),
   głębokość wg suwaka; notche aktywne tylko przy wykrytym brumie (płynnie).
5. **Dekwantyzer** – dla cichych fragmentów (< −66 dBFS): adaptacyjny LP 1. rzędu + ekspansja w dół szumu kwantyzacji;
   suwak = próg i głębokość. Wyłączony wpływ powyżej −60 dBFS.
6. **Bramka szumów** – 4 pasma LR4 (150/1k/5k Hz), ekspander w dół 1:2…1:4, próg = estymowany szum + margines, histereza 3 dB,
   atak 2 ms, zwolnienie 150 ms. Suwak: próg (−80…−50 dBFS) i ratio.

### Dynamika
7. **AGC** – głośność ważona K, okno 3 s; wolna regulacja do celu −16 LUFS (pozycja przed procesorem),
   szybkość 1…6 dB/s wg suwaka, zakres −12…+12 dB, zamrożenie poniżej −45 LUFS (bramka ciszy), zwolnienie w górę wolniej niż w dół.
8. **Multiband 2** – 3 pasma LR4 (200 Hz, 3 kHz), wolny kompresor RMS (atak 300 ms, zwolnienie 2 s), ratio 1,5…3, wyrównanie barwy między utworami.
9. **Kompresor** – szerokopasmowy, RMS 5 ms, ratio 1,2…4:1, kolano 6 dB, atak 10 ms, zwolnienie zależne od programu (dwa stałe czasowe 80 ms / 1 s).
10. **Multiband 1** – 5 pasm LR4 (100, 400, 1600, 6400 Hz), szybkie kompresory szczytowo-RMS z miękkim kolanem; ratio i próg wg suwaka;
    sprzężenie pasm (maks. różnica GR sąsiadów 6 dB) chroni barwę.
11. **Punch** – projektor transjentów, 3 pasma: różnica obwiedni szybkiej (1 ms) i wolnej (50 ms) w dB → wzmocnienie ataku
    do +6 dB × suwak, z ograniczeniem szczytów oddanym do toru wyjściowego.

### Bas
12. **Głęboki bas** (True Bass) – pik 55 Hz (Q 1,2) + HPF subsonik 22 Hz (Butterworth 4. rz.), wzmocnienie dynamiczne:
    zmniejszane, gdy obwiednia basu przekracza próg (kompresja pasma <120 Hz), suwak 0…+8 dB.
13. **Bass Boost** – półka niska 90 Hz, +0…+6 dB, z nasyceniem miękkim (tanh) pasma <200 Hz przy dużych poziomach.
14. **Bas harmoniczny** (Immersive Bass) – psychoakustyczny bas (brakująca podstawowa; Larsen & Aarts, MaxxBass US 5,930,373 – wygasł):
    pasmo 40–120 Hz → generator harmonicznych (wielomiany Czebyszewa T2–T4 na sygnale znormalizowanym obwiednią) → BP 120–400 Hz
    → domieszka do sygnału. Suwak: poziom harmonicznych. Ważne dla małych przetworników słuchawek BT.

### Barwa
15. **Korektor** – stała krzywa „słuchawkowa” 5 filtrów (półka 105 Hz +4, pik 250 Hz −1, pik 3 kHz +1, pik 6,5 kHz −1,5, półka 10 kHz +2);
    suwak skaluje wszystkie wzmocnienia w dB liniowo.
16. **Auto EQ** (STFT B) – długoterminowe widmo 1/3 okt. (τ = 8 s) porównane z celem (−4,5 dB/okt. od 1 kHz, różowy z lekkim nachyleniem),
    korekta ±6 dB × suwak, wygładzona w częstotliwości (ERB) i w czasie.
17. **Balans widma** (Spectrum Control, STFT B) – szybszy (τ 200 ms) kompresor dynamicznych rezonansów: bin powyżej wygładzonej obwiedni
    (1/3 okt.) o > 3 dB jest ściągany w stronę obwiedni; suwak = ratio i maks. redukcja (do 9 dB).

### Stereo
18. **Rotacja fazy** – 4 sekcje all-pass 2. rzędu (SVF), f = 150…300 Hz wg suwaka; symetryzuje przebiegi mowy przed clipperem.
19. **Szerokość** – M/S: wzmocnienie S powyżej 200 Hz do ×1,6 × suwak; poniżej 120 Hz S tłumione (bas mono – kompatybilność),
    strażnik korelacji: przy korelacji L/R < 0,2 przyrost S ograniczony (bez „dziury” w środku, mono-kompatybilność).

### Wyjście
20. **Pre-limiter** – limiter z lookahead 1,5 ms, próg −3 dB względem sufitu, zwolnienie 30–150 ms; przygotowuje sygnał dla clippera.
21. **Clipper HD** – clipper z kasowaniem zniekształceń (inspiracja: wygasłe US 4,208,548 i US 4,495,643, Orban):
    4× oversampling, iteracyjne „clip-and-filter” (3 iteracje): d = x − clip(x); d̂ = filtr pasmowy(d) (usuwa produkty < 150 Hz
    i > 18 kHz – nieprzyjemne dla ucha IMD basu); x ← x − d̂. Ochrona basu: pasmo <150 Hz ograniczane osobno, łagodniej.
    Suwak: drive 0…+6 dB.
22. **Prosty clipper** – miękki clipper wielomianowy z ADAA 1. rzędu (Parker i in. 2016) + oversampling 2×.
23. **Hard Limit** – limiter true-peak: detekcja na sygnale 4× (BS.1770), wymagane wzmocnienie g = min(1, sufit/szczyt),
    minimum przesuwne na długości L = lookahead, potem średnia ruchoma długości L (gwarantuje osiągnięcie g w chwili szczytu,
    bez przeregulowania), zwolnienie wykładnicze 80 ms (adaptacyjne), margines 0,05 dB, na końcu twardy clip próbek do sufitu.
    Suwak: 100% = sufit −1,0 dBTP; mniejsze = wolniejsze zwolnienie? (do ustalenia – Hard Limit powinien być zawsze aktywny).

## 3. Lookahead / latencja

Cel: 4096 próbek @44,1 kHz (tryb jakości), 1024 (tryb niskiej latencji). Budżet: STFT A (2048) + STFT B (2048) = 4096 przekracza z resztą;
→ do rozważenia: jedna szyna STFT albo STFT 1024 w trybie niskiej latencji.

## 4. Pytania otwarte do dopracowania

1. Czy łączyć STFT A i B w jedną szynę (oszczędność latencji i CPU) mimo innej pozycji w torze?
2. Projekt all-passów kompensujących w drzewie LR4 dla 5 pasm – kolejność i koszt.
3. Hard Limit: gwarancja „nigdy > sufit dBTP” – czy twardy clip próbek po limiterze wystarczy, czy potrzebny drugi przebieg TP?
4. Clipper HD: iteracje vs. koszt CPU (cel < 15% rdzenia dla wszystkiego).
5. Znaczenie suwaka dla Hard Limit i Rotacji fazy (moduły „zero-jedynkowe”).
6. Crossfade on/off modułów z filtrami fazowymi (Rotacja fazy, LR4) – filtr grzebieniowy w czasie 10 ms.
