//! Album queries, detail tracklists, and album download status.

use std::sync::Arc;
use rusqlite::params;
use tauri::State;

use crate::db::DbPool;
use crate::db::models::*;

// --- Albums ---

#[tauri::command]
pub fn library_get_albums(
    db: State<'_, Arc<DbPool>>,
    offset: i64,
    limit: i64,
    search: Option<String>,
) -> Result<(Vec<Album>, i64), String> {
    let conn = crate::db::lock(&db)?;
    crate::db::albums::get_albums(&conn, offset, limit, search.as_deref()).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_album(db: State<'_, Arc<DbPool>>, id: i64) -> Result<Option<Album>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::albums::get_album(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_recently_played_albums(db: State<'_, Arc<DbPool>>, limit: i64) -> Result<Vec<Album>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::albums::get_recently_played_albums(&conn, limit).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn library_get_recently_added_albums(db: State<'_, Arc<DbPool>>, limit: i64) -> Result<Vec<Album>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::albums::get_recently_added_albums(&conn, limit).map_err(|e| e.to_string())
}

// --- Album Tracks ---

#[tauri::command]
pub fn library_get_album_tracks(
    db: State<'_, Arc<DbPool>>,
    album_id: i64,
) -> Result<Vec<Track>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::tracks::get_tracks_by_album(&conn, album_id).map_err(|e| e.to_string())
}

// --- Album Download Status ---

#[tauri::command]
pub fn library_get_album_download_status(
    db: State<'_, Arc<DbPool>>,
    album_id: i64,
) -> Result<serde_json::Value, String> {
    let conn = crate::db::lock(&db)?;

    // An unknown album id is "unknown" status, not an error (matches the
    // batch variant's behavior).
    let tracklist_json: Option<String> = match conn.query_row(
        "SELECT enriched_tracklist FROM albums WHERE id = ?1",
        params![album_id],
        |row| row.get(0),
    ) {
        Ok(v) => v,
        Err(rusqlite::Error::QueryReturnedNoRows) => None,
        Err(e) => return Err(e.to_string()),
    };

    let Some(json_str) = tracklist_json else {
        return Ok(serde_json::json!({ "total_expected": 0, "total_local": 0, "status": "unknown" }));
    };

    let tracklist: Vec<serde_json::Value> = serde_json::from_str(&json_str).unwrap_or_default();
    if tracklist.is_empty() {
        return Ok(serde_json::json!({ "total_expected": 0, "total_local": 0, "status": "unknown" }));
    }

    let total_expected = tracklist.len() as i64;
    let total_local: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tracks WHERE album_id = ?1",
        params![album_id],
        |row| row.get(0),
    ).unwrap_or(0);

    let status = if total_local >= total_expected {
        "complete"
    } else if total_local > 0 {
        "partial"
    } else {
        "none"
    };

    Ok(serde_json::json!({
        "total_expected": total_expected,
        "total_local": total_local,
        "status": status,
    }))
}

#[tauri::command]
pub fn library_get_albums_download_status(
    db: State<'_, Arc<DbPool>>,
    album_ids: Vec<i64>,
) -> Result<serde_json::Value, String> {
    let conn = crate::db::lock(&db)?;
    let mut results = serde_json::Map::new();

    // One aggregate query for all local track counts instead of a COUNT per album.
    let mut local_counts: std::collections::HashMap<i64, i64> =
        std::collections::HashMap::new();
    if !album_ids.is_empty() {
        let placeholders = album_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT album_id, COUNT(*) FROM tracks WHERE album_id IN ({}) GROUP BY album_id",
            placeholders
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(album_ids.iter()), |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows.filter_map(|r| r.ok()) {
            local_counts.insert(row.0, row.1);
        }
    }

    for album_id in &album_ids {
        let tracklist_json: Option<String> = conn.query_row(
            "SELECT enriched_tracklist FROM albums WHERE id = ?1",
            params![album_id],
            |row| row.get(0),
        ).unwrap_or(None);

        let total_local: i64 = local_counts.get(album_id).copied().unwrap_or(0);

        let (total_expected, status) = if let Some(ref json_str) = tracklist_json {
            let tracklist: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap_or_default();
            if tracklist.is_empty() {
                (0i64, "unknown")
            } else {
                let expected = tracklist.len() as i64;
                let s = if total_local >= expected { "complete" } else if total_local > 0 { "partial" } else { "none" };
                (expected, s)
            }
        } else {
            (0, "unknown")
        };

        results.insert(album_id.to_string(), serde_json::json!({
            "total_expected": total_expected,
            "total_local": total_local,
            "status": status,
        }));
    }

    Ok(serde_json::Value::Object(results))
}

#[tauri::command]
pub fn library_get_artist_missing_albums(
    db: State<'_, Arc<DbPool>>,
    artist_id: i64,
) -> Result<serde_json::Value, String> {
    let conn = crate::db::lock(&db)?;

    // Get enriched discography
    let disco_json: Option<String> = conn.query_row(
        "SELECT enriched_discography FROM artists WHERE id = ?1",
        params![artist_id],
        |row| row.get(0),
    ).map_err(|e| e.to_string())?;

    let Some(json_str) = disco_json else {
        return Ok(serde_json::json!({ "missing": [], "total_discography": 0 }));
    };

    let discography: Vec<crate::metadata::musicbrainz::ArtistDiscographyEntry> =
        serde_json::from_str(&json_str).unwrap_or_default();

    // Get local albums for this artist (titles, lowercased for comparison)
    let mut stmt = conn.prepare(
        "SELECT LOWER(title), musicbrainz_id FROM albums WHERE artist_id = ?1"
    ).map_err(|e| e.to_string())?;
    let local_albums: std::collections::HashSet<String> = stmt.query_map(params![artist_id], |row| {
        row.get::<_, String>(0)
    }).map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let local_mbids: std::collections::HashSet<String> = {
        let mut stmt2 = conn.prepare(
            "SELECT musicbrainz_id FROM albums WHERE artist_id = ?1 AND musicbrainz_id IS NOT NULL"
        ).map_err(|e| e.to_string())?;
        let results: std::collections::HashSet<String> = stmt2.query_map(params![artist_id], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();
        results
    };

    let missing: Vec<&crate::metadata::musicbrainz::ArtistDiscographyEntry> = discography.iter()
        .filter(|entry| {
            !local_mbids.contains(&entry.mbid)
                && !local_albums.contains(&entry.title.to_lowercase())
        })
        .collect();

    let total = discography.len();
    Ok(serde_json::json!({
        "missing": missing,
        "total_discography": total,
    }))
}
