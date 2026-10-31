//! Clustering und Event-Verwaltung (PLAN.md, Abschnitt 6.2).
//!
//! Events leben ausschließlich in der Datenbank. Regeln:
//! * Nach einem Scan werden nur noch nicht zugeordnete Aufnahmen einsortiert,
//!   Bearbeitungen des Nutzers bleiben erhalten.
//! * Bestätigte Events werden von keiner Automatik verändert.
//! * "Neu berechnen" ersetzt nur unbestätigte Events.

pub mod cluster;
pub mod places;

use std::collections::{HashMap, HashSet};

use chrono::{Duration, NaiveDateTime};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;

use crate::meta::parse_exif_date;
pub use cluster::{Settings, TzMode};
use cluster::Item;

const FMT: &str = "%Y-%m-%dT%H:%M:%S";

fn parse_time(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, FMT).ok().or_else(|| parse_exif_date(s).map(|(t, _)| t))
}

fn fmt_time(t: NaiveDateTime) -> String {
    t.format(FMT).to_string()
}

// ---------------------------------------------------------------------------
// Einstellungen
// ---------------------------------------------------------------------------

fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM setting WHERE key = ?1", [key], |r| r.get(0)).optional()
}

fn set_setting(conn: &Connection, key: &str, value: &str) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO setting (key, value) VALUES (?1, ?2) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn load_settings(conn: &Connection) -> rusqlite::Result<Settings> {
    let d = Settings::default();
    let num = |k: &str, default: f64| -> rusqlite::Result<f64> {
        Ok(get_setting(conn, k)?.and_then(|v| v.parse().ok()).filter(|v: &f64| v.is_finite() && *v > 0.0).unwrap_or(default))
    };
    Ok(Settings {
        gap_hours: num("events.gap_hours", d.gap_hours)?,
        gps_km: num("events.gps_km", d.gps_km)?,
        merge_multiday: get_setting(conn, "events.merge_multiday")?.map_or(d.merge_multiday, |v| v == "1"),
    })
}

pub fn save_settings(conn: &Connection, s: &Settings) -> rusqlite::Result<()> {
    set_setting(conn, "events.gap_hours", &s.gap_hours.clamp(0.25, 72.0).to_string())?;
    set_setting(conn, "events.gps_km", &s.gps_km.clamp(1.0, 5000.0).to_string())?;
    set_setting(conn, "events.merge_multiday", if s.merge_multiday { "1" } else { "0" })
}

// ---------------------------------------------------------------------------
// Aufnahmen laden
// ---------------------------------------------------------------------------

/// Kamerabezeichnung für die Zeitzonen-Einstellung.
pub fn camera_label(make: Option<&str>, model: Option<&str>) -> String {
    model.or(make).map(str::trim).filter(|s| !s.is_empty()).unwrap_or(crate::i18n::tr("Unbekannte Kamera", "Unknown camera")).to_string()
}

#[derive(Debug, Clone, Serialize)]
pub struct GroupRow {
    pub group_id: i64,
    pub event_id: Option<i64>,
    pub root_id: i64,
    pub rel_path: String,
    pub kind: String,
    pub kinds: Vec<String>,
    pub thumb: Option<String>,
    /// Wanduhrzeit nach Zeitzonen-Korrektur.
    pub time: Option<String>,
    pub date_source: Option<String>,
    pub duration_s: Option<f64>,
    pub stars: i64,
    pub flag: String,
    /// Inhalt, aus dem die Vollbild-Vorschau entsteht: Kamera-JPEG vor RAW.
    pub preview_hash: Option<String>,
    /// Video der Gruppe (Video-Aufnahme oder Live-Photo-Teil).
    pub video_asset_id: Option<i64>,
    /// Serie (Burst): Gruppen-ID der ersten Aufnahme der Serie und ihre Länge.
    pub burst_id: Option<i64>,
    pub burst_size: usize,
    #[serde(skip)]
    gps: Option<(f64, f64)>,
    #[serde(skip)]
    camera: String,
    #[serde(skip)]
    quicktime_utc: bool,
    #[serde(skip)]
    raw_time: Option<NaiveDateTime>,
    #[serde(skip)]
    effective: Option<NaiveDateTime>,
}

