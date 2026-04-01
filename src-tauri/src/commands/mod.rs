pub mod player;

use std::sync::Arc;
use rusqlite::params;
use tauri::{Emitter, Manager, State};
use walkdir::WalkDir;

use crate::db::DbPool;
use crate::db::models::*;
use crate::db::monitored::{MonitoredPlaylist, MonitoredEntry};
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
    search: Option<String>,
) -> Result<TrackPage, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::tracks::get_tracks(&conn, offset, limit, &sort_by, &sort_dir, search.as_deref())
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

    let conn = db.lock().map_err(|e| e.to_string())?;

    // Optionally delete downloaded files
    if delete_files {
        let download_dir = {
            let dir = crate::db::settings::get_setting(&conn, "download_dir").ok().flatten();
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
            let _ = std::fs::remove_dir_all(&download_dir);
            let _ = std::fs::create_dir_all(&download_dir);
        }
    }

    // Clear all tables (order matters for foreign keys)
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
             tokenize='unicode61 remove_diacritics 2'
         );"
    ).map_err(|e| e.to_string())?;

    // Remove cover art
    if let Ok(covers_dir) = app_handle.path().app_data_dir().map(|d| d.join("covers")) {
        if covers_dir.exists() {
            let _ = std::fs::remove_dir_all(&covers_dir);
            let _ = std::fs::create_dir_all(&covers_dir);
        }
    }

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
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::albums::get_albums(&conn, offset, limit, search.as_deref()).map_err(|e| e.to_string())
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
    search: Option<String>,
) -> Result<(Vec<Artist>, i64), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::artists::get_artists(&conn, offset, limit, search.as_deref()).map_err(|e| e.to_string())
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
                label: None,
                release_date: None,
                description: None,
                album_type: None,
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

                // Propagate cover art to album and artist
                if let Some(ref cover) = cover_art_path {
                    if let Some(aid) = album_id {
                        let _ = crate::db::albums::update_cover_art_if_missing(&conn, aid, cover);
                    }
                    if let Some(aid) = artist_id {
                        let _ = crate::db::artists::update_image_if_missing(&conn, aid, cover);
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
pub async fn download_start(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    url: String,
    format: Option<String>,
    quality: Option<String>,
) -> Result<Download, String> {
    let parsed = crate::download::url_parser::parse_url(&url);
    let default_format = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::settings::get_setting(&conn, "download_format")
            .ok().flatten()
            .unwrap_or_else(|| "mp3".to_string())
    };
    let fmt = format.unwrap_or(default_format);
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
    let default_format = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::settings::get_setting(&conn, "download_format")
            .ok().flatten()
            .unwrap_or_else(|| "mp3".to_string())
    };
    let fmt = format.unwrap_or(default_format);
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
pub async fn download_retry(
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

// --- Manager (Monitored Playlists) ---

#[tauri::command]
pub fn manager_get_playlists(db: State<'_, Arc<DbPool>>) -> Result<Vec<MonitoredPlaylist>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::monitored::get_monitored_playlists(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn manager_get_entries(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
) -> Result<Vec<MonitoredEntry>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::monitored::get_entries(&conn, playlist_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn manager_get_new_entries(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
) -> Result<Vec<MonitoredEntry>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::monitored::get_new_entries(&conn, playlist_id).map_err(|e| e.to_string())
}

/// Add a playlist URL to monitor. Fetches metadata via yt-dlp and stores entries.
#[tauri::command]
pub async fn manager_add_playlist(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    url: String,
) -> Result<MonitoredPlaylist, String> {
    let parsed = crate::download::url_parser::parse_url(&url);

    // Resolve yt-dlp binary
    let bin_dir = crate::download::setup::get_bin_dir(&app_handle);
    let ytdlp_binary = crate::download::setup::resolve_ytdlp(&bin_dir)
        .unwrap_or_else(|| "yt-dlp".to_string());
    let ffmpeg_dir = crate::download::setup::resolve_ffmpeg_dir(&bin_dir);
    let cookies_from_browser = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::settings::get_setting(&conn, "cookies_from_browser")
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
    };

    // Fetch playlist info
    let fetch_result = crate::download::ytdlp::get_playlist_entries(
        &ytdlp_binary,
        ffmpeg_dir.as_deref(),
        &url,
        cookies_from_browser.as_deref(),
    )
    .await?;

    let entries = fetch_result.entries;

    if entries.is_empty() {
        return Err("No entries found. Make sure this is a valid playlist URL.".to_string());
    }

    // Use the actual playlist title from yt-dlp, fall back to platform name
    let playlist_name = fetch_result
        .playlist_title
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| format!("{} playlist", parsed.platform));

    // Create playlist in DB
    let playlist = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::monitored::create_monitored_playlist(
            &conn,
            &playlist_name,
            &parsed.platform,
            &parsed.clean_url,
            None,
        )
        .map_err(|e| e.to_string())?
    };

    // Store entries — prefer music-specific fields (track > title, artist > uploader)
    let entry_data: Vec<_> = entries
        .iter()
        .map(|e| {
            let best_title = e.track.clone().unwrap_or_else(|| e.title.clone());
            let best_artist = e.artist.clone().or_else(|| e.uploader.clone());
            (
                e.webpage_url.clone().unwrap_or_default(),
                Some(best_title),
                best_artist,
                e.duration,
                e.thumbnail.clone(),
            )
        })
        .filter(|(url, _, _, _, _)| !url.is_empty())
        .collect();

    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::monitored::upsert_entries(&conn, playlist.id, &entry_data)
            .map_err(|e| e.to_string())?;
    }

    // Return the full monitored playlist with counts
    let conn = db.lock().map_err(|e| e.to_string())?;
    let playlists = crate::db::monitored::get_monitored_playlists(&conn)
        .map_err(|e| e.to_string())?;
    playlists
        .into_iter()
        .find(|p| p.id == playlist.id)
        .ok_or_else(|| "Failed to retrieve created playlist".to_string())
}

