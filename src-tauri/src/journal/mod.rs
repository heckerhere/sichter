//! Laufprotokoll, Ausführung, Undo und Wiederaufnahme (PLAN.md 3.7).
//!
//! * Jede Operation steht vor der Ausführung als `planned` im Journal und
//!   wird danach `done` bzw. `failed`.
//! * Vor jeder Operation wird der Zustand im Dateisystem geprüft. Dadurch ist
//!   die Ausführung idempotent: Nach einem Absturz (auch wenn der letzte
//!   Datenbank-Commit verloren ging) wird erledigte Arbeit erkannt statt doppelt
//!   ausgeführt, und nichts wird "passend gemacht".
//! * Gruppen sind atomar: Scheitert ein Mitglied, werden die schon bewegten
//!   Mitglieder derselben Gruppe sofort zurückgeschoben.
//! * Nach jeder erledigten Operation wird der Index nachgezogen, damit
//!   Bewertungen und Events an den Dateien bleiben.
//! * Nur hier entstehen `OperationId` und `TrashToken`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::fsops::appdata::AppDataDir;
use crate::fsops::{self, FsOpError, OperationId, PathGuard, TrashToken};
use crate::hash::{self, FileHash};

#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("{0}")]
    Invalid(String),
    #[error("Datenbankfehler: {0}")]
    Db(#[from] rusqlite::Error),
    /// Nur für Tests: simulierter Absturz nach N Dateioperationen.
    #[error("simulierter Absturz")]
    SimulatedCrash,
}

pub type Result<T> = std::result::Result<T, JournalError>;

#[derive(Debug, Clone)]
struct OpRow {
    id: i64,
    op: String,
    src: Option<PathBuf>,
    dst: Option<PathBuf>,
    hash: Option<String>,
    asset_id: Option<i64>,
    group_id: Option<i64>,
    keep_path: Option<PathBuf>,
    status: String,
    /// Zusatzdaten (XMP: Bewertung, geschriebener Hash, ob gesichert).
    note: serde_json::Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RunProgress {
    pub run_id: i64,
    pub done: usize,
    pub total: usize,
    pub current: Option<String>,
    pub paused: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunReport {
    pub run_id: i64,
    pub dry_run: bool,
    /// Dateien (angelegte Ordner zählen nicht mit).
    pub done: usize,
    pub failed: usize,
    pub undone: usize,
    pub skipped: usize,
    pub cancelled: bool,
    /// Probelauf bzw. Fehler: (Pfad, Meldung).
    pub messages: Vec<(String, String)>,
    pub log_path: Option<String>,
    /// Papierkorb-Operationen, die nicht automatisch zurückgelegt werden können.
    pub manual_restore: Vec<String>,
}

pub struct Exec<'a> {
    pub app: &'a AppDataDir,
    pub cancel: &'a AtomicBool,
    pub pause: &'a AtomicBool,
    pub progress: &'a dyn Fn(RunProgress),
    /// Nur für Tests: nach so vielen Dateioperationen "abstürzen", bevor das
    /// Journal aktualisiert wird.
    pub simulate_crash_after: Option<usize>,
    /// exiftool für den XMP-Export (Merge bestehender Sidecars).
    pub exiftool: Option<&'a crate::tools::ToolCommand>,
}

/// Was eine einzelne Operation zum Ausführen braucht.
struct Ctx<'a> {
    guard: &'a PathGuard,
    trash: Option<&'a TrashToken>,
    app: &'a AppDataDir,
    exiftool: Option<&'a crate::tools::ToolCommand>,
}

fn load_ops(conn: &Connection, run_id: i64) -> rusqlite::Result<Vec<OpRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, seq, op, src, dst, hash, asset_id, group_id, keep_path, status, note FROM operation WHERE run_id = ?1 ORDER BY seq",
    )?;
    stmt.query_map([run_id], |r| {
        let path = |i: usize| -> rusqlite::Result<Option<PathBuf>> { Ok(r.get::<_, Option<String>>(i)?.map(PathBuf::from)) };
        Ok(OpRow {
            id: r.get(0)?,
            op: r.get(2)?,
            src: path(3)?,
            dst: path(4)?,
            hash: r.get(5)?,
            asset_id: r.get(6)?,
            group_id: r.get(7)?,
            keep_path: path(8)?,
            status: r.get(9)?,
            note: r.get::<_, Option<String>>(10)?.and_then(|n| serde_json::from_str(&n).ok()).unwrap_or_default(),
        })
    })?
    .collect()
}

