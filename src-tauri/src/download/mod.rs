pub mod deezer;
pub mod metadata;
pub mod setup;
pub mod source;
pub mod spotify;
pub mod url_parser;
pub mod ytdlp;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::sync::{Mutex, Semaphore};

use crate::db::DbPool;
use crate::metadata::tags;

#[derive(Debug, Serialize, Clone)]
pub struct DownloadEvent {
    pub id: i64,
    pub status: String,
    pub progress: f64,
    pub speed: Option<String>,
    pub eta: Option<String>,
    pub error: Option<String>,
    pub title: Option<String>,
    pub track_id: Option<i64>,
}

pub struct DownloadManager {
    /// Dedicated DB connection for downloads — avoids blocking the main app connection.
    db: Arc<DbPool>,
    app_handle: tauri::AppHandle,
    active_tasks: Arc<Mutex<HashMap<i64, tokio::task::JoinHandle<()>>>>,
    concurrency: Arc<Semaphore>,
    /// Pluggable audio sources (Spotify, Deezer, etc.) for direct platform downloads
    pub(crate) sources: Arc<tokio::sync::RwLock<source::SourceRegistry>>,
    /// Tracks YouTube URLs already used per album to prevent duplicate downloads
    used_urls_by_album: Arc<tokio::sync::RwLock<HashMap<i64, HashSet<String>>>>,
}

impl DownloadManager {
    pub fn new(db: Arc<DbPool>, app_handle: tauri::AppHandle) -> Self {
        let sources = source::SourceRegistry::from_settings(&db);
        Self {
            db,
            app_handle,
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            concurrency: Arc::new(Semaphore::new(2)),
            sources: Arc::new(tokio::sync::RwLock::new(sources)),
            used_urls_by_album: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
        }
    }

    /// Rebuild sources from current settings (call after credentials change)
    pub async fn refresh_sources(&self) {
        let new_sources = source::SourceRegistry::from_settings(&self.db);
        let mut guard = self.sources.write().await;
        *guard = new_sources;
        log::info!("Download sources refreshed");
    }

    /// Get status of all configured sources
    pub async fn get_sources_status(&self) -> Vec<source::SourceStatus> {
        let guard = self.sources.read().await;
        guard.get_statuses(&self.db)
    }

    /// Test a specific source's connection
    pub async fn test_source(&self, platform: &str) -> Result<(), source::SourceError> {
        let guard = self.sources.read().await;
        guard.test_source(platform).await
    }

    fn get_download_dir(&self) -> Result<PathBuf, String> {
        let conn = crate::db::lock(&self.db)?;
        let dir = crate::db::settings::get_setting(&conn, "download_dir")
            .ok()
            .flatten();
        match dir {
            Some(d) if !d.is_empty() => Ok(PathBuf::from(d)),
            _ => {
                let app_dir = self
                    .app_handle
                    .path()
                    .app_data_dir()
                    .unwrap_or_else(|_| PathBuf::from("."));
                Ok(app_dir.join("downloads"))
            }
        }
    }

    fn get_bin_dir(&self) -> PathBuf {
        setup::get_bin_dir(&self.app_handle)
    }

    /// Resolve the yt-dlp binary path (local bin or PATH fallback)
    fn resolve_ytdlp(&self) -> String {
        let bin_dir = self.get_bin_dir();
        setup::resolve_ytdlp(&bin_dir).unwrap_or_else(|| "yt-dlp".to_string())
    }

    /// Resolve the ffmpeg directory (local bin or None for PATH)
    fn resolve_ffmpeg_dir(&self) -> Option<String> {
        let bin_dir = self.get_bin_dir();
        setup::resolve_ffmpeg_dir(&bin_dir)
    }

    /// Get the cookies-from-browser setting (e.g. "chrome", "firefox", "edge").
    /// Defaults to "chrome" to avoid bot detection on YouTube.
    fn get_cookies_from_browser(&self) -> Option<String> {
        let conn = match self.db.lock() {
            Ok(c) => c,
            Err(e) => {
                log::error!("DB mutex poisoned in get_cookies_from_browser: {}", e);
                return None;
            }
        };
        crate::db::settings::get_cookies_browser(&conn)
    }

    /// Resume any downloads that were interrupted by an app shutdown.
    /// Resets 'downloading'/'processing' back to 'queued', then restarts all queued downloads.
    /// Must be called after the Tokio runtime is available (i.e., not during sync `setup()`).
    pub fn resume_interrupted(self: &Arc<Self>) {
        let ids: Vec<i64> = {
            let conn = match self.db.lock() {
                Ok(c) => c,
                Err(_) => return,
            };
            // Reset in-progress downloads so they restart cleanly
            let _ = conn.execute(
                "UPDATE downloads SET status = 'queued', progress = 0, error_message = NULL
                 WHERE status IN ('downloading', 'processing')",
                [],
            );
            let mut stmt = match conn
                .prepare("SELECT id FROM downloads WHERE status = 'queued' ORDER BY created_at ASC")
            {
                Ok(s) => s,
                Err(_) => return,
            };
            let result: Vec<i64> = match stmt.query_map([], |row| row.get(0)) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(e) => {
                    log::error!("Failed to query interrupted downloads: {}", e);
                    return;
                }
            };
            result
        };
        if !ids.is_empty() {
            log::info!("Resuming {} interrupted download(s)", ids.len());
            let mgr = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                for id in ids {
                    mgr.start_download(id, None);
                }
            });
        }
    }

    pub fn start_download(&self, download_id: i64, title: Option<String>) {
        let db = self.db.clone();
        let app_handle = self.app_handle.clone();
        let active_tasks = self.active_tasks.clone();
        let download_dir = match self.get_download_dir() {
            Ok(d) => d,
            Err(e) => {
                log::error!("[download] {}", e);
                emit_event(&self.app_handle, download_id, "error", 0.0, Some(e), None, None, title);
                return;
            }
        };
        let ytdlp_binary = self.resolve_ytdlp();
        let ffmpeg_dir = self.resolve_ffmpeg_dir();
        log::info!("[download] id={} ffmpeg_dir={:?} ytdlp={}", download_id, ffmpeg_dir, ytdlp_binary);
        let cookies_from_browser = self.get_cookies_from_browser();
        let semaphore = self.concurrency.clone();
        let sources = self.sources.clone();
        let used_urls_by_album = self.used_urls_by_album.clone();

        // Emit queued event immediately so the frontend can show all pending downloads
        emit_event(&self.app_handle, download_id, "queued", 0.0, None, None, None, title);

        let active_tasks_insert = self.active_tasks.clone();
        tokio::spawn(async move {
            let handle = tokio::spawn(async move {
                // Wait for a concurrency slot (limits parallel yt-dlp processes)
                let _permit = match semaphore.acquire().await {
                    Ok(p) => p,
                    Err(e) => {
                        log::error!("Download semaphore closed unexpectedly: {}", e);
                        return;
                    }
                };
                run_download(db, app_handle, download_id, download_dir, ytdlp_binary, ffmpeg_dir, cookies_from_browser, sources, used_urls_by_album)
                    .await;
                active_tasks.lock().await.remove(&download_id);
            });
            active_tasks_insert.lock().await.insert(download_id, handle);
        });
    }

    pub async fn cancel_download(&self, download_id: i64) {
        let mut tasks = self.active_tasks.lock().await;
        if let Some(handle) = tasks.remove(&download_id) {
            handle.abort();
        }
        if let Ok(conn) = self.db.lock() {
            let _ = crate::db::downloads::cancel_download(&conn, download_id);
        }
        let _ = self.app_handle.emit(
            "download-event",
            DownloadEvent {
                id: download_id,
                status: "cancelled".into(),
                progress: 0.0,
                speed: None,
                eta: None,
                error: None,
                title: None,
                track_id: None,
            },
        );
    }

    /// Cancel **all** active downloads. Called when the library is reset or the app exits.
    pub async fn cancel_all(&self) {
        let mut tasks = self.active_tasks.lock().await;
        for (id, handle) in tasks.drain() {
            handle.abort();
            let _ = self.app_handle.emit(
                "download-event",
                DownloadEvent {
                    id,
                    status: "cancelled".into(),
                    progress: 0.0,
                    speed: None,
                    eta: None,
                    error: None,
                    title: None,
                    track_id: None,
                },
            );
        }
        // Mark all in-progress downloads as cancelled in the DB and emit events for each
        if let Ok(conn) = self.db.lock() {
            // Collect IDs before updating so we can emit events
            let ids: Vec<i64> = {
                let mut stmt = match conn.prepare(
                    "SELECT id FROM downloads WHERE status IN ('queued', 'downloading', 'processing')"
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        log::error!("Failed to prepare cancel_all query: {}", e);
                        return;
                    }
                };
                let result: Vec<i64> = match stmt.query_map([], |row| row.get(0)) {
                    Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                    Err(e) => {
                        log::error!("Failed to query downloads for cancellation: {}", e);
                        return;
                    }
                };
                result
            };
            let _ = conn.execute(
                "UPDATE downloads SET status = 'cancelled' WHERE status IN ('queued', 'downloading', 'processing')",
                [],
            );
            // Emit cancel events for downloads that weren't in active_tasks
            // (e.g. queued but waiting for semaphore)
            for id in ids {
                let _ = self.app_handle.emit(
                    "download-event",
                    DownloadEvent {
                        id,
                        status: "cancelled".into(),
                        progress: 0.0,
                        speed: None,
                        eta: None,
                        error: None,
                        title: None,
                        track_id: None,
                    },
                );
            }
        }
    }
}