fn camera_modes(conn: &Connection) -> rusqlite::Result<HashMap<String, TzMode>> {
    let mut stmt = conn.prepare("SELECT camera, mode FROM camera_tz")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (camera, mode) = row?;
        if let Some(m) = TzMode::parse(&mode) {
            out.insert(camera, m);
        }
    }
    Ok(out)
}

fn load_rows(conn: &Connection, only_event: Option<i64>) -> rusqlite::Result<Vec<GroupRow>> {
    let modes = camera_modes(conn)?;
    let sql = format!(
        "SELECT g.id, ea.event_id, p.root_id, p.rel_path, p.kind,
            (SELECT group_concat(x.kind) FROM asset x WHERE x.group_id = g.id),
            (SELECT x.hash FROM asset x JOIN thumb t ON t.hash = x.hash AND t.status = 'ok' WHERE x.group_id = g.id
              ORDER BY CASE x.kind WHEN 'jpeg' THEN 0 WHEN 'heic' THEN 1 WHEN 'video' THEN 4 WHEN 'raw' THEN 3 ELSE 2 END LIMIT 1),
            m.taken_at, m.date_source, v.duration_s, m.quicktime_utc, m.camera_make, m.camera_model,
            (SELECT mm.gps_lat || ',' || mm.gps_lon FROM asset x JOIN meta mm ON mm.asset_id = x.id
              WHERE x.group_id = g.id AND mm.gps_lat IS NOT NULL LIMIT 1),
            coalesce(rt.stars, 0), coalesce(rt.flag, 'none'),
            (SELECT x.hash FROM asset x WHERE x.group_id = g.id AND x.hash IS NOT NULL AND x.kind NOT IN ('sidecar', 'other')
              ORDER BY CASE x.kind WHEN 'jpeg' THEN 0 WHEN 'heic' THEN 1 WHEN 'raw' THEN 3 WHEN 'video' THEN 4 ELSE 2 END LIMIT 1),
            (SELECT x.id FROM asset x WHERE x.group_id = g.id AND x.kind = 'video' LIMIT 1)
         FROM asset_group g
         JOIN asset p ON p.id = g.primary_asset_id
         LEFT JOIN meta m ON m.asset_id = p.id
         LEFT JOIN video_meta v ON v.asset_id = p.id
         LEFT JOIN event_asset ea ON ea.group_id = g.id
         LEFT JOIN rating rt ON rt.group_id = g.id
         {}",
        if only_event.is_some() { "WHERE ea.event_id = ?1" } else { "" }
    );
    let mut stmt = conn.prepare(&sql)?;
    let map = |r: &rusqlite::Row| -> rusqlite::Result<GroupRow> {
        let kinds: Option<String> = r.get(5)?;
        let raw_time = r.get::<_, Option<String>>(7)?.as_deref().and_then(parse_time);
        let quicktime_utc: bool = r.get::<_, Option<bool>>(10)?.unwrap_or(false);
        let make: Option<String> = r.get(11)?;
        let camera = camera_label(make.as_deref(), r.get::<_, Option<String>>(12)?.as_deref());
        let mode = modes.get(&camera).copied().unwrap_or_else(|| cluster::default_tz_mode(make.as_deref()));
        let effective = raw_time.map(|t| cluster::effective_time(t, quicktime_utc, mode));
        let gps = r.get::<_, Option<String>>(13)?.and_then(|s| {
            let (a, b) = s.split_once(',')?;
            Some((a.parse().ok()?, b.parse().ok()?))
        });
        Ok(GroupRow {
            group_id: r.get(0)?,
            event_id: r.get(1)?,
            root_id: r.get(2)?,
            rel_path: r.get(3)?,
            kind: r.get(4)?,
            kinds: kinds.map(|k| k.split(',').map(str::to_string).collect()).unwrap_or_default(),
            thumb: r.get(6)?,
            time: effective.map(fmt_time),
            date_source: r.get(8)?,
            duration_s: r.get(9)?,
            stars: r.get(14)?,
            flag: r.get(15)?,
            preview_hash: r.get(16)?,
            video_asset_id: r.get(17)?,
            burst_id: None,
            burst_size: 0,
            gps,
            camera,
            quicktime_utc,
            raw_time,
            effective,
        })
    };
    let rows = match only_event {
        Some(id) => stmt.query_map([id], map)?.collect::<rusqlite::Result<Vec<_>>>()?,
        None => stmt.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()?,
    };
    Ok(rows)
}

