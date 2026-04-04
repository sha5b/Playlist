use std::sync::Arc;
use rusqlite::params;
use tauri::{Emitter, Manager, State};

use crate::db::DbPool;
use crate::db::models::*;
use crate::download::DownloadManager;

const ALLOWED_FORMATS: &[&str] = &["mp3", "opus", "flac", "m4a", "wav", "ogg", "vorbis"];
const ALLOWED_QUALITIES: &[&str] = &["best", "320", "256", "192", "128"];

fn validate_download_params(format: &str, quality: &str) -> Result<(), String> {
    if !ALLOWED_FORMATS.contains(&format) {
        return Err(format!("Invalid download format '{}'. Allowed: {}", format, ALLOWED_FORMATS.join(", ")));
    }
    if !ALLOWED_QUALITIES.contains(&quality) {
        return Err(format!("Invalid download quality '{}'. Allowed: {}", quality, ALLOWED_QUALITIES.join(", ")));
    }
    Ok(())
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
    let (default_format, default_quality) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let f = crate::db::settings::get_setting(&conn, "download_format")
            .ok().flatten()
            .unwrap_or_else(|| "mp3".to_string());
        let q = crate::db::settings::get_setting(&conn, "download_quality")
            .ok().flatten()
            .unwrap_or_else(|| "best".to_string());
        (f, q)
    };
    let fmt = format.unwrap_or(default_format);
    let qual = quality.unwrap_or(default_quality);
    validate_download_params(&fmt, &qual)?;

    // For Spotify URLs, fetch metadata and convert to YouTube search
    let (final_url, title, artist) = if parsed.platform == "spotify" {
        log::info!("Converting Spotify URL to YouTube search: {}", url);
        match crate::download::spotify::fetch_track_metadata(&url).await {
            Some((track_title, track_artist)) => {
                let search_query = match track_artist {
                    Some(ref a) => format!("{} - {}", a, track_title),
                    None => track_title.clone(),
                };
                let yt_url = format!("ytsearch1:{}", search_query);
                (yt_url, Some(track_title), track_artist)
            }
            None => {
                return Err("Failed to fetch Spotify track metadata".to_string());
            }
        }
    } else {
        (parsed.clean_url.clone(), None, None)
    };

    let download = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::downloads::create_download(
            &conn,
            &final_url,
            title.as_deref(),
            artist.as_deref(),
            &parsed.platform,
            &fmt,
            &qual,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .map_err(|e| e.to_string())?
    };

    manager.start_download(download.id, download.title.clone());
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
    let (default_format, default_quality) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let f = crate::db::settings::get_setting(&conn, "download_format")
            .ok().flatten()
            .unwrap_or_else(|| "mp3".to_string());
        let q = crate::db::settings::get_setting(&conn, "download_quality")
            .ok().flatten()
            .unwrap_or_else(|| "best".to_string());
        (f, q)
    };
    let fmt = format.unwrap_or(default_format);
    let qual = quality.unwrap_or(default_quality);
    let mut downloads = Vec::new();

    for url in &urls {
        let parsed = crate::download::url_parser::parse_url(url);

        // For Spotify URLs, fetch metadata and convert to YouTube search
        let (final_url, title, artist) = if parsed.platform == "spotify" {
            log::info!("Converting Spotify URL to YouTube search: {}", url);
            match crate::download::spotify::fetch_track_metadata(url).await {
                Some((track_title, track_artist)) => {
                    let search_query = match track_artist {
                        Some(ref a) => format!("{} - {}", a, track_title),
                        None => track_title.clone(),
                    };
                    let yt_url = format!("ytsearch1:{}", search_query);
                    (yt_url, Some(track_title), track_artist)
                }
                None => {
                    log::warn!("Failed to fetch Spotify track metadata for {}, skipping", url);
                    continue;
                }
            }
        } else {
            (parsed.clean_url.clone(), None, None)
        };

        let download = {
            let conn = db.lock().map_err(|e| e.to_string())?;
            crate::db::downloads::create_download(
                &conn,
                &final_url,
                title.as_deref(),
                artist.as_deref(),
                &parsed.platform,
                &fmt,
                &qual,
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .map_err(|e| e.to_string())?
        };
        manager.start_download(download.id, download.title.clone());
        downloads.push(download);
    }

    Ok(downloads)
}

