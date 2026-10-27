//! Metadaten über exiftool (Fotos, RAW, Videos) und Datums-Fallbacks.
//!
//! Datumspriorität:
//! * Fotos: `DateTimeOriginal` → EXIF `CreateDate` → XMP → QuickTime.
//! * Videos: QuickTime `CreationDate` (mit Zeitzone) → QuickTime `CreateDate`
//!   (laut Spezifikation UTC, oft aber Ortszeit; markiert als `quicktime_utc`).
//! * Ohne Metadaten: Datum im Dateinamen, danach mtime. Beides gilt als
//!   "Datum unsicher".

pub mod exiftool;
pub mod xmp;

use std::sync::LazyLock;

use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone};
use regex::Regex;
use serde_json::Value;

/// Tags, die beim Scan abgefragt werden (`-G` liefert Gruppenpräfixe).
pub const SCAN_TAGS: &[&str] = &[
    "-EXIF:DateTimeOriginal",
    "-EXIF:OffsetTimeOriginal",
    "-EXIF:CreateDate",
    "-XMP:DateTimeOriginal",
    "-XMP:DateCreated",
    "-QuickTime:CreationDate",
    "-QuickTime:CreateDate",
    "-Make",
    "-Model",
    "-LensModel",
    "-Lens",
    "-Composite:GPSLatitude",
    "-Composite:GPSLongitude",
    "-Composite:ImageSize",
    "-Orientation",
    "-ContentIdentifier",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateSource {
    Exif,
    Filename,
    Mtime,
}

impl DateSource {
    pub fn as_str(self) -> &'static str {
        match self {
            DateSource::Exif => "exif",
            DateSource::Filename => "filename",
            DateSource::Mtime => "mtime",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ParsedMeta {
    /// Lokale Aufnahmezeit (Wanduhrzeit der Kamera).
    pub taken_at: Option<NaiveDateTime>,
    /// Minuten gegenüber UTC, falls bekannt.
    pub tz_offset: Option<i32>,
    pub date_source: Option<DateSource>,
    pub quicktime_utc: bool,
    pub camera_make: Option<String>,
    pub camera_model: Option<String>,
    pub lens: Option<String>,
    pub gps_lat: Option<f64>,
    pub gps_lon: Option<f64>,
    pub width: Option<i64>,
    pub height: Option<i64>,
    pub orientation: Option<i64>,
    pub content_id: Option<String>,
}

impl ParsedMeta {
    pub fn taken_at_string(&self) -> Option<String> {
        self.taken_at.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string())
    }
}

/// Erster Wert, dessen Schlüssel (ohne Gruppenpräfix) `name` ist.
fn any_group<'a>(obj: &'a Value, name: &str) -> Option<&'a Value> {
    obj.as_object()?
        .iter()
        .find(|(k, _)| k.rsplit(':').next() == Some(name))
        .map(|(_, v)| v)
}

fn string_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()).filter(|s| !s.is_empty()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Parst exiftool-Datumswerte wie `2023:05:12 15:01:10`, `...+02:00`,
/// `...Z` oder mit Sekundenbruchteilen. Nullwerte gelten als fehlend.
pub fn parse_exif_date(s: &str) -> Option<(NaiveDateTime, Option<i32>)> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{4})[:-](\d{2})[:-](\d{2})[ T](\d{2}):(\d{2})(?::(\d{2}))?(?:\.\d+)?\s*(Z|[+-]\d{2}:?\d{2})?").unwrap()
    });
    let c = RE.captures(s.trim())?;
    let num = |i: usize| c.get(i).map_or(Some(0), |m| m.as_str().parse::<u32>().ok());
    let year = num(1)? as i32;
    if !(1900..=2100).contains(&year) {
        return None;
    }
    let dt = NaiveDate::from_ymd_opt(year, num(2)?, num(3)?)?.and_hms_opt(num(4)?, num(5)?, num(6)?)?;
    let tz = c.get(7).and_then(|m| parse_offset(m.as_str()));
    Some((dt, tz))
}

fn parse_offset(s: &str) -> Option<i32> {
    if s == "Z" {
        return Some(0);
    }
    let sign = if s.starts_with('-') { -1 } else { 1 };
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 4 {
        return None;
    }
    let hours: i32 = digits[..2].parse().ok()?;
    let minutes: i32 = digits[2..].parse().ok()?;
    (hours <= 14 && minutes < 60).then_some(sign * (hours * 60 + minutes))
}

