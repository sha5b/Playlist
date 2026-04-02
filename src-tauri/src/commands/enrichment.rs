use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use rusqlite::params;
use tauri::{Emitter, Manager, State};

use crate::db::DbPool;

/// Global cancellation flag for metadata scans
static METADATA_SCAN_CANCELLED: AtomicBool = AtomicBool::new(false);

#[derive(Debug, serde::Serialize)]
pub struct EnrichResult {
    pub track_id: i64,
    pub fields_updated: i64,
    pub completeness: i64,
}

#[derive(Debug, serde::Serialize)]
pub struct ScanMissingResult {
    pub total_tracks: i64,
    pub enriched: i64,
    pub failed: i64,
    pub completeness_avg: i64,
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
        let conn = db.lock().map_err(|e| e.to_string())?;
        let track = crate::db::tracks::get_track(&conn, track_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Track not found".to_string())?;
        (track.title, track.artist_name, track.duration_ms, track.lyrics.is_some(), track.music_video_url.is_some())
    };

    let enrichment = crate::metadata::musicbrainz::enrich_track(&title, artist_name.as_deref()).await?;

    // Apply enrichment to DB (only fill missing fields) — scoped to drop conn before async work
    let mut updated = 0i64;
    {
    let conn = db.lock().map_err(|e| e.to_string())?;

    macro_rules! update_if_missing {
        ($col:expr, $val:expr) => {
            if let Some(ref v) = $val {
                let changed = conn.execute(
                    &format!("UPDATE tracks SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                    rusqlite::params![v, track_id],
                ).unwrap_or(0);
                updated += changed as i64;
            }
        };
    }

    update_if_missing!("musicbrainz_id", enrichment.musicbrainz_id);
    update_if_missing!("genre", enrichment.genre);
    update_if_missing!("release_date", enrichment.release_date);
    update_if_missing!("isrc", enrichment.isrc);
    update_if_missing!("description", enrichment.description);
    update_if_missing!("label", enrichment.label);
    update_if_missing!("language", enrichment.language);

    // Merge MusicBrainz tags with existing tags
    if let Some(ref new_tags) = enrichment.tags {
        let existing_json: Option<String> = conn.query_row(
            "SELECT tags FROM tracks WHERE id = ?1",
            rusqlite::params![track_id],
            |row| row.get(0),
        ).ok().flatten();
        let mut all_tags: Vec<String> = existing_json
            .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
            .unwrap_or_default();
        for tag in new_tags {
            let lower = tag.to_lowercase();
            if !all_tags.iter().any(|t| t.to_lowercase() == lower) {
                all_tags.push(tag.clone());
            }
        }
        if !all_tags.is_empty() {
            if let Ok(json) = serde_json::to_string(&all_tags) {
                let changed = conn.execute(
                    "UPDATE tracks SET tags = ?1 WHERE id = ?2",
                    rusqlite::params![json, track_id],
                ).unwrap_or(0);
                updated += changed as i64;
            }
        }
    }

    // Update artist info if we have MusicBrainz data
    if let Some(ref mb_artist_id) = enrichment.artist_musicbrainz_id {
        // Get the track's artist_id
        let artist_id: Option<i64> = conn.query_row(
            "SELECT artist_id FROM tracks WHERE id = ?1",
            rusqlite::params![track_id],
            |row| row.get(0),
        ).ok();
        if let Some(aid) = artist_id {
            let _ = conn.execute(
                "UPDATE artists SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL",
                rusqlite::params![mb_artist_id, aid],
            );
            if let Some(ref sn) = enrichment.artist_sort_name {
                let _ = conn.execute(
                    "UPDATE artists SET sort_name = ?1 WHERE id = ?2 AND sort_name IS NULL",
                    rusqlite::params![sn, aid],
                );
            }
            if let Some(ref at) = enrichment.artist_type {
                let _ = conn.execute(
                    "UPDATE artists SET artist_type = ?1 WHERE id = ?2 AND artist_type IS NULL",
                    rusqlite::params![at, aid],
                );
            }
            if let Some(ref c) = enrichment.artist_country {
                let _ = conn.execute(
                    "UPDATE artists SET country = ?1 WHERE id = ?2 AND country IS NULL",
                    rusqlite::params![c, aid],
                );
            }
            if let Some(by) = enrichment.artist_begin_year {
                let _ = conn.execute(
                    "UPDATE artists SET begin_year = ?1 WHERE id = ?2 AND begin_year IS NULL",
                    rusqlite::params![by, aid],
                );
            }
            if let Some(ref url) = enrichment.artist_website_url {
                let _ = conn.execute(
                    "UPDATE artists SET website_url = ?1 WHERE id = ?2 AND website_url IS NULL",
                    rusqlite::params![url, aid],
                );
            }
        }
    }

    // Update album info
    if let Some(ref mb_album_id) = enrichment.album_musicbrainz_id {
        let album_id: Option<i64> = conn.query_row(
            "SELECT album_id FROM tracks WHERE id = ?1",
            rusqlite::params![track_id],
            |row| row.get(0),
        ).ok().flatten();
        if let Some(aid) = album_id {
            let _ = conn.execute(
                "UPDATE albums SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL",
                rusqlite::params![mb_album_id, aid],
            );
            if let Some(ref rd) = enrichment.album_release_date {
                let _ = conn.execute(
                    "UPDATE albums SET release_date = ?1 WHERE id = ?2 AND release_date IS NULL",
                    rusqlite::params![rd, aid],
                );
            }
            if let Some(ref at) = enrichment.album_type {
                let _ = conn.execute(
                    "UPDATE albums SET album_type = ?1 WHERE id = ?2 AND album_type IS NULL",
                    rusqlite::params![at, aid],
                );
            }
            if let Some(ref url) = enrichment.album_purchase_url {
                let _ = conn.execute(
                    "UPDATE albums SET purchase_url = ?1 WHERE id = ?2 AND purchase_url IS NULL",
                    rusqlite::params![url, aid],
                );
            }
        }
    }

    // Release DB lock before async lyrics/MV fetches
    let _ = crate::db::tracks::update_fts(&conn, track_id);
    } // conn dropped here

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
        let mut mv_url = enrichment.music_video_url.clone();

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
                .map_or(false, |v| v == "true");
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

    let conn = db.lock().map_err(|e| e.to_string())?;
    let completeness = crate::db::tracks::update_completeness(&conn, track_id)
        .map_err(|e| e.to_string())?;

    Ok(EnrichResult { track_id, fields_updated: updated, completeness })
}

/// Enrich an album's metadata from MusicBrainz + Last.fm, including tracklist and cover art
#[derive(Debug, serde::Serialize)]
pub struct EnrichAlbumResult {
    pub album_id: i64,
    pub fields_updated: i64,
    pub tracks_added: i64,
    pub tracklist: Vec<crate::metadata::musicbrainz::AlbumTrackInfo>,
}

#[tauri::command]
pub async fn enrich_album(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    album_id: i64,
) -> Result<EnrichAlbumResult, String> {
    // Get album info for search
    let (title, artist_name, existing_cover, existing_description, existing_genre) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let album = crate::db::albums::get_album(&conn, album_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Album not found".to_string())?;
        (album.title, album.artist_name, album.cover_art_path, album.description, album.genre)
    };

    // Fetch MusicBrainz data
    let enrichment = crate::metadata::musicbrainz::enrich_album(&title, artist_name.as_deref()).await?;

    // Fetch Last.fm data in parallel (don't fail if it errors)
    let lastfm_data = if let Some(ref artist) = artist_name {
        crate::metadata::lastfm::get_album_info(&title, artist).await.ok()
    } else {
        None
    };

    // Fetch artist data from Last.fm for bio/image
    let lastfm_artist = if let Some(ref artist) = artist_name {
        crate::metadata::lastfm::get_artist_info(artist).await.ok()
    } else {
        None
    };

    // Apply all DB updates in a block so conn is dropped before async cover art download
    let (mut updated, artist_id) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let mut updated = 0i64;

        macro_rules! update_album_if_missing {
            ($col:expr, $val:expr) => {
                if let Some(ref v) = $val {
                    let changed = conn.execute(
                        &format!("UPDATE albums SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                        rusqlite::params![v, album_id],
                    ).unwrap_or(0);
                    updated += changed as i64;
                }
            };
        }

        update_album_if_missing!("musicbrainz_id", enrichment.musicbrainz_id);
        update_album_if_missing!("release_date", enrichment.release_date);
        update_album_if_missing!("label", enrichment.label);
        update_album_if_missing!("album_type", enrichment.album_type);

        // Genre: prefer Last.fm tags (joined), fallback to MusicBrainz
        if existing_genre.is_none() {
            let genre = lastfm_data.as_ref()
                .filter(|d| !d.tags.is_empty())
                .map(|d| d.tags.join(", "))
                .or(enrichment.genre.clone());
            update_album_if_missing!("genre", genre);
        }

        // Description: prefer Last.fm wiki
        if existing_description.is_none() {
            let desc = lastfm_data.as_ref()
                .and_then(|d| d.description.clone());
            update_album_if_missing!("description", desc);
        }

        if let Some(tt) = enrichment.total_tracks {
            let changed = conn.execute(
                "UPDATE albums SET total_tracks = ?1 WHERE id = ?2 AND total_tracks IS NULL",
                rusqlite::params![tt, album_id],
            ).unwrap_or(0);
            updated += changed as i64;
        }
        if let Some(td) = enrichment.total_discs {
            let changed = conn.execute(
                "UPDATE albums SET total_discs = ?1 WHERE id = ?2 AND total_discs IS NULL",
                rusqlite::params![td, album_id],
            ).unwrap_or(0);
            updated += changed as i64;
        }

        // Update artist info
        let artist_id: Option<i64> = conn.query_row(
            "SELECT artist_id FROM albums WHERE id = ?1",
            rusqlite::params![album_id],
            |row| row.get(0),
        ).ok().flatten();

        if let Some(aid) = artist_id {
            if let Some(ref mb_artist_id) = enrichment.artist_musicbrainz_id {
                let _ = conn.execute("UPDATE artists SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL", rusqlite::params![mb_artist_id, aid]);
                if let Some(ref v) = enrichment.artist_sort_name { let _ = conn.execute("UPDATE artists SET sort_name = ?1 WHERE id = ?2 AND sort_name IS NULL", rusqlite::params![v, aid]); }
                if let Some(ref v) = enrichment.artist_type { let _ = conn.execute("UPDATE artists SET artist_type = ?1 WHERE id = ?2 AND artist_type IS NULL", rusqlite::params![v, aid]); }
                if let Some(ref v) = enrichment.artist_country { let _ = conn.execute("UPDATE artists SET country = ?1 WHERE id = ?2 AND country IS NULL", rusqlite::params![v, aid]); }
                if let Some(v) = enrichment.artist_begin_year { let _ = conn.execute("UPDATE artists SET begin_year = ?1 WHERE id = ?2 AND begin_year IS NULL", rusqlite::params![v, aid]); }
            }
            // Artist bio from Last.fm
            if let Some(ref lfm_artist) = lastfm_artist {
                if let Some(ref bio) = lfm_artist.bio {
                    let _ = conn.execute("UPDATE artists SET bio = ?1 WHERE id = ?2 AND (bio IS NULL OR bio = '')", rusqlite::params![bio, aid]);
                }
            }
        }

        (updated, artist_id)
    }; // conn dropped here

    // Download cover art if album has no cover
    if existing_cover.is_none() {
        let covers_dir = app_handle
            .path()
            .app_data_dir()
            .map(|d| d.join("covers"))
            .ok();

        if let Some(covers_dir) = covers_dir {
            let _ = std::fs::create_dir_all(&covers_dir);
            let mut cover_bytes: Option<Vec<u8>> = None;

            // Try Cover Art Archive first (highest quality)
            if let Some(ref mbid) = enrichment.musicbrainz_id {
                cover_bytes = crate::metadata::musicbrainz::download_cover_art(mbid).await;
            }

            // Fallback to Last.fm image
            if cover_bytes.is_none() {
                if let Some(ref url) = lastfm_data.as_ref().and_then(|d| d.image_url.clone()) {
                    cover_bytes = crate::metadata::lastfm::download_image(url).await;
                }
            }

            if let Some(bytes) = cover_bytes {
                let filename = format!("album_{}.jpg", album_id);
                let path = covers_dir.join(&filename);
                if std::fs::write(&path, &bytes).is_ok() {
                    let path_str = path.to_string_lossy().to_string();
                    if let Ok(conn) = db.lock() {
                        let _ = conn.execute(
                            "UPDATE albums SET cover_art_path = ?1 WHERE id = ?2",
                            rusqlite::params![path_str, album_id],
                        );
                        // Also update tracks that belong to this album and have no cover
                        let _ = conn.execute(
                            "UPDATE tracks SET cover_art_path = ?1 WHERE album_id = ?2 AND cover_art_path IS NULL",
                            rusqlite::params![path_str, album_id],
                        );
                        updated += 1;
                    }
                }
            }
        }

        // Download artist image if missing
        if let Some(aid) = artist_id {
            let artist_has_image: bool = db.lock().ok()
                .and_then(|conn| conn.query_row(
                    "SELECT image_path IS NOT NULL FROM artists WHERE id = ?1",
                    rusqlite::params![aid],
                    |row| row.get::<_, bool>(0),
                ).ok())
                .unwrap_or(true);

            if !artist_has_image {
                let mut artist_img_bytes: Option<Vec<u8>> = None;
                if let Some(ref lfm_artist) = lastfm_artist {
                    if let Some(ref url) = lfm_artist.image_url {
                        artist_img_bytes = crate::metadata::lastfm::download_image(url).await;
                    }
                }
                if let Some(bytes) = artist_img_bytes {
                    if let Some(covers_dir) = app_handle.path().app_data_dir().map(|d| d.join("covers")).ok() {
                        let filename = format!("artist_{}.jpg", aid);
                        let path = covers_dir.join(&filename);
                        if std::fs::write(&path, &bytes).is_ok() {
                            let path_str = path.to_string_lossy().to_string();
                            if let Ok(conn) = db.lock() {
                                let _ = conn.execute(
                                    "UPDATE artists SET image_path = ?1 WHERE id = ?2 AND image_path IS NULL",
                                    rusqlite::params![path_str, aid],
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let tracklist = enrichment.tracklist.clone();
    let tracks_added = tracklist.len() as i64;

    // Persist the enriched tracklist as JSON so it survives page reloads
    if !tracklist.is_empty() {
        if let Ok(json) = serde_json::to_string(&tracklist) {
            if let Ok(conn) = db.lock() {
                let _ = conn.execute(
                    "UPDATE albums SET enriched_tracklist = ?1 WHERE id = ?2",
                    rusqlite::params![json, album_id],
                );
            }
        }
    }

    Ok(EnrichAlbumResult { album_id, fields_updated: updated, tracks_added, tracklist })
}

/// Scan all tracks with low metadata completeness and enrich them
#[tauri::command]
pub async fn scan_missing_metadata(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
) -> Result<ScanMissingResult, String> {
    // Get tracks that need enrichment (completeness < 70 OR completeness = 0)
    // Note: We don't recompute completeness for tracks at 0% before enriching them,
    // because that would prevent re-enrichment after metadata deletion.
    // Tracks at 0% are explicitly marked for enrichment (e.g., after deletion).
    let tracks_to_enrich: Vec<(i64, String, Option<String>)> = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        let mut stmt = conn.prepare(
            "SELECT t.id, t.title, a.name
             FROM tracks t
             LEFT JOIN artists a ON t.artist_id = a.id
             WHERE t.metadata_completeness < 70
             ORDER BY t.metadata_completeness ASC"
        ).map_err(|e| e.to_string())?;
        let result: Vec<_> = stmt.query_map([], |row| Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
        )))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
        result
    };

    let total = tracks_to_enrich.len() as i64;
    let mut enriched = 0i64;
    let mut failed = 0i64;

    // Reset cancellation flag at start of scan
    METADATA_SCAN_CANCELLED.store(false, Ordering::Relaxed);

    for (i, (track_id, title, artist_name)) in tracks_to_enrich.iter().enumerate() {
        // Check cancellation
        if METADATA_SCAN_CANCELLED.load(Ordering::Relaxed) {
            log::info!("Metadata scan cancelled by user at {}/{}", i, total);
            break;
        }

        // Rate limit: 1 request per second for MusicBrainz
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        }

        // Emit progress event
        let _ = app_handle.emit(
            "metadata-scan-progress",
            serde_json::json!({
                "current": i + 1,
                "total": total,
                "track_title": title,
            }),
        );

        // Check cancellation again after rate-limit sleep
        if METADATA_SCAN_CANCELLED.load(Ordering::Relaxed) {
            log::info!("Metadata scan cancelled by user at {}/{}", i, total);
            break;
        }

        let artist_for_lastfm = artist_name.as_deref().unwrap_or("unknown");
        let mb_result = crate::metadata::musicbrainz::enrich_track(title, artist_name.as_deref()).await;

        // Check cancellation after API call
        if METADATA_SCAN_CANCELLED.load(Ordering::Relaxed) {
            log::info!("Metadata scan cancelled by user at {}/{}", i, total);
            break;
        }

        match mb_result {
            Ok(enrichment) => {
                // Apply MusicBrainz enrichment (lock scope limited to avoid holding across await)
                if let Ok(conn) = db.lock() {
                    macro_rules! update_if_missing {
                        ($col:expr, $val:expr) => {
                            if let Some(ref v) = $val {
                                let _ = conn.execute(
                                    &format!("UPDATE tracks SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                                    rusqlite::params![v, track_id],
                                );
                            }
                        };
                    }
                    update_if_missing!("musicbrainz_id", enrichment.musicbrainz_id);
                    update_if_missing!("genre", enrichment.genre);
                    update_if_missing!("release_date", enrichment.release_date);
                    update_if_missing!("isrc", enrichment.isrc);
                    update_if_missing!("description", enrichment.description);
                    update_if_missing!("label", enrichment.label);
                    update_if_missing!("language", enrichment.language);

                    // Merge MusicBrainz tags
                    if let Some(ref new_tags) = enrichment.tags {
                        let existing_json: Option<String> = conn.query_row(
                            "SELECT tags FROM tracks WHERE id = ?1",
                            rusqlite::params![track_id],
                            |row| row.get(0),
                        ).ok().flatten();
                        let mut all_tags: Vec<String> = existing_json
                            .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
                            .unwrap_or_default();
                        for tag in new_tags {
                            let lower = tag.to_lowercase();
                            if !all_tags.iter().any(|t| t.to_lowercase() == lower) {
                                all_tags.push(tag.clone());
                            }
                        }
                        if let Ok(json) = serde_json::to_string(&all_tags) {
                            let _ = conn.execute("UPDATE tracks SET tags = ?1 WHERE id = ?2", rusqlite::params![json, track_id]);
                        }
                    }

                    // Update artist
                    if let Some(ref mb_id) = enrichment.artist_musicbrainz_id {
                        let artist_id: Option<i64> = conn.query_row(
                            "SELECT artist_id FROM tracks WHERE id = ?1",
                            rusqlite::params![track_id],
                            |row| row.get(0),
                        ).ok();
                        if let Some(aid) = artist_id {
                            let _ = conn.execute("UPDATE artists SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL", rusqlite::params![mb_id, aid]);
                            if let Some(ref v) = enrichment.artist_sort_name { let _ = conn.execute("UPDATE artists SET sort_name = ?1 WHERE id = ?2 AND sort_name IS NULL", rusqlite::params![v, aid]); }
                            if let Some(ref v) = enrichment.artist_type { let _ = conn.execute("UPDATE artists SET artist_type = ?1 WHERE id = ?2 AND artist_type IS NULL", rusqlite::params![v, aid]); }
                            if let Some(ref v) = enrichment.artist_country { let _ = conn.execute("UPDATE artists SET country = ?1 WHERE id = ?2 AND country IS NULL", rusqlite::params![v, aid]); }
                            if let Some(v) = enrichment.artist_begin_year { let _ = conn.execute("UPDATE artists SET begin_year = ?1 WHERE id = ?2 AND begin_year IS NULL", rusqlite::params![v, aid]); }
                        }
                    }
                } // conn dropped here before await

                // Also try Last.fm for supplementary genre/description if MusicBrainz didn't provide them
                if !METADATA_SCAN_CANCELLED.load(Ordering::Relaxed) {
                    if let Ok(lastfm_data) = crate::metadata::lastfm::get_track_info(title, artist_for_lastfm).await {
                        if !lastfm_data.tags.is_empty() || lastfm_data.description.is_some() {
                            if let Ok(conn) = db.lock() {
                                macro_rules! update_if_missing {
                                    ($col:expr, $val:expr) => {
                                        if let Some(ref v) = $val {
                                            let _ = conn.execute(
                                                &format!("UPDATE tracks SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                                                rusqlite::params![v, track_id],
                                            );
                                        }
                                    };
                                }
                                if !lastfm_data.tags.is_empty() {
                                    let genre_val: Option<String> = Some(lastfm_data.tags.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
                                    update_if_missing!("genre", genre_val);
                                }
                                if lastfm_data.description.is_some() {
                                    update_if_missing!("description", lastfm_data.description);
                                }
                            }
                        }
                    }
                }

                if let Ok(conn) = db.lock() {
                    let _ = crate::db::tracks::update_completeness(&conn, *track_id);
                }

                // Collect which fields were found from MusicBrainz
                let mut mb_fields = Vec::new();
                if enrichment.musicbrainz_id.is_some() { mb_fields.push("musicbrainz_id"); }
                if enrichment.genre.is_some() { mb_fields.push("genre"); }
                if enrichment.release_date.is_some() { mb_fields.push("release_date"); }
                if enrichment.isrc.is_some() { mb_fields.push("isrc"); }
                if enrichment.description.is_some() { mb_fields.push("description"); }
                if enrichment.label.is_some() { mb_fields.push("label"); }
                if enrichment.language.is_some() { mb_fields.push("language"); }
                if enrichment.tags.is_some() { mb_fields.push("tags"); }
                if enrichment.music_video_url.is_some() { mb_fields.push("music_video_url"); }
                if enrichment.artist_website_url.is_some() { mb_fields.push("artist_website"); }
                if enrichment.album_purchase_url.is_some() { mb_fields.push("album_purchase_url"); }

                let _ = app_handle.emit("metadata-enrich-detail", serde_json::json!({
                    "item_type": "track",
                    "id": track_id,
                    "title": title,
                    "artist": artist_name,
                    "status": "success",
                    "sources": {
                        "musicbrainz": mb_fields,
                    },
                }));

                enriched += 1;
            }
            Err(e) => {
                log::warn!("MusicBrainz failed for track {} ({}): {}", track_id, title, e);

                // Check cancellation before Last.fm fallback
                if METADATA_SCAN_CANCELLED.load(Ordering::Relaxed) {
                    log::info!("Metadata scan cancelled by user at {}/{}", i, total);
                    break;
                }

                // Fallback: try Last.fm
                match crate::metadata::lastfm::get_track_info(title, artist_for_lastfm).await {
                    Ok(lastfm_data) => {
                        let has_genre = !lastfm_data.tags.is_empty();
                        let has_desc = lastfm_data.description.is_some();

                        if has_genre || has_desc {
                            if let Ok(conn) = db.lock() {
                                macro_rules! update_if_missing {
                                    ($col:expr, $val:expr) => {
                                        if let Some(ref v) = $val {
                                            let _ = conn.execute(
                                                &format!("UPDATE tracks SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                                                rusqlite::params![v, track_id],
                                            );
                                        }
                                    };
                                }

                                if has_genre {
                                    let genre_val: Option<String> = Some(lastfm_data.tags.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
                                    update_if_missing!("genre", genre_val);
                                }
                                if has_desc {
                                    update_if_missing!("description", lastfm_data.description);
                                }

                                let _ = crate::db::tracks::update_completeness(&conn, *track_id);
                            }

                            let mut lfm_fields = Vec::new();
                            if has_genre { lfm_fields.push("genre"); }
                            if has_desc { lfm_fields.push("description"); }

                            let _ = app_handle.emit("metadata-enrich-detail", serde_json::json!({
                                "item_type": "track",
                                "id": track_id,
                                "title": title,
                                "artist": artist_name,
                                "status": "partial",
                                "sources": {
                                    "lastfm": lfm_fields,
                                },
                                "note": "MusicBrainz failed, used Last.fm fallback",
                            }));

                            enriched += 1;
                        } else {
                            let _ = app_handle.emit("metadata-enrich-detail", serde_json::json!({
                                "item_type": "track",
                                "id": track_id,
                                "title": title,
                                "artist": artist_name,
                                "status": "failed",
                                "error": format!("{}", e),
                            }));
                            failed += 1;
                        }
                    }
                    Err(lastfm_err) => {
                        log::warn!("Last.fm fallback also failed for track {} ({}): {}", track_id, title, lastfm_err);
                        let _ = app_handle.emit("metadata-enrich-detail", serde_json::json!({
                            "item_type": "track",
                            "id": track_id,
                            "title": title,
                            "artist": artist_name,
                            "status": "failed",
                            "error": format!("MusicBrainz: {} | Last.fm: {}", e, lastfm_err),
                        }));
                        failed += 1;
                    }
                }
            }
        }
    }

    // Compute average completeness
    let completeness_avg = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT COALESCE(AVG(metadata_completeness), 0) FROM tracks",
            [],
            |row| row.get::<_, i64>(0),
        ).unwrap_or(0)
    };

    // Emit completion event
    let _ = app_handle.emit("metadata-scan-complete", serde_json::json!({
        "enriched": enriched,
        "failed": failed,
        "completeness_avg": completeness_avg,
    }));

    Ok(ScanMissingResult { total_tracks: total, enriched, failed, completeness_avg })
}

/// Background auto-enrichment: enriches all albums and tracks with missing metadata.
/// Called once on app startup as a background task.
pub async fn auto_enrich_library(
    db: Arc<std::sync::Mutex<rusqlite::Connection>>,
    app_handle: tauri::AppHandle,
) {
    log::info!("Starting background auto-enrichment...");

    let covers_dir = match app_handle.path().app_data_dir().map(|d| d.join("covers")) {
        Ok(d) => { let _ = std::fs::create_dir_all(&d); d },
        Err(_) => return,
    };

    // 1. Enrich albums that have no musicbrainz_id
    let albums_to_enrich: Vec<(i64, String, Option<String>, Option<String>)> = {
        match db.lock() {
            Ok(conn) => {
                let mut stmt = match conn.prepare(
                    "SELECT al.id, al.title, a.name, al.cover_art_path
                     FROM albums al
                     LEFT JOIN artists a ON al.artist_id = a.id
                     WHERE al.musicbrainz_id IS NULL
                     ORDER BY (CASE WHEN al.cover_art_path IS NULL THEN 0 ELSE 1 END)
                            + (CASE WHEN al.year IS NULL THEN 0 ELSE 1 END)
                            + (CASE WHEN al.genre IS NULL THEN 0 ELSE 1 END)
                            + (CASE WHEN al.label IS NULL THEN 0 ELSE 1 END)
                            + (CASE WHEN al.description IS NULL THEN 0 ELSE 1 END)
                     ASC
                     LIMIT 100"
                ) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let result: Vec<_> = match stmt.query_map([], |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, Option<String>>(3)?,
                ))) {
                    Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                    Err(e) => {
                        log::error!("Failed to query albums for enrichment: {}", e);
                        return;
                    }
                };
                result
            }
            Err(_) => return,
        }
    };

    let total_albums = albums_to_enrich.len();
    log::info!("Auto-enriching {} albums", total_albums);

    let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
        "phase": "albums",
        "current": 0,
        "total": total_albums,
    }));

    for (i, (album_id, title, artist_name, existing_cover)) in albums_to_enrich.iter().enumerate() {
        // Check cancellation
        if METADATA_SCAN_CANCELLED.load(Ordering::Relaxed) {
            log::info!("Auto-enrichment cancelled by user");
            return;
        }

        // Rate limit for MusicBrainz
        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        }

        let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
            "phase": "albums",
            "current": i + 1,
            "total": total_albums,
            "title": title,
        }));

