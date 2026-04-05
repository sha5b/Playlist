mod audio;
mod commands;
mod db;
mod devices;
mod download;
mod error;
mod metadata;

use std::sync::Arc;
use tauri::image::Image;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{Emitter, Manager};

/// Request background status via the XDG Desktop Portal so the app
/// appears in GNOME's "Background Apps" indicator with a way to quit.
#[cfg(target_os = "linux")]
async fn request_background_status() -> Result<(), Box<dyn std::error::Error>> {
    use zbus::zvariant::{OwnedValue, Value};

    let connection = zbus::Connection::session().await?;
    let proxy = zbus::Proxy::new(
        &connection,
        "org.freedesktop.portal.Desktop",
        "/org/freedesktop/portal/desktop",
        "org.freedesktop.portal.Background",
    )
    .await?;

    let mut options: std::collections::HashMap<&str, Value<'_>> = std::collections::HashMap::new();
    options.insert("reason", Value::from("Music playback continues in the background"));
    options.insert("autostart", Value::from(false));

    let _reply: OwnedValue = proxy.call("RequestBackground", &("", options)).await?;
    log::info!("Registered with XDG Background portal");
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {}))
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_fs::init())
        .setup(|app| {
            // Enable logging in all builds (debug gets more detail)
            {
                use tauri_plugin_log::{Target, TargetKind};
                let log_level = if cfg!(debug_assertions) {
                    log::LevelFilter::Debug
                } else {
                    log::LevelFilter::Info
                };
                app.handle().plugin(
                    tauri_plugin_log::Builder::default()
                        .level(log_level)
                        .targets([
                            Target::new(TargetKind::Stdout),
                            Target::new(TargetKind::Webview),
                            Target::new(TargetKind::LogDir { file_name: None }),
                        ])
                        .build(),
                )?;
            }

            let app_data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| {
                    log::error!("Failed to get app data directory: {}", e);
                    e
                })?;

            let pool = db::init_db(&app_data_dir)
                .map_err(|e| {
                    log::error!("Failed to initialize database: {}", e);
                    Box::<dyn std::error::Error>::from(format!("Database initialization failed: {}", e))
                })?;

            app.manage(Arc::new(pool));

            // Resolve ffmpeg binary for audio transcoding (opus, wma, etc.)
            let ffmpeg_path = {
                let bin_dir = download::setup::get_bin_dir(app.handle());
                let local = download::setup::get_ffmpeg_path(&bin_dir);
                if local.exists() {
                    Some(local.to_string_lossy().to_string())
                } else {
                    Some("ffmpeg".to_string()) // try system PATH
                }
            };

            // Initialize audio engine with event forwarding to frontend
            let handle = app.handle().clone();
            let engine = audio::AudioEngine::new(ffmpeg_path, Box::new(move |event| {
                let _ = handle.emit("player-event", &event);
            }));
            app.manage(Arc::new(engine));

            // Initialize download manager with its own DB connection
            // so downloads don't block UI/player queries on the main connection.
            let dl_conn = db::open_download_conn(&app_data_dir)
                .map_err(|e| {
                    log::error!("Failed to open download DB connection: {}", e);
                    Box::<dyn std::error::Error>::from(format!("Download DB connection failed: {}", e))
                })?;
            let dl_db = Arc::new(std::sync::Mutex::new(dl_conn));
            let dl_manager = Arc::new(download::DownloadManager::new(dl_db, app.handle().clone()));
            dl_manager.resume_interrupted();
            app.manage(dl_manager);

            // Initialize device manager for USB device sync
            let device_manager = Arc::new(devices::DeviceManager::new(app.handle().clone()));
            app.manage(device_manager);

            // System tray with playback controls
            let show = MenuItemBuilder::with_id("show", "Show Playlist").build(app)?;
            let play_pause = MenuItemBuilder::with_id("play_pause", "Play / Pause").build(app)?;
            let next_track = MenuItemBuilder::with_id("next", "Next Track").build(app)?;
            let prev_track = MenuItemBuilder::with_id("prev", "Previous Track").build(app)?;
            let quit = MenuItemBuilder::with_id("quit", "Exit").build(app)?;
            let tray_menu = MenuBuilder::new(app)
                .item(&show)
                .separator()
                .item(&prev_track)
                .item(&play_pause)
                .item(&next_track)
                .separator()
                .item(&quit)
                .build()?;

            let tray_icon = Image::from_bytes(include_bytes!("../icons/tray-icon.png"))
                .expect("bundled tray icon must be valid");

            // Set window icon on Linux (dev mode shows generic icon otherwise).
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(icon) = Image::from_bytes(include_bytes!("../icons/icon.png")) {
                    let _ = window.set_icon(icon);
                }
            }

            // Store tray icon in managed state so it lives for the app's lifetime.
            // Without this, the TrayIcon is dropped at the end of setup() and
            // disappears on Linux (libappindicator destroys it on drop).
            let tray = TrayIconBuilder::new()
                .icon(tray_icon)
                .icon_as_template(cfg!(target_os = "macos"))
                .menu(&tray_menu)
                .show_menu_on_left_click(false)
                .tooltip("Playlist")
                .on_menu_event(|app, event| {
                    match event.id().as_ref() {
                        "show" => {
                            if let Some(window) = app.get_webview_window("main") {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                        "play_pause" => {
                            if let Some(engine) = app.try_state::<Arc<audio::AudioEngine>>() {
                                let state = engine.get_state();
                                match state.state {
                                    audio::engine::PlayerState::Playing => {
                                        engine.send(audio::engine::PlayerCommand::Pause);
                                    }
                                    audio::engine::PlayerState::Paused => {
                                        engine.send(audio::engine::PlayerCommand::Resume);
                                    }
                                    _ => {}
                                }
                            }
                        }
                        "next" => {
                            if let Some(engine) = app.try_state::<Arc<audio::AudioEngine>>() {
                                engine.send(audio::engine::PlayerCommand::Next);
                            }
                        }
                        "prev" => {
                            if let Some(engine) = app.try_state::<Arc<audio::AudioEngine>>() {
                                engine.send(audio::engine::PlayerCommand::Prev);
                            }
                        }
                        "quit" => {
                            app.exit(0);
                        }
                        _ => {}
                    }
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            if window.is_visible().unwrap_or(false) {
                                let _ = window.hide();
                            } else {
                                let _ = window.show();
                                let _ = window.unminimize();
                                let _ = window.set_focus();
                            }
                        }
                    }
                })
                .build(app)?;
            app.manage(tray);

            // Spawn background auto-enrichment task
            {
                let db = app.state::<Arc<std::sync::Mutex<rusqlite::Connection>>>().inner().clone();
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    // Small delay to let the UI load first
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                    commands::auto_enrich_library(db, handle).await;
                });
            }

            // On Linux, register with XDG Background portal so the app
            // appears in GNOME's "Background Apps" indicator when minimized.
            #[cfg(target_os = "linux")]
            {
                tauri::async_runtime::spawn(async {
                    let _ = request_background_status().await;
                });
            }

            log::info!("Playlist app initialized successfully");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_library_stats,
            // Tracks
            commands::library_get_tracks,
            commands::library_get_track,
            commands::library_get_genres,
            commands::library_get_tracks_by_genre,
            commands::library_delete_track,
            commands::library_delete_album_tracks,
            commands::library_reset,
            // Albums
            commands::library_get_albums,
            commands::library_get_album,
            commands::library_get_recently_played_albums,
            commands::library_get_recently_added_albums,
            // Artists
            commands::library_get_artists,
            commands::library_get_artist,
            // Playlists
            commands::library_get_playlists,
            commands::library_get_playlist,
            commands::library_get_playlist_tracks,
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
            commands::download_search_and_start,
            commands::download_search_and_start_batch,
            commands::download_cancel,
            commands::download_retry,
            commands::download_get_active,
            commands::download_get_history,
            commands::download_clear_history,
            // Download sources
            commands::download_get_sources_status,
            commands::download_set_source_credentials,
            commands::download_test_source,
            commands::download_refresh_sources,
            // Manager (monitored playlists)
            commands::manager_get_playlists,
            commands::manager_get_entries,
            commands::manager_get_new_entries,
            commands::manager_add_playlist,
            commands::manager_sync_playlist,
            commands::manager_download_entry,
            commands::manager_download_new,
            commands::manager_skip_entry,
            commands::manager_cancel_entry,
            commands::manager_cancel_all,
            commands::manager_remove_playlist,
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
            commands::player::player_move_in_queue,
            commands::player::player_skip_to,
            commands::player::player_remove_from_queue,
            commands::player::player_clear_queue,
            commands::player::player_get_state,
            commands::player::player_get_queue,
            commands::player::player_random_tracks,
            commands::player::player_get_audio_devices,
            commands::player::player_set_audio_device,
            commands::player::player_record_play,
            // Metadata enrichment
            commands::enrich_track,
            commands::enrich_album,
            commands::scan_missing_metadata,
            commands::get_metadata_stats,
            commands::metadata_stop_scan,
            commands::metadata_delete_all,
            commands::metadata_cleanup_duplicates,
            commands::metadata_cleanup_duplicate_tracks,
            commands::detect_album_mismatches,
            // Album download status
            commands::library_get_album_download_status,
            commands::library_get_albums_download_status,
            commands::library_export,
            // Artist enrichment
            commands::enrich_artist,
            commands::library_get_artist_missing_albums,
            commands::download_artist_missing,
            commands::download_music_video,
            // Devices
            commands::devices::devices_scan,
            commands::devices::devices_get_all,
            commands::devices::devices_get_detail,
            commands::devices::devices_configure,
            commands::devices::devices_add_playlist,
            commands::devices::devices_remove_playlist,
            commands::devices::devices_sync,
            commands::devices::devices_cancel_sync,
            commands::devices::devices_clear_history,
        ])
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            match event {
                tauri::RunEvent::Exit => {
                    // Gracefully shut down the audio thread before the process exits
                    if let Some(engine) = app_handle.try_state::<Arc<audio::AudioEngine>>() {
                        engine.shutdown();
                    }
                    log::info!("Application exiting gracefully");
                }
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => {
                    // macOS: clicking the dock icon when the window is hidden should show it
                    if let Some(window) = app_handle.get_webview_window("main") {
                        let _ = window.show();
                        let _ = window.unminimize();
                        let _ = window.set_focus();
                    }
                }
                _ => {}
            }
        });
}
