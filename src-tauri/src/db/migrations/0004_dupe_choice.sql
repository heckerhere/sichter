-- Vom Nutzer gewählte Kopie, die bei einer Duplikatgruppe bleiben soll.
-- Wird erst vom Planer (Phase 6) verwendet und nie automatisch gesetzt.
CREATE TABLE dupe_choice (
    hash           TEXT PRIMARY KEY,
    keep_asset_id  INTEGER NOT NULL REFERENCES asset (id) ON DELETE CASCADE,
    updated_at     TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
