//! Lyrics fetching via LRCLIB (https://lrclib.net) — free, no API key required.

use serde::Deserialize;

const LRCLIB_BASE: &str = "https://lrclib.net/api";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Playlist/0.1.0")
        .build()
        .unwrap_or_default()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct LrclibResponse {
    synced_lyrics: Option<String>,
    plain_lyrics: Option<String>,
    track_name: Option<String>,
    artist_name: Option<String>,
}

/// Search LRCLIB for lyrics. Tries exact match first, then fuzzy search.
pub async fn fetch_lyrics(
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
) -> Result<String, String> {
    // Try exact match first
    if let Ok(lyrics) = fetch_exact(title, artist, duration_secs).await {
        return Ok(lyrics);
    }

    // Fall back to fuzzy search
    fetch_search(title, artist).await
}

async fn fetch_exact(
    title: &str,
    artist: &str,
    duration_secs: Option<f64>,
) -> Result<String, String> {
    let mut url = format!(
        "{}/get?track_name={}&artist_name={}",
        LRCLIB_BASE,
        urlencoding(title),
        urlencoding(artist),
    );
    if let Some(dur) = duration_secs {
        url.push_str(&format!("&duration={}", dur.round() as i64));
    }

    let resp = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("LRCLIB request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err("No lyrics found".to_string());
    }

    let data: LrclibResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse LRCLIB response: {}", e))?;

    extract_lyrics(data)
}

async fn fetch_search(title: &str, artist: &str) -> Result<String, String> {
    let query = format!("{} {}", artist, title);
    let url = format!("{}/search?q={}", LRCLIB_BASE, urlencoding(&query));

    let resp = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("LRCLIB search failed: {}", e))?;

    if !resp.status().is_success() {
        return Err("No lyrics found".to_string());
    }

    let results: Vec<LrclibResponse> = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse LRCLIB search response: {}", e))?;

    // Pick the first result whose track/artist actually match — the search
    // endpoint is fuzzy, and blindly taking the first hit stored the wrong
    // song's lyrics permanently (updates only fill NULL fields).
    for result in results {
        let title_ok = result.track_name.as_deref()
            .map(|t| normalized_match(t, title))
            .unwrap_or(false);
        let artist_ok = result.artist_name.as_deref()
            .map(|a| normalized_match(a, artist))
            .unwrap_or(false);
        if title_ok && artist_ok {
            if let Ok(lyrics) = extract_lyrics(result) {
                return Ok(lyrics);
            }
        }
    }

    Err("No lyrics found".to_string())
}

/// Case/punctuation-insensitive containment match in either direction
/// (handles "Song (Remastered)" vs "Song" and "Artist feat. X" vs "Artist").
fn normalized_match(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect::<String>()
    };
    let (na, nb) = (norm(a), norm(b));
    if na.is_empty() || nb.is_empty() {
        return false;
    }
    na.contains(&nb) || nb.contains(&na)
}

fn extract_lyrics(data: LrclibResponse) -> Result<String, String> {
    data.synced_lyrics
        .or(data.plain_lyrics)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "No lyrics found".to_string())
}

fn urlencoding(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}
