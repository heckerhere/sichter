//! Akzeptanztest Phase 4: drei Tage und eine Reise. Alle Änderungen nur in der DB.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use sichter_lib::db::{self, library};
use sichter_lib::events;
use sichter_lib::fsops::appdata::AppDataDir;
use sichter_lib::scan::Scanner;
use sichter_lib::tools::Tools;

const BERLIN: (f64, f64) = (52.52, 13.405);
const HAMBURG: (f64, f64) = (53.55, 9.99);
const SPLIT: (f64, f64) = (43.508, 16.44);
const TROGIR: (f64, f64) = (43.517, 16.251);

/// (Ordner, Datei, Zeit, GPS)
type Shot = (&'static str, &'static str, &'static str, Option<(f64, f64)>);
const SHOTS: &[Shot] = &[
    ("2023-05-12 Geburtstag", "a1.jpg", "2023:05:12 15:00:00", Some(BERLIN)),
    ("2023-05-12 Geburtstag", "a2.jpg", "2023:05:12 16:10:00", Some(BERLIN)),
    ("2023-05-12 Geburtstag", "a3.jpg", "2023:05:12 17:30:00", None),
    ("DCIM", "b1.jpg", "2023:05:12 23:45:00", Some(BERLIN)),
    ("Hamburg", "c1.jpg", "2023:06:03 10:00:00", Some(HAMBURG)),
    ("Hamburg", "c2.jpg", "2023:06:03 11:00:00", Some(HAMBURG)),
    ("Hamburg", "c3.jpg", "2023:06:03 13:00:00", Some(BERLIN)),
    ("2023-07 Kroatien", "d1.jpg", "2023:07:12 18:00:00", Some(SPLIT)),
    ("2023-07 Kroatien", "d2.jpg", "2023:07:13 09:00:00", Some(TROGIR)),
    ("2023-07 Kroatien", "d3.jpg", "2023:07:13 19:00:00", Some(SPLIT)),
    ("2023-07 Kroatien", "d4.jpg", "2023:07:14 10:00:00", None),
    ("2023-07 Kroatien", "d5.jpg", "2023:07:14 11:00:00", Some(SPLIT)),
    ("2023-07 Kroatien", "d6.jpg", "2023:07:15 12:00:00", Some(SPLIT)),
];

fn make_library(lib: &Path) {
    let base = std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library/misc/IMG_20230512_101500.jpg")).unwrap();
    // Über die App-Auflösung, damit es mit Perl-Skript und Windows-exe gleich funktioniert.
    let exiftool = Tools::resolve(None).exiftool.expect("exiftool fehlt");
    for (i, (folder, name, time, gps)) in SHOTS.iter().enumerate() {
        let dir = lib.join(folder);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut bytes = base.clone();
        bytes.extend_from_slice(&(i as u32).to_le_bytes()); // jede Datei einzigartig
        std::fs::write(&path, bytes).unwrap();
        let mut cmd = exiftool.command();
        cmd.args(["-q", "-overwrite_original", &format!("-DateTimeOriginal={time}")]);
        if let Some((lat, lon)) = gps {
            cmd.args([format!("-GPSLatitude={lat}"), "-GPSLatitudeRef=N".into(), format!("-GPSLongitude={lon}"), "-GPSLongitudeRef=E".into()]);
        }
        assert!(cmd.arg(&path).status().unwrap().success());
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, (u64, std::time::SystemTime, String)> {
    walkdir::WalkDir::new(root)
        .into_iter()
        .map(|e| e.unwrap())
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let m = e.metadata().unwrap();
            let h = blake3::hash(&std::fs::read(e.path()).unwrap()).to_hex().to_string();
            (e.path().to_path_buf(), (m.len(), m.modified().unwrap(), h))
        })
        .collect()
}

#[test]
fn three_days_and_a_trip_end_to_end() {
    let dir = tempfile::tempdir().unwrap();
    let lib = dir.path().join("Fotos");
    make_library(&lib);
    let before = snapshot(&lib);

    let app = AppDataDir::init(dir.path().join("appdata")).unwrap();
    let mut conn = db::open(&app.join("t.db").unwrap()).unwrap();
    library::add_root(&conn, &lib, app.path()).unwrap();
    let tools = Tools::resolve(None);
    let cancel = AtomicBool::new(false);
    let scan = |conn: &mut rusqlite::Connection| {
        Scanner { conn, tools: &tools, app: &app, cancel: &cancel, progress: &|_| {} }.run(None).unwrap()
    };
    let report = scan(&mut conn);
    assert_eq!(report.events_assigned, 13);

    let list = events::list_events(&conn).unwrap();
    let summary: Vec<(usize, &str, Option<&str>)> =
        list.iter().map(|e| (e.count, e.suggested_title.as_str(), e.date_start.as_deref())).collect();
    assert_eq!(
        summary,
        vec![
            (3, "Geburtstag", Some("2023-05-12T15:00:00")),
            (1, "", Some("2023-05-12T23:45:00")),
            (2, "Hamburg", Some("2023-06-03T10:00:00")),
            (1, "Hamburg", Some("2023-06-03T13:00:00")),
            (6, "Kroatien", Some("2023-07-12T18:00:00")),
        ]
    );
    let trip = &list[4];
    assert_eq!(trip.date_end.as_deref(), Some("2023-07-15T12:00:00"));
    assert_eq!(trip.thumbs.len(), 4);

    // Bearbeiten: Abendfoto zum Geburtstag, umbenennen, bestätigen.
    let birthday = list[0].id;
    let merged = events::merge(&conn, &[list[1].id, birthday]).unwrap().unwrap();
    assert_eq!(merged, birthday);
    events::rename(&conn, birthday, "Leos Geburtstag").unwrap();
    events::set_confirmed(&conn, birthday, true).unwrap();

    // Teilen: Reise ab dem 14. als eigenes Event.
    let items = events::event_items(&conn, trip.id).unwrap();
    let at = items.iter().find(|i| i.time.as_deref() == Some("2023-07-14T10:00:00")).unwrap().group_id;
    let second = events::split(&conn, trip.id, at).unwrap().unwrap();
    assert_eq!(events::event_items(&conn, trip.id).unwrap().len(), 3);
    assert_eq!(events::event_items(&conn, second).unwrap().len(), 3);

    // Hamburg-Ausreißer zurück zu Hamburg verschieben.
    let list = events::list_events(&conn).unwrap();
    let hamburg = list.iter().find(|e| e.count == 2 && e.suggested_title == "Hamburg").unwrap().id;
    let stray = list.iter().find(|e| e.count == 1).unwrap().id;
    let stray_group = events::event_items(&conn, stray).unwrap()[0].group_id;
    events::move_groups(&conn, &[stray_group], Some(hamburg)).unwrap();
    assert!(events::list_events(&conn).unwrap().iter().all(|e| e.id != stray), "leeres Event wird entfernt");

    // Neu berechnen verändert bestätigte Events nicht.
    events::recluster(&conn).unwrap();
    let list = events::list_events(&conn).unwrap();
    let b = list.iter().find(|e| e.id == birthday).unwrap();
    assert_eq!((b.title.as_str(), b.count, b.confirmed), ("Leos Geburtstag", 4, true));
    assert_eq!(list.iter().map(|e| e.count).sum::<usize>(), 13);

    // Rescan: nichts Neues, Bearbeitungen bleiben.
    let report = scan(&mut conn);
    assert_eq!(report.events_assigned, 0);
    assert_eq!(events::list_events(&conn).unwrap().iter().find(|e| e.id == birthday).unwrap().title, "Leos Geburtstag");

    // Alle Änderungen nur in der Datenbank.
    assert_eq!(before, snapshot(&lib));
}

#[test]
fn settings_roundtrip_and_clamp() {
    let conn = db::open_in_memory().unwrap();
    assert_eq!(events::load_settings(&conn).unwrap(), events::Settings::default());
    events::save_settings(&conn, &events::Settings { gap_hours: 2.5, gps_km: 0.0, merge_multiday: false }).unwrap();
    let s = events::load_settings(&conn).unwrap();
    assert_eq!((s.gap_hours, s.gps_km, s.merge_multiday), (2.5, 1.0, false));
}
