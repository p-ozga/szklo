# Brief dla Claude Design: odtwarzacz Szkła połączony ze Spotify (4 wersje)

Wklej poniższy tekst do Claude Design w projekcie Szkła.

---

Zaprojektuj **4 różne wersje** panelu odtwarzacza dla aplikacji **Szkło**, czyli procesora dźwięku na Windows. Ten panel jest na **odwrocie okna**: okno obraca się w 3D jak karta. Na przodzie są moduły DSP, a na odwrocie źródło dźwięku, presety i ten odtwarzacz.

**Styl (obowiązkowy):**
- Ciemne „szkło”: tło `#061214`, tekst `#EEF9FA` (68% i 60% krycia dla tekstu drugorzędnego), akcent `#1FB8C9` / `#25E2F7`. Używaj wyłącznie palety opartej na `#1FB8C9`, bez zieleni i żółci typowych dla starych odtwarzaczy.
- Tło okna jest półprzezroczyste (ok. 15%) z rozmyciem jak mrożone szkło. Karty z treścią są nieprzezroczyste, mają zaokrąglone rogi ~22 px i delikatną jasną obwódkę.
- Czcionki: Space Grotesk w tytułach, Inter w treści.
- Okno ma ok. 1280×800 px.

**Stan zwinięty (mała karta, wspólny dla wszystkich wersji):** zaokrąglona karta z tytułem i wykonawcą, paskiem postępu z czasem oraz małymi przyciskami: losowo, poprzedni, play/pauza, następny, powtarzanie. W rogu jest ikona „powiększ”. Ma tam też być wejście do źródeł: „Dodaj pliki z dysku” i „Spotify”. Wymyśl, jak to pokazać graficznie.

**Stan powiększony (tu różnią się 4 wersje):** karta wypełnia całe okno.
- Przyciski zostają małe, tej samej wielkości co w stanie zwiniętym. Rozciąga się tylko pasek postępu.
- Na górze jest **wizualizacja w duchu klasycznych odtwarzaczy**: duży cyfrowy zegar czasu (np. `00:51`), wskaźnik odtwarzania i **słupkowy analizator widma** ze szczytami. Kolory mają być z naszej palety (turkus do bieli), nie zielono-żółte.
- **Spotify:** biblioteka wszystkich playlist użytkownika (lista lub siatka okładek) oraz **kolejka utworów**, czyli lista „Następne” z numerem, tytułem, wykonawcą i czasem; bieżący utwór wyróżniony.
- **Pliki z dysku:** ta sama lista utworów, tylko ze źródłem „Dysk”.
- Przycisk **„Wizualizacja pełnoekranowa”**. Po kliknięciu cała karta wypełnia się wizualizacją, znikają wszystkie kontrolki, a zostaje tylko mała ikona „zmniejsz” w rogu.

**Proponowane kierunki wersji:**
1. **Klasyczny** – zegar i analizator na górze, pod nimi dwie kolumny: playlisty po lewej, kolejka po prawej.
2. **Okładkowy** – duża okładka bieżącego utworu z analizatorem pod nią, playlisty jako siatka okładek.
3. **Kompaktowy pro** – gęste tabele w stylu menedżera playlist (sortowanie, czas łączny), analizator jako wąski pasek.
4. **Własna propozycja** – coś, co najlepiej łączy te funkcje.

**Pokaż w każdej wersji:** stan zwinięty, stan powiększony ze Spotify i tryb wizualizacji pełnoekranowej (karta wypełniona wizualizacją, sama ikona „zmniejsz”).

**Zakazy:** żadnych logo ani nazw cudzych odtwarzaczy, żadnych kopii ich skórek. „Spotify” może się pojawić tylko jako nazwa źródła, bez ich logo.
