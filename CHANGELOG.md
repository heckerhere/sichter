# Changelog

## Phase 7: Feinschliff (macOS) und Windows-Vorbereitung (2026-10-27)

- **LGPL-ffmpeg 8.1.3** (`scripts/build-ffmpeg-lgpl.sh`): selbst gebaut ohne GPL-/Nonfree-Teile, ohne externe Bibliotheken, statisch, universal (arm64 + x86_64), nur System-Frameworks. Quelle per SHA-256 gepinnt, das Skript prüft Lizenz und Bibliotheken. Damit ist der offene Punkt aus Phase 0 erledigt.
- **ffmpeg/ffprobe als Tauri-Sidecars** (`src-tauri/binaries/<tool>-<target>`), damit sie beim Signieren mit signiert werden. In der Entwicklung legt `fetch-sidecars.sh` das System-ffmpeg als Platzhalter ab.
- **Universal Binary** gebaut und geprüft: App und ffmpeg universell, exiftool aus dem Bundle mit Wächter, keine verwaisten Prozesse nach dem Beenden. Signierung und Notarisierung sind vorbereitet (Hardened Runtime, Umgebungsvariablen in der README), brauchen aber eine Developer-ID.
- **Build-Fix:** `strip = true` zerstörte beim Bauen mit `--target` das Prozedurmakro `tauri_macros`. Jetzt `strip = "debuginfo"`, nachgestellt und per Gegenprobe bestätigt.
- **Gesperrte Ordner:** Schalter „sperren“ je Root in der Bibliothek. Gesperrte Roots sind weder Quelle noch Ziel (galt im Planer schon, jetzt bedienbar).
- **Leere Ordner bereinigen:** Plan „Leere Ordner in den Papierkorb“ unter Einstellungen → Wartung. Leer heißt rekursiv nichts außer `.DS_Store`/`Thumbs.db`. Nur der oberste Ordner eines leeren Teilbaums, nie der Root, nie versteckte oder Paket-Ordner, nie in gesperrten Roots. Direkt vor dem Papierkorb wird erneut geprüft.
- **Einstellungen:** Dry-Run-Modus, Wartung (leere Ordner, Cache-Größen, Cache leeren), Datenordner im Finder zeigen, Systemstatus.
- **Windows-Vorbereitung** (ungeprüft, Umsetzung später auf Windows): `scripts/fetch-sidecars.ps1` (exiftool-exe, LGPL-ffmpeg von BtbN mit GPL-Prüfung), `\\?\`-Präfix für lange Pfade bei `MoveFileExW` (Logik getestet), NSIS/MSI-Konfiguration, CI-Matrix macOS + Windows mit Tests und Clippy, manueller Installer-Build im Workflow. Zwei Tests sind plattformneutral gemacht.
- **Tests:** 133 grün.

## Phase 6: Planer und sichere Ausführung (2026-10-27)

- **Sicherheitsmodell-Review** vor Beginn, siehe `docs/SICHERHEITSMODELL-REVIEW.md`, nach der Phase aktualisiert.
- **Planer** (`plan/`): Vorlagen mit `{Jahr}`, `{Monat}`, `{Datum}`, `{Anlass}`, `{Kamera}`, `{Original-Dateiname}`, optional eigene Video-Vorlage und „von bis“ für mehrtägige Events. Aufnahmen ohne Event kommen nach `{Jahr}/_Unsortiert`. Windows-kompatible Bereinigung (verbotene Zeichen, `CON`/`NUL`/…, Punkt oder Leerzeichen am Ende, 120 Zeichen je Teil, NFC). Originalnamen bleiben unangetastet, wenn sie unproblematisch sind. Ganze Gruppen. Konflikte: gleicher Inhalt → Quelle bleibt, als Duplikat gemeldet (auch gegenüber anderen geplanten Dateien); anderer Inhalt → ` (n)` am gemeinsamen Basisnamen aller Mitglieder (`IMG_1 (2).CR2`, `IMG_1 (2).CR2.xmp`). Gesperrte Roots weder Quelle noch Ziel, Ziel muss ein Bibliotheks-Root sein. Speicherplatzprüfung für Kopien und Cross-Volume-Verschiebungen.
- **Weitere Planarten:** Ausschuss → `_Aussortiert/<bisheriger Pfad>` (Standard), Ausschuss → Papierkorb (nur einzeln freigegebene Gruppen), Duplikate → `_Duplikate/…` oder Papierkorb. Eine Datei wird nur entfernt, wenn ihre ganze Gruppe redundant ist.
- **Journal** (`journal/`): Probelauf (prüft Vorbedingungen, ändert nichts). Ausführung nur mit Run-ID und bestätigter Dateianzahl und nur bei ausgeschaltetem Dry-Run-Modus. Gruppenweise mit Rollback der Gruppe bei Fehlern. Vor jeder Operation Prüfung des Ist-Zustands (idempotente Wiederaufnahme nach Absturz). Index-Pflege nach jedem Verschieben (Bewertungen und Events bleiben). Papierkorb bei Duplikaten erst nach erneuter Prüfung der bleibenden Kopie. Pause und Abbruch zwischen Gruppen. Undo rückwärts: Kopien in den Papierkorb, selbst angelegte leere Ordner entfernen. Für Papierkorb-Einträge der ehrliche Hinweis „im Finder zurücklegen“. JSON- und CSV-Protokoll je Lauf. Beim Start werden unterbrochene Läufe als abgebrochen markiert und sind fortsetzbar.
- **`fsops`:** `finish_interrupted_move` (nur bei identischer Kopie), `rmdir_if_empty`. `OperationId` und `TrashToken` entstehen nur im Journal (Guard-Test).
- **UI:** „Umsortieren“ mit Vorlagen-Editor, Platzhalter-Chips und Live-Vorschau. Plan-Ansicht mit Zählern, Speicherplatz, Hinweisen, Vorher-/Nachher-Baum, Operationsliste, Probelauf, Ausführen (Bestätigung mit Anzahl, beim Papierkorb mit vollständiger Dateiliste), Pause/Abbrechen, Ergebnis, Undo und Protokoll. „Verlauf“ aller Läufe. Aktionen in „Duplikate“ (Auswahl, `_Duplikate` oder Papierkorb) und „Sichten“ (Ausschuss nach `_Aussortiert` oder Papierkorb mit Einzelfreigabe). Dry-Run-Schalter in den Einstellungen, Statusleiste mit laufendem Lauf, offenen Plänen und letztem Lauf.
- **Migration 0006:** Felder für Läufe und Operationen.
- **Gefundene und behobene Fehler** (siehe Review): nummerierte Kopie statt Duplikatmeldung, zwei Abstürze bei Ordnernamen mit Umlaut am Anfang (Events, Duplikate), unvollständige Papierkorb-Bestätigung bei über 500 Dateien.
- **Tests:** 127 grün plus explizit ausführbare Tests (echter Papierkorb, echte RAM-Disk, Performance). Abgedeckt: Hash-Invariante, Undo stellt den Baum identisch wieder her, Probelauf, Bestätigung, Kollisionen, Gruppen, Gruppen-Rollback, Absturz und Wiederaufnahme, Abbrechen und Fortsetzen, gesperrte Roots, Kopieren als Obermenge, Unicode/Leerzeichen/reservierte Namen, Ausschuss- und Duplikat-Pläne, Property-Test mit Zufallsbäumen. Der Cross-Volume-Test auf einer RAM-Disk wurde einmal erfolgreich ausgeführt.

## Phase 5: Sichten (2026-10-27)

- **Große Vorschauen** (`thumbs/preview.rs`): 2560 px JPEG im App-Cache `previews/`. Bei RAW+JPEG kommt die Vorschau aus dem Kamera-JPEG, bei reinen RAWs aus dem größten eingebetteten Preview. Erzeugung bei Bedarf über das Protokoll `preview://`, jede Vorschau nur einmal gleichzeitig (parallele Anfragen warten). Beim Öffnen einer Sichtung werden alle Vorschauen im Hintergrund vorbereitet, vom aktuellen Bild nach außen, mit 3 Threads. Eine neue Sichtung beendet die vorige Vorbereitung.
- **Videos** über `media://<asset_id>` mit HTTP-Range (spulbar, Blöcke à 4 MB). Ausgeliefert werden nur indexierte Videos, nur lesend. Kann der Webview den Codec nicht abspielen, erscheint „Im Standardplayer öffnen“.
- **Vollbild-Sichtung:** ← → blättern, P behalten, X Ausschuss, 1–5 Sterne, 0 keine Sterne, U zurücksetzen, C Vergleich (1–4 Bilder, Tab wechselt den Fokus), Leertaste Video, Esc. Optional „Nach P/X weiter“. Thumbnail erscheint sofort, die große Vorschau darüber, Nachbarn werden vorgeladen. Filmstreifen mit Markierungen, Filter Alle / Unbewertet / Behalten / Ausschuss / 3+ Sterne. Die gefilterte Liste ist eine Momentaufnahme, damit ein Bild nach dem Bewerten nicht verschwindet.
- **Startseite „Sichten“:** alle Aufnahmen oder je Event, mit Fortschritt (behalten / Ausschuss / bewertet).
- **Bewertungen** (`db/ratings.rs`) nur in der Datenbank, Sterne und Markierung unabhängig. Leere Bewertung entfernt die Zeile. „Einsparbar: Ausschuss“ in der Speicheransicht, mit RAW, JPEG und Sidecars der Gruppe.
- **Fix:** Temp-Dateien im App-Verzeichnis haben jetzt eindeutige Namen. Parallele Schreiber derselben Cache-Datei kollidierten vorher (mit Test).
- **Tests:** 96 grün. Performance (Release, 500 × 12-MP-JPEG): einzelne Vorschau ohne Cache 78 ms, alle 500 im Hintergrund in 13 s (~38/s), aus dem Cache 0,05 ms je Bild.

