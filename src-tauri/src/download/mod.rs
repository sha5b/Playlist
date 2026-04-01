pub mod setup;
pub mod url_parser;
pub mod ytdlp;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{Emitter, Manager};
use tokio::sync::Mutex;

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
    db: Arc<DbPool>,
    app_handle: tauri::AppHandle,
    active_tasks: Arc<Mutex<HashMap<i64, tokio::task::JoinHandle<()>>>>,
}

impl DownloadManager {
    pub fn new(db: Arc<DbPool>, app_handle: tauri::AppHandle) -> Self {
        Self {
            db,
            app_handle,
            active_tasks: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_download_dir(&self) -> PathBuf {
        let conn = self.db.lock().unwrap();
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

    pub fn start_download(&self, download_id: i64) {
        let db = self.db.clone();
        let app_handle = self.app_handle.clone();
        let active_tasks = self.active_tasks.clone();
        let download_dir = self.get_download_dir();
        let ytdlp_binary = self.resolve_ytdlp();
        let ffmpeg_dir = self.resolve_ffmpeg_dir();

        let handle = tokio::spawn(async move {
            run_download(db, app_handle, download_id, download_dir, ytdlp_binary, ffmpeg_dir).await;
            active_tasks.lock().await.remove(&download_id);
        });

        let active_tasks = self.active_tasks.clone();
        tokio::spawn(async move {
            active_tasks.lock().await.insert(download_id, handle);
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
        let conn = db.lock().unwrap();
        let _ =
            crate::db::downloads::update_download_status(&conn, download_id, "downloading", None);
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
            let conn = db.lock().unwrap();
            let _ = crate::db::downloads::update_download_title(
                &conn,
                download_id,
                &info.title,
                info.uploader.as_deref(),
            );
            emit_event(
                &app_handle,
                download_id,
                "downloading",
                0.0,
                None,
                None,
                None,
                Some(info.title.clone()),
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

    // Download the audio
    let app_handle_progress = app_handle.clone();
    let dl_id = download_id;

    let result = ytdlp::download_audio(
        &ytdlp_binary,
        ffmpeg_dir.as_deref(),
        &download.url,
        &download_dir,
        &download.format,
        &download.quality,
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
            {
                let conn = db.lock().unwrap();
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

            let track_id = import_downloaded_file(&db, &app_handle, &file_path);

            {
                let conn = db.lock().unwrap();
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
            }
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
        }
        Err(e) => {
            fail_download(&db, &app_handle, download_id, &e);
        }
    }
}

fn import_downloaded_file(
    db: &Arc<DbPool>,
    app_handle: &tauri::AppHandle,
    file_path: &str,
) -> Option<i64> {
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        log::warn!("Downloaded file not found: {}", file_path);
        return None;
    }

    let tag_data = match tags::read_tags(path) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("Failed to read tags from downloaded file: {}", e);
            return None;
        }
    };

    let covers_dir = app_handle.path().app_data_dir().ok()?.join("covers");

    let cover_art_path = tags::extract_cover_art(path, &covers_dir).unwrap_or(None);

    let conn = db.lock().ok()?;

    let artist_id = tag_data
        .artist
        .as_ref()
        .and_then(|name| crate::db::artists::find_or_create(&conn, name).ok());

    let album_id = tag_data.album.as_ref().and_then(|title| {
        crate::db::albums::find_or_create(
            &conn,
            title,
            artist_id,
            tag_data.album_artist.as_deref(),
            tag_data.year.map(|y| y as i64),
        )
        .ok()
    });

    let file_size = std::fs::metadata(path).map(|m| m.len() as i64).ok();

    let result = conn.execute(
        "INSERT INTO tracks (title, artist_id, album_id, album_artist, duration_ms,
            track_number, disc_number, genre, year, file_path, file_size, format,
            bitrate, sample_rate, channels, cover_art_path, source_platform)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'download')",
        rusqlite::params![
            tag_data.title.unwrap_or_else(|| "Unknown".to_string()),
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
        ],
    );

    match result {
        Ok(_) => {
            let track_id = conn.last_insert_rowid();
            let _ = crate::db::tracks::update_fts(&conn, track_id);
            log::info!(
                "Imported downloaded track: {} (id={})",
                file_path,
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
    if let Ok(conn) = db.lock() {
        let _ = crate::db::downloads::update_download_status(&conn, id, "failed", Some(error));
    }
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
