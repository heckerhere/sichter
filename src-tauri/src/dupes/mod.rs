//! Datei- und Ordner-Duplikate (PLAN.md, Abschnitt 6.4). Rein lesend.
//!
//! * Dateien: gleiche Größe, dann gleicher BLAKE3-Hash.
//! * Ordner 1:1: Hash über die sortierte Liste (relativer Pfad, Datei-Hash) des
//!   gesamten Unterbaums.
//! * Inhaltsgleich: dasselbe nur über die Datei-Hashes, Namen egal.
//! * Teilmenge: Alle Inhalte von Ordner A stecken auch in Ordner B.
//!
//! Nicht gehashte Dateien (unbekannte Typen) gehen mit Name und Größe in den
//! Ordnervergleich ein. Gemeldet werden jeweils nur die obersten Ordnerpaare:
//! Sind zwei Ordner identisch, sind es ihre Unterordner automatisch auch.
//!
//! Vorschläge, welche Kopie bleibt, werden nie automatisch übernommen.

use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{Connection, params};
use serde::Serialize;

#[derive(Debug, Clone)]
pub struct FileRec {
    pub asset_id: i64,
    pub root_id: i64,
    pub rel_path: String,
    pub size: i64,
    pub hash: Option<String>,
    pub mtime: i64,
    pub taken_at: Option<String>,
    pub thumb: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize)]
pub struct FolderKey {
    pub root_id: i64,
    /// Relativer Ordnerpfad, `""` für den Root.
    pub rel_path: String,
}

impl FolderKey {
    fn parent(&self) -> Option<FolderKey> {
        if self.rel_path.is_empty() {
            return None;
        }
        let parent = self.rel_path.rsplit_once('/').map_or("", |(p, _)| p);
        Some(FolderKey { root_id: self.root_id, rel_path: parent.to_string() })
    }

    /// `self` liegt (echt) unterhalb von `other`.
    fn is_inside(&self, other: &FolderKey) -> bool {
        self.root_id == other.root_id
            && self.rel_path != other.rel_path
            && (other.rel_path.is_empty() || self.rel_path.starts_with(&format!("{}/", other.rel_path)))
    }
}

