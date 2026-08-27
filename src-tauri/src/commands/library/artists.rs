//! Artist queries and artist detail tracklists.

use std::sync::Arc;
use tauri::State;

use crate::db::DbPool;
use crate::db::models::*;

// --- Artists ---

#[tauri::command]
pub fn library_get_artists(
    db: State<'_, Arc<DbPool>>,
    offset: i64,
    limit: i64,
    search: Option<String>,
) -> Result<(Vec<Artist>, i64), String> {
    let conn = crate::db::lock(&db)?;
    crate::db::artists::get_artists(&conn, offset, limit, search.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_artist(db: State<'_, Arc<DbPool>>, id: i64) -> Result<Option<Artist>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::artists::get_artist(&conn, id).map_err(|e| e.to_string())
}

// --- Artist Tracks & Albums ---

#[tauri::command]
pub fn library_get_artist_tracks(
    db: State<'_, Arc<DbPool>>,
    artist_id: i64,
) -> Result<Vec<Track>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::tracks::get_tracks_by_artist(&conn, artist_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_artist_albums(
    db: State<'_, Arc<DbPool>>,
    artist_id: i64,
) -> Result<Vec<Album>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::albums::get_albums_by_artist(&conn, artist_id).map_err(|e| e.to_string())
}
