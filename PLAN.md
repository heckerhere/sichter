# PLAN: Foto-Sortierer ("Sichter")

Desktop-App für **macOS (Priorität) und Windows** zum Sortieren, Aussortieren und Umstrukturieren großer Foto- und Videobestände:
- Bilder: JPEG, HEIC, PNG, TIFF, WebP
- RAW: CR2, CR3, NEF, ARW, DNG, RAF, ORF, RW2
- Videos: MP4, MOV, M4V, AVI, MTS/M2TS, 3GP, MKV

Oberstes Prinzip: **Die App löscht nie etwas eigenständig.** Jede Dateioperation ist eine geplante, angezeigte, bestätigte und rückgängig machbare Aktion.

---

## 1. Ziele

1. Bestehende Ordner einlesen (nur lesend), Metadaten indexieren.
2. Fotos automatisch zu **Events/Anlässen** gruppieren (Zeitlücken, GPS, Kamera), vom Nutzer prüfen, zusammenführen, teilen und benennen lassen.
3. **Aussortieren** (Culling): Bilder schnell sichten, bewerten, als "Ausschuss" markieren.
4. **Duplikate** finden: identische Dateien, 1:1 identische Ordner, Ordner die Teilmenge eines anderen sind.
5. **Umsortieren** nach Vorlage, z. B. `{Jahr}/{YYYY-MM-DD} {Anlass}/`, mit Vorschau, Journal und Undo. Standard ist Verschieben, Kopieren ist als Option wählbar.
6. **Speicherübersicht:** Speicherverbrauch pro Ordner, aufgeschlüsselt nach Dateityp, inklusive einsparbarem Platz durch Duplikate und Ausschuss.

## 2. Nicht-Ziele (v1)

- Kein Bildbearbeitungsprogramm, kein RAW-Entwickler, kein Videoschnitt.
- Keine Cloud, kein Sync, kein Account.
- Keine KI-Gesichtserkennung (später optional).
- Kein endgültiges Löschen, auch nicht auf Wunsch (siehe Sicherheitsmodell).

## 3. Sicherheitsmodell (nicht verhandelbar)

Diese Regeln gelten für jede Phase und müssen durch Tests abgesichert sein.

1. **Lesen und Schreiben sind getrennt.** Scan, Analyse, Event-Erkennung, Duplikatsuche und Bewertung verändern keine Datei im Quellbestand. Bewertungen leben in der App-Datenbank. XMP-Export ist eine optionale Funktion, standardmäßig **aus** (siehe 6.3).
2. **Alle Änderungen laufen über einen Operationsplan.** Die App erzeugt eine Liste geplanter Operationen (`move`, `copy`, `mkdir`, `trash`). Der Nutzer sieht die Liste als Vorher/Nachher-Baum mit Zählern, bevor irgendetwas passiert. Ausführung nur nach Klick auf "Ausführen" plus Bestätigungsdialog mit Anzahl betroffener Dateien.
3. **Dry-Run ist der Standard.** Jeder Plan kann als reine Simulation laufen und protokolliert, was passieren würde.
4. **Kein Überschreiben.** Existiert eine Zieldatei, wird nicht überschrieben. Bei identischem Hash: als Duplikat melden, Quelle bleibt liegen. Bei abweichendem Inhalt: Suffix ` (2)` usw.
5. **Verschieben ist verifiziert.** Gleiches Volume: atomares `rename`. Anderes Volume: kopieren, Hash der Kopie prüfen, erst dann Quelle entfernen. Bei Abweichung: Abbruch, Quelle bleibt.
6. **Löschen gibt es nur als "In den Papierkorb".** Über den System-Papierkorb (Rust-Crate `trash`), niemals `remove_file`. Nur für Dateien, die der Nutzer einzeln oder als Gruppe explizit freigegeben hat. Zusätzlicher Bestätigungsdialog mit Dateiliste. Duplikate werden nur dann zum Papierkorb vorgeschlagen, wenn eine verifizierte identische Kopie (gleicher BLAKE3-Hash, gleiche Größe) an anderer Stelle bleibt.
7. **Journal und Undo.** Jede ausgeführte Operation wird *vor* der Ausführung als "geplant" und *danach* als "erledigt" in SQLite geschrieben. Ein Abbruch (Absturz, Stromausfall) ist dadurch nachvollziehbar und fortsetzbar. Jeder Lauf ist als Ganzes rückgängig machbar (Rückwärts-Moves). Zusätzlich wird pro Lauf eine lesbare CSV/JSON-Logdatei neben die Datenbank geschrieben.
8. **Zusammengehörige Dateien bleiben zusammen.** RAW+JPEG-Paare (gleicher Basisname), Sidecars (`.xmp`, `.aae`, `.thm`, `.pp3`), Live-Photo-Videos (`.mov` mit gleichem Basisnamen oder ContentIdentifier) werden als eine Einheit behandelt.
9. **Unbekannte Dateien werden nicht angefasst.** Nur explizit unterstützte Endungen werden bewegt. Alles andere bleibt liegen und erscheint in einem Bericht.
10. **Leere Ordner** nach dem Umsortieren werden nur nach Bestätigung entfernt, und nur wenn sie wirklich leer sind (auch ohne versteckte Dateien außer `.DS_Store`/`Thumbs.db`).
11. **Schutzlisten.** Nutzer kann Ordner als "gesperrt" markieren, dort plant die App keinerlei Operationen.

