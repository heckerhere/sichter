//! BLAKE3-Hashing von Dateien. Liest nur, schreibt nie.

use std::fmt;
use std::fs::File;
use std::io;
use std::path::Path;

/// BLAKE3-Hash einer Datei. In der Datenbank als Hex-String gespeichert.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileHash(blake3::Hash);

impl FileHash {
    pub fn to_hex(&self) -> String {
        self.0.to_hex().to_string()
    }

    pub fn from_hex(hex: &str) -> Option<Self> {
        blake3::Hash::from_hex(hex).ok().map(Self)
    }
}

impl fmt::Debug for FileHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "FileHash({})", self.to_hex())
    }
}

pub fn hash_file(path: &Path) -> io::Result<FileHash> {
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(File::open(path)?)?;
    Ok(FileHash(hasher.finalize()))
}

#[cfg(test)]
pub fn hash_bytes(bytes: &[u8]) -> FileHash {
    FileHash(blake3::hash(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip() {
        let h = hash_bytes(b"sichter");
        assert_eq!(FileHash::from_hex(&h.to_hex()), Some(h));
        assert_eq!(FileHash::from_hex("kein hash"), None);
    }

    #[test]
    fn hashes_file_like_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.bin");
        let content = vec![7u8; 200_000];
        // Auch Testcode schreibt nur über fsops, damit der Guard ausnahmslos gilt.
        crate::fsops::test_support::write(&path, &content);
        assert_eq!(hash_file(&path).unwrap(), hash_bytes(&content));
    }
}
