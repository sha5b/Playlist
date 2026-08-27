//! Search query construction and URL resolution for track downloads.

use std::collections::HashSet;

use super::ytdlp;
use super::matching;

/// Build search query variations (plain text, no engine prefix) to try in order.
/// The caller runs each through YouTube Music first, then plain YouTube.
/// When an album name is available, an album-aware query is added for accuracy.
///
/// Note: raw ISRC is intentionally NOT used as a text search — ISRCs don't appear
/// in YouTube titles, so it only wastes a request. ISRC is used for the direct
/// Odesli/SongLink lookup instead (a deterministic link, not a text search).
pub(super) fn build_search_variations(query: &str, album: Option<&str>) -> Vec<String> {
    let mut variations = Vec::new();
    let clean_query = clean_search_query(query);

    // 1. Album-aware search (best for pinning the correct album version)
    if let Some(album_name) = album {
        if !album_name.is_empty() {
            variations.push(format!("{} {}", query, album_name));
        }
    }

    // 2. Primary search: Artist - Title
    variations.push(query.to_string());

    // 3. "Artist - Title" → title-focused variations
    if query.contains(" - ") {
        let parts: Vec<&str> = query.splitn(2, " - ").collect();
        if parts.len() == 2 {
            let artist = parts[0].trim();
            let title = parts[1].trim();

            // Dash replaced by space (plain "Artist Title")
            variations.push(format!("{} {}", artist, title));

            // Artist + cleaned title (removes "(feat. …)" etc.)
            let clean_title = clean_search_query(title);
            if clean_title != title {
                variations.push(format!("{} {}", artist, clean_title));
            }

            // Just the title (last resort for obscure tracks)
            if title.split_whitespace().count() >= 2 {
                variations.push(title.to_string());
            }
        }
    }

    // 4. Cleaned full query if different
    if clean_query != query {
        variations.push(clean_query);
    }

    // Dedupe, preserve order, cap attempts.
    let mut seen = std::collections::HashSet::new();
    variations.retain(|v| seen.insert(v.clone()));
    variations.truncate(6);
    variations
}

/// Try to resolve a search query to a downloadable URL: YouTube Music first
/// (catalog-only, clean metadata), then plain YouTube as a fallback. Each engine's
/// results are scored by the matcher; returns `None` if nothing clears the bar
/// (caller then fails the track rather than downloading the wrong song).
#[allow(clippy::too_many_arguments)]
pub(super) async fn resolve_search_url(
    ytdlp_binary: &str,
    ffmpeg_dir: Option<&str>,
    cookies: Option<&str>,
    variations: &[String],
    scoring_query: &str,
    expected_duration_secs: Option<f64>,
    strict: bool,
    exclude_urls: &HashSet<String>,
) -> Option<String> {
    for variation in variations {
        // YouTube Music (preferred).
        if let Ok(results) = ytdlp::search_music_tracks(ytdlp_binary, ffmpeg_dir, variation, 6, cookies).await {
            if !results.is_empty() {
                if let Some(url) = matching::pick_best_match(&results, scoring_query, expected_duration_secs, strict, exclude_urls) {
                    log::info!("[download] YouTube Music matched via '{}'", variation);
                    return Some(url);
                }
            }
        }
        // Plain YouTube fallback.
        if let Ok(results) = ytdlp::search_info(ytdlp_binary, ffmpeg_dir, &format!("ytsearch6:{}", variation), cookies).await {
            if !results.is_empty() {
                if let Some(url) = matching::pick_best_match(&results, scoring_query, expected_duration_secs, strict, exclude_urls) {
                    log::info!("[download] YouTube matched via '{}'", variation);
                    return Some(url);
                }
            }
        }
    }
    None
}

/// Clean a search query by removing common noise like feat., ft., parenthetical info, etc.
pub(super) fn clean_search_query(query: &str) -> String {
    let mut result = query.to_string();

    // Only remove YouTube-specific noise — keep musically meaningful modifiers
    // (Remix, Remastered, Live, Acoustic, Deluxe, feat. etc.) since they help
    // find the correct version on YouTube Music.
    let remove_patterns = [
        "(Official Video)", "(Official Music Video)", "(Official Audio)",
        "(Lyric Video)", "(Lyrics)", "(Audio)", "(Music Video)",
        "(Visualizer)", "(Official Visualizer)", "(Official Lyric Video)",
        "[Official Video]", "[Official Music Video]", "[Official Audio]",
        "[Lyric Video]", "[Lyrics]", "[Audio]", "[Music Video]",
        "[Official]", "(Official)",
    ];
    for pattern in &remove_patterns {
        result = result.replace(pattern, "");
        result = result.replace(&pattern.to_lowercase(), "");
    }

    // Clean up extra whitespace
    result = result.split_whitespace().collect::<Vec<_>>().join(" ");
    result.trim().to_string()
}

/// Split "Artist / Title" or "Artist - Title" patterns common in YouTube video titles.
/// Returns (title, artist_name). If the title doesn't match a pattern, returns the original
/// title and the provided fallback artist.
pub(super) fn split_title_artist(raw_title: &str, fallback_artist: Option<&str>) -> (String, Option<String>) {
    // Delimiters ordered by specificity — " / " is almost always "Artist / Title"
    let delimiters = [" / ", " - ", " – ", " — "];

    for delim in &delimiters {
        if let Some(pos) = raw_title.find(delim) {
            let left = raw_title[..pos].trim();
            let right = raw_title[pos + delim.len()..].trim();
            // Both parts must be non-empty and reasonable length
            if !left.is_empty() && !right.is_empty() && left.len() < 200 && right.len() < 200 {
                // "Artist / Title" — left is artist, right is title
                return (right.to_string(), Some(left.to_string()));
            }
        }
    }

    // No splitting pattern found — return as-is
    (raw_title.to_string(), fallback_artist.map(|s| s.to_string()))
}

/// Check if a genre string is actually a YouTube category (not a real music genre).
pub(super) fn is_youtube_category(genre: &str) -> bool {
    let categories = [
        "People & Blogs", "Entertainment", "Education", "Science & Technology",
        "News & Politics", "Howto & Style", "Comedy", "Film & Animation",
        "Autos & Vehicles", "Pets & Animals", "Sports", "Travel & Events",
        "Gaming", "Nonprofits & Activism",
    ];
    categories.iter().any(|c| c.eq_ignore_ascii_case(genre))
}
