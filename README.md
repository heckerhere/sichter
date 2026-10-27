# Sichter

Desktop-App (Tauri 2 + Vue 3) zum Sortieren, Aussortieren und Umstrukturieren großer Foto- und Videobestände. Grundsatz: **Die App löscht nie etwas eigenständig.** Details und Fahrplan stehen in [PLAN.md](PLAN.md).

## Entwicklung (macOS)

Voraussetzungen: Rust (stable), Node 22, pnpm, Xcode Command Line Tools, Perl (bei macOS dabei), `brew install ffmpeg` (nur als Platzhalter für die Entwicklung).

```sh
./scripts/fetch-sidecars.sh   # exiftool nach src-tauri/resources/exiftool (SHA-256-geprüft),
                              # ffmpeg/ffprobe-Platzhalter nach src-tauri/binaries
pnpm install
pnpm tauri dev
cargo test --manifest-path src-tauri/Cargo.toml
# Performance-Tests (10.000 Dateien, 500 Vorschauen):
cargo test --release --manifest-path src-tauri/Cargo.toml --test perf -- --ignored --nocapture
# Tests mit Wirkung auf das System (RAM-Disk, echter Papierkorb), nur bewusst ausführen:
cargo test --manifest-path src-tauri/Cargo.toml --test runs real_ -- --ignored
```

## Sidecars

| Werkzeug | Entwicklung | Release |
|---|---|---|
| exiftool | gebündelte Perl-Distribution (`resources/exiftool`), über System-Perl gestartet, mit Wächter gegen verwaiste Prozesse | macOS: gleich. Windows: `exiftool.exe` + `exiftool_files` |
| ffmpeg / ffprobe | Tauri-Sidecar `src-tauri/binaries/<tool>-<target>`, Platzhalter aus Homebrew (GPL, die App warnt) | macOS: `scripts/build-ffmpeg-lgpl.sh` baut LGPL 8.1.3 universal, statisch, nur Systembibliotheken. Windows: LGPL-Build von BtbN über `scripts/fetch-sidecars.ps1` |

Überschreiben per Umgebungsvariable: `SICHTER_EXIFTOOL`, `SICHTER_FFMPEG`, `SICHTER_FFPROBE`.

## Release bauen

```sh
./scripts/build-ffmpeg-lgpl.sh                       # einmalig, ca. 5–10 Minuten
rustup target add aarch64-apple-darwin x86_64-apple-darwin
pnpm tauri build --target universal-apple-darwin     # .app und .dmg
```

**Signieren und notarisieren** (benötigt eine Apple-Developer-ID): Vor dem Build setzen

```sh
export APPLE_SIGNING_IDENTITY="Developer ID Application: Name (TEAMID)"
export APPLE_ID="…" APPLE_PASSWORD="<app-spezifisches Passwort>" APPLE_TEAM_ID="TEAMID"
```

Tauri signiert dann App und Sidecars (Hardened Runtime ist aktiv) und reicht zur Notarisierung ein. In GitHub Actions dieselben Werte als Secrets. Der Workflow `CI` baut mit „Run workflow → bundle“ Installer für macOS (universal) und Windows (NSIS, MSI).

**Windows:** `pwsh scripts/fetch-sidecars.ps1`, dann `pnpm tauri build`. Die Windows-Tests laufen im CI-Job `windows-latest`.

## Fixtures

`src-tauri/tests/fixtures/library` wird mit `scripts/make-fixtures.sh <Image-ExifTool-x/t/images>` erzeugt. RAW-Dateien sind Metadaten-Stubs aus der exiftool-Testsuite, JPEG/HEIC/PNG/TIFF/Videos sind echte, generierte Medien. Tests kopieren Fixtures immer in ein temporäres Verzeichnis und laufen nie gegen echte Fotoordner.

## Schreibzugriff

Nur `src-tauri/src/fsops/` schreibt ins Dateisystem. `src-tauri/tests/fsops_guard.rs` schlägt fehl, sobald anderswo ein schreibender `std::fs`-Aufruf auftaucht.
