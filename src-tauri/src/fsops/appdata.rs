//! Schreibzugriff auf das App-Datenverzeichnis (Datenbank, Thumbnail-Cache,
//! Laufprotokolle). Pfade außerhalb dieses Verzeichnisses sind nicht erreichbar.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct AppDataDir {
    root: PathBuf,
}

impl AppDataDir {
    /// Legt das Verzeichnis bei Bedarf an.
    pub fn init(root: PathBuf) -> io::Result<Self> {
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    /// Hängt einen relativen Pfad an. `..`, absolute Pfade und Laufwerksangaben
    /// werden abgewiesen.
    pub fn join(&self, rel: impl AsRef<Path>) -> io::Result<PathBuf> {
        let rel = rel.as_ref();
        let valid = rel.components().next().is_some()
            && rel.components().all(|c| matches!(c, Component::Normal(_)));
        if !valid {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("ungültiger Pfad im App-Datenverzeichnis: {}", rel.display()),
            ));
        }
        Ok(self.root.join(rel))
    }

    pub fn ensure_dir(&self, rel: impl AsRef<Path>) -> io::Result<PathBuf> {
        let path = self.join(rel)?;
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    /// Entfernt einen Unterordner des App-Verzeichnisses vollständig (Cache leeren).
    /// Das App-Verzeichnis selbst und Pfade außerhalb sind nicht erreichbar.
    pub fn clear_dir(&self, rel: impl AsRef<Path>) -> io::Result<()> {
        let path = self.join(rel)?;
        match fs::remove_dir_all(&path) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Belegter Platz eines Unterordners in Bytes (nur lesend).
    pub fn dir_size(&self, rel: impl AsRef<Path>) -> u64 {
        let Ok(path) = self.join(rel) else { return 0 };
        walkdir::WalkDir::new(path).into_iter().filter_map(Result::ok).filter_map(|e| e.metadata().ok()).filter(|m| m.is_file()).map(|m| m.len()).sum()
    }

    /// Schreibt eine Datei atomar (temporäre Datei, dann Rename). Innerhalb des
    /// App-Verzeichnisses darf überschrieben werden, es gehört der App.
    pub fn write_file(&self, rel: impl AsRef<Path>, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = self.join(rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        // Eindeutiger Temp-Name: Parallele Schreiber derselben Datei (z. B. zwei
        // gleichzeitige Vorschau-Anfragen) dürfen sich nicht in die Quere kommen.
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
        tmp_name.push(format!(".{}-{n}.tmp", std::process::id()));
        let tmp = path.with_file_name(tmp_name);
        fs::write(&tmp, bytes)?;
        fs::rename(&tmp, &path)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_escaping_paths() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppDataDir::init(dir.path().join("app")).unwrap();
        assert!(app.join("../x").is_err());
        assert!(app.join("cache/../../x").is_err());
        assert!(app.join(dir.path()).is_err());
        assert!(app.join("").is_err());
        assert!(app.join("cache/thumbs").is_ok());
    }

    #[test]
    fn parallel_writers_do_not_collide() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppDataDir::init(dir.path().join("app")).unwrap();
        std::thread::scope(|s| {
            for i in 0..16u8 {
                let app = &app;
                s.spawn(move || app.write_file("cache/same.bin", &[i; 4096]).unwrap());
            }
        });
        let bytes = fs::read(app.join("cache/same.bin").unwrap()).unwrap();
        assert!(bytes.iter().all(|b| *b == bytes[0]), "Datei ist ein Gemisch mehrerer Schreiber");
        let leftovers = fs::read_dir(app.join("cache").unwrap()).unwrap().count();
        assert_eq!(leftovers, 1, "keine Temp-Dateien übrig");
    }

    #[test]
    fn clear_dir_stays_inside() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppDataDir::init(dir.path().join("app")).unwrap();
        app.write_file("previews/ab/x.jpg", b"123").unwrap();
        assert_eq!(app.dir_size("previews"), 3);
        app.clear_dir("previews").unwrap();
        assert!(!app.join("previews").unwrap().exists());
        app.clear_dir("previews").unwrap(); // fehlend ist kein Fehler
        assert!(app.clear_dir("..").is_err());
        assert!(app.clear_dir("").is_err());
        assert!(app.path().exists(), "App-Verzeichnis selbst bleibt");
    }

    #[test]
    fn writes_and_overwrites_inside() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppDataDir::init(dir.path().join("app")).unwrap();
        let p = app.write_file("logs/run-1.json", b"1").unwrap();
        app.write_file("logs/run-1.json", b"2").unwrap();
        assert_eq!(fs::read(p).unwrap(), b"2");
    }
}
