//! Zielpfad-Vorlagen und Windows-kompatible Bereinigung von Pfadteilen.
//!
//! Platzhalter: `{Jahr}`, `{Monat}`, `{Datum}` (YYYY-MM-DD), `{Anlass}`,
//! `{Kamera}`, `{Original-Dateiname}`. Der letzte Pfadteil muss
//! `{Original-Dateiname}` enthalten, damit zusammengehörige Dateien (RAW+JPEG,
//! Sidecars) ihre gemeinsamen Namen behalten.

use chrono::{Datelike, NaiveDate};
use unicode_normalization::UnicodeNormalization;

pub const DEFAULT_TEMPLATE: &str = "{Jahr}/{Datum} {Anlass}/{Original-Dateiname}";
pub const PLACEHOLDERS: &[&str] = &["{Jahr}", "{Monat}", "{Datum}", "{Anlass}", "{Kamera}", "{Original-Dateiname}"];
/// Längenlimit je Pfadteil (Zeichen). Hält auch tiefe Strukturen unter den
/// Grenzen gängiger Dateisysteme.
const MAX_SEGMENT: usize = 120;

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub struct Context<'a> {
    pub start: NaiveDate,
    /// Ende eines mehrtägigen Events (für die Schreibweise "von bis").
    pub end: NaiveDate,
    pub occasion: &'a str,
    pub camera: &'a str,
    pub original_name: &'a str,
    /// Mehrtägige Events als "2024-07-12 bis 2024-07-20" statt nur Startdatum.
    pub date_range: bool,
}

pub fn validate(template: &str) -> Result<(), String> {
    let t = template.trim();
    if t.is_empty() {
        return Err("Die Vorlage ist leer.".into());
    }
    if t.starts_with('/') || t.starts_with('\\') || t.contains(':') {
        return Err("Die Vorlage muss ein relativer Pfad sein.".into());
    }
    let segments: Vec<&str> = t.split(['/', '\\']).collect();
    if segments.iter().any(|s| s.trim() == ".." || s.trim() == ".") {
        return Err("„..“ und „.“ sind in der Vorlage nicht erlaubt.".into());
    }
    if !segments.last().is_some_and(|s| s.contains("{Original-Dateiname}")) {
        return Err("Der letzte Teil der Vorlage muss {Original-Dateiname} enthalten.".into());
    }
    let mut rest = t.to_string();
    for p in PLACEHOLDERS {
        rest = rest.replace(p, "");
    }
    if let Some(start) = rest.find('{') {
        let unknown: String = rest[start..].chars().take_while(|c| *c != '}').chain(Some('}')).collect();
        return Err(format!("Unbekannter Platzhalter {unknown}. Erlaubt: {}", PLACEHOLDERS.join(", ")));
    }
    Ok(())
}

/// Wendet die Vorlage an. Ergebnis: bereinigte Ordnerteile und Dateiname.
pub fn render(template: &str, ctx: &Context) -> (Vec<String>, String) {
    let date = if ctx.date_range && ctx.end != ctx.start {
        format!("{} bis {}", ctx.start.format("%Y-%m-%d"), ctx.end.format("%Y-%m-%d"))
    } else {
        ctx.start.format("%Y-%m-%d").to_string()
    };
    let fill = |s: &str| {
        s.replace("{Jahr}", &ctx.start.year().to_string())
            .replace("{Monat}", &format!("{:02}", ctx.start.month()))
            .replace("{Datum}", &date)
            .replace("{Anlass}", ctx.occasion)
            .replace("{Kamera}", ctx.camera)
    };
    let segments: Vec<&str> = template.trim().split(['/', '\\']).collect();
    let (file_tpl, dir_tpls) = segments.split_last().expect("validiert: nicht leer");
    let dirs: Vec<String> = dir_tpls.iter().map(|s| sanitize_segment(&fill(s))).filter(|s| s != "_").collect();
    // Den Originalnamen erst nach dem Füllen einsetzen: Er wird nur so weit
    // angefasst, wie es für das Zielsystem nötig ist.
    let prefix_suffix = fill(file_tpl);
    let file = if prefix_suffix == "{Original-Dateiname}" {
        sanitize_file_name(ctx.original_name)
    } else {
        sanitize_file_name(&prefix_suffix.replace("{Original-Dateiname}", ctx.original_name))
    };
    (dirs, file)
}

fn clean_chars(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_whitespace() { ' ' } else if c.is_control() || r#"<>:"/\|?*"#.contains(c) { '_' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn fix_reserved(name: String) -> String {
    let base = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&base.as_str()) { format!("_{name}") } else { name }
}

/// Ordnername: verbotene Zeichen ersetzen, Leerzeichen zusammenfassen, keine
/// Punkte/Leerzeichen am Ende (Windows), reservierte Namen entschärfen, NFC.
pub fn sanitize_segment(s: &str) -> String {
    let mut out: String = clean_chars(s).nfc().collect();
    out = out.trim_end_matches(['.', ' ']).trim_start().to_string();
    if out.chars().count() > MAX_SEGMENT {
        out = out.chars().take(MAX_SEGMENT).collect::<String>().trim_end_matches(['.', ' ']).to_string();
    }
    if out.is_empty() {
        return "_".into();
    }
    fix_reserved(out)
}

