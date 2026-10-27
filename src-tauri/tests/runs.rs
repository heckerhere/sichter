//! Pflichttests aus PLAN.md, Abschnitt 9: Planer, Ausführung, Undo, Absturz,
//! Kollisionen, Gruppen, gesperrte Roots, Unicode. Ausschließlich in temporären
//! Verzeichnissen.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use rusqlite::Connection;
use sichter_lib::db::{self, library, ratings};
use sichter_lib::fsops::appdata::AppDataDir;
use sichter_lib::journal::{self, Exec, JournalError};
use sichter_lib::plan::{self, Mode, ReorganizeParams, RunKind, template::DEFAULT_TEMPLATE};
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

/// Alle Dateien (relativer Pfad → Hash) und alle Ordner unterhalb von `root`.
fn tree(root: &Path) -> (BTreeMap<String, String>, BTreeSet<String>) {
    let mut files = BTreeMap::new();
    let mut dirs = BTreeSet::new();
    for e in walkdir::WalkDir::new(root).min_depth(1) {
        let e = e.unwrap();
        let rel = e.path().strip_prefix(root).unwrap().to_string_lossy().into_owned();
        if e.file_type().is_dir() {
            dirs.insert(rel);
        } else {
            files.insert(rel, blake3::hash(&std::fs::read(e.path()).unwrap()).to_hex().to_string());
        }
    }
    (files, dirs)
}

fn hash_multiset(root: &Path) -> Vec<String> {
    let mut v: Vec<String> = tree(root).0.into_values().collect();
    v.sort();
    v
}

struct Env {
    _dir: tempfile::TempDir,
    lib: PathBuf,
    app: AppDataDir,
    conn: Connection,
    tools: Tools,
    root: i64,
}

impl Env {
    fn with_library(fill: impl FnOnce(&Path)) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let lib = dir.path().join("Fotos");
        std::fs::create_dir_all(&lib).unwrap();
        // Kanonisch wie der gespeicherte Root (macOS: /var -> /private/var).
        let lib = lib.canonicalize().unwrap();
        fill(&lib);
        let app = AppDataDir::init(dir.path().join("appdata")).unwrap();
        let conn = db::open(&app.join("t.db").unwrap()).unwrap();
        let root = library::add_root(&conn, &lib, app.path()).unwrap();
        let mut env = Env { _dir: dir, lib, app, conn, tools: Tools::resolve(None), root };
        env.scan();
        env
    }

    fn fixtures() -> Self {
        Self::with_library(|lib| copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library"), lib))
    }

    fn scan(&mut self) -> sichter_lib::scan::ScanReport {
        let cancel = AtomicBool::new(false);
        Scanner { conn: &mut self.conn, tools: &self.tools, app: &self.app, cancel: &cancel, progress: &|_| {} }.run(None).unwrap()
    }

    fn params(&self, mode: Mode) -> ReorganizeParams {
        ReorganizeParams {
            source_root_ids: vec![self.root],
            target_root_id: self.root,
            template: DEFAULT_TEMPLATE.into(),
            video_template: None,
            mode,
            date_range: false,
            only_confirmed: false,
        }
    }

    fn plan(&self, p: &ReorganizeParams) -> (i64, plan::Summary, usize) {
        let (ops, summary) = plan::plan_reorganize(&self.conn, p).unwrap();
        let files = ops.iter().filter(|o| o.op != plan::Op::Mkdir).count();
        let meta = plan::NewRun { kind: RunKind::Reorganize, title: "Test", target_root_id: Some(p.target_root_id), dry_run: false };
        let id = plan::store(&self.conn, &meta, p, &ops, &summary).unwrap();
        (id, summary, files)
    }

    fn exec<T>(&self, f: impl FnOnce(&Exec) -> T) -> T {
        self.exec_crash(None, f)
    }

    fn exec_crash<T>(&self, crash: Option<usize>, f: impl FnOnce(&Exec) -> T) -> T {
        let (cancel, pause) = (AtomicBool::new(false), AtomicBool::new(false));
        f(&Exec { app: &self.app, cancel: &cancel, pause: &pause, progress: &|_| {}, simulate_crash_after: crash })
    }

    fn one<T: rusqlite::types::FromSql>(&self, sql: &str) -> T {
        self.conn.query_row(sql, [], |r| r.get(0)).unwrap()
    }
}

