use rusqlite::{params, Connection, Row};

use super::models::{Playlist, Track, TrackPage};
use super::tracks::{row_to_track, TRACK_COLUMNS};

fn row_to_playlist(row: &Row) -> Result<Playlist, rusqlite::Error> {
    Ok(Playlist {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        cover_art_path: row.get(3)?,
        source_platform: row.get(4)?,
        source_url: row.get(5)?,
        track_count: row.get(6)?,
        total_duration_ms: row.get(7)?,
        is_synced: row.get::<_, i64>(8)? != 0,
        last_synced_at: row.get(9)?,
        created_at: row.get(10)?,
        is_smart: row.get::<_, i64>(11)? != 0,
        rules: row.get(12)?,
    })
}

const PLAYLIST_COLUMNS: &str =
    "id, name, description, cover_art_path, source_platform, source_url,
     track_count, total_duration_ms, is_synced, last_synced_at, created_at,
     is_smart, rules";

pub fn get_playlists(conn: &Connection) -> Result<Vec<Playlist>, rusqlite::Error> {
    let sql = format!(
        "SELECT {} FROM playlists ORDER BY created_at DESC",
        PLAYLIST_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let playlists = stmt
        .query_map([], row_to_playlist)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(playlists)
}

pub fn create_playlist(
    conn: &Connection,
    name: &str,
    description: Option<&str>,
) -> Result<Playlist, rusqlite::Error> {
    conn.execute(
        "INSERT INTO playlists (name, description) VALUES (?1, ?2)",
        params![name, description],
    )?;

    let id = conn.last_insert_rowid();
    get_playlist(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn create_smart_playlist(
    conn: &Connection,
    name: &str,
    description: Option<&str>,
    rules_json: &str,
) -> Result<Playlist, rusqlite::Error> {
    conn.execute(
        "INSERT INTO playlists (name, description, is_smart, rules) VALUES (?1, ?2, 1, ?3)",
        params![name, description, rules_json],
    )?;

    let id = conn.last_insert_rowid();
    get_playlist(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn update_smart_playlist(
    conn: &Connection,
    id: i64,
    name: Option<&str>,
    description: Option<&str>,
    rules_json: Option<&str>,
) -> Result<Playlist, rusqlite::Error> {
    if let Some(rules) = rules_json {
        conn.execute(
            "UPDATE playlists SET rules = ?1, updated_at = datetime('now')
             WHERE id = ?2 AND is_smart = 1",
            params![rules, id],
        )?;
    }
    update_playlist(conn, id, name, description)
}

/// Returns the rule JSON when the playlist is a smart playlist, None otherwise.
pub fn smart_rules(conn: &Connection, playlist_id: i64) -> Result<Option<String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT rules FROM playlists WHERE id = ?1 AND is_smart = 1")?;
    let result = stmt
        .query_map(params![playlist_id], |row| row.get::<_, Option<String>>(0))?
        .next()
        .transpose()
        .map(|opt| opt.flatten());
    result
}

pub fn get_playlist(conn: &Connection, id: i64) -> Result<Option<Playlist>, rusqlite::Error> {
    let sql = format!(
        "SELECT {} FROM playlists WHERE id = ?1",
        PLAYLIST_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let result = stmt.query_map(params![id], row_to_playlist)?
        .next()
        .transpose();
    result
}

pub fn update_playlist(
    conn: &Connection,
    id: i64,
    name: Option<&str>,
    description: Option<&str>,
) -> Result<Playlist, rusqlite::Error> {
    if let Some(name) = name {
        conn.execute(
            "UPDATE playlists SET name = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![name, id],
        )?;
    }
    if let Some(desc) = description {
        conn.execute(
            "UPDATE playlists SET description = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![desc, id],
        )?;
    }
    get_playlist(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn delete_playlist(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute("DELETE FROM playlist_tracks WHERE playlist_id = ?1", params![id])?;
    conn.execute("DELETE FROM playlists WHERE id = ?1", params![id])?;
    Ok(())
}

pub fn add_track_to_playlist(
    conn: &Connection,
    playlist_id: i64,
    track_id: i64,
) -> Result<(), rusqlite::Error> {
    if smart_rules(conn, playlist_id)?.is_some() {
        return Err(smart_readonly_err());
    }
    let next_pos: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM playlist_tracks WHERE playlist_id = ?1",
            params![playlist_id],
            |row| row.get(0),
        )
        .unwrap_or(1);

    conn.execute(
        "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position) VALUES (?1, ?2, ?3)",
        params![playlist_id, track_id, next_pos],
    )?;

    refresh_playlist_counts(conn, playlist_id)?;
    Ok(())
}

pub fn remove_track_from_playlist(
    conn: &Connection,
    playlist_id: i64,
    track_id: i64,
) -> Result<(), rusqlite::Error> {
    if smart_rules(conn, playlist_id)?.is_some() {
        return Err(smart_readonly_err());
    }
    conn.execute(
        "DELETE FROM playlist_tracks WHERE playlist_id = ?1 AND track_id = ?2",
        params![playlist_id, track_id],
    )?;

    refresh_playlist_counts(conn, playlist_id)?;
    Ok(())
}

pub fn reorder_playlist(
    conn: &Connection,
    playlist_id: i64,
    from: i64,
    to: i64,
) -> Result<(), rusqlite::Error> {
    if smart_rules(conn, playlist_id)?.is_some() {
        return Err(smart_readonly_err());
    }
    // Positions can have gaps (after removals) or duplicates, while the UI
    // sends list INDICES. Load the ordered list, move in memory, and write
    // back a dense 0..n position sequence — this both performs the move and
    // repairs any drifted positions.
    let mut track_ids: Vec<i64> = conn
        .prepare(
            "SELECT track_id FROM playlist_tracks
             WHERE playlist_id = ?1 ORDER BY position, rowid",
        )?
        .query_map(params![playlist_id], |row| row.get(0))?
        .collect::<Result<Vec<_>, _>>()?;

    let len = track_ids.len() as i64;
    if from < 0 || from >= len || to < 0 || to >= len {
        return Ok(());
    }

    let item = track_ids.remove(from as usize);
    track_ids.insert(to as usize, item);

    for (pos, tid) in track_ids.iter().enumerate() {
        conn.execute(
            "UPDATE playlist_tracks SET position = ?1 WHERE playlist_id = ?2 AND track_id = ?3",
            params![pos as i64, playlist_id, tid],
        )?;
    }

    Ok(())
}

/// Backfill playlist_tracks from monitored_playlist_entries for downloaded tracks
/// that haven't been linked yet (e.g. tracks downloaded before auto-link was added).
pub fn backfill_playlist_tracks(conn: &Connection, playlist_id: i64) -> Result<(), rusqlite::Error> {
    // Smart playlists have no manual/monitored track links — nothing to backfill.
    if smart_rules(conn, playlist_id)?.is_some() {
        return Ok(());
    }
    let inserted: usize = conn.execute(
        "INSERT OR IGNORE INTO playlist_tracks (playlist_id, track_id, position)
         SELECT ?1, e.track_id,
                COALESCE((SELECT MAX(position) FROM playlist_tracks WHERE playlist_id = ?1), 0) + ROW_NUMBER() OVER (ORDER BY e.position)
         FROM monitored_playlist_entries e
         WHERE e.playlist_id = ?1
           AND e.track_id IS NOT NULL
           AND e.track_id NOT IN (SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1)",
        params![playlist_id],
    )?;
    if inserted > 0 {
        refresh_playlist_counts(conn, playlist_id)?;
    }
    Ok(())
}

/// Error used when a manual-edit operation targets a smart playlist.
fn smart_readonly_err() -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_CONSTRAINT),
        Some("Smart playlists are rule-based and cannot be edited manually".to_string()),
    )
}

/// Map a smart-rule evaluation error into a rusqlite error so smart and
/// normal playlists share one return type.
fn smart_eval_err(msg: String) -> rusqlite::Error {
    rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_ERROR),
        Some(msg),
    )
}

pub fn get_playlist_tracks(conn: &Connection, playlist_id: i64) -> Result<Vec<Track>, rusqlite::Error> {
    // Smart playlists: compute the track list from rules instead of
    // playlist_tracks, so every caller (detail page, play/queue, pagination)
    // transparently gets the evaluated tracks.
    if let Some(rules) = smart_rules(conn, playlist_id)? {
        let tracks = super::smart::evaluate_all(conn, &rules).map_err(smart_eval_err)?;
        refresh_smart_playlist_counts(conn, playlist_id, &tracks);
        return Ok(tracks);
    }
    let sql = format!(
        "SELECT {}
         FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE pt.playlist_id = ?1
         ORDER BY pt.position",
        TRACK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![playlist_id], row_to_track)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tracks)
}

