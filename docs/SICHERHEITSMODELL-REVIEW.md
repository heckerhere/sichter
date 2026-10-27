# Sicherheitsmodell-Review vor Phase 6

Prüfung von PLAN.md, Abschnitt 3, gegen den Code (Stand nach Phase 5, Commit `60bdef6`), wie in Abschnitt 11 verlangt. Für jede Regel: der Ist-Stand, wo sie durchgesetzt wird und was Phase 6 noch liefern muss. Nach Phase 6 ist die Tabelle aktualisiert (Spalte „Nach Phase 6“).

## Ausgangslage

- Außerhalb von `src/fsops/` gibt es **keinen** schreibenden Dateizugriff, erzwungen durch `tests/fsops_guard.rs` (Textsuche, mit Selbsttest).
- Die einzigen Schreibzugriffe bis Phase 5 laufen über `fsops::appdata::AppDataDir` und betreffen nur das App-Datenverzeichnis: `thumbs/`, `previews/`, Datenbank. `AppDataDir::join` weist `..`, absolute Pfade und leere Pfade ab (getestet).
- Die Bestand-Funktionen `move_verified`, `copy_verified`, `mkdir` und `trash` haben bis Phase 5 **keinen Aufrufer**.
- `OperationId::from_journal` und `TrashToken::from_confirmed_plan` sind `pub(crate)`. Theoretisch könnte also jedes Modul der Crate sie erzeugen. → Phase 6: zusätzlicher Guard-Test, dass sie nur in `journal/` bzw. `plan/` vorkommen.
- Protokolle `thumb://`, `preview://` und `media://` lesen nur. `media://` liefert ausschließlich indexierte Videos per Asset-ID, `preview://` und `thumb://` nur Cache-Dateien per gültigem 64-stelligem Hex-Hash.
- „Im Standardplayer öffnen“ (Sichten, nicht abspielbare Videos) übergibt die Datei an ein externes Programm. Was dieses damit tut, liegt außerhalb von Sichter.

## Regeln

| # | Regel | Stand vor Phase 6 | Phase 6 muss liefern | Nach Phase 6 |
|---|---|---|---|---|
| 1 | Lesen und Schreiben getrennt | Erfüllt. Scan, Analyse, Events und Bewertungen schreiben nur in die DB. Guard-Test. | Nichts Neues. Guard um Token-Konstruktoren erweitern. | Erfüllt. Zusätzlicher Guard-Test `tokens_are_only_minted_by_the_journal`. |
| 2 | Alle Änderungen über einen Operationsplan, Vorschau, Bestätigung mit Dateianzahl | Kein Aufrufer, also nicht verletzbar. | Planer erzeugt `run` + `operation`-Zeilen. Ausführung nur per Command mit Run-ID **und** bestätigter Anzahl, die mit dem Plan übereinstimmen muss. | Erfüllt. `journal::execute` verlangt Run-ID und die bestätigte Dateianzahl (Test `dry_run_changes_nothing_and_execution_needs_matching_confirmation`). |
| 3 | Dry-Run ist Standard | `run.dry_run` hat Standard 1. Hinweis in der Statusleiste nur als Anzeige. | Globale Einstellung „Dry-Run-Modus“, Standard an. Probelauf prüft Vorbedingungen und protokolliert, ohne zu schreiben. Echte Ausführung nur bei ausgeschaltetem Modus. | Erfüllt. Einstellung `runs.dry_run` (Standard an), `run_execute` verweigert bei aktivem Modus, Probelauf ändert nichts (Test). |
| 4 | Kein Überschreiben; gleicher Hash = Duplikat melden, Quelle bleibt; sonst Suffix ` (2)` | `fsops` verweigert bestehende Ziele (No-Replace-Rename, `create_new`), getestet. | Konfliktauflösung im Planer: identisch → überspringen und melden, verschieden → Suffix für die ganze Gruppe gleich. | Erfüllt, inkl. Befund aus dem Test: identische Dateien mit demselben *geplanten* Ziel werden nicht mehr als „(2)“ kopiert, sondern als Duplikat gemeldet (3 Tests). |
| 5 | Verschieben verifiziert, Cross-Volume per Kopie + Hash | `move_verified` prüft Quell-Hash gegen den Index, kopiert über Volume-Grenzen und prüft die Kopie. Der Kopierfehlerfall ist getestet, der Cross-Volume-Zweig selbst noch nicht direkt. | Test-Hook „Cross-Device erzwingen“ + Test des kompletten Zweigs inkl. Hash-Fehler. | Erfüllt. Test-Hook plus echter Test auf RAM-Disk (`real_cross_volume_move_and_undo`, läuft explizit). Unterbrochenes Cross-Volume-Verschieben wird nur bei identischer Kopie abgeschlossen. |
| 6 | Löschen nur in den Papierkorb, nur explizit freigegeben, Duplikate nur bei verifizierter Kopie | `trash` nur mit `TrashToken`, kein `remove_file` außer nach verifizierter Kopie bzw. eigener fehlerhafter Kopie. | Papierkorb-Pläne nur mit Einzelfreigabe. Bei Duplikaten speichert jede Operation den Pfad der Kopie, die bleibt. Direkt vor dem Papierkorb wird die bleibende Kopie neu gehasht. | Erfüllt. Einzelfreigabe in der UI, Bestätigungsdialog mit vollständiger Dateiliste (wird bei Papierkorb-Läufen nie gekürzt), erneute Prüfung der bleibenden Kopie (Test). Echter Papierkorb-Test läuft explizit. |
| 7 | Journal vor/nach jeder Operation, Abbruch nachvollziehbar, Undo pro Lauf, CSV/JSON-Log | Tabellen vorhanden, keine Logik. | Status vor der Ausführung geplant, danach erledigt. Wiederaufnahme anhand des Dateisystemzustands (idempotent), damit auch ein verlorener letzter DB-Commit keinen Schaden anrichtet. Undo rückwärts. Log je Lauf. | Erfüllt. Absturz-Simulation, Wiederaufnahme, Abbruch/Fortsetzen, Undo, JSON- und CSV-Log (Tests), Property-Test mit Zufallsbäumen (12 Fälle, einmalig 80). |
| 8 | Zusammengehörige Dateien bleiben zusammen | Paarbildung vorhanden (Phase 1). | Planer plant immer ganze Gruppen. Duplikat-Aktionen überspringen Dateien, deren übrige Gruppenmitglieder nicht mit entfernt werden. | Erfüllt. Gruppen werden am Stück ausgeführt, bei Fehler zurückgerollt (Test `group_failure_rolls_back_the_whole_group`), Duplikate zerreißen keine Gruppen (Test). |
| 9 | Unbekannte Dateien werden nicht angefasst | Art `other` und verwaiste Sidecars haben keine Gruppe. | Planer nimmt nur Gruppenmitglieder (keine `other`). Test. | Erfüllt (Test: `notizen.txt` bleibt am Ort). |
| 10 | Leere Ordner nur nach Bestätigung | Keine Logik. | Phase 7 laut Plan. Phase 6 entfernt nur beim Undo selbst angelegte, leere Ordner. | Teilweise. Phase 6 entfernt nur beim Undo selbst angelegte, wirklich leere Ordner. Die Bereinigung leerer Quellordner folgt in Phase 7. |
| 11 | Gesperrte Roots: keine Operationen | `library_root.locked` und `PathGuard` existieren, werden aber nirgends angewendet. | Planer schließt gesperrte Roots als Quelle und Ziel aus. Der Executor baut den `PathGuard` bei jeder Ausführung **neu aus der DB** (Sperre nach Planung greift trotzdem). Test. | Erfüllt. Gesperrt als Ziel → kein Plan, als Quelle → keine Gruppen, Sperre nach der Planung → Ausführung verweigert (Test). |

