pub mod deezer;
pub mod import;
pub mod matching;
pub mod metadata;
pub mod pipeline;
pub mod search;
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

use pipeline::run_download;

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

/// Default download location: the OS music folder (`~/Music` on Linux/macOS,
/// `%USERPROFILE%\Music` on Windows) under an app-managed "Playlist" subfolder,
/// so downloads land where users expect to find their music. Falls back to the
/// app-data downloads dir only when the OS exposes no music folder.
pub fn default_download_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    match app_handle.path().audio_dir() {
        Ok(dir) => dir.join("Playlist"),
        Err(_) => app_handle
            .path()
            .app_data_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("downloads"),
    }
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
            _ => Ok(default_download_dir(&self.app_handle)),
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
                // Persist and emit "failed" — "error" is not a status the
                // frontend (or the DB) knows, so the row would vanish from
                // every list and resurrect as an eternally-"queued" ghost.
                if let Ok(conn) = self.db.lock() {
                    let _ = crate::db::downloads::update_download_status(
                        &conn, download_id, "failed", Some(&e),
                    );
                }
                emit_event(&self.app_handle, download_id, "failed", 0.0, Some(e), None, None, title);
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
            // Reset the linked monitored entry, otherwise it stays badged
            // "queued/downloading" forever in the playlist view.
            reset_entries_for_downloads(&conn, &self.app_handle, &[download_id]);
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
        let mut cancelled_ids: Vec<i64> = Vec::new();
        for (id, handle) in tasks.drain() {
            handle.abort();
            cancelled_ids.push(id);
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
            for &id in &ids {
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
            // Reset all linked monitored entries back to "new" — without this,
            // playlist entries stay stuck showing "queued/downloading" and the
            // playlist cards keep their spinner even though nothing is running.
            cancelled_ids.extend(ids);
            reset_entries_for_downloads(&conn, &self.app_handle, &cancelled_ids);
        }
    }
}

/// Reset the monitored playlist entries linked to the given downloads back to
/// "new" (clearing the download link) and notify the frontend per entry.
pub(super) fn reset_entries_for_downloads(
    conn: &rusqlite::Connection,
    app_handle: &tauri::AppHandle,
    download_ids: &[i64],
) {
    for &download_id in download_ids {
        let eid: Option<i64> = conn
            .query_row(
                "SELECT id FROM monitored_playlist_entries WHERE download_id = ?1",
                rusqlite::params![download_id],
                |row| row.get(0),
            )
            .ok();
        if let Some(eid) = eid {
            let _ = crate::db::monitored::update_entry_status(conn, eid, "new", None, None);
            let _ = app_handle.emit(
                "manager-entry-updated",
                serde_json::json!({ "entry_id": eid, "status": "new" }),
            );
        }
    }
}

/// Update a download's status and its linked monitored entry in one step.
/// Emits a manager-entry-updated event if there's a linked entry.
pub(super) fn update_download_status_with_entry(
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

#[allow(clippy::too_many_arguments)]
pub(super) fn emit_event(
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
