/// Fetch track metadata (title, artist) from Spotify's oEmbed API.
/// Works for any public Spotify URL without authentication.
/// Returns (title, artist) parsed from the oEmbed "title" field which is formatted as "Song - Artist".
pub async fn fetch_track_metadata(url: &str) -> Option<(String, Option<String>)> {
    let oembed_url = format!("https://open.spotify.com/oembed?url={}", url);
    let client = reqwest::Client::new();
    let resp = client.get(&oembed_url).send().await.ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;

    let title = data["title"].as_str()?;

    // Try to extract artist from the HTML embed content
    let html = data["html"].as_str().unwrap_or("");
    let mut artist = None;

    // Parse artist from HTML - look for patterns like 'Song by Artist' or extract from iframe src
    if !html.is_empty() {
        // Try to extract from "Song by Artist" pattern in HTML
        if let Some(by_pos) = html.find(" by ") {
            let after_by = &html[by_pos + 4..];
            if let Some(end_pos) = after_by.find(&['<', '"', '\''][..]) {
                let artist_str = after_by[..end_pos].trim();
                if !artist_str.is_empty() {
                    artist = Some(artist_str.to_string());
                }
            }
        }
    }

    // Fallback: try description field
    if artist.is_none() {
        let description = data["description"].as_str().unwrap_or("");
        if !description.is_empty() {
            artist = Some(description.to_string());
        }
    }

    Some((title.to_string(), artist))
}

/// Parse a track entry from Spotify API JSON into a VideoInfo.
fn parse_spotify_track(track: &serde_json::Value, album_name: Option<&str>) -> Option<super::ytdlp::VideoInfo> {
    let title = track["title"].as_str()
        .or_else(|| track["name"].as_str())?;
    let artist = track["subtitle"].as_str()
        .or_else(|| {
            track["artists"].as_array()
                .and_then(|a| a.first())
                .and_then(|a| a["name"].as_str())
        })
        .map(|s| s.to_string());
    let duration = track["duration"].as_f64()
        .or_else(|| track["duration_ms"].as_f64().map(|ms| ms / 1000.0));
    let track_uri = track["uri"].as_str().unwrap_or("").to_string();

    // ISRC from embed data (if available)
    let isrc = track["external_ids"]["isrc"].as_str()
        .or_else(|| track["isrc"].as_str())
        .map(|s| s.to_string());

    Some(super::ytdlp::VideoInfo {
        title: title.to_string(),
        track: Some(title.to_string()),
        artist: artist.clone(),
        uploader: artist,
        duration,
        thumbnail: None,
        webpage_url: if track_uri.is_empty() { None } else { Some(track_uri) },
        album: album_name.map(|s| s.to_string()),
        genre: None,
        release_year: None,
        description: None,
        track_number: None,
        disc_number: None,
        composer: None,
        language: None,
        tags: None,
        channel_url: None,
        isrc,
    })
}

/// Parse a track from the Spotify Web API response format (different from embed format).
fn parse_api_track(item: &serde_json::Value, fallback_album: Option<&str>) -> Option<super::ytdlp::VideoInfo> {
    // Playlist tracks are wrapped in {track: ...}, album tracks are direct
    let track = if item["track"].is_object() {
        &item["track"]
    } else {
        item
    };

    let title = track["name"].as_str()?;
    let artist = track["artists"].as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["name"].as_str())
        .map(|s| s.to_string());
    let duration = track["duration_ms"].as_f64().map(|ms| ms / 1000.0);
    let track_uri = track["uri"].as_str().unwrap_or("").to_string();
    let album_name = track["album"]["name"].as_str()
        .map(|s| s.to_string())
        .or_else(|| fallback_album.map(|s| s.to_string()));
    // ISRC from Spotify Web API response
    let isrc = track["external_ids"]["isrc"].as_str()
        .map(|s| s.to_string());

    Some(super::ytdlp::VideoInfo {
        title: title.to_string(),
        track: Some(title.to_string()),
        artist: artist.clone(),
        uploader: artist,
        duration,
        thumbnail: None,
        webpage_url: if track_uri.is_empty() { None } else { Some(track_uri) },
        album: album_name,
        genre: None,
        release_year: None,
        description: None,
        track_number: None,
        disc_number: None,
        composer: None,
        language: None,
        tags: None,
        channel_url: None,
        isrc,
    })
}

