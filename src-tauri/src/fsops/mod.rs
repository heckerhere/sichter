//! EINZIGE Stelle mit Schreibzugriff auf das Dateisystem (PLAN.md, Abschnitt 5.3).
//!
//! Kein anderes Modul ruft `std::fs` schreibend auf; `tests/fsops_guard.rs`
//! erzwingt das. Regeln für Operationen im Foto-Bestand:
//!
//! * Jede Operation verlangt eine [`OperationId`], die nur das Journal vergibt.
//! * Es wird nie überschrieben. Existiert das Ziel, schlägt die Operation fehl.
//! * Verschieben auf demselben Volume ist ein atomares No-Replace-Rename. Über
//!   Volume-Grenzen wird kopiert, die Kopie per BLAKE3 geprüft und erst dann
//!   die Quelle entfernt.
//! * Löschen gibt es nur als "In den Papierkorb", mit [`TrashToken`].
//! * Pfade in gesperrten Roots werden abgewiesen ([`PathGuard`]).
//!
//! App-interne Dateien (Datenbank-Verzeichnis, Thumbnail-Cache, Logs) laufen
//! über [`appdata::AppDataDir`] und sind auf das App-Datenverzeichnis begrenzt.

pub mod appdata;
mod rename;

use std::fs::{self, File};
use std::io;
use std::path::{Component, Path, PathBuf};

use crate::hash::{self, FileHash};

#[derive(Debug, thiserror::Error)]
pub enum FsOpError {
    #[error("Quelle fehlt oder ist keine reguläre Datei: {0}")]
    SourceMissing(PathBuf),
    #[error("Quelle hat sich seit dem Scan verändert: {0}")]
    SourceChanged(PathBuf),
    #[error("Ziel existiert bereits: {0}")]
    DestinationExists(PathBuf),
    #[error("Zielordner existiert nicht: {0}")]
    MissingParent(PathBuf),
    #[error("Pfad liegt in einem gesperrten Ordner: {0}")]
    Locked(PathBuf),
    #[error("Kopie stimmt nicht mit der Quelle überein, Quelle bleibt unverändert: {0}")]
    CopyVerifyFailed(PathBuf),
    #[error("Pfad ist kein Ordner: {0}")]
    NotADirectory(PathBuf),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub type Result<T> = std::result::Result<T, FsOpError>;

/// Nachweis, dass eine Operation im Journal als "geplant" steht.
///
/// Nur das Journal erzeugt Werte dieses Typs (Phase 6). Ohne Journal-Eintrag
/// ist keine Ausführung möglich.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationId(i64);

impl OperationId {
    #[allow(dead_code)] // Aufrufer ist das Journal (Phase 6).
    pub(crate) fn from_journal(id: i64) -> Self {
        Self(id)
    }

    pub fn get(self) -> i64 {
        self.0
    }
}

/// Nachweis, dass der Nutzer eine Papierkorb-Operation in einem Plan
/// ausdrücklich bestätigt hat. Wird nur vom Planer ausgestellt (Phase 6).
#[derive(Debug)]
pub struct TrashToken {
    _private: (),
}

impl TrashToken {
    #[allow(dead_code)] // Aufrufer ist der Planer (Phase 6).
    pub(crate) fn from_confirmed_plan() -> Self {
        Self { _private: () }
    }
}

/// Weist Pfade in gesperrten Roots ab.
///
/// Der Vergleich ist auf macOS und Windows unabhängig von Groß-/Kleinschreibung,
/// weil die Standard-Dateisysteme dort so arbeiten. Im Zweifel wird also eher
/// zu viel als zu wenig gesperrt.
#[derive(Debug, Default, Clone)]
pub struct PathGuard {
    locked: Vec<Vec<String>>,
}

impl PathGuard {
    pub fn new<I, P>(locked_roots: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        Self {
            locked: locked_roots
                .into_iter()
                .map(|p| comparable_components(&normalize(p.as_ref())))
                .collect(),
        }
    }

