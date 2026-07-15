use std::sync::Arc;
use rusqlite::params;
use tauri::State;

use crate::db::DbPool;
use crate::db::models::*;
use crate::db::monitored::{MonitoredPlaylist, MonitoredEntry};
use crate::download::DownloadManager;
use crate::download::ytdlp::VideoInfo;

type EntryTuple = (String, Option<String>, Option<String>, Option<f64>, Option<String>, Option<String>);

/// Map fetched playlist entries to the tuple format expected by `upsert_entries`.
/// For DRM platforms (Spotify, etc.) entries may lack a direct URL; fall back to ytsearch.
fn map_entries(entries: &[VideoInfo], label: &str) -> Vec<EntryTuple> {
    entries
        .iter()
        .map(|e| {
            let best_title = e.track.clone().unwrap_or_else(|| e.title.clone());
            let best_artist = e.artist.clone().or_else(|| e.uploader.clone());
            let url = e.webpage_url.clone()
                .filter(|u| !u.is_empty())
                .unwrap_or_else(|| {
                    let query = match &best_artist {
                        Some(a) => format!("{} - {}", a, best_title),
                        None => best_title.clone(),
                    };
                    log::warn!("[{}] No webpage_url for '{}', falling back to search: ytsearch5:{}", label, best_title, query);
                    format!("ytsearch5:{}", query)
                });
            log::info!("[{}] Entry '{}' -> URL: {}", label, best_title, url);
            // Normalize duration: some extractors (e.g. Spotify) return milliseconds
            // instead of seconds.  Any value > 36000 (10 hours) is clearly in ms.
            let duration_secs = e.duration.map(|d| if d > 36_000.0 { d / 1000.0 } else { d });
            (url, Some(best_title), best_artist, duration_secs, e.thumbnail.clone(), e.isrc.clone())
        })
        .collect()
}

#[tauri::command]
pub fn manager_get_playlists(db: State<'_, Arc<DbPool>>) -> Result<Vec<MonitoredPlaylist>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::monitored::get_monitored_playlists(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn manager_get_entries(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
) -> Result<Vec<MonitoredEntry>, String> {
    log::debug!("manager_get_entries called with playlist_id: {}", playlist_id);
    let conn = crate::db::lock(&db)?;
    let entries = crate::db::monitored::get_entries(&conn, playlist_id).map_err(|e| e.to_string())?;
    log::debug!("manager_get_entries returning {} entries", entries.len());
    Ok(entries)
}

#[tauri::command]
pub fn manager_get_new_entries(
    db: State<'_, Arc<DbPool>>,
    playlist_id: i64,
) -> Result<Vec<MonitoredEntry>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::monitored::get_new_entries(&conn, playlist_id).map_err(|e| e.to_string())
}

