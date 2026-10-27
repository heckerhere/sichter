//! Paarbildung innerhalb eines Ordners (Sicherheitsmodell 3.8).
//!
//! * Medien mit gleichem Basisnamen bilden eine Einheit: RAW+JPEG, HEIC+MOV
//!   (Live Photo).
//! * Sidecars (`.xmp`, `.aae`, `.thm`, `.pp3`) hängen sich an `IMG_1.*` oder
//!   an `IMG_1.CR2` (Schreibweise `IMG_1.CR2.xmp`).
//! * Gleiche Apple-ContentIdentifier verbinden Live Photos auch bei
//!   unterschiedlichen Dateinamen.
//!
//! Der Vergleich ignoriert Groß-/Kleinschreibung und Unicode-Normalform
//! (macOS liefert Umlaute teils als NFD).

use std::collections::HashMap;

use unicode_normalization::UnicodeNormalization;

use super::kind::Kind;

#[derive(Debug, Clone)]
pub struct GroupInput {
    pub asset_id: i64,
    /// Nur der Dateiname, ohne Ordner.
    pub file_name: String,
    pub kind: Kind,
    pub content_id: Option<String>,
}

fn normalize(s: &str) -> String {
    s.nfc().collect::<String>().to_lowercase()
}

/// Dateiname ohne letzte Endung.
fn stem(name: &str) -> &str {
    match name.rfind('.') {
        Some(0) | None => name,
        Some(i) => &name[..i],
    }
}

fn primary_rank(kind: Kind) -> u8 {
    match kind {
        Kind::Raw => 0,
        Kind::Jpeg | Kind::Heic | Kind::Tiff | Kind::Png | Kind::Webp => 1,
        Kind::Video => 2,
        Kind::Sidecar | Kind::Other => 3,
    }
}

/// Gruppen als Listen von Asset-IDs, primäres Asset zuerst. Dateien ohne
/// Medium (`other`, verwaiste Sidecars) gehören zu keiner Gruppe.
pub fn group_directory(files: &[GroupInput]) -> Vec<Vec<i64>> {
    // Bucket pro normalisiertem Basisnamen.
    let mut bucket_of_stem: HashMap<String, usize> = HashMap::new();
    let mut buckets: Vec<Vec<&GroupInput>> = Vec::new();
    for file in files.iter().filter(|f| f.kind.is_media()) {
        let key = normalize(stem(&file.file_name));
        let index = *bucket_of_stem.entry(key).or_insert_with(|| {
            buckets.push(Vec::new());
            buckets.len() - 1
        });
        buckets[index].push(file);
    }

    // Live Photos mit gleicher ContentIdentifier zusammenführen (Union-Find light).
    let mut target: Vec<usize> = (0..buckets.len()).collect();
    fn find(target: &mut [usize], mut i: usize) -> usize {
        while target[i] != i {
            target[i] = target[target[i]];
            i = target[i];
        }
        i
    }
    let mut bucket_of_content: HashMap<&str, usize> = HashMap::new();
    for (index, bucket) in buckets.iter().enumerate() {
        for file in bucket {
            if let Some(cid) = file.content_id.as_deref().filter(|c| !c.is_empty()) {
                if let Some(&other) = bucket_of_content.get(cid) {
                    let (a, b) = (find(&mut target, index), find(&mut target, other));
                    target[a] = b;
                } else {
                    bucket_of_content.insert(cid, index);
                }
            }
        }
    }

    // Sidecars zuordnen: erst der volle Basisname (IMG_1.CR2.xmp -> "img_1.cr2"
    // passt zu keinem Bucket), dann ohne weitere Endung ("img_1").
    let mut sidecars: Vec<Vec<i64>> = vec![Vec::new(); buckets.len()];
    for file in files.iter().filter(|f| f.kind == Kind::Sidecar) {
        let base = stem(&file.file_name);
        let found = bucket_of_stem
            .get(&normalize(base))
            .or_else(|| bucket_of_stem.get(&normalize(stem(base))));
        if let Some(&index) = found {
            sidecars[index].push(file.asset_id);
        }
    }

    let mut merged: HashMap<usize, (Vec<&GroupInput>, Vec<i64>)> = HashMap::new();
    for index in 0..buckets.len() {
        let root = find(&mut target, index);
        let entry = merged.entry(root).or_default();
        entry.0.extend(buckets[index].iter().copied());
        entry.1.extend(sidecars[index].iter().copied());
    }

    let mut groups: Vec<Vec<i64>> = merged
        .into_values()
        .map(|(mut media, side)| {
            media.sort_by(|a, b| {
                primary_rank(a.kind)
                    .cmp(&primary_rank(b.kind))
                    .then_with(|| a.file_name.cmp(&b.file_name))
            });
            let mut ids: Vec<i64> = media.iter().map(|f| f.asset_id).collect();
            let mut side = side;
            side.sort_unstable();
            ids.extend(side);
            ids
        })
        .collect();
    groups.sort_by_key(|g| g[0]);
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(id: i64, name: &str, kind: Kind) -> GroupInput {
        GroupInput { asset_id: id, file_name: name.into(), kind, content_id: None }
    }

    #[test]
    fn raw_jpeg_xmp_form_one_group_with_raw_first() {
        let files = [
            input(1, "IMG_0001.JPG", Kind::Jpeg),
            input(2, "IMG_0001.CR2", Kind::Raw),
            input(3, "IMG_0001.xmp", Kind::Sidecar),
            input(4, "IMG_0002.JPG", Kind::Jpeg),
        ];
        assert_eq!(group_directory(&files), vec![vec![2, 1, 3], vec![4]]);
    }

    #[test]
    fn darktable_style_sidecar() {
        let files = [input(1, "DSC_1.NEF", Kind::Raw), input(2, "DSC_1.NEF.xmp", Kind::Sidecar)];
        assert_eq!(group_directory(&files), vec![vec![1, 2]]);
    }

    #[test]
    fn live_photo_by_name_and_by_content_id() {
        let by_name = [
            input(1, "IMG_0100.HEIC", Kind::Heic),
            input(2, "IMG_0100.MOV", Kind::Video),
            input(3, "IMG_0100.AAE", Kind::Sidecar),
        ];
        assert_eq!(group_directory(&by_name), vec![vec![1, 2, 3]]);

        let mut photo = input(1, "IMG_0100.HEIC", Kind::Heic);
        photo.content_id = Some("ABC".into());
        let mut video = input(2, "IMG_0100 (1).MOV", Kind::Video);
        video.content_id = Some("ABC".into());
        assert_eq!(group_directory(&[photo, video]), vec![vec![1, 2]]);
    }

    #[test]
    fn ignores_case_and_unicode_form() {
        let nfd = "Gru\u{0308}n.jpg"; // "Grün" als NFD
        let files = [input(1, nfd, Kind::Jpeg), input(2, "GRÜN.CR2", Kind::Raw)];
        assert_eq!(group_directory(&files), vec![vec![2, 1]]);
    }

    #[test]
    fn orphans_and_other_files_stay_ungrouped() {
        let files = [
            input(1, "notizen.txt", Kind::Other),
            input(2, "verwaist.xmp", Kind::Sidecar),
            input(3, "a.jpg", Kind::Jpeg),
        ];
        assert_eq!(group_directory(&files), vec![vec![3]]);
    }
}
