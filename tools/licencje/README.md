# Lista elementów cudzych

1. `cargo metadata --format-version 1 > %TEMP%\szklo_meta.json`
2. `.venv\Scripts\python.exe tools\licencje\licencje.py`
3. `.venv\Scripts\python.exe tools\licencje\licencje_pliki.py`

Wynik: `app-pc/ui/licencje.txt`, `app-android/app/src/main/assets/licencje.txt`, `THIRD_PARTY.md`. Uruchamiaj przy każdej zmianie zależności.
