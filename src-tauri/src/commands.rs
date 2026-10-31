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
    /// Steuerung des laufenden Laufs (Ausführen, Probelauf, Undo).
    pub run: Mutex<Option<RunControl>>,
}

#[derive(Clone)]
pub struct RunControl {
    pub run_id: i64,
    pub cancel: Arc<AtomicBool>,
    pub pause: Arc<AtomicBool>,
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

// --- Planer und Läufe -------------------------------------------------------

use crate::journal::{self, Exec, RunReport};
use crate::plan::{self, NewRun, ReorganizeParams, RunKind};

fn dry_run_mode(conn: &rusqlite::Connection) -> bool {
    conn.query_row("SELECT value FROM setting WHERE key = 'runs.dry_run'", [], |r| r.get::<_, String>(0))
        .map(|v| v != "0")
        .unwrap_or(true)
}

#[tauri::command]
pub fn runs_dry_run_mode(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(dry_run_mode(&*db(&state)?))
}

#[tauri::command]
pub fn runs_set_dry_run_mode(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    db(&state)?
        .execute(
            "INSERT INTO setting (key, value) VALUES ('runs.dry_run', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [if enabled { "1" } else { "0" }],
        )
        .map(|_| ())
        .map_err(err)
}

#[derive(Serialize)]
pub struct TemplatePreview {
    error: Option<String>,
    samples: Vec<(String, String)>,
}

/// Live-Vorschau der Vorlage mit Beispielpfaden.
#[tauri::command]
pub fn plan_template_preview(state: State<'_, AppState>, params: ReorganizeParams) -> CmdResult<TemplatePreview> {
    Ok(match plan::template_samples(&*db(&state)?, &params, 8) {
        Ok(samples) => TemplatePreview { error: None, samples },
        Err(e) => TemplatePreview { error: Some(e.to_string()), samples: Vec::new() },
    })
}

fn store_plan(state: &State<'_, AppState>, kind: RunKind, title: &str, target: Option<i64>, params: &impl Serialize, planned: plan::Result<(Vec<plan::PlannedOp>, plan::Summary)>) -> CmdResult<i64> {
    let (ops, summary) = planned.map_err(err)?;
    let db = db(state)?;
    let meta = NewRun { kind, title, target_root_id: target, dry_run: dry_run_mode(&db) };
    plan::store(&db, &meta, params, &ops, &summary).map_err(err)
}

#[tauri::command]
pub fn plan_reorganize(state: State<'_, AppState>, params: ReorganizeParams) -> CmdResult<i64> {
    let planned = plan::plan_reorganize(&*db(&state)?, &params);
    let title = if params.mode == plan::Mode::Copy { "Umsortieren (Kopieren)" } else { "Umsortieren" };
    store_plan(&state, RunKind::Reorganize, title, Some(params.target_root_id), &params, planned)
}

#[tauri::command]
pub fn plan_rejects(state: State<'_, AppState>, to_trash: bool, approved_groups: Vec<i64>) -> CmdResult<i64> {
    if to_trash {
        let planned = plan::plan_rejects_to_trash(&*db(&state)?, &approved_groups);
        store_plan(&state, RunKind::RejectsToTrash, "Ausschuss in den Papierkorb", None, &approved_groups, planned)
    } else {
        let planned = plan::plan_rejects_to_folder(&*db(&state)?);
        store_plan(&state, RunKind::RejectsToFolder, "Ausschuss nach _Aussortiert", None, &(), planned)
    }
}

#[tauri::command]
pub fn plan_dupes(state: State<'_, AppState>, hashes: Vec<String>, to_trash: bool) -> CmdResult<i64> {
    let planned = plan::plan_dupes(&*db(&state)?, &hashes, to_trash);
    let (kind, title) =
        if to_trash { (RunKind::DupesToTrash, "Doppelte Dateien in den Papierkorb") } else { (RunKind::DupesToFolder, "Doppelte Dateien nach _Duplikate") };
    store_plan(&state, kind, title, None, &hashes, planned)
}

#[tauri::command]
pub fn plan_empty_dirs(state: State<'_, AppState>) -> CmdResult<i64> {
    let planned = plan::plan_empty_dirs(&*db(&state)?);
    store_plan(&state, RunKind::EmptyDirsToTrash, "Leere Ordner in den Papierkorb", None, &(), planned)
}

#[tauri::command]
pub fn plan_xmp_export(state: State<'_, AppState>) -> CmdResult<i64> {
    let planned = plan::plan_xmp_export(&*db(&state)?);
    store_plan(&state, RunKind::XmpExport, "Bewertungen als XMP exportieren", None, &(), planned)
}

#[derive(Serialize)]
pub struct PhotosStatus {
    /// Plattform hat eine Fotos-Mediathek (macOS).
    supported: bool,
    access: crate::photos::Access,
}

#[tauri::command]
pub fn photos_status() -> PhotosStatus {
    let access = crate::photos::access();
    PhotosStatus { supported: access != crate::photos::Access::Unsupported, access }
}

/// Fragt nach Zugriff auf die Mediathek. Blockiert bis zur Antwort, daher async.
#[tauri::command]
pub async fn photos_request_access() -> crate::photos::Access {
    tauri::async_runtime::spawn_blocking(crate::photos::request_access).await.unwrap_or(crate::photos::Access::Denied)
}

/// Öffnet Systemeinstellungen → Datenschutz & Sicherheit → Fotos.
#[tauri::command]
pub fn photos_open_privacy_settings() -> CmdResult<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Photos").spawn().map_err(err)?;
    }
    Ok(())
}

