//! Zielpfad-Vorlagen, Operationsplan-Erzeugung, Konfliktauflösung.
//!
//! Der Planer liest nur (Index und Dateisystem) und schreibt den Plan als
//! `run` + `operation`-Zeilen in die Datenbank. Ausgeführt wird ausschließlich
//! vom Journal (`journal/`), nach Vorschau und Bestätigung.
//!
//! Regeln (Sicherheitsmodell, Abschnitt 3):
//! * Immer ganze Gruppen (RAW+JPEG+Sidecars, Live Photos), nie `other`-Dateien.
//! * Gesperrte Roots sind weder Quelle noch Ziel.
//! * Ziel belegt mit gleichem Inhalt: nicht bewegen, als Duplikat melden.
//!   Mit anderem Inhalt: Suffix ` (n)` für alle Mitglieder der Gruppe gleich.
//! * Papierkorb nur für ausdrücklich freigegebene Gruppen; bei Duplikaten mit
//!   Pfad der Kopie, die bleibt.

pub mod space;
pub mod template;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{Datelike, NaiveDate};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;

use crate::scan::crawl::from_rel_path;

#[derive(Debug, thiserror::Error)]
pub enum PlanError {
    #[error("{0}")]
    Invalid(String),
    #[error("Datenbankfehler: {0}")]
    Db(#[from] rusqlite::Error),
}

pub type Result<T> = std::result::Result<T, PlanError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunKind {
    Reorganize,
    RejectsToFolder,
    RejectsToTrash,
    DupesToFolder,
    DupesToTrash,
    EmptyDirsToTrash,
    XmpExport,
}

impl RunKind {
    pub fn as_str(self) -> &'static str {
        match self {
            RunKind::Reorganize => "reorganize",
            RunKind::RejectsToFolder => "rejects_to_folder",
            RunKind::RejectsToTrash => "rejects_to_trash",
            RunKind::DupesToFolder => "dupes_to_folder",
            RunKind::DupesToTrash => "dupes_to_trash",
            RunKind::EmptyDirsToTrash => "empty_dirs_to_trash",
            RunKind::XmpExport => "xmp_export",
        }
    }

