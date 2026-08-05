//! Listening-history statistics (backed by the `plays` event table).

use std::sync::Arc;
use rusqlite::params;
use serde::Serialize;
use tauri::State;

use crate::db::DbPool;

#[derive(Debug, Serialize)]
pub struct DayPlays {
    pub day: String,
    pub count: i64,
}

#[derive(Debug, Serialize)]
pub struct StatsOverview {
    pub total_plays: i64,
    pub total_listening_ms: i64,
    pub distinct_tracks: i64,
    pub distinct_artists: i64,
    pub distinct_albums: i64,
    pub plays_per_day: Vec<DayPlays>,
}

#[tauri::command]
pub fn stats_overview(db: State<'_, Arc<DbPool>>) -> Result<StatsOverview, String> {
    let conn = crate::db::lock(&db)?;

    let (total_plays, total_listening_ms, distinct_tracks, distinct_artists, distinct_albums) = conn
        .query_row(
            "SELECT COUNT(*),
                    COALESCE(SUM(t.duration_ms), 0),
                    COUNT(DISTINCT p.track_id),
                    COUNT(DISTINCT t.artist_id),
                    COUNT(DISTINCT t.album_id)
             FROM plays p
             LEFT JOIN tracks t ON t.id = p.track_id",
            [],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            },
        )
        .map_err(|e| e.to_string())?;

    let mut stmt = conn
        .prepare(
            "SELECT date(played_at) AS day, COUNT(*)
             FROM plays
             WHERE played_at >= datetime('now', '-90 days')
             GROUP BY day
             ORDER BY day ASC",
        )
        .map_err(|e| e.to_string())?;
    let plays_per_day: Vec<DayPlays> = stmt
        .query_map([], |row| {
            Ok(DayPlays {
                day: row.get(0)?,
                count: row.get(1)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(StatsOverview {
        total_plays,
        total_listening_ms,
        distinct_tracks,
        distinct_artists,
        distinct_albums,
        plays_per_day,
    })
}

#[derive(Debug, Serialize)]
pub struct TopTrack {
    pub id: i64,
    pub title: String,
    pub artist_id: Option<i64>,
    pub artist_name: Option<String>,
    pub cover_art_path: Option<String>,
    pub play_count: i64,
}

#[derive(Debug, Serialize)]
pub struct TopArtist {
    pub id: i64,
    pub name: String,
    pub image_path: Option<String>,
    pub play_count: i64,
}

#[derive(Debug, Serialize)]
pub struct TopAlbum {
    pub id: i64,
    pub title: String,
    pub artist_name: Option<String>,
    pub cover_art_path: Option<String>,
    pub play_count: i64,
}

#[derive(Debug, Serialize)]
pub struct StatsTop {
    pub tracks: Vec<TopTrack>,
    pub artists: Vec<TopArtist>,
    pub albums: Vec<TopAlbum>,
}

/// SQL time filter for a stats period ("week" | "month" | "year" | "all").
fn period_modifier(period: &str) -> Option<&'static str> {
    match period {
        "week" => Some("-7 days"),
        "month" => Some("-1 month"),
        "year" => Some("-1 year"),
        _ => None,
    }
}

#[tauri::command]
pub fn stats_top(
    db: State<'_, Arc<DbPool>>,
    period: String,
    limit: Option<i64>,
) -> Result<StatsTop, String> {
    let conn = crate::db::lock(&db)?;
    let limit = limit.unwrap_or(10).clamp(1, 100);
    let modifier = period_modifier(&period);

    // Shared WHERE clause: "?1" is the time modifier; when period = all we
    // pass a modifier that matches everything via the OR branch.
    let where_clause = if modifier.is_some() {
        "WHERE p.played_at >= datetime('now', ?1)"
    } else {
        "WHERE ?1 = ?1"
    };
    let modifier_param = modifier.unwrap_or("all");

    let tracks_sql = format!(
        "SELECT t.id, t.title, t.artist_id, a.name, t.cover_art_path, COUNT(*) AS c
         FROM plays p
         JOIN tracks t ON t.id = p.track_id
         LEFT JOIN artists a ON t.artist_id = a.id
         {where_clause}
         GROUP BY t.id
         ORDER BY c DESC, t.title ASC
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&tracks_sql).map_err(|e| e.to_string())?;
    let tracks: Vec<TopTrack> = stmt
        .query_map(params![modifier_param, limit], |row| {
            Ok(TopTrack {
                id: row.get(0)?,
                title: row.get(1)?,
                artist_id: row.get(2)?,
                artist_name: row.get(3)?,
                cover_art_path: row.get(4)?,
                play_count: row.get(5)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let artists_sql = format!(
        "SELECT a.id, a.name, a.image_path, COUNT(*) AS c
         FROM plays p
         JOIN tracks t ON t.id = p.track_id
         JOIN artists a ON t.artist_id = a.id
         {where_clause}
         GROUP BY a.id
         ORDER BY c DESC, a.name ASC
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&artists_sql).map_err(|e| e.to_string())?;
    let artists: Vec<TopArtist> = stmt
        .query_map(params![modifier_param, limit], |row| {
            Ok(TopArtist {
                id: row.get(0)?,
                name: row.get(1)?,
                image_path: row.get(2)?,
                play_count: row.get(3)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let albums_sql = format!(
        "SELECT al.id, al.title, ar.name, al.cover_art_path, COUNT(*) AS c
         FROM plays p
         JOIN tracks t ON t.id = p.track_id
         JOIN albums al ON t.album_id = al.id
         LEFT JOIN artists ar ON al.artist_id = ar.id
         {where_clause}
         GROUP BY al.id
         ORDER BY c DESC, al.title ASC
         LIMIT ?2"
    );
    let mut stmt = conn.prepare(&albums_sql).map_err(|e| e.to_string())?;
    let albums: Vec<TopAlbum> = stmt
        .query_map(params![modifier_param, limit], |row| {
            Ok(TopAlbum {
                id: row.get(0)?,
                title: row.get(1)?,
                artist_name: row.get(2)?,
                cover_art_path: row.get(3)?,
                play_count: row.get(4)?,
            })
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(StatsTop { tracks, artists, albums })
}
