pub mod deezer;
pub mod matching;
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
        let items: Vec<(i64, Option<String>)> = {
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
            // Select the title too so resumed downloads carry their name (no blank "ghost" rows).
            let mut stmt = match conn
                .prepare("SELECT id, title FROM downloads WHERE status = 'queued' ORDER BY created_at ASC")
            {
                Ok(s) => s,
                Err(_) => return,
            };
            let result: Vec<(i64, Option<String>)> = match stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))
            }) {
                Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                Err(e) => {
                    log::error!("Failed to query interrupted downloads: {}", e);
                    return;
                }
            };
            result
        };
        if !items.is_empty() {
            log::info!("Resuming {} interrupted download(s)", items.len());
            let mgr = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                for (id, title) in items {
                    mgr.start_download(id, title);
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
        log::info!("[download] id={} dir={:?} ffmpeg_dir={:?} ytdlp={}", download_id, download_dir, ffmpeg_dir, ytdlp_binary);
        let cookies_from_browser = self.get_cookies_from_browser();
        let semaphore = self.concurrency.clone();
        let sources = self.sources.clone();
        let used_urls_by_album = self.used_urls_by_album.clone();

        // Emit queued event immediately so the frontend can show all pending downloads
        emit_event(&self.app_handle, download_id, "queued", 0.0, None, None, None, title);

        let active_tasks_insert = self.active_tasks.clone();
        tokio::spawn(async move {
            // Hold the map lock across spawn+insert: the worker's final
            // `remove` must wait for the insert, otherwise a fast worker could
            // finish before its handle is inserted, leaking a stale entry.
            let mut tasks = active_tasks_insert.lock().await;
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
            tasks.insert(download_id, handle);
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
                    .trim_start_matches("ytsearch5:")
                    .trim_start_matches("ytsearch1:")
                    .trim_start_matches("scsearch5:")
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

            // --- Layer 0: pin the exact recording identity from free metadata ---
            // Single/playlist tracks arrive with no ISRC and no duration, so the matcher
            // has no duration guard. Deezer's public API (no auth) supplies both — but
            // only after we VERIFY it returned the same song, so a wrong first hit can't
            // inject a wrong ISRC/duration. This also lets Layer 2 (Odesli) find a link.
            let mut expected_secs = download.target_duration_ms.map(|ms| ms as f64 / 1000.0);
            let mut isrc = download.target_isrc.clone();
            let mut album_hint = download.target_album_name.clone();
            if (isrc.is_none() || expected_secs.is_none()) && download.title.is_some() {
                if let Some(meta) = deezer::search_public_metadata(&search_query).await {
                    let title_ok = download.title.as_deref()
                        .map(|t| matching::title_similarity(t, &meta.title) >= 0.6)
                        .unwrap_or(false);
                    let artist_ok = match (primary_artist.as_deref(), meta.artist.as_deref()) {
                        (Some(a), Some(ma)) => matching::title_similarity(a, ma) >= 0.5,
                        (Some(_), None) => false,
                        (None, _) => true,
                    };
                    if title_ok && artist_ok {
                        if isrc.is_none() { isrc = meta.isrc.clone(); }
                        if expected_secs.is_none() { expected_secs = meta.duration; }
                        if album_hint.is_none() { album_hint = meta.album.clone(); }
                        log::info!("[download] id={} Deezer identity verified: isrc={:?} dur={:?}s", download_id, isrc, expected_secs);
                        // Persist so post-download verification & tagging can use it.
                        if let Ok(conn) = db.lock() {
                            let _ = conn.execute(
                                "UPDATE downloads SET target_isrc = COALESCE(target_isrc, ?1),
                                     target_duration_ms = COALESCE(target_duration_ms, ?2) WHERE id = ?3",
                                rusqlite::params![isrc.as_deref(), expected_secs.map(|s| (s * 1000.0) as i64), download_id],
                            );
                        }
                    } else {
                        log::info!("[download] id={} Deezer result '{}' by {:?} did not verify, ignoring", download_id, meta.title, meta.artist);
                    }
                }
            }

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
                if let Some(ref isrc_val) = isrc {
                    log::info!("[download] id={} trying SongLink ISRC lookup: {}", download_id, isrc_val);
                    if let Some(url) = crate::metadata::songlink::resolve_isrc(isrc_val).await {
                        if !album_used_urls.contains(&url) {
                            log::info!("[download] id={} SongLink resolved → {}", download_id, url);
                            best_url = Some(url);
                        } else {
                            log::info!("[download] id={} SongLink URL already used by another album track, skipping", download_id);
                        }
                    }
                }
            }

            // --- Layer 3: YouTube Music (then YouTube) search with scored matching ---
            if best_url.is_none() {
                let variations = build_search_variations(&search_query, album_hint.as_deref());
                best_url = resolve_search_url(
                    &ytdlp_binary,
                    ffmpeg_dir.as_deref(),
                    cookies_from_browser.as_deref(),
                    &variations,
                    &search_query,
                    expected_secs,
                    is_album_track,
                    &album_used_urls,
                ).await;
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
                        // Release the URL claim so a retry of this track (or a
                        // sibling album track) can use it again.
                        if let Some(album_id) = download.target_album_id {
                            let mut guard = used_urls_by_album.write().await;
                            if let Some(set) = guard.get_mut(&album_id) {
                                set.remove(url);
                            }
                        }
                        false
                    }
                }
            } else {
                log::warn!("[download] id={} no smart match found for '{}'", download_id, search_query);
                errors.push("no matching result found by duration/title".to_string());
                false
            }
        } else {
            // Direct URL path (search URLs always take the smart-match branch).
            // Fetch metadata first so the UI shows a real title while
            // downloading and Phase-2 fallbacks have title/artist to work with.
            {
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
                // Check if the track already exists in the library.
                // A bare FTS hit is NOT enough: it must also pass the same
                // title/artist similarity gates as search matching, otherwise a
                // failed download gets marked "completed" against an unrelated
                // library track (e.g. a cover) and is never retried.
                let expected: Option<(String, Option<String>)> = if let Some(ref info) = ytdlp_info {
                    let title = info.track.as_deref().unwrap_or(&info.title).to_string();
                    let artist = info.artist.clone().or_else(|| info.uploader.clone());
                    Some((title, artist))
                } else if let Ok(conn) = db.lock() {
                    conn.query_row(
                        "SELECT title, artist FROM downloads WHERE id = ?1",
                        rusqlite::params![download_id],
                        |row| {
                            let t: Option<String> = row.get(0)?;
                            let a: Option<String> = row.get(1)?;
                            Ok(t.map(|t| (t, a)))
                        },
                    ).ok().flatten()
                } else {
                    None
                };

                let library_match = expected.as_ref().and_then(|(exp_title, exp_artist)| {
                    let q = match exp_artist {
                        Some(a) => format!("{} {}", a, exp_title),
                        None => exp_title.clone(),
                    };
                    let candidate = if let Ok(conn) = db.lock() {
                        crate::db::tracks::search_tracks_fts(&conn, &q, 1)
                            .ok()
                            .and_then(|tracks| tracks.into_iter().next())
                    } else {
                        None
                    };
                    candidate.filter(|track| {
                        let title_ok =
                            matching::title_similarity(exp_title, &track.title) >= 0.67;
                        let artist_ok = match (exp_artist, &track.artist_name) {
                            (Some(want), Some(have)) => {
                                let want_t = matching::tokens(want);
                                let have_t = matching::tokens(have);
                                let matched = want_t
                                    .iter()
                                    .filter(|w| have_t.contains(w))
                                    .count();
                                matched * 2 >= want_t.len().max(1)
                            }
                            // No artist on either side — don't reject on missing data
                            _ => true,
                        };
                        if !(title_ok && artist_ok) {
                            log::info!(
                                "Download {} FTS candidate '{}' rejected (title_ok={}, artist_ok={})",
                                download_id, track.title, title_ok, artist_ok
                            );
                        }
                        title_ok && artist_ok
                    })
                });

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
                    // NB: the frontend listens for "library-updated" (not "library-changed")
                    let _ = app_handle.emit("library-updated", ());
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
                            // Persist duration/album from the platform so the duration guard
                            // and tagging below can use them (fills only when still empty).
                            let _ = conn.execute(
                                "UPDATE downloads SET target_duration_ms = COALESCE(target_duration_ms, ?1),
                                     target_album_name = COALESCE(target_album_name, ?2) WHERE id = ?3",
                                rusqlite::params![
                                    meta.duration.map(|d| (d * 1000.0) as i64),
                                    meta.album.as_deref(),
                                    download_id
                                ],
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

                // Persist any ISRC/duration we just resolved so tagging/verification use it.
                if isrc.is_some() || expected_duration_secs.is_some() {
                    if let Ok(conn) = db.lock() {
                        let _ = conn.execute(
                            "UPDATE downloads SET target_isrc = COALESCE(target_isrc, ?1),
                                 target_duration_ms = COALESCE(target_duration_ms, ?2) WHERE id = ?3",
                            rusqlite::params![isrc.as_deref(), expected_duration_secs.map(|s| (s * 1000.0) as i64), download_id],
                        );
                    }
                }

                // Step 3: YouTube Music / YouTube search fallback with scored matching
                if let Some(ref query) = search_query {
                    let variations = build_search_variations(query, None);
                    if let Some(best_url) = resolve_search_url(
                        &ytdlp_binary,
                        ffmpeg_dir.as_deref(),
                        cookies_from_browser.as_deref(),
                        &variations,
                        query,
                        expected_duration_secs,
                        false,
                        &failed_urls,
                    ).await {
                        log::info!("[download] id={} fallback search matched → {}", download_id, best_url);
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
                                handle_download_success(&db, &app_handle, download_id, &file_path, &ytdlp_info).await;
                                return;
                            }
                            Err(e) => {
                                failed_urls.insert(best_url.clone());
                                errors.push(format!("YouTube fallback download: {}", e));
                            }
                        }
                    } else {
                        errors.push("YouTube fallback: no confident match".to_string());
                    }
                }

                // Step 4: SoundCloud search as final fallback (with scored matching)
                if let Some(ref query) = search_query {
                    let sc_info_query = format!("scsearch6:{}", query);
                    log::info!("Final fallback: searching SoundCloud for '{}'", sc_info_query);

                    // Try scored match first — never blindly download the first SoundCloud result.
                    let sc_best_url = match ytdlp::search_info(
                        &ytdlp_binary,
                        ffmpeg_dir.as_deref(),
                        &sc_info_query,
                        cookies_from_browser.as_deref(),
                    ).await {
                        Ok(results) => matching::pick_best_match(&results, query, expected_duration_secs, false, &failed_urls),
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
                "SELECT title, artist, url, target_album_id, target_artist_id, target_isrc, target_disc_number, target_track_number, target_duration_ms, target_album_name FROM downloads WHERE id = ?1",
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
                    row.get::<_, Option<String>>(9)?,
                )),
            ).ok()
        } else {
            None
        };
        let (title, artist, source_url, target_album_id, target_artist_id, target_isrc, target_disc_number, target_track_number, target_duration_ms, target_album_name) = match dl_row {
            Some((t, a, u, alb, art, isrc, disc, track, dur, alb_name)) => (t, a, Some(u), alb, art, isrc, disc, track, dur, alb_name),
            None => (None, None, None, None, None, None, None, None, None, None),
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
            target_album_name,
        }
    };
    let (track_id, file_path) = match import_downloaded_file(db, app_handle, file_path, &dl_meta).await {
        ImportOutcome::Imported { track_id, file_path } => (track_id, file_path),
        ImportOutcome::WrongSong { actual_ms, expected_ms } => {
            // Delete the wrong file and fail the download (fail & flag) so the library
            // never contains the wrong song. The user can retry to pick another source.
            let _ = std::fs::remove_file(file_path);
            fail_download(
                db,
                app_handle,
                download_id,
                &format!(
                    "Wrong song rejected: downloaded duration {}s does not match expected {}s",
                    actual_ms.map(|ms| ms / 1000).unwrap_or(0),
                    expected_ms / 1000,
                ),
            );
            return;
        }
    };

    // Batch all post-completion DB updates in a single lock scope
    let entry_id = if let Ok(conn) = db.lock() {
        let _ = conn.execute_batch("BEGIN");
        let _ = crate::db::downloads::update_download_file(
            &conn,
            download_id,
            &file_path,
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
    target_album_name: Option<String>,
}

/// Result of importing a downloaded file into the library.
enum ImportOutcome {
    /// File imported. `track_id` is present unless the DB insert itself failed.
    /// `file_path` is the final on-disk path (files are moved into an
    /// Artist/Album/NN - Title structure after tagging).
    Imported { track_id: Option<i64>, file_path: String },
    /// The file's duration doesn't match the expected track — wrong song; the
    /// caller should delete the file and fail the download.
    WrongSong { actual_ms: Option<u64>, expected_ms: i64 },
}

/// Sanitize one path component (folder or filename) for cross-platform filesystems.
fn sanitize_component(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let t = s.trim().trim_end_matches('.').trim();
    if t.is_empty() { "Unknown".to_string() } else { t.to_string() }
}

/// Move a downloaded file into `<download_dir>/<Artist>/<Album>/<NN - Title>.ext`
/// (same structure as the library export). Returns the new path, or the original
/// path unchanged on any failure.
fn organize_file(
    current: &std::path::Path,
    artist: &str,
    album: &str,
    disc: Option<i64>,
    track: Option<i64>,
    title: &str,
) -> std::path::PathBuf {
    let download_dir = match current.parent() {
        Some(p) => p,
        None => return current.to_path_buf(),
    };
    let ext = current.extension().and_then(|e| e.to_str()).unwrap_or("mp3");
    let prefix = match (disc, track) {
        (Some(d), Some(n)) if d > 1 => format!("{}-{:02}", d, n),
        (_, Some(n)) => format!("{:02}", n),
        _ => String::new(),
    };
    let safe_title = sanitize_component(title);
    let base = if prefix.is_empty() {
        safe_title.clone()
    } else {
        format!("{} - {}", prefix, safe_title)
    };
    let target_dir = download_dir
        .join(sanitize_component(artist))
        .join(sanitize_component(album));
    if std::fs::create_dir_all(&target_dir).is_err() {
        return current.to_path_buf();
    }
    let mut target = target_dir.join(format!("{}.{}", base, ext));
    if target != current && target.exists() {
        if prefix.is_empty() {
            // No track number: disambiguate with the unique download stem so distinct
            // singles that share a title don't clobber each other.
            let stem = current.file_stem().and_then(|s| s.to_str()).unwrap_or("dl");
            target = target_dir.join(format!("{} [{}].{}", base, stem, ext));
        } else {
            // Same album position: replace the previous file for this track.
            let _ = std::fs::remove_file(&target);
        }
    }
    match std::fs::rename(current, &target) {
        Ok(_) => target,
        Err(_) => {
            // Different filesystem — fall back to copy + delete.
            if std::fs::copy(current, &target).is_ok() {
                let _ = std::fs::remove_file(current);
                target
            } else {
                current.to_path_buf()
            }
        }
    }
}

async fn import_downloaded_file(
    db: &Arc<DbPool>,
    app_handle: &tauri::AppHandle,
    file_path: &str,
    dl_meta: &DownloadMeta,
) -> ImportOutcome {
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        log::warn!("Downloaded file not found: {}", file_path);
        return ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() };
    }

    // Run sync I/O (tag reading, cover extraction) off the async runtime
    let path_buf = path.to_path_buf();
    let covers_dir = match app_handle.path().app_data_dir() {
        Ok(d) => d.join("covers"),
        Err(_) => return ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() },
    };
    let covers_dir_clone = covers_dir.clone();
    let (tag_data, cover_art_path) = match tokio::task::spawn_blocking(move || {
        // Single file read for both tags and cover art (halves memory usage)
        match tags::read_tags_and_cover(&path_buf, &covers_dir_clone) {
            Ok((data, cover)) => (data, cover),
            Err(e) => {
                log::warn!("Failed to read tags from downloaded file: {}", e);
                (tags::TagData::default(), None)
            }
        }
    }).await {
        Ok(v) => v,
        Err(_) => return ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() },
    };

    // Post-download verification (align with metadata): compare the downloaded file's
    // actual duration against the expected duration from MusicBrainz/Deezer. If they
    // differ beyond tolerance we grabbed the wrong song — reject it (fail & flag) rather
    // than import incorrect audio. Applies to ALL downloads with a known expected duration.
    if let (Some(expected_ms), Some(actual_ms)) = (dl_meta.target_duration_ms, tag_data.duration_ms) {
        if expected_ms > 0 {
            let diff_ms = (expected_ms - actual_ms as i64).unsigned_abs();
            let tolerance_ms = ((expected_ms as f64 * 0.12) as u64).max(12_000);
            if diff_ms > tolerance_ms {
                log::warn!(
                    "Post-download duration mismatch for '{}': expected {}ms, got {}ms (diff {}ms > {}ms) — rejecting wrong song",
                    dl_meta.title.as_deref().unwrap_or("?"), expected_ms, actual_ms, diff_ms, tolerance_ms
                );
                return ImportOutcome::WrongSong { actual_ms: tag_data.duration_ms, expected_ms };
            }
        }
    }
    let is_album_download = dl_meta.target_album_id.is_some();

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

    let conn = match db.lock() {
        Ok(c) => c,
        Err(_) => return ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() },
    };

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
    // For album tracks, default a missing disc number to 1 so the dedup lookup below
    // (which uses disc = 1) matches the value we actually store (B3 fix).
    let disc_number = if is_album_download {
        dl_meta.target_disc_number.or(tag_data.disc_number.map(|d| d as i64)).or(Some(1))
    } else {
        tag_data.disc_number.map(|d| d as i64).or(dl_meta.target_disc_number)
    };

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

            // Propagate cover art to the album only. Do NOT use the embedded cover as the
            // artist image — it's album/track art, not an artist photo, and because
            // update_image_if_missing only writes when NULL it would permanently block the
            // real artist photo that enrichment (Last.fm) fetches later (fix A1).
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

            // --- Organize the file into Artist/Album/NN - Title.ext and write correct
            // tags, so downloads are self-describing offline (like the library export). ---
            let album_name_str = tag_data.album.clone()
                .or_else(|| dl_meta.album.clone())
                .or_else(|| dl_meta.target_album_name.clone())
                .unwrap_or_else(|| "Unknown Album".to_string());
            let artist_for_path = artist_name.clone().unwrap_or_else(|| "Unknown Artist".to_string());

            let final_path = organize_file(
                std::path::Path::new(file_path),
                &artist_for_path,
                &album_name_str,
                disc_number,
                track_number,
                &title,
            );

            let tw = tags::TagWrite {
                title: Some(title.clone()),
                artist: artist_name.clone(),
                album: Some(album_name_str),
                album_artist: artist_name.clone(),
                track_number: track_number.map(|n| n as u32),
                disc_number: disc_number.map(|d| d as u32),
                year: year.map(|y| y as u32),
                genre: genre_for_album.clone(),
            };
            if let Err(e) = tags::write_tags(&final_path, &tw) {
                log::warn!("Failed to write tags to {:?}: {}", final_path, e);
            }

            let final_path_str = final_path.to_string_lossy().to_string();
            if final_path_str.as_str() != file_path {
                let _ = conn.execute(
                    "UPDATE tracks SET file_path = ?1 WHERE id = ?2",
                    rusqlite::params![final_path_str, track_id],
                );
            }

            log::info!("Imported downloaded track: {} (id={}) -> {}", title, track_id, final_path_str);
            ImportOutcome::Imported { track_id: Some(track_id), file_path: final_path_str }
        }
        Err(e) => {
            log::warn!("Failed to insert downloaded track: {}", e);
            ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() }
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