## Phase 4: Events (2026-10-27)

- **Clustering** (`events/cluster.rs`): neues Event bei mehr als 4 h Pause oder mehr als 30 km Sprung zur letzten Aufnahme mit GPS, beides einstellbar. Mehrtägig: Folgegruppe am nächsten Tag nahe am GPS-Schwerpunkt wird angehängt. Innerhalb eines Tages wird nur verbunden, wenn die Gruppe schon mehrere Tage umfasst, also eine Reise ist. Ohne GPS keine Vermutung über einen Aufenthalt.
- **Regeln:** Events leben nur in der Datenbank. Nach jedem Scan werden nur noch nicht zugeordnete Aufnahmen einsortiert (in ein zeitlich passendes unbestätigtes Event oder ein neues). „Neu berechnen“ ersetzt nur unbestätigte Events, mit Rückfrage. Bestätigte Events verändert keine Automatik. Zusammenführen, Teilen und Verschieben heben die Bestätigung auf. Leere Events verschwinden.
- **Titelvorschlag:** häufigster aussagekräftiger Ordnername ohne Datumspräfix. DCIM, 100CANON, „Bilder“, „Camera“ usw. zählen nicht.
- **Video-Zeitzone:** QuickTime-`CreateDate` ohne Zeitzone wird pro Kamera als UTC oder Ortszeit gelesen. Standard: Handys (Apple, Google, Samsung …) UTC, Kameras Ortszeit. In der UI umstellbar. Ein Hinweis „Zeitzone prüfen“ erscheint, wenn ein Video über 1 h neben den Fotos derselben Kamera liegt, die andere Deutung aber passt. Umrechnung in die Zeitzone dieses Rechners. Auf Reisen in eine andere Zeitzone kann das eine Stunde danebenliegen, Hinweis bleibt dann sichtbar.
- **UI „Events“:** Timeline nach Monat, Karten mit vier gleichmäßig verteilten Vorschaubildern, Titel direkt editierbar mit „Vorschlag übernehmen“, Zeitraum, Anzahl, „n Tage“, „unsicheres Datum“, Zeitzonen-Hinweis, Bestätigen. Mehrfachauswahl zum Zusammenführen. Detailansicht mit „Ab hier teilen“, „Verschieben nach vorheriges / nächstes / neues Event“ und Notiz. Einstellungen für Pause, Entfernung und Mehrtägigkeit, Kamera-Zeitzonen.
- **Migration 0005:** `setting`, `camera_tz`, `event.suggested_title`.
- **Tests:** 89 grün. Akzeptanztest „drei Tage und eine Reise“ mit echten JPEGs (EXIF-Datum, GPS): Geburtstag / spätes Abendfoto / Hamburg / GPS-Sprung nach Berlin / viertägige Kroatienreise. Danach Zusammenführen, Umbenennen, Bestätigen, Teilen, Verschieben, Neuberechnen ohne Verlust bestätigter Events, Rescan, Quellordner unverändert.

