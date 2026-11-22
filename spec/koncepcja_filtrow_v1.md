# Szkło – koncepcja filtrów i algorytmów DSP, v1 (do implementacji)

Status: specyfikacja gotowa do implementacji (zastępuje szkic v0 = `koncepcja_filtrow.md`, którego nie zmieniamy).
Źródła: literatura DSP, wygasłe patenty, własne wyprowadzenia i symulacje (numpy/scipy) – bez cudzego kodu.
Stałe oznaczone **[S]** są wartościami startowymi do strojenia w fazie 5; wszystkie pozostałe wynikają z wyprowadzeń
i nie powinny być zmieniane bez powodu. Liczby oznaczone **[sym]** zostały sprawdzone symulacją przy pisaniu tej wersji.

Spis: 0 założenia · 1 odpowiedzi na pytania otwarte · 2 tor i latencja · 3 prymitywy · 4 moduły (23) · 5 presety ·
6 CPU · 7 testy jednostkowe prymitywów · 8 zmiany względem v0.

---

## 0. Założenia wspólne

- Wewnątrz f64, I/O f32. Bufory między modułami w grafie są f64 (konwersja f32↔f64 tylko na wejściu i wyjściu grafu;
  trait `Module` ze scenariusza operuje na f32 – w `core` moduły dostają wewnętrzny wariant `process_f64`, a wersja f32 jest adapterem).
- fs ∈ {44,1; 48; 88,2; 96} kHz. Wszystko, co jest w Hz/ms, liczone z `fs`. Długości w próbkach podane niżej dotyczą
  „rodziny 1×” (44,1/48 kHz); dla 88,2/96 kHz mnożymy przez 2 tam, gdzie zaznaczono `×F` (`F = 1` albo `2`).
- Każdy moduł ma stałą latencję (niezależną od on/off i suwaka). Wyłączony = linia opóźniająca, bit-exact.
- **Suwak** `a ∈ [0,1]`: wygładzanie jednobiegunowe τ = 20 ms liczone raz na blok sterujący `K = 16·F` próbek, wartość
  w bloku interpolowana liniowo. Z wygładzonego `a` liczone są parametry (funkcje z rozdz. 4) – też raz na blok.
  Funkcje pomocnicze:

```
lin(a, lo, hi)  = lo + (hi − lo)·a
expo(a, lo, hi) = lo·(hi/lo)^a
sstep(x)        = x²(3 − 2x) dla x∈[0,1], obcięte do [0,1]
dB(x) = 20·log10(x),  und(x) = 10^(x/20)
```

- **Tempo sterowania** (ważne dla CPU): cała dynamika (kompresory, AGC, bramka, Punch, detektory basu) liczy detektor
  i komputer wzmocnienia raz na blok `K = 16·F` próbek (0,36 ms); wzmocnienie liniowe jest interpolowane liniowo
  wewnątrz bloku. Wyjątek: limitery i clippery (co próbkę).
- **On/off (bez trzasków, także dla modułów z filtrami fazowymi)** – procedura dwufazowa:
  1. *neutralizacja* (50 ms): wewnętrzny mnożnik `e` 1→0 (przy włączaniu 0→1) skaluje „działanie” modułu:
     wzmocnienia w dB × e, domieszki × e, częstotliwość rotatora → f_off (patrz moduł). Po tej fazie moduł jest
     liniowy i co najwyżej wszechprzepustowy.
  2. *przenikanie* 10 ms równo-amplitudowe (`y = (1−t)·wet + t·dry_opóźnione`, `t = sstep`) i dopiero potem zatrzymanie
     obliczeń modułu (stany filtrów zerowane).
  Dla modułów bez fazy 1 (czysto addytywnych) wykonuje się tylko fazę 2.
- Denormale: po każdym bloku stany `|s| < 1e−30 → 0`.
- Poziom odniesienia: **sufit** `C = und(−1,0)` (−1,0 dBTP, parametr globalny). Sygnał wewnątrz toru może przekraczać 1,0
  (f64) – nic przed stopniem wyjściowym nie obcina.

---

## 1. Odpowiedzi na „Pytania otwarte” z v0

1. **Jedna czy dwie szyny STFT?** Dwie, ale różnej długości: **A = 2048/512** (naprawa: Szum, Naprawa MP3) i
   **B = 1024/256** (barwa: Auto EQ, Balans widma). Łączenie odpada, bo między nimi stoją AGC i Korektor, a naprawa musi
   widzieć sygnał nietknięty. B może być krótsza (rozdzielczość 43 Hz wystarcza dla barwy powyżej ~200 Hz),
   co oszczędza 1024 próbki. Koszt CPU obu szyn < 1,5 % rdzenia.
2. **All-passy w drzewie LR4.** Drzewo zbalansowane (podział najpierw na środkowej częstotliwości): dla 5 pasm potrzeba
   **4 sekcji all-pass 2. rzędu** (zamiast 6 w drzewie „obieranym” od dołu). Struktura i dowód w 3.3. LR4 liczone na SVF:
   3 SVF na jedną zwrotnicę (pierwszy wspólny dla LP i HP). Razem 16 sekcji/kanał dla 5 pasm.
3. **Gwarancja true-peak.** Twardy clip próbek po limiterze **nie** daje gwarancji (sam tworzy przerosty międzypróbkowe)
   i w v1 nie jest elementem algorytmu. Wystarcza **jeden** przebieg limitera, jeżeli: detekcja jest na sygnale 4×
   dokładnym interpolatorem (3.6), wzmocnienie = minimum przesuwne → zwolnienie → średnia ruchoma (3.10), a cel
   to `C − 0,10 dB`. Symulacja **[sym]**: błąd własny (modulacja wzmocnienia zmienia przebieg między próbkami)
   ≤ 0,007 dB przy L = 64 i ≤ 0,004 dB przy L = 128, nawet przy 10 dB redukcji; drugi przebieg TP nic już nie
   wykrywał, więc go nie ma. Margines 0,10 dB pokrywa różnice między miernikami 4× (różne FIR-y): dla materiału
   z pasmem ≤ 19 kHz trzy różne mierniki 4× pokazywały −0,09…−0,06 dB względem sufitu. Zastrzeżenie: dla sygnału
   z pełną energią w paśmie 20–22 kHz (biały szum, sweep do Nyquista) inny miernik może pokazać do ok. +0,1…+0,3 dB –
   to rozrzut samych mierników, nieusuwalny po stronie limitera; harness powinien mierzyć TP tym samym FIR-em co 3.6.
4. **Clipper HD: iteracje a koszt.** Zamiast 3 iteracji „clip-and-filter” na 4×: (a) stopień „bas robi miejsce” na 1×
   (liniowofazowy podział, zmienny próg basu) + (b) **2 przebiegi** clip → decymacja na 4× (drugi usuwa przerost po
   filtrze). Koszt ≈ 21 M operacji/s (≈ 1 % rdzenia PC), latencja 211 próbek. Symulacja **[sym]** (bas 60 Hz + pasmo
   mowy, +3…+6 dB ponad próg, μ = 0,5…0,75): zniekształcenia w paśmie 0,6–6 kHz niższe o 9–15 dB niż w zwykłym
   clipperze 4× (dla szumu „muzycznego” szerokopasmowego o 4–7 dB), kosztem 1,5–4 dB poziomu RMS (głównie basu);
   dla materiału bez basu wynik identyczny ze zwykłym clipperem.
5. **Suwak dla modułów „zero-jedynkowych”.** Hard Limit: `a` = dosterowanie 0…+6 dB (a = 0: limiter tylko pilnuje sufitu).
   Rotacja fazy: `a` = częstotliwość all-passów 10 → 200 Hz wykładniczo (przy 10 Hz rotacja leży poniżej pasma
   słyszalnego – praktycznie przezroczysta; zmiana f na SVF/TPT nie daje trzasków).
6. **Crossfade przy filtrach fazowych.** Procedura dwufazowa z rozdz. 0. Dla Rotacji fazy neutralizacja sprowadza
   f do 10 Hz, więc w chwili przenikania różnica faz powyżej ~60 Hz jest < 20° (brak grzebienia). Dla modułów LR4
   po neutralizacji zostaje all-pass sumy pasm; 10-ms przenikanie z sygnałem suchym daje chwilowe wąskie zapady przy
   częstotliwościach zwrotnic (ubytek szerokopasmowy < 0,5 dB przez < 10 ms) – akceptujemy, bo dzieje się tylko przy
   ręcznym przełączeniu modułu; alternatywa (FIR liniowofazowe) kosztowałaby latencję, której nie mamy.

---

## 2. Tor i latencja

### 2.1 Kolejność (v1)