/// Search the raw HTML for an access token. Spotify may embed it outside of __NEXT_DATA__
/// in script tags or inline JSON configuration.
fn extract_token_from_html(html: &str) -> Option<String> {
    // Search for "accessToken":"<token>" pattern anywhere in the HTML
    let pattern = "\"accessToken\":\"";
    if let Some(start) = html.find(pattern) {
        let token_start = start + pattern.len();
        if let Some(end) = html[token_start..].find('"') {
            let token = &html[token_start..token_start + end];
            if !token.is_empty() && token.len() > 20 {
                return Some(token.to_string());
            }
        }
    }
    None
}

/// Fetch an anonymous access token from Spotify's web player token endpoint.
/// Tries multiple approaches: web player token with cookies, embed token, and cookieless.
async fn fetch_anonymous_token(_client: &reqwest::Client) -> Option<String> {
    let jar_client = reqwest::Client::builder()
        .cookie_store(true)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
        .build()
        .ok()?;

    // Establish session cookies by visiting the main page
    let _ = jar_client.get("https://open.spotify.com/").send().await;

    // Attempt 1: Web player token with session cookies
    let token_urls = [
        "https://open.spotify.com/get_access_token?reason=transport&productType=web_player",
        "https://open.spotify.com/get_access_token?reason=transport&productType=embed",
    ];

    for token_url in &token_urls {
        if let Ok(resp) = jar_client
            .get(*token_url)
            .header("Referer", "https://open.spotify.com/")
            .send()
            .await
        {
            if let Ok(data) = resp.json::<serde_json::Value>().await {
                if let Some(token) = data["accessToken"].as_str() {
                    if !token.is_empty() {
                        log::info!("[spotify] Obtained anonymous access token from {}", token_url);
                        return Some(token.to_string());
                    }
                }
            }
        }
    }

    // Attempt 2: Try without cookies (works in some regions)
    let bare_client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
        .build()
        .ok()?;
    if let Ok(resp) = bare_client
        .get("https://open.spotify.com/get_access_token?reason=transport&productType=web_player")
        .send()
        .await
    {
        if let Ok(data) = resp.json::<serde_json::Value>().await {
            if let Some(token) = data["accessToken"].as_str() {
                if !token.is_empty() {
                    log::info!("[spotify] Obtained anonymous access token (cookieless)");
                    return Some(token.to_string());
                }
            }
        }
    }

    log::warn!("[spotify] All anonymous token acquisition methods failed");
    None
}

