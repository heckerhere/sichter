-- Phase 8: Wahrnehmungs-Hash für Serien, XMP-Export als Operation.

-- dHash (64 Bit) je Inhalt, aus dem Thumbnail berechnet.
CREATE TABLE phash (
    hash   TEXT PRIMARY KEY,
    dhash  INTEGER NOT NULL
);

-- Neue Operation 'xmp'. SQLite kann CHECK-Bedingungen nicht ändern: Tabelle neu
-- aufbauen, Inhalt unverändert übernehmen.
CREATE TABLE operation_new (
    id      INTEGER PRIMARY KEY,
    run_id  INTEGER NOT NULL REFERENCES run (id) ON DELETE CASCADE,
    seq     INTEGER NOT NULL,
    op      TEXT    NOT NULL CHECK (op IN ('move', 'copy', 'mkdir', 'trash', 'xmp')),
    src     TEXT,
    dst     TEXT,
    hash    TEXT,
    status  TEXT    NOT NULL DEFAULT 'planned'
              CHECK (status IN ('planned', 'done', 'failed', 'undone')),
    error   TEXT,
    asset_id INTEGER,
    group_id INTEGER,
    size    INTEGER,
    keep_path TEXT,
    note    TEXT,
    UNIQUE (run_id, seq)
);
INSERT INTO operation_new (id, run_id, seq, op, src, dst, hash, status, error, asset_id, group_id, size, keep_path, note)
    SELECT id, run_id, seq, op, src, dst, hash, status, error, asset_id, group_id, size, keep_path, note FROM operation;
DROP TABLE operation;
ALTER TABLE operation_new RENAME TO operation;
CREATE INDEX operation_run ON operation (run_id, seq);
