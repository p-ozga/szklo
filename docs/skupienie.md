# Skupienie i Wyciszenie – skąd te filtry i presety

To opis decyzji projektowych, nie porada medyczna. Filtry niczego nie leczą; mają sprawić, żeby muzyka mniej przeszkadzała
w pracy albo mniej pobudzała. Jeśli coś pogarsza samopoczucie, wyłącz to.

Źródła zebrane 25.11.2026. Liczby pochodzą z pełnych tekstów albo ze streszczeń prac (część tylko ze streszczeń), więc przed
cytowaniem dalej warto je sprawdzić w oryginale.

## Co mówią badania

| Ustalenie | Źródło | Siła dowodu | Co z tego zrobiliśmy |
|---|---|---|---|
| Stały szum poprawia wyniki zadań u osób z ADHD lub dużą nieuwagą (g ≈ 0,25), a u pozostałych lekko je pogarsza (g ≈ −0,21) | Nigg i in. 2024, JAACAP, metaanaliza 13 badań; Helps i in. 2014; Söderlund i in. 2007 | umiarkowana (głównie dzieci, szum 65–80 dB bez muzyki) | filtr **Szum tła**, domyślnie wyłączony |
| Działa samo stałe pobudzenie: ton 100 Hz pomagał jak szum różowy | Rijmen i in. 2024, Neuropsychologia | pojedyncze badanie | szum bez żadnej modulacji; barwa mało ważna |
| Zrozumiałe słowa szkodzą czytaniu i pamięci; muzyka instrumentalna prawie nie | Perham i Currie 2014; Souza i Barbosa 2023 | mocna | filtr **Mniej słów** |
| Szkoda rośnie ze zrozumiałością mowy (od STI ok. 0,2, pełna ok. 0,45) | Hongisto 2005; Haapakangas i in. 2020 | mocna (biura, nie muzyka) | ściszanie środka w paśmie mowy + szum, który przykrywa słowa |
| Przeszkadza dźwięk, który się ZMIENIA; miarą jest siła fluktuacji głośności, największa ok. 4 Hz | Schlittmeier i in. 2012; Jones i Macken 1993 | umiarkowana | filtr **Równy strumień** |
| U dorosłych z ADHD dźwięk w tle zakłóca bardziej | Pelletier i in., J Atten Disord | pojedyncze badanie | wszystkie trzy filtry razem w presecie Skupienie |
| Muzyka pomagała chłopcom z ADHD w zadaniach (własny wybór muzyki) | Abikoff i in. 1996; Madjar i in. 2020 | słaba–umiarkowana | muzyka zostaje; filtry jej nie zastępują |
| Tempo i głośność podnoszą pobudzenie | Husain i in. 2002; Thompson i in. 2001 | mocna | Wyciszenie: ciszej i równiej; tempa nie zmieniamy |
| Nadwrażliwość słuchowa jest w ADHD częsta (ok. 22–43% dorosłych w ankiecie) | Bijlenga i in. 2017 | słaba (bez grupy kontrolnej) | łagodniejsza góra (Kaptur) w obu presetach |
| Modulacja muzyki 16 Hz pomagała osobom z objawami ADHD | Woods i in. 2024 (autorzy z firmy Brain.fm) | słaba (p = 0,048, brak niezależnej replikacji) | **nie robimy** – patrz niżej |
| Dudnienia różnicowe (binaural beats) | przeglądy 2022–2023 | brak wiarygodnych dowodów | nie robimy |

### Choroba afektywna dwubiegunowa – dowodów jest mało

- W manii i hipomanii większość badanych zgłasza wyostrzone zmysły (Parker i in. 2017, N = 66); w ankiecie „głośna muzyka”
  była wśród wyzwalaczy podawanych przez samych chorych (Proudfoot i in. 2012). Badań kontrolowanych nie ma.
- Słabsze „bramkowanie” bodźców słuchowych (P50) jest dobrze udokumentowane (metaanaliza, ok. 570 pacjentów).
- Muzyka przed snem poprawia jakość snu u dorosłych (Cochrane, Jespersen 2022), ale nie badano tego w tej chorobie.
- Wniosek ostrożny: preset **Wyciszenie** zmniejsza bodźce (ciszej, równiej, bez wyostrzania, miększa góra). To komfort,
  nie terapia. Sen i leczenie są ważniejsze niż jakikolwiek filtr.

## Filtry

**Mniej słów** (Stereo) – ścisza to, co jest dokładnie na środku sceny w paśmie 350 Hz–4,6 kHz, czyli zwykle śpiew
(do −16 dB czystego środka). Instrumenty po bokach, bas i najwyższa góra zostają. Pomiar na 15 utworach: środek w paśmie mowy
ciszej o 3 dB względem całości (od 0 dB w muzyce bez wokalu do −6 dB w reggae). Słów nie usuwa – zmniejsza ich wyrazistość.