fn to_item(row: &GroupRow) -> Option<Item> {
    let folder = row.rel_path.rsplit_once('/').map_or("", |(d, _)| d);
    Some(Item {
        group_id: row.group_id,
        time: row.effective?,
        gps: row.gps,
        folder: folder.rsplit('/').next().unwrap_or("").to_string(),
    })
}

// ---------------------------------------------------------------------------
// Automatische Zuordnung
// ---------------------------------------------------------------------------

/// Titelvorschlag: aussagekräftiger Ordnername, sonst Ortsname aus GPS.
fn suggestion(items: &[Item]) -> String {
    let from_folder = cluster::suggest_title(items);
    if !from_folder.is_empty() {
        return from_folder;
    }
    let points: Vec<(f64, f64)> = items.iter().filter_map(|i| i.gps).collect();
    places::place_for(&points).map(|p| p.name).unwrap_or_default()
}

fn create_event(conn: &Connection, items: &[Item]) -> rusqlite::Result<i64> {
    conn.execute("INSERT INTO event (title, suggested_title) VALUES ('', ?1)", [suggestion(items)])?;
    let id = conn.last_insert_rowid();
    add_items(conn, id, items)?;
    Ok(id)
}

fn add_items(conn: &Connection, event_id: i64, items: &[Item]) -> rusqlite::Result<()> {
    let mut stmt = conn.prepare("INSERT OR REPLACE INTO event_asset (event_id, group_id) VALUES (?1, ?2)")?;
    for i in items {
        // Eine Aufnahme gehört zu genau einem Event.
        conn.execute("DELETE FROM event_asset WHERE group_id = ?1 AND event_id != ?2", params![i.group_id, event_id])?;
        stmt.execute(params![event_id, i.group_id])?;
    }
    Ok(())
}

/// Aktualisiert Zeitraum und Titelvorschlag eines Events aus seinen Aufnahmen.
fn refresh_event(conn: &Connection, event_id: i64) -> rusqlite::Result<()> {
    let items: Vec<Item> = load_rows(conn, Some(event_id))?.iter().filter_map(to_item).collect();
    let start = items.iter().map(|i| i.time).min();
    let end = items.iter().map(|i| i.time).max();
    conn.execute(
        "UPDATE event SET date_start = ?2, date_end = ?3, suggested_title = ?4 WHERE id = ?1",
        params![event_id, start.map(fmt_time), end.map(fmt_time), suggestion(&items)],
    )?;
    Ok(())
}