pub fn get_playlist_tracks_page(
    conn: &Connection,
    playlist_id: i64,
    offset: i64,
    limit: i64,
) -> Result<TrackPage, rusqlite::Error> {
    if let Some(rules) = smart_rules(conn, playlist_id)? {
        let parsed = super::smart::parse_rules(&rules).map_err(smart_eval_err)?;
        let page = super::smart::evaluate_page(conn, &parsed, offset, limit)
            .map_err(smart_eval_err)?;
        return Ok(page);
    }

    let total: i64 = conn.query_row(
        "SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?1",
        params![playlist_id],
        |row| row.get(0),
    )?;

    let sql = format!(
        "SELECT {}
         FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE pt.playlist_id = ?1
         ORDER BY pt.position
         LIMIT ?2 OFFSET ?3",
        TRACK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![playlist_id, limit, offset], row_to_track)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(TrackPage { tracks, total })
}

/// Keep the stored track_count/total_duration_ms of a smart playlist roughly
/// in sync so the playlists grid shows real numbers. Best-effort (evaluation
/// already succeeded; a failed stats write shouldn't fail the read).
fn refresh_smart_playlist_counts(conn: &Connection, playlist_id: i64, tracks: &[Track]) {
    let total_ms: i64 = tracks.iter().filter_map(|t| t.duration_ms).sum();
    let _ = conn.execute(
        "UPDATE playlists SET track_count = ?1, total_duration_ms = ?2 WHERE id = ?3",
        params![tracks.len() as i64, total_ms, playlist_id],
    );
}

fn refresh_playlist_counts(conn: &Connection, playlist_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE playlists SET
            track_count = (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = ?1),
            total_duration_ms = (
                SELECT COALESCE(SUM(t.duration_ms), 0)
                FROM playlist_tracks pt JOIN tracks t ON t.id = pt.track_id
                WHERE pt.playlist_id = ?1
            ),
            updated_at = datetime('now')
         WHERE id = ?1",
        params![playlist_id],
    )?;
    Ok(())
}
