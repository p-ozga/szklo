# Szkło w Equalizer APO – cały dźwięk systemu (Spotify, YouTube, Bluetooth)

Equalizer APO działa jako APO Windows (w procesie usługi audio `audiodg.exe`), więc przetwarza dźwięk
**każdej** aplikacji, bez sterownika i bez wirtualnego kabla. Hostuje wtyczki **VST 2.x** (nie VST3) –
dlatego Szkło dostarcza wtyczkę VST2 `szklo_vst2.dll` (64-bit).

## Instalacja (jednorazowo)

1. Pobierz i zainstaluj Equalizer APO (64-bit) ze strony projektu na SourceForge.
2. W „Configurator” zaznacz urządzenie, na którym słuchasz (np. słuchawki Bluetooth – „Słuchawki”/„Headphones”).
   Uwaga: urządzenia BT mają czasem dwa punkty końcowe (stereo i zestaw głośnomówiący) – zaznacz **stereo**.
3. Uruchom ponownie komputer (wymagane przez Equalizer APO).
4. Skopiuj `szklo_vst2.dll` (z `target\release\`) do `C:\ProgramData\Szklo\` (folder `VSTPlugins` w Program Files
   wymaga administratora). Usługa audio Windows działa jako LOCAL SERVICE – nadaj jej odczyt:
   `icacls C:\ProgramData\Szklo /grant *S-1-5-19:(OI)(CI)RX`
5. W `C:\Program Files\EqualizerAPO\config\config.txt` (folder jest zapisywalny dla użytkowników; zrób kopię) wpisz:
   `VSTPlugin: Library "C:\ProgramData\Szklo\szklo_vst2.dll"`
   – albo dodaj wtyczkę w „Configuration Editor” (`+` → *Plugins* → *VST Plugin*). Equalizer APO przeładuje się sam.
6. **Urządzenie domyślne:** Spotify i przeglądarka grają na domyślne wyjście. Equalizer APO działa tylko na urządzeniach
   zaznaczonych w Configuratorze – jeśli domyślne są np. głośniki laptopa, Szkło ich nie przetwarza.
6. Uruchom aplikację **Szklo.exe** → odwróć okno (kliknij logo albo klawisz **O**) → *Tryb pracy*:
   „Wtyczka w Equalizer APO”. Stan zmieni się na „wtyczka w Equalizer APO połączona”, mierniki ożyją.

## Jak to działa

- Wtyczka w `audiodg.exe` wystawia lokalny named pipe `\\.\pipe\szklo-ctl` (bez sieci). Aplikacja wysyła
  stan modułów przy każdej zmianie suwaka, wtyczka odsyła mierniki ~30 razy na sekundę.
- Stan jest też zapisywany w konfiguracji Equalizer APO (chunk VST), więc po restarcie komputera
  przetwarzanie działa nawet bez uruchomionej aplikacji.
- Latencja toru (tryb jakości) to ok. 93 ms przy 44,1 kHz – do muzyki bez znaczenia; przy wideo
  wtyczka zgłasza opóźnienie, ale nie każdy odtwarzacz je kompensuje.

## Diagnostyka

Utwórz folder `C:\ProgramData\Szklo\log` z prawem zapisu dla usługi audio (`icacls … /grant *S-1-5-19:(OI)(CI)M`) –
wtyczka zacznie zapisywać `szklo_diag.txt` (instancje, fs, liczba kanałów, szczyty, paniki). Usuń folder, żeby wyłączyć.

Znany problem (rozwiązany 2026-11-22): crate `vst` 0.4 trzymał dane wtyczki w polu `AEffect.user`, które należy do hosta;
Equalizer APO w audiodg.exe je nadpisuje → wtyczka widziała 0 kanałów i nic nie przetwarzała. Łatka w `third_party/vst-0.4.0`.

## Plan B (bez Equalizer APO)

Aplikacja ma wbudowany silnik: *Tryb pracy* → „Silnik w aplikacji”. Ustaw wirtualny kabel (np. VB-CABLE)
jako domyślne wyjście systemu, w Szkle wybierz wejście „CABLE Output” (albo „Przechwyć: CABLE Input”)
i wyjście = słuchawki. Oba urządzenia muszą mieć tę samą częstotliwość próbkowania (Panel dźwięku → Właściwości → Zaawansowane).

## Test

Spotify → słuchawki BT: przycisk „Porównaj z oryginałem” (trzymaj, albo spacja) przełącza na sygnał bez
przetwarzania – różnica powinna być od razu słyszalna, a przełączenie bez trzasków.