```
WE → miernik WE (K, LUFS)
 [Naprawa]  Declipper → Dekwantyzer → DC-blok (HPF 1. rz. 5 Hz) → Dehummer
            → szyna STFT A { Szum i brum → Naprawa MP3 (a: dziury, b: pasmo) } → Naprawa MP3 (c: pre-echo, w czasie)
            → Bramka szumów
 [Poziom]   AGC
 [Barwa]    Korektor → szyna STFT B { Auto EQ → Balans widma }
 [Stereo]   Szerokość
 [Bas]      Głęboki bas → Bass Boost → Bas harmoniczny
 [Dynamika] Multiband 2 (3 pasma, wolny) → Kompresor → Multiband 1 (5 pasm, szybki) → Punch
 [Wyjście]  Rotacja fazy → Pre-limiter → Clipper HD → Prosty clipper → Hard Limit → wyrównanie latencji → miernik WY → WY
```

Zmiany kolejności względem v0 i powody:

- **Declipper i Dekwantyzer przed DC-blokiem.** Oba potrzebują nietkniętych próbek (płaskie szczyty, siatka 16-bit);
  HPF pochyla płaskie szczyty i rozmywa siatkę.
- **Dehummer przed szyną A.** Brum zawyża estymatę szumu (minimum statistics traktuje stały ton jako szum) i jest
  taniej usuwany wąskimi filtrami niż widmowo.
- **W szynie A najpierw Szum, potem Naprawa MP3.** Detekcja dziur idzie na widmie wejściowym szyny, a wypełnienie jest
  dodawane po odszumieniu – inaczej odszumiacz usuwałby dopiero co dodany szum wypełniający.
- **Szerokość po AGC i barwie, przed dynamiką** – kompresory (sprzężone stereo) kontrolują już poszerzony sygnał,
  a strażnik korelacji pracuje na sygnale o znanym poziomie.
- **Rotacja fazy tuż przed stopniem wyjściowym.** Jej jedyny cel to symetryczny przebieg dla clippera/limitera;
  zwrotnice LR4 po drodze same są all-passami i zmieniałyby kształt po rotatorze umieszczonym wcześniej.
- Reszta jak w v0 (AGC przed barwą; bas przed kompresją wielopasmową, która go kontroluje; Punch po szybkiej kompresji,
  jego szczyty przejmuje clipper).

### 2.2 Budżet latencji

Tryb jakości (44,1 i 48 kHz; dla 88,2/96 kHz w nawiasie):

| Element | Próbki | (96 kHz) |
|---|---|---|
| Declipper | 512 | (1024) |
| Dekwantyzer | 8 | (16) |
| Szyna STFT A (N = 2048, skok 512) | 2048 | (4096) |
| Szyna STFT B (N = 1024, skok 256) | 1024 | (2048) |
| Pre-limiter (L = 64) | 64 | (128) |
| Clipper HD (63 podział + 8 obwiednia + 2×70 oversampling) | 211 | (283) |
| Prosty clipper (1×70 oversampling) | 70 | (70) |
| Hard Limit (20 interpolator + 128 lookahead) | 148 | (276) |
| Wyrównanie (linia opóźniająca na końcu) | 11 | (251) |
| **Razem** | **4096** | **(8192)** |

Pozostałe moduły mają latencję 0. Wtyczka raportuje hostowi 4096 (8192).

Tryb niskiej latencji (1024 próbki, oznaczony w GUI jako obniżona jakość) **[S]**: Declipper 64 (rząd 16, serie ≤ 16),
Dekwantyzer 8, STFT A 512/128, STFT B 256/64, Pre-limiter 16, Clipper HD 77 (podział B = 32 → 31, obwiednia 8,
jeden przebieg z krótszym półpasmowym 63-tap: 31 + 7), Prosty clipper 38, Hard Limit 44 (interpolator Th = 12,
L = 32), wyrównanie 9. Razem 1024. Implementujemy po zaliczeniu trybu jakości.

---

## 3. Prymitywy (`core/src/dsp/`)

### 3.1 SVF Simpera (trapezowy, „TPT”) – wszystkie filtry modulowane i zwrotnice

Źródło: A. Simper (Cytomic), „Linear Trapezoidal Integrated SVF”, 2013 (wzory z publikacji).

```
parametry: g = tan(π·fc/fs),  k = 1/Q
a1 = 1/(1 + g·(g + k));  a2 = g·a1;  a3 = g·a2
stan: ic1, ic2 (start 0)
krok (wejście v0):
  v3  = v0 − ic2
  v1  = a1·ic1 + a2·v3
  v2  = ic2 + a2·ic1 + a3·v3
  ic1 = 2·v1 − ic1
  ic2 = 2·v2 − ic2
wyjścia:
  LP = v2      BP = v1      HP = v0 − k·v1 − v2
  notch = v0 − k·v1         allpass = v0 − 2·k·v1
  ogólnie: y = m0·v0 + m1·v1 + m2·v2
dzwon (peak), A = 10^(G_dB/40):   g = tan(π fc/fs),      k = 1/(Q·A), m0 = 1,  m1 = k·(A² − 1), m2 = 0
półka niska:                      g = tan(π fc/fs)/√A,   k = 1/Q,     m0 = 1,  m1 = k·(A − 1),  m2 = A² − 1
półka wysoka:                     g = tan(π fc/fs)·√A,   k = 1/Q,     m0 = A², m1 = k·(1 − A)·A, m2 = 1 − A²
```

Uwagi: dzwon z `G = 0 dB` daje dokładnie `y = v0` (m1 = 0) – wygodne dla filtrów dynamicznych. Butterworth 2. rzędu:
`k = √2`. Ograniczyć `fc ≤ 0,45·fs`. Współczynniki przeliczać najwyżej raz na blok sterujący; `tan` tylko gdy zmienia się fc.

Filtr 1. rzędu (TPT) – DC-blok, rotator fazy:

```
g = tan(π fc/fs);  G = g/(1 + g)
v = (x − s)·G;  lp = v + s;  s = lp + v
hp = x − lp;    ap = 2·lp − x        (ap: faza 0° → −180°, −90° przy fc)
```

### 3.2 Biquad TDF-II – tylko filtry stałe lub wolno zmienne (Korektor, ważenie K)

```
y = b0·x + s1;  s1 = b1·x − a1·y + s2;  s2 = b2·x − a2·y
```

Współczynniki z RBJ (bilinearne) dla f0 < fs/8, „dopasowane” (3.4) dla f0 ≥ fs/8.

### 3.3 Zwrotnice LR4 z kompensacją fazy

Jedna zwrotnica przy `f` = 3 SVF Butterwortha (`k = √2`, to samo `g`):

```
(lp1, hp1) = SVF_a(x)          // jeden filtr daje LP i HP
LP4 = SVF_b(lp1).LP            // LR4 dolny  = Butterworth² 
HP4 = SVF_c(hp1).HP            // LR4 górny (bez odwracania znaku)
LP4 + HP4 = A_f(x),  A_f = all-pass 2. rzędu: SVF.allpass z k = √2 przy tej samej f
```

(Dowód: `(ω⁴ + s⁴)/(s² + √2ωs + ω²)² = (s² − √2ωs + ω²)/(s² + √2ωs + ω²)`; po transformacji biliniowej tożsamość zostaje.)

**5 pasm** (f1 < f2 < f3 < f4), drzewo zbalansowane, 4 all-passy:

```
x ─┬─ LP4(f2) ─ A(f3) ─ A(f4) ─┬─ LP4(f1) → p1
   │                           └─ HP4(f1) → p2
   └─ HP4(f2) ─ A(f1) ─┬─ LP4(f3) ─ A(f4) → p3
                       └─ HP4(f3) ─┬─ LP4(f4) → p4
                                   └─ HP4(f4) → p5
p1+…+p5 = A(f1)·A(f2)·A(f3)·A(f4)·x     (amplituda płaska)
```

Sprawdzenie: `p1+p2 = A1·A3·A4·LP2`; `p4+p5 = A4·HP3·A1·HP2`; `p3 = A4·LP3·A1·HP2`; suma górnej gałęzi `= A1·A3·A4·HP2`;
całość `= A1·A3·A4·(LP2 + HP2) = A1·A2·A3·A4`.

**4 pasma** (f1<f2<f3): `x → LP4(f2) → A(f3) → {LP4(f1), HP4(f1)}`; `x → HP4(f2) → A(f1) → {LP4(f3), HP4(f3)}` (2 all-passy).
**3 pasma** (f1<f2): `x → LP4(f1) → A(f2) → p1`; `x → HP4(f1) → {LP4(f2) → p2, HP4(f2) → p3}` (1 all-pass).

Test: suma pasm przy wzmocnieniach 1 ma |H| = 1 ± 0,001 dB w 20 Hz–20 kHz.

### 3.4 Filtry „dopasowane” 2. rzędu (pik/półka powyżej fs/8)

Metoda w duchu Vicanka (2016), ale w postaci wyprowadzonej tu od zera, jednej dla wszystkich typów: bieguny przez
niezmienność odpowiedzi impulsowej, licznik z dopasowania |H|² w trzech punktach: DC, f_m, Nyquist.
Wejście: analogowy prototyp – para biegunów `(f_p, Q_p)` oraz funkcja `M(f) = |H_a(j2πf)|²`.

