//! Freier Speicherplatz und Volume-Zugehörigkeit (nur lesend).

use std::path::Path;

/// Freier Platz in Bytes auf dem Volume von `path`.
#[cfg(unix)]
pub fn free_bytes(path: &Path) -> Option<u64> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: gültiger, nullterminierter Pfad und ein Zeiger auf eine eigene Struktur.
    if unsafe { libc::statvfs(c.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    // Feldtypen unterscheiden sich je Plattform (u32/u64).
    #[allow(clippy::unnecessary_cast)]
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(windows)]
pub fn free_bytes(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free = 0u64;
    // SAFETY: nullterminierter Pfad, Ausgabezeiger auf eigene Variable.
    let ok = unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
    (ok != 0).then_some(free)
}

/// Liegen beide Pfade auf demselben Volume? `None`, wenn unbekannt.
#[cfg(unix)]
pub fn same_volume(a: &Path, b: &Path) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;
    Some(std::fs::metadata(a).ok()?.dev() == std::fs::metadata(b).ok()?.dev())
}

#[cfg(windows)]
pub fn same_volume(a: &Path, b: &Path) -> Option<bool> {
    let root = |p: &Path| p.components().next().map(|c| c.as_os_str().to_ascii_lowercase());
    Some(root(a)? == root(b)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_free_space_and_volume() {
        let dir = tempfile::tempdir().unwrap();
        assert!(free_bytes(dir.path()).unwrap() > 0);
        assert_eq!(same_volume(dir.path(), dir.path()), Some(true));
    }
}