## Zusätzliche Befunde

1. **Index nach Verschieben aktualisieren.** Ohne Aktualisierung zeigten Index, Bewertungen und Events auf alte Pfade, und ein Rescan würde die Dateien als „gelöscht + neu“ werten. Dabei gingen Bewertungen verloren. → Executor aktualisiert `asset.root_id`/`rel_path` pro erledigter Operation, die Gruppen-IDs bleiben.
2. **Ziel außerhalb der Bibliothek.** Dateien würden aus dem Index fallen. → Ziel muss ein (nicht gesperrter) Bibliotheks-Root sein.
3. **Planungsstand vs. Ausführung.** Zwischen Planung und Ausführung kann sich der Bestand ändern. → Jede Operation prüft beim Ausführen erneut: Quelle existiert, Hash stimmt, Ziel frei. Sonst schlägt die Operation fehl und der Lauf läuft weiter. Nichts wird „passend gemacht“.
4. **Papierkorb-Undo auf macOS.** Die `trash`-Crate kann auf macOS nicht programmatisch zurücklegen. → Undo markiert solche Operationen als „im Finder über ‚Zurücklegen‘ wiederherstellen“, statt etwas vorzutäuschen.

## Befunde aus den Phase-6-Tests

Der Property-Test und die Kollisionstests haben drei echte Fehler gefunden. Alle sind behoben und durch Regressionstests abgesichert:

1. **Planer:** Zwei identische Quelldateien mit demselben Ziel ergaben eine nummerierte Kopie `IMG (2).JPG` statt einer Duplikatmeldung (Verstoß gegen Regel 4).
2. **Event-Erkennung (Phase 4):** Ordnernamen, die mit einem Umlaut beginnen („Grüße aus Köln“), ließen den Scan abstürzen (Byte-Index mitten im Zeichen).
3. **Duplikat-Vorschlag (Phase 3):** derselbe Fehler bei Ordnernamen wie „Güßen“.

Außerdem gefunden: Die Liste der Operationen in der Plan-Ansicht ist auf 500 gekürzt. Bei Papierkorb-Läufen hätte die Bestätigung damit nicht alle Dateien gezeigt. Jetzt werden Papierkorb-Läufe nie gekürzt.
