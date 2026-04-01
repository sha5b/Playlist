use rusqlite::{params, Connection};

use super::models::Artist;

pub fn find_or_create(conn: &Connection, name: &str) -> Result<i64, rusqlite::Error> {
    if let Ok(id) = conn.query_row(
        "SELECT id FROM artists WHERE name = ?1 COLLATE NOCASE",
        params![name],
        |row| row.get::<_, i64>(0),
    ) {
        return Ok(id);
    }

    conn.execute(
        "INSERT INTO artists (name) VALUES (?1)",
        params![name],
    )?;

    Ok(conn.last_insert_rowid())
}

pub fn get_artists(conn: &Connection, offset: i64, limit: i64) -> Result<(Vec<Artist>, i64), rusqlite::Error> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM artists", [], |row| row.get(0))?;

    let mut stmt = conn.prepare(
        "SELECT a.id, a.name, a.sort_name, a.musicbrainz_id, a.image_path, a.bio,
                COUNT(t.id) as track_count
         FROM artists a
         LEFT JOIN tracks t ON t.artist_id = a.id
         GROUP BY a.id
         ORDER BY a.name COLLATE NOCASE
         LIMIT ?1 OFFSET ?2",
    )?;

    let artists = stmt
        .query_map(params![limit, offset], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_name: row.get(2)?,
                musicbrainz_id: row.get(3)?,
                image_path: row.get(4)?,
                bio: row.get(5)?,
                track_count: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    Ok((artists, total))
}

pub fn get_artist(conn: &Connection, id: i64) -> Result<Option<Artist>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.name, a.sort_name, a.musicbrainz_id, a.image_path, a.bio,
                COUNT(t.id) as track_count
         FROM artists a
         LEFT JOIN tracks t ON t.artist_id = a.id
         WHERE a.id = ?1
         GROUP BY a.id",
    )?;

    let mut rows = stmt.query_map(params![id], |row| {
        Ok(Artist {
            id: row.get(0)?,
            name: row.get(1)?,
            sort_name: row.get(2)?,
            musicbrainz_id: row.get(3)?,
            image_path: row.get(4)?,
            bio: row.get(5)?,
            track_count: row.get(6)?,
        })
    })?;

    match rows.next() {
        Some(Ok(a)) => Ok(Some(a)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}