fn dir_of(env: &Env, file: &str) -> String {
    let rel: String = env.one(&format!("SELECT rel_path FROM asset WHERE rel_path LIKE '%/{file}' OR rel_path = '{file}'"));
    rel.rsplit_once('/').map_or(String::new(), |(d, _)| d.to_string())
}

#[test]
fn reorganize_moves_whole_groups_updates_index_and_keeps_hashes() {
    let mut env = Env::fixtures();
    let before = hash_multiset(&env.lib);
    let rated = sichter_lib::scan::group_of(&env.conn, env.root, "2023-05-12 Geburtstag/IMG_0001.CR2").unwrap().unwrap();
    ratings::set(&env.conn, rated, Some(5), None).unwrap();

    let (run, summary, files) = env.plan(&env.params(Mode::Move));
    assert!(summary.files > 0 && summary.conflicts.is_empty(), "{summary:?}");
    let report = env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!((report.failed, report.done), (0, files), "{:?}", report.messages);

    // Invariante: Menge der Hashes unverändert.
    assert_eq!(before, hash_multiset(&env.lib));
    // RAW + JPEG + XMP liegen zusammen, ebenso das Live Photo.
    let raw_dir = dir_of(&env, "IMG_0001.CR2");
    // RAW bestimmt das Datum der Gruppe (Stub von 2005), der Titel kommt aus dem Ordnernamen.
    assert_eq!(raw_dir, "2005/2005-08-03 Geburtstag");
    assert_eq!(dir_of(&env, "IMG_0001.JPG"), raw_dir);
    assert_eq!(dir_of(&env, "IMG_0001.xmp"), raw_dir);
    let live = dir_of(&env, "IMG_0100.HEIC");
    assert_eq!(dir_of(&env, "IMG_0100.MOV"), live);
    assert_eq!(dir_of(&env, "IMG_0100.AAE"), live);
    // Unbekannte Datei bleibt, wo sie war.
    assert!(env.lib.join("misc/notizen.txt").is_file());
    // Index folgt den Dateien: Bewertung bleibt, Rescan findet nichts Neues.
    assert_eq!(sichter_lib::scan::group_of(&env.conn, env.root, &format!("{raw_dir}/IMG_0001.CR2")).unwrap(), Some(rated));
    let r = env.scan();
    assert_eq!((r.new, r.removed, r.changed), (0, 0, 0));
    assert!(env.lib.join(&raw_dir).join("IMG_0001.CR2").is_file());
    // Log je Lauf.
    let log: String = env.one(&format!("SELECT log_path FROM run WHERE id = {run}"));
    assert!(Path::new(&log).is_file() && Path::new(&log.replace(".json", ".csv")).is_file());
}

#[test]
fn undo_restores_identical_tree() {
    let env = Env::fixtures();
    let before = tree(&env.lib);
    let (run, _, files) = env.plan(&env.params(Mode::Move));
    env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_ne!(before, tree(&env.lib));
    let report = env.exec(|ex| journal::undo(&env.conn, run, ex)).unwrap();
    assert_eq!(report.failed, 0, "{:?}", report.messages);
    assert_eq!(before, tree(&env.lib), "Dateien und Ordner wie vorher");
    assert_eq!(env.one::<String>(&format!("SELECT status FROM run WHERE id = {run}")), "undone");
    let back: i64 = env.one("SELECT count(*) FROM asset WHERE rel_path = '2023-05-12 Geburtstag/IMG_0001.CR2'");
    assert_eq!(back, 1, "Index zurück am alten Ort");
}