#[derive(Debug, Serialize)]
pub struct DupeFile {
    pub asset_id: i64,
    pub root_id: i64,
    pub rel_path: String,
    pub mtime: i64,
    pub taken_at: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct FileGroup {
    pub hash: String,
    pub size: i64,
    pub thumb: Option<String>,
    /// Vorgeschlagene Kopie zuerst.
    pub files: Vec<DupeFile>,
    pub suggestion_reason: String,
    /// Vom Nutzer gewählte Kopie, falls er eine gewählt hat.
    pub keep_asset_id: Option<i64>,
    /// Platz, der frei würde, wenn nur eine Kopie bliebe.
    pub redundant_size: i64,
}

#[derive(Debug, Serialize)]
pub struct FolderInfo {
    pub key: FolderKey,
    pub file_count: usize,
    pub size: i64,
}

#[derive(Debug, Serialize)]
pub struct FolderPair {
    /// Vorgeschlagen zum Behalten.
    pub keep: FolderInfo,
    pub other: FolderInfo,
    pub suggestion_reason: String,
}

#[derive(Debug, Serialize)]
pub struct SubsetPair {
    /// Kleinerer Ordner, dessen Inhalt vollständig in `superset` steckt.
    pub subset: FolderInfo,
    pub superset: FolderInfo,
    /// Anteil von `superset`, der in `subset` vorhanden ist (0–1).
    pub coverage: f64,
}

#[derive(Debug, Default, Serialize)]
pub struct Analysis {
    pub file_groups: Vec<FileGroup>,
    pub identical_folders: Vec<FolderPair>,
    pub same_content_folders: Vec<FolderPair>,
    pub subset_folders: Vec<SubsetPair>,
    pub redundant_size: i64,
}

// ---------------------------------------------------------------------------
// Vorschlag "welche Kopie bleibt"
// ---------------------------------------------------------------------------

const MESSY_WORDS: &[&str] = &[
    "kopie", "copy", "backup", "sicherung", "duplikat", "duplicate", "downloads", "download", "tmp", "temp",
    "neuer ordner", "new folder", "unbenannt", "untitled", "export", "desktop", "papierkorb", "trash", "alt", "old",
];

/// Je kleiner, desto "ordentlicher". Liefert Punktzahl und Begründung.
fn messiness(path: &str) -> (i32, Vec<&'static str>) {
    let lower = path.to_lowercase();
    let mut score = 0;
    let mut reasons = Vec::new();
    let words: Vec<&str> = lower.split(['/', ' ', '_', '-', '.', '(', ')']).filter(|w| !w.is_empty()).collect();
    if MESSY_WORDS.iter().any(|m| if m.contains(' ') { lower.contains(m) } else { words.contains(m) }) {
        score += 10;
        reasons.push("Pfad enthält Wörter wie „Kopie“, „Backup“ oder „Downloads“");
    }
    // Nummerierte Kopien: "Bild (2).jpg", "Bild-1.jpg", "Bild copy 2.jpg".
    let name = lower.rsplit('/').next().unwrap_or(&lower);
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let numbered = stem.ends_with(')') && stem.contains(" (")
        || stem.rsplit_once('-').is_some_and(|(_, n)| n.len() <= 2 && n.chars().all(|c| c.is_ascii_digit()));
    if numbered {
        score += 5;
        reasons.push("Dateiname sieht nach nummerierter Kopie aus");
    }
    // Datumsordner (2023, 2023-05, 2023-05-12 …) gelten als sortiert.
    let dated = path.split('/').any(|seg| seg.len() >= 4 && seg[..4].chars().all(|c| c.is_ascii_digit()) && seg.starts_with(['1', '2']));
    if dated {
        score -= 3;
    }
    (score, reasons)
}

fn suggest<T>(items: &mut [T], path: impl Fn(&T) -> String, mtime: impl Fn(&T) -> i64) -> String {
    items.sort_by(|a, b| {
        messiness(&path(a))
            .0
            .cmp(&messiness(&path(b)).0)
            .then_with(|| mtime(a).cmp(&mtime(b)))
            .then_with(|| path(a).cmp(&path(b)))
    });
    let keep = messiness(&path(&items[0]));
    let worst = items[1..].iter().map(|i| messiness(&path(i))).max_by_key(|m| m.0);
    match worst {
        Some((score, reasons)) if score > keep.0 && !reasons.is_empty() => format!("Andere Kopie: {}", reasons.join(", ")),
        Some((score, _)) if score > keep.0 => "Liegt in einem nach Datum sortierten Ordner".into(),
        _ => "Älteste Kopie".into(),
    }
}

// ---------------------------------------------------------------------------
// Analyse
// ---------------------------------------------------------------------------

fn file_groups(files: &[FileRec]) -> Vec<FileGroup> {
    // Größen-Vorfilter, dann Hash.
    let mut by_size: HashMap<i64, Vec<&FileRec>> = HashMap::new();
    for f in files.iter().filter(|f| f.hash.is_some()) {
        by_size.entry(f.size).or_default().push(f);
    }
    let mut groups = Vec::new();
    for same_size in by_size.into_values().filter(|v| v.len() > 1) {
        let mut by_hash: HashMap<&str, Vec<&FileRec>> = HashMap::new();
        for f in same_size {
            by_hash.entry(f.hash.as_deref().unwrap_or_default()).or_default().push(f);
        }
        for (hash, mut members) in by_hash.into_iter().filter(|(_, v)| v.len() > 1) {
            let reason = suggest(&mut members, |f| f.rel_path.clone(), |f| f.mtime);
            let size = members[0].size;
            groups.push(FileGroup {
                hash: hash.to_string(),
                size,
                thumb: members.iter().find_map(|f| f.thumb.clone()),
                redundant_size: size * (members.len() as i64 - 1),
                files: members
                    .iter()
                    .map(|f| DupeFile {
                        asset_id: f.asset_id,
                        root_id: f.root_id,
                        rel_path: f.rel_path.clone(),
                        mtime: f.mtime,
                        taken_at: f.taken_at.clone(),
                    })
                    .collect(),
                suggestion_reason: reason,
                keep_asset_id: None,
            });
        }
    }
    groups.sort_by(|a, b| b.redundant_size.cmp(&a.redundant_size).then_with(|| a.hash.cmp(&b.hash)));
    groups
}

/// Inhalt eines Ordners (rekursiv): (Pfad relativ zum Ordner, Inhaltsschlüssel, Größe).
type Entry = (String, String, i64);

fn folder_contents(files: &[FileRec]) -> BTreeMap<FolderKey, Vec<Entry>> {
    let mut map: BTreeMap<FolderKey, Vec<Entry>> = BTreeMap::new();
    for f in files {
        // Nicht gehashte Dateien: Name und Größe als Ersatzschlüssel.
        let content = f.hash.clone().unwrap_or_else(|| format!("?{}:{}", f.size, f.rel_path.rsplit('/').next().unwrap_or("")));
        let dir = f.rel_path.rsplit_once('/').map_or("", |(d, _)| d);
        let mut key = Some(FolderKey { root_id: f.root_id, rel_path: dir.to_string() });
        while let Some(k) = key {
            let rel = if k.rel_path.is_empty() { f.rel_path.clone() } else { f.rel_path[k.rel_path.len() + 1..].to_string() };
            map.entry(k.clone()).or_default().push((rel, content.clone(), f.size));
            key = k.parent();
        }
    }
    for entries in map.values_mut() {
        entries.sort();
    }
    map
}

fn digest(parts: impl Iterator<Item = String>) -> String {
    let mut h = blake3::Hasher::new();
    for p in parts {
        h.update(p.as_bytes());
        h.update(b"\0");
    }
    h.finalize().to_hex().to_string()
}

fn info(key: &FolderKey, entries: &[Entry]) -> FolderInfo {
    FolderInfo { key: key.clone(), file_count: entries.len(), size: entries.iter().map(|e| e.2).sum() }
}

/// Ordnerpaare mit gleichem Schlüssel, ohne Paare, deren Eltern ebenfalls ein
/// Paar bilden (die sind dann schon abgedeckt).
fn matching_pairs(
    contents: &BTreeMap<FolderKey, Vec<Entry>>,
    key_of: impl Fn(&[Entry]) -> String,
    exclude: &HashSet<(FolderKey, FolderKey)>,
) -> (Vec<FolderPair>, HashSet<(FolderKey, FolderKey)>) {
    let keys: HashMap<&FolderKey, String> = contents.iter().map(|(k, e)| (k, key_of(e))).collect();
    let mut buckets: HashMap<&str, Vec<&FolderKey>> = HashMap::new();
    for (k, digest) in &keys {
        buckets.entry(digest.as_str()).or_default().push(k);
    }
    let same = |a: &FolderKey, b: &FolderKey| keys.get(a).is_some_and(|ka| keys.get(b) == Some(ka));
    let mut all = HashSet::new();
    let mut pairs = Vec::new();
    for members in buckets.into_values().filter(|m| m.len() > 1) {
        for (i, a) in members.iter().enumerate() {
            for b in &members[i + 1..] {
                if a.is_inside(b) || b.is_inside(a) {
                    continue;
                }
                let (a, b) = if a < b { (*a, *b) } else { (*b, *a) };
                all.insert((a.clone(), b.clone()));
                if exclude.contains(&(a.clone(), b.clone())) {
                    continue;
                }
                let parents_match = matches!((a.parent(), b.parent()), (Some(pa), Some(pb)) if pa != pb && same(&pa, &pb));
                if parents_match {
                    continue;
                }
                let mut two = [(a, &contents[a]), (b, &contents[b])];
                let path = |(k, _): &(&FolderKey, &Vec<Entry>)| format!("{}/{}", k.root_id, k.rel_path);
                let reason = suggest(&mut two, path, |_| 0);
                pairs.push(FolderPair { keep: info(two[0].0, two[0].1), other: info(two[1].0, two[1].1), suggestion_reason: reason });
            }
        }
    }
    pairs.sort_by(|x, y| y.keep.size.cmp(&x.keep.size).then_with(|| x.keep.key.cmp(&y.keep.key)));
    (pairs, all)
}

fn subset_pairs(contents: &BTreeMap<FolderKey, Vec<Entry>>, equal: &HashSet<(FolderKey, FolderKey)>) -> Vec<SubsetPair> {
    let sets: HashMap<&FolderKey, HashSet<&str>> =
        contents.iter().map(|(k, e)| (k, e.iter().map(|(_, c, _)| c.as_str()).collect())).collect();
    let mut holders: HashMap<&str, Vec<&FolderKey>> = HashMap::new();
    for (k, set) in &sets {
        for c in set {
            holders.entry(c).or_default().push(k);
        }
    }
    let is_equal = |a: &FolderKey, b: &FolderKey| {
        let (x, y) = if a < b { (a, b) } else { (b, a) };
        equal.contains(&(x.clone(), y.clone()))
    };
    let contains = |sup: &FolderKey, sub: &FolderKey| sets[sub].iter().all(|c| sets[sup].contains(c));
    // Gleiche Partner je Ordner. Hat A einen identischen Zwilling innerhalb von B,
    // ist "A steckt in B" schon durch das Ordnerpaar erklärt.
    let mut twins: HashMap<&FolderKey, Vec<&FolderKey>> = HashMap::new();
    for (x, y) in equal {
        twins.entry(x).or_default().push(y);
        twins.entry(y).or_default().push(x);
    }
    let explained = |a: &FolderKey, b: &FolderKey| twins.get(a).is_some_and(|t| t.iter().any(|c| c.is_inside(b)));

    let mut found: Vec<(&FolderKey, &FolderKey)> = Vec::new();
    for (a, set) in &sets {
        // Einzeldateien zeigt schon die Dateiliste.
        if set.len() < 2 {
            continue;
        }
        let rarest = set.iter().min_by_key(|c| holders[*c].len()).expect("nicht leer");
        let mut supersets: Vec<&FolderKey> = holders[rarest]
            .iter()
            .copied()
            .filter(|b| *b != *a && !a.is_inside(b) && !b.is_inside(a))
            .filter(|b| sets[*b].len() > set.len() && !is_equal(a, b) && !explained(a, b))
            .filter(|b| contains(b, a))
            .collect();
        // Nur das kleinste B: Enthält schon ein Unterordner von B alles, ist B überflüssig.
        let snapshot = supersets.clone();
        supersets.retain(|b| !snapshot.iter().any(|c| c.is_inside(b)));
        for b in supersets {
            found.push((a, b));
        }
    }
    // Nur das größte A: Steckt schon der Elternordner von A in B, ist A abgedeckt.
    let pairs: HashSet<(&FolderKey, &FolderKey)> = found.iter().copied().collect();
    let mut out: Vec<SubsetPair> = found
        .into_iter()
        .filter(|(a, b)| !a.parent().is_some_and(|pa| sets.get(&pa).is_some_and(|_| pairs.contains(&(&pa, *b)))))
        .map(|(a, b)| SubsetPair {
            coverage: sets[a].len() as f64 / sets[b].len() as f64,
            subset: info(a, &contents[a]),
            superset: info(b, &contents[b]),
        })
        .collect();
    out.sort_by(|x, y| y.subset.size.cmp(&x.subset.size).then_with(|| x.subset.key.cmp(&y.subset.key)));
    out
}

pub fn analyze(files: &[FileRec]) -> Analysis {
    let file_groups = file_groups(files);
    let contents = folder_contents(files);
    let (identical_folders, identical_all) =
        matching_pairs(&contents, |e| digest(e.iter().map(|(p, c, _)| format!("{p}\t{c}"))), &HashSet::new());
    let (same_content_folders, same_all) = matching_pairs(
        &contents,
        |e| {
            let mut c: Vec<String> = e.iter().map(|(_, c, _)| c.clone()).collect();
            c.sort();
            digest(c.into_iter())
        },
        &identical_all,
    );
    let equal: HashSet<_> = identical_all.union(&same_all).cloned().collect();
    let subset_folders = subset_pairs(&contents, &equal);
    Analysis {
        redundant_size: file_groups.iter().map(|g| g.redundant_size).sum(),
        file_groups,
        identical_folders,
        same_content_folders,
        subset_folders,
    }
}

pub fn load_files(conn: &Connection) -> rusqlite::Result<Vec<FileRec>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.root_id, a.rel_path, a.size, a.hash, a.mtime, m.taken_at,
                (SELECT t.hash FROM thumb t WHERE t.hash = a.hash AND t.status = 'ok')
         FROM asset a LEFT JOIN meta m ON m.asset_id = a.id",
    )?;
    stmt.query_map(params![], |r| {
        Ok(FileRec {
            asset_id: r.get(0)?,
            root_id: r.get(1)?,
            rel_path: r.get(2)?,
            size: r.get(3)?,
            hash: r.get(4)?,
            mtime: r.get(5)?,
            taken_at: r.get(6)?,
            thumb: r.get(7)?,
        })
    })?
    .collect()
}

