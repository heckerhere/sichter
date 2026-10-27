# Changelog

## Phase 0: Gerüst (2026-10-27)

- Tauri 2 + Vue 3 + TypeScript + Vite + Tailwind 4 + Pinia + Vue Router. Linke Navigation mit allen Bereichen aus PLAN.md, Abschnitt 7, Statusleiste mit Dry-Run-Hinweis.
- Rust-Modulstruktur gemäß Abschnitt 5.1, zusätzlich `tools/` für das Auffinden der Sidecars.
- SQLite (`rusqlite`, gebündelt) im WAL-Modus mit Fremdschlüsseln, Migrationen über `PRAGMA user_version`. Eine neuere Datenbank wird abgewiesen, nicht angefasst.
- Initiales Schema aus Abschnitt 5.2. Ergänzt: `meta.date_source` (`exif` / `filename` / `mtime`) für „Datum unsicher“, `library_root.added_at`.
- exiftool 13.55 als gebündelte Ressource, langlebiger `-stay_open`-Prozess. stderr wird in einem eigenen Thread geleert, damit ein voller Puffer nichts blockiert.
- ffmpeg/ffprobe: Auflösung über Umgebungsvariable → Bundle → PATH, Versionsanzeige mit GPL-Warnung.
- `fsops` als einziger Schreibpfad:
  - `move_verified`: macOS `renamex_np(RENAME_EXCL)`, Windows `MoveFileExW` ohne Replace-Flag, also atomar und ohne Überschreiben. Über Volume-Grenzen oder auf Dateisystemen ohne No-Replace-Rename: Kopie mit `create_new`, BLAKE3-Prüfung, erst dann Quelle entfernen.
  - `copy_verified` (erhält mtime), `mkdir` (nicht rekursiv), `trash` (nur mit `TrashToken`).
  - `OperationId` und `TrashToken` lassen sich nur crate-intern erzeugen (Journal bzw. Planer in Phase 6).
  - `PathGuard` für gesperrte Roots. Auf macOS und Windows wird ohne Beachtung der Groß-/Kleinschreibung verglichen, also im Zweifel strenger.
  - `appdata::AppDataDir` für DB, Cache und Logs, ohne Möglichkeit, das Verzeichnis per `..` zu verlassen.
- Guard-Test `tests/fsops_guard.rs` plus GitHub-Actions-Workflow (macOS).
- Fixtures: JPEG (mit EXIF und GPS), RAW+JPEG+XMP-Gruppe, CR2, CR3, NEF, DNG, RAF, RW2, HEIC, PNG, TIFF, WebP, MP4, MOV, Live Photo (HEIC+MOV+AAE), Datei ohne EXIF mit Datum im Namen, unbekannte Datei, `.DS_Store`.
- 32 Tests grün.

### Offen / bewusste Entscheidungen

- **ffmpeg-LGPL-Bundle fehlt noch.** Das Homebrew-ffmpeg ist GPL und dynamisch gegen Homebrew-Bibliotheken gelinkt, lässt sich also nicht bündeln. Bis Phase 7 kommt ffmpeg aus dem PATH.
- **ARW-Fixture fehlt.** Die exiftool-Testsuite enthält keine ARW-Datei. Nachliefern mit einer CC0-Probe von raw.pixls.us.
- exiftool braucht das System-Perl. macOS liefert es noch mit. Falls Apple es entfernt, wäre eine eingebettete Perl-Laufzeit nötig.
- Wenn ein Dateisystem kein No-Replace-Rename kann (z. B. manche SMB/exFAT-Volumes), wird kopiert und verifiziert statt per prüfendem `rename`. Das ist die sicherere Variante, weil sie keine Wettlaufsituation beim Überschreiben zulässt.