#[derive(Clone, Serialize)]
struct PhotosProgress {
    done: usize,
    total: usize,
}

/// Plant den Import aus der Fotos-Mediathek. Liest die Mediathek (kann bei
/// großen Mediatheken etwas dauern) außerhalb der Datenbanksperre.
#[tauri::command]
pub async fn plan_photos_import(app: AppHandle, params: plan::import::ImportParams) -> CmdResult<i64> {
    tauri::async_runtime::spawn_blocking(move || {
        let photos = crate::photos::system().ok_or("Die Fotos-Mediathek gibt es nur unter macOS")?;
        // Fortschritt höchstens ~10-mal pro Sekunde, Anfang und Ende immer.
        let last = Mutex::new(Instant::now() - Duration::from_secs(1));
        let progress = |done: usize, total: usize| {
            if let Ok(mut l) = last.lock()
                && (done == 0 || done >= total || l.elapsed() >= Duration::from_millis(100))
            {
                *l = Instant::now();
                let _ = app.emit("photos-progress", PhotosProgress { done, total });
            }
        };
        let assets = photos.assets(&crate::photos::ListOptions { include_hidden: params.include_hidden }, &progress)?;
        let state = app.state::<AppState>();
        let planned = plan::import::plan_photos_import(&*db(&state)?, &assets, &params);
        store_plan(&state, RunKind::PhotosImport, "Import aus der Fotos-Mediathek", None, &params, planned)
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub fn xmp_export_enabled(state: State<'_, AppState>) -> CmdResult<bool> {
    Ok(db(&state)?.query_row("SELECT value = '1' FROM setting WHERE key = 'xmp.export'", [], |r| r.get(0)).unwrap_or(false))
}

#[tauri::command]
pub fn set_xmp_export_enabled(state: State<'_, AppState>, enabled: bool) -> CmdResult<()> {
    db(&state)?
        .execute(
            "INSERT INTO setting (key, value) VALUES ('xmp.export', ?1) ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            [if enabled { "1" } else { "0" }],
        )
        .map(|_| ())
        .map_err(err)
}

#[tauri::command]
pub fn set_root_locked(state: State<'_, AppState>, id: i64, locked: bool) -> CmdResult<()> {
    if !locked && state.run.lock().map_err(err)?.is_some() {
        return Err("Während ein Lauf aktiv ist, kann keine Sperre aufgehoben werden".into());
    }
    library::set_locked(&*db(&state)?, id, locked).map_err(err)
}

#[derive(Serialize)]
pub struct CacheInfo {
    data_dir: String,
    thumbs: u64,
    previews: u64,
    logs: u64,
}

#[tauri::command]
pub fn cache_info(state: State<'_, AppState>) -> CmdResult<CacheInfo> {
    let d = &state.data_dir;
    Ok(CacheInfo { data_dir: d.path().display().to_string(), thumbs: d.dir_size("thumbs"), previews: d.dir_size("previews"), logs: d.dir_size("logs") })
}

/// Leert einen Cache. Vorschauen und Thumbnails entstehen bei Bedarf bzw. beim
/// nächsten Scan neu. Protokolle sind kein Cache und bleiben.
#[tauri::command]
pub fn cache_clear(state: State<'_, AppState>, which: String) -> CmdResult<()> {
    match which.as_str() {
        "previews" => state.data_dir.clear_dir("previews").map_err(err),
        "thumbs" => {
            db(&state)?.execute("DELETE FROM thumb", []).map_err(err)?;
            state.data_dir.clear_dir("thumbs").map_err(err)
        }
        _ => Err("Unbekannter Cache".into()),
    }
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<'_, AppState>) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener().open_path(state.data_dir.path().to_string_lossy(), None::<&str>).map_err(err)
}

#[tauri::command]
pub fn run_discard(state: State<'_, AppState>, run_id: i64) -> CmdResult<()> {
    plan::discard(&*db(&state)?, run_id).map_err(err)
}

#[derive(Serialize)]
pub struct RunOpRow {
    seq: i64,
    op: String,
    src: Option<String>,
    dst: Option<String>,
    size: Option<i64>,
    status: String,
    error: Option<String>,
    keep_path: Option<String>,
}

#[derive(Serialize)]
pub struct RunDetails {
    id: i64,
    kind: String,
    title: String,
    status: String,
    dry_run: bool,
    created_at: String,
    finished_at: Option<String>,
    log_path: Option<String>,
    summary: serde_json::Value,
    /// Planungsparameter (z. B. Zielordner eines Imports).
    params: serde_json::Value,
    files: usize,
    mkdirs: usize,
    counts: std::collections::BTreeMap<String, usize>,
    before: std::collections::BTreeMap<String, usize>,
    after: std::collections::BTreeMap<String, usize>,
    /// Erste Operationen für die Liste (vollständig im Protokoll).
    ops: Vec<RunOpRow>,
    total_ops: usize,
}

#[tauri::command]
pub fn run_details(state: State<'_, AppState>, run_id: i64) -> CmdResult<RunDetails> {
    let db = db(&state)?;
    #[allow(clippy::type_complexity)]
    let (kind, title, status, dry_run, created_at, finished_at, log_path, summary, params): (String, String, String, bool, String, Option<String>, Option<String>, String, String) = db
        .query_row(
            "SELECT kind, title, status, dry_run, created_at, finished_at, log_path, summary, params FROM run WHERE id = ?1",
            [run_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?, r.get(7)?, r.get(8)?)),
        )
        .map_err(|_| "Lauf nicht gefunden".to_string())?;
    let mut stmt = db
        .prepare("SELECT seq, op, src, dst, size, status, error, keep_path FROM operation WHERE run_id = ?1 ORDER BY seq")
        .map_err(err)?;
    let all: Vec<RunOpRow> = stmt
        .query_map([run_id], |r| {
            Ok(RunOpRow { seq: r.get(0)?, op: r.get(1)?, src: r.get(2)?, dst: r.get(3)?, size: r.get(4)?, status: r.get(5)?, error: r.get(6)?, keep_path: r.get(7)? })
        })
        .map_err(err)?
        .collect::<rusqlite::Result<_>>()
        .map_err(err)?;
    let (before, after) = plan::folder_counts(&all.iter().map(|o| (o.op.clone(), o.src.clone(), o.dst.clone())).collect::<Vec<_>>());
    let mut counts = std::collections::BTreeMap::new();
    for o in &all {
        *counts.entry(format!("{}:{}", o.op, o.status)).or_insert(0) += 1;
    }
    let files = all.iter().filter(|o| o.op != "mkdir").count();
    let total_ops = all.len();
    // Interessante Operationen zuerst: Fehler, dann Dateioperationen.
    let mut ops = all;
    ops.sort_by_key(|o| (o.status != "failed", o.op == "mkdir", o.seq));
    // Papierkorb-Läufe nie kürzen: Die Bestätigung muss jede Datei zeigen.
    if !kind.ends_with("to_trash") {
        ops.truncate(500);
    }
    Ok(RunDetails {
        id: run_id,
        kind,
        title,
        status,
        dry_run,
        created_at,
        finished_at,
        log_path,
        summary: serde_json::from_str(&summary).unwrap_or_default(),
        params: serde_json::from_str(&params).unwrap_or_default(),
        files,
        mkdirs: total_ops - files,
        counts,
        before,
        after,
        ops,
        total_ops,
    })
}

#[derive(Serialize)]
pub struct RunListItem {
    id: i64,
    kind: String,
    title: String,
    status: String,
    dry_run: bool,
    created_at: String,
    finished_at: Option<String>,
    files: i64,
    done: i64,
    failed: i64,
    undone: i64,
    log_path: Option<String>,
}

#[tauri::command]
pub fn runs_list(state: State<'_, AppState>) -> CmdResult<Vec<RunListItem>> {
    let db = db(&state)?;
    let mut stmt = db
        .prepare(
            "SELECT r.id, r.kind, r.title, r.status, r.dry_run, r.created_at, r.finished_at, r.log_path,
                sum(o.op != 'mkdir'), sum(o.op != 'mkdir' AND o.status = 'done'), sum(o.status = 'failed'), sum(o.op != 'mkdir' AND o.status = 'undone')
             FROM run r LEFT JOIN operation o ON o.run_id = r.id GROUP BY r.id ORDER BY r.id DESC",
        )
        .map_err(err)?;
    stmt.query_map([], |r| {
        Ok(RunListItem {
            id: r.get(0)?,
            kind: r.get(1)?,
            title: r.get(2)?,
            status: r.get(3)?,
            dry_run: r.get(4)?,
            created_at: r.get(5)?,
            finished_at: r.get(6)?,
            log_path: r.get(7)?,
            files: r.get::<_, Option<i64>>(8)?.unwrap_or(0),
            done: r.get::<_, Option<i64>>(9)?.unwrap_or(0),
            failed: r.get::<_, Option<i64>>(10)?.unwrap_or(0),
            undone: r.get::<_, Option<i64>>(11)?.unwrap_or(0),
        })
    })
    .map_err(err)?
    .collect::<rusqlite::Result<_>>()
    .map_err(err)
}

#[derive(Clone, Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum RunFinished {
    Ok { run_id: i64, action: &'static str, report: RunReport },
    Error { run_id: i64, action: &'static str, message: String },
}

/// Startet Probelauf, Ausführung oder Undo im Hintergrund.
fn spawn_run(app: AppHandle, state: &State<'_, AppState>, run_id: i64, action: &'static str, confirmed_files: usize) -> CmdResult<()> {
    let control = {
        let mut slot = state.run.lock().map_err(err)?;
        if slot.is_some() {
            return Err("Es läuft bereits ein anderer Lauf".into());
        }
        if state.scan.lock().map_err(err)?.is_some() {
            return Err("Bitte warte, bis der Scan abgeschlossen ist".into());
        }
        let control = RunControl { run_id, cancel: Arc::new(AtomicBool::new(false)), pause: Arc::new(AtomicBool::new(false)) };
        *slot = Some(control.clone());
        control
    };
    std::thread::Builder::new()
        .name("run".into())
        .spawn(move || {
            let state = app.state::<AppState>();
            let result = (|| -> Result<RunReport, String> {
                let conn = crate::db::open(&state.db_path).map_err(err)?;
                let last = Mutex::new(Instant::now() - Duration::from_secs(1));
                let progress = |p: journal::RunProgress| {
                    if let Ok(mut l) = last.lock()
                        && (l.elapsed() >= Duration::from_millis(100) || p.done + 1 >= p.total)
                    {
                        *l = Instant::now();
                        let _ = app.emit("run-progress", p);
                    }
                };
                let ex = Exec { app: &state.data_dir, cancel: &control.cancel, pause: &control.pause, progress: &progress, simulate_crash_after: None, exiftool: state.tools.exiftool.as_ref(), photos: crate::photos::system() };
                match action {
                    "dry" => journal::dry_run(&conn, run_id, &ex),
                    "execute" => journal::execute(&conn, run_id, confirmed_files, &ex),
                    _ => journal::undo(&conn, run_id, &ex),
                }
                .map_err(err)
            })();
            if let Ok(mut slot) = state.run.lock() {
                *slot = None;
            }
            let payload = match result {
                Ok(report) => RunFinished::Ok { run_id, action, report },
                Err(message) => RunFinished::Error { run_id, action, message },
            };
            let _ = app.emit("run-finished", payload);
        })
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn run_dry(app: AppHandle, state: State<'_, AppState>, run_id: i64) -> CmdResult<()> {
    spawn_run(app, &state, run_id, "dry", 0)
}

/// Echte Ausführung. Nur bei ausgeschaltetem Dry-Run-Modus und mit der
/// Dateianzahl, die der Nutzer im Bestätigungsdialog gesehen hat.
#[tauri::command]
pub fn run_execute(app: AppHandle, state: State<'_, AppState>, run_id: i64, confirmed_files: usize) -> CmdResult<()> {
    if dry_run_mode(&*db(&state)?) {
        return Err("Der Dry-Run-Modus ist aktiv. Schalte ihn zum Ausführen in den Einstellungen aus.".into());
    }
    spawn_run(app, &state, run_id, "execute", confirmed_files)
}

#[tauri::command]
pub fn run_undo(app: AppHandle, state: State<'_, AppState>, run_id: i64) -> CmdResult<()> {
    spawn_run(app, &state, run_id, "undo", 0)
}

#[tauri::command]
pub fn run_pause(state: State<'_, AppState>, paused: bool) -> CmdResult<()> {
    if let Some(c) = state.run.lock().map_err(err)?.as_ref() {
        c.pause.store(paused, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub fn run_cancel(state: State<'_, AppState>) -> CmdResult<()> {
    if let Some(c) = state.run.lock().map_err(err)?.as_ref() {
        c.cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[derive(Serialize)]
pub struct RunState {
    running: Option<i64>,
    paused: bool,
    open_plans: i64,
    last_run: Option<String>,
}

#[tauri::command]
pub fn run_state(state: State<'_, AppState>) -> CmdResult<RunState> {
    let running = state.run.lock().map_err(err)?.as_ref().map(|c| (c.run_id, c.pause.load(Ordering::Relaxed)));
    let db = db(&state)?;
    let open_plans = db.query_row("SELECT count(*) FROM run WHERE status IN ('planned', 'aborted')", [], |r| r.get(0)).map_err(err)?;
    let last_run = db
        .query_row("SELECT finished_at FROM run WHERE finished_at IS NOT NULL ORDER BY finished_at DESC LIMIT 1", [], |r| r.get(0))
        .ok();
    Ok(RunState { running: running.map(|r| r.0), paused: running.is_some_and(|r| r.1), open_plans, last_run })
}

/// Öffnet das Protokoll eines Laufs im Standardprogramm.
#[tauri::command]
pub fn run_open_log(app: AppHandle, state: State<'_, AppState>, run_id: i64) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let path: Option<String> = db(&state)?.query_row("SELECT log_path FROM run WHERE id = ?1", [run_id], |r| r.get(0)).map_err(err)?;
    let path = path.ok_or("Für diesen Lauf gibt es noch kein Protokoll")?;
    app.opener().open_path(path, None::<&str>).map_err(err)
}

#[derive(Serialize)]
pub struct RejectGroup {
    group_id: i64,
    thumb: Option<String>,
    files: Vec<(String, i64)>,
}

/// Als Ausschuss markierte Gruppen mit allen Dateien (für die Einzelfreigabe).
#[tauri::command]
pub fn rejects_list(state: State<'_, AppState>) -> CmdResult<Vec<RejectGroup>> {
    let db = db(&state)?;
    let mut stmt = db
        .prepare(
            "SELECT a.group_id, r.path || '/' || a.rel_path, a.size,
                (SELECT t.hash FROM asset x JOIN thumb t ON t.hash = x.hash AND t.status = 'ok' WHERE x.group_id = a.group_id LIMIT 1)
             FROM asset a JOIN library_root r ON r.id = a.root_id JOIN rating rt ON rt.group_id = a.group_id
             WHERE rt.flag = 'reject' AND r.locked = 0 ORDER BY a.group_id, a.rel_path",
        )
        .map_err(err)?;
    let mut out: Vec<RejectGroup> = Vec::new();
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?, r.get::<_, Option<String>>(3)?)))
        .map_err(err)?;
    for row in rows {
        let (g, path, size, thumb) = row.map_err(err)?;
        match out.last_mut() {
            Some(last) if last.group_id == g => last.files.push((path, size)),
            _ => out.push(RejectGroup { group_id: g, thumb, files: vec![(path, size)] }),
        }
    }
    Ok(out)
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
        if state.run.lock().map_err(err)?.is_some() {
            return Err("Während ein Lauf Dateien verändert, ist kein Scan möglich".into());
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
