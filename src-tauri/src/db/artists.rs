use rusqlite::{params, Connection, Row};

use super::models::Artist;

const ARTIST_COLUMNS: &str =
    "a.id, a.name, a.sort_name, a.musicbrainz_id, a.image_path, a.bio,
     a.country, a.begin_year, a.artist_type,
     COUNT(t.id) as track_count";

fn row_to_artist(row: &Row) -> Result<Artist, rusqlite::Error> {
    Ok(Artist {
        id: row.get(0)?,
        name: row.get(1)?,
        sort_name: row.get(2)?,
        musicbrainz_id: row.get(3)?,
        image_path: row.get(4)?,
        bio: row.get(5)?,
        country: row.get(6)?,
        begin_year: row.get(7)?,
        artist_type: row.get(8)?,
        track_count: row.get(9)?,
    })
}

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

pub fn update_image_if_missing(
    conn: &Connection,
    artist_id: i64,
    image_path: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE artists SET image_path = ?1 WHERE id = ?2 AND image_path IS NULL",
        params![image_path, artist_id],
    )?;
    Ok(())
}

pub fn get_artists(conn: &Connection, offset: i64, limit: i64, search: Option<&str>) -> Result<(Vec<Artist>, i64), rusqlite::Error> {
    let (having_clause, pattern) = match search {
        Some(q) if !q.trim().is_empty() => {
            ("HAVING a.name LIKE ?3".to_string(), Some(format!("%{}%", q)))
        }
        _ => (String::new(), None),
    };

    let total: i64 = if let Some(ref p) = pattern {
        conn.query_row(
            "SELECT COUNT(*) FROM artists WHERE name LIKE ?1",
            params![p],
            |row| row.get(0),
        )?
    } else {
        conn.query_row("SELECT COUNT(*) FROM artists", [], |row| row.get(0))?
    };

    let sql = format!(
        "SELECT {}
         FROM artists a
         LEFT JOIN tracks t ON t.artist_id = a.id
         GROUP BY a.id
         {}
         ORDER BY a.name COLLATE NOCASE
         LIMIT ?1 OFFSET ?2",
        ARTIST_COLUMNS, having_clause
    );

    let mut stmt = conn.prepare(&sql)?;
    let artists: Vec<Artist> = if let Some(ref p) = pattern {
        stmt.query_map(params![limit, offset, p], |row| row_to_artist(row))?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![limit, offset], |row| row_to_artist(row))?
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok((artists, total))
}

pub fn get_artist(conn: &Connection, id: i64) -> Result<Option<Artist>, rusqlite::Error> {
    let sql = format!(
        "SELECT {}
         FROM artists a
         LEFT JOIN tracks t ON t.artist_id = a.id
         WHERE a.id = ?1
         GROUP BY a.id",
        ARTIST_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;

    let mut rows = stmt.query_map(params![id], |row| row_to_artist(row))?;

    match rows.next() {
        Some(Ok(a)) => Ok(Some(a)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}