    pub fn uses_trash(self) -> bool {
        self.as_str().ends_with("_to_trash")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Move,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    Mkdir,
    Move,
    Copy,
    Trash,
    /// XMP-Sidecar mit Bewertung schreiben bzw. mergen.
    Xmp,
}

impl Op {
    pub fn as_str(self) -> &'static str {
        match self {
            Op::Mkdir => "mkdir",
            Op::Move => "move",
            Op::Copy => "copy",
            Op::Trash => "trash",
            Op::Xmp => "xmp",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PlannedOp {
    pub op: Op,
    pub src: Option<PathBuf>,
    pub dst: Option<PathBuf>,
    pub hash: Option<String>,
    pub size: i64,
    pub asset_id: Option<i64>,
    pub group_id: Option<i64>,
    pub keep_path: Option<PathBuf>,
    /// Bei XMP: die zu schreibende Bewertung.
    pub rating: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    /// Ziel existiert mit gleichem Inhalt: Quelle bleibt liegen.
    DuplicateAtTarget,
    /// Ziel existiert mit anderem Inhalt: mit Suffix umbenannt.
    Renamed,
    /// Noch ohne Prüfsumme (Scan nicht abgeschlossen): übersprungen.
    NotHashed,
    /// Teil einer Gruppe, deren übrige Dateien nicht mit entfernt würden.
    GroupPartial,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub kind: ConflictKind,
    pub path: String,
    pub target: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Summary {
    pub files: usize,
    pub bytes: i64,
    pub mkdirs: usize,
    pub groups: usize,
    pub in_place: usize,
    pub skipped_unconfirmed: usize,
    pub conflicts: Vec<Conflict>,
    /// Bytes, die auf ein anderes Volume kopiert werden müssen.
    pub bytes_to_copy: i64,
    pub free_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReorganizeParams {
    pub source_root_ids: Vec<i64>,
    pub target_root_id: i64,
    pub template: String,
    pub video_template: Option<String>,
    pub mode: Mode,
    pub date_range: bool,
    pub only_confirmed: bool,
}

// ---------------------------------------------------------------------------
// Eingaben aus dem Index
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct Member {
    asset_id: i64,
    rel_path: String,
    hash: Option<String>,
    size: i64,
    kind: String,
}

impl Member {
    fn name(&self) -> &str {
        self.rel_path.rsplit('/').next().unwrap_or(&self.rel_path)
    }
}

#[derive(Debug, Clone)]
struct Group {
    id: i64,
    root_id: i64,
    root: PathBuf,
    /// Primäres Mitglied zuerst.
    members: Vec<Member>,
}

impl Group {
    fn primary(&self) -> &Member {
        &self.members[0]
    }

    fn dir(&self) -> &str {
        self.primary().rel_path.rsplit_once('/').map_or("", |(d, _)| d)
    }
}

fn locked_roots(conn: &Connection) -> rusqlite::Result<HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT id FROM library_root WHERE locked = 1")?;
    stmt.query_map([], |r| r.get(0))?.collect()
}

/// Alle Gruppen in nicht gesperrten Roots. Nur Gruppenmitglieder, also nie
/// `other`-Dateien oder verwaiste Sidecars.
fn load_groups(conn: &Connection) -> rusqlite::Result<Vec<Group>> {
    let locked = locked_roots(conn)?;
    let mut stmt = conn.prepare(
        "SELECT g.id, a.id, a.root_id, r.path, a.rel_path, a.hash, a.size, a.kind, (a.id = g.primary_asset_id)
         FROM asset a JOIN asset_group g ON g.id = a.group_id JOIN library_root r ON r.id = a.root_id
         WHERE a.kind != 'other'
         ORDER BY g.id, (a.id = g.primary_asset_id) DESC, a.rel_path",
    )?;
    let mut groups: Vec<Group> = Vec::new();
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            Member { asset_id: r.get(1)?, rel_path: r.get(4)?, hash: r.get(5)?, size: r.get(6)?, kind: r.get(7)? },
            r.get::<_, i64>(2)?,
            r.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (gid, member, root_id, root) = row?;
        if locked.contains(&root_id) {
            continue;
        }
        match groups.last_mut() {
            Some(g) if g.id == gid => g.members.push(member),
            _ => groups.push(Group { id: gid, root_id, root: PathBuf::from(root), members: vec![member] }),
        }
    }
    Ok(groups)
}

fn root_info(conn: &Connection, id: i64) -> Result<(PathBuf, bool)> {
    conn.query_row("SELECT path, locked FROM library_root WHERE id = ?1", [id], |r| Ok((PathBuf::from(r.get::<_, String>(0)?), r.get(1)?)))
        .map_err(|_| PlanError::Invalid("Zielordner ist nicht Teil der Bibliothek.".into()))
}

// ---------------------------------------------------------------------------
// Kern: Gruppen an Zielordner verschieben/kopieren
// ---------------------------------------------------------------------------

/// Vergleichsschlüssel für Pfade: Unicode-NFC, auf macOS/Windows ohne
/// Groß-/Kleinschreibung (wie die Standard-Dateisysteme dort).
fn key(p: &Path) -> String {
    let s: String = p.to_string_lossy().nfc().collect();
    if cfg!(any(target_os = "macos", windows)) { s.to_lowercase() } else { s }
}

struct Target<'a> {
    group: &'a Group,
    dest_root: &'a Path,
    dest_dir: PathBuf,
    /// Zielname je Mitglied (gleiche Reihenfolge wie `group.members`).
    names: Vec<String>,
}

#[derive(Default)]
struct Builder {
    ops: Vec<PlannedOp>,
    summary: Summary,
    planned_dirs: HashSet<String>,
    /// Geplante Zieldateien mit ihrem Inhalt.
    planned_files: HashMap<String, String>,
}

enum Slot {
    Free,
    Identical,
    Taken,
}

impl Builder {
    fn slot(&self, path: &Path, hash: &str) -> Slot {
        // Schon für eine andere Datei geplant: gleicher Inhalt ist ein Duplikat
        // (Quelle bleibt liegen), anderer Inhalt ein Konflikt.
        if let Some(planned) = self.planned_files.get(&key(path)) {
            return if planned == hash { Slot::Identical } else { Slot::Taken };
        }
        if std::fs::symlink_metadata(path).is_err() {
            return Slot::Free;
        }
        match crate::hash::hash_file(path) {
            Ok(h) if h.to_hex() == hash => Slot::Identical,
            _ => Slot::Taken,
        }
    }

