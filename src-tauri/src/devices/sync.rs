use crate::db::devices as db_devices;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceSyncProgress {
    pub device_id: i64,
    pub playlist_id: i64,
    pub current: i64,
    pub total: i64,
    pub track_title: String,
    pub status: String, // "copying", "converting", "generating_playlist", "done", "error", "cancelled"
    pub error: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceSyncResult {
    pub device_id: i64,
    pub playlist_id: i64,
    pub synced: i64,
    pub skipped: i64,
    pub failed: i64,
}

pub async fn sync_playlist_to_device(
    app_handle: AppHandle,
    db: Arc<std::sync::Mutex<Connection>>,
    device_id: i64,
    playlist_id: i64,
    cancel_token: Arc<AtomicBool>,
) -> Result<DeviceSyncResult, String> {
    // Get device config
    let (device, tracks_to_sync) = {
        let conn = crate::db::lock(&db)?;
        let device = db_devices::get_device_by_id(&conn, device_id)
            .map_err(|e| format!("Device not found: {}", e))?;
        let tracks = db_devices::get_unsynced_tracks(&conn, device_id, playlist_id)
            .map_err(|e| format!("Failed to get unsynced tracks: {}", e))?;
        (device, tracks)
    };

    let mount_path = device.mount_path.as_deref()
        .ok_or("Device has no mount path — is it connected?")?;

    if !Path::new(mount_path).exists() {
        return Err("Device mount path does not exist — is it connected?".to_string());
    }

    let music_dir = PathBuf::from(mount_path).join(&device.music_dir);
    std::fs::create_dir_all(&music_dir)
        .map_err(|e| format!("Failed to create music directory on device: {}", e))?;

    let total = tracks_to_sync.len() as i64;
    let mut synced = 0i64;
    let mut failed = 0i64;

    // Resolve ffmpeg path for potential transcoding
    let ffmpeg_path = resolve_ffmpeg(&app_handle);

    for (i, track) in tracks_to_sync.iter().enumerate() {
        if cancel_token.load(Ordering::Relaxed) {
            emit_progress(&app_handle, device_id, playlist_id, i as i64, total, &track.title, "cancelled", None);
            return Ok(DeviceSyncResult {
                device_id,
                playlist_id,
                synced,
                skipped: total - synced - failed - (total - i as i64),
                failed,
            });
        }

        let source_path = Path::new(&track.file_path);
        if !source_path.exists() {
            log::warn!("Source file missing for track {}: {}", track.id, track.file_path);
            failed += 1;
            continue;
        }

        let source_format = track.format.as_deref().unwrap_or("unknown");
        let needs_conversion = device.output_format != "original" && source_format != device.output_format;

        let status = if needs_conversion { "converting" } else { "copying" };
        emit_progress(&app_handle, device_id, playlist_id, i as i64, total, &track.title, status, None);

        // Build destination path: Artist/Album/TrackNum - Title.ext
        let dest_ext = if device.output_format == "original" {
            source_path.extension().and_then(|e| e.to_str()).unwrap_or("mp3")
        } else {
            &device.output_format
        };

        let artist_name = sanitize_filename(
            track.artist_name.as_deref().unwrap_or("Unknown Artist"),
        );
        let album_title = sanitize_filename(
            track.album_title.as_deref().unwrap_or("Unknown Album"),
        );
        let track_num = track.track_number.unwrap_or(0);
        let title = sanitize_filename(&track.title);
        let filename = if track_num > 0 {
            format!("{:02} - {}.{}", track_num, title, dest_ext)
        } else {
            format!("{}.{}", title, dest_ext)
        };

        let dest_dir = music_dir.join(&artist_name).join(&album_title);
        if let Err(e) = std::fs::create_dir_all(&dest_dir) {
            log::error!("Failed to create directory {:?}: {}", dest_dir, e);
            failed += 1;
            continue;
        }

        let dest_path = dest_dir.join(&filename);
        let relative_path = dest_path
            .strip_prefix(mount_path)
            .unwrap_or(&dest_path)
            .to_string_lossy()
            .to_string();

        let result = if needs_conversion {
            transcode_file(source_path, &dest_path, &device.output_format, &device.output_bitrate, ffmpeg_path.as_deref()).await
        } else {
            copy_file(source_path, &dest_path).await
        };

        match result {
            Ok(()) => {
                let conn = crate::db::lock(&db)?;
                let _ = db_devices::record_synced_track(
                    &conn,
                    device_id,
                    track.id,
                    playlist_id,
                    &relative_path,
                    dest_ext,
                    None,
                );
                synced += 1;
            }
            Err(e) => {
                log::error!("Failed to sync track {} to device: {}", track.id, e);
                failed += 1;
            }
        }
    }

    // Generate M3U playlist if enabled
    if device.generate_m3u {
        emit_progress(&app_handle, device_id, playlist_id, total, total, "", "generating_playlist", None);

        let conn = crate::db::lock(&db)?;
        if let Err(e) = generate_m3u(&conn, device_id, playlist_id, &music_dir) {
            log::error!("Failed to generate M3U: {}", e);
        }
    }

    // Update sync timestamp
    {
        let conn = crate::db::lock(&db)?;
        let _ = db_devices::update_playlist_sync_time(&conn, device_id, playlist_id);
    }

    emit_progress(&app_handle, device_id, playlist_id, total, total, "", "done", None);

    Ok(DeviceSyncResult {
        device_id,
        playlist_id,
        synced,
        skipped: 0,
        failed,
    })
}

async fn copy_file(src: &Path, dest: &Path) -> Result<(), String> {
    tokio::fs::copy(src, dest)
        .await
        .map_err(|e| format!("Copy failed: {}", e))?;
    Ok(())
}

async fn transcode_file(
    src: &Path,
    dest: &Path,
    format: &str,
    bitrate: &str,
    ffmpeg_path: Option<&str>,
) -> Result<(), String> {
    let ffmpeg = ffmpeg_path.unwrap_or("ffmpeg");

    let mut args = vec![
        "-i".to_string(),
        src.to_string_lossy().to_string(),
        "-y".to_string(), // overwrite
    ];

    match format {
        "mp3" => {
            args.extend_from_slice(&[
                "-codec:a".to_string(),
                "libmp3lame".to_string(),
                "-b:a".to_string(),
                format!("{}k", bitrate),
            ]);
        }
        "opus" => {
            args.extend_from_slice(&[
                "-codec:a".to_string(),
                "libopus".to_string(),
                "-b:a".to_string(),
                format!("{}k", bitrate),
            ]);
        }
        "flac" => {
            args.extend_from_slice(&[
                "-codec:a".to_string(),
                "flac".to_string(),
            ]);
        }
        _ => {
            // Copy codec as-is
            args.extend_from_slice(&["-codec:a".to_string(), "copy".to_string()]);
        }
    }

    args.push(dest.to_string_lossy().to_string());

    let output = tokio::process::Command::new(ffmpeg)
        .args(&args)
        .output()
        .await
        .map_err(|e| format!("ffmpeg failed to start: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("ffmpeg error: {}", stderr));
    }

    Ok(())
}

fn generate_m3u(
    conn: &Connection,
    device_id: i64,
    playlist_id: i64,
    music_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    // Get playlist name
    let playlist_name: String = conn.query_row(
        "SELECT name FROM playlists WHERE id = ?1",
        [playlist_id],
        |row| row.get(0),
    )?;

    // Get synced tracks for this playlist on this device
    let mut stmt = conn.prepare(
        "SELECT dts.file_path_on_device, t.title, t.duration_ms
         FROM device_track_sync dts
         JOIN tracks t ON t.id = dts.track_id
         WHERE dts.device_id = ?1 AND dts.playlist_id = ?2
         ORDER BY (SELECT position FROM playlist_tracks WHERE playlist_id = ?2 AND track_id = dts.track_id)",
    )?;

    let entries: Vec<(String, String, Option<i64>)> = stmt
        .query_map(rusqlite::params![device_id, playlist_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    if entries.is_empty() {
        return Ok(());
    }

    let m3u_filename = sanitize_filename(&playlist_name) + ".m3u";
    let m3u_path = music_dir.join(&m3u_filename);

    let mut content = String::from("#EXTM3U\n");
    for (file_path, title, duration_ms) in &entries {
        let duration_secs = duration_ms.unwrap_or(0) / 1000;
        content.push_str(&format!("#EXTINF:{},{}\n", duration_secs, title));
        content.push_str(file_path);
        content.push('\n');
    }

    std::fs::write(&m3u_path, content)?;
    log::info!("Generated M3U playlist: {:?}", m3u_path);

    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn emit_progress(
    app_handle: &AppHandle,
    device_id: i64,
    playlist_id: i64,
    current: i64,
    total: i64,
    track_title: &str,
    status: &str,
    error: Option<String>,
) {
    let progress = DeviceSyncProgress {
        device_id,
        playlist_id,
        current,
        total,
        track_title: track_title.to_string(),
        status: status.to_string(),
        error,
    };
    let _ = app_handle.emit("device-sync-progress", &progress);
}

fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn resolve_ffmpeg(app_handle: &AppHandle) -> Option<String> {
    use crate::download::setup;
    let bin_dir = setup::get_bin_dir(app_handle);
    let local = setup::get_ffmpeg_path(&bin_dir);
    if local.exists() {
        Some(local.to_string_lossy().to_string())
    } else {
        Some("ffmpeg".to_string())
    }
}
