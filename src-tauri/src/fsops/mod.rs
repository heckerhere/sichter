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
    #[error("Nur XMP-Sidecars dürfen geschrieben werden: {0}")]
    NotASidecar(PathBuf),
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
    /// Nur das Journal ruft das auf (per Guard-Test geprüft).
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
    /// Nur das Journal ruft das auf, für bestätigte Papierkorb-Pläne und Undo
    /// von Kopien (per Guard-Test geprüft).
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
    #[cfg(test)]
    let renamed = if test_support::force_cross_device() { Err(rename::RenameError::CrossDevice) } else { rename::rename_noreplace(src, dst) };
    #[cfg(not(test))]
    let renamed = rename::rename_noreplace(src, dst);
    match renamed {
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

/// Schließt ein Verschieben über Volume-Grenzen ab, das nach der Kopie und vor
/// dem Entfernen der Quelle unterbrochen wurde (Absturz, Stromausfall). Die
/// Quelle wird nur entfernt, wenn Quelle und Ziel beide nachweislich den
/// erwarteten Inhalt haben.
pub fn finish_interrupted_move(_op: OperationId, src: &Path, dst: &Path, expected: &FileHash, guard: &PathGuard) -> Result<()> {
    guard.check(src)?;
    guard.check(dst)?;
    match fs::symlink_metadata(src) {
        Ok(meta) if meta.is_file() => {}
        _ => return Err(FsOpError::SourceMissing(src.into())),
    }
    if hash::hash_file(dst)? != *expected {
        return Err(FsOpError::CopyVerifyFailed(dst.into()));
    }
    if hash::hash_file(src)? != *expected {
        return Err(FsOpError::SourceChanged(src.into()));
    }
    fs::remove_file(src)?;
    Ok(())
}

fn is_xmp(path: &Path) -> bool {
    path.extension().is_some_and(|e| e.eq_ignore_ascii_case("xmp"))
}

/// Schreibt ein XMP-Sidecar (XMP-Export, PLAN.md 6.3). Die einzige Funktion, die
/// eine bestehende Datei ersetzt, und nur unter diesen Bedingungen:
/// * Endung `.xmp`, nie eine Bilddatei.
/// * `previous = None`: Die Datei darf noch nicht existieren (wird neu angelegt).
/// * `previous = Some(h)`: Die Datei muss genau den Inhalt `h` haben (Stand der
///   Planung, vorher im Journal gesichert); dann atomar per Temp-Datei ersetzt.
pub fn write_sidecar(_op: OperationId, path: &Path, bytes: &[u8], previous: Option<&FileHash>, guard: &PathGuard) -> Result<()> {
    guard.check(path)?;
    if !is_xmp(path) {
        return Err(FsOpError::NotASidecar(path.into()));
    }
    match previous {
        None => {
            let mut file = match File::options().write(true).create_new(true).open(path) {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => return Err(FsOpError::DestinationExists(path.into())),
                Err(e) => return Err(e.into()),
            };
            io::Write::write_all(&mut file, bytes)?;
            file.sync_all()?;
        }
        Some(expected) => {
            if hash::hash_file(path)? != *expected {
                return Err(FsOpError::SourceChanged(path.into()));
            }
            let mut tmp_name = std::ffi::OsString::from(".");
            tmp_name.push(path.file_name().unwrap_or_default());
            tmp_name.push(".sichter-tmp");
            let tmp = path.with_file_name(tmp_name);
            let mut file = File::options().write(true).create_new(true).open(&tmp)?;
            let written = io::Write::write_all(&mut file, bytes).and_then(|()| file.sync_all());
            if let Err(e) = written {
                let _ = fs::remove_file(&tmp);
                return Err(e.into());
            }
            fs::rename(&tmp, path)?;
        }
    }
    Ok(())
}

/// Entfernt ein XMP-Sidecar, das Sichter selbst angelegt hat (Undo des
/// XMP-Exports). Nur, wenn der Inhalt noch genau dem Geschriebenen entspricht.
pub fn remove_own_sidecar(_op: OperationId, path: &Path, written: &FileHash, guard: &PathGuard) -> Result<()> {
    guard.check(path)?;
    if !is_xmp(path) {
        return Err(FsOpError::NotASidecar(path.into()));
    }
    if hash::hash_file(path)? != *written {
        return Err(FsOpError::SourceChanged(path.into()));
    }
    fs::remove_file(path)?;
    Ok(())
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

/// Entfernt einen Ordner nur, wenn er wirklich leer ist (für das Rückgängigmachen
/// selbst angelegter Ordner). `false`, wenn er nicht leer ist oder fehlt.
/// Systemdateien wie `.DS_Store` zählen hier als Inhalt: Was nicht leer ist,
/// bleibt stehen.
pub fn rmdir_if_empty(_op: OperationId, path: &Path, guard: &PathGuard) -> Result<bool> {
    guard.check(path)?;
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => return Err(FsOpError::NotADirectory(path.into())),
        Err(_) => return Ok(false),
    }
    if fs::read_dir(path)?.next().is_some() {
        return Ok(false);
    }
    // remove_dir schlägt ohnehin fehl, falls inzwischen etwas hineingekommen ist.
    match fs::remove_dir(path) {
        Ok(()) => Ok(true),
        Err(_) => Ok(false),
    }
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
        static FORCE_CROSS_DEVICE: Cell<bool> = const { Cell::new(false) };
    }

    /// Lässt alle Verschiebungen im aktuellen Thread den Cross-Volume-Weg nehmen.
    pub fn set_force_cross_device(on: bool) {
        FORCE_CROSS_DEVICE.with(|c| c.set(on));
    }

    pub(super) fn force_cross_device() -> bool {
        FORCE_CROSS_DEVICE.with(Cell::get)
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
    fn cross_volume_move_copies_verifies_and_removes_source() {
        let (dir, src) = setup();
        let dst = dir.path().join("ziel").join("IMG_0001.JPG");
        test_support::set_force_cross_device(true);
        let kind = move_verified(op(), &src, &dst, &hash_bytes(b"jpeg-daten"), &PathGuard::default());
        test_support::set_force_cross_device(false);
        assert_eq!(kind.unwrap(), MoveKind::CopiedAcrossVolumes);
        assert!(!src.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), b"jpeg-daten");
    }

    #[test]
    fn cross_volume_move_with_bad_copy_keeps_source() {
        let (dir, src) = setup();
        let dst = dir.path().join("ziel").join("IMG_0001.JPG");
        test_support::set_force_cross_device(true);
        test_support::corrupt_next_copy();
        let err = move_verified(op(), &src, &dst, &hash_bytes(b"jpeg-daten"), &PathGuard::default()).unwrap_err();
        test_support::set_force_cross_device(false);
        assert!(matches!(err, FsOpError::CopyVerifyFailed(_)));
        assert_eq!(std::fs::read(&src).unwrap(), b"jpeg-daten", "Quelle unverändert");
        assert!(!dst.exists(), "fehlerhafte Kopie entfernt");
    }

    #[test]
    fn finishing_interrupted_move_requires_identical_copy() {
        let (dir, src) = setup();
        let dst = dir.path().join("ziel").join("IMG_0001.JPG");
        let h = hash_bytes(b"jpeg-daten");
        test_support::write(&dst, b"halbe kopie");
        assert!(matches!(finish_interrupted_move(op(), &src, &dst, &h, &PathGuard::default()), Err(FsOpError::CopyVerifyFailed(_))));
        assert!(src.exists(), "Quelle bleibt bei abweichender Kopie");
        test_support::write(&dst, b"jpeg-daten");
        finish_interrupted_move(op(), &src, &dst, &h, &PathGuard::default()).unwrap();
        assert!(!src.exists() && dst.exists());
    }

    #[test]
    fn sidecar_writes_are_restricted() {
        let dir = tempfile::tempdir().unwrap();
        let g = PathGuard::default();
        let xmp = dir.path().join("IMG_1.xmp");
        let jpg = dir.path().join("IMG_1.jpg");
        // Nur .xmp, nie Bilddateien.
        assert!(matches!(write_sidecar(op(), &jpg, b"x", None, &g), Err(FsOpError::NotASidecar(_))));
        write_sidecar(op(), &xmp, b"v1", None, &g).unwrap();
        // Neu anlegen überschreibt nie.
        assert!(matches!(write_sidecar(op(), &xmp, b"v2", None, &g), Err(FsOpError::DestinationExists(_))));
        // Ersetzen nur bei erwartetem Inhalt.
        assert!(matches!(write_sidecar(op(), &xmp, b"v2", Some(&hash_bytes(b"anders")), &g), Err(FsOpError::SourceChanged(_))));
        write_sidecar(op(), &xmp, b"v2", Some(&hash_bytes(b"v1")), &g).unwrap();
        assert_eq!(std::fs::read(&xmp).unwrap(), b"v2");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1, "keine Temp-Datei übrig");
        // Entfernen nur des eigenen Inhalts.
        assert!(remove_own_sidecar(op(), &xmp, &hash_bytes(b"v1"), &g).is_err());
        remove_own_sidecar(op(), &xmp, &hash_bytes(b"v2"), &g).unwrap();
        assert!(!xmp.exists());
        assert!(matches!(write_sidecar(op(), &xmp, b"v1", None, &PathGuard::new([dir.path()])), Err(FsOpError::Locked(_))));
    }

    #[test]
    fn rmdir_only_removes_empty_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let guard = PathGuard::default();
        let empty = dir.path().join("leer");
        let full = dir.path().join("voll");
        std::fs::create_dir(&empty).unwrap();
        test_support::write(&full.join(".DS_Store"), b"x");
        assert!(rmdir_if_empty(op(), &empty, &guard).unwrap());
        assert!(!empty.exists());
        assert!(!rmdir_if_empty(op(), &full, &guard).unwrap());
        assert!(full.join(".DS_Store").exists());
        assert!(!rmdir_if_empty(op(), &dir.path().join("fehlt"), &guard).unwrap());
        assert!(matches!(rmdir_if_empty(op(), &full, &PathGuard::new([&full])), Err(FsOpError::Locked(_))));
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