```
w_p = 2π·f_p/fs;  q = 1/(2·Q_p)
a1 = −2·e^(−q·w_p)·cos (w_p·√(1 − q²))      gdy q ≤ 1
a1 = −2·e^(−q·w_p)·cosh(w_p·√(q² − 1))      gdy q > 1
a2 = e^(−2·q·w_p)

A0 = (1 + a1 + a2)²;  A1 = (1 − a1 + a2)²;  A2 = −4·a2
w_m = 2π·f_m/fs;  φ1 = sin²(w_m/2);  φ0 = 1 − φ1;  φ2 = 4·φ0·φ1
B0 = A0·M(0)
B1 = A1·M(fs/2)
B2 = ( M(f_m)·(A0·φ0 + A1·φ1 + A2·φ2) − B0·φ0 − B1·φ1 ) / φ2
W  = (√B0 + √B1)/2
b0 = ( W + √max(W² + B2, 0) )/2
b1 = (√B0 − √B1)/2
b2 = W − b0
```

(Tożsamość: `|b0 + b1 z⁻¹ + b2 z⁻²|² = B0·cos²(w/2) + B1·sin²(w/2) + B2·sin²(w)` z `B0 = (b0+b1+b2)²`,
`B1 = (b0−b1+b2)²`, `B2 = −4·b0·b2`; wybór większego pierwiastka dla b0 daje filtr minimalnofazowy.)

Prototypy (`A = 10^(G_dB/40)`, `Ω = f/f0`, `s = jΩ`):

```
pik:          M(f) = ((1−Ω²)² + (Ω·A/Q)²) / ((1−Ω²)² + (Ω/(A·Q))²);   f_p = f0,      Q_p = A·Q,  f_m = f0
półka wysoka: H_a = A·(A·s² + (√A/Q)·s + 1)/(s² + (√A/Q)·s + A);       f_p = f0·√A,   Q_p = Q,    f_m = f0
półka niska:  H_a = A·(s² + (√A/Q)·s + A)/(A·s² + (√A/Q)·s + 1);       f_p = f0/√A,   Q_p = Q,    f_m = f0
```

Dokładność **[sym]** @44,1 kHz względem prototypu analogowego do 0,9·Nyquista: pik 6,5 kHz (−1,5 dB, Q 1,2): 0,06 dB;
półka 10 kHz ±6 dB: 0,12 dB; pik 3 kHz: 0,01 dB. Metoda traci dokładność dla wąskich pików powyżej fs/4
(pik 12 kHz +6 dB Q 1: 0,5 dB; 16 kHz Q 3: 2 dB) – w Szkle takich nie ma; zakres użycia: f0 ≤ fs/4.
Wartości kontrolne @44,1 kHz, półka wysoka 10 kHz +2 dB Q 0,7071:
`b = [1.12554823, −0.51124161, 0.17198491]`, `a = [1, −0.33203296, 0.11832450]`.

### 3.5 Oversampling ×2·×2 (FIR półpasmowe, liniowofazowe)

```
N = 4·K + 3,  M = (N − 1)/2
h[n] = 0,5·sinc((n − M)/2)·kaiser(n),   sinc(x) = sin(πx)/(πx)
kaiser(n) = I0(β·√(1 − ((n − M)/M)²)) / I0(β),   I0(x) = Σ_k ((x/2)^k / k!)²
po wyliczeniu: h[M] = 0,5 dokładnie; pozostałe przeskalować, by Σ_{n≠M} h[n] = 0,5
```

| Stopień | N | β | Pasmo (−0,01 dB) | −80 dB od | Opóźnienie góra+dół |
|---|---|---|---|---|---|
| 1 (1×↔2×) | 127 | 9,0 | 0,2299·fs₂ (20,3 kHz @44,1) | 0,2723·fs₂ (24,0 kHz) | 126 próbek 2× = 63 próbki 1× |
| 2 (2×↔4×) | 27 | 9,0 | 0,1525·fs₄ (26,9 kHz) | 0,358·fs₄ (63,2 kHz) | 26 próbek 4× + 2 próbki dopełnienia = 7 próbek 1× |
| 1 „LL” | 63 | 7,0 | 0,2158·fs₂ (19,0 kHz) | −60 dB od 25,1 kHz | 31 próbek 1× |

(wartości pasm **[sym]**). Interpolacja: wstawić zera, filtr `2·h` (polifazowo: próbki parzyste = wejście opóźnione o M,
nieparzyste = splot z niezerowymi współczynnikami). Decymacja: filtr `h`, co druga próbka. W stopniu 2 dodać po jednej
próbce opóźnienia 4× przed i po nieliniowości, żeby łączna latencja była całkowita: **70 próbek 1×** na jeden przebieg
góra–dół. Te same współczynniki dla wszystkich fs (dla 96 kHz można pominąć stopień 2 – wtedy 63).

### 3.6 Interpolator true-peak 4× (detekcja w limiterze i miernik)

Nie używamy 48-współczynnikowego filtru z załącznika BS.1770 (przy 44,1 kHz zaniża o kilka dziesiątych dB powyżej
15 kHz); projekt własny, zgodny z metodą BS.1770 (4× nadpróbkowanie, wartość bezwzględna, maksimum):

```
Th = 20 (połowa długości w próbkach wejściowych), β = 5,5
h[m] = sinc(m/4)·kaiser(m),   m = −4·Th … 4·Th   (161 współczynników, okno Kaisera na całej długości)
faza p ∈ {1,2,3}:  y_p[n] = Σ_{k=−Th+1}^{Th} c_p[k]·x[n + k],   c_p[k] = h[4k − p]   (40 współczynników)
każdą fazę znormalizować: Σ_k c_p[k] = 1
faza 0: y_0[n] = x[n]
p[n] = max(|y_0[n]|, |y_1[n]|, |y_2[n]|, |y_3[n]|)   → szczyt w przedziale [n, n+1)
opóźnienie przyczynowe: D_tp = Th = 20 próbek
```

Dokładność **[sym]** @44,1 kHz: pasmo do 20 kHz −0,007/+0,010 dB, obrazy ≤ 0,0012 (0,011 dB); @48 kHz lepiej.
Dla 88,2/96 kHz te same współczynniki. Tryb niskiej latencji: Th = 12, β = 4,5 (−0,34 dB przy 20 kHz – dodać 0,3 dB marginesu).

### 3.7 Detektory obwiedni i komputer wzmocnienia

Na blok sterujący (K próbek), stereo sprzężone:

```
szczyt:  pk = max po bloku i kanałach |x|;      x_G = dB(pk + 1e−9)
RMS:     ms = α_d·ms + (1 − α_d)·mean_blok(x_L² + x_R²)/2;   x_G = 10·log10(ms + 1e−18)
współczynnik dla stałej czasowej τ (63 %):  α = exp(−K/(τ·fs))
```

Komputer wzmocnienia (Giannoulis, Massberg, Reiss 2012) – próg T, ratio R, kolano W [dB]:

```
d = x_G − T
y_G = x_G                                  gdy 2d < −W
y_G = x_G + (1/R − 1)·(d + W/2)²/(2W)      gdy |2d| ≤ W
y_G = T + d/R                              gdy 2d > W
c = x_G − y_G   (≥ 0, wymagana redukcja w dB)
```

Balistyka na redukcji (w dB), potem makijaż:

```
s = α_A·s + (1 − α_A)·c   gdy c > s,   inaczej   s = α_R·s + (1 − α_R)·c
gain_dB = M − s;   gain = und(gain_dB)  → interpolacja liniowa w bloku
```

Zwolnienie zależne od programu (dwie gałęzie): `s_f` (atak τ_A, zwolnienie τ_R1) i `s_s` (atak 10·τ_A, zwolnienie τ_R2);
`s = max(s_f, s_s)`.
Ekspander w dół (bramka): `c = (R_e − 1)·max(0, T − x_G)` z kolanem jak wyżej (lustrzanie), ograniczone do `c ≤ D_max`.

### 3.8 Ważenie K i głośność (BS.1770) dla dowolnego fs

Parametry prototypu analogowego (literatura: odtworzenie filtru normy, np. De Man 2018), biliniowo z `K = tan(π f0/fs)`:

```
półka:  f0 = 1681,974450955533 Hz;  G = 3,999843853973347 dB;  Q = 0,7071752369554196
        Vh = 10^(G/20);  Vb = Vh^0,4996667741545416;  K = tan(π f0/fs);  a0 = 1 + K/Q + K²
        b0 = (Vh + Vb·K/Q + K²)/a0;  b1 = 2(K² − Vh)/a0;  b2 = (Vh − Vb·K/Q + K²)/a0
        a1 = 2(K² − 1)/a0;           a2 = (1 − K/Q + K²)/a0
HPF:    f0 = 38,13547087602444 Hz;  Q = 0,5003270373238773;  K = tan(π f0/fs);  a0 = 1 + K/Q + K²
        b = [1, −2, 1]  (bez dzielenia przez a0 – jak w normie);  a1 = 2(K² − 1)/a0;  a2 = (1 − K/Q + K²)/a0
głośność: L = −0,691 + 10·log10( Σ_kanały mean(y_K²) )   [LUFS]
```

