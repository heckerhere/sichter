//! Import aus der Fotos-Mediathek: Planung, Ausführung, Wiederaufnahme und
//! erneuter Import, gegen eine Attrappe der Mediathek in temporären Verzeichnissen.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use rusqlite::Connection;
use sichter_lib::db;
use sichter_lib::fsops::appdata::AppDataDir;
use sichter_lib::journal::{self, Exec, JournalError};
use sichter_lib::photos::{Asset, ListOptions, PhotoLibrary, Resource, ResourceKind};
use sichter_lib::plan::{self, RunKind, import::ImportParams};

/// Mittag UTC (14.11.2023): in jeder Zeitzone derselbe Kalendertag.
const NOON: i64 = 1_700_049_600;

struct FakeLibrary {
    assets: Vec<Asset>,
    data: HashMap<(String, String), Vec<u8>>,
    /// Originale, deren Export fehlschlägt (z. B. nur in iCloud).
    failing: Mutex<Vec<String>>,
}

impl FakeLibrary {
    fn new() -> Self {
        let mut lib = Self { assets: Vec::new(), data: HashMap::new(), failing: Mutex::new(Vec::new()) };
        lib.add("A", Some(NOON), &[(ResourceKind::Photo, "IMG_0001.HEIC"), (ResourceKind::PairedVideo, "IMG_0001.MOV")]);
        lib.add("B", Some(NOON + 3600), &[(ResourceKind::Photo, "IMG_0002.JPG"), (ResourceKind::AlternatePhoto, "IMG_0002.DNG")]);
        lib.add("C", Some(NOON + 7200), &[(ResourceKind::Video, "IMG_0003.MOV")]);
        lib
    }

    fn add(&mut self, id: &str, created: Option<i64>, resources: &[(ResourceKind, &str)]) {
        let resources = resources
            .iter()
            .map(|(kind, name)| {
                let bytes = format!("{id}/{name}").into_bytes();
                self.data.insert((id.to_string(), name.to_string()), bytes.clone());
                Resource { kind: *kind, original_name: name.to_string(), size: Some(bytes.len() as i64) }
            })
            .collect();
        self.assets.push(Asset { id: id.into(), created, favorite: false, resources });
    }
}

impl PhotoLibrary for FakeLibrary {
    fn assets(&self, _opts: &ListOptions) -> Result<Vec<Asset>, String> {
        Ok(self.assets.clone())
    }

    fn export(&self, asset_id: &str, _kind: ResourceKind, original_name: &str, to: &Path, _allow_network: bool) -> Result<(), String> {
        if self.failing.lock().unwrap().iter().any(|n| n == original_name) {
            return Err("Original liegt nur in iCloud".into());
        }
        assert!(!to.exists(), "Export darf nur in eine neue Datei schreiben");
        let bytes = self.data.get(&(asset_id.to_string(), original_name.to_string())).ok_or("unbekannt")?;
        std::fs::write(to, bytes).map_err(|e| e.to_string())
    }
}

struct Env {
    _dir: tempfile::TempDir,
    target: std::path::PathBuf,
    app: AppDataDir,
    conn: Connection,
    photos: FakeLibrary,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("Import");
        std::fs::create_dir(&target).unwrap();
        let app = AppDataDir::init(dir.path().join("appdata")).unwrap();
        let conn = db::open(&app.join("sichter.db").unwrap()).unwrap();
        Self { _dir: dir, target, app, conn, photos: FakeLibrary::new() }
    }

    fn params(&self) -> ImportParams {
        ImportParams { target: self.target.display().to_string(), include_hidden: false, download_originals: true }
    }

    fn plan(&self) -> Result<(i64, plan::Summary, usize), plan::PlanError> {
        let assets = self.photos.assets(&ListOptions::default()).unwrap();
        let (ops, summary) = plan::import::plan_photos_import(&self.conn, &assets, &self.params())?;
        let files = ops.iter().filter(|o| o.op != plan::Op::Mkdir).count();
        let meta = plan::NewRun { kind: RunKind::PhotosImport, title: "Import", target_root_id: None, dry_run: false };
        Ok((plan::store(&self.conn, &meta, &self.params(), &ops, &summary)?, summary, files))
    }

    fn exec<T>(&self, crash: Option<usize>, f: impl FnOnce(&Exec) -> T) -> T {
        let (cancel, pause) = (AtomicBool::new(false), AtomicBool::new(false));
        f(&Exec { app: &self.app, cancel: &cancel, pause: &pause, progress: &|_| {}, simulate_crash_after: crash, exiftool: None, photos: Some(&self.photos) })
    }

    fn day_dir(&self) -> std::path::PathBuf {
        use chrono::TimeZone;
        let t = chrono::Local.timestamp_opt(NOON, 0).unwrap();
        self.target.join(t.format("%Y").to_string()).join(t.format("%Y-%m-%d").to_string())
    }

    /// Alle Dateien unterhalb des Ziels (relativ), inklusive versteckter.
    fn files(&self) -> Vec<String> {
        let mut out: Vec<String> = walkdir::WalkDir::new(&self.target)
            .into_iter()
            .map(|e| e.unwrap())
            .filter(|e| e.file_type().is_file())
            .map(|e| e.path().strip_prefix(&self.target).unwrap().display().to_string())
            .collect();
        out.sort();
        out
    }
}

