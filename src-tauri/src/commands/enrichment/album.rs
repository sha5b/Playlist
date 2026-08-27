//! Album enrichment (MusicBrainz + Last.fm) and album mismatch detection.

use std::sync::Arc;
use rusqlite::params;
use tauri::{Manager, State};

use crate::db::DbPool;

use super::artist::fetch_artist_image_bytes;

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
    // Pass 1 — exact membership across ALL families first, so blended genres
    // that are explicitly listed ("country rock" → country, not rock) and
    // short genres that appear inside longer family words ("funk" inside
    // "g-funk") resolve to their intended family.
    if let Some(i) = GENRE_FAMILIES.iter().position(|family| family.contains(&lower.as_str())) {
        return Some(i);
    }
    // Pass 2 — one-way substring: classify unlisted multi-word genres by the
    // family word they contain ("post-rock" → rock). The reverse direction
    // (family word containing the genre) matched "funk" to hip-hop via
    // "g-funk" and produced bogus mismatch reports.
    GENRE_FAMILIES.iter().position(|family| {
        family.iter().any(|g| lower != *g && lower.contains(g))
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