/// Nach dem Entfernen von Aufnahmen (Ordner aus der Bibliothek entfernt,
/// Papierkorb-Lauf): Events ohne Aufnahmen verschwinden, auch bestätigte, und
/// die übrigen bekommen wieder passende Zeiträume und Titelvorschläge.
pub fn cleanup(conn: &Connection) -> rusqlite::Result<()> {
    let tx = conn.unchecked_transaction()?;
    remove_empty_events(&tx)?;
    let ids: Vec<i64> = {
        let mut stmt = tx.prepare("SELECT id FROM event")?;
        stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    for id in ids {
        refresh_event(&tx, id)?;
    }
    tx.commit()
}

fn remove_empty_events(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute("DELETE FROM event WHERE id NOT IN (SELECT DISTINCT event_id FROM event_asset)", [])?;
    Ok(())
}

/// Sortiert noch nicht zugeordnete Aufnahmen ein: in ein zeitlich passendes
/// unbestätigtes Event oder in ein neues. Gibt die Zahl neu zugeordneter Gruppen zurück.
pub fn assign_new(conn: &Connection) -> rusqlite::Result<usize> {
    let settings = load_settings(conn)?;
    let rows = load_rows(conn, None)?;
    let unassigned: Vec<Item> = rows.iter().filter(|r| r.event_id.is_none()).filter_map(to_item).collect();
    let count = unassigned.len();
    let tx = conn.unchecked_transaction()?;
    if count > 0 {
        let gap = Duration::seconds((settings.gap_hours * 3600.0) as i64);
        let mut stmt = tx.prepare("SELECT id, date_start, date_end FROM event WHERE confirmed = 0 AND date_start IS NOT NULL")?;
        let open: Vec<(i64, NaiveDateTime, NaiveDateTime)> = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))?
            .filter_map(|r| r.ok())
            .filter_map(|(id, s, e)| Some((id, parse_time(&s)?, parse_time(&e)?)))
            .collect();
        drop(stmt);
        let mut touched = HashSet::new();
        for c in cluster::cluster(unassigned, &settings) {
            let (first, last) = (c[0].time, c[c.len() - 1].time);
            let target = open.iter().find(|(_, s, e)| first <= *e + gap && last >= *s - gap).map(|(id, _, _)| *id);
            match target {
                Some(id) => {
                    add_items(&tx, id, &c)?;
                    touched.insert(id);
                }
                None => {
                    let id = create_event(&tx, &c)?;
                    touched.insert(id);
                }
            }
        }
        for id in touched {
            refresh_event(&tx, id)?;
        }
    }
    remove_empty_events(&tx)?;
    tx.commit()?;
    Ok(count)
}

/// Verwirft alle unbestätigten Events und berechnet sie neu.
pub fn recluster(conn: &Connection) -> rusqlite::Result<usize> {
    conn.execute("DELETE FROM event WHERE confirmed = 0", [])?;
    assign_new(conn)
}

// ---------------------------------------------------------------------------
// Abfragen für die UI
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct EventCard {
    pub id: i64,
    pub title: String,
    pub suggested_title: String,
    pub date_start: Option<String>,
    pub date_end: Option<String>,
    pub confirmed: bool,
    pub notes: String,
    pub count: usize,
    pub video_count: usize,
    pub uncertain_count: usize,
    /// Gruppen mit Sternen oder Markierung.
    pub rated_count: usize,
    pub keep_count: usize,
    pub reject_count: usize,
    pub thumbs: Vec<String>,
    /// Videos, deren Zeit nach der anderen Zeitzonen-Deutung besser zu den Fotos derselben Kamera passt.
    pub tz_hint_cameras: Vec<String>,
    /// Ort am GPS-Schwerpunkt (GeoNames), falls Aufnahmen GPS haben.
    pub place: Option<places::Place>,
}

/// Hinweis, wenn Videos einer Kamera ohne Zeitzone mehr als eine Stunde neben
/// den Fotos derselben Kamera liegen, die andere Deutung aber passen würde.
fn tz_hints(rows: &[&GroupRow]) -> Vec<String> {
    let mut cameras = Vec::new();
    for v in rows.iter().filter(|r| r.quicktime_utc) {
        let (Some(eff), Some(raw)) = (v.effective, v.raw_time) else { continue };
        let photos: Vec<NaiveDateTime> =
            rows.iter().filter(|r| !r.quicktime_utc && r.camera == v.camera && r.date_source.as_deref() == Some("exif")).filter_map(|r| r.effective).collect();
        if photos.is_empty() {
            continue;
        }
        let nearest = |t: NaiveDateTime| photos.iter().map(|p| (*p - t).num_minutes().abs()).min().unwrap_or(i64::MAX);
        // Die andere Deutung: war umgerechnet -> roh, war roh -> umgerechnet.
        let alternative = if eff == raw { cluster::effective_time(raw, true, TzMode::Utc) } else { raw };
        if nearest(eff) > 60 && nearest(alternative) <= 60 && !cameras.contains(&v.camera) {
            cameras.push(v.camera.clone());
        }
    }
    cameras
}

