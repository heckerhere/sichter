//! Preview-Extraktion und Thumbnail-Cache.
//!
//! Quellen je Art:
//! * JPEG, PNG, TIFF, WebP: direkt dekodiert.
//! * RAW: eingebettetes JPEG per exiftool (`PreviewImage`, `JpgFromRaw`,
//!   `ThumbnailImage`), kein Demosaicing.
//! * HEIC und Video: Einzelbild per ffmpeg (Video bei ca. 10 % der Laufzeit).
//!
//! Cache: `thumbs/<2 Zeichen>/<hash>.webp` im App-Datenverzeichnis, Schlüssel
//! ist der BLAKE3-Hash des Originals. Gleiche Inhalte teilen sich ein Thumbnail.
//!
//! Für die Vollbildansicht (Sichten) gibt es zusätzlich große Vorschauen als
//! JPEG unter `previews/`, erzeugt bei Bedarf bzw. im Hintergrund vorab.

pub mod dhash;
pub mod preview;

use std::path::{Path, PathBuf};

use fast_image_resize::{images::Image, PixelType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, ImageReader, metadata::Orientation};

use crate::fsops::appdata::AppDataDir;
use crate::meta::exiftool::ExifTool;
use crate::scan::kind::Kind;
use crate::tools::ToolCommand;

/// Version des Erzeugers. Erhöhen, wenn neue Quellen oder Fehlerbehebungen
/// hinzukommen: Fehlschläge älterer Versionen werden dann erneut versucht.
/// 2: Kachel-HEICs (iPhone) werden ohne ffmpeg-Skalierfilter dekodiert.
pub const VERSION: i64 = 2;

/// Längste Kante in Pixeln. Reicht für Grid-Kacheln auf Retina-Displays.
pub const MAX_EDGE: u32 = 512;
const WEBP_QUALITY: f32 = 78.0;
/// Längste Kante der Vollbild-Vorschau.
pub const PREVIEW_EDGE: u32 = 2560;
const PREVIEW_JPEG_QUALITY: u8 = 85;

pub fn cache_rel_path(hash_hex: &str) -> PathBuf {
    Path::new("thumbs").join(&hash_hex[..2]).join(format!("{hash_hex}.webp"))
}

pub fn preview_rel_path(hash_hex: &str) -> PathBuf {
    Path::new("previews").join(&hash_hex[..2]).join(format!("{hash_hex}.jpg"))
}

/// Alles, was für ein Thumbnail eines Assets nötig ist.
pub struct ThumbSource<'a> {
    pub path: &'a Path,
    pub kind: Kind,
    pub orientation: Option<i64>,
    pub duration_s: Option<f64>,
}

pub struct Thumbnailer<'a> {
    pub ffmpeg: Option<&'a ToolCommand>,
}

impl Thumbnailer<'_> {
    /// Erzeugt das Thumbnail als WebP-Bytes. `Ok(None)`, wenn die Datei kein
    /// Vorschaubild hergibt (z. B. RAW ohne eingebettetes JPEG).
    pub fn render(&self, src: &ThumbSource, exiftool: &mut dyn FnMut(&Path) -> Option<Vec<u8>>) -> Result<Option<Vec<u8>>, String> {
        self.decode(src, MAX_EDGE, exiftool)?.map(|img| encode_webp(&resize(img, MAX_EDGE)?)).transpose()
    }

    /// Große Vorschau als JPEG-Bytes für die Vollbildansicht.
    pub fn render_preview(&self, src: &ThumbSource, exiftool: &mut dyn FnMut(&Path) -> Option<Vec<u8>>) -> Result<Option<Vec<u8>>, String> {
        self.decode(src, PREVIEW_EDGE, exiftool)?.map(|img| encode_jpeg(&resize(img, PREVIEW_EDGE)?)).transpose()
    }

    fn decode(&self, src: &ThumbSource, max_edge: u32, exiftool: &mut dyn FnMut(&Path) -> Option<Vec<u8>>) -> Result<Option<DynamicImage>, String> {
        Ok(match src.kind {
            Kind::Jpeg | Kind::Png | Kind::Tiff | Kind::Webp => {
                let img = ImageReader::open(src.path)
                    .map_err(|e| e.to_string())?
                    .with_guessed_format()
                    .map_err(|e| e.to_string())?
                    .decode()
                    .map_err(|e| e.to_string())?;
                Some(apply_orientation(img, src.orientation))
            }
            Kind::Raw => match exiftool(src.path) {
                Some(jpeg) => {
                    let img = image::load_from_memory(&jpeg).map_err(|e| e.to_string())?;
                    // Eingebettete Previews sind meist ungedreht gespeichert.
                    Some(apply_orientation(img, src.orientation))
                }
                None => None,
            },
            Kind::Heic | Kind::Video => {
                let ffmpeg = self.ffmpeg.ok_or(crate::i18n::tr("ffmpeg nicht verfügbar", "ffmpeg not available"))?;
                let at = (src.kind == Kind::Video).then(|| src.duration_s.map_or(0.0, |d| d * 0.1));
                let jpeg = crate::video::extract_frame_jpeg(ffmpeg, src.path, at, max_edge)?;
                Some(image::load_from_memory(&jpeg).map_err(|e| e.to_string())?)
            }
            Kind::Sidecar | Kind::Other => None,
        })
    }
}

