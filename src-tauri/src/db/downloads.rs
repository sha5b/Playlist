use rusqlite::{params, Connection};

use super::models::Download;

fn row_to_download(row: &rusqlite::Row) -> rusqlite::Result<Download> {
    Ok(Download {
        id: row.get(0)?,
        url: row.get(1)?,
        title: row.get(2)?,
        artist: row.get(3)?,
        platform: row.get(4)?,
        status: row.get(5)?,
        progress: row.get(6)?,
        error_message: row.get(7)?,
        file_path: row.get(8)?,
        track_id: row.get(9)?,
        playlist_id: row.get(10)?,
        format: row.get(11)?,
        quality: row.get(12)?,
        created_at: row.get(13)?,
        started_at: row.get(14)?,
        completed_at: row.get(15)?,
        target_album_id: row.get(16)?,
        target_artist_id: row.get(17)?,
    })
}

const SELECT_COLS: &str = "id, url, title, artist, platform, status, progress, error_message,
    file_path, track_id, playlist_id, format, quality, created_at, started_at, completed_at,
    target_album_id, target_artist_id";

pub fn create_download(
    conn: &Connection,
    url: &str,
    title: Option<&str>,
    artist: Option<&str>,
    platform: &str,
    format: &str,
    quality: &str,
    target_album_id: Option<i64>,
    target_artist_id: Option<i64>,
) -> Result<Download, rusqlite::Error> {
    conn.execute(
        "INSERT INTO downloads (url, title, artist, platform, format, quality, target_album_id, target_artist_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![url, title, artist, platform, format, quality, target_album_id, target_artist_id],
    )?;
    let id = conn.last_insert_rowid();
    get_download(conn, id)?.ok_or(rusqlite::Error::QueryReturnedNoRows)
}

pub fn get_download(conn: &Connection, id: i64) -> Result<Option<Download>, rusqlite::Error> {
    let sql = format!("SELECT {} FROM downloads WHERE id = ?1", SELECT_COLS);
    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query_map(params![id], row_to_download)?;
    match rows.next() {
        Some(Ok(d)) => Ok(Some(d)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

pub fn get_active_downloads(conn: &Connection) -> Result<Vec<Download>, rusqlite::Error> {
    let sql = format!(
        "SELECT {} FROM downloads WHERE status IN ('queued', 'downloading', 'processing')
         ORDER BY created_at ASC",
        SELECT_COLS
    );
    let mut stmt = conn.prepare(&sql)?;
    let downloads = stmt
        .query_map([], row_to_download)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(downloads)
}

pub fn get_download_history(
    conn: &Connection,
    offset: i64,
    limit: i64,
) -> Result<(Vec<Download>, i64), rusqlite::Error> {
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM downloads", [], |row| row.get(0))?;

    let sql = format!(
        "SELECT {} FROM downloads ORDER BY created_at DESC LIMIT ?1 OFFSET ?2",
        SELECT_COLS
    );
    let mut stmt = conn.prepare(&sql)?;
    let downloads = stmt
        .query_map(params![limit, offset], row_to_download)?
        .collect::<Result<Vec<_>, _>>()?;

    Ok((downloads, total))
}

pub fn update_download_status(
    conn: &Connection,
    id: i64,
    status: &str,
    error_message: Option<&str>,
) -> Result<(), rusqlite::Error> {
    match status {
        "downloading" => {
            conn.execute(
                "UPDATE downloads SET status = ?2, started_at = datetime('now') WHERE id = ?1",
                params![id, status],
            )?;
        }
        "completed" => {
            conn.execute(
                "UPDATE downloads SET status = ?2, completed_at = datetime('now'), progress = 100.0 WHERE id = ?1",
                params![id, status],
            )?;
        }
        "failed" | "cancelled" => {
            conn.execute(
                "UPDATE downloads SET status = ?2, error_message = ?3 WHERE id = ?1",
                params![id, status, error_message],
            )?;
        }
        _ => {
            conn.execute(
                "UPDATE downloads SET status = ?2 WHERE id = ?1",
                params![id, status],
            )?;
        }
    }
    Ok(())
}

pub fn update_download_title(
    conn: &Connection,
    id: i64,
    title: &str,
    artist: Option<&str>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE downloads SET title = ?2, artist = ?3 WHERE id = ?1",
        params![id, title, artist],
    )?;
    Ok(())
}

pub fn update_download_file(
    conn: &Connection,
    id: i64,
    file_path: &str,
    track_id: Option<i64>,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE downloads SET file_path = ?2, track_id = ?3 WHERE id = ?1",
        params![id, file_path, track_id],
    )?;
    Ok(())
}

pub fn reset_download_for_retry(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE downloads SET status = 'queued', progress = 0, error_message = NULL,
         started_at = NULL, completed_at = NULL WHERE id = ?1",
        params![id],
    )?;
    Ok(())
}

pub fn cancel_download(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE downloads SET status = 'cancelled' WHERE id = ?1 AND status IN ('queued', 'downloading')",
        params![id],
    )?;
    Ok(())
}

pub fn clear_completed(conn: &Connection) -> Result<i64, rusqlite::Error> {
    let count = conn.execute(
        "DELETE FROM downloads WHERE status IN ('completed', 'failed', 'cancelled')",
        [],
    )?;
    Ok(count as i64)
}
