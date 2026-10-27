//! Offline-Ortsnamen aus GeoNames (cities1000, CC BY 4.0) für Titelvorschläge.
//!
//! Die Daten werden beim ersten Gebrauch geladen und in 1°-Zellen einsortiert.
//! Gewählt wird nicht stur der nächste Ort: Im Zentrum von Split liegt oft ein
//! Vorort näher als "Split" selbst. Bewertet wird deshalb Größe gegen Abstand.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde::Serialize;

use super::cluster::distance_km;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Place {
    pub name: String,
    /// ISO-3166-Ländercode, z. B. "HR".
    pub country: String,
    #[serde(skip)]
    lat: f64,
    #[serde(skip)]
    lon: f64,
    #[serde(skip)]
    population: u32,
}

pub struct Places {
    cells: HashMap<(i32, i32), Vec<Place>>,
}

/// Größter Abstand, bis zu dem ein Ort als "hier" gilt.
const MAX_KM: f64 = 30.0;
/// Eine Zehnerpotenz mehr Einwohner wiegt 5 km Abstand auf.
const KM_PER_MAGNITUDE: f64 = 5.0;

fn cell(lat: f64, lon: f64) -> (i32, i32) {
    (lat.floor() as i32, lon.floor() as i32)
}

impl Places {
    pub fn parse(tsv: &str) -> Self {
        let mut cells: HashMap<(i32, i32), Vec<Place>> = HashMap::new();
        for line in tsv.lines() {
            let mut f = line.split('\t');
            let (Some(name), Some(lat), Some(lon), Some(cc), pop) = (f.next(), f.next(), f.next(), f.next(), f.next()) else { continue };
            let (Ok(lat), Ok(lon)) = (lat.parse::<f64>(), lon.parse::<f64>()) else { continue };
            let place = Place { name: name.to_string(), country: cc.to_string(), lat, lon, population: pop.and_then(|p| p.parse().ok()).unwrap_or(0) };
            cells.entry(cell(lat, lon)).or_default().push(place);
        }
        Self { cells }
    }

    pub fn load(path: &Path) -> std::io::Result<Self> {
        Ok(Self::parse(&std::fs::read_to_string(path)?))
    }

    pub fn len(&self) -> usize {
        self.cells.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Passendster Ort im Umkreis von 30 km.
    pub fn lookup(&self, lat: f64, lon: f64) -> Option<&Place> {
        let (cy, cx) = cell(lat, lon);
        // Am Polarkreis ist ein Längengrad schmal: zwei Zellen zur Seite genügen bis ca. 60° N.
        let candidates = (-1..=1).flat_map(|dy| (-2..=2).map(move |dx| (cy + dy, cx + dx))).filter_map(|c| self.cells.get(&c)).flatten();
        candidates
            .filter_map(|p| {
                let d = distance_km((lat, lon), (p.lat, p.lon));
                (d <= MAX_KM).then(|| (p, (p.population.max(1) as f64).log10() - d / KM_PER_MAGNITUDE))
            })
            .max_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(p, _)| p)
    }
}

static PATH: OnceLock<PathBuf> = OnceLock::new();
static PLACES: OnceLock<Option<Places>> = OnceLock::new();

/// Legt fest, wo die Ortsdaten liegen (gebündelte Ressource). Ohne Aufruf wird
/// in Debug-Builds `src-tauri/resources/geo` verwendet.
pub fn init(resource_dir: Option<&Path>) {
    if let Some(dir) = resource_dir {
        let _ = PATH.set(dir.join("geo").join("cities1000.tsv"));
    }
}

/// Ortsdaten, falls vorhanden. Fehlen sie, gibt es eben keine Ortsvorschläge.
pub fn get() -> Option<&'static Places> {
    PLACES
        .get_or_init(|| {
            let candidates = PATH
                .get()
                .cloned()
                .into_iter()
                .chain(cfg!(debug_assertions).then(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/geo/cities1000.tsv")));
            candidates.filter(|p| p.is_file()).find_map(|p| Places::load(&p).ok())
        })
        .as_ref()
}

/// Ort zum Schwerpunkt mehrerer Koordinaten.
pub fn place_for(points: &[(f64, f64)]) -> Option<Place> {
    if points.is_empty() {
        return None;
    }
    let n = points.len() as f64;
    let (lat, lon) = (points.iter().map(|p| p.0).sum::<f64>() / n, points.iter().map(|p| p.1).sum::<f64>() / n);
    get()?.lookup(lat, lon).cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "Split\t43.50891\t16.43915\tHR\t149830\n\
                          Kaštel Sućurac\t43.54694\t16.42361\tHR\t6000\n\
                          Podstrana\t43.48694\t16.55333\tHR\t9000\n\
                          Trogir\t43.5125\t16.25167\tHR\t10900\n\
                          Berlin\t52.52437\t13.41053\tDE\t3426354\n";

    #[test]
    fn prefers_the_city_over_a_slightly_closer_village() {
        let places = Places::parse(SAMPLE);
        // Zwischen Split und Kaštel Sućurac, etwas näher am Dorf.
        assert_eq!(places.lookup(43.535, 16.43).unwrap().name, "Split");
        // Direkt in Trogir.
        assert_eq!(places.lookup(43.516, 16.25).unwrap().name, "Trogir");
        // Mitten auf der Adria: nichts im Umkreis.
        assert!(places.lookup(43.0, 15.0).is_none());
    }

    #[test]
    fn real_dataset_if_available() {
        let Some(places) = get() else { return }; // ohne scripts/fetch-geonames.sh nicht vorhanden
        assert!(places.len() > 100_000);
        let berlin = places.lookup(52.5163, 13.3777).unwrap(); // Brandenburger Tor
        assert_eq!((berlin.name.as_str(), berlin.country.as_str()), ("Berlin", "DE"));
        assert_eq!(places.lookup(43.5081, 16.4402).unwrap().name, "Split");
    }
}
