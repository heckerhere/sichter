# Sichter

Desktop-App (Tauri 2 + Vue 3) zum Sortieren, Aussortieren und Umstrukturieren großer Foto- und Videobestände. Grundsatz: **Die App löscht nie etwas eigenständig.** Details und Fahrplan stehen in [PLAN.md](PLAN.md).

## Entwicklung (macOS)

Voraussetzungen: Rust (stable), Node 22, pnpm, Xcode Command Line Tools, Perl (bei macOS dabei), `brew install ffmpeg`.

```sh
./scripts/fetch-sidecars.sh   # lädt exiftool nach src-tauri/resources/exiftool (SHA-256-geprüft)
pnpm install
pnpm tauri dev
cargo test --manifest-path src-tauri/Cargo.toml
# Performance-Test (10.000 Dateien):
cargo test --release --manifest-path src-tauri/Cargo.toml --test perf -- --ignored --nocapture
```

## Sidecars

| Werkzeug | Entwicklung | Release |
|---|---|---|
| exiftool | gebündelt (`resources/exiftool`), über System-Perl gestartet | gebündelt |
| ffmpeg / ffprobe | aus dem `PATH` (Homebrew, GPL-Build) | LGPL-Build in `resources/bin/` (Phase 7, offen) |

Überschreiben per Umgebungsvariable: `SICHTER_EXIFTOOL`, `SICHTER_FFMPEG`, `SICHTER_FFPROBE`.

## Fixtures

`src-tauri/tests/fixtures/library` wird mit `scripts/make-fixtures.sh <Image-ExifTool-x/t/images>` erzeugt. RAW-Dateien sind Metadaten-Stubs aus der exiftool-Testsuite, JPEG/HEIC/PNG/TIFF/Videos sind echte, generierte Medien. Tests kopieren Fixtures immer in ein temporäres Verzeichnis und laufen nie gegen echte Fotoordner.

## Schreibzugriff

Nur `src-tauri/src/fsops/` schreibt ins Dateisystem. `src-tauri/tests/fsops_guard.rs` schlägt fehl, sobald anderswo ein schreibender `std::fs`-Aufruf auftaucht.
