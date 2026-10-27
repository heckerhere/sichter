//! Integrationstests für den Scan. Laufen ausschließlich gegen Kopien der
//! Fixtures in temporären Verzeichnissen.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::SystemTime;

use rusqlite::Connection;
use sichter_lib::db::{self, library};
use sichter_lib::fsops::appdata::AppDataDir;
use sichter_lib::scan::{ScanReport, Scanner};
use sichter_lib::tools::Tools;

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let target = dst.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

/// Zustand jeder Datei unterhalb von `root` (inkl. versteckter Dateien).
fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, SystemTime, String)> {
    let mut out = BTreeMap::new();
    for entry in walkdir::WalkDir::new(root) {
        let entry = entry.unwrap();
        if entry.file_type().is_file() {
            let meta = entry.metadata().unwrap();
            let hash = blake3::hash(&std::fs::read(entry.path()).unwrap()).to_hex().to_string();
            out.insert(entry.path().to_path_buf(), (meta.len(), meta.modified().unwrap(), hash));
        }
    }
    out
}

struct Env {
    _dir: tempfile::TempDir,
    library: PathBuf,
    app: AppDataDir,
    conn: Connection,
    tools: Tools,
    root_id: i64,
}

impl Env {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("Fotos");
        copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library"), &library);
        let app = AppDataDir::init(dir.path().join("appdata")).unwrap();
        let conn = db::open(&app.join("test.db").unwrap()).unwrap();
        let root_id = library::add_root(&conn, &library, app.path()).unwrap();
        Env { _dir: dir, library, app, conn, tools: Tools::resolve(None), root_id }
    }

    fn scan(&mut self) -> ScanReport {
        let cancel = AtomicBool::new(false);
        let mut scanner =
            Scanner { conn: &mut self.conn, tools: &self.tools, app: &self.app, cancel: &cancel, progress: &|_| {} };
        let report = scanner.run(None).unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        report
    }

    fn group(&self, rel: &str) -> Option<i64> {
        sichter_lib::scan::group_of(&self.conn, self.root_id, rel).unwrap()
    }

    fn one<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

#[test]
fn scan_does_not_modify_source() {
    let mut env = Env::new();
    let before = snapshot(&env.library);
    env.scan();
    env.scan();
    assert_eq!(before, snapshot(&env.library), "Scan hat den Quellordner verändert");
}