    fn ensure_dirs(&mut self, dest_root: &Path, dir: &Path) {
        let mut missing = Vec::new();
        let mut current = dir;
        while current.starts_with(dest_root) && current != dest_root {
            if !current.is_dir() && !self.planned_dirs.contains(&key(current)) {
                missing.push(current.to_path_buf());
            }
            match current.parent() {
                Some(p) => current = p,
                None => break,
            }
        }
        for d in missing.into_iter().rev() {
            self.planned_dirs.insert(key(&d));
            self.summary.mkdirs += 1;
            self.ops.push(PlannedOp { op: Op::Mkdir, src: None, dst: Some(d), hash: None, size: 0, asset_id: None, group_id: None, keep_path: None, rating: None });
        }
    }

    fn place(&mut self, t: Target, op: Op) {
        let g = t.group;
        if g.members.iter().any(|m| m.hash.is_none()) {
            self.summary.conflicts.push(Conflict { kind: ConflictKind::NotHashed, path: g.root.join(&g.primary().rel_path).display().to_string(), target: None });
            return;
        }
        let srcs: Vec<PathBuf> = g.members.iter().map(|m| from_rel_path(&g.root, &m.rel_path)).collect();
        let in_place: Vec<bool> = srcs.iter().zip(&t.names).map(|(s, n)| key(s) == key(&t.dest_dir.join(n))).collect();
        if in_place.iter().all(|x| *x) {
            self.summary.in_place += 1;
            return;
        }

        // Konflikte für die Mitglieder, die tatsächlich bewegt werden.
        let slots: Vec<Option<Slot>> = g
            .members
            .iter()
            .zip(&t.names)
            .zip(&in_place)
            .map(|((m, n), here)| (!here).then(|| self.slot(&t.dest_dir.join(n), m.hash.as_deref().unwrap_or_default())))
            .collect();
        let moving: Vec<&Slot> = slots.iter().flatten().collect();
        if moving.iter().all(|s| matches!(s, Slot::Identical)) {
            for ((m, n), s) in g.members.iter().zip(&t.names).zip(&slots) {
                if s.is_some() {
                    self.summary.conflicts.push(Conflict {
                        kind: ConflictKind::DuplicateAtTarget,
                        path: g.root.join(&m.rel_path).display().to_string(),
                        target: Some(t.dest_dir.join(n).display().to_string()),
                    });
                }
            }
            return;
        }
        let mut names = t.names.clone();
        if moving.iter().any(|s| !matches!(s, Slot::Free)) {
            // Gemeinsamer Suffix für alle Mitglieder, damit Paare erkennbar bleiben.
            let primary = &t.names[0];
            let base = primary.rfind('.').filter(|i| *i > 0).map_or(primary.as_str(), |i| &primary[..i]).to_string();
            let mut n = 2;
            loop {
                let candidate: Vec<String> = t.names.iter().map(|name| template::with_suffix(name, &base, n)).collect();
                let free = candidate.iter().zip(&in_place).all(|(c, here)| {
                    *here || (!self.planned_files.contains_key(&key(&t.dest_dir.join(c))) && std::fs::symlink_metadata(t.dest_dir.join(c)).is_err())
                });
                if free {
                    names = candidate;
                    break;
                }
                n += 1;
            }
            for ((m, name), here) in g.members.iter().zip(&names).zip(&in_place) {
                if !here {
                    self.summary.conflicts.push(Conflict {
                        kind: ConflictKind::Renamed,
                        path: g.root.join(&m.rel_path).display().to_string(),
                        target: Some(t.dest_dir.join(name).display().to_string()),
                    });
                }
            }
        }

        self.ensure_dirs(t.dest_root, &t.dest_dir);
        let cross_volume = space::same_volume(&g.root, t.dest_root) == Some(false);
        for (((m, src), name), here) in g.members.iter().zip(&srcs).zip(&names).zip(&in_place) {
            if *here {
                continue;
            }
            let dst = t.dest_dir.join(name);
            self.planned_files.insert(key(&dst), m.hash.clone().unwrap_or_default());
            self.summary.files += 1;
            self.summary.bytes += m.size;
            if op == Op::Copy || cross_volume {
                self.summary.bytes_to_copy += m.size;
            }
            self.ops.push(PlannedOp {
                op,
                src: Some(src.clone()),
                dst: Some(dst),
                hash: m.hash.clone(),
                size: m.size,
                asset_id: Some(m.asset_id),
                group_id: Some(g.id),
                keep_path: None,
                rating: None,
            });
        }
        self.summary.groups += 1;
    }

