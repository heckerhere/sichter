pub mod commands;
pub mod db;
pub mod dupes;
pub mod events;
pub mod fsops;
pub mod hash;
pub mod journal;
pub mod meta;
pub mod plan;
pub mod scan;
pub mod storage;
pub mod thumbs;
pub mod tools;
pub mod video;

use std::sync::Mutex;

use tauri::Manager;

use commands::AppState;
use fsops::appdata::AppDataDir;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .register_uri_scheme_protocol("thumb", |ctx, request| commands::thumb_protocol(ctx.app_handle(), &request))
        .setup(|app| {
            let data_dir = AppDataDir::init(app.path().app_data_dir()?)?;
            let db_path = data_dir.join("sichter.db")?;
            let db = db::open(&db_path)?;
            let tools = tools::Tools::resolve(app.path().resource_dir().ok().as_deref());
            app.manage(AppState {
                data_dir,
                db_path,
                db: Mutex::new(db),
                tools,
                exiftool: Mutex::new(None),
                scan: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_roots,
            commands::add_roots,
            commands::remove_root,
            commands::list_groups,
            commands::list_unhandled,
            commands::library_stats,
            commands::start_scan,
            commands::cancel_scan,
            commands::scan_running,
        ])
        .run(tauri::generate_context!())
        .expect("Fehler beim Starten der App");
}
