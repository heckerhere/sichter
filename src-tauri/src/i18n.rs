//! Sprache für Meldungen und vom Backend erzeugte Namen (Deutsch/Englisch).
//!
//! Die Oberfläche teilt die gewählte Sprache beim Start und bei jeder Änderung
//! mit ([`set_english`]). Standard ist Deutsch, damit bestehende Tests und
//! Protokolle unverändert bleiben.
//!
//! Verwendung: `tr("Quelle fehlt", "Source missing")` für feste Texte,
//! `trf!("Datei {}", "File {}", name)` für Texte mit Werten.

use std::sync::atomic::{AtomicBool, Ordering};

static ENGLISH: AtomicBool = AtomicBool::new(false);

pub fn set_english(on: bool) {
    ENGLISH.store(on, Ordering::Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Ordering::Relaxed)
}

/// Wählt den Text der aktuellen Sprache.
pub fn tr(de: &'static str, en: &'static str) -> &'static str {
    if is_english() { en } else { de }
}

/// Wie [`tr`], mit Formatierung: `trf!("{} Dateien", "{} files", n)`.
#[macro_export]
macro_rules! trf {
    ($de:literal, $en:literal $(, $arg:expr)* $(,)?) => {
        if $crate::i18n::is_english() { format!($en $(, $arg)*) } else { format!($de $(, $arg)*) }
    };
}

/// Ordnernamen, die Sichter selbst anlegt. Erkannt werden immer beide Sprachen.
pub mod dirs {
    use super::tr;

    pub fn unsorted() -> &'static str {
        tr("_Unsortiert", "_Unsorted")
    }
    pub fn no_date() -> &'static str {
        tr("_Ohne Datum", "_No date")
    }
    pub fn rejects() -> &'static str {
        tr("_Aussortiert", "_Rejected")
    }
    pub const REJECTS_ALL: &[&str] = &["_Aussortiert", "_Rejected"];
    pub fn duplicates() -> &'static str {
        tr("_Duplikate", "_Duplicates")
    }
    /// Import aus der Mediathek: Objekte ohne Aufnahmedatum.
    pub fn import_no_date() -> &'static str {
        tr("Ohne Datum", "No date")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_language() {
        // Andere Tests laufen parallel mit Deutsch: nur lesen, nicht umschalten.
        assert_eq!(tr("a", "b"), if is_english() { "b" } else { "a" });
        assert_eq!(trf!("{} Dateien", "{} files", 3), if is_english() { "3 files" } else { "3 Dateien" });
    }
}
