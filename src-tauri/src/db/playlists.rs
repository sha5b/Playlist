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
    })
}

const PLAYLIST_COLUMNS: &str =
    "id, name, description, cover_art_path, source_platform, source_url,
     track_count, total_duration_ms, is_synced, last_synced_at, created_at";

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

pub fn get_playlist_tracks(conn: &Connection, playlist_id: i64) -> Result<Vec<Track>, rusqlite::Error> {
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