/// Sync a monitored playlist - fetch latest entries and detect new ones
#[tauri::command]
pub async fn manager_sync_playlist(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    playlist_id: i64,
) -> Result<SyncResult, String> {
    // Get the playlist source URL
    let source_url = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let playlist = crate::db::playlists::get_playlist(&conn, playlist_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Playlist not found".to_string())?;
        playlist
            .source_url
            .ok_or_else(|| "Playlist has no source URL".to_string())?
    };

    // Resolve yt-dlp binary
    let bin_dir = crate::download::setup::get_bin_dir(&app_handle);
    let ytdlp_binary = crate::download::setup::resolve_ytdlp(&bin_dir)
        .unwrap_or_else(|| "yt-dlp".to_string());
    let ffmpeg_dir = crate::download::setup::resolve_ffmpeg_dir(&bin_dir);
    let cookies_from_browser = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::settings::get_setting(&conn, "cookies_from_browser")
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
    };

    // Fetch current entries
    let fetch_result = crate::download::ytdlp::get_playlist_entries(
        &ytdlp_binary,
        ffmpeg_dir.as_deref(),
        &source_url,
        cookies_from_browser.as_deref(),
    )
    .await?;

    let entry_data: Vec<_> = fetch_result.entries
        .iter()
        .map(|e| {
            let best_title = e.track.clone().unwrap_or_else(|| e.title.clone());
            let best_artist = e.artist.clone().or_else(|| e.uploader.clone());
            (
                e.webpage_url.clone().unwrap_or_default(),
                Some(best_title),
                best_artist,
                e.duration,
                e.thumbnail.clone(),
            )
        })
        .filter(|(url, _, _, _, _)| !url.is_empty())
        .collect();

    let (new_count, total_count) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::monitored::upsert_entries(&conn, playlist_id, &entry_data)
            .map_err(|e| e.to_string())?
    };

    Ok(SyncResult {
        playlist_id,
        new_count,
        total_count,
    })
}

/// Download a single entry from a monitored playlist
#[tauri::command]
pub async fn manager_download_entry(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    entry_id: i64,
    format: Option<String>,
    quality: Option<String>,
) -> Result<Download, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

    let entry = crate::db::monitored::get_entry(&conn, entry_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Entry not found".to_string())?;

    let parsed = crate::download::url_parser::parse_url(&entry.source_url);
    let default_format = crate::db::settings::get_setting(&conn, "download_format")
        .ok().flatten()
        .unwrap_or_else(|| "mp3".to_string());
    let fmt = format.unwrap_or(default_format);
    let qual = quality.unwrap_or_else(|| "best".to_string());

    let download = crate::db::downloads::create_download(
        &conn,
        &parsed.clean_url,
        entry.title.as_deref(),
        entry.artist.as_deref(),
        &parsed.platform,
        &fmt,
        &qual,
    )
    .map_err(|e| e.to_string())?;

    // Update entry status to queued with download_id
    crate::db::monitored::update_entry_status(&conn, entry_id, "queued", Some(download.id), None)
        .map_err(|e| e.to_string())?;

    drop(conn);
    manager.start_download(download.id);
    Ok(download)
}