## 4. Tech-Stack

- **Tauri 2** (Rust-Backend, Web-Frontend). Entwicklung und Tests zuerst auf macOS (Apple Silicon + Intel als Universal Binary), Windows ab Phase 7 voll unterstützt. Pfadlogik von Anfang an plattformneutral schreiben (`std::path`, keine hartkodierten `/`).
- **Frontend:** Vue 3 + Vite + TypeScript, Pinia, Tailwind. Virtualisierte Grids (z. B. `vue-virtual-scroller`) für zehntausende Thumbnails.
- **Datenbank:** SQLite über `rusqlite` (WAL-Modus), Migrationen versioniert.
- **Metadaten:** `exiftool` als gebündeltes Sidecar-Binary im `-stay_open`-Modus (beste RAW-Abdeckung inkl. CR3). Fallback für JPEG/HEIC: Crate `kamadak-exif`.
- **RAW-Vorschau:** eingebettetes JPEG-Preview per exiftool extrahieren (`-PreviewImage`, `-JpgFromRaw`). Kein Demosaicing nötig.
- **Thumbnails:** Crate `image` + `fast_image_resize`, Cache als WebP im App-Datenverzeichnis, Schlüssel = Datei-Hash.
- **Hashing:** BLAKE3 (Crate `blake3`), parallel mit `rayon`. Vorfilter über Dateigröße.
- **Videos:** `ffprobe` (Dauer, Codec, Auflösung) und `ffmpeg` (Vorschaubild bei ca. 10 % der Laufzeit) als gebündelte Sidecar-Binaries, LGPL-Build. Metadaten zusätzlich über exiftool (QuickTime/MP4-Tags). Wiedergabe im UI über das HTML5-`<video>`-Element, wo der Codec unterstützt wird, sonst nur Vorschaubild plus "Im Standardplayer öffnen".
- **Speicherübersicht:** Treemap/Sunburst mit Apache ECharts.
- **Papierkorb:** Crate `trash`.
- **Distribution:** macOS signiert und notarisiert (Developer ID), Windows-Installer (MSI/NSIS) später.
- **Dateisystem-Zugriff:** ausschließlich über ein internes Modul `fsops` (siehe 5.3). Kein anderer Code ruft `std::fs` schreibend auf. Per Lint/Grep-Test erzwungen.

## 5. Architektur

### 5.1 Module (Rust)

```
src-tauri/src/
  db/          Schema, Migrationen, Queries
  scan/        Ordner-Crawler, Dateierkennung, Paarbildung (RAW+JPEG, Sidecars)
  meta/        exiftool-Prozess, Parsing (Datum, Kamera, Objektiv, GPS, Ausrichtung)
  hash/        BLAKE3, Größen-Vorfilter
  thumbs/      Preview-Extraktion, Thumbnail-Cache
  events/      Clustering und Event-Verwaltung
  dupes/       Datei- und Ordner-Duplikate
  storage/     Größenaggregation pro Ordner und Dateityp
  video/       ffprobe/ffmpeg-Anbindung
  plan/        Zielpfad-Vorlagen, Operationsplan-Erzeugung, Konfliktauflösung
  fsops/       EINZIGE Stelle mit Schreibzugriff: move, copy+verify, mkdir, trash
  journal/     Laufprotokoll, Undo, Wiederaufnahme
  commands.rs  Tauri-Commands (dünne Schicht)
```