## Phase 3: Duplikate (2026-10-27)

- **`dupes/`** (rein lesend): Dateigruppen (zuerst gleiche Größe, dann gleicher BLAKE3-Hash), 1:1 identische Ordner (Hash über sortierte Paare aus relativem Pfad und Datei-Hash, rekursiv), inhaltsgleiche Ordner mit anderen Dateinamen und Teilmengen (Inhalt von A steckt vollständig in B, mit Abdeckungsgrad).
- **Rauschfilter:** Gemeldet werden nur die obersten Ordnerpaare. Bei Teilmengen nur das größte A und das kleinste B, und keine Teilmenge, deren identischer Zwilling schon in B liegt.
- Nicht gehashte Dateien (unbekannte Typen) gehen mit Name und Größe in den Ordnervergleich ein.
- **Vorschlag, welche Kopie bleibt:** Pfade mit „Kopie“, „Backup“, „Downloads“, „Neuer Ordner“ … oder nummerierte Dateinamen („Bild (2).jpg“) werden abgewertet, Datumsordner bevorzugt, sonst gewinnt die älteste Kopie. Jeder Vorschlag kommt mit Begründung und wird nie automatisch übernommen.
- **Nutzerauswahl** (Migration 0004, Tabelle `dupe_choice`): nur gespeichert, ausgeführt wird erst über den Planer in Phase 6. Asset-IDs mit anderem Inhalt werden abgewiesen.
- **UI „Duplikate“:** Tabs Dateien / Identische Ordner / Gleicher Inhalt / Teilmengen mit Vorschaubild, Fundorten, Auswahl per Radio-Button, Kennzeichnung „Vorschlag“ und „Deine Wahl“. In der Speicheransicht ist „Einsparbar: doppelte Dateien“ jetzt befüllt.
- **Tests:** 80 grün, darunter der Akzeptanztest mit 1:1 kopiertem, halb kopiertem und umbenanntem Ordner.