pub fn list_events(conn: &Connection) -> rusqlite::Result<Vec<EventCard>> {
    let rows = load_rows(conn, None)?;
    let mut by_event: HashMap<i64, Vec<&GroupRow>> = HashMap::new();
    for r in &rows {
        if let Some(id) = r.event_id {
            by_event.entry(id).or_default().push(r);
        }
    }
    let mut stmt = conn.prepare("SELECT id, title, suggested_title, date_start, date_end, confirmed, notes FROM event ORDER BY date_start, id")?;
    let events = stmt
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?))
        })?
        .collect::<rusqlite::Result<Vec<(i64, String, String, Option<String>, Option<String>, bool, String)>>>()?;
    Ok(events
        .into_iter()
        .map(|(id, title, suggested_title, date_start, date_end, confirmed, notes)| {
            let mut members = by_event.remove(&id).unwrap_or_default();
            members.sort_by(|a, b| a.time.cmp(&b.time));
            // Vorschaubilder gleichmäßig über das Event verteilt.
            let with_thumb: Vec<&String> = members.iter().filter_map(|m| m.thumb.as_ref()).collect();
            let thumbs = if with_thumb.len() <= 4 {
                with_thumb.into_iter().cloned().collect()
            } else {
                (0..4).map(|i| with_thumb[i * (with_thumb.len() - 1) / 3].clone()).collect()
            };
            EventCard {
                id,
                title,
                suggested_title,
                date_start,
                date_end,
                confirmed,
                notes,
                count: members.len(),
                video_count: members.iter().filter(|m| m.kind == "video").count(),
                uncertain_count: members.iter().filter(|m| m.date_source.as_deref().is_some_and(|s| s != "exif")).count(),
                rated_count: members.iter().filter(|m| m.stars > 0 || m.flag != "none").count(),
                keep_count: members.iter().filter(|m| m.flag == "keep").count(),
                reject_count: members.iter().filter(|m| m.flag == "reject").count(),
                tz_hint_cameras: tz_hints(&members),
                place: places::place_for(&members.iter().filter_map(|m| m.gps).collect::<Vec<_>>()),
                thumbs,
            }
        })
        .collect())
}

/// Ortsname je Event (für den Platzhalter {Ort} im Planer).
pub fn event_places(conn: &Connection) -> rusqlite::Result<HashMap<i64, String>> {
    let mut points: HashMap<i64, Vec<(f64, f64)>> = HashMap::new();
    for r in load_rows(conn, None)? {
        if let (Some(e), Some(g)) = (r.event_id, r.gps) {
            points.entry(e).or_default().push(g);
        }
    }
    Ok(points.into_iter().filter_map(|(e, p)| places::place_for(&p).map(|pl| (e, pl.name))).collect())
}

/// Zeit, Kamera und Event je Gruppe (für den Planer).
#[derive(Debug, Clone)]
pub struct GroupContext {
    pub time: Option<NaiveDateTime>,
    pub camera: String,
    pub event_id: Option<i64>,
}

pub fn group_contexts(conn: &Connection) -> rusqlite::Result<HashMap<i64, GroupContext>> {
    Ok(load_rows(conn, None)?
        .into_iter()
        .map(|r| (r.group_id, GroupContext { time: r.effective, camera: r.camera, event_id: r.event_id }))
        .collect())
}

/// Höchster Zeitabstand und dHash-Abstand innerhalb einer Serie.
const BURST_GAP_S: i64 = 2;
const BURST_MAX_DISTANCE: u32 = 12;

/// Markiert Serien: aufeinanderfolgende Fotos (keine Videos), höchstens 2 s
/// auseinander und mit ähnlichem Bildinhalt. `rows` muss zeitlich sortiert sein.
fn assign_bursts(rows: &mut [GroupRow], dhash: &HashMap<String, u64>) {
    let hash_of = |r: &GroupRow| r.thumb.as_ref().and_then(|t| dhash.get(t)).copied();
    let mut start = 0;
    for i in 1..=rows.len() {
        let continues = i < rows.len() && {
            let (a, b) = (&rows[i - 1], &rows[i]);
            let photos = a.kind != "video" && b.kind != "video";
            let close = matches!((a.effective, b.effective), (Some(x), Some(y)) if (y - x).num_seconds().abs() <= BURST_GAP_S);
            let similar = matches!((hash_of(a), hash_of(b)), (Some(x), Some(y)) if crate::thumbs::dhash::distance(x, y) <= BURST_MAX_DISTANCE);
            photos && close && similar
        };
        if !continues {
            let len = i - start;
            if len >= 2 {
                let id = rows[start].group_id;
                for r in &mut rows[start..i] {
                    r.burst_id = Some(id);
                    r.burst_size = len;
                }
            }
            start = i;
        }
    }
}

