-- Initiales Schema (PLAN.md, Abschnitt 5.2).
-- Zeitstempel als ISO-8601-Text (UTC), Dateizeiten (mtime) als Unix-Nanosekunden.

CREATE TABLE library_root (
    id        INTEGER PRIMARY KEY,
    path      TEXT    NOT NULL UNIQUE,
    locked    INTEGER NOT NULL DEFAULT 0 CHECK (locked IN (0, 1)),
    added_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

-- RAW+JPEG+Sidecars bzw. Live Photos als Einheit.
CREATE TABLE asset_group (
    id               INTEGER PRIMARY KEY,
    primary_asset_id INTEGER REFERENCES asset (id) ON DELETE SET NULL
);

CREATE TABLE asset (
    id        INTEGER PRIMARY KEY,
    root_id   INTEGER NOT NULL REFERENCES library_root (id) ON DELETE CASCADE,
    rel_path  TEXT    NOT NULL,
    size      INTEGER NOT NULL,
    mtime     INTEGER NOT NULL,
    hash      TEXT,
    kind      TEXT    NOT NULL CHECK (kind IN
                ('raw', 'jpeg', 'heic', 'png', 'tiff', 'webp', 'video', 'sidecar', 'other')),
    group_id  INTEGER REFERENCES asset_group (id) ON DELETE SET NULL,
    UNIQUE (root_id, rel_path)
);
CREATE INDEX asset_hash ON asset (hash);
CREATE INDEX asset_size ON asset (size);
CREATE INDEX asset_group_id ON asset (group_id);

CREATE TABLE video_meta (
    asset_id    INTEGER PRIMARY KEY REFERENCES asset (id) ON DELETE CASCADE,
    duration_s  REAL,
    codec       TEXT,
    fps         REAL,
    width       INTEGER,
    height      INTEGER
);

CREATE TABLE meta (
    asset_id      INTEGER PRIMARY KEY REFERENCES asset (id) ON DELETE CASCADE,
    taken_at      TEXT,
    tz_offset     INTEGER,             -- Minuten gegenüber UTC, NULL = unbekannt
    -- Herkunft des Datums; alles außer 'exif' gilt im UI als "Datum unsicher".
    date_source   TEXT CHECK (date_source IN ('exif', 'filename', 'mtime')),
    camera_make   TEXT,
    camera_model  TEXT,
    lens          TEXT,
    gps_lat       REAL,
    gps_lon       REAL,
    width         INTEGER,
    height        INTEGER,
    orientation   INTEGER
);
CREATE INDEX meta_taken_at ON meta (taken_at);

CREATE TABLE event (
    id          INTEGER PRIMARY KEY,
    title       TEXT    NOT NULL DEFAULT '',
    date_start  TEXT,
    date_end    TEXT,
    notes       TEXT    NOT NULL DEFAULT '',
    confirmed   INTEGER NOT NULL DEFAULT 0 CHECK (confirmed IN (0, 1))
);

CREATE TABLE event_asset (
    event_id  INTEGER NOT NULL REFERENCES event (id) ON DELETE CASCADE,
    group_id  INTEGER NOT NULL REFERENCES asset_group (id) ON DELETE CASCADE,
    PRIMARY KEY (event_id, group_id)
);
CREATE INDEX event_asset_group ON event_asset (group_id);

CREATE TABLE rating (
    group_id    INTEGER PRIMARY KEY REFERENCES asset_group (id) ON DELETE CASCADE,
    stars       INTEGER NOT NULL DEFAULT 0 CHECK (stars BETWEEN 0 AND 5),
    flag        TEXT    NOT NULL DEFAULT 'none' CHECK (flag IN ('keep', 'reject', 'none')),
    updated_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

CREATE TABLE folder (
    id            INTEGER PRIMARY KEY,
    root_id       INTEGER NOT NULL REFERENCES library_root (id) ON DELETE CASCADE,
    rel_path      TEXT    NOT NULL,
    parent_id     INTEGER REFERENCES folder (id) ON DELETE CASCADE,
    content_hash  TEXT,
    file_count    INTEGER NOT NULL DEFAULT 0,
    own_size      INTEGER NOT NULL DEFAULT 0,
    total_size    INTEGER NOT NULL DEFAULT 0,
    UNIQUE (root_id, rel_path)
);
CREATE INDEX folder_parent ON folder (parent_id);

-- Rekursiv aggregiert.
CREATE TABLE folder_size_by_kind (
    folder_id   INTEGER NOT NULL REFERENCES folder (id) ON DELETE CASCADE,
    kind        TEXT    NOT NULL,
    file_count  INTEGER NOT NULL DEFAULT 0,
    total_size  INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (folder_id, kind)
);

CREATE TABLE run (
    id          INTEGER PRIMARY KEY,
    kind        TEXT    NOT NULL,
    created_at  TEXT    NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    status      TEXT    NOT NULL DEFAULT 'planned'
                  CHECK (status IN ('planned', 'running', 'done', 'aborted', 'undone')),
    dry_run     INTEGER NOT NULL DEFAULT 1 CHECK (dry_run IN (0, 1))
);

CREATE TABLE operation (
    id      INTEGER PRIMARY KEY,
    run_id  INTEGER NOT NULL REFERENCES run (id) ON DELETE CASCADE,
    seq     INTEGER NOT NULL,
    op      TEXT    NOT NULL CHECK (op IN ('move', 'copy', 'mkdir', 'trash')),
    src     TEXT,
    dst     TEXT,
    hash    TEXT,
    status  TEXT    NOT NULL DEFAULT 'planned'
              CHECK (status IN ('planned', 'done', 'failed', 'undone')),
    error   TEXT,
    UNIQUE (run_id, seq)
);
