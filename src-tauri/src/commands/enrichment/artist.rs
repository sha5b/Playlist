//! Artist enrichment and image fetching (Deezer first, Last.fm fallback).

use std::sync::Arc;
use rusqlite::params;
use tauri::{Manager, State};

use crate::db::DbPool;

/// Fetch artist image bytes: Deezer first (Last.fm's artist.getinfo has only
/// returned a placeholder star since 2019), then any usable Last.fm URL.
pub(crate) async fn fetch_artist_image_bytes(
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
    use crate::commands::enrichment::maintenance::primary_artist_key;

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
