use rusqlite::{params, Connection, Row};

use super::models::{Track, TrackPage};

/// Shared column list for track queries. Indexes 0–33.
pub const TRACK_COLUMNS: &str =
    "t.id, t.title, t.duration_ms, t.track_number, t.disc_number,
     t.genre, t.year, t.file_path, t.file_size, t.format, t.bitrate,
     t.sample_rate, t.channels, t.cover_art_path, t.source_platform,
     t.source_url, t.play_count, t.last_played_at, t.date_added,
     a.name as artist_name, al.title as album_title, t.album_artist,
     t.artist_id, t.album_id,
     t.description, t.label, t.release_date, t.composer, t.language,
     t.metadata_completeness, t.tags, t.lyrics, t.music_video_url, t.music_video_path";

/// Map a row (using TRACK_COLUMNS order) into a Track.
pub fn row_to_track(row: &Row) -> Result<Track, rusqlite::Error> {
    Ok(Track {
        id: row.get(0)?,
        title: row.get(1)?,
        artist_id: row.get(22)?,
        album_id: row.get(23)?,
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
        description: row.get(24)?,
        label: row.get(25)?,
        release_date: row.get(26)?,
        composer: row.get(27)?,
        language: row.get(28)?,
        metadata_completeness: row.get(29)?,
        tags: row.get(30)?,
        lyrics: row.get(31)?,
        music_video_url: row.get(32)?,
        music_video_path: row.get(33)?,
    })
}

/// Return "ASC" or "DESC" based on the direction string.
fn sql_dir(sort_dir: &str) -> &'static str {
    if sort_dir == "asc" { "ASC" } else { "DESC" }
}

pub fn get_tracks(
    conn: &Connection,
    offset: i64,
    limit: i64,
    sort_by: &str,
    sort_dir: &str,
    search: Option<&str>,
) -> Result<TrackPage, rusqlite::Error> {
    let limit = limit.clamp(1, 500);
    let offset = offset.max(0);
    let dir = sql_dir(sort_dir);
    let order_clause = match sort_by {
        "title" => format!("t.title {dir}"),
        "artist_name" => format!("a.name {dir}"),
        "duration_ms" => format!("t.duration_ms {dir}"),
        "play_count" => format!("t.play_count {dir}"),
        "last_played" => "t.last_played_at DESC NULLS LAST".to_string(),
        "year" => format!("t.year {dir}"),
        "random" => "RANDOM()".to_string(),
        _ => format!("t.date_added {dir}"),
    };

    // When searching, use FTS for matching then apply sort/pagination
    let fts_query = search
        .filter(|q| !q.trim().is_empty())
        .map(|q| {
            q.split_whitespace()
                .map(|w| format!("{}*", w))
                .collect::<Vec<_>>()
                .join(" ")
        });

    let total: i64 = if let Some(ref fts) = fts_query {
        conn.query_row(
            "SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH ?1",
            params![fts],
            |row| row.get(0),
        )?
    } else {
        conn.query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0))?
    };

    let (where_clause, param_offset) = if fts_query.is_some() {
        ("JOIN tracks_fts ON tracks_fts.rowid = t.id WHERE tracks_fts MATCH ?1", 1)
    } else {
        ("", 0)
    };

    let sql = format!(
        "SELECT {}
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         {}
         ORDER BY {}
         LIMIT ?{} OFFSET ?{}",
        TRACK_COLUMNS, where_clause, order_clause, param_offset + 1, param_offset + 2
    );

    let mut stmt = conn.prepare(&sql)?;
    let tracks = if let Some(ref fts) = fts_query {
        stmt.query_map(params![fts, limit, offset], row_to_track)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![limit, offset], row_to_track)?
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok(TrackPage { tracks, total })
}

