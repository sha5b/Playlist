//! Last.fm API for fetching tags, artist images, and supplementary metadata.
//! Uses the public API (no key required for some endpoints, but API key gives better access).
//! We use a shared community key pattern — or fallback to MusicBrainz tags only.

use serde::Deserialize;

// Last.fm API key — use env var at compile time, fall back to shared community key.
// An empty env var (e.g. an unset CI secret expanding to "") counts as unset.
pub(crate) const LASTFM_API_KEY: &str = match option_env!("LASTFM_API_KEY") {
    Some(key) if !key.is_empty() => key,
    _ => "8fc89f699e4ff452a53bb4aab795e4e7",
};
const LASTFM_BASE: &str = "https://ws.audioscrobbler.com/2.0/";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Playlist/0.1.0")
        .build()
        .unwrap_or_default()
}

// ── Response types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct AlbumInfoResponse {
    album: Option<AlbumInfo>,
}

#[derive(Debug, Deserialize)]
struct AlbumInfo {
    tags: Option<TagsContainer>,
    image: Option<Vec<LastfmImage>>,
    wiki: Option<Wiki>,
}

#[derive(Debug, Deserialize)]
struct ArtistInfoResponse {
    artist: Option<ArtistInfo>,
}

#[derive(Debug, Deserialize)]
struct ArtistInfo {
    bio: Option<Bio>,
    image: Option<Vec<LastfmImage>>,
}

#[derive(Debug, Deserialize)]
struct Bio {
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TagsContainer {
    tag: Option<Vec<LastfmTag>>,
}

#[derive(Debug, Deserialize)]
struct LastfmTag {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LastfmImage {
    #[serde(rename = "#text")]
    url: Option<String>,
    size: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Wiki {
    summary: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TrackInfoResponse {
    track: Option<TrackInfo>,
}

#[derive(Debug, Deserialize)]
struct TrackInfo {
    tags: Option<TagsContainer>,
    wiki: Option<Wiki>,
}

// ── Public result types ───────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct LastfmAlbumData {
    pub tags: Vec<String>,
    pub description: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct LastfmArtistData {
    pub bio: Option<String>,
    pub image_url: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct LastfmTrackData {
    pub tags: Vec<String>,
    pub description: Option<String>,
}

// ── Public API ────────────────────────────────────────────────────────────

/// Fetch album info from Last.fm
pub async fn get_album_info(album: &str, artist: &str) -> Result<LastfmAlbumData, String> {
    let url = format!(
        "{}?method=album.getinfo&api_key={}&artist={}&album={}&format=json",
        LASTFM_BASE, LASTFM_API_KEY, urlencoding(artist), urlencoding(album)
    );

    let resp: AlbumInfoResponse = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Last.fm request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse Last.fm response: {}", e))?;

    let info = resp.album.ok_or_else(|| "No Last.fm album data".to_string())?;

    let tags = info.tags
        .and_then(|t| t.tag)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| t.name)
        .collect();

    let description = info.wiki
        .and_then(|w| w.summary)
        .map(|s| strip_html_tags(&s))
        .filter(|s| !s.is_empty());

    let image_url = get_best_image(&info.image);

    Ok(LastfmAlbumData { tags, description, image_url })
}

/// Fetch artist info from Last.fm
pub async fn get_artist_info(artist: &str) -> Result<LastfmArtistData, String> {
    let url = format!(
        "{}?method=artist.getinfo&api_key={}&artist={}&format=json",
        LASTFM_BASE, LASTFM_API_KEY, urlencoding(artist)
    );

    let resp: ArtistInfoResponse = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Last.fm request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse Last.fm response: {}", e))?;

    let info = resp.artist.ok_or_else(|| "No Last.fm artist data".to_string())?;

    let bio = info.bio
        .and_then(|b| b.summary)
        .map(|s| strip_html_tags(&s))
        .filter(|s| !s.is_empty());

    let image_url = get_best_image(&info.image);

    Ok(LastfmArtistData { bio, image_url })
}

/// Fetch track info from Last.fm
pub async fn get_track_info(track: &str, artist: &str) -> Result<LastfmTrackData, String> {
    let url = format!(
        "{}?method=track.getinfo&api_key={}&artist={}&track={}&format=json",
        LASTFM_BASE, LASTFM_API_KEY, urlencoding(artist), urlencoding(track)
    );

    let resp: TrackInfoResponse = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Last.fm request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse Last.fm response: {}", e))?;

    let info = resp.track.ok_or_else(|| "No Last.fm track data".to_string())?;

    let tags = info.tags
        .and_then(|t| t.tag)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| t.name)
        .collect();

    let description = info.wiki
        .and_then(|w| w.summary)
        .map(|s| strip_html_tags(&s))
        .filter(|s| !s.is_empty());

    Ok(LastfmTrackData { tags, description })
}

/// Download image bytes from a URL
pub async fn download_image(url: &str) -> Option<Vec<u8>> {
    match client().get(url).send().await {
        Ok(resp) if resp.status().is_success() => {
            resp.bytes().await.ok().map(|b| b.to_vec())
        }
        _ => None,
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────

/// Last.fm stopped serving real artist images in 2019 — artist.getinfo now
/// returns this generic "star" placeholder for every artist. Treat it as no image.
const LASTFM_PLACEHOLDER_HASH: &str = "2a96cbd8b46e442fc41c2b86b821562f";

fn is_usable_image_url(url: &str) -> bool {
    !url.is_empty() && !url.contains(LASTFM_PLACEHOLDER_HASH)
}

fn get_best_image(images: &Option<Vec<LastfmImage>>) -> Option<String> {
    let imgs = images.as_ref()?;
    // Prefer extralarge > large > medium
    for size in &["extralarge", "large", "medium"] {
        if let Some(img) = imgs.iter().find(|i| i.size.as_deref() == Some(size)) {
            if let Some(url) = &img.url {
                if is_usable_image_url(url) {
                    return Some(url.clone());
                }
            }
        }
    }
    // Fallback: last image (usually largest)
    imgs.last()
        .and_then(|i| i.url.clone())
        .filter(|u| is_usable_image_url(u))
}

fn strip_html_tags(s: &str) -> String {
    // Simple HTML tag stripping — good enough for Last.fm wiki summaries
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    result.trim().to_string()
}

pub(crate) fn urlencoding(s: &str) -> String {
    // Encode each UTF-8 byte (not Unicode code point) for correct percent-encoding
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}