## Phase 2: Speicherübersicht (2026-10-27)

- **`storage/`:** baut `folder` und `folder_size_by_kind` nach jedem Scan aus dem Index neu auf, ohne den Ordner erneut zu lesen. `own_size` = Dateien direkt im Ordner, `total_size` und `file_count` rekursiv. Nicht erreichbare Roots behalten ihre letzten Werte.
- **Ansicht „Speicher“:** Gesamtgröße mit gestapeltem Balken und Legende (Größe, Anteil, Anzahl je Kategorie). Ordnerbaum mit Spalten Gesamt / Eigene / Dateien / Anteil am Ordner darüber / Typ-Balken, sortierbar nach Größe, Anzahl oder Name. Treemap und Sunburst mit Zoom per Klick und Brotkrumen-Leiste, Tooltip mit Aufschlüsselung. Top-10-Listen größte Ordner und größte Dateien. Mehrere Roots als Tabs.
- **„Einsparbar“:** JPEG-Anteil von RAW+JPEG-Paaren als reine Information. Duplikate und Ausschuss kommen mit Phase 3 bzw. 5.
- **Farben:** fünf Kategorien in fester Reihenfolge (RAW blau, Bilder orange, Video aqua, Sidecars gelb, Sonstiges grau), mit dem dataviz-Validator gegen die echten App-Flächen in hell und dunkel geprüft: alle harten Prüfungen bestanden. Aqua und Gelb liegen hell unter 3:1 Kontrast, deshalb stehen die Werte immer auch in Legende und Tabelle.
- ECharts wird nur in der Speicheransicht geladen (Startpaket 122 KB statt 593 KB).
- **Tests:** 74 grün. Ordnersummen stimmen exakt mit dem Dateisystem überein (ohne ignorierte versteckte Dateien). Abgleich mit `du -A -k` mit dokumentierter Toleranz: versteckte Dateien werden herausgerechnet, plus höchstens 1 KiB Rundung je Datei und 4 KiB je Ordner.

### Fix nach Phase 1
- Gekachelte iPhone-HEICs (Tile Grid) ergaben keine Vorschau: ffmpeg setzt sie über einen eigenen Filtergraphen zusammen, der sich nicht mit `-vf` kombinieren lässt. Standbilder werden jetzt in Rust skaliert. Neue Fixture `misc/kacheln.heic`. Thumbnail-Version eingeführt (Migration 0003), damit Fehlschläge älterer Versionen einmal neu versucht werden.

## Phase 1: Scan und Index (2026-10-27)

