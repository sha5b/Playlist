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

/// Resolve the effective format/quality for a new download: explicit argument,
/// else the saved setting, else the built-in default. Validates the result so
/// every entry point (single, batch, search) rejects bad values up front.
fn resolve_format_quality(
    db: &State<'_, Arc<DbPool>>,
    format: Option<String>,
    quality: Option<String>,
) -> Result<(String, String), String> {
    let (default_format, default_quality) = {
        let conn = crate::db::lock(db)?;
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
    Ok((fmt, qual))
}

/// Convert a Spotify URL into a YouTube search URL with title/artist context.
/// Returns the original cleaned URL for non-Spotify platforms, plus the
/// platform name for the download row.
async fn to_download_url(
    url: &str,
) -> Result<(String, Option<String>, Option<String>, String), String> {
    let parsed = crate::download::url_parser::parse_url(url);
    if parsed.platform == "spotify" {
        log::info!("Converting Spotify URL to YouTube search: {}", url);
        match crate::download::spotify::fetch_track_metadata(url).await {
            Some((title, artist)) => {
                let search_query = match artist {
                    Some(ref a) => format!("{} - {}", a, title),
                    None => title.clone(),
                };
                Ok((format!("ytsearch5:{}", search_query), Some(title), artist, parsed.platform))
            }
            None => Err("Failed to fetch Spotify track metadata".to_string()),
        }
    } else {
        Ok((parsed.clean_url.clone(), None, None, parsed.platform))
    }
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

/// The OS music folder default used when no custom download dir is set
/// (e.g. `~/Music/Playlist`). The settings page shows this as the active default.
#[tauri::command]
pub fn download_default_dir(app_handle: tauri::AppHandle) -> String {
    crate::download::default_download_dir(&app_handle)
        .to_string_lossy()
        .to_string()
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
    let (fmt, qual) = resolve_format_quality(&db, format, quality)?;

    // For Spotify URLs, fetch metadata and convert to YouTube search
    let (final_url, title, artist, platform) = to_download_url(&url).await?;

    let download = {
        let conn = crate::db::lock(&db)?;
        crate::db::downloads::create_download(
            &conn,
            &final_url,
            title.as_deref(),
            artist.as_deref(),
            &platform,
            &fmt,
            &qual,
            None,
            None,
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
    let (fmt, qual) = resolve_format_quality(&db, format, quality)?;
    let mut downloads = Vec::new();

    for url in &urls {
        // For Spotify URLs, fetch metadata and convert to YouTube search
        let (final_url, title, artist, platform) = match to_download_url(url).await {
            Ok(v) => v,
            Err(e) => {
                log::warn!("{} for {}, skipping", e, url);
                continue;
            }
        };

        let download = {
            let conn = crate::db::lock(&db)?;
            crate::db::downloads::create_download(
                &conn,
                &final_url,
                title.as_deref(),
                artist.as_deref(),
                &platform,
                &fmt,
                &qual,
                None,
                None,
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
#[allow(clippy::too_many_arguments)]
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
    let search_url = format!("ytsearch5:{}", query);
    let (fmt, qual) = resolve_format_quality(&db, format, quality)?;

    let download = {
        let conn = crate::db::lock(&db)?;
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
            None,
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
    let (fmt, qual) = resolve_format_quality(&db, format, quality)?;
    let mut downloads = Vec::new();

    for req in &queries {
        let search_url = format!("ytsearch5:{}", req.query);
        let download = {
            let conn = crate::db::lock(&db)?;
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

/// Cancel every active/queued download across all playlists.
#[tauri::command]
pub async fn download_cancel_all(
    manager: State<'_, Arc<DownloadManager>>,
) -> Result<(), String> {
    manager.cancel_all().await;
    Ok(())
}

#[tauri::command]
pub async fn download_retry(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<DownloadManager>>,
    id: i64,
) -> Result<Download, String> {
    let conn = crate::db::lock(&db)?;
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
    let conn = crate::db::lock(&db)?;
    crate::db::downloads::get_active_downloads(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn download_get_history(
    db: State<'_, Arc<DbPool>>,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Download>, i64), String> {
    let conn = crate::db::lock(&db)?;
    crate::db::downloads::get_download_history(&conn, offset, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn download_clear_history(db: State<'_, Arc<DbPool>>) -> Result<i64, String> {
    let conn = crate::db::lock(&db)?;
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
        let conn = crate::db::lock(&db)?;
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
        let conn = crate::db::lock(&db)?;
        conn.query_row(
            "SELECT name FROM artists WHERE id = ?1",
            params![artist_id],
            |row| row.get(0),
        ).map_err(|e| e.to_string())?
    };

    let (default_format, default_quality) = resolve_format_quality(&db, None, None)?;

    let mut all_downloads = Vec::new();

    for mbid in &album_mbids {
        // Rate limit for MusicBrainz — every iteration makes a request, so
        // the sleep must be unconditional (gating it on queued downloads let
        // bursts of failed lookups hammer the API).
        tokio::time::sleep(std::time::Duration::from_millis(crate::metadata::musicbrainz::MB_RATE_LIMIT_MS)).await;

        // Fetch the release group's primary release to get tracklist
        let detail_url = format!(
            "https://musicbrainz.org/ws/2/release-group/{}?inc=releases&fmt=json",
            mbid
        );
        let client = reqwest::Client::builder()
            .user_agent("Playlist/0.1.0 (https://github.com/sha5b/Playlist)")
            .build()
            .unwrap_or_default();

        // Capture the release-group title too — used as the album name fallback so
        // distinct un-matched releases don't all collapse into one "Unknown Album" (B4).
        let (release_id, rg_title) = match client.get(&detail_url).send().await {
            Ok(resp) => {
                let json: serde_json::Value = resp.json().await.unwrap_or_default();
                let rid = json["releases"].as_array()
                    .and_then(|arr| arr.first())
                    .and_then(|r| r["id"].as_str())
                    .map(|s| s.to_string());
                let title = json["title"].as_str().map(|s| s.to_string());
                (rid, title)
            }
            Err(_) => (None, None),
        };

        let Some(release_id) = release_id else { continue };

        // Rate limit
        tokio::time::sleep(std::time::Duration::from_millis(crate::metadata::musicbrainz::MB_RATE_LIMIT_MS)).await;

        // Fetch tracklist from the release (include ISRCs for precise matching)
        let release_url = format!(
            "https://musicbrainz.org/ws/2/release/{}?inc=recordings+isrcs&fmt=json",
            release_id
        );
        // (title, disc, track_num, duration_ms, isrc, recording_mbid)
        #[allow(clippy::type_complexity)]
        let tracks: Vec<(String, i64, i64, Option<i64>, Option<String>, Option<String>)> = match client.get(&release_url).send().await {
            Ok(resp) => {
                let json: serde_json::Value = resp.json().await.unwrap_or_default();
                let mut tracks = Vec::new();
                if let Some(media) = json["media"].as_array() {
                    for medium in media {
                        let disc = medium["position"].as_i64().unwrap_or(1);
                        if let Some(medium_tracks) = medium["tracks"].as_array() {
                            for (idx, track) in medium_tracks.iter().enumerate() {
                                let title = track["title"].as_str().unwrap_or("").to_string();
                                // Prefer the numeric "position" — "number" is a
                                // free-form string ("A1", "B2" on vinyl) and
                                // collapsing those to 0 made every track of the
                                // disc overwrite the same slot on import.
                                let num = track["position"].as_i64()
                                    .or_else(|| track["number"].as_str()
                                        .and_then(|n| n.parse::<i64>().ok()))
                                    .unwrap_or(idx as i64 + 1);
                                // Duration from recording.length (milliseconds)
                                let duration_ms = track["recording"]["length"].as_i64()
                                    .or_else(|| track["length"].as_i64());
                                // First ISRC from the recording
                                let isrc = track["recording"]["isrcs"].as_array()
                                    .and_then(|arr| arr.first())
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                // Recording MusicBrainz ID for direct URL lookup
                                let recording_mbid = track["recording"]["id"].as_str()
                                    .map(|s| s.to_string());
                                if !title.is_empty() {
                                    tracks.push((title, disc, num, duration_ms, isrc, recording_mbid));
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
        let (album_id, album_title) = {
            let conn = crate::db::lock(&db)?;
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
            let album_title = album_title_from_rg
                .or_else(|| rg_title.clone())
                .unwrap_or_else(|| "Unknown Album".to_string());
            let aid = crate::db::albums::find_or_create(
                &conn,
                &album_title,
                Some(artist_id),
                Some(&artist_name),
                None,
            ).map_err(|e| e.to_string())?;
            // Set the MusicBrainz ID on the album. Store the RELEASE-GROUP mbid
            // (the loop parameter), not the release id — "missing albums"
            // detection compares against release-group MBIDs from the
            // discography, so a release id here can never match and the album
            // would be reported missing (and re-downloaded) forever.
            let _ = conn.execute(
                "UPDATE albums SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL",
                params![mbid, aid],
            );
            // Store enriched tracklist
            let tracklist_json: Vec<serde_json::Value> = tracks.iter().map(|(title, disc, num, dur, _isrc, _mbid)| {
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
            (aid, album_title)
        };

        // Queue downloads for each track using ytmsearch5 for better matching
        for (title, disc, num, duration_ms, isrc, recording_mbid) in &tracks {
            let query = format!("{} - {}", artist_name, title);
            let search_url = format!("ytsearch5:{}", query);
            let download = {
                let conn = crate::db::lock(&db)?;
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
                    Some(&album_title),
                    recording_mbid.as_deref(),
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
        let conn = crate::db::lock(&db)?;
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
        let conn = crate::db::lock(&db)?;
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
        let conn = crate::db::lock(&db)?;
        conn.execute(
            "UPDATE tracks SET music_video_path = ?1 WHERE id = ?2",
            params![path, track_id],
        ).map_err(|e| e.to_string())?;
    }

    Ok(path)
}