/// Fetch playlist entries from Spotify using the public embed/oEmbed API.
/// This doesn't require authentication — works for any public Spotify playlist/album.
///
/// Note: Spotify deprecated username/password auth (forced OAuth2 mid-2025).
/// Native librespot downloads no longer work. Spotify URLs are handled by the
/// fallback chain (yt-dlp metadata extraction → Deezer search → YouTube search).
pub async fn fetch_playlist_entries(url: &str) -> Result<super::ytdlp::PlaylistFetchResult, super::source::SourceError> {
    use super::source::SourceError;

    let url = url.trim();

    // Extract playlist/album ID from URL
    let (item_type, item_id) = if url.contains("/playlist/") {
        let id = url.split("/playlist/").nth(1).unwrap_or("")
            .split('?').next().unwrap_or("")
            .split('/').next().unwrap_or("");
        ("playlist", id)
    } else if url.contains("/album/") {
        let id = url.split("/album/").nth(1).unwrap_or("")
            .split('?').next().unwrap_or("")
            .split('/').next().unwrap_or("");
        ("album", id)
    } else {
        return Err(SourceError::Other("URL is not a Spotify playlist or album".into()));
    };

    if item_id.is_empty() {
        return Err(SourceError::Other("Could not extract ID from Spotify URL".into()));
    }

    // Use Spotify's public embed API to get playlist/album tracks
    // This doesn't require authentication
    let embed_url = format!("https://open.spotify.com/embed/{}/{}", item_type, item_id);
    let client = reqwest::Client::new();

    let resp = client.get(&embed_url)
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36")
        .send()
        .await
        .map_err(|e| SourceError::NetworkError(format!("Failed to fetch Spotify embed: {}", e)))?;

    let html = resp.text().await
        .map_err(|e| SourceError::NetworkError(format!("Failed to read response: {}", e)))?;

    // The embed page contains a <script id="__NEXT_DATA__"> with JSON data including track list
    let json_str = html.split(r#"<script id="__NEXT_DATA__" type="application/json">"#)
        .nth(1)
        .and_then(|s| s.split("</script>").next());

    if let Some(json_str) = json_str {
        if let Ok(data) = serde_json::from_str::<serde_json::Value>(json_str) {
            let entity = &data["props"]["pageProps"]["state"]["data"]["entity"];
            let playlist_name = entity["name"].as_str().map(|s| s.to_string());

            let track_list = if entity["trackList"].is_array() {
                &entity["trackList"]
            } else if entity["tracks"]["items"].is_array() {
                &entity["tracks"]["items"]
            } else {
                return Err(SourceError::Other("Could not find track list in Spotify embed data".into()));
            };

            let mut entries = Vec::new();
            if let Some(tracks) = track_list.as_array() {
                for track in tracks {
                    if let Some(info) = parse_spotify_track(track, playlist_name.as_deref()) {
                        entries.push(info);
                    }
                }
            }

            // Check if there are more tracks than what the embed page returned.
            // The embed page typically only includes ~100 tracks.
            // Use the anonymous access token from the embed data to fetch remaining pages.
            let total_tracks = entity["tracks"]["totalCount"].as_u64()
                .or_else(|| entity["trackCount"].as_u64())
                .or_else(|| entity["tracks"]["total"].as_u64());

            // If we can't determine total, or total > what we have, attempt pagination.
            // When total is unknown, always try -- the loop stops on empty items / null next.
            let should_paginate = match total_tracks {
                Some(total) => (total as usize) > entries.len(),
                None => true,
            };

            log::info!(
                "[spotify] Embed returned {} tracks, detected total: {:?}, will paginate: {}",
                entries.len(), total_tracks, should_paginate
            );

            if should_paginate {
                // Try to extract the access token from the embed data JSON paths,
                // then from the raw HTML, then from the anonymous token endpoint.
                let embed_token = data["props"]["pageProps"]["state"]["data"]["accessToken"].as_str()
                    .or_else(|| data["props"]["pageProps"]["accessToken"].as_str())
                    .map(|s| s.to_string());

                let token = if embed_token.is_some() {
                    log::info!("[spotify] Found token in __NEXT_DATA__ JSON");
                    embed_token
                } else if let Some(html_token) = extract_token_from_html(&html) {
                    log::info!("[spotify] Found token in raw HTML");
                    Some(html_token)
                } else {
                    log::info!("[spotify] No token in embed page, trying anonymous token endpoint");
                    fetch_anonymous_token(&client).await
                };

                if let Some(token) = token {
                    log::info!(
                        "[spotify] Embed returned {}/{:?} tracks, fetching remaining via API",
                        entries.len(), total_tracks
                    );
                    let mut offset = entries.len();
                    let max_offset: usize = total_tracks.map(|t| t as usize).unwrap_or(5000);
                    let api_path = if item_type == "playlist" {
                        format!("https://api.spotify.com/v1/playlists/{}/tracks", item_id)
                    } else {
                        format!("https://api.spotify.com/v1/albums/{}/tracks", item_id)
                    };

                    // Wait before first API call — the embed page load already counts toward rate limits
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                    let mut rate_limit_retries = 0u32;

                    loop {
                        if offset >= max_offset {
                            break;
                        }
                        let api_url = format!("{}?offset={}&limit=100", api_path, offset);
                        let api_resp = client.get(&api_url)
                            .header("Authorization", format!("Bearer {}", token))
                            .send()
                            .await;

                        match api_resp {
                            Ok(resp) => {
                                let status = resp.status();

                                // Handle rate limiting (429) with retry + backoff
                                if status.as_u16() == 429 {
                                    rate_limit_retries += 1;
                                    if rate_limit_retries > 10 {
                                        log::warn!("[spotify] Too many rate limit retries, stopping pagination");
                                        break;
                                    }
                                    let retry_after = resp.headers()
                                        .get("retry-after")
                                        .and_then(|v| v.to_str().ok())
                                        .and_then(|v| v.parse::<u64>().ok())
                                        .unwrap_or(3);
                                    // Use at least the retry-after value, with increasing backoff
                                    let wait = retry_after.max(2 * rate_limit_retries as u64).min(30);
                                    log::info!("[spotify] Rate limited at offset {}, waiting {}s (attempt {}/10)...", offset, wait, rate_limit_retries);
                                    tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                                    continue; // retry same offset
                                }
                                rate_limit_retries = 0; // reset on success

                                let body = resp.text().await.unwrap_or_default();

                                if !status.is_success() {
                                    log::warn!(
                                        "[spotify] API returned HTTP {} at offset {}: {}",
                                        status, offset, &body[..body.len().min(200)]
                                    );
                                    break;
                                }

                                match serde_json::from_str::<serde_json::Value>(&body) {
                                    Ok(page) => {
                                        if let Some(items) = page["items"].as_array() {
                                            if items.is_empty() {
                                                break;
                                            }
                                            for item in items {
                                                if let Some(info) = parse_api_track(item, playlist_name.as_deref()) {
                                                    entries.push(info);
                                                }
                                            }
                                            offset += items.len();
                                            log::info!("[spotify] Fetched page at offset {}, got {} items, total so far: {}", offset - items.len(), items.len(), entries.len());

                                            // Delay between pages to avoid rate limiting
                                            tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

                                            if page["next"].is_null() {
                                                break;
                                            }
                                        } else {
                                            log::warn!("[spotify] No 'items' array in API response at offset {}: {}", offset, &body[..body.len().min(300)]);
                                            break;
                                        }
                                    }
                                    Err(e) => {
                                        log::warn!("[spotify] Failed to parse API page at offset {}: {} — body: {}", offset, e, &body[..body.len().min(200)]);
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                log::warn!("[spotify] API request failed at offset {}: {}", offset, e);
                                break;
                            }
                        }
                    }
                    log::info!("[spotify] Fetched {} total tracks", entries.len());
                } else {
                    log::warn!(
                        "[spotify] Embed has {}/{:?} tracks but could not obtain any access token for pagination. \
                         Only the first {} tracks were imported.",
                        entries.len(), total_tracks, entries.len()
                    );
                }
            }

            if entries.is_empty() {
                return Err(SourceError::Other("No tracks found in Spotify playlist".into()));
            }

            return Ok(super::ytdlp::PlaylistFetchResult {
                playlist_title: playlist_name,
                entries,
            });
        }
    }

    // Fallback: try Spotify's oEmbed API for basic info
    let oembed_url = format!("https://open.spotify.com/oembed?url={}", url);
    let resp = client.get(&oembed_url)
        .send()
        .await
        .map_err(|e| SourceError::NetworkError(format!("oEmbed failed: {}", e)))?;

    let oembed: serde_json::Value = resp.json().await
        .map_err(|e| SourceError::NetworkError(format!("oEmbed parse error: {}", e)))?;

    let title = oembed["title"].as_str().unwrap_or("Spotify Playlist").to_string();

    Err(SourceError::Other(format!(
        "Could not extract tracks from '{}'. The playlist may be private — configure Spotify credentials in Settings.",
        title
    )))
}
