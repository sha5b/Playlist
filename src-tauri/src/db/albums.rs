use rusqlite::{params, Connection, Row};

use super::models::Album;

/// Shared column list for album queries. Indexes 0–16.
const ALBUM_COLUMNS: &str =
    "al.id, al.title, al.artist_id, al.album_artist, al.year, al.genre,
     al.total_tracks, al.total_discs, al.musicbrainz_id, al.cover_art_path,
     al.label, al.release_date, al.description, al.album_type,
     al.enriched_tracklist, al.purchase_url, a.name as artist_name, COUNT(t.id) as track_count";

fn row_to_album(row: &Row) -> Result<Album, rusqlite::Error> {
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
        label: row.get(10)?,
        release_date: row.get(11)?,
        description: row.get(12)?,
        album_type: row.get(13)?,
        enriched_tracklist: row.get(14)?,
        purchase_url: row.get(15)?,
        artist_name: row.get(16)?,
        track_count: row.get(17)?,
    })
}

pub fn find_or_create(
    conn: &Connection,
    title: &str,
    artist_id: Option<i64>,
    album_artist: Option<&str>,
    year: Option<i64>,
) -> Result<i64, rusqlite::Error> {
    // Match by title alone (case-insensitive) — the same album with different
    // featured artists should not create separate album records.
    let existing: Option<i64> = conn.query_row(
        "SELECT id FROM albums WHERE title = ?1 COLLATE NOCASE LIMIT 1",
        params![title],
        |row| row.get(0),
    ).ok();

    if let Some(id) = existing {
        // Back-fill artist_id if the existing album doesn't have one yet
        if artist_id.is_some() {
            conn.execute(
                "UPDATE albums SET artist_id = COALESCE(artist_id, ?2) WHERE id = ?1",
                params![id, artist_id],
            )?;
        }
        return Ok(id);
    }

    conn.execute(
        "INSERT INTO albums (title, artist_id, album_artist, year) VALUES (?1, ?2, ?3, ?4)",
        params![title, artist_id, album_artist, year],
    )?;

    Ok(conn.last_insert_rowid())
}

pub fn update_cover_art_if_missing(
    conn: &Connection,
    album_id: i64,
    cover_art_path: &str,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE albums SET cover_art_path = ?1 WHERE id = ?2 AND cover_art_path IS NULL",
        params![cover_art_path, album_id],
    )?;
    Ok(())
}

pub fn update_metadata_if_missing(
    conn: &Connection,
    album_id: i64,
    total_tracks: Option<i64>,
    total_discs: Option<i64>,
    genre: Option<&str>,
) -> Result<(), rusqlite::Error> {
    if let Some(tt) = total_tracks {
        conn.execute(
            "UPDATE albums SET total_tracks = ?1 WHERE id = ?2 AND total_tracks IS NULL",
            params![tt, album_id],
        )?;
    }
    if let Some(td) = total_discs {
        conn.execute(
            "UPDATE albums SET total_discs = ?1 WHERE id = ?2 AND total_discs IS NULL",
            params![td, album_id],
        )?;
    }
    if let Some(g) = genre {
        conn.execute(
            "UPDATE albums SET genre = ?1 WHERE id = ?2 AND genre IS NULL",
            params![g, album_id],
        )?;
    }
    Ok(())
}

pub fn get_albums(conn: &Connection, offset: i64, limit: i64, search: Option<&str>) -> Result<(Vec<Album>, i64), rusqlite::Error> {
    let (where_clause, pattern) = match search {
        Some(q) if !q.trim().is_empty() => {
            ("HAVING al.title LIKE ?3 OR a.name LIKE ?3".to_string(), Some(format!("%{}%", q)))
        }
        _ => (String::new(), None),
    };

    let count_sql = if pattern.is_some() {
        "SELECT COUNT(*) FROM (
                SELECT al.id FROM albums al
                LEFT JOIN artists a ON al.artist_id = a.id
                WHERE al.title LIKE ?1 OR a.name LIKE ?1
            )".to_string()
    } else {
        "SELECT COUNT(*) FROM albums".to_string()
    };

    let total: i64 = if let Some(ref p) = pattern {
        conn.query_row(&count_sql, params![p], |row| row.get(0))?
    } else {
        conn.query_row(&count_sql, [], |row| row.get(0))?
    };

    let sql = format!(
        "SELECT {}
         FROM albums al
         LEFT JOIN artists a ON al.artist_id = a.id
         LEFT JOIN tracks t ON t.album_id = al.id
         GROUP BY al.id
         {}
         ORDER BY al.title COLLATE NOCASE
         LIMIT ?1 OFFSET ?2",
        ALBUM_COLUMNS, where_clause
    );

    let mut stmt = conn.prepare(&sql)?;
    let albums: Vec<Album> = if let Some(ref p) = pattern {
        stmt.query_map(params![limit, offset, p], row_to_album)?
            .collect::<Result<Vec<_>, _>>()?
    } else {
        stmt.query_map(params![limit, offset], row_to_album)?
            .collect::<Result<Vec<_>, _>>()?
    };

    Ok((albums, total))
}

pub fn get_albums_by_artist(conn: &Connection, artist_id: i64) -> Result<Vec<Album>, rusqlite::Error> {
    let sql = format!(
        "SELECT {}
         FROM albums al
         LEFT JOIN artists a ON al.artist_id = a.id
         LEFT JOIN tracks t ON t.album_id = al.id
         WHERE al.artist_id = ?1
         GROUP BY al.id
         ORDER BY al.year DESC, al.title COLLATE NOCASE",
        ALBUM_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;

    let albums = stmt
        .query_map(params![artist_id], row_to_album)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok(albums)
}

pub fn get_recently_played_albums(conn: &Connection, limit: i64) -> Result<Vec<Album>, rusqlite::Error> {
    let sql = format!(
        "SELECT {}
         FROM albums al
         LEFT JOIN artists a ON al.artist_id = a.id
         LEFT JOIN tracks t ON t.album_id = al.id
         WHERE t.last_played_at IS NOT NULL
         GROUP BY al.id
         ORDER BY MAX(t.last_played_at) DESC
         LIMIT ?1",
        ALBUM_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let albums = stmt
        .query_map(params![limit], row_to_album)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(albums)
}

pub fn get_recently_added_albums(conn: &Connection, limit: i64) -> Result<Vec<Album>, rusqlite::Error> {
    let sql = format!(
        "SELECT {}
         FROM albums al
         LEFT JOIN artists a ON al.artist_id = a.id
         LEFT JOIN tracks t ON t.album_id = al.id
         GROUP BY al.id
         ORDER BY MAX(t.date_added) DESC
         LIMIT ?1",
        ALBUM_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let albums = stmt
        .query_map(params![limit], row_to_album)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(albums)
}

pub fn get_album(conn: &Connection, id: i64) -> Result<Option<Album>, rusqlite::Error> {
    let sql = format!(
        "SELECT {}
         FROM albums al
         LEFT JOIN artists a ON al.artist_id = a.id
         LEFT JOIN tracks t ON t.album_id = al.id
         WHERE al.id = ?1
         GROUP BY al.id",
        ALBUM_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let result = stmt.query_map(params![id], row_to_album)?
        .next()
        .transpose();
    result
}
