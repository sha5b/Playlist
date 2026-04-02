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

    fn get_download_dir(&self) -> PathBuf {
        let conn = self.db.lock().expect("DB mutex poisoned");
        let dir = crate::db::settings::get_setting(&conn, "download_dir")
            .ok()
            .flatten();
        match dir {
            Some(d) if !d.is_empty() => PathBuf::from(d),
            _ => {
                let app_dir = self
                    .app_handle
                    .path()
                    .app_data_dir()
                    .unwrap_or_else(|_| PathBuf::from("."));
                app_dir.join("downloads")
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
        let conn = self.db.lock().expect("DB mutex poisoned");
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
            stmt.query_map([], |row| row.get(0))
                .unwrap()
                .filter_map(|r| r.ok())
                .collect()
        };
        if !ids.is_empty() {
            log::info!("Resuming {} interrupted download(s)", ids.len());
            let mgr = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                for id in ids {
                    mgr.start_download(id);
                }
            });
        }
    }

    pub fn start_download(&self, download_id: i64) {
        let db = self.db.clone();
        let app_handle = self.app_handle.clone();
        let active_tasks = self.active_tasks.clone();
        let download_dir = self.get_download_dir();
        let ytdlp_binary = self.resolve_ytdlp();
        let ffmpeg_dir = self.resolve_ffmpeg_dir();
        let cookies_from_browser = self.get_cookies_from_browser();
        let semaphore = self.concurrency.clone();
        let sources = self.sources.clone();

        // Emit queued event immediately so the frontend can show all pending downloads
        emit_event(&self.app_handle, download_id, "queued", 0.0, None, None, None, None);

        let active_tasks_insert = self.active_tasks.clone();
        tokio::spawn(async move {
            let handle = tokio::spawn(async move {
                // Wait for a concurrency slot (limits parallel yt-dlp processes)
                let _permit = semaphore.acquire().await.expect("Semaphore closed");
                run_download(db, app_handle, download_id, download_dir, ytdlp_binary, ffmpeg_dir, cookies_from_browser, sources)
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
                let mut stmt = conn.prepare(
                    "SELECT id FROM downloads WHERE status IN ('queued', 'downloading', 'processing')"
                ).unwrap();
                stmt.query_map([], |row| row.get(0))
                    .unwrap()
                    .filter_map(|r| r.ok())
                    .collect()
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

async fn run_download(
    db: Arc<DbPool>,
    app_handle: tauri::AppHandle,
    download_id: i64,
    download_dir: PathBuf,
    ytdlp_binary: String,
    ffmpeg_dir: Option<String>,
    cookies_from_browser: Option<String>,
    sources: Arc<tokio::sync::RwLock<source::SourceRegistry>>,
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
                download.url = format!("ytsearch1:{}", search_query);
                // Update the download URL in database
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
        fail_download(
            &db,
            &app_handle,
            download_id,
            &format!("Failed to create download dir: {}", e),
        );
        return;
    }

    // Update status to downloading
    {
        let entry_id = if let Ok(conn) = db.lock() {
            let _ = crate::db::downloads::update_download_status(&conn, download_id, "downloading", None);
            // Update linked monitored entry
            let eid: Option<i64> = conn.query_row(
                "SELECT id FROM monitored_playlist_entries WHERE download_id = ?1",
                rusqlite::params![download_id],
                |row| row.get::<_, i64>(0),
            ).ok();
            if let Some(eid) = eid {
                let _ = crate::db::monitored::update_entry_status(&conn, eid, "downloading", Some(download_id), None);
            }
            eid
        } else {
            None
        };
        if let Some(eid) = entry_id {
            let _ = app_handle.emit(
                "manager-entry-updated",
                serde_json::json!({ "entry_id": eid, "status": "downloading" }),
            );
        }
    }
    emit_event(
        &app_handle,
        download_id,
        "downloading",
        0.0,
        None,
        None,
        None,
        None,
    );

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
                let query = match (&download.artist, &download.title) {
                    (Some(a), Some(t)) => Some(format!("{} - {}", a, t)),
                    (None, Some(t)) => Some(t.clone()),
                    _ => None,
                };
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
        // Only fetch metadata separately for search URLs (need it for duration matching later).
        // For direct URLs, skip the extra yt-dlp call — download immediately for speed.
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

                // Step 3: YouTube search fallback with duration validation
                if let Some(ref query) = search_query {
                    // Get expected duration from yt-dlp info or monitored entry
                    let expected_duration_secs = ytdlp_info.as_ref().and_then(|i| i.duration).or_else(|| {
                        if let Ok(conn) = db.lock() {
                            conn.query_row(
                                "SELECT e.duration_seconds FROM monitored_playlist_entries e WHERE e.download_id = ?1",
                                rusqlite::params![download_id],
                                |row| row.get::<_, Option<f64>>(0),
                            ).ok().flatten()
                        } else {
                            None
                        }
                    });

                    // Build search queries, using ytsearch5: to get multiple results for duration matching
                    let query_variations = build_search_variations(query);

                    for (attempt, yt_query) in query_variations.iter().enumerate() {
                        // Replace ytsearch1: with ytsearch5: to get multiple candidates
                        let info_query = yt_query
                            .replace("ytsearch1:", "ytsearch5:")
                            .replace("ytmsearch1:", "ytmsearch5:")
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
                                if let Some(best_url) = pick_best_duration_match(&results, expected_duration_secs) {
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
                                    log::info!("Bot detection likely, waiting 3 seconds before retry...");
                                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                                }
                            }
                        }
                    }
                }

                // Step 4: SoundCloud search as final fallback
                if let Some(ref query) = search_query {
                    let sc_query = format!("scsearch1:{}", query);
                    log::info!("Final fallback: searching SoundCloud for '{}'", sc_query);

                    let app_handle_sc = app_handle.clone();
                    let dl_id_sc = download_id;
                    let sc_result = ytdlp::download_audio(
                        &ytdlp_binary,
                        ffmpeg_dir.as_deref(),
                        &sc_query,
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
                "SELECT title, artist, url, target_album_id, target_artist_id FROM downloads WHERE id = ?1",
                rusqlite::params![download_id],
                |row| Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                )),
            ).ok()
        } else {
            None
        };
        let (title, artist, source_url, target_album_id, target_artist_id) = match dl_row {
            Some((t, a, u, alb, art)) => (t, a, Some(u), alb, art),
            None => (None, None, None, None, None),
        };
        DownloadMeta {
            title,
            artist,
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

    // Use file tags if available, fall back to download metadata from yt-dlp
    let title = tag_data.title
        .or_else(|| dl_meta.title.clone())
        .unwrap_or_else(|| "Unknown".to_string());
    let artist_name = tag_data.artist
        .or_else(|| dl_meta.artist.clone());

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
        tag_data.album.as_ref().and_then(|alb| {
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

    // Merge yt-dlp fallback fields with file tags (file tags win)
    let genre = tag_data.genre.or_else(|| dl_meta.genre.clone());
    let year = tag_data.year.map(|y| y as i64)
        .or_else(|| dl_meta.release_year.as_ref().and_then(|y| y.parse::<i64>().ok()));
    let description = dl_meta.description.clone();
    let language = dl_meta.language.clone();
    let composer = dl_meta.composer.clone();
    let release_date = dl_meta.release_year.clone();

    let tags = dl_meta.tags.clone();

    let result = conn.execute(
        "INSERT INTO tracks (title, artist_id, album_id, album_artist, duration_ms,
            track_number, disc_number, genre, year, file_path, file_size, format,
            bitrate, sample_rate, channels, cover_art_path, source_platform, source_url,
            description, language, composer, release_date, tags)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'download', ?17, ?18, ?19, ?20, ?21, ?22)",
        rusqlite::params![
            title,
            artist_id,
            album_id,
            tag_data.album_artist,
            tag_data.duration_ms.map(|d| d as i64),
            tag_data.track_number.map(|t| t as i64),
            tag_data.disc_number.map(|d| d as i64),
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
        ],
    );

    match result {
        Ok(_) => {
            let track_id = conn.last_insert_rowid();
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

/// Check if a candidate duration is "in the ballpark" of the expected duration.
/// Returns true if within 30% or 30 seconds (whichever is more lenient).
fn duration_acceptable(expected_secs: f64, candidate_secs: f64) -> bool {
    if expected_secs <= 0.0 || candidate_secs <= 0.0 {
        return true; // Can't validate, allow it
    }
    let diff = (expected_secs - candidate_secs).abs();
    let pct_threshold = expected_secs * 0.30;
    diff <= pct_threshold.max(30.0)
}

/// Pick the best duration-matching result from a list of search results.
/// Returns the URL of the best match, or None if no acceptable match found.
fn pick_best_duration_match(
    results: &[ytdlp::VideoInfo],
    expected_duration_secs: Option<f64>,
) -> Option<String> {
    if results.is_empty() {
        return None;
    }
    let expected = match expected_duration_secs {
        Some(d) if d > 0.0 => d,
        _ => {
            // No expected duration — just return the first result with a URL
            return results.iter().find_map(|r| r.webpage_url.clone());
        }
    };

    // Find all results with acceptable duration, pick the closest
    let mut best: Option<(&ytdlp::VideoInfo, f64)> = None;
    for result in results {
        let candidate_dur = match result.duration {
            Some(d) if d > 0.0 => d,
            _ => continue,
        };
        if !duration_acceptable(expected, candidate_dur) {
            log::info!(
                "Skipping '{}' — duration {:.0}s vs expected {:.0}s",
                result.title, candidate_dur, expected
            );
            continue;
        }
        let diff = (expected - candidate_dur).abs();
        if best.is_none() || diff < best.unwrap().1 {
            best = Some((result, diff));
        }
    }

    if let Some((result, diff)) = best {
        log::info!(
            "Best match: '{}' — duration diff {:.0}s",
            result.title, diff
        );
        result.webpage_url.clone()
    } else {
        log::warn!("No search results had acceptable duration (expected {:.0}s)", expected);
        None
    }
}

/// Build comprehensive search query variations for YouTube/SoundCloud fallback.
/// Returns a list of yt-dlp search queries to try in order.
fn build_search_variations(query: &str) -> Vec<String> {
    let mut variations = Vec::new();
    
    // Clean the query: remove feat./ft., parentheses with remix/version info, etc.
    let clean_query = clean_search_query(query);
    
    // 1. Original query as-is
    variations.push(format!("ytsearch1:{}", query));
    
    // 2. Query with " - " replaced by space (Artist Title instead of Artist - Title)
    if query.contains(" - ") {
        variations.push(format!("ytsearch1:{}", query.replace(" - ", " ")));
    }
    
    // 3. If we have "Artist - Title" format, try variations
    if query.contains(" - ") {
        let parts: Vec<&str> = query.splitn(2, " - ").collect();
        if parts.len() == 2 {
            let artist = parts[0].trim();
            let title = parts[1].trim();
            
            // Title Artist (reversed)
            variations.push(format!("ytsearch1:{} {}", title, artist));
            
            // Title only (for covers/remixes that might not have original artist)
            variations.push(format!("ytsearch1:{}", title));
            
            // Artist only + cleaned title (removes feat. etc)
            let clean_title = clean_search_query(title);
            if clean_title != title {
                variations.push(format!("ytsearch1:{} {}", artist, clean_title));
            }
            
            // Add "audio" suffix for official audio uploads
            variations.push(format!("ytsearch1:{} {} audio", artist, title));
            
            // Add "lyrics" suffix (lyric videos are common)
            variations.push(format!("ytsearch1:{} {} lyrics", artist, title));
        }
    }
    
    // 4. Cleaned query if different from original
    if clean_query != query {
        variations.push(format!("ytsearch1:{}", clean_query));
    }
    
    // 5. YouTube Music search (often has better music results)
    variations.push(format!("ytmsearch1:{}", query));
    
    // Remove duplicates while preserving order
    let mut seen = std::collections::HashSet::new();
    variations.retain(|v| seen.insert(v.clone()));
    
    // Limit to reasonable number of attempts
    variations.truncate(8);
    
    variations
}

/// Clean a search query by removing common noise like feat., ft., parenthetical info, etc.
fn clean_search_query(query: &str) -> String {
    let mut result = query.to_string();
    
    // Remove feat./ft./featuring patterns
    let feat_patterns = [
        " (feat. ", " (ft. ", " (featuring ", 
        " [feat. ", " [ft. ", " [featuring ",
        " feat. ", " ft. ", " featuring ",
    ];
    for pattern in &feat_patterns {
        if let Some(pos) = result.to_lowercase().find(&pattern.to_lowercase()) {
            // Find the closing bracket if there is one
            let after = &result[pos..];
            if after.starts_with(" (") || after.starts_with(" [") {
                if let Some(close) = after.find(|c| c == ')' || c == ']') {
                    result = format!("{}{}", &result[..pos], &result[pos + close + 1..]);
                } else {
                    result = result[..pos].to_string();
                }
            } else {
                result = result[..pos].to_string();
            }
        }
    }
    
    // Remove common parenthetical suffixes that might not match
    let remove_patterns = [
        "(Official Video)", "(Official Music Video)", "(Official Audio)",
        "(Lyric Video)", "(Lyrics)", "(Audio)", "(Music Video)",
        "(Remastered)", "(Remaster)", "(Radio Edit)", "(Single Version)",
        "[Official Video]", "[Official Music Video]", "[Official Audio]",
        "[Lyric Video]", "[Lyrics]", "[Audio]", "[Music Video]",
    ];
    for pattern in &remove_patterns {
        result = result.replace(pattern, "");
        // Also try lowercase
        result = result.replace(&pattern.to_lowercase(), "");
    }
    
    // Clean up extra whitespace
    result = result.split_whitespace().collect::<Vec<_>>().join(" ");
    result.trim().to_string()
}