fn run_info(conn: &Connection, run_id: i64) -> Result<(String, String)> {
    conn.query_row("SELECT kind, status FROM run WHERE id = ?1", [run_id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| JournalError::Invalid("Lauf nicht gefunden.".into()))
}

/// Gesperrte Roots, bei jeder Ausführung frisch aus der Datenbank.
fn guard(conn: &Connection) -> rusqlite::Result<PathGuard> {
    let mut stmt = conn.prepare("SELECT path FROM library_root WHERE locked = 1")?;
    let roots: Vec<String> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(PathGuard::new(roots))
}

fn exists(p: &Path) -> bool {
    std::fs::symlink_metadata(p).is_ok()
}

fn matches_hash(p: &Path, expected: &FileHash) -> bool {
    hash::hash_file(p).is_ok_and(|h| h == *expected)
}

/// Root und relativer Pfad zu einem absoluten Pfad (für die Index-Pflege).
fn locate(conn: &Connection, path: &Path) -> rusqlite::Result<Option<(i64, String)>> {
    let mut stmt = conn.prepare("SELECT id, path FROM library_root")?;
    let roots: Vec<(i64, String)> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    Ok(roots.into_iter().find_map(|(id, root)| crate::scan::crawl::to_rel_path(Path::new(&root), path).map(|rel| (id, rel))))
}

fn set_status(conn: &Connection, op: &OpRow, status: &str, note: Option<&str>) -> rusqlite::Result<()> {
    conn.execute("UPDATE operation SET status = ?2, error = ?3 WHERE id = ?1", params![op.id, status, note])?;
    Ok(())
}

/// Verschiebt den Index-Eintrag mit der Datei; Gruppen-ID, Bewertung und Events bleiben.
fn reindex_move(conn: &Connection, asset_id: Option<i64>, to: &Path) -> rusqlite::Result<()> {
    let (Some(id), Some((root_id, rel))) = (asset_id, locate(conn, to)?) else { return Ok(()) };
    conn.execute("UPDATE asset SET root_id = ?2, rel_path = ?3 WHERE id = ?1", params![id, root_id, rel])?;
    Ok(())
}

enum Outcome {
    Done,
    /// War schon erledigt (z. B. vor einem Absturz).
    AlreadyDone,
    Failed(String),
}

fn describe(e: &FsOpError) -> String {
    e.to_string()
}

/// Führt eine einzelne Operation aus, nach Prüfung des Ist-Zustands.
fn backup_path(op: &OpRow) -> PathBuf {
    PathBuf::from("xmp-backup").join(format!("op-{}.xmp", op.id))
}

/// XMP-Export: bestehendes Sidecar sichern und mergen, sonst neu anlegen.
/// Gibt den Hash des geschriebenen Inhalts zurück (für Undo).
fn apply_xmp(op: &OpRow, ctx: &Ctx) -> std::result::Result<(FileHash, bool), String> {
    let id = OperationId::from_journal(op.id);
    let path = op.dst.as_ref().ok_or("Pfad fehlt")?;
    let rating = op.note["rating"].as_i64().ok_or("Bewertung fehlt")?;
    let planned = op.hash.as_deref().and_then(FileHash::from_hex);
    let bytes = match (&planned, exists(path)) {
        (Some(expected), true) => {
            if !matches_hash(path, expected) {
                return Err("XMP wurde seit der Planung verändert, nichts geschrieben".into());
            }
            // Original vor dem Merge sichern (Undo stellt es Byte für Byte wieder her).
            let original = std::fs::read(path).map_err(|e| e.to_string())?;
            ctx.app.write_file(backup_path(op), &original).map_err(|e| e.to_string())?;
            crate::meta::xmp::merge(ctx.exiftool.ok_or("exiftool nicht verfügbar")?, path, rating)?
        }
        (Some(_), false) => return Err("XMP fehlt inzwischen".into()),
        (None, true) => return Err("Inzwischen liegt dort eine XMP-Datei, nichts überschrieben".into()),
        (None, false) => crate::meta::xmp::new_sidecar(rating),
    };
    fsops::write_sidecar(id, path, &bytes, planned.as_ref(), ctx.guard).map_err(|e| describe(&e))?;
    Ok((hash::hash_file(path).map_err(|e| e.to_string())?, planned.is_some()))
}

fn apply(op: &OpRow, ctx: &Ctx) -> Outcome {
    let (guard, trash) = (ctx.guard, ctx.trash);
    let id = OperationId::from_journal(op.id);
    let expected = op.hash.as_deref().and_then(FileHash::from_hex);
    match op.op.as_str() {
        "mkdir" => {
            let Some(dst) = &op.dst else { return Outcome::Failed("Zielpfad fehlt".into()) };
            match fsops::mkdir(id, dst, guard) {
                Ok(true) => Outcome::Done,
                Ok(false) => Outcome::AlreadyDone,
                Err(e) => Outcome::Failed(describe(&e)),
            }
        }
        "move" | "copy" => {
            let (Some(src), Some(dst), Some(expected)) = (&op.src, &op.dst, expected) else { return Outcome::Failed("Unvollständige Operation".into()) };
            let (src_there, dst_there) = (exists(src), exists(dst));
            if dst_there {
                // Schon erledigt? Nur wenn das Ziel genau den erwarteten Inhalt hat.
                if !matches_hash(dst, &expected) {
                    return Outcome::Failed("Ziel existiert bereits mit anderem Inhalt, nichts verändert".into());
                }
                if op.op == "copy" || !src_there {
                    return Outcome::AlreadyDone;
                }
                // Verschieben über Volumes, unterbrochen nach verifizierter Kopie.
                return match fsops::finish_interrupted_move(id, src, dst, &expected, guard) {
                    Ok(()) => Outcome::Done,
                    Err(e) => Outcome::Failed(describe(&e)),
                };
            }
            if !src_there {
                return Outcome::Failed("Quelle fehlt".into());
            }
            let result = if op.op == "move" {
                fsops::move_verified(id, src, dst, &expected, guard).map(|_| ())
            } else {
                fsops::copy_verified(id, src, dst, &expected, guard)
            };
            match result {
                Ok(()) => Outcome::Done,
                Err(e) => Outcome::Failed(describe(&e)),
            }
        }
        "trash" => {
            let Some(src) = &op.src else { return Outcome::Failed("Pfad fehlt".into()) };
            let Some(token) = trash else { return Outcome::Failed("Papierkorb nicht freigegeben".into()) };
            if !exists(src) {
                return Outcome::AlreadyDone;
            }
            // Leerer Ordner: direkt vorher erneut prüfen, dass er noch leer ist.
            if src.is_dir() && !crate::plan::is_effectively_empty(src) {
                return Outcome::Failed("Ordner ist nicht mehr leer, bleibt stehen".into());
            }
            if let Some(expected) = &expected {
                if !matches_hash(src, expected) {
                    return Outcome::Failed("Datei hat sich seit dem Scan verändert, nicht in den Papierkorb gelegt".into());
                }
                // Duplikat: Die bleibende Kopie muss jetzt noch existieren und identisch sein.
                if let Some(keep) = op.keep_path.as_ref().filter(|k| !matches_hash(k, expected)) {
                    return Outcome::Failed(format!("Bleibende Kopie fehlt oder ist verändert ({}), nicht in den Papierkorb gelegt", keep.display()));
                }
            }
            match fsops::trash(id, src, token, guard) {
                Ok(()) => Outcome::Done,
                Err(e) => Outcome::Failed(describe(&e)),
            }
        }
        "xmp" => {
            // Wiederaufnahme: Schon geschrieben, wenn der Inhalt dem Geschriebenen entspricht.
            let written = op.note["written"].as_str().and_then(FileHash::from_hex);
            if let (Some(w), Some(p)) = (&written, &op.dst)
                && matches_hash(p, w)
            {
                return Outcome::AlreadyDone;
            }
            match apply_xmp(op, ctx) {
                Ok(_) => Outcome::Done,
                Err(e) => Outcome::Failed(e),
            }
        }
        other => Outcome::Failed(format!("Unbekannte Operation {other}")),
    }
}

/// Macht eine erledigte Operation rückgängig (für Undo und Gruppen-Rollback).
fn revert(op: &OpRow, ctx: &Ctx) -> std::result::Result<bool, String> {
    let (guard, trash) = (ctx.guard, ctx.trash);
    let id = OperationId::from_journal(op.id);
    let expected = op.hash.as_deref().and_then(FileHash::from_hex);
    match op.op.as_str() {
        "mkdir" => {
            let Some(dst) = &op.dst else { return Ok(false) };
            fsops::rmdir_if_empty(id, dst, guard).map_err(|e| describe(&e))
        }
        "move" => {
            let (Some(src), Some(dst), Some(expected)) = (&op.src, &op.dst, expected) else { return Err("Unvollständige Operation".into()) };
            match (exists(src), exists(dst)) {
                (true, false) => Ok(true), // schon zurück
                (false, true) => fsops::move_verified(id, dst, src, &expected, guard).map(|_| true).map_err(|e| describe(&e)),
                (true, true) => Err("Ursprünglicher Ort ist inzwischen wieder belegt".into()),
                (false, false) => Err("Datei ist an keinem der beiden Orte".into()),
            }
        }
        "copy" => {
            // Die Kopie wieder entfernen, aber nur in den Papierkorb und nur,
            // wenn das Original unverändert noch da ist.
            let (Some(src), Some(dst), Some(expected)) = (&op.src, &op.dst, expected) else { return Err("Unvollständige Operation".into()) };
            if !exists(dst) {
                return Ok(true);
            }
            let Some(token) = trash else { return Err("Papierkorb nicht freigegeben".into()) };
            if !matches_hash(src, &expected) || !matches_hash(dst, &expected) {
                return Err("Original oder Kopie verändert, Kopie bleibt liegen".into());
            }
            fsops::trash(id, dst, token, guard).map(|_| true).map_err(|e| describe(&e))
        }
        "trash" => Err("Liegt im Papierkorb: bitte dort „Zurücklegen“ wählen".into()),
        "xmp" => {
            let path = op.dst.as_ref().ok_or("Pfad fehlt")?;
            let written = op.note["written"].as_str().and_then(FileHash::from_hex).ok_or("Geschriebener Stand unbekannt")?;
            if !exists(path) {
                return Err("XMP fehlt".into());
            }
            if op.note["backup"].as_bool() == Some(true) {
                let original = std::fs::read(ctx.app.join(backup_path(op)).map_err(|e| e.to_string())?).map_err(|e| format!("Sicherung fehlt: {e}"))?;
                fsops::write_sidecar(id, path, &original, Some(&written), guard).map(|_| true).map_err(|e| describe(&e))
            } else {
                fsops::remove_own_sidecar(id, path, &written, guard).map(|_| true).map_err(|e| describe(&e))
            }
        }
        other => Err(format!("Unbekannte Operation {other}")),
    }
}

/// Merkt sich nach dem XMP-Schreiben den neuen Inhalt und ob gesichert wurde.
fn record_xmp(conn: &Connection, op: &OpRow, app: &AppDataDir) -> rusqlite::Result<()> {
    let Some(path) = &op.dst else { return Ok(()) };
    let mut note = op.note.clone();
    if let Ok(h) = hash::hash_file(path) {
        note["written"] = serde_json::Value::String(h.to_hex());
    }
    note["backup"] = serde_json::Value::Bool(app.join(backup_path(op)).is_ok_and(|p| p.is_file()));
    conn.execute("UPDATE operation SET note = ?2 WHERE id = ?1", params![op.id, note.to_string()])?;
    Ok(())
}

/// Operation mit aktuellem Journal-Stand (z. B. nach record_xmp) neu laden.
fn reload(conn: &Connection, op: &OpRow) -> rusqlite::Result<OpRow> {
    let note: Option<String> = conn.query_row("SELECT note FROM operation WHERE id = ?1", [op.id], |r| r.get(0))?;
    let mut fresh = op.clone();
    fresh.note = note.and_then(|n| serde_json::from_str(&n).ok()).unwrap_or_default();
    Ok(fresh)
}

fn after_success(conn: &Connection, op: &OpRow) -> rusqlite::Result<()> {
    match op.op.as_str() {
        "move" => reindex_move(conn, op.asset_id, op.dst.as_deref().expect("geprüft")),
        "trash" => {
            if let Some(id) = op.asset_id {
                conn.execute("DELETE FROM asset WHERE id = ?1", [id])?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn wait_while_paused(ex: &Exec) {
    while ex.pause.load(Ordering::Relaxed) && !ex.cancel.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn label(op: &OpRow) -> String {
    op.src.as_ref().or(op.dst.as_ref()).and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Probelauf: prüft alle Vorbedingungen, verändert nichts und lässt den Plan bestehen.
pub fn dry_run(conn: &Connection, run_id: i64, ex: &Exec) -> Result<RunReport> {
    let (_, status) = run_info(conn, run_id)?;
    if status != "planned" && status != "aborted" {
        return Err(JournalError::Invalid("Dieser Lauf ist bereits abgeschlossen.".into()));
    }
    let guard = guard(conn)?;
    let ops: Vec<OpRow> = load_ops(conn, run_id)?.into_iter().filter(|o| o.status == "planned").collect();
    let mut report = RunReport { run_id, dry_run: true, ..Default::default() };
    let mut planned_dirs: Vec<PathBuf> = Vec::new();
    for (i, op) in ops.iter().enumerate() {
        (ex.progress)(RunProgress { run_id, done: i, total: ops.len(), current: Some(label(op)), paused: false });
        let problem = match op.op.as_str() {
            "mkdir" => op.dst.as_ref().and_then(|d| {
                planned_dirs.push(d.clone());
                guard.check(d).err().map(|e| e.to_string())
            }),
            "move" | "copy" | "trash" => {
                let src = op.src.as_ref().expect("Quelle gesetzt");
                let size_now = std::fs::metadata(src).map(|m| m.len() as i64).ok();
                let size_planned: Option<i64> = conn.query_row("SELECT size FROM operation WHERE id = ?1", [op.id], |r| r.get(0)).ok();
                if size_now.is_none() {
                    Some("Quelle fehlt".into())
                } else if size_now != size_planned {
                    Some("Quelle hat sich seit der Planung verändert".into())
                } else if let Some(e) = guard.check(src).err() {
                    Some(e.to_string())
                } else if let Some(dst) = &op.dst {
                    let parent_ok = dst.parent().is_some_and(|p| p.is_dir() || planned_dirs.iter().any(|d| d == p));
                    if exists(dst) {
                        Some("Ziel existiert bereits".into())
                    } else if !parent_ok {
                        Some("Zielordner fehlt".into())
                    } else {
                        guard.check(dst).err().map(|e| e.to_string())
                    }
                } else {
                    op.keep_path.as_ref().filter(|k| !exists(k)).map(|k| format!("Bleibende Kopie fehlt: {}", k.display()))
                }
            }
            "xmp" => {
                let dst = op.dst.as_ref().expect("Ziel gesetzt");
                match op.hash.as_deref().and_then(FileHash::from_hex) {
                    Some(h) if !matches_hash(dst, &h) => Some("XMP wurde seit der Planung verändert".into()),
                    None if exists(dst) => Some("Inzwischen liegt dort eine XMP-Datei".into()),
                    _ => guard.check(dst).err().map(|e| e.to_string()),
                }
            }
            _ => Some("Unbekannte Operation".into()),
        };
        match problem {
            Some(msg) => {
                report.failed += 1;
                report.messages.push((op.src.as_ref().or(op.dst.as_ref()).map(|p| p.display().to_string()).unwrap_or_default(), msg));
            }
            None if op.op != "mkdir" => report.done += 1,
            None => {}
        }
    }
    report.log_path = write_log(conn, ex.app, run_id, "probelauf").ok();
    Ok(report)
}

/// Führt einen geplanten (oder abgebrochenen) Lauf aus bzw. setzt ihn fort.
/// `confirmed_files` muss der in der Bestätigung angezeigten Dateianzahl entsprechen.
pub fn execute(conn: &Connection, run_id: i64, confirmed_files: usize, ex: &Exec) -> Result<RunReport> {
    let (kind, status) = run_info(conn, run_id)?;
    if status != "planned" && status != "aborted" {
        return Err(JournalError::Invalid("Dieser Lauf ist bereits abgeschlossen.".into()));
    }
    let all = load_ops(conn, run_id)?;
    let files = all.iter().filter(|o| o.op != "mkdir").count();
    if confirmed_files != files {
        return Err(JournalError::Invalid(format!("Bestätigung passt nicht zum Plan ({confirmed_files} statt {files} Dateien).")));
    }
    let uses_trash = kind.ends_with("_to_trash");
    let token = uses_trash.then(TrashToken::from_confirmed_plan);
    let guard = guard(conn)?;
    conn.execute("UPDATE run SET status = 'running', dry_run = 0 WHERE id = ?1", [run_id])?;

    let pending: Vec<&OpRow> = all.iter().filter(|o| o.status == "planned").collect();
    let total = pending.len();
    let mut report = RunReport { run_id, ..Default::default() };
    let mut fs_ops = 0usize;
    let mut i = 0;
    while i < pending.len() {
        // Ganze Gruppe am Stück (mkdir-Operationen einzeln).
        let group = pending[i].group_id;
        let mut j = i + 1;
        while group.is_some() && j < pending.len() && pending[j].group_id == group {
            j += 1;
        }
        let chunk = &pending[i..j];
        i = j;

        wait_while_paused(ex);
        if ex.cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        let mut done_in_chunk: Vec<&OpRow> = Vec::new();
        let mut failure: Option<(usize, String)> = None;
        for (k, op) in chunk.iter().enumerate() {
            (ex.progress)(RunProgress { run_id, done: report.done + report.failed, total, current: Some(label(op)), paused: false });
            let ctx = Ctx { guard: &guard, trash: token.as_ref(), app: ex.app, exiftool: ex.exiftool };
            match apply(op, &ctx) {
                Outcome::Done | Outcome::AlreadyDone => {
                    fs_ops += 1;
                    if ex.simulate_crash_after == Some(fs_ops) {
                        return Err(JournalError::SimulatedCrash);
                    }
                    let tx = conn.unchecked_transaction()?;
                    set_status(&tx, op, "done", None)?;
                    after_success(&tx, op)?;
                    if op.op == "xmp" {
                        record_xmp(&tx, op, ex.app)?;
                    }
                    tx.commit()?;
                    done_in_chunk.push(op);
                    if op.op != "mkdir" {
                        report.done += 1;
                    }
                }
                Outcome::Failed(msg) => {
                    failure = Some((k, msg));
                    break;
                }
            }
        }
        if let Some((k, msg)) = failure {
            let failed = chunk[k];
            set_status(conn, failed, "failed", Some(&msg))?;
            report.failed += 1;
            report.messages.push((label(failed), msg.clone()));
            // Rest der Gruppe nicht anfassen, Erledigtes zurückschieben.
            for rest in &chunk[k + 1..] {
                set_status(conn, rest, "failed", Some("Übersprungen, weil ein anderes Mitglied der Gruppe fehlschlug"))?;
                report.skipped += 1;
            }
            for op in done_in_chunk.iter().rev() {
                let ctx = Ctx { guard: &guard, trash: None, app: ex.app, exiftool: ex.exiftool };
                match revert(&reload(conn, op)?, &ctx) {
                    Ok(_) => {
                        let tx = conn.unchecked_transaction()?;
                        set_status(&tx, op, "undone", Some("Gruppe zurückgesetzt, weil ein anderes Mitglied fehlschlug"))?;
                        if op.op == "move" {
                            reindex_move(&tx, op.asset_id, op.src.as_deref().expect("geprüft"))?;
                        }
                        tx.commit()?;
                        if op.op != "mkdir" {
                            report.done -= 1;
                            report.undone += 1;
                        }
                    }
                    Err(e) => report.messages.push((label(op), format!("Zurücksetzen fehlgeschlagen: {e}"))),
                }
            }
        }
    }

    let final_status = if report.cancelled { "aborted" } else { "done" };
    conn.execute(
        "UPDATE run SET status = ?2, finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1",
        params![run_id, final_status],
    )?;
    refresh_after_run(conn)?;
    report.log_path = write_log(conn, ex.app, run_id, "lauf").ok();
    Ok(report)
}

/// Macht einen ausgeführten Lauf rückwärts rückgängig.
pub fn undo(conn: &Connection, run_id: i64, ex: &Exec) -> Result<RunReport> {
    let (_, status) = run_info(conn, run_id)?;
    if status != "done" && status != "aborted" {
        return Err(JournalError::Invalid("Nur ausgeführte oder abgebrochene Läufe können rückgängig gemacht werden.".into()));
    }
    let guard = guard(conn)?;
    let token = TrashToken::from_confirmed_plan();
    let ops: Vec<OpRow> = load_ops(conn, run_id)?.into_iter().filter(|o| o.status == "done").rev().collect();
    let mut report = RunReport { run_id, ..Default::default() };
    for (i, op) in ops.iter().enumerate() {
        wait_while_paused(ex);
        if ex.cancel.load(Ordering::Relaxed) {
            report.cancelled = true;
            break;
        }
        (ex.progress)(RunProgress { run_id, done: i, total: ops.len(), current: Some(label(op)), paused: false });
        if op.op == "trash" {
            report.manual_restore.push(op.src.as_ref().map(|p| p.display().to_string()).unwrap_or_default());
            continue;
        }
        let ctx = Ctx { guard: &guard, trash: Some(&token), app: ex.app, exiftool: ex.exiftool };
        match revert(op, &ctx) {
            Ok(_) => {
                let tx = conn.unchecked_transaction()?;
                set_status(&tx, op, "undone", None)?;
                if op.op == "move" {
                    reindex_move(&tx, op.asset_id, op.src.as_deref().expect("geprüft"))?;
                }
                tx.commit()?;
                if op.op != "mkdir" {
                    report.undone += 1;
                }
            }
            Err(e) => {
                report.failed += 1;
                report.messages.push((label(op), e));
            }
        }
    }
    if !report.cancelled && report.failed == 0 {
        conn.execute("UPDATE run SET status = 'undone', finished_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now') WHERE id = ?1", [run_id])?;
    }
    refresh_after_run(conn)?;
    report.log_path = write_log(conn, ex.app, run_id, "undo").ok();
    Ok(report)
}

/// Beim Start: Läufe, die beim letzten Beenden noch liefen, gelten als abgebrochen.
pub fn recover_interrupted(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM run WHERE status = 'running'")?;
        stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    for id in &ids {
        conn.execute("UPDATE run SET status = 'aborted' WHERE id = ?1", [id])?;
    }
    Ok(ids)
}

/// Ordnergrößen und leere Gruppen nach einem Lauf aktualisieren.
fn refresh_after_run(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM asset_group WHERE id NOT IN (SELECT group_id FROM asset WHERE group_id IS NOT NULL)", [])?;
    let roots: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT id, path FROM library_root")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    for (id, path) in roots {
        if Path::new(&path).is_dir() {
            crate::storage::rebuild(conn, id)?;
        }
    }
    Ok(())
}

#[derive(Serialize)]
struct LogEntry {
    seq: i64,
    op: String,
    src: Option<String>,
    dst: Option<String>,
    hash: Option<String>,
    status: String,
    error: Option<String>,
}

/// Lesbares Protokoll je Lauf als JSON und CSV im App-Verzeichnis `logs/`.
fn write_log(conn: &Connection, app: &AppDataDir, run_id: i64, label: &str) -> std::io::Result<String> {
    let entries: Vec<LogEntry> = (|| -> rusqlite::Result<Vec<LogEntry>> {
        let mut stmt = conn.prepare("SELECT seq, op, src, dst, hash, status, error FROM operation WHERE run_id = ?1 ORDER BY seq")?;
        stmt.query_map([run_id], |r| {
            Ok(LogEntry { seq: r.get(0)?, op: r.get(1)?, src: r.get(2)?, dst: r.get(3)?, hash: r.get(4)?, status: r.get(5)?, error: r.get(6)? })
        })?
        .collect()
    })()
    .map_err(std::io::Error::other)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let base = format!("logs/lauf-{run_id}-{label}-{stamp}");
    let json = serde_json::to_vec_pretty(&entries).map_err(std::io::Error::other)?;
    let json_path = app.write_file(format!("{base}.json"), &json)?;
    let csv_field = |s: &Option<String>| format!("\"{}\"", s.as_deref().unwrap_or("").replace('"', "\"\""));
    let mut csv = String::from("seq;op;src;dst;hash;status;error\n");
    for e in &entries {
        csv.push_str(&format!(
            "{};{};{};{};{};{};{}\n",
            e.seq,
            e.op,
            csv_field(&e.src),
            csv_field(&e.dst),
            csv_field(&e.hash),
            e.status,
            csv_field(&e.error)
        ));
    }
    app.write_file(format!("{base}.csv"), csv.as_bytes())?;
    let path = json_path.display().to_string();
    conn.execute("UPDATE run SET log_path = ?2 WHERE id = ?1", params![run_id, path]).map_err(std::io::Error::other)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recover_marks_running_as_aborted() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("INSERT INTO run (kind, status) VALUES ('reorganize', 'running'), ('reorganize', 'done')", []).unwrap();
        assert_eq!(recover_interrupted(&conn).unwrap(), vec![1]);
        let s: String = conn.query_row("SELECT status FROM run WHERE id = 1", [], |r| r.get(0)).unwrap();
        assert_eq!(s, "aborted");
    }
}