/// Wertet ein exiftool-JSON-Objekt (`-j -n -G`) aus.
pub fn parse_exiftool_object(obj: &Value, is_video: bool) -> ParsedMeta {
    let get = |key: &str| obj.get(key).and_then(string_of);
    let date = |key: &str| get(key).and_then(|s| parse_exif_date(&s));

    let mut meta = ParsedMeta::default();
    let photo_order = ["EXIF:DateTimeOriginal", "EXIF:CreateDate", "XMP:DateTimeOriginal", "XMP:DateCreated", "QuickTime:CreationDate"];
    let found = if is_video {
        date("QuickTime:CreationDate").map(|d| (d, false)).or_else(|| date("QuickTime:CreateDate").map(|d| (d, true)))
    } else {
        photo_order.iter().find_map(|k| date(k)).map(|d| (d, false))
    };
    if let Some(((dt, tz), utc_ambiguous)) = found {
        meta.taken_at = Some(dt);
        meta.date_source = Some(DateSource::Exif);
        meta.quicktime_utc = utc_ambiguous && tz.is_none();
        meta.tz_offset = tz.or_else(|| get("EXIF:OffsetTimeOriginal").and_then(|s| parse_offset(&s)));
    }

    meta.camera_make = any_group(obj, "Make").and_then(string_of);
    meta.camera_model = any_group(obj, "Model").and_then(string_of);
    meta.lens = any_group(obj, "LensModel").or_else(|| any_group(obj, "Lens")).and_then(string_of);
    meta.gps_lat = obj.get("Composite:GPSLatitude").and_then(Value::as_f64);
    meta.gps_lon = obj.get("Composite:GPSLongitude").and_then(Value::as_f64);
    if meta.gps_lat == Some(0.0) && meta.gps_lon == Some(0.0) {
        // 0/0 ist fast immer ein Platzhalter ohne GPS-Fix.
        meta.gps_lat = None;
        meta.gps_lon = None;
    }
    if let Some(size) = get("Composite:ImageSize") {
        let mut parts = size.split(|c: char| c == 'x' || c.is_whitespace()).filter_map(|p| p.parse().ok());
        meta.width = parts.next();
        meta.height = parts.next();
    }
    meta.orientation = any_group(obj, "Orientation").and_then(Value::as_i64).filter(|o| (1..=8).contains(o));
    meta.content_id = any_group(obj, "ContentIdentifier").and_then(string_of);
    meta
}

/// Datum aus Dateinamen wie `IMG_20230512_101500.jpg`, `PXL_20230512_101500123.jpg`,
/// `Screenshot 2023-05-12 at 10.15.00.png` oder `IMG-20230512-WA0001.jpg`.
pub fn date_from_filename(name: &str) -> Option<NaiveDateTime> {
    static WITH_TIME: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?:^|\D)((?:19|20)\d{2})[-_.]?(\d{2})[-_.]?(\d{2})(?: at |[ _T-])?(\d{2})[-_.:h]?(\d{2})[-_.:m]?(\d{2})(?:\D|$|\d{3}\D)").unwrap()
    });
    static DATE_ONLY: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?:^|\D)((?:19|20)\d{2})[-_.]?(\d{2})[-_.]?(\d{2})(?:\D|$)").unwrap());

    let n = |c: &regex::Captures, i: usize| c[i].parse::<u32>().ok();
    if let Some(c) = WITH_TIME.captures(name) {
        let date = NaiveDate::from_ymd_opt(n(&c, 1)? as i32, n(&c, 2)?, n(&c, 3)?);
        if let Some(dt) = date.and_then(|d| d.and_hms_opt(n(&c, 4)?, n(&c, 5)?, n(&c, 6)?)) {
            return Some(dt);
        }
    }
    let c = DATE_ONLY.captures(name)?;
    NaiveDate::from_ymd_opt(n(&c, 1)? as i32, n(&c, 2)?, n(&c, 3)?)?.and_hms_opt(0, 0, 0)
}

/// mtime als lokale Uhrzeit samt Offset.
pub fn date_from_mtime(mtime_ns: i64) -> Option<(NaiveDateTime, i32)> {
    let utc = DateTime::from_timestamp(mtime_ns.div_euclid(1_000_000_000), 0)?;
    let local: DateTime<FixedOffset> = Local.from_utc_datetime(&utc.naive_utc()).fixed_offset();
    Some((local.naive_local(), local.offset().local_minus_utc() / 60))
}

