//! Import audio files from the filesystem into the library.

use std::sync::Arc;
use rusqlite::params;
use tauri::{Emitter, Manager, State};

use crate::db::DbPool;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use crate::metadata::tags;

// --- Import ---

/// Recursively collect audio files under a directory.
fn collect_audio_files(root: &Path) -> Vec<PathBuf> {
    WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && tags::is_audio_file(e.path()))
        .map(|e| e.into_path())
        .collect()
}

/// Import a list of audio files into the library. Shared by folder import,
/// dropped-paths import, the folder-watch auto-import service, etc.
/// Skips files already in the DB by path.
/// Returns the number of newly imported tracks.
pub fn import_audio_files(
    conn: &rusqlite::Connection,
    covers_dir: &Path,
    audio_files: &[PathBuf],
) -> Result<i64, String> {
    let mut imported = 0i64;

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

    for file_path in audio_files {
        let file_path = file_path.as_path();
        let path_str = file_path.to_string_lossy().to_string();

        if existing_paths.contains(&path_str) {
            continue;
        }

        // Read tags and cover art in a single file read
        let (tag_data, cover_art_path) = match tags::read_tags_and_cover(file_path, covers_dir) {
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
            .and_then(|name| crate::db::artists::find_or_create(conn, name).ok());

        // Find or create album
        let album_id = tag_data.album.as_ref().and_then(|title| {
            crate::db::albums::find_or_create(
                conn,
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
                        let _ = crate::db::albums::update_cover_art_if_missing(conn, aid, cover);
                    }
                }
                // Propagate album metadata from tags
                if let Some(aid) = album_id {
                    let _ = crate::db::albums::update_metadata_if_missing(
                        conn,
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
        let _ = crate::db::tracks::update_fts(conn, track_id);
    }

    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;

    Ok(imported)
}

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

    let audio_files = collect_audio_files(Path::new(&path));
    let imported = import_audio_files(&conn, &covers_dir, &audio_files)?;

    log::info!("Imported {} tracks from {}", imported, path);
    if imported > 0 {
        let _ = app_handle.emit("library-updated", ());
    }
    Ok(imported)
}

/// Import a mix of dropped paths: directories are scanned recursively for
/// audio files, plain files are imported when they have an audio extension.
#[tauri::command]
pub fn library_import_paths(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    paths: Vec<String>,
) -> Result<i64, String> {
    let conn = crate::db::lock(&db)?;

    let covers_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e: tauri::Error| e.to_string())?
        .join("covers");

    let mut files: Vec<PathBuf> = Vec::new();
    for p in &paths {
        let pb = PathBuf::from(p);
        if pb.is_dir() {
            files.extend(collect_audio_files(&pb));
        } else if pb.is_file() && tags::is_audio_file(&pb) {
            files.push(pb);
        }
    }
    files.sort();
    files.dedup();

    let imported = import_audio_files(&conn, &covers_dir, &files)?;

    log::info!("Imported {} tracks from {} dropped path(s)", imported, paths.len());
    if imported > 0 {
        let _ = app_handle.emit("library-updated", ());
    }
    Ok(imported)
}
