//! Rekursiver, rein lesender Ordner-Crawl.
//!
//! Folgt keinen Symlinks. Versteckte Dateien, Systemdateien und
//! Mediathek-Pakete anderer Programme werden übersprungen: Dateien darin zu
//! verschieben würde diese Mediatheken beschädigen.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::UNIX_EPOCH;

use walkdir::WalkDir;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// Relativ zum Root, Trennzeichen immer `/`.
    pub rel_path: String,
    pub size: u64,
    pub mtime_ns: i64,
}

#[derive(Debug, Default)]
pub struct CrawlResult {
    pub files: Vec<FileEntry>,
    /// Übersprungene Mediathek-Pakete (relativ), für den Bericht.
    pub skipped_packages: Vec<String>,
    /// Nicht lesbare Einträge: (Pfad, Fehler).
    pub errors: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
#[error("Scan abgebrochen")]
pub struct Cancelled;

const IGNORED_DIRS: &[&str] = &["$RECYCLE.BIN", "System Volume Information", "@eaDir", "#recycle", "#snapshot"];
const IGNORED_FILES: &[&str] = &["Thumbs.db", "desktop.ini", "Icon\r"];
/// Paket-Endungen fremder Mediatheken und Kataloge.
const PACKAGE_EXTENSIONS: &[&str] = &[
    "photoslibrary",
    "photolibrary",
    "migratedphotolibrary",
    "aplibrary",
    "lrdata",
    "cocatalog",
    "cosessiondb",
    "fcpbundle",
    "imovielibrary",
    "tvlibrary",
    "app",
];

pub fn is_package_dir(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| PACKAGE_EXTENSIONS.iter().any(|p| p.eq_ignore_ascii_case(e)))
}

fn is_ignored_name(name: &str, is_dir: bool) -> bool {
    if name.starts_with('.') {
        // Deckt .DS_Store, ._AppleDouble, .Trashes, .Spotlight-V100 usw. ab.
        return true;
    }
    if is_dir {
        IGNORED_DIRS.iter().any(|d| d.eq_ignore_ascii_case(name))
    } else {
        IGNORED_FILES.iter().any(|f| f.eq_ignore_ascii_case(name))
    }
}

/// Wandelt einen Pfad unterhalb von `root` in die gespeicherte Form um.
pub fn to_rel_path(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_str()?),
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// Umkehrung von [`to_rel_path`].
pub fn from_rel_path(root: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(root.to_path_buf(), |acc, part| acc.join(part))
}

pub fn crawl(root: &Path, cancel: &AtomicBool, mut progress: impl FnMut(usize)) -> Result<CrawlResult, Cancelled> {
    let mut result = CrawlResult::default();
    let mut walker = WalkDir::new(root).follow_links(false).into_iter();
    while let Some(entry) = walker.next() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled);
        }
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                let path = err.path().map(|p| p.display().to_string()).unwrap_or_default();
                result.errors.push((path, err.to_string()));
                continue;
            }
        };
        if entry.depth() == 0 {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        let file_type = entry.file_type();
        if file_type.is_dir() {
            if is_ignored_name(&name, true) {
                walker.skip_current_dir();
            } else if is_package_dir(&name) {
                result.skipped_packages.push(to_rel_path(root, entry.path()).unwrap_or_else(|| name.to_string()));
                walker.skip_current_dir();
            }
            continue;
        }
        if !file_type.is_file() || is_ignored_name(&name, false) {
            // Symlinks und Sonderdateien werden nicht indexiert.
            continue;
        }
        let Some(rel_path) = to_rel_path(root, entry.path()) else {
            result.errors.push((entry.path().display().to_string(), "Dateiname ist kein gültiges UTF-8".into()));
            continue;
        };
        match entry.metadata() {
            Ok(meta) => {
                let mtime_ns = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_nanos() as i64);
                result.files.push(FileEntry { rel_path, size: meta.len(), mtime_ns });
                progress(result.files.len());
            }
            Err(err) => result.errors.push((rel_path, err.to_string())),
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::test_support::write;

    #[test]
    fn crawls_and_ignores_system_files() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("a/IMG_1.JPG"), b"1");
        write(&root.join("a/.DS_Store"), b"x");
        write(&root.join("a/._IMG_1.JPG"), b"x");
        write(&root.join("a/Thumbs.db"), b"x");
        write(&root.join(".Trashes/IMG_2.JPG"), b"x");
        write(&root.join("@eaDir/IMG_3.JPG"), b"x");
        write(&root.join("Fotos.photoslibrary/originals/IMG_4.HEIC"), b"x");
        write(&root.join("Grüße/b c.jpg"), b"22");

        let result = crawl(root, &AtomicBool::new(false), |_| {}).unwrap();
        let mut paths: Vec<_> = result.files.iter().map(|f| f.rel_path.as_str()).collect();
        paths.sort();
        assert_eq!(paths.len(), 2, "{paths:?}");
        assert_eq!(paths[0], "Grüße/b c.jpg");
        assert_eq!(paths[1], "a/IMG_1.JPG");
        assert_eq!(result.skipped_packages, vec!["Fotos.photoslibrary".to_string()]);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        write(&outside.path().join("secret.jpg"), b"x");
        crate::fsops::test_support::link_dir(outside.path(), &dir.path().join("link"));
        let result = crawl(dir.path(), &AtomicBool::new(false), |_| {}).unwrap();
        assert!(result.files.is_empty());
    }

    #[test]
    fn rel_path_roundtrip() {
        let root = Path::new("/x/Fotos");
        let path = root.join("2023").join("a b.jpg");
        let rel = to_rel_path(root, &path).unwrap();
        assert_eq!(rel, "2023/a b.jpg");
        assert_eq!(from_rel_path(root, &rel), path);
    }

    #[test]
    fn honours_cancel() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("a.jpg"), b"1");
        assert!(crawl(dir.path(), &AtomicBool::new(true), |_| {}).is_err());
    }
}
