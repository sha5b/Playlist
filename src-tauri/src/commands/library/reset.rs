//! Full library reset: wipe tables, delete download files and cover art.

use std::sync::Arc;
use tauri::{Emitter, Manager, State};

use crate::db::DbPool;

fn cleanup_download_files(conn: &rusqlite::Connection, app_handle: &tauri::AppHandle) {
    let download_dir = {
        let dir = crate::db::settings::get_setting(conn, "download_dir").ok().flatten();
        match dir {
            Some(d) if !d.is_empty() => std::path::PathBuf::from(d),
            _ => app_handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("downloads"),
        }
    };
    if download_dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&download_dir) {
            log::warn!("Failed to remove downloads directory: {}", e);
        }
        if let Err(e) = std::fs::create_dir_all(&download_dir) {
            log::warn!("Failed to recreate downloads directory: {}", e);
        }
    }
}

fn reset_database_tables(conn: &rusqlite::Connection) -> Result<(), String> {
    conn.execute_batch(
        "DELETE FROM monitored_playlist_entries;
         DELETE FROM playlist_tracks;
         DELETE FROM downloads;
         DELETE FROM tracks;
         DELETE FROM albums;
         DELETE FROM artists;
         DELETE FROM playlists;
         DROP TABLE IF EXISTS tracks_fts;
         CREATE VIRTUAL TABLE tracks_fts USING fts5(
             title, artist_name, album_title, album_artist, genre,
             content='',
             contentless_delete=1,
             tokenize='unicode61 remove_diacritics 2'
         );"
    ).map_err(|e| e.to_string())
}

fn cleanup_cover_art(app_handle: &tauri::AppHandle) {
    if let Ok(covers_dir) = app_handle.path().app_data_dir().map(|d| d.join("covers")) {
        if covers_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&covers_dir) {
                log::warn!("Failed to remove covers directory: {}", e);
            }
            if let Err(e) = std::fs::create_dir_all(&covers_dir) {
                log::warn!("Failed to recreate covers directory: {}", e);
            }
        }
    }
}

/// Delete all library data: tracks (and their files), albums, artists, playlists, downloads, monitored entries.
/// Settings are preserved.
#[tauri::command]
pub async fn library_reset(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<crate::download::DownloadManager>>,
    app_handle: tauri::AppHandle,
    delete_files: bool,
) -> Result<(), String> {
    // Cancel all active downloads before wiping data
    manager.cancel_all().await;

    let conn = crate::db::lock(&db)?;

    if delete_files {
        cleanup_download_files(&conn, &app_handle);
    }
    reset_database_tables(&conn)?;
    cleanup_cover_art(&app_handle);

    // The frontend listens for "library-updated" — "library-changed" was a
    // dead event, so pages kept showing the wiped library after a reset.
    let _ = app_handle.emit("library-updated", ());
    log::info!("Library reset complete");
    Ok(())
}
