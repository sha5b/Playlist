mod audio;
mod commands;
mod db;
mod download;
mod metadata;

use std::sync::Arc;
use tauri::{Emitter, Manager};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            if cfg!(debug_assertions) {
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log::LevelFilter::Info)
                        .build(),
                )?;
            }

            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            let pool = db::init_db(&app_data_dir)
                .expect("Failed to initialize database");

            app.manage(Arc::new(pool));

            // Initialize audio engine with event forwarding to frontend
            let handle = app.handle().clone();
            let engine = audio::AudioEngine::new(Box::new(move |event| {
                let _ = handle.emit("player-event", &event);
            }));
            app.manage(Arc::new(engine));

            // Initialize download manager
            let db_arc: Arc<db::DbPool> = app.state::<Arc<db::DbPool>>().inner().clone();
            let dl_manager = download::DownloadManager::new(db_arc, app.handle().clone());
            app.manage(Arc::new(dl_manager));

            log::info!("Playlist app initialized successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::greet,
            commands::get_library_stats,
            // Tracks
            commands::library_get_tracks,
            commands::library_get_track,
            commands::library_delete_track,
            // Albums
            commands::library_get_albums,
            commands::library_get_album,
            // Artists
            commands::library_get_artists,
            commands::library_get_artist,
            // Playlists
            commands::library_get_playlists,
            commands::library_get_playlist,
            commands::library_create_playlist,
            commands::library_update_playlist,
            commands::library_delete_playlist,
            commands::library_add_to_playlist,
            commands::library_remove_from_playlist,
            commands::library_reorder_playlist,
            // Detail pages
            commands::library_get_album_tracks,
            commands::library_get_artist_tracks,
            commands::library_get_artist_albums,
            // Search
            commands::search,
            // Import
            commands::library_import_folder,
            // Settings
            commands::settings_get,
            commands::settings_set,
            commands::settings_get_all,
            // Downloads
            commands::download_parse_url,
            commands::download_check_deps,
            commands::download_ensure_deps,
            commands::download_start,
            commands::download_start_batch,
            commands::download_cancel,
            commands::download_retry,
            commands::download_get_active,
            commands::download_get_history,
            commands::download_clear_history,
            // Player
            commands::player::player_play_track,
            commands::player::player_play_tracks,
            commands::player::player_pause,
            commands::player::player_resume,
            commands::player::player_stop,
            commands::player::player_next,
            commands::player::player_prev,
            commands::player::player_seek,
            commands::player::player_set_volume,
            commands::player::player_set_shuffle,
            commands::player::player_set_repeat,
            commands::player::player_add_to_queue,
            commands::player::player_add_next,
            commands::player::player_remove_from_queue,
            commands::player::player_clear_queue,
            commands::player::player_get_state,
            commands::player::player_get_queue,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
