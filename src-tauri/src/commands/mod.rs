pub mod player;

use std::sync::Arc;
use rusqlite::params;
use tauri::{Manager, State};
use walkdir::WalkDir;

use crate::db::DbPool;
use crate::db::models::*;
use crate::download::DownloadManager;
use crate::metadata::tags;

// --- Library Stats ---

#[tauri::command]
pub fn greet(name: &str) -> String {
    format!("Hello, {}! Welcome to Playlist.", name)
}

#[tauri::command]
pub fn get_library_stats(db: State<'_, Arc<DbPool>>) -> Result<LibraryStats, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

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
) -> Result<TrackPage, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::tracks::get_tracks(&conn, offset, limit, &sort_by, &sort_dir)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_track(db: State<'_, Arc<DbPool>>, id: i64) -> Result<Option<Track>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::tracks::get_track(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_delete_track(
    db: State<'_, Arc<DbPool>>,
    id: i64,
    delete_file: bool,
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let file_path = crate::db::tracks::delete_track(&conn, id, delete_file)
        .map_err(|e| e.to_string())?;

    if let Some(path) = file_path {
        let _ = std::fs::remove_file(path);
    }
    Ok(())
}

// --- Albums ---

#[tauri::command]
pub fn library_get_albums(
    db: State<'_, Arc<DbPool>>,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Album>, i64), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::albums::get_albums(&conn, offset, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_album(db: State<'_, Arc<DbPool>>, id: i64) -> Result<Option<Album>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::albums::get_album(&conn, id).map_err(|e| e.to_string())
}

// --- Artists ---

#[tauri::command]
pub fn library_get_artists(
    db: State<'_, Arc<DbPool>>,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Artist>, i64), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::artists::get_artists(&conn, offset, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_artist(db: State<'_, Arc<DbPool>>, id: i64) -> Result<Option<Artist>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::artists::get_artist(&conn, id).map_err(|e| e.to_string())
}

// --- Playlists ---

#[tauri::command]
pub fn library_get_playlists(db: State<'_, Arc<DbPool>>) -> Result<Vec<Playlist>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::playlists::get_playlists(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_playlist(
    db: State<'_, Arc<DbPool>>,
    id: i64,
) -> Result<Option<PlaylistDetail>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let playlist = crate::db::playlists::get_playlist(&conn, id).map_err(|e| e.to_string())?;
    match playlist {
        Some(p) => {
            let tracks = crate::db::playlists::get_playlist_tracks(&conn, id)
                .map_err(|e| e.to_string())?;
            Ok(Some(PlaylistDetail { playlist: p, tracks }))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub fn library_create_playlist(
    db: State<'_, Arc<DbPool>>,
    name: String,
    description: Option<String>,
) -> Result<Playlist, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
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
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::playlists::update_playlist(&conn, id, name.as_deref(), description.as_deref())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_delete_playlist(db: State<'_, Arc<DbPool>>, id: i64) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::playlists::delete_playlist(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_add_to_playlist(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
    track_ids: Vec<i64>,
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
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
    let conn = db.lock().map_err(|e| e.to_string())?;
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
    let conn = db.lock().map_err(|e| e.to_string())?;
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
    let conn = db.lock().map_err(|e| e.to_string())?;
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
                track_count: row.get(6)?,
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
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::tracks::get_tracks_by_album(&conn, album_id).map_err(|e| e.to_string())
}

// --- Artist Tracks & Albums ---

#[tauri::command]
pub fn library_get_artist_tracks(
    db: State<'_, Arc<DbPool>>,
    artist_id: i64,
) -> Result<Vec<Track>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::tracks::get_tracks_by_artist(&conn, artist_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_artist_albums(
    db: State<'_, Arc<DbPool>>,
    artist_id: i64,
) -> Result<Vec<Album>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::albums::get_albums_by_artist(&conn, artist_id).map_err(|e| e.to_string())
}

// --- Settings ---

#[tauri::command]
pub fn settings_get(
    db: State<'_, Arc<DbPool>>,
    key: String,
) -> Result<Option<String>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::settings::get_setting(&conn, &key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_set(
    db: State<'_, Arc<DbPool>>,
    key: String,
    value: String,
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::settings::set_setting(&conn, &key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_get_all(
    db: State<'_, Arc<DbPool>>,
) -> Result<Vec<(String, String)>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::settings::get_all_settings(&conn).map_err(|e| e.to_string())
}

// --- Import ---

#[tauri::command]
pub fn library_import_folder(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    path: String,
) -> Result<i64, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

    let covers_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e: tauri::Error| e.to_string())?
        .join("covers");

    let mut imported = 0i64;

    for entry in WalkDir::new(&path).follow_links(true).into_iter().filter_map(|e| e.ok()) {
        let file_path = entry.path();
        if !file_path.is_file() || !tags::is_audio_file(file_path) {
            continue;
        }

        // Skip if already in library
        let path_str = file_path.to_string_lossy().to_string();
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) > 0 FROM tracks WHERE file_path = ?1",
                params![path_str],
                |row| row.get(0),
            )
            .unwrap_or(false);

        if exists {
            continue;
        }

        // Read tags
        let tag_data = match tags::read_tags(file_path) {
            Ok(d) => d,
            Err(e) => {
                log::warn!("Failed to read tags from {:?}: {}", file_path, e);
                continue;
            }
        };

        // Extract cover art
        let cover_art_path = tags::extract_cover_art(file_path, &covers_dir).unwrap_or(None);

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
                let track_id = conn.last_insert_rowid();
                let _ = crate::db::tracks::update_fts(&conn, track_id);
                imported += 1;
            }
            Err(e) => {
                log::warn!("Failed to insert track {:?}: {}", file_path, e);
            }
        }
    }

    log::info!("Imported {} tracks from {}", imported, path);
    Ok(imported)
}

