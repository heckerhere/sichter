//! SQLite-Datenbank: Verbindung, WAL-Modus und versionierte Migrationen.
//!
//! Die Schemaversion steht in `PRAGMA user_version`. Jede Migration läuft in
//! einer eigenen Transaktion; bestehende Migrationen werden nie verändert, nur
//! neue angehängt.

use std::path::Path;

use rusqlite::{Connection, OpenFlags};

pub mod library;

const MIGRATIONS: &[&str] = &[
    include_str!("migrations/0001_init.sql"),
    include_str!("migrations/0002_scan.sql"),
    include_str!("migrations/0003_thumb_version.sql"),
    include_str!("migrations/0004_dupe_choice.sql"),
    include_str!("migrations/0005_events.sql"),
];

pub fn schema_version() -> u32 {
    MIGRATIONS.len() as u32
}

/// Öffnet die Datenbank (legt die Datei bei Bedarf an) und migriert sie.
pub fn open(path: &Path) -> rusqlite::Result<Connection> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// In-Memory-Datenbank für Tests.
pub fn open_in_memory() -> rusqlite::Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> rusqlite::Result<()> {
    // In-Memory-Datenbanken bleiben bei "memory", das ist für Tests in Ordnung.
    let _mode: String =
        conn.pragma_update_and_check(None, "journal_mode", "WAL", |row| row.get(0))?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    Ok(())
}

pub fn current_version(conn: &Connection) -> rusqlite::Result<u32> {
    conn.pragma_query_value(None, "user_version", |row| row.get(0))
}

fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    let current = current_version(conn)? as usize;
    if current > MIGRATIONS.len() {
        // Datenbank stammt aus einer neueren App-Version: nicht anfassen.
        return Err(rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_MISMATCH),
            Some(format!(
                "Datenbank hat Schemaversion {current}, diese App kennt nur {}",
                MIGRATIONS.len()
            )),
        ));
    }
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as u32)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_to_latest_version() {
        let conn = open_in_memory().unwrap();
        assert_eq!(current_version(&conn).unwrap(), schema_version());
    }

    #[test]
    fn migration_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        drop(open(&path).unwrap());
        let conn = open(&path).unwrap();
        assert_eq!(current_version(&conn).unwrap(), schema_version());
    }

    #[test]
    fn uses_wal_mode() {
        let dir = tempfile::tempdir().unwrap();
        let conn = open(&dir.path().join("test.db")).unwrap();
        let mode: String = conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(mode, "wal");
    }

    #[test]
    fn schema_contains_all_tables() {
        let conn = open_in_memory().unwrap();
        let expected = [
            "asset", "asset_group", "event", "event_asset", "folder", "folder_size_by_kind",
            "library_root", "meta", "operation", "rating", "run", "video_meta",
        ];
        for table in expected {
            let count: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(count, 1, "Tabelle {table} fehlt");
        }
    }

    #[test]
    fn enforces_foreign_keys_and_checks() {
        let conn = open_in_memory().unwrap();
        let orphan = conn.execute(
            "INSERT INTO asset (root_id, rel_path, size, mtime, kind) VALUES (99, 'a.jpg', 1, 0, 'jpeg')",
            [],
        );
        assert!(orphan.is_err(), "Fremdschlüssel muss greifen");

        conn.execute("INSERT INTO library_root (path) VALUES ('/x')", []).unwrap();
        let bad_kind = conn.execute(
            "INSERT INTO asset (root_id, rel_path, size, mtime, kind) VALUES (1, 'a.jpg', 1, 0, 'pdf')",
            [],
        );
        assert!(bad_kind.is_err(), "CHECK auf kind muss greifen");
    }

    #[test]
    fn refuses_newer_schema() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.db");
        {
            let conn = open(&path).unwrap();
            conn.pragma_update(None, "user_version", schema_version() + 1).unwrap();
        }
        assert!(open(&path).is_err());
    }
}
