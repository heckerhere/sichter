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

    /// Schreibt eine Datei atomar (temporäre Datei, dann Rename). Innerhalb des
    /// App-Verzeichnisses darf überschrieben werden, es gehört der App.
    pub fn write_file(&self, rel: impl AsRef<Path>, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = self.join(rel)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
        tmp_name.push(".tmp");
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
    fn writes_and_overwrites_inside() {
        let dir = tempfile::tempdir().unwrap();
        let app = AppDataDir::init(dir.path().join("app")).unwrap();
        let p = app.write_file("logs/run-1.json", b"1").unwrap();
        app.write_file("logs/run-1.json", b"2").unwrap();
        assert_eq!(fs::read(p).unwrap(), b"2");
    }
}