### 5.2 Datenmodell (SQLite, vereinfacht)

- `library_root(id, path, locked)`
- `asset(id, root_id, rel_path, size, mtime, hash, kind[raw|jpeg|heic|video|...], group_id)`
- `video_meta(asset_id, duration_s, codec, fps, width, height)`
- `asset_group(id, primary_asset_id)`: RAW+JPEG+Sidecars als Einheit
- `meta(asset_id, taken_at, tz_offset, camera_make, camera_model, lens, gps_lat, gps_lon, width, height, orientation)`
- `event(id, title, date_start, date_end, notes, confirmed)`
- `event_asset(event_id, group_id)`
- `rating(group_id, stars, flag[keep|reject|none], updated_at)`
- `folder(id, rel_path, root_id, parent_id, content_hash, file_count, own_size, total_size)`
- `folder_size_by_kind(folder_id, kind, file_count, total_size)` (rekursiv aggregiert)
- `run(id, kind, created_at, status[planned|running|done|aborted|undone], dry_run)`
- `operation(id, run_id, seq, op, src, dst, hash, status[planned|done|failed|undone], error)`

### 5.3 `fsops`-Vertrag

- `move_verified(src, dst) -> Result` prüft: Quelle existiert, Hash stimmt mit Index, Ziel existiert nicht, Zielpfad liegt nicht in gesperrtem Root.
- `trash(path)` nur mit Token aus einem bestätigten Plan.
- Alle Funktionen verlangen eine `OperationId`, ohne Journal-Eintrag keine Ausführung.

## 6. Funktionen im Detail

### 6.1 Scan
- Nutzer wählt einen oder mehrere Quellordner.
- Rekursiver Crawl, ignoriert Systemdateien. Inkrementell: nur neue/geänderte Dateien (Größe + mtime) erneut verarbeiten.
- Fortschrittsanzeige, abbrechbar.

### 6.2 Event-Erkennung (Vorsortierung)
- Sortierung nach `taken_at`. Fehlt EXIF-Datum: Fallback auf Datum im Dateinamen (`IMG_20230512_...`), danach mtime, jeweils sichtbar markiert ("Datum unsicher").
- Videos: Aufnahmedatum aus QuickTime `CreationDate` (enthält Zeitzone, bevorzugt) bzw. `CreateDate`. Achtung: `CreateDate` ist laut Spezifikation UTC, viele Kameras schreiben aber Ortszeit. Pro Kameramodell einstellbare Korrektur ("Zeit ist UTC" / "Zeit ist lokal"), Abweichungen zu Fotos derselben Kamera im selben Event als Hinweis anzeigen.
- Videos werden wie Fotos Events zugeordnet.
- Neues Event, wenn Zeitlücke > X Stunden (Standard 4 h, im UI einstellbar) oder GPS-Distanz > Y km (Standard 30 km).
- Mehrtägige Events (Urlaub) erkennen: aufeinanderfolgende Tage mit ähnlichem GPS-Schwerpunkt optional zusammenfassen.
- Vorschlag für Titel: Ortsname wäre schön, ist aber offline schwierig. v1: Titel leer bzw. bisheriger Ordnername als Vorschlag.
- UI: Timeline mit Event-Karten (Datum, Anzahl, 4 Vorschaubilder). Aktionen: umbenennen, zusammenführen, an Bild X teilen, einzelne Bilder verschieben, Event bestätigen.

### 6.3 Aussortieren (Culling)
- Vollbild-Ansicht pro Event mit Tastatursteuerung: Pfeile, `P` behalten, `X` Ausschuss, `1-5` Sterne, `U` zurücksetzen.
- Vergleichsansicht für Serien (2-4 Bilder nebeneinander).
- Optional: Ähnlichkeitsgruppen (Burst-Serien) per Perceptual Hash (`image_hasher`), Zeitabstand < 2 s.
- Videos: Vorschaubild, Dauer, Abspielen im Vollbild (sofern Codec unterstützt), gleiche Bewertungstasten.
- **Optional: XMP-Export** (Einstellung, standardmäßig aus). Schreibt Sterne und Pick/Reject in `.xmp`-Sidecars neben RAW-Dateien, damit Lightroom/Capture One/Bridge sie lesen. Nie in die Bilddatei selbst. Bestehende XMP-Dateien werden gemergt, nicht überschrieben, und vorher im Journal gesichert (Undo möglich). Läuft wie jede Schreibaktion über einen bestätigten Plan.
- Ausschuss bleibt physisch liegen, bis der Nutzer in einem separaten Schritt "Ausschuss in Papierkorb" bzw. "Ausschuss in Ordner `_Aussortiert` verschieben" plant und bestätigt. Standard ist der Ordner, nicht der Papierkorb.