#[derive(serde::Deserialize)]
pub struct SearchDownloadRequest {
    pub query: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album_id: Option<i64>,
    pub artist_id: Option<i64>,
}

#[tauri::command]
pub async fn download_search_and_start(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    query: String,
    title: Option<String>,
    artist: Option<String>,
    album_id: Option<i64>,
    artist_id: Option<i64>,
    disc_number: Option<i64>,
    track_number: Option<i64>,
    format: Option<String>,
    quality: Option<String>,
) -> Result<Download, String> {
    let search_url = format!("ytsearch1:{}", query);
    let (default_format, default_quality) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let f = crate::db::settings::get_setting(&conn, "download_format")
            .ok().flatten()
            .unwrap_or_else(|| "mp3".to_string());
        let q = crate::db::settings::get_setting(&conn, "download_quality")
            .ok().flatten()
            .unwrap_or_else(|| "best".to_string());
        (f, q)
    };
    let fmt = format.unwrap_or(default_format);
    let qual = quality.unwrap_or(default_quality);

    let download = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::downloads::create_download(
            &conn,
            &search_url,
            title.as_deref(),
            artist.as_deref(),
            "youtube_search",
            &fmt,
            &qual,
            album_id,
            artist_id,
            None,
            disc_number,
            track_number,
            None,
        )
        .map_err(|e| e.to_string())?
    };

    manager.start_download(download.id, download.title.clone());
    Ok(download)
}

