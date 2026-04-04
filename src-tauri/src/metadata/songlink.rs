use std::collections::HashMap;

use serde::Deserialize;

#[derive(Deserialize)]
struct SongLinkResponse {
    #[serde(rename = "linksByPlatform")]
    links_by_platform: Option<HashMap<String, PlatformLink>>,
}

#[derive(Deserialize)]
struct PlatformLink {
    url: Option<String>,
}

/// Resolve an ISRC to a YouTube URL via the SongLink/Odesli API.
/// Returns `None` on any failure (timeout, network error, no result).
pub async fn resolve_isrc(isrc: &str) -> Option<String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;

    let url = format!(
        "https://api.song.link/v1-alpha.1/links?isrc={}&platform=youtube",
        isrc
    );

    log::info!("SongLink: resolving ISRC {} → YouTube URL", isrc);

    let resp = client.get(&url).send().await.ok()?;
    if !resp.status().is_success() {
        log::warn!("SongLink: HTTP {} for ISRC {}", resp.status(), isrc);
        return None;
    }

    let data: SongLinkResponse = resp.json().await.ok()?;
    let links = data.links_by_platform?;

    // Prefer YouTube Music, fall back to YouTube
    let yt_url = links
        .get("youtubeMusic")
        .and_then(|l| l.url.clone())
        .or_else(|| links.get("youtube").and_then(|l| l.url.clone()));

    if let Some(ref u) = yt_url {
        log::info!("SongLink: resolved ISRC {} → {}", isrc, u);
    } else {
        log::warn!("SongLink: no YouTube link found for ISRC {}", isrc);
    }

    yt_url
}