/// Dateiname: wie Ordnername, kürzt aber vor der Endung. Ein unproblematischer
/// Originalname bleibt Byte für Byte erhalten (auch seine Unicode-Form), damit
/// nichts unnötig umbenannt wird.
pub fn sanitize_file_name(name: &str) -> String {
    let needs_change = name.chars().any(|c| c.is_control() || r#"<>:"/\|?*"#.contains(c))
        || name.ends_with(['.', ' '])
        || name.starts_with(' ')
        || name.chars().count() > MAX_SEGMENT
        || RESERVED.contains(&name.split('.').next().unwrap_or("").to_ascii_uppercase().as_str())
        || name.is_empty();
    if !needs_change {
        return name.to_string();
    }
    let cleaned: String = clean_chars(name).trim_end_matches(['.', ' ']).trim_start().to_string();
    let (stem, ext) = match cleaned.rfind('.') {
        Some(i) if i > 0 => (&cleaned[..i], &cleaned[i..]),
        _ => (cleaned.as_str(), ""),
    };
    let budget = MAX_SEGMENT.saturating_sub(ext.chars().count()).max(1);
    let stem: String = stem.chars().take(budget).collect();
    let out = format!("{}{ext}", stem.trim_end_matches(['.', ' ']));
    if out.is_empty() || out == ext { fix_reserved(format!("_{ext}")) } else { fix_reserved(out) }
}

/// Hängt " (n)" an den gemeinsamen Basisnamen an: `IMG_1.CR2` → `IMG_1 (2).CR2`,
/// `IMG_1.CR2.xmp` → `IMG_1 (2).CR2.xmp`. So bleiben Gruppen erkennbar.
pub fn with_suffix(name: &str, base: &str, n: u32) -> String {
    if !base.is_empty() && name.starts_with(base) {
        format!("{base} ({n}){}", &name[base.len()..])
    } else {
        match name.rfind('.') {
            Some(i) if i > 0 => format!("{} ({n}){}", &name[..i], &name[i..]),
            _ => format!("{name} ({n})"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(occasion: &'a str, name: &'a str) -> Context<'a> {
        Context {
            start: NaiveDate::from_ymd_opt(2024, 7, 12).unwrap(),
            end: NaiveDate::from_ymd_opt(2024, 7, 20).unwrap(),
            occasion,
            camera: "Canon EOS R6",
            original_name: name,
            date_range: false,
        }
    }

    #[test]
    fn default_template() {
        let (dirs, file) = render(DEFAULT_TEMPLATE, &ctx("Kroatien", "IMG_0001.CR2"));
        assert_eq!(dirs, ["2024", "2024-07-12 Kroatien"]);
        assert_eq!(file, "IMG_0001.CR2");
    }

    #[test]
    fn empty_occasion_and_date_range() {
        let (dirs, _) = render(DEFAULT_TEMPLATE, &ctx("", "a.jpg"));
        assert_eq!(dirs, ["2024", "2024-07-12"], "kein Leerzeichen am Ende");
        let mut c = ctx("Kroatien", "a.jpg");
        c.date_range = true;
        assert_eq!(render(DEFAULT_TEMPLATE, &c).0[1], "2024-07-12 bis 2024-07-20 Kroatien");
    }

    #[test]
    fn other_placeholders_and_prefix() {
        let (dirs, file) = render("{Jahr}/{Monat}/{Kamera}/{Datum}_{Original-Dateiname}", &ctx("", "IMG_1.JPG"));
        assert_eq!(dirs, ["2024", "07", "Canon EOS R6"]);
        assert_eq!(file, "2024-07-12_IMG_1.JPG");
    }

    #[test]
    fn validation() {
        assert!(validate(DEFAULT_TEMPLATE).is_ok());
        assert!(validate("").is_err());
        assert!(validate("/abs/{Original-Dateiname}").is_err());
        assert!(validate("C:/x/{Original-Dateiname}").is_err());
        assert!(validate("{Jahr}/../{Original-Dateiname}").is_err());
        assert!(validate("{Jahr}/{Datum}").is_err(), "Dateiname fehlt");
        let err = validate("{Jahr}/{Ort}/{Original-Dateiname}").unwrap_err();
        assert!(err.contains("{Ort}"), "{err}");
    }

    #[test]
    fn sanitizes_for_windows() {
        assert_eq!(sanitize_segment("Feier: 50/50?"), "Feier_ 50_50_");
        assert_eq!(sanitize_segment("Urlaub. "), "Urlaub");
        assert_eq!(sanitize_segment("CON"), "_CON");
        assert_eq!(sanitize_segment("nul.txt"), "_nul.txt");
        assert_eq!(sanitize_segment("   "), "_");
        assert_eq!(sanitize_segment("a  \t b"), "a b");
        assert_eq!(sanitize_segment(&"x".repeat(300)).chars().count(), MAX_SEGMENT);
        // NFD wird zu NFC.
        assert_eq!(sanitize_segment("Gru\u{0308}n"), "Grün");
    }

    #[test]
    fn file_names_stay_untouched_unless_needed() {
        let nfd = "Gru\u{0308}n.jpg";
        assert_eq!(sanitize_file_name(nfd), nfd, "Originalnamen nicht unnötig umschreiben");
        assert_eq!(sanitize_file_name("a:b.jpg"), "a_b.jpg");
        assert_eq!(sanitize_file_name("AUX.JPG"), "_AUX.JPG");
        let long = format!("{}.jpeg", "y".repeat(300));
        let cut = sanitize_file_name(&long);
        assert!(cut.ends_with(".jpeg") && cut.chars().count() == MAX_SEGMENT, "{cut}");
    }

    #[test]
    fn suffix_keeps_group_names_aligned() {
        assert_eq!(with_suffix("IMG_1.CR2", "IMG_1", 2), "IMG_1 (2).CR2");
        assert_eq!(with_suffix("IMG_1.CR2.xmp", "IMG_1", 2), "IMG_1 (2).CR2.xmp");
        assert_eq!(with_suffix("anders.mov", "IMG_1", 3), "anders (3).mov");
        assert_eq!(with_suffix("README", "", 2), "README (2)");
    }
}