### 6.4 Duplikate
- **Dateien:** Gruppen gleicher Größe, dann BLAKE3. Ergebnis: Duplikatgruppen mit allen Fundorten.
- **Ordner 1:1:** Ordner-Hash = Hash über sortierte Liste `(relativer Name, Datei-Hash)`. Gleicher Hash = identischer Ordner. Zusätzlich Variante "inhaltsgleich trotz anderer Dateinamen" (nur Datei-Hashes sortiert).
- **Ordner als Teilmenge:** Ordner A, dessen Datei-Hashes vollständig in Ordner B enthalten sind (z. B. halber Kopiervorgang).
- UI: Liste mit Paaren, Größe, Pfad, Datum. Nutzer wählt pro Gruppe, welche Kopie bleibt. Vorgeschlagen wird die Kopie im "ordentlicheren" Pfad, aber nie automatisch übernommen.

### 6.5 Umsortieren
- Ziel-Root wählen (darf auch der Quell-Root sein).
- Vorlage-Editor mit Platzhaltern: `{Jahr}`, `{Monat}`, `{Datum}` (YYYY-MM-DD), `{Anlass}`, `{Kamera}`, `{Original-Dateiname}`. Standard: `{Jahr}/{Datum} {Anlass}/{Original-Dateiname}`.
- Mehrtägige Events: `{Datum}` = Startdatum, optional `2024-07-12 bis 2024-07-20 Kroatien`.
- Bilder ohne Event landen in `{Jahr}/_Unsortiert/`.
- Dateinamen-Bereinigung: verbotene Zeichen ersetzen (Windows-kompatibel), Länge begrenzen.
- **Standard: Verschieben.** Option "Kopieren statt Verschieben" (Quelle bleibt unberührt) als Schalter im Planer. Vor dem Kopieren freien Speicherplatz am Ziel prüfen und anzeigen.
- Optional getrennte Vorlage für Videos, z. B. `{Jahr}/{Datum} {Anlass}/Videos/`.
- Plan-Vorschau als Baum mit Zählern und Konfliktliste, dann Ausführung mit Fortschritt, Pause und Abbruch.

### 6.6 Speicherübersicht
- Ordnerbaum mit Spalten: Gesamtgröße, eigene Größe, Anzahl Dateien, Anteil am Elternordner. Sortierbar nach Größe.
- Aufschlüsselung pro Ordner nach Typ: RAW, JPEG/HEIC, Video, Sidecars, Sonstiges (als gestapelter Balken).
- Treemap- bzw. Sunburst-Ansicht, Klick zoomt in Unterordner.
- Kennzahlen "Einsparbar": Größe aller redundanten Duplikat-Kopien, Größe des markierten Ausschusses, RAW+JPEG-Paare (JPEG-Anteil, nur als Info).
- Top-Liste: größte Ordner, größte Einzeldateien (meist Videos).
- Reine Leseansicht, Aktionen nur per Sprung in Duplikate/Sichten/Umsortieren.
- Werte kommen aus dem Index, Aktualisierung nach jedem Scan bzw. Lauf.

## 7. UI-Struktur

Linke Navigation: **Bibliothek** · **Speicher** · **Events** · **Sichten** · **Duplikate** · **Umsortieren** · **Verlauf** (Läufe mit Undo) · **Einstellungen**

Statusleiste unten: Anzahl Dateien, offene Pläne, letzter Lauf. Klar sichtbarer Hinweis, wenn sich die App im Dry-Run-Modus befindet.

## 8. Umsetzungsphasen für Claude Code

