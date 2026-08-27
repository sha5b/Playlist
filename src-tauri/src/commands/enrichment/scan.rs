//! Bulk metadata scans: fill missing metadata for the whole library.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{{Emitter, Manager, State}};

use crate::db::DbPool;

use super::artist::fetch_artist_image_bytes;

/// Global cancellation flag for metadata scans
pub(crate) static METADATA_SCAN_CANCELLED: AtomicBool = AtomicBool::new(false);

/// True while a metadata scan is running. Prevents concurrent scans, which
/// would double MusicBrainz traffic and clear the cancellation flag of a
/// still-running scan (un-cancelling it).
pub(crate) static METADATA_SCAN_RUNNING: AtomicBool = AtomicBool::new(false);

/// RAII guard that marks the scan finished even on early returns/errors.
struct ScanRunningGuard;
impl Drop for ScanRunningGuard {
    fn drop(&mut self) {
        METADATA_SCAN_RUNNING.store(false, Ordering::Relaxed);
    }
}

#[derive(Debug, serde::Serialize)]
pub struct ScanMissingResult {
    pub total_tracks: i64,
    pub enriched: i64,
    pub failed: i64,
    pub completeness_avg: i64,
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

        let artist_for_lastfm = artist_name.as_deref();
        let mb_result = crate::metadata::musicbrainz::enrich_track_search_only(title, artist_name.as_deref()).await;

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
                    if let Some(lfm_artist) = artist_for_lastfm {
                    if let Ok(lastfm_data) = crate::metadata::lastfm::get_track_info(title, lfm_artist).await {
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

                // Fallback: try Last.fm (only with a real artist — querying
                // with a literal "unknown" returns an unrelated track whose
                // tags would be written permanently)
                let lastfm_fallback = match artist_for_lastfm {
                    Some(a) => crate::metadata::lastfm::get_track_info(title, a).await,
                    None => Err("no artist".to_string()),
                };
                match lastfm_fallback {
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

    // Compute average completeness. SQLite's AVG() returns REAL — reading it
    // as i64 fails type conversion and silently yields 0, so round it here.
    let completeness_avg = {
        let conn = crate::db::lock(&db)?;
        conn.query_row(
            "SELECT CAST(COALESCE(AVG(metadata_completeness), 0) AS INTEGER) FROM tracks",
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
        match crate::metadata::musicbrainz::enrich_track_search_only(title, artist_name.as_deref()).await {
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
