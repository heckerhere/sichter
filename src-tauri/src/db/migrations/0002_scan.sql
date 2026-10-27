-- Phase 1: Felder für Scan, Paarbildung und Thumbnails.

-- Apple ContentIdentifier: verbindet Live-Photo-Bild und -Video auch bei
-- unterschiedlichen Dateinamen.
ALTER TABLE meta ADD COLUMN content_id TEXT;

-- 1, wenn taken_at aus QuickTime:CreateDate ohne Zeitzone stammt. Laut
-- Spezifikation UTC, viele Kameras schreiben aber Ortszeit. Korrektur pro
-- Kameramodell folgt in Phase 4.
ALTER TABLE meta ADD COLUMN quicktime_utc INTEGER NOT NULL DEFAULT 0;

-- Endung und Magic Bytes widersprechen sich (z. B. HEIC mit Endung .jpg).
ALTER TABLE asset ADD COLUMN ext_mismatch INTEGER NOT NULL DEFAULT 0;

-- Metadaten wurden für den aktuellen Stand (size, mtime) gelesen.
ALTER TABLE asset ADD COLUMN meta_done INTEGER NOT NULL DEFAULT 0;

CREATE INDEX asset_root_path ON asset (root_id, rel_path);

-- Thumbnail-Status pro Inhalt. Fehlschläge werden gemerkt, damit nicht jeder
-- Scan es erneut versucht.
CREATE TABLE thumb (
    hash    TEXT PRIMARY KEY,
    status  TEXT NOT NULL CHECK (status IN ('ok', 'failed', 'none')),
    error   TEXT
);