Jede Phase endet mit lauffähigem Stand, Tests, abgehakter Checkliste (Abschnitt 12) und kurzem Update in `CHANGELOG.md`. Entwicklung auf macOS, Windows-Kompatibilität im Code von Anfang an mitdenken.

**Phase 0: Gerüst**
- Tauri 2 + Vue 3 + TS + Tailwind aufsetzen, SQLite mit Migrationen.
- exiftool, ffmpeg und ffprobe als Sidecars einbinden (macOS zuerst).
- `fsops` als einziger Schreibpfad, CI-Grep-Test gegen schreibende `std::fs`-Aufrufe außerhalb.
- Akzeptanz: App startet, zeigt Versionen aller Sidecars.

**Phase 1: Scan und Index**
- Crawler, Paarbildung, Metadaten (Fotos, RAW, Videos), BLAKE3, Thumbnails inkl. RAW-Previews und Video-Frames.
- Akzeptanz: 10.000 Dateien inkrementell scanbar, Grid-Ansicht. Test: Scan verändert keine mtime und keinen Hash im Quellordner.

**Phase 2: Speicherübersicht**
- Ordneraggregation, Ordnerbaum mit Größen, Typaufschlüsselung, Treemap, Top-Listen.
- Akzeptanz: Summen stimmen mit `du` auf Fixture überein (Toleranz für Blockgrößen dokumentiert).

**Phase 3: Duplikate**
- Datei-, Ordner-1:1- und Teilmengen-Erkennung, UI-Liste ohne Aktionen, "Einsparbar"-Kennzahl in der Speicheransicht.
- Akzeptanz: Fixture mit kopiertem und halb kopiertem Ordner wird korrekt erkannt.

**Phase 4: Events**
- Clustering, Video-Zeitzonenkorrektur, Event-UI (benennen, zusammenführen, teilen, bestätigen).
- Akzeptanz: Fixture mit drei Tagen und einer Reise ergibt sinnvolle Vorschläge, alle Änderungen nur in der DB.

**Phase 5: Sichten (Culling)**
- Vollbild, Video-Wiedergabe, Tastatursteuerung, Vergleichsansicht, Bewertungen in DB.
- Akzeptanz: 500 Bilder flüssig durchklickbar (Previews vorgeladen).

**Phase 6: Planer und sichere Ausführung**
- Vorlagen-Editor, Verschieben (Standard) und Kopieren (Option), Plan-Erzeugung, Konfliktauflösung, Vorschau-Baum, Dry-Run.
- Journal, Ausführung, verifiziertes Cross-Volume-Move, Undo, Wiederaufnahme.
- Duplikat- und Ausschuss-Aktionen nutzen denselben Planer.
- Akzeptanz: alle Tests aus Abschnitt 9 grün.

