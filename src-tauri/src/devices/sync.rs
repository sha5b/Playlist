use super::detect;
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
    let result = run_sync(&app_handle, db, device_id, playlist_id, cancel_token).await;
    if let Err(e) = &result {
        // Every failure must emit an "error" progress event: the frontend only
        // clears its progress state on "done"/"error"/"cancelled", so a silent
        // early return would freeze the sync buttons forever.
        emit_progress(&app_handle, device_id, playlist_id, 0, 0, "", "error", Some(e.clone()));
    }
    result
}

async fn run_sync(
    app_handle: &AppHandle,
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
        let tracks =
            db_devices::get_unsynced_tracks(&conn, device_id, playlist_id, &device.output_format)
                .map_err(|e| format!("Failed to get unsynced tracks: {}", e))?;
        (device, tracks)
    };

    let mount_path = device.mount_path.as_deref()
        .ok_or("Device has no mount path — is it connected?")?;

    if !Path::new(mount_path).exists() {
        return Err("Device mount path does not exist — is it connected?".to_string());
    }

    let music_subdir = Path::new(&device.music_dir);
    if music_subdir.as_os_str().is_empty()
        || music_subdir.is_absolute()
        || music_subdir.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("Invalid device music directory".to_string());
    }
    let target_format = device.output_format.to_ascii_lowercase();
    if !matches!(target_format.as_str(), "original" | "mp3" | "opus" | "flac") {
        return Err(format!("Unsupported device output format: {target_format}"));
    }

    let music_dir = PathBuf::from(mount_path).join(music_subdir);
    std::fs::create_dir_all(&music_dir)
        .map_err(|e| format!("Failed to create music directory on device: {}", e))?;

    // Free-space guard: estimate required bytes from source file sizes and refuse to
    // start if the device clearly can't hold them (avoids opaque per-file copy failures).
    // Scale by target format: transcoding lossy sources to FLAC blows the size up
    // (~6x); mp3/opus/original stay around 1x (usually smaller, so conservative).
    let estimated_bytes: u64 = tracks_to_sync
        .iter()
        .filter_map(|t| {
            let len = std::fs::metadata(&t.file_path).ok()?.len();
            let src_format = t.format.as_deref().unwrap_or("").to_ascii_lowercase();
            let source_is_lossless =
                matches!(src_format.as_str(), "flac" | "wav" | "aiff" | "aif" | "alac");
            let factor = if device.output_format == "flac" && !source_is_lossless {
                6.0
            } else {
                1.0
            };
            Some((len as f64 * factor) as u64)
        })
        .sum();
    if let (_, Some(free)) = detect::get_fs_stats(Path::new(mount_path)) {
        // Keep a 5% headroom margin.
        if estimated_bytes as i64 > free - (free / 20) {
            return Err(format!(
                "Not enough space on device: need ~{} MB but only {} MB free",
                estimated_bytes / 1_048_576,
                free / 1_048_576
            ));
        }
    }

    let total = tracks_to_sync.len() as i64;
    let mut synced = 0i64;
    let mut failed = 0i64;

    // Resolve ffmpeg path for potential transcoding
    let ffmpeg_path = resolve_ffmpeg(app_handle);

    for (i, track) in tracks_to_sync.iter().enumerate() {
        if cancel_token.load(Ordering::Relaxed) {
            emit_progress(app_handle, device_id, playlist_id, i as i64, total, &track.title, "cancelled", None);
            return Ok(DeviceSyncResult {
                device_id,
                playlist_id,
                synced,
                skipped: total - i as i64, // tracks not yet attempted
                failed,
            });
        }

        // Detect mid-sync disconnect and abort cleanly rather than failing every
        // remaining track against a dead mount point.
        if !Path::new(mount_path).exists() {
            emit_progress(app_handle, device_id, playlist_id, i as i64, total, &track.title, "error", Some("Device disconnected".into()));
            return Err("Device disconnected during sync".to_string());
        }

        let source_path = Path::new(&track.file_path);
        if !source_path.exists() {
            log::warn!("Source file missing for track {}: {}", track.id, track.file_path);
            failed += 1;
            continue;
        }

        // The file extension is authoritative for sync. Database format metadata
        // can be stale after a file replacement, and must never cause a file to be
        // transcoded or merely renamed to the wrong container.
        let source_format = source_path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp3")
            .to_ascii_lowercase();
        let needs_conversion = target_format != "original" && source_format != target_format;

        let status = if needs_conversion { "converting" } else { "copying" };
        // 1-based: this is the track currently being processed ("1 of N", not "0 of N")
        emit_progress(app_handle, device_id, playlist_id, i as i64 + 1, total, &track.title, status, None);

        // Build destination path: Artist/Album/TrackNum - Title.ext
        let dest_ext = if target_format == "original" {
            source_format.as_str()
        } else {
            target_format.as_str()
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
            // No track number — disambiguate by track id so distinct tracks that share a
            // title don't sanitize to the same path and overwrite each other on the device.
            format!("{} [{}].{}", title, track.id, dest_ext)
        };

        let dest_dir = music_dir.join(&artist_name).join(&album_title);
        if let Err(e) = std::fs::create_dir_all(&dest_dir) {
            log::error!("Failed to create directory {:?}: {}", dest_dir, e);
            failed += 1;
            continue;
        }

        let dest_path = dest_dir.join(&filename);
        // Path recorded for the M3U must be relative to the music_dir (where the M3U
        // lives), using forward slashes — not relative to the mount root, which would
        // double the music-dir segment and break the playlist on the device.
        let relative_path = dest_path
            .strip_prefix(&music_dir)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| filename.clone());

        let source_size = std::fs::metadata(source_path).ok().map(|m| m.len() as i64);

        // If this track was already synced under another playlist to the exact
        // same on-device path, skip the physical copy — but still record the
        // (device, track, playlist) row so this playlist's M3U/counts include it.
        let already_on_device = {
            let conn = crate::db::lock(&db)?;
            db_devices::is_file_already_on_device(&conn, device_id, track.id, &relative_path)
                .unwrap_or(false)
                && dest_path.exists()
        };

        let previous_path = {
            let conn = crate::db::lock(&db)?;
            db_devices::get_synced_track_path(&conn, device_id, track.id, playlist_id)
                .map_err(|e| format!("Failed to read sync history: {e}"))?
        };

        let result = if already_on_device {
            Ok(())
        } else if needs_conversion {
            transcode_file(source_path, &dest_path, &target_format, &device.output_bitrate, ffmpeg_path.as_deref()).await
        } else {
            copy_file(source_path, &dest_path).await
        };

        match result {
            Ok(()) => {
                // Flush the copied data to the device before we ever report
                // success/"done" — USB media is frequently yanked right after.
                if !already_on_device {
                    match std::fs::File::open(&dest_path) {
                        Ok(f) => {
                            if let Err(e) = f.sync_all() {
                                log::warn!("Failed to flush {:?} to device: {}", dest_path, e);
                            }
                        }
                        Err(e) => log::warn!("Failed to open {:?} for flush: {}", dest_path, e),
                    }
                }
                let conn = crate::db::lock(&db)?;
                db_devices::record_synced_track(
                    &conn,
                    device_id,
                    track.id,
                    playlist_id,
                    &relative_path,
                    dest_ext,
                    source_size,
                    None,
                )
                .map_err(|e| format!("Failed to record synced track: {e}"))?;
                // Changing the output format changes the destination filename.
                // Once no playlist references the old path, remove it so switching
                // from Opus to Original doesn't leave a duplicate Opus library behind.
                if let Some(old_path) = previous_path.filter(|p| p != &relative_path) {
                    let still_referenced = db_devices::is_device_file_referenced(
                        &conn,
                        device_id,
                        &old_path,
                    )
                    .unwrap_or(true);
                    if !still_referenced {
                        let old_abs = music_dir.join(&old_path);
                        if let Err(e) = std::fs::remove_file(&old_abs) {
                            if e.kind() != std::io::ErrorKind::NotFound {
                                log::warn!("Failed to remove superseded file {:?}: {}", old_abs, e);
                            }
                        }
                    }
                }
                synced += 1;
            }
            Err(e) => {
                log::error!("Failed to sync track {} to device: {}", track.id, e);
                failed += 1;
            }
        }
    }

    // Reconcile: drop sync rows for tracks no longer in the playlist, and delete
    // their files from the device when no other sync row references the same path
    // (otherwise removed tracks linger in M3Us and files accumulate forever).
    {
        let conn = crate::db::lock(&db)?;
        let stale = db_devices::get_stale_synced_tracks(&conn, device_id, playlist_id)
            .map_err(|e| format!("Failed to find stale synced tracks: {}", e))?;
        for (track_id, path_on_device) in stale {
            if let Err(e) = db_devices::remove_synced_track(&conn, device_id, track_id, playlist_id) {
                log::warn!("Failed to remove stale sync row for track {}: {}", track_id, e);
                continue;
            }
            let still_referenced =
                db_devices::is_device_file_referenced(&conn, device_id, &path_on_device)
                    .unwrap_or(true);
            if !still_referenced {
                let abs = music_dir.join(&path_on_device);
                if abs.exists() {
                    if let Err(e) = std::fs::remove_file(&abs) {
                        log::warn!("Failed to remove stale file {:?} from device: {}", abs, e);
                    } else {
                        log::info!("Removed stale file from device: {:?}", abs);
                    }
                }
            }
        }
    }

    // Generate M3U playlist if enabled. If disabled (or the playlist became
    // empty), remove an older generated file so the device isn't left with a
    // stale playlist pointing at deleted or superseded tracks.
    if device.generate_m3u {
        emit_progress(app_handle, device_id, playlist_id, total, total, "", "generating_playlist", None);

        let conn = crate::db::lock(&db)?;
        generate_m3u(&conn, device_id, playlist_id, &music_dir)
            .map_err(|e| format!("Failed to generate M3U: {e}"))?;
    } else {
        let conn = crate::db::lock(&db)?;
        remove_m3u(&conn, playlist_id, &music_dir)
            .map_err(|e| format!("Failed to remove disabled M3U: {e}"))?;
    }

    // Update sync timestamp
    {
        let conn = crate::db::lock(&db)?;
        db_devices::update_playlist_sync_time(&conn, device_id, playlist_id)
            .map_err(|e| format!("Failed to update sync time: {e}"))?;
    }

    if failed > 0 {
        return Err(format!("{} track{} failed to sync", failed, if failed == 1 { "" } else { "s" }));
    }

    emit_progress(app_handle, device_id, playlist_id, total, total, "", "done", None);

    Ok(DeviceSyncResult {
        device_id,
        playlist_id,
        synced,
        skipped: 0,
        failed,
    })
}

