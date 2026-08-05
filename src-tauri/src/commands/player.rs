use std::sync::Arc;
use rusqlite::params;
use tauri::State;

use crate::audio::engine::{AudioEngine, PlaybackState, PlayerCommand, RepeatMode};
use crate::audio::queue::QueueTrack;
use crate::db::DbPool;

/// Build a QueueTrack from a track ID by reading from the database.
fn track_from_db(conn: &rusqlite::Connection, id: i64) -> Result<QueueTrack, String> {
    let track = conn.query_row(
        "SELECT t.id, t.title, a.name, al.title, t.duration_ms, t.file_path, t.cover_art_path, t.gain_db
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.id = ?1",
        rusqlite::params![id],
        |row| {
            Ok(QueueTrack {
                id: row.get(0)?,
                title: row.get(1)?,
                artist_name: row.get(2)?,
                album_title: row.get(3)?,
                duration_ms: row.get(4)?,
                file_path: row.get(5)?,
                cover_art_path: row.get(6)?,
                gain_db: row.get(7)?,
            })
        },
    )
    .map_err(|e| format!("Track not found (id={}): {}", id, e))?;
    log::info!("[player] Loaded track from DB: id={}, title=\"{}\", path=\"{}\"", track.id, track.title, track.file_path);
    Ok(track)
}

/// Compute-on-first-play fallback: when normalization is enabled and the
/// track has no measured gain yet, measure it in the background so the gain
/// applies from the next play of that track.
fn maybe_compute_gain(
    db: &Arc<DbPool>,
    engine: &Arc<AudioEngine>,
    conn: &rusqlite::Connection,
    track: &QueueTrack,
) {
    if track.gain_db.is_some() {
        return;
    }
    let normalize = crate::db::settings::get_setting(conn, "normalize_volume")
        .ok()
        .flatten()
        .as_deref()
        == Some("true");
    if !normalize {
        return;
    }
    let ffmpeg = engine.ffmpeg_path().unwrap_or("ffmpeg").to_string();
    crate::audio::loudness::compute_gain_if_missing_async(
        Arc::clone(db),
        ffmpeg,
        track.id,
        track.file_path.clone(),
    );
}

#[tauri::command]
pub fn player_play_track(
    db: State<'_, Arc<DbPool>>,
    engine: State<'_, Arc<AudioEngine>>,
    track_id: i64,
) -> Result<(), String> {
    log::info!("[player] player_play_track: id={}", track_id);
    let conn = db.lock().map_err(|e| {
        log::error!("[player] DB lock failed: {}", e);
        e.to_string()
    })?;
    let queue_track = track_from_db(&conn, track_id)?;
    maybe_compute_gain(db.inner(), engine.inner(), &conn, &queue_track);
    engine.send(PlayerCommand::PlaySingle(queue_track));
    Ok(())
}

