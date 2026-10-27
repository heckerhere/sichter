//! Reiner Clustering-Algorithmus (ohne Datenbank).

use chrono::{Duration, Local, NaiveDateTime, TimeZone};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    /// Neue Gruppe ab dieser Pause zwischen zwei Aufnahmen.
    pub gap_hours: f64,
    /// Neue Gruppe ab dieser Entfernung zur letzten Aufnahme mit GPS.
    pub gps_km: f64,
    /// Aufeinanderfolgende Tage am selben Ort zu einem Event zusammenfassen.
    pub merge_multiday: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self { gap_hours: 4.0, gps_km: 30.0, merge_multiday: true }
    }
}

#[derive(Debug, Clone)]
pub struct Item {
    pub group_id: i64,
    pub time: NaiveDateTime,
    pub gps: Option<(f64, f64)>,
    /// Name des Ordners, in dem die Datei liegt (für den Titelvorschlag).
    pub folder: String,
}

/// Entfernung zweier Koordinaten in km (Haversine).
pub fn distance_km(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1, lat2, lon2) = (a.0.to_radians(), a.1.to_radians(), b.0.to_radians(), b.1.to_radians());
    let h = ((lat2 - lat1) / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    2.0 * 6371.0 * h.sqrt().asin()
}

fn centroid(items: &[Item]) -> Option<(f64, f64)> {
    let points: Vec<_> = items.iter().filter_map(|i| i.gps).collect();
    if points.is_empty() {
        return None;
    }
    let n = points.len() as f64;
    Some((points.iter().map(|p| p.0).sum::<f64>() / n, points.iter().map(|p| p.1).sum::<f64>() / n))
}

/// Teilt nach Zeitlücke und GPS-Sprung, fasst danach optional mehrtägige
/// Aufenthalte am selben Ort zusammen. Ergebnis zeitlich sortiert.
pub fn cluster(mut items: Vec<Item>, settings: &Settings) -> Vec<Vec<Item>> {
    items.sort_by(|a, b| a.time.cmp(&b.time).then(a.group_id.cmp(&b.group_id)));
    let gap = Duration::seconds((settings.gap_hours * 3600.0) as i64);
    let mut clusters: Vec<Vec<Item>> = Vec::new();
    let mut last_gps: Option<(f64, f64)> = None;
    for item in items {
        let split = match clusters.last().and_then(|c| c.last()) {
            None => true,
            Some(prev) => {
                item.time - prev.time > gap
                    || matches!((last_gps, item.gps), (Some(a), Some(b)) if distance_km(a, b) > settings.gps_km)
            }
        };
        if split {
            clusters.push(Vec::new());
            last_gps = None;
        }
        if item.gps.is_some() {
            last_gps = item.gps;
        }
        clusters.last_mut().expect("eben angelegt").push(item);
    }

    if !settings.merge_multiday {
        return clusters;
    }
    // Mehrtägig: Folgegruppe beginnt am selben oder nächsten Kalendertag und
    // liegt (beide mit GPS) nahe am Schwerpunkt der bisherigen Gruppe.
    let mut merged: Vec<Vec<Item>> = Vec::new();
    for c in clusters {
        if let Some(prev) = merged.last_mut() {
            let prev_day = prev.last().expect("nicht leer").time.date();
            let next_day = c[0].time.date();
            let consecutive = next_day - prev_day <= Duration::days(1);
            let near = matches!((centroid(prev), centroid(&c)), (Some(a), Some(b)) if distance_km(a, b) <= settings.gps_km);
            // Am selben Tag hat die Zeitlücke bewusst getrennt. Nur wenn die
            // bisherige Gruppe schon über mehrere Tage geht (also eine Reise ist),
            // gehört auch eine Pause innerhalb eines Tages dazu.
            let spans_days = prev[0].time.date() != prev_day;
            if consecutive && near && (next_day != prev_day || spans_days) {
                prev.extend(c);
                continue;
            }
        }
        merged.push(c);
    }
    merged
}

const GENERIC_FOLDERS: &[&str] = &[
    "dcim", "camera", "kamera", "bilder", "fotos", "photos", "pictures", "images", "import", "imports", "iphone", "android",
    "whatsapp images", "whatsapp", "screenshots", "misc", "raw", "video", "videos", "jpg", "jpeg", "neuer ordner", "new folder",
    "unbenannt", "untitled", "desktop", "downloads", "export", "originals", "masters",
];

