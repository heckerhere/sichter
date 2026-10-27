//! Atomares Umbenennen, das ein bestehendes Ziel nie überschreibt.
//!
//! `std::fs::rename` überschreibt auf Unix stillschweigend, deshalb pro
//! Plattform der passende Systemaufruf.

use std::io;
use std::path::Path;

#[derive(Debug)]
pub(super) enum RenameError {
    AlreadyExists,
    /// Quelle und Ziel liegen auf verschiedenen Volumes.
    CrossDevice,
    /// Das Dateisystem kann kein No-Replace-Rename (z. B. manche Netzlaufwerke).
    Unsupported,
    Io(io::Error),
}

#[cfg(target_os = "macos")]
pub(super) fn rename_noreplace(src: &Path, dst: &Path) -> Result<(), RenameError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let to_c = |p: &Path| {
        CString::new(p.as_os_str().as_bytes())
            .map_err(|e| RenameError::Io(io::Error::new(io::ErrorKind::InvalidInput, e)))
    };
    let (src_c, dst_c) = (to_c(src)?, to_c(dst)?);
    // SAFETY: beide Zeiger zeigen auf gültige, nullterminierte Strings.
    let rc = unsafe { libc::renamex_np(src_c.as_ptr(), dst_c.as_ptr(), libc::RENAME_EXCL) };
    if rc == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    Err(match err.raw_os_error() {
        Some(libc::EEXIST) => RenameError::AlreadyExists,
        Some(libc::EXDEV) => RenameError::CrossDevice,
        Some(libc::ENOTSUP | libc::EINVAL) => RenameError::Unsupported,
        _ => RenameError::Io(err),
    })
}

/// Andere Unix-Systeme: Hardlink anlegen (schlägt fehl, wenn das Ziel existiert)
/// und danach den alten Namen entfernen. Nur für Entwicklung/CI relevant.
#[cfg(all(unix, not(target_os = "macos")))]
pub(super) fn rename_noreplace(src: &Path, dst: &Path) -> Result<(), RenameError> {
    match std::fs::hard_link(src, dst) {
        Ok(()) => std::fs::remove_file(src).map_err(RenameError::Io),
        Err(e) => Err(match e.raw_os_error() {
            Some(libc::EEXIST) => RenameError::AlreadyExists,
            Some(libc::EXDEV) => RenameError::CrossDevice,
            Some(libc::EPERM | libc::ENOTSUP) => RenameError::Unsupported,
            _ => RenameError::Io(e),
        }),
    }
}

#[cfg(windows)]
pub(super) fn rename_noreplace(src: &Path, dst: &Path) -> Result<(), RenameError> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS, ERROR_NOT_SAME_DEVICE};
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    // Lange Pfade (über MAX_PATH) brauchen bei direkten Win32-Aufrufen das
    // Präfix \\?\ (std::fs macht das selbst, MoveFileExW nicht).
    let wide = |p: &Path| long_path(p).as_os_str().encode_wide().chain(Some(0)).collect::<Vec<u16>>();
    let (src_w, dst_w) = (wide(src), wide(dst));
    // Ohne MOVEFILE_REPLACE_EXISTING und ohne MOVEFILE_COPY_ALLOWED:
    // überschreibt nie und bewegt nicht über Volume-Grenzen.
    // SAFETY: beide Puffer sind nullterminierte UTF-16-Strings.
    let ok = unsafe { MoveFileExW(src_w.as_ptr(), dst_w.as_ptr(), 0) };
    if ok != 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    Err(match err.raw_os_error().map(|c| c as u32) {
        Some(ERROR_ALREADY_EXISTS | ERROR_FILE_EXISTS) => RenameError::AlreadyExists,
        Some(ERROR_NOT_SAME_DEVICE) => RenameError::CrossDevice,
        _ => RenameError::Io(err),
    })
}

/// `C:\a\b` → `\\?\C:\a\b`, `\\server\share\x` → `\\?\UNC\server\share\x`.
/// Nur absolute Pfade ab 240 Zeichen; kürzere bleiben unverändert.
#[cfg(any(windows, test))]
pub(super) fn long_path(p: &Path) -> std::path::PathBuf {
    let s = p.to_string_lossy();
    if s.len() < 240 || s.starts_with(r"\\?\") {
        return p.to_path_buf();
    }
    if let Some(unc) = s.strip_prefix(r"\\") {
        return format!(r"\\?\UNC\{unc}").into();
    }
    if s.as_bytes().get(1) == Some(&b':') {
        return format!(r"\\?\{s}").into();
    }
    p.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fsops::test_support::write;

    #[test]
    fn refuses_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        write(&a, b"a");
        write(&b, b"b");
        assert!(matches!(rename_noreplace(&a, &b), Err(RenameError::AlreadyExists)));
        assert_eq!(std::fs::read(&b).unwrap(), b"b");
        assert!(a.exists());
    }

    #[test]
    fn long_windows_paths_get_verbatim_prefix() {
        let long = format!(r"C:\Fotos\{}", "a".repeat(250));
        assert!(long_path(Path::new(&long)).to_string_lossy().starts_with(r"\\?\C:\Fotos"));
        let unc = format!(r"\\nas\fotos\{}", "b".repeat(250));
        assert!(long_path(Path::new(&unc)).to_string_lossy().starts_with(r"\\?\UNC\nas\fotos"));
        assert_eq!(long_path(Path::new(r"C:\kurz")), Path::new(r"C:\kurz"));
    }

    #[test]
    fn renames_when_free() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a"), dir.path().join("b"));
        write(&a, b"a");
        rename_noreplace(&a, &b).unwrap();
        assert!(!a.exists());
        assert_eq!(std::fs::read(&b).unwrap(), b"a");
    }
}
