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