/// Add a playlist URL to monitor. Creates the playlist in DB immediately so it
/// appears in the UI, then fetches tracks in the background. Emits
/// `manager-playlist-updated` when tracks are loaded, or `manager-playlist-error` on failure.
#[tauri::command]
pub async fn manager_add_playlist(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    app_handle: tauri::AppHandle,
    url: String,
) -> Result<MonitoredPlaylist, String> {
    use tauri::Emitter;

    let parsed = crate::download::url_parser::parse_url(&url);

    // Create playlist in DB immediately so it shows up in the UI
    let placeholder_name = format!("{} playlist", parsed.platform);
    let playlist = {
        let conn = crate::db::lock(&db)?;
        crate::db::monitored::create_monitored_playlist(
            &conn,
            &placeholder_name,
            &parsed.platform,
            &parsed.clean_url,
            None,
        )
        .map_err(|e| e.to_string())?
    };

    // Return the playlist with counts (will be 0 entries at this point)
    let result = {
        let conn = crate::db::lock(&db)?;
        crate::db::monitored::get_monitored_playlists(&conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|p| p.id == playlist.id)
            .ok_or_else(|| "Failed to retrieve created playlist".to_string())?
    };

    // Resolve yt-dlp binary and settings before spawning
    let bin_dir = crate::download::setup::get_bin_dir(&app_handle);
    let ytdlp_binary = crate::download::setup::resolve_ytdlp(&bin_dir)
        .unwrap_or_else(|| "yt-dlp".to_string());
    let ffmpeg_dir = crate::download::setup::resolve_ffmpeg_dir(&bin_dir);
    let cookies_from_browser = {
        let conn = crate::db::lock(&db)?;
        crate::db::settings::get_cookies_browser(&conn)
    };

    let db_clone = db.inner().clone();
    let manager_clone = manager.inner().clone();
    let app_clone = app_handle.clone();
    let playlist_id = playlist.id;

    // Spawn track fetching in the background
    tokio::spawn(async move {
        let fetch_result = async {
            let drm_platforms = ["spotify", "apple_music", "tidal", "deezer", "amazon_music"];
            let fetch_result = if drm_platforms.contains(&parsed.platform.as_str()) {
                log::info!("Using native API for {} playlist (skipping yt-dlp)", parsed.platform);
                let sources = manager_clone.sources.read().await;
                sources.fetch_playlist_entries(&parsed.platform, &url)
                    .await
                    .map_err(|e| format!("Native API error: {}", e))?
            } else {
                crate::download::ytdlp::get_playlist_entries(
                    &ytdlp_binary,
                    ffmpeg_dir.as_deref(),
                    &url,
                    cookies_from_browser.as_deref(),
                )
                .await?
            };

            let entries = fetch_result.entries;
            if entries.is_empty() {
                return Err("No entries found. Make sure this is a valid playlist URL.".to_string());
            }

            // Update playlist name if we got a real one
            if let Some(name) = fetch_result.playlist_title.as_ref().filter(|t| !t.is_empty()) {
                let conn = crate::db::lock(&db_clone)?;
                conn.execute(
                    "UPDATE playlists SET name = ?1 WHERE id = ?2",
                    rusqlite::params![name, playlist_id],
                ).map_err(|e| e.to_string())?;
            }

            let entry_data = map_entries(&entries, "add");
            {
                let conn = crate::db::lock(&db_clone)?;
                crate::db::monitored::upsert_entries(&conn, playlist_id, &entry_data)
                    .map_err(|e| e.to_string())?;
            }

            // Download thumbnails in background
            {
                let db_inner = db_clone.clone();
                let app_inner = app_clone.clone();
                tokio::spawn(async move {
                    download_playlist_thumbnails(&db_inner, &app_inner, playlist_id).await;
                });
            }

            // Get updated playlist with counts
            let conn = crate::db::lock(&db_clone)?;
            let playlists = crate::db::monitored::get_monitored_playlists(&conn)
                .map_err(|e| e.to_string())?;
            playlists
                .into_iter()
                .find(|p| p.id == playlist_id)
                .ok_or_else(|| "Failed to retrieve playlist".to_string())
        }.await;

        match fetch_result {
            Ok(playlist) => {
                let _ = app_clone.emit("manager-playlist-updated", &playlist);
            }
            Err(e) => {
                log::error!("Background playlist fetch failed: {}", e);
                // Remove the empty placeholder on failure
                if let Ok(conn) = db_clone.lock() {
                    let _ = crate::db::monitored::delete_monitored_playlist(&conn, playlist_id);
                }
                let _ = app_clone.emit("manager-playlist-error", serde_json::json!({
                    "url": url,
                    "error": e.to_string(),
                    "playlist_id": playlist_id,
                }));
            }
        }
    });

    Ok(result)
}