Kontrola **[sym]**: dla 48 kHz wychodzi dokładnie tabela normy (`b0 = 1,53512485958697`, `a1 = −1,69065929318241`,
HPF `a1 = −1,99004745483398`). Wzmocnienie przy 997 Hz: 0,691 dB @48 k, 0,694 @44,1 k, 0,674 @96 k.

### 3.9 STFT (szyny A i B)

```
okno analizy i syntezy: Hann okresowy  w[n] = 0,5 − 0,5·cos(2πn/N),  n = 0…N−1
skok H = N/4;   Σ_k w²(n − kH) = 1,5   → skala wyjścia 1/(1,5·N)  (FFT nienormalizowane: odwrotna(prosta(x)) = N·x)
ramka t: X_t[k] = FFT_rzeczywista( w[n]·x[tH + n] ),  k = 0…N/2
wyjście: y[tH + n] += w[n]·IFFT(Y_t)[n]/(1,5·N)
latencja = N próbek (ramkę liczymy po nadejściu jej ostatniej próbki; gotowe próbki oddajemy przez następne H próbek)
```

- Hann/Hann zamiast sqrt-Hann z v0: niższe listki boczne analizy (−31 dB, 18 dB/okt.) – lepsza detekcja dziur i estymacja szumu.
- FFT: `realfft` (na `rustfft`), plan tworzony w `reset`. Stereo: każdy kanał osobno, wzmocnienia wspólne (liczone z mocy L+R),
  żeby nie ruszać obrazu stereo.
- Gdy żaden moduł szyny nie jest włączony, szyna jest linią opóźniającą (bit-exact); włączanie/wyłączanie szyny – przenikanie 10 ms.
- **Wstawianie szumu** (Naprawa MP3): prążek o losowej fazie i amplitudzie `A` daje po ponownej analizie moc `0,25·A²`
  **[sym]** (ramki są niezależne). Aby uzyskać docelową moc prążka `P`, wstawiać `A = 2·√P`.
- Rozmiary: A: N = 2048·F, B: N = 1024·F.

### 3.10 Limiter z lookahead (wspólny dla Pre-limitera i Hard Limit)

```
wejście: p[n] – szczyt (true-peak z 3.6 albo |x|), już po maks. z kanałów;  T – cel;  L – lookahead
q[n]     = max(p[n], p[n−1])
g_req[n] = min(1, T/max(q[n], 1e−12))
g_min[n] = min(g_req[n−L … n])                    // okno L+1, kolejka monotoniczna O(1)
r[n]     = min(g_min[n], r[n−1] + k_rel·(1 − r[n−1]))        // natychmiast w dół, wykładniczo w górę
g_s[n]   = (1/L)·Σ_{i=0}^{L−1} r[n−i]             // suma bieżąca; co 4096 próbek przeliczyć od nowa (dryf)
wyjście: y[n] = g_s[n]·x[n − D − L]               // D = opóźnienie detektora (D_tp albo 0)
```

Gwarancja: każde `r[m]`, `m ∈ [n−L+1, n]`, jest ≤ `g_req[n−L]` (okno minimum każdego z nich zawiera indeks n−L),
więc `g_s[n] ≤ g_req[n−L]` – w chwili szczytu wzmocnienie jest co najmniej tak małe, jak trzeba; narastanie redukcji
jest rozłożone na L próbek (bez przeregulowania, bo średnia z wartości ≤ 1).

Zwolnienie adaptacyjne: `k_rel = 1 − exp(−1/(τ_rel·fs))`, `τ_rel = τ0·(1 + 9·min(1, GR_avg/6 dB))`, gdzie `GR_avg` to
średnia (τ = 1 s) z `−dB(g_s)`. Przy sporadycznych szczytach zwolnienie szybkie (τ0), przy ciągłej pracy do 10× wolniejsze
(mniej pompowania i intermodulacji basu).

### 3.11 Miękki clip z kolanem parabolicznym i ADAA

Funkcja (nieparzysta; próg θ, półszerokość kolana w ∈ (0,1)), `x1 = θ(1−w)`, `x2 = θ(1+w)`, dla `u = |x|`:

```
f(u) = u                           0 ≤ u ≤ x1
f(u) = u − (u − x1)²/(4·θ·w)       x1 < u < x2
f(u) = θ                           u ≥ x2            (f i f′ ciągłe)
pierwotna (parzysta):
F(u) = u²/2                        0 ≤ u ≤ x1
F(u) = u²/2 − (u − x1)³/(12·θ·w)   x1 < u < x2
F(u) = x2²/2 − (2/3)·θ²·w² + θ·(u − x2)     u ≥ x2
```

ADAA 1. rzędu (Parker, Zavalishin, Le Bihan 2016):

```
y[n] = (F(x[n]) − F(x[n−1])) / (x[n] − x[n−1])     gdy |x[n] − x[n−1]| > 1e−6·θ
y[n] = f((x[n] + x[n−1])/2)                         w przeciwnym razie
```

**Decyzja: w v1 ADAA nie jest używane w torze.** Powód: w obszarze liniowym ADAA-1 jest filtrem `[½, ½]` – opóźnia
o pół próbki (latencja niecałkowita → grzebień przy przenikaniu on/off) i tłumi `cos(π f/fs_os)`: przy 2× to −2,4 dB
przy 20 kHz, przy 4× jeszcze −0,56 dB. Kompensacja wymaga osobnego projektu FIR, a zysk jest mały, bo 4× oversampling
z filtrami z 3.5 kosztuje tylko ~9 M operacji/s. Wzory zostają jako wariant oszczędny do oceny w fazie Android
(wtedy: 2× + ADAA + korekcja opadania w filtrze decymującym). Oba clippery używają `f` z nadpróbkowaniem 4×.

---

## 4. Moduły

Format: **Algorytm** · **Suwak** (mapowanie `a` → parametry) · **Latencja**. Częstotliwości zwrotnic i progi – [S].

### Naprawa

#### 1. Declipper (latencja 512·F)

Praca ramkowa na kanał: skok `H_d = 256·F`, okno analizy `N_d = 1024·F` (ostatnie próbki), obszar zatwierdzany
`[n−511F, n−256F]` (prawy kontekst 256·F).

Detekcja (na skok):
```
M = maks |x| z ostatnich 2 s
tryb twardy:   serie ≥ 3 kolejnych próbek tego samego znaku z |x| ≥ 0,98·M, różniących się między sobą < 1e−4·M;
               ≥ 5 takich serii w 2 s → wykryto, θ̂ = mediana poziomów serii (osobno dla + i −)
tryb miękki (tylko a > 0,5; materiał po kodeku stratnym):
               ρ = #(|x| ≥ 0,89·M) / #(|x| ≥ 0,71·M) z 2 s;  ρ > 0,35 → wykryto, θ̂ = 0,89·M
próbka „obcięta”: |x| ≥ c(a)·θ̂; seria = ciąg obciętych tego samego znaku, rozszerzony o 1 próbkę z każdej strony
```
Rekonstrukcja AR (Janssen, Veldhuis, Vries 1986), rząd `p = 32·F`, 2 iteracje na skok:
```
1. autokorelacja r[0…p] okna N_d (okno Hanna, bieżące estymaty w miejscach obciętych), r[0] ·= 1 + 1e−6
2. Levinson–Durbin → a[0…p], a[0] = 1   (błąd predykcji e[n] = Σ_k a[k]·s[n−k])
3. b[l] = Σ_{k=0}^{p−|l|} a[k]·a[k+|l|],  |l| ≤ p
4. dla każdej serii o indeksach t_0…t_{m−1} (m ≤ 64·F), reszta próbek traktowana jako znana:
     B[i][j] = b[t_i − t_j]  (m×m, symetryczna dodatnio określona)
     z[i]    = Σ_{n znane, |t_i − n| ≤ p} b[t_i − n]·s[n]
     rozwiązać B·x = −z (Cholesky);  s[t_i] ← x[i]
5. spójność: jeśli znak·s[t_i] < θ̂ → s[t_i] = znak·θ̂
   ograniczenie przyrostu: nadwyżka e = |s| − θ̂ → e′ = E·tanh(e/E),  E = θ̂·(und(G_max) − 1)
```
Przypadki brzegowe: seria 64·F < m ≤ 192·F → zamiast AR sześcienny Hermite przez próbki brzegowe z ich nachyleniami
(różnice jednostronne), wynik też przez krok 5; m > 192·F albo seria niemieszcząca się w prawym kontekście → nie ruszać
(to celowa fala prostokątna, nie przester). Udział próbek obciętych w oknie > 35 % → nie ruszać. Najwyżej 48 serii na skok
(najpierw najdłuższe). Brak wykrycia → wyjście = wejście opóźnione, bit-exact. Nie stosujemy tłumienia −3 dB z v0
(tor jest f64, szczyty > 1 są dozwolone).

