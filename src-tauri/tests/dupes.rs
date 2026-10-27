//! Akzeptanztest Phase 3: kopierter, halb kopierter und umbenannter Ordner.

use std::path::Path;
use std::sync::atomic::AtomicBool;

use sichter_lib::db::{self, library};
use sichter_lib::dupes::{self, FolderKey};
use sichter_lib::fsops::appdata::AppDataDir;
use sichter_lib::scan::Scanner;
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

#[test]
fn detects_copied_half_copied_and_renamed_folders() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("Fotos");
    copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library"), &lib);
    let event = lib.join("2023-05-12 Geburtstag");
    // 1:1-Kopie.
    copy_dir(&event, &lib.join("Backup Geburtstag"));
    // Halb kopiert: nur zwei von vier Dateien.
    std::fs::create_dir_all(lib.join("USB halb")).unwrap();
    for f in ["IMG_0001.JPG", "IMG_0002.JPG"] {
        std::fs::copy(event.join(f), lib.join("USB halb").join(f)).unwrap();
    }
    // Inhaltsgleich, aber umbenannt.
    std::fs::create_dir_all(lib.join("Umbenannt")).unwrap();
    for (i, entry) in std::fs::read_dir(&event).unwrap().enumerate() {
        let entry = entry.unwrap();
        let ext = entry.path().extension().unwrap().to_string_lossy().into_owned();
        std::fs::copy(entry.path(), lib.join("Umbenannt").join(format!("bild_{i}.{ext}"))).unwrap();
    }

    let app = AppDataDir::init(dir.path().join("appdata")).unwrap();
    let mut conn = db::open(&app.join("t.db").unwrap()).unwrap();
    let root = library::add_root(&conn, &lib, app.path()).unwrap();
    let tools = Tools::resolve(None);
    let cancel = AtomicBool::new(false);
    Scanner { conn: &mut conn, tools: &tools, app: &app, cancel: &cancel, progress: &|_| {} }.run(None).unwrap();

    let a = dupes::analyze_db(&conn).unwrap();
    let key = |p: &str| FolderKey { root_id: root, rel_path: p.into() };

    // Jede der vier Geburtstagsdateien existiert mehrfach.
    let jpg = a.file_groups.iter().find(|g| g.files.iter().any(|f| f.rel_path == "2023-05-12 Geburtstag/IMG_0001.JPG")).unwrap();
    assert_eq!(jpg.files.len(), 4, "Original, Backup, halb, umbenannt");
    assert_eq!(jpg.files[0].rel_path, "2023-05-12 Geburtstag/IMG_0001.JPG", "Vorschlag: Datumsordner statt Backup/USB");

    // 1:1 identischer Ordner, Vorschlag behält den Datumsordner.
    assert_eq!(a.identical_folders.len(), 1, "{:?}", a.identical_folders);
    assert_eq!(a.identical_folders[0].keep.key, key("2023-05-12 Geburtstag"));
    assert_eq!(a.identical_folders[0].other.key, key("Backup Geburtstag"));

    // Umbenannter Ordner: inhaltsgleich zu beiden.
    let renamed: Vec<_> = a.same_content_folders.iter().filter(|p| p.other.key == key("Umbenannt") || p.keep.key == key("Umbenannt")).collect();
    assert_eq!(renamed.len(), 2, "{:?}", a.same_content_folders);

    // Halb kopierter Ordner als Teilmenge.
    let half: Vec<_> = a.subset_folders.iter().filter(|s| s.subset.key == key("USB halb")).collect();
    assert!(half.iter().any(|s| s.superset.key == key("2023-05-12 Geburtstag")), "{:?}", a.subset_folders);
    assert!(half.iter().all(|s| (s.coverage - 0.5).abs() < 1e-9));
    // Identische Ordner tauchen nicht zusätzlich als Teilmenge auf.
    assert!(!a.subset_folders.iter().any(|s| s.subset.key == key("Backup Geburtstag")));

    // Einsparbar in der Speicherübersicht.
    let summary = sichter_lib::storage::summary(&conn).unwrap();
    assert_eq!(summary.duplicate_size, a.redundant_size);
    assert!(summary.duplicate_size > 0);

    // Auswahl merken, nie automatisch übernehmen.
    assert_eq!(jpg.keep_asset_id, None);
    let other = jpg.files[1].asset_id;
    dupes::set_choice(&conn, &jpg.hash, Some(other)).unwrap();
    let again = dupes::analyze_db(&conn).unwrap();
    assert_eq!(again.file_groups.iter().find(|g| g.hash == jpg.hash).unwrap().keep_asset_id, Some(other));
    // Fremde Asset-ID wird abgewiesen.
    let foreign = a.file_groups.iter().find(|g| g.hash != jpg.hash).unwrap().files[0].asset_id;
    assert!(dupes::set_choice(&conn, &jpg.hash, Some(foreign)).is_err());
}
