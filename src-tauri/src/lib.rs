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
        // Vorschau-Erzeugung und Dateilesen blockieren: eigener Thread je Anfrage.
        .register_asynchronous_uri_scheme_protocol("preview", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || responder.respond(commands::preview_protocol(&app, &request)));
        })
        .register_asynchronous_uri_scheme_protocol("media", |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || responder.respond(commands::media_protocol(&app, &request)));
        })
        .setup(|app| {
            let data_dir = AppDataDir::init(app.path().app_data_dir()?)?;
            let db_path = data_dir.join("sichter.db")?;
            let db = db::open(&db_path)?;
            // Läufe, die beim letzten Beenden noch liefen, gelten als abgebrochen
            // und können fortgesetzt oder rückgängig gemacht werden.
            journal::recover_interrupted(&db)?;
            let tools = tools::Tools::resolve(app.path().resource_dir().ok().as_deref());
            let previews = thumbs::preview::PreviewService::new(data_dir.clone(), tools.clone());
            app.manage(AppState {
                previews,
                run: Mutex::new(None),
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
            commands::storage_folders,
            commands::storage_summary,
            commands::largest_files,
            commands::dupes_analyze,
            commands::set_dupe_choice,
            commands::events_overview,
            commands::event_items,
            commands::events_save_settings,
            commands::events_recluster,
            commands::event_rename,
            commands::event_set_notes,
            commands::event_confirm,
            commands::events_merge,
            commands::event_split,
            commands::events_move,
            commands::camera_tz_set,
            commands::cull_items,
            commands::set_rating,
            commands::reset_rating,
            commands::previews_prepare,
            commands::open_in_default_app,
            commands::runs_dry_run_mode,
            commands::runs_set_dry_run_mode,
            commands::plan_template_preview,
            commands::plan_reorganize,
            commands::plan_rejects,
            commands::plan_dupes,
            commands::run_discard,
            commands::run_details,
            commands::runs_list,
            commands::run_dry,
            commands::run_execute,
            commands::run_undo,
            commands::run_pause,
            commands::run_cancel,
            commands::run_state,
            commands::run_open_log,
            commands::rejects_list,
        ])
        .run(tauri::generate_context!())
        .expect("Fehler beim Starten der App");
}
