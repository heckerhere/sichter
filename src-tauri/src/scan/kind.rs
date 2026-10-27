//! Dateityp-Erkennung über Endung und Magic Bytes.
//!
//! Die Endung entscheidet, ob eine Datei überhaupt unterstützt ist
//! (Sicherheitsmodell 3.9). Die Magic Bytes korrigieren die Art innerhalb der
//! unterstützten Medien (z. B. HEIC mit Endung `.jpg`). Passt der Inhalt zu gar
//! keinem Medienformat, wird die Datei als `other` geführt und nie bewegt.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Raw,
    Jpeg,
    Heic,
    Png,
    Tiff,
    Webp,
    Video,
    Sidecar,
    Other,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Raw => "raw",
            Kind::Jpeg => "jpeg",
            Kind::Heic => "heic",
            Kind::Png => "png",
            Kind::Tiff => "tiff",
            Kind::Webp => "webp",
            Kind::Video => "video",
            Kind::Sidecar => "sidecar",
            Kind::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Kind {
        match s {
            "raw" => Kind::Raw,
            "jpeg" => Kind::Jpeg,
            "heic" => Kind::Heic,
            "png" => Kind::Png,
            "tiff" => Kind::Tiff,
            "webp" => Kind::Webp,
            "video" => Kind::Video,
            "sidecar" => Kind::Sidecar,
            _ => Kind::Other,
        }
    }

    /// Fotos und Videos, die angezeigt und bewertet werden.
    pub fn is_media(self) -> bool {
        !matches!(self, Kind::Sidecar | Kind::Other)
    }

    pub fn is_image(self) -> bool {
        self.is_media() && self != Kind::Video
    }
}

pub const RAW_EXTENSIONS: &[&str] = &["cr2", "cr3", "nef", "arw", "dng", "raf", "orf", "rw2"];
pub const SIDECAR_EXTENSIONS: &[&str] = &["xmp", "aae", "thm", "pp3"];
const VIDEO_EXTENSIONS: &[&str] = &["mp4", "mov", "m4v", "avi", "mts", "m2ts", "3gp", "mkv"];

pub fn kind_from_extension(path: &Path) -> Kind {
    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
        return Kind::Other;
    };
    let ext = ext.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" | "jpe" => Kind::Jpeg,
        "heic" | "heif" | "hif" => Kind::Heic,
        "png" => Kind::Png,
        "tif" | "tiff" => Kind::Tiff,
        "webp" => Kind::Webp,
        e if RAW_EXTENSIONS.contains(&e) => Kind::Raw,
        e if VIDEO_EXTENSIONS.contains(&e) => Kind::Video,
        e if SIDECAR_EXTENSIONS.contains(&e) => Kind::Sidecar,
        _ => Kind::Other,
    }
}

/// Was die ersten Bytes über das Format verraten.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Magic {
    Jpeg,
    Png,
    /// TIFF-Container: TIFF selbst und die meisten RAW-Formate.
    TiffLike,
    Heif,
    /// ISO-BMFF-Video (MP4, MOV, 3GP) oder alte QuickTime-Atome.
    IsoVideo,
    /// Canon CR3 (ISO-BMFF mit Marke `crx `).
    Cr3,
    Raf,
    Webp,
    Avi,
    Matroska,
    MpegTs,
    Unknown,
}

fn sniff(head: &[u8]) -> Magic {
    let at = |offset: usize, pattern: &[u8]| head.get(offset..offset + pattern.len()) == Some(pattern);
    if at(0, &[0xFF, 0xD8, 0xFF]) {
        return Magic::Jpeg;
    }
    if at(0, b"\x89PNG\r\n\x1a\n") {
        return Magic::Png;
    }
    // TIFF (II*\0 / MM\0*), Panasonic RW2 (IIU\0), Olympus ORF (IIRO / IIRS / MMOR).
    if at(0, b"II*\0") || at(0, b"MM\0*") || at(0, b"IIU\0") || at(0, b"IIRO") || at(0, b"IIRS") || at(0, b"MMOR") {
        return Magic::TiffLike;
    }
    if at(0, b"FUJIFILMCCD-RAW") {
        return Magic::Raf;
    }
    if at(0, b"RIFF") && at(8, b"WEBP") {
        return Magic::Webp;
    }
    if at(0, b"RIFF") && at(8, b"AVI ") {
        return Magic::Avi;
    }
    if at(0, &[0x1A, 0x45, 0xDF, 0xA3]) {
        return Magic::Matroska;
    }
    if at(4, b"ftyp") {
        let brand = head.get(8..12).unwrap_or_default();
        return match brand {
            b"crx " => Magic::Cr3,
            b"heic" | b"heix" | b"heim" | b"heis" | b"hevc" | b"hevx" | b"mif1" | b"msf1" | b"avif" => Magic::Heif,
            _ => Magic::IsoVideo,
        };
    }
    if [b"moov", b"mdat", b"wide", b"free", b"skip", b"pnot"].iter().any(|atom| at(4, *atom)) {
        return Magic::IsoVideo;
    }
    // MPEG-TS (0x47 alle 188 Bytes) bzw. M2TS (4 Byte Zeitstempel davor).
    if head.first() == Some(&0x47) || head.get(4) == Some(&0x47) {
        return Magic::MpegTs;
    }
    Magic::Unknown
}

