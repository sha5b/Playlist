use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedUrl {
    pub platform: String,
    pub url_type: String, // "track", "playlist", "album", "channel", "unknown"
    pub clean_url: String,
}

/// Extract the value of a query parameter from a URL.
fn query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let query = url.split_once('?')?.1;
    // Strip any fragment
    let query = query.split('#').next().unwrap_or(query);
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=')?;
        (k == key && !v.is_empty()).then_some(v)
    })
}

/// Canonicalize a YouTube playlist URL to the dedicated playlist page.
///
/// Watch URLs like `youtube.com/watch?v=XXX&list=PLYYY` make yt-dlp extract the
/// playlist from the watch page's sidebar, which only exposes the first ~100
/// entries (+ the current video). The canonical `playlist?list=PLYYY` page
/// paginates through the full playlist. Mix/radio lists (`RD…`) have no
/// standalone playlist page, so those keep the original URL.
fn canonical_youtube_playlist_url(url: &str) -> Option<String> {
    let list_id = query_param(url, "list")?;
    if list_id.starts_with("RD") {
        return None;
    }
    let host = if url.contains("music.youtube.com") {
        "music.youtube.com"
    } else {
        "www.youtube.com"
    };
    Some(format!("https://{}/playlist?list={}", host, list_id))
}

pub fn parse_url(url: &str) -> ParsedUrl {
    let url = url.trim();

    // YouTube
    if url.contains("youtube.com") || url.contains("youtu.be") || url.contains("music.youtube.com")
    {
        let url_type = if url.contains("list=") || url.contains("/playlist") {
            "playlist"
        } else if url.contains("/channel/") || url.contains("/@") {
            "channel"
        } else {
            "track"
        };
        let clean_url = if url_type == "playlist" {
            canonical_youtube_playlist_url(url).unwrap_or_else(|| url.into())
        } else {
            url.into()
        };
        return ParsedUrl {
            platform: "youtube".into(),
            url_type: url_type.into(),
            clean_url,
        };
    }

    // Spotify
    if url.contains("open.spotify.com") || url.contains("spotify.link") {
        let url_type = if url.contains("/playlist/") {
            "playlist"
        } else if url.contains("/album/") {
            "album"
        } else if url.contains("/track/") {
            "track"
        } else if url.contains("/artist/") {
            "channel"
        } else {
            "unknown"
        };
        return ParsedUrl {
            platform: "spotify".into(),
            url_type: url_type.into(),
            clean_url: url.into(),
        };
    }

    // SoundCloud
    if url.contains("soundcloud.com") {
        let url_type = if url.contains("/sets/") {
            "playlist"
        } else {
            "track"
        };
        return ParsedUrl {
            platform: "soundcloud".into(),
            url_type: url_type.into(),
            clean_url: url.into(),
        };
    }

    // Bandcamp
    if url.contains("bandcamp.com") {
        let url_type = if url.contains("/album/") {
            "album"
        } else if url.contains("/track/") {
            "track"
        } else {
            "unknown"
        };
        return ParsedUrl {
            platform: "bandcamp".into(),
            url_type: url_type.into(),
            clean_url: url.into(),
        };
    }

    // Apple Music
    if url.contains("music.apple.com") {
        let url_type = if url.contains("/playlist/") {
            "playlist"
        } else if url.contains("/album/") {
            "album"
        } else if url.contains("/song/") {
            "track"
        } else if url.contains("/artist/") {
            "channel"
        } else {
            "unknown"
        };
        return ParsedUrl {
            platform: "apple_music".into(),
            url_type: url_type.into(),
            clean_url: url.into(),
        };
    }

    // Tidal
    if url.contains("tidal.com") {
        let url_type = if url.contains("/playlist/") {
            "playlist"
        } else if url.contains("/album/") {
            "album"
        } else if url.contains("/track/") {
            "track"
        } else if url.contains("/artist/") {
            "channel"
        } else {
            "unknown"
        };
        return ParsedUrl {
            platform: "tidal".into(),
            url_type: url_type.into(),
            clean_url: url.into(),
        };
    }

    // Deezer
    if url.contains("deezer.com") || url.contains("deezer.page.link") {
        let url_type = if url.contains("/playlist/") {
            "playlist"
        } else if url.contains("/album/") {
            "album"
        } else if url.contains("/track/") {
            "track"
        } else if url.contains("/artist/") {
            "channel"
        } else {
            "unknown"
        };
        return ParsedUrl {
            platform: "deezer".into(),
            url_type: url_type.into(),
            clean_url: url.into(),
        };
    }

    // Amazon Music
    if url.contains("music.amazon.") || (url.contains("amazon.") && url.contains("/music/")) {
        let url_type = if url.contains("/playlist") {
            "playlist"
        } else if url.contains("/album") {
            "album"
        } else if url.contains("/track") {
            "track"
        } else if url.contains("/artist") {
            "channel"
        } else {
            "unknown"
        };
        return ParsedUrl {
            platform: "amazon_music".into(),
            url_type: url_type.into(),
            clean_url: url.into(),
        };
    }

    // Direct audio file
    let lower = url.to_lowercase();
    if lower.ends_with(".mp3")
        || lower.ends_with(".flac")
        || lower.ends_with(".opus")
        || lower.ends_with(".ogg")
        || lower.ends_with(".m4a")
        || lower.ends_with(".wav")
    {
        return ParsedUrl {
            platform: "direct".into(),
            url_type: "track".into(),
            clean_url: url.into(),
        };
    }

    // Unknown — let yt-dlp try anyway
    ParsedUrl {
        platform: "other".into(),
        url_type: "unknown".into(),
        clean_url: url.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_url_with_list_is_canonicalized_to_playlist_page() {
        let p = parse_url("https://www.youtube.com/watch?v=abc123&list=PLxyz789&index=5");
        assert_eq!(p.platform, "youtube");
        assert_eq!(p.url_type, "playlist");
        assert_eq!(p.clean_url, "https://www.youtube.com/playlist?list=PLxyz789");
    }

    #[test]
    fn music_youtube_keeps_music_host() {
        let p = parse_url("https://music.youtube.com/watch?v=abc&list=PLmusic1");
        assert_eq!(p.clean_url, "https://music.youtube.com/playlist?list=PLmusic1");
    }

    #[test]
    fn plain_playlist_url_stays_canonical() {
        let p = parse_url("https://www.youtube.com/playlist?list=PLxyz789");
        assert_eq!(p.clean_url, "https://www.youtube.com/playlist?list=PLxyz789");
    }

    #[test]
    fn mix_radio_lists_are_left_alone() {
        // RD… mixes have no standalone playlist page
        let url = "https://www.youtube.com/watch?v=abc&list=RDabc123";
        let p = parse_url(url);
        assert_eq!(p.url_type, "playlist");
        assert_eq!(p.clean_url, url);
    }

    #[test]
    fn plain_watch_url_is_a_track() {
        let p = parse_url("https://www.youtube.com/watch?v=abc123");
        assert_eq!(p.url_type, "track");
        assert_eq!(p.clean_url, "https://www.youtube.com/watch?v=abc123");
    }

    #[test]
    fn fragment_does_not_leak_into_list_id() {
        let p = parse_url("https://www.youtube.com/watch?v=a&list=PLabc#t=30");
        assert_eq!(p.clean_url, "https://www.youtube.com/playlist?list=PLabc");
    }
}
