//! Ordner-Crawler, Dateierkennung, Paarbildung und Index-Aufbau.
//!
//! Ablauf je Scan: Crawl und Abgleich mit dem Index → Metadaten → Paarbildung
//! → BLAKE3 → Thumbnails. Jeder Schritt verarbeitet nur, was noch fehlt; ein
//! abgebrochener Scan wird beim nächsten Lauf fortgesetzt.
//!
//! Der Scan liest den Quellbestand ausschließlich. Geschrieben wird nur in die
//! App-Datenbank und den Thumbnail-Cache.

pub mod crawl;
pub mod group;
pub mod kind;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

use rayon::prelude::*;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

use crate::fsops::appdata::AppDataDir;
use crate::meta::exiftool::ExifTool;
use crate::meta::{self, ParsedMeta};
use crate::tools::Tools;
use crate::{hash, thumbs, video};
use crawl::{Cancelled, from_rel_path};
use group::GroupInput;
use kind::Kind;

const META_BATCH: usize = 100;
const HASH_BATCH: usize = 256;
/// Parallele Lesezugriffe beim Hashen. Mehr bringt auf Festplatten nichts und
/// erzeugt Kopfbewegungen; auf SSDs ist BLAKE3 ohnehin I/O-gebunden.
const HASH_THREADS: usize = 4;

/// Fortschritt einer Phase. Phasen in Reihenfolge: `crawl`, `detect`, `meta`,
/// `group`, `hash`, `thumbs`, `done`. Der Aufrufer drosselt die Weitergabe.
#[derive(Debug, Clone, Serialize)]
pub struct Progress {
    pub phase: &'static str,
    pub done: u64,
    pub total: u64,
    pub current: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanReport {
    pub files_total: u64,
    pub new: u64,
    pub changed: u64,
    pub removed: u64,
    pub unchanged: u64,
    pub hashed: u64,
    pub thumbs: u64,
    pub thumbs_failed: u64,
    pub events_assigned: u64,
    pub skipped_packages: Vec<String>,
    /// Roots, die nicht erreichbar waren. Ihr Index bleibt unverändert.
    pub unreachable_roots: Vec<String>,
    pub errors: Vec<(String, String)>,
    pub cancelled: bool,
    pub duration_ms: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("{}: {}", crate::i18n::tr("Datenbankfehler", "Database error"), .0)]
    Db(#[from] rusqlite::Error),
    #[error("exiftool: {0}")]
    ExifTool(#[from] crate::meta::exiftool::ExifToolError),
    #[error("{}", crate::i18n::tr("exiftool wurde nicht gefunden", "exiftool was not found"))]
    NoExifTool,
}

pub struct Scanner<'a> {
    pub conn: &'a mut Connection,
    pub tools: &'a Tools,
    pub app: &'a AppDataDir,
    pub cancel: &'a AtomicBool,
    pub progress: &'a (dyn Fn(Progress) + Sync),
}

struct Root {
    id: i64,
    path: PathBuf,
}

/// Ordner (root_id, relativer Ordnerpfad), in dem neu gruppiert werden muss.
type DirKey = (i64, String);
type Step<'a> = fn(&mut Scanner<'a>, &mut ScanReport, &HashSet<DirKey>) -> Result<(), ScanError>;
/// Ergebnis pro Thumbnail: WebP-Bytes, `None` (kein Vorschaubild) oder Fehler.
type ThumbResult = Result<Option<Vec<u8>>, String>;

fn parent_dir(rel_path: &str) -> String {
    rel_path.rsplit_once('/').map_or(String::new(), |(dir, _)| dir.to_string())
}

fn file_name(rel_path: &str) -> &str {
    rel_path.rsplit_once('/').map_or(rel_path, |(_, name)| name)
}

