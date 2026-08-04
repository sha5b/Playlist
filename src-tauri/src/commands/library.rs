use std::sync::Arc;
use std::path::{Path, PathBuf};
use rusqlite::params;
use tauri::{Emitter, Manager, State};
use walkdir::WalkDir;

use crate::db::DbPool;
use crate::db::models::*;
use crate::metadata::tags;

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
) -> Result<TrackPage, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::tracks::get_tracks(&conn, offset, limit, &sort_by, &sort_dir, search.as_deref())
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

    // Delete files from disk (non-fatal if missing)
    for path in &file_paths {
        if let Err(e) = std::fs::remove_file(path) {
            log::warn!("Failed to delete track file {}: {}", path, e);
        }
    }

    // Bulk delete from DB — FTS, playlist_tracks, then tracks
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
        "DELETE FROM playlist_tracks WHERE track_id IN ({ids});
         DELETE FROM tracks WHERE id IN ({ids});",
        ids = id_list
    )).map_err(|e| e.to_string())?;

    log::info!("Deleted {} tracks from album {}", count, album_id);

    // Notify frontend
    let _ = app_handle.emit("library-updated", ());

    Ok(count)
}

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

// --- Albums ---

#[tauri::command]
pub fn library_get_albums(
    db: State<'_, Arc<DbPool>>,
    offset: i64,
    limit: i64,
    search: Option<String>,
) -> Result<(Vec<Album>, i64), String> {
    let conn = crate::db::lock(&db)?;
    crate::db::albums::get_albums(&conn, offset, limit, search.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_album(db: State<'_, Arc<DbPool>>, id: i64) -> Result<Option<Album>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::albums::get_album(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_recently_played_albums(db: State<'_, Arc<DbPool>>, limit: i64) -> Result<Vec<Album>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::albums::get_recently_played_albums(&conn, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_recently_added_albums(db: State<'_, Arc<DbPool>>, limit: i64) -> Result<Vec<Album>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::albums::get_recently_added_albums(&conn, limit).map_err(|e| e.to_string())
}

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

// --- Playlists ---

#[tauri::command]
pub fn library_get_playlists(db: State<'_, Arc<DbPool>>) -> Result<Vec<Playlist>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::playlists::get_playlists(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_playlist(
    db: State<'_, Arc<DbPool>>,
    id: i64,
) -> Result<Option<PlaylistDetail>, String> {
    let conn = crate::db::lock(&db)?;
    let playlist = crate::db::playlists::get_playlist(&conn, id).map_err(|e| e.to_string())?;
    match playlist {
        Some(p) => {
            // Backfill playlist_tracks from monitored entries for already-downloaded tracks
            if let Err(e) = crate::db::playlists::backfill_playlist_tracks(&conn, id) {
                log::warn!("Backfill playlist_tracks failed for playlist {}: {}", id, e);
            }
            let tracks = crate::db::playlists::get_playlist_tracks(&conn, id)
                .map_err(|e| e.to_string())?;
            Ok(Some(PlaylistDetail { playlist: p, tracks }))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub fn library_get_playlist_tracks(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
    offset: i64,
    limit: i64,
) -> Result<TrackPage, String> {
    let conn = crate::db::lock(&db)?;
    if let Err(e) = crate::db::playlists::backfill_playlist_tracks(&conn, playlist_id) {
        log::warn!("Backfill playlist_tracks failed for playlist {}: {}", playlist_id, e);
    }
    crate::db::playlists::get_playlist_tracks_page(&conn, playlist_id, offset, limit)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_create_playlist(
    db: State<'_, Arc<DbPool>>,
    name: String,
    description: Option<String>,
) -> Result<Playlist, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::playlists::create_playlist(&conn, &name, description.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_update_playlist(
    db: State<'_, Arc<DbPool>>,
    id: i64,
    name: Option<String>,
    description: Option<String>,
) -> Result<Playlist, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::playlists::update_playlist(&conn, id, name.as_deref(), description.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_delete_playlist(db: State<'_, Arc<DbPool>>, id: i64) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    crate::db::playlists::delete_playlist(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_add_to_playlist(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
    track_ids: Vec<i64>,
) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    for track_id in track_ids {
        crate::db::playlists::add_track_to_playlist(&conn, playlist_id, track_id)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
pub fn library_remove_from_playlist(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
    track_id: i64,
) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    crate::db::playlists::remove_track_from_playlist(&conn, playlist_id, track_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_reorder_playlist(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
    from: i64,
    to: i64,
) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    crate::db::playlists::reorder_playlist(&conn, playlist_id, from, to)
        .map_err(|e| e.to_string())
}

// --- Search ---

#[tauri::command]
pub fn search(
    db: State<'_, Arc<DbPool>>,
    query: String,
    limit: Option<i64>,
) -> Result<SearchResults, String> {
    let conn = crate::db::lock(&db)?;
    let limit = limit.unwrap_or(50);

    let tracks = if query.trim().is_empty() {
        vec![]
    } else {
        crate::db::tracks::search_tracks_fts(&conn, &query, limit)
            .unwrap_or_default()
    };

    // Also search albums and artists by name
    let albums = if query.trim().is_empty() {
        vec![]
    } else {
        let pattern = format!("%{}%", query);
        let mut stmt = conn
            .prepare(
                "SELECT al.id, al.title, al.artist_id, al.album_artist, al.year, al.genre,
                        al.total_tracks, al.total_discs, al.musicbrainz_id, al.cover_art_path,
                        a.name as artist_name, COUNT(t.id) as track_count
                 FROM albums al
                 LEFT JOIN artists a ON al.artist_id = a.id
                 LEFT JOIN tracks t ON t.album_id = al.id
                 WHERE al.title LIKE ?1
                 GROUP BY al.id
                 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let result = stmt.query_map(params![pattern, limit], |row| {
            Ok(Album {
                id: row.get(0)?,
                title: row.get(1)?,
                artist_id: row.get(2)?,
                album_artist: row.get(3)?,
                year: row.get(4)?,
                genre: row.get(5)?,
                total_tracks: row.get(6)?,
                total_discs: row.get(7)?,
                musicbrainz_id: row.get(8)?,
                cover_art_path: row.get(9)?,
                label: None,
                release_date: None,
                description: None,
                album_type: None,
                enriched_tracklist: None,
                purchase_url: None,
                artist_name: row.get(10)?,
                track_count: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();
        result
    };

    let artists = if query.trim().is_empty() {
        vec![]
    } else {
        let pattern = format!("%{}%", query);
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.name, a.sort_name, a.musicbrainz_id, a.image_path, a.bio,
                        COUNT(t.id) as track_count
                 FROM artists a
                 LEFT JOIN tracks t ON t.artist_id = a.id
                 WHERE a.name LIKE ?1
                 GROUP BY a.id
                 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let result = stmt.query_map(params![pattern, limit], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_name: row.get(2)?,
                musicbrainz_id: row.get(3)?,
                image_path: row.get(4)?,
                bio: row.get(5)?,
                country: None,
                begin_year: None,
                artist_type: None,
                website_url: None,
                track_count: row.get(6)?,
                has_enriched_discography: false,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();
        result
    };

    Ok(SearchResults {
        tracks,
        albums,
        artists,
    })
}

// --- Album Tracks ---

#[tauri::command]
pub fn library_get_album_tracks(
    db: State<'_, Arc<DbPool>>,
    album_id: i64,
) -> Result<Vec<Track>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::tracks::get_tracks_by_album(&conn, album_id).map_err(|e| e.to_string())
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

// --- Settings ---

#[tauri::command]
pub fn settings_get(
    db: State<'_, Arc<DbPool>>,
    key: String,
) -> Result<Option<String>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::settings::get_setting(&conn, &key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_set(
    db: State<'_, Arc<DbPool>>,
    key: String,
    value: String,
) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    crate::db::settings::set_setting(&conn, &key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_get_all(
    db: State<'_, Arc<DbPool>>,
) -> Result<Vec<(String, String)>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::settings::get_all_settings(&conn).map_err(|e| e.to_string())
}

// --- Import ---

#[tauri::command]
pub fn library_import_folder(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    path: String,
) -> Result<i64, String> {
    let conn = crate::db::lock(&db)?;

    let covers_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e: tauri::Error| e.to_string())?
        .join("covers");

    let mut imported = 0i64;

    // Collect all audio file paths first
    let audio_files: Vec<_> = WalkDir::new(&path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && tags::is_audio_file(e.path()))
        .collect();

    if audio_files.is_empty() {
        return Ok(0);
    }

    // Batch-check which files already exist in the library
    let existing_paths: std::collections::HashSet<String> = {
        let mut stmt = conn
            .prepare("SELECT file_path FROM tracks")
            .map_err(|e| e.to_string())?;
        let rows: Vec<String> = stmt.query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        rows.into_iter().collect()
    };

    // Wrap all inserts in a single transaction (10-50x faster than auto-commit per row)
    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;

    let mut fts_ids: Vec<i64> = Vec::new();

    for entry in &audio_files {
        let file_path = entry.path();
        let path_str = file_path.to_string_lossy().to_string();

        if existing_paths.contains(&path_str) {
            continue;
        }

        // Read tags and cover art in a single file read
        let (tag_data, cover_art_path) = match tags::read_tags_and_cover(file_path, &covers_dir) {
            Ok((d, c)) => (d, c),
            Err(e) => {
                log::warn!("Failed to read tags from {:?}: {}", file_path, e);
                continue;
            }
        };

        // Find or create artist
        let artist_id = tag_data
            .artist
            .as_ref()
            .and_then(|name| crate::db::artists::find_or_create(&conn, name).ok());

        // Find or create album
        let album_id = tag_data.album.as_ref().and_then(|title| {
            crate::db::albums::find_or_create(
                &conn,
                title,
                artist_id,
                tag_data.album_artist.as_deref(),
                tag_data.year.map(|y| y as i64),
            )
            .ok()
        });

        // Get file size
        let file_size = std::fs::metadata(file_path).map(|m| m.len() as i64).ok();

        // Save values needed after INSERT (before they get moved into params)
        let total_tracks_val = tag_data.total_tracks.map(|t| t as i64);
        let total_discs_val = tag_data.total_discs.map(|d| d as i64);
        let genre_for_album = tag_data.genre.clone();

        // Insert track
        let result = conn.execute(
            "INSERT INTO tracks (title, artist_id, album_id, album_artist, duration_ms,
                track_number, disc_number, genre, year, file_path, file_size, format,
                bitrate, sample_rate, channels, cover_art_path, source_platform)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'local')",
            params![
                tag_data.title.unwrap_or_else(|| "Unknown".to_string()),
                artist_id,
                album_id,
                tag_data.album_artist,
                tag_data.duration_ms.map(|d| d as i64),
                tag_data.track_number.map(|t| t as i64),
                tag_data.disc_number.map(|d| d as i64),
                tag_data.genre,
                tag_data.year.map(|y| y as i64),
                path_str,
                file_size,
                tag_data.format,
                tag_data.bitrate.map(|b| b as i64),
                tag_data.sample_rate.map(|s| s as i64),
                tag_data.channels.map(|c| c as i64),
                cover_art_path,
            ],
        );

        match result {
            Ok(_) => {
                fts_ids.push(conn.last_insert_rowid());
                imported += 1;

                // Propagate cover art to the album only — the embedded image is album/track
                // art, not an artist photo, and setting it here would permanently block the
                // real artist photo enrichment fetches later (fix A1).
                if let Some(ref cover) = cover_art_path {
                    if let Some(aid) = album_id {
                        let _ = crate::db::albums::update_cover_art_if_missing(&conn, aid, cover);
                    }
                }
                // Propagate album metadata from tags
                if let Some(aid) = album_id {
                    let _ = crate::db::albums::update_metadata_if_missing(
                        &conn,
                        aid,
                        total_tracks_val,
                        total_discs_val,
                        genre_for_album.as_deref(),
                    );
                }
            }
            Err(e) => {
                log::warn!("Failed to insert track {:?}: {}", file_path, e);
            }
        }
    }

    // Batch FTS updates after all inserts
    for track_id in fts_ids {
        let _ = crate::db::tracks::update_fts(&conn, track_id);
    }

    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;

    log::info!("Imported {} tracks from {}", imported, path);
    if imported > 0 {
        let _ = app_handle.emit("library-updated", ());
    }
    Ok(imported)
}

// --- Album Download Status ---

#[tauri::command]
pub fn library_get_album_download_status(
    db: State<'_, Arc<DbPool>>,
    album_id: i64,
) -> Result<serde_json::Value, String> {
    let conn = crate::db::lock(&db)?;

    // An unknown album id is "unknown" status, not an error (matches the
    // batch variant's behavior).
    let tracklist_json: Option<String> = match conn.query_row(
        "SELECT enriched_tracklist FROM albums WHERE id = ?1",
        params![album_id],
        |row| row.get(0),
    ) {
        Ok(v) => v,
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.to_string()),
    };

    let Some(json_str) = tracklist_json else {
        return Ok(serde_json::json!({ "total_expected": 0, "total_local": 0, "status": "unknown" }));
    };

    let tracklist: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap_or_default();
    if tracklist.is_empty() {
        return Ok(serde_json::json!({ "total_expected": 0, "total_local": 0, "status": "unknown" }));
    }

    let total_expected = tracklist.len() as i64;
    let total_local: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE album_id = ?1",
        params![album_id],
        |row| row.get(0),
    ).unwrap_or(0);

    let status = if total_local >= total_expected {
        "complete"
    } else if total_local > 0 {
        "partial"
    } else {
        "none"
    };

    Ok(serde_json::json!({
        "total_expected": total_expected,
        "total_local": total_local,
        "status": status,
    }))
}

#[tauri::command]
pub fn library_get_albums_download_status(
    db: State<'_, Arc<DbPool>>,
    album_ids: Vec<i64>,
) -> Result<serde_json::Value, String> {
    let conn = crate::db::lock(&db)?;
    let mut results = serde_json::Map::new();

    for album_id in &album_ids {
        let tracklist_json: Option<String> = conn.query_row(
            "SELECT enriched_tracklist FROM albums WHERE id = ?1",
            params![album_id],
            |row| row.get(0),
        ).unwrap_or(None);

        let (total_expected, status) = if let Some(ref json_str) = tracklist_json {
            let tracklist: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap_or_default();
            if tracklist.is_empty() {
                (0i64, "unknown")
            } else {
                let expected = tracklist.len() as i64;
                let local: i64 = conn.query_row(
                    "SELECT COUNT(*) FROM tracks WHERE album_id = ?1",
                    params![album_id],
                    |row| row.get(0),
                ).unwrap_or(0);
                let s = if local >= expected { "complete" } else if local > 0 { "partial" } else { "none" };
                (expected, s)
            }
        } else {
            (0, "unknown")
        };

        let total_local: i64 = conn.query_row(
            "SELECT COUNT(*) FROM tracks WHERE album_id = ?1",
            params![album_id],
            |row| row.get(0),
        ).unwrap_or(0);

        results.insert(album_id.to_string(), serde_json::json!({
            "total_expected": total_expected,
            "total_local": total_local,
            "status": status,
        }));
    }

    Ok(serde_json::Value::Object(results))
}

#[tauri::command]
pub fn library_get_artist_missing_albums(
    db: State<'_, Arc<DbPool>>,
    artist_id: i64,
) -> Result<serde_json::Value, String> {
    let conn = crate::db::lock(&db)?;

    // Get enriched discography
    let disco_json: Option<String> = conn.query_row(
        "SELECT enriched_discography FROM artists WHERE id = ?1",
        params![artist_id],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;

    let Some(json_str) = disco_json else {
        return Ok(serde_json::json!({ "missing": [], "total_discography": 0 }));
    };

    let discography: Vec<crate::metadata::musicbrainz::ArtistDiscographyEntry> =
        serde_json::from_str(&json_str).unwrap_or_default();

    // Get local albums for this artist (titles, lowercased for comparison)
    let mut stmt = conn.prepare(
        "SELECT LOWER(title), musicbrainz_id FROM albums WHERE artist_id = ?1"
    ).map_err(|e| e.to_string())?;
    let local_albums: std::collections::HashSet<String> = stmt.query_map(params![artist_id], |row| {
        row.get::<_, String>(0)
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let local_mbids: std::collections::HashSet<String> = {
        let mut stmt2 = conn.prepare(
            "SELECT musicbrainz_id FROM albums WHERE artist_id = ?1 AND musicbrainz_id IS NOT NULL"
        ).map_err(|e| e.to_string())?;
        let results: std::collections::HashSet<String> = stmt2.query_map(params![artist_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        results
    };

    let missing: Vec<&crate::metadata::musicbrainz::ArtistDiscographyEntry> = discography.iter()
        .filter(|entry| {
            !local_mbids.contains(&entry.mbid)
                && !local_albums.contains(&entry.title.to_lowercase())
        })
        .collect();

    let total = discography.len();
    Ok(serde_json::json!({
        "missing": missing,
        "total_discography": total,
    }))
}

// --- Export Library ---

/// Sanitize a string for use as a filename/directory name
fn sanitize_filename(name: &str) -> String {
    let sanitized: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    let trimmed = sanitized.trim().trim_matches('.');
    if trimmed.is_empty() {
        "Unknown".to_string()
    } else {
        trimmed.to_string()
    }
}

#[derive(serde::Serialize, Clone)]
pub struct ExportProgress {
    pub current: i64,
    pub total: i64,
    pub track_title: String,
}

#[derive(serde::Serialize)]
pub struct ExportResult {
    pub exported: i64,
    pub skipped: i64,
    pub failed: i64,
    pub destination: String,
}

#[tauri::command]
pub async fn library_export(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    destination: String,
) -> Result<ExportResult, String> {
    let dest = PathBuf::from(&destination);
    if !dest.exists() {
        std::fs::create_dir_all(&dest).map_err(|e| format!("Failed to create destination: {}", e))?;
    }

    // Query all tracks with artist and album names
    #[allow(clippy::type_complexity)]
    let rows: Vec<(i64, String, Option<String>, Option<String>, Option<i64>, Option<i64>, String)> = {
        let conn = crate::db::lock(&db)?;
        let mut stmt = conn.prepare(
            "SELECT t.id, t.file_path, ar.name, al.title, t.track_number, t.disc_number, t.title
             FROM tracks t
             LEFT JOIN artists ar ON t.artist_id = ar.id
             LEFT JOIN albums al ON t.album_id = al.id
             ORDER BY ar.name, al.title, t.disc_number, t.track_number"
        ).map_err(|e| e.to_string())?;

        let result: Vec<_> = stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<i64>>(4)?,
                row.get::<_, Option<i64>>(5)?,
                row.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
        result
    };

    let total = rows.len() as i64;
    let mut exported: i64 = 0;
    let mut skipped: i64 = 0;
    let mut failed: i64 = 0;

    for (i, (track_id, file_path, artist_name, album_title, track_num, disc_num, title)) in rows.iter().enumerate() {
        // Emit progress
        let _ = app_handle.emit("export-progress", ExportProgress {
            current: i as i64 + 1,
            total,
            track_title: title.clone(),
        });

        let src = Path::new(file_path);
        if !src.exists() {
            skipped += 1;
            continue;
        }

        let ext = src.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp3");

        let artist_dir = sanitize_filename(artist_name.as_deref().unwrap_or("Unknown Artist"));
        let album_dir = sanitize_filename(album_title.as_deref().unwrap_or("Unknown Album"));

        // Build filename: "01 - Title.ext" or "1-01 - Title.ext" for multi-disc
        let track_prefix = match (disc_num, track_num) {
            (Some(d), Some(n)) if *d > 1 => format!("{}-{:02}", d, n),
            (_, Some(n)) => format!("{:02}", n),
            _ => String::new(),
        };
        let safe_title = sanitize_filename(title);
        let filename = if track_prefix.is_empty() {
            // No track number — disambiguate by track id so two tracks that
            // sanitize to the same "Title.ext" don't collide (the second was
            // silently skipped as "already exists").
            format!("{} [{}].{}", safe_title, track_id, ext)
        } else {
            format!("{} - {}.{}", track_prefix, safe_title, ext)
        };

        let target_dir = dest.join(&artist_dir).join(&album_dir);
        if let Err(e) = std::fs::create_dir_all(&target_dir) {
            log::warn!("Failed to create dir {:?}: {}", target_dir, e);
            failed += 1;
            continue;
        }

        let target_file = target_dir.join(&filename);
        if target_file.exists() {
            skipped += 1;
            continue;
        }

        match std::fs::copy(src, &target_file) {
            Ok(_) => exported += 1,
            Err(e) => {
                log::warn!("Failed to copy {:?} -> {:?}: {}", src, target_file, e);
                failed += 1;
            }
        }
    }

    log::info!("Export complete: {} exported, {} skipped, {} failed to {}", exported, skipped, failed, destination);
    Ok(ExportResult {
        exported,
        skipped,
        failed,
        destination,
    })
}