#[tauri::command]
pub fn player_play_tracks(
    db: State<'_, Arc<DbPool>>,
    engine: State<'_, Arc<AudioEngine>>,
    track_ids: Vec<i64>,
    start_index: usize,
) -> Result<(), String> {
    log::info!("[player] player_play_tracks: {} tracks, start_index={}", track_ids.len(), start_index);
    if track_ids.is_empty() {
        return Ok(());
    }
    let conn = db.lock().map_err(|e| {
        log::error!("[player] DB lock failed: {}", e);
        e.to_string()
    })?;
    // Batch-load all tracks in one query, then reorder to match requested order
    let placeholders: Vec<String> = (1..=track_ids.len()).map(|i| format!("?{}", i)).collect();
    let sql = format!(
        "SELECT t.id, t.title, a.name, al.title, t.duration_ms, t.file_path, t.cover_art_path, t.gain_db
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.id IN ({})",
        placeholders.join(",")
    );
    let params: Vec<&dyn rusqlite::types::ToSql> = track_ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(params.as_slice(), |row| {
        Ok(QueueTrack {
            id: row.get(0)?,
            title: row.get(1)?,
            artist_name: row.get(2)?,
            album_title: row.get(3)?,
            duration_ms: row.get(4)?,
            file_path: row.get(5)?,
            cover_art_path: row.get(6)?,
            gain_db: row.get(7)?,
        })
    }).map_err(|e| e.to_string())?;
    let loaded: std::collections::HashMap<i64, QueueTrack> = rows
        .filter_map(|r| r.ok())
        .map(|t| (t.id, t))
        .collect();
    // Preserve original order from track_ids. Clone (don't remove) so a track
    // appearing twice in a playlist stays twice in the queue — removing
    // collapsed duplicates and shifted start_index onto the wrong track.
    let tracks: Vec<QueueTrack> = track_ids.iter()
        .filter_map(|id| loaded.get(id).cloned())
        .collect();
    if tracks.is_empty() {
        return Err("No valid tracks found".to_string());
    }
    let clamped_index = start_index.min(tracks.len() - 1);
    maybe_compute_gain(db.inner(), engine.inner(), &conn, &tracks[clamped_index]);
    // Pre-decode the starting track on this thread (Tauri thread pool) so the
    // audio thread can start playback instantly without blocking or stuttering.
    let source = AudioEngine::decode_track(&tracks[clamped_index].file_path, engine.ffmpeg_path()).ok();
    engine.send(PlayerCommand::Play {
        tracks,
        start_index: clamped_index,
        source,
    });
    Ok(())
}

#[tauri::command]
pub fn player_pause(engine: State<'_, Arc<AudioEngine>>) -> Result<(), String> {
    engine.send(PlayerCommand::Pause);
    Ok(())
}

#[tauri::command]
pub fn player_resume(engine: State<'_, Arc<AudioEngine>>) -> Result<(), String> {
    engine.send(PlayerCommand::Resume);
    Ok(())
}

#[tauri::command]
pub fn player_stop(engine: State<'_, Arc<AudioEngine>>) -> Result<(), String> {
    engine.send(PlayerCommand::Stop);
    Ok(())
}

#[tauri::command]
pub fn player_next(engine: State<'_, Arc<AudioEngine>>) -> Result<(), String> {
    engine.send(PlayerCommand::Next);
    Ok(())
}

#[tauri::command]
pub fn player_prev(engine: State<'_, Arc<AudioEngine>>) -> Result<(), String> {
    engine.send(PlayerCommand::Prev);
    Ok(())
}

#[tauri::command]
pub fn player_seek(
    engine: State<'_, Arc<AudioEngine>>,
    position_seconds: f64,
) -> Result<(), String> {
    engine.send(PlayerCommand::Seek(position_seconds));
    Ok(())
}

#[tauri::command]
pub fn player_set_volume(
    engine: State<'_, Arc<AudioEngine>>,
    volume: f64,
) -> Result<(), String> {
    engine.send(PlayerCommand::SetVolume(volume));
    Ok(())
}

#[tauri::command]
pub fn player_set_shuffle(
    engine: State<'_, Arc<AudioEngine>>,
    shuffle: bool,
) -> Result<(), String> {
    engine.send(PlayerCommand::SetShuffle(shuffle));
    Ok(())
}

#[tauri::command]
pub fn player_set_repeat(
    engine: State<'_, Arc<AudioEngine>>,
    mode: String,
) -> Result<(), String> {
    let repeat = match mode.as_str() {
        "all" => RepeatMode::All,
        "one" => RepeatMode::One,
        _ => RepeatMode::Off,
    };
    engine.send(PlayerCommand::SetRepeat(repeat));
    Ok(())
}

#[tauri::command]
pub fn player_add_to_queue(
    db: State<'_, Arc<DbPool>>,
    engine: State<'_, Arc<AudioEngine>>,
    track_id: i64,
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let queue_track = track_from_db(&conn, track_id)?;
    engine.send(PlayerCommand::AddToQueue(queue_track));
    Ok(())
}

