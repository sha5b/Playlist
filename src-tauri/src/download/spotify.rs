/// Known-good persisted-query hash for the `fetchPlaylistContents` Pathfinder
/// operation, used as a fallback when scraping the hash from the web player's JS
/// bundles fails (e.g. Spotify serves our HTTP client a page without the bundles).
/// Spotify rotates these occasionally; discovery is still tried first.
const FALLBACK_PLAYLIST_HASH: &str =
    "a65e12194ed5fc443a1cdebed5fabe33ca5b07b987185d63c72483867ad13cb4";

/// Fetch track metadata (title, artist) from Spotify's oEmbed API.
/// Works for any public Spotify URL without authentication.
/// Returns (title, artist) parsed from the oEmbed "title" field which is formatted as "Song - Artist".
pub async fn fetch_track_metadata(url: &str) -> Option<(String, Option<String>)> {
    // Percent-encode: raw `?si=…&…` query params in the target URL would
    // otherwise truncate the `url=` parameter and 404 the oEmbed request.
    let oembed_url = format!(
        "https://open.spotify.com/oembed?url={}",
        super::metadata::url_encode(url)
    );
    let client = reqwest::Client::new();
    let resp = client.get(&oembed_url).send().await.ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;

    let title = data["title"].as_str()?;

    // Try to extract artist from the HTML embed content
    let html = data["html"].as_str().unwrap_or("");
    let mut artist = None;

    // Try to extract artist from "Song by Artist" pattern in HTML
    if let Some(by_pos) = html.find(" by ") {
        let after_by = &html[by_pos + 4..];
        if let Some(end_pos) = after_by.find(&['<', '"', '\''][..]) {
            let artist_str = after_by[..end_pos].trim();
            if !artist_str.is_empty() {
                artist = Some(artist_str.to_string());
            }
        }
    }

    // Fallback: try description field
    if artist.is_none() {
        if let Some(description) = data["description"].as_str().filter(|s| !s.is_empty()) {
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
    // The embed page's `duration` field is MILLISECONDS (like `duration_ms`);
    // storing it raw as seconds broke every duration-based match gate.
    let duration = track["duration"].as_f64()
        .or_else(|| track["duration_ms"].as_f64())
        .map(|ms| ms / 1000.0);
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

/// Search the raw HTML for an access token. Spotify may embed it outside of __NEXT_DATA__
/// in script tags or inline JSON configuration.
fn extract_token_from_html(html: &str) -> Option<String> {
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
async fn fetch_anonymous_token(_client: &reqwest::Client) -> Option<String> {
    let jar_client = reqwest::Client::builder()
        .cookie_store(true)
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
        .build()
        .ok()?;

    let _ = jar_client.get("https://open.spotify.com/").send().await;

    for token_url in &[
        "https://open.spotify.com/get_access_token?reason=transport&productType=embed",
        "https://open.spotify.com/get_access_token?reason=transport&productType=web_player",
    ] {
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

    log::warn!("[spotify] All anonymous token acquisition methods failed");
    None
}

/// Parse a track from the Spotify Web API response format (different from embed format).
fn parse_api_track(item: &serde_json::Value, fallback_album: Option<&str>, cover_url: Option<&str>) -> Option<super::ytdlp::VideoInfo> {
    let track = if item["track"].is_object() { &item["track"] } else { item };

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
    let isrc = track["external_ids"]["isrc"].as_str()
        .map(|s| s.to_string());

    Some(super::ytdlp::VideoInfo {
        title: title.to_string(),
        track: Some(title.to_string()),
        artist: artist.clone(),
        uploader: artist,
        duration,
        thumbnail: cover_url.map(|s| s.to_string()),
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

/// Search a JS source string for a sha256 hash associated with a given GraphQL operation name.
/// Handles multiple JS bundler patterns:
///   - `new X("fetchPlaylist","query","<64-hex>",null)` (Spotify web player)
///   - `sha256Hash:"<64-hex>"` near operation name
///   - `hash:"<64-hex>"` near operation name
fn find_hash_for_operation(js: &str, operation: &str) -> Option<String> {
    let mut pos = 0;
    while let Some(idx) = js[pos..].find(operation) {
        let abs_idx = pos + idx;
        let search_end = (abs_idx + 500).min(js.len());
        let context = &js[abs_idx..search_end];

        // Pattern 1: constructor style — "operationName","query","<hash>" or "operationName","mutation","<hash>"
        for type_str in &["\",\"query\",\"", "\",\"mutation\",\""] {
            if let Some(type_pos) = context.find(type_str) {
                let hash_start = type_pos + type_str.len();
                if hash_start + 64 <= context.len() {
                    let candidate = &context[hash_start..hash_start + 64];
                    if candidate.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Some(candidate.to_string());
                    }
                }
            }
        }

        // Pattern 2: sha256Hash/hash key style
        for prefix in &["sha256Hash:\"", "hash:\"", "sha256Hash\":\""] {
            if let Some(hash_start) = context.find(prefix) {
                let hash_val_start = hash_start + prefix.len();
                if hash_val_start + 64 <= context.len() {
                    let candidate = &context[hash_val_start..hash_val_start + 64];
                    if candidate.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Some(candidate.to_string());
                    }
                }
            }
        }

        pos = abs_idx + operation.len();
    }
    None
}

/// Discover the Pathfinder API persisted-query hash for playlist fetching by
/// scanning Spotify's web player JS bundles for known GraphQL operation names.
async fn discover_pathfinder_hash(client: &reqwest::Client, item_type: &str, item_id: &str) -> Option<(String, String)> {
    // Fetch the actual web player page (not the marketing landing page)
    // so we get the correct JS bundles that contain GraphQL hashes
    let page_url = format!("https://open.spotify.com/{}/{}", item_type, item_id);
    let resp = client.get(&page_url).send().await.ok()?;
    let html = resp.text().await.ok()?;

    // Collect JS bundle URLs from <script src="..."> tags
    let mut js_urls: Vec<String> = Vec::new();
    let mut search_start = 0;
    while let Some(pos) = html[search_start..].find("src=\"https://") {
        let abs_pos = search_start + pos + 5;
        if let Some(end) = html[abs_pos..].find('"') {
            let url = &html[abs_pos..abs_pos + end];
            if url.ends_with(".js") {
                js_urls.push(url.to_string());
            }
            search_start = abs_pos + end;
        } else {
            break;
        }
    }

    log::info!("[spotify] Found {} JS bundles from web player page", js_urls.len());

    // Try multiple possible GraphQL operation names.
    // Prefer fetchPlaylistContents — it requires fewer variables than fetchPlaylist.
    let operation_names = [
        "fetchPlaylistContents",
        "fetchPlaylist",
        "getPlaylist",
        "playlistContents",
    ];

    for js_url in &js_urls {
        let resp = match client.get(js_url).send().await {
            Ok(r) => r,
            Err(_) => continue,
        };
        let js = match resp.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        for op_name in &operation_names {
            if let Some(hash) = find_hash_for_operation(&js, op_name) {
                log::info!("[spotify] Discovered hash for '{}': {}...", op_name, &hash[..16]);
                return Some((op_name.to_string(), hash));
            }
        }
    }

    log::warn!("[spotify] Could not discover any playlist hash from {} JS bundles", js_urls.len());
    None
}

/// Fetch a page of playlist tracks via Spotify's internal Pathfinder GraphQL API.
async fn fetch_pathfinder_page(
    client: &reqwest::Client,
    token: &str,
    item_id: &str,
    operation_name: &str,
    hash: &str,
    offset: usize,
    limit: usize,
) -> Result<serde_json::Value, String> {
    let uri = format!("spotify:playlist:{}", item_id);
    // Variables must match what the web player sends for the chosen operation.
    // fetchPlaylistContents needs: uri, offset, limit, includeEpisodeContentRatingsV2
    // fetchPlaylist additionally needs: enableWatchFeedEntrypoint
    let mut vars = serde_json::json!({
        "uri": uri,
        "offset": offset,
        "limit": limit,
        "includeEpisodeContentRatingsV2": false
    });
    if operation_name == "fetchPlaylist" {
        vars["enableWatchFeedEntrypoint"] = serde_json::json!(false);
    }
    let variables = vars;
    let extensions = serde_json::json!({
        "persistedQuery": {
            "version": 1,
            "sha256Hash": hash
        }
    });

    let resp = client
        .get("https://api-partner.spotify.com/pathfinder/v1/query")
        .header("Authorization", format!("Bearer {}", token))
        .header("app-platform", "WebPlayer")
        .header("origin", "https://open.spotify.com")
        .header("referer", "https://open.spotify.com/")
        .header("accept", "application/json")
        .query(&[
            ("operationName", operation_name),
            ("variables", &variables.to_string()),
            ("extensions", &extensions.to_string()),
        ])
        .send()
        .await
        .map_err(|e| format!("Pathfinder request failed: {}", e))?;

    let status = resp.status();
    let body = resp.text().await.map_err(|e| format!("Failed to read body: {}", e))?;

    if !status.is_success() {
        return Err(format!("HTTP {}: {}", status, &body[..body.len().min(300)]));
    }

    serde_json::from_str(&body).map_err(|e| format!("JSON parse error: {}", e))
}

/// Parse a track from a Pathfinder API response item.
/// Structure: `items[].itemV2.data` → `{name, uri, duration, artists, albumOfTrack}`
fn parse_pathfinder_track(item: &serde_json::Value, cover_url: Option<&str>) -> Option<super::ytdlp::VideoInfo> {
    let item_v2 = item.get("itemV2").and_then(|v| v.get("data"))?;
    // Some responses nest as itemV2.data.data, others as itemV2.data directly
    let track = if item_v2.get("name").is_some() { item_v2 } else { item_v2.get("data")? };

    let title = track["name"].as_str()?;
    let uri = track["uri"].as_str().unwrap_or("").to_string();
    // Spotify's current shape uses `trackDuration.totalMilliseconds`; keep the older
    // `duration`/`duration_ms` as fallbacks.
    let duration = track["trackDuration"]["totalMilliseconds"].as_f64()
        .or_else(|| track["duration"]["totalMilliseconds"].as_f64())
        .or_else(|| track["duration_ms"].as_f64())
        .map(|ms| ms / 1000.0);
    let artist = track["artists"]["items"].as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["profile"]["name"].as_str().or_else(|| a["name"].as_str()))
        .map(|s| s.to_string());
    let album = track["albumOfTrack"]["name"].as_str()
        .or_else(|| track["album"]["name"].as_str())
        .map(|s| s.to_string());
    // Track/disc numbers are top-level on the Track node.
    let track_number = track["trackNumber"].as_i64();
    let disc_number = track["discNumber"].as_i64();
    let isrc = track["playability"]["isrc"].as_str()
        .or_else(|| track["externalIds"]["isrc"].as_str())
        .map(|s| s.to_string());

    Some(super::ytdlp::VideoInfo {
        title: title.to_string(),
        track: Some(title.to_string()),
        artist: artist.clone(),
        uploader: artist,
        duration,
        thumbnail: cover_url.map(|s| s.to_string()),
        webpage_url: if uri.is_empty() { None } else { Some(uri) },
        album,
        genre: None,
        release_year: None,
        description: None,
        track_number,
        disc_number,
        composer: None,
        language: None,
        tags: None,
        channel_url: None,
        isrc,
    })
}

/// Fetch playlist entries from Spotify using the public embed page + API pagination.
/// Step 1: embed page → first ~100 tracks + total count + cover image + access token.
/// Step 2: if total > 100, paginate via Spotify API using the token from step 1.
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

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/125.0.0.0 Safari/537.36")
        .build()
        .map_err(|e| SourceError::NetworkError(format!("Failed to build HTTP client: {}", e)))?;

    // --- Step 1: Fetch embed page to get playlist name, cover image, and first batch of tracks ---
    let embed_url = format!("https://open.spotify.com/embed/{}/{}", item_type, item_id);

    let resp = client.get(&embed_url)
        .send()
        .await
        .map_err(|e| SourceError::NetworkError(format!("Failed to fetch Spotify embed: {}", e)))?;

    let html = resp.text().await
        .map_err(|e| SourceError::NetworkError(format!("Failed to read response: {}", e)))?;

    let json_str = html.split(r#"<script id="__NEXT_DATA__" type="application/json">"#)
        .nth(1)
        .and_then(|s| s.split("</script>").next());

    let json_str = json_str.ok_or_else(|| SourceError::Other("Could not find __NEXT_DATA__ in Spotify embed page".into()))?;
    let data = serde_json::from_str::<serde_json::Value>(json_str)
        .map_err(|e| SourceError::Other(format!("Failed to parse embed JSON: {}", e)))?;

    let entity = &data["props"]["pageProps"]["state"]["data"]["entity"];
    let playlist_name = entity["name"].as_str().map(|s| s.to_string());

    // Extract playlist cover image from embed data
    let cover_url = entity["coverArt"]["sources"].as_array()
        .and_then(|sources| sources.iter()
            .max_by_key(|s| s["width"].as_u64().unwrap_or(0))
            .and_then(|s| s["url"].as_str()))
        .or_else(|| entity["images"].as_array()
            .and_then(|imgs| imgs.first())
            .and_then(|i| i["url"].as_str()))
        .or_else(|| entity["coverArt"]["url"].as_str())
        .map(|s| s.to_string());

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
            if let Some(mut info) = parse_spotify_track(track, playlist_name.as_deref()) {
                if info.thumbnail.is_none() {
                    info.thumbnail = cover_url.clone();
                }
                entries.push(info);
            }
        }
    }

    let total_tracks = entity["tracks"]["totalCount"].as_u64()
        .or_else(|| entity["trackCount"].as_u64())
        .or_else(|| entity["tracks"]["total"].as_u64());

    // Log the full entity keys to debug what fields are available
    if total_tracks.is_none() {
        let keys: Vec<&str> = entity.as_object().map(|o| o.keys().map(|k| k.as_str()).collect()).unwrap_or_default();
        log::info!("[spotify] Entity keys: {:?}", keys);
        if let Some(tracks_obj) = entity["tracks"].as_object() {
            let track_keys: Vec<&str> = tracks_obj.keys().map(|k| k.as_str()).collect();
            log::info!("[spotify] Entity.tracks keys: {:?}", track_keys);
        }
    }

    log::info!("[spotify] Embed page returned {} tracks, total: {:?}", entries.len(), total_tracks);

    // --- Step 2: If there are more tracks than the embed returned, paginate ---
    let need_more = match total_tracks {
        Some(total) => (total as usize) > entries.len(),
        None => entries.len() >= 100,
    };

    if need_more && item_type == "playlist" {
        let total = total_tracks.map(|t| t as usize).unwrap_or(10000);

        // Get access token from embed data or anonymous endpoint
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
            let entries_from_embed = entries.len();

            // --- Strategy A: Pathfinder (internal GraphQL API) ---
            // Always attempt Pathfinder: use a freshly-scraped persisted-query hash if
            // discovery works, otherwise the bundled fallback hash. (Previously, if hash
            // discovery failed we skipped straight to the v1 API, which Spotify now hard
            // rate-limits — leaving the playlist stuck at the embed's first ~100 tracks.)
            let (op_name, hash) = match discover_pathfinder_hash(&client, item_type, item_id).await {
                Some((op, h)) => (op, h),
                None => {
                    log::warn!("[spotify] hash discovery failed; using bundled fallback hash");
                    ("fetchPlaylistContents".to_string(), FALLBACK_PLAYLIST_HASH.to_string())
                }
            };
            log::info!("[spotify] Pathfinder op '{}' (hash {}...)", op_name, &hash[..16.min(hash.len())]);
            {
                let mut offset = entries_from_embed;
                let page_size = 100; // proven-stable page size for the Pathfinder query
                let mut fail_streak = 0u32;

                loop {
                    if offset >= total { break; }

                    tokio::time::sleep(std::time::Duration::from_millis(700)).await;

                    match fetch_pathfinder_page(&client, &token, item_id, &op_name, &hash, offset, page_size).await {
                        Ok(data) => {
                            // A stale/invalid persisted-query hash returns HTTP 200 with an
                            // `errors` array (e.g. PersistedQueryNotFound) — stop and let the
                            // v1 fallback try rather than looping forever.
                            if data.get("errors").map(|e| !e.is_null()).unwrap_or(false) {
                                log::warn!("[spotify] Pathfinder query error: {}", data["errors"]);
                                break;
                            }
                            let content = &data["data"]["playlistV2"]["content"];
                            let pf_total = content["totalCount"].as_u64().map(|t| t as usize);
                            let effective_total = pf_total.unwrap_or(total);

                            if let Some(items) = content["items"].as_array() {
                                if items.is_empty() { break; }
                                let batch_len = items.len();
                                for item in items {
                                    if let Some(info) = parse_pathfinder_track(item, cover_url.as_deref()) {
                                        entries.push(info);
                                    }
                                }
                                log::info!(
                                    "[spotify] Pathfinder offset {}: got {} items, total so far: {}/{}",
                                    offset, batch_len, entries.len(), effective_total
                                );
                                offset += batch_len;
                                fail_streak = 0;
                                if offset >= effective_total { break; }
                            } else {
                                break;
                            }
                        }
                        Err(e) => {
                            // Retry a few times on transient errors before giving up so a
                            // single hiccup doesn't truncate a large (thousands) playlist.
                            fail_streak += 1;
                            log::warn!("[spotify] Pathfinder page at offset {} failed ({}/3): {}", offset, fail_streak, e);
                            if fail_streak >= 3 { break; }
                            tokio::time::sleep(std::time::Duration::from_secs(2 * fail_streak as u64)).await;
                        }
                    }
                }

                if entries.len() > entries_from_embed {
                    log::info!("[spotify] Pathfinder fetched {} total tracks", entries.len());
                }
            }

            // --- Strategy B: v1 REST API fallback ---
            // Runs whenever tracks are still missing — including after a
            // PARTIALLY successful Pathfinder run that aborted mid-playlist
            // (persistent errors after some pages). Previously any Pathfinder
            // progress suppressed this fallback and truncated the playlist.
            if entries.len() < total {
                log::info!("[spotify] Falling back to v1 API for pagination");
                let mut offset = entries.len();
                let api_path = format!("https://api.spotify.com/v1/playlists/{}/tracks", item_id);
                let mut rate_limit_retries = 0u32;

                while offset < total {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;

                    let api_url = format!("{}?offset={}&limit=100", api_path, offset);
                    let api_resp = match client.get(&api_url)
                        .header("Authorization", format!("Bearer {}", token))
                        .send()
                        .await
                    {
                        Ok(r) => r,
                        Err(e) => {
                            log::warn!("[spotify] v1 API request failed at offset {}: {}", offset, e);
                            break;
                        }
                    };

                    let status = api_resp.status();
                    if status.as_u16() == 429 {
                        // Honor Retry-After and back off — don't give up on the first 429
                        // (Spotify aggressively rate-limits the anonymous token), or the
                        // playlist would stop at the first embed page (~100 tracks).
                        rate_limit_retries += 1;
                        if rate_limit_retries > 6 {
                            log::warn!("[spotify] v1 API still rate limited after {} retries — stopping at {} tracks", rate_limit_retries, entries.len());
                            break;
                        }
                        let retry_after = api_resp.headers()
                            .get("retry-after")
                            .and_then(|v| v.to_str().ok())
                            .and_then(|v| v.parse::<u64>().ok())
                            .unwrap_or(5)
                            .min(30);
                        let wait = retry_after.max(2 * rate_limit_retries as u64);
                        log::info!("[spotify] v1 API rate limited at offset {}, waiting {}s ({}/6)", offset, wait, rate_limit_retries);
                        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
                        continue;
                    }
                    rate_limit_retries = 0;

                    let body = api_resp.text().await.unwrap_or_default();
                    if !status.is_success() {
                        log::warn!("[spotify] v1 API HTTP {} at offset {}: {}", status, offset, &body[..body.len().min(200)]);
                        break;
                    }

                    match serde_json::from_str::<serde_json::Value>(&body) {
                        Ok(page) => {
                            if let Some(items) = page["items"].as_array() {
                                if items.is_empty() { break; }
                                let batch_len = items.len();
                                for item in items {
                                    if let Some(info) = parse_api_track(item, playlist_name.as_deref(), cover_url.as_deref()) {
                                        entries.push(info);
                                    }
                                }
                                log::info!("[spotify] v1 API offset {}: {} items, total: {}/{}", offset, batch_len, entries.len(), total);
                                offset += batch_len;
                                if page["next"].is_null() { break; }
                            } else {
                                break;
                            }
                        }
                        Err(e) => {
                            log::warn!("[spotify] v1 API parse error at offset {}: {}", offset, e);
                            break;
                        }
                    }
                }
                log::info!("[spotify] v1 API: {} total tracks after fallback", entries.len());
            }
        } else {
            log::warn!(
                "[spotify] Have {} tracks but no access token for pagination. Only embed batch imported.",
                entries.len()
            );
        }
    } else if need_more && item_type == "album" {
        // Album pagination via v1 API (albums are typically <100 tracks, rare edge case)
        let total = total_tracks.map(|t| t as usize).unwrap_or(10000);
        let embed_token = data["props"]["pageProps"]["state"]["data"]["accessToken"].as_str()
            .or_else(|| data["props"]["pageProps"]["accessToken"].as_str())
            .map(|s| s.to_string());
        let token = embed_token
            .or_else(|| extract_token_from_html(&html).map(|s| s.to_string()));

        if let Some(token) = token {
            let mut offset = entries.len();
            let api_path = format!("https://api.spotify.com/v1/albums/{}/tracks", item_id);
            while offset < total {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let api_url = format!("{}?offset={}&limit=100", api_path, offset);
                let resp = client.get(&api_url)
                    .header("Authorization", format!("Bearer {}", token))
                    .send().await;
                match resp {
                    Ok(r) if r.status().is_success() => {
                        let body = r.text().await.unwrap_or_default();
                        if let Ok(page) = serde_json::from_str::<serde_json::Value>(&body) {
                            if let Some(items) = page["items"].as_array() {
                                if items.is_empty() { break; }
                                let batch = items.len();
                                for item in items {
                                    if let Some(info) = parse_api_track(item, playlist_name.as_deref(), cover_url.as_deref()) {
                                        entries.push(info);
                                    }
                                }
                                offset += batch;
                                if page["next"].is_null() { break; }
                            } else { break; }
                        } else { break; }
                    }
                    _ => break,
                }
            }
        }
    }

    if entries.is_empty() {
        return Err(SourceError::Other("No tracks found in Spotify playlist".into()));
    }

    Ok(super::ytdlp::PlaylistFetchResult {
        playlist_title: playlist_name,
        playlist_thumbnail: cover_url,
        entries,
    })
}