- **Scan-Pipeline** (`scan/`): Crawl und Abgleich mit dem Index → Metadaten → Paarbildung → BLAKE3 → Thumbnails. Jeder Schritt verarbeitet nur, was fehlt. Ein abgebrochener Scan wird beim nächsten Lauf fortgesetzt.
- **Crawler:** folgt keinen Symlinks, ignoriert versteckte Dateien und Systemdateien (`.DS_Store`, `._*`, `Thumbs.db`, `@eaDir`, `$RECYCLE.BIN` …).
- **Neu, Sicherheit:** Pakete fremder Mediatheken werden übersprungen und im Bericht genannt (`*.photoslibrary`, `*.lrdata`, `*.cocatalog`, `*.fcpbundle` …). Dateien daraus später zu verschieben würde diese Mediatheken beschädigen.
- **Neu, Sicherheit:** Ein nicht erreichbarer Root (Platte abgezogen) und nicht lesbare Unterordner zählen nicht als „gelöscht“. Index, Bewertungen und Events bleiben erhalten.
- **Dateityp:** Die Endung entscheidet, ob eine Datei unterstützt wird, die Magic Bytes korrigieren die Art (z. B. HEIC mit Endung `.jpg`, markiert als `ext_mismatch`). Inhalt, der zu keinem Medium passt, wird als `other` geführt und nie bewegt.
- **Paarbildung:** RAW+JPEG, Sidecars (`IMG_1.xmp` und `IMG_1.CR2.xmp`), Live Photos über den Basisnamen und zusätzlich über die Apple-ContentIdentifier. Vergleich NFC-normalisiert und ohne Groß-/Kleinschreibung. Gruppen-IDs bleiben über Rescans stabil, damit Bewertungen erhalten bleiben.
- **Metadaten:** ein exiftool-Prozess, Abfrage in 100er-Batches. Datumspriorität: Fotos `DateTimeOriginal` (+`OffsetTimeOriginal`), Videos QuickTime `CreationDate` (mit Zeitzone) vor `CreateDate` (markiert als `quicktime_utc` für die Korrektur in Phase 4). Fallback auf das Datum im Dateinamen (IMG_/VID_/PXL_/WhatsApp/Screenshot/Dropbox-Schema), danach mtime. Beides wird als „Datum unsicher“ angezeigt. ffprobe liefert Dauer, Codec, fps und Auflösung.
- **BLAKE3:** 4 parallele Leser. Dateien mit mehrfach vorkommender Größe kommen zuerst dran, weil nur sie Duplikate sein können. Gehasht wird trotzdem alles außer `other`, weil `fsops` und der Thumbnail-Cache den Hash brauchen.
- **Thumbnails:** WebP, 512 px, im App-Datenverzeichnis, Schlüssel ist der Datei-Hash. Quellen: JPEG/PNG/TIFF/WebP direkt mit EXIF-Ausrichtung, RAW über das eingebettete Preview (exiftool-Prozesspool, Binärausgabe über stdout), HEIC und Video per ffmpeg über stdout (Video bei 10 % der Laufzeit). Fehlschläge werden gemerkt und nicht bei jedem Scan neu versucht.
- **`thumb://`-Protokoll:** liefert ausschließlich Dateien aus dem Thumbnail-Cache, nur anhand eines gültigen 64-stelligen Hashs.
- **UI:** Ordner hinzufügen (mehrere), Roots mit Dateianzahl und Größe, „nicht erreichbar“-Hinweis, Entfernen nur aus dem Index mit Rückfrage. Scan-Fortschritt pro Phase mit Abbruch, Scan-Bericht, virtualisiertes Grid mit Filtern (Alle, Fotos, RAW, Videos, Datum unsicher), Badges (RAW+JPEG, Live, XMP, Videodauer) und eine Liste nicht unterstützter Dateien. Systemstatus zusätzlich unter Einstellungen.
- **Migration 0002:** `meta.content_id`, `meta.quicktime_utc`, `asset.ext_mismatch`, `asset.meta_done`, Tabelle `thumb`.
- **Tests:** 65 grün, plus ein Performance-Test. Abgedeckt: Quellordner unverändert (Größe, mtime, Hash jeder Datei inkl. versteckter), Gruppen, Datumsquellen, inkrementeller Rescan, Bewertungen überleben Rescan und Teiländerungen, nicht erreichbarer Root, nicht lesbarer Ordner, Abbruch und Fortsetzung.
- **Performance** (Apple Silicon, Release, `cargo test --release --test perf -- --ignored --nocapture`): Erstscan von 10.000 JPEGs inkl. Metadaten, Hashes und Thumbnails in 18,7 s, inkrementeller Rescan in 77 ms.

### Offen
- Die UI-Abläufe (Ordnerdialog, Fortschritt, Grid) sind kompiliert und typgeprüft, aber nicht per Klick getestet: In dieser Umgebung fehlt die Berechtigung für Bildschirmaufnahmen.
- Die RAW-Fixtures CR2, CR3, NEF und DNG sind Stubs ohne eingebettetes Preview. Der Preview-Weg ist mit RAF und RW2 getestet. Echte Kamera-RAWs sollten noch geprüft werden (zusammen mit ARW, siehe Phase 0).

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
