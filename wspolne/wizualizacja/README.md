# Wizualizacja muzyki

Wspólna dla programu na PC (WebView2) i aplikacji Android (WebView).

| Plik | Co |
|---|---|
| `butterchurn.min.js` | silnik: Butterchurn 2.6.7, Jordan Berg i współtwórcy, licencja MIT (`LICENSE-butterchurn.txt`) – otwarta implementacja formatu presetów MilkDrop w WebGL 2. Plik z paczki npm bez zmian poza usunięciem linii `sourceMappingURL` |
| `presety.js` | presety Szkła – własne, licencja jak reszta projektu (MIT) |
| `wiz.js` | uruchamianie, zmiana presetów, przyjmowanie fali dźwięku od programu |
| `index.html` | strona dla Androida i do prób: `index.html?test=1` (sztuczna muzyka), `&preset=N`, `&fps=N` |

Dźwięk nie przechodzi przez Web Audio: program wywołuje `SzkloWiz.fala(base64)` z 2048 bajtami (1024 próbki lewego
i 1024 prawego kanału, 8 bitów ze środkiem 128), ok. 30 razy na sekundę.

MilkDrop to wizualizacja Ryana Geissa (Nullsoft). Szkło nie jest z nią powiązane i nie zawiera jej kodu ani presetów
innych autorów.
