//! Performance-Test: 10.000 Dateien (PLAN.md, Phase 1).
//! Ausführen mit: cargo test --release --test perf -- --ignored --nocapture

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

use sichter_lib::db::{self, library};
use sichter_lib::fsops::appdata::AppDataDir;
use sichter_lib::scan::Scanner;
use sichter_lib::tools::Tools;

const FILES: usize = 10_000;

#[test]
#[ignore = "langsam; explizit mit --ignored ausführen"]
fn scans_10k_files_incrementally() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("Fotos");
    let jpeg = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/2023-05-12 Geburtstag/IMG_0001.JPG")).unwrap();
    for i in 0..FILES {
        let folder = lib.join(format!("2023-{:02} Ordner {}", i % 12 + 1, i / 100));
        std::fs::create_dir_all(&folder).unwrap();
        // Bytes nach dem JPEG-Ende machen jede Datei einzigartig.
        let mut bytes = jpeg.clone();
        bytes.extend_from_slice(&(i as u64).to_le_bytes());
        std::fs::write(folder.join(format!("IMG_{i:05}.JPG")), bytes).unwrap();
    }

    let app = AppDataDir::init(dir.path().join("appdata")).unwrap();
    let mut conn = db::open(&app.join("perf.db").unwrap()).unwrap();
    library::add_root(&conn, &lib, app.path()).unwrap();
    // Im Release-Build wird nur das Ressourcenverzeichnis durchsucht, wie in der gebündelten App.
    let tools = Tools::resolve(Some(&Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")));
    let cancel = AtomicBool::new(false);

    let mut run = || {
        let started = Instant::now();
        let report = Scanner { conn: &mut conn, tools: &tools, app: &app, cancel: &cancel, progress: &|_| {} }
            .run(None)
            .unwrap();
        (report, started.elapsed())
    };

    let (first, first_time) = run();
    assert_eq!(first.new as usize, FILES);
    assert_eq!(first.thumbs as usize, FILES);
    let (second, second_time) = run();
    assert_eq!(second.unchanged as usize, FILES);
    assert_eq!(second.hashed + second.thumbs, 0);

    println!("Erstscan {FILES} Dateien: {first_time:.2?}, inkrementeller Rescan: {second_time:.2?}");
    assert!(second_time < Duration::from_secs(5), "Rescan zu langsam: {second_time:?}");
}
