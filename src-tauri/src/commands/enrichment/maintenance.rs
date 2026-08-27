//! Library maintenance: metadata deletion and duplicate cleanup.

use std::sync::Arc;
use rusqlite::params;
use tauri::State;

use crate::db::DbPool;

use super::scan::METADATA_SCAN_CANCELLED;
use std::sync::atomic::Ordering;

#[tauri::command]
pub fn metadata_delete_all(
    db: State<'_, Arc<DbPool>>,
) -> Result<(), String> {
    // Stop any running scan first
    METADATA_SCAN_CANCELLED.store(true, Ordering::Relaxed);

    let conn = crate::db::lock(&db)?;
    conn.execute_batch("
        UPDATE tracks SET musicbrainz_id=NULL, genre=NULL, isrc=NULL, description=NULL,
            label=NULL, language=NULL, release_date=NULL, composer=NULL,
            year=NULL, tags=NULL, lyrics=NULL;
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
pub(crate) fn primary_artist_key(name: Option<&str>) -> Option<String> {
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