impl Scanner<'_> {
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn report(&self, phase: &'static str, done: u64, total: u64, current: Option<String>) {
        (self.progress)(Progress { phase, done, total, current });
    }

    /// Scannt die angegebenen Roots (alle, wenn `None`).
    pub fn run(&mut self, root_ids: Option<&[i64]>) -> Result<ScanReport, ScanError> {
        let started = Instant::now();
        let mut report = ScanReport::default();
        let roots = self.load_roots(root_ids)?;

        let mut touched: HashSet<DirKey> = HashSet::new();
        for root in &roots {
            if self.cancelled() {
                break;
            }
            self.sync_root(root, &mut report, &mut touched)?;
        }
        // Ordnergrößen hängen nur vom Index ab (Pfad, Größe, Art), deshalb gleich
        // hier. Nicht erreichbare Roots behalten ihre letzten Werte.
        for root in roots.iter().filter(|r| r.path.is_dir()) {
            crate::storage::rebuild(self.conn, root.id)?;
        }

        let steps: [Step; 5] = [Self::read_metadata, Self::regroup, Self::hash_assets, Self::make_thumbnails, Self::perceptual_hashes];
        for step in steps {
            if self.cancelled() {
                break;
            }
            step(self, &mut report, &touched)?;
        }
        // Neue Aufnahmen in Events einsortieren. Bestehende Events und
        // Bearbeitungen bleiben unverändert.
        report.events_assigned = crate::events::assign_new(self.conn)? as u64;

        report.cancelled = self.cancelled();
        report.duration_ms = started.elapsed().as_millis() as u64;
        self.report("done", 1, 1, None);
        Ok(report)
    }

    fn load_roots(&self, root_ids: Option<&[i64]>) -> rusqlite::Result<Vec<Root>> {
        let mut stmt = self.conn.prepare("SELECT id, path FROM library_root ORDER BY id")?;
        let roots = stmt
            .query_map([], |row| Ok(Root { id: row.get(0)?, path: PathBuf::from(row.get::<_, String>(1)?) }))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(roots.into_iter().filter(|r| root_ids.is_none_or(|ids| ids.contains(&r.id))).collect())
    }

    /// Crawl eines Roots und Abgleich mit dem Index.
    fn sync_root(&mut self, root: &Root, report: &mut ScanReport, touched: &mut HashSet<DirKey>) -> Result<(), ScanError> {
        if !root.path.is_dir() {
            // Nicht eingesteckt oder umbenannt: auf keinen Fall als "alles gelöscht" werten.
            report.unreachable_roots.push(root.path.display().to_string());
            return Ok(());
        }
        let label = root.path.display().to_string();
        self.report("crawl", 0, 0, Some(label.clone()));
        let crawled = match crawl::crawl(&root.path, self.cancel, |n| {
            if n.is_multiple_of(50) {
                self.report("crawl", n as u64, 0, Some(label.clone()));
            }
        }) {
            Ok(c) => c,
            Err(Cancelled) => return Ok(()),
        };
        report.files_total += crawled.files.len() as u64;
        report.skipped_packages.extend(crawled.skipped_packages.iter().map(|p| format!("{label}/{p}")));

        // Ordner mit Lesefehlern: dort nichts als gelöscht werten.
        let unreadable: Vec<String> = crawled
            .errors
            .iter()
            .filter_map(|(p, _)| crawl::to_rel_path(&root.path, Path::new(p)))
            .collect();
        report.errors.extend(crawled.errors);

        let mut existing: HashMap<String, (i64, i64, i64)> = HashMap::new();
        {
            let mut stmt = self.conn.prepare("SELECT id, rel_path, size, mtime FROM asset WHERE root_id = ?1")?;
            let rows = stmt.query_map([root.id], |r| Ok((r.get::<_, String>(1)?, (r.get(0)?, r.get(2)?, r.get(3)?))))?;
            for row in rows {
                let (path, value) = row?;
                existing.insert(path, value);
            }
        }

        let mut to_detect = Vec::new();
        let mut seen = HashSet::with_capacity(crawled.files.len());
        for file in &crawled.files {
            seen.insert(file.rel_path.as_str());
            match existing.get(&file.rel_path) {
                Some(&(_, size, mtime)) if size == file.size as i64 && mtime == file.mtime_ns => report.unchanged += 1,
                Some(&(id, _, _)) => to_detect.push((Some(id), file)),
                None => to_detect.push((None, file)),
            }
        }
        let removed: Vec<(i64, &String)> = existing
            .iter()
            .filter(|(path, _)| !seen.contains(path.as_str()))
            .filter(|(path, _)| !unreadable.iter().any(|u| u.is_empty() || path.starts_with(&format!("{u}/")) || *path == u))
            .map(|(path, (id, _, _))| (*id, path))
            .collect();

        // Magic Bytes lesen ist I/O, deshalb parallel vor der Transaktion.
        let (progress, total, checked) = (self.progress, to_detect.len() as u64, AtomicU64::new(0));
        progress(Progress { phase: "detect", done: 0, total, current: None });
        let detected: Vec<_> = to_detect
            .par_iter()
            .map(|(id, file)| {
                let det = kind::detect(&from_rel_path(&root.path, &file.rel_path));
                let done = checked.fetch_add(1, Ordering::Relaxed) + 1;
                progress(Progress { phase: "detect", done, total, current: Some(file_name(&file.rel_path).to_string()) });
                (*id, *file, det)
            })
            .collect();

        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO asset (root_id, rel_path, size, mtime, kind, ext_mismatch) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            let mut update = tx.prepare(
                "UPDATE asset SET size = ?2, mtime = ?3, kind = ?4, ext_mismatch = ?5, hash = NULL, meta_done = 0 WHERE id = ?1",
            )?;
            let mut clear_meta = tx.prepare("DELETE FROM meta WHERE asset_id = ?1")?;
            let mut clear_video = tx.prepare("DELETE FROM video_meta WHERE asset_id = ?1")?;
            for (id, file, det) in &detected {
                let (size, mtime, kind_str, mismatch) = (file.size as i64, file.mtime_ns, det.kind.as_str(), det.ext_mismatch);
                match id {
                    Some(id) => {
                        update.execute(params![id, size, mtime, kind_str, mismatch])?;
                        clear_meta.execute([id])?;
                        clear_video.execute([id])?;
                        report.changed += 1;
                    }
                    None => {
                        insert.execute(params![root.id, file.rel_path, size, mtime, kind_str, mismatch])?;
                        report.new += 1;
                    }
                }
                touched.insert((root.id, parent_dir(&file.rel_path)));
            }
            let mut delete = tx.prepare("DELETE FROM asset WHERE id = ?1")?;
            for (id, path) in &removed {
                delete.execute([id])?;
                touched.insert((root.id, parent_dir(path)));
                report.removed += 1;
            }
        }
        tx.commit()?;
        Ok(())
    }

    fn read_metadata(&mut self, _: &mut ScanReport, _: &HashSet<DirKey>) -> Result<(), ScanError> {
        struct Pending {
            id: i64,
            path: PathBuf,
            kind: Kind,
            mtime: i64,
        }
        let pending: Vec<Pending> = {
            let mut stmt = self.conn.prepare(
                "SELECT a.id, r.path, a.rel_path, a.kind, a.mtime FROM asset a JOIN library_root r ON r.id = a.root_id
                 WHERE a.meta_done = 0 AND a.kind NOT IN ('sidecar', 'other')",
            )?;
            stmt.query_map([], |r| {
                Ok(Pending {
                    id: r.get(0)?,
                    path: from_rel_path(Path::new(&r.get::<_, String>(1)?), &r.get::<_, String>(2)?),
                    kind: Kind::parse(&r.get::<_, String>(3)?),
                    mtime: r.get(4)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        if pending.is_empty() {
            return Ok(());
        }
        let exiftool_cmd = self.tools.exiftool.as_ref().ok_or(ScanError::NoExifTool)?;
        let mut exiftool = ExifTool::spawn(exiftool_cmd)?;
        let total = pending.len() as u64;
        let mut done = 0u64;

        for batch in pending.chunks(META_BATCH) {
            if self.cancelled() {
                return Ok(());
            }
            self.report("meta", done, total, batch[0].path.file_name().map(|n| n.to_string_lossy().into_owned()));

            let mut args: Vec<String> = ["-j", "-n", "-G", "-fast"].map(String::from).to_vec();
            args.extend(meta::SCAN_TAGS.iter().map(|t| t.to_string()));
            let paths: Vec<String> = batch.iter().filter_map(|p| p.path.to_str().map(str::to_string)).collect();
            args.extend(paths.iter().cloned());
            let output = exiftool.execute(&args)?;
            let parsed: Vec<Value> = serde_json::from_str(output.stdout.trim()).unwrap_or_default();
            // SourceFile kommt unter Windows mit '/' zurück.
            let by_source: HashMap<String, &Value> = parsed
                .iter()
                .filter_map(|o| Some((o["SourceFile"].as_str()?.replace('\\', "/"), o)))
                .collect();

            let videos: Vec<(i64, video::VideoMeta)> = match self.tools.ffprobe.as_ref() {
                Some(ffprobe) => batch
                    .par_iter()
                    .filter(|p| p.kind == Kind::Video)
                    .filter_map(|p| video::probe(ffprobe, &p.path).ok().map(|m| (p.id, m)))
                    .collect(),
                None => Vec::new(),
            };
            let videos: HashMap<i64, video::VideoMeta> = videos.into_iter().collect();

            let tx = self.conn.transaction()?;
            for item in batch {
                let key = item.path.to_string_lossy().replace('\\', "/");
                let mut parsed = by_source
                    .get(&key)
                    .map(|o| meta::parse_exiftool_object(o, item.kind == Kind::Video))
                    .unwrap_or_default();
                let name = item.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                meta::apply_date_fallback(&mut parsed, &name, item.mtime);
                let video = videos.get(&item.id);
                if let Some(v) = video {
                    parsed.width = parsed.width.or(v.width);
                    parsed.height = parsed.height.or(v.height);
                }
                write_meta(&tx, item.id, &parsed, video)?;
            }
            tx.commit()?;
            done += batch.len() as u64;
            self.report("meta", done, total, None);
        }
        Ok(())
    }

    fn regroup(&mut self, _: &mut ScanReport, touched: &HashSet<DirKey>) -> Result<(), ScanError> {
        if touched.is_empty() {
            return Ok(());
        }
        self.report("group", 0, touched.len() as u64, None);
        let roots: HashSet<i64> = touched.iter().map(|(r, _)| *r).collect();
        let tx = self.conn.transaction()?;
        for root_id in roots {
            // Einmal alle Assets des Roots laden, nach Ordner sortiert.
            let mut by_dir: HashMap<String, Vec<(GroupInput, Option<i64>)>> = HashMap::new();
            {
                let mut stmt = tx.prepare(
                    "SELECT a.id, a.rel_path, a.kind, a.group_id, m.content_id FROM asset a
                     LEFT JOIN meta m ON m.asset_id = a.id WHERE a.root_id = ?1",
                )?;
                let rows = stmt.query_map([root_id], |r| {
                    let rel: String = r.get(1)?;
                    Ok((
                        parent_dir(&rel),
                        GroupInput {
                            asset_id: r.get(0)?,
                            file_name: file_name(&rel).to_string(),
                            kind: Kind::parse(&r.get::<_, String>(2)?),
                            content_id: r.get(4)?,
                        },
                        r.get::<_, Option<i64>>(3)?,
                    ))
                })?;
                for row in rows {
                    let (dir, input, group) = row?;
                    if touched.contains(&(root_id, dir.clone())) {
                        by_dir.entry(dir).or_default().push((input, group));
                    }
                }
            }
            for files in by_dir.values() {
                apply_groups(&tx, files)?;
            }
        }
        tx.execute(
            "DELETE FROM asset_group WHERE id NOT IN (SELECT group_id FROM asset WHERE group_id IS NOT NULL)",
            [],
        )?;
        tx.commit()?;
        Ok(())
    }

    fn hash_assets(&mut self, report: &mut ScanReport, _: &HashSet<DirKey>) -> Result<(), ScanError> {
        let mut pending: Vec<(i64, PathBuf, i64)> = {
            let mut stmt = self.conn.prepare(
                "SELECT a.id, r.path, a.rel_path, a.size FROM asset a JOIN library_root r ON r.id = a.root_id
                 WHERE a.hash IS NULL AND a.kind != 'other'",
            )?;
            stmt.query_map([], |r| {
                Ok((r.get(0)?, from_rel_path(Path::new(&r.get::<_, String>(1)?), &r.get::<_, String>(2)?), r.get(3)?))
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        if pending.is_empty() {
            return Ok(());
        }
        // Größen-Vorfilter: Dateien mit einer Größe, die mehrfach vorkommt, zuerst.
        // Nur sie können Duplikate sein, die Duplikatsuche hat so früh Daten.
        let colliding: HashSet<i64> = {
            let mut stmt = self.conn.prepare("SELECT size FROM asset WHERE kind != 'other' GROUP BY size HAVING count(*) > 1")?;
            stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        pending.sort_by_key(|(_, _, size)| !colliding.contains(size));

        let pool = rayon::ThreadPoolBuilder::new().num_threads(HASH_THREADS).build().expect("Thread-Pool");
        let (cancel, progress) = (self.cancel, self.progress);
        let total = pending.len() as u64;
        let done = AtomicU64::new(0);
        progress(Progress { phase: "hash", done: 0, total, current: None });
        for batch in pending.chunks(HASH_BATCH) {
            if self.cancelled() {
                return Ok(());
            }
            let results: Vec<(i64, Result<String, String>)> = pool.install(|| {
                batch
                    .par_iter()
                    .map(|(id, path, _)| {
                        if cancel.load(Ordering::Relaxed) {
                            return (*id, Err("abgebrochen".into()));
                        }
                        let r = hash::hash_file(path).map(|h| h.to_hex()).map_err(|e| format!("{}: {e}", path.display()));
                        let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                        let current = path.file_name().map(|f| f.to_string_lossy().into_owned());
                        progress(Progress { phase: "hash", done: n, total, current });
                        (*id, r)
                    })
                    .collect()
            });
            let tx = self.conn.transaction()?;
            {
                let mut set = tx.prepare("UPDATE asset SET hash = ?2 WHERE id = ?1")?;
                for (id, result) in results {
                    match result {
                        Ok(hex) => {
                            set.execute(params![id, hex])?;
                            report.hashed += 1;
                        }
                        Err(e) if e != "abgebrochen" => report.errors.push((id.to_string(), e)),
                        Err(_) => {}
                    }
                }
            }
            tx.commit()?;
        }
        Ok(())
    }

    fn make_thumbnails(&mut self, report: &mut ScanReport, _: &HashSet<DirKey>) -> Result<(), ScanError> {
        // Fehlende Cache-Dateien (vom Nutzer gelöscht) neu erzeugen lassen.
        {
            let ok: Vec<String> = {
                let mut stmt = self.conn.prepare("SELECT hash FROM thumb WHERE status = 'ok'")?;
                stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
            };
            let missing: Vec<&String> =
                ok.iter().filter(|h| self.app.join(thumbs::cache_rel_path(h)).is_ok_and(|p| !p.is_file())).collect();
            for hash in missing {
                self.conn.execute("DELETE FROM thumb WHERE hash = ?1", [hash])?;
            }
        }

        struct Pending {
            hash: String,
            path: PathBuf,
            kind: Kind,
            orientation: Option<i64>,
            duration: Option<f64>,
        }
        let pending: Vec<Pending> = {
            let mut stmt = self.conn.prepare(
                "SELECT a.hash, r.path, a.rel_path, a.kind, m.orientation, v.duration_s
                 FROM asset a JOIN library_root r ON r.id = a.root_id
                 LEFT JOIN meta m ON m.asset_id = a.id LEFT JOIN video_meta v ON v.asset_id = a.id
                 LEFT JOIN thumb t ON t.hash = a.hash
                 WHERE a.hash IS NOT NULL AND a.kind NOT IN ('sidecar', 'other')
                   AND (t.hash IS NULL OR (t.status != 'ok' AND t.version < ?1))
                 GROUP BY a.hash",
            )?;
            stmt.query_map([thumbs::VERSION], |r| {
                Ok(Pending {
                    hash: r.get(0)?,
                    path: from_rel_path(Path::new(&r.get::<_, String>(1)?), &r.get::<_, String>(2)?),
                    kind: Kind::parse(&r.get::<_, String>(3)?),
                    orientation: r.get(4)?,
                    duration: r.get(5)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?
        };
        if pending.is_empty() {
            return Ok(());
        }

        let thumbnailer = thumbs::Thumbnailer { ffmpeg: self.tools.ffmpeg.as_ref() };
        // Kleiner Pool von exiftool-Prozessen für RAW-Previews.
        let pool: Mutex<Vec<ExifTool>> = Mutex::new(Vec::new());
        let exiftool_cmd = self.tools.exiftool.as_ref();
        let (cancel, progress) = (self.cancel, self.progress);
        let total = pending.len() as u64;
        let done = AtomicU64::new(0);
        progress(Progress { phase: "thumbs", done: 0, total, current: None });

        for batch in pending.chunks(128) {
            if self.cancelled() {
                return Ok(());
            }
            let results: Vec<(&str, ThumbResult)> = batch
                .par_iter()
                .map(|p| {
                    if cancel.load(Ordering::Relaxed) {
                        return (p.hash.as_str(), Err("abgebrochen".to_string()));
                    }
                    let src = thumbs::ThumbSource { path: &p.path, kind: p.kind, orientation: p.orientation, duration_s: p.duration };
                    let mut raw = |path: &Path| {
                        let mut et = pool.lock().ok()?.pop().or_else(|| exiftool_cmd.and_then(|c| ExifTool::spawn(c).ok()))?;
                        let bytes = thumbs::raw_preview(&mut et, path);
                        if let Ok(mut pool) = pool.lock() {
                            pool.push(et);
                        }
                        bytes
                    };
                    let result = thumbnailer.render(&src, &mut raw);
                    let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                    let current = p.path.file_name().map(|f| f.to_string_lossy().into_owned());
                    progress(Progress { phase: "thumbs", done: n, total, current });
                    (p.hash.as_str(), result)
                })
                .collect();

            let tx = self.conn.transaction()?;
            {
                let mut set = tx.prepare("INSERT OR REPLACE INTO thumb (hash, status, error, version) VALUES (?1, ?2, ?3, ?4)")?;
                for (hash, result) in results {
                    match result {
                        Ok(Some(webp)) => match thumbs::store(self.app, hash, &webp) {
                            Ok(_) => {
                                set.execute(params![hash, "ok", None::<String>, thumbs::VERSION])?;
                                report.thumbs += 1;
                            }
                            Err(e) => report.errors.push((hash.to_string(), e.to_string())),
                        },
                        Ok(None) => {
                            set.execute(params![hash, "none", None::<String>, thumbs::VERSION])?;
                        }
                        Err(e) if e == "abgebrochen" => {}
                        Err(e) => {
                            set.execute(params![hash, "failed", e, thumbs::VERSION])?;
                            report.thumbs_failed += 1;
                        }
                    }
                }
            }
            tx.commit()?;
        }
        Ok(())
    }
}

impl Scanner<'_> {
    /// dHash für Serien-Erkennung, aus den Thumbnails im Cache (schnell, kein
    /// erneutes Lesen der Originale).
    fn perceptual_hashes(&mut self, _: &mut ScanReport, _: &HashSet<DirKey>) -> Result<(), ScanError> {
        let pending: Vec<String> = {
            let mut stmt = self.conn.prepare(
                "SELECT t.hash FROM thumb t LEFT JOIN phash p ON p.hash = t.hash WHERE t.status = 'ok' AND p.hash IS NULL",
            )?;
            stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        let app = self.app;
        let results: Vec<(String, u64)> = pending
            .par_iter()
            .filter_map(|h| {
                let path = app.join(thumbs::cache_rel_path(h)).ok()?;
                let img = image::open(path).ok()?;
                Some((h.clone(), thumbs::dhash::dhash(&img)))
            })
            .collect();
        let tx = self.conn.transaction()?;
        {
            let mut insert = tx.prepare("INSERT OR REPLACE INTO phash (hash, dhash) VALUES (?1, ?2)")?;
            for (h, d) in results {
                // SQLite kennt nur vorzeichenbehaftete 64 Bit.
                insert.execute(params![h, d as i64])?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

fn write_meta(tx: &rusqlite::Transaction, id: i64, m: &ParsedMeta, video: Option<&video::VideoMeta>) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT OR REPLACE INTO meta (asset_id, taken_at, tz_offset, date_source, camera_make, camera_model, lens,
            gps_lat, gps_lon, width, height, orientation, content_id, quicktime_utc)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            id,
            m.taken_at_string(),
            m.tz_offset,
            m.date_source.map(|d| d.as_str()),
            m.camera_make,
            m.camera_model,
            m.lens,
            m.gps_lat,
            m.gps_lon,
            m.width,
            m.height,
            m.orientation,
            m.content_id,
            m.quicktime_utc,
        ],
    )?;
    if let Some(v) = video {
        tx.execute(
            "INSERT OR REPLACE INTO video_meta (asset_id, duration_s, codec, fps, width, height) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, v.duration_s, v.codec, v.fps, v.width, v.height],
        )?;
    }
    tx.execute("UPDATE asset SET meta_done = 1 WHERE id = ?1", [id])?;
    Ok(())
}

/// Setzt die Gruppen eines Ordners und behält dabei bestehende Gruppen-IDs,
/// damit Bewertungen und Event-Zuordnungen einen Rescan überleben.
fn apply_groups(tx: &rusqlite::Transaction, files: &[(GroupInput, Option<i64>)]) -> rusqlite::Result<()> {
    let inputs: Vec<GroupInput> = files.iter().map(|(i, _)| i.clone()).collect();
    let current: HashMap<i64, Option<i64>> = files.iter().map(|(i, g)| (i.asset_id, *g)).collect();
    let groups = group::group_directory(&inputs);

    let mut claimed: HashSet<i64> = HashSet::new();
    let mut grouped: HashSet<i64> = HashSet::new();
    for members in &groups {
        let primary = members[0];
        // Bevorzugt die Gruppe des primären Assets, sonst die eines Mitglieds.
        let reuse = std::iter::once(primary)
            .chain(members.iter().copied())
            .filter_map(|id| current.get(&id).copied().flatten())
            .find(|g| !claimed.contains(g));
        let group_id = match reuse {
            Some(g) => {
                tx.execute("UPDATE asset_group SET primary_asset_id = ?2 WHERE id = ?1", params![g, primary])?;
                g
            }
            None => {
                tx.execute("INSERT INTO asset_group (primary_asset_id) VALUES (?1)", [primary])?;
                tx.last_insert_rowid()
            }
        };
        claimed.insert(group_id);
        for id in members {
            grouped.insert(*id);
            if current.get(id).copied().flatten() != Some(group_id) {
                tx.execute("UPDATE asset SET group_id = ?2 WHERE id = ?1", params![id, group_id])?;
            }
        }
    }
    for (id, group) in &current {
        if group.is_some() && !grouped.contains(id) {
            tx.execute("UPDATE asset SET group_id = NULL WHERE id = ?1", [id])?;
        }
    }
    Ok(())
}

/// Liest die gespeicherte Gruppen-ID eines Assets (für Tests und Commands).
pub fn group_of(conn: &Connection, root_id: i64, rel_path: &str) -> rusqlite::Result<Option<i64>> {
    conn.query_row("SELECT group_id FROM asset WHERE root_id = ?1 AND rel_path = ?2", params![root_id, rel_path], |r| r.get(0))
        .optional()
        .map(Option::flatten)
}