/// Download all new entries from a monitored playlist.
/// No batch limit — concurrency is controlled by the DownloadManager semaphore.
#[tauri::command]
pub async fn manager_download_new(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    playlist_id: i64,
    format: Option<String>,
    quality: Option<String>,
) -> Result<BatchDownloadResult, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

    let entries = crate::db::monitored::get_new_entries(&conn, playlist_id)
        .map_err(|e| e.to_string())?;

    let default_format = crate::db::settings::get_setting(&conn, "download_format")
        .ok().flatten()
        .unwrap_or_else(|| "mp3".to_string());
    let fmt = format.unwrap_or(default_format);
    let qual = quality.unwrap_or_else(|| "best".to_string());

    // Use a transaction for bulk inserts (fast even for thousands of entries)
    conn.execute_batch("BEGIN IMMEDIATE").map_err(|e| e.to_string())?;

    let result: Result<Vec<i64>, String> = (|| {
        let mut download_ids = Vec::new();
        for entry in &entries {
            let parsed = crate::download::url_parser::parse_url(&entry.source_url);
            let download = crate::db::downloads::create_download(
                &conn,
                &parsed.clean_url,
                entry.title.as_deref(),
                entry.artist.as_deref(),
                &parsed.platform,
                &fmt,
                &qual,
            )
            .map_err(|e| e.to_string())?;

            crate::db::monitored::update_entry_status(
                &conn,
                entry.id,
                "queued",
                Some(download.id),
                None,
            )
            .map_err(|e| e.to_string())?;

            download_ids.push(download.id);
        }
        Ok(download_ids)
    })();

    match result {
        Ok(download_ids) => {
            conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
            let queued = download_ids.len() as i64;
            drop(conn);
            // Start all downloads (concurrency semaphore limits parallel yt-dlp processes)
            for id in download_ids {
                manager.start_download(id);
            }
            Ok(BatchDownloadResult { queued, playlist_id })
        }
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Skip an entry (mark as skipped so it won't show as "new")
#[tauri::command]
pub fn manager_skip_entry(
    db: State<'_, Arc<DbPool>>,
    entry_id: i64,
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::monitored::skip_entry(&conn, entry_id).map_err(|e| e.to_string())
}

/// Cancel a single entry's download and reset to "new"
#[tauri::command]
pub async fn manager_cancel_entry(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    entry_id: i64,
) -> Result<(), String> {
    let download_id = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let entry = crate::db::monitored::get_entry(&conn, entry_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Entry not found".to_string())?;
        entry.download_id
    };

    // Cancel the download if one exists
    if let Some(did) = download_id {
        manager.cancel_download(did).await;
    }

    // Reset entry back to "new"
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::monitored::update_entry_status(&conn, entry_id, "new", None, None)
        .map_err(|e| e.to_string())
}

/// Cancel all queued/downloading entries for a playlist
#[tauri::command]
pub async fn manager_cancel_all(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    playlist_id: i64,
) -> Result<i64, String> {
    let entries: Vec<(i64, Option<i64>)> = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT id, download_id FROM monitored_playlist_entries
             WHERE playlist_id = ?1 AND status IN ('queued', 'downloading')"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(params![playlist_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, Option<i64>>(1)?))
        }).map_err(|e| e.to_string())?;
        let result: Vec<_> = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        result
    };

    let count = entries.len() as i64;

    // Cancel all active downloads
    for (_, download_id) in &entries {
        if let Some(did) = download_id {
            manager.cancel_download(*did).await;
        }
    }

    // Reset all entries to "new"
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        for (entry_id, _) in &entries {
            let _ = crate::db::monitored::update_entry_status(&conn, *entry_id, "new", None, None);
        }
    }

    Ok(count)
}

/// Remove a monitored playlist, cancelling any active downloads first
#[tauri::command]
pub async fn manager_remove_playlist(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<crate::download::DownloadManager>>,
    playlist_id: i64,
) -> Result<(), String> {
    // Collect download IDs in a separate scope so the MutexGuard is dropped before .await
    let download_ids: Vec<i64> = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT download_id FROM monitored_playlist_entries
             WHERE playlist_id = ?1 AND status IN ('queued', 'downloading') AND download_id IS NOT NULL"
        ).map_err(|e| e.to_string())?;
        let rows: Vec<i64> = stmt.query_map(rusqlite::params![playlist_id], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        rows
    };
    for id in download_ids {
        manager.cancel_download(id).await;
    }
    let conn = db.lock().map_err(|e| e.to_string())?;
    crate::db::monitored::delete_monitored_playlist(&conn, playlist_id)
        .map_err(|e| e.to_string())
}

// --- Metadata Enrichment ---

