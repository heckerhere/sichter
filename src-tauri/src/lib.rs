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
        .setup(|app| {
            let data_dir = AppDataDir::init(app.path().app_data_dir()?)?;
            let db = db::open(&data_dir.join("sichter.db")?)?;
            let tools = tools::Tools::resolve(app.path().resource_dir().ok().as_deref());
            app.manage(AppState {
                data_dir,
                db: Mutex::new(db),
                tools,
                exiftool: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![commands::app_info])
        .run(tauri::generate_context!())
        .expect("Fehler beim Starten der App");
}
