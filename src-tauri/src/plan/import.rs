//! Plan für den Import aus der Fotos-Mediathek.
//!
//! Ziel ist `<Zielordner>/<Jahr>/<Datum>/<Originalname>`, mit dem
//! Aufnahmedatum laut Mediathek (Ortszeit). Alle Originale eines Fotos-Objekts
//! (Foto, RAW-Alternative, Live-Photo-Video) bilden eine Gruppe: Sie behalten
//! einen gemeinsamen Basisnamen, damit der Scan sie wieder als Einheit erkennt,
//! und werden gemeinsam exportiert oder gar nicht.
//!
//! Schon in einem früheren Lauf importierte Originale werden nicht noch einmal
//! geplant (erkannt am Journal, nicht am Dateinamen).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use chrono::TimeZone;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::{Builder, Conflict, ConflictKind, Op, PlanError, PlannedOp, Result, Summary, key, space, template};
use crate::fsops::PathGuard;
use crate::photos::{Asset, ResourceKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportParams {
    pub target: String,
    /// Ausgeblendete Fotos mit importieren.
    #[serde(default)]
    pub include_hidden: bool,
    /// Originale, die nur in iCloud liegen, herunterladen.
    pub download_originals: bool,
}

/// Was im Journal je Export-Operation steht (Spalte `note`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportNote {
    pub asset: String,
    pub kind: ResourceKind,
    pub name: String,
    pub created: Option<i64>,
    pub network: bool,
    #[serde(default)]
    pub favorite: bool,
}

/// Anzeigename der Quelle im Plan (kein echter Pfad).
pub fn source_label() -> &'static str {
    crate::i18n::tr("Fotos-Mediathek", "Photos library")
}

fn already_imported(conn: &Connection) -> rusqlite::Result<HashSet<(String, String)>> {
    let mut stmt = conn.prepare("SELECT note FROM operation WHERE op = 'export' AND status = 'done'")?;
    let notes: Vec<Option<String>> = stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
    Ok(notes
        .into_iter()
        .flatten()
        .filter_map(|n| serde_json::from_str::<ExportNote>(&n).ok())
        .map(|n| (n.asset, format!("{:?}/{}", n.kind, n.name)))
        .collect())
}

fn date_dir(target: &Path, created: Option<i64>) -> PathBuf {
    match created.and_then(|s| chrono::Local.timestamp_opt(s, 0).single()) {
        Some(t) => target.join(t.format("%Y").to_string()).join(t.format("%Y-%m-%d").to_string()),
        None => target.join(crate::i18n::dirs::import_no_date()),
    }
}

fn stem(name: &str) -> &str {
    name.rfind('.').filter(|i| *i > 0).map_or(name, |i| &name[..i])
}

