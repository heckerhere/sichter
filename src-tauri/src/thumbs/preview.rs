//! Große Vorschauen für die Vollbildansicht.
//!
//! Erzeugt bei der ersten Anfrage oder vorab im Hintergrund (`prepare`). Jede
//! Vorschau wird nur einmal gleichzeitig erzeugt; parallele Anfragen auf
//! denselben Hash warten auf das Ergebnis.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use rayon::prelude::*;
use rusqlite::{Connection, OptionalExtension};

use super::{ThumbSource, Thumbnailer, preview_rel_path, raw_preview_largest};
use crate::fsops::appdata::AppDataDir;
use crate::meta::exiftool::ExifTool;
use crate::scan::kind::Kind;
use crate::tools::Tools;

/// Quelle einer Vorschau, aus dem Index ermittelt.
#[derive(Debug, Clone)]
pub struct PreviewSource {
    pub path: PathBuf,
    pub kind: Kind,
    pub orientation: Option<i64>,
    pub duration_s: Option<f64>,
}

/// Eine Datei mit diesem Inhalt (gleicher Hash = gleicher Inhalt). Welches
/// Mitglied einer RAW+JPEG-Gruppe die Vorschau liefert, entscheidet der Aufrufer.
pub fn preview_source(conn: &Connection, hash: &str) -> rusqlite::Result<Option<PreviewSource>> {
    conn.query_row(
        "SELECT r.path, a.rel_path, a.kind, m.orientation, v.duration_s
         FROM asset a JOIN library_root r ON r.id = a.root_id
         LEFT JOIN meta m ON m.asset_id = a.id LEFT JOIN video_meta v ON v.asset_id = a.id
         WHERE a.hash = ?1 AND a.kind NOT IN ('sidecar', 'other') LIMIT 1",
        [hash],
        |r| {
            Ok(PreviewSource {
                path: crate::scan::crawl::from_rel_path(Path::new(&r.get::<_, String>(0)?), &r.get::<_, String>(1)?),
                kind: Kind::parse(&r.get::<_, String>(2)?),
                orientation: r.get(3)?,
                duration_s: r.get(4)?,
            })
        },
    )
    .optional()
}

pub fn valid_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

pub struct PreviewService {
    app: AppDataDir,
    tools: Tools,
    exiftools: Mutex<Vec<ExifTool>>,
    in_flight: Mutex<HashSet<String>>,
    done: Condvar,
    /// Erhöht sich bei jedem `prepare`; ältere Vorbereitungen brechen ab.
    epoch: AtomicU64,
}

impl PreviewService {
    pub fn new(app: AppDataDir, tools: Tools) -> Arc<Self> {
        Arc::new(Self {
            app,
            tools,
            exiftools: Mutex::new(Vec::new()),
            in_flight: Mutex::new(HashSet::new()),
            done: Condvar::new(),
            epoch: AtomicU64::new(0),
        })
    }

    pub fn cached(&self, hash: &str) -> Option<PathBuf> {
        if !valid_hash(hash) {
            return None;
        }
        self.app.join(preview_rel_path(hash)).ok().filter(|p| p.is_file())
    }

    /// Pfad der Vorschau; erzeugt sie bei Bedarf. `Ok(None)`: Datei gibt kein Bild her.
    pub fn ensure(&self, hash: &str, source: impl FnOnce() -> Option<PreviewSource>) -> Result<Option<PathBuf>, String> {
        if !valid_hash(hash) {
            return Err("ungültiger Hash".into());
        }
        if let Some(p) = self.cached(hash) {
            return Ok(Some(p));
        }
        // Läuft schon eine Erzeugung für diesen Hash, darauf warten.
        {
            let mut busy = self.in_flight.lock().map_err(|e| e.to_string())?;
            while busy.contains(hash) {
                busy = self.done.wait(busy).map_err(|e| e.to_string())?;
            }
            if let Some(p) = self.cached(hash) {
                return Ok(Some(p));
            }
            busy.insert(hash.to_string());
        }
        let result = self.generate(hash, source);
        if let Ok(mut busy) = self.in_flight.lock() {
            busy.remove(hash);
        }
        self.done.notify_all();
        result
    }

