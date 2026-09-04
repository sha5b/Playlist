
/// Escape SQL LIKE wildcards in user input so `%`/`_` in a search string match
/// literally. Every LIKE that interpolates user input must pair this with
/// `ESCAPE '\'` in its query.
pub fn escape_like(q: &str) -> String {
    q.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

pub mod albums;
pub mod artists;
pub mod devices;
pub mod downloads;
pub mod migrations;
pub mod models;
pub mod monitored;
pub mod playlists;
pub mod settings;
pub mod smart;
pub mod tracks;

use rusqlite::Connection;
use std::path::Path;
use std::sync::{Mutex, MutexGuard};

pub type DbPool = Mutex<Connection>;

/// Lock the database connection, converting a poisoned-lock error to `String`
/// for Tauri command compatibility.  Replaces the 60+ occurrences of
/// `db.lock().map_err(|e| e.to_string())` scattered across the codebase.
pub fn lock(pool: &DbPool) -> Result<MutexGuard<'_, Connection>, String> {
    pool.lock().map_err(|e| format!("DB lock poisoned: {}", e))
}

/// Update a single column to `val` only when the current value is NULL or empty.
/// Returns the number of rows changed (0 or 1).
pub fn update_field_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    id: i64,
    val: &dyn rusqlite::types::ToSql,
) -> i64 {
    conn.execute(
        &format!(
            "UPDATE {table} SET {column} = ?1 WHERE id = ?2 AND ({column} IS NULL OR {column} = '')"
        ),
        rusqlite::params![val, id],
    )
    .unwrap_or(0) as i64
}

/// Merge new tags into an existing JSON tag array on a track, deduplicating
/// case-insensitively.  Returns the number of rows changed.
pub fn merge_tags(conn: &Connection, track_id: i64, new_tags: &[String]) -> i64 {
    let existing_json: Option<String> = conn
        .query_row(
            "SELECT tags FROM tracks WHERE id = ?1",
            rusqlite::params![track_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    let mut all_tags: Vec<String> = existing_json
        .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
        .unwrap_or_default();
    for tag in new_tags {
        let lower = tag.to_lowercase();
        if !all_tags.iter().any(|t| t.to_lowercase() == lower) {
            all_tags.push(tag.clone());
        }
    }
    if all_tags.is_empty() {
        return 0;
    }
    match serde_json::to_string(&all_tags) {
        Ok(json) => conn
            .execute(
                "UPDATE tracks SET tags = ?1 WHERE id = ?2",
                rusqlite::params![json, track_id],
            )
            .unwrap_or(0) as i64,
        Err(_) => 0,
    }
}

/// Apply MusicBrainz artist fields to an artist row, filling only NULL columns.
#[allow(clippy::too_many_arguments)]
pub fn apply_artist_enrichment(
    conn: &Connection,
    artist_id: i64,
    mb_id: Option<&str>,
    sort_name: Option<&str>,
    artist_type: Option<&str>,
    country: Option<&str>,
    begin_year: Option<i64>,
    website_url: Option<&str>,
) {
    macro_rules! fill {
        ($col:literal, $val:expr) => {
            if let Some(v) = $val {
                update_field_if_missing(conn, "artists", $col, artist_id, &v);
            }
        };
    }
    fill!("musicbrainz_id", mb_id);
    fill!("sort_name", sort_name);
    fill!("artist_type", artist_type);
    fill!("country", country);
    fill!("begin_year", begin_year);
    fill!("website_url", website_url);
}

pub fn init_db(app_data_dir: &Path) -> Result<DbPool, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(app_data_dir)?;
    let db_path = app_data_dir.join("playlist.db");
    let conn = Connection::open(&db_path)?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;

    migrations::run(&conn)?;

    Ok(Mutex::new(conn))
}

/// Open a second connection to the same database for use by the download manager.
/// WAL mode allows concurrent readers/writers without blocking the main connection.
pub fn open_download_conn(app_data_dir: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    let db_path = app_data_dir.join("playlist.db");
    let conn = Connection::open(&db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(conn)
}