        // MusicBrainz album enrichment
        let enrichment = match crate::metadata::musicbrainz::enrich_album(&title, artist_name.as_deref()).await {
            Ok(e) => e,
            Err(e) => {
                log::warn!("Failed to enrich album '{}': {}", title, e);
                let _ = app_handle.emit("metadata-enrich-detail", serde_json::json!({
                    "item_type": "album",
                    "id": album_id,
                    "title": title,
                    "artist": artist_name,
                    "status": "failed",
                    "error": format!("{}", e),
                }));
                continue;
            }
        };

        // Last.fm album data
        let lastfm_data = if let Some(ref artist) = artist_name {
            crate::metadata::lastfm::get_album_info(&title, artist).await.ok()
        } else {
            None
        };

        if let Ok(conn) = db.lock() {
            macro_rules! update_album {
                ($col:expr, $val:expr) => {
                    if let Some(ref v) = $val {
                        let _ = conn.execute(
                            &format!("UPDATE albums SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                            rusqlite::params![v, album_id],
                        );
                    }
                };
            }

            update_album!("musicbrainz_id", enrichment.musicbrainz_id);
            update_album!("release_date", enrichment.release_date);
            update_album!("label", enrichment.label);
            update_album!("album_type", enrichment.album_type);

            // Genre: prefer Last.fm tags
            let genre = lastfm_data.as_ref()
                .filter(|d| !d.tags.is_empty())
                .map(|d| d.tags.join(", "))
                .or(enrichment.genre.clone());
            update_album!("genre", genre);

            // Description from Last.fm
            let desc = lastfm_data.as_ref().and_then(|d| d.description.clone());
            update_album!("description", desc);

            if let Some(tt) = enrichment.total_tracks {
                let _ = conn.execute("UPDATE albums SET total_tracks = ?1 WHERE id = ?2 AND total_tracks IS NULL", rusqlite::params![tt, album_id]);
            }
            if let Some(td) = enrichment.total_discs {
                let _ = conn.execute("UPDATE albums SET total_discs = ?1 WHERE id = ?2 AND total_discs IS NULL", rusqlite::params![td, album_id]);
            }

            // Save enriched tracklist
            if !enrichment.tracklist.is_empty() {
                if let Ok(json) = serde_json::to_string(&enrichment.tracklist) {
                    let _ = conn.execute(
                        "UPDATE albums SET enriched_tracklist = ?1 WHERE id = ?2 AND enriched_tracklist IS NULL",
                        rusqlite::params![json, album_id],
                    );
                }
            }

            // Artist enrichment
            let artist_id: Option<i64> = conn.query_row(
                "SELECT artist_id FROM albums WHERE id = ?1", rusqlite::params![album_id], |row| row.get(0),
            ).ok().flatten();
            if let Some(aid) = artist_id {
                if let Some(ref mb_id) = enrichment.artist_musicbrainz_id {
                    let _ = conn.execute("UPDATE artists SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL", rusqlite::params![mb_id, aid]);
                }
                if let Some(ref v) = enrichment.artist_sort_name { let _ = conn.execute("UPDATE artists SET sort_name = ?1 WHERE id = ?2 AND sort_name IS NULL", rusqlite::params![v, aid]); }
                if let Some(ref v) = enrichment.artist_type { let _ = conn.execute("UPDATE artists SET artist_type = ?1 WHERE id = ?2 AND artist_type IS NULL", rusqlite::params![v, aid]); }
                if let Some(ref v) = enrichment.artist_country { let _ = conn.execute("UPDATE artists SET country = ?1 WHERE id = ?2 AND country IS NULL", rusqlite::params![v, aid]); }
                if let Some(v) = enrichment.artist_begin_year { let _ = conn.execute("UPDATE artists SET begin_year = ?1 WHERE id = ?2 AND begin_year IS NULL", rusqlite::params![v, aid]); }
            }
        }

        // Download cover art if missing
        if existing_cover.is_none() {
            let mut cover_bytes: Option<Vec<u8>> = None;
            if let Some(ref mbid) = enrichment.musicbrainz_id {
                cover_bytes = crate::metadata::musicbrainz::download_cover_art(mbid).await;
            }
            if cover_bytes.is_none() {
                if let Some(ref url) = lastfm_data.as_ref().and_then(|d| d.image_url.clone()) {
                    cover_bytes = crate::metadata::lastfm::download_image(url).await;
                }
            }
            if let Some(bytes) = cover_bytes {
                let filename = format!("album_{}.jpg", album_id);
                let path = covers_dir.join(&filename);
                if std::fs::write(&path, &bytes).is_ok() {
                    let path_str = path.to_string_lossy().to_string();
                    if let Ok(conn) = db.lock() {
                        let _ = conn.execute("UPDATE albums SET cover_art_path = ?1 WHERE id = ?2", rusqlite::params![path_str, album_id]);
                        let _ = conn.execute("UPDATE tracks SET cover_art_path = ?1 WHERE album_id = ?2 AND cover_art_path IS NULL", rusqlite::params![path_str, album_id]);
                    }
                }
            }
        }

        // Download artist image if missing
        let artist_needs_image: Option<i64> = db.lock().ok().and_then(|conn| {
            let aid: Option<i64> = conn.query_row(
                "SELECT artist_id FROM albums WHERE id = ?1", rusqlite::params![album_id], |row| row.get(0),
            ).ok().flatten();
            aid.filter(|&aid| {
                !conn.query_row(
                    "SELECT image_path IS NOT NULL FROM artists WHERE id = ?1",
                    rusqlite::params![aid], |row| row.get::<_, bool>(0),
                ).unwrap_or(true)
            })
        });

        if let Some(aid) = artist_needs_image {
            if let Some(ref artist) = artist_name {
                if let Ok(lfm) = crate::metadata::lastfm::get_artist_info(artist).await {
                    if let Some(ref url) = lfm.image_url {
                        if let Some(bytes) = crate::metadata::lastfm::download_image(url).await {
                            let filename = format!("artist_{}.jpg", aid);
                            let path = covers_dir.join(&filename);
                            if std::fs::write(&path, &bytes).is_ok() {
                                let path_str = path.to_string_lossy().to_string();
                                if let Ok(conn) = db.lock() {
                                    let _ = conn.execute("UPDATE artists SET image_path = ?1 WHERE id = ?2 AND image_path IS NULL", rusqlite::params![path_str, aid]);
                                    if let Some(ref bio) = lfm.bio {
                                        let _ = conn.execute("UPDATE artists SET bio = ?1 WHERE id = ?2 AND (bio IS NULL OR bio = '')", rusqlite::params![bio, aid]);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        // Emit detail event for album enrichment
        let mut mb_fields = Vec::new();
        if enrichment.musicbrainz_id.is_some() { mb_fields.push("musicbrainz_id"); }
        if enrichment.release_date.is_some() { mb_fields.push("release_date"); }
        if enrichment.label.is_some() { mb_fields.push("label"); }
        if enrichment.album_type.is_some() { mb_fields.push("album_type"); }
        if enrichment.genre.is_some() { mb_fields.push("genre"); }
        if !enrichment.tracklist.is_empty() { mb_fields.push("tracklist"); }
        if enrichment.total_tracks.is_some() { mb_fields.push("total_tracks"); }

        let mut lfm_fields = Vec::new();
        if let Some(ref d) = lastfm_data {
            if !d.tags.is_empty() { lfm_fields.push("genre"); }
            if d.description.is_some() { lfm_fields.push("description"); }
            if d.image_url.is_some() { lfm_fields.push("cover_art"); }
        }

        let _ = app_handle.emit("metadata-enrich-detail", serde_json::json!({
            "item_type": "album",
            "id": album_id,
            "title": title,
            "artist": artist_name,
            "status": "success",
            "sources": {
                "musicbrainz": mb_fields,
                "lastfm": lfm_fields,
            },
        }));
    }

    // 2. Enrich tracks with low completeness
    let tracks_to_enrich: Vec<(i64, String, Option<String>)> = {
        match db.lock() {
            Ok(conn) => {
                // First recompute zero-completeness tracks
                if let Ok(mut stmt) = conn.prepare("SELECT id FROM tracks WHERE metadata_completeness = 0") {
                    let ids: Vec<i64> = match stmt.query_map([], |row| row.get(0)) {
                        Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                        Err(e) => {
                            log::error!("Failed to query zero-completeness tracks: {}", e);
                            Vec::new()
                        }
                    };
                    drop(stmt);
                    for id in &ids {
                        let _ = crate::db::tracks::update_completeness(&conn, *id);
                    }
                }

                let mut stmt = match conn.prepare(
                    "SELECT t.id, t.title, a.name
                     FROM tracks t
                     LEFT JOIN artists a ON t.artist_id = a.id
                     WHERE t.metadata_completeness < 70
                     ORDER BY t.metadata_completeness ASC
                     LIMIT 100"
                ) {
                    Ok(s) => s,
                    Err(_) => return,
                };
                let result: Vec<_> = match stmt.query_map([], |row| Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))) {
                    Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
                    Err(e) => {
                        log::error!("Failed to query tracks for enrichment: {}", e);
                        return;
                    }
                };
                result
            }
            Err(_) => return,
        }
    };

    let total_tracks = tracks_to_enrich.len();
    log::info!("Auto-enriching {} tracks", total_tracks);

    for (i, (track_id, title, artist_name)) in tracks_to_enrich.iter().enumerate() {
        // Check cancellation
        if METADATA_SCAN_CANCELLED.load(Ordering::Relaxed) {
            log::info!("Auto-enrichment cancelled by user during tracks phase");
            return;
        }

        if i > 0 {
            tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        }

        let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
            "phase": "tracks",
            "current": i + 1,
            "total": total_tracks,
            "title": title,
        }));

        // MusicBrainz track enrichment
        match crate::metadata::musicbrainz::enrich_track(&title, artist_name.as_deref()).await {
            Ok(enrichment) => {
                // Apply MusicBrainz data
                if let Ok(conn) = db.lock() {
                    macro_rules! update_track {
                        ($col:expr, $val:expr) => {
                            if let Some(ref v) = $val {
                                let _ = conn.execute(
                                    &format!("UPDATE tracks SET {} = ?1 WHERE id = ?2 AND ({} IS NULL OR {} = '')", $col, $col, $col),
                                    rusqlite::params![v, track_id],
                                );
                            }
                        };
                    }
                    update_track!("musicbrainz_id", enrichment.musicbrainz_id);
                    update_track!("genre", enrichment.genre);
                    update_track!("release_date", enrichment.release_date);
                    update_track!("isrc", enrichment.isrc);
                    update_track!("description", enrichment.description);
                    update_track!("label", enrichment.label);
                    update_track!("language", enrichment.language);

                    // Merge tags
                    if let Some(ref new_tags) = enrichment.tags {
                        let existing_json: Option<String> = conn.query_row(
                            "SELECT tags FROM tracks WHERE id = ?1",
                            rusqlite::params![track_id],
                            |row| row.get(0),
                        ).ok().flatten();
                        let mut all_tags: Vec<String> = existing_json
                            .and_then(|j| serde_json::from_str::<Vec<String>>(&j).ok())
                            .unwrap_or_default();
                        for tag in new_tags {
                            let lower = tag.to_lowercase();
                            if !all_tags.iter().any(|t| t.to_lowercase() == lower) {
                                all_tags.push(tag.clone());
                            }
                        }
                        if let Ok(json) = serde_json::to_string(&all_tags) {
                            let _ = conn.execute("UPDATE tracks SET tags = ?1 WHERE id = ?2", rusqlite::params![json, track_id]);
                        }
                    }
                }
                // conn is dropped here

                // Also enrich with Last.fm track tags
                if let Some(ref artist) = artist_name {
                    if let Ok(lfm) = crate::metadata::lastfm::get_track_info(&title, artist).await {
                        if let Ok(conn) = db.lock() {
                            if !lfm.tags.is_empty() {
                                let tags_str = lfm.tags.join(", ");
                                let _ = conn.execute(
                                    "UPDATE tracks SET genre = ?1 WHERE id = ?2 AND (genre IS NULL OR genre = '')",
                                    rusqlite::params![tags_str, track_id],
                                );
                            }
                            if let Some(ref desc) = lfm.description {
                                let _ = conn.execute(
                                    "UPDATE tracks SET description = ?1 WHERE id = ?2 AND (description IS NULL OR description = '')",
                                    rusqlite::params![desc, track_id],
                                );
                            }
                        }
                    }
                }

                if let Ok(conn) = db.lock() {
                    let _ = crate::db::tracks::update_completeness(&conn, *track_id);
                }

                let mut mb_fields = Vec::new();
                if enrichment.musicbrainz_id.is_some() { mb_fields.push("musicbrainz_id"); }
                if enrichment.genre.is_some() { mb_fields.push("genre"); }
                if enrichment.release_date.is_some() { mb_fields.push("release_date"); }
                if enrichment.isrc.is_some() { mb_fields.push("isrc"); }
                if enrichment.description.is_some() { mb_fields.push("description"); }
                if enrichment.label.is_some() { mb_fields.push("label"); }
                if enrichment.language.is_some() { mb_fields.push("language"); }
                if enrichment.tags.is_some() { mb_fields.push("tags"); }

                let _ = app_handle.emit("metadata-enrich-detail", serde_json::json!({
                    "item_type": "track",
                    "id": track_id,
                    "title": title,
                    "artist": artist_name,
                    "status": "success",
                    "sources": {
                        "musicbrainz": mb_fields,
                    },
                }));
            }
            Err(e) => {
                log::warn!("Failed to enrich track '{}': {}", title, e);
                let _ = app_handle.emit("metadata-enrich-detail", serde_json::json!({
                    "item_type": "track",
                    "id": track_id,
                    "title": title,
                    "artist": artist_name,
                    "status": "failed",
                    "error": format!("{}", e),
                }));
            }
        }
    }

    let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
        "phase": "complete",
        "albums_enriched": total_albums,
        "tracks_enriched": total_tracks,
    }));