/// Füllt ein fehlendes Datum aus Dateiname bzw. mtime.
pub fn apply_date_fallback(meta: &mut ParsedMeta, file_name: &str, mtime_ns: i64) {
    if meta.taken_at.is_some() {
        return;
    }
    if let Some(dt) = date_from_filename(file_name) {
        meta.taken_at = Some(dt);
        meta.date_source = Some(DateSource::Filename);
    } else if let Some((dt, offset)) = date_from_mtime(mtime_ns) {
        meta.taken_at = Some(dt);
        meta.tz_offset = Some(offset);
        meta.date_source = Some(DateSource::Mtime);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    #[test]
    fn parses_exif_dates() {
        assert_eq!(parse_exif_date("2023:05:12 15:01:10"), Some((dt("2023-05-12 15:01:10"), None)));
        assert_eq!(parse_exif_date("2024:07:14 18:05:00+02:00"), Some((dt("2024-07-14 18:05:00"), Some(120))));
        assert_eq!(parse_exif_date("2024:07:14 18:05:00.123-05:30"), Some((dt("2024-07-14 18:05:00"), Some(-330))));
        assert_eq!(parse_exif_date("2024:07:14 18:05:00Z"), Some((dt("2024-07-14 18:05:00"), Some(0))));
        assert_eq!(parse_exif_date("0000:00:00 00:00:00"), None);
        assert_eq!(parse_exif_date("2023:13:40 10:00:00"), None);
        assert_eq!(parse_exif_date(""), None);
    }

    #[test]
    fn photo_prefers_datetimeoriginal_with_offset() {
        let obj = json!({
            "EXIF:CreateDate": "2020:01:01 00:00:00",
            "EXIF:DateTimeOriginal": "2023:05:12 15:01:10",
            "EXIF:OffsetTimeOriginal": "+02:00",
            "EXIF:Make": "Canon",
            "EXIF:Model": "Canon EOS 5D Mark IV",
            "Composite:GPSLatitude": 52.52,
            "Composite:GPSLongitude": -13.4,
            "Composite:ImageSize": "640 480",
            "EXIF:Orientation": 6
        });
        let m = parse_exiftool_object(&obj, false);
        assert_eq!(m.taken_at, Some(dt("2023-05-12 15:01:10")));
        assert_eq!(m.tz_offset, Some(120));
        assert_eq!(m.date_source, Some(DateSource::Exif));
        assert_eq!(m.camera_model.as_deref(), Some("Canon EOS 5D Mark IV"));
        assert_eq!((m.gps_lat, m.gps_lon), (Some(52.52), Some(-13.4)));
        assert_eq!((m.width, m.height), (Some(640), Some(480)));
        assert_eq!(m.orientation, Some(6));
    }

    #[test]
    fn video_prefers_creationdate_and_flags_utc_createdate() {
        let with_tz = json!({"QuickTime:CreationDate": "2024:07:14 18:05:00+02:00", "QuickTime:CreateDate": "2024:07:14 16:05:00"});
        let m = parse_exiftool_object(&with_tz, true);
        assert_eq!(m.taken_at, Some(dt("2024-07-14 18:05:00")));
        assert_eq!(m.tz_offset, Some(120));
        assert!(!m.quicktime_utc);

        let only_create = json!({"QuickTime:CreationDate": "0000:00:00 00:00:00", "QuickTime:CreateDate": "2023:05:12 13:30:00"});
        let m = parse_exiftool_object(&only_create, true);
        assert_eq!(m.taken_at, Some(dt("2023-05-12 13:30:00")));
        assert!(m.quicktime_utc);
    }

    #[test]
    fn ignores_null_island_gps() {
        let m = parse_exiftool_object(&json!({"Composite:GPSLatitude": 0.0, "Composite:GPSLongitude": 0.0}), false);
        assert_eq!(m.gps_lat, None);
    }

    #[test]
    fn dates_from_filenames() {
        let cases = [
            ("IMG_20230512_101500.jpg", Some("2023-05-12 10:15:00")),
            ("VID_20230512_101500.mp4", Some("2023-05-12 10:15:00")),
            ("PXL_20230512_101500123.jpg", Some("2023-05-12 10:15:00")),
            ("20230512_101500.jpg", Some("2023-05-12 10:15:00")),
            ("Screenshot 2023-05-12 at 10.15.00.png", Some("2023-05-12 10:15:00")),
            ("2023-05-12 10.15.00.jpg", Some("2023-05-12 10:15:00")),
            ("IMG-20230512-WA0001.jpg", Some("2023-05-12 00:00:00")),
            ("IMG_0001.JPG", None),
            ("DSC_99999999.JPG", None),
            ("IMG_20231340_101500.jpg", None),
        ];
        for (name, expected) in cases {
            assert_eq!(date_from_filename(name), expected.map(dt), "{name}");
        }
    }

    #[test]
    fn fallback_order() {
        let mut m = ParsedMeta::default();
        apply_date_fallback(&mut m, "IMG_20230512_101500.jpg", 0);
        assert_eq!(m.date_source, Some(DateSource::Filename));

        let mut m = ParsedMeta::default();
        apply_date_fallback(&mut m, "IMG_0001.jpg", 1_700_000_000_000_000_000);
        assert_eq!(m.date_source, Some(DateSource::Mtime));
        assert!(m.taken_at.is_some() && m.tz_offset.is_some());
    }
}
