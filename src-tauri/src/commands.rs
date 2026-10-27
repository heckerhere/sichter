//! Tauri-Commands. Dünne Schicht: Eingaben prüfen, an Module delegieren.

use std::sync::Mutex;

use serde::Serialize;
use tauri::State;

use crate::fsops::appdata::AppDataDir;
use crate::meta::exiftool::ExifTool;
use crate::tools::{self, ToolCommand, ToolSource, Tools};

pub struct AppState {
    pub data_dir: AppDataDir,
    pub db: Mutex<rusqlite::Connection>,
    pub tools: Tools,
    pub exiftool: Mutex<Option<ExifTool>>,
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
pub fn app_info(state: State<'_, AppState>) -> Result<AppInfo, String> {
    let db_schema_version = {
        let db = state.db.lock().map_err(|e| e.to_string())?;
        crate::db::current_version(&db).map_err(|e| e.to_string())?
    };

    let exiftool = tool_info("exiftool", state.tools.exiftool.as_ref(), |tool| {
        let mut guard = state.exiftool.lock().map_err(|e| e.to_string())?;
        if guard.is_none() {
            *guard = Some(ExifTool::spawn(tool).map_err(|e| e.to_string())?);
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
