//! Größenaggregation pro Ordner und Dateityp (PLAN.md, Abschnitt 6.6).
//!
//! Die Tabellen `folder` und `folder_size_by_kind` werden nach jedem Scan aus
//! dem Index neu berechnet; das Dateisystem wird dafür nicht erneut gelesen.
//! Gezählt werden nur indexierte Dateien. Versteckte Dateien, Systemdateien und
//! übersprungene Mediathek-Pakete fehlen also bewusst, siehe Test gegen `du`.
//!
//! * `own_size`: Dateien direkt im Ordner.
//! * `total_size`, `file_count`: rekursiv inklusive aller Unterordner.
//! * `folder_size_by_kind`: rekursiv, je Dateiart.

use std::collections::{BTreeMap, HashMap};

use rusqlite::{Connection, params};
use serde::Serialize;

#[derive(Default)]
struct Agg {
    own_size: i64,
    total_size: i64,
    file_count: i64,
    kinds: HashMap<String, (i64, i64)>,
}

fn parent_of(dir: &str) -> Option<&str> {
    if dir.is_empty() {
        None
    } else {
        Some(dir.rsplit_once('/').map_or("", |(parent, _)| parent))
    }
}

/// Berechnet die Ordnergrößen eines Roots neu.
pub fn rebuild(conn: &Connection, root_id: i64) -> rusqlite::Result<()> {
    let mut folders: BTreeMap<String, Agg> = BTreeMap::new();
    folders.entry(String::new()).or_default();
    {
        let mut stmt = conn.prepare("SELECT rel_path, size, kind FROM asset WHERE root_id = ?1")?;
        let rows = stmt.query_map([root_id], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, String>(2)?)))?;
        for row in rows {
            let (rel_path, size, kind) = row?;
            let dir = rel_path.rsplit_once('/').map_or("", |(d, _)| d).to_string();
            folders.entry(dir.clone()).or_default().own_size += size;
            let mut current = Some(dir.as_str());
            while let Some(d) = current {
                let agg = folders.entry(d.to_string()).or_default();
                agg.total_size += size;
                agg.file_count += 1;
                let entry = agg.kinds.entry(kind.clone()).or_default();
                entry.0 += 1;
                entry.1 += size;
                current = parent_of(d);
            }
        }
    }

    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM folder WHERE root_id = ?1", [root_id])?;
    {
        let mut insert_folder = tx.prepare(
            "INSERT INTO folder (root_id, rel_path, parent_id, file_count, own_size, total_size) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        let mut insert_kind =
            tx.prepare("INSERT INTO folder_size_by_kind (folder_id, kind, file_count, total_size) VALUES (?1, ?2, ?3, ?4)")?;
        // BTreeMap-Reihenfolge: Eltern ("a") kommen immer vor Kindern ("a/b").
        let mut ids: HashMap<&str, i64> = HashMap::new();
        for (dir, agg) in &folders {
            let parent_id = parent_of(dir).and_then(|p| ids.get(p).copied());
            insert_folder.execute(params![root_id, dir, parent_id, agg.file_count, agg.own_size, agg.total_size])?;
            let id = tx.last_insert_rowid();
            ids.insert(dir.as_str(), id);
            for (kind, (count, size)) in &agg.kinds {
                insert_kind.execute(params![id, kind, count, size])?;
            }
        }
    }
    tx.commit()
}

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
pub struct KindSize {
    pub kind: String,
    pub count: i64,
    pub size: i64,
}

#[derive(Debug, Serialize)]
pub struct FolderRow {
    pub id: i64,
    pub parent_id: Option<i64>,
    pub rel_path: String,
    pub file_count: i64,
    pub own_size: i64,
    pub total_size: i64,
    pub kinds: Vec<KindSize>,
}

/// Alle Ordner eines Roots, größte zuerst.
pub fn folders(conn: &Connection, root_id: i64) -> rusqlite::Result<Vec<FolderRow>> {
    let mut kinds: HashMap<i64, Vec<KindSize>> = HashMap::new();
    {
        let mut stmt = conn.prepare(
            "SELECT k.folder_id, k.kind, k.file_count, k.total_size FROM folder_size_by_kind k
             JOIN folder f ON f.id = k.folder_id WHERE f.root_id = ?1",
        )?;
        let rows = stmt.query_map([root_id], |r| {
            Ok((r.get::<_, i64>(0)?, KindSize { kind: r.get(1)?, count: r.get(2)?, size: r.get(3)? }))
        })?;
        for row in rows {
            let (id, k) = row?;
            kinds.entry(id).or_default().push(k);
        }
    }
    let mut stmt = conn.prepare(
        "SELECT id, parent_id, rel_path, file_count, own_size, total_size FROM folder WHERE root_id = ?1
         ORDER BY total_size DESC, rel_path",
    )?;
    stmt.query_map([root_id], |r| {
        let id: i64 = r.get(0)?;
        Ok(FolderRow {
            id,
            parent_id: r.get(1)?,
            rel_path: r.get(2)?,
            file_count: r.get(3)?,
            own_size: r.get(4)?,
            total_size: r.get(5)?,
            kinds: Vec::new(),
        })
    })?
    .map(|row| {
        row.map(|mut f| {
            let mut k = kinds.remove(&f.id).unwrap_or_default();
            k.sort_by(|a, b| b.size.cmp(&a.size));
            f.kinds = k;
            f
        })
    })
    .collect()
}

#[derive(Debug, Serialize)]
pub struct LargeFile {
    pub root_id: i64,
    pub rel_path: String,
    pub size: i64,
    pub kind: String,
}