**Suwak:** `c(a) = lin(a, 0,999, 0,97)` w trybie twardym; w miękkim `c = 1 − 0,2·(a − 0,5)` (a = 1 → 0,90);
`G_max = lin(a, 2, 6) dB`; zmiany wyniku skalowane `min(1, 4a)` (y = x + min(1,4a)·(s − x)), więc a = 0 jest przezroczyste.
Ograniczenie: dla MP3 płaskie szczyty są rozmyte przez kodek – skuteczność mniejsza niż dla źródeł bezstratnych.

#### 2. Naprawa MP3 (szyna A + część czasowa; latencja zawarta w szynie)

**(a) Dziury widmowe.** Pasma `b` o szerokości 1/6 oktawy od 3 kHz do min(f_odc, 16 kHz), nie węższe niż 8 prążków.
```
P_b(t)  = średnia moc prążków pasma (L+R)
P_lt,b  = średnia długoterminowa: gdy pasmo nie jest dziurą: τ = 300 ms; gdy jest: opada z τ = 1 s
sąsiedzi: P_n,b = średnia geometryczna P_{b−1}, P_{b+1}
dziura  ⇔ P_b < 0,063·P_lt,b (−12 dB)  i  P_b < 0,063·P_n,b
cel     : P_fill = a_f·( 0,25·P_lt,b − P_b )⁺           (uzupełnienie do −6 dB poniżej niedawnego poziomu)
każdy prążek pasma (po odszumianiu): Y[k] += 2·√P_fill·e^{jφ},  φ losowe na prążek i ramkę, niezależne dla L i R
```
**(b) Rozszerzenie pasma.** Widmo długoterminowe (τ = 2 s) w pasmach 1/6 okt.; odcięcie `f_c` = najwyższe pasmo
o poziomie > −80 dBFS, powyżej którego poziom spada o > 20 dB w jednym paśmie; ważne dla 7 kHz ≤ f_c ≤ 19 kHz, stabilne
≥ 2 s (histereza 1 pasmo). Kopiowanie w górę:
```
m = przesunięcie w prążkach, wielokrotność 4, odpowiadające Δ = min(f_c/3, 4 kHz)
   (wielokrotność 4 przy skoku N/4 zachowuje spójność fazy między ramkami: e^{j2π·m·H/N} = 1)
dla k ∈ (k_c, min(k_c + m, k(20 kHz))]:   Y[k] += g_b(k)·X[k − m]
g_b(k) = a_b · und( S·log2(f_k/f_{k−m}) − 3 ),   S = nachylenie widma [dB/okt.] z regresji w oktawie poniżej f_c,
                                                 obcięte do [−24, −6]
```
**(c) Pre-echo (w dziedzinie czasu, za syntezą szyny A).** Detektor patrzy na *wejście* szyny, tłumienie działa na jej
*wyjściu* – szyna daje 2048 próbek wyprzedzenia za darmo.
```
h = HP 2. rzędu 3 kHz (SVF) sygnału wejściowego;  e_f = obwiednia |h| (atak 0,3 ms, zwolnienie 5 ms)
atak w chwili t0 ⇔ e_f(t0) > 4·mean(e_f w [t0 − 5 ms, t0 − 1 ms])   (+12 dB),  martwy czas 30 ms
pre-echo obecne ⇔ mean(e_f² w [t0 − 12 ms, t0 − 1 ms]) > 4·mean(e_f² w [t0 − 40 ms, t0 − 16 ms])
wtedy na wyjściu: y = x + (g(t) − 1)·HP_3k(x),  g: 1 → und(−D_pe) (półokres cosinusa od t0 − 16 ms do t0 − 8 ms),
                  stałe do t0 − 1 ms, powrót do 1 w 0,5 ms (kończy się przed t0)
```
Postać `x + (g − 1)·HP(x)` jest dokładnie przezroczysta przy g = 1.

**Suwak:** `a_f = a`; `a_b = sstep(a)` (domieszka (b) rośnie łagodnie); `D_pe = 9·a dB`.

#### 3. Szum i brum (szyna A)

Uproszczone minimum statistics (Martin 2001) + Wiener z decyzją kierowaną (Ephraim–Malah 1984), na prążek, moc L+R:
```
P[k]   = 0,85·P[k] + 0,15·|X[k]|²                         (τ ≈ 70 ms przy skoku 11,6 ms)
minimum z ~1,5 s: 8 podokien po 16 ramek; N_raw[k] = 1,66·min(bieżące podokno, 8 zapamiętanych)   [S]
ochrona muzyki:   N[k] = min( N_raw[k], 2·mediana_{±1/6 okt.}(N_raw), N_cap[k] )
   N_cap[k] – moc prążka odpowiadająca poziomowi L_cap(a) dBFS w paśmie 1/3 okt. (szum równomierny w paśmie)
γ = |X|²/N;   ξ = 0,92·G_poprz²·γ_poprz + 0,08·max(γ − 1, 0)
G_w = ξ/(β_o + ξ);   G = max(G_w, G_min);   wygładzenie G po częstotliwości [¼, ½, ¼];   Y = G·X
```
Mediana po częstotliwości odrzuca długie nuty (wąskie), limit `N_cap` nie pozwala uznać za szum niczego głośniejszego niż
L_cap. Stałe tony (brum) zostawiamy Dehummerowi.

**Suwak:** `G_min = und(−15·a)` (a = 0 → G ≡ 1, przezroczyste); `β_o = 1 + a` (nadodejmowanie);
`L_cap = lin(a, −66, −42) dBFS`.

#### 4. Dehummer (latencja 0)

Detekcja na sygnale mono zdecymowanym ×8·F (po LP), dla f0 ∈ {50, 60} Hz:
```
demodulacja kwadraturowa przy k·f0, k = 1…8: I/Q przez LP 1. rzędu 2 Hz → amplituda A_k, faza φ_k
odniesienie: RMS pasm przy k·f0·2^(±1/4) (BP Q = 8), uśrednione 1 s → R_k;  nadwyżka_k = max(0, dB(A_k/R_k))
brum(f0) ⇔ przez ≥ 3 s: istnieje k ≤ 3 z nadwyżka_k > 10 dB oraz wahanie A_k (maks./min. z 3 s) < 3 dB
strojenie: Δf = f0·clamp( dφ_1/dt /(2π·f0), ±2 % ) (z nachylenia fazy, uśrednione 2 s) → f0′ = f0 + Δf
```
Usuwanie: 8 dzwonów SVF przy `k·f0′`, k = 1…8, Q = 40 [S], wzmocnienie
`G_k = −e_h·min(D_h, nadwyżka_k)` dB (harmoniczna bez nadwyżki nie jest ruszana),
`e_h` – płynne włączanie 0→1 w 1 s po wykryciu i 1→0 w 3 s po zaniku. Dzwon z 0 dB jest dokładnie przezroczysty;
gdy `e_h = 0`, filtry nie są liczone.

**Suwak:** `D_h = 30·a dB` (maks. głębokość).

#### 5. Dekwantyzer (latencja 8·F)

Idea: skwantowana próbka to przedział `[x − q/2, x + q/2]`; szukamy gładszego sygnału, który z niego nie wychodzi.
Błąd jest z konstrukcji ≤ q/2.
```
wykrycie siatki (okna 0,5 s): ≥ 95 % próbek spełnia |x·2^15 − round(x·2^15)| < 1e−3 → q = 2^−15;
   analogicznie test 2^−7 (8 bitów); brak siatki → moduł przezroczysty (przenikanie 50 ms)
y⁰ = x;  powtórz 4 razy:  y ← splot(y, [1,4,6,4,1]/16);  y ← clamp(y, x − q/2, x + q/2)
waga poziomu: w = 1 − sstep( (L_pk − (L_on − 6))/12 ),  L_pk = obwiednia szczytowa (zwolnienie 50 ms) w dBFS
wyjście: x + m·w·(y − x)
```
Obliczenia tylko gdy `w > 0`. **Suwak:** `m = min(1, 2a)`; `L_on = lin(a, −72, −54) dBFS`.

#### 6. Bramka szumów (latencja 0)

4 pasma LR4 (150 Hz, 1 kHz, 5 kHz; drzewo z 3.3), w każdym ekspander w dół:
```
poziom pasma: RMS τ = 10 ms → x_G
szum pasma:   N_b = minimum x_G (wygładzonego 100 ms) z ostatnich 10 s
próg:         T_b = min( N_b + 6 dB, T_max )
ekspander:    c = (R_e − 1)·max(0, T_b − x_G), kolano 6 dB, c ≤ D_max;  histereza: otwarcie przy T_b + 3 dB
balistyka:    otwieranie (c maleje) 2 ms, przytrzymanie 50 ms, zamykanie 150 ms
```
**Suwak:** `R_e = lin(a, 1, 3)` (a = 0 → brak działania); `T_max = lin(a, −80, −50) dBFS`; `D_max = 18·a dB`.

### Poziom i dynamika

#### 7. AGC (latencja 0)

