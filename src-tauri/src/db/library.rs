//! Bibliotheks-Roots und Abfragen für die Bibliotheksansicht.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum RootError {
    #[error("Ordner nicht gefunden: {0}")]
    NotFound(String),
    #[error("Ordner ist bereits Teil der Bibliothek: {0}")]
    Overlaps(String),
    #[error("Das App-Datenverzeichnis kann nicht als Quelle dienen")]
    AppDataDir,
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}

#[derive(Debug, Serialize)]
pub struct RootInfo {
    pub id: i64,
    pub path: String,
    pub locked: bool,
    pub reachable: bool,
    pub file_count: i64,
    pub total_size: i64,
}

fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// Fügt einen Quellordner hinzu. Verschachtelte Roots werden abgewiesen, sonst
/// stünden Dateien doppelt im Index.
pub fn add_root(conn: &Connection, path: &Path, app_data: &Path) -> Result<i64, RootError> {
    let canonical = path.canonicalize().map_err(|_| RootError::NotFound(path.display().to_string()))?;
    if !canonical.is_dir() {
        return Err(RootError::NotFound(path.display().to_string()));
    }
    let app_data = app_data.canonicalize().unwrap_or_else(|_| app_data.to_path_buf());
    if overlaps(&canonical, &app_data) {
        return Err(RootError::AppDataDir);
    }
    for root in list_root_paths(conn)? {
        if overlaps(&canonical, &root) {
            return Err(RootError::Overlaps(root.display().to_string()));
        }
    }
    conn.execute("INSERT INTO library_root (path) VALUES (?1)", [canonical.to_string_lossy()])?;
    Ok(conn.last_insert_rowid())
}

/// Gesperrte Roots sind weder Quelle noch Ziel einer Operation (PLAN.md 3.11).
pub fn set_locked(conn: &Connection, id: i64, locked: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE library_root SET locked = ?2 WHERE id = ?1", params![id, locked])?;
    Ok(())
}

/// Entfernt einen Root nur aus dem Index. Dateien bleiben unberührt.
pub fn remove_root(conn: &Connection, id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM library_root WHERE id = ?1", [id])?;
    conn.execute(
        "DELETE FROM asset_group WHERE id NOT IN (SELECT group_id FROM asset WHERE group_id IS NOT NULL)",
        [],
    )?;
    Ok(())
}

fn list_root_paths(conn: &Connection) -> rusqlite::Result<Vec<PathBuf>> {
    let mut stmt = conn.prepare("SELECT path FROM library_root")?;
    stmt.query_map([], |r| Ok(PathBuf::from(r.get::<_, String>(0)?)))?.collect()
}

pub fn list_roots(conn: &Connection) -> rusqlite::Result<Vec<RootInfo>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.path, r.locked, count(a.id), coalesce(sum(a.size), 0)
         FROM library_root r LEFT JOIN asset a ON a.root_id = r.id GROUP BY r.id ORDER BY r.path",
    )?;
    stmt.query_map([], |r| {
        let path: String = r.get(1)?;
        Ok(RootInfo {
            id: r.get(0)?,
            reachable: Path::new(&path).is_dir(),
            path,
            locked: r.get(2)?,
            file_count: r.get(3)?,
            total_size: r.get(4)?,
        })
    })?
    .collect()
}

/// Eine Kachel im Bibliotheks-Grid: eine Gruppe (RAW+JPEG, Live Photo, ...).
#[derive(Debug, Serialize)]
pub struct GridItem {
    pub group_id: i64,
    pub asset_id: i64,
    pub root_id: i64,
    pub rel_path: String,
    pub kind: String,
    /// Arten aller Mitglieder, z. B. `["raw", "jpeg", "sidecar"]`.
    pub kinds: Vec<String>,
    pub thumb: Option<String>,
    pub taken_at: Option<String>,
    pub date_source: Option<String>,
    pub duration_s: Option<f64>,
    pub ext_mismatch: bool,
}