    log::info!("Background auto-enrichment complete: {} albums, {} tracks", total_albums, total_tracks);
}

/// Get metadata stats for the library
#[tauri::command]
pub fn get_metadata_stats(
    db: State<'_, Arc<DbPool>>,
) -> Result<serde_json::Value, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM tracks", [], |row| row.get(0)).unwrap_or(0);
    let avg: f64 = conn.query_row("SELECT COALESCE(AVG(metadata_completeness), 0) FROM tracks", [], |row| row.get(0)).unwrap_or(0.0);
    let complete: i64 = conn.query_row("SELECT COUNT(*) FROM tracks WHERE metadata_completeness >= 80", [], |row| row.get(0)).unwrap_or(0);
    let incomplete: i64 = conn.query_row("SELECT COUNT(*) FROM tracks WHERE metadata_completeness < 50", [], |row| row.get(0)).unwrap_or(0);

    Ok(serde_json::json!({
        "total_tracks": total,
        "average_completeness": avg.round() as i64,
        "complete_tracks": complete,
        "incomplete_tracks": incomplete,
    }))
}

#[tauri::command]
pub fn metadata_stop_scan() -> Result<(), String> {
    METADATA_SCAN_CANCELLED.store(true, Ordering::Relaxed);
    log::info!("Metadata scan stop requested");
    Ok(())
}

