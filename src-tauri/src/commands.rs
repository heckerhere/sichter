//! Tauri-Commands. Dünne Schicht: Eingaben prüfen, an Module delegieren.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::library::{self, GridItem, LibraryStats, OtherFile, RootInfo};
use crate::fsops::appdata::AppDataDir;
use crate::meta::exiftool::ExifTool;
use crate::scan::{Progress, ScanReport, Scanner};
use crate::storage::{self, FolderRow, LargeFile, Summary};
use crate::tools::{self, ToolCommand, ToolSource, Tools};

pub struct AppState {
    pub data_dir: AppDataDir,
    pub db_path: PathBuf,
    pub db: Mutex<rusqlite::Connection>,
    pub tools: Tools,
    pub exiftool: Mutex<Option<ExifTool>>,
    /// Abbruch-Flag des laufenden Scans; `None`, wenn keiner läuft.
    pub scan: Mutex<Option<Arc<AtomicBool>>>,
    pub previews: Arc<crate::thumbs::preview::PreviewService>,
}

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn db<'a>(state: &'a State<'_, AppState>) -> CmdResult<std::sync::MutexGuard<'a, rusqlite::Connection>> {
    state.db.lock().map_err(err)
}

#[derive(Serialize)]
pub struct ToolInfo {
    name: &'static str,
    version: Option<String>,
    path: Option<String>,
    source: Option<ToolSource>,
    error: Option<String>,
    /// Nur ffmpeg: Build enthält GPL-Komponenten (nicht für Releases geeignet).
    gpl: Option<bool>,
}