/// Ordnername ohne führendes Datum; leer, wenn der Name nichts aussagt
/// (DCIM, 100CANON, "Bilder" …).
pub fn title_from_folder(folder: &str) -> String {
    let mut rest = folder.trim();
    // Datumspräfix wie "2023-05-12 ", "2023_05 ", "20230512-" oder "2023 " entfernen.
    let digits = rest.chars().take_while(|c| c.is_ascii_digit() || matches!(c, '-' | '_' | '.')).count();
    if digits >= 4 && rest[..digits].chars().filter(char::is_ascii_digit).count() >= 4 {
        rest = rest[digits..].trim_start_matches([' ', '-', '_', '.']).trim();
    }
    let lower = rest.to_lowercase();
    let camera_folder = lower.len() >= 3 && lower[..3].chars().all(|c| c.is_ascii_digit()) && lower[3..].chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if rest.is_empty() || camera_folder || GENERIC_FOLDERS.contains(&lower.as_str()) {
        return String::new();
    }
    rest.to_string()
}

/// Häufigster aussagekräftiger Ordnername einer Gruppe von Aufnahmen.
pub fn suggest_title(items: &[Item]) -> String {
    let mut counts: Vec<(String, usize)> = Vec::new();
    for item in items {
        let t = title_from_folder(&item.folder);
        if t.is_empty() {
            continue;
        }
        match counts.iter_mut().find(|(name, _)| *name == t) {
            Some(entry) => entry.1 += 1,
            None => counts.push((t, 1)),
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(t, _)| t).unwrap_or_default()
}

/// Wie die Zeit eines Videos ohne Zeitzone zu lesen ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TzMode {
    Utc,
    Local,
}

impl TzMode {
    pub fn as_str(self) -> &'static str {
        match self {
            TzMode::Utc => "utc",
            TzMode::Local => "local",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "utc" => Some(TzMode::Utc),
            "local" => Some(TzMode::Local),
            _ => None,
        }
    }
}

const PHONE_MAKERS: &[&str] = &["apple", "google", "samsung", "xiaomi", "huawei", "oneplus", "oppo", "motorola", "realme", "honor", "nothing"];

/// Standard: Handys halten sich an die Spezifikation (UTC), Kameras schreiben
/// meist Ortszeit. Pro Kameramodell überschreibbar.
pub fn default_tz_mode(make: Option<&str>) -> TzMode {
    let make = make.unwrap_or_default().to_lowercase();
    if PHONE_MAKERS.iter().any(|m| make.contains(m)) { TzMode::Utc } else { TzMode::Local }
}