    pub fn check(&self, path: &Path) -> Result<()> {
        let candidate = comparable_components(&normalize(path));
        if self.locked.iter().any(|root| candidate.starts_with(root)) {
            return Err(FsOpError::Locked(path.to_path_buf()));
        }
        Ok(())
    }
}

/// Löst Symlinks auf, soweit der Pfad existiert. Bei nicht existierenden Zielen
/// wird der nächste existierende Vorfahr kanonisiert und der Rest angehängt.
fn normalize(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut rest = Vec::new();
    loop {
        if let Ok(canonical) = fs::canonicalize(existing) {
            return rest.iter().rev().fold(canonical, |acc, part| acc.join(part));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn comparable_components(path: &Path) -> Vec<String> {
    path.components()
        .filter(|c| !matches!(c, Component::CurDir))
        .map(|c| {
            let s = c.as_os_str().to_string_lossy().into_owned();
            if cfg!(any(target_os = "macos", windows)) {
                s.to_lowercase()
            } else {
                s
            }
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveKind {
    /// Atomares Rename auf demselben Volume.
    Renamed,
    /// Kopiert, verifiziert, Quelle entfernt.
    CopiedAcrossVolumes,
}

/// Verschiebt `src` nach `dst`, ohne zu überschreiben.
///
/// Prüft vorher: Quelle existiert und hat den erwarteten Hash, Ziel existiert
/// nicht, Zielordner existiert, keiner der Pfade liegt in einem gesperrten Root.
pub fn move_verified(
    _op: OperationId,
    src: &Path,
    dst: &Path,
    expected: &FileHash,
    guard: &PathGuard,
) -> Result<MoveKind> {
    preflight(src, dst, expected, guard)?;
    match rename::rename_noreplace(src, dst) {
        Ok(()) => Ok(MoveKind::Renamed),
        Err(rename::RenameError::AlreadyExists) => Err(FsOpError::DestinationExists(dst.into())),
        Err(rename::RenameError::CrossDevice | rename::RenameError::Unsupported) => {
            // Auch wenn das Dateisystem kein No-Replace-Rename kann: Kopieren mit
            // create_new ist atomar gegen Überschreiben, deshalb dieser Weg.
            copy_and_verify(src, dst, expected)?;
            fs::remove_file(src)?;
            Ok(MoveKind::CopiedAcrossVolumes)
        }
        Err(rename::RenameError::Io(e)) => Err(e.into()),
    }
}

/// Kopiert `src` nach `dst`, ohne zu überschreiben, und prüft die Kopie per Hash.
/// Die Quelle bleibt unberührt.
pub fn copy_verified(
    _op: OperationId,
    src: &Path,
    dst: &Path,
    expected: &FileHash,
    guard: &PathGuard,
) -> Result<()> {
    preflight(src, dst, expected, guard)?;
    copy_and_verify(src, dst, expected)
}

/// Legt einen einzelnen Ordner an (nicht rekursiv). Gibt `false` zurück, wenn
/// er schon existiert.
pub fn mkdir(_op: OperationId, path: &Path, guard: &PathGuard) -> Result<bool> {
    guard.check(path)?;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => return Ok(false),
        Ok(_) => return Err(FsOpError::NotADirectory(path.into())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    match path.parent() {
        Some(parent) if parent.is_dir() => {}
        _ => return Err(FsOpError::MissingParent(path.into())),
    }
    fs::create_dir(path)?;
    Ok(true)
}

/// Legt `path` in den System-Papierkorb. Endgültiges Löschen gibt es nicht.
pub fn trash(_op: OperationId, path: &Path, _token: &TrashToken, guard: &PathGuard) -> Result<()> {
    guard.check(path)?;
    if fs::symlink_metadata(path).is_err() {
        return Err(FsOpError::SourceMissing(path.into()));
    }
    trash::delete(path).map_err(|e| FsOpError::Io(io::Error::other(e.to_string())))
}

fn preflight(src: &Path, dst: &Path, expected: &FileHash, guard: &PathGuard) -> Result<()> {
    guard.check(src)?;
    guard.check(dst)?;
    match fs::symlink_metadata(src) {
        Ok(meta) if meta.is_file() => {}
        _ => return Err(FsOpError::SourceMissing(src.into())),
    }
    if fs::symlink_metadata(dst).is_ok() {
        return Err(FsOpError::DestinationExists(dst.into()));
    }
    match dst.parent() {
        Some(parent) if parent.is_dir() => {}
        _ => return Err(FsOpError::MissingParent(dst.into())),
    }
    if hash::hash_file(src)? != *expected {
        return Err(FsOpError::SourceChanged(src.into()));
    }
    Ok(())
}

fn copy_and_verify(src: &Path, dst: &Path, expected: &FileHash) -> Result<()> {
    let result = copy_no_clobber(src, dst).and_then(|()| {
        if hash::hash_file(dst)? == *expected {
            Ok(())
        } else {
            Err(FsOpError::CopyVerifyFailed(dst.into()))
        }
    });
    if let Err(err) = &result {
        // Eine eigene, unvollständige oder fehlerhafte Kopie wieder entfernen.
        // Existierte das Ziel schon vorher, gehört es nicht uns: liegen lassen.
        if !matches!(err, FsOpError::DestinationExists(_)) {
            let _ = fs::remove_file(dst);
        }
    }
    result
}

fn copy_no_clobber(src: &Path, dst: &Path) -> Result<()> {
    let mut input = File::open(src)?;
    let modified = input.metadata()?.modified()?;
    let mut output = match File::options().write(true).create_new(true).open(dst) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            return Err(FsOpError::DestinationExists(dst.into()));
        }
        Err(e) => return Err(e.into()),
    };
    io::copy(&mut input, &mut output)?;
    #[cfg(test)]
    test_support::maybe_corrupt(&mut output)?;
    output.sync_all()?;
    output.set_modified(modified)?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Hilfen für Tests in anderen Modulen, damit auch Testcode nur hier schreibt.

    use std::cell::Cell;
    use std::io::{self, Seek, SeekFrom, Write};
    use std::path::Path;

    thread_local! {
        static CORRUPT_NEXT_COPY: Cell<bool> = const { Cell::new(false) };
    }

    pub fn write(path: &Path, bytes: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, bytes).unwrap();
    }

    #[cfg(unix)]
    pub fn link_dir(target: &Path, link: &Path) {
        std::os::unix::fs::symlink(target, link).unwrap();
    }

    /// Lässt die nächste Kopie im aktuellen Thread fehlerhaft enden.
    pub fn corrupt_next_copy() {
        CORRUPT_NEXT_COPY.with(|c| c.set(true));
    }

    pub(super) fn maybe_corrupt(file: &mut std::fs::File) -> io::Result<()> {
        if CORRUPT_NEXT_COPY.with(|c| c.replace(false)) {
            file.seek(SeekFrom::Start(0))?;
            file.write_all(b"X")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::{hash_bytes, hash_file};

    fn op() -> OperationId {
        OperationId::from_journal(1)
    }

    fn setup() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("quelle").join("IMG_0001.JPG");
        test_support::write(&src, b"jpeg-daten");
        std::fs::create_dir(dir.path().join("ziel")).unwrap();
        (dir, src)
    }

    #[test]
    fn moves_within_volume() {
        let (dir, src) = setup();
        let dst = dir.path().join("ziel").join("IMG_0001.JPG");
        let h = hash_bytes(b"jpeg-daten");
        let kind = move_verified(op(), &src, &dst, &h, &PathGuard::default()).unwrap();
        assert_eq!(kind, MoveKind::Renamed);
        assert!(!src.exists());
        assert_eq!(hash_file(&dst).unwrap(), h);
    }

    #[test]
    fn move_never_overwrites() {
        let (dir, src) = setup();
        let dst = dir.path().join("ziel").join("IMG_0001.JPG");
        test_support::write(&dst, b"anderer inhalt");
        let err = move_verified(op(), &src, &dst, &hash_bytes(b"jpeg-daten"), &PathGuard::default())
            .unwrap_err();
        assert!(matches!(err, FsOpError::DestinationExists(_)));
        assert!(src.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), b"anderer inhalt");
    }

    #[test]
    fn move_rejects_changed_source() {
        let (dir, src) = setup();
        let dst = dir.path().join("ziel").join("IMG_0001.JPG");
        let err = move_verified(op(), &src, &dst, &hash_bytes(b"alt"), &PathGuard::default())
            .unwrap_err();
        assert!(matches!(err, FsOpError::SourceChanged(_)));
        assert!(src.exists() && !dst.exists());
    }

    #[test]
    fn move_requires_existing_parent() {
        let (dir, src) = setup();
        let dst = dir.path().join("fehlt").join("IMG_0001.JPG");
        let err = move_verified(op(), &src, &dst, &hash_bytes(b"jpeg-daten"), &PathGuard::default())
            .unwrap_err();
        assert!(matches!(err, FsOpError::MissingParent(_)));
        assert!(src.exists());
    }

    #[test]
    fn locked_roots_are_never_touched() {
        let (dir, src) = setup();
        let dst = dir.path().join("ziel").join("IMG_0001.JPG");
        let h = hash_bytes(b"jpeg-daten");

        let lock_src = PathGuard::new([dir.path().join("quelle")]);
        assert!(matches!(
            move_verified(op(), &src, &dst, &h, &lock_src),
            Err(FsOpError::Locked(_))
        ));
        let lock_dst = PathGuard::new([dir.path().join("ziel")]);
        assert!(matches!(
            copy_verified(op(), &src, &dst, &h, &lock_dst),
            Err(FsOpError::Locked(_))
        ));
        assert!(matches!(
            mkdir(op(), &dir.path().join("ziel").join("neu"), &lock_dst),
            Err(FsOpError::Locked(_))
        ));
        assert!(src.exists() && !dst.exists());
    }

    #[test]
    fn guard_ignores_prefix_lookalikes() {
        let dir = tempfile::tempdir().unwrap();
        let guard = PathGuard::new([dir.path().join("Fotos")]);
        assert!(guard.check(&dir.path().join("Fotos2").join("a.jpg")).is_ok());
        assert!(guard.check(&dir.path().join("Fotos").join("a.jpg")).is_err());
    }

    #[cfg(any(target_os = "macos", windows))]
    #[test]
    fn guard_is_case_insensitive() {
        let dir = tempfile::tempdir().unwrap();
        let guard = PathGuard::new([dir.path().join("Fotos")]);
        assert!(guard.check(&dir.path().join("FOTOS").join("a.jpg")).is_err());
    }

    #[test]
    fn copy_keeps_source_and_mtime() {
        let (dir, src) = setup();
        let dst = dir.path().join("ziel").join("IMG_0001.JPG");
        copy_verified(op(), &src, &dst, &hash_bytes(b"jpeg-daten"), &PathGuard::default()).unwrap();
        assert!(src.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), b"jpeg-daten");
        let m = |p: &Path| std::fs::metadata(p).unwrap().modified().unwrap();
        assert_eq!(m(&src), m(&dst));
    }

    #[test]
    fn failed_copy_verification_keeps_source_and_removes_copy() {
        let (dir, src) = setup();
        let dst = dir.path().join("ziel").join("IMG_0001.JPG");
        test_support::corrupt_next_copy();
        let err = copy_and_verify(&src, &dst, &hash_bytes(b"jpeg-daten")).unwrap_err();
        assert!(matches!(err, FsOpError::CopyVerifyFailed(_)));
        assert!(src.exists());
        assert!(!dst.exists());
    }

    #[test]
    fn mkdir_is_not_recursive_and_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let guard = PathGuard::default();
        let deep = dir.path().join("a").join("b");
        assert!(matches!(mkdir(op(), &deep, &guard), Err(FsOpError::MissingParent(_))));
        assert!(mkdir(op(), &dir.path().join("a"), &guard).unwrap());
        assert!(!mkdir(op(), &dir.path().join("a"), &guard).unwrap());
    }

    #[test]
    fn unicode_paths() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("Grüße aus Köln").join("Bild ä.jpg");
        test_support::write(&src, b"x");
        let dst = dir.path().join("Ölmühle – Ausflug.jpg");
        move_verified(op(), &src, &dst, &hash_bytes(b"x"), &PathGuard::default()).unwrap();
        assert!(dst.exists() && !src.exists());
    }
}