/// Build search query variations (plain text, no engine prefix) to try in order.
/// The caller runs each through YouTube Music first, then plain YouTube.
/// When an album name is available, an album-aware query is added for accuracy.
///
/// Note: raw ISRC is intentionally NOT used as a text search — ISRCs don't appear
/// in YouTube titles, so it only wastes a request. ISRC is used for the direct
/// Odesli/SongLink lookup instead (a deterministic link, not a text search).
fn build_search_variations(query: &str, album: Option<&str>) -> Vec<String> {
    let mut variations = Vec::new();
    let clean_query = clean_search_query(query);

    // 1. Album-aware search (best for pinning the correct album version)
    if let Some(album_name) = album {
        if !album_name.is_empty() {
            variations.push(format!("{} {}", query, album_name));
        }
    }

    // 2. Primary search: Artist - Title
    variations.push(query.to_string());

    // 3. "Artist - Title" → title-focused variations
    if query.contains(" - ") {
        let parts: Vec<&str> = query.splitn(2, " - ").collect();
        if parts.len() == 2 {
            let artist = parts[0].trim();
            let title = parts[1].trim();

            // Dash replaced by space (plain "Artist Title")
            variations.push(format!("{} {}", artist, title));

            // Artist + cleaned title (removes "(feat. …)" etc.)
            let clean_title = clean_search_query(title);
            if clean_title != title {
                variations.push(format!("{} {}", artist, clean_title));
            }

            // Just the title (last resort for obscure tracks)
            if title.split_whitespace().count() >= 2 {
                variations.push(title.to_string());
            }
        }
    }

    // 4. Cleaned full query if different
    if clean_query != query {
        variations.push(clean_query);
    }

    // Dedupe, preserve order, cap attempts.
    let mut seen = std::collections::HashSet::new();
    variations.retain(|v| seen.insert(v.clone()));
    variations.truncate(6);
    variations
}