/// Wanduhrzeit einer Aufnahme. Nur QuickTime-Zeiten ohne Zeitzone werden je
/// nach Modus von UTC in die Zeitzone dieses Rechners umgerechnet.
pub fn effective_time(taken_at: NaiveDateTime, quicktime_utc: bool, mode: TzMode) -> NaiveDateTime {
    if quicktime_utc && mode == TzMode::Utc {
        Local.from_utc_datetime(&taken_at).naive_local()
    } else {
        taken_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M").unwrap()
    }

    fn item(id: i64, time: &str, gps: Option<(f64, f64)>, folder: &str) -> Item {
        Item { group_id: id, time: t(time), gps, folder: folder.into() }
    }

    const BERLIN: (f64, f64) = (52.52, 13.405);
    const HAMBURG: (f64, f64) = (53.55, 9.99);
    const SPLIT: (f64, f64) = (43.508, 16.44);
    const TROGIR: (f64, f64) = (43.517, 16.251); // ca. 15 km von Split

    #[test]
    fn haversine_is_plausible() {
        let d = distance_km(BERLIN, HAMBURG);
        assert!((250.0..260.0).contains(&d), "{d}");
    }

    #[test]
    fn three_days_and_a_trip() {
        let items = vec![
            // Tag 1: Geburtstag, danach 6 h Pause bis zum Abend.
            item(1, "2023-05-12 15:00", Some(BERLIN), "2023-05-12 Geburtstag"),
            item(2, "2023-05-12 16:10", Some(BERLIN), "2023-05-12 Geburtstag"),
            item(3, "2023-05-12 17:30", None, "2023-05-12 Geburtstag"),
            item(4, "2023-05-12 23:45", Some(BERLIN), "DCIM"),
            // Tag 2: Hamburg, dann am selben Tag 2 h später Berlin (GPS-Sprung).
            item(5, "2023-06-03 10:00", Some(HAMBURG), "Hamburg"),
            item(6, "2023-06-03 11:00", Some(HAMBURG), "Hamburg"),
            item(7, "2023-06-03 13:00", Some(BERLIN), "Hamburg"),
            // Reise: vier Tage um Split, über Nacht jeweils > 4 h Pause.
            item(8, "2023-07-12 18:00", Some(SPLIT), "2023-07 Kroatien"),
            item(9, "2023-07-13 09:00", Some(TROGIR), "2023-07 Kroatien"),
            item(10, "2023-07-13 19:00", Some(SPLIT), "2023-07 Kroatien"),
            item(11, "2023-07-14 10:00", None, "2023-07 Kroatien"),
            item(12, "2023-07-14 11:00", Some(SPLIT), "2023-07 Kroatien"),
            item(13, "2023-07-15 12:00", Some(SPLIT), "100CANON"),
        ];
        let clusters = cluster(items, &Settings::default());
        let ids: Vec<Vec<i64>> = clusters.iter().map(|c| c.iter().map(|i| i.group_id).collect()).collect();
        assert_eq!(ids, vec![vec![1, 2, 3], vec![4], vec![5, 6], vec![7], vec![8, 9, 10, 11, 12, 13]]);
        assert_eq!(suggest_title(&clusters[0]), "Geburtstag");
        assert_eq!(suggest_title(&clusters[1]), "");
        assert_eq!(suggest_title(&clusters[4]), "Kroatien");
    }

    #[test]
    fn settings_change_result() {
        let items = vec![item(1, "2023-05-12 10:00", None, ""), item(2, "2023-05-12 13:00", None, "")];
        assert_eq!(cluster(items.clone(), &Settings::default()).len(), 1);
        assert_eq!(cluster(items, &Settings { gap_hours: 2.0, ..Settings::default() }).len(), 2);

        let far = vec![item(1, "2023-05-12 10:00", Some(BERLIN), ""), item(2, "2023-05-12 10:30", Some(HAMBURG), "")];
        assert_eq!(cluster(far.clone(), &Settings::default()).len(), 2);
        assert_eq!(cluster(far, &Settings { gps_km: 500.0, ..Settings::default() }).len(), 1);
    }

    #[test]
    fn multiday_merge_needs_gps_and_consecutive_days() {
        let trip = vec![item(1, "2023-07-12 18:00", Some(SPLIT), ""), item(2, "2023-07-13 09:00", Some(SPLIT), "")];
        assert_eq!(cluster(trip.clone(), &Settings::default()).len(), 1);
        assert_eq!(cluster(trip, &Settings { merge_multiday: false, ..Settings::default() }).len(), 2);
        // Ohne GPS keine Vermutung über einen Aufenthalt.
        let no_gps = vec![item(1, "2023-07-12 18:00", None, ""), item(2, "2023-07-13 09:00", None, "")];
        assert_eq!(cluster(no_gps, &Settings::default()).len(), 2);
        // Zwei Tage Pause: kein durchgehender Aufenthalt.
        let gap = vec![item(1, "2023-07-12 18:00", Some(SPLIT), ""), item(2, "2023-07-14 09:00", Some(SPLIT), "")];
        assert_eq!(cluster(gap, &Settings::default()).len(), 2);
    }

    #[test]
    fn folder_titles() {
        assert_eq!(title_from_folder("2023-05-12 Geburtstag Leo"), "Geburtstag Leo");
        assert_eq!(title_from_folder("20230512_Hochzeit"), "Hochzeit");
        assert_eq!(title_from_folder("2023 Urlaub"), "Urlaub");
        assert_eq!(title_from_folder("2023-05-12"), "");
        assert_eq!(title_from_folder("100CANON"), "");
        assert_eq!(title_from_folder("101_PANA"), "");
        assert_eq!(title_from_folder("DCIM"), "");
        assert_eq!(title_from_folder("Bilder"), "");
        assert_eq!(title_from_folder("Fotobuch Leo"), "Fotobuch Leo");
    }

    #[test]
    fn tz_defaults_and_conversion() {
        assert_eq!(default_tz_mode(Some("Apple")), TzMode::Utc);
        assert_eq!(default_tz_mode(Some("samsung")), TzMode::Utc);
        assert_eq!(default_tz_mode(Some("Canon")), TzMode::Local);
        assert_eq!(default_tz_mode(None), TzMode::Local);
        let time = t("2023-07-13 10:00");
        assert_eq!(effective_time(time, false, TzMode::Utc), time, "nur QuickTime-UTC-Zeiten werden umgerechnet");
        assert_eq!(effective_time(time, true, TzMode::Local), time);
        let local = effective_time(time, true, TzMode::Utc);
        let offset = Local.from_utc_datetime(&time).offset().local_minus_utc() as i64;
        assert_eq!(local - time, Duration::seconds(offset));
    }
}