**Phase 7: Windows und Feinschliff**
- Windows-Build mit Sidecars, Pfadlängen (`\\?\`-Präfix), reservierte Namen (`CON`, `NUL`, ...), Laufwerksbuchstaben, Papierkorb unter Windows.
- Gesperrte Ordner, Leere-Ordner-Bereinigung, Export des Laufprotokolls, Einstellungen.
- macOS Signierung und Notarisierung, Windows-Installer.

**Phase 8: Optionale Erweiterungen**
- XMP-Export der Bewertungen (standardmäßig aus).
- Burst-/Ähnlichkeitsgruppen per Perceptual Hash.
- Offline-Reverse-Geocoding für Titelvorschläge (GeoNames-Datensatz).

## 9. Tests (Pflicht)

- **Invariante:** Nach jedem Lauf gilt `Menge der Hashes vorher == Menge der Hashes nachher` (bei Kopier-Läufen: Obermenge). Papierkorb-Läufe: jeder getrashte Hash existiert noch mindestens einmal oder wurde einzeln freigegeben.
- Property-Tests (`proptest`): zufällige Ordnerbäume, zufällige Pläne, Ausführung, Undo, danach Baum identisch zum Ausgangszustand.
- Absturz-Simulation: Ausführung nach N Operationen abbrechen, Wiederaufnahme und Undo prüfen.
- Kollisionen: gleichnamige Dateien mit gleichem und unterschiedlichem Inhalt.
- Cross-Volume: zweites temporäres Volume bzw. Mock, Hash-Fehler beim Kopieren simulieren.
- Gruppen: RAW+JPEG+XMP und Live-Photo (HEIC+MOV) werden immer gemeinsam bewegt.
- Unicode-Pfade (Umlaute, NFC/NFD auf macOS), lange Pfade, Leerzeichen, Windows-reservierte Namen.
- Gesperrte Roots werden nie Ziel oder Quelle einer Operation.
- XMP-Export: bestehende Sidecars werden gemergt, Undo stellt Originalinhalt wieder her.

## 10. Entscheidungen

| Thema | Entscheidung |
|---|---|
| Plattform | macOS und Windows, macOS hat Priorität |
| Umsortieren | Standard Verschieben, Kopieren als Option |
| XMP-Export | Optionale Funktion, standardmäßig aus (Phase 8) |
| Videos | Voll unterstützt: Scan, Events, Sichten, Duplikate, Umsortieren |
| Speicherübersicht | Eigener Bereich, Phase 2 |

## 11. Arbeitsanweisung an Claude Code

- Phase für Phase vorgehen, nach jeder Phase stoppen, kurz berichten und die Checkliste in Abschnitt 12 abhaken (`[ ]` zu `[x]`).
- Vor Phase 6 das Sicherheitsmodell (Abschnitt 3) als Checkliste gegen den Code prüfen.
- Niemals Testläufe gegen echte Foto-/Videoordner des Nutzers, nur gegen Fixtures in einem temporären Verzeichnis.
- Bei Unklarheiten zu Dateioperationen immer die sicherere Variante wählen und im Bericht erwähnen.

## 12. Fortschritts-Checkliste

### Phase 0: Gerüst
- [x] Tauri 2 Projekt mit Vue 3, TypeScript, Vite, Tailwind, Pinia anlegen
- [x] Projektstruktur der Rust-Module (Abschnitt 5.1) anlegen
- [x] SQLite mit `rusqlite`, WAL-Modus, Migrationssystem
- [x] Initiales Schema (Abschnitt 5.2)
- [x] exiftool als Sidecar (macOS) mit `-stay_open`-Prozessverwaltung
- [x] ffmpeg und ffprobe als Sidecar (macOS, LGPL-Build)
- [x] Modul `fsops` als einziger Schreibpfad
- [x] CI-Test: kein schreibender `std::fs`-Aufruf außerhalb von `fsops`
- [ ] Testfixtures-Ordner (JPEG, HEIC, PNG, CR2, CR3, NEF, ARW, DNG, MP4, MOV, Live-Photo-Paar, XMP, Datei ohne EXIF, unbekannte Datei) — alles außer ARW vorhanden
- [x] Startbildschirm zeigt Sidecar-Versionen

### Phase 1: Scan und Index
- [x] Ordnerauswahl (mehrere Quell-Roots)
- [x] Rekursiver Crawler mit Ignorierliste für Systemdateien
- [x] Dateityp-Erkennung über Endung und Magic Bytes
- [x] Paarbildung: RAW+JPEG, Sidecars, Live Photos
- [x] Metadaten Fotos/RAW via exiftool
- [x] Metadaten Videos via exiftool + ffprobe
- [x] Datums-Fallback (Dateiname, mtime) mit Markierung "Datum unsicher"
- [x] BLAKE3-Hashing parallel mit Größen-Vorfilter
- [x] Inkrementeller Scan (Größe + mtime)
- [x] RAW-Preview-Extraktion
- [x] Video-Vorschaubild via ffmpeg
- [x] Thumbnail-Cache (WebP, Hash als Schlüssel)
- [x] Fortschrittsanzeige und Abbruch
- [x] Bibliotheks-Grid (virtualisiert) mit Filter nach Typ
- [x] Test: Scan verändert Quellordner nicht
- [x] Performance-Test 10.000 Dateien

### Phase 2: Speicherübersicht
- [x] Ordnertabelle mit `own_size`, `total_size`, Dateianzahl
- [x] Aggregation nach Dateityp
- [x] Ordnerbaum-Ansicht, sortierbar nach Größe
- [x] Gestapelter Balken pro Ordner nach Typ
- [x] Treemap/Sunburst mit Zoom in Unterordner
- [x] Top-Listen größte Ordner und Dateien
- [x] Aktualisierung nach Scan und Lauf
- [x] Test: Summen gegen Fixture

### Phase 3: Duplikate
- [x] Datei-Duplikatgruppen (Größe, dann Hash)
- [x] Ordner-Hash und 1:1-Ordnererkennung
- [x] Inhaltsgleiche Ordner trotz anderer Dateinamen
- [x] Teilmengen-Erkennung
- [x] Duplikate-UI mit Fundorten und Vorschlag "welche Kopie bleibt"
- [x] Kennzahl "Einsparbar" in Speicheransicht
- [x] Test: Fixture mit kopiertem und halb kopiertem Ordner

### Phase 4: Events
- [x] Clustering nach Zeitlücke (einstellbar)
- [x] Clustering nach GPS-Distanz (einstellbar)
- [x] Mehrtägige Events zusammenfassen
- [x] Video-Zeitzonenkorrektur pro Kameramodell
- [x] Timeline-UI mit Event-Karten
- [x] Event umbenennen, zusammenführen, teilen, Bilder verschieben
- [x] Event bestätigen
- [x] Bisheriger Ordnername als Titelvorschlag
- [x] Test: Fixture mit drei Tagen und Reise

### Phase 5: Sichten
- [x] Vollbildansicht Fotos
- [x] Videowiedergabe bzw. Fallback "Im Standardplayer öffnen"
- [x] Tastatursteuerung (Pfeile, P, X, 1-5, U)
- [x] Vergleichsansicht 2-4 Bilder
- [x] Bewertungen und Flags in DB
- [x] Preloading der Nachbarbilder
- [x] Filter nach Bewertung/Flag
- [x] Performance-Test 500 Bilder

### Phase 6: Planer und sichere Ausführung
- [x] Sicherheitsmodell-Review (Abschnitt 3) gegen Code
- [x] Vorlagen-Editor mit Platzhaltern und Live-Vorschau
- [x] Separate Video-Vorlage (optional)
- [x] Schalter Verschieben (Standard) / Kopieren
- [x] Speicherplatzprüfung vor Kopieren
- [x] Dateinamen-Bereinigung (Windows-kompatibel)
- [x] Plan-Erzeugung inkl. Gruppen (RAW+JPEG+Sidecars, Live Photos)
- [x] Konfliktauflösung (gleicher Hash, Suffix)
- [x] Vorschau-Baum mit Zählern und Konfliktliste
- [x] Dry-Run-Modus
- [x] Journal (geplant/erledigt) vor und nach jeder Operation
- [x] Ausführung mit Fortschritt, Pause, Abbruch
- [x] Verifiziertes Cross-Volume-Move
- [x] Undo pro Lauf
- [x] Wiederaufnahme nach Absturz
- [x] Ausschuss nach `_Aussortiert` verschieben
- [x] Papierkorb-Aktion mit Einzelfreigabe und Bestätigungsdialog
- [x] Duplikat-Aktionen über Planer
- [x] Verlaufsansicht aller Läufe
- [x] CSV/JSON-Log pro Lauf
- [x] Alle Tests aus Abschnitt 9 grün

### Phase 7: Windows und Feinschliff
- [ ] Windows-Sidecars (exiftool, ffmpeg, ffprobe) — später auf Windows; Skript `scripts/fetch-sidecars.ps1` vorbereitet, ungeprüft
- [ ] Lange Pfade und reservierte Namen — reservierte Namen erledigt und getestet, `\\?\`-Präfix implementiert; unter Windows noch ungeprüft
- [ ] Papierkorb unter Windows testen — später auf Windows
- [ ] Testsuite auf Windows grün — später auf Windows; CI-Job `windows-latest` vorbereitet. Stand 2026-10-31: Build und Sidecars laufen in der CI, die exiftool-Tests hängen dort (`-stay_open` mit `exiftool.exe`), Job daher vorerst nicht blockierend
- [x] Gesperrte Ordner
- [x] Leere-Ordner-Bereinigung mit Bestätigung
- [x] Einstellungsseite
- [ ] macOS Universal Binary, Signierung, Notarisierung — Universal Binary gebaut und geprüft; Signierung/Notarisierung braucht eine Developer-ID (README)
- [ ] Windows-Installer — später auf Windows; NSIS/MSI konfiguriert

### Phase 8: Optional
- [x] XMP-Export (standardmäßig aus) mit Merge und Undo
- [x] Burst-/Ähnlichkeitsgruppen
- [x] Offline-Reverse-Geocoding
