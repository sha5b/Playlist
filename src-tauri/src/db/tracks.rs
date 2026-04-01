use rusqlite::{params, Connection};

use super::models::{Track, TrackPage};

pub fn get_tracks(
    conn: &Connection,
    offset: i64,
    limit: i64,
    sort_by: &str,
    sort_dir: &str,
    search: Option<&str>,
) -> Result<TrackPage, rusqlite::Error> {
    let allowed_sort = match sort_by {
        "title" | "date_added" | "duration_ms" | "play_count" | "year" => sort_by,
        _ => "date_added",
    };
    let dir = if sort_dir == "asc" { "ASC" } else { "DESC" };

    // When searching, use FTS for matching then apply sort/pagination
    let (where_clause, count_sql, use_fts) = match search {
        Some(q) if !q.trim().is_empty() => {
            let fts_query = q.split_whitespace()
                .map(|w| format!("{}*", w))
                .collect::<Vec<_>>()
                .join(" ");
            (
                format!("JOIN tracks_fts ON tracks_fts.rowid = t.id WHERE tracks_fts MATCH '{}'", fts_query.replace('\'', "''")),
                format!("SELECT COUNT(*) FROM tracks_fts WHERE tracks_fts MATCH '{}'", fts_query.replace('\'', "''")),
                true,
            )
        }
        _ => (String::new(), "SELECT COUNT(*) FROM tracks".to_string(), false),
    };
    let _ = use_fts;

    let total: i64 = conn.query_row(&count_sql, [], |row| row.get(0))?;

    let sql = format!(
        "SELECT t.id, t.title, t.duration_ms, t.track_number, t.disc_number,
                t.genre, t.year, t.file_path, t.file_size, t.format, t.bitrate,
                t.sample_rate, t.channels, t.cover_art_path, t.source_platform,
                t.source_url, t.play_count, t.last_played_at, t.date_added,
                a.name as artist_name, al.title as album_title, t.album_artist
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         {}
         ORDER BY t.{} {}
         LIMIT ?1 OFFSET ?2",
        where_clause, allowed_sort, dir
    );

    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![limit, offset], |row| {
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

    Ok(TrackPage { tracks, total })
}

pub fn get_track(conn: &Connection, id: i64) -> Result<Option<Track>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.duration_ms, t.track_number, t.disc_number,
                t.genre, t.year, t.file_path, t.file_size, t.format, t.bitrate,
                t.sample_rate, t.channels, t.cover_art_path, t.source_platform,
                t.source_url, t.play_count, t.last_played_at, t.date_added,
                a.name as artist_name, al.title as album_title, t.album_artist
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.id = ?1",
    )?;

    let mut rows = stmt.query_map(params![id], |row| {
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
    })?;

    match rows.next() {
        Some(Ok(track)) => Ok(Some(track)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
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

    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.duration_ms, t.track_number, t.disc_number,
                t.genre, t.year, t.file_path, t.file_size, t.format, t.bitrate,
                t.sample_rate, t.channels, t.cover_art_path, t.source_platform,
                t.source_url, t.play_count, t.last_played_at, t.date_added,
                a.name as artist_name, al.title as album_title, t.album_artist
         FROM tracks_fts
         JOIN tracks t ON t.id = tracks_fts.rowid
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE tracks_fts MATCH ?1
         ORDER BY bm25(tracks_fts)
         LIMIT ?2",
    )?;

    let tracks = stmt
        .query_map(params![fts_query, limit], |row| {
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

pub fn get_tracks_by_album(conn: &Connection, album_id: i64) -> Result<Vec<Track>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.duration_ms, t.track_number, t.disc_number,
                t.genre, t.year, t.file_path, t.file_size, t.format, t.bitrate,
                t.sample_rate, t.channels, t.cover_art_path, t.source_platform,
                t.source_url, t.play_count, t.last_played_at, t.date_added,
                a.name as artist_name, al.title as album_title, t.album_artist,
                t.artist_id, t.album_id
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.album_id = ?1
         ORDER BY t.disc_number, t.track_number, t.title",
    )?;

    let tracks = stmt
        .query_map(params![album_id], |row| {
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
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(tracks)
}

pub fn get_tracks_by_artist(conn: &Connection, artist_id: i64) -> Result<Vec<Track>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.title, t.duration_ms, t.track_number, t.disc_number,
                t.genre, t.year, t.file_path, t.file_size, t.format, t.bitrate,
                t.sample_rate, t.channels, t.cover_art_path, t.source_platform,
                t.source_url, t.play_count, t.last_played_at, t.date_added,
                a.name as artist_name, al.title as album_title, t.album_artist,
                t.artist_id, t.album_id
         FROM tracks t
         LEFT JOIN artists a ON t.artist_id = a.id
         LEFT JOIN albums al ON t.album_id = al.id
         WHERE t.artist_id = ?1
         ORDER BY t.date_added DESC",
    )?;

    let tracks = stmt
        .query_map(params![artist_id], |row| {
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
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(tracks)
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
