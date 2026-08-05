//! Deezer public API — used for artist images.
//! No API key required. Last.fm's artist.getinfo has returned a generic
//! placeholder star image (or nothing) since 2019, so Deezer is the primary
//! source for artist images.

use serde::Deserialize;

const DEEZER_SEARCH_ARTIST: &str = "https://api.deezer.com/search/artist";

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Option<Vec<DeezerArtist>>,
}

#[derive(Debug, Deserialize)]
struct DeezerArtist {
    name: Option<String>,
    picture_xl: Option<String>,
    picture_big: Option<String>,
    picture_medium: Option<String>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Playlist/0.1.0")
        .build()
        .unwrap_or_default()
}

/// Search Deezer for an artist and return the best available picture URL.
/// Prefers an exact (case-insensitive) name match among the top results,
/// falling back to the first result. Returns None on any failure — callers
/// treat this as "no image from Deezer" and fall back to Last.fm.
pub async fn get_artist_image_url(name: &str) -> Option<String> {
    let url = format!(
        "{}?q={}&limit=5",
        DEEZER_SEARCH_ARTIST,
        super::lastfm::urlencoding(name)
    );

    let resp: SearchResponse = client().get(&url).send().await.ok()?.json().await.ok()?;
    let artists = resp.data?;

    let target = name.to_lowercase();
    let best = artists
        .iter()
        .find(|a| a.name.as_deref().is_some_and(|n| n.to_lowercase() == target))
        .or_else(|| artists.first())?;

    // Prefer xl > big > medium. Deezer serves a generic placeholder for
    // artists without a photo — its URL contains "/artist//" (empty id
    // segment), so skip those.
    let url = [&best.picture_xl, &best.picture_big, &best.picture_medium]
        .into_iter()
        .filter_map(|u| u.clone())
        .find(|u| !u.is_empty() && !u.contains("/artist//"));
    url
}