/// Analyse samt gespeicherter Nutzerauswahl.
pub fn analyze_db(conn: &Connection) -> rusqlite::Result<Analysis> {
    let mut analysis = analyze(&load_files(conn)?);
    let mut stmt = conn.prepare("SELECT hash, keep_asset_id FROM dupe_choice")?;
    let choices: HashMap<String, i64> = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?;
    for g in &mut analysis.file_groups {
        // Nur gültige Auswahl übernehmen (die Datei kann inzwischen fehlen).
        g.keep_asset_id = choices.get(&g.hash).copied().filter(|id| g.files.iter().any(|f| f.asset_id == *id));
    }
    Ok(analysis)
}

/// Speichert, welche Kopie bleiben soll (`None` = Auswahl zurücksetzen).
pub fn set_choice(conn: &Connection, hash: &str, keep_asset_id: Option<i64>) -> rusqlite::Result<()> {
    match keep_asset_id {
        Some(id) => {
            // Nur Assets, die wirklich diesen Inhalt haben.
            let ok: bool = conn.query_row("SELECT count(*) > 0 FROM asset WHERE id = ?1 AND hash = ?2", params![id, hash], |r| r.get(0))?;
            if !ok {
                return Err(rusqlite::Error::QueryReturnedNoRows);
            }
            conn.execute(
                "INSERT INTO dupe_choice (hash, keep_asset_id) VALUES (?1, ?2)
                 ON CONFLICT (hash) DO UPDATE SET keep_asset_id = excluded.keep_asset_id, updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')",
                params![hash, id],
            )?;
        }
        None => {
            conn.execute("DELETE FROM dupe_choice WHERE hash = ?1", [hash])?;
        }
    }
    Ok(())
}