#[derive(Serialize)]
pub struct AppInfo {
    app_version: &'static str,
    data_dir: String,
    db_schema_version: u32,
    tools: Vec<ToolInfo>,
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> CmdResult<AppInfo> {
    let db_schema_version = {
        let db = state.db.lock().map_err(err)?;
        crate::db::current_version(&db).map_err(err)?
    };

    let exiftool = tool_info("exiftool", state.tools.exiftool.as_ref(), |tool| {
        let mut guard = state.exiftool.lock().map_err(err)?;
        if guard.is_none() {
            *guard = Some(ExifTool::spawn(tool).map_err(err)?);
        }
        let result = guard.as_mut().map(ExifTool::version).transpose();
        match result {
            Ok(v) => Ok(v.unwrap_or_default()),
            Err(e) => {
                // Prozess ist vermutlich tot: beim nächsten Aufruf neu starten.
                *guard = None;
                Err(e.to_string())
            }
        }
    });
    let mut ffmpeg = tool_info("ffmpeg", state.tools.ffmpeg.as_ref(), tools::ffmpeg_style_version);
    ffmpeg.gpl = state.tools.ffmpeg.as_ref().and_then(tools::ffmpeg_is_gpl);
    let ffprobe = tool_info("ffprobe", state.tools.ffprobe.as_ref(), tools::ffmpeg_style_version);

    Ok(AppInfo {
        app_version: env!("CARGO_PKG_VERSION"),
        data_dir: state.data_dir.path().display().to_string(),
        db_schema_version,
        tools: vec![exiftool, ffmpeg, ffprobe],
    })
}

fn tool_info(
    name: &'static str,
    tool: Option<&ToolCommand>,
    version: impl FnOnce(&ToolCommand) -> Result<String, String>,
) -> ToolInfo {
    let Some(tool) = tool else {
        return ToolInfo {
            name,
            version: None,
            path: None,
            source: None,
            error: Some("nicht gefunden".into()),
            gpl: None,
        };
    };
    let (version, error) = match version(tool) {
        Ok(v) => (Some(v), None),
        Err(e) => (None, Some(e)),
    };
    ToolInfo {
        name,
        version,
        path: Some(tool.display_path.display().to_string()),
        source: Some(tool.source),
        error,
        gpl: None,
    }
}

#[tauri::command]
pub fn list_roots(state: State<'_, AppState>) -> CmdResult<Vec<RootInfo>> {
    library::list_roots(&*db(&state)?).map_err(err)
}

#[derive(Serialize)]
pub struct AddRootResult {
    path: String,
    id: Option<i64>,
    error: Option<String>,
}

#[tauri::command]
pub fn add_roots(state: State<'_, AppState>, paths: Vec<String>) -> CmdResult<Vec<AddRootResult>> {
    let db = db(&state)?;
    Ok(paths
        .into_iter()
        .map(|path| match library::add_root(&db, path.as_ref(), state.data_dir.path()) {
            Ok(id) => AddRootResult { path, id: Some(id), error: None },
            Err(e) => AddRootResult { path, id: None, error: Some(e.to_string()) },
        })
        .collect())
}

/// Entfernt den Ordner nur aus der Bibliothek. Dateien bleiben unberührt.
#[tauri::command]
pub fn remove_root(state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    if state.scan.lock().map_err(err)?.is_some() {
        return Err("Während eines Scans können keine Ordner entfernt werden".into());
    }
    library::remove_root(&*db(&state)?, id).map_err(err)
}

#[tauri::command]
pub fn list_groups(state: State<'_, AppState>) -> CmdResult<Vec<GridItem>> {
    library::list_groups(&*db(&state)?).map_err(err)
}

#[tauri::command]
pub fn list_unhandled(state: State<'_, AppState>) -> CmdResult<Vec<OtherFile>> {
    library::list_unhandled(&*db(&state)?).map_err(err)
}

#[tauri::command]
pub fn library_stats(state: State<'_, AppState>) -> CmdResult<LibraryStats> {
    library::stats(&*db(&state)?).map_err(err)
}

// --- Events: alle Änderungen nur in der Datenbank ---------------------------

use crate::events::{self, CameraTz, EventCard, GroupRow, Settings as EventSettings, TzMode};

#[derive(Serialize)]
pub struct EventsOverview {
    events: Vec<EventCard>,
    unassigned: i64,
    settings: EventSettings,
    cameras: Vec<CameraTz>,
}

#[tauri::command]
pub fn events_overview(state: State<'_, AppState>) -> CmdResult<EventsOverview> {
    let db = db(&state)?;
    Ok(EventsOverview {
        events: events::list_events(&db).map_err(err)?,
        unassigned: events::unassigned_count(&db).map_err(err)?,
        settings: events::load_settings(&db).map_err(err)?,
        cameras: events::cameras_with_ambiguous_video(&db).map_err(err)?,
    })
}

#[tauri::command]
pub fn event_items(state: State<'_, AppState>, event_id: i64) -> CmdResult<Vec<GroupRow>> {
    events::event_items(&*db(&state)?, event_id).map_err(err)
}

#[tauri::command]
pub fn events_save_settings(state: State<'_, AppState>, settings: EventSettings) -> CmdResult<()> {
    events::save_settings(&*db(&state)?, &settings).map_err(err)
}

#[tauri::command]
pub fn events_recluster(state: State<'_, AppState>) -> CmdResult<usize> {
    events::recluster(&*db(&state)?).map_err(err)
}

#[tauri::command]
pub fn event_rename(state: State<'_, AppState>, event_id: i64, title: String) -> CmdResult<()> {
    events::rename(&*db(&state)?, event_id, &title).map_err(err)
}

#[tauri::command]
pub fn event_set_notes(state: State<'_, AppState>, event_id: i64, notes: String) -> CmdResult<()> {
    events::set_notes(&*db(&state)?, event_id, &notes).map_err(err)
}

#[tauri::command]
pub fn event_confirm(state: State<'_, AppState>, event_id: i64, confirmed: bool) -> CmdResult<()> {
    events::set_confirmed(&*db(&state)?, event_id, confirmed).map_err(err)
}

#[tauri::command]
pub fn events_merge(state: State<'_, AppState>, event_ids: Vec<i64>) -> CmdResult<Option<i64>> {
    events::merge(&*db(&state)?, &event_ids).map_err(err)
}

#[tauri::command]
pub fn event_split(state: State<'_, AppState>, event_id: i64, group_id: i64) -> CmdResult<Option<i64>> {
    events::split(&*db(&state)?, event_id, group_id).map_err(err)
}

#[tauri::command]
pub fn events_move(state: State<'_, AppState>, group_ids: Vec<i64>, target: Option<i64>) -> CmdResult<i64> {
    events::move_groups(&*db(&state)?, &group_ids, target).map_err(err)
}

#[tauri::command]
pub fn camera_tz_set(state: State<'_, AppState>, camera: String, mode: Option<TzMode>) -> CmdResult<()> {
    events::set_camera_mode(&*db(&state)?, &camera, mode).map_err(err)
}

// --- Sichten: Bewertungen nur in der Datenbank -------------------------------

use crate::db::ratings::{self, Flag};

#[tauri::command]
pub fn cull_items(state: State<'_, AppState>, event_id: Option<i64>) -> CmdResult<Vec<GroupRow>> {
    let db = db(&state)?;
    match event_id {
        Some(id) => events::event_items(&db, id),
        None => events::all_items(&db),
    }
    .map_err(err)
}

#[tauri::command]
pub fn set_rating(state: State<'_, AppState>, group_id: i64, stars: Option<i64>, flag: Option<Flag>) -> CmdResult<()> {
    ratings::set(&*db(&state)?, group_id, stars, flag).map_err(err)
}

#[tauri::command]
pub fn reset_rating(state: State<'_, AppState>, group_id: i64) -> CmdResult<()> {
    ratings::reset(&*db(&state)?, group_id).map_err(err)
}

/// Bereitet Vollbild-Vorschauen im Hintergrund vor (Reihenfolge = Priorität).
#[tauri::command]
pub fn previews_prepare(state: State<'_, AppState>, hashes: Vec<String>) -> CmdResult<()> {
    let items = {
        let db = db(&state)?;
        hashes
            .into_iter()
            .filter(|h| crate::thumbs::preview::valid_hash(h) && state.previews.cached(h).is_none())
            .filter_map(|h| crate::thumbs::preview::preview_source(&db, &h).ok().flatten().map(|s| (h, s)))
            .collect::<Vec<_>>()
    };
    state.previews.prepare(items);
    Ok(())
}

/// Öffnet eine Datei im Standardprogramm des Systems (nur lesend für Sichter).
#[tauri::command]
pub fn open_in_default_app(app: AppHandle, state: State<'_, AppState>, asset_id: i64) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let path = asset_path(&*db(&state)?, asset_id, false).ok_or("Datei nicht gefunden")?;
    app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(err)
}

fn asset_path(conn: &rusqlite::Connection, asset_id: i64, video_only: bool) -> Option<PathBuf> {
    let sql = format!(
        "SELECT r.path, a.rel_path FROM asset a JOIN library_root r ON r.id = a.root_id WHERE a.id = ?1 {}",
        if video_only { "AND a.kind = 'video'" } else { "AND a.kind != 'other'" }
    );
    conn.query_row(&sql, [asset_id], |r| Ok(crate::scan::crawl::from_rel_path(std::path::Path::new(&r.get::<_, String>(0)?), &r.get::<_, String>(1)?)))
        .ok()
}

type HttpResponse = tauri::http::Response<Vec<u8>>;

fn http(status: u16, mime: &str) -> tauri::http::response::Builder {
    tauri::http::Response::builder().status(status).header("Content-Type", mime).header("Access-Control-Allow-Origin", "*")
}

/// `preview://localhost/<hash>`: große Vorschau, wird bei Bedarf erzeugt.
pub fn preview_protocol(app: &AppHandle, request: &tauri::http::Request<Vec<u8>>) -> HttpResponse {
    let hash = request.uri().path().trim_start_matches('/').to_string();
    let state = app.state::<AppState>();
    let lookup = || state.db.lock().ok().and_then(|db| crate::thumbs::preview::preview_source(&db, &hash).ok().flatten());
    match state.previews.ensure(&hash, lookup) {
        Ok(Some(path)) => match std::fs::read(path) {
            Ok(bytes) => http(200, "image/jpeg").header("Cache-Control", "max-age=31536000, immutable").body(bytes),
            Err(_) => http(404, "text/plain").body(Vec::new()),
        },
        Ok(None) => http(404, "text/plain").body(Vec::new()),
        Err(_) => http(400, "text/plain").body(Vec::new()),
    }
    .expect("gültige Antwort")
}

/// `media://localhost/<asset_id>`: Videodatei mit Range-Unterstützung zum
/// Abspielen. Liefert ausschließlich indexierte Videos, nur lesend.
pub fn media_protocol(app: &AppHandle, request: &tauri::http::Request<Vec<u8>>) -> HttpResponse {
    use std::io::{Read, Seek, SeekFrom};
    const CHUNK: u64 = 4 << 20;
    let not_found = || http(404, "text/plain").body(Vec::new()).expect("gültige Antwort");
    let Ok(id) = request.uri().path().trim_start_matches('/').parse::<i64>() else { return not_found() };
    let state = app.state::<AppState>();
    let Some(path) = state.db.lock().ok().and_then(|db| asset_path(&db, id, true)) else { return not_found() };
    let Ok(mut file) = std::fs::File::open(&path) else { return not_found() };
    let Ok(len) = file.metadata().map(|m| m.len()) else { return not_found() };
    let mime = crate::video::mime_for(&path);
    let range = request.headers().get("range").and_then(|v| v.to_str().ok());
    let (start, end) = crate::video::parse_range(range, len, CHUNK).unwrap_or((0, len.saturating_sub(1).min(CHUNK - 1)));
    let mut body = vec![0u8; (end + 1 - start) as usize];
    if file.seek(SeekFrom::Start(start)).and_then(|_| file.read_exact(&mut body)).is_err() {
        return not_found();
    }
    http(206, mime)
        .header("Accept-Ranges", "bytes")
        .header("Content-Range", format!("bytes {start}-{end}/{len}"))
        .header("Content-Length", body.len().to_string())
        .body(body)
        .expect("gültige Antwort")
}

#[tauri::command]
pub fn dupes_analyze(state: State<'_, AppState>) -> CmdResult<crate::dupes::Analysis> {
    crate::dupes::analyze_db(&*db(&state)?).map_err(err)
}

/// Merkt sich nur die Auswahl. Ausgeführt wird erst über einen Plan (Phase 6).
#[tauri::command]
pub fn set_dupe_choice(state: State<'_, AppState>, hash: String, keep_asset_id: Option<i64>) -> CmdResult<()> {
    crate::dupes::set_choice(&*db(&state)?, &hash, keep_asset_id).map_err(|_| "Ungültige Auswahl".to_string())
}

#[tauri::command]
pub fn storage_folders(state: State<'_, AppState>, root_id: i64) -> CmdResult<Vec<FolderRow>> {
    storage::folders(&*db(&state)?, root_id).map_err(err)
}

#[tauri::command]
pub fn storage_summary(state: State<'_, AppState>) -> CmdResult<Summary> {
    storage::summary(&*db(&state)?).map_err(err)
}

#[tauri::command]
pub fn largest_files(state: State<'_, AppState>, limit: i64) -> CmdResult<Vec<LargeFile>> {
    storage::largest_files(&*db(&state)?, limit.clamp(1, 500)).map_err(err)
}

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum ScanFinished {
    Ok { report: ScanReport },
    Error { message: String },
}

/// Startet einen Scan im Hintergrund. Fortschritt kommt als Event
/// `scan-progress`, das Ergebnis als `scan-finished`.
#[tauri::command]
pub fn start_scan(app: AppHandle, state: State<'_, AppState>, root_ids: Option<Vec<i64>>) -> CmdResult<()> {
    let cancel = {
        let mut slot = state.scan.lock().map_err(err)?;
        if slot.is_some() {
            return Err("Es läuft bereits ein Scan".into());
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *slot = Some(cancel.clone());
        cancel
    };
    std::thread::Builder::new()
        .name("scan".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let result = run_scan(&app, &state, &cancel, root_ids.as_deref());
            if let Ok(mut slot) = state.scan.lock() {
                *slot = None;
            }
            let payload = match result {
                Ok(report) => ScanFinished::Ok { report },
                Err(message) => ScanFinished::Error { message },
            };
            let _ = app.emit("scan-finished", payload);
        })
        .map_err(err)?;
    Ok(())
}

fn run_scan(app: &AppHandle, state: &AppState, cancel: &AtomicBool, root_ids: Option<&[i64]>) -> Result<ScanReport, String> {
    // Eigene Verbindung: Die UI bleibt währenddessen über die Hauptverbindung bedienbar (WAL).
    let mut conn = crate::db::open(&state.db_path).map_err(err)?;
    // Drosselung auf ~10 Events pro Sekunde. Phasenwechsel und das Ende einer
    // Phase gehen immer durch, damit die Anzeige nie veraltet stehen bleibt.
    let last_emit = Mutex::new((Instant::now() - Duration::from_secs(1), ""));
    let progress = |p: Progress| {
        let Ok(mut last) = last_emit.lock() else { return };
        let important = p.phase != last.1 || (p.total > 0 && p.done >= p.total);
        if important || last.0.elapsed() >= Duration::from_millis(100) {
            *last = (Instant::now(), p.phase);
            let _ = app.emit("scan-progress", p);
        }
    };
    let mut scanner = Scanner { conn: &mut conn, tools: &state.tools, app: &state.data_dir, cancel, progress: &progress };
    scanner.run(root_ids).map_err(err)
}

#[tauri::command]
pub fn cancel_scan(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(flag) = state.scan.lock().map_err(err)?.as_ref() {
        flag.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub fn scan_running(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(state.scan.lock().map_err(err)?.is_some())
}

/// `thumb://localhost/<hash>`: liefert nur WebP-Dateien aus dem
/// Thumbnail-Cache, ausschließlich anhand eines gültigen BLAKE3-Hex-Hashs.
pub fn thumb_protocol(app: &AppHandle, request: &tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let respond = |status: u16, mime: &str, body: Vec<u8>| {
        tauri::http::Response::builder()
            .status(status)
            .header("Content-Type", mime)
            .header("Cache-Control", "max-age=31536000, immutable")
            .header("Access-Control-Allow-Origin", "*")
            .body(body)
            .expect("gültige Antwort")
    };
    let hash = request.uri().path().trim_start_matches('/');
    let valid = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    if !valid {
        return respond(400, "text/plain", b"ungueltiger Hash".to_vec());
    }
    let state = app.state::<AppState>();
    match state
        .data_dir
        .join(crate::thumbs::cache_rel_path(hash))
        .and_then(std::fs::read)
    {
        Ok(bytes) => respond(200, "image/webp", bytes),
        Err(_) => respond(404, "text/plain", Vec::new()),
    }
}
