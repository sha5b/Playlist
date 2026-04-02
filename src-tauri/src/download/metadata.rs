/// Platform-specific metadata extraction for DRM-protected services.
/// These use public embed/oEmbed APIs that don't require authentication.

use reqwest::Client;

/// Extracted track metadata from any platform
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration: Option<f64>,
}

/// Fetch track metadata from any supported DRM platform.
/// Returns (title, artist) for search query construction.
pub async fn fetch_track_metadata(url: &str, platform: &str) -> Option<TrackMetadata> {
    match platform {
        "spotify" => fetch_spotify_metadata(url).await,
        "deezer" => fetch_deezer_metadata(url).await,
        "apple_music" => fetch_apple_music_metadata(url).await,
        "tidal" => fetch_tidal_metadata(url).await,
        "amazon_music" => fetch_amazon_music_metadata(url).await,
        _ => None,
    }
}

/// Build a search query from metadata
pub fn build_search_query(meta: &TrackMetadata) -> String {
    match &meta.artist {
        Some(artist) => format!("{} - {}", artist, meta.title),
        None => meta.title.clone(),
    }
}

/// Spotify oEmbed API - works for any public track
async fn fetch_spotify_metadata(url: &str) -> Option<TrackMetadata> {
    let client = Client::new();
    let oembed_url = format!("https://open.spotify.com/oembed?url={}", url);
    
    let resp = client.get(&oembed_url).send().await.ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;
    
    let title = data["title"].as_str()?.to_string();
    
    // Try to extract artist from HTML embed or description
    let html = data["html"].as_str().unwrap_or("");
    let mut artist = None;
    
    // Parse "Song by Artist" pattern
    if let Some(by_pos) = html.find(" by ") {
        let after_by = &html[by_pos + 4..];
        if let Some(end_pos) = after_by.find(&['<', '"', '\''][..]) {
            let artist_str = after_by[..end_pos].trim();
            if !artist_str.is_empty() {
                artist = Some(artist_str.to_string());
            }
        }
    }
    
    // Fallback to description
    if artist.is_none() {
        if let Some(desc) = data["description"].as_str() {
            if !desc.is_empty() {
                artist = Some(desc.to_string());
            }
        }
    }
    
    Some(TrackMetadata {
        title,
        artist,
        album: None,
        duration: None,
    })
}

/// Deezer public API - works for any track
async fn fetch_deezer_metadata(url: &str) -> Option<TrackMetadata> {
    let client = Client::new();
    
    // Extract track ID from URL
    let track_id = if url.contains("/track/") {
        url.split("/track/").nth(1)?
            .split('?').next()?
            .split('/').next()?
    } else {
        return None;
    };
    
    if track_id.is_empty() || !track_id.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    
    let api_url = format!("https://api.deezer.com/track/{}", track_id);
    let resp = client.get(&api_url).send().await.ok()?;
    let data: serde_json::Value = resp.json().await.ok()?;
    
    if data["error"].is_object() {
        return None;
    }
    
    let title = data["title"].as_str()?.to_string();
    let artist = data["artist"]["name"].as_str().map(|s| s.to_string());
    let album = data["album"]["title"].as_str().map(|s| s.to_string());
    let duration = data["duration"].as_f64();
    
    Some(TrackMetadata {
        title,
        artist,
        album,
        duration,
    })
}

/// Apple Music - scrape from embed page or use MusicKit JS data
async fn fetch_apple_music_metadata(url: &str) -> Option<TrackMetadata> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .ok()?;
    
    // Apple Music URLs: https://music.apple.com/{region}/song/{name}/{id}
    // or album URLs with ?i= for specific track
    
    // Try oEmbed first (Apple supports it)
    let oembed_url = format!("https://music.apple.com/oembed?url={}", url_encode(url));
    if let Ok(resp) = client.get(&oembed_url).send().await {
        if let Ok(data) = resp.json::<serde_json::Value>().await {
            if let Some(title) = data["title"].as_str() {
                // Apple oEmbed title format: "Song Name - Artist Name"
                let parts: Vec<&str> = title.splitn(2, " - ").collect();
                if parts.len() == 2 {
                    return Some(TrackMetadata {
                        title: parts[0].trim().to_string(),
                        artist: Some(parts[1].trim().to_string()),
                        album: None,
                        duration: None,
                    });
                }
                return Some(TrackMetadata {
                    title: title.to_string(),
                    artist: None,
                    album: None,
                    duration: None,
                });
            }
        }
    }
    
    // Fallback: scrape the page for meta tags
    let resp = client.get(url).send().await.ok()?;
    let html = resp.text().await.ok()?;
    
    // Look for og:title meta tag
    let title = extract_meta_content(&html, "og:title")
        .or_else(|| extract_meta_content(&html, "twitter:title"))?;
    
    // og:description often has artist info
    let description = extract_meta_content(&html, "og:description");
    let artist = description.and_then(|d| {
        // Format is often "Song · Artist · Album · Year"
        let parts: Vec<&str> = d.split(" · ").collect();
        if parts.len() >= 2 {
            Some(parts[1].to_string())
        } else {
            None
        }
    });
    
    // Try to get album from music:album meta
    let album = extract_meta_content(&html, "music:album");
    
    Some(TrackMetadata {
        title,
        artist,
        album,
        duration: None,
    })
}