fn load_dhashes(conn: &Connection) -> rusqlite::Result<HashMap<String, u64>> {
    let mut stmt = conn.prepare("SELECT hash, dhash FROM phash")?;
    stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64)))?.collect()
}

fn sorted_with_bursts(conn: &Connection, mut rows: Vec<GroupRow>) -> rusqlite::Result<Vec<GroupRow>> {
    rows.sort_by(|a, b| a.time.cmp(&b.time).then(a.group_id.cmp(&b.group_id)));
    assign_bursts(&mut rows, &load_dhashes(conn)?);
    Ok(rows)
}

/// Alle Aufnahmen zeitlich sortiert (Sichten ohne Event-Auswahl).
pub fn all_items(conn: &Connection) -> rusqlite::Result<Vec<GroupRow>> {
    sorted_with_bursts(conn, load_rows(conn, None)?)
}

pub fn event_items(conn: &Connection, event_id: i64) -> rusqlite::Result<Vec<GroupRow>> {
    sorted_with_bursts(conn, load_rows(conn, Some(event_id))?)
}

/// Aufnahmen, die (noch) keinem Event zugeordnet sind, z. B. ohne gelesenes Datum.
pub fn unassigned_count(conn: &Connection) -> rusqlite::Result<i64> {
    conn.query_row("SELECT count(*) FROM asset_group WHERE id NOT IN (SELECT group_id FROM event_asset)", [], |r| r.get(0))
}

// ---------------------------------------------------------------------------
// Bearbeiten (nur Datenbank)
// ---------------------------------------------------------------------------

pub fn rename(conn: &Connection, event_id: i64, title: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE event SET title = ?2 WHERE id = ?1", params![event_id, title.trim()])?;
    Ok(())
}

pub fn set_notes(conn: &Connection, event_id: i64, notes: &str) -> rusqlite::Result<()> {
    conn.execute("UPDATE event SET notes = ?2 WHERE id = ?1", params![event_id, notes])?;
    Ok(())
}

pub fn set_confirmed(conn: &Connection, event_id: i64, confirmed: bool) -> rusqlite::Result<()> {
    conn.execute("UPDATE event SET confirmed = ?2 WHERE id = ?1", params![event_id, confirmed])?;
    Ok(())
}

/// Führt Events im frühesten zusammen. Titel: der erste vergebene.
pub fn merge(conn: &Connection, event_ids: &[i64]) -> rusqlite::Result<Option<i64>> {
    if event_ids.len() < 2 {
        return Ok(event_ids.first().copied());
    }
    let tx = conn.unchecked_transaction()?;
    let placeholders = vec!["?"; event_ids.len()].join(",");
    let ordered: Vec<(i64, String)> = {
        let mut stmt = tx.prepare(&format!("SELECT id, title FROM event WHERE id IN ({placeholders}) ORDER BY date_start, id"))?;
        stmt.query_map(rusqlite::params_from_iter(event_ids), |r| Ok((r.get(0)?, r.get(1)?)))?.collect::<rusqlite::Result<_>>()?
    };
    let Some((target, _)) = ordered.first().cloned() else { return Ok(None) };
    let title = ordered.iter().map(|(_, t)| t.clone()).find(|t| !t.is_empty()).unwrap_or_default();
    for (id, _) in &ordered[1..] {
        tx.execute("UPDATE event_asset SET event_id = ?1 WHERE event_id = ?2", params![target, id])?;
        tx.execute("DELETE FROM event WHERE id = ?1", [id])?;
    }
    // Inhalt hat sich geändert: Bestätigung erneut einholen.
    tx.execute("UPDATE event SET title = ?2, confirmed = 0 WHERE id = ?1", params![target, title])?;
    refresh_event(&tx, target)?;
    tx.commit()?;
    Ok(Some(target))
}

