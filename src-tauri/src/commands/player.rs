use std::sync::Arc;
use tauri::State;

use crate::audio::engine::{AudioEngine, PlaybackState, PlayerCommand, RepeatMode};
use crate::audio::queue::QueueTrack;
use crate::db::DbPool;

/// Build a QueueTrack from a track ID by reading from the database.
fn track_from_db(conn: &rusqlite::Connection, id: i64) -> Result<QueueTrack, String> {
    conn.query_row(
        "SELECT t.id, t.title, a.name, al.title, t.duration_ms, t.file_path, t.cover_art_path
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
            })
        },
    )
    .map_err(|e| format!("Track not found: {}", e))
}

#[tauri::command]
pub fn player_play_track(
    db: State<'_, Arc<DbPool>>,
    engine: State<'_, Arc<AudioEngine>>,
    track_id: i64,
) -> Result<(), String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let queue_track = track_from_db(&conn, track_id)?;
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
    let conn = db.lock().map_err(|e| e.to_string())?;
    let mut tracks = Vec::new();
    for id in track_ids {
        tracks.push(track_from_db(&conn, id)?);
    }
    engine.send(PlayerCommand::Play {
        tracks,
        start_index,
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
