# Szkło – aplikacja Android

Kotlin + Jetpack Compose, pakiet `pl.szklo`, minSdk 26, targetSdk/compileSdk 35.
Rdzeń DSP w Rust (`rust/`, crate `szklo-jni`, biblioteka `libszklo_jni.so`) wołany przez JNI
z klasy `pl.szklo.core.NativeEngine`.

## Co jest w środku

| Ścieżka | Zawartość |
|---|---|
| `app/src/main/java/pl/szklo/core/NativeEngine.kt` | most JNI (`System.loadLibrary("szklo_jni")`, `AutoCloseable`, blokada RW przy zamykaniu) |
| `app/src/main/java/pl/szklo/model/` | katalog 32 modułów (kolejność = `core/src/catalog.rs`), grupy, presety (= `core/src/presets.rs`) |
| `app/src/main/java/pl/szklo/audio/AudioHub.kt` | jedno źródło prawdy: parametry, stan odtwarzacza, sesje innych aplikacji, aktywny silnik |
| `app/src/main/java/pl/szklo/audio/LocalPlayerService.kt`, `PcmPlayer.kt` | odtwarzanie pliku: MediaExtractor → MediaCodec → float → `NativeEngine.process` → AudioTrack float (częstotliwość z pliku) |
| `app/src/main/java/pl/szklo/audio/GlobalEffectsService.kt`, `GlobalEffectsBridge.kt`, `DpMapping.kt`, `EffectSessionReceiver.kt` | tryb „inne aplikacje”: rozgłoszenia sesji efektów + `DynamicsProcessing` (API 28+) – **przybliżenie** naszych modułów |
| `app/src/main/java/pl/szklo/ui/` | UI 1:1 z makietą „Android 2 · Pokrętło mocy”, odwracanie okna, strona tylna |
| `app/src/main/res/font/` | Inter 400/500/600, Space Grotesk 500/600 (statyczne instancje z Google Fonts, OFL) |
| `app/src/main/assets/licencje/` | `OFL-Inter.txt`, `OFL-SpaceGrotesk.txt` |
| `app/src/test/` | testy JVM: geometria pokrętła, profil plam tła, presety, neutralność mapowania DP |

Raport o przetwarzaniu dźwięku innych aplikacji: `../spec/android_przetwarzanie_innych_aplikacji.md`.

## Wymagania

- Android Studio (Ladybug 2024.2 lub nowsze) z JDK 17+ (wbudowany JBR wystarcza), Android SDK 35.
- Do rdzenia: Rust stable, `cargo-ndk`, Android NDK (r26 lub nowszy).

## Krok 1 – biblioteka natywna `libszklo_jni.so`

```powershell
# jednorazowo
rustup target add aarch64-linux-android armv7-linux-androideabi
cargo install cargo-ndk
# NDK z Android Studio: SDK Manager → SDK Tools → NDK (Side by side)
$env:ANDROID_NDK_HOME = "$env:LOCALAPPDATA\Android\Sdk\ndk\<wersja>"

# budowa (z katalogu app-android/rust)
cd D:\Soft\DSP\app-android\rust
cargo ndk -t arm64-v8a -t armeabi-v7a -P 26 -o ..\app\src\main\jniLibs build --release
```

Uwaga na ścieżkę `-o`: jest liczona od bieżącego katalogu. Z `app-android/rust` właściwa to
`../app/src/main/jniLibs` (polecenie z `-o app/src/main/jniLibs` położyłoby pliki w
`app-android/rust/app/...`, gdzie Gradle ich nie widzi). Wynik:

```
app/src/main/jniLibs/arm64-v8a/libszklo_jni.so
app/src/main/jniLibs/armeabi-v7a/libszklo_jni.so
```

