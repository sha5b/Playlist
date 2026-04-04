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

    // Pick the first result that has lyrics, preferring synced
    for result in results {
        if let Ok(lyrics) = extract_lyrics(result) {
            return Ok(lyrics);
        }
    }

    Err("No lyrics found".to_string())
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