/// Platz, der frei würde, wenn von jeder doppelten Datei nur eine Kopie bliebe.
pub fn redundant_size(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row(
        "SELECT coalesce(sum(size * (n - 1)), 0) FROM
           (SELECT size, count(*) AS n FROM asset WHERE hash IS NOT NULL GROUP BY hash, size HAVING n > 1)",
        [],
        |r| r.get(0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(id: i64, path: &str, hash: &str, size: i64) -> FileRec {
        FileRec {
            asset_id: id,
            root_id: 1,
            rel_path: path.into(),
            size,
            hash: Some(hash.into()),
            mtime: id,
            taken_at: None,
            thumb: None,
        }
    }

    fn key(p: &str) -> FolderKey {
        FolderKey { root_id: 1, rel_path: p.into() }
    }

    #[test]
    fn groups_identical_files_and_suggests_tidy_copy() {
        let files = [
            file(1, "Downloads/IMG_1 (2).jpg", "h1", 10),
            file(2, "2023/2023-05-12 Geburtstag/IMG_1.jpg", "h1", 10),
            file(3, "andere.jpg", "h2", 10), // gleiche Größe, anderer Inhalt
        ];
        let a = analyze(&files);
        assert_eq!(a.file_groups.len(), 1);
        let g = &a.file_groups[0];
        assert_eq!(g.files[0].rel_path, "2023/2023-05-12 Geburtstag/IMG_1.jpg");
        assert_eq!(g.redundant_size, 10);
        assert!(g.suggestion_reason.contains("Downloads"), "{}", g.suggestion_reason);
    }

    #[test]
    fn reports_only_topmost_identical_folders() {
        let files = [
            file(1, "A/x/1.jpg", "h1", 1),
            file(2, "A/x/2.jpg", "h2", 1),
            file(3, "A/3.jpg", "h3", 1),
            file(4, "B/x/1.jpg", "h1", 1),
            file(5, "B/x/2.jpg", "h2", 1),
            file(6, "B/3.jpg", "h3", 1),
        ];
        let a = analyze(&files);
        assert_eq!(a.identical_folders.len(), 1, "{:?}", a.identical_folders);
        let p = &a.identical_folders[0];
        assert_eq!((&p.keep.key, &p.other.key), (&key("A"), &key("B")));
        assert!(a.same_content_folders.is_empty());
        assert!(a.subset_folders.is_empty());
    }

    #[test]
    fn same_content_despite_other_names() {
        let files = [file(1, "A/1.jpg", "h1", 1), file(2, "A/2.jpg", "h2", 1), file(3, "B/eins.jpg", "h1", 1), file(4, "B/zwei.jpg", "h2", 1)];
        let a = analyze(&files);
        assert!(a.identical_folders.is_empty());
        assert_eq!(a.same_content_folders.len(), 1);
    }

    #[test]
    fn finds_half_copied_folder_with_minimal_superset() {
        let files = [
            file(1, "Fotos/Urlaub/1.jpg", "h1", 1),
            file(2, "Fotos/Urlaub/2.jpg", "h2", 1),
            file(3, "Fotos/Urlaub/3.jpg", "h3", 1),
            file(4, "Fotos/Urlaub/4.jpg", "h4", 1),
            file(5, "USB/Urlaub halb/1.jpg", "h1", 1),
            file(6, "USB/Urlaub halb/2.jpg", "h2", 1),
        ];
        let a = analyze(&files);
        assert_eq!(a.subset_folders.len(), 1, "{:?}", a.subset_folders);
        let s = &a.subset_folders[0];
        // "USB" (Elternordner) ist ebenfalls Teilmenge, aber "USB/Urlaub halb" ist
        // nicht abgedeckt, weil USB nur aus diesem Ordner besteht: gemeldet wird der oberste.
        assert_eq!(s.superset.key, key("Fotos/Urlaub"));
        assert_eq!(s.subset.key, key("USB"));
        assert!((s.coverage - 0.5).abs() < 1e-9);
    }

    #[test]
    fn unhashed_files_compare_by_name_and_size() {
        let mut a1 = file(1, "A/1.jpg", "h1", 1);
        let mut b1 = file(3, "B/1.jpg", "h1", 1);
        a1.hash = Some("h1".into());
        b1.hash = Some("h1".into());
        let mut a2 = file(2, "A/notiz.txt", "", 5);
        let mut b2 = file(4, "B/notiz.txt", "", 6);
        a2.hash = None;
        b2.hash = None;
        // Unterschiedlich große Notizdateien: Ordner sind nicht identisch.
        assert!(analyze(&[a1, a2, b1, b2]).identical_folders.is_empty());
    }
}