pub fn largest_files(conn: &Connection, limit: i64) -> rusqlite::Result<Vec<LargeFile>> {
    let mut stmt = conn.prepare("SELECT root_id, rel_path, size, kind FROM asset ORDER BY size DESC LIMIT ?1")?;
    stmt.query_map([limit], |r| Ok(LargeFile { root_id: r.get(0)?, rel_path: r.get(1)?, size: r.get(2)?, kind: r.get(3)? }))?
        .collect()
}

#[derive(Debug, Serialize)]
pub struct Summary {
    pub total_size: i64,
    pub file_count: i64,
    pub kinds: Vec<KindSize>,
    /// JPEG/HEIC-Anteil von RAW+JPEG-Paaren. Nur Information: Beide Dateien
    /// sind Originale, Sichter schlägt hier nichts zum Löschen vor.
    pub raw_pair_jpeg_size: i64,
    pub raw_pair_count: i64,
    /// Größe aller redundanten Kopien doppelter Dateien.
    pub duplicate_size: i64,
    /// Als Ausschuss markiert: Anzahl Gruppen und Größe aller ihrer Dateien.
    pub reject_count: i64,
    pub reject_size: i64,
}

pub fn summary(conn: &Connection) -> rusqlite::Result<Summary> {
    let mut stmt = conn.prepare("SELECT kind, count(*), coalesce(sum(size), 0) FROM asset GROUP BY kind ORDER BY 3 DESC")?;
    let kinds: Vec<KindSize> =
        stmt.query_map([], |r| Ok(KindSize { kind: r.get(0)?, count: r.get(1)?, size: r.get(2)? }))?.collect::<rusqlite::Result<_>>()?;
    let (reject_count, reject_size) = crate::db::ratings::reject_size(conn)?;
    let (raw_pair_count, raw_pair_jpeg_size) = conn.query_row(
        "SELECT count(*), coalesce(sum(a.size), 0) FROM asset a
         WHERE a.kind IN ('jpeg', 'heic') AND a.group_id IN (SELECT group_id FROM asset WHERE kind = 'raw' AND group_id IS NOT NULL)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok(Summary {
        total_size: kinds.iter().map(|k| k.size).sum(),
        file_count: kinds.iter().map(|k| k.count).sum(),
        kinds,
        raw_pair_jpeg_size,
        raw_pair_count,
        duplicate_size: crate::dupes::redundant_size(conn)?,
        reject_count,
        reject_size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn insert(conn: &Connection, rel: &str, size: i64, kind: &str) {
        conn.execute(
            "INSERT INTO asset (root_id, rel_path, size, mtime, kind) VALUES (1, ?1, ?2, 0, ?3)",
            params![rel, size, kind],
        )
        .unwrap();
    }

    #[test]
    fn aggregates_own_total_and_kinds_recursively() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("INSERT INTO library_root (path) VALUES ('/x')", []).unwrap();
        insert(&conn, "top.jpg", 10, "jpeg");
        insert(&conn, "2023/a.cr2", 100, "raw");
        insert(&conn, "2023/a.jpg", 20, "jpeg");
        insert(&conn, "2023/Urlaub/b.mov", 500, "video");
        // Ordner ohne eigene Dateien, nur mit Unterordner.
        insert(&conn, "leer/tief/c.png", 7, "png");
        rebuild(&conn, 1).unwrap();

        let rows = folders(&conn, 1).unwrap();
        let get = |p: &str| rows.iter().find(|f| f.rel_path == p).unwrap();
        let root = get("");
        assert_eq!((root.own_size, root.total_size, root.file_count), (10, 637, 5));
        assert_eq!(root.parent_id, None);
        let y = get("2023");
        assert_eq!((y.own_size, y.total_size, y.file_count), (120, 620, 3));
        assert_eq!(y.parent_id, Some(root.id));
        assert_eq!(get("2023/Urlaub").parent_id, Some(y.id));
        assert_eq!((get("leer").own_size, get("leer").total_size), (0, 7));
        assert_eq!(y.kinds[0], KindSize { kind: "video".into(), count: 1, size: 500 });
        assert_eq!(root.kinds.iter().map(|k| k.size).sum::<i64>(), root.total_size);
        // Größte zuerst.
        assert_eq!(rows[0].rel_path, "");
    }

    #[test]
    fn rebuild_replaces_previous_state() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("INSERT INTO library_root (path) VALUES ('/x')", []).unwrap();
        insert(&conn, "a/b.jpg", 5, "jpeg");
        rebuild(&conn, 1).unwrap();
        conn.execute("DELETE FROM asset", []).unwrap();
        insert(&conn, "c.jpg", 3, "jpeg");
        rebuild(&conn, 1).unwrap();
        let rows = folders(&conn, 1).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_size, 3);
    }

    #[test]
    fn summary_reports_raw_pair_jpeg_share() {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("INSERT INTO library_root (path) VALUES ('/x')", []).unwrap();
        conn.execute("INSERT INTO asset_group (id) VALUES (1), (2)", []).unwrap();
        insert(&conn, "a.cr2", 100, "raw");
        insert(&conn, "a.jpg", 20, "jpeg");
        insert(&conn, "b.jpg", 30, "jpeg");
        conn.execute("UPDATE asset SET group_id = 1 WHERE rel_path IN ('a.cr2', 'a.jpg')", []).unwrap();
        conn.execute("UPDATE asset SET group_id = 2 WHERE rel_path = 'b.jpg'", []).unwrap();
        let s = summary(&conn).unwrap();
        assert_eq!((s.raw_pair_count, s.raw_pair_jpeg_size), (1, 20));
        assert_eq!(s.total_size, 150);
    }
}
