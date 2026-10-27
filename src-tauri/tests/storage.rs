//! Speicherübersicht gegen den echten Dateibestand der Fixtures.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use sichter_lib::db::{self, library};
use sichter_lib::fsops::appdata::AppDataDir;
use sichter_lib::scan::Scanner;
use sichter_lib::storage;
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

/// Summe der Dateigrößen unterhalb von `dir`, getrennt nach indexierbar und
/// ignoriert (versteckte Dateien wie `.DS_Store`).
fn walk_sizes(dir: &Path) -> (i64, i64, usize, usize) {
    let (mut indexed, mut ignored, mut files, mut dirs) = (0, 0, 0, 0);
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry.unwrap();
        if entry.file_type().is_dir() {
            dirs += 1;
            continue;
        }
        files += 1;
        let size = entry.metadata().unwrap().len() as i64;
        if entry.file_name().to_string_lossy().starts_with('.') {
            ignored += size;
        } else {
            indexed += size;
        }
    }
    (indexed, ignored, files, dirs)
}

fn setup() -> (tempfile::TempDir, PathBuf, rusqlite::Connection, i64) {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("Fotos");
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library"), &lib);
    let app = AppDataDir::init(dir.path().join("appdata")).unwrap();
    let mut conn = db::open(&app.join("t.db").unwrap()).unwrap();
    let root_id = library::add_root(&conn, &lib, app.path()).unwrap();
    let tools = Tools::resolve(None);
    let cancel = AtomicBool::new(false);
    Scanner { conn: &mut conn, tools: &tools, app: &app, cancel: &cancel, progress: &|_| {} }.run(None).unwrap();
    (dir, lib, conn, root_id)
}

#[test]
fn folder_sums_match_filesystem_exactly() {
    let (_dir, lib, conn, root_id) = setup();
    let rows = storage::folders(&conn, root_id).unwrap();
    assert_eq!(rows.len(), 6, "Root + 5 Unterordner");
    for row in &rows {
        let path = row.rel_path.split('/').filter(|p| !p.is_empty()).fold(lib.clone(), |p, s| p.join(s));
        let (indexed, _, _, _) = walk_sizes(&path);
        assert_eq!(row.total_size, indexed, "{}", row.rel_path);
        assert_eq!(row.kinds.iter().map(|k| k.size).sum::<i64>(), row.total_size, "{}", row.rel_path);
        assert_eq!(row.kinds.iter().map(|k| k.count).sum::<i64>(), row.file_count, "{}", row.rel_path);
    }
    let summary = storage::summary(&conn).unwrap();
    assert_eq!(summary.total_size, rows.iter().find(|r| r.rel_path.is_empty()).unwrap().total_size);
    assert_eq!(summary.raw_pair_count, 1);
}

/// Abgleich mit `du -A` (scheinbare Größe, macOS). Dokumentierte Toleranz:
/// * `du` zählt versteckte Dateien mit (`.DS_Store`), Sichter ignoriert sie: wird
///   exakt herausgerechnet.
/// * `du -A -k` rundet jede Datei und jeden Ordnereintrag auf volle KiB auf und
///   zählt die Größe der Ordnereinträge selbst: höchstens 1 KiB je Datei plus
///   ein paar KiB je Ordner (APFS-Verzeichnisgröße), hier großzügig 4 KiB.
#[cfg(target_os = "macos")]
#[test]
fn root_total_matches_du_within_block_tolerance() {
    let (_dir, lib, conn, root_id) = setup();
    let total = storage::folders(&conn, root_id).unwrap().into_iter().find(|r| r.rel_path.is_empty()).unwrap().total_size;
    let out = std::process::Command::new("du").args(["-A", "-k", "-s"]).arg(&lib).output().unwrap();
    let du_bytes: i64 = String::from_utf8_lossy(&out.stdout).split_whitespace().next().unwrap().parse::<i64>().unwrap() * 1024;
    let (_, ignored, files, dirs) = walk_sizes(&lib);
    let tolerance = files as i64 * 1024 + dirs as i64 * 4096;
    let diff = du_bytes - ignored - total;
    assert!((0..=tolerance).contains(&diff), "du {du_bytes} B, Sichter {total} B, ignoriert {ignored} B, Toleranz {tolerance} B");
}

#[test]
fn rescan_updates_sizes() {
    let (dir, lib, mut conn, root_id) = setup();
    std::fs::remove_file(lib.join("video/clip.mp4")).unwrap();
    let app = AppDataDir::init(dir.path().join("appdata")).unwrap();
    let tools = Tools::resolve(None);
    let cancel = AtomicBool::new(false);
    Scanner { conn: &mut conn, tools: &tools, app: &app, cancel: &cancel, progress: &|_| {} }.run(None).unwrap();
    let rows = storage::folders(&conn, root_id).unwrap();
    let video = rows.iter().find(|r| r.rel_path == "video").unwrap();
    assert_eq!(video.total_size, walk_sizes(&lib.join("video")).0);
    assert_eq!(video.file_count, 1);
}