    fn generate(&self, hash: &str, source: impl FnOnce() -> Option<PreviewSource>) -> Result<Option<PathBuf>, String> {
        let Some(src) = source() else { return Ok(None) };
        let thumbnailer = Thumbnailer { ffmpeg: self.tools.ffmpeg.as_ref() };
        let mut raw = |path: &Path| {
            let mut et = self.exiftools.lock().ok()?.pop().or_else(|| self.tools.exiftool.as_ref().and_then(|c| ExifTool::spawn(c).ok()))?;
            let bytes = raw_preview_largest(&mut et, path);
            if let Ok(mut pool) = self.exiftools.lock() {
                pool.push(et);
            }
            bytes
        };
        let thumb_src = ThumbSource { path: &src.path, kind: src.kind, orientation: src.orientation, duration_s: src.duration_s };
        match thumbnailer.render_preview(&thumb_src, &mut raw)? {
            Some(jpeg) => self.app.write_file(preview_rel_path(hash), &jpeg).map(Some).map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    /// Erzeugt fehlende Vorschauen im Hintergrund, in der angegebenen
    /// Reihenfolge (aktuelles Bild und Nachbarn zuerst). Eine neue Vorbereitung
    /// beendet die vorige.
    pub fn prepare(self: &Arc<Self>, items: Vec<(String, PreviewSource)>) {
        let epoch = self.epoch.fetch_add(1, Ordering::SeqCst) + 1;
        let this = Arc::clone(self);
        std::thread::spawn(move || {
            // Wenige Threads: Die UI soll beim Durchklicken nicht ausgebremst werden.
            let Ok(pool) = rayon::ThreadPoolBuilder::new().num_threads(3).build() else { return };
            pool.install(|| {
                items.into_par_iter().for_each(|(hash, src)| {
                    if this.epoch.load(Ordering::SeqCst) != epoch {
                        return;
                    }
                    let _ = this.ensure(&hash, || Some(src));
                });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library").join(rel)
    }

    #[test]
    fn generates_once_and_serves_from_cache() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppDataDir::init(dir.path().join("app")).unwrap();
        let service = PreviewService::new(app, Tools::resolve(None));
        let hash = "a".repeat(64);
        let src = PreviewSource { path: fixture("2023-05-12 Geburtstag/IMG_0001.JPG"), kind: Kind::Jpeg, orientation: None, duration_s: None };

        // Viele gleichzeitige Anfragen: genau eine Erzeugung.
        let calls = AtomicU64::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    let p = service
                        .ensure(&hash, || {
                            calls.fetch_add(1, Ordering::SeqCst);
                            Some(src.clone())
                        })
                        .unwrap()
                        .unwrap();
                    assert!(p.is_file());
                });
            }
        });
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let img = image::open(service.cached(&hash).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (640, 480), "kleine Bilder werden nicht hochskaliert");
    }

    #[test]
    fn rejects_invalid_hashes_and_missing_sources() {
        let dir = tempfile::tempdir().unwrap();
        let service = PreviewService::new(AppDataDir::init(dir.path().join("app")).unwrap(), Tools::resolve(None));
        assert!(service.ensure("../../etc/passwd", || None).is_err());
        assert_eq!(service.ensure(&"b".repeat(64), || None).unwrap(), None);
    }

    #[test]
    fn raw_preview_uses_largest_embedded_jpeg() {
        let tools = Tools::resolve(None);
        let mut et = ExifTool::spawn(tools.exiftool.as_ref().unwrap()).unwrap();
        // RAF-Stub: PreviewImage 12167 Byte, ThumbnailImage 9362 Byte.
        let bytes = raw_preview_largest(&mut et, &fixture("raw/DSCF0001.RAF")).unwrap();
        assert_eq!(bytes.len(), 12167);
    }
}
