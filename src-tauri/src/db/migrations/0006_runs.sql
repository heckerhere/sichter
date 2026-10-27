-- Phase 6: Planer und Journal.

ALTER TABLE run ADD COLUMN title TEXT NOT NULL DEFAULT '';
ALTER TABLE run ADD COLUMN target_root_id INTEGER;
-- Planungsparameter und Ergebnis (Konflikte, Zähler) als JSON.
ALTER TABLE run ADD COLUMN params TEXT NOT NULL DEFAULT '{}';
ALTER TABLE run ADD COLUMN summary TEXT NOT NULL DEFAULT '{}';
ALTER TABLE run ADD COLUMN finished_at TEXT;
ALTER TABLE run ADD COLUMN log_path TEXT;

-- Bezug zum Index (bewusst ohne Fremdschlüssel: Nach dem Papierkorb ist das Asset weg,
-- die Operation bleibt als Protokoll erhalten).
ALTER TABLE operation ADD COLUMN asset_id INTEGER;
ALTER TABLE operation ADD COLUMN group_id INTEGER;
ALTER TABLE operation ADD COLUMN size INTEGER;
-- Bei Duplikaten: Pfad der Kopie, die bleibt. Wird vor dem Papierkorb erneut geprüft.
ALTER TABLE operation ADD COLUMN keep_path TEXT;
ALTER TABLE operation ADD COLUMN note TEXT;

CREATE INDEX operation_run ON operation (run_id, seq);
CREATE INDEX run_status ON run (status);
