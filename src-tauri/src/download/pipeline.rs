//! The download pipeline: fast-path sources, smart search matching,
//! fallbacks, and post-download success/failure handling.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tauri::Emitter;

use crate::db::DbPool;
use super::{deezer, matching, metadata, source, spotify, ytdlp, DownloadEvent};

use super::import::{import_downloaded_file, DownloadMeta, ImportOutcome};
use super::search::{build_search_variations, resolve_search_url};
use super::{emit_event, update_download_status_with_entry};

/// Build a search query string from a download's title and artist.
fn build_download_search_query(download: &crate::db::models::Download) -> Option<String> {
    match (&download.artist, &download.title) {
        (Some(a), Some(t)) => Some(format!("{} - {}", a, t)),
        (None, Some(t)) => Some(t.clone()),
        _ => None,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn run_download(
    db: Arc<DbPool>,
    app_handle: tauri::AppHandle,
    download_id: i64,
    download_dir: PathBuf,
    ytdlp_binary: String,
    ffmpeg_dir: Option<String>,
    cookies_from_browser: Option<String>,
    sources: Arc<tokio::sync::RwLock<source::SourceRegistry>>,
    used_urls_by_album: Arc<tokio::sync::RwLock<HashMap<i64, HashSet<String>>>>,
) {
    let mut download = {
        let conn = match db.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        match crate::db::downloads::get_download(&conn, download_id) {
            Ok(Some(d)) => d,
            _ => return,
        }
    };

    log::info!("[download] id={} platform='{}' url='{}' title={:?}", download_id, download.platform, download.url, download.title);

    // Convert Spotify URLs to YouTube search on-the-fly for legacy downloads
    if download.url.starts_with("https://open.spotify.com/") || download.url.starts_with("spotify:") {
        log::info!("Converting legacy Spotify URL to YouTube search for download {}", download_id);
        match spotify::fetch_track_metadata(&download.url).await {
            Some((title, artist)) => {
                let search_query = match artist {
                    Some(ref a) => format!("{} - {}", a, title),
                    None => title.clone(),
                };
                download.url = format!("ytsearch5:{}", search_query);
                if let Ok(conn) = db.lock() {
                    let _ = conn.execute(
                        "UPDATE downloads SET url = ?1, title = ?2, artist = ?3 WHERE id = ?4",
                        rusqlite::params![&download.url, &title, artist.as_deref(), download_id],
                    );
                }
            }
            None => {
                fail_download(&db, &app_handle, download_id, "Failed to fetch Spotify metadata for conversion");
                return;
            }
        }
    }

    if let Err(e) = std::fs::create_dir_all(&download_dir) {
        fail_download(&db, &app_handle, download_id, &format!("Failed to create download dir: {}", e));
        return;
    }

    // Update status to downloading and notify linked monitored entry
    update_download_status_with_entry(&db, &app_handle, download_id, "downloading");
    emit_event(&app_handle, download_id, "downloading", 0.0, None, None, None, None);

    let file_stem = format!("dl_{}", download_id);
    let platform = download.platform.clone();
    let drm_platforms = ["spotify", "apple_music", "tidal", "deezer", "amazon_music"];
    let is_drm_platform = drm_platforms.contains(&platform.as_str());
    let is_search_url = download.url.starts_with("ytsearch") || download.url.starts_with("ytmsearch") || download.url.starts_with("scsearch");

    // --- Phase 1: Fast direct download ---
    // For DRM platforms, try native source first (Deezer etc.) — it's a fast direct download.
    // For direct URLs (YouTube, SoundCloud, etc.), try yt-dlp first.
    let mut errors: Vec<String> = Vec::new();
    let mut failed_urls: HashSet<String> = HashSet::new();
    let mut ytdlp_info: Option<ytdlp::VideoInfo> = None;

    // Try native source FIRST for DRM platforms (fast, no searching)
    if is_drm_platform {
        let native_result = {
            let sources_guard = sources.read().await;
            if let Some(src) = sources_guard.get_for_platform(&platform) {
                log::info!("Trying native {} source for download {} (fast path)", platform, download_id);
                let app_handle_native = app_handle.clone();
                let dl_id_native = download_id;
                Some(src.download(
                    &download.url,
                    &download_dir,
                    &file_stem,
                    &download.format,
                    Box::new(move |pct| {
                        emit_event(&app_handle_native, dl_id_native, "downloading", pct, None, None, None, None);
                    }),
                ).await)
            } else {
                // Try cross-platform search with title/artist (e.g. Deezer search for Spotify tracks)
                let query = build_download_search_query(&download);
                if let Some(ref q) = query {
                    if let Some(src) = sources_guard.get_best_search_source() {
                        log::info!("Trying {} search for download {} (fast path): {}", src.platform(), download_id, q);
                        let app_handle_search = app_handle.clone();
                        let dl_id_search = download_id;
                        Some(src.search_download(
                            q,
                            &download_dir,
                            &file_stem,
                            &download.format,
                            Box::new(move |pct| {
                                emit_event(&app_handle_search, dl_id_search, "downloading", pct, None, None, None, None);
                            }),
                        ).await)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }
        };

        if let Some(Ok(file_path)) = native_result {
            handle_download_success(&db, &app_handle, download_id, &file_path, &None).await;
            return;
        }
        if let Some(Err(native_err)) = native_result {
            errors.push(format!("native {}: {}", platform, native_err));
        }
    }

    // Try yt-dlp direct download (fast for YouTube/SoundCloud/Bandcamp URLs, skipped if search URL on DRM platform)
    let skip_ytdlp_initial = is_drm_platform && is_search_url;
    let ytdlp_success = if !skip_ytdlp_initial {
        if is_search_url {
            // Smart search with cascading resolver: try deterministic lookups first,
            // then fall back to search with improved scoring and album-level dedup.
            // Works with or without target_duration_ms (title matching is always used).
            // Build search query using primary artist only (first artist before comma/&/feat)
            // to avoid polluting search with too many featured artists
            let primary_artist = download.artist.as_deref().map(|a| {
                a.split(&[',', '&'][..])
                    .next()
                    .unwrap_or(a)
                    .replace("feat.", "")
                    .replace("ft.", "")
                    .trim()
                    .to_string()
            });
            let search_query = match (&primary_artist, &download.title) {
                (Some(a), Some(t)) => format!("{} - {}", a, t),
                (_, Some(t)) => t.clone(),
                _ => download.url
                    .trim_start_matches("ytsearch5:")
                    .trim_start_matches("ytsearch1:")
                    .trim_start_matches("scsearch5:")
                    .to_string(),
            };
            let is_album_track = download.target_album_id.is_some();

            // Get URLs already used by other tracks in this album
            let album_used_urls: HashSet<String> = if let Some(album_id) = download.target_album_id {
                let guard = used_urls_by_album.read().await;
                guard.get(&album_id).cloned().unwrap_or_default()
            } else {
                HashSet::new()
            };

            // --- Layer 0: pin the exact recording identity from free metadata ---
            // Single/playlist tracks arrive with no ISRC and no duration, so the matcher
            // has no duration guard. Deezer's public API (no auth) supplies both — but
            // only after we VERIFY it returned the same song, so a wrong first hit can't
            // inject a wrong ISRC/duration. This also lets Layer 2 (Odesli) find a link.
            let mut expected_secs = download.target_duration_ms.map(|ms| ms as f64 / 1000.0);
            let mut isrc = download.target_isrc.clone();
            let mut album_hint = download.target_album_name.clone();
            if (isrc.is_none() || expected_secs.is_none()) && download.title.is_some() {
                if let Some(meta) = deezer::search_public_metadata(&search_query).await {
                    let title_ok = download.title.as_deref()
                        .map(|t| matching::title_similarity(t, &meta.title) >= 0.6)
                        .unwrap_or(false);
                    let artist_ok = match (primary_artist.as_deref(), meta.artist.as_deref()) {
                        (Some(a), Some(ma)) => matching::title_similarity(a, ma) >= 0.5,
                        (Some(_), None) => false,
                        (None, _) => true,
                    };
                    if title_ok && artist_ok {
                        if isrc.is_none() { isrc = meta.isrc.clone(); }
                        if expected_secs.is_none() { expected_secs = meta.duration; }
                        if album_hint.is_none() { album_hint = meta.album.clone(); }
                        log::info!("[download] id={} Deezer identity verified: isrc={:?} dur={:?}s", download_id, isrc, expected_secs);
                        // Persist so post-download verification & tagging can use it.
                        if let Ok(conn) = db.lock() {
                            let _ = conn.execute(
                                "UPDATE downloads SET target_isrc = COALESCE(target_isrc, ?1),
                                     target_duration_ms = COALESCE(target_duration_ms, ?2) WHERE id = ?3",
                                rusqlite::params![isrc.as_deref(), expected_secs.map(|s| (s * 1000.0) as i64), download_id],
                            );
                        }
                    } else {
                        log::info!("[download] id={} Deezer result '{}' by {:?} did not verify, ignoring", download_id, meta.title, meta.artist);
                    }
                }
            }

            let mut best_url: Option<String> = None;

            // --- Layer 1: MusicBrainz URL relationships (direct YouTube link) ---
            if best_url.is_none() {
                if let Some(ref mbid) = download.target_recording_mbid {
                    log::info!("[download] id={} trying MusicBrainz URL rels for recording {}", download_id, mbid);
                    if let Some(url) = crate::metadata::musicbrainz::lookup_music_video_url(mbid).await {
                        if !album_used_urls.contains(&url) {
                            log::info!("[download] id={} MusicBrainz resolved → {}", download_id, url);
                            best_url = Some(url);
                        } else {
                            log::info!("[download] id={} MusicBrainz URL already used by another album track, skipping", download_id);
                        }
                    }
                }
            }

            // --- Layer 2: SongLink/Odesli ISRC → YouTube URL ---
            if best_url.is_none() {
                if let Some(ref isrc_val) = isrc {
                    log::info!("[download] id={} trying SongLink ISRC lookup: {}", download_id, isrc_val);
                    if let Some(url) = crate::metadata::songlink::resolve_isrc(isrc_val).await {
                        if !album_used_urls.contains(&url) {
                            log::info!("[download] id={} SongLink resolved → {}", download_id, url);
                            best_url = Some(url);
                        } else {
                            log::info!("[download] id={} SongLink URL already used by another album track, skipping", download_id);
                        }
                    }
                }
            }

            // --- Layer 3: YouTube Music (then YouTube) search with scored matching ---
            if best_url.is_none() {
                let variations = build_search_variations(&search_query, album_hint.as_deref());
                best_url = resolve_search_url(
                    &ytdlp_binary,
                    ffmpeg_dir.as_deref(),
                    cookies_from_browser.as_deref(),
                    &variations,
                    &search_query,
                    expected_secs,
                    is_album_track,
                    &album_used_urls,
                ).await;
            }

            if let Some(ref url) = best_url {
                // Register this URL as used for album dedup
                if let Some(album_id) = download.target_album_id {
                    let mut guard = used_urls_by_album.write().await;
                    guard.entry(album_id).or_default().insert(url.clone());
                }

                let app_handle_progress = app_handle.clone();
                let dl_id = download_id;
                let result = ytdlp::download_audio(
                    &ytdlp_binary,
                    ffmpeg_dir.as_deref(),
                    url,
                    &download_dir,
                    &download.format,
                    &download.quality,
                    &file_stem,
                    cookies_from_browser.as_deref(),
                    move |progress| {
                        emit_event(&app_handle_progress, dl_id, "downloading", progress.percent, progress.speed.clone(), progress.eta.clone(), None, None);
                    },
                ).await;
                match result {
                    Ok(file_path) => {
                        handle_download_success(&db, &app_handle, download_id, &file_path, &None).await;
                        return;
                    }
                    Err(e) => {
                        log::warn!("[download] id={} smart match download FAILED: {}", download_id, e);
                        errors.push(format!("smart-match: {}", e));
                        failed_urls.insert(url.clone());
                        // Release the URL claim so a retry of this track (or a
                        // sibling album track) can use it again.
                        if let Some(album_id) = download.target_album_id {
                            let mut guard = used_urls_by_album.write().await;
                            if let Some(set) = guard.get_mut(&album_id) {
                                set.remove(url);
                            }
                        }
                        false
                    }
                }
            } else {
                log::warn!("[download] id={} no smart match found for '{}'", download_id, search_query);
                errors.push("no matching result found by duration/title".to_string());
                false
            }
        } else {
            // Direct URL path (search URLs always take the smart-match branch).
            // Fetch metadata first so the UI shows a real title while
            // downloading and Phase-2 fallbacks have title/artist to work with.
            {
                ytdlp_info = match ytdlp::get_info(
                    &ytdlp_binary,
                    ffmpeg_dir.as_deref(),
                    &download.url,
                    cookies_from_browser.as_deref(),
                ).await {
                    Ok(info) => {
                        let best_title = info.track.as_deref().unwrap_or(&info.title);
                        let best_artist = info.artist.as_deref().or(info.uploader.as_deref());
                        if let Ok(conn) = db.lock() {
                            let _ = crate::db::downloads::update_download_title(&conn, download_id, best_title, best_artist);
                        }
                        emit_event(&app_handle, download_id, "downloading", 0.0, None, None, None, Some(best_title.to_string()));
                        Some(info)
                    }
                    Err(e) => {
                        log::warn!("Could not fetch metadata for download {}: {}", download_id, e);
                        None
                    }
                };
            }

            let app_handle_progress = app_handle.clone();
            let dl_id = download_id;
            let result = ytdlp::download_audio(
                &ytdlp_binary,
                ffmpeg_dir.as_deref(),
                &download.url,
                &download_dir,
                &download.format,
                &download.quality,
                &file_stem,
                cookies_from_browser.as_deref(),
                move |progress| {
                    emit_event(&app_handle_progress, dl_id, "downloading", progress.percent, progress.speed.clone(), progress.eta.clone(), None, None);
                },
            ).await;

            match result {
                Ok(file_path) => {
                    handle_download_success(&db, &app_handle, download_id, &file_path, &ytdlp_info).await;
                    return;
                }
                Err(e) => {
                    log::warn!("[download] id={} yt-dlp direct download FAILED: {}", download_id, e);
                    errors.push(format!("yt-dlp: {}", e));
                    failed_urls.insert(download.url.clone());
                    false
                }
            }
        }
    } else {
        log::info!("Skipping yt-dlp search for DRM download {} (will use search fallback)", download_id);
        false
    };

    // --- Phase 2: All fast methods failed — search fallbacks for this track ---
    if !ytdlp_success {
                // Check if the track already exists in the library.
                // A bare FTS hit is NOT enough: it must also pass the same
                // title/artist similarity gates as search matching, otherwise a
                // failed download gets marked "completed" against an unrelated
                // library track (e.g. a cover) and is never retried.
                let expected: Option<(String, Option<String>)> = if let Some(ref info) = ytdlp_info {
                    let title = info.track.as_deref().unwrap_or(&info.title).to_string();
                    let artist = info.artist.clone().or_else(|| info.uploader.clone());
                    Some((title, artist))
                } else if let Ok(conn) = db.lock() {
                    conn.query_row(
                        "SELECT title, artist FROM downloads WHERE id = ?1",
                        rusqlite::params![download_id],
                        |row| {
                            let t: Option<String> = row.get(0)?;
                            let a: Option<String> = row.get(1)?;
                            Ok(t.map(|t| (t, a)))
                        },
                    ).ok().flatten()
                } else {
                    None
                };

                let library_match = expected.as_ref().and_then(|(exp_title, exp_artist)| {
                    let q = match exp_artist {
                        Some(a) => format!("{} {}", a, exp_title),
                        None => exp_title.clone(),
                    };
                    let candidate = if let Ok(conn) = db.lock() {
                        crate::db::tracks::search_tracks_fts(&conn, &q, 1)
                            .ok()
                            .and_then(|tracks| tracks.into_iter().next())
                    } else {
                        None
                    };
                    candidate.filter(|track| {
                        let title_ok =
                            matching::title_similarity(exp_title, &track.title) >= 0.67;
                        let artist_ok = match (exp_artist, &track.artist_name) {
                            (Some(want), Some(have)) => {
                                let want_t = matching::tokens(want);
                                let have_t = matching::tokens(have);
                                let matched = want_t
                                    .iter()
                                    .filter(|w| have_t.contains(w))
                                    .count();
                                matched * 2 >= want_t.len().max(1)
                            }
                            // No artist on either side — don't reject on missing data
                            _ => true,
                        };
                        if !(title_ok && artist_ok) {
                            log::info!(
                                "Download {} FTS candidate '{}' rejected (title_ok={}, artist_ok={})",
                                download_id, track.title, title_ok, artist_ok
                            );
                        }
                        title_ok && artist_ok
                    })
                });

                if let Some(track) = library_match {
                    log::info!("Download {} matched existing library track {} ('{}')", download_id, track.id, track.title);
                    let entry_id = if let Ok(conn) = db.lock() {
                        let _ = conn.execute_batch("BEGIN");
                        let _ = crate::db::downloads::update_download_file(&conn, download_id, &track.file_path, Some(track.id));
                        let _ = crate::db::downloads::update_download_status(&conn, download_id, "completed", None);
                        let eid: Option<i64> = conn.query_row(
                            "SELECT id FROM monitored_playlist_entries WHERE download_id = ?1",
                            rusqlite::params![download_id],
                            |row| row.get(0),
                        ).ok();
                        if let Some(eid) = eid {
                            let _ = crate::db::monitored::update_entry_status(&conn, eid, "downloaded", Some(download_id), Some(track.id));
                        }
                        let _ = conn.execute_batch("COMMIT");
                        eid
                    } else {
                        None
                    };
                    emit_event(&app_handle, download_id, "completed", 100.0, None, None, None, Some(track.title.clone()));
                    if let Some(eid) = entry_id {
                        let _ = app_handle.emit("manager-entry-updated", serde_json::json!({
                            "entry_id": eid, "status": "downloaded", "track_id": track.id
                        }));
                    }
                    // NB: the frontend listens for "library-updated" (not "library-changed")
                    let _ = app_handle.emit("library-updated", ());
                    return;
                }

                // Cross-platform search via best available source (e.g. Deezer search)
                let search_query = if let Some(ref info) = ytdlp_info {
                    let title = info.track.as_deref().unwrap_or(&info.title);
                    let artist = info.artist.as_deref().or(info.uploader.as_deref());
                    match artist {
                        Some(a) => Some(format!("{} - {}", a, title)),
                        None => Some(title.to_string()),
                    }
                } else {
                    // Use download title/artist from DB
                    if let Ok(conn) = db.lock() {
                        conn.query_row(
                            "SELECT title, artist FROM downloads WHERE id = ?1",
                            rusqlite::params![download_id],
                            |row| {
                                let t: Option<String> = row.get(0)?;
                                let a: Option<String> = row.get(1)?;
                                Ok(match (t, a) {
                                    (Some(t), Some(a)) => Some(format!("{} - {}", a, t)),
                                    (Some(t), None) => Some(t),
                                    _ => None,
                                })
                            },
                        ).ok().flatten()
                    } else {
                        None
                    }
                };

                // If no search query yet (yt-dlp metadata failed), try platform-specific metadata APIs
                let search_query = if search_query.is_none() && is_drm_platform {
                    log::info!("yt-dlp metadata failed for {} URL, trying platform API for download {}", platform, download_id);
                    if let Some(meta) = metadata::fetch_track_metadata(&download.url, &platform).await {
                        // Update DB with the metadata we found
                        if let Ok(conn) = db.lock() {
                            let _ = crate::db::downloads::update_download_title(
                                &conn,
                                download_id,
                                &meta.title,
                                meta.artist.as_deref(),
                            );
                            // Persist duration/album from the platform so the duration guard
                            // and tagging below can use them (fills only when still empty).
                            let _ = conn.execute(
                                "UPDATE downloads SET target_duration_ms = COALESCE(target_duration_ms, ?1),
                                     target_album_name = COALESCE(target_album_name, ?2) WHERE id = ?3",
                                rusqlite::params![
                                    meta.duration.map(|d| (d * 1000.0) as i64),
                                    meta.album.as_deref(),
                                    download_id
                                ],
                            );
                        }
                        emit_event(&app_handle, download_id, "downloading", 0.0, None, None, None, Some(meta.title.clone()));
                        Some(metadata::build_search_query(&meta))
                    } else {
                        log::warn!("Platform metadata API also failed for {} download {}", platform, download_id);
                        None
                    }
                } else {
                    search_query
                };

                if let Some(ref query) = search_query {
                    let cross_result = if is_drm_platform {
                        let sources_guard = sources.read().await;
                        if let Some(src) = sources_guard.get_best_search_source() {
                            // Don't search on the same platform that just failed natively
                            if src.platform() != platform {
                                log::info!("Trying cross-platform search on {} for: {}", src.platform(), query);
                                let app_handle_cross = app_handle.clone();
                                let dl_id_cross = download_id;
                                Some(src.search_download(
                                    query,
                                    &download_dir,
                                    &file_stem,
                                    &download.format,
                                    Box::new(move |pct| {
                                        emit_event(&app_handle_cross, dl_id_cross, "downloading", pct, None, None, None, None);
                                    }),
                                ).await)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    if let Some(Ok(file_path)) = cross_result {
                        handle_download_success(&db, &app_handle, download_id, &file_path, &ytdlp_info).await;
                        return;
                    }
                    if let Some(Err(cross_err)) = cross_result {
                        errors.push(format!("cross-search: {}", cross_err));
                    }
                }

                // Get expected duration and ISRC for search matching (used by YouTube + SoundCloud)
                let mut expected_duration_secs = ytdlp_info.as_ref().and_then(|i| i.duration)
                    .or_else(|| download.target_duration_ms.map(|ms| ms as f64 / 1000.0))
                    .or_else(|| {
                        if let Ok(conn) = db.lock() {
                            conn.query_row(
                                "SELECT e.duration_seconds FROM monitored_playlist_entries e WHERE e.download_id = ?1",
                                rusqlite::params![download_id],
                                |row| row.get::<_, Option<f64>>(0),
                            ).ok().flatten()
                            // Safety: normalize any legacy ms values still stored in the column
                            .map(|d| if d > 36_000.0 { d / 1000.0 } else { d })
                        } else {
                            None
                        }
                    });

                // Get ISRC for precise matching (from yt-dlp info, download record, or monitored entry)
                let mut isrc = ytdlp_info.as_ref().and_then(|i| i.isrc.clone())
                    .or_else(|| {
                        if let Ok(conn) = db.lock() {
                            conn.query_row(
                                "SELECT target_isrc FROM downloads WHERE id = ?1",
                                rusqlite::params![download_id],
                                |row| row.get::<_, Option<String>>(0),
                            ).ok().flatten()
                        } else {
                            None
                        }
                    })
                    .or_else(|| {
                        if let Ok(conn) = db.lock() {
                            conn.query_row(
                                "SELECT e.isrc FROM monitored_playlist_entries e WHERE e.download_id = ?1",
                                rusqlite::params![download_id],
                                |row| row.get::<_, Option<String>>(0),
                            ).ok().flatten()
                        } else {
                            None
                        }
                    });

                // If no ISRC yet, try Deezer's public API to find one (no auth required)
                if let Some(ref query) = search_query {
                    if isrc.is_none() {
                        log::info!("No ISRC available, trying Deezer public API for: {}", query);
                        if let Some(deezer_meta) = deezer::search_public_metadata(query).await {
                            if let Some(ref deezer_isrc) = deezer_meta.isrc {
                                log::info!("Deezer public API found ISRC: {}", deezer_isrc);
                                isrc = Some(deezer_isrc.clone());
                            }
                            if expected_duration_secs.is_none() {
                                if let Some(dur) = deezer_meta.duration {
                                    log::info!("Using Deezer duration for matching: {:.0}s", dur);
                                    expected_duration_secs = Some(dur);
                                }
                            }
                        }
                    }
                }

                // Persist any ISRC/duration we just resolved so tagging/verification use it.
                if isrc.is_some() || expected_duration_secs.is_some() {
                    if let Ok(conn) = db.lock() {
                        let _ = conn.execute(
                            "UPDATE downloads SET target_isrc = COALESCE(target_isrc, ?1),
                                 target_duration_ms = COALESCE(target_duration_ms, ?2) WHERE id = ?3",
                            rusqlite::params![isrc.as_deref(), expected_duration_secs.map(|s| (s * 1000.0) as i64), download_id],
                        );
                    }
                }

                // Step 3: YouTube Music / YouTube search fallback with scored matching
                if let Some(ref query) = search_query {
                    let variations = build_search_variations(query, None);
                    if let Some(best_url) = resolve_search_url(
                        &ytdlp_binary,
                        ffmpeg_dir.as_deref(),
                        cookies_from_browser.as_deref(),
                        &variations,
                        query,
                        expected_duration_secs,
                        false,
                        &failed_urls,
                    ).await {
                        log::info!("[download] id={} fallback search matched → {}", download_id, best_url);
                        let app_handle_yt = app_handle.clone();
                        let dl_id_yt = download_id;
                        let yt_result = ytdlp::download_audio(
                            &ytdlp_binary,
                            ffmpeg_dir.as_deref(),
                            &best_url,
                            &download_dir,
                            &download.format,
                            &download.quality,
                            &file_stem,
                            cookies_from_browser.as_deref(),
                            move |progress| {
                                emit_event(&app_handle_yt, dl_id_yt, "downloading", progress.percent, progress.speed.clone(), progress.eta.clone(), None, None);
                            },
                        ).await;
                        match yt_result {
                            Ok(file_path) => {
                                handle_download_success(&db, &app_handle, download_id, &file_path, &ytdlp_info).await;
                                return;
                            }
                            Err(e) => {
                                failed_urls.insert(best_url.clone());
                                errors.push(format!("YouTube fallback download: {}", e));
                            }
                        }
                    } else {
                        errors.push("YouTube fallback: no confident match".to_string());
                    }
                }

                // Step 4: SoundCloud search as final fallback (with scored matching)
                if let Some(ref query) = search_query {
                    let sc_info_query = format!("scsearch6:{}", query);
                    log::info!("Final fallback: searching SoundCloud for '{}'", sc_info_query);

                    // Try scored match first — never blindly download the first SoundCloud result.
                    let sc_best_url = match ytdlp::search_info(
                        &ytdlp_binary,
                        ffmpeg_dir.as_deref(),
                        &sc_info_query,
                        cookies_from_browser.as_deref(),
                    ).await {
                        Ok(results) => matching::pick_best_match(&results, query, expected_duration_secs, false, &failed_urls),
                        Err(_) => None,
                    };

                    // Only download from SoundCloud if we found a duration-matched result
                    // Don't blindly download the first SoundCloud result
                    let sc_download_url = match sc_best_url {
                        Some(url) => url,
                        None => {
                            log::info!("SoundCloud search: no duration-matched result, skipping blind download");
                            errors.push("SoundCloud search: no matching result".to_string());
                            // Skip to the "all fallbacks failed" handler below
                            fail_download(&db, &app_handle, download_id, &errors.join("; "));
                            return;
                        }
                    };

                    let app_handle_sc = app_handle.clone();
                    let dl_id_sc = download_id;
                    let sc_result = ytdlp::download_audio(
                        &ytdlp_binary,
                        ffmpeg_dir.as_deref(),
                        &sc_download_url,
                        &download_dir,
                        &download.format,
                        &download.quality,
                        &file_stem,
                        cookies_from_browser.as_deref(),
                        move |progress| {
                            emit_event(&app_handle_sc, dl_id_sc, "downloading", progress.percent, progress.speed.clone(), progress.eta.clone(), None, None);
                        },
                    )
                    .await;

                    match sc_result {
                        Ok(file_path) => {
                            log::info!("SoundCloud search succeeded");
                            handle_download_success(&db, &app_handle, download_id, &file_path, &ytdlp_info).await;
                            return;
                        }
                        Err(e) => {
                            errors.push(format!("SoundCloud search: {}", e));
                        }
                    }
                }

                // If all fallbacks failed, mark as failed
                fail_download(&db, &app_handle, download_id, &errors.join("; "));
    }
}

async fn handle_download_success(
    db: &Arc<DbPool>,
    app_handle: &tauri::AppHandle,
    download_id: i64,
    file_path: &str,
    ytdlp_info: &Option<ytdlp::VideoInfo>,
) {
    if let Ok(conn) = db.lock() {
        let _ = crate::db::downloads::update_download_status(
            &conn,
            download_id,
            "processing",
            None,
        );
    }
    emit_event(
        app_handle,
        download_id,
        "processing",
        100.0,
        None,
        None,
        None,
        None,
    );

    // Build fallback metadata from the download record + yt-dlp info
    let dl_meta = {
        let dl_row = if let Ok(conn) = db.lock() {
            conn.query_row(
                "SELECT title, artist, url, target_album_id, target_artist_id, target_isrc, target_disc_number, target_track_number, target_duration_ms, target_album_name FROM downloads WHERE id = ?1",
                rusqlite::params![download_id],
                |row| Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                    row.get::<_, Option<i64>>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<i64>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                    row.get::<_, Option<i64>>(8)?,
                    row.get::<_, Option<String>>(9)?,
                )),
            ).ok()
        } else {
            None
        };
        let (title, artist, source_url, target_album_id, target_artist_id, target_isrc, target_disc_number, target_track_number, target_duration_ms, target_album_name) = match dl_row {
            Some((t, a, u, alb, art, isrc, disc, track, dur, alb_name)) => (t, a, Some(u), alb, art, isrc, disc, track, dur, alb_name),
            None => (None, None, None, None, None, None, None, None, None, None),
        };
        DownloadMeta {
            title,
            artist,
            album: ytdlp_info.as_ref().and_then(|i| i.album.clone()),
            source_url,
            description: ytdlp_info.as_ref().and_then(|i| i.description.clone()),
            genre: ytdlp_info.as_ref().and_then(|i| i.genre.clone()),
            release_year: ytdlp_info.as_ref().and_then(|i| i.release_year.clone()),
            language: ytdlp_info.as_ref().and_then(|i| i.language.clone()),
            composer: ytdlp_info.as_ref().and_then(|i| i.composer.clone()),
            tags: ytdlp_info.as_ref().and_then(|i| {
                i.tags.as_ref().map(|t| serde_json::to_string(t).unwrap_or_default())
            }),
            channel_url: ytdlp_info.as_ref().and_then(|i| i.channel_url.clone()),
            target_album_id,
            target_artist_id,
            isrc: ytdlp_info.as_ref().and_then(|i| i.isrc.clone()).or(target_isrc),
            target_disc_number,
            target_track_number,
            target_duration_ms,
            target_album_name,
        }
    };
    let (track_id, file_path) = match import_downloaded_file(db, app_handle, file_path, &dl_meta).await {
        ImportOutcome::Imported { track_id, file_path } => (track_id, file_path),
        ImportOutcome::WrongSong { actual_ms, expected_ms } => {
            // Delete the wrong file and fail the download (fail & flag) so the library
            // never contains the wrong song. The user can retry to pick another source.
            let _ = std::fs::remove_file(file_path);
            fail_download(
                db,
                app_handle,
                download_id,
                &format!(
                    "Wrong song rejected: downloaded duration {}s does not match expected {}s",
                    actual_ms.map(|ms| ms / 1000).unwrap_or(0),
                    expected_ms / 1000,
                ),
            );
            return;
        }
    };

    // Batch all post-completion DB updates in a single lock scope
    let entry_id = if let Ok(conn) = db.lock() {
        let _ = conn.execute_batch("BEGIN");
        let _ = crate::db::downloads::update_download_file(
            &conn,
            download_id,
            &file_path,
            track_id,
        );
        let _ = crate::db::downloads::update_download_status(
            &conn,
            download_id,
            "completed",
            None,
        );
        let eid: Option<i64> = conn.query_row(
            "SELECT id FROM monitored_playlist_entries WHERE download_id = ?1",
            rusqlite::params![download_id],
            |row| row.get(0),
        ).ok();
        if let Some(eid) = eid {
            let _ = crate::db::monitored::update_entry_status(
                &conn, eid, "downloaded", Some(download_id), track_id,
            );
            // Auto-add downloaded track to the library playlist
            if let Some(tid) = track_id {
                // Find the playlist_id for this entry
                if let Ok(playlist_id) = conn.query_row(
                    "SELECT playlist_id FROM monitored_playlist_entries WHERE id = ?1",
                    rusqlite::params![eid],
                    |row| row.get::<_, i64>(0),
                ) {
                    let _ = crate::db::playlists::add_track_to_playlist(&conn, playlist_id, tid);
                }
            }
        }
        let _ = conn.execute_batch("COMMIT");
        eid
    } else {
        None
    };
    let _ = app_handle.emit(
        "download-event",
        DownloadEvent {
            id: download_id,
            status: "completed".into(),
            progress: 100.0,
            speed: None,
            eta: None,
            error: None,
            title: None,
            track_id,
        },
    );
    // Notify frontend that the library has new content
    if track_id.is_some() {
        let _ = app_handle.emit("library-updated", ());
    }
    if let Some(eid) = entry_id {
        let _ = app_handle.emit(
            "manager-entry-updated",
            serde_json::json!({ "entry_id": eid, "status": "downloaded" }),
        );
    }
}

pub(super) fn fail_download(db: &Arc<DbPool>, app_handle: &tauri::AppHandle, id: i64, error: &str) {
    log::error!("Download {} failed: {}", id, error);
    let entry_id = if let Ok(conn) = db.lock() {
        let _ = crate::db::downloads::update_download_status(&conn, id, "failed", Some(error));
        // Update linked monitored entry
        let eid: Option<i64> = conn.query_row(
            "SELECT id FROM monitored_playlist_entries WHERE download_id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        ).ok();
        if let Some(eid) = eid {
            let _ = crate::db::monitored::update_entry_status(&conn, eid, "failed", Some(id), None);
        }
        eid
    } else {
        None
    };
    let _ = app_handle.emit(
        "download-event",
        DownloadEvent {
            id,
            status: "failed".into(),
            progress: 0.0,
            speed: None,
            eta: None,
            error: Some(error.to_string()),
            title: None,
            track_id: None,
        },
    );
    if let Some(eid) = entry_id {
        let _ = app_handle.emit(
            "manager-entry-updated",
            serde_json::json!({ "entry_id": eid, "status": "failed" }),
        );
    }
}