```
ważenie K (3.8) obu kanałów → bloki 100 ms: z_i = Σ_kan mean(y_K²)
L_M = −0,691 + 10·log10(mean z ostatnich 4 bloków);   L_S = to samo z 30 bloków (3 s)
bramka: aktywne ⇔ L_M > max(−50 LUFS, L_est − 20 LU);  gdy aktywne: L_est = L_S, inaczej L_est zamrożone
G_cel = clamp( k_a·(T_AGC − L_est), −12, +R_up ) dB,   T_AGC = −18 LUFS [S]
co blok 100 ms: G += clamp(G_cel − G, −v_dn·0,1, +v_up·0,1)
awaryjnie: gdy L_M + G > T_AGC + 8 LU (głośność chwilowa po wzmocnieniu), w tym bloku v_dn ×3
wzmocnienie: liniowa rampa und(G) w obrębie 100 ms
```
Start: G = 0 dB; przez pierwsze 3 s L_S liczone z dostępnych bloków.

**Suwak:** `k_a = min(1, 2a)` (część korekty; a = 0 → G = 0 dB); `R_up = lin(a, 6, 12) dB`;
`v_up = lin(a, 0,5, 3) dB/s`; `v_dn = lin(a, 2, 8) dB/s`.

#### 8. Multiband 2 – 3 pasma, wolny (latencja 0)

LR4: 200 Hz, 3 kHz. Detektor RMS τ = 50 ms. Kolano 10 dB. Atak 300 ms, zwolnienie 2 s. Zamrażanie: gdy poziom pasma
< −55 dBFS, `s` nie maleje (nie podbijamy szumu w pauzach). Redukcja ≤ 9 dB.

| Pasmo | T0 [dBFS RMS] | L_ref |
|---|---|---|
| < 200 Hz | −22 | −20 |
| 200 Hz–3 kHz | −22 | −20 |
| > 3 kHz | −30 | −28 |

**Suwak:** `R = lin(a, 1, 2,5)`; `T_b = T0_b − 8·a`; makijaż `M_b = 0,6·(1 − 1/R)·(L_ref,b − T_b)` dB. a = 0 → R = 1, M = 0.

#### 9. Kompresor szerokopasmowy (latencja 0)

Detektor RMS τ = 10 ms (L+R). Kolano 6 dB. Dwie gałęzie zwolnienia (3.7): τ_A = 10 ms, τ_R1 = 80 ms, τ_R2 = 1 s.
**Suwak:** `R = lin(a, 1, 3)`; `T = −20 − 4·a dBFS`; `M = 0,7·(1 − 1/R)·(−19 − T)` dB.

#### 10. Multiband 1 – 5 pasm, szybki (latencja 0)

LR4: 100, 400, 1600, 6400 Hz (drzewo z 3.3). Detektor: `x_G = max(RMS τ = 5 ms, szczyt − 9 dB)`. Kolano 6 dB. Redukcja ≤ 12 dB.

| Pasmo | T0 | L_ref | Atak | Zwolnienie |
|---|---|---|---|---|
| 1 (< 100 Hz) | −22 | −20 | 20 ms | 250 ms |
| 2 | −23 | −21 | 15 ms | 200 ms |
| 3 | −25 | −23 | 10 ms | 150 ms |
| 4 | −30 | −28 | 5 ms | 100 ms |
| 5 (> 6,4 kHz) | −36 | −34 | 3 ms | 80 ms |

Sprzężenie pasm (ochrona barwy), na redukcjach `s_b` po balistyce, Δ = 6 dB:
```
dla b = 2…5: s_b = max(s_b, s_{b−1} − Δ);     dla b = 4…1: s_b = max(s_b, s_{b+1} − Δ)
```
**Suwak:** `R = lin(a, 1, 4)`; `T_b = T0_b − 10·a`; `M_b = 0,6·(1 − 1/R)·(L_ref,b − T_b)` dB.

#### 11. Punch (latencja 0)

3 pasma LR4 (150 Hz, 3 kHz). W każdym paśmie, na blok sterujący:
```
e_f: obwiednia szczytowa, atak 0,3 ms, zwolnienie 30 ms
e_s: obwiednia szczytowa, atak 25 ms,  zwolnienie 30 ms      (to samo zwolnienie → różnica ≠ 0 tylko w ataku)
d   = dB((e_f + 1e−3)/(e_s + 1e−3))                           (1e−3: brak działania poniżej ok. −60 dBFS)
G   = min(κ·max(d − 1, 0), G_max) dB;   wygładzenie G: narastanie 1 ms, opadanie 30 ms
```
Wagi pasm: ×1,0 (dół), ×1,0 (środek), ×0,6 (góra – żeby nie wyostrzać sybilantów).
**Suwak:** `κ = 0,8·a`; `G_max = 6·a dB`.

### Bas

#### 12. Głęboki bas (latencja 0)

```
HPF podsoniczny: SVF HP Butterworth 2. rz., f_hp
dzwon SVF 55 Hz, Q = 0,9, wzmocnienie G_dyn (modulowane co blok)
detektor: RMS (τ = 50 ms) wejścia po LP 2. rz. 120 Hz → L_lf [dBFS]
G_dyn = G_b · clamp( (−12 − L_lf)/18, 0, 1 )      (pełne podbicie przy L_lf ≤ −30, zero przy ≥ −12 dBFS)
wygładzenie G_dyn: w dół 20 ms, w górę 300 ms
```
**Suwak:** `G_b = 8·a dB`; `f_hp = 8 + 14·min(1, 2a) Hz` (a = 0: 8 Hz, −0,1 dB przy 20 Hz; od a = 0,5: 22 Hz).

#### 13. Bass Boost (latencja 0)

Statyczna półka niska SVF: 100 Hz, Q = 0,7, `G = 8·a dB`. Bez nasycania (saturator z v0 usunięty: wymagał podziału pasma
i dawał intermodulację; ochronę basu zapewniają pasmo 1 Multibandu 1 i stopień basowy Clippera HD).

#### 14. Bas harmoniczny (latencja 0)

Brakująca podstawowa (Larsen & Aarts; US 5,930,373 – wygasły). Tor boczny mono:
```
m  = (L + R)/2
s  = LP4(110 Hz)( HP 2. rz. 40 Hz (m) )                       // pasmo podstawowych
e  = obwiednia szczytowa |s|: atak 5 ms, zwolnienie 60 ms
u  = clamp( s/(e + 1e−4), −1, 1 )
h  = 1,0·T2(u) + 0,7·T3(u) + 0,35·T4(u)                        // T2 = 2u²−1, T3 = 4u³−3u, T4 = 8u⁴−8u²+1
v  = LP 2. rz. 400 Hz( LP 2. rz. 400 Hz( HP 2. rz. 100 Hz( e·h ) ) )   // usuwa składową stałą i resztki
L += g_h·v;  R += g_h·v
```
Wielomian 4. stopnia na sygnale ≤ 110 Hz daje składowe ≤ 440 Hz – bez aliasingu, bez oversamplingu.
**Suwak:** `g_h = a^1,5` (a = 0,5 → −9 dB, a = 1 → 0 dB względem obwiedni podstawowej).

### Barwa

#### 15. Korektor (latencja 0)

5 biquadów TDF-II, wzmocnienia skalowane suwakiem liniowo w dB; krzywa [S]:

| Filtr | f0 | Q | G przy a = 1 | Projekt |
|---|---|---|---|---|
| półka niska | 105 Hz | 0,71 | +4,0 dB | RBJ |
| pik | 250 Hz | 1,0 | −1,0 dB | RBJ |
| pik | 3 kHz | 1,0 | +1,0 dB | RBJ |
| pik | 6,5 kHz | 1,2 | −1,5 dB | dopasowany (3.4) |
| półka wysoka | 10 kHz | 0,71 | +2,0 dB | dopasowany (3.4) |

**Suwak:** `G_i(a) = a·G_i`. Współczynniki przeliczane raz na blok sterujący tylko, gdy `a` się zmienia (krok ≤ 0,02 dB
na blok – bez „zipperów”).

#### 16. Auto EQ (szyna B)

```
pasma: 1/3 oktawy od 50 Hz do 16 kHz, nie węższe niż 2 prążki (najniższe pasma łączone)
L_b  = 10·log10(moc pasma L+R), uśredniana τ = 8 s tylko gdy poziom ramki > −50 dBFS
cel (poziomy pasm 1/3 okt., dB względem 1 kHz):
   T_b = −1,5·log2(f_b/1000)                    60 Hz ≤ f_b ≤ 8 kHz
   dodatkowo −3 dB/okt. powyżej 8 kHz, −6 dB/okt. poniżej 60 Hz
o    = mean_b(L_b − T_b)                          (usuwa zależność od poziomu)
E_b  = clamp( 0,7·(T_b − (L_b − o)), −G_ae, +G_ae )
wygładzenie E_b po pasmach [¼, ½, ¼], po czasie τ = 1 s; interpolacja liniowa w log f do prążków → G[k] (rzeczywiste)
```
Poprawka względem v0: −4,5 dB/okt. dotyczy gęstości widmowej; w poziomach pasm 1/3 okt. to **−1,5 dB/okt.**
(nachylenie −4,5 na pasmach tercjowych dawałoby bardzo ciemny dźwięk).
**Suwak:** `G_ae = 6·a dB`.

#### 17. Balans widma (szyna B)