// --- Downloads ---

#[tauri::command]
pub fn download_parse_url(url: String) -> UrlInfo {
    let parsed = crate::download::url_parser::parse_url(&url);
    UrlInfo {
        platform: parsed.platform,
        url_type: parsed.url_type,
        clean_url: parsed.clean_url,
        title: None,
    }
}

#[tauri::command]
pub async fn download_check_deps(
    app_handle: tauri::AppHandle,
) -> Result<crate::download::setup::DepsStatus, String> {
    let bin_dir = crate::download::setup::get_bin_dir(&app_handle);
    Ok(crate::download::setup::check_deps(&bin_dir).await)
}

#[tauri::command]
pub async fn download_ensure_deps(
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    let bin_dir = crate::download::setup::get_bin_dir(&app_handle);
    crate::download::setup::ensure_deps(&bin_dir, &app_handle).await
}

#[tauri::command]
pub fn download_start(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    url: String,
    format: Option<String>,
    quality: Option<String>,
) -> Result<Download, String> {
    let parsed = crate::download::url_parser::parse_url(&url);
    let fmt = format.unwrap_or_else(|| "opus".to_string());
    let qual = quality.unwrap_or_else(|| "best".to_string());

    let download = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::downloads::create_download(
            &conn,
            &parsed.clean_url,
            None,
            None,
            &parsed.platform,
            &fmt,
            &qual,
        )
        .map_err(|e| e.to_string())?
    };

    manager.start_download(download.id);
    Ok(download)
}

#[tauri::command]
pub async fn download_start_batch(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    urls: Vec<String>,
    format: Option<String>,
    quality: Option<String>,
) -> Result<Vec<Download>, String> {
    let fmt = format.unwrap_or_else(|| "opus".to_string());
    let qual = quality.unwrap_or_else(|| "best".to_string());
    let mut downloads = Vec::new();

    for url in &urls {
        let parsed = crate::download::url_parser::parse_url(url);
        let download = {
            let conn = db.lock().map_err(|e| e.to_string())?;
            crate::db::downloads::create_download(
                &conn,
                &parsed.clean_url,
                None,
                None,
                &parsed.platform,
                &fmt,
                &qual,
            )
            .map_err(|e| e.to_string())?
        };
        manager.start_download(download.id);
        downloads.push(download);
    }

    Ok(downloads)
}

#[tauri::command]
pub async fn download_cancel(
    manager: State<'_, Arc<DownloadManager>>,
    id: i64,
) -> Result<(), String> {
    manager.cancel_download(id).await;
    Ok(())
}

#[tauri::command]
pub fn download_retry(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    id: i64,
) -> Result<Download, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::downloads::reset_download_for_retry(&conn, id).map_err(|e| e.to_string())?;
    let download = crate::db::downloads::get_download(&conn, id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Download not found".to_string())?;
    drop(conn);
    manager.start_download(id);
    Ok(download)
}

#[tauri::command]
pub fn download_get_active(db: State<'_, Arc<DbPool>>) -> Result<Vec<Download>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::downloads::get_active_downloads(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn download_get_history(
    db: State<'_, Arc<DbPool>>,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Download>, i64), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::downloads::get_download_history(&conn, offset, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn download_clear_history(db: State<'_, Arc<DbPool>>) -> Result<i64, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::downloads::clear_completed(&conn).map_err(|e| e.to_string())
}
