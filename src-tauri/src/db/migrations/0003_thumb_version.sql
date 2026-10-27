-- Version des Thumbnail-Erzeugers. Fehlschläge und "kein Vorschaubild" älterer
-- Versionen werden erneut versucht, sobald der Erzeuger verbessert wurde.
ALTER TABLE thumb ADD COLUMN version INTEGER NOT NULL DEFAULT 1;
