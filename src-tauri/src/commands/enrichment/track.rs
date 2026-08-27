//! Enrich a single track from MusicBrainz.

use std::sync::Arc;
use tauri::{Manager, State};

use crate::db::DbPool;

#[derive(Debug, serde::Serialize)]
pub struct EnrichResult {
    pub track_id: i64,
    pub fields_updated: i64,
    pub completeness: i64,
}

/// Enrich a single track's metadata from MusicBrainz
#[tauri::command]
pub async fn enrich_track(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    track_id: i64,
) -> Result<EnrichResult, String> {
    // Get track info for MusicBrainz search
    let (title, artist_name, duration_ms, has_lyrics, has_mv) = {
        let conn = crate::db::lock(&db)?;
        let track = crate::db::tracks::get_track(&conn, track_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Track not found".to_string())?;
        (track.title, track.artist_name, track.duration_ms, track.lyrics.is_some(), track.music_video_url.is_some())
    };

    let enrichment = match crate::metadata::musicbrainz::enrich_track(&title, artist_name.as_deref()).await {
        Ok(e) => Some(e),
        Err(e) => {
            log::warn!("MusicBrainz enrichment failed for '{}': {}", title, e);
            None
        }
    };

    // Apply enrichment to DB (only fill missing fields) — scoped to drop conn before async work
    let mut updated = 0i64;
    if let Some(ref enrichment) = enrichment {
        let conn = crate::db::lock(&db)?;

        // Fill missing track fields from MusicBrainz
        let track_fields: &[(&str, &Option<String>)] = &[
            ("musicbrainz_id", &enrichment.musicbrainz_id),
            ("genre", &enrichment.genre),
            ("release_date", &enrichment.release_date),
            ("isrc", &enrichment.isrc),
            ("description", &enrichment.description),
            ("label", &enrichment.label),
            ("language", &enrichment.language),
            ("composer", &enrichment.composer),
        ];
        for &(col, val) in track_fields {
            if let Some(ref v) = val {
                updated += crate::db::update_field_if_missing(&conn, "tracks", col, track_id, v);
            }
        }

        // Populate track year from release_date if missing
        if let Some(ref rd) = enrichment.release_date {
            if rd.len() >= 4 {
                if let Ok(year) = rd[..4].parse::<i64>() {
                    let _ = conn.execute(
                        "UPDATE tracks SET year = ?1 WHERE id = ?2 AND year IS NULL",
                        rusqlite::params![year, track_id],
                    );
                }
            }
        }

        // Merge MusicBrainz tags with existing tags
        if let Some(ref new_tags) = enrichment.tags {
            updated += crate::db::merge_tags(&conn, track_id, new_tags);
        }

        // Update artist info if we have MusicBrainz data
        if enrichment.artist_musicbrainz_id.is_some() {
            let artist_id: Option<i64> = conn.query_row(
                "SELECT artist_id FROM tracks WHERE id = ?1",
                rusqlite::params![track_id],
                |row| row.get(0),
            ).ok();
            if let Some(aid) = artist_id {
                crate::db::apply_artist_enrichment(
                    &conn, aid,
                    enrichment.artist_musicbrainz_id.as_deref(),
                    enrichment.artist_sort_name.as_deref(),
                    enrichment.artist_type.as_deref(),
                    enrichment.artist_country.as_deref(),
                    enrichment.artist_begin_year,
                    enrichment.artist_website_url.as_deref(),
                );
            }
        }

        // Update album info
        {
            // Prefer the release-group MBID for albums.musicbrainz_id (it must
            // match discography entries during missing-album detection); fall
            // back to the release id when MusicBrainz didn't provide one.
            let album_mbid = enrichment.album_release_group_mbid.as_ref()
                .or(enrichment.album_musicbrainz_id.as_ref());
            let album_id: Option<i64> = conn.query_row(
                "SELECT album_id FROM tracks WHERE id = ?1",
                rusqlite::params![track_id],
                |row| row.get(0),
            ).ok().flatten();
            if let Some(aid) = album_id {
                for &(col, val) in &[
                    ("musicbrainz_id", album_mbid.map(|s| s.as_str())),
                    ("release_date", enrichment.album_release_date.as_deref()),
                    ("album_type", enrichment.album_type.as_deref()),
                    ("purchase_url", enrichment.album_purchase_url.as_deref()),
                ] {
                    if let Some(v) = val {
                        crate::db::update_field_if_missing(&conn, "albums", col, aid, &v);
                    }
                }
            }
        }

        // Release DB lock before async lyrics/MV fetches
        let _ = crate::db::tracks::update_fts(&conn, track_id);
    } // conn dropped here (enrichment block)

    // Fetch lyrics if missing
    if !has_lyrics {
        if let Some(ref art) = artist_name {
            let duration_secs = duration_ms.map(|ms| ms as f64 / 1000.0);
            if let Ok(lyrics) = crate::metadata::lyrics::fetch_lyrics(&title, art, duration_secs).await {
                if let Ok(conn) = db.lock() {
                    let changed = conn.execute(
                        "UPDATE tracks SET lyrics = ?1 WHERE id = ?2 AND lyrics IS NULL",
                        rusqlite::params![lyrics, track_id],
                    ).unwrap_or(0);
                    updated += changed as i64;
                }
            }
        }
    }

    // Find official music video if missing
    if !has_mv {
        // Step 1: Use MusicBrainz confirmed MV URL if available
        let mut mv_url = enrichment.as_ref().and_then(|e| e.music_video_url.clone());

        // Step 2: Fall back to YouTube search for official music video
        if mv_url.is_none() {
            if let Some(ref art) = artist_name {
                let bin_dir = crate::download::setup::get_bin_dir(&app_handle);
                let binary = crate::download::setup::resolve_ytdlp(&bin_dir)
                    .unwrap_or_else(|| "yt-dlp".to_string());
                let ffmpeg_dir = crate::download::setup::resolve_ffmpeg_dir(&bin_dir);
                let cookies = db.lock().ok()
                    .and_then(|conn| crate::db::settings::get_cookies_browser(&conn));
                mv_url = crate::download::ytdlp::search_music_video(
                    &binary,
                    ffmpeg_dir.as_deref(),
                    art,
                    &title,
                    cookies.as_deref(),
                ).await;
            }
        }

        if let Some(ref mv) = mv_url {
            if let Ok(conn) = db.lock() {
                let changed = conn.execute(
                    "UPDATE tracks SET music_video_url = ?1 WHERE id = ?2 AND music_video_url IS NULL",
                    rusqlite::params![mv, track_id],
                ).unwrap_or(0);
                updated += changed as i64;
            }

            // Auto-download music video if setting is enabled
            let auto_dl = db.lock().ok()
                .and_then(|conn| crate::db::settings::get_setting(&conn, "auto_download_music_videos").ok().flatten())
                .is_some_and(|v| v == "true");
            if auto_dl {
                let mv_dir = app_handle.path().app_data_dir()
                    .ok()
                    .map(|d| d.join("music_videos"));
                if let Some(ref mv_dir) = mv_dir {
                    let _ = std::fs::create_dir_all(mv_dir);
                    let bin_dir = crate::download::setup::get_bin_dir(&app_handle);
                    let binary = crate::download::setup::resolve_ytdlp(&bin_dir)
                        .unwrap_or_else(|| "yt-dlp".to_string());
                    let ffmpeg_dir = crate::download::setup::resolve_ffmpeg_dir(&bin_dir);
                    let cookies = db.lock().ok()
                        .and_then(|conn| crate::db::settings::get_cookies_browser(&conn));
                    let file_stem = format!("mv_{}", track_id);
                    if let Ok(path) = crate::download::ytdlp::download_video(
                        &binary,
                        ffmpeg_dir.as_deref(),
                        mv,
                        mv_dir,
                        &file_stem,
                        cookies.as_deref(),
                        |_| {}, // Background auto-download, no progress needed
                    ).await {
                        if let Ok(conn) = db.lock() {
                            let _ = conn.execute(
                                "UPDATE tracks SET music_video_path = ?1 WHERE id = ?2",
                                rusqlite::params![path, track_id],
                            );
                        }
                    }
                }
            }
        }
    }

    let conn = crate::db::lock(&db)?;
    let completeness = crate::db::tracks::update_completeness(&conn, track_id)
        .map_err(|e| e.to_string())?;

    Ok(EnrichResult { track_id, fields_updated: updated, completeness })
}