async fn copy_file(src: &Path, dest: &Path) -> Result<(), String> {
    // Copy to a temp file then atomically rename, so a disconnect mid-copy leaves a
    // stray ".part" file rather than a truncated file masquerading as a real track.
    let tmp = dest.with_extension("part");
    if let Err(e) = tokio::fs::copy(src, &tmp).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Copy failed: {}", e));
    }
    finalize_temp(&tmp, dest).await
}

/// Move a fully-written temp file onto its final name. FUSE backends such as
/// the GVfs MTP mount (phones) may not support rename — fall back to a plain
/// copy so syncing to a phone still works, just without the atomic swap.
async fn finalize_temp(tmp: &Path, dest: &Path) -> Result<(), String> {
    if let Err(e) = tokio::fs::rename(tmp, dest).await {
        if tokio::fs::copy(tmp, dest).await.is_ok() {
            let _ = tokio::fs::remove_file(tmp).await;
            return Ok(());
        }
        let _ = tokio::fs::remove_file(tmp).await;
        return Err(format!("Finalize failed: {}", e));
    }
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

    // The stored bitrate is free-form — strip any trailing "k"/"kbps" so a
    // value like "320k" doesn't become the invalid "320kk", and fall back to
    // a sane default if it isn't numeric at all.
    let bitrate: String = {
        let digits: String = bitrate.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() { "320".to_string() } else { digits }
    };
    let bitrate = bitrate.as_str();

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
            // Unknown target format: let ffmpeg pick the container's default
            // encoder. `-codec:a copy` here put e.g. FLAC data in an .m4a
            // container, which players reject.
            args.extend_from_slice(&["-b:a".to_string(), format!("{}k", bitrate)]);
        }
    }

    // Like copy_file's ".part" pattern: write to a temp name and rename on
    // success, so a mid-transcode disconnect/kill never leaves a truncated file
    // masquerading as a real track. Keep the real extension last so ffmpeg
    // still infers the output container from it.
    let dest_ext = dest.extension().and_then(|e| e.to_str()).unwrap_or("tmp");
    let tmp = dest.with_extension(format!("part.{}", dest_ext));
    args.push(tmp.to_string_lossy().to_string());

    let mut cmd = tokio::process::Command::new(ffmpeg);
    cmd.args(&args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // IDLE_PRIORITY_CLASS | CREATE_NO_WINDOW
        cmd.creation_flags(0x00000040 | 0x08000000);
    }
    let output = cmd
        .output()
        .await
        .map_err(|e| {
            format!("ffmpeg failed to start: {}", e)
        });

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            let _ = tokio::fs::remove_file(&tmp).await;
            return Err(e);
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("ffmpeg error: {}", stderr));
    }

    finalize_temp(&tmp, dest).await
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

    // Get synced tracks for this playlist on this device. The inner join on
    // playlist_tracks drops tracks that were removed from the playlist (their
    // sync rows are reconciled separately) and guarantees a non-NULL position.
    let mut stmt = conn.prepare(
        "SELECT dts.file_path_on_device, t.title, t.duration_ms
         FROM device_track_sync dts
         JOIN playlist_tracks pt ON pt.playlist_id = dts.playlist_id AND pt.track_id = dts.track_id
         JOIN tracks t ON t.id = dts.track_id
         WHERE dts.device_id = ?1 AND dts.playlist_id = ?2
         ORDER BY pt.position",
    )?;

    let entries: Vec<(String, String, Option<i64>)> = stmt
        .query_map(rusqlite::params![device_id, playlist_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let m3u_filename = sanitize_filename(&playlist_name) + ".m3u";
    let m3u_path = music_dir.join(&m3u_filename);

    if entries.is_empty() {
        if let Err(e) = std::fs::remove_file(&m3u_path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(e.into());
            }
        }
        return Ok(());
    }

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

fn remove_m3u(
    conn: &Connection,
    playlist_id: i64,
    music_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let playlist_name: String = conn.query_row(
        "SELECT name FROM playlists WHERE id = ?1",
        [playlist_id],
        |row| row.get(0),
    )?;
    let path = music_dir.join(sanitize_filename(&playlist_name) + ".m3u");
    if let Err(e) = std::fs::remove_file(path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(e.into());
        }
    }
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
    // Replace characters illegal on FAT32/exFAT/NTFS plus control chars, strip trailing
    // dots/spaces (Windows removes them), avoid reserved DOS device names, and never
    // return an empty string.
    let mut s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .trim_end_matches('.')
        .trim()
        .to_string();

    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&s.to_ascii_uppercase().as_str()) {
        s.push('_');
    }
    if s.is_empty() {
        s.push_str("untitled");
    }
    s
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