#[test]
fn indexes_fixtures_with_groups_dates_hashes_and_thumbs() {
    let mut env = Env::new();
    let report = env.scan();
    // .DS_Store wird ignoriert, alles andere indexiert.
    assert_eq!(report.files_total, 20);
    assert_eq!(report.new, 20);

    // RAW + JPEG + XMP als eine Einheit, RAW primär.
    let g = env.group("2023-05-12 Geburtstag/IMG_0001.CR2").unwrap();
    assert_eq!(env.group("2023-05-12 Geburtstag/IMG_0001.JPG"), Some(g));
    assert_eq!(env.group("2023-05-12 Geburtstag/IMG_0001.xmp"), Some(g));
    let primary: String = env.one(&format!(
        "SELECT a.rel_path FROM asset_group g JOIN asset a ON a.id = g.primary_asset_id WHERE g.id = {g}"
    ));
    assert!(primary.ends_with("IMG_0001.CR2"));

    // Live Photo: HEIC + MOV + AAE.
    let live = env.group("iPhone/IMG_0100.HEIC").unwrap();
    assert_eq!(env.group("iPhone/IMG_0100.MOV"), Some(live));
    assert_eq!(env.group("iPhone/IMG_0100.AAE"), Some(live));

    // Unbekannte Datei: indexiert, aber ohne Gruppe.
    assert_eq!(env.group("misc/notizen.txt"), None);
    let other: i64 = env.one("SELECT count(*) FROM asset WHERE kind = 'other'");
    assert_eq!(other, 1);

    // Datum aus EXIF, aus dem Dateinamen und aus QuickTime mit Zeitzone.
    let exif: String = env.one(
        "SELECT taken_at || '|' || date_source FROM meta JOIN asset ON asset.id = meta.asset_id WHERE rel_path LIKE '%IMG_0002.JPG'",
    );
    assert_eq!(exif, "2023-05-12T15:20:44|exif");
    let from_name: String = env.one(
        "SELECT taken_at || '|' || date_source FROM meta JOIN asset ON asset.id = meta.asset_id WHERE rel_path LIKE '%IMG_20230512_101500.jpg'",
    );
    assert_eq!(from_name, "2023-05-12T10:15:00|filename");
    let mov: String = env.one(
        "SELECT taken_at || '|' || tz_offset FROM meta JOIN asset ON asset.id = meta.asset_id WHERE rel_path = 'iPhone/IMG_0100.MOV'",
    );
    assert_eq!(mov, "2024-07-14T18:05:00|120");
    let gps: f64 = env.one("SELECT gps_lat FROM meta JOIN asset ON asset.id = meta.asset_id WHERE rel_path LIKE '%IMG_0002.JPG'");
    assert!((gps - 52.52).abs() < 1e-6);

    // Videometadaten via ffprobe.
    let codec: String = env.one("SELECT codec FROM video_meta JOIN asset ON asset.id = video_meta.asset_id WHERE rel_path = 'video/clip.mp4'");
    assert_eq!(codec, "h264");

    // Alle Nicht-"other"-Dateien gehasht, Thumbnails im Cache.
    let unhashed: i64 = env.one("SELECT count(*) FROM asset WHERE hash IS NULL AND kind != 'other'");
    assert_eq!(unhashed, 0);
    let thumbs_ok: i64 = env.one("SELECT count(*) FROM thumb WHERE status = 'ok'");
    // JPEG x3, PNG, TIFF, WebP, HEIC x2, MP4, MOV x2, RAF, RW2 = 13; NEF/DNG/CR2/CR3-Stubs ohne Preview.
    assert_eq!(thumbs_ok, 13, "{report:?}");
    let any: String = env.one("SELECT hash FROM thumb WHERE status = 'ok' LIMIT 1");
    assert!(env.app.join(format!("thumbs/{}/{any}.webp", &any[..2])).unwrap().is_file());

    let grid = library::list_groups(&env.conn).unwrap();
    // Geburtstag 2, iPhone 1, misc 5, raw 5, video 2.
    assert_eq!(grid.len(), 15);
    let raw_jpeg = grid.iter().find(|i| i.group_id == g).unwrap();
    assert!(raw_jpeg.thumb.is_some(), "RAW+JPEG-Gruppe nutzt das JPEG-Thumbnail");
}

#[test]
fn incremental_rescan_only_touches_changes() {
    let mut env = Env::new();
    env.scan();

    let second = env.scan();
    assert_eq!((second.new, second.changed, second.removed, second.hashed, second.thumbs), (0, 0, 0, 0, 0));
    assert_eq!(second.unchanged, 20);

    // Datei ändern: genau eine Datei neu einlesen und hashen.
    let path = env.library.join("2023-05-12 Geburtstag/IMG_0002.JPG");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(b"\0\0");
    std::fs::write(&path, bytes).unwrap();
    let third = env.scan();
    assert_eq!((third.changed, third.hashed, third.thumbs), (1, 1, 1));
}

#[test]
fn ratings_survive_rescan_and_partial_group_changes() {
    let mut env = Env::new();
    env.scan();
    let g = env.group("2023-05-12 Geburtstag/IMG_0001.CR2").unwrap();
    env.conn.execute("INSERT INTO rating (group_id, stars, flag) VALUES (?1, 4, 'keep')", [g]).unwrap();

    // JPEG verschwindet (z. B. vom Nutzer außerhalb der App gelöscht).
    std::fs::remove_file(env.library.join("2023-05-12 Geburtstag/IMG_0001.JPG")).unwrap();
    let report = env.scan();
    assert_eq!(report.removed, 1);
    assert_eq!(env.group("2023-05-12 Geburtstag/IMG_0001.CR2"), Some(g));
    let stars: i64 = env.one(&format!("SELECT stars FROM rating WHERE group_id = {g}"));
    assert_eq!(stars, 4);
}