pub fn list_groups(conn: &Connection) -> rusqlite::Result<Vec<GridItem>> {
    // Thumbnail: JPEG/HEIC-Mitglieder zuerst, sie sehen meist besser aus als
    // eingebettete RAW-Previews.
    let mut stmt = conn.prepare(
        "SELECT g.id, p.id, p.root_id, p.rel_path, p.kind,
            (SELECT group_concat(x.kind) FROM asset x WHERE x.group_id = g.id),
            (SELECT x.hash FROM asset x JOIN thumb t ON t.hash = x.hash AND t.status = 'ok'
              WHERE x.group_id = g.id
              ORDER BY CASE x.kind WHEN 'jpeg' THEN 0 WHEN 'heic' THEN 1 WHEN 'video' THEN 4 WHEN 'raw' THEN 3 ELSE 2 END
              LIMIT 1),
            m.taken_at, m.date_source, v.duration_s, p.ext_mismatch
         FROM asset_group g
         JOIN asset p ON p.id = g.primary_asset_id
         LEFT JOIN meta m ON m.asset_id = p.id
         LEFT JOIN video_meta v ON v.asset_id = p.id
         ORDER BY m.taken_at IS NULL, m.taken_at, p.rel_path",
    )?;
    stmt.query_map([], |r| {
        let kinds: Option<String> = r.get(5)?;
        Ok(GridItem {
            group_id: r.get(0)?,
            asset_id: r.get(1)?,
            root_id: r.get(2)?,
            rel_path: r.get(3)?,
            kind: r.get(4)?,
            kinds: kinds.map(|k| k.split(',').map(str::to_string).collect()).unwrap_or_default(),
            thumb: r.get(6)?,
            taken_at: r.get(7)?,
            date_source: r.get(8)?,
            duration_s: r.get(9)?,
            ext_mismatch: r.get(10)?,
        })
    })?
    .collect()
}

#[derive(Debug, Serialize)]
pub struct OtherFile {
    pub root_id: i64,
    pub rel_path: String,
    pub size: i64,
    pub kind: String,
}

/// Dateien, die nie bewegt werden: unbekannte Typen und verwaiste Sidecars.
pub fn list_unhandled(conn: &Connection) -> rusqlite::Result<Vec<OtherFile>> {
    let mut stmt = conn.prepare(
        "SELECT root_id, rel_path, size, kind FROM asset WHERE group_id IS NULL ORDER BY root_id, rel_path",
    )?;
    stmt.query_map([], |r| Ok(OtherFile { root_id: r.get(0)?, rel_path: r.get(1)?, size: r.get(2)?, kind: r.get(3)? }))?
        .collect()
}

#[derive(Debug, Default, Serialize)]
pub struct LibraryStats {
    pub files: i64,
    pub groups: i64,
    pub total_size: i64,
    pub uncertain_dates: i64,
}

pub fn stats(conn: &Connection) -> rusqlite::Result<LibraryStats> {
    Ok(LibraryStats {
        files: conn.query_row("SELECT count(*) FROM asset", [], |r| r.get(0))?,
        groups: conn.query_row("SELECT count(*) FROM asset_group", [], |r| r.get(0))?,
        total_size: conn.query_row("SELECT coalesce(sum(size), 0) FROM asset", [], |r| r.get(0))?,
        uncertain_dates: conn.query_row(
            "SELECT count(*) FROM meta WHERE date_source IS NOT NULL AND date_source != 'exif'",
            [],
            |r| r.get(0),
        )?,
    })
}

pub fn root_path(conn: &Connection, id: i64) -> rusqlite::Result<PathBuf> {
    conn.query_row("SELECT path FROM library_root WHERE id = ?1", params![id], |r| Ok(PathBuf::from(r.get::<_, String>(0)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_nested_and_app_data_roots() {
        let conn = crate::db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path().join("app");
        crate::fsops::test_support::write(&dir.path().join("Fotos/2023/a.jpg"), b"x");
        crate::fsops::test_support::write(&app.join("x"), b"x");

        add_root(&conn, &dir.path().join("Fotos"), &app).unwrap();
        assert!(matches!(add_root(&conn, &dir.path().join("Fotos/2023"), &app), Err(RootError::Overlaps(_))));
        assert!(matches!(add_root(&conn, dir.path(), &app), Err(RootError::Overlaps(_)) | Err(RootError::AppDataDir)));
        assert!(matches!(add_root(&conn, &app, &app), Err(RootError::AppDataDir)));
        assert!(matches!(add_root(&conn, &dir.path().join("fehlt"), &app), Err(RootError::NotFound(_))));
    }
}