pub fn get_track(conn: &Connection, id: i64) -> Result<Option<Track>, rusqlite::Error> {
    let sql = format!(
        "SELECT {}
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.id = ?1",
        TRACK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let result = stmt.query_map(params![id], row_to_track)?
        .next()
        .transpose();
    result
}

pub fn delete_track(conn: &Connection, id: i64, delete_file: bool) -> Result<Option<String>, rusqlite::Error> {
    let file_path: Option<String> = if delete_file {
        conn.query_row("SELECT file_path FROM tracks WHERE id = ?1", params![id], |row| {
            row.get(0)
        })
        .ok()
    } else {
        None
    };

    // Remove from FTS
    conn.execute(
        "DELETE FROM tracks_fts WHERE rowid = ?1",
        params![id],
    )?;

    // Remove from playlist_tracks
    conn.execute(
        "DELETE FROM playlist_tracks WHERE track_id = ?1",
        params![id],
    )?;

    conn.execute("DELETE FROM tracks WHERE id = ?1", params![id])?;

    Ok(file_path)
}

pub fn search_tracks_fts(
    conn: &Connection,
    query: &str,
    limit: i64,
) -> Result<Vec<Track>, rusqlite::Error> {
    let fts_query = query
        .split_whitespace()
        .map(|w| format!("{}*", w))
        .collect::<Vec<_>>()
        .join(" ");

    let sql = format!(
        "SELECT {}
         FROM tracks_fts
         JOIN tracks t ON t.id = tracks_fts.rowid
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE tracks_fts MATCH ?1
         ORDER BY bm25(tracks_fts)
         LIMIT ?2",
        TRACK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;

    let tracks = stmt
        .query_map(params![fts_query, limit], row_to_track)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(tracks)
}

pub fn get_tracks_by_album(conn: &Connection, album_id: i64) -> Result<Vec<Track>, rusqlite::Error> {
    let sql = format!(
        "SELECT {}
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.album_id = ?1
         ORDER BY t.disc_number, t.track_number, t.title",
        TRACK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;

    let tracks = stmt
        .query_map(params![album_id], row_to_track)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(tracks)
}

pub fn get_tracks_by_artist(conn: &Connection, artist_id: i64) -> Result<Vec<Track>, rusqlite::Error> {
    let sql = format!(
        "SELECT {}
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.artist_id = ?1
         ORDER BY t.date_added DESC",
        TRACK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;

    let tracks = stmt
        .query_map(params![artist_id], row_to_track)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(tracks)
}

pub fn get_distinct_genres(conn: &Connection) -> Result<Vec<String>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT genre FROM tracks WHERE genre IS NOT NULL AND genre != '' ORDER BY genre",
    )?;
    let genres = stmt
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(genres)
}

pub fn get_tracks_by_genre(
    conn: &Connection,
    genre: &str,
    limit: i64,
) -> Result<Vec<Track>, rusqlite::Error> {
    let sql = format!(
        "SELECT {}
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.genre = ?1
         ORDER BY RANDOM()
         LIMIT ?2",
        TRACK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![genre, limit], row_to_track)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tracks)
}

/// Compute metadata completeness as a percentage (0–100).
/// Checks key fields: title, artist, album, genre, year, track_number, cover_art, duration, description.
pub fn compute_completeness(conn: &Connection, track_id: i64) -> Result<i64, rusqlite::Error> {
    #[allow(clippy::type_complexity)]
    let row: (
        Option<String>, Option<i64>, Option<i64>, Option<String>,
        Option<i64>, Option<i64>, Option<String>, Option<i64>,
        Option<String>, Option<String>, Option<String>,
    ) = conn.query_row(
        "SELECT title, artist_id, album_id, genre, year, track_number,
                cover_art_path, duration_ms, description, label, release_date
         FROM tracks WHERE id = ?1",
        params![track_id],
        |row| Ok((
            row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?,
            row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?,
            row.get(8)?, row.get(9)?, row.get(10)?,
        )),
    )?;

    let fields: &[bool] = &[
        row.0.as_ref().is_some_and(|s| !s.is_empty() && s != "Unknown"), // title
        row.1.is_some(),  // artist
        row.2.is_some(),  // album
        row.3.as_ref().is_some_and(|s| !s.is_empty()), // genre
        row.4.is_some(),  // year
        row.5.is_some(),  // track_number
        row.6.as_ref().is_some_and(|s| !s.is_empty()), // cover_art
        row.7.is_some(),  // duration
        row.8.as_ref().is_some_and(|s| !s.is_empty()), // description
        row.9.as_ref().is_some_and(|s| !s.is_empty()),  // label
        row.10.as_ref().is_some_and(|s| !s.is_empty()), // release_date
    ];

    let filled = fields.iter().filter(|&&b| b).count();
    let pct = (filled as f64 / fields.len() as f64 * 100.0).round() as i64;
    Ok(pct)
}

/// Recompute and store metadata_completeness for a track.
pub fn update_completeness(conn: &Connection, track_id: i64) -> Result<i64, rusqlite::Error> {
    let pct = compute_completeness(conn, track_id)?;
    conn.execute(
        "UPDATE tracks SET metadata_completeness = ?1 WHERE id = ?2",
        params![pct, track_id],
    )?;
    Ok(pct)
}

/// Record a play: increment play_count and set last_played_at to now.
pub fn record_play(conn: &Connection, track_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE tracks SET play_count = play_count + 1, last_played_at = datetime('now') WHERE id = ?1",
        params![track_id],
    )?;
    Ok(())
}

pub fn update_fts(conn: &Connection, track_id: i64) -> Result<(), rusqlite::Error> {
    // Delete old entry
    let _ = conn.execute("DELETE FROM tracks_fts WHERE rowid = ?1", params![track_id]);

    // Insert new entry with denormalized data
    conn.execute(
        "INSERT INTO tracks_fts(rowid, title, artist_name, album_title, album_artist, genre)
         SELECT t.id, t.title, COALESCE(a.name, ''), COALESCE(al.title, ''), COALESCE(t.album_artist, ''), COALESCE(t.genre, '')
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.id = ?1",
        params![track_id],
    )?;

    Ok(())
}