pub fn plan_photos_import(conn: &Connection, assets: &[Asset], p: &ImportParams) -> Result<(Vec<PlannedOp>, Summary)> {
    let target = PathBuf::from(&p.target);
    if !target.is_dir() {
        return Err(PlanError::Invalid(crate::i18n::tr("Der Zielordner existiert nicht.", "The target folder does not exist.").into()));
    }
    if target.components().any(|c| c.as_os_str().to_string_lossy().to_lowercase().ends_with(".photoslibrary")) {
        return Err(PlanError::Invalid(crate::i18n::tr("Der Zielordner darf nicht in einer Fotos-Mediathek liegen.", "The target folder must not be inside a Photos library.").into()));
    }
    let locked: Vec<String> = {
        let mut stmt = conn.prepare("SELECT path FROM library_root WHERE locked = 1")?;
        stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    if PathGuard::new(locked).check(&target).is_err() {
        return Err(PlanError::Invalid(crate::i18n::tr("Der Zielordner liegt in einem gesperrten Ordner.", "The target folder is inside a locked folder.").into()));
    }

    let done = already_imported(conn)?;
    let mut sorted: Vec<&Asset> = assets.iter().collect();
    sorted.sort_by(|a, b| (a.created.is_none(), a.created, &a.id).cmp(&(b.created.is_none(), b.created, &b.id)));

    let mut b = Builder::default();
    let mut group = 0i64;
    for asset in sorted {
        if asset.resources.is_empty() {
            b.summary.not_exportable += 1;
            continue;
        }
        let todo: Vec<_> = asset.resources.iter().filter(|r| !done.contains(&(asset.id.clone(), format!("{:?}/{}", r.kind, r.original_name)))).collect();
        if todo.is_empty() {
            b.summary.already_imported += 1;
            continue;
        }
        let dir = date_dir(&target, asset.created);

        // Zielnamen: bereinigt, innerhalb des Objekts eindeutig.
        let mut names: Vec<String> = Vec::new();
        for r in &todo {
            let mut name = template::sanitize_file_name(&r.original_name);
            let mut n = 2;
            while names.iter().any(|x| key(Path::new(x)) == key(Path::new(&name))) {
                name = template::with_suffix(&template::sanitize_file_name(&r.original_name), "", n);
                n += 1;
            }
            names.push(name);
        }
        let taken = |b: &Builder, names: &[String]| {
            names.iter().any(|n| {
                let path = dir.join(n);
                b.planned_files.contains_key(&key(&path)) || std::fs::symlink_metadata(&path).is_ok()
            })
        };
        if taken(&b, &names) {
            // Gemeinsamer Suffix, damit Foto, RAW und Live-Video zusammen erkennbar bleiben.
            let base = stem(&names[0]).to_string();
            let mut n = 2;
            let renamed = loop {
                let candidate: Vec<String> = names.iter().map(|name| template::with_suffix(name, &base, n)).collect();
                if !taken(&b, &candidate) {
                    break candidate;
                }
                n += 1;
            };
            for name in &renamed {
                b.summary.conflicts.push(Conflict {
                    kind: ConflictKind::Renamed,
                    path: format!("{}/{}", source_label(), names[0]),
                    target: Some(dir.join(name).display().to_string()),
                });
            }
            names = renamed;
        }

        b.ensure_dirs(&target, &dir);
        group += 1;
        for (r, name) in todo.iter().zip(&names) {
            let dst = dir.join(name);
            let size = r.size.unwrap_or(0);
            b.planned_files.insert(key(&dst), String::new());
            b.summary.files += 1;
            b.summary.bytes += size;
            b.summary.bytes_to_copy += size;
            let note = ExportNote {
                asset: asset.id.clone(),
                kind: r.kind,
                name: r.original_name.clone(),
                created: asset.created,
                network: p.download_originals,
                favorite: asset.favorite,
            };
            b.ops.push(PlannedOp {
                op: Op::Export,
                src: Some(PathBuf::from(format!("{}/{}", source_label(), r.original_name))),
                dst: Some(dst),
                hash: None,
                size,
                asset_id: None,
                group_id: Some(group),
                keep_path: None,
                rating: None,
                note: serde_json::to_value(note).ok(),
            });
        }
        b.summary.groups += 1;
    }
    if b.summary.files == 0 {
        let msg = if b.summary.already_imported > 0 {
            crate::trf!("Nichts zu importieren: Alle {} Objekte wurden schon importiert.", "Nothing to import: all {} items were already imported.", b.summary.already_imported)
        } else {
            crate::i18n::tr("Die Mediathek enthält keine exportierbaren Fotos oder Videos.", "The library contains no exportable photos or videos.").to_string()
        };
        return Err(PlanError::Invalid(msg));
    }
    b.summary.free_bytes = space::free_bytes(&target);
    Ok((b.ops, b.summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::photos::Resource;

    fn res(kind: ResourceKind, name: &str) -> Resource {
        Resource { kind, original_name: name.into(), size: Some(10) }
    }

    fn asset(id: &str, created: Option<i64>, resources: Vec<Resource>) -> Asset {
        Asset { id: id.into(), created, favorite: false, resources }
    }

    #[test]
    fn groups_keep_a_common_name_on_collision() {
        let conn = crate::db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        // Mittag UTC: in jeder Zeitzone derselbe Tag.
        let noon = 1_700_049_600;
        let assets = vec![
            asset("A", Some(noon), vec![res(ResourceKind::Photo, "IMG_1.HEIC"), res(ResourceKind::PairedVideo, "IMG_1.MOV")]),
            asset("B", Some(noon + 60), vec![res(ResourceKind::Photo, "IMG_1.HEIC"), res(ResourceKind::PairedVideo, "IMG_1.MOV")]),
            asset("C", None, vec![res(ResourceKind::Photo, "a:b.jpg")]),
            asset("D", Some(noon), vec![]),
        ];
        let p = ImportParams { target: dir.path().display().to_string(), include_hidden: false, download_originals: true };
        let (ops, s) = plan_photos_import(&conn, &assets, &p).unwrap();
        let dsts: Vec<String> =
            ops.iter()
                .filter(|o| o.op == Op::Export)
                // Plattformneutral vergleichen (Windows trennt mit "\\").
                .map(|o| o.dst.as_ref().unwrap().strip_prefix(dir.path()).unwrap().components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"))
                .collect();
        let day = chrono::Local.timestamp_opt(noon, 0).unwrap().format("%Y/%Y-%m-%d").to_string();
        assert_eq!(
            dsts,
            vec![
                format!("{day}/IMG_1.HEIC"),
                format!("{day}/IMG_1.MOV"),
                format!("{day}/IMG_1 (2).HEIC"),
                format!("{day}/IMG_1 (2).MOV"),
                format!("{}/a_b.jpg", crate::i18n::dirs::import_no_date()),
            ]
        );
        assert_eq!((s.files, s.groups, s.not_exportable, s.bytes), (5, 3, 1, 50));
        assert_eq!(s.mkdirs, 3);
        // Gruppen-IDs je Objekt.
        let groups: Vec<i64> = ops.iter().filter_map(|o| o.group_id).collect();
        assert_eq!(groups, vec![1, 1, 2, 2, 3]);
    }

    #[test]
    fn rejects_bad_targets() {
        let conn = crate::db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let assets = vec![asset("A", Some(0), vec![res(ResourceKind::Photo, "x.jpg")])];
        let p = |t: &Path| ImportParams { target: t.display().to_string(), include_hidden: false, download_originals: false };
        assert!(plan_photos_import(&conn, &assets, &p(&dir.path().join("fehlt"))).is_err());
        conn.execute("INSERT INTO library_root (path, locked) VALUES (?1, 1)", [dir.path().display().to_string()]).unwrap();
        assert!(plan_photos_import(&conn, &assets, &p(dir.path())).is_err());
    }
}