/// Tidal - scrape from embed/share page
async fn fetch_tidal_metadata(url: &str) -> Option<TrackMetadata> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .ok()?;
    
    // Tidal URLs: https://tidal.com/browse/track/{id} or https://listen.tidal.com/track/{id}
    
    // Try oEmbed
    let oembed_url = format!("https://oembed.tidal.com/?url={}", url_encode(url));
    if let Ok(resp) = client.get(&oembed_url).send().await {
        if let Ok(data) = resp.json::<serde_json::Value>().await {
            if let Some(title) = data["title"].as_str() {
                // Tidal oEmbed title: "Track Name by Artist Name"
                if let Some(by_pos) = title.find(" by ") {
                    let track_title = title[..by_pos].trim().to_string();
                    let artist = title[by_pos + 4..].trim().to_string();
                    return Some(TrackMetadata {
                        title: track_title,
                        artist: Some(artist),
                        album: None,
                        duration: None,
                    });
                }
                return Some(TrackMetadata {
                    title: title.to_string(),
                    artist: None,
                    album: None,
                    duration: None,
                });
            }
        }
    }
    
    // Fallback: scrape meta tags
    let resp = client.get(url).send().await.ok()?;
    let html = resp.text().await.ok()?;
    
    let title = extract_meta_content(&html, "og:title")
        .or_else(|| extract_meta_content(&html, "twitter:title"))?;
    
    // Tidal og:title is often "Track - Artist"
    let (final_title, artist) = if title.contains(" - ") {
        let parts: Vec<&str> = title.splitn(2, " - ").collect();
        (parts[0].trim().to_string(), Some(parts[1].trim().to_string()))
    } else {
        (title, None)
    };
    
    Some(TrackMetadata {
        title: final_title,
        artist,
        album: extract_meta_content(&html, "music:album:title"),
        duration: None,
    })
}

/// Amazon Music - scrape from share page
async fn fetch_amazon_music_metadata(url: &str) -> Option<TrackMetadata> {
    let client = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()
        .ok()?;
    
    // Amazon Music URLs vary by region: music.amazon.com, music.amazon.co.uk, etc.
    
    let resp = client.get(url).send().await.ok()?;
    let html = resp.text().await.ok()?;
    
    // Amazon uses og:title in format "Song Name by Artist Name on Amazon Music"
    let og_title = extract_meta_content(&html, "og:title")?;
    
    // Parse "Song Name by Artist Name on Amazon Music"
    let title_part = og_title.split(" on Amazon Music").next().unwrap_or(&og_title);
    
    let (title, artist) = if let Some(by_pos) = title_part.rfind(" by ") {
        let track = title_part[..by_pos].trim().to_string();
        let art = title_part[by_pos + 4..].trim().to_string();
        (track, Some(art))
    } else {
        (title_part.to_string(), None)
    };
    
    // Try to get album from description
    let album = extract_meta_content(&html, "og:description").and_then(|d| {
        // Often contains album name
        if d.contains("from the album") {
            d.split("from the album").nth(1)
                .and_then(|s| s.split('.').next())
                .map(|s| s.trim().to_string())
        } else {
            None
        }
    });
    
    Some(TrackMetadata {
        title,
        artist,
        album,
        duration: None,
    })
}

/// Helper to extract content from meta tags
fn extract_meta_content(html: &str, property: &str) -> Option<String> {
    // Look for <meta property="X" content="Y"> or <meta name="X" content="Y">
    let patterns = [
        format!(r#"property="{}""#, property),
        format!(r#"name="{}""#, property),
        format!(r#"property='{}'"#, property),
        format!(r#"name='{}'"#, property),
    ];
    
    for pattern in &patterns {
        if let Some(pos) = html.find(pattern.as_str()) {
            // Find the content attribute nearby
            let search_area = &html[pos..std::cmp::min(pos + 500, html.len())];
            
            // Try content="..." 
            if let Some(content_pos) = search_area.find("content=\"") {
                let start = content_pos + 9;
                if let Some(end) = search_area[start..].find('"') {
                    let content = &search_area[start..start + end];
                    if !content.is_empty() {
                        return Some(html_decode(content));
                    }
                }
            }
            
            // Try content='...'
            if let Some(content_pos) = search_area.find("content='") {
                let start = content_pos + 9;
                if let Some(end) = search_area[start..].find('\'') {
                    let content = &search_area[start..start + end];
                    if !content.is_empty() {
                        return Some(html_decode(content));
                    }
                }
            }
        }
    }
    
    None
}

/// Basic HTML entity decoding
fn html_decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

/// Simple URL encoding for query parameters
fn url_encode(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 3);
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => result.push(c),
            _ => {
                for byte in c.to_string().as_bytes() {
                    result.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_build_search_query() {
        let meta = TrackMetadata {
            title: "Bohemian Rhapsody".into(),
            artist: Some("Queen".into()),
            album: None,
            duration: None,
        };
        assert_eq!(build_search_query(&meta), "Queen - Bohemian Rhapsody");
        
        let meta_no_artist = TrackMetadata {
            title: "Unknown Track".into(),
            artist: None,
            album: None,
            duration: None,
        };
        assert_eq!(build_search_query(&meta_no_artist), "Unknown Track");
    }
}
