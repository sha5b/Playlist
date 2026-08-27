//! Playlist CRUD, smart playlists, and M3U import/export.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use rusqlite::params;
use tauri::{Emitter, State};

use crate::db::DbPool;
use crate::db::models::*;

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

// --- Smart playlists ---

#[tauri::command]
pub fn library_create_smart_playlist(
    db: State<'_, Arc<DbPool>>,
    name: String,
    description: Option<String>,
    rules: String,
) -> Result<Playlist, String> {
    crate::db::smart::validate_rules(&rules)?;
    let conn = crate::db::lock(&db)?;
    crate::db::playlists::create_smart_playlist(&conn, &name, description.as_deref(), &rules)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_update_smart_playlist(
    db: State<'_, Arc<DbPool>>,
    id: i64,
    name: Option<String>,
    description: Option<String>,
    rules: Option<String>,
) -> Result<Playlist, String> {
    if let Some(ref r) = rules {
        crate::db::smart::validate_rules(r)?;
    }
    let conn = crate::db::lock(&db)?;
    crate::db::playlists::update_smart_playlist(
        &conn,
        id,
        name.as_deref(),
        description.as_deref(),
        rules.as_deref(),
    )
    .map_err(|e| e.to_string())
}

/// Count how many tracks a rule set currently matches (live preview).
#[tauri::command]
pub fn library_smart_playlist_preview(
    db: State<'_, Arc<DbPool>>,
    rules: String,
) -> Result<i64, String> {
    let parsed = crate::db::smart::parse_rules(&rules)?;
    let conn = crate::db::lock(&db)?;
    crate::db::smart::count_tracks(&conn, &parsed)
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

// --- Playlist M3U export / import ---

#[tauri::command]
pub fn library_export_playlist(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
    dest_path: String,
) -> Result<i64, String> {
    let conn = crate::db::lock(&db)?;

    // Ensure the playlist exists (gives a clearer error than an empty file)
    let _name: String = conn
        .query_row(
            "SELECT name FROM playlists WHERE id = ?1",
            params![playlist_id],
            |row| row.get(0),
        )
        .map_err(|_| "Playlist not found".to_string())?;

    let entries: Vec<(String, String, Option<i64>, Option<String>)> = {
        let mut stmt = conn
            .prepare(
                "SELECT t.file_path, t.title, t.duration_ms, ar.name
                 FROM playlist_tracks pt
                 JOIN tracks t ON t.id = pt.track_id
                 LEFT JOIN artists ar ON ar.id = t.artist_id
                 WHERE pt.playlist_id = ?1
                 ORDER BY pt.position",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(params![playlist_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        rows
    };

    if entries.is_empty() {
        return Err("Playlist has no tracks to export".to_string());
    }

    let mut content = String::from("#EXTM3U\n");
    for (file_path, title, duration_ms, artist) in &entries {
        let duration_secs = duration_ms.unwrap_or(0) / 1000;
        let display = match artist {
            Some(a) if !a.is_empty() => format!("{} - {}", a, title),
            _ => title.clone(),
        };
        content.push_str(&format!("#EXTINF:{},{}\n", duration_secs, display));
        content.push_str(file_path);
        content.push('\n');
    }

    let dest = PathBuf::from(&dest_path);
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }
    std::fs::write(&dest, content).map_err(|e| format!("Failed to write playlist: {}", e))?;

    log::info!("Exported playlist {} ({} tracks) to {}", playlist_id, entries.len(), dest_path);
    Ok(entries.len() as i64)
}

#[derive(serde::Serialize)]
pub struct M3uImportResult {
    pub playlist_id: i64,
    pub playlist_name: String,
    pub matched: i64,
    pub unmatched: i64,
    pub unmatched_entries: Vec<String>,
}

#[tauri::command]
pub fn library_import_m3u(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    path: String,
) -> Result<M3uImportResult, String> {
    let m3u_path = PathBuf::from(&path);
    let content = std::fs::read_to_string(&m3u_path)
        .map_err(|e| format!("Failed to read playlist file: {}", e))?;
    let base_dir = m3u_path.parent().map(|p| p.to_path_buf());

    // Parse: each non-comment line is a track path; a preceding #EXTINF
    // carries "duration,artist - title" metadata used for fallback matching.
    struct M3uEntry {
        raw_path: String,
        artist: Option<String>,
        title: Option<String>,
    }

    let mut entries: Vec<M3uEntry> = Vec::new();
    let mut pending: Option<(Option<String>, Option<String>)> = None; // (artist, title)

    for line in content.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("#EXTINF:") {
            // "#EXTINF:123,Artist - Title" (fields before the comma are ignored)
            let desc = rest.split_once(',').map(|(_, d)| d.trim()).unwrap_or("");
            if desc.is_empty() {
                pending = None;
            } else if let Some((artist, title)) = desc.split_once(" - ") {
                pending = Some((
                    Some(artist.trim().to_string()).filter(|s| !s.is_empty()),
                    Some(title.trim().to_string()).filter(|s| !s.is_empty()),
                ));
            } else {
                pending = Some((None, Some(desc.to_string())));
            }
            continue;
        }
        if line.starts_with('#') {
            continue;
        }
        let (artist, title) = pending.take().unwrap_or((None, None));
        entries.push(M3uEntry {
            raw_path: line.to_string(),
            artist,
            title,
        });
    }

    if entries.is_empty() {
        return Err("No tracks found in playlist file".to_string());
    }

    let playlist_name = m3u_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "Imported Playlist".to_string());

    let conn = crate::db::lock(&db)?;

    let playlist = crate::db::playlists::create_playlist(&conn, &playlist_name, None)
        .map_err(|e| e.to_string())?;

    let mut matched = 0i64;
    let mut unmatched = 0i64;
    let mut unmatched_entries: Vec<String> = Vec::new();

    for entry in &entries {
        // Resolve relative paths against the m3u file's directory
        let entry_path = Path::new(&entry.raw_path);
        let resolved: PathBuf = if entry_path.is_absolute() {
            entry_path.to_path_buf()
        } else if let Some(ref base) = base_dir {
            base.join(entry_path)
        } else {
            entry_path.to_path_buf()
        };

        // 1) exact file path match (raw, resolved, then canonicalized)
        let mut candidates: Vec<String> = vec![
            entry.raw_path.clone(),
            resolved.to_string_lossy().to_string(),
        ];
        if let Ok(canon) = std::fs::canonicalize(&resolved) {
            candidates.push(canon.to_string_lossy().to_string());
        }
        candidates.dedup();

        let mut track_id: Option<i64> = None;
        for cand in &candidates {
            match conn.query_row(
                "SELECT id FROM tracks WHERE file_path = ?1",
                params![cand],
                |row| row.get::<_, i64>(0),
            ) {
                Ok(id) => {
                    track_id = Some(id);
                    break;
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => continue,
                Err(e) => return Err(e.to_string()),
            }
        }

        // 2) fallback: (title, artist) from the EXTINF line
        if track_id.is_none() {
            if let Some(ref title) = entry.title {
                let result = conn.query_row(
                    "SELECT t.id FROM tracks t
                     LEFT JOIN artists ar ON ar.id = t.artist_id
                     WHERE t.title = ?1 COLLATE NOCASE
                       AND (?2 IS NULL OR ar.name = ?2 COLLATE NOCASE)
                     LIMIT 1",
                    params![title, entry.artist],
                    |row| row.get::<_, i64>(0),
                );
                match result {
                    Ok(id) => track_id = Some(id),
                    Err(rusqlite::Error::QueryReturnedNoRows) => {}
                    Err(e) => return Err(e.to_string()),
                }
            }
        }

        match track_id {
            Some(id) => {
                crate::db::playlists::add_track_to_playlist(&conn, playlist.id, id)
                    .map_err(|e| e.to_string())?;
                matched += 1;
            }
            None => {
                unmatched += 1;
                unmatched_entries.push(entry.raw_path.clone());
            }
        }
    }

    log::info!(
        "Imported m3u '{}': {} matched, {} unmatched",
        playlist_name, matched, unmatched
    );
    let _ = app_handle.emit("library-updated", ());

    Ok(M3uImportResult {
        playlist_id: playlist.id,
        playlist_name,
        matched,
        unmatched,
        unmatched_entries,
    })
}
