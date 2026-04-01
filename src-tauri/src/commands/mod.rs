use std::sync::Arc;
use tauri::State;

use crate::db::DbPool;
use crate::db::models::LibraryStats;

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to Playlist.", name)
}

#[tauri::command]
pub fn get_library_stats(db: State<'_, Arc<DbPool>>) -> Result<LibraryStats, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

    let total_tracks: i64 = conn
        .query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    let total_albums: i64 = conn
        .query_row("SELECT COUNT(*) FROM albums", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    let total_artists: i64 = conn
        .query_row("SELECT COUNT(*) FROM artists", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    let total_playlists: i64 = conn
        .query_row("SELECT COUNT(*) FROM playlists", [], |row| row.get(0))
        .map_err(|e| e.to_string())?;

    let total_duration_ms: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(duration_ms), 0) FROM tracks",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    let total_size_bytes: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(file_size), 0) FROM tracks",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;

    Ok(LibraryStats {
        total_tracks,
        total_albums,
        total_artists,
        total_playlists,
        total_duration_ms,
        total_size_bytes,
    })
}