#[test]
fn dry_run_changes_nothing_and_execution_needs_matching_confirmation() {
    let env = Env::fixtures();
    let before = tree(&env.lib);
    let (run, _, files) = env.plan(&env.params(Mode::Move));
    let report = env.exec(|ex| journal::dry_run(&env.conn, run, ex)).unwrap();
    assert_eq!((report.done, report.failed), (files + report.done - files, 0), "{:?}", report.messages);
    assert_eq!(before, tree(&env.lib));
    assert_eq!(env.one::<String>(&format!("SELECT status FROM run WHERE id = {run}")), "planned", "Plan bleibt bestehen");

    let wrong = env.exec(|ex| journal::execute(&env.conn, run, files - 1, ex));
    assert!(matches!(wrong, Err(JournalError::Invalid(_))));
    assert_eq!(before, tree(&env.lib));
}

/// Ziel eines Assets laut Plan (relativ zur Bibliothek).
fn planned_target(env: &Env, ops: &[plan::PlannedOp], src_rel: &str) -> PathBuf {
    ops.iter().find(|o| o.src.as_deref() == Some(&env.lib.join(src_rel))).and_then(|o| o.dst.clone()).expect("geplant")
}

#[test]
fn collision_with_different_content_renames_the_whole_group() {
    let env = Env::with_library(|lib| copy_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/2023-05-12 Geburtstag"), &lib.join("Import")));
    let p = env.params(Mode::Move);
    let (ops, _) = plan::plan_reorganize(&env.conn, &p).unwrap();
    let jpg_target = planned_target(&env, &ops, "Import/IMG_0001.JPG");
    // Am Ziel liegt schon eine andere Datei mit diesem Namen.
    std::fs::create_dir_all(jpg_target.parent().unwrap()).unwrap();
    std::fs::write(&jpg_target, b"anderer Inhalt").unwrap();

    let (ops, summary) = plan::plan_reorganize(&env.conn, &p).unwrap();
    let mut names: Vec<String> = ["Import/IMG_0001.CR2", "Import/IMG_0001.JPG", "Import/IMG_0001.xmp"]
        .iter()
        .map(|s| planned_target(&env, &ops, s).file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(names, ["IMG_0001 (2).CR2", "IMG_0001 (2).JPG", "IMG_0001 (2).xmp"], "Suffix für alle Mitglieder gleich");
    assert!(summary.conflicts.iter().all(|c| c.kind == plan::ConflictKind::Renamed));

    let before = hash_multiset(&env.lib);
    let files = ops.iter().filter(|o| o.op != plan::Op::Mkdir).count();
    let run = plan::store(&env.conn, &plan::NewRun { kind: RunKind::Reorganize, title: "t", target_root_id: Some(env.root), dry_run: false }, &p, &ops, &summary).unwrap();
    env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!(before, hash_multiset(&env.lib), "nichts überschrieben");
    assert_eq!(std::fs::read(&jpg_target).unwrap(), b"anderer Inhalt");
}

#[test]
fn identical_file_at_target_stays_at_source() {
    let env = Env::with_library(|lib| {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/2023-05-12 Geburtstag/IMG_0002.JPG");
        std::fs::create_dir_all(lib.join("Import")).unwrap();
        std::fs::copy(&src, lib.join("Import/IMG_0002.JPG")).unwrap();
    });
    let p = env.params(Mode::Move);
    let (ops, _) = plan::plan_reorganize(&env.conn, &p).unwrap();
    let target = planned_target(&env, &ops, "Import/IMG_0002.JPG");
    std::fs::create_dir_all(target.parent().unwrap()).unwrap();
    std::fs::copy(env.lib.join("Import/IMG_0002.JPG"), &target).unwrap();

    let (ops, summary) = plan::plan_reorganize(&env.conn, &p).unwrap();
    assert!(ops.iter().all(|o| o.op == plan::Op::Mkdir), "{ops:?}");
    assert_eq!(summary.conflicts.len(), 1);
    assert_eq!(summary.conflicts[0].kind, plan::ConflictKind::DuplicateAtTarget);
}

#[test]
fn two_identical_sources_for_one_target_do_not_create_a_numbered_copy() {
    let env = Env::with_library(|lib| {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/2023-05-12 Geburtstag/IMG_0002.JPG");
        for d in ["Import", "Handy"] {
            std::fs::create_dir_all(lib.join(d)).unwrap();
            std::fs::copy(&src, lib.join(d).join("IMG_0002.JPG")).unwrap();
        }
    });
    // Beide Kopien im selben Event: gleiches Ziel. Eine wird verschoben, die andere bleibt.
    let (ops, summary) = plan::plan_reorganize(&env.conn, &env.params(Mode::Move)).unwrap();
    let moves: Vec<_> = ops.iter().filter(|o| o.op == plan::Op::Move).collect();
    assert_eq!(moves.len(), 1, "{ops:?}");
    assert!(!moves[0].dst.as_ref().unwrap().to_string_lossy().contains("(2)"));
    assert_eq!(summary.conflicts.len(), 1);
    assert_eq!(summary.conflicts[0].kind, plan::ConflictKind::DuplicateAtTarget);
}

#[test]
fn crash_mid_run_then_resume_and_undo() {
    let env = Env::fixtures();
    let before = tree(&env.lib);
    let (run, _, files) = env.plan(&env.params(Mode::Move));
    // Absturz nach 4 Dateioperationen: Die 4. ist im Dateisystem erledigt, aber nicht im Journal.
    let crash = env.exec_crash(Some(4), |ex| journal::execute(&env.conn, run, files, ex));
    assert!(matches!(crash, Err(JournalError::SimulatedCrash)));
    assert_eq!(journal::recover_interrupted(&env.conn).unwrap(), vec![run]);

    let report = env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!(report.failed, 0, "Wiederaufnahme erkennt die erledigte Operation: {:?}", report.messages);
    assert_eq!(hash_multiset(&env.lib), {
        let mut v: Vec<String> = before.0.values().cloned().collect();
        v.sort();
        v
    });
    env.exec(|ex| journal::undo(&env.conn, run, ex)).unwrap();
    assert_eq!(before, tree(&env.lib));
}

#[test]
fn locked_roots_are_never_source_or_target() {
    let env = Env::fixtures();
    let before = tree(&env.lib);
    env.conn.execute("UPDATE library_root SET locked = 1", []).unwrap();
    assert!(plan::plan_reorganize(&env.conn, &env.params(Mode::Move)).is_err(), "gesperrtes Ziel");

    // Sperre erst nach der Planung: Ausführung verweigert jede Operation.
    env.conn.execute("UPDATE library_root SET locked = 0", []).unwrap();
    let (run, _, files) = env.plan(&env.params(Mode::Move));
    env.conn.execute("UPDATE library_root SET locked = 1", []).unwrap();
    let report = env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!(report.done, 0);
    assert_eq!(before, tree(&env.lib));

    // Gesperrte Quelle: ihre Gruppen werden gar nicht erst geplant.
    let other = env.lib.parent().unwrap().join("Ziel");
    std::fs::create_dir_all(&other).unwrap();
    let target = library::add_root(&env.conn, &other, env.app.path()).unwrap();
    let mut p = env.params(Mode::Copy);
    p.target_root_id = target;
    let (ops, _) = plan::plan_reorganize(&env.conn, &p).unwrap();
    assert!(ops.is_empty());
}

#[test]
fn copy_keeps_sources_and_is_a_superset() {
    let env = Env::fixtures();
    let other = env.lib.parent().unwrap().join("Kopie");
    std::fs::create_dir_all(&other).unwrap();
    let target = library::add_root(&env.conn, &other, env.app.path()).unwrap();
    let dir_before = tree(&env.lib);
    let mut p = env.params(Mode::Copy);
    p.target_root_id = target;
    let (run, summary, files) = env.plan(&p);
    assert!(summary.free_bytes.is_some_and(|f| f as i64 > summary.bytes_to_copy));
    env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!(dir_before, tree(&env.lib), "Quelle unberührt");
    let copied: BTreeSet<String> = tree(&other).0.into_values().collect();
    let originals: BTreeSet<String> = dir_before.0.iter().filter(|(p, _)| !p.ends_with("notizen.txt") && !p.ends_with(".DS_Store")).map(|(_, h)| h.clone()).collect();
    assert!(originals.is_subset(&copied) || originals.difference(&copied).count() == 0, "alle Medien kopiert");
}

#[test]
fn group_failure_rolls_back_the_whole_group() {
    let env = Env::fixtures();
    let (run, _, files) = env.plan(&env.params(Mode::Move));
    // JPEG der RAW+JPEG-Gruppe nach der Planung verändern: Hash passt nicht mehr.
    let jpg = env.lib.join("2023-05-12 Geburtstag/IMG_0001.JPG");
    let mut bytes = std::fs::read(&jpg).unwrap();
    bytes.push(0);
    std::fs::write(&jpg, bytes).unwrap();
    let report = env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert!(report.failed >= 1);
    // Die ganze Gruppe ist am alten Ort, keine Hälfte wurde verschoben.
    for f in ["IMG_0001.CR2", "IMG_0001.JPG", "IMG_0001.xmp"] {
        assert!(env.lib.join("2023-05-12 Geburtstag").join(f).is_file(), "{f}");
    }
}

#[test]
fn unicode_spaces_and_reserved_names() {
    let env = Env::with_library(|lib| {
        let jpg = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/2023-05-12 Geburtstag/IMG_0002.JPG")).unwrap();
        let nfd = lib.join("Gru\u{0308}ße aus Ko\u{0308}ln");
        std::fs::create_dir_all(&nfd).unwrap();
        std::fs::write(nfd.join("Bild mit  Leerzeichen.jpg"), &jpg).unwrap();
        let mut b = jpg.clone();
        b.push(1);
        std::fs::write(nfd.join("CON.jpg"), b).unwrap();
    });
    let before = hash_multiset(&env.lib);
    let (run, _, files) = env.plan(&env.params(Mode::Move));
    env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!(before, hash_multiset(&env.lib));
    assert!(env.lib.join("2023/2023-05-12 Grüße aus Köln/_CON.jpg").is_file(), "{:?}", tree(&env.lib).0.keys().collect::<Vec<_>>());
    env.exec(|ex| journal::undo(&env.conn, run, ex)).unwrap();
    assert!(env.lib.join("Gru\u{0308}ße aus Ko\u{0308}ln/CON.jpg").is_file());
}

#[test]
fn rejects_to_folder_and_trash_plans() {
    let env = Env::fixtures();
    let g = sichter_lib::scan::group_of(&env.conn, env.root, "2023-05-12 Geburtstag/IMG_0001.CR2").unwrap().unwrap();
    ratings::set(&env.conn, g, None, Some(ratings::Flag::Reject)).unwrap();
    let (ops, _) = plan::plan_rejects_to_folder(&env.conn).unwrap();
    let dsts: Vec<String> = ops.iter().filter_map(|o| o.dst.as_ref()).map(|d| d.strip_prefix(&env.lib).unwrap().display().to_string()).collect();
    assert!(dsts.contains(&"_Aussortiert/2023-05-12 Geburtstag/IMG_0001.CR2".to_string()), "{dsts:?}");
    assert!(dsts.contains(&"_Aussortiert/2023-05-12 Geburtstag/IMG_0001.xmp".to_string()));
    // Papierkorb nur mit Einzelfreigabe.
    assert!(plan::plan_rejects_to_trash(&env.conn, &[]).is_err());
    let (trash, _) = plan::plan_rejects_to_trash(&env.conn, &[g, 999_999]).unwrap();
    assert_eq!(trash.len(), 3, "nur die freigegebene, als Ausschuss markierte Gruppe");
}

#[test]
fn duplicate_trash_refuses_when_kept_copy_changed() {
    let env = Env::with_library(|lib| {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/2023-05-12 Geburtstag/IMG_0002.JPG");
        std::fs::create_dir_all(lib.join("2023")).unwrap();
        std::fs::create_dir_all(lib.join("Downloads")).unwrap();
        std::fs::copy(&src, lib.join("2023/IMG_0002.JPG")).unwrap();
        std::fs::copy(&src, lib.join("Downloads/IMG_0002 (2).JPG")).unwrap();
    });
    let analysis = sichter_lib::dupes::analyze_db(&env.conn).unwrap();
    let hash = analysis.file_groups[0].hash.clone();
    let (ops, summary) = plan::plan_dupes(&env.conn, &[hash], true).unwrap();
    assert_eq!(ops.len(), 1);
    assert!(ops[0].src.as_ref().unwrap().ends_with("Downloads/IMG_0002 (2).JPG"));
    assert!(ops[0].keep_path.as_ref().unwrap().ends_with("2023/IMG_0002.JPG"));
    let run = plan::store(&env.conn, &plan::NewRun { kind: RunKind::DupesToTrash, title: "t", target_root_id: None, dry_run: false }, &(), &ops, &summary).unwrap();
    // Bleibende Kopie nachträglich verändern: Papierkorb wird verweigert.
    std::fs::write(env.lib.join("2023/IMG_0002.JPG"), "verändert").unwrap();
    let report = env.exec(|ex| journal::execute(&env.conn, run, 1, ex)).unwrap();
    assert_eq!((report.done, report.failed), (0, 1), "{:?}", report.messages);
    assert!(env.lib.join("Downloads/IMG_0002 (2).JPG").is_file());
}

#[test]
fn duplicates_never_split_groups() {
    let env = Env::with_library(|lib| {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/2023-05-12 Geburtstag");
        copy_dir(&src, &lib.join("A"));
        // In B nur das JPEG ohne RAW: Das JPEG in A gehört zu einer RAW-Gruppe.
        std::fs::create_dir_all(lib.join("B")).unwrap();
        std::fs::copy(src.join("IMG_0001.JPG"), lib.join("B/IMG_0001.JPG")).unwrap();
    });
    let analysis = sichter_lib::dupes::analyze_db(&env.conn).unwrap();
    let g = analysis.file_groups.iter().find(|g| g.files.iter().any(|f| f.rel_path == "A/IMG_0001.JPG")).unwrap();
    // Nutzer wählt die Kopie in B zum Behalten: A/IMG_0001.JPG wäre zu entfernen, aber
    // dann bliebe das RAW in A allein.
    let keep_b = g.files.iter().find(|f| f.rel_path == "B/IMG_0001.JPG").unwrap().asset_id;
    sichter_lib::dupes::set_choice(&env.conn, &g.hash, Some(keep_b)).unwrap();
    let (ops, summary) = plan::plan_dupes(&env.conn, std::slice::from_ref(&g.hash), true).unwrap();
    assert!(ops.is_empty());
    assert_eq!(summary.conflicts[0].kind, plan::ConflictKind::GroupPartial);
}

/// Echter Papierkorb: legt eine Testdatei in den Papierkorb des Systems.
/// Ausführen mit: cargo test --test runs real_trash -- --ignored
#[test]
#[ignore = "legt eine Datei in den echten System-Papierkorb"]
fn real_trash_for_approved_rejects() {
    let env = Env::with_library(|lib| {
        std::fs::copy(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/misc/screenshot.png"), lib.join("sichter-testdatei.png")).unwrap();
    });
    let g = sichter_lib::scan::group_of(&env.conn, env.root, "sichter-testdatei.png").unwrap().unwrap();
    ratings::set(&env.conn, g, None, Some(ratings::Flag::Reject)).unwrap();
    let (ops, summary) = plan::plan_rejects_to_trash(&env.conn, &[g]).unwrap();
    let run = plan::store(&env.conn, &plan::NewRun { kind: RunKind::RejectsToTrash, title: "t", target_root_id: None, dry_run: false }, &(), &ops, &summary).unwrap();
    let report = env.exec(|ex| journal::execute(&env.conn, run, 1, ex)).unwrap();
    assert_eq!(report.done, 1);
    assert!(!env.lib.join("sichter-testdatei.png").exists());
    let undo = env.exec(|ex| journal::undo(&env.conn, run, ex)).unwrap();
    assert_eq!(undo.manual_restore.len(), 1, "Papierkorb wird nicht vorgetäuscht zurückgelegt");
}

mod property {
    //! Zufällige Ordnerbäume: planen, ausführen, rückgängig machen. Danach muss
    //! der Baum identisch sein, dazwischen gilt die Hash-Invariante.
    use super::*;
    use proptest::prelude::*;

    const DIRS: &[&str] = &["", "A", "A/B", "Grüße aus Köln", "C/D e"];
    const NAMES: &[&str] = &["IMG_1.JPG", "IMG_2.JPG", "foto.jpg", "Bild 3.JPG"];
    const TEMPLATES: &[&str] = &[DEFAULT_TEMPLATE, "{Jahr}/{Monat}/{Original-Dateiname}", "Alle/{Datum}_{Original-Dateiname}"];

    proptest! {
        #![proptest_config(ProptestConfig { cases: 12, failure_persistence: None, ..ProptestConfig::default() })]

        #[test]
        fn plan_execute_undo_roundtrip(
            files in proptest::collection::vec((0..DIRS.len(), 0..NAMES.len(), 0u8..4), 1..10),
            template in 0..TEMPLATES.len(),
        ) {
            let base = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/2023-05-12 Geburtstag/IMG_0002.JPG")).unwrap();
            let env = Env::with_library(|lib| {
                for (d, n, tail) in &files {
                    let dir = lib.join(DIRS[*d]);
                    std::fs::create_dir_all(&dir).unwrap();
                    // Wenige verschiedene Inhalte: erzeugt Duplikate und Namenskonflikte.
                    let mut b = base.clone();
                    b.push(*tail);
                    std::fs::write(dir.join(NAMES[*n]), b).unwrap();
                }
            });
            let before = tree(&env.lib);
            let mut p = env.params(Mode::Move);
            p.template = TEMPLATES[template].into();
            let (ops, summary) = plan::plan_reorganize(&env.conn, &p).unwrap();
            let files_n = ops.iter().filter(|o| o.op != plan::Op::Mkdir).count();
            if files_n == 0 {
                return Ok(());
            }
            let meta = plan::NewRun { kind: RunKind::Reorganize, title: "p", target_root_id: Some(env.root), dry_run: false };
            let run = plan::store(&env.conn, &meta, &p, &ops, &summary).unwrap();
            let report = env.exec(|ex| journal::execute(&env.conn, run, files_n, ex)).unwrap();
            prop_assert_eq!(report.failed, 0, "{:?}", report.messages);
            prop_assert_eq!(hash_multiset(&env.lib), { let mut v: Vec<String> = before.0.values().cloned().collect(); v.sort(); v });
            let undo = env.exec(|ex| journal::undo(&env.conn, run, ex)).unwrap();
            prop_assert_eq!(undo.failed, 0, "{:?}", undo.messages);
            prop_assert_eq!(&before, &tree(&env.lib));
        }
    }
}

#[test]
fn cancel_stops_after_current_group_and_run_can_be_resumed() {
    let env = Env::fixtures();
    let before = hash_multiset(&env.lib);
    let (run, _, files) = env.plan(&env.params(Mode::Move));
    let (cancel, pause) = (AtomicBool::new(false), AtomicBool::new(false));
    let calls = std::sync::atomic::AtomicUsize::new(0);
    // Nach der ersten Fortschrittsmeldung abbrechen.
    let progress = |_: journal::RunProgress| {
        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 3 {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    };
    let ex = Exec { app: &env.app, cancel: &cancel, pause: &pause, progress: &progress, simulate_crash_after: None };
    let first = journal::execute(&env.conn, run, files, &ex).unwrap();
    assert!(first.cancelled && first.done < files, "{first:?}");
    assert_eq!(env.one::<String>(&format!("SELECT status FROM run WHERE id = {run}")), "aborted");
    assert_eq!(before, hash_multiset(&env.lib));

    let rest = env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
    assert_eq!(first.done + rest.done, files);
    assert_eq!(env.one::<String>(&format!("SELECT status FROM run WHERE id = {run}")), "done");
}

/// Verschieben auf ein echtes zweites Volume (RAM-Disk über hdiutil, macOS).
/// Ausführen mit: cargo test --test runs real_cross_volume -- --ignored
#[cfg(target_os = "macos")]
#[test]
#[ignore = "legt kurzzeitig eine RAM-Disk an"]
fn real_cross_volume_move_and_undo() {
    use std::process::Command;
    let out = Command::new("hdiutil").args(["attach", "-nomount", "ram://65536"]).output().unwrap();
    let dev = String::from_utf8_lossy(&out.stdout).trim().to_string();
    assert!(dev.starts_with("/dev/disk"), "{dev}");
    let name = format!("SichterTest{}", std::process::id());
    let ok = Command::new("diskutil").args(["erasevolume", "APFS", &name, &dev]).output().unwrap().status.success();
    let volume = PathBuf::from(format!("/Volumes/{name}"));
    let result = std::panic::catch_unwind(|| {
        assert!(ok && volume.is_dir());
        let env = Env::fixtures();
        let target = volume.join("Fotos");
        std::fs::create_dir_all(&target).unwrap();
        let target_id = library::add_root(&env.conn, &target, env.app.path()).unwrap();
        let before = tree(&env.lib);
        let mut p = env.params(Mode::Move);
        p.target_root_id = target_id;
        let (run, summary, files) = env.plan(&p);
        assert!(summary.bytes_to_copy > 0, "anderes Volume erkannt");
        let report = env.exec(|ex| journal::execute(&env.conn, run, files, ex)).unwrap();
        assert_eq!((report.done, report.failed), (files, 0), "{:?}", report.messages);
        let moved: Vec<String> = tree(&target).0.into_values().collect();
        assert_eq!(moved.len(), files);
        env.exec(|ex| journal::undo(&env.conn, run, ex)).unwrap();
        assert_eq!(before.0, tree(&env.lib).0, "alle Dateien zurück auf dem ersten Volume");
    });
    Command::new("hdiutil").args(["detach", &dev, "-force"]).output().unwrap();
    result.unwrap();
}

#[test]
fn empty_folder_cleanup_only_plans_truly_empty_topmost_folders() {
    let env = Env::with_library(|lib| {
        std::fs::create_dir_all(lib.join("leer/tief/tiefer")).unwrap();
        std::fs::write(lib.join("leer/.DS_Store"), b"x").unwrap();
        std::fs::write(lib.join("leer/tief/Thumbs.db"), b"x").unwrap();
        std::fs::create_dir_all(lib.join("versteckt")).unwrap();
        std::fs::write(lib.join("versteckt/.geheim"), b"x").unwrap();
        std::fs::create_dir_all(lib.join("voll/leer drin")).unwrap();
        std::fs::write(lib.join("voll/notiz.txt"), b"x").unwrap();
    });
    let (ops, _) = plan::plan_empty_dirs(&env.conn).unwrap();
    let mut dirs: Vec<String> = ops.iter().map(|o| o.src.as_ref().unwrap().strip_prefix(&env.lib).unwrap().display().to_string()).collect();
    dirs.sort();
    // "leer" als Ganzes (nicht seine Unterordner), "voll/leer drin" einzeln, "versteckt" nicht (andere versteckte Datei).
    assert_eq!(dirs, ["leer", "voll/leer drin"]);

    // Wird ein Ordner nach der Planung befüllt, bleibt er stehen (ohne den Papierkorb zu berühren).
    let summary = plan::Summary::default();
    let meta = plan::NewRun { kind: RunKind::EmptyDirsToTrash, title: "t", target_root_id: None, dry_run: false };
    let only_full: Vec<_> = ops.into_iter().filter(|o| o.src.as_ref().unwrap().ends_with("leer drin")).collect();
    let run = plan::store(&env.conn, &meta, &(), &only_full, &summary).unwrap();
    std::fs::write(env.lib.join("voll/leer drin/neu.jpg"), b"x").unwrap();
    let report = env.exec(|ex| journal::execute(&env.conn, run, 1, ex)).unwrap();
    assert_eq!((report.done, report.failed), (0, 1));
    assert!(env.lib.join("voll/leer drin/neu.jpg").is_file());
}

#[test]
fn locking_a_root_via_library_blocks_plans() {
    let env = Env::fixtures();
    library::set_locked(&env.conn, env.root, true).unwrap();
    assert!(plan::plan_reorganize(&env.conn, &env.params(Mode::Move)).is_err());
    assert!(plan::plan_empty_dirs(&env.conn).is_err(), "gesperrte Roots werden nicht nach leeren Ordnern durchsucht");
}