#[tauri::command]
pub async fn download_search_and_start_batch(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    queries: Vec<SearchDownloadRequest>,
    format: Option<String>,
    quality: Option<String>,
) -> Result<Vec<Download>, String> {
    let (default_format, default_quality) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let f = crate::db::settings::get_setting(&conn, "download_format")
            .ok().flatten()
            .unwrap_or_else(|| "mp3".to_string());
        let q = crate::db::settings::get_setting(&conn, "download_quality")
            .ok().flatten()
            .unwrap_or_else(|| "best".to_string());
        (f, q)
    };
    let fmt = format.unwrap_or(default_format);
    let qual = quality.unwrap_or(default_quality);
    let mut downloads = Vec::new();

    for req in &queries {
        let search_url = format!("ytsearch1:{}", req.query);
        let download = {
            let conn = db.lock().map_err(|e| e.to_string())?;
            crate::db::downloads::create_download(
                &conn,
                &search_url,
                req.title.as_deref(),
                req.artist.as_deref(),
                "youtube_search",
                &fmt,
                &qual,
                req.album_id,
                req.artist_id,
                None,
                None,
                None,
                None,
            )
            .map_err(|e| e.to_string())?
        };
        manager.start_download(download.id, download.title.clone());
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
    manager.start_download(id, download.title.clone());
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

// --- Download Sources ---

#[tauri::command]
pub async fn download_get_sources_status(
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<Vec<crate::download::source::SourceStatus>, String> {
    Ok(manager.get_sources_status().await)
}

#[tauri::command]
pub async fn download_set_source_credentials(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    platform: String,
    credentials: std::collections::HashMap<String, String>,
) -> Result<(), String> {
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        for (key, value) in &credentials {
            let setting_key = format!("{}_{}", platform, key);
            crate::db::settings::set_setting(&conn, &setting_key, value)
                .map_err(|e| e.to_string())?;
        }
    }
    // Rebuild sources with new credentials
    manager.refresh_sources().await;
    Ok(())
}

#[tauri::command]
pub async fn download_test_source(
    manager: State<'_, Arc<DownloadManager>>,
    platform: String,
) -> Result<(), String> {
    manager
        .test_source(&platform)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn download_refresh_sources(
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.refresh_sources().await;
    Ok(())
}

#[tauri::command]
pub async fn download_artist_missing(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    artist_id: i64,
    album_mbids: Vec<String>,
) -> Result<Vec<Download>, String> {
    let artist_name: String = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT name FROM artists WHERE id = ?1",
            params![artist_id],
            |row| row.get(0),
        ).map_err(|e| e.to_string())?
    };

    let (default_format, default_quality) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let f = crate::db::settings::get_setting(&conn, "download_format")
            .ok().flatten()
            .unwrap_or_else(|| "mp3".to_string());
        let q = crate::db::settings::get_setting(&conn, "download_quality")
            .ok().flatten()
            .unwrap_or_else(|| "best".to_string());
        (f, q)
    };

    let mut all_downloads = Vec::new();

    for mbid in &album_mbids {
        // Rate limit for MusicBrainz
        if !all_downloads.is_empty() {
            tokio::time::sleep(std::time::Duration::from_millis(crate::metadata::musicbrainz::MB_RATE_LIMIT_MS)).await;
        }

        // Fetch the release group's primary release to get tracklist
        let detail_url = format!(
            "https://musicbrainz.org/ws/2/release-group/{}?inc=releases&fmt=json",
            mbid
        );
        let client = reqwest::Client::builder()
            .user_agent("Playlist/0.1.0 (https://github.com/sha5b/Playlist)")
            .build()
            .unwrap_or_default();

        let release_id = match client.get(&detail_url).send().await {
            Ok(resp) => {
                let json: serde_json::Value = resp.json().await.unwrap_or_default();
                json["releases"].as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|r| r["id"].as_str())
                    .map(|s| s.to_string())
            }
            Err(_) => None,
        };

        let Some(release_id) = release_id else { continue };

        // Rate limit
        tokio::time::sleep(std::time::Duration::from_millis(crate::metadata::musicbrainz::MB_RATE_LIMIT_MS)).await;

        // Fetch tracklist from the release (include ISRCs for precise matching)
        let release_url = format!(
            "https://musicbrainz.org/ws/2/release/{}?inc=recordings+isrcs&fmt=json",
            release_id
        );
        // (title, disc, track_num, duration_ms, isrc)
        let tracks: Vec<(String, i64, i64, Option<i64>, Option<String>)> = match client.get(&release_url).send().await {
            Ok(resp) => {
                let json: serde_json::Value = resp.json().await.unwrap_or_default();
                let mut tracks = Vec::new();
                if let Some(media) = json["media"].as_array() {
                    for medium in media {
                        let disc = medium["position"].as_i64().unwrap_or(1);
                        if let Some(medium_tracks) = medium["tracks"].as_array() {
                            for track in medium_tracks {
                                let title = track["title"].as_str().unwrap_or("").to_string();
                                let num = track["number"].as_str()
                                    .and_then(|n| n.parse::<i64>().ok())
                                    .unwrap_or(0);
                                // Duration from recording.length (milliseconds)
                                let duration_ms = track["recording"]["length"].as_i64()
                                    .or_else(|| track["length"].as_i64());
                                // First ISRC from the recording
                                let isrc = track["recording"]["isrcs"].as_array()
                                    .and_then(|arr| arr.first())
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                if !title.is_empty() {
                                    tracks.push((title, disc, num, duration_ms, isrc));
                                }
                            }
                        }
                    }
                }
                tracks
            }
            Err(_) => continue,
        };

        // Create an album in the DB for this release and queue downloads
        let album_id = {
            let conn = db.lock().map_err(|e| e.to_string())?;
            let album_title_from_rg: Option<String> = {
                // Get the album title from the enriched discography
                let disco_json: Option<String> = conn.query_row(
                    "SELECT enriched_discography FROM artists WHERE id = ?1",
                    params![artist_id],
                    |row| row.get(0),
                ).unwrap_or(None);
                disco_json.and_then(|j| {
                    let disco: Vec<serde_json::Value> = serde_json::from_str(&j).unwrap_or_default();
                    disco.iter()
                        .find(|e| e["mbid"].as_str() == Some(mbid))
                        .and_then(|e| e["title"].as_str())
                        .map(|s| s.to_string())
                })
            };
            let album_title = album_title_from_rg.unwrap_or_else(|| "Unknown Album".to_string());
            let aid = crate::db::albums::find_or_create(
                &conn,
                &album_title,
                Some(artist_id),
                Some(&artist_name),
                None,
            ).map_err(|e| e.to_string())?;
            // Set the MusicBrainz ID on the album
            let _ = conn.execute(
                "UPDATE albums SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL",
                params![release_id, aid],
            );
            // Store enriched tracklist
            let tracklist_json: Vec<serde_json::Value> = tracks.iter().map(|(title, disc, num, dur, _isrc)| {
                serde_json::json!({
                    "disc_number": disc,
                    "track_number": num,
                    "title": title,
                    "duration_ms": dur,
                })
            }).collect();
            let tl_str = serde_json::to_string(&tracklist_json).unwrap_or_default();
            let _ = conn.execute(
                "UPDATE albums SET enriched_tracklist = ?1, total_tracks = ?2 WHERE id = ?3",
                params![tl_str, tracks.len() as i64, aid],
            );
            aid
        };

        // Queue downloads for each track using ytmsearch5 for better matching
        for (title, disc, num, duration_ms, isrc) in &tracks {
            let query = format!("{} - {}", artist_name, title);
            let search_url = format!("ytmsearch5:{}", query);
            let download = {
                let conn = db.lock().map_err(|e| e.to_string())?;
                crate::db::downloads::create_download(
                    &conn,
                    &search_url,
                    Some(title),
                    Some(&artist_name),
                    "youtube_search",
                    &default_format,
                    &default_quality,
                    Some(album_id),
                    Some(artist_id),
                    isrc.as_deref(),
                    Some(*disc),
                    Some(*num),
                    *duration_ms,
                )
                .map_err(|e| e.to_string())?
            };
            manager.start_download(download.id, download.title.clone());
            all_downloads.push(download);
        }
    }

    Ok(all_downloads)
}