/// Teilt ein Event: Die gewählte Aufnahme und alle späteren kommen in ein neues Event.
pub fn split(conn: &Connection, event_id: i64, at_group_id: i64) -> rusqlite::Result<Option<i64>> {
    let rows = event_items(conn, event_id)?;
    let Some(pos) = rows.iter().position(|r| r.group_id == at_group_id) else { return Ok(None) };
    if pos == 0 {
        return Ok(None);
    }
    let tx = conn.unchecked_transaction()?;
    let moved: Vec<Item> = rows[pos..].iter().filter_map(to_item).collect();
    let new_id = create_event(&tx, &moved)?;
    tx.execute("UPDATE event SET confirmed = 0 WHERE id = ?1", [event_id])?;
    refresh_event(&tx, event_id)?;
    refresh_event(&tx, new_id)?;
    tx.commit()?;
    Ok(Some(new_id))
}

/// Verschiebt Aufnahmen in ein anderes Event (`None`: in ein neues).
pub fn move_groups(conn: &Connection, group_ids: &[i64], target: Option<i64>) -> rusqlite::Result<i64> {
    let tx = conn.unchecked_transaction()?;
    let sources: Vec<i64> = {
        let placeholders = vec!["?"; group_ids.len().max(1)].join(",");
        let mut stmt = tx.prepare(&format!("SELECT DISTINCT event_id FROM event_asset WHERE group_id IN ({placeholders})"))?;
        stmt.query_map(rusqlite::params_from_iter(group_ids), |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let target = match target {
        Some(id) => id,
        None => {
            tx.execute("INSERT INTO event (title) VALUES ('')", [])?;
            tx.last_insert_rowid()
        }
    };
    for g in group_ids {
        tx.execute("DELETE FROM event_asset WHERE group_id = ?1", [g])?;
        tx.execute("INSERT INTO event_asset (event_id, group_id) VALUES (?1, ?2)", params![target, g])?;
    }
    for id in sources.iter().chain(std::iter::once(&target)) {
        tx.execute("UPDATE event SET confirmed = 0 WHERE id = ?1", [id])?;
        refresh_event(&tx, *id)?;
    }
    remove_empty_events(&tx)?;
    tx.commit()?;
    Ok(target)
}

// ---------------------------------------------------------------------------
// Video-Zeitzone pro Kamera
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CameraTz {
    pub camera: String,
    pub video_count: usize,
    pub mode: TzMode,
    pub is_default: bool,
}

pub fn cameras_with_ambiguous_video(conn: &Connection) -> rusqlite::Result<Vec<CameraTz>> {
    let overrides = camera_modes(conn)?;
    let mut stmt = conn.prepare(
        "SELECT m.camera_make, m.camera_model, count(*) FROM meta m JOIN asset a ON a.id = m.asset_id
         WHERE m.quicktime_utc = 1 AND a.kind = 'video' GROUP BY 1, 2",
    )?;
    let mut out: Vec<CameraTz> = Vec::new();
    let rows = stmt.query_map([], |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, i64>(2)?)))?;
    for row in rows {
        let (make, model, n) = row?;
        let camera = camera_label(make.as_deref(), model.as_deref());
        if let Some(existing) = out.iter_mut().find(|c| c.camera == camera) {
            existing.video_count += n as usize;
            continue;
        }
        let default = cluster::default_tz_mode(make.as_deref());
        let mode = overrides.get(&camera).copied().unwrap_or(default);
        let is_default = !overrides.contains_key(&camera);
        out.push(CameraTz { camera, video_count: n as usize, mode, is_default });
    }
    out.sort_by_key(|a| std::cmp::Reverse(a.video_count));
    Ok(out)
}