#[derive(Debug, serde::Serialize)]
pub struct EnrichResult {
    pub track_id: i64,
    pub fields_updated: i64,
    pub completeness: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ScanMissingResult {
    pub total_tracks: i64,
    pub enriched: i64,
    pub failed: i64,
    pub completeness_avg: i64,
}

/// Enrich a single track's metadata from MusicBrainz
#[tauri::command]
pub async fn enrich_track(
    db: State<'_, Arc<DbPool>>,
    track_id: i64,
) -> Result<EnrichResult, String> {
    // Get track info for MusicBrainz search
    let (title, artist_name) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let track = crate::db::tracks::get_track(&conn, track_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Track not found".to_string())?;
        (track.title, track.artist_name)
    };

    let enrichment = crate::metadata::musicbrainz::enrich_track(&title, artist_name.as_deref()).await?;

    // Apply enrichment to DB (only fill missing fields)
    let conn = db.lock().map_err(|e| e.to_string())?;
    let mut updated = 0i64;

    macro_rules! update_if_missing {
        ($col:expr, $val:expr) => {
            if let Some(ref v) = $val {
                let changed = conn.execute(
                    &format!("UPDATE tracks SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                    rusqlite::params![v, track_id],
                ).unwrap_or(0);
                updated += changed as i64;
            }
        };
    }

    update_if_missing!("musicbrainz_id", enrichment.musicbrainz_id);
    update_if_missing!("genre", enrichment.genre);
    update_if_missing!("release_date", enrichment.release_date);
    update_if_missing!("isrc", enrichment.isrc);
    update_if_missing!("description", enrichment.description);
    update_if_missing!("label", enrichment.label);
    update_if_missing!("language", enrichment.language);

    // Update artist info if we have MusicBrainz data
    if let Some(ref mb_artist_id) = enrichment.artist_musicbrainz_id {
        // Get the track's artist_id
        let artist_id: Option<i64> = conn.query_row(
            "SELECT artist_id FROM tracks WHERE id = ?1",
            rusqlite::params![track_id],
            |row| row.get(0),
        ).ok();
        if let Some(aid) = artist_id {
            let _ = conn.execute(
                "UPDATE artists SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL",
                rusqlite::params![mb_artist_id, aid],
            );
            if let Some(ref sn) = enrichment.artist_sort_name {
                let _ = conn.execute(
                    "UPDATE artists SET sort_name = ?1 WHERE id = ?2 AND sort_name IS NULL",
                    rusqlite::params![sn, aid],
                );
            }
            if let Some(ref at) = enrichment.artist_type {
                let _ = conn.execute(
                    "UPDATE artists SET artist_type = ?1 WHERE id = ?2 AND artist_type IS NULL",
                    rusqlite::params![at, aid],
                );
            }
            if let Some(ref c) = enrichment.artist_country {
                let _ = conn.execute(
                    "UPDATE artists SET country = ?1 WHERE id = ?2 AND country IS NULL",
                    rusqlite::params![c, aid],
                );
            }
            if let Some(by) = enrichment.artist_begin_year {
                let _ = conn.execute(
                    "UPDATE artists SET begin_year = ?1 WHERE id = ?2 AND begin_year IS NULL",
                    rusqlite::params![by, aid],
                );
            }
        }
    }

    // Update album info
    if let Some(ref mb_album_id) = enrichment.album_musicbrainz_id {
        let album_id: Option<i64> = conn.query_row(
            "SELECT album_id FROM tracks WHERE id = ?1",
            rusqlite::params![track_id],
            |row| row.get(0),
        ).ok().flatten();
        if let Some(aid) = album_id {
            let _ = conn.execute(
                "UPDATE albums SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL",
                rusqlite::params![mb_album_id, aid],
            );
            if let Some(ref rd) = enrichment.album_release_date {
                let _ = conn.execute(
                    "UPDATE albums SET release_date = ?1 WHERE id = ?2 AND release_date IS NULL",
                    rusqlite::params![rd, aid],
                );
            }
            if let Some(ref at) = enrichment.album_type {
                let _ = conn.execute(
                    "UPDATE albums SET album_type = ?1 WHERE id = ?2 AND album_type IS NULL",
                    rusqlite::params![at, aid],
                );
            }
        }
    }

    let completeness = crate::db::tracks::update_completeness(&conn, track_id)
        .map_err(|e| e.to_string())?;

    Ok(EnrichResult { track_id, fields_updated: updated, completeness })
}

/// Enrich an album's metadata from MusicBrainz + Last.fm, including tracklist and cover art
#[derive(Debug, serde::Serialize)]
pub struct EnrichAlbumResult {
    pub album_id: i64,
    pub fields_updated: i64,
    pub tracks_added: i64,
    pub tracklist: Vec<crate::metadata::musicbrainz::AlbumTrackInfo>,
}

#[tauri::command]
pub async fn enrich_album(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    album_id: i64,
) -> Result<EnrichAlbumResult, String> {
    // Get album info for search
    let (title, artist_name, existing_cover, existing_description, existing_genre) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let album = crate::db::albums::get_album(&conn, album_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Album not found".to_string())?;
        (album.title, album.artist_name, album.cover_art_path, album.description, album.genre)
    };

    // Fetch MusicBrainz data
    let enrichment = crate::metadata::musicbrainz::enrich_album(&title, artist_name.as_deref()).await?;

    // Fetch Last.fm data in parallel (don't fail if it errors)
    let lastfm_data = if let Some(ref artist) = artist_name {
        crate::metadata::lastfm::get_album_info(&title, artist).await.ok()
    } else {
        None
    };

    // Fetch artist data from Last.fm for bio/image
    let lastfm_artist = if let Some(ref artist) = artist_name {
        crate::metadata::lastfm::get_artist_info(artist).await.ok()
    } else {
        None
    };

    // Apply all DB updates in a block so conn is dropped before async cover art download
    let (mut updated, artist_id) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let mut updated = 0i64;

        macro_rules! update_album_if_missing {
            ($col:expr, $val:expr) => {
                if let Some(ref v) = $val {
                    let changed = conn.execute(
                        &format!("UPDATE albums SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                        rusqlite::params![v, album_id],
                    ).unwrap_or(0);
                    updated += changed as i64;
                }
            };
        }

        update_album_if_missing!("musicbrainz_id", enrichment.musicbrainz_id);
        update_album_if_missing!("release_date", enrichment.release_date);
        update_album_if_missing!("label", enrichment.label);
        update_album_if_missing!("album_type", enrichment.album_type);

        // Genre: prefer Last.fm tags (joined), fallback to MusicBrainz
        if existing_genre.is_none() {
            let genre = lastfm_data.as_ref()
                .filter(|d| !d.tags.is_empty())
                .map(|d| d.tags.join(", "))
                .or(enrichment.genre.clone());
            update_album_if_missing!("genre", genre);
        }

        // Description: prefer Last.fm wiki
        if existing_description.is_none() {
            let desc = lastfm_data.as_ref()
                .and_then(|d| d.description.clone());
            update_album_if_missing!("description", desc);
        }

        if let Some(tt) = enrichment.total_tracks {
            let changed = conn.execute(
                "UPDATE albums SET total_tracks = ?1 WHERE id = ?2 AND total_tracks IS NULL",
                rusqlite::params![tt, album_id],
            ).unwrap_or(0);
            updated += changed as i64;
        }
        if let Some(td) = enrichment.total_discs {
            let changed = conn.execute(
                "UPDATE albums SET total_discs = ?1 WHERE id = ?2 AND total_discs IS NULL",
                rusqlite::params![td, album_id],
            ).unwrap_or(0);
            updated += changed as i64;
        }

        // Update artist info
        let artist_id: Option<i64> = conn.query_row(
            "SELECT artist_id FROM albums WHERE id = ?1",
            rusqlite::params![album_id],
            |row| row.get(0),
        ).ok().flatten();

        if let Some(aid) = artist_id {
            if let Some(ref mb_artist_id) = enrichment.artist_musicbrainz_id {
                let _ = conn.execute("UPDATE artists SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL", rusqlite::params![mb_artist_id, aid]);
                if let Some(ref v) = enrichment.artist_sort_name { let _ = conn.execute("UPDATE artists SET sort_name = ?1 WHERE id = ?2 AND sort_name IS NULL", rusqlite::params![v, aid]); }
                if let Some(ref v) = enrichment.artist_type { let _ = conn.execute("UPDATE artists SET artist_type = ?1 WHERE id = ?2 AND artist_type IS NULL", rusqlite::params![v, aid]); }
                if let Some(ref v) = enrichment.artist_country { let _ = conn.execute("UPDATE artists SET country = ?1 WHERE id = ?2 AND country IS NULL", rusqlite::params![v, aid]); }
                if let Some(v) = enrichment.artist_begin_year { let _ = conn.execute("UPDATE artists SET begin_year = ?1 WHERE id = ?2 AND begin_year IS NULL", rusqlite::params![v, aid]); }
            }
            // Artist bio from Last.fm
            if let Some(ref lfm_artist) = lastfm_artist {
                if let Some(ref bio) = lfm_artist.bio {
                    let _ = conn.execute("UPDATE artists SET bio = ?1 WHERE id = ?2 AND (bio IS NULL OR bio = '')", rusqlite::params![bio, aid]);
                }
            }
        }

        (updated, artist_id)
    }; // conn dropped here

    // Download cover art if album has no cover
    if existing_cover.is_none() {
        let covers_dir = app_handle
            .path()
            .app_data_dir()
            .map(|d| d.join("covers"))
            .ok();

        if let Some(covers_dir) = covers_dir {
            let _ = std::fs::create_dir_all(&covers_dir);
            let mut cover_bytes: Option<Vec<u8>> = None;

            // Try Cover Art Archive first (highest quality)
            if let Some(ref mbid) = enrichment.musicbrainz_id {
                cover_bytes = crate::metadata::musicbrainz::download_cover_art(mbid).await;
            }

            // Fallback to Last.fm image
            if cover_bytes.is_none() {
                if let Some(ref url) = lastfm_data.as_ref().and_then(|d| d.image_url.clone()) {
                    cover_bytes = crate::metadata::lastfm::download_image(url).await;
                }
            }

            if let Some(bytes) = cover_bytes {
                let filename = format!("album_{}.jpg", album_id);
                let path = covers_dir.join(&filename);
                if std::fs::write(&path, &bytes).is_ok() {
                    let path_str = path.to_string_lossy().to_string();
                    if let Ok(conn) = db.lock() {
                        let _ = conn.execute(
                            "UPDATE albums SET cover_art_path = ?1 WHERE id = ?2",
                            rusqlite::params![path_str, album_id],
                        );
                        // Also update tracks that belong to this album and have no cover
                        let _ = conn.execute(
                            "UPDATE tracks SET cover_art_path = ?1 WHERE album_id = ?2 AND cover_art_path IS NULL",
                            rusqlite::params![path_str, album_id],
                        );
                        updated += 1;
                    }
                }
            }
        }

        // Download artist image if missing
        if let Some(aid) = artist_id {
            let artist_has_image: bool = db.lock().ok()
                .and_then(|conn| conn.query_row(
                    "SELECT image_path IS NOT NULL FROM artists WHERE id = ?1",
                    rusqlite::params![aid],
                    |row| row.get::<_, bool>(0),
                ).ok())
                .unwrap_or(true);

            if !artist_has_image {
                let mut artist_img_bytes: Option<Vec<u8>> = None;
                if let Some(ref lfm_artist) = lastfm_artist {
                    if let Some(ref url) = lfm_artist.image_url {
                        artist_img_bytes = crate::metadata::lastfm::download_image(url).await;
                    }
                }
                if let Some(bytes) = artist_img_bytes {
                    if let Some(covers_dir) = app_handle.path().app_data_dir().map(|d| d.join("covers")).ok() {
                        let filename = format!("artist_{}.jpg", aid);
                        let path = covers_dir.join(&filename);
                        if std::fs::write(&path, &bytes).is_ok() {
                            let path_str = path.to_string_lossy().to_string();
                            if let Ok(conn) = db.lock() {
                                let _ = conn.execute(
                                    "UPDATE artists SET image_path = ?1 WHERE id = ?2 AND image_path IS NULL",
                                    rusqlite::params![path_str, aid],
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let tracklist = enrichment.tracklist.clone();
    let tracks_added = tracklist.len() as i64;

    Ok(EnrichAlbumResult { album_id, fields_updated: updated, tracks_added, tracklist })
}

/// Scan all tracks with low metadata completeness and enrich them
#[tauri::command]
pub async fn scan_missing_metadata(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
) -> Result<ScanMissingResult, String> {
    // First, recompute completeness for all tracks that still have 0
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT id FROM tracks WHERE metadata_completeness = 0"
        ).map_err(|e| e.to_string())?;
        let ids: Vec<i64> = stmt.query_map([], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        drop(stmt);
        for id in &ids {
            let _ = crate::db::tracks::update_completeness(&conn, *id);
        }
    }

    // Get tracks that need enrichment (completeness < 70)
    let tracks_to_enrich: Vec<(i64, String, Option<String>)> = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT t.id, t.title, a.name
             FROM tracks t
             LEFT JOIN artists a ON t.artist_id = a.id
             WHERE t.metadata_completeness < 70
             ORDER BY t.metadata_completeness ASC
             LIMIT 50"
        ).map_err(|e| e.to_string())?;
        let result: Vec<_> = stmt.query_map([], |row| Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        )))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
        result
    };

    let total = tracks_to_enrich.len() as i64;
    let mut enriched = 0i64;
    let mut failed = 0i64;

    for (i, (track_id, title, artist_name)) in tracks_to_enrich.iter().enumerate() {
        // Rate limit: 1 request per second for MusicBrainz
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        }

        // Emit progress event
        let _ = app_handle.emit(
            "metadata-scan-progress",
            serde_json::json!({
                "current": i + 1,
                "total": total,
                "track_title": title,
            }),
        );

        match crate::metadata::musicbrainz::enrich_track(title, artist_name.as_deref()).await {
            Ok(enrichment) => {
                if let Ok(conn) = db.lock() {
                    // Apply enrichment (same logic as enrich_track command but inlined)
                    macro_rules! update_if_missing {
                        ($col:expr, $val:expr) => {
                            if let Some(ref v) = $val {
                                let _ = conn.execute(
                                    &format!("UPDATE tracks SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                                    rusqlite::params![v, track_id],
                                );
                            }
                        };
                    }
                    update_if_missing!("musicbrainz_id", enrichment.musicbrainz_id);
                    update_if_missing!("genre", enrichment.genre);
                    update_if_missing!("release_date", enrichment.release_date);
                    update_if_missing!("isrc", enrichment.isrc);
                    update_if_missing!("description", enrichment.description);
                    update_if_missing!("label", enrichment.label);
                    update_if_missing!("language", enrichment.language);

                    // Update artist
                    if let Some(ref mb_id) = enrichment.artist_musicbrainz_id {
                        let artist_id: Option<i64> = conn.query_row(
                            "SELECT artist_id FROM tracks WHERE id = ?1",
                            rusqlite::params![track_id],
                            |row| row.get(0),
                        ).ok();
                        if let Some(aid) = artist_id {
                            let _ = conn.execute("UPDATE artists SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL", rusqlite::params![mb_id, aid]);
                            if let Some(ref v) = enrichment.artist_sort_name { let _ = conn.execute("UPDATE artists SET sort_name = ?1 WHERE id = ?2 AND sort_name IS NULL", rusqlite::params![v, aid]); }
                            if let Some(ref v) = enrichment.artist_type { let _ = conn.execute("UPDATE artists SET artist_type = ?1 WHERE id = ?2 AND artist_type IS NULL", rusqlite::params![v, aid]); }
                            if let Some(ref v) = enrichment.artist_country { let _ = conn.execute("UPDATE artists SET country = ?1 WHERE id = ?2 AND country IS NULL", rusqlite::params![v, aid]); }
                            if let Some(v) = enrichment.artist_begin_year { let _ = conn.execute("UPDATE artists SET begin_year = ?1 WHERE id = ?2 AND begin_year IS NULL", rusqlite::params![v, aid]); }
                        }
                    }

                    let _ = crate::db::tracks::update_completeness(&conn, *track_id);
                    enriched += 1;
                }
            }
            Err(e) => {
                log::warn!("Failed to enrich track {} ({}): {}", track_id, title, e);
                failed += 1;
            }
        }
    }

    // Compute average completeness
    let completeness_avg = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT COALESCE(AVG(metadata_completeness), 0) FROM tracks",
            [],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0)
    };

    // Emit completion event
    let _ = app_handle.emit("metadata-scan-complete", serde_json::json!({
        "enriched": enriched,
        "failed": failed,
        "completeness_avg": completeness_avg,
    }));

    Ok(ScanMissingResult { total_tracks: total, enriched, failed, completeness_avg })
}

/// Background auto-enrichment: enriches all albums and tracks with missing metadata.
/// Called once on app startup as a background task.
pub async fn auto_enrich_library(
    db: Arc<std::sync::Mutex<rusqlite::Connection>>,
    app_handle: tauri::AppHandle,
) {
    log::info!("Starting background auto-enrichment...");

    let covers_dir = match app_handle.path().app_data_dir().map(|d| d.join("covers")) {
        Ok(d) => { let _ = std::fs::create_dir_all(&d); d },
        Err(_) => return,
    };

    // 1. Enrich albums that have no musicbrainz_id
    let albums_to_enrich: Vec<(i64, String, Option<String>, Option<String>)> = {
        match db.lock() {
            Ok(conn) => {
                let mut stmt = match conn.prepare(
                    "SELECT al.id, al.title, a.name, al.cover_art_path
                     FROM albums al
                     LEFT JOIN artists a ON al.artist_id = a.id
                     WHERE al.musicbrainz_id IS NULL
                     LIMIT 100"
                ) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let result: Vec<_> = stmt.query_map([], |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                )))
                .unwrap_or_else(|_| panic!())
                .filter_map(|r| r.ok())
                .collect();
                result
            }
            Err(_) => return,
        }
    };

    let total_albums = albums_to_enrich.len();
    log::info!("Auto-enriching {} albums", total_albums);

    let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
        "phase": "albums",
        "current": 0,
        "total": total_albums,
    }));

    for (i, (album_id, title, artist_name, existing_cover)) in albums_to_enrich.iter().enumerate() {
        // Rate limit for MusicBrainz
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        }

        let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
            "phase": "albums",
            "current": i + 1,
            "total": total_albums,
            "title": title,
        }));

        // MusicBrainz album enrichment
        let enrichment = match crate::metadata::musicbrainz::enrich_album(&title, artist_name.as_deref()).await {
            Ok(e) => e,
            Err(e) => {
                log::warn!("Failed to enrich album '{}': {}", title, e);
                continue;
            }
        };

        // Last.fm album data
        let lastfm_data = if let Some(ref artist) = artist_name {
            crate::metadata::lastfm::get_album_info(&title, artist).await.ok()
        } else {
            None
        };

        if let Ok(conn) = db.lock() {
            macro_rules! update_album {
                ($col:expr, $val:expr) => {
                    if let Some(ref v) = $val {
                        let _ = conn.execute(
                            &format!("UPDATE albums SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                            rusqlite::params![v, album_id],
                        );
                    }
                };
            }

            update_album!("musicbrainz_id", enrichment.musicbrainz_id);
            update_album!("release_date", enrichment.release_date);
            update_album!("label", enrichment.label);
            update_album!("album_type", enrichment.album_type);

            // Genre: prefer Last.fm tags
            let genre = lastfm_data.as_ref()
                .filter(|d| !d.tags.is_empty())
                .map(|d| d.tags.join(", "))
                .or(enrichment.genre.clone());
            update_album!("genre", genre);

            // Description from Last.fm
            let desc = lastfm_data.as_ref().and_then(|d| d.description.clone());
            update_album!("description", desc);

            if let Some(tt) = enrichment.total_tracks {
                let _ = conn.execute("UPDATE albums SET total_tracks = ?1 WHERE id = ?2 AND total_tracks IS NULL", rusqlite::params![tt, album_id]);
            }
            if let Some(td) = enrichment.total_discs {
                let _ = conn.execute("UPDATE albums SET total_discs = ?1 WHERE id = ?2 AND total_discs IS NULL", rusqlite::params![td, album_id]);
            }

            // Artist enrichment
            let artist_id: Option<i64> = conn.query_row(
                "SELECT artist_id FROM albums WHERE id = ?1", rusqlite::params![album_id], |row| row.get(0),
            ).ok().flatten();
            if let Some(aid) = artist_id {
                if let Some(ref mb_id) = enrichment.artist_musicbrainz_id {
                    let _ = conn.execute("UPDATE artists SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL", rusqlite::params![mb_id, aid]);
                }
                if let Some(ref v) = enrichment.artist_sort_name { let _ = conn.execute("UPDATE artists SET sort_name = ?1 WHERE id = ?2 AND sort_name IS NULL", rusqlite::params![v, aid]); }
                if let Some(ref v) = enrichment.artist_type { let _ = conn.execute("UPDATE artists SET artist_type = ?1 WHERE id = ?2 AND artist_type IS NULL", rusqlite::params![v, aid]); }
                if let Some(ref v) = enrichment.artist_country { let _ = conn.execute("UPDATE artists SET country = ?1 WHERE id = ?2 AND country IS NULL", rusqlite::params![v, aid]); }
                if let Some(v) = enrichment.artist_begin_year { let _ = conn.execute("UPDATE artists SET begin_year = ?1 WHERE id = ?2 AND begin_year IS NULL", rusqlite::params![v, aid]); }
            }
        }

        // Download cover art if missing
        if existing_cover.is_none() {
            let mut cover_bytes: Option<Vec<u8>> = None;
            if let Some(ref mbid) = enrichment.musicbrainz_id {
                cover_bytes = crate::metadata::musicbrainz::download_cover_art(mbid).await;
            }
            if cover_bytes.is_none() {
                if let Some(ref url) = lastfm_data.as_ref().and_then(|d| d.image_url.clone()) {
                    cover_bytes = crate::metadata::lastfm::download_image(url).await;
                }
            }
            if let Some(bytes) = cover_bytes {
                let filename = format!("album_{}.jpg", album_id);
                let path = covers_dir.join(&filename);
                if std::fs::write(&path, &bytes).is_ok() {
                    let path_str = path.to_string_lossy().to_string();
                    if let Ok(conn) = db.lock() {
                        let _ = conn.execute("UPDATE albums SET cover_art_path = ?1 WHERE id = ?2", rusqlite::params![path_str, album_id]);
                        let _ = conn.execute("UPDATE tracks SET cover_art_path = ?1 WHERE album_id = ?2 AND cover_art_path IS NULL", rusqlite::params![path_str, album_id]);
                    }
                }
            }
        }

        // Download artist image if missing
        let artist_needs_image: Option<i64> = db.lock().ok().and_then(|conn| {
            let aid: Option<i64> = conn.query_row(
                "SELECT artist_id FROM albums WHERE id = ?1", rusqlite::params![album_id], |row| row.get(0),
            ).ok().flatten();
            aid.filter(|&aid| {
                !conn.query_row(
                    "SELECT image_path IS NOT NULL FROM artists WHERE id = ?1",
                    rusqlite::params![aid], |row| row.get::<_, bool>(0),
                ).unwrap_or(true)
            })
        });

        if let Some(aid) = artist_needs_image {
            if let Some(ref artist) = artist_name {
                if let Ok(lfm) = crate::metadata::lastfm::get_artist_info(artist).await {
                    if let Some(ref url) = lfm.image_url {
                        if let Some(bytes) = crate::metadata::lastfm::download_image(url).await {
                            let filename = format!("artist_{}.jpg", aid);
                            let path = covers_dir.join(&filename);
                            if std::fs::write(&path, &bytes).is_ok() {
                                let path_str = path.to_string_lossy().to_string();
                                if let Ok(conn) = db.lock() {
                                    let _ = conn.execute("UPDATE artists SET image_path = ?1 WHERE id = ?2 AND image_path IS NULL", rusqlite::params![path_str, aid]);
                                    if let Some(ref bio) = lfm.bio {
                                        let _ = conn.execute("UPDATE artists SET bio = ?1 WHERE id = ?2 AND (bio IS NULL OR bio = '')", rusqlite::params![bio, aid]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Enrich tracks with low completeness
    let tracks_to_enrich: Vec<(i64, String, Option<String>)> = {
        match db.lock() {
            Ok(conn) => {
                // First recompute zero-completeness tracks
                if let Ok(mut stmt) = conn.prepare("SELECT id FROM tracks WHERE metadata_completeness = 0") {
                    let ids: Vec<i64> = stmt.query_map([], |row| row.get(0))
                        .unwrap_or_else(|_| panic!())
                        .filter_map(|r| r.ok())
                        .collect();
                    drop(stmt);
                    for id in &ids {
                        let _ = crate::db::tracks::update_completeness(&conn, *id);
                    }
                }

                let mut stmt = match conn.prepare(
                    "SELECT t.id, t.title, a.name
                     FROM tracks t
                     LEFT JOIN artists a ON t.artist_id = a.id
                     WHERE t.metadata_completeness < 70
                     ORDER BY t.metadata_completeness ASC
                     LIMIT 100"
                ) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let result: Vec<_> = stmt.query_map([], |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                )))
                .unwrap_or_else(|_| panic!())
                .filter_map(|r| r.ok())
                .collect();
                result
            }
            Err(_) => return,
        }
    };

    let total_tracks = tracks_to_enrich.len();
    log::info!("Auto-enriching {} tracks", total_tracks);

    for (i, (track_id, title, artist_name)) in tracks_to_enrich.iter().enumerate() {
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        }

        let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
            "phase": "tracks",
            "current": i + 1,
            "total": total_tracks,
            "title": title,
        }));

        // MusicBrainz track enrichment
        match crate::metadata::musicbrainz::enrich_track(&title, artist_name.as_deref()).await {
            Ok(enrichment) => {
                // Apply MusicBrainz data
                if let Ok(conn) = db.lock() {
                    macro_rules! update_track {
                        ($col:expr, $val:expr) => {
                            if let Some(ref v) = $val {
                                let _ = conn.execute(
                                    &format!("UPDATE tracks SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                                    rusqlite::params![v, track_id],
                                );
                            }
                        };
                    }
                    update_track!("musicbrainz_id", enrichment.musicbrainz_id);
                    update_track!("genre", enrichment.genre);
                    update_track!("release_date", enrichment.release_date);
                    update_track!("isrc", enrichment.isrc);
                    update_track!("description", enrichment.description);
                    update_track!("label", enrichment.label);
                    update_track!("language", enrichment.language);
                }
                // conn is dropped here

                // Also enrich with Last.fm track tags
                if let Some(ref artist) = artist_name {
                    if let Ok(lfm) = crate::metadata::lastfm::get_track_info(&title, artist).await {
                        if let Ok(conn) = db.lock() {
                            if !lfm.tags.is_empty() {
                                let tags_str = lfm.tags.join(", ");
                                let _ = conn.execute(
                                    "UPDATE tracks SET genre = ?1 WHERE id = ?2 AND (genre IS NULL OR genre = '')",
                                    rusqlite::params![tags_str, track_id],
                                );
                            }
                            if let Some(ref desc) = lfm.description {
                                let _ = conn.execute(
                                    "UPDATE tracks SET description = ?1 WHERE id = ?2 AND (description IS NULL OR description = '')",
                                    rusqlite::params![desc, track_id],
                                );
                            }
                        }
                    }
                }

                if let Ok(conn) = db.lock() {
                    let _ = crate::db::tracks::update_completeness(&conn, *track_id);
                }
            }
            Err(e) => {
                log::warn!("Failed to enrich track '{}': {}", title, e);
            }
        }
    }

    let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
        "phase": "complete",
        "albums_enriched": total_albums,
        "tracks_enriched": total_tracks,
    }));

    log::info!("Background auto-enrichment complete: {} albums, {} tracks", total_albums, total_tracks);
}

/// Get metadata stats for the library
#[tauri::command]
pub fn get_metadata_stats(
    db: State<'_, Arc<DbPool>>,
) -> Result<serde_json::Value, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0)).unwrap_or(0);
    let avg: f64 = conn.query_row("SELECT COALESCE(AVG(metadata_completeness), 0) FROM tracks", [], |row| row.get(0)).unwrap_or(0.0);
    let complete: i64 = conn.query_row("SELECT COUNT(*) FROM tracks WHERE metadata_completeness >= 80", [], |row| row.get(0)).unwrap_or(0);
    let incomplete: i64 = conn.query_row("SELECT COUNT(*) FROM tracks WHERE metadata_completeness < 50", [], |row| row.get(0)).unwrap_or(0);

    Ok(serde_json::json!({
        "total_tracks": total,
        "average_completeness": avg.round() as i64,
        "complete_tracks": complete,
        "incomplete_tracks": incomplete,
    }))
}
