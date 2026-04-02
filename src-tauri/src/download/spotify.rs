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
    })
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
                .or_else(|| entity["trackList"].as_array().map(|_| {
                    // trackList format doesn't have totalCount; check trackCount on the entity
                    entity["trackCount"].as_u64().unwrap_or(0)
                }))
                .unwrap_or(entries.len() as u64);

            if (total_tracks as usize) > entries.len() {
                // Try to extract the anonymous access token from the embed page data
                let access_token = data["props"]["pageProps"]["state"]["data"]["accessToken"].as_str()
                    .or_else(|| data["props"]["pageProps"]["accessToken"].as_str());

                if let Some(token) = access_token {
                    log::info!(
                        "[spotify] Embed returned {}/{} tracks, fetching remaining via API",
                        entries.len(), total_tracks
                    );
                    let mut offset = entries.len();
                    let api_path = if item_type == "playlist" {
                        format!("https://api.spotify.com/v1/playlists/{}/tracks", item_id)
                    } else {
                        format!("https://api.spotify.com/v1/albums/{}/tracks", item_id)
                    };

                    loop {
                        if offset >= total_tracks as usize {
                            break;
                        }
                        let api_url = format!("{}?offset={}&limit=100", api_path, offset);
                        let api_resp = client.get(&api_url)
                            .header("Authorization", format!("Bearer {}", token))
                            .send()
                            .await;

                        match api_resp {
                            Ok(resp) => {
                                if let Ok(page) = resp.json::<serde_json::Value>().await {
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

                                        // If there's no "next" page, we're done
                                        if page["next"].is_null() {
                                            break;
                                        }
                                    } else {
                                        break;
                                    }
                                } else {
                                    log::warn!("[spotify] Failed to parse API page at offset {}", offset);
                                    break;
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
                        "[spotify] Embed has {}/{} tracks but no access token found for pagination",
                        entries.len(), total_tracks
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
