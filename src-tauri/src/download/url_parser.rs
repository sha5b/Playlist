use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedUrl {
    pub platform: String,
    pub url_type: String, // "track", "playlist", "album", "channel", "unknown"
    pub clean_url: String,
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
        return ParsedUrl {
            platform: "youtube".into(),
            url_type: url_type.into(),
            clean_url: url.into(),
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
