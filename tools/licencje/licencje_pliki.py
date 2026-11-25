import json, os

L = json.load(open(os.path.join(os.environ['TEMP'], 'szklo_lic.json'), encoding='utf-8'))
ROOT = r'D:\Soft\DSP'

WSPOLNE = """Szkło zawiera elementy, które NIE są naszą własnością. Należą do ich autorów i są używane zgodnie z ich licencjami:

• Czcionka Inter – Rasmus Andersson i współtwórcy, licencja SIL Open Font License 1.1.
• Czcionka Space Grotesk – Florian Karsten i współtwórcy, licencja SIL Open Font License 1.1.
• Filtr „Binauralny”: pomiary HRTF manekina KEMAR – Bill Gardner i Keith Martin, MIT Media Lab, © 1994 MIT Media Laboratory; udostępnione bez ograniczeń użycia z prośbą o podanie autorów (sound.media.mit.edu/resources/KEMAR.html). KEMAR to znak swojego właściciela.
• Biblioteki open source (pełna lista niżej) – każda należy do swoich autorów; licencje MIT, Apache-2.0, BSD, MPL-2.0, ISC i podobne. Teksty licencji są w repozytoriach podanych przy każdej pozycji.
"""

PC = WSPOLNE + """• Microsoft Edge WebView2 Runtime – Microsoft Corporation; wyświetla okno programu (składnik systemu Windows).
• Equalizer APO – Jonas Thedering, licencja GNU GPL 2.0. To osobny program instalowany przez użytkownika; Szkło go nie zawiera, dostarcza tylko własną wtyczkę, którą Equalizer APO ładuje.
• VST to zarejestrowany znak towarowy Steinberg Media Technologies GmbH. Wtyczka Szkła korzysta z otwartej biblioteki „vst” (RustAudio, MIT); Szkło nie jest powiązane ze Steinberg.
• Spotify to znak towarowy Spotify AB. Szkło nie jest powiązane ze Spotify ani przez nie wspierane. Odtwarzanie Spotify w Szkle działa dzięki librespot (librespot-org i współtwórcy, MIT) – nieoficjalnemu, otwartemu klientowi; wymaga konta Premium i działa na ryzyko użytkownika.
• Wbudowany odtwarzacz: dekodowanie Symphonia (Philip Deljanov i współtwórcy, MPL-2.0), przeliczanie częstotliwości rubato (Henrik Enquist, MIT), widmo RustFFT (MIT/Apache-2.0), okno wyboru plików rfd (MIT).
• Mrożone szkło okna: window-vibrancy (Tauri Programme within The Commons Conservancy, MIT/Apache-2.0).
• MilkDrop 3 – osobny program autorstwa milkdrop2077 (Serge Blanc), oparty na BeatDrop (Maxim Volskiy) i MilkDrop 2 (Ryan Geiss / Nullsoft). Kod MilkDrop 3 jest na licencji BSD-3 (Copyright (c) 2018 Maxim Volskiy and individual contributors). Szkło go NIE zawiera: na żądanie użytkownika (przycisk „Pobierz i zainstaluj”) pobiera oficjalny instalator autora z github.com/milkdrop2077/MilkDrop3/releases i go uruchamia, a potem tylko uruchamia program i pokazuje jego okno w karcie odtwarzacza. Instalator zawiera też presety wielu autorów – na ich warunkach.
"""

ANDROID = WSPOLNE + """• Android Jetpack (AndroidX, Compose, Material 3) – Google LLC, Apache-2.0. Kotlin i kotlinx.coroutines – JetBrains s.r.o., Apache-2.0.
• Odbiornik Spotify Connect działa dzięki librespot (librespot-org i współtwórcy, MIT) – nieoficjalnemu, otwartemu klientowi Spotify. Dekodowanie dźwięku: Symphonia (Philip Deljanov i współtwórcy, MPL-2.0).
• Spotify i Spotify Connect to znaki towarowe Spotify AB. Szkło nie jest powiązane ze Spotify ani przez nie wspierane; wymaga konta Premium i działa na ryzyko użytkownika.
"""


def plik(naglowek, sekcje):
    t = naglowek + '\n'
    for tytul, lista in sekcje:
        t += f'\n{tytul} ({len(lista)}):\n' + '\n'.join('  ' + x for x in lista) + '\n'
    return t


pc_txt = plik(PC, [('Biblioteki programu na PC', L['pc']), ('Biblioteki wtyczki dla Equalizer APO', L['plugin'])])
an_txt = plik(ANDROID, [('Biblioteki rdzenia (Rust)', L['android'])])
for f in ('OFL-Inter.txt', 'OFL-SpaceGrotesk.txt'):
    an_txt += '\n\n===== ' + f + ' =====\n' + open(os.path.join(ROOT, 'app-pc', 'ui', 'fonts', f), encoding='utf-8').read()

open(os.path.join(ROOT, r'app-pc\ui\licencje.txt'), 'w', encoding='utf-8', newline='\n').write(pc_txt)
os.makedirs(os.path.join(ROOT, r'app-android\app\src\main\assets'), exist_ok=True)
open(os.path.join(ROOT, r'app-android\app\src\main\assets\licencje.txt'), 'w', encoding='utf-8', newline='\n').write(an_txt)

md = '# Elementy cudze w Szkle (licencje i znaki towarowe)\n\n'
md += 'Ten plik generuje skrypt z `cargo metadata`. Ta sama treść jest w aplikacjach: PC (odwrót okna → „O programie i licencje”) i Android (odwrót → „O programie i licencje”).\n'
md += 'Przy każdej nowej zależności lub zasobie trzeba zaktualizować oba pliki `licencje.txt` i ten dokument.\n\n'
md += '## PC\n\n```\n' + pc_txt + '```\n\n## Android\n\n```\n' + an_txt + '```\n'
open(os.path.join(ROOT, 'THIRD_PARTY.md'), 'w', encoding='utf-8', newline='\n').write(md)
print(len(pc_txt), len(an_txt))

