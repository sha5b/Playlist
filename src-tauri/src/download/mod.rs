pub mod setup;
pub mod url_parser;
pub mod ytdlp;

use std::collections::HashMap;
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
}

impl DownloadManager {
    pub fn new(db: Arc<DbPool>, app_handle: tauri::AppHandle) -> Self {
        Self {
            db,
            app_handle,
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
            concurrency: Arc::new(Semaphore::new(2)),
        }
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
        let semaphore = self.concurrency.clone();

        // Emit queued event immediately so the frontend can show all pending downloads
        emit_event(&self.app_handle, download_id, "queued", 0.0, None, None, None, None);

        let active_tasks_insert = self.active_tasks.clone();
        tokio::spawn(async move {
            let handle = tokio::spawn(async move {
                // Wait for a concurrency slot (limits parallel yt-dlp processes)
                let _permit = semaphore.acquire().await.expect("Semaphore closed");
                run_download(db, app_handle, download_id, download_dir, ytdlp_binary, ffmpeg_dir)
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
        // Mark all in-progress downloads as cancelled in the DB
        if let Ok(conn) = self.db.lock() {
            let _ = conn.execute(
                "UPDATE downloads SET status = 'cancelled' WHERE status IN ('queued', 'downloading', 'processing')",
                [],
            );
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
) {
    let download = {
        let conn = match db.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        match crate::db::downloads::get_download(&conn, download_id) {
            Ok(Some(d)) => d,
            _ => return,
        }
    };

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

    // Fetch metadata first
    match ytdlp::get_info(
        &ytdlp_binary,
        ffmpeg_dir.as_deref(),
        &download.url,
    )
    .await
    {
        Ok(info) => {
            // Prefer music-specific fields: track > title, artist > uploader
            let best_title = info.track.as_deref().unwrap_or(&info.title);
            let best_artist = info.artist.as_deref().or(info.uploader.as_deref());
            if let Ok(conn) = db.lock() {
                let _ = crate::db::downloads::update_download_title(
                    &conn,
                    download_id,
                    best_title,
                    best_artist,
                );
            }
            emit_event(
                &app_handle,
                download_id,
                "downloading",
                0.0,
                None,
                None,
                None,
                Some(best_title.to_string()),
            );
        }
        Err(e) => {
            log::warn!(
                "Could not fetch metadata for download {}: {}",
                download_id,
                e
            );
        }
    }

    // Download the audio using download_id as filename to avoid encoding issues
    let app_handle_progress = app_handle.clone();
    let dl_id = download_id;
    let file_stem = format!("dl_{}", download_id);

    let result = ytdlp::download_audio(
        &ytdlp_binary,
        ffmpeg_dir.as_deref(),
        &download.url,
        &download_dir,
        &download.format,
        &download.quality,
        &file_stem,
        move |progress| {
            emit_event(
                &app_handle_progress,
                dl_id,
                "downloading",
                progress.percent,
                progress.speed.clone(),
                progress.eta.clone(),
                None,
                None,
            );
        },
    )
    .await;

    match result {
        Ok(file_path) => {
            if let Ok(conn) = db.lock() {
                let _ = crate::db::downloads::update_download_status(
                    &conn,
                    download_id,
                    "processing",
                    None,
                );
            }
            emit_event(
                &app_handle,
                download_id,
                "processing",
                100.0,
                None,
                None,
                None,
                None,
            );

            // Build fallback metadata from the download record (populated by yt-dlp get_info)
            let dl_meta = {
                let title_artist = if let Ok(conn) = db.lock() {
                    conn.query_row(
                        "SELECT title, artist, url FROM downloads WHERE id = ?1",
                        rusqlite::params![download_id],
                        |row| Ok((
                            row.get::<_, Option<String>>(0)?,
                            row.get::<_, Option<String>>(1)?,
                            row.get::<_, String>(2)?,
                        )),
                    ).ok()
                } else {
                    None
                };
                match title_artist {
                    Some((t, a, u)) => DownloadMeta { title: t, artist: a, source_url: Some(u) },
                    None => DownloadMeta { title: None, artist: None, source_url: None },
                }
            };
            let track_id = import_downloaded_file(&db, &app_handle, &file_path, &dl_meta).await;

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
        Err(e) => {
            fail_download(&db, &app_handle, download_id, &e);
        }
    }
}

/// Metadata from the download record to use as fallback when file tags are missing
struct DownloadMeta {
    title: Option<String>,
    artist: Option<String>,
    source_url: Option<String>,
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

    let artist_id = artist_name
        .as_ref()
        .and_then(|name| crate::db::artists::find_or_create(&conn, name).ok());

    let album_id = tag_data.album.as_ref().and_then(|alb| {
        crate::db::albums::find_or_create(
            &conn,
            alb,
            artist_id,
            tag_data.album_artist.as_deref(),
            tag_data.year.map(|y| y as i64),
        )
        .ok()
    });

    let file_size = std::fs::metadata(path).map(|m| m.len() as i64).ok();

    // Save values needed after INSERT (before they get moved into params)
    let total_tracks_val = tag_data.total_tracks.map(|t| t as i64);
    let total_discs_val = tag_data.total_discs.map(|d| d as i64);
    let genre_for_album = tag_data.genre.clone();

    let result = conn.execute(
        "INSERT INTO tracks (title, artist_id, album_id, album_artist, duration_ms,
            track_number, disc_number, genre, year, file_path, file_size, format,
            bitrate, sample_rate, channels, cover_art_path, source_platform, source_url)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'download', ?17)",
        rusqlite::params![
            title,
            artist_id,
            album_id,
            tag_data.album_artist,
            tag_data.duration_ms.map(|d| d as i64),
            tag_data.track_number.map(|t| t as i64),
            tag_data.disc_number.map(|d| d as i64),
            tag_data.genre,
            tag_data.year.map(|y| y as i64),
            file_path,
            file_size,
            tag_data.format,
            tag_data.bitrate.map(|b| b as i64),
            tag_data.sample_rate.map(|s| s as i64),
            tag_data.channels.map(|c| c as i64),
            cover_art_path,
            dl_meta.source_url,
        ],
    );

    match result {
        Ok(_) => {
            let track_id = conn.last_insert_rowid();
            let _ = crate::db::tracks::update_fts(&conn, track_id);

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