/// Wie [`raw_preview`], aber das größte eingebettete JPEG (für die Vollbildansicht).
pub fn raw_preview_largest(exiftool: &mut ExifTool, path: &Path) -> Option<Vec<u8>> {
    let path = path.to_str()?;
    ["-JpgFromRaw", "-PreviewImage", "-ThumbnailImage"]
        .iter()
        .filter_map(|tag| {
            let bytes = exiftool.execute_binary(&["-b", tag, path]).ok()?;
            (bytes.len() > 2 && bytes[..2] == [0xFF, 0xD8]).then_some(bytes)
        })
        .max_by_key(Vec::len)
}

/// Holt das größte brauchbare eingebettete JPEG aus einer RAW-Datei.
pub fn raw_preview(exiftool: &mut ExifTool, path: &Path) -> Option<Vec<u8>> {
    let path = path.to_str()?;
    ["-PreviewImage", "-JpgFromRaw", "-ThumbnailImage"].iter().find_map(|tag| {
        let bytes = exiftool.execute_binary(&["-b", tag, path]).ok()?;
        (bytes.len() > 2 && bytes[..2] == [0xFF, 0xD8]).then_some(bytes)
    })
}

fn apply_orientation(mut img: DynamicImage, orientation: Option<i64>) -> DynamicImage {
    if let Some(o) = orientation.and_then(|o| u8::try_from(o).ok()).and_then(Orientation::from_exif) {
        img.apply_orientation(o);
    }
    img
}

fn resize(img: DynamicImage, max_edge: u32) -> Result<DynamicImage, String> {
    let img = DynamicImage::ImageRgb8(img.to_rgb8());
    let (w, h) = (img.width(), img.height());
    if w == 0 || h == 0 {
        return Err(crate::i18n::tr("Bild ohne Pixel", "Image without pixels").into());
    }
    if w.max(h) <= max_edge {
        return Ok(img);
    }
    let scale = max_edge as f64 / w.max(h) as f64;
    let (tw, th) = (((w as f64 * scale).round() as u32).max(1), ((h as f64 * scale).round() as u32).max(1));
    let mut dst = Image::new(tw, th, PixelType::U8x3);
    Resizer::new()
        .resize(&img, &mut dst, &ResizeOptions::new().resize_alg(ResizeAlg::Convolution(fast_image_resize::FilterType::Lanczos3)))
        .map_err(|e| e.to_string())?;
    image::RgbImage::from_raw(tw, th, dst.into_vec())
        .map(DynamicImage::ImageRgb8)
        .ok_or_else(|| crate::i18n::tr("Skalierung fehlgeschlagen", "Scaling failed").into())
}

fn encode_webp(img: &DynamicImage) -> Result<Vec<u8>, String> {
    let rgb = img.to_rgb8();
    let encoded = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height()).encode(WEBP_QUALITY);
    Ok(encoded.to_vec())
}

fn encode_jpeg(img: &DynamicImage) -> Result<Vec<u8>, String> {
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, PREVIEW_JPEG_QUALITY)
        .encode_image(&rgb)
        .map_err(|e| e.to_string())?;
    Ok(out)
}

pub fn store(app: &AppDataDir, hash_hex: &str, webp: &[u8]) -> std::io::Result<PathBuf> {
    app.write_file(cache_rel_path(hash_hex), webp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::Tools;

    fn fixture(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library").join(rel)
    }

    fn decode(webp: &[u8]) -> DynamicImage {
        image::load_from_memory_with_format(webp, image::ImageFormat::WebP).unwrap()
    }

    #[test]
    fn renders_all_fixture_kinds() {
        let tools = Tools::resolve(None);
        let mut et = ExifTool::spawn(tools.exiftool.as_ref().unwrap()).unwrap();
        let thumbnailer = Thumbnailer { ffmpeg: tools.ffmpeg.as_ref() };
        let cases = [
            ("2023-05-12 Geburtstag/IMG_0001.JPG", Kind::Jpeg, true),
            ("misc/screenshot.png", Kind::Png, true),
            ("misc/scan.tif", Kind::Tiff, true),
            ("misc/sticker.webp", Kind::Webp, true),
            ("iPhone/IMG_0100.HEIC", Kind::Heic, true),
            // Kachel-HEIC wie vom iPhone (Tile Grid).
            ("misc/kacheln.heic", Kind::Heic, true),
            ("video/clip.mp4", Kind::Video, true),
            ("raw/DSCF0001.RAF", Kind::Raw, true),
            ("raw/P1000001.RW2", Kind::Raw, true),
            // Metadaten-Stub ohne eingebettetes Bild.
            ("raw/DSC_0001.NEF", Kind::Raw, false),
        ];
        for (rel, kind, expect_image) in cases {
            let path = fixture(rel);
            let src = ThumbSource { path: &path, kind, orientation: None, duration_s: Some(2.0) };
            let out = thumbnailer.render(&src, &mut |p| raw_preview(&mut et, p)).unwrap();
            assert_eq!(out.is_some(), expect_image, "{rel}");
            if let Some(webp) = out {
                let img = decode(&webp);
                assert!(img.width().max(img.height()) <= MAX_EDGE, "{rel}");
            }
        }
    }

    #[test]
    fn applies_orientation_and_limits_size() {
        let tools = Tools::resolve(None);
        let thumbnailer = Thumbnailer { ffmpeg: tools.ffmpeg.as_ref() };
        let path = fixture("2023-05-12 Geburtstag/IMG_0001.JPG"); // 640x480
        let src = ThumbSource { path: &path, kind: Kind::Jpeg, orientation: Some(6), duration_s: None };
        let img = decode(&thumbnailer.render(&src, &mut |_| None).unwrap().unwrap());
        assert_eq!((img.width(), img.height()), (384, 512));
    }
}
