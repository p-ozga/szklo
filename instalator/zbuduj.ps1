# Buduje instalator Szkła: dist\Szklo-<wersja>-Windows-x64.msi
# Wymaga: Rust (MSVC), WiX 5 (`dotnet tool install --global wix`). Uruchom z katalogu repozytorium:
#   powershell -ExecutionPolicy Bypass -File instalator\zbuduj.ps1
$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
Set-Location $repo
$wersja = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
Write-Host "Szkło $wersja – budowanie programu i wtyczki…"
# Ścieżki z komputera budującego (folder użytkownika = jego nazwa, folder repozytorium) NIE mogą trafić do plików
# programu: Rust zapisuje je w komunikatach błędów. Zamiana na neutralne nazwy. RUSTFLAGS zastępuje rustflags
# z .cargo/config.toml, więc crt-static jest powtórzony tutaj.
$env:RUSTFLAGS = "-C target-feature=+crt-static --remap-path-prefix=$($env:USERPROFILE)=~ --remap-path-prefix=$repo=szklo"
cargo build --release -p szklo-app -p szklo-vst2
Remove-Item Env:RUSTFLAGS
if ($LASTEXITCODE -ne 0) { throw "cargo build nie powiodło się" }
# APK tej samej wersji z dist (opcjonalny składnik instalatora) – starszy nie może trafić do instalatora
$apk = Get-ChildItem dist -Filter "Szklo-$wersja-*-arm64.apk" -ErrorAction SilentlyContinue | Sort-Object LastWriteTime | Select-Object -Last 1
if (-not $apk) { throw "brak APK $wersja w dist\ – zbuduj aplikację na Androida albo usuń składnik Android z instalator\Szklo.wxs" }
New-Item -ItemType Directory -Force dist | Out-Null
$msi = "dist\Szklo-$wersja-Windows-x64.msi"
wix build instalator\Szklo.wxs -arch x64 -d "Wersja=$wersja" -d "Zrodla=$repo" -d "Apk=$($apk.FullName)" -o $msi
if ($LASTEXITCODE -ne 0) { throw "wix build nie powiodło się" }
# walidacja ICE; ICE38/43/57 to fałszywe alarmy dla instalacji „dla wszystkich użytkowników” (skróty w menu Start/na pulpicie
# wspólnych), ICE61 – celowo pozwalamy zainstalować tę samą wersję ponownie
wix msi validate $msi -sice ICE38 -sice ICE43 -sice ICE57 -sice ICE61
if ($LASTEXITCODE -ne 0) { throw "walidacja MSI nie powiodła się" }
Remove-Item "dist\Szklo-$wersja-Windows-x64.wixpdb" -ErrorAction SilentlyContinue
# kontrola: w programie nie może być nazwy użytkownika ani ścieżki repozytorium
$nazwa = Split-Path -Leaf $env:USERPROFILE
foreach ($plik in "target\release\Szklo.exe", "target\release\szklo_vst2.dll") {
    $tekst = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes($plik))
    if ($tekst.IndexOf($env:USERPROFILE, [StringComparison]::OrdinalIgnoreCase) -ge 0 -or $tekst.IndexOf("\$nazwa\", [StringComparison]::OrdinalIgnoreCase) -ge 0 -or $tekst.IndexOf($repo, [StringComparison]::OrdinalIgnoreCase) -ge 0) {
        throw "w $plik została ścieżka z tego komputera – nie publikuj"
    }
}
Write-Host "Gotowe: $msi ($([int]((Get-Item $msi).Length / 1MB)) MB)"
