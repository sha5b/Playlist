//! Import a downloaded file into the library: verification, tagging,
//! and on-disk organization into Artist/Album/NN - Title.ext.

use std::sync::Arc;

use tauri::Manager;

use crate::db::DbPool;
use crate::metadata::tags;

use super::search::{is_youtube_category, split_title_artist};

/// Metadata from the download record + yt-dlp to use as fallback when file tags are missing
pub(super) struct DownloadMeta {
    pub(super) title: Option<String>,
    pub(super) artist: Option<String>,
    pub(super) album: Option<String>,
    pub(super) source_url: Option<String>,
    pub(super) description: Option<String>,
    pub(super) genre: Option<String>,
    pub(super) release_year: Option<String>,
    pub(super) language: Option<String>,
    pub(super) composer: Option<String>,
    pub(super) tags: Option<String>,
    pub(super) channel_url: Option<String>,
    pub(super) target_album_id: Option<i64>,
    pub(super) target_artist_id: Option<i64>,
    pub(super) isrc: Option<String>,
    pub(super) target_disc_number: Option<i64>,
    pub(super) target_track_number: Option<i64>,
    pub(super) target_duration_ms: Option<i64>,
    pub(super) target_album_name: Option<String>,
}

/// Result of importing a downloaded file into the library.
pub(super) enum ImportOutcome {
    /// File imported. `track_id` is present unless the DB insert itself failed.
    /// `file_path` is the final on-disk path (files are moved into an
    /// Artist/Album/NN - Title structure after tagging).
    Imported { track_id: Option<i64>, file_path: String },
    /// The file's duration doesn't match the expected track — wrong song; the
    /// caller should delete the file and fail the download.
    WrongSong { actual_ms: Option<u64>, expected_ms: i64 },
}

/// Sanitize one path component (folder or filename) for cross-platform filesystems.
fn sanitize_component(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    let t = s.trim().trim_end_matches('.').trim();
    if t.is_empty() { "Unknown".to_string() } else { t.to_string() }
}

/// Move a downloaded file into `<download_dir>/<Artist>/<Album>/<NN - Title>.ext`
/// (same structure as the library export). Returns the new path, or the original
/// path unchanged on any failure.
fn organize_file(
    current: &std::path::Path,
    artist: &str,
    album: &str,
    disc: Option<i64>,
    track: Option<i64>,
    title: &str,
) -> std::path::PathBuf {
    let download_dir = match current.parent() {
        Some(p) => p,
        None => return current.to_path_buf(),
    };
    let ext = current.extension().and_then(|e| e.to_str()).unwrap_or("mp3");
    let prefix = match (disc, track) {
        (Some(d), Some(n)) if d > 1 => format!("{}-{:02}", d, n),
        (_, Some(n)) => format!("{:02}", n),
        _ => String::new(),
    };
    let safe_title = sanitize_component(title);
    let base = if prefix.is_empty() {
        safe_title.clone()
    } else {
        format!("{} - {}", prefix, safe_title)
    };
    let target_dir = download_dir
        .join(sanitize_component(artist))
        .join(sanitize_component(album));
    if std::fs::create_dir_all(&target_dir).is_err() {
        return current.to_path_buf();
    }
    let mut target = target_dir.join(format!("{}.{}", base, ext));
    if target != current && target.exists() {
        if prefix.is_empty() {
            // No track number: disambiguate with the unique download stem so distinct
            // singles that share a title don't clobber each other.
            let stem = current.file_stem().and_then(|s| s.to_str()).unwrap_or("dl");
            target = target_dir.join(format!("{} [{}].{}", base, stem, ext));
        } else {
            // Same album position: replace the previous file for this track.
            let _ = std::fs::remove_file(&target);
        }
    }
    match std::fs::rename(current, &target) {
        Ok(_) => target,
        Err(_) => {
            // Different filesystem — fall back to copy + delete.
            if std::fs::copy(current, &target).is_ok() {
                let _ = std::fs::remove_file(current);
                target
            } else {
                current.to_path_buf()
            }
        }
    }
}

