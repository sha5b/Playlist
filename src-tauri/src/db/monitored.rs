use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::models::Playlist;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MonitoredEntry {
    pub id: i64,
    pub playlist_id: i64,
    pub source_url: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub duration_seconds: Option<f64>,
    pub thumbnail: Option<String>,
    pub status: String,
    pub download_id: Option<i64>,
    pub track_id: Option<i64>,
    pub position: i64,
    pub first_seen_at: String,
    pub downloaded_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MonitoredPlaylist {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub cover_art_path: Option<String>,
    pub source_platform: Option<String>,
    pub source_url: Option<String>,
    pub source_id: Option<String>,
    pub track_count: i64,
    pub total_duration_ms: i64,
    pub is_synced: bool,
    pub last_synced_at: Option<String>,
    pub created_at: String,
    pub new_count: i64,
    pub downloaded_count: i64,
    pub active_count: i64,
    pub total_entries: i64,
}

/// Get all playlists that have a source_url (i.e., monitored external playlists)
pub fn get_monitored_playlists(conn: &Connection) -> Result<Vec<MonitoredPlaylist>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.description, p.cover_art_path, p.source_platform,
                p.source_url, p.source_id, p.track_count, p.total_duration_ms,
                p.is_synced, p.last_synced_at, p.created_at,
                COALESCE(SUM(CASE WHEN e.status = 'new' THEN 1 ELSE 0 END), 0) as new_count,
                COALESCE(SUM(CASE WHEN e.status = 'downloaded' THEN 1 ELSE 0 END), 0) as downloaded_count,
                COALESCE(SUM(CASE WHEN e.status IN ('queued', 'downloading') THEN 1 ELSE 0 END), 0) as active_count,
                COUNT(e.id) as total_entries
         FROM playlists p
         LEFT JOIN monitored_playlist_entries e ON e.playlist_id = p.id
         WHERE p.source_url IS NOT NULL AND p.source_url != ''
         GROUP BY p.id
         ORDER BY p.created_at DESC",
    )?;

    let rows = stmt.query_map([], |row| {
        Ok(MonitoredPlaylist {
            id: row.get(0)?,
            name: row.get(1)?,
            description: row.get(2)?,
            cover_art_path: row.get(3)?,
            source_platform: row.get(4)?,
            source_url: row.get(5)?,
            source_id: row.get(6)?,
            track_count: row.get(7)?,
            total_duration_ms: row.get(8)?,
            is_synced: row.get(9)?,
            last_synced_at: row.get(10)?,
            created_at: row.get(11)?,
            new_count: row.get(12)?,
            downloaded_count: row.get(13)?,
            active_count: row.get(14)?,
            total_entries: row.get(15)?,
        })
    })?;

    rows.collect()
}

/// Create a monitored playlist from an external source
pub fn create_monitored_playlist(
    conn: &Connection,
    name: &str,
    platform: &str,
    source_url: &str,
    source_id: Option<&str>,
) -> Result<Playlist, rusqlite::Error> {
    conn.execute(
        "INSERT INTO playlists (name, source_platform, source_url, source_id, is_synced)
         VALUES (?1, ?2, ?3, ?4, 1)",
        params![name, platform, source_url, source_id],
    )?;
    let id = conn.last_insert_rowid();

    conn.query_row(
        "SELECT id, name, description, cover_art_path, source_platform, source_url,
                track_count, total_duration_ms, is_synced, last_synced_at, created_at
         FROM playlists WHERE id = ?1",
        params![id],
        |row| {
            Ok(Playlist {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                cover_art_path: row.get(3)?,
                source_platform: row.get(4)?,
                source_url: row.get(5)?,
                track_count: row.get(6)?,
                total_duration_ms: row.get(7)?,
                is_synced: row.get(8)?,
                last_synced_at: row.get(9)?,
                created_at: row.get(10)?,
            })
        },
    )
}

/// Upsert entries for a monitored playlist. Returns (new_count, total_count).
pub fn upsert_entries(
    conn: &Connection,
    playlist_id: i64,
    entries: &[(String, Option<String>, Option<String>, Option<f64>, Option<String>)], // (url, title, artist, duration, thumbnail)
) -> Result<(i64, i64), rusqlite::Error> {
    log::info!("upsert_entries called for playlist_id: {} with {} entries", playlist_id, entries.len());
    let mut new_count = 0i64;

    // Batch all upserts in a single transaction for dramatically faster writes
    conn.execute_batch("BEGIN")?;

    // Pre-fetch existing URLs to detect new vs update without per-row queries
    let mut existing_stmt = conn.prepare(
        "SELECT source_url FROM monitored_playlist_entries WHERE playlist_id = ?1",
    )?;
    let existing_urls: std::collections::HashSet<String> = existing_stmt
        .query_map(params![playlist_id], |row| row.get::<_, String>(0))?
        .filter_map(|r| r.ok())
        .collect();
    drop(existing_stmt);

    let mut insert_stmt = conn.prepare(
        "INSERT INTO monitored_playlist_entries (playlist_id, source_url, title, artist, duration_seconds, thumbnail, position)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
         ON CONFLICT(playlist_id, source_url) DO UPDATE SET
            title = COALESCE(excluded.title, monitored_playlist_entries.title),
            artist = COALESCE(excluded.artist, monitored_playlist_entries.artist),
            duration_seconds = COALESCE(excluded.duration_seconds, monitored_playlist_entries.duration_seconds),
            thumbnail = COALESCE(excluded.thumbnail, monitored_playlist_entries.thumbnail),
            position = excluded.position",
    )?;

    for (i, (url, title, artist, duration, thumbnail)) in entries.iter().enumerate() {
        insert_stmt.execute(params![
            playlist_id,
            url,
            title,
            artist,
            duration,
            thumbnail,
            i as i64,
        ])?;
        // If the URL wasn't in the existing set, it's a new entry
        if !existing_urls.contains(url) {
            new_count += 1;
        }
    }
    drop(insert_stmt);

    let total_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM monitored_playlist_entries WHERE playlist_id = ?1",
        params![playlist_id],
        |row| row.get(0),
    )?;

    // Update last_synced_at and track_count
    conn.execute(
        "UPDATE playlists SET last_synced_at = datetime('now'), track_count = ?2, updated_at = datetime('now') WHERE id = ?1",
        params![playlist_id, total_count],
    )?;

    conn.execute_batch("COMMIT")?;

    log::info!("upsert_entries completed: new={}, total={} for playlist_id={}", new_count, total_count, playlist_id);
    Ok((new_count, total_count))
}