#[tauri::command]
pub fn player_add_next(
    db: State<'_, Arc<DbPool>>,
    engine: State<'_, Arc<AudioEngine>>,
    track_id: i64,
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let queue_track = track_from_db(&conn, track_id)?;
    engine.send(PlayerCommand::AddNext(queue_track));
    Ok(())
}

#[tauri::command]
pub fn player_move_in_queue(
    engine: State<'_, Arc<AudioEngine>>,
    from_index: usize,
    to_index: usize,
) -> Result<(), String> {
    engine.send(PlayerCommand::MoveInQueue { from: from_index, to: to_index });
    Ok(())
}

#[tauri::command]
pub fn player_skip_to(
    engine: State<'_, Arc<AudioEngine>>,
    index: usize,
) -> Result<(), String> {
    engine.send(PlayerCommand::SkipTo(index));
    Ok(())
}

#[tauri::command]
pub fn player_remove_from_queue(
    engine: State<'_, Arc<AudioEngine>>,
    index: usize,
) -> Result<(), String> {
    engine.send(PlayerCommand::RemoveFromQueue(index));
    Ok(())
}

#[tauri::command]
pub fn player_clear_queue(engine: State<'_, Arc<AudioEngine>>) -> Result<(), String> {
    engine.send(PlayerCommand::ClearQueue);
    Ok(())
}

#[tauri::command]
pub fn player_get_state(engine: State<'_, Arc<AudioEngine>>) -> Result<PlaybackState, String> {
    Ok(engine.get_state())
}

#[tauri::command]
pub fn player_get_queue(engine: State<'_, Arc<AudioEngine>>) -> Result<(Vec<QueueTrack>, Option<usize>), String> {
    Ok(engine.get_queue())
}

#[tauri::command]
pub fn player_get_audio_devices() -> Vec<(String, bool)> {
    AudioEngine::list_output_devices()
}

#[tauri::command]
pub fn player_set_audio_device(
    engine: State<'_, Arc<AudioEngine>>,
    device_name: Option<String>,
) -> Result<(), String> {
    engine.send(PlayerCommand::SwitchDevice(device_name));
    Ok(())
}

/// Enable/disable ReplayGain-style volume normalization in the engine.
/// Persist the setting separately via `settings_set("normalize_volume", ...)`.
#[tauri::command]
pub fn player_set_normalize(
    engine: State<'_, Arc<AudioEngine>>,
    enabled: bool,
) -> Result<(), String> {
    engine.send(PlayerCommand::SetNormalize(enabled));
    Ok(())
}

/// Set the crossfade duration (seconds, 0-12; 0 = off/gapless).
/// Persist the setting separately via `settings_set("crossfade_seconds", ...)`.
#[tauri::command]
pub fn player_set_crossfade(
    engine: State<'_, Arc<AudioEngine>>,
    seconds: f64,
) -> Result<(), String> {
    let ms = (seconds.clamp(0.0, 12.0) * 1000.0).round() as u64;
    engine.send(PlayerCommand::SetCrossfade(ms));
    Ok(())
}

/// Start a background loudness scan filling missing `gain_db` values for the
/// whole library. Returns immediately; progress arrives as
/// `gain-scan-progress` events. Errors if a scan is already running.
#[tauri::command]
pub fn audio_scan_gains(
    db: State<'_, Arc<DbPool>>,
    engine: State<'_, Arc<AudioEngine>>,
    app: tauri::AppHandle,
) -> Result<(), String> {
    let ffmpeg = engine.ffmpeg_path().unwrap_or("ffmpeg").to_string();
    crate::audio::loudness::start_scan(db.inner().clone(), ffmpeg, app)
}

/// Cancel a running loudness scan (best effort — finishes the current file).
#[tauri::command]
pub fn audio_cancel_gain_scan() -> Result<(), String> {
    crate::audio::loudness::cancel_scan();
    Ok(())
}