    fn trash(&mut self, g: &Group, keep: Option<&HashMap<i64, PathBuf>>) {
        for m in &g.members {
            self.summary.files += 1;
            self.summary.bytes += m.size;
            self.ops.push(PlannedOp {
                op: Op::Trash,
                src: Some(from_rel_path(&g.root, &m.rel_path)),
                dst: None,
                hash: m.hash.clone(),
                size: m.size,
                asset_id: Some(m.asset_id),
                group_id: Some(g.id),
                keep_path: keep.and_then(|k| k.get(&m.asset_id).cloned()),
                rating: None,
            });
        }
        self.summary.groups += 1;
    }
}

// ---------------------------------------------------------------------------
// Planarten
// ---------------------------------------------------------------------------

/// Umsortieren nach Vorlage (PLAN.md 6.5).
pub fn plan_reorganize(conn: &Connection, p: &ReorganizeParams) -> Result<(Vec<PlannedOp>, Summary)> {
    template::validate(&p.template).map_err(PlanError::Invalid)?;
    if let Some(v) = p.video_template.as_deref().filter(|v| !v.trim().is_empty()) {
        template::validate(v).map_err(|e| PlanError::Invalid(format!("Video-Vorlage: {e}")))?;
    }
    let (dest_root, locked) = root_info(conn, p.target_root_id)?;
    if locked {
        return Err(PlanError::Invalid("Der Zielordner ist gesperrt.".into()));
    }
    if !dest_root.is_dir() {
        return Err(PlanError::Invalid("Der Zielordner ist nicht erreichbar.".into()));
    }
    let contexts = crate::events::group_contexts(conn)?;
    let places = if p.template.contains("{Ort}") || p.video_template.as_deref().is_some_and(|v| v.contains("{Ort}")) {
        crate::events::event_places(conn)?
    } else {
        HashMap::new()
    };
    let events: HashMap<i64, (String, Option<NaiveDate>, Option<NaiveDate>, bool)> = {
        let mut stmt = conn.prepare("SELECT id, CASE WHEN title != '' THEN title ELSE suggested_title END, date_start, date_end, confirmed FROM event")?;
        let day = |s: Option<String>| s.and_then(|s| NaiveDate::parse_from_str(&s[..10.min(s.len())], "%Y-%m-%d").ok());
        stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, (r.get(1)?, day(r.get(2)?), day(r.get(3)?), r.get(4)?))))?
            .collect::<rusqlite::Result<_>>()?
    };

    let groups: Vec<Group> = load_groups(conn)?.into_iter().filter(|g| p.source_root_ids.contains(&g.root_id)).collect();
    let mut b = Builder::default();
    let op = match p.mode {
        Mode::Move => Op::Move,
        Mode::Copy => Op::Copy,
    };
    for g in &groups {
        let ctx = contexts.get(&g.id);
        let event = ctx.and_then(|c| c.event_id).and_then(|id| events.get(&id));
        if p.only_confirmed && !event.is_some_and(|e| e.3) {
            b.summary.skipped_unconfirmed += 1;
            continue;
        }
        let time = ctx.and_then(|c| c.time);
        let camera = ctx.map(|c| c.camera.as_str()).unwrap_or("");
        let (dest_dir, names) = match (event, time) {
            (Some((title, Some(start), end, _)), _) => {
                let tpl = match p.video_template.as_deref().filter(|v| !v.trim().is_empty()) {
                    Some(v) if g.primary().kind == "video" => v,
                    _ => p.template.as_str(),
                };
                let render = |name: &str| {
                    template::render(
                        tpl,
                        &template::Context {
                            start: *start,
                            end: end.unwrap_or(*start),
                            occasion: title,
                            place: ctx.and_then(|c| c.event_id).and_then(|id| places.get(&id)).map_or("", String::as_str),
                            camera,
                            original_name: name,
                            date_range: p.date_range,
                        },
                    )
                };
                let (dirs, _) = render(g.primary().name());
                let names = g.members.iter().map(|m| render(m.name()).1).collect();
                (dirs.iter().fold(dest_root.clone(), |acc, d| acc.join(d)), names)
            }
            // Ohne Event: {Jahr}/_Unsortiert, Dateinamen unverändert.
            (_, Some(t)) => (dest_root.join(t.year().to_string()).join("_Unsortiert"), g.members.iter().map(|m| template::sanitize_file_name(m.name())).collect()),
            (_, None) => (dest_root.join("_Ohne Datum"), g.members.iter().map(|m| template::sanitize_file_name(m.name())).collect()),
        };
        b.place(Target { group: g, dest_root: &dest_root, dest_dir, names }, op);
    }
    b.summary.free_bytes = space::free_bytes(&dest_root);
    Ok((b.ops, b.summary))
}