/// Try to resolve a search query to a downloadable URL: YouTube Music first
/// (catalog-only, clean metadata), then plain YouTube as a fallback. Each engine's
/// results are scored by the matcher; returns `None` if nothing clears the bar
/// (caller then fails the track rather than downloading the wrong song).
#[allow(clippy::too_many_arguments)]
async fn resolve_search_url(
    ytdlp_binary: &str,
    ffmpeg_dir: Option<&str>,
    cookies: Option<&str>,
    variations: &[String],
    scoring_query: &str,
    expected_duration_secs: Option<f64>,
    strict: bool,
    exclude_urls: &HashSet<String>,
) -> Option<String> {
    for variation in variations {
        // YouTube Music (preferred).
        if let Ok(results) = ytdlp::search_music_tracks(ytdlp_binary, ffmpeg_dir, variation, 6, cookies).await {
            if !results.is_empty() {
                if let Some(url) = matching::pick_best_match(&results, scoring_query, expected_duration_secs, strict, exclude_urls) {
                    log::info!("[download] YouTube Music matched via '{}'", variation);
                    return Some(url);
                }
            }
        }
        // Plain YouTube fallback.
        if let Ok(results) = ytdlp::search_info(ytdlp_binary, ffmpeg_dir, &format!("ytsearch6:{}", variation), cookies).await {
            if !results.is_empty() {
                if let Some(url) = matching::pick_best_match(&results, scoring_query, expected_duration_secs, strict, exclude_urls) {
                    log::info!("[download] YouTube matched via '{}'", variation);
                    return Some(url);
                }
            }
        }
    }
    None
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
