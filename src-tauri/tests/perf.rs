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

/// Phase 5: 500 Bilder flüssig durchklickbar (Vorschauen vorbereitet).
/// Ausführen mit: cargo test --release --test perf previews -- --ignored --nocapture
#[test]
#[ignore = "langsam; explizit mit --ignored ausführen"]
fn previews_for_500_photos() {
    use rayon::prelude::*;
    use sichter_lib::scan::kind::Kind;
    use sichter_lib::thumbs::preview::{PreviewService, PreviewSource};

    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.jpg");
    // 12-Megapixel-Testbild (4000x3000), wie von einer typischen Kamera.
    assert!(std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-f", "lavfi", "-i", "testsrc2=s=4000x3000", "-frames:v", "1", "-q:v", "3"])
        .arg(&base)
        .status()
        .unwrap()
        .success());
    let bytes = std::fs::read(&base).unwrap();
    let photos: Vec<_> = (0..500u32)
        .map(|i| {
            let p = dir.path().join(format!("IMG_{i:04}.JPG"));
            let mut b = bytes.clone();
            b.extend_from_slice(&i.to_le_bytes());
            std::fs::write(&p, b).unwrap();
            (format!("{:064x}", i + 1), p)
        })
        .collect();

    let app = AppDataDir::init(dir.path().join("appdata")).unwrap();
    let tools = Tools::resolve(Some(&Path::new(env!("CARGO_MANIFEST_DIR")).join("resources")));
    let service = PreviewService::new(app, tools);
    let source = |p: &Path| PreviewSource { path: p.to_path_buf(), kind: Kind::Jpeg, orientation: None, duration_s: None };

    let started = Instant::now();
    service.ensure(&photos[0].0, || Some(source(&photos[0].1))).unwrap().unwrap();
    let cold = started.elapsed();

    let started = Instant::now();
    let pool = rayon::ThreadPoolBuilder::new().num_threads(3).build().unwrap();
    pool.install(|| photos.par_iter().for_each(|(h, p)| drop(service.ensure(h, || Some(source(p))).unwrap())));
    let all = started.elapsed();

    let started = Instant::now();
    for (h, _) in &photos {
        let path = service.cached(h).unwrap();
        assert!(!std::fs::read(path).unwrap().is_empty());
    }
    let per_cached = started.elapsed() / photos.len() as u32;

    println!("Vorschau ohne Cache: {cold:.0?} · 500 vorbereiten (3 Threads): {all:.1?} · aus dem Cache je Bild: {per_cached:.2?}");
    assert!(per_cached < Duration::from_millis(20), "Auslieferung aus dem Cache zu langsam: {per_cached:?}");
    assert!(cold < Duration::from_secs(2), "Einzelne Vorschau zu langsam: {cold:?}");
}