/// Ausschuss in `_Aussortiert` verschieben (Standard für Ausschuss).
pub fn plan_rejects_to_folder(conn: &Connection) -> Result<(Vec<PlannedOp>, Summary)> {
    let rejected = rejected_groups(conn)?;
    let mut b = Builder::default();
    for g in load_groups(conn)?.iter().filter(|g| rejected.contains(&g.id)) {
        if g.dir().split('/').next() == Some("_Aussortiert") {
            b.summary.in_place += 1;
            continue;
        }
        let dest_dir = from_rel_path(&g.root.join("_Aussortiert"), g.dir());
        let names = g.members.iter().map(|m| m.name().to_string()).collect();
        let root = g.root.clone();
        b.place(Target { group: g, dest_root: &root, dest_dir, names }, Op::Move);
    }
    Ok((b.ops, b.summary))
}

fn rejected_groups(conn: &Connection) -> rusqlite::Result<HashSet<i64>> {
    let mut stmt = conn.prepare("SELECT group_id FROM rating WHERE flag = 'reject'")?;
    stmt.query_map([], |r| r.get(0))?.collect()
}

/// Ausschuss in den Papierkorb: nur die einzeln freigegebenen Gruppen, die
/// noch als Ausschuss markiert sind.
pub fn plan_rejects_to_trash(conn: &Connection, approved_groups: &[i64]) -> Result<(Vec<PlannedOp>, Summary)> {
    let rejected = rejected_groups(conn)?;
    let approved: HashSet<i64> = approved_groups.iter().copied().filter(|g| rejected.contains(g)).collect();
    if approved.is_empty() {
        return Err(PlanError::Invalid("Keine freigegebenen Ausschuss-Aufnahmen.".into()));
    }
    let mut b = Builder::default();
    for g in load_groups(conn)?.iter().filter(|g| approved.contains(&g.id)) {
        b.trash(g, None);
    }
    Ok((b.ops, b.summary))
}