pub(super) async fn import_downloaded_file(
    db: &Arc<DbPool>,
    app_handle: &tauri::AppHandle,
    file_path: &str,
    dl_meta: &DownloadMeta,
) -> ImportOutcome {
    let path = std::path::Path::new(file_path);
    if !path.exists() {
        log::warn!("Downloaded file not found: {}", file_path);
        return ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() };
    }

    // Run sync I/O (tag reading, cover extraction) off the async runtime
    let path_buf = path.to_path_buf();
    let covers_dir = match app_handle.path().app_data_dir() {
        Ok(d) => d.join("covers"),
        Err(_) => return ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() },
    };
    let covers_dir_clone = covers_dir.clone();
    let (tag_data, cover_art_path) = match tokio::task::spawn_blocking(move || {
        // Single file read for both tags and cover art (halves memory usage)
        match tags::read_tags_and_cover(&path_buf, &covers_dir_clone) {
            Ok((data, cover)) => (data, cover),
            Err(e) => {
                log::warn!("Failed to read tags from downloaded file: {}", e);
                (tags::TagData::default(), None)
            }
        }
    }).await {
        Ok(v) => v,
        Err(_) => return ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() },
    };

    // Post-download verification (align with metadata): compare the downloaded file's
    // actual duration against the expected duration from MusicBrainz/Deezer. If they
    // differ beyond tolerance we grabbed the wrong song — reject it (fail & flag) rather
    // than import incorrect audio. Applies to ALL downloads with a known expected duration.
    if let (Some(expected_ms), Some(actual_ms)) = (dl_meta.target_duration_ms, tag_data.duration_ms) {
        if expected_ms > 0 {
            let diff_ms = (expected_ms - actual_ms as i64).unsigned_abs();
            let tolerance_ms = ((expected_ms as f64 * 0.12) as u64).max(12_000);
            if diff_ms > tolerance_ms {
                log::warn!(
                    "Post-download duration mismatch for '{}': expected {}ms, got {}ms (diff {}ms > {}ms) — rejecting wrong song",
                    dl_meta.title.as_deref().unwrap_or("?"), expected_ms, actual_ms, diff_ms, tolerance_ms
                );
                return ImportOutcome::WrongSong { actual_ms: tag_data.duration_ms, expected_ms };
            }
        }
    }
    let is_album_download = dl_meta.target_album_id.is_some();

    // For album track downloads (target_album_id set), use the download metadata as the
    // authoritative source — we know the correct title/artist from MusicBrainz.
    // YouTube file tags contain the video title which is often wrong/noisy.
    // For other downloads, prefer file tags and fall back to download metadata.
    let raw_title = if is_album_download {
        dl_meta.title.clone()
            .or(tag_data.title)
            .unwrap_or_else(|| "Unknown".to_string())
    } else {
        tag_data.title
            .or_else(|| dl_meta.title.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    };
    let raw_artist = if is_album_download {
        dl_meta.artist.clone()
            .or(tag_data.artist)
    } else {
        tag_data.artist
            .or_else(|| dl_meta.artist.clone())
    };

    let (title, artist_name) = split_title_artist(&raw_title, raw_artist.as_deref());

    let conn = match db.lock() {
        Ok(c) => c,
        Err(_) => return ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() },
    };

    // Use target IDs from download context if available (e.g., downloading missing album tracks),
    // otherwise fall back to find_or_create from file tags
    let artist_id = if let Some(target_aid) = dl_meta.target_artist_id {
        Some(target_aid)
    } else {
        artist_name
            .as_ref()
            .and_then(|name| crate::db::artists::find_or_create(&conn, name).ok())
    };

    let album_id = if let Some(target_alb) = dl_meta.target_album_id {
        Some(target_alb)
    } else {
        // Try file tags first, then fall back to yt-dlp album metadata
        let album_name = tag_data.album.as_ref().or(dl_meta.album.as_ref());
        album_name.and_then(|alb| {
            crate::db::albums::find_or_create(
                &conn,
                alb,
                artist_id,
                tag_data.album_artist.as_deref(),
                tag_data.year.map(|y| y as i64),
            )
            .ok()
        })
    };

    let file_size = std::fs::metadata(path).map(|m| m.len() as i64).ok();

    // Save values needed after INSERT (before they get moved into params)
    let total_tracks_val = tag_data.total_tracks.map(|t| t as i64);
    let total_discs_val = tag_data.total_discs.map(|d| d as i64);
    let genre_for_album = tag_data.genre.clone();

    // Merge yt-dlp fallback fields with file tags (file tags win).
    // Filter out YouTube categories that aren't real music genres.
    let genre = tag_data.genre.or_else(|| dl_meta.genre.clone())
        .filter(|g| !is_youtube_category(g));
    let year = tag_data.year.map(|y| y as i64)
        .or_else(|| dl_meta.release_year.as_ref().and_then(|y| y.parse::<i64>().ok()));
    let description = dl_meta.description.clone();
    let language = dl_meta.language.clone();
    let composer = dl_meta.composer.clone();
    let release_date = dl_meta.release_year.clone();

    let tags = dl_meta.tags.clone();
    let isrc = dl_meta.isrc.clone();

    let track_number = if is_album_download { dl_meta.target_track_number.or(tag_data.track_number.map(|t| t as i64)) } else { tag_data.track_number.map(|t| t as i64).or(dl_meta.target_track_number) };
    // For album tracks, default a missing disc number to 1 so the dedup lookup below
    // (which uses disc = 1) matches the value we actually store (B3 fix).
    let disc_number = if is_album_download {
        dl_meta.target_disc_number.or(tag_data.disc_number.map(|d| d as i64)).or(Some(1))
    } else {
        tag_data.disc_number.map(|d| d as i64).or(dl_meta.target_disc_number)
    };

    // For album downloads, check if a track already exists at this position (e.g. a placeholder
    // or previously downloaded track). If so, update it in-place instead of creating a duplicate.
    let existing_track_id: Option<i64> = if is_album_download {
        if let (Some(alb_id), Some(tn)) = (album_id, track_number) {
            let dn = disc_number.unwrap_or(1);
            conn.query_row(
                "SELECT id FROM tracks WHERE album_id = ?1 AND track_number = ?2 AND disc_number = ?3",
                rusqlite::params![alb_id, tn, dn],
                |row| row.get(0),
            ).ok()
        } else {
            None
        }
    } else {
        None
    };

    let result = if let Some(existing_id) = existing_track_id {
        // Update the existing track row with the downloaded file's data
        conn.execute(
            "UPDATE tracks SET title = ?1, artist_id = ?2, album_artist = ?3, duration_ms = ?4,
                genre = ?5, year = ?6, file_path = ?7, file_size = ?8, format = ?9,
                bitrate = ?10, sample_rate = ?11, channels = ?12, cover_art_path = ?13,
                source_platform = 'download', source_url = ?14,
                description = ?15, language = ?16, composer = ?17, release_date = ?18, tags = ?19, isrc = ?20
             WHERE id = ?21",
            rusqlite::params![
                title,
                artist_id,
                tag_data.album_artist,
                tag_data.duration_ms.map(|d| d as i64),
                genre,
                year,
                file_path,
                file_size,
                tag_data.format,
                tag_data.bitrate.map(|b| b as i64),
                tag_data.sample_rate.map(|s| s as i64),
                tag_data.channels.map(|c| c as i64),
                cover_art_path,
                dl_meta.source_url,
                description,
                language,
                composer,
                release_date,
                tags,
                isrc,
                existing_id,
            ],
        ).map(|_| existing_id)
    } else {
        conn.execute(
            "INSERT INTO tracks (title, artist_id, album_id, album_artist, duration_ms,
                track_number, disc_number, genre, year, file_path, file_size, format,
                bitrate, sample_rate, channels, cover_art_path, source_platform, source_url,
                description, language, composer, release_date, tags, isrc)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, 'download', ?17, ?18, ?19, ?20, ?21, ?22, ?23)",
            rusqlite::params![
                title,
                artist_id,
                album_id,
                tag_data.album_artist,
                tag_data.duration_ms.map(|d| d as i64),
                track_number,
                disc_number,
                genre,
                year,
                file_path,
                file_size,
                tag_data.format,
                tag_data.bitrate.map(|b| b as i64),
                tag_data.sample_rate.map(|s| s as i64),
                tag_data.channels.map(|c| c as i64),
                cover_art_path,
                dl_meta.source_url,
                description,
                language,
                composer,
                release_date,
                tags,
                isrc,
            ],
        ).map(|_| conn.last_insert_rowid())
    };

    match result {
        Ok(track_id) => {
            let _ = crate::db::tracks::update_fts(&conn, track_id);
            let _ = crate::db::tracks::update_completeness(&conn, track_id);

            // Propagate channel_url to artist website_url
            if let Some(ref url) = dl_meta.channel_url {
                if let Some(aid) = artist_id {
                    let _ = conn.execute(
                        "UPDATE artists SET website_url = ?1 WHERE id = ?2 AND website_url IS NULL",
                        rusqlite::params![url, aid],
                    );
                }
            }

            // Propagate cover art to the album only. Do NOT use the embedded cover as the
            // artist image — it's album/track art, not an artist photo, and because
            // update_image_if_missing only writes when NULL it would permanently block the
            // real artist photo that enrichment (Last.fm) fetches later (fix A1).
            if let Some(ref cover) = cover_art_path {
                if let Some(aid) = album_id {
                    let _ = crate::db::albums::update_cover_art_if_missing(&conn, aid, cover);
                }
            }
            // Propagate album metadata from tags
            if let Some(aid) = album_id {
                let _ = crate::db::albums::update_metadata_if_missing(
                    &conn,
                    aid,
                    total_tracks_val,
                    total_discs_val,
                    genre_for_album.as_deref(),
                );
            }

            // --- Organize the file into Artist/Album/NN - Title.ext and write correct
            // tags, so downloads are self-describing offline (like the library export). ---
            let album_name_str = tag_data.album.clone()
                .or_else(|| dl_meta.album.clone())
                .or_else(|| dl_meta.target_album_name.clone())
                .unwrap_or_else(|| "Unknown Album".to_string());
            let artist_for_path = artist_name.clone().unwrap_or_else(|| "Unknown Artist".to_string());

            let final_path = organize_file(
                std::path::Path::new(file_path),
                &artist_for_path,
                &album_name_str,
                disc_number,
                track_number,
                &title,
            );

            let tw = tags::TagWrite {
                title: Some(title.clone()),
                artist: artist_name.clone(),
                album: Some(album_name_str),
                album_artist: artist_name.clone(),
                track_number: track_number.map(|n| n as u32),
                disc_number: disc_number.map(|d| d as u32),
                year: year.map(|y| y as u32),
                genre: genre_for_album.clone(),
            };
            if let Err(e) = tags::write_tags(&final_path, &tw) {
                log::warn!("Failed to write tags to {:?}: {}", final_path, e);
            }

            let final_path_str = final_path.to_string_lossy().to_string();
            if final_path_str.as_str() != file_path {
                let _ = conn.execute(
                    "UPDATE tracks SET file_path = ?1 WHERE id = ?2",
                    rusqlite::params![final_path_str, track_id],
                );
            }

            log::info!("Imported downloaded track: {} (id={}) -> {}", title, track_id, final_path_str);
            ImportOutcome::Imported { track_id: Some(track_id), file_path: final_path_str }
        }
        Err(e) => {
            log::warn!("Failed to insert downloaded track: {}", e);
            ImportOutcome::Imported { track_id: None, file_path: file_path.to_string() }
        }
    }
}
