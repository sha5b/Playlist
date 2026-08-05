use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use rusqlite::params;
use tauri::{Emitter, Manager, State};

use crate::db::DbPool;

/// Global cancellation flag for metadata scans
static METADATA_SCAN_CANCELLED: AtomicBool = AtomicBool::new(false);

/// True while a metadata scan is running. Prevents concurrent scans, which
/// would double MusicBrainz traffic and clear the cancellation flag of a
/// still-running scan (un-cancelling it).
static METADATA_SCAN_RUNNING: AtomicBool = AtomicBool::new(false);

/// RAII guard that marks the scan finished even on early returns/errors.
struct ScanRunningGuard;
impl Drop for ScanRunningGuard {
    fn drop(&mut self) {
        METADATA_SCAN_RUNNING.store(false, Ordering::Relaxed);
    }
}

/// Fetch artist image bytes: Deezer first (Last.fm's artist.getinfo has only
/// returned a placeholder star since 2019), then any usable Last.fm URL.
async fn fetch_artist_image_bytes(
    artist_name: &str,
    lastfm_url: Option<&str>,
) -> Option<Vec<u8>> {
    if let Some(url) = crate::metadata::deezer::get_artist_image_url(artist_name).await {
        if let Some(bytes) = crate::metadata::lastfm::download_image(&url).await {
            return Some(bytes);
        }
    }
    if let Some(url) = lastfm_url {
        return crate::metadata::lastfm::download_image(url).await;
    }
    None
}

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
        if let Some(ref mb_album_id) = enrichment.album_musicbrainz_id {
            let album_id: Option<i64> = conn.query_row(
                "SELECT album_id FROM tracks WHERE id = ?1",
                rusqlite::params![track_id],
                |row| row.get(0),
            ).ok().flatten();
            if let Some(aid) = album_id {
                for &(col, val) in &[
                    ("musicbrainz_id", Some(mb_album_id.as_str())),
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

/// Enrich an album's metadata from MusicBrainz + Last.fm, including tracklist and cover art
#[derive(Debug, serde::Serialize)]
pub struct EnrichAlbumResult {
    pub album_id: i64,
    pub fields_updated: i64,
    /// Number of tracks in the canonical tracklist we discovered and stored (for
    /// showing the full album with placeholders). NOT tracks inserted into the library —
    /// actual audio is only added via the download path.
    pub tracklist_size: i64,
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
        let conn = crate::db::lock(&db)?;
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
        let conn = crate::db::lock(&db)?;
        let mut updated = 0i64;

        // Fill missing album fields
        let album_fields: &[(&str, &Option<String>)] = &[
            ("musicbrainz_id", &enrichment.musicbrainz_id),
            ("release_date", &enrichment.release_date),
            ("label", &enrichment.label),
            ("album_type", &enrichment.album_type),
        ];
        for &(col, val) in album_fields {
            if let Some(ref v) = val {
                updated += crate::db::update_field_if_missing(&conn, "albums", col, album_id, v);
            }
        }

        // Genre: prefer Last.fm tags (joined), fallback to MusicBrainz
        if existing_genre.is_none() {
            let genre = lastfm_data.as_ref()
                .filter(|d| !d.tags.is_empty())
                .map(|d| d.tags.join(", "))
                .or(enrichment.genre.clone());
            if let Some(ref v) = genre {
                updated += crate::db::update_field_if_missing(&conn, "albums", "genre", album_id, v);
            }
        }

        // Description: prefer Last.fm wiki
        if existing_description.is_none() {
            let desc = lastfm_data.as_ref().and_then(|d| d.description.clone());
            if let Some(ref v) = desc {
                updated += crate::db::update_field_if_missing(&conn, "albums", "description", album_id, v);
            }
        }

        if let Some(tt) = enrichment.total_tracks {
            updated += conn.execute(
                "UPDATE albums SET total_tracks = ?1 WHERE id = ?2 AND total_tracks IS NULL",
                rusqlite::params![tt, album_id],
            ).unwrap_or(0) as i64;
        }
        if let Some(td) = enrichment.total_discs {
            updated += conn.execute(
                "UPDATE albums SET total_discs = ?1 WHERE id = ?2 AND total_discs IS NULL",
                rusqlite::params![td, album_id],
            ).unwrap_or(0) as i64;
        }

        // Update artist info
        let artist_id: Option<i64> = conn.query_row(
            "SELECT artist_id FROM albums WHERE id = ?1",
            rusqlite::params![album_id],
            |row| row.get(0),
        ).ok().flatten();

        if let Some(aid) = artist_id {
            crate::db::apply_artist_enrichment(
                &conn, aid,
                enrichment.artist_musicbrainz_id.as_deref(),
                enrichment.artist_sort_name.as_deref(),
                enrichment.artist_type.as_deref(),
                enrichment.artist_country.as_deref(),
                enrichment.artist_begin_year,
                None,
            );
            // Artist bio from Last.fm
            if let Some(ref lfm_artist) = lastfm_artist {
                if let Some(ref bio) = lfm_artist.bio {
                    crate::db::update_field_if_missing(&conn, "artists", "bio", aid, bio);
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
    }

    // Download artist image if missing — independent of the album cover state.
    // (This used to be nested inside `existing_cover.is_none()`, so any album
    // that already had a cover never fetched its artist's image.)
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
            if let Some(ref artist) = artist_name {
                let lfm_url = lastfm_artist.as_ref().and_then(|a| a.image_url.as_deref());
                artist_img_bytes = fetch_artist_image_bytes(artist, lfm_url).await;
            }
            if let Some(bytes) = artist_img_bytes {
                if let Ok(covers_dir) = app_handle.path().app_data_dir().map(|d| d.join("covers")) {
                    let _ = std::fs::create_dir_all(&covers_dir);
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

    let tracklist = enrichment.tracklist.clone();
    let tracklist_size = tracklist.len() as i64;

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

    Ok(EnrichAlbumResult { album_id, fields_updated: updated, tracklist_size, tracklist })
}

/// Scan all tracks with low metadata completeness and enrich them
#[tauri::command]
pub async fn scan_missing_metadata(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
) -> Result<ScanMissingResult, String> {
    // Only one scan at a time — a second scan would double MusicBrainz
    // traffic and reset the cancellation flag of the running scan.
    if METADATA_SCAN_RUNNING
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("A metadata scan is already running".to_string());
    }
    let _running_guard = ScanRunningGuard;

    // Get tracks that need enrichment (completeness < 70 OR completeness = 0)
    // Note: We don't recompute completeness for tracks at 0% before enriching them,
    // because that would prevent re-enrichment after metadata deletion.
    // Tracks at 0% are explicitly marked for enrichment (e.g., after deletion).
    let tracks_to_enrich: Vec<(i64, String, Option<String>)> = {
        let conn = crate::db::lock(&db)?;
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
            tokio::time::sleep(std::time::Duration::from_millis(crate::metadata::musicbrainz::MB_RATE_LIMIT_MS)).await;
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
                    let track_fields: &[(&str, &Option<String>)] = &[
                        ("musicbrainz_id", &enrichment.musicbrainz_id),
                        ("genre", &enrichment.genre),
                        ("release_date", &enrichment.release_date),
                        ("isrc", &enrichment.isrc),
                        ("description", &enrichment.description),
                        ("label", &enrichment.label),
                        ("language", &enrichment.language),
                    ];
                    for &(col, val) in track_fields {
                        if let Some(ref v) = val {
                            crate::db::update_field_if_missing(&conn, "tracks", col, *track_id, v);
                        }
                    }

                    if let Some(ref new_tags) = enrichment.tags {
                        crate::db::merge_tags(&conn, *track_id, new_tags);
                    }

                    // Update artist
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
                                None,
                            );
                        }
                    }
                } // conn dropped here before await

                // Also try Last.fm for supplementary genre/description if MusicBrainz didn't provide them
                if !METADATA_SCAN_CANCELLED.load(Ordering::Relaxed) {
                    if let Ok(lastfm_data) = crate::metadata::lastfm::get_track_info(title, artist_for_lastfm).await {
                        if let Ok(conn) = db.lock() {
                            if !lastfm_data.tags.is_empty() {
                                let genre_val = lastfm_data.tags.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
                                crate::db::update_field_if_missing(&conn, "tracks", "genre", *track_id, &genre_val);
                            }
                            if let Some(ref desc) = lastfm_data.description {
                                crate::db::update_field_if_missing(&conn, "tracks", "description", *track_id, desc);
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
                                if has_genre {
                                    let genre_val = lastfm_data.tags.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
                                    crate::db::update_field_if_missing(&conn, "tracks", "genre", *track_id, &genre_val);
                                }
                                if let Some(ref desc) = lastfm_data.description {
                                    crate::db::update_field_if_missing(&conn, "tracks", "description", *track_id, desc);
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
        let conn = crate::db::lock(&db)?;
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
            tokio::time::sleep(std::time::Duration::from_millis(crate::metadata::musicbrainz::MB_RATE_LIMIT_MS)).await;
        }

        let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
            "phase": "albums",
            "current": i + 1,
            "total": total_albums,
            "title": title,
        }));

        // MusicBrainz album enrichment
        let enrichment = match crate::metadata::musicbrainz::enrich_album(title, artist_name.as_deref()).await {
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
            crate::metadata::lastfm::get_album_info(title, artist).await.ok()
        } else {
            None
        };

        if let Ok(conn) = db.lock() {
            let album_fields: &[(&str, &Option<String>)] = &[
                ("musicbrainz_id", &enrichment.musicbrainz_id),
                ("release_date", &enrichment.release_date),
                ("label", &enrichment.label),
                ("album_type", &enrichment.album_type),
            ];
            for &(col, val) in album_fields {
                if let Some(ref v) = val {
                    crate::db::update_field_if_missing(&conn, "albums", col, *album_id, v);
                }
            }

            // Genre: prefer Last.fm tags
            let genre = lastfm_data.as_ref()
                .filter(|d| !d.tags.is_empty())
                .map(|d| d.tags.join(", "))
                .or(enrichment.genre.clone());
            if let Some(ref v) = genre {
                crate::db::update_field_if_missing(&conn, "albums", "genre", *album_id, v);
            }

            // Description from Last.fm
            if let Some(ref desc) = lastfm_data.as_ref().and_then(|d| d.description.clone()) {
                crate::db::update_field_if_missing(&conn, "albums", "description", *album_id, desc);
            }

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
                crate::db::apply_artist_enrichment(
                    &conn, aid,
                    enrichment.artist_musicbrainz_id.as_deref(),
                    enrichment.artist_sort_name.as_deref(),
                    enrichment.artist_type.as_deref(),
                    enrichment.artist_country.as_deref(),
                    enrichment.artist_begin_year,
                    None,
                );
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
                let lfm = crate::metadata::lastfm::get_artist_info(artist).await.ok();
                let lfm_url = lfm.as_ref().and_then(|l| l.image_url.as_deref());
                if let Some(bytes) = fetch_artist_image_bytes(artist, lfm_url).await {
                    let filename = format!("artist_{}.jpg", aid);
                    let path = covers_dir.join(&filename);
                    if std::fs::write(&path, &bytes).is_ok() {
                        let path_str = path.to_string_lossy().to_string();
                        if let Ok(conn) = db.lock() {
                            let _ = conn.execute("UPDATE artists SET image_path = ?1 WHERE id = ?2 AND image_path IS NULL", rusqlite::params![path_str, aid]);
                        }
                    }
                }
                // Bio still comes from Last.fm even when the image comes from Deezer
                // (or when no image was found at all).
                if let Some(bio) = lfm.as_ref().and_then(|l| l.bio.as_ref()) {
                    if let Ok(conn) = db.lock() {
                        let _ = conn.execute("UPDATE artists SET bio = ?1 WHERE id = ?2 AND (bio IS NULL OR bio = '')", rusqlite::params![bio, aid]);
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
            tokio::time::sleep(std::time::Duration::from_millis(crate::metadata::musicbrainz::MB_RATE_LIMIT_MS)).await;
        }

        let _ = app_handle.emit("auto-enrich-progress", serde_json::json!({
            "phase": "tracks",
            "current": i + 1,
            "total": total_tracks,
            "title": title,
        }));

        // MusicBrainz track enrichment
        match crate::metadata::musicbrainz::enrich_track(title, artist_name.as_deref()).await {
            Ok(enrichment) => {
                // Apply MusicBrainz data
                if let Ok(conn) = db.lock() {
                    let track_fields: &[(&str, &Option<String>)] = &[
                        ("musicbrainz_id", &enrichment.musicbrainz_id),
                        ("genre", &enrichment.genre),
                        ("release_date", &enrichment.release_date),
                        ("isrc", &enrichment.isrc),
                        ("description", &enrichment.description),
                        ("label", &enrichment.label),
                        ("language", &enrichment.language),
                    ];
                    for &(col, val) in track_fields {
                        if let Some(ref v) = val {
                            crate::db::update_field_if_missing(&conn, "tracks", col, *track_id, v);
                        }
                    }

                    if let Some(ref new_tags) = enrichment.tags {
                        crate::db::merge_tags(&conn, *track_id, new_tags);
                    }
                }
                // conn is dropped here

                // Also enrich with Last.fm track tags
                if let Some(ref artist) = artist_name {
                    if let Ok(lfm) = crate::metadata::lastfm::get_track_info(title, artist).await {
                        if let Ok(conn) = db.lock() {
                            if !lfm.tags.is_empty() {
                                let tags_str = lfm.tags.join(", ");
                                crate::db::update_field_if_missing(&conn, "tracks", "genre", *track_id, &tags_str);
                            }
                            if let Some(ref desc) = lfm.description {
                                crate::db::update_field_if_missing(&conn, "tracks", "description", *track_id, desc);
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
    let conn = crate::db::lock(&db)?;
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

    let conn = crate::db::lock(&db)?;
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

/// Artist names that legitimately contain a comma, so the comma must not be read
/// as a credit separator. Lowercase, compared whole.
const COMMA_IN_NAME: &[&str] = &[
    "earth, wind & fire",
    "tyler, the creator",
    "crosby, stills & nash",
    "crosby, stills, nash & young",
    "emerson, lake & palmer",
    "blood, sweat & tears",
    "peter, paul and mary",
    "hannah williams, the affirmations",
    "kool, rock-ski",
];

/// Scrape labels that end up glued to the front of an artist name. The library
/// holds a dozen of these — `PREMIERE: Aleksandir`, `Lyrics: Miracle Musical` —
/// each of which forks an album away from its clean twin.
const SCRAPE_PREFIXES: &[&str] = &[
    "premiere", "première", "premier", "lyrics", "full album", "out now",
    "free download", "video", "audio",
];

/// The artist's own name, for deciding whether two same-titled albums are the
/// same album.
///
/// Album identity has to survive three things ingest does to a credit line:
///
/// 1. **Featured artists appended.** `Princess Nokia, Wiki` and `Princess Nokia`
///    are one album, and `Dr. Dre, Eminem, Xzibit` and `Dr. Dre, Hittman,
///    Six-Two, Nate Dogg, Kurupt` are one album, so the key is the credit line's
///    *first* name and not the whole string or its id.
/// 2. **A scraped label in front.** `PREMIERE : Aleksandir` is Aleksandir.
/// 3. **A scraped counter behind.** `JUN FUKAMACHI...02` is Jun Fukamachi.
///
/// The comma is not always a separator, which is why `COMMA_IN_NAME` exists:
/// splitting `Earth, Wind & Fire` at its comma would key that album on "earth".
/// The blast radius of a wrong split is bounded — this decides album *grouping*
/// only and never edits the artists table — but the guard costs nothing.
///
/// Returns `None` for a missing or empty name, which callers treat as "unknown
/// artist, compatible with anything".
fn primary_artist_key(name: Option<&str>) -> Option<String> {
    let raw = name?.trim();
    if raw.is_empty() {
        return None;
    }

    // 1. a leading scrape label, up to the first ':' — "PREMIERE : X", "Lyrics: X"
    let mut s = raw;
    if let Some((head, tail)) = s.split_once(':') {
        let head = head.trim().to_lowercase();
        if SCRAPE_PREFIXES.contains(&head.as_str()) && !tail.trim().is_empty() {
            s = tail.trim();
        }
    }

    // 2. a trailing "...NN" counter, and a trailing " Official"
    if let Some(idx) = s.rfind("..") {
        let tail = s[idx..].trim_start_matches('.');
        if !tail.is_empty() && tail.chars().all(|c| c.is_ascii_digit()) {
            // `rfind` lands inside the run of dots, so the rest of the run has to
            // go too — "JUN FUKAMACHI...02" must not keep a trailing dot.
            s = s[..idx].trim_end_matches('.').trim_end();
        }
    }
    if let Some(stripped) = s.strip_suffix(" Official") {
        if !stripped.trim().is_empty() {
            s = stripped.trim_end();
        }
    }

    if s.is_empty() {
        s = raw;
    }
    let lower = s.to_lowercase();
    if COMMA_IN_NAME.contains(&lower.as_str()) {
        return Some(lower);
    }

    // 3. the first credit in the line
    let primary = lower.split(", ").next().unwrap_or(&lower).trim().to_string();
    if primary.is_empty() {
        Some(lower)
    } else {
        Some(primary)
    }
}

#[tauri::command]
pub fn metadata_cleanup_duplicates(
    db: State<'_, Arc<DbPool>>,
) -> Result<serde_json::Value, String> {
    let conn = crate::db::lock(&db)?;

    // === PHASE 1: Merge duplicate albums ===
    // Group by LOWER(title), but only merge albums whose artists are
    // compatible: same primary artist, or one side has no artist yet. Merging on
    // title alone destroyed data — "Greatest Hits" by two different artists got
    // collapsed into one album with the wrong artist's metadata.
    //
    // Compatibility is decided on the artist's *primary name* and not on
    // artist_id, because artist_id splits one album into several. Ingest stores a
    // whole credit line as a single artists row, so an album arrives under as
    // many artist rows as it has featured line-ups: "1992 Deluxe" sat under both
    // `Princess Nokia` and `Princess Nokia, Wiki`, and "2001" under
    // `Dr. Dre, Eminem, Xzibit` and `Dr. Dre, Hittman, Six-Two, Nate Dogg,
    // Kurupt`. Different ids, so the old rule read them as different albums and
    // left every such pair on the shelf. See `primary_artist_key`.
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
        // All albums with this title, the one with the most tracks first
        let albums: Vec<(i64, Option<i64>, Option<String>)> = conn.prepare(
            "SELECT a.id, a.artist_id, ar.name FROM albums a
             LEFT JOIN artists ar ON ar.id = a.artist_id
             LEFT JOIN (SELECT album_id, COUNT(*) as cnt FROM tracks WHERE album_id IS NOT NULL GROUP BY album_id) tc
               ON tc.album_id = a.id
             WHERE LOWER(a.title) = ?1
             ORDER BY COALESCE(tc.cnt, 0) DESC, a.id ASC"
        )
        .map_err(|e| e.to_string())?
        .query_map(params![lower_title], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

        // Partition into artist-compatible groups. An album with no artist
        // joins the first group (covers imports that predate artist tagging);
        // albums with different primary artists stay separate.
        let mut groups: Vec<(Option<String>, Option<i64>, Vec<i64>)> = Vec::new();
        for (id, artist_id, artist_name) in albums {
            let key = primary_artist_key(artist_name.as_deref());
            let existing = groups.iter_mut().find(|(g_key, _, _)| {
                match (g_key.as_deref(), key.as_deref()) {
                    (Some(a), Some(b)) => a == b,
                    _ => true, // either side unknown — compatible
                }
            });
            match existing {
                Some(group) => {
                    if group.0.is_none() {
                        group.0 = key;
                    }
                    if group.1.is_none() {
                        group.1 = artist_id;
                    }
                    group.2.push(id);
                }
                None => groups.push((key, artist_id, vec![id])),
            }
        }

        for (_group_key, group_artist, member_ids) in groups {
            if member_ids.len() <= 1 {
                continue;
            }
            let keep_id = member_ids[0];
            let delete_ids = &member_ids[1..];

            for &dup_id in delete_ids {
                // Move all tracks from the duplicate to the kept album
                conn.execute(
                    "UPDATE tracks SET album_id = ?1 WHERE album_id = ?2",
                    params![keep_id, dup_id],
                ).map_err(|e| e.to_string())?;

                // Fill in missing metadata fields FROM THIS DUPLICATE ONLY —
                // never from unrelated same-title albums by other artists.
                conn.execute(
                    "UPDATE albums SET
                        cover_art_path = COALESCE(cover_art_path, (SELECT cover_art_path FROM albums WHERE id = ?2)),
                        year           = COALESCE(year,           (SELECT year           FROM albums WHERE id = ?2)),
                        genre          = COALESCE(genre,          (SELECT genre          FROM albums WHERE id = ?2)),
                        musicbrainz_id = COALESCE(musicbrainz_id, (SELECT musicbrainz_id FROM albums WHERE id = ?2)),
                        label          = COALESCE(label,          (SELECT label          FROM albums WHERE id = ?2)),
                        release_date   = COALESCE(release_date,   (SELECT release_date   FROM albums WHERE id = ?2)),
                        description    = COALESCE(description,    (SELECT description    FROM albums WHERE id = ?2))
                     WHERE id = ?1",
                    params![keep_id, dup_id],
                ).map_err(|e| e.to_string())?;

                conn.execute("DELETE FROM albums WHERE id = ?1", params![dup_id])
                    .map_err(|e| e.to_string())?;
                deleted_albums += 1;
            }

            // If the kept album has no artist but the group resolved one, adopt it
            if let Some(artist_id) = group_artist {
                conn.execute(
                    "UPDATE albums SET artist_id = COALESCE(artist_id, ?2) WHERE id = ?1",
                    params![keep_id, artist_id],
                ).map_err(|e| e.to_string())?;
            }

            merged_album_groups += 1;
        }
    }

    // Clean up orphaned albums (no tracks)
    let orphaned = conn.execute(
        "DELETE FROM albums WHERE id NOT IN (SELECT DISTINCT album_id FROM tracks WHERE album_id IS NOT NULL)",
        [],
    ).map_err(|e| e.to_string())?;

    // === PHASE 2: Deduplicate tracks ===
    let (merged_track_groups, deleted_tracks) = dedup_duplicate_tracks(&conn)?;

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

/// Deduplicate tracks with identical (title, artist, album), keeping the copy
/// with the best metadata/file size. The album is part of the group key on
/// purpose: the same song on a studio album AND a compilation/single is two
/// distinct recordings — the old title+artist key deleted one of the audio
/// files irreversibly.
fn dedup_duplicate_tracks(conn: &rusqlite::Connection) -> Result<(i64, i64), String> {
    let dup_track_groups: Vec<(String, Option<i64>, Option<i64>)> = conn.prepare(
        "SELECT LOWER(title), artist_id, album_id FROM tracks
         GROUP BY LOWER(title), artist_id, album_id
         HAVING COUNT(*) > 1"
    )
    .map_err(|e| e.to_string())?
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let mut merged_track_groups = 0i64;
    let mut deleted_tracks = 0i64;

    for (lower_title, artist_id, album_id) in &dup_track_groups {
        // All tracks in this group, best quality first (`IS` is NULL-safe)
        let track_ids: Vec<i64> = conn.prepare(
            "SELECT id FROM tracks
             WHERE LOWER(title) = ?1 AND artist_id IS ?2 AND album_id IS ?3
             ORDER BY COALESCE(metadata_completeness, 0) DESC, COALESCE(file_size, 0) DESC, id ASC"
        )
        .map_err(|e| e.to_string())?
        .query_map(params![lower_title, artist_id, album_id], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

        if track_ids.len() <= 1 {
            continue;
        }

        let keep_id = track_ids[0];
        let delete_ids = &track_ids[1..];

        // Keeper's file path — never delete the file the kept row points at
        // (two DB rows can reference the same file on disk).
        let keep_path: Option<String> = conn.query_row(
            "SELECT file_path FROM tracks WHERE id = ?1",
            params![keep_id],
            |row| row.get(0),
        ).ok();

        for &dup_id in delete_ids {
            // Re-point playlist references to the kept track
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
            let _ = conn.execute("DELETE FROM tracks_fts WHERE rowid = ?1", params![dup_id]);

            if let Some(path) = file_path {
                if keep_path.as_deref() != Some(path.as_str()) {
                    let _ = std::fs::remove_file(&path);
                }
            }

            deleted_tracks += 1;
        }

        merged_track_groups += 1;
    }

    Ok((merged_track_groups, deleted_tracks))
}

/// Deduplicate tracks only (same title + artist + album, keep best quality).
#[tauri::command]
pub fn metadata_cleanup_duplicate_tracks(
    db: State<'_, Arc<DbPool>>,
) -> Result<serde_json::Value, String> {
    let conn = crate::db::lock(&db)?;

    let (merged_track_groups, deleted_tracks) = dedup_duplicate_tracks(&conn)?;

    log::info!(
        "Track cleanup: merged {} groups, deleted {} duplicate tracks",
        merged_track_groups, deleted_tracks
    );

    Ok(serde_json::json!({
        "merged_track_groups": merged_track_groups,
        "deleted_duplicate_tracks": deleted_tracks
    }))
}

// ── Mismatch detection ──────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub struct TrackMismatch {
    pub track_id: i64,
    pub track_title: String,
    pub album_title: String,
    pub album_id: i64,
    pub reasons: Vec<String>,
    pub track_genre: Option<String>,
    pub album_genre: Option<String>,
    pub track_artist: Option<String>,
    pub album_artist: Option<String>,
}

/// Genre families for fuzzy matching — unknown genres are assumed compatible.
const GENRE_FAMILIES: &[&[&str]] = &[
    &["hip hop", "hip-hop", "rap", "trap", "gangsta rap", "boom bap", "dirty south", "conscious hip hop", "g-funk", "crunk", "southern hip hop", "west coast hip hop", "east coast hip hop"],
    &["rock", "hard rock", "alternative rock", "indie rock", "punk rock", "garage rock", "psychedelic rock", "progressive rock", "grunge", "post-punk", "new wave"],
    &["metal", "heavy metal", "death metal", "black metal", "thrash metal", "doom metal", "metalcore", "nu metal", "power metal", "symphonic metal", "progressive metal", "deathcore"],
    &["pop", "synth-pop", "dance-pop", "electropop", "indie pop", "dream pop", "art pop", "k-pop", "j-pop", "bubblegum pop", "teen pop"],
    &["electronic", "edm", "house", "techno", "trance", "dubstep", "drum and bass", "ambient", "idm", "downtempo", "electro"],
    &["r&b", "rnb", "rhythm and blues", "neo soul", "soul", "funk", "contemporary r&b", "motown", "new jack swing"],
    &["jazz", "smooth jazz", "bebop", "free jazz", "jazz fusion", "swing", "cool jazz", "acid jazz"],
    &["classical", "baroque", "romantic", "contemporary classical", "opera", "orchestral", "chamber music", "minimalism"],
    &["country", "country rock", "alt-country", "bluegrass", "americana", "outlaw country", "country pop"],
    &["reggae", "dancehall", "dub", "ska", "ragga", "roots reggae", "lovers rock"],
    &["blues", "delta blues", "electric blues", "chicago blues", "blues rock"],
    &["latin", "salsa", "reggaeton", "bossa nova", "cumbia", "bachata", "latin pop", "merengue", "latin rock"],
    &["folk", "indie folk", "folk rock", "acoustic", "singer-songwriter", "traditional folk"],
];

fn genre_family(genre: &str) -> Option<usize> {
    let lower = genre.to_lowercase();
    GENRE_FAMILIES.iter().position(|family| {
        family.iter().any(|g| lower.contains(g) || g.contains(&*lower))
    })
}

fn genres_compatible(a: &str, b: &str) -> bool {
    match (genre_family(a), genre_family(b)) {
        (Some(x), Some(y)) => x == y,
        _ => true, // If we can't classify either, assume compatible
    }
}

fn artist_matches(track_artist: &str, album_artist: &str) -> bool {
    let ta = track_artist.to_lowercase();
    let aa = album_artist.to_lowercase();
    if ta == aa { return true; }
    // Track artist starts with album artist (handles "feat." variations)
    if ta.starts_with(&aa) { return true; }
    // Album artist is contained in track artist
    if ta.contains(&aa) { return true; }
    // Various Artists compilation
    if aa == "various artists" || aa == "va" { return true; }
    false
}

fn tags_overlap(tags_a: &[String], tags_b: &[String]) -> bool {
    if tags_a.is_empty() || tags_b.is_empty() { return true; } // Can't compare, assume ok
    let set_a: std::collections::HashSet<String> = tags_a.iter().map(|t| t.to_lowercase()).collect();
    tags_b.iter().any(|t| set_a.contains(&t.to_lowercase()))
}

fn parse_tags(tags_json: &Option<String>) -> Vec<String> {
    match tags_json {
        Some(s) => serde_json::from_str(s).unwrap_or_default(),
        None => vec![],
    }
}

/// Detect tracks that don't match their album's genre/artist/tags.
#[tauri::command]
pub fn detect_album_mismatches(
    db: State<'_, Arc<DbPool>>,
) -> Result<Vec<TrackMismatch>, String> {
    let conn = crate::db::lock(&db)?;

    // Get all albums that have at least 2 tracks
    let album_rows: Vec<(i64, String, Option<String>, Option<String>)> = conn.prepare(
        "SELECT a.id, a.title, a.genre, COALESCE(ar.name, a.album_artist)
         FROM albums a
         LEFT JOIN artists ar ON ar.id = a.artist_id
         INNER JOIN tracks t ON t.album_id = a.id
         GROUP BY a.id
         HAVING COUNT(t.id) >= 2"
    )
    .map_err(|e| e.to_string())?
    .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))
    .map_err(|e| e.to_string())?
    .filter_map(|r| r.ok())
    .collect();

    let mut mismatches = Vec::new();

    for (album_id, album_title, album_genre, album_artist) in &album_rows {
        // Get all tracks for this album
        let track_rows: Vec<(i64, String, Option<String>, Option<String>, Option<String>, i64)> = conn.prepare(
            "SELECT t.id, t.title, t.genre, COALESCE(ar.name, t.album_artist), t.tags, COALESCE(t.metadata_completeness, 0)
             FROM tracks t
             LEFT JOIN artists ar ON ar.id = t.artist_id
             WHERE t.album_id = ?1"
        )
        .map_err(|e| e.to_string())?
        .query_map(params![album_id], |row| Ok((
            row.get(0)?, row.get(1)?, row.get(2)?,
            row.get(3)?, row.get(4)?, row.get(5)?,
        )))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

        // Build consensus tags from all tracks in the album
        let all_tags: Vec<Vec<String>> = track_rows.iter()
            .map(|(_, _, _, _, tags, _)| parse_tags(tags))
            .collect();
        let consensus_tags: Vec<String> = {
            let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for tags in &all_tags {
                for tag in tags {
                    *counts.entry(tag.to_lowercase()).or_insert(0) += 1;
                }
            }
            // Tags appearing in at least 2 tracks or more than 30% of tracks
            let threshold = std::cmp::max(2, track_rows.len() * 30 / 100);
            counts.into_iter()
                .filter(|(_, count)| *count >= threshold)
                .map(|(tag, _)| tag)
                .collect()
        };

        for (track_id, track_title, track_genre, track_artist, track_tags, completeness) in &track_rows {
            if *completeness < 30 { continue; } // Skip poorly enriched tracks

            let mut reasons = Vec::new();

            // Check 1: Artist mismatch
            if let (Some(ta), Some(aa)) = (track_artist, album_artist) {
                if !ta.is_empty() && !aa.is_empty() && !artist_matches(ta, aa) {
                    reasons.push(format!("Artist '{}' doesn't match album artist '{}'", ta, aa));
                }
            }

            // Check 2: Genre mismatch
            if let (Some(tg), Some(ag)) = (track_genre, album_genre) {
                if !tg.is_empty() && !ag.is_empty() && !genres_compatible(tg, ag) {
                    reasons.push(format!("Genre '{}' doesn't match album genre '{}'", tg, ag));
                }
            }

            // Check 3: Tag outlier — compare this track's tags with consensus
            let my_tags = parse_tags(track_tags);
            if !consensus_tags.is_empty() && !my_tags.is_empty() && !tags_overlap(&my_tags, &consensus_tags) {
                reasons.push("Track tags have no overlap with album consensus tags".to_string());
            }

            if !reasons.is_empty() {
                mismatches.push(TrackMismatch {
                    track_id: *track_id,
                    track_title: track_title.clone(),
                    album_title: album_title.clone(),
                    album_id: *album_id,
                    reasons,
                    track_genre: track_genre.clone(),
                    album_genre: album_genre.clone(),
                    track_artist: track_artist.clone(),
                    album_artist: album_artist.clone(),
                });
            }
        }
    }

    log::info!("Mismatch detection: found {} mismatched tracks", mismatches.len());
    Ok(mismatches)
}

// --- Artist Enrichment ---

#[tauri::command]
pub async fn enrich_artist(
    db: State<'_, Arc<DbPool>>,
    artist_id: i64,
) -> Result<serde_json::Value, String> {
    let (name, existing_mbid): (String, Option<String>) = {
        let conn = crate::db::lock(&db)?;
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
        tokio::time::sleep(std::time::Duration::from_millis(crate::metadata::musicbrainz::MB_RATE_LIMIT_MS)).await;
        id
    };

    // Fetch discography
    let discography = crate::metadata::musicbrainz::get_artist_discography(&mbid).await?;

    // Store as JSON on artist
    let json = serde_json::to_string(&discography).map_err(|e| e.to_string())?;
    {
        let conn = crate::db::lock(&db)?;
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

/// Lazily fetch an artist's image (Deezer first, Last.fm fallback), cache it
/// to the covers dir in app data, and store the path in the DB.
/// Called by the artist detail page when the artist has no image yet.
/// Returns the cached image path, or None if no image could be found.
#[tauri::command]
pub async fn fetch_artist_image(
    db: State<'_, Arc<DbPool>>,
    app_handle: tauri::AppHandle,
    artist_id: i64,
) -> Result<Option<String>, String> {
    let (name, existing): (String, Option<String>) = {
        let conn = crate::db::lock(&db)?;
        conn.query_row(
            "SELECT name, image_path FROM artists WHERE id = ?1",
            params![artist_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ).map_err(|e| e.to_string())?
    };

    // Already have an image on disk — nothing to do.
    if let Some(ref p) = existing {
        if std::path::Path::new(p).exists() {
            return Ok(Some(p.clone()));
        }
    }

    let lfm = crate::metadata::lastfm::get_artist_info(&name).await.ok();
    let lfm_url = lfm.as_ref().and_then(|l| l.image_url.as_deref());
    let Some(bytes) = fetch_artist_image_bytes(&name, lfm_url).await else {
        return Ok(None);
    };

    let covers_dir = app_handle
        .path()
        .app_data_dir()
        .map(|d| d.join("covers"))
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&covers_dir).map_err(|e| e.to_string())?;
    let path = covers_dir.join(format!("artist_{}.jpg", artist_id));
    std::fs::write(&path, &bytes).map_err(|e| e.to_string())?;
    let path_str = path.to_string_lossy().to_string();

    {
        let conn = crate::db::lock(&db)?;
        conn.execute(
            "UPDATE artists SET image_path = ?1 WHERE id = ?2",
            params![path_str, artist_id],
        ).map_err(|e| e.to_string())?;
        // Opportunistically fill the bio while we have Last.fm data in hand.
        if let Some(bio) = lfm.as_ref().and_then(|l| l.bio.as_ref()) {
            let _ = conn.execute(
                "UPDATE artists SET bio = ?1 WHERE id = ?2 AND (bio IS NULL OR bio = '')",
                params![bio, artist_id],
            );
        }
    }

    Ok(Some(path_str))
}

#[cfg(test)]
mod tests {
    use super::primary_artist_key;

    fn key(name: &str) -> String {
        primary_artist_key(Some(name)).expect("a non-empty name has a key")
    }

    #[test]
    fn featured_artists_do_not_fork_an_album() {
        // The duplicates seen in the library: same album, different credit line.
        assert_eq!(key("Princess Nokia, Wiki"), key("Princess Nokia"));
        assert_eq!(
            key("Dr. Dre, Hittman, Six-Two, Nate Dogg, Kurupt"),
            key("Dr. Dre, Eminem, Xzibit")
        );
        assert_eq!(key("Gorillaz, Moonchild Sanelly"), key("Gorillaz, Robert Smith"));
        assert_eq!(key("DANGERDOOM, MF DOOM, Danger Mouse"), key("DANGERDOOM"));
        assert_eq!(key("Şatellites, Vicky Ashkenazy"), key("Şatellites"));
    }

    #[test]
    fn scrape_labels_are_stripped() {
        assert_eq!(key("PREMIERE : Aleksandir"), key("Aleksandir"));
        assert_eq!(key("PREMIERE: Aleksandir"), key("Aleksandir"));
        assert_eq!(key("Lyrics: Miracle Musical"), key("Miracle Musical"));
        assert_eq!(key("JUN FUKAMACHI...02"), key("Jun Fukamachi"));
        assert_eq!(key("Birdy Nam Nam Official"), key("Birdy Nam Nam"));
    }

    #[test]
    fn different_artists_stay_apart() {
        // Both pairs share an album title in the library and must NOT merge.
        assert_ne!(key("The Little Dippers, Buddy Killen"), key("Flight Facilities"));
        assert_ne!(key("The Wolfgang Press"), key("AUDREY NUNA"));
    }

    #[test]
    fn a_comma_inside_a_name_is_not_a_separator() {
        assert_eq!(key("Earth, Wind & Fire"), "earth, wind & fire");
        assert_eq!(key("Tyler, The Creator"), "tyler, the creator");
        // ...and such a name must not collide with the bare first word.
        assert_ne!(key("Earth, Wind & Fire"), key("Earth"));
    }

    #[test]
    fn a_missing_name_has_no_key() {
        assert_eq!(primary_artist_key(None), None);
        assert_eq!(primary_artist_key(Some("   ")), None);
    }

    #[test]
    fn a_name_that_is_only_a_label_survives() {
        // "PREMIERE" alone is not a prefix to strip — there is nothing behind it.
        assert_eq!(key("PREMIERE"), "premiere");
    }
}
