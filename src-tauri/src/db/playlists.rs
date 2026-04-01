use rusqlite::{params, Connection};

use super::models::{Playlist, Track};

pub fn get_playlists(conn: &Connection) -> Result<Vec<Playlist>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, name, description, cover_art_path, source_platform, source_url,
                track_count, total_duration_ms, is_synced, last_synced_at, created_at
         FROM playlists
         ORDER BY created_at DESC",
    )?;

    let playlists = stmt
        .query_map([], |row| {
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
        })?
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
    let mut stmt = conn.prepare(
        "SELECT id, name, description, cover_art_path, source_platform, source_url,
                track_count, total_duration_ms, is_synced, last_synced_at, created_at
         FROM playlists WHERE id = ?1",
    )?;

    let mut rows = stmt.query_map(params![id], |row| {
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
    })?;

    match rows.next() {
        Some(Ok(p)) => Ok(Some(p)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
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
    // Get the track at the 'from' position
    let track_id: i64 = conn.query_row(
        "SELECT track_id FROM playlist_tracks WHERE playlist_id = ?1 AND position = ?2",
        params![playlist_id, from],
        |row| row.get(0),
    )?;

    if from < to {
        conn.execute(
            "UPDATE playlist_tracks SET position = position - 1
             WHERE playlist_id = ?1 AND position > ?2 AND position <= ?3",
            params![playlist_id, from, to],
        )?;
    } else {
        conn.execute(
            "UPDATE playlist_tracks SET position = position + 1
             WHERE playlist_id = ?1 AND position >= ?2 AND position < ?3",
            params![playlist_id, to, from],
        )?;
    }

    conn.execute(
        "UPDATE playlist_tracks SET position = ?1 WHERE playlist_id = ?2 AND track_id = ?3",
        params![to, playlist_id, track_id],
    )?;

    Ok(())
}

pub fn get_playlist_tracks(conn: &Connection, playlist_id: i64) -> Result<Vec<Track>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.duration_ms, t.track_number, t.disc_number,
                t.genre, t.year, t.file_path, t.file_size, t.format, t.bitrate,
                t.sample_rate, t.channels, t.cover_art_path, t.source_platform,
                t.source_url, t.play_count, t.last_played_at, t.date_added,
                a.name as artist_name, al.title as album_title, t.album_artist
         FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE pt.playlist_id = ?1
         ORDER BY pt.position",
    )?;

    let tracks = stmt
        .query_map(params![playlist_id], |row| {
            Ok(Track {
                id: row.get(0)?,
                title: row.get(1)?,
                artist_id: None,
                album_id: None,
                album_artist: row.get(21)?,
                duration_ms: row.get(2)?,
                track_number: row.get(3)?,
                disc_number: row.get(4)?,
                genre: row.get(5)?,
                year: row.get(6)?,
                file_path: row.get(7)?,
                file_size: row.get(8)?,
                format: row.get(9)?,
                bitrate: row.get(10)?,
                sample_rate: row.get(11)?,
                channels: row.get(12)?,
                cover_art_path: row.get(13)?,
                source_platform: row.get(14)?,
                source_url: row.get(15)?,
                play_count: row.get(16)?,
                last_played_at: row.get(17)?,
                date_added: row.get(18)?,
                artist_name: row.get(19)?,
                album_title: row.get(20)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(tracks)
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