/// Redundante Kopien doppelter Dateien. Entfernt wird eine Datei nur, wenn
/// alle Mitglieder ihrer Gruppe ebenfalls redundante Kopien sind, sonst würde
/// ein RAW+JPEG-Paar zerrissen.
pub fn plan_dupes(conn: &Connection, approved_hashes: &[String], to_trash: bool) -> Result<(Vec<PlannedOp>, Summary)> {
    let analysis = crate::dupes::analyze_db(conn)?;
    let approved: HashSet<&str> = approved_hashes.iter().map(String::as_str).collect();
    let roots: HashMap<i64, PathBuf> = {
        let mut stmt = conn.prepare("SELECT id, path FROM library_root")?;
        stmt.query_map([], |r| Ok((r.get(0)?, PathBuf::from(r.get::<_, String>(1)?))))?.collect::<rusqlite::Result<_>>()?
    };
    // Zu entfernende Assets und die jeweils bleibende Kopie.
    let mut keep_of: HashMap<i64, PathBuf> = HashMap::new();
    for g in analysis.file_groups.iter().filter(|g| approved.contains(g.hash.as_str())) {
        let keep_id = g.keep_asset_id.unwrap_or(g.files[0].asset_id);
        let Some(keep) = g.files.iter().find(|f| f.asset_id == keep_id) else { continue };
        let keep_path = from_rel_path(&roots[&keep.root_id], &keep.rel_path);
        for f in g.files.iter().filter(|f| f.asset_id != keep_id) {
            keep_of.insert(f.asset_id, keep_path.clone());
        }
    }
    let mut b = Builder::default();
    let groups = load_groups(conn)?;
    for g in &groups {
        let removable = g.members.iter().filter(|m| keep_of.contains_key(&m.asset_id)).count();
        if removable == 0 {
            continue;
        }
        if removable < g.members.len() {
            for m in g.members.iter().filter(|m| keep_of.contains_key(&m.asset_id)) {
                b.summary.conflicts.push(Conflict { kind: ConflictKind::GroupPartial, path: g.root.join(&m.rel_path).display().to_string(), target: None });
            }
            continue;
        }
        if to_trash {
            b.trash(g, Some(&keep_of));
        } else {
            let dest_dir = from_rel_path(&g.root.join("_Duplikate"), g.dir());
            let names = g.members.iter().map(|m| m.name().to_string()).collect();
            let root = g.root.clone();
            b.place(Target { group: g, dest_root: &root, dest_dir, names }, Op::Move);
        }
    }
    Ok((b.ops, b.summary))
}