#[tauri::command]
pub async fn download_music_video(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    track_id: i64,
) -> Result<String, String> {
    // Get track info
    let mv_url = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let url: Option<String> = conn.query_row(
            "SELECT music_video_url FROM tracks WHERE id = ?1",
            params![track_id],
            |row| row.get(0),
        ).map_err(|e| e.to_string())?;
        url
    };

    let mv_url = mv_url.ok_or("Track has no music video URL")?;

    // Store music videos in app data dir (not user's download dir)
    let mv_dir = app_handle.path().app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?
        .join("music_videos");
    std::fs::create_dir_all(&mv_dir)
        .map_err(|e| format!("Failed to create music_videos dir: {}", e))?;
    let download_dir = mv_dir.to_string_lossy().to_string();

    let bin_dir = crate::download::setup::get_bin_dir(&app_handle);
    let binary = crate::download::setup::resolve_ytdlp(&bin_dir)
        .unwrap_or_else(|| "yt-dlp".to_string());
    let ffmpeg_dir = crate::download::setup::resolve_ffmpeg_dir(&bin_dir);
    let cookies = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::settings::get_cookies_browser(&conn)
    };

    let file_stem = format!("mv_{}", track_id);
    let output_dir = std::path::Path::new(&download_dir);

    let app_handle_progress = app_handle.clone();
    let tid = track_id;
    let path = crate::download::ytdlp::download_video(
        &binary,
        ffmpeg_dir.as_deref(),
        &mv_url,
        output_dir,
        &file_stem,
        cookies.as_deref(),
        move |progress| {
            let _ = app_handle_progress.emit("music-video-download-progress", serde_json::json!({
                "track_id": tid,
                "percent": progress.percent,
                "speed": progress.speed,
                "eta": progress.eta,
            }));
        },
    ).await?;

    // Save path to DB
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE tracks SET music_video_path = ?1 WHERE id = ?2",
            params![path, track_id],
        ).map_err(|e| e.to_string())?;
    }

    Ok(path)
}