/// Record a play for a track (increment play_count, set last_played_at),
/// append a listening-history event, and queue a Last.fm scrobble when
/// connected + enabled.
#[tauri::command]
pub fn player_record_play(
    db: State<'_, Arc<DbPool>>,
    track_id: i64,
) -> Result<(), String> {
    let mut should_flush = false;
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        crate::db::tracks::record_play(&conn, track_id).map_err(|e| e.to_string())?;

        // Listening-history event row (drives the stats page)
        conn.execute("INSERT INTO plays (track_id) VALUES (?1)", params![track_id])
            .map_err(|e| e.to_string())?;

        // Last.fm: queue a scrobble (offline-safe), flushed in the background
        if crate::metadata::scrobble::scrobbling_session(&conn).is_some() {
            if let Ok(track) = track_from_db(&conn, track_id) {
                if let Some(artist) = track.artist_name.as_deref().filter(|a| !a.is_empty()) {
                    let played_at = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs() as i64)
                        .unwrap_or(0);
                    if let Err(e) = crate::metadata::scrobble::queue_scrobble(
                        &conn,
                        artist,
                        &track.title,
                        track.album_title.as_deref(),
                        track.duration_ms.map(|ms| ms / 1000),
                        played_at,
                    ) {
                        log::warn!("[lastfm] Failed to queue scrobble: {}", e);
                    } else {
                        should_flush = true;
                    }
                }
            }
        }
    }
    if should_flush {
        let db_arc = db.inner().clone();
        tauri::async_runtime::spawn(async move {
            crate::metadata::scrobble::flush_pending(&db_arc).await;
        });
    }
    Ok(())
}

/// Filter a list of track IDs down to those that still exist in the library,
/// preserving the input order (duplicates included). Used to restore a
/// persisted queue while gracefully skipping deleted tracks.
#[tauri::command]
pub fn player_filter_existing_tracks(
    db: State<'_, Arc<DbPool>>,
    track_ids: Vec<i64>,
) -> Result<Vec<i64>, String> {
    if track_ids.is_empty() {
        return Ok(Vec::new());
    }
    let conn = db.lock().map_err(|e| e.to_string())?;
    let placeholders: Vec<String> = (1..=track_ids.len()).map(|i| format!("?{}", i)).collect();
    let sql = format!(
        "SELECT id FROM tracks WHERE id IN ({})",
        placeholders.join(",")
    );
    let params: Vec<&dyn rusqlite::types::ToSql> =
        track_ids.iter().map(|id| id as &dyn rusqlite::types::ToSql).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let existing: std::collections::HashSet<i64> = stmt
        .query_map(params.as_slice(), |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(track_ids.into_iter().filter(|id| existing.contains(id)).collect())
}

/// Get random tracks from the library, optionally excluding certain track IDs.
#[tauri::command]
pub fn player_random_tracks(
    db: State<'_, Arc<DbPool>>,
    exclude_ids: Vec<i64>,
    limit: usize,
) -> Result<Vec<i64>, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let limit = if limit == 0 { 1 } else { limit };
    let limit_i64 = limit as i64;
    if exclude_ids.is_empty() {
        let mut stmt = conn.prepare(
            "SELECT id FROM tracks ORDER BY RANDOM() LIMIT ?1"
        ).map_err(|e| e.to_string())?;
        let ids: Vec<i64> = stmt.query_map(params![limit_i64], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    } else {
        let param_start = 1;
        let placeholders: Vec<String> = (param_start..param_start + exclude_ids.len()).map(|i| format!("?{}", i)).collect();
        let limit_param = param_start + exclude_ids.len();
        let sql = format!(
            "SELECT id FROM tracks WHERE id NOT IN ({}) ORDER BY RANDOM() LIMIT ?{}",
            placeholders.join(","), limit_param
        );
        let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> = exclude_ids.iter()
            .map(|id| Box::new(*id) as Box<dyn rusqlite::types::ToSql>)
            .collect();
        all_params.push(Box::new(limit_i64));
        let param_refs: Vec<&dyn rusqlite::types::ToSql> = all_params.iter()
            .map(|p| p.as_ref())
            .collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let ids: Vec<i64> = stmt.query_map(param_refs.as_slice(), |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        Ok(ids)
    }
}
