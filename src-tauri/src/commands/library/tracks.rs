//! Track queries, tag editing, and per-track deletion.

use std::path::Path;
use std::sync::Arc;
use crate::metadata::tags;
use rusqlite::params;
use tauri::{Emitter, State};

use crate::db::DbPool;
use crate::db::models::*;

// --- Library Stats ---

#[tauri::command]
pub fn get_library_stats(db: State<'_, Arc<DbPool>>) -> Result<LibraryStats, String> {
    let conn = crate::db::lock(&db)?;

    let total_tracks: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    let total_albums: i64 = conn.query_row("SELECT COUNT(*) FROM albums", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    let total_artists: i64 = conn.query_row("SELECT COUNT(*) FROM artists", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    let total_playlists: i64 = conn.query_row("SELECT COUNT(*) FROM playlists", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    let total_duration_ms: i64 = conn.query_row("SELECT COALESCE(SUM(duration_ms), 0) FROM tracks", [], |row| row.get(0)).map_err(|e| e.to_string())?;
    let total_size_bytes: i64 = conn.query_row("SELECT COALESCE(SUM(file_size), 0) FROM tracks", [], |row| row.get(0)).map_err(|e| e.to_string())?;

    Ok(LibraryStats {
        total_tracks,
        total_albums,
        total_artists,
        total_playlists,
        total_duration_ms,
        total_size_bytes,
    })
}

// --- Tracks ---

#[tauri::command]
pub fn library_get_tracks(
    db: State<'_, Arc<DbPool>>,
    offset: i64,
    limit: i64,
    sort_by: String,
    sort_dir: String,
    search: Option<String>,
    seed: Option<i64>,
) -> Result<TrackPage, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::tracks::get_tracks(&conn, offset, limit, &sort_by, &sort_dir, search.as_deref(), seed)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_track(db: State<'_, Arc<DbPool>>, id: i64) -> Result<Option<Track>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::tracks::get_track(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_genres(db: State<'_, Arc<DbPool>>) -> Result<Vec<String>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::tracks::get_distinct_genres(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_tracks_by_genre(
    db: State<'_, Arc<DbPool>>,
    genre: String,
    limit: i64,
) -> Result<Vec<Track>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::tracks::get_tracks_by_genre(&conn, &genre, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_delete_track(
    db: State<'_, Arc<DbPool>>,
    id: i64,
    delete_file: bool,
) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    let file_path = crate::db::tracks::delete_track(&conn, id, delete_file)
        .map_err(|e| e.to_string())?;

    if let Some(ref path) = file_path {
        if let Err(e) = std::fs::remove_file(path) {
            log::warn!("Failed to delete track file {}: {}", path, e);
        }
    }
    Ok(())
}

// --- Tag Editing ---

/// Editable tag fields. Every field is optional: `None` (or blank) means
/// "keep the current value" — only provided fields are applied.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct TrackTagUpdate {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub album_artist: Option<String>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub track_number: Option<i64>,
}

/// Trim a string field and treat empty strings as "not provided".
fn non_empty(s: &Option<String>) -> Option<String> {
    s.as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

/// Apply a tag update to a single track:
/// 1. write the tags into the audio file (lofty) — if this fails the DB is untouched,
/// 2. update the DB row, resolving-or-creating artist/album relations,
/// 3. reindex FTS and refresh metadata completeness.
///
/// NOTE: re-assigning artist/album can leave orphaned artist/album rows behind;
/// there is no shared orphan-cleanup helper, so they are left in place
/// (metadata_cleanup_duplicates removes orphaned albums on demand).
fn apply_tag_update(
    conn: &rusqlite::Connection,
    track_id: i64,
    update: &TrackTagUpdate,
) -> Result<(), String> {
    let title = non_empty(&update.title);
    let artist = non_empty(&update.artist);
    let album = non_empty(&update.album);
    let album_artist = non_empty(&update.album_artist);
    let genre = non_empty(&update.genre);

    if title.is_none()
        && artist.is_none()
        && album.is_none()
        && album_artist.is_none()
        && genre.is_none()
        && update.year.is_none()
        && update.track_number.is_none()
    {
        return Ok(()); // nothing to apply
    }

    let (file_path, cur_artist_id, cur_year): (String, Option<i64>, Option<i64>) = conn
        .query_row(
            "SELECT file_path, artist_id, year FROM tracks WHERE id = ?1",
            params![track_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .map_err(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => format!("Track {} not found", track_id),
            e => e.to_string(),
        })?;

    // 1) Write into the audio file first so the file and DB can't diverge
    //    on failure.
    let tw = tags::TagWrite {
        title: title.clone(),
        artist: artist.clone(),
        album: album.clone(),
        album_artist: album_artist.clone(),
        track_number: update.track_number.and_then(|n| u32::try_from(n).ok()),
        disc_number: None,
        year: update.year.and_then(|y| u32::try_from(y).ok()),
        genre: genre.clone(),
    };
    tags::write_tags(Path::new(&file_path), &tw)
        .map_err(|e| format!("Failed to write tags to {}: {}", file_path, e))?;

    // 2) Update DB relations + columns.
    let new_artist_id = match &artist {
        Some(name) => {
            let id = crate::db::artists::find_or_create(conn, name).map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE tracks SET artist_id = ?1 WHERE id = ?2",
                params![id, track_id],
            )
            .map_err(|e| e.to_string())?;
            Some(id)
        }
        None => cur_artist_id,
    };

    if let Some(album_title) = &album {
        let album_id = crate::db::albums::find_or_create(
            conn,
            album_title,
            new_artist_id,
            album_artist.as_deref(),
            update.year.or(cur_year),
        )
        .map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE tracks SET album_id = ?1 WHERE id = ?2",
            params![album_id, track_id],
        )
        .map_err(|e| e.to_string())?;
    }

    let scalar_updates: [(&str, Option<&dyn rusqlite::types::ToSql>); 5] = [
        ("title", title.as_ref().map(|v| v as &dyn rusqlite::types::ToSql)),
        ("album_artist", album_artist.as_ref().map(|v| v as &dyn rusqlite::types::ToSql)),
        ("genre", genre.as_ref().map(|v| v as &dyn rusqlite::types::ToSql)),
        ("year", update.year.as_ref().map(|v| v as &dyn rusqlite::types::ToSql)),
        ("track_number", update.track_number.as_ref().map(|v| v as &dyn rusqlite::types::ToSql)),
    ];
    for (column, value) in scalar_updates {
        if let Some(v) = value {
            conn.execute(
                &format!("UPDATE tracks SET {} = ?1 WHERE id = ?2", column),
                params![v, track_id],
            )
            .map_err(|e| e.to_string())?;
        }
    }

    // 3) Reindex FTS + refresh completeness.
    crate::db::tracks::update_fts(conn, track_id).map_err(|e| e.to_string())?;
    let _ = crate::db::tracks::update_completeness(conn, track_id);

    Ok(())
}

#[tauri::command]
pub fn library_update_track_tags(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    track_id: i64,
    tags: TrackTagUpdate,
) -> Result<Track, String> {
    let track = {
        let conn = crate::db::lock(&db)?;
        apply_tag_update(&conn, track_id, &tags)?;
        crate::db::tracks::get_track(&conn, track_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| format!("Track {} not found", track_id))?
    };
    let _ = app_handle.emit("library-updated", ());
    Ok(track)
}

/// Apply the provided tag fields to many tracks (e.g. fix album/artist/genre
/// across a selection). Tracks that fail are skipped; if any fail, an error
/// listing them is returned after the others were applied.
#[tauri::command]
pub fn library_update_tracks_tags(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    track_ids: Vec<i64>,
    tags: TrackTagUpdate,
) -> Result<Vec<Track>, String> {
    let mut updated: Vec<Track> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    {
        let conn = crate::db::lock(&db)?;
        for &track_id in &track_ids {
            match apply_tag_update(&conn, track_id, &tags) {
                Ok(()) => match crate::db::tracks::get_track(&conn, track_id) {
                    Ok(Some(t)) => updated.push(t),
                    Ok(None) => errors.push(format!("track {}: not found", track_id)),
                    Err(e) => errors.push(format!("track {}: {}", track_id, e)),
                },
                Err(e) => errors.push(format!("track {}: {}", track_id, e)),
            }
        }
    }
    if !updated.is_empty() {
        let _ = app_handle.emit("library-updated", ());
    }
    if !errors.is_empty() {
        return Err(format!(
            "Updated {} of {} tracks; failures: {}",
            updated.len(),
            track_ids.len(),
            errors.join("; ")
        ));
    }
    Ok(updated)
}

/// Delete all tracks in an album (and their files) but keep the album + enriched tracklist
/// so greyed-out placeholders remain for re-downloading.
#[tauri::command]
pub fn library_delete_album_tracks(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    album_id: i64,
) -> Result<i64, String> {
    let conn = crate::db::lock(&db)?;

    // Collect file paths to delete from disk
    let file_paths: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT file_path FROM tracks WHERE album_id = ?1 AND file_path IS NOT NULL")
            .map_err(|e| e.to_string())?;
        let paths: Vec<String> = stmt.query_map(params![album_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        paths
    };

    // Bulk delete from DB first — tracks_fts, playlist_tracks, then tracks.
    // DB rows must go before files so a failed delete can't leave rows
    // pointing at already-removed files. tracks_fts is contentless (no
    // triggers), so its rows must be purged explicitly or search pagination
    // counts ghost tracks forever.
    let track_ids: Vec<i64> = {
        let mut stmt = conn
            .prepare("SELECT id FROM tracks WHERE album_id = ?1")
            .map_err(|e| e.to_string())?;
        let ids: Vec<i64> = stmt.query_map(params![album_id], |row| row.get::<_, i64>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        ids
    };

    let count = track_ids.len() as i64;
    if count == 0 {
        return Ok(0);
    }

    // Build comma-separated ID list for IN clause
    let id_list: String = track_ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");

    conn.execute_batch(&format!(
        "DELETE FROM tracks_fts WHERE rowid IN ({ids});
         DELETE FROM playlist_tracks WHERE track_id IN ({ids});
         DELETE FROM tracks WHERE id IN ({ids});",
        ids = id_list
    )).map_err(|e| e.to_string())?;

    // Delete files from disk only after the DB commit (non-fatal if missing)
    for path in &file_paths {
        if let Err(e) = std::fs::remove_file(path) {
            log::warn!("Failed to delete track file {}: {}", path, e);
        }
    }

    log::info!("Deleted {} tracks from album {}", count, album_id);

    // Notify frontend
    let _ = app_handle.emit("library-updated", ());

    Ok(count)
}