**Równy strumień** (Dynamika) – zmniejsza wahania głośności wokół średniej w sześciu pasmach: uderzenia ciszej, dołki głośniej,
barwa i średnia głośność bez zmian. Pomiar: wahania 2–8 Hz mniejsze o 4,4 dB przy 50% i ok. 6 dB (połowa) przy 75–100%.
Dla porównania zwykłe kompresory z tego programu prawie ich nie zmieniają (0…−1 dB), a Kryształ je zwiększa (+6 dB).

**Szum tła** (Barwa) – cichy szum różowy, osobny dla lewego i prawego ucha, bez dudnienia i bez ostrej góry. Poziom idzie za
muzyką: od 27 dB pod nią (suwak blisko zera) do 8 dB pod nią (100%). Po 3 s ciszy sam wygasa. Badań szumu zmieszanego
z muzyką nie ma, więc poziom ustaw sam: ma być ledwo zauważalny, nie męczący.

## Presety

| Moduł | Skupienie | Wyciszenie |
|---|---:|---:|
| Mniej słów | 85 | – |
| Równy strumień | 60 | 80 |
| Szum tła | 45 | – |
| Kaptur (przestrzeń, miększa góra) | 30 | 45 |
| AGC (równa głośność między utworami) | 60 | 70 |
| Multiband 2 / Kompresor | 30 / – | 40 / 40 |
| Ciepły bas / Radiowy bas | 45 / 40 | 30 / – |
| Głęboki bas / Bas harmoniczny | 25 / 25 | – |
| Auto EQ / Balans widma | 40 / 30 | 40 / 50 |
| Szerokość | 15 | 10 |
| Kryształ, Punch, Dynamiczna góra, clippery | wyłączone | wyłączone |

Zmierzone działanie (mediana z 30 fragmentów, 15 gatunków): Skupienie – wahania głośności 2–8 Hz −5,8 dB, środek w paśmie
mowy −3 dB, głośność −6,5 LU. Wyciszenie – wahania −5,6 dB, głośność −7,1 LU.

Dlaczego w Skupieniu zostaje bas i przestrzeń: stałe, przewidywalne pobudzenie pomaga (model „umiarkowanego pobudzenia
mózgu”, Sikström i Söderlund 2007), a równy bas i szeroka scena dają je bez nowych zdarzeń przyciągających uwagę.

## Jak używać

- Najwięcej daje wybór muzyki: instrumentalna, równa, znana. Filtry pomagają, gdy puszczasz coś innego.
- Głośność umiarkowana (ok. 60–70 dB). Szum i muzyka razem nie powinny być głośniejsze niż zwykła rozmowa.
- Korzyść w badaniach była największa, gdy bodziec był nowy. Zmieniaj playlistę albo rób przerwy.
- Szum tła wyłącz, jeśli po kilku minutach nadal go zauważasz albo męczy.

## Czego celowo nie ma

- **Modulacji muzyki w rytmie 8–32 Hz.** Dowód jest słaby (jedno badanie producenta), a metoda jest objęta kilkoma
  patentami firmy BrainFM w USA (m.in. US 11,205,414, US 11,532,298, US 12,406,651). Żaden nasz filtr nie moduluje dźwięku
  w zadanym rytmie; Równy strumień robi coś odwrotnego – usuwa część wahań, które już są w muzyce.
- **Dudnień różnicowych i tonów izochronicznych** – brak wiarygodnych dowodów.
- **Rozpoznawania mowy, wydzielania ścieżek, mikrofonu.** Filtry działają tylko na odtwarzanym dźwięku.

## Patenty sprawdzone dla tych filtrów (nie jest to porada prawna)

- Mniej słów: US 6,405,163 (Creative, widmowe usuwanie głosu) i US 6,909,787 (MediaTek, odejmowanie sumy mono z torem górnym)
  wygasły. US 8,045,719 (Dolby, do 2028) dotyczy wyprowadzania kanału środkowego z modelem uszu – u nas nie ma kanału
  środkowego ani takiego modelu. US 8,705,769 (STMicro) – według Google wygasł za nieopłacenie; dotyczy wektorów widmowych
  i wyjścia trzykanałowego.
- Szum tła: patenty Bose (US 8,964,997, US 9,503,803) wymagają pomiaru hałasu otoczenia mikrofonem – tu go nie ma, poziom
  zależy tylko od muzyki.
- Równy strumień: każde pasmo steruje tylko sobą (US 9,258,031 wymaga zależności między pasmami); bez ramek i współczynnika
  szczytu (US 8,879,750).