Crate jest członkiem workspace’u `D:\Soft\DSP\Cargo.toml`, więc pliki pośrednie trafiają do
`D:\Soft\DSP\target\`. Katalog `jniLibs` jest w `.gitignore`.

To samo z Gradle (zadanie `cargoNdkBuild` w `app/build.gradle.kts`):

```powershell
.\gradlew :app:cargoNdkBuild                   # ręcznie
.\gradlew :app:assembleDebug -Pszklo.buildRust=true   # automatycznie przed każdym buildem
```

(albo `szklo.buildRust=true` w `gradle.properties`).

Emulator na PC z Windows to zwykle x86_64 – dla niego dodaj `-t x86_64` (i `rustup target add x86_64-linux-android`).
Bez biblioteki aplikacja się nie wywraca: UI działa, odtwarzacz gra bez obróbki, a strona tylna mówi o braku rdzenia.

## Krok 2 – aplikacja

W repozytorium jest tylko `gradle/wrapper/gradle-wrapper.properties` (Gradle 8.11.1), bez `gradlew` i `gradle-wrapper.jar`.

**Android Studio:** File → Open → `D:\Soft\DSP\app-android` → Sync (Studio pobierze Gradle wg wrappera) →
Run ▶ na telefonie (arm64) lub `Build → Build APK(s)`.

**Wiersz poleceń:** jednorazowo wygeneruj wrapper (wymaga zainstalowanego Gradle 8.9+):

```powershell
cd D:\Soft\DSP\app-android
gradle wrapper --gradle-version 8.11.1
.\gradlew :app:assembleDebug          # APK: app/build/outputs/apk/debug/app-debug.apk
.\gradlew :app:testDebugUnitTest      # testy JVM
.\gradlew :app:installDebug           # instalacja na podłączonym telefonie
```

`local.properties` (ścieżka do SDK) Android Studio tworzy samo; z wiersza poleceń ustaw `ANDROID_HOME`
albo wpisz `sdk.dir=C\:\\Users\\<ty>\\AppData\\Local\\Android\\Sdk`.

## Obsługa

- **Pokrętło**: przeciąganie po okręgu (bez przeskoku przez lukę na dole), haptyka co 5%, podwójne stuknięcie =
  moc z presetu. TalkBack: suwak z akcjami „Zwiększ/Zmniejsz o 5%”.
- **Chipy grup** zmieniają zawartość panelu (przenikanie 150 ms). **Panel** przeciągany za uchwyt albo listę do pełnej wysokości.
- **„Codzienny ▾”**: presety (przejście 150 ms), przełącznik przetwarzania (bypass) i trybu „Inne aplikacje”.
- **Odwrócenie okna**: stuknięcie w napis „Szkło” albo przesunięcie palcem w poziomie po nagłówku → strona „Odtwarzacz”
  (plik z telefonu przez pełny rdzeń; zaślepka pod sterowanie innymi odtwarzaczami – interfejs `PlayerControlsBackside`).
  Powrót: „Wróć” albo systemowe „wstecz”.
- **Spotify**: Ustawienia → Odtwarzanie → Korektor otwiera Szkło (intencja `DISPLAY_AUDIO_EFFECT_CONTROL_PANEL`) i włącza tryb
  „Inne aplikacje” – działa wtedy przybliżenie przez `DynamicsProcessing`, nie pełny rdzeń (patrz raport w `spec/`).

## Odstępstwa i decyzje (do akceptacji Pawła)

- Przycisk odwracania nie ma osobnej ikony (makieta jej nie przewiduje) – gest na nagłówku.
- Moc ogólna presetów (`Preset.master`: 68/85/60/50%) istnieje tylko w Kotlinie – presety Rust jej nie mają.
- Plamy tła: profil koła rozmytego gaussem, ucięty do zera na r + blur (jak w scenariuszu) – stała `TAIL_FACTOR`
  w `GlowBackground.kt`, jeśli porównanie SSIM z eksportem makiety pokaże różnicę na brzegach.
- Rozmycie panelu: CSS `blur(36px)` = σ 36 px; RenderEffect przyjmuje promień (σ = 0,577·r + 0,5) – przeliczone.
- Panel dolny: 356 dp + wysokość paska nawigacji (treść panelu ma wtedy dokładnie wymiary z makiety).

## Budowa na tym komputerze (sprawdzone 2026-11-22, Sony Xperia 10 VI / XQ-ES54, arm64)

Narzędzia są w `D:\Soft\Android` (JDK 17, SDK 34/35, NDK 27.2, Gradle 8.11.1).

```powershell
cd D:\Soft\DSP\app-android\rust
$env:ANDROID_NDK_HOME='D:\Soft\Android\sdk\ndk\27.2.12479018'
cargo ndk -t arm64-v8a -P 26 -o ..\app\src\main\jniLibs build --release
cd ..
$env:JAVA_HOME='D:\Soft\Android\jdk'; $env:ANDROID_HOME='D:\Soft\Android\sdk'
D:\Soft\Android\gradle\bin\gradle.bat :app:assembleDebug :app:testDebugUnitTest
```

APK: `app\build\outputs\apk\debug\app-debug.apk` (kopia: `D:\Soft\DSP\dist\Szklo-Xperia10VI-arm64.apk`).
Instalacja przez USB: `D:\Soft\Android\sdk\platform-tools\adb.exe install -r <apk>` (na telefonie: Opcje programisty → Debugowanie USB).

## Spotify Connect („Szkło”)

Spotify nie pozwala zwykłym aplikacjom przetwarzać swojego dźwięku efektem systemowym na tyle mocno, jak robi to PC.
Dlatego Szkło jest też **odbiornikiem Spotify Connect** (librespot, wymaga Premium): muzyka idzie
Spotify → librespot → pełny silnik Szkła (te same moduły co na PC, w tym Ciepły bas) → AAudio.

1. Szkło → odwróć okno → karta **Spotify Connect** → **Włącz**.
2. Za pierwszym razem otworzy się przeglądarka z logowaniem Spotify; po zalogowaniu wróć do Szkła.
3. W aplikacji Spotify stuknij ikonę urządzeń i wybierz **Szkło**.

Poświadczenia zostają w pamięci aplikacji (`files/spotify`); **Wyloguj** je usuwa.
Gdy odbiornik działa, efekt systemowy na sesji 0 jest wyłączony, żeby dźwięk nie był przetwarzany dwa razy.