#[test]
fn unreachable_root_keeps_index() {
    let mut env = Env::new();
    env.scan();
    let before: i64 = env.one("SELECT count(*) FROM asset");

    // Externe Platte "abgezogen".
    let moved = env.library.with_file_name("woanders");
    std::fs::rename(&env.library, &moved).unwrap();
    let report = env.scan();
    assert_eq!(report.unreachable_roots.len(), 1);
    assert_eq!(report.removed, 0);
    assert_eq!(env.one::<i64>("SELECT count(*) FROM asset"), before);
}

#[cfg(unix)]
#[test]
fn unreadable_folder_is_not_treated_as_deleted() {
    use std::os::unix::fs::PermissionsExt;
    let mut env = Env::new();
    env.scan();
    let before: i64 = env.one("SELECT count(*) FROM asset");

    let dir = env.library.join("raw");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o000)).unwrap();
    let cancel = AtomicBool::new(false);
    let report = Scanner { conn: &mut env.conn, tools: &env.tools, app: &env.app, cancel: &cancel, progress: &|_| {} }
        .run(None)
        .unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(!report.errors.is_empty());
    assert_eq!(report.removed, 0);
    assert_eq!(env.one::<i64>("SELECT count(*) FROM asset"), before);
}

#[test]
fn cancelled_scan_resumes() {
    let mut env = Env::new();
    let cancel = AtomicBool::new(true);
    let report = Scanner { conn: &mut env.conn, tools: &env.tools, app: &env.app, cancel: &cancel, progress: &|_| {} }
        .run(None)
        .unwrap();
    assert!(report.cancelled);
    let full = env.scan();
    assert_eq!(full.new, 20);
}

#[test]
fn reports_every_phase_in_order_with_totals() {
    use std::sync::Mutex;
    let mut env = Env::new();
    let seen: Mutex<Vec<sichter_lib::scan::Progress>> = Mutex::new(Vec::new());
    let cancel = AtomicBool::new(false);
    let progress = |p: sichter_lib::scan::Progress| seen.lock().unwrap().push(p);
    Scanner { conn: &mut env.conn, tools: &env.tools, app: &env.app, cancel: &cancel, progress: &progress }
        .run(None)
        .unwrap();
    let seen = seen.into_inner().unwrap();

    let mut phases: Vec<&str> = seen.iter().map(|p| p.phase).collect();
    phases.dedup();
    assert_eq!(phases, ["crawl", "detect", "meta", "group", "hash", "thumbs", "done"]);
    // Jede Phase mit bekannter Gesamtzahl meldet sich sofort mit 0 und endet bei total.
    for phase in ["detect", "meta", "hash", "thumbs"] {
        let events: Vec<_> = seen.iter().filter(|p| p.phase == phase).collect();
        assert_eq!(events.first().unwrap().done, 0, "{phase}");
        let last = events.iter().map(|p| p.done).max().unwrap();
        assert_eq!(last, events[0].total, "{phase}");
    }
}

#[test]
fn retries_failed_thumbnails_from_older_versions() {
    let mut env = Env::new();
    env.scan();
    // Fehlschlag einer älteren Version simulieren (wie die Kachel-HEICs vor Version 2).
    env.conn
        .execute("UPDATE thumb SET status = 'failed', error = 'alt', version = 1 WHERE hash = (SELECT hash FROM asset WHERE rel_path = 'misc/kacheln.heic')", [])
        .unwrap();
    let report = env.scan();
    assert_eq!(report.thumbs, 1);
    // Fehlschläge der aktuellen Version werden dagegen nicht ständig wiederholt.
    env.conn.execute("UPDATE thumb SET status = 'failed', version = ?1 WHERE status = 'ok'", [sichter_lib::thumbs::VERSION]).unwrap();
    assert_eq!(env.scan().thumbs, 0);
}