#[test]
fn imports_originals_into_date_folders() {
    let env = Env::new();
    let (run, summary, files) = env.plan().unwrap();
    assert_eq!((summary.files, summary.groups), (5, 3));

    let dry = env.exec(None, |ex| journal::dry_run(&env.conn, run, ex)).unwrap();
    assert_eq!((dry.done, dry.failed), (5, 0));
    assert!(env.files().is_empty(), "Probelauf verändert nichts");

    let report = env.exec(None, |ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!((report.done, report.failed), (5, 0), "{:?}", report.messages);
    let day = env.day_dir();
    for (id, name) in [("A", "IMG_0001.HEIC"), ("A", "IMG_0001.MOV"), ("B", "IMG_0002.JPG"), ("B", "IMG_0002.DNG"), ("C", "IMG_0003.MOV")] {
        let path = day.join(name);
        assert_eq!(std::fs::read(&path).unwrap(), format!("{id}/{name}").into_bytes(), "{name}");
    }
    // Datiert auf den Aufnahmezeitpunkt, damit der Datums-Fallback stimmt.
    let mtime = std::fs::metadata(day.join("IMG_0001.HEIC")).unwrap().modified().unwrap();
    assert_eq!(mtime, std::time::UNIX_EPOCH + std::time::Duration::from_secs(NOON as u64));
    assert_eq!(env.files().len(), 5, "keine Temp-Dateien: {:?}", env.files());
    // Hash steht im Journal (für Undo).
    let hashed: i64 = env.conn.query_row("SELECT count(*) FROM operation WHERE run_id = ?1 AND op = 'export' AND hash IS NOT NULL", [run], |r| r.get(0)).unwrap();
    assert_eq!(hashed, 5);

    // Zweiter Import: nichts doppelt.
    let err = env.plan().unwrap_err();
    assert!(err.to_string().contains("schon importiert"), "{err}");
}

#[test]
fn reimport_only_plans_new_photos_and_renames_on_collision() {
    let mut env = Env::new();
    let (run, _, files) = env.plan().unwrap();
    env.exec(None, |ex| journal::execute(&env.conn, run, files, ex)).unwrap();

    // Neues Foto mit gleichem Originalnamen am selben Tag (z. B. zweite Kamera).
    env.photos.add("D", Some(NOON + 60), &[(ResourceKind::Photo, "IMG_0001.HEIC"), (ResourceKind::PairedVideo, "IMG_0001.MOV")]);
    let (run2, summary, files2) = env.plan().unwrap();
    assert_eq!((summary.files, summary.already_imported), (2, 3));
    env.exec(None, |ex| journal::execute(&env.conn, run2, files2, ex)).unwrap();
    let day = env.day_dir();
    assert_eq!(std::fs::read(day.join("IMG_0001 (2).HEIC")).unwrap(), b"D/IMG_0001.HEIC");
    assert_eq!(std::fs::read(day.join("IMG_0001 (2).MOV")).unwrap(), b"D/IMG_0001.MOV");
    assert_eq!(std::fs::read(day.join("IMG_0001.HEIC")).unwrap(), b"A/IMG_0001.HEIC", "nichts überschrieben");
}

#[test]
fn resumes_after_crash_without_duplicates() {
    let env = Env::new();
    let (run, _, files) = env.plan().unwrap();
    // Absturz nach dem dritten Export, bevor das Journal ihn als erledigt führt.
    let crashed = env.exec(Some(3), |ex| journal::execute(&env.conn, run, files, ex));
    assert!(matches!(crashed, Err(JournalError::SimulatedCrash)));
    journal::recover_interrupted(&env.conn).unwrap();

    let report = env.exec(None, |ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!(report.failed, 0, "{:?}", report.messages);
    assert_eq!(env.files().len(), 5, "{:?}", env.files());
    let done: i64 = env.conn.query_row("SELECT count(*) FROM operation WHERE run_id = ?1 AND op = 'export' AND status = 'done'", [run], |r| r.get(0)).unwrap();
    assert_eq!(done, 5);
}

#[test]
fn failed_export_is_reported_and_leaves_nothing_behind() {
    let env = Env::new();
    env.photos.failing.lock().unwrap().push("IMG_0003.MOV".into());
    let (run, _, files) = env.plan().unwrap();
    let report = env.exec(None, |ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!((report.done, report.failed), (4, 1));
    assert!(report.messages[0].1.contains("iCloud"), "{:?}", report.messages);
    assert_eq!(env.files().len(), 4, "keine halben Dateien: {:?}", env.files());

    // Später erneut: nur das fehlende Video wird geplant.
    env.photos.failing.lock().unwrap().clear();
    let (run2, summary, files2) = env.plan().unwrap();
    assert_eq!(summary.files, 1);
    env.exec(None, |ex| journal::execute(&env.conn, run2, files2, ex)).unwrap();
    assert_eq!(env.files().len(), 5);
}

/// Legt Dateien in den echten Papierkorb. Nur bewusst ausführen:
/// `cargo test --test photos_import real_ -- --ignored`
#[test]
#[ignore]
fn real_group_rollback_and_undo_use_trash() {
    let env = Env::new();
    // RAW der Gruppe B schlägt fehl: Das schon exportierte JPEG wird zurückgenommen.
    env.photos.failing.lock().unwrap().push("IMG_0002.DNG".into());
    let (run, _, files) = env.plan().unwrap();
    let report = env.exec(None, |ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!((report.done, report.failed, report.undone), (3, 1, 1), "{:?}", report.messages);
    assert!(!env.day_dir().join("IMG_0002.JPG").exists());

    let undo = env.exec(None, |ex| journal::undo(&env.conn, run, ex)).unwrap();
    assert_eq!(undo.failed, 0, "{:?}", undo.messages);
    assert!(env.files().is_empty());
}
