//! ffprobe/ffmpeg-Anbindung für Videometadaten und Vorschaubilder.
//! Ausgaben gehen ausschließlich über stdout, es entstehen keine Dateien.

use std::path::Path;
use std::process::Stdio;

use serde_json::Value;

use crate::tools::ToolCommand;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct VideoMeta {
    pub duration_s: Option<f64>,
    pub codec: Option<String>,
    pub fps: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
}

pub fn probe(ffprobe: &ToolCommand, path: &Path) -> Result<VideoMeta, String> {
    let output = ffprobe
        .command()
        .args(["-v", "error", "-print_format", "json", "-show_format", "-show_streams"])
        .arg(path)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    let json: Value = serde_json::from_slice(&output.stdout).map_err(|e| e.to_string())?;
    Ok(parse_probe(&json))
}

fn parse_fraction(s: &str) -> Option<f64> {
    let (num, den) = s.split_once('/').unwrap_or((s, "1"));
    let (num, den): (f64, f64) = (num.parse().ok()?, den.parse().ok()?);
    (den != 0.0 && num > 0.0).then_some(num / den)
}

pub fn parse_probe(json: &Value) -> VideoMeta {
    let stream = json["streams"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["codec_type"] == "video" && s["disposition"]["attached_pic"] != 1));
    let duration = json["format"]["duration"]
        .as_str()
        .and_then(|d| d.parse::<f64>().ok())
        .or_else(|| stream.and_then(|s| s["duration"].as_str()).and_then(|d| d.parse().ok()));
    VideoMeta {
        duration_s: duration.filter(|d| d.is_finite() && *d > 0.0),
        codec: stream.and_then(|s| s["codec_name"].as_str()).map(str::to_string),
        fps: stream
            .and_then(|s| s["avg_frame_rate"].as_str().and_then(parse_fraction).or_else(|| s["r_frame_rate"].as_str().and_then(parse_fraction))),
        width: stream.and_then(|s| s["width"].as_i64()),
        height: stream.and_then(|s| s["height"].as_i64()),
    }
}

/// Ein Einzelbild als JPEG über stdout. `at_s` ist die Position im Video,
/// bei Standbildern (HEIC) `None`. ffmpeg dreht dabei automatisch gemäß
/// Rotationsangabe.
///
/// Standbilder werden nicht von ffmpeg skaliert: iPhone-HEICs sind Kachel-Bilder
/// (Tile Grid), die ffmpeg über einen eigenen Filtergraphen zusammensetzt, und
/// der lässt sich nicht mit `-vf` kombinieren. Verkleinert wird danach in Rust.
pub fn extract_frame_jpeg(ffmpeg: &ToolCommand, path: &Path, at_s: Option<f64>, max_edge: u32) -> Result<Vec<u8>, String> {
    let mut cmd = ffmpeg.command();
    cmd.args(["-v", "error", "-nostdin"]);
    if let Some(at) = at_s {
        cmd.args(["-ss", &format!("{at:.3}")]);
    }
    cmd.arg("-i").arg(path).args(["-frames:v", "1"]);
    if at_s.is_some() {
        let scale = format!("scale='min({max_edge},iw)':'min({max_edge},ih)':force_original_aspect_ratio=decrease");
        cmd.args(["-vf", &scale]);
    }
    let output = cmd
        .args(["-f", "image2pipe", "-c:v", "mjpeg", "-q:v", "3", "-"])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() || output.stdout.is_empty() {
        let err = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if err.is_empty() { crate::i18n::tr("kein Bild erzeugt", "no image produced").into() } else { err });
    }
    Ok(output.stdout)
}

/// Wertet einen HTTP-Range-Header (`bytes=start-end`) aus. Liefert den
/// inklusiven Bereich, begrenzt auf `max_chunk` Bytes; `None` ohne gültigen Header.
pub fn parse_range(header: Option<&str>, len: u64, max_chunk: u64) -> Option<(u64, u64)> {
    let spec = header?.trim().strip_prefix("bytes=")?;
    let (start, end) = spec.split(',').next()?.split_once('-')?;
    let (start, end) = match (start.trim(), end.trim()) {
        ("", suffix) => {
            // "bytes=-500": die letzten 500 Bytes.
            let n: u64 = suffix.parse().ok()?;
            (len.saturating_sub(n), len.checked_sub(1)?)
        }
        (s, "") => (s.parse().ok()?, len.checked_sub(1)?),
        (s, e) => (s.parse().ok()?, e.parse::<u64>().ok()?.min(len.checked_sub(1)?)),
    };
    if start > end || start >= len {
        return None;
    }
    Some((start, end.min(start + max_chunk - 1)))
}

pub fn mime_for(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
        Some("mp4") => "video/mp4",
        Some("m4v") => "video/x-m4v",
        Some("mov") => "video/quicktime",
        Some("3gp") => "video/3gpp",
        Some("mkv") => "video/x-matroska",
        Some("avi") => "video/x-msvideo",
        Some("mts") | Some("m2ts") => "video/mp2t",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ranges() {
        let mb = 1 << 20;
        assert_eq!(parse_range(Some("bytes=0-"), 1000, mb), Some((0, 999)));
        assert_eq!(parse_range(Some("bytes=100-199"), 1000, mb), Some((100, 199)));
        assert_eq!(parse_range(Some("bytes=900-5000"), 1000, mb), Some((900, 999)));
        assert_eq!(parse_range(Some("bytes=-100"), 1000, mb), Some((900, 999)));
        assert_eq!(parse_range(Some("bytes=0-"), 10 * mb, mb), Some((0, mb - 1)), "auf Blockgröße begrenzt");
        assert_eq!(parse_range(Some("bytes=1000-"), 1000, mb), None);
        assert_eq!(parse_range(Some("bytes=5-2"), 1000, mb), None);
        assert_eq!(parse_range(Some("items=0-1"), 1000, mb), None);
        assert_eq!(parse_range(None, 1000, mb), None);
        assert_eq!(parse_range(Some("bytes=0-"), 0, mb), None);
    }
    use crate::tools::Tools;
    use serde_json::json;

    fn fixture(rel: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/library").join(rel)
    }

    #[test]
    fn parses_probe_json() {
        let json = json!({
            "streams": [
                {"codec_type": "audio", "codec_name": "aac"},
                {"codec_type": "video", "codec_name": "h264", "width": 320, "height": 240, "avg_frame_rate": "30000/1001"}
            ],
            "format": {"duration": "2.000000"}
        });
        let m = parse_probe(&json);
        assert_eq!(m.codec.as_deref(), Some("h264"));
        assert_eq!((m.width, m.height), (Some(320), Some(240)));
        assert!((m.fps.unwrap() - 29.97).abs() < 0.01);
        assert_eq!(m.duration_s, Some(2.0));
    }

    #[test]
    fn probes_fixture_video() {
        let tools = Tools::resolve(None);
        let m = probe(tools.ffprobe.as_ref().unwrap(), &fixture("video/clip.mp4")).unwrap();
        assert_eq!(m.codec.as_deref(), Some("h264"));
        assert_eq!(m.width, Some(320));
        assert!((m.duration_s.unwrap() - 2.0).abs() < 0.1);
    }

    #[test]
    fn extracts_frames_from_video_and_heic() {
        let tools = Tools::resolve(None);
        let ffmpeg = tools.ffmpeg.as_ref().unwrap();
        for (rel, at) in [("video/clip.mp4", Some(0.2)), ("iPhone/IMG_0100.HEIC", None)] {
            let jpeg = extract_frame_jpeg(ffmpeg, &fixture(rel), at, 256).unwrap();
            assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "{rel}");
        }
    }
}