/// XMP-Export der Bewertungen (PLAN.md 6.3, standardmäßig aus). Für jede
/// bewertete Gruppe mit RAW: ein bestehendes Sidecar der Gruppe wird gemergt,
/// sonst ein neues `<RAW-Name ohne Endung>.xmp` angelegt. Nie in die Bilddatei.
pub fn plan_xmp_export(conn: &Connection) -> Result<(Vec<PlannedOp>, Summary)> {
    let enabled: bool = conn
        .query_row("SELECT value = '1' FROM setting WHERE key = 'xmp.export'", [], |r| r.get(0))
        .unwrap_or(false);
    if !enabled {
        return Err(PlanError::Invalid("Der XMP-Export ist ausgeschaltet (Einstellungen).".into()));
    }
    let ratings: HashMap<i64, i64> = {
        let mut stmt = conn.prepare("SELECT group_id, CASE WHEN flag = 'reject' THEN -1 ELSE stars END FROM rating")?;
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let mut b = Builder::default();
    for g in load_groups(conn)? {
        let Some(&rating) = ratings.get(&g.id).filter(|r| **r != 0) else { continue };
        let Some(raw) = g.members.iter().find(|m| m.kind == "raw") else { continue };
        let raw_name = raw.name();
        let stem = raw_name.rfind('.').map_or(raw_name, |i| &raw_name[..i]);
        // Vorhandenes Sidecar der Gruppe: "IMG_1.xmp" oder darktable-Stil "IMG_1.CR2.xmp".
        let existing = g.members.iter().find(|m| {
            let n = m.name().to_lowercase();
            m.kind == "sidecar" && (n == format!("{}.xmp", stem.to_lowercase()) || n == format!("{}.xmp", raw_name.to_lowercase()))
        });
        let (path, hash) = match existing {
            Some(x) if x.hash.is_none() => {
                b.summary.conflicts.push(Conflict { kind: ConflictKind::NotHashed, path: g.root.join(&x.rel_path).display().to_string(), target: None });
                continue;
            }
            Some(x) => (from_rel_path(&g.root, &x.rel_path), x.hash.clone()),
            None => {
                let dir = raw.rel_path.rsplit_once('/').map_or("", |(d, _)| d);
                let path = from_rel_path(&g.root, dir).join(format!("{stem}.xmp"));
                if std::fs::symlink_metadata(&path).is_ok() {
                    // Liegt da, ist aber nicht im Index: erst scannen.
                    b.summary.conflicts.push(Conflict { kind: ConflictKind::NotHashed, path: path.display().to_string(), target: None });
                    continue;
                }
                (path, None)
            }
        };
        b.summary.files += 1;
        b.summary.groups += 1;
        b.ops.push(PlannedOp {
            op: Op::Xmp,
            src: None,
            dst: Some(path),
            hash,
            size: 0,
            asset_id: Some(raw.asset_id),
            group_id: Some(g.id),
            keep_path: None,
            rating: Some(rating),
        });
    }
    if b.ops.is_empty() {
        return Err(PlanError::Invalid("Keine bewerteten RAW-Aufnahmen gefunden.".into()));
    }
    Ok((b.ops, b.summary))
}

/// Dateien, die einen Ordner nicht "nicht leer" machen (PLAN.md 3.10).
const IGNORABLE: &[&str] = &[".DS_Store", "Thumbs.db"];

/// Enthält der Ordner (rekursiv) nichts außer `.DS_Store`/`Thumbs.db` und
/// ebensolchen Unterordnern? Symlinks und alles Unlesbare gelten als Inhalt.
pub fn is_effectively_empty(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    for entry in entries {
        let Ok(entry) = entry else { return false };
        let Ok(ft) = entry.file_type() else { return false };
        let name = entry.file_name();
        if ft.is_dir() {
            if !is_effectively_empty(&entry.path()) {
                return false;
            }
        } else if !(ft.is_file() && IGNORABLE.iter().any(|i| name == *i)) {
            return false;
        }
    }
    true
}

/// Leere Ordner in nicht gesperrten Roots, in den Papierkorb (umkehrbar).
/// Gemeldet wird jeweils der oberste leere Ordner eines leeren Teilbaums,
/// nie der Root selbst und nie versteckte Ordner.
pub fn plan_empty_dirs(conn: &Connection) -> Result<(Vec<PlannedOp>, Summary)> {
    let roots: Vec<PathBuf> = {
        let mut stmt = conn.prepare("SELECT path FROM library_root WHERE locked = 0")?;
        stmt.query_map([], |r| Ok(PathBuf::from(r.get::<_, String>(0)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let mut b = Builder::default();
    for root in roots.iter().filter(|r| r.is_dir()) {
        let mut walker = walkdir::WalkDir::new(root).min_depth(1).follow_links(false).into_iter();
        while let Some(Ok(entry)) = walker.next() {
            if !entry.file_type().is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            if name.starts_with('.') || crate::scan::crawl::is_package_dir(&name) {
                walker.skip_current_dir();
                continue;
            }
            if is_effectively_empty(entry.path()) {
                b.summary.files += 1;
                b.ops.push(PlannedOp {
                    op: Op::Trash,
                    src: Some(entry.path().to_path_buf()),
                    dst: None,
                    hash: None,
                    size: 0,
                    asset_id: None,
                    group_id: None,
                    keep_path: None,
                    rating: None,
                });
                // Unterordner gehören dazu, nicht einzeln aufführen.
                walker.skip_current_dir();
            }
        }
    }
    if b.ops.is_empty() {
        return Err(PlanError::Invalid("Keine leeren Ordner gefunden.".into()));
    }
    Ok((b.ops, b.summary))
}

/// Beispiel-Zuordnungen für die Live-Vorschau der Vorlage (ohne Konfliktprüfung).
pub fn template_samples(conn: &Connection, p: &ReorganizeParams, limit: usize) -> Result<Vec<(String, String)>> {
    let (ops, _) = plan_reorganize(conn, &ReorganizeParams { only_confirmed: false, ..p.clone() })?;
    let (dest_root, _) = root_info(conn, p.target_root_id)?;
    let mut seen_dirs = HashSet::new();
    Ok(ops
        .iter()
        .filter(|o| o.op != Op::Mkdir)
        .filter(|o| seen_dirs.insert(o.dst.as_ref().and_then(|d| d.parent()).map(Path::to_path_buf)))
        .take(limit)
        .map(|o| {
            let rel = |p: &Option<PathBuf>| p.as_ref().map(|p| p.strip_prefix(&dest_root).unwrap_or(p).display().to_string()).unwrap_or_default();
            (o.src.as_ref().map(|s| s.display().to_string()).unwrap_or_default(), rel(&o.dst))
        })
        .collect())
}

// ---------------------------------------------------------------------------
// Speichern
// ---------------------------------------------------------------------------

/// Kopfdaten eines neuen Laufs.
#[derive(Clone, Copy)]
pub struct NewRun<'a> {
    pub kind: RunKind,
    pub title: &'a str,
    pub target_root_id: Option<i64>,
    pub dry_run: bool,
}

/// Legt den Plan als Lauf mit Status "geplant" an.
pub fn store(conn: &Connection, run: &NewRun, params: &impl Serialize, ops: &[PlannedOp], summary: &Summary) -> Result<i64> {
    let NewRun { kind, title, target_root_id, dry_run } = *run;
    if ops.iter().all(|o| o.op == Op::Mkdir) {
        return Err(PlanError::Invalid("Der Plan enthält keine Dateioperationen. Alles liegt bereits am Ziel oder wurde übersprungen.".into()));
    }
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO run (kind, status, dry_run, title, target_root_id, params, summary) VALUES (?1, 'planned', ?2, ?3, ?4, ?5, ?6)",
        params![
            kind.as_str(),
            dry_run,
            title,
            target_root_id,
            serde_json::to_string(params).unwrap_or_default(),
            serde_json::to_string(summary).unwrap_or_default()
        ],
    )?;
    let run_id = tx.last_insert_rowid();
    {
        let mut insert = tx.prepare(
            "INSERT INTO operation (run_id, seq, op, src, dst, hash, size, asset_id, group_id, keep_path, note)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )?;
        let path = |p: &Option<PathBuf>| p.as_ref().map(|p| p.to_string_lossy().into_owned());
        for (seq, o) in ops.iter().enumerate() {
            let note = o.rating.map(|r| serde_json::json!({ "rating": r }).to_string());
            insert.execute(params![run_id, seq as i64, o.op.as_str(), path(&o.src), path(&o.dst), o.hash, o.size, o.asset_id, o.group_id, path(&o.keep_path), note])?;
        }
    }
    tx.commit()?;
    Ok(run_id)
}

/// Verwirft einen noch nicht ausgeführten Plan.
pub fn discard(conn: &Connection, run_id: i64) -> Result<()> {
    let n = conn.execute("DELETE FROM run WHERE id = ?1 AND status = 'planned'", [run_id])?;
    if n == 0 {
        return Err(PlanError::Invalid("Nur geplante, noch nicht ausgeführte Läufe können verworfen werden.".into()));
    }
    Ok(())
}

/// Ordnerbaum vorher (Quellen) und nachher (Ziele) mit Dateianzahl je Ordner.
pub fn folder_counts(ops: &[(String, Option<String>, Option<String>)]) -> (BTreeMap<String, usize>, BTreeMap<String, usize>) {
    let mut before = BTreeMap::new();
    let mut after = BTreeMap::new();
    for (op, src, dst) in ops {
        if op == "mkdir" {
            continue;
        }
        let parent = |p: &str| Path::new(p).parent().map(|d| d.display().to_string()).unwrap_or_default();
        if let Some(s) = src {
            *before.entry(parent(s)).or_insert(0) += 1;
        }
        if let Some(d) = dst {
            *after.entry(parent(d)).or_insert(0) += 1;
        }
    }
    (before, after)
}
