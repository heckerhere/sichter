//! Bewertungen (Sterne, Behalten/Ausschuss) pro Gruppe. Nur in der Datenbank,
//! nie in den Dateien (XMP-Export ist optional, Phase 8).

use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Flag {
    None,
    Keep,
    Reject,
}

impl Flag {
    pub fn as_str(self) -> &'static str {
        match self {
            Flag::None => "none",
            Flag::Keep => "keep",
            Flag::Reject => "reject",
        }
    }
}

/// Setzt Sterne und/oder Markierung. Unveränderte Felder bleiben erhalten.
pub fn set(conn: &Connection, group_id: i64, stars: Option<i64>, flag: Option<Flag>) -> rusqlite::Result<()> {
    if let Some(s) = stars.filter(|s| !(0..=5).contains(s)) {
        return Err(rusqlite::Error::InvalidParameterName(format!("Sterne müssen zwischen 0 und 5 liegen: {s}")));
    }
    conn.execute(
        "INSERT INTO rating (group_id, stars, flag) VALUES (?1, coalesce(?2, 0), coalesce(?3, 'none'))
         ON CONFLICT (group_id) DO UPDATE SET
            stars = coalesce(?2, stars),
            flag = coalesce(?3, flag),
            updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
        params![group_id, stars, flag.map(Flag::as_str)],
    )?;
    // Leere Bewertung braucht keine Zeile.
    conn.execute("DELETE FROM rating WHERE group_id = ?1 AND stars = 0 AND flag = 'none'", [group_id])?;
    Ok(())
}

pub fn reset(conn: &Connection, group_id: i64) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM rating WHERE group_id = ?1", [group_id])?;
    Ok(())
}

/// Gesamtgröße aller Dateien in als Ausschuss markierten Gruppen.
pub fn reject_size(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    conn.query_row(
        "SELECT count(DISTINCT a.group_id), coalesce(sum(a.size), 0) FROM asset a
         JOIN rating r ON r.group_id = a.group_id WHERE r.flag = 'reject'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = crate::db::open_in_memory().unwrap();
        conn.execute("INSERT INTO library_root (path) VALUES ('/x')", []).unwrap();
        conn.execute("INSERT INTO asset_group (id) VALUES (1)", []).unwrap();
        conn.execute("INSERT INTO asset (root_id, rel_path, size, mtime, kind, group_id) VALUES (1, 'a.cr2', 100, 0, 'raw', 1), (1, 'a.jpg', 20, 0, 'jpeg', 1)", [])
            .unwrap();
        conn
    }

    fn get(conn: &Connection) -> Option<(i64, String)> {
        conn.query_row("SELECT stars, flag FROM rating WHERE group_id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).ok()
    }

    #[test]
    fn stars_and_flags_are_independent() {
        let conn = setup();
        set(&conn, 1, Some(4), None).unwrap();
        set(&conn, 1, None, Some(Flag::Keep)).unwrap();
        assert_eq!(get(&conn), Some((4, "keep".into())));
        set(&conn, 1, None, Some(Flag::Reject)).unwrap();
        assert_eq!(get(&conn), Some((4, "reject".into())));
        assert_eq!(reject_size(&conn).unwrap(), (1, 120), "RAW+JPEG zählen beide");
        reset(&conn, 1).unwrap();
        assert_eq!(get(&conn), None);
    }

    #[test]
    fn empty_rating_removes_row_and_invalid_stars_fail() {
        let conn = setup();
        set(&conn, 1, Some(3), None).unwrap();
        set(&conn, 1, Some(0), Some(Flag::None)).unwrap();
        assert_eq!(get(&conn), None);
        assert!(set(&conn, 1, Some(6), None).is_err());
    }
}