/// Update a download's status and its linked monitored entry in one step.
/// Emits a manager-entry-updated event if there's a linked entry.
fn update_download_status_with_entry(
    db: &Arc<DbPool>,
    app_handle: &tauri::AppHandle,
    download_id: i64,
    status: &str,
) {
    if let Ok(conn) = db.lock() {
        let _ = crate::db::downloads::update_download_status(&conn, download_id, status, None);
        let eid: Option<i64> = conn.query_row(
            "SELECT id FROM monitored_playlist_entries WHERE download_id = ?1",
            rusqlite::params![download_id],
            |row| row.get::<_, i64>(0),
        ).ok();
        if let Some(eid) = eid {
            let _ = crate::db::monitored::update_entry_status(&conn, eid, status, Some(download_id), None);
            let _ = app_handle.emit(
                "manager-entry-updated",
                serde_json::json!({ "entry_id": eid, "status": status }),
            );
        }
    }
}

/// Build a search query string from a download's title and artist.
fn build_download_search_query(download: &crate::db::models::Download) -> Option<String> {
    match (&download.artist, &download.title) {
        (Some(a), Some(t)) => Some(format!("{} - {}", a, t)),
        (None, Some(t)) => Some(t.clone()),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
async fn run_download(
    db: Arc<DbPool>,
    app_handle: tauri::AppHandle,
    download_id: i64,
    download_dir: PathBuf,
    ytdlp_binary: String,
    ffmpeg_dir: Option<String>,
    cookies_from_browser: Option<String>,
    sources: Arc<tokio::sync::RwLock<source::SourceRegistry>>,
    used_urls_by_album: Arc<tokio::sync::RwLock<HashMap<i64, HashSet<String>>>>,
) {
    let mut download = {
        let conn = match db.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        match crate::db::downloads::get_download(&conn, download_id) {
            Ok(Some(d)) => d,
            _ => return,
        }
    };

    log::info!("[download] id={} platform='{}' url='{}' title={:?}", download_id, download.platform, download.url, download.title);

    // Convert Spotify URLs to YouTube search on-the-fly for legacy downloads
    if download.url.starts_with("https://open.spotify.com/") || download.url.starts_with("spotify:") {
        log::info!("Converting legacy Spotify URL to YouTube search for download {}", download_id);
        match spotify::fetch_track_metadata(&download.url).await {
            Some((title, artist)) => {
                let search_query = match artist {
                    Some(ref a) => format!("{} - {}", a, title),
                    None => title.clone(),
                };
                download.url = format!("ytsearch5:{}", search_query);
                if let Ok(conn) = db.lock() {
                    let _ = conn.execute(
                        "UPDATE downloads SET url = ?1, title = ?2, artist = ?3 WHERE id = ?4",
                        rusqlite::params![&download.url, &title, artist.as_deref(), download_id],
                    );
                }
            }
            None => {
                fail_download(&db, &app_handle, download_id, "Failed to fetch Spotify metadata for conversion");
                return;
            }
        }
    }

    if let Err(e) = std::fs::create_dir_all(&download_dir) {
        fail_download(&db, &app_handle, download_id, &format!("Failed to create download dir: {}", e));
        return;
    }

    // Update status to downloading and notify linked monitored entry
    update_download_status_with_entry(&db, &app_handle, download_id, "downloading");
    emit_event(&app_handle, download_id, "downloading", 0.0, None, None, None, None);

    let file_stem = format!("dl_{}", download_id);
    let platform = download.platform.clone();
    let drm_platforms = ["spotify", "apple_music", "tidal", "deezer", "amazon_music"];
    let is_drm_platform = drm_platforms.contains(&platform.as_str());
    let is_search_url = download.url.starts_with("ytsearch") || download.url.starts_with("ytmsearch") || download.url.starts_with("scsearch");

    // --- Phase 1: Fast direct download ---
    // For DRM platforms, try native source first (Deezer etc.) — it's a fast direct download.
    // For direct URLs (YouTube, SoundCloud, etc.), try yt-dlp first.
    let mut errors: Vec<String> = Vec::new();
    let mut failed_urls: HashSet<String> = HashSet::new();
    let mut ytdlp_info: Option<ytdlp::VideoInfo> = None;

    // Try native source FIRST for DRM platforms (fast, no searching)
    if is_drm_platform {
        let native_result = {
            let sources_guard = sources.read().await;
            if let Some(src) = sources_guard.get_for_platform(&platform) {
                log::info!("Trying native {} source for download {} (fast path)", platform, download_id);
                let app_handle_native = app_handle.clone();
                let dl_id_native = download_id;
                Some(src.download(
                    &download.url,
                    &download_dir,
                    &file_stem,
                    &download.format,
                    Box::new(move |pct| {
                        emit_event(&app_handle_native, dl_id_native, "downloading", pct, None, None, None, None);
                    }),
                ).await)
            } else {
                // Try cross-platform search with title/artist (e.g. Deezer search for Spotify tracks)
                let query = build_download_search_query(&download);
                if let Some(ref q) = query {
                    if let Some(src) = sources_guard.get_best_search_source() {
                        log::info!("Trying {} search for download {} (fast path): {}", src.platform(), download_id, q);
                        let app_handle_search = app_handle.clone();
                        let dl_id_search = download_id;
                        Some(src.search_download(
                            q,
                            &download_dir,
                            &file_stem,
                            &download.format,
                            Box::new(move |pct| {
                                emit_event(&app_handle_search, dl_id_search, "downloading", pct, None, None, None, None);
                            }),
                        ).await)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        if let Some(Ok(file_path)) = native_result {
            handle_download_success(&db, &app_handle, download_id, &file_path, &None).await;
            return;
        }
        if let Some(Err(native_err)) = native_result {
            errors.push(format!("native {}: {}", platform, native_err));
        }
    }

    // Try yt-dlp direct download (fast for YouTube/SoundCloud/Bandcamp URLs, skipped if search URL on DRM platform)
    let skip_ytdlp_initial = is_drm_platform && is_search_url;
    let ytdlp_success = if !skip_ytdlp_initial {
        if is_search_url {
            // Smart search with cascading resolver: try deterministic lookups first,
            // then fall back to search with improved scoring and album-level dedup.
            // Works with or without target_duration_ms (title matching is always used).
            let expected_secs = download.target_duration_ms.map(|ms| ms as f64 / 1000.0);
            // Build search query using primary artist only (first artist before comma/&/feat)
            // to avoid polluting search with too many featured artists
            let primary_artist = download.artist.as_deref().map(|a| {
                a.split(&[',', '&'][..])
                    .next()
                    .unwrap_or(a)
                    .replace("feat.", "")
                    .replace("ft.", "")
                    .trim()
                    .to_string()
            });
            let search_query = match (&primary_artist, &download.title) {
                (Some(a), Some(t)) => format!("{} - {}", a, t),
                (_, Some(t)) => t.clone(),
                _ => download.url
                    .trim_start_matches("ytmsearch5:")
                    .trim_start_matches("ytmsearch1:")
                    .trim_start_matches("ytsearch5:")
                    .trim_start_matches("ytmsearch1:")
                    .trim_start_matches("ytsearch1:")
                    .to_string(),
            };
            let is_album_track = download.target_album_id.is_some();

            // Get URLs already used by other tracks in this album
            let album_used_urls: HashSet<String> = if let Some(album_id) = download.target_album_id {
                let guard = used_urls_by_album.read().await;
                guard.get(&album_id).cloned().unwrap_or_default()
            } else {
                HashSet::new()
            };

            let mut best_url: Option<String> = None;

            // --- Layer 1: MusicBrainz URL relationships (direct YouTube link) ---
            if best_url.is_none() {
                if let Some(ref mbid) = download.target_recording_mbid {
                    log::info!("[download] id={} trying MusicBrainz URL rels for recording {}", download_id, mbid);
                    if let Some(url) = crate::metadata::musicbrainz::lookup_music_video_url(mbid).await {
                        if !album_used_urls.contains(&url) {
                            log::info!("[download] id={} MusicBrainz resolved → {}", download_id, url);
                            best_url = Some(url);
                        } else {
                            log::info!("[download] id={} MusicBrainz URL already used by another album track, skipping", download_id);
                        }
                    }
                }
            }

            // --- Layer 2: SongLink/Odesli ISRC → YouTube URL ---
            if best_url.is_none() {
                if let Some(ref isrc) = download.target_isrc {
                    log::info!("[download] id={} trying SongLink ISRC lookup: {}", download_id, isrc);
                    if let Some(url) = crate::metadata::songlink::resolve_isrc(isrc).await {
                        if !album_used_urls.contains(&url) {
                            log::info!("[download] id={} SongLink resolved → {}", download_id, url);
                            best_url = Some(url);
                        } else {
                            log::info!("[download] id={} SongLink URL already used by another album track, skipping", download_id);
                        }
                    }
                }
            }

            // --- Layer 3: yt-dlp search with improved matching ---
            if best_url.is_none() {
                let search_variations = build_search_variations(
                    &search_query,
                    download.target_isrc.as_deref(),
                    download.target_album_name.as_deref(),
                );

                for variation in &search_variations {
                    match ytdlp::get_search_results(
                        &ytdlp_binary,
                        ffmpeg_dir.as_deref(),
                        variation,
                        cookies_from_browser.as_deref(),
                    ).await {
                        Ok(results) if !results.is_empty() => {
                            if let Some(url) = pick_best_match(&results, expected_secs, Some(&search_query), is_album_track, &album_used_urls) {
                                log::info!("[download] id={} search match found via '{}': {}", download_id, variation, url);
                                best_url = Some(url);
                                break;
                            }
                        }
                        Ok(_) => {
                            log::warn!("Search variation '{}' returned no results", variation);
                        }
                        Err(e) => {
                            log::warn!("Search variation '{}' failed: {}", variation, e);
                        }
                    }
                }
            }

            if let Some(ref url) = best_url {
                // Register this URL as used for album dedup
                if let Some(album_id) = download.target_album_id {
                    let mut guard = used_urls_by_album.write().await;
                    guard.entry(album_id).or_default().insert(url.clone());
                }

                let app_handle_progress = app_handle.clone();
                let dl_id = download_id;
                let result = ytdlp::download_audio(
                    &ytdlp_binary,
                    ffmpeg_dir.as_deref(),
                    url,
                    &download_dir,
                    &download.format,
                    &download.quality,
                    &file_stem,
                    cookies_from_browser.as_deref(),
                    move |progress| {
                        emit_event(&app_handle_progress, dl_id, "downloading", progress.percent, progress.speed.clone(), progress.eta.clone(), None, None);
                    },
                ).await;
                match result {
                    Ok(file_path) => {
                        handle_download_success(&db, &app_handle, download_id, &file_path, &None).await;
                        return;
                    }
                    Err(e) => {
                        log::warn!("[download] id={} smart match download FAILED: {}", download_id, e);
                        errors.push(format!("smart-match: {}", e));
                        failed_urls.insert(url.clone());
                        false
                    }
                }
            } else {
                log::warn!("[download] id={} no smart match found for '{}'", download_id, search_query);
                errors.push("no matching result found by duration/title".to_string());
                false
            }
        } else {
            // Original path: search URLs without duration or direct URLs
            if is_search_url {
                ytdlp_info = match ytdlp::get_info(
                    &ytdlp_binary,
                    ffmpeg_dir.as_deref(),
                    &download.url,
                    cookies_from_browser.as_deref(),
                ).await {
                    Ok(info) => {
                        let best_title = info.track.as_deref().unwrap_or(&info.title);
                        let best_artist = info.artist.as_deref().or(info.uploader.as_deref());
                        if let Ok(conn) = db.lock() {
                            let _ = crate::db::downloads::update_download_title(&conn, download_id, best_title, best_artist);
                        }
                        emit_event(&app_handle, download_id, "downloading", 0.0, None, None, None, Some(best_title.to_string()));
                        Some(info)
                    }
                    Err(e) => {
                        log::warn!("Could not fetch metadata for download {}: {}", download_id, e);
                        None
                    }
                };
            }

            let app_handle_progress = app_handle.clone();
            let dl_id = download_id;
            let result = ytdlp::download_audio(
                &ytdlp_binary,
                ffmpeg_dir.as_deref(),
                &download.url,
                &download_dir,
                &download.format,
                &download.quality,
                &file_stem,
                cookies_from_browser.as_deref(),
                move |progress| {
                    emit_event(&app_handle_progress, dl_id, "downloading", progress.percent, progress.speed.clone(), progress.eta.clone(), None, None);
                },
            ).await;

            match result {
                Ok(file_path) => {
                    handle_download_success(&db, &app_handle, download_id, &file_path, &ytdlp_info).await;
                    return;
                }
                Err(e) => {
                    log::warn!("[download] id={} yt-dlp direct download FAILED: {}", download_id, e);
                    errors.push(format!("yt-dlp: {}", e));
                    failed_urls.insert(download.url.clone());
                    false
                }
            }
        }
    } else {
        log::info!("Skipping yt-dlp search for DRM download {} (will use search fallback)", download_id);
        false
    };

    // --- Phase 2: All fast methods failed — search fallbacks for this track ---
    if !ytdlp_success {
                // Check if the track already exists in the library
                let library_match = {
                    let search_query = if let Some(ref info) = ytdlp_info {
                        let title = info.track.as_deref().unwrap_or(&info.title);
                        let artist = info.artist.as_deref().or(info.uploader.as_deref());
                        match artist {
                            Some(a) => Some(format!("{} {}", a, title)),
                            None => Some(title.to_string()),
                        }
                    } else if let Ok(conn) = db.lock() {
                        conn.query_row(
                            "SELECT title, artist FROM downloads WHERE id = ?1",
                            rusqlite::params![download_id],
                            |row| {
                                let t: Option<String> = row.get(0)?;
                                let a: Option<String> = row.get(1)?;
                                Ok(match (t, a) {
                                    (Some(t), Some(a)) => Some(format!("{} {}", a, t)),
                                    (Some(t), None) => Some(t),
                                    _ => None,
                                })
                            },
                        ).ok().flatten()
                    } else {
                        None
                    };

                    if let Some(ref q) = search_query {
                        if let Ok(conn) = db.lock() {
                            crate::db::tracks::search_tracks_fts(&conn, q, 1)
                                .ok()
                                .and_then(|tracks| tracks.into_iter().next())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                };

                if let Some(track) = library_match {
                    log::info!("Download {} matched existing library track {} ('{}')", download_id, track.id, track.title);
                    let entry_id = if let Ok(conn) = db.lock() {
                        let _ = conn.execute_batch("BEGIN");
                        let _ = crate::db::downloads::update_download_file(&conn, download_id, &track.file_path, Some(track.id));
                        let _ = crate::db::downloads::update_download_status(&conn, download_id, "completed", None);
                        let eid: Option<i64> = conn.query_row(
                            "SELECT id FROM monitored_playlist_entries WHERE download_id = ?1",
                            rusqlite::params![download_id],
                            |row| row.get(0),
                        ).ok();
                        if let Some(eid) = eid {
                            let _ = crate::db::monitored::update_entry_status(&conn, eid, "downloaded", Some(download_id), Some(track.id));
                        }
                        let _ = conn.execute_batch("COMMIT");
                        eid
                    } else {
                        None
                    };
                    emit_event(&app_handle, download_id, "completed", 100.0, None, None, None, Some(track.title.clone()));
                    if let Some(eid) = entry_id {
                        let _ = app_handle.emit("manager-entry-updated", serde_json::json!({
                            "entry_id": eid, "status": "downloaded", "track_id": track.id
                        }));
                    }
                    let _ = app_handle.emit("library-changed", ());
                    return;
                }

                // Cross-platform search via best available source (e.g. Deezer search)
                let search_query = if let Some(ref info) = ytdlp_info {
                    let title = info.track.as_deref().unwrap_or(&info.title);
                    let artist = info.artist.as_deref().or(info.uploader.as_deref());
                    match artist {
                        Some(a) => Some(format!("{} - {}", a, title)),
                        None => Some(title.to_string()),
                    }
                } else {
                    // Use download title/artist from DB
                    if let Ok(conn) = db.lock() {
                        conn.query_row(
                            "SELECT title, artist FROM downloads WHERE id = ?1",
                            rusqlite::params![download_id],
                            |row| {
                                let t: Option<String> = row.get(0)?;
                                let a: Option<String> = row.get(1)?;
                                Ok(match (t, a) {
                                    (Some(t), Some(a)) => Some(format!("{} - {}", a, t)),
                                    (Some(t), None) => Some(t),
                                    _ => None,
                                })
                            },
                        ).ok().flatten()
                    } else {
                        None
                    }
                };

                // If no search query yet (yt-dlp metadata failed), try platform-specific metadata APIs
                let search_query = if search_query.is_none() && is_drm_platform {
                    log::info!("yt-dlp metadata failed for {} URL, trying platform API for download {}", platform, download_id);
                    if let Some(meta) = metadata::fetch_track_metadata(&download.url, &platform).await {
                        // Update DB with the metadata we found
                        if let Ok(conn) = db.lock() {
                            let _ = crate::db::downloads::update_download_title(
                                &conn,
                                download_id,
                                &meta.title,
                                meta.artist.as_deref(),
                            );
                        }
                        emit_event(&app_handle, download_id, "downloading", 0.0, None, None, None, Some(meta.title.clone()));
                        Some(metadata::build_search_query(&meta))
                    } else {
                        log::warn!("Platform metadata API also failed for {} download {}", platform, download_id);
                        None
                    }
                } else {
                    search_query
                };

                if let Some(ref query) = search_query {
                    let cross_result = if is_drm_platform {
                        let sources_guard = sources.read().await;
                        if let Some(src) = sources_guard.get_best_search_source() {
                            // Don't search on the same platform that just failed natively
                            if src.platform() != platform {
                                log::info!("Trying cross-platform search on {} for: {}", src.platform(), query);
                                let app_handle_cross = app_handle.clone();
                                let dl_id_cross = download_id;
                                Some(src.search_download(
                                    query,
                                    &download_dir,
                                    &file_stem,
                                    &download.format,
                                    Box::new(move |pct| {
                                        emit_event(&app_handle_cross, dl_id_cross, "downloading", pct, None, None, None, None);
                                    }),
                                ).await)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(Ok(file_path)) = cross_result {
                        handle_download_success(&db, &app_handle, download_id, &file_path, &ytdlp_info).await;
                        return;
                    }
                    if let Some(Err(cross_err)) = cross_result {
                        errors.push(format!("cross-search: {}", cross_err));
                    }
                }

                // Get expected duration and ISRC for search matching (used by YouTube + SoundCloud)
                let mut expected_duration_secs = ytdlp_info.as_ref().and_then(|i| i.duration)
                    .or_else(|| download.target_duration_ms.map(|ms| ms as f64 / 1000.0))
                    .or_else(|| {
                        if let Ok(conn) = db.lock() {
                            conn.query_row(
                                "SELECT e.duration_seconds FROM monitored_playlist_entries e WHERE e.download_id = ?1",
                                rusqlite::params![download_id],
                                |row| row.get::<_, Option<f64>>(0),
                            ).ok().flatten()
                            // Safety: normalize any legacy ms values still stored in the column
                            .map(|d| if d > 36_000.0 { d / 1000.0 } else { d })
                        } else {
                            None
                        }
                    });

                // Get ISRC for precise matching (from yt-dlp info, download record, or monitored entry)
                let mut isrc = ytdlp_info.as_ref().and_then(|i| i.isrc.clone())
                    .or_else(|| {
                        if let Ok(conn) = db.lock() {
                            conn.query_row(
                                "SELECT target_isrc FROM downloads WHERE id = ?1",
                                rusqlite::params![download_id],
                                |row| row.get::<_, Option<String>>(0),
                            ).ok().flatten()
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        if let Ok(conn) = db.lock() {
                            conn.query_row(
                                "SELECT e.isrc FROM monitored_playlist_entries e WHERE e.download_id = ?1",
                                rusqlite::params![download_id],
                                |row| row.get::<_, Option<String>>(0),
                            ).ok().flatten()
                        } else {
                            None
                        }
                    });

                // If no ISRC yet, try Deezer's public API to find one (no auth required)
                if let Some(ref query) = search_query {
                    if isrc.is_none() {
                        log::info!("No ISRC available, trying Deezer public API for: {}", query);
                        if let Some(deezer_meta) = deezer::search_public_metadata(query).await {
                            if let Some(ref deezer_isrc) = deezer_meta.isrc {
                                log::info!("Deezer public API found ISRC: {}", deezer_isrc);
                                isrc = Some(deezer_isrc.clone());
                            }
                            if expected_duration_secs.is_none() {
                                if let Some(dur) = deezer_meta.duration {
                                    log::info!("Using Deezer duration for matching: {:.0}s", dur);
                                    expected_duration_secs = Some(dur);
                                }
                            }
                        }
                    }
                }

                // Step 3: YouTube search fallback with duration validation
                if let Some(ref query) = search_query {
                    // Build search queries, using ytsearch5: to get multiple results for duration matching
                    let query_variations = build_search_variations(query, isrc.as_deref(), None);

                    for (attempt, yt_query) in query_variations.iter().enumerate() {
                        // Replace ytsearch1: with ytsearch5: to get multiple candidates
                        let info_query = yt_query
                            .replace("ytsearch1:", "ytsearch5:")
                            .replace("ytmsearch1:", "ytsearch5:")
                            .replace("scsearch1:", "scsearch5:");

                        log::info!("YouTube search attempt {}: searching '{}'", attempt + 1, info_query);

                        // First, search and get info for duration matching
                        match ytdlp::search_info(
                            &ytdlp_binary,
                            ffmpeg_dir.as_deref(),
                            &info_query,
                            cookies_from_browser.as_deref(),
                        ).await {
                            Ok(results) => {
                                if let Some(best_url) = pick_best_match(&results, expected_duration_secs, Some(query), false, &HashSet::new()) {
                                    if failed_urls.contains(&best_url) {
                                        log::info!("YouTube search attempt {}: skipping already-failed URL {}", attempt + 1, best_url);
                                        continue;
                                    }
                                    log::info!("YouTube search attempt {}: downloading best match from {}", attempt + 1, best_url);
                                    let app_handle_yt = app_handle.clone();
                                    let dl_id_yt = download_id;
                                    let yt_result = ytdlp::download_audio(
                                        &ytdlp_binary,
                                        ffmpeg_dir.as_deref(),
                                        &best_url,
                                        &download_dir,
                                        &download.format,
                                        &download.quality,
                                        &file_stem,
                                        cookies_from_browser.as_deref(),
                                        move |progress| {
                                            emit_event(&app_handle_yt, dl_id_yt, "downloading", progress.percent, progress.speed.clone(), progress.eta.clone(), None, None);
                                        },
                                    ).await;

                                    match yt_result {
                                        Ok(file_path) => {
                                            log::info!("YouTube search succeeded on attempt {}", attempt + 1);
                                            handle_download_success(&db, &app_handle, download_id, &file_path, &ytdlp_info).await;
                                            return;
                                        }
                                        Err(e) => {
                                            failed_urls.insert(best_url.clone());
                                            errors.push(format!("YouTube download attempt {}: {}", attempt + 1, e));
                                        }
                                    }
                                } else {
                                    errors.push(format!("YouTube search attempt {}: no duration-matching results", attempt + 1));
                                }
                            }
                            Err(e) => {
                                errors.push(format!("YouTube search attempt {}: {}", attempt + 1, e));
                                if e.contains("exit code: 1") || e.contains("exit code: 120") {
                                    let backoff_secs = 3 * 2u64.pow((attempt as u32).min(3));
                                    log::info!("Bot detection likely, waiting {} seconds before retry...", backoff_secs);
                                    tokio::time::sleep(std::time::Duration::from_secs(backoff_secs)).await;
                                }
                            }
                        }
                    }
                }

                // Step 4: SoundCloud search as final fallback (with duration matching)
                if let Some(ref query) = search_query {
                    let sc_info_query = format!("scsearch5:{}", query);
                    log::info!("Final fallback: searching SoundCloud for '{}'", sc_info_query);

                    // Try duration-matched search first (like YouTube)
                    let sc_best_url = match ytdlp::search_info(
                        &ytdlp_binary,
                        ffmpeg_dir.as_deref(),
                        &sc_info_query,
                        cookies_from_browser.as_deref(),
                    ).await {
                        Ok(results) => pick_best_match(&results, expected_duration_secs, Some(query), false, &HashSet::new()),
                        Err(_) => None,
                    };

                    // Only download from SoundCloud if we found a duration-matched result
                    // Don't blindly download the first SoundCloud result
                    let sc_download_url = match sc_best_url {
                        Some(url) => url,
                        None => {
                            log::info!("SoundCloud search: no duration-matched result, skipping blind download");
                            errors.push("SoundCloud search: no matching result".to_string());
                            // Skip to the "all fallbacks failed" handler below
                            fail_download(&db, &app_handle, download_id, &errors.join("; "));
                            return;
                        }
                    };

                    let app_handle_sc = app_handle.clone();
                    let dl_id_sc = download_id;
                    let sc_result = ytdlp::download_audio(
                        &ytdlp_binary,
                        ffmpeg_dir.as_deref(),
                        &sc_download_url,
                        &download_dir,
                        &download.format,
                        &download.quality,
                        &file_stem,
                        cookies_from_browser.as_deref(),
                        move |progress| {
                            emit_event(&app_handle_sc, dl_id_sc, "downloading", progress.percent, progress.speed.clone(), progress.eta.clone(), None, None);
                        },
                    )
                    .await;

                    match sc_result {
                        Ok(file_path) => {
                            log::info!("SoundCloud search succeeded");
                            handle_download_success(&db, &app_handle, download_id, &file_path, &ytdlp_info).await;
                            return;
                        }
                        Err(e) => {
                            errors.push(format!("SoundCloud search: {}", e));
                        }
                    }
                }

                // If all fallbacks failed, mark as failed
                fail_download(&db, &app_handle, download_id, &errors.join("; "));
    }
}

async fn handle_download_success(
    db: &Arc<DbPool>,
    app_handle: &tauri::AppHandle,
    download_id: i64,
    file_path: &str,
    ytdlp_info: &Option<ytdlp::VideoInfo>,
) {
    if let Ok(conn) = db.lock() {
        let _ = crate::db::downloads::update_download_status(
            &conn,
            download_id,
            "processing",
            None,
        );
    }
    emit_event(
        app_handle,
        download_id,
        "processing",
        100.0,
        None,
        None,
        None,
        None,
    );

    // Build fallback metadata from the download record + yt-dlp info
    let dl_meta = {
        let dl_row = if let Ok(conn) = db.lock() {
            conn.query_row(
                "SELECT title, artist, url, target_album_id, target_artist_id, target_isrc, target_disc_number, target_track_number, target_duration_ms FROM downloads WHERE id = ?1",
                rusqlite::params![download_id],
                |row| Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                )),
            ).ok()
        } else {
            None
        };
        let (title, artist, source_url, target_album_id, target_artist_id, target_isrc, target_disc_number, target_track_number, target_duration_ms) = match dl_row {
            Some((t, a, u, alb, art, isrc, disc, track, dur)) => (t, a, Some(u), alb, art, isrc, disc, track, dur),
            None => (None, None, None, None, None, None, None, None, None),
        };
        DownloadMeta {
            title,
            artist,
            album: ytdlp_info.as_ref().and_then(|i| i.album.clone()),
            source_url,
            description: ytdlp_info.as_ref().and_then(|i| i.description.clone()),
            genre: ytdlp_info.as_ref().and_then(|i| i.genre.clone()),
            release_year: ytdlp_info.as_ref().and_then(|i| i.release_year.clone()),
            language: ytdlp_info.as_ref().and_then(|i| i.language.clone()),
            composer: ytdlp_info.as_ref().and_then(|i| i.composer.clone()),
            tags: ytdlp_info.as_ref().and_then(|i| {
                i.tags.as_ref().map(|t| serde_json::to_string(t).unwrap_or_default())
            }),
            channel_url: ytdlp_info.as_ref().and_then(|i| i.channel_url.clone()),
            target_album_id,
            target_artist_id,
            isrc: ytdlp_info.as_ref().and_then(|i| i.isrc.clone()).or(target_isrc),
            target_disc_number,
            target_track_number,
            target_duration_ms,
        }
    };
    let track_id = import_downloaded_file(db, app_handle, file_path, &dl_meta).await;

    // Batch all post-completion DB updates in a single lock scope
    let entry_id = if let Ok(conn) = db.lock() {
        let _ = conn.execute_batch("BEGIN");
        let _ = crate::db::downloads::update_download_file(
            &conn,
            download_id,
            file_path,
            track_id,
        );
        let _ = crate::db::downloads::update_download_status(
            &conn,
            download_id,
            "completed",
            None,
        );
        let eid: Option<i64> = conn.query_row(
            "SELECT id FROM monitored_playlist_entries WHERE download_id = ?1",
            rusqlite::params![download_id],
            |row| row.get(0),
        ).ok();
        if let Some(eid) = eid {
            let _ = crate::db::monitored::update_entry_status(
                &conn, eid, "downloaded", Some(download_id), track_id,
            );
            // Auto-add downloaded track to the library playlist
            if let Some(tid) = track_id {
                // Find the playlist_id for this entry
                if let Ok(playlist_id) = conn.query_row(
                    "SELECT playlist_id FROM monitored_playlist_entries WHERE id = ?1",
                    rusqlite::params![eid],
                    |row| row.get::<_, i64>(0),
                ) {
                    let _ = crate::db::playlists::add_track_to_playlist(&conn, playlist_id, tid);
                }
            }
        }
        let _ = conn.execute_batch("COMMIT");
        eid
    } else {
        None
    };
    let _ = app_handle.emit(
        "download-event",
        DownloadEvent {
            id: download_id,
            status: "completed".into(),
            progress: 100.0,
            speed: None,
            eta: None,
            error: None,
            title: None,
            track_id,
        },
    );
    // Notify frontend that the library has new content
    if track_id.is_some() {
        let _ = app_handle.emit("library-updated", ());
    }
    if let Some(eid) = entry_id {
        let _ = app_handle.emit(
            "manager-entry-updated",
            serde_json::json!({ "entry_id": eid, "status": "downloaded" }),
        );
    }
}

/// Metadata from the download record + yt-dlp to use as fallback when file tags are missing
struct DownloadMeta {
    title: Option<String>,
    artist: Option<String>,
    album: Option<String>,
    source_url: Option<String>,
    description: Option<String>,
    genre: Option<String>,
    release_year: Option<String>,
    language: Option<String>,
    composer: Option<String>,
    tags: Option<String>,
    channel_url: Option<String>,
    target_album_id: Option<i64>,
    target_artist_id: Option<i64>,
    isrc: Option<String>,
    target_disc_number: Option<i64>,
    target_track_number: Option<i64>,
    target_duration_ms: Option<i64>,
}

async fn import_downloaded_file(
    db: &Arc<DbPool>,
    app_handle: &tauri::AppHandle,
    file_path: &str,
    dl_meta: &DownloadMeta,
) -> Option<i64> {
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        log::warn!("Downloaded file not found: {}", file_path);
        return None;
    }

    // Run sync I/O (tag reading, cover extraction) off the async runtime
    let path_buf = path.to_path_buf();
    let covers_dir = app_handle.path().app_data_dir().ok()?.join("covers");
    let covers_dir_clone = covers_dir.clone();
    let (tag_data, cover_art_path) = tokio::task::spawn_blocking(move || {
        // Single file read for both tags and cover art (halves memory usage)
        match tags::read_tags_and_cover(&path_buf, &covers_dir_clone) {
            Ok((data, cover)) => (data, cover),
            Err(e) => {
                log::warn!("Failed to read tags from downloaded file: {}", e);
                (tags::TagData::default(), None)
            }
        }
    }).await.ok()?;

    // Post-download verification: if this is an album track download with a known expected
    // duration, check the actual file duration against it. If they differ by more than 10
    // seconds, the downloaded file is likely the wrong track — demote it to a standalone
    // download so it doesn't get assigned to the album with incorrect metadata.
    let mut is_album_download = dl_meta.target_album_id.is_some();
    if is_album_download {
        if let (Some(expected_ms), Some(actual_ms)) = (dl_meta.target_duration_ms, tag_data.duration_ms) {
            let diff_ms = (expected_ms - actual_ms as i64).unsigned_abs();
            if diff_ms > 10_000 {
                log::warn!(
                    "Post-download duration mismatch for '{}': expected {}ms, got {}ms (diff {}ms). \
                     Demoting to standalone track — likely wrong YouTube result.",
                    dl_meta.title.as_deref().unwrap_or("?"), expected_ms, actual_ms, diff_ms
                );
                is_album_download = false;
            }
        }
    }

    // For album track downloads (target_album_id set), use the download metadata as the
    // authoritative source — we know the correct title/artist from MusicBrainz.
    // YouTube file tags contain the video title which is often wrong/noisy.
    // For other downloads, prefer file tags and fall back to download metadata.
    let raw_title = if is_album_download {
        dl_meta.title.clone()
            .or(tag_data.title)
            .unwrap_or_else(|| "Unknown".to_string())
    } else {
        tag_data.title
            .or_else(|| dl_meta.title.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    };
    let raw_artist = if is_album_download {
        dl_meta.artist.clone()
            .or(tag_data.artist)
    } else {
        tag_data.artist
            .or_else(|| dl_meta.artist.clone())
    };

    let (title, artist_name) = split_title_artist(&raw_title, raw_artist.as_deref());

    let conn = db.lock().ok()?;

    // Use target IDs from download context if available (e.g., downloading missing album tracks),
    // otherwise fall back to find_or_create from file tags
    let artist_id = if let Some(target_aid) = dl_meta.target_artist_id {
        Some(target_aid)
    } else {
        artist_name
            .as_ref()
            .and_then(|name| crate::db::artists::find_or_create(&conn, name).ok())
    };

    let album_id = if let Some(target_alb) = dl_meta.target_album_id {
        Some(target_alb)
    } else {
        // Try file tags first, then fall back to yt-dlp album metadata
        let album_name = tag_data.album.as_ref().or(dl_meta.album.as_ref());
        album_name.and_then(|alb| {
            crate::db::albums::find_or_create(
                &conn,
                alb,
                artist_id,
                tag_data.album_artist.as_deref(),
                tag_data.year.map(|y| y as i64),
            )
            .ok()
        })
    };

    let file_size = std::fs::metadata(path).map(|m| m.len() as i64).ok();

    // Save values needed after INSERT (before they get moved into params)
    let total_tracks_val = tag_data.total_tracks.map(|t| t as i64);
    let total_discs_val = tag_data.total_discs.map(|d| d as i64);
    let genre_for_album = tag_data.genre.clone();

    // Merge yt-dlp fallback fields with file tags (file tags win).
    // Filter out YouTube categories that aren't real music genres.
    let genre = tag_data.genre.or_else(|| dl_meta.genre.clone())
        .filter(|g| !is_youtube_category(g));
    let year = tag_data.year.map(|y| y as i64)
        .or_else(|| dl_meta.release_year.as_ref().and_then(|y| y.parse::<i64>().ok()));
    let description = dl_meta.description.clone();
    let language = dl_meta.language.clone();
    let composer = dl_meta.composer.clone();
    let release_date = dl_meta.release_year.clone();

    let tags = dl_meta.tags.clone();
    let isrc = dl_meta.isrc.clone();

    let track_number = if is_album_download { dl_meta.target_track_number.or(tag_data.track_number.map(|t| t as i64)) } else { tag_data.track_number.map(|t| t as i64).or(dl_meta.target_track_number) };
    let disc_number = if is_album_download { dl_meta.target_disc_number.or(tag_data.disc_number.map(|d| d as i64)) } else { tag_data.disc_number.map(|d| d as i64).or(dl_meta.target_disc_number) };

    // For album downloads, check if a track already exists at this position (e.g. a placeholder
    // or previously downloaded track). If so, update it in-place instead of creating a duplicate.
    let existing_track_id: Option<i64> = if is_album_download {
        if let (Some(alb_id), Some(tn)) = (album_id, track_number) {
            let dn = disc_number.unwrap_or(1);
            conn.query_row(
                "SELECT id FROM tracks WHERE album_id = ?1 AND track_number = ?2 AND disc_number = ?3",
                rusqlite::params![alb_id, tn, dn],
                |row| row.get(0),
            ).ok()
        } else {
            None
        }
    } else {
        None
    };

    let result = if let Some(existing_id) = existing_track_id {
        // Update the existing track row with the downloaded file's data
        conn.execute(
            "UPDATE tracks SET title = ?1, artist_id = ?2, album_artist = ?3, duration_ms = ?4,
                genre = ?5, year = ?6, file_path = ?7, file_size = ?8, format = ?9,
                bitrate = ?10, sample_rate = ?11, channels = ?12, cover_art_path = ?13,
                source_platform = 'download', source_url = ?14,
                description = ?15, language = ?16, composer = ?17, release_date = ?18, tags = ?19, isrc = ?20
             WHERE id = ?21",
            rusqlite::params![
                title,
                artist_id,
                tag_data.album_artist,
                tag_data.duration_ms.map(|d| d as i64),
                genre,
                year,
                file_path,
                file_size,
                tag_data.format,
                tag_data.bitrate.map(|b| b as i64),
                tag_data.sample_rate.map(|s| s as i64),
                tag_data.channels.map(|c| c as i64),
                cover_art_path,
                dl_meta.source_url,
                description,
                language,
                composer,
                release_date,
                tags,
                isrc,
                existing_id,
            ],
        ).map(|_| existing_id)
    } else {
        conn.execute(
            "INSERT INTO tracks (title, artist_id, album_id, album_artist, duration_ms,
                track_number, disc_number, genre, year, file_path, file_size, format,
                bitrate, sample_rate, channels, cover_art_path, source_platform, source_url,
                description, language, composer, release_date, tags, isrc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'download', ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
            rusqlite::params![
                title,
                artist_id,
                album_id,
                tag_data.album_artist,
                tag_data.duration_ms.map(|d| d as i64),
                track_number,
                disc_number,
                genre,
                year,
                file_path,
                file_size,
                tag_data.format,
                tag_data.bitrate.map(|b| b as i64),
                tag_data.sample_rate.map(|s| s as i64),
                tag_data.channels.map(|c| c as i64),
                cover_art_path,
                dl_meta.source_url,
                description,
                language,
                composer,
                release_date,
                tags,
                isrc,
            ],
        ).map(|_| conn.last_insert_rowid())
    };

    match result {
        Ok(track_id) => {
            let _ = crate::db::tracks::update_fts(&conn, track_id);
            let _ = crate::db::tracks::update_completeness(&conn, track_id);

            // Propagate channel_url to artist website_url
            if let Some(ref url) = dl_meta.channel_url {
                if let Some(aid) = artist_id {
                    let _ = conn.execute(
                        "UPDATE artists SET website_url = ?1 WHERE id = ?2 AND website_url IS NULL",
                        rusqlite::params![url, aid],
                    );
                }
            }

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

            log::info!(
                "Imported downloaded track: {} (id={})",
                title,
                track_id
            );
            Some(track_id)
        }
        Err(e) => {
            log::warn!("Failed to insert downloaded track: {}", e);
            None
        }
    }
}

fn fail_download(db: &Arc<DbPool>, app_handle: &tauri::AppHandle, id: i64, error: &str) {
    log::error!("Download {} failed: {}", id, error);
    let entry_id = if let Ok(conn) = db.lock() {
        let _ = crate::db::downloads::update_download_status(&conn, id, "failed", Some(error));
        // Update linked monitored entry
        let eid: Option<i64> = conn.query_row(
            "SELECT id FROM monitored_playlist_entries WHERE download_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        ).ok();
        if let Some(eid) = eid {
            let _ = crate::db::monitored::update_entry_status(&conn, eid, "failed", Some(id), None);
        }
        eid
    } else {
        None
    };
    let _ = app_handle.emit(
        "download-event",
        DownloadEvent {
            id,
            status: "failed".into(),
            progress: 0.0,
            speed: None,
            eta: None,
            error: Some(error.to_string()),
            title: None,
            track_id: None,
        },
    );
    if let Some(eid) = entry_id {
        let _ = app_handle.emit(
            "manager-entry-updated",
            serde_json::json!({ "entry_id": eid, "status": "failed" }),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn emit_event(
    app_handle: &tauri::AppHandle,
    id: i64,
    status: &str,
    progress: f64,
    speed: Option<String>,
    eta: Option<String>,
    error: Option<String>,
    title: Option<String>,
) {
    let _ = app_handle.emit(
        "download-event",
        DownloadEvent {
            id,
            status: status.into(),
            progress,
            speed,
            eta,
            error,
            title,
            track_id: None,
        },
    );
}

/// Check if a search result title is relevant to the expected query.
/// Returns true if at least some significant words from the query appear in the result title.
fn title_is_relevant(query: &str, result_title: &str) -> bool {
    let normalize = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .replace(['(', ')', '[', ']', '-', '/', '&', ',', '.', '\'', '"'], " ")
            .split_whitespace()
            .filter(|w| w.len() > 1) // skip single chars
            .filter(|w| !matches!(*w, "the" | "of" | "and" | "in" | "to" | "ft" | "feat" | "official" | "video" | "audio" | "lyrics"))
            .map(|w| w.to_string())
            .collect()
    };

    // If the query is "Artist - Title", focus on matching the Title part
    // since artist words match many tracks by the same artist
    if query.contains(" - ") {
        let parts: Vec<&str> = query.splitn(2, " - ").collect();
        if parts.len() == 2 {
            let title_part = parts[1].trim();
            let title_words_query = normalize(title_part);
            let title_words_result = normalize(result_title);

            if !title_words_query.is_empty() && !title_words_result.is_empty() {
                let matching = title_words_query.iter()
                    .filter(|qw| title_words_result.iter().any(|tw| tw.contains(qw.as_str()) || qw.contains(tw.as_str())))
                    .count();
                let relevance = matching as f64 / title_words_query.len() as f64;
                // At least 50% of the track title words must appear in the result
                return relevance >= 0.5;
            }
        }
    }

    // Fallback: full query matching
    let query_words = normalize(query);
    let title_words = normalize(result_title);

    if query_words.is_empty() || title_words.is_empty() {
        return true; // can't validate, allow
    }

    let matching = query_words.iter()
        .filter(|qw| title_words.iter().any(|tw| tw.contains(qw.as_str()) || qw.contains(tw.as_str())))
        .count();

    let relevance = matching as f64 / query_words.len() as f64;
    relevance >= 0.5
}

/// Check if a candidate duration is "in the ballpark" of the expected duration.
/// `strict` mode (for album tracks) uses ±5 seconds; normal mode uses 15% or 30 seconds.
fn duration_acceptable(expected_secs: f64, candidate_secs: f64, strict: bool) -> bool {
    if expected_secs <= 0.0 || candidate_secs <= 0.0 {
        return true; // Can't validate, allow it
    }
    let diff = (expected_secs - candidate_secs).abs();
    if strict {
        diff <= 5.0
    } else {
        let pct_threshold = expected_secs * 0.15;
        diff <= pct_threshold.max(30.0)
    }
}

/// Pick the best duration-matching result from a list of search results.
/// Returns the URL of the best match, or None if no acceptable match found.
/// When a search_query is provided, results with completely unrelated titles are rejected.
/// `strict_duration` enables tighter duration matching (±5s) for album tracks.
/// `exclude_urls` prevents selecting URLs already used by other tracks in the same album.
fn pick_best_match(
    results: &[ytdlp::VideoInfo],
    expected_duration_secs: Option<f64>,
    search_query: Option<&str>,
    strict_duration: bool,
    exclude_urls: &HashSet<String>,
) -> Option<String> {
    if results.is_empty() {
        return None;
    }

    let expected = expected_duration_secs.filter(|&d| d > 0.0);

    // Score all candidates and pick the best one
    let mut best: Option<(&ytdlp::VideoInfo, f64)> = None; // (result, score) — higher is better
    for result in results {
        // Skip URLs already used by other tracks in the same album
        if let Some(ref url) = result.webpage_url {
            if exclude_urls.contains(url) {
                log::info!("Skipping '{}' — URL already used by another album track", result.title);
                continue;
            }
        }

        // Title relevance check (always applied when we have a query)
        if let Some(query) = search_query {
            if !title_is_relevant(query, &result.title) {
                log::info!(
                    "Skipping '{}' — title not relevant to query '{}'",
                    result.title, query
                );
                continue;
            }
        }

        // Duration scoring
        let duration_score = if let Some(exp) = expected {
            let candidate_dur = match result.duration {
                Some(d) if d > 0.0 => d,
                _ => {
                    // No candidate duration — give a low score but don't skip
                    0.3
                }
            };
            if candidate_dur > 0.0 {
                if !duration_acceptable(exp, candidate_dur, strict_duration) {
                    log::info!(
                        "Skipping '{}' — duration {:.0}s vs expected {:.0}s (too different)",
                        result.title, candidate_dur, exp
                    );
                    continue;
                }
                // Score: 1.0 for exact match, decreasing with difference
                let diff = (exp - candidate_dur).abs();
                1.0 - (diff / exp).min(1.0)
            } else {
                0.3
            }
        } else {
            // No expected duration — all candidates get equal duration score
            0.5
        };

        if best.as_ref().map_or(true, |(_, prev_score)| duration_score > *prev_score) {
            best = Some((result, duration_score));
        }
    }

    if let Some((result, score)) = best {
        log::info!(
            "Best match: '{}' — score {:.2} (expected duration: {:?}s)",
            result.title, score, expected
        );
        result.webpage_url.clone()
    } else {
        log::warn!("No search results passed relevance/duration filters");
        None
    }
}

/// Build comprehensive search query variations for YouTube/SoundCloud fallback.
/// Returns a list of yt-dlp search queries to try in order.
/// When an ISRC is available, it's tried first on YouTube Music for precise matching.
/// When an album name is available, album-aware queries are added for better accuracy.
fn build_search_variations(query: &str, isrc: Option<&str>, album: Option<&str>) -> Vec<String> {
    let mut variations = Vec::new();

    // Clean the query: remove feat./ft., parentheses with remix/version info, etc.
    let clean_query = clean_search_query(query);

    // 0. ISRC-based search (most precise match possible)
    if let Some(isrc) = isrc {
        log::info!("ISRC available for search: {}", isrc);
        variations.push(format!("ytsearch5:{}", isrc));
    }

    // 1. Album-aware search (better for finding correct album version)
    if let Some(album_name) = album {
        variations.push(format!("ytsearch5:{} {}", query, album_name));
    }

    // 2. Primary search: Artist - Title
    variations.push(format!("ytsearch5:{}", query));

    // 3. If we have "Artist - Title" format, try title-focused variations
    if query.contains(" - ") {
        let parts: Vec<&str> = query.splitn(2, " - ").collect();
        if parts.len() == 2 {
            let artist = parts[0].trim();
            let title = parts[1].trim();

            // Title + artist (reversed order — sometimes more effective)
            variations.push(format!("ytsearch5:{} {}", title, artist));

            // Just the title (useful for obscure tracks)
            if title.split_whitespace().count() >= 2 {
                variations.push(format!("ytsearch5:{}", title));
            }

            // Artist + cleaned title (removes feat. etc)
            let clean_title = clean_search_query(title);
            if clean_title != title {
                variations.push(format!("ytsearch5:{} {}", artist, clean_title));
            }

            // Add "audio" suffix for official audio uploads
            variations.push(format!("ytsearch5:{} {} audio", artist, title));
        }
    }

    // 4. Query with " - " replaced by space
    if query.contains(" - ") {
        variations.push(format!("ytsearch5:{}", query.replace(" - ", " ")));
    }

    // 5. Cleaned query if different from original
    if clean_query != query {
        variations.push(format!("ytsearch5:{}", clean_query));
    }

    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    variations.retain(|v| seen.insert(v.clone()));

    // Limit to reasonable number of attempts
    variations.truncate(10);

    variations
}

/// Clean a search query by removing common noise like feat., ft., parenthetical info, etc.
fn clean_search_query(query: &str) -> String {
    let mut result = query.to_string();

    // Only remove YouTube-specific noise — keep musically meaningful modifiers
    // (Remix, Remastered, Live, Acoustic, Deluxe, feat. etc.) since they help
    // find the correct version on YouTube Music.
    let remove_patterns = [
        "(Official Video)", "(Official Music Video)", "(Official Audio)",
        "(Lyric Video)", "(Lyrics)", "(Audio)", "(Music Video)",
        "(Visualizer)", "(Official Visualizer)", "(Official Lyric Video)",
        "[Official Video]", "[Official Music Video]", "[Official Audio]",
        "[Lyric Video]", "[Lyrics]", "[Audio]", "[Music Video]",
        "[Official]", "(Official)",
    ];
    for pattern in &remove_patterns {
        result = result.replace(pattern, "");
        result = result.replace(&pattern.to_lowercase(), "");
    }

    // Clean up extra whitespace
    result = result.split_whitespace().collect::<Vec<_>>().join(" ");
    result.trim().to_string()
}

/// Split "Artist / Title" or "Artist - Title" patterns common in YouTube video titles.
/// Returns (title, artist_name). If the title doesn't match a pattern, returns the original
/// title and the provided fallback artist.
fn split_title_artist(raw_title: &str, fallback_artist: Option<&str>) -> (String, Option<String>) {
    // Delimiters ordered by specificity — " / " is almost always "Artist / Title"
    let delimiters = [" / ", " - ", " – ", " — "];

    for delim in &delimiters {
        if let Some(pos) = raw_title.find(delim) {
            let left = raw_title[..pos].trim();
            let right = raw_title[pos + delim.len()..].trim();
            // Both parts must be non-empty and reasonable length
            if !left.is_empty() && !right.is_empty() && left.len() < 200 && right.len() < 200 {
                // "Artist / Title" — left is artist, right is title
                return (right.to_string(), Some(left.to_string()));
            }
        }
    }

    // No splitting pattern found — return as-is
    (raw_title.to_string(), fallback_artist.map(|s| s.to_string()))
}

/// Check if a genre string is actually a YouTube category (not a real music genre).
fn is_youtube_category(genre: &str) -> bool {
    let categories = [
        "People & Blogs", "Entertainment", "Education", "Science & Technology",
        "News & Politics", "Howto & Style", "Comedy", "Film & Animation",
        "Autos & Vehicles", "Pets & Animals", "Sports", "Travel & Events",
        "Gaming", "Nonprofits & Activism",
    ];
    categories.iter().any(|c| c.eq_ignore_ascii_case(genre))
}