Dynamiczne wyrównywanie nierówności widma w skali 1/6 oktawy (nie pojedynczych harmonicznych):
```
M_f[k] = moc wygładzona w ±1/12 okt. (sumy skumulowane);   M_c[k] = moc wygładzona w ±1/2 okt.
x[k]   = 10·log10(M_f/M_c) − θ_b,   θ_b = 3 dB
GR[k]  = min( D_b, s_b·max(x[k], 0) ) · w_f(k),  w_f: 0 poniżej 200 Hz, rampa do 1 przy 400 Hz, 1 do 16 kHz
balistyka na prążek: atak 20 ms, zwolnienie 200 ms;  wygładzenie po częstotliwości [¼, ½, ¼]
G[k]   = und(−GR[k])
```
**Suwak:** `s_b = lin(a, 0, 0,67)` (odpowiada ratio 1…3); `D_b = 9·a dB`.

### Stereo

#### 18. Szerokość (latencja 0)

```
M = (L + R)/2;  S = (L − R)/2
S_h = SVF HP Butterworth 2. rz. 150 Hz (S);   S_l = S − S_h
ρ   = E[L·R]/√(E[L²]·E[R²] + 1e−12),  uśrednianie 100 ms
straż = clamp(ρ/0,4, 0, 1) · clamp( E[M²]/(E[S²] + 1e−12), 0, 1 )
S′  = (1 + w_s·straż)·S_h + (1 − m_s)·S_l
L′ = M + S′;  R′ = M − S′
```
**Suwak:** `w_s = 0,6·a`; `m_s = 0,5·a` (zwężenie basu). a = 0 → S′ = S dokładnie.

#### 19. Rotacja fazy (latencja 0)

4 all-passy 1. rzędu (TPT z 3.1, wyjście `ap`) w kaskadzie, ta sama `f_r` dla L i R.
**Suwak:** `f_r = expo(a, 10, 200) Hz`. On/off: neutralizacja = zjazd `f_r` do 10 Hz w 50 ms, potem przenikanie.

### Wyjście (jeden obiekt `OutputStage` z czterema podmodułami i wspólnym kontekstem `{C, g_d}`)

`C` = sufit (domyślnie und(−1,0)), `θ = C·und(−0,3)` = próg clipperów (0,3 dB zapasu na przerost po filtrach),
`g_d` = dosterowanie Clippera HD (1, gdy wyłączony).

#### 20. Pre-limiter (latencja 64·F)

Limiter z 3.10 na szczycie próbkowym (`p = max(|L|, |R|)`, D = 0), L = 64·F, τ0 = 50 ms.
Cel wyrażony względem tego, co zobaczy clipper: `T = θ·und(X)/g_d` – ogranicza głębokość obcinania do X dB.
**Suwak:** `X = lin(a, 12, 2) dB` (a = 0: działa dopiero 12 dB ponad progiem clippera – praktycznie nigdy).

#### 21. Clipper HD (latencja 63 + 8 + 2×70 = 211; dla F = 2: 127 + 16 + 140)

```
v = g_d·x
— stopień 1 (1×): „bas robi miejsce”
  Lw = box_B(box_B(v)),  B = 64·F           // dwie średnie ruchome w kaskadzie = jądro trójkątne, liniowa faza,
                                            // opóźnienie B−1 = 63; −6 dB przy ≈ 300 Hz; jądro dodatnie (bez dzwonienia)
  Hw = v[n − (B−1)] − Lw                    // dokładne dopełnienie
  e  = maks. przesuwne |Hw| z okna 72·F próbek kończącego się 8·F próbek w przyszłości (czyli Lw, Hw opóźnić o 8·F),
       potem wygładzenie jednobiegunowe τ = 0,3 ms
  θ_L = max(θ − μ·e, 0,25·θ),  μ = 0,6 [S]
  Lc  = f(Lw; próg θ_L, w = 0,25)           // clip z kolanem (3.11) o zmiennym progu
  v1  = Lc + Hw
— stopień 2 (4×), wykonany dwukrotnie:
  u = interpolacja 4× (3.5);  u = f(u; próg θ, w = 0,06);  v ← decymacja 4×
```
Gdy nic nie przekracza progów, stopień 1 odtwarza sygnał dokładnie, a stopień 2 w granicach zafalowania filtrów.
Kanały przetwarzane niezależnie. Po module możliwy przerost do ok. +0,2…+0,6 dB ponad θ (do +1,4 dB dla materiału
z bardzo mocną górą) **[sym]** – przejmuje go Hard Limit; bez Hard Limit sufit nie jest gwarantowany.
**Suwak:** `g_d = und(6·a)`. a = 0: obcina tylko to, co przekracza θ.

#### 22. Prosty clipper (latencja 70)

Jeden przebieg: interpolacja 4× → `f(u; próg θ, w)` → decymacja. Wejście mnożone przez `g_s`.
**Suwak:** `g_s = und(6·a)`; `w = lin(a, 0,25, 0,10)` (łagodne kolano przy małym dosterowaniu).
Przy włączonym Clipperze HD sygnał jest już ≤ θ i moduł praktycznie nie działa – presety włączają jeden z nich.

#### 23. Hard Limit (latencja 20 + 128·F)

```
x_d = g_h·x
p[n] = max po kanałach szczytu 4× (3.6) – detektor na x_d, opóźnienie D_tp = 20
limiter 3.10: T = C·und(−0,10),  L = 128·F,  τ0 = 30 ms,  to samo wzmocnienie dla L i R
```
Bez końcowego clipa próbek. W kompilacji testowej asercja `TP_4×(wyjście) ≤ C`. Gdy moduł jest wyłączony, sufit nie
jest pilnowany (GUI powinno to sygnalizować); linia opóźniająca zostaje.
**Suwak:** `g_h = und(6·a)`. Z włączonym clipperem zalecane a ≤ 0,3 (większe dosterowanie limitera po clipperze pompuje).

---

## 5. Presety (on/off + moc w %)

„–” = wyłączony.

| # | Moduł | Codzienny | Głośno w tramwaju | Stare nagrania | Noc |
|---|---|---|---|---|---|
| 1 | Declipper | 40 | 30 | 70 | 40 |
| 2 | Naprawa MP3 | 40 | 40 | 60 | 40 |
| 3 | Szum i brum | – | – | 55 | – |
| 4 | Dehummer | – | – | 70 | – |
| 5 | Dekwantyzer | 50 | – | 70 | 60 |
| 6 | Bramka szumów | – | – | 35 | – |
| 7 | AGC | 50 | 90 | 60 | 80 |
| 8 | Multiband 1 | 30 | 75 | 25 | 40 |
| 9 | Multiband 2 | 35 | 70 | 45 | 60 |
| 10 | Kompresor | – | 50 | – | 60 |
| 11 | Punch | 25 | 20 | 40 | – |
| 12 | Głęboki bas | 35 | – | 30 | – |
| 13 | Bas harmoniczny | 30 | 70 | 45 | 30 |
| 14 | Bass Boost | – | 30 | 20 | 25 |
| 15 | Korektor | 40 | 60 | 30 | 70 |
| 16 | Auto EQ | 40 | 50 | 80 | 40 |
| 17 | Balans widma | 25 | 40 | 45 | 50 |
| 18 | Szerokość | 20 | 10 | 15 | 10 |
| 19 | Rotacja fazy | – | 60 | – | – |
| 20 | Clipper HD | 25 | 75 | 20 | – |
| 21 | Pre-limiter | 30 | 60 | 30 | 50 |
| 22 | Prosty clipper | – | – | – | 20 |
| 23 | Hard Limit | 0 | 30 | 0 | 0 |

Uzasadnienie w skrócie. **Codzienny:** lekka naprawa, wyrównanie głośności i barwy, mało kompresji.
**Tramwaj:** hałas otoczenia maskuje ciche fragmenty i najniższy bas – mocne AGC i kompresja, głośność z Clippera HD,
bas przeniesiony w harmoniczne (Głęboki bas wyłączony, bo zjada zapas), więcej środka/góry z Korektora.
**Stare nagrania:** pełna sekcja naprawy, silne Auto EQ, Punch przywraca ataki, mało głośności.
**Noc:** mały zakres dynamiki bez podbijania ataków i najniższego basu, kontur głośności z Korektora (ciche słuchanie),
łagodniejsza góra (Balans widma), bez Clippera HD (mniej CPU/baterii). Hard Limit włączony zawsze (moc 0 = sam sufit).

---

## 6. Szacunek CPU (44,1 kHz, stereo)

Jednostka: miliony prostych operacji zmiennoprzecinkowych na sekundę (mnożenie+dodawanie ≈ 1). Przeliczenie na procent
rdzenia PC przy 0,5 ns/operację (x86-64, skalarne f64, bez ręcznego SIMD). To szacunek do weryfikacji benchmarkiem.

