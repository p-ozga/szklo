# Instalator Szkła (Windows)

Wynik: `dist\Szklo-<wersja>-Windows-x64.msi`, jeden plik, instalacja dla wszystkich użytkowników.

## Budowa

```powershell
powershell -ExecutionPolicy Bypass -File instalator\zbuduj.ps1
```

Potrzebne: Rust z MSVC, WiX 5 (`dotnet tool install --global wix`) oraz APK w `dist\`. Skrypt buduje
`Szklo.exe` i wtyczkę `szklo_vst2.dll`, potem tworzy MSI i sprawdza go walidacją ICE.

## Co instaluje

- `C:\Program Files\Szklo\Szklo.exe` ze skrótem w menu Start i opcjonalnie na pulpicie.
- Wtyczkę `szklo_vst2.dll`. Instalator kopiuje ją do `C:\ProgramData\Szklo\` i wpisuje do `config.txt` Equalizer APO
  (`Szklo.exe --instaluj-wtyczke`).
  - Folder `C:\ProgramData\Szklo\` jest tylko do odczytu dla zwykłych użytkowników, bo wtyczkę ładuje usługa
    dźwięku Windows.
  - Przed pierwszą zmianą `config.txt` instalator zapisuje kopię jako `config.przed_szklo.txt`.
- Opcjonalnie aplikację na Androida (`Android\Szklo-Android.apk`).
- Licencję (`LICENCJA.txt`), listę cudzych bibliotek (`ELEMENTY_CUDZE.txt`) i instrukcję Equalizer APO.

Biblioteka uruchomieniowa C jest wbudowana w pliki (`.cargo/config.toml`, `crt-static`). Visual C++
Redistributable nie jest potrzebny.

## Czego instalator NIE zawiera

To programy innych autorów. Szkło pobiera je z oficjalnych stron dopiero wtedy, gdy są potrzebne.

| Program | Autor / licencja | Kiedy |
|---|---|---|
| Equalizer APO | Jonas Thedering, GPL-2.0 (SourceForge) | Opcja na początku instalacji, wykonywana przy pierwszym uruchomieniu Szkła, albo przycisk „Pobierz i zainstaluj Equalizer APO” |
| Microsoft Edge WebView2 Runtime | Microsoft | Jest w Windows 11. Gdy go brak, Szkło przy starcie proponuje instalację |
| MilkDrop 3 | milkdrop2077, BSD-3 (GitHub) | Przycisk „Pobierz i zainstaluj” w karcie odtwarzacza |
| Spotify | Spotify AB | Instalujesz sam ze Sklepu Microsoft. Szkło tylko steruje aplikacją |

## Odinstalowanie

Aplikacje → Szkło → Odinstaluj. Odinstalowanie usuwa wpis Szkła z `config.txt` Equalizer APO i pliki programu.

Plik wtyczki wczytany przez usługę dźwięku znika dopiero po jej restarcie albo po ponownym uruchomieniu
komputera.

Ustawienia użytkownika (`%APPDATA%\Szklo`) zostają.

## Instalacja bez okien

```powershell
msiexec /i Szklo-0.3.0-Windows-x64.msi /qn
```

Polecenie trzeba uruchomić jako administrator. Domyślnie instaluje program, wtyczkę i skrót na pulpicie.

Wybrane składniki: `ADDLOCAL=Szklo,Wtyczka,Pulpit,Android`.

Plik jest niepodpisany, więc Windows SmartScreen może pokazać ostrzeżenie: wybierz „Więcej informacji” →
„Uruchom mimo to”.
