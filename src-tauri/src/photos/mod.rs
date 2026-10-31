//! Import aus der Apple-Fotos-Mediathek (nur macOS, über PhotoKit).
//!
//! Die Mediathek wird nur gelesen. Sichter kopiert die **Originale** (inklusive
//! RAW-Alternativen und Live-Photo-Videos) über einen geplanten, bestätigten
//! Lauf in einen normalen Ordner; Bearbeitungen aus Fotos bleiben in Fotos.
//! Der direkte Zugriff auf das `.photoslibrary`-Paket ist durch macOS
//! geschützt und wäre ohnehin nicht stabil (UUID-Dateinamen, Optimierung für
//! iCloud). PhotoKit liefert Originalnamen und lädt fehlende Originale aus
//! iCloud nach.
//!
//! Geschrieben wird ausschließlich über [`crate::fsops::export_new_file`]: Die
//! Mediathek schreibt in eine von fsops gewählte, neue Temp-Datei, fsops
//! benennt sie ohne Überschreiben um.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
mod photokit;

/// Zugriffsstatus auf die Mediathek (entspricht `PHAuthorizationStatus`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    NotDetermined,
    Restricted,
    Denied,
    Authorized,
    /// Nur ausgewählte Fotos freigegeben.
    Limited,
    /// Plattform ohne Fotos-Mediathek.
    Unsupported,
}

impl Access {
    pub fn can_read(self) -> bool {
        matches!(self, Access::Authorized | Access::Limited)
    }
}

/// Welche Originaldatei eines Fotos-Objekts (Teilmenge von `PHAssetResourceType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Photo,
    Video,
    /// Zweites Original, z. B. das RAW eines RAW+JPEG-Paars.
    AlternatePhoto,
    /// Video eines Live Photos.
    PairedVideo,
}

#[derive(Debug, Clone)]
pub struct Resource {
    pub kind: ResourceKind,
    pub original_name: String,
    /// Größe in Bytes, soweit PhotoKit sie verrät.
    pub size: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Asset {
    /// `localIdentifier`, stabil innerhalb einer Mediathek.
    pub id: String,
    /// Aufnahmezeitpunkt (Unix-Sekunden, UTC).
    pub created: Option<i64>,
    pub favorite: bool,
    /// Originale in der Reihenfolge Hauptdatei, Alternativen, Live-Video.
    pub resources: Vec<Resource>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct ListOptions {
    pub include_hidden: bool,
}

/// Lesender Zugriff auf eine Foto-Mediathek. In Tests durch eine Attrappe ersetzt.
pub trait PhotoLibrary: Send + Sync {
    fn assets(&self, opts: &ListOptions) -> Result<Vec<Asset>, String>;
    /// Schreibt eine Originaldatei in die (noch nicht existierende) Datei `to`.
    /// `allow_network`: fehlende Originale aus iCloud laden.
    fn export(&self, asset_id: &str, kind: ResourceKind, original_name: &str, to: &Path, allow_network: bool) -> Result<(), String>;
}

/// Die Fotos-Mediathek des Systems, falls die Plattform eine hat.
pub fn system() -> Option<&'static dyn PhotoLibrary> {
    #[cfg(target_os = "macos")]
    {
        Some(&photokit::PhotoKit)
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

pub fn access() -> Access {
    #[cfg(target_os = "macos")]
    {
        photokit::access()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Access::Unsupported
    }
}

/// Fragt (falls noch nicht geschehen) nach Zugriff und wartet auf die Antwort.
pub fn request_access() -> Access {
    #[cfg(target_os = "macos")]
    {
        photokit::request_access()
    }
    #[cfg(not(target_os = "macos"))]
    {
        Access::Unsupported
    }
}