/// Sync a monitored playlist - fetch latest entries and detect new ones
#[tauri::command]
pub async fn manager_sync_playlist(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    app_handle: tauri::AppHandle,
    playlist_id: i64,
) -> Result<SyncResult, String> {
    // Get the playlist source URL and platform
    let (source_url, platform) = {
        let conn = crate::db::lock(&db)?;
        let playlist = crate::db::monitored::get_monitored_playlists(&conn)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|p| p.id == playlist_id)
            .ok_or_else(|| "Playlist not found".to_string())?;
        let url = playlist.source_url.clone()
            .ok_or_else(|| "Playlist has no source URL".to_string())?;
        let platform = playlist.source_platform.clone().unwrap_or_default();
        (url, platform)
    };

    // Resolve yt-dlp binary
    let bin_dir = crate::download::setup::get_bin_dir(&app_handle);
    let ytdlp_binary = crate::download::setup::resolve_ytdlp(&bin_dir)
        .unwrap_or_else(|| "yt-dlp".to_string());
    let ffmpeg_dir = crate::download::setup::resolve_ffmpeg_dir(&bin_dir);
    let cookies_from_browser = {
        let conn = crate::db::lock(&db)?;
        crate::db::settings::get_cookies_browser(&conn)
    };

    // For DRM platforms, use native API (yt-dlp blocks Spotify with DRM error)
    let drm_platforms = ["spotify", "apple_music", "tidal", "deezer", "amazon_music"];
    let fetch_result = if drm_platforms.contains(&platform.as_str()) {
        log::info!("Using native API for {} playlist sync (skipping yt-dlp)", platform);
        let sources = manager.sources.read().await;
        sources.fetch_playlist_entries(&platform, &source_url)
            .await
            .map_err(|e| format!("Native API error: {}", e))?
    } else {
        crate::download::ytdlp::get_playlist_entries(
            &ytdlp_binary,
            ffmpeg_dir.as_deref(),
            &source_url,
            cookies_from_browser.as_deref(),
        )
        .await?
    };

    let entry_data = map_entries(&fetch_result.entries, "sync");

    let (new_count, total_count) = {
        let conn = crate::db::lock(&db)?;
        crate::db::monitored::upsert_entries(&conn, playlist_id, &entry_data)
            .map_err(|e| e.to_string())?
    };

    // Spawn async task to download thumbnails in the background
    {
        let db_clone = db.inner().clone();
        let app_clone = app_handle.clone();
        tokio::spawn(async move {
            download_playlist_thumbnails(&db_clone, &app_clone, playlist_id).await;
        });
    }

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
    let conn = crate::db::lock(&db)?;

    let entry = crate::db::monitored::get_entry(&conn, entry_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Entry not found".to_string())?;

    let parsed = crate::download::url_parser::parse_url(&entry.source_url);

    // Use the playlist's source_platform when the URL-parsed platform is "other"
    // (e.g. ytsearch1: URLs from Spotify playlists)
    let playlist_platform: String = conn.query_row(
        "SELECT COALESCE(p.source_platform, '') FROM playlists p
         JOIN monitored_playlist_entries e ON e.playlist_id = p.id
         WHERE e.id = ?1",
        rusqlite::params![entry_id],
        |row| row.get(0),
    ).unwrap_or_default();
    let platform = if !playlist_platform.is_empty() && parsed.platform == "other" {
        &playlist_platform
    } else {
        &parsed.platform
    };

    let default_format = crate::db::settings::get_setting(&conn, "download_format")
        .ok().flatten()
        .unwrap_or_else(|| "mp3".to_string());
    let default_quality = crate::db::settings::get_setting(&conn, "download_quality")
        .ok().flatten()
        .unwrap_or_else(|| "best".to_string());
    let fmt = format.unwrap_or(default_format);
    let qual = quality.unwrap_or(default_quality);

    // For Spotify URLs, convert to YouTube Music search (entry already has metadata)
    let final_url = if parsed.platform == "spotify" && entry.source_url.starts_with("http") {
        log::info!("Converting Spotify entry to YouTube Music search: {}", entry.source_url);
        let search_query = match (&entry.artist, &entry.title) {
            (Some(a), Some(t)) => format!("{} - {}", a, t),
            (None, Some(t)) => t.clone(),
            _ => return Err("Entry missing title for YouTube search".to_string()),
        };
        format!("ytsearch5:{}", search_query)
    } else {
        parsed.clean_url.clone()
    };

    let download = crate::db::downloads::create_download(
        &conn,
        &final_url,
        entry.title.as_deref(),
        entry.artist.as_deref(),
        platform,
        &fmt,
        &qual,
        None,
        None,
        entry.isrc.as_deref(),
        None,
        None,
        None,
        None,
        None,
    )
    .map_err(|e| e.to_string())?;

    // Update entry status to queued with download_id
    crate::db::monitored::update_entry_status(&conn, entry_id, "queued", Some(download.id), None)
        .map_err(|e| e.to_string())?;

    drop(conn);
    manager.start_download(download.id, download.title.clone());
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
    let conn = crate::db::lock(&db)?;

    let entries = crate::db::monitored::get_new_entries(&conn, playlist_id)
        .map_err(|e| e.to_string())?;

    // Use the playlist's source_platform so DRM entries get the correct platform
    // (otherwise ytsearch1: URLs from Spotify playlists get platform "other" and
    // native sources like Deezer are never tried)
    let playlist_platform: String = conn.query_row(
        "SELECT COALESCE(source_platform, '') FROM playlists WHERE id = ?1",
        rusqlite::params![playlist_id],
        |row| row.get(0),
    ).unwrap_or_default();

    let default_format = crate::db::settings::get_setting(&conn, "download_format")
        .ok().flatten()
        .unwrap_or_else(|| "mp3".to_string());
    let default_quality = crate::db::settings::get_setting(&conn, "download_quality")
        .ok().flatten()
        .unwrap_or_else(|| "best".to_string());
    let fmt = format.unwrap_or(default_format);
    let qual = quality.unwrap_or(default_quality);

    // Use a transaction for bulk inserts (fast even for thousands of entries)
    conn.execute_batch("BEGIN IMMEDIATE").map_err(|e| e.to_string())?;

    let result: Result<Vec<(i64, Option<String>)>, String> = (|| {
        let mut download_ids: Vec<(i64, Option<String>)> = Vec::new();
        for entry in &entries {
            let parsed = crate::download::url_parser::parse_url(&entry.source_url);
            // Prefer the playlist's platform for DRM sources; fall back to URL-parsed platform
            let platform = if !playlist_platform.is_empty() && parsed.platform == "other" {
                &playlist_platform
            } else {
                &parsed.platform
            };
            let download = crate::db::downloads::create_download(
                &conn,
                &parsed.clean_url,
                entry.title.as_deref(),
                entry.artist.as_deref(),
                platform,
                &fmt,
                &qual,
                None,
                None,
                entry.isrc.as_deref(),
                None,
                None,
                None,
                None,
                None,
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

            download_ids.push((download.id, entry.title.clone()));
        }
        Ok(download_ids)
    })();

    match result {
        Ok(download_ids) => {
            conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
            let queued = download_ids.len() as i64;
            drop(conn);
            // Start all downloads (concurrency semaphore limits parallel yt-dlp processes).
            // Pass the title so the initial "queued" event isn't a blank "ghost" row.
            for (id, title) in download_ids {
                manager.start_download(id, title);
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
    let conn = crate::db::lock(&db)?;
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
        let conn = crate::db::lock(&db)?;
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
    let conn = crate::db::lock(&db)?;
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
        let conn = crate::db::lock(&db)?;
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
        let conn = crate::db::lock(&db)?;
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
        let conn = crate::db::lock(&db)?;
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
    let conn = crate::db::lock(&db)?;
    crate::db::monitored::delete_monitored_playlist(&conn, playlist_id)
        .map_err(|e| e.to_string())
}

/// Download thumbnails for playlist entries and set the playlist cover.
/// Runs in the background — errors are logged, not propagated.
async fn download_playlist_thumbnails(
    db: &Arc<crate::db::DbPool>,
    app_handle: &tauri::AppHandle,
    playlist_id: i64,
) {
    use tauri::{Manager, Emitter};

    let thumbnails_dir = match app_handle.path().app_data_dir() {
        Ok(dir) => dir.join("thumbnails"),
        Err(_) => return,
    };
    if let Err(e) = std::fs::create_dir_all(&thumbnails_dir) {
        log::warn!("Failed to create thumbnails dir: {}", e);
        return;
    }

    // Get entries with remote thumbnail URLs
    let entries = match db.lock() {
        Ok(conn) => crate::db::monitored::get_entries_with_remote_thumbnails(&conn, playlist_id).unwrap_or_default(),
        Err(_) => return,
    };

    let mut first_local_path: Option<String> = None;

    for (entry_id, thumb_url) in &entries {
        let bytes = match crate::metadata::lastfm::download_image(thumb_url).await {
            Some(b) => b,
            None => {
                log::warn!("Failed to download thumbnail for entry {}", entry_id);
                continue;
            }
        };

        // Determine extension from URL or default to jpg
        let ext = if thumb_url.contains(".png") { "png" }
            else if thumb_url.contains(".webp") { "webp" }
            else { "jpg" };

        let filename = format!("entry_{}_{}.{}", playlist_id, entry_id, ext);
        let local_path = thumbnails_dir.join(&filename);

        if let Err(e) = std::fs::write(&local_path, &bytes) {
            log::warn!("Failed to write thumbnail {}: {}", filename, e);
            continue;
        }

        let path_str = local_path.to_string_lossy().to_string();

        if first_local_path.is_none() {
            first_local_path = Some(path_str.clone());
        }

        if let Ok(conn) = db.lock() {
            let _ = crate::db::monitored::update_entry_thumbnail(&conn, *entry_id, &path_str);
        }
    }

    // Use first entry's thumbnail as the playlist cover if none is set
    if let Some(cover_path) = first_local_path {
        if let Ok(conn) = db.lock() {
            let _ = conn.execute(
                "UPDATE playlists SET cover_art_path = ?1 WHERE id = ?2 AND cover_art_path IS NULL",
                rusqlite::params![cover_path, playlist_id],
            );
        }
    }

    // Notify frontend that thumbnails are ready
    let _ = app_handle.emit("manager-thumbnails-ready", serde_json::json!({ "playlist_id": playlist_id }));
}