| Moduł | M op/s | % rdzenia PC | Uwagi |
|---|---|---|---|
| Infrastruktura (DC, mierniki K WE/WY, wygładzanie) | 4 | 0,2 | |
| Declipper | 13 | 0,65 | ~1 gdy nie ma przesteru |
| Dekwantyzer | 3 | 0,15 | liczy tylko w cichych fragmentach |
| Dehummer | 9 | 0,45 | 0,5 gdy brak brumu |
| Szyna STFT A (FFT, okna) | 10,5 | 0,5 | wspólna dla 2 i 3 |
| Naprawa MP3 | 7 | 0,35 | |
| Szum i brum | 8 | 0,4 | |
| Bramka szumów | 12 | 0,6 | |
| AGC | 2 | 0,1 | |
| Multiband 2 | 8 | 0,4 | |
| Kompresor | 1 | 0,05 | |
| Multiband 1 | 18 | 0,9 | 32 sekcje SVF |
| Punch | 9 | 0,45 | |
| Głęboki bas | 3,5 | 0,2 | |
| Bas harmoniczny | 4 | 0,2 | |
| Bass Boost | 1 | 0,05 | |
| Korektor | 4 | 0,2 | |
| Szyna STFT B | 9,5 | 0,5 | wspólna dla 16 i 17 |
| Auto EQ | 1,5 | 0,1 | |
| Balans widma | 7 | 0,35 | |
| Szerokość | 2,5 | 0,1 | |
| Rotacja fazy | 2 | 0,1 | |
| Pre-limiter | 3 | 0,15 | |
| Clipper HD | 21 | 1,05 | 2 przebiegi 4× |
| Prosty clipper | 9 | 0,45 | |
| Hard Limit | 13 | 0,65 | 240 MAC/próbkę na interpolator |
| **Razem (wszystko włączone)** | **≈ 185** | **≈ 9 %** | cel < 15 % |

Android (średni telefon, ~3× wolniej na rdzeń): ≈ 25–28 % – na granicy celu 25 %. Rezerwy, w kolejności stosowania:
(1) FIR-y (oversampling, interpolator TP) w f32 z autowektoryzacją/NEON: −15 M; (2) Clipper HD jeden przebieg 4×: −9 M;
(3) interpolator TP z Th = 12: −4 M; (4) L/R w SVF jako para (struktura 2-elementowa, autowektoryzacja): −15…20 M;
(5) Bramka szumów 2-pasmowa: −6 M. Po (1)–(3) ≈ 157 M → ok. 22 %.

---

## 7. Testy jednostkowe prymitywów (minimum)

- SVF/LR4: suma pasm 3/4/5 = all-pass, |H| = 1 ± 0,001 dB; stabilność przy skoku fc 20 Hz ↔ 18 kHz co blok.
- Filtr dopasowany: wartości kontrolne z 3.4; odchyłka od prototypu ≤ 0,15 dB dla filtrów Korektora.
- Ważenie K: współczynniki @48 kHz = tabela normy (12 cyfr); sinus 997 Hz −23 dBFS stereo → −23,0 ± 0,05 LUFS.
- Półpasmowe: tłumienie ≥ 80 dB od podanych częstotliwości; góra+dół = czyste opóźnienie 70 próbek ± 0,001 dB do 20 kHz.
- Interpolator TP: sinus fs/4 z fazą 45° (szczyt 0 dBFS, próbki −3,01 dB) → odczyt 0 ± 0,02 dB; sinusy do 20 kHz ± 0,02 dB.
- STFT: tożsamość (bez modyfikacji) = opóźnienie N, błąd < 1e−12; wstawiony szum o mocy P → pomiar P ± 0,5 dB.
- Limiter 3.10: szum różowy/biały +10 dB ponad cel → własny pomiar TP wyjścia ≤ T + 0,01 dB; sygnał poniżej celu → wyjście
  = wejście opóźnione, bit-exact.
- Clip z kolanem: f i F ciągłe w x1, x2 (różnice < 1e−12).
- Każdy moduł: a = 0 → różnica względem opóźnionego wejścia ≤ wartość podana w opisie (dla większości 0 albo all-pass);
  przemiatanie a 0→1 w 2 s na sinusie 100 Hz i 1 kHz – detektor nieciągłości; on/off co 200 ms – brak trzasków; brak NaN
  dla ciszy, DC, impulsu, pełnej skali ±1 i sygnału +12 dBFS.

---

## 8. Zmiany względem v0

1. Tor: Declipper i Dekwantyzer przed DC-blokiem; Dehummer przed szyną A; w szynie A Szum przed Naprawą MP3;
   Szerokość po barwie; Rotacja fazy przeniesiona tuż przed stopień wyjściowy.
2. Latencja domknięta co do próbki: 4096 @44,1/48 kHz (tabela 2.2), z linią wyrównującą 11 próbek; zarys trybu 1024.
3. Szyny STFT: A = 2048/512, B = 1024/256 (v0: dwie po 2048 – nie mieściły się w budżecie); okna Hann/Hann ze skalą
   1/(1,5·N) zamiast sqrt-Hann; jawna latencja N; współczynnik ×2 przy wstawianiu szumu.
4. LR4: drzewo zbalansowane z 4 all-passami dla 5 pasm, realizacja na SVF (3 filtry na zwrotnicę), podane też 3 i 4 pasma.
5. SVF: pełne wzory (g, k, a1–a3, wyjścia, dzwon, półki) i filtr 1. rzędu TPT.
6. Filtry dopasowane: własne wyprowadzenie 3-punktowe z jawnymi wzorami i zakresem stosowania (f0 ≤ fs/4).
7. Ważenie K: wzory dla dowolnego fs z parametrów analogowych, sprawdzone z tabelą normy.
8. True-peak: własny interpolator 161-współczynnikowy (Th = 20, Kaiser β = 5,5) zamiast 48-współczynnikowego z normy.
9. Hard Limit: jeden przebieg, okno minimum L+1 + zwolnienie + średnia L z dowodem; cel C − 0,10 dB; usunięty końcowy
   twardy clip próbek; suwak = dosterowanie 0…+6 dB; zwolnienie adaptacyjne zdefiniowane wzorem.
10. Clipper HD: nowy algorytm – liniowofazowy podział jądrem trójkątnym, próg basu zależny od obwiedni reszty pasma,
    potem 2 przebiegi clip+decymacja na 4× (zamiast 3 iteracji clip-and-filter z filtrem pasmowym zniekształceń).
11. Prosty clipper: clip z kolanem parabolicznym na 4× bez ADAA; ADAA opisane (funkcja, pierwotna, wzór) i odrzucone
    z uzasadnieniem (pół próbki opóźnienia, −2,4 dB przy 20 kHz na 2×).
12. Oversampling: konkretne długości (127 i 27, β = 9), opóźnienie 70 próbek na przebieg, wariant 63-tap.
13. Dynamika liczona w tempie sterowania (co 16 próbek) z interpolacją wzmocnienia; wzory kolana, balistyki, dwóch gałęzi
    zwolnienia, ekspandera; tabele progów/czasów dla Multibandów; makijaż automatyczny; sprzężenie pasm wzorem.
14. Declipper: dwa tryby detekcji (twardy / po kodeku stratnym), AR rząd 32 z równaniami Janssena na serię, 2 iteracje,
    limity (seria ≤ 64 AR, ≤ 192 Hermite, dłuższe bez zmian; > 35 % obciętych – bez zmian); usunięte stałe −3 dB.
15. Naprawa MP3: dziury wykrywane względem historii pasma (nie mediany widma – ta wypełniałaby doliny między
    harmonicznymi), dolna granica 3 kHz zamiast 600 Hz; kopiowanie pasma z przesunięciem o wielokrotność 4 prążków;
    pre-echo w dziedzinie czasu z wyprzedzeniem danym przez szynę.
16. Szum: uproszczone minimum statistics z medianą po częstotliwości i limitem poziomu; podłoga 0…−15 dB (a = 0 przezroczyste).
17. Dehummer: detekcja kwadraturowa ze strojeniem ±2 %, dzwony o ujemnym wzmocnieniu zamiast notchy.
18. Dekwantyzer: nowy algorytm (wygładzanie z rzutowaniem na przedział kwantyzacji, błąd ≤ q/2, wykrywanie siatki).
19. AGC: cel −18 LUFS, bramka względna, częściowa korekta przy małym suwaku, prędkości jako funkcje suwaka.
20. Auto EQ: cel poprawiony na −1,5 dB/okt. w poziomach pasm tercjowych; Balans widma działa w skali 1/6 okt. od 200 Hz.
21. Bass Boost bez saturatora; Głęboki bas z jawną regułą dynamiczną; Bas harmoniczny – pełny tor z wagami Czebyszewa.
22. Pre-limiter: próg zdefiniowany jako maksymalna głębokość obcinania (2…12 dB ponad próg clippera), nie −3 dB pod sufitem.
23. On/off dwufazowe (neutralizacja → przenikanie); mapowania suwaków dla wszystkich 23 modułów z a = 0 przezroczystym
    (wyjątki opisane: all-pass zwrotnic, HPF 8 Hz w Głębokim basie).
24. Dodane: 4 presety, tabela CPU (≈ 9 % rdzenia PC, plan oszczędności dla Androida), lista testów prymitywów.