/// Setzt die Deutung für eine Kamera und aktualisiert die Zeiträume aller Events.
pub fn set_camera_mode(conn: &Connection, camera: &str, mode: Option<TzMode>) -> rusqlite::Result<()> {
    match mode {
        Some(m) => conn.execute(
            "INSERT INTO camera_tz (camera, mode) VALUES (?1, ?2) ON CONFLICT (camera) DO UPDATE SET mode = excluded.mode",
            params![camera, m.as_str()],
        )?,
        None => conn.execute("DELETE FROM camera_tz WHERE camera = ?1", [camera])?,
    };
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare("SELECT id FROM event")?;
        stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
    };
    let tx = conn.unchecked_transaction()?;
    for id in ids {
        refresh_event(&tx, id)?;
    }
    tx.commit()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Local, TimeZone};

    fn row(id: i64, camera: &str, time: &str, quicktime_utc: bool, mode: TzMode) -> GroupRow {
        let raw = NaiveDateTime::parse_from_str(time, "%Y-%m-%d %H:%M").unwrap();
        let effective = cluster::effective_time(raw, quicktime_utc, mode);
        GroupRow {
            group_id: id,
            event_id: Some(1),
            root_id: 1,
            rel_path: format!("{id}.jpg"),
            kind: if quicktime_utc { "video".into() } else { "jpeg".into() },
            kinds: vec![],
            thumb: None,
            time: Some(fmt_time(effective)),
            date_source: Some("exif".into()),
            duration_s: None,
            stars: 0,
            flag: "none".into(),
            preview_hash: None,
            video_asset_id: None,
            burst_id: None,
            burst_size: 0,
            gps: None,
            camera: camera.into(),
            quicktime_utc,
            raw_time: Some(raw),
            effective: Some(effective),
        }
    }

    #[test]
    fn bursts_need_close_time_and_similar_content() {
        let mut rows: Vec<GroupRow> = [(1, "10:00"), (2, "10:00"), (3, "10:00"), (4, "10:05"), (5, "10:05")]
            .iter()
            .map(|(id, t)| {
                let mut r = row(*id, "Cam", &format!("2023-07-13 {t}"), false, TzMode::Local);
                r.thumb = Some(format!("t{id}"));
                r
            })
            .collect();
        // 1, 2 ähnlich und gleichzeitig: Serie. 3 gleichzeitig, aber anderes Motiv.
        // 4, 5 ähnlich, aber 5 ist ein Video: keine Serie.
        rows[4].kind = "video".into();
        let dhash: HashMap<String, u64> = [("t1", 0u64), ("t2", 0b111), ("t3", u64::MAX), ("t4", 0), ("t5", 0)]
            .iter()
            .map(|(k, v)| (k.to_string(), *v))
            .collect();
        assign_bursts(&mut rows, &dhash);
        let ids: Vec<(Option<i64>, usize)> = rows.iter().map(|r| (r.burst_id, r.burst_size)).collect();
        assert_eq!(ids, vec![(Some(1), 2), (Some(1), 2), (None, 0), (None, 0), (None, 0)]);
    }

    #[test]
    fn hints_when_other_timezone_reading_fits_photos() {
        // Offset dieses Rechners im Juli; ohne Offset (UTC-Rechner) gibt es nichts zu prüfen.
        let offset_h = Local.from_utc_datetime(&NaiveDateTime::parse_from_str("2023-07-13 10:00", "%Y-%m-%d %H:%M").unwrap()).offset().local_minus_utc() / 3600;
        if offset_h.abs() < 2 {
            return;
        }
        // Kamera schreibt UTC (Video 08:00 UTC = 10:00 Ortszeit), Nutzer hat "Ortszeit" eingestellt.
        let utc_time = format!("2023-07-13 {:02}:00", 10 - offset_h);
        let photo = row(1, "Canon EOS R6", "2023-07-13 10:05", false, TzMode::Local);
        let video = row(2, "Canon EOS R6", &utc_time, true, TzMode::Local);
        assert_eq!(tz_hints(&[&photo, &video]), vec!["Canon EOS R6".to_string()]);
        // Mit passender Einstellung kein Hinweis.
        let fixed = row(2, "Canon EOS R6", &utc_time, true, TzMode::Utc);
        assert!(tz_hints(&[&photo, &fixed]).is_empty());
        // Fotos einer anderen Kamera zählen nicht.
        let other = row(1, "iPhone 15", "2023-07-13 10:05", false, TzMode::Local);
        assert!(tz_hints(&[&other, &video]).is_empty());
    }
}
