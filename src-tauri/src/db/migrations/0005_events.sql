-- Phase 4: Einstellungen, Video-Zeitzone pro Kamera, Titelvorschlag.

CREATE TABLE setting (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

-- Wie QuickTime-Zeiten ohne Zeitzone zu lesen sind. Fehlt ein Eintrag, gilt
-- der Standard je Hersteller (Handys UTC, Kameras Ortszeit).
CREATE TABLE camera_tz (
    camera  TEXT PRIMARY KEY,
    mode    TEXT NOT NULL CHECK (mode IN ('utc', 'local'))
);

ALTER TABLE event ADD COLUMN suggested_title TEXT NOT NULL DEFAULT '';
CREATE INDEX event_date_start ON event (date_start);
