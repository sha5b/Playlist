use rusqlite::{params, Connection};

use super::models::Album;

pub fn find_or_create(
    conn: &Connection,
    title: &str,
    artist_id: Option<i64>,
    album_artist: Option<&str>,
    year: Option<i64>,
) -> Result<i64, rusqlite::Error> {
    // Try to find existing album by title + artist
    let existing = if let Some(aid) = artist_id {
        conn.query_row(
            "SELECT id FROM albums WHERE title = ?1 COLLATE NOCASE AND artist_id = ?2",
            params![title, aid],
            |row| row.get::<_, i64>(0),
        )
        .ok()
    } else {
        conn.query_row(
            "SELECT id FROM albums WHERE title = ?1 COLLATE NOCASE AND artist_id IS NULL",
            params![title],
            |row| row.get::<_, i64>(0),
        )
        .ok()
    };

    if let Some(id) = existing {
        return Ok(id);
    }

    conn.execute(
        "INSERT INTO albums (title, artist_id, album_artist, year) VALUES (?1, ?2, ?3, ?4)",
        params![title, artist_id, album_artist, year],
    )?;

    Ok(conn.last_insert_rowid())
}

pub fn get_albums(conn: &Connection, offset: i64, limit: i64) -> Result<(Vec<Album>, i64), rusqlite::Error> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM albums", [], |row| row.get(0))?;

    let mut stmt = conn.prepare(
        "SELECT al.id, al.title, al.artist_id, al.album_artist, al.year, al.genre,
                al.total_tracks, al.total_discs, al.musicbrainz_id, al.cover_art_path,
                a.name as artist_name, COUNT(t.id) as track_count
         FROM albums al
         LEFT JOIN artists a ON al.artist_id = a.id
         LEFT JOIN tracks t ON t.album_id = al.id
         GROUP BY al.id
         ORDER BY al.title COLLATE NOCASE
         LIMIT ?1 OFFSET ?2",
    )?;

    let albums = stmt
        .query_map(params![limit, offset], |row| {
            Ok(Album {
                id: row.get(0)?,
                title: row.get(1)?,
                artist_id: row.get(2)?,
                album_artist: row.get(3)?,
                year: row.get(4)?,
                genre: row.get(5)?,
                total_tracks: row.get(6)?,
                total_discs: row.get(7)?,
                musicbrainz_id: row.get(8)?,
                cover_art_path: row.get(9)?,
                artist_name: row.get(10)?,
                track_count: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok((albums, total))
}

pub fn get_albums_by_artist(conn: &Connection, artist_id: i64) -> Result<Vec<Album>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT al.id, al.title, al.artist_id, al.album_artist, al.year, al.genre,
                al.total_tracks, al.total_discs, al.musicbrainz_id, al.cover_art_path,
                a.name as artist_name, COUNT(t.id) as track_count
         FROM albums al
         LEFT JOIN artists a ON al.artist_id = a.id
         LEFT JOIN tracks t ON t.album_id = al.id
         WHERE al.artist_id = ?1
         GROUP BY al.id
         ORDER BY al.year DESC, al.title COLLATE NOCASE",
    )?;

    let albums = stmt
        .query_map(params![artist_id], |row| {
            Ok(Album {
                id: row.get(0)?,
                title: row.get(1)?,
                artist_id: row.get(2)?,
                album_artist: row.get(3)?,
                year: row.get(4)?,
                genre: row.get(5)?,
                total_tracks: row.get(6)?,
                total_discs: row.get(7)?,
                musicbrainz_id: row.get(8)?,
                cover_art_path: row.get(9)?,
                artist_name: row.get(10)?,
                track_count: row.get(11)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(albums)
}

pub fn get_album(conn: &Connection, id: i64) -> Result<Option<Album>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT al.id, al.title, al.artist_id, al.album_artist, al.year, al.genre,
                al.total_tracks, al.total_discs, al.musicbrainz_id, al.cover_art_path,
                a.name as artist_name, COUNT(t.id) as track_count
         FROM albums al
         LEFT JOIN artists a ON al.artist_id = a.id
         LEFT JOIN tracks t ON t.album_id = al.id
         WHERE al.id = ?1
         GROUP BY al.id",
    )?;

    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Album {
            id: row.get(0)?,
            title: row.get(1)?,
            artist_id: row.get(2)?,
            album_artist: row.get(3)?,
            year: row.get(4)?,
            genre: row.get(5)?,
            total_tracks: row.get(6)?,
            total_discs: row.get(7)?,
            musicbrainz_id: row.get(8)?,
            cover_art_path: row.get(9)?,
            artist_name: row.get(10)?,
            track_count: row.get(11)?,
        })
    })?;

    match rows.next() {
        Some(Ok(a)) => Ok(Some(a)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}