fn entry_from_row(row: &rusqlite::Row) -> rusqlite::Result<MonitoredEntry> {
    Ok(MonitoredEntry {
        id: row.get(0)?,
        playlist_id: row.get(1)?,
        source_url: row.get(2)?,
        title: row.get(3)?,
        artist: row.get(4)?,
        duration_seconds: row.get(5)?,
        thumbnail: row.get(6)?,
        status: row.get(7)?,
        download_id: row.get(8)?,
        track_id: row.get(9)?,
        position: row.get(10)?,
        first_seen_at: row.get(11)?,
        downloaded_at: row.get(12)?,
    })
}

const ENTRY_COLUMNS: &str =
    "id, playlist_id, source_url, title, artist, duration_seconds, thumbnail, \
     status, download_id, track_id, position, first_seen_at, downloaded_at";

fn query_entries(conn: &Connection, playlist_id: i64, status_filter: Option<&str>) -> Result<Vec<MonitoredEntry>, rusqlite::Error> {
    match status_filter {
        Some(status) => {
            let sql = format!(
                "SELECT {} FROM monitored_playlist_entries WHERE playlist_id = ?1 AND status = ?2 ORDER BY position ASC",
                ENTRY_COLUMNS
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![playlist_id, status], entry_from_row)?;
            rows.collect()
        }
        None => {
            let sql = format!(
                "SELECT {} FROM monitored_playlist_entries WHERE playlist_id = ?1 ORDER BY position ASC",
                ENTRY_COLUMNS
            );
            let mut stmt = conn.prepare(&sql)?;
            let rows = stmt.query_map(params![playlist_id], entry_from_row)?;
            rows.collect()
        }
    }
}

/// Get all entries for a monitored playlist
pub fn get_entries(conn: &Connection, playlist_id: i64) -> Result<Vec<MonitoredEntry>, rusqlite::Error> {
    log::info!("get_entries called for playlist_id: {}", playlist_id);
    let entries = query_entries(conn, playlist_id, None)?;
    log::info!("get_entries returning {} entries for playlist_id: {}", entries.len(), playlist_id);
    Ok(entries)
}

/// Get new (not yet downloaded) entries for a monitored playlist
pub fn get_new_entries(conn: &Connection, playlist_id: i64) -> Result<Vec<MonitoredEntry>, rusqlite::Error> {
    query_entries(conn, playlist_id, Some("new"))
}

/// Update entry status
pub fn update_entry_status(
    conn: &Connection,
    entry_id: i64,
    status: &str,
    download_id: Option<i64>,
    track_id: Option<i64>,
) -> Result<(), rusqlite::Error> {
    if status == "downloaded" {
        conn.execute(
            "UPDATE monitored_playlist_entries SET status = ?2, download_id = ?3, track_id = ?4, downloaded_at = datetime('now') WHERE id = ?1",
            params![entry_id, status, download_id, track_id],
        )?;
    } else {
        conn.execute(
            "UPDATE monitored_playlist_entries SET status = ?2, download_id = ?3, track_id = ?4 WHERE id = ?1",
            params![entry_id, status, download_id, track_id],
        )?;
    }
    Ok(())
}

/// Mark entry as skipped
pub fn skip_entry(conn: &Connection, entry_id: i64) -> Result<(), rusqlite::Error> {
    conn.execute(
        "UPDATE monitored_playlist_entries SET status = 'skipped' WHERE id = ?1",
        params![entry_id],
    )?;
    Ok(())
}

/// Delete a monitored playlist and its entries
pub fn delete_monitored_playlist(conn: &Connection, id: i64) -> Result<(), rusqlite::Error> {
    // Entries cascade-delete via FK
    conn.execute("DELETE FROM playlists WHERE id = ?1", params![id])?;
    Ok(())
}

/// Get a single entry by id
pub fn get_entry(conn: &Connection, entry_id: i64) -> Result<Option<MonitoredEntry>, rusqlite::Error> {
    let sql = format!(
        "SELECT {} FROM monitored_playlist_entries WHERE id = ?1",
        ENTRY_COLUMNS
    );
    conn.query_row(&sql, params![entry_id], entry_from_row).optional()
}

trait OptionalExt<T> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for Result<T, rusqlite::Error> {
    fn optional(self) -> Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
