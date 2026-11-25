# Słuch – środowisko do badania, czy filtry SŁYCHAĆ

Pytanie nie brzmi „czy filtr zmienia sygnał”, tylko „czy człowiek usłyszy różnicę”. Środowisko puszcza fragmenty 15 utworów
(15 gatunków) przez każdy filtr przy kilku położeniach suwaka i ocenia różnicę modelem ludzkiego słuchu.

## Materiał

Po jednym utworze z 15 gatunków: pierwsze miejsce listy gatunku w serwisie Jamendo (darmowe słuchanie i pobieranie, licencje
Creative Commons), stan z 25.11.2026. Lista z autorami i licencjami: `utwory.csv`. Utworów NIE ma w repozytorium (licencje
BY-NC-ND/BY-NC-SA: tylko użytek własny, bez rozpowszechniania przeróbek) – pobiera się je do `dane/mp3/` jako
`<gatunek>_<id>.mp3` ze strony utworu.

Z każdego utworu dwa fragmenty po 12 s: najgłośniejszy i spokojny (20. percentyl głośności). Do filtrów naprawczych
dodatkowo wersje z usterką: przester (+8 dB i obcięcie), szum −48 dBFS, brum 50 Hz, 8 bitów, MP3 o niskiej przepływności.

## Model słuchu (`ucho.py`)

1. Poziom odsłuchu: RMS 1,0 ≙ 94 dB SPL (muzyka ok. 74 dB SPL).
2. Filtry słuchowe: 40 pasm po 1 ERB, kształt roex (Glasberg i Moore 1990; Moore i Glasberg 1983) – daje maskowanie.
3. Próg słyszenia w ciszy (Terhardt 1979) jako szum wewnętrzny – zmian poniżej progu nie liczymy.
4. Cztery wymiary w progach słyszalności (JND):
   - **głośność** – ITU-R BS.1770, 0,8 dB = 1 JND;
   - **barwa** – długookresowy wzorzec pobudzenia po wyrównaniu głośności, 1 dB w czterech sąsiednich pasmach = 1 JND;
   - **dynamika** – zmienna w czasie część różnicy (kompresja, wyostrzanie, dodany szum), ramki 12 ms, 8 grup pasm;
   - **przestrzeń** – bok/środek z progiem wewnętrznym −17 dB (odtwarza progi korelacji międzyusznej: 0,04 przy ρ = 1,
     0,35 przy ρ = 0 – Pollack i Trittipoe 1959; Gabriel i Colburn 1981), 3 dB = 1 JND; balans 1 dB = 1 JND.
5. Dwie miary rozpraszania (nie progi): **fluktuacja** – głębokość wahań obwiedni 2–8 Hz w paśmie mowy (Schlittmeier i in.
   2012), **środek** – poziom środka sceny w paśmie mowy względem całości (przybliżenie „ile wokalu”).

Kontrola modelu: `python tools/sluch/ucho.py <plik.wav>` – 20 manipulacji o znanej słyszalności (głośność ±0,3…2 dB, obrót
fazy, półki 0,5–3 dB, szum −96/−70/−50 dBFS, obcięcie 16 kHz, szerokość, falowanie głośności).

### Skala – i poprawka na prawdziwe słuchanie

1 JND to próg wprawnego słuchacza w bezpośrednim porównaniu A/B. Zwykłe słuchanie jest dużo mniej czułe. Kotwice z uwag
użytkownika: Ciepły bas v2 przy 100% = 3,5 JND – „działa, jakby nie mógł”. Dlatego:

| JND | znaczenie |
|---|---|
| < 1 | niesłyszalne |
| 1–2 | tylko w bezpośrednim A/B |
| 2–4 | słabe („jakby nie mógł”) |
| 4–8 | wyraźne |
| > 8 | bardzo mocne |

Dobry filtr powinien przy 100% dawać co najmniej ok. 5 JND i zaczynać być słyszalny poniżej 25% suwaka.

## Przester

`ucho.przester(x, y, fs)` – ile wyjścia nie da się wyjaśnić wolno zmiennym filtrem liniowym (dB względem sygnału, pasmo
600 Hz–16 kHz). Kotwice: lekkie obcięcie szczytów −38, wyraźne −21, mocne −13. Dla filtrów przestrzennych i Szumu tła liczba
obejmuje celowo dodany dźwięk. Bas filtrów z podziałem pasma: `przester_proba.py moduly <klucze>` (30 czystych tonów),
własny zestaw z `%APPDATA%\Szklo\ustawienia.json`: `przester_proba3.py`.

Strona z wynikami: `python tools/sluch/strona.py <etykieta> [etykieta sprzed strojenia]` → `wyniki/<etykieta>.html`.

## Użycie

```powershell
.venv\Scripts\python.exe -I tools\sluch\przygotuj.py          # fragmenty i usterki (raz)
.venv\Scripts\python.exe tools\sluch\badaj.py --presety       # wszystkie filtry × 5 położeń × 30 fragmentów (ok. 6 min)
.venv\Scripts\python.exe tools\sluch\badaj.py --moduly space,hood --poziomy 25,50,100 --etykieta proba
.venv\Scripts\python.exe tools\sluch\badaj.py --zestaw "hood=on:0.5 bed=on:0.3" --etykieta moj_zestaw
```

Wyniki: `wyniki/<etykieta>.csv` (każdy pomiar) i `wyniki/<etykieta>.md` (tabele). `--cli` pozwala zbadać starszą wersję.

## Ograniczenia

Model jest uproszczony (brak maskowania w czasie, stały poziom odsłuchu, jedna para uszu „średnia”), a wagi między wymiarami
są dobrane do znanych progów, nie zmierzone na słuchaczach. To narzędzie do porównywania filtrów i wyłapywania martwych
suwaków, nie zastępstwo odsłuchu.