#[tauri::command]
pub fn metadata_delete_all(
    db: State<'_, Arc<DbPool>>,
) -> Result<(), String> {
    // Stop any running scan first
    METADATA_SCAN_CANCELLED.store(true, Ordering::Relaxed);

    let conn = db.lock().map_err(|e| e.to_string())?;
    conn.execute_batch("
        UPDATE tracks SET musicbrainz_id=NULL, genre=NULL, isrc=NULL, description=NULL,
            label=NULL, language=NULL, release_date=NULL, composer=NULL;
        UPDATE albums SET musicbrainz_id=NULL, label=NULL, release_date=NULL,
            description=NULL, album_type=NULL, enriched_tracklist=NULL,
            cover_art_path=NULL, genre=NULL, total_tracks=NULL, total_discs=NULL;
        UPDATE artists SET musicbrainz_id=NULL, bio=NULL, country=NULL,
            begin_year=NULL, artist_type=NULL, enriched_discography=NULL;
        DELETE FROM enrichments;
        DELETE FROM downloads WHERE status IN ('completed', 'failed', 'cancelled');
        UPDATE monitored_playlist_entries SET status='new', download_id=NULL, track_id=NULL, downloaded_at=NULL
            WHERE status IN ('downloaded', 'skipped');
    ").map_err(|e| e.to_string())?;

    // Recalculate metadata_completeness for all tracks to reflect actual state
    let mut stmt = conn.prepare("SELECT id FROM tracks").map_err(|e| e.to_string())?;
    let ids: Vec<i64> = stmt.query_map([], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    for id in ids {
        let _ = crate::db::tracks::update_completeness(&conn, id);
    }

    log::info!("All metadata and download history deleted");
    Ok(())
}

#[tauri::command]
pub fn metadata_cleanup_duplicates(
    db: State<'_, Arc<DbPool>>,
) -> Result<serde_json::Value, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

    // === PHASE 1: Merge duplicate albums ===
    // Group by LOWER(title) only — same album name with different artist_ids
    // (e.g. different featured artists) should be merged into one album.
    let dup_titles: Vec<String> = conn.prepare(
        "SELECT LOWER(title) FROM albums GROUP BY LOWER(title) HAVING COUNT(*) > 1"
    )
    .map_err(|e| e.to_string())?
    .query_map([], |row| row.get(0))
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let mut merged_album_groups = 0;
    let mut deleted_albums = 0;

    for lower_title in &dup_titles {
        // Get all album IDs with this title, ordered so the one with the most tracks comes first
        let album_ids: Vec<i64> = conn.prepare(
            "SELECT a.id FROM albums a
             LEFT JOIN (SELECT album_id, COUNT(*) as cnt FROM tracks WHERE album_id IS NOT NULL GROUP BY album_id) tc
               ON tc.album_id = a.id
             WHERE LOWER(a.title) = ?1
             ORDER BY COALESCE(tc.cnt, 0) DESC, a.id ASC"
        )
        .map_err(|e| e.to_string())?
        .query_map(params![lower_title], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

        if album_ids.len() <= 1 {
            continue;
        }

        let keep_id = album_ids[0];
        let delete_ids = &album_ids[1..];

        // Merge: move all tracks from duplicate albums to the kept album
        for &dup_id in delete_ids {
            conn.execute(
                "UPDATE tracks SET album_id = ?1 WHERE album_id = ?2",
                params![keep_id, dup_id],
            ).map_err(|e| e.to_string())?;
        }

        // If the kept album has no artist_id but a duplicate does, adopt it
        let keep_artist: Option<i64> = conn.query_row(
            "SELECT artist_id FROM albums WHERE id = ?1", params![keep_id], |row| row.get(0),
        ).map_err(|e| e.to_string())?;
        if keep_artist.is_none() {
            for &dup_id in delete_ids {
                let dup_artist: Option<i64> = conn.query_row(
                    "SELECT artist_id FROM albums WHERE id = ?1", params![dup_id], |row| row.get(0),
                ).map_err(|e| e.to_string())?;
                if dup_artist.is_some() {
                    conn.execute(
                        "UPDATE albums SET artist_id = ?2 WHERE id = ?1",
                        params![keep_id, dup_artist],
                    ).map_err(|e| e.to_string())?;
                    break;
                }
            }
        }

        // Fill in missing metadata fields from duplicates
        conn.execute(
            "UPDATE albums SET
                cover_art_path = COALESCE(cover_art_path, (SELECT cover_art_path FROM albums WHERE LOWER(title) = ?2 AND cover_art_path IS NOT NULL AND id != ?1 LIMIT 1)),
                year = COALESCE(year, (SELECT year FROM albums WHERE LOWER(title) = ?2 AND year IS NOT NULL AND id != ?1 LIMIT 1)),
                genre = COALESCE(genre, (SELECT genre FROM albums WHERE LOWER(title) = ?2 AND genre IS NOT NULL AND id != ?1 LIMIT 1)),
                musicbrainz_id = COALESCE(musicbrainz_id, (SELECT musicbrainz_id FROM albums WHERE LOWER(title) = ?2 AND musicbrainz_id IS NOT NULL AND id != ?1 LIMIT 1)),
                label = COALESCE(label, (SELECT label FROM albums WHERE LOWER(title) = ?2 AND label IS NOT NULL AND id != ?1 LIMIT 1)),
                release_date = COALESCE(release_date, (SELECT release_date FROM albums WHERE LOWER(title) = ?2 AND release_date IS NOT NULL AND id != ?1 LIMIT 1)),
                description = COALESCE(description, (SELECT description FROM albums WHERE LOWER(title) = ?2 AND description IS NOT NULL AND id != ?1 LIMIT 1))
             WHERE id = ?1",
            params![keep_id, lower_title],
        ).map_err(|e| e.to_string())?;

        // Delete the duplicate albums
        for &dup_id in delete_ids {
            conn.execute("DELETE FROM albums WHERE id = ?1", params![dup_id])
                .map_err(|e| e.to_string())?;
            deleted_albums += 1;
        }

        merged_album_groups += 1;
    }

    // Clean up orphaned albums (no tracks)
    let orphaned = conn.execute(
        "DELETE FROM albums WHERE id NOT IN (SELECT DISTINCT album_id FROM tracks WHERE album_id IS NOT NULL)",
        [],
    ).map_err(|e| e.to_string())?;

    // === PHASE 2: Deduplicate tracks ===
    // Find tracks with same title + same artist (case-insensitive) that are duplicates.
    // Keep the one with: highest metadata_completeness, then largest file_size, then lowest id.
    let dup_track_groups: Vec<(String, Option<i64>)> = conn.prepare(
        "SELECT LOWER(title), artist_id FROM tracks
         GROUP BY LOWER(title), artist_id
         HAVING COUNT(*) > 1"
    )
    .map_err(|e| e.to_string())?
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let mut merged_track_groups = 0;
    let mut deleted_tracks = 0;

    for (lower_title, artist_id) in &dup_track_groups {
        // Get all track IDs for this group, best quality first
        let track_ids: Vec<i64> = if let Some(aid) = artist_id {
            conn.prepare(
                "SELECT id FROM tracks
                 WHERE LOWER(title) = ?1 AND artist_id = ?2
                 ORDER BY COALESCE(metadata_completeness, 0) DESC, COALESCE(file_size, 0) DESC, id ASC"
            )
            .map_err(|e| e.to_string())?
            .query_map(params![lower_title, aid], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
        } else {
            conn.prepare(
                "SELECT id FROM tracks
                 WHERE LOWER(title) = ?1 AND artist_id IS NULL
                 ORDER BY COALESCE(metadata_completeness, 0) DESC, COALESCE(file_size, 0) DESC, id ASC"
            )
            .map_err(|e| e.to_string())?
            .query_map(params![lower_title], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
        };

        if track_ids.len() <= 1 {
            continue;
        }

        let keep_id = track_ids[0];
        let delete_ids = &track_ids[1..];

        // Update playlist_tracks to point to the kept track
        for &dup_id in delete_ids {
            // Only update if the kept track isn't already in the same playlist
            conn.execute(
                "UPDATE OR IGNORE playlist_tracks SET track_id = ?1 WHERE track_id = ?2",
                params![keep_id, dup_id],
            ).map_err(|e| e.to_string())?;
            // Remove any remaining references that conflicted
            conn.execute(
                "DELETE FROM playlist_tracks WHERE track_id = ?1",
                params![dup_id],
            ).map_err(|e| e.to_string())?;
        }

        // Delete duplicate tracks (and their files)
        for &dup_id in delete_ids {
            // Get file path before deleting so we can clean up the file
            let file_path: Option<String> = conn.query_row(
                "SELECT file_path FROM tracks WHERE id = ?1",
                params![dup_id],
                |row| row.get(0),
            ).ok();

            conn.execute("DELETE FROM tracks WHERE id = ?1", params![dup_id])
                .map_err(|e| e.to_string())?;

            // Remove the duplicate file from disk
            if let Some(path) = file_path {
                let _ = std::fs::remove_file(&path);
            }

            deleted_tracks += 1;
        }

        merged_track_groups += 1;
    }

    log::info!(
        "Cleanup: merged {} album groups (deleted {}), removed {} orphaned albums, merged {} track groups (deleted {} duplicate tracks)",
        merged_album_groups, deleted_albums, orphaned, merged_track_groups, deleted_tracks
    );

    Ok(serde_json::json!({
        "merged_album_groups": merged_album_groups,
        "deleted_duplicate_albums": deleted_albums,
        "orphaned_albums_removed": orphaned,
        "merged_track_groups": merged_track_groups,
        "deleted_duplicate_tracks": deleted_tracks
    }))
}

/// Deduplicate tracks only (same title + artist, keep best quality).
#[tauri::command]
pub fn metadata_cleanup_duplicate_tracks(
    db: State<'_, Arc<DbPool>>,
) -> Result<serde_json::Value, String> {
    let conn = db.lock().map_err(|e| e.to_string())?;

    let dup_track_groups: Vec<(String, Option<i64>)> = conn.prepare(
        "SELECT LOWER(title), artist_id FROM tracks
         GROUP BY LOWER(title), artist_id
         HAVING COUNT(*) > 1"
    )
    .map_err(|e| e.to_string())?
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let mut merged_track_groups = 0;
    let mut deleted_tracks = 0;

    for (lower_title, artist_id) in &dup_track_groups {
        let track_ids: Vec<i64> = if let Some(aid) = artist_id {
            conn.prepare(
                "SELECT id FROM tracks
                 WHERE LOWER(title) = ?1 AND artist_id = ?2
                 ORDER BY COALESCE(metadata_completeness, 0) DESC, COALESCE(file_size, 0) DESC, id ASC"
            )
            .map_err(|e| e.to_string())?
            .query_map(params![lower_title, aid], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
        } else {
            conn.prepare(
                "SELECT id FROM tracks
                 WHERE LOWER(title) = ?1 AND artist_id IS NULL
                 ORDER BY COALESCE(metadata_completeness, 0) DESC, COALESCE(file_size, 0) DESC, id ASC"
            )
            .map_err(|e| e.to_string())?
            .query_map(params![lower_title], |row| row.get(0))
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect()
        };

        if track_ids.len() <= 1 {
            continue;
        }

        let keep_id = track_ids[0];
        let delete_ids = &track_ids[1..];

        for &dup_id in delete_ids {
            conn.execute(
                "UPDATE OR IGNORE playlist_tracks SET track_id = ?1 WHERE track_id = ?2",
                params![keep_id, dup_id],
            ).map_err(|e| e.to_string())?;
            conn.execute(
                "DELETE FROM playlist_tracks WHERE track_id = ?1",
                params![dup_id],
            ).map_err(|e| e.to_string())?;

            let file_path: Option<String> = conn.query_row(
                "SELECT file_path FROM tracks WHERE id = ?1",
                params![dup_id],
                |row| row.get(0),
            ).ok();

            conn.execute("DELETE FROM tracks WHERE id = ?1", params![dup_id])
                .map_err(|e| e.to_string())?;

            if let Some(path) = file_path {
                let _ = std::fs::remove_file(&path);
            }

            deleted_tracks += 1;
        }

        merged_track_groups += 1;
    }

    log::info!(
        "Track cleanup: merged {} groups, deleted {} duplicate tracks",
        merged_track_groups, deleted_tracks
    );

    Ok(serde_json::json!({
        "merged_track_groups": merged_track_groups,
        "deleted_duplicate_tracks": deleted_tracks
    }))
}

// --- Artist Enrichment ---

#[tauri::command]
pub async fn enrich_artist(
    db: State<'_, Arc<DbPool>>,
    artist_id: i64,
) -> Result<serde_json::Value, String> {
    let (name, existing_mbid): (String, Option<String>) = {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.query_row(
            "SELECT name, musicbrainz_id FROM artists WHERE id = ?1",
            params![artist_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?)),
        ).map_err(|e| e.to_string())?
    };

    // Get or search for MusicBrainz artist ID
    let mbid = if let Some(ref id) = existing_mbid {
        id.clone()
    } else {
        let id = crate::metadata::musicbrainz::search_artist(&name).await?;
        // Save the MBID
        if let Ok(conn) = db.lock() {
            let _ = conn.execute(
                "UPDATE artists SET musicbrainz_id = ?1 WHERE id = ?2 AND musicbrainz_id IS NULL",
                params![id, artist_id],
            );
        }
        // Rate limit
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        id
    };

    // Fetch discography
    let discography = crate::metadata::musicbrainz::get_artist_discography(&mbid).await?;

    // Store as JSON on artist
    let json = serde_json::to_string(&discography).map_err(|e| e.to_string())?;
    {
        let conn = db.lock().map_err(|e| e.to_string())?;
        conn.execute(
            "UPDATE artists SET enriched_discography = ?1 WHERE id = ?2",
            params![json, artist_id],
        ).map_err(|e| e.to_string())?;
    }

    Ok(serde_json::json!({
        "artist_id": artist_id,
        "mbid": mbid,
        "total_releases": discography.len(),
        "discography": discography,
    }))
}