/// Ergebnis der Erkennung: Art und ob die Endung nicht zum Inhalt passt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detected {
    pub kind: Kind,
    pub ext_mismatch: bool,
}

pub fn detect(path: &Path) -> Detected {
    let by_ext = kind_from_extension(path);
    // Sidecars (XMP, AAE, THM, PP3) und unbekannte Endungen nicht weiter prüfen.
    if !by_ext.is_media() {
        return Detected { kind: by_ext, ext_mismatch: false };
    }
    let mut head = [0u8; 32];
    let len = File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
    detect_from_bytes(by_ext, &head[..len])
}

fn detect_from_bytes(by_ext: Kind, head: &[u8]) -> Detected {
    let ok = |kind| Detected { kind, ext_mismatch: false };
    let corrected = |kind| Detected { kind, ext_mismatch: true };
    let magic = sniff(head);
    match (by_ext, magic) {
        (Kind::Raw, Magic::TiffLike | Magic::Cr3 | Magic::Raf) => ok(Kind::Raw),
        (Kind::Tiff, Magic::TiffLike) => ok(Kind::Tiff),
        (Kind::Jpeg, Magic::Jpeg) => ok(Kind::Jpeg),
        (Kind::Png, Magic::Png) => ok(Kind::Png),
        (Kind::Heic, Magic::Heif) => ok(Kind::Heic),
        (Kind::Webp, Magic::Webp) => ok(Kind::Webp),
        (Kind::Video, Magic::IsoVideo | Magic::Avi | Magic::Matroska | Magic::MpegTs) => ok(Kind::Video),
        // Inhalt ist ein anderes unterstütztes Medienformat.
        (_, Magic::Jpeg) => corrected(Kind::Jpeg),
        (_, Magic::Png) => corrected(Kind::Png),
        (_, Magic::Heif) => corrected(Kind::Heic),
        (_, Magic::Webp) => corrected(Kind::Webp),
        // RAW/TIFF sind nicht sicher unterscheidbar; MPEG-TS ist zu schwach
        // erkennbar (erstes Byte 0x47), um eine Bild-Endung zu überstimmen.
        (Kind::Raw | Kind::Tiff, _)
        | (_, Magic::Unknown | Magic::TiffLike | Magic::Cr3 | Magic::Raf | Magic::MpegTs) => {
            corrected(Kind::Other)
        }
        (_, Magic::IsoVideo | Magic::Avi | Magic::Matroska) => corrected(Kind::Video),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library").join(rel)
    }

    #[test]
    fn detects_fixture_kinds() {
        let cases = [
            ("2023-05-12 Geburtstag/IMG_0001.JPG", Kind::Jpeg),
            ("2023-05-12 Geburtstag/IMG_0001.CR2", Kind::Raw),
            ("2023-05-12 Geburtstag/IMG_0001.xmp", Kind::Sidecar),
            ("raw/CR3_0001.CR3", Kind::Raw),
            ("raw/DSC_0001.NEF", Kind::Raw),
            ("raw/PXL_0001.DNG", Kind::Raw),
            ("raw/DSCF0001.RAF", Kind::Raw),
            ("raw/P1000001.RW2", Kind::Raw),
            ("iPhone/IMG_0100.HEIC", Kind::Heic),
            ("iPhone/IMG_0100.MOV", Kind::Video),
            ("iPhone/IMG_0100.AAE", Kind::Sidecar),
            ("video/clip.mp4", Kind::Video),
            ("misc/screenshot.png", Kind::Png),
            ("misc/scan.tif", Kind::Tiff),
            ("misc/sticker.webp", Kind::Webp),
            ("misc/notizen.txt", Kind::Other),
        ];
        for (rel, expected) in cases {
            let d = detect(&fixture(rel));
            assert_eq!(d.kind, expected, "{rel}");
            assert!(!d.ext_mismatch, "{rel}");
        }
    }

    #[test]
    fn corrects_wrong_extension() {
        let heic = [0, 0, 0, 0x18, b'f', b't', b'y', b'p', b'h', b'e', b'i', b'c'];
        assert_eq!(detect_from_bytes(Kind::Jpeg, &heic), Detected { kind: Kind::Heic, ext_mismatch: true });
    }

    #[test]
    fn text_with_image_extension_is_other() {
        assert_eq!(detect_from_bytes(Kind::Jpeg, b"hello world").kind, Kind::Other);
        assert_eq!(detect_from_bytes(Kind::Raw, b"hello world").kind, Kind::Other);
        assert_eq!(detect_from_bytes(Kind::Jpeg, b"Gruss").kind, Kind::Other);
    }

    #[test]
    fn extension_is_case_insensitive() {
        assert_eq!(kind_from_extension(Path::new("a/B.NeF")), Kind::Raw);
        assert_eq!(kind_from_extension(Path::new("a/B.M2TS")), Kind::Video);
        assert_eq!(kind_from_extension(Path::new("a/B")), Kind::Other);
    }
}
