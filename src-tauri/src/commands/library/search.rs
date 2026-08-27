//! Full-text search across tracks, albums, artists, and playlists.

use std::sync::Arc;
use rusqlite::params;
use tauri::State;

use crate::db::DbPool;
use crate::db::models::*;

// --- Search ---

#[tauri::command]
pub fn search(
    db: State<'_, Arc<DbPool>>,
    query: String,
    limit: Option<i64>,
) -> Result<SearchResults, String> {
    let conn = crate::db::lock(&db)?;
    let limit = limit.unwrap_or(50);

    let tracks = if query.trim().is_empty() {
        vec![]
    } else {
        crate::db::tracks::search_tracks_fts(&conn, &query, limit)
            .unwrap_or_default()
    };

    // Also search albums and artists by name
    let albums = if query.trim().is_empty() {
        vec![]
    } else {
        let pattern = format!("%{}%", crate::db::escape_like(&query));
        let mut stmt = conn
            .prepare(
                "SELECT al.id, al.title, al.artist_id, al.album_artist, al.year, al.genre,
                        al.total_tracks, al.total_discs, al.musicbrainz_id, al.cover_art_path,
                        a.name as artist_name, COUNT(t.id) as track_count
                 FROM albums al
                 LEFT JOIN artists a ON al.artist_id = a.id
                 LEFT JOIN tracks t ON t.album_id = al.id
                 WHERE al.title LIKE ?1 ESCAPE '\\'
                 GROUP BY al.id
                 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let result = stmt.query_map(params![pattern, limit], |row| {
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
                label: None,
                release_date: None,
                description: None,
                album_type: None,
                enriched_tracklist: None,
                purchase_url: None,
                artist_name: row.get(10)?,
                track_count: row.get(11)?,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();
        result
    };

    let artists = if query.trim().is_empty() {
        vec![]
    } else {
        let pattern = format!("%{}%", crate::db::escape_like(&query));
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.name, a.sort_name, a.musicbrainz_id, a.image_path, a.bio,
                        COUNT(t.id) as track_count
                 FROM artists a
                 LEFT JOIN tracks t ON t.artist_id = a.id
                 WHERE a.name LIKE ?1 ESCAPE '\\'
                 GROUP BY a.id
                 LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;

        let result = stmt.query_map(params![pattern, limit], |row| {
            Ok(Artist {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_name: row.get(2)?,
                musicbrainz_id: row.get(3)?,
                image_path: row.get(4)?,
                bio: row.get(5)?,
                country: None,
                begin_year: None,
                artist_type: None,
                website_url: None,
                track_count: row.get(6)?,
                has_enriched_discography: false,
                fallback_cover_path: None,
            })
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .unwrap_or_default();
        result
    };

    Ok(SearchResults {
        tracks,
        albums,
        artists,
    })
}
