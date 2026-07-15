//! Track-matching heuristics: choose the *correct* audio among search results.
//!
//! These are pure functions (no I/O) so the scoring logic is easy to reason about
//! and unit-test. The download orchestrator (`mod.rs`) feeds candidate
//! `VideoInfo`s in and gets back the best URL — or `None`, which means "no
//! confident match, fail the track" (better than silently grabbing the wrong song).

use std::collections::HashSet;

use super::ytdlp::VideoInfo;

/// Words that indicate an *alternate* version we should avoid unless the query
/// explicitly asks for it (e.g. the user searched for a live/remix version).
/// Kept deliberately narrow: musically-neutral tags like "remaster", "deluxe",
/// "radio edit" are NOT here because they're the same recording.
const VARIANT_KEYWORDS: &[&str] = &[
    "live", "remix", "cover", "karaoke", "instrumental", "nightcore",
    "sped up", "spedup", "slowed", "reverb", "8 bit", "8-bit", "chipmunk",
    "reaction", "review", "tutorial", "lesson", "how to play", "backing track",
    "mashup", "parody", "loop", "1 hour", "one hour", "full album",
    "acoustic", "unplugged", "guitar cover", "piano cover", "drum cover",
    "remake", "flip", "bootleg", "extended mix", "radio version",
];

/// Common noise tokens dropped before comparing titles.
const STOPWORDS: &[&str] = &[
    "the", "of", "and", "in", "to", "a", "an", "ft", "feat", "featuring",
    "official", "video", "audio", "lyric", "lyrics", "hd", "hq", "mv",
    "music", "with", "prod", "explicit",
];

/// Normalize a string into comparable lowercase tokens (punctuation stripped,
/// stopwords removed, single characters dropped).
pub fn tokens(s: &str) -> Vec<String> {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|w| w.len() > 1)
        .filter(|w| !STOPWORDS.contains(w))
        .map(|w| w.to_string())
        .collect()
}

/// Two tokens are considered equivalent on exact match, or when one contains the
/// other AND both are reasonably long (>= 5 chars) — this handles plurals/tense
/// ("lover"/"lovers") without the false positives short substrings cause
/// (the old code matched "one" inside "money").
fn token_eq(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    a.len() >= 5 && b.len() >= 5 && (a.contains(b) || b.contains(a))
}

/// Split an "Artist - Title" query into (artist, title). If there's no delimiter
/// the whole string is treated as the title with no artist.
pub fn split_artist_title(query: &str) -> (Option<&str>, &str) {
    for delim in [" - ", " – ", " — "] {
        if let Some((a, t)) = query.split_once(delim) {
            let a = a.trim();
            let t = t.trim();
            if !a.is_empty() && !t.is_empty() {
                return (Some(a), t);
            }
        }
    }
    (None, query.trim())
}

/// Fraction (0..1) of the *expected* title's tokens that appear in the candidate.
/// Directional on purpose: we want the wanted song's words to be present, and
/// don't penalize the candidate for extra words like "(Official Video)".
pub fn title_similarity(expected_title: &str, candidate_title: &str) -> f64 {
    let want = tokens(expected_title);
    let have = tokens(candidate_title);
    if want.is_empty() || have.is_empty() {
        return 0.0;
    }
    let matched = want
        .iter()
        .filter(|w| have.iter().any(|h| token_eq(w, h)))
        .count();
    matched as f64 / want.len() as f64
}

/// Does the candidate credit the expected artist? Checks the yt-dlp `artist`
/// field, the uploader/channel, and the video title (covers the "Artist - Title"
/// title convention and YouTube Music "Artist - Topic" channels).
pub fn artist_matches(expected_artist: &str, v: &VideoInfo) -> bool {
    let want = tokens(expected_artist);
    if want.is_empty() {
        return true;
    }
    let mut haystack = String::new();
    if let Some(a) = &v.artist {
        haystack.push_str(a);
        haystack.push(' ');
    }
    if let Some(u) = &v.uploader {
        haystack.push_str(u);
        haystack.push(' ');
    }
    haystack.push_str(&v.title);
    let have = tokens(&haystack);
    // Require that a majority of the artist's tokens appear (handles "The", punctuation).
    let matched = want.iter().filter(|w| have.iter().any(|h| token_eq(w, h))).count();
    matched * 2 >= want.len().max(1)
}

/// True if the candidate title contains an unwanted variant keyword that the
/// query did NOT ask for.
pub fn has_unwanted_variant(query: &str, candidate_title: &str) -> bool {
    let q = query.to_lowercase();
    let c = candidate_title.to_lowercase();
    VARIANT_KEYWORDS
        .iter()
        .any(|kw| c.contains(kw) && !q.contains(kw))
}

/// Is this a clean catalog/official audio source we should prefer?
/// YouTube Music "- Topic" channels and VEVO/official uploads are the cleanest.
fn is_clean_source(v: &VideoInfo) -> bool {
    let uploader = v.uploader.as_deref().unwrap_or("").to_lowercase();
    let title = v.title.to_lowercase();
    uploader.ends_with("- topic")
        || uploader.contains("vevo")
        || title.contains("official audio")
        || v.album.is_some()
}

/// How close a candidate's duration is to the expected one.
/// `strict` (album tracks) allows ±5s; normal allows the larger of 15% or 30s.
pub fn duration_acceptable(expected_secs: f64, candidate_secs: f64, strict: bool) -> bool {
    if expected_secs <= 0.0 || candidate_secs <= 0.0 {
        return true; // can't validate — don't reject on missing data
    }
    let diff = (expected_secs - candidate_secs).abs();
    if strict {
        diff <= 5.0
    } else {
        diff <= (expected_secs * 0.15).max(30.0)
    }
}

/// Minimum composite score required to accept a match (fail & flag otherwise).
const MIN_SCORE: f64 = 0.55;
/// Minimum title similarity gate.
const MIN_TITLE_SIM: f64 = 0.5;
const MIN_TITLE_SIM_STRICT: f64 = 0.67;

/// Pick the best-matching result URL, or `None` if nothing clears the confidence
/// bar. `query` is the "Artist - Title" (or bare title) search string; it drives
/// artist/title/variant checks. `strict` tightens gates for album tracks.
pub fn pick_best_match(
    results: &[VideoInfo],
    query: &str,
    expected_duration_secs: Option<f64>,
    strict: bool,
    exclude_urls: &HashSet<String>,
) -> Option<String> {
    let (expected_artist, expected_title) = split_artist_title(query);
    let expected = expected_duration_secs.filter(|&d| d > 0.0);
    let min_title = if strict { MIN_TITLE_SIM_STRICT } else { MIN_TITLE_SIM };

    let mut best: Option<(&VideoInfo, f64)> = None;
    for r in results {
        // Skip URLs already claimed by another track in this album.
        if let Some(url) = &r.webpage_url {
            if exclude_urls.contains(url) {
                continue;
            }
        } else {
            continue; // no URL, can't download it
        }

        // --- Hard gates ---
        let title_sim = title_similarity(expected_title, &r.title);
        if title_sim < min_title {
            log::info!("[match] reject '{}': title sim {:.2} < {:.2}", r.title, title_sim, min_title);
            continue;
        }
        if let Some(artist) = expected_artist {
            if !artist_matches(artist, r) {
                log::info!("[match] reject '{}': artist '{}' not credited", r.title, artist);
                continue;
            }
        }
        if has_unwanted_variant(query, &r.title) {
            log::info!("[match] reject '{}': unwanted variant not in query", r.title);
            continue;
        }
        if let (Some(exp), Some(cand)) = (expected, r.duration) {
            if !duration_acceptable(exp, cand, strict) {
                log::info!("[match] reject '{}': duration {:.0}s vs {:.0}s", r.title, cand, exp);
                continue;
            }
        }

        // --- Soft score among survivors ---
        let duration_score = match (expected, r.duration) {
            (Some(exp), Some(cand)) if cand > 0.0 => 1.0 - ((exp - cand).abs() / exp).min(1.0),
            (Some(_), _) => 0.3, // expected known but candidate missing duration
            _ => 0.5,
        };
        let artist_score = match expected_artist {
            Some(a) if r.artist.as_deref().is_some_and(|ra| artist_matches(a, r) && !ra.is_empty()) => 1.0,
            Some(_) => 0.6, // matched via title/uploader only
            None => 0.5,
        };
        let bonus = if is_clean_source(r) { 0.1 } else { 0.0 };
        let score = 0.5 * title_sim + 0.3 * artist_score + 0.2 * duration_score + bonus;

        if score >= MIN_SCORE && best.as_ref().map_or(true, |(_, s)| score > *s) {
            best = Some((r, score));
        }
    }

    match best {
        Some((r, score)) => {
            log::info!("[match] best '{}' score {:.2} (dur {:?}s)", r.title, score, expected);
            r.webpage_url.clone()
        }
        None => {
            log::warn!("[match] no candidate cleared the confidence bar for '{}'", query);
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vi(title: &str, artist: Option<&str>, uploader: Option<&str>, dur: Option<f64>) -> VideoInfo {
        VideoInfo {
            title: title.into(),
            track: None,
            artist: artist.map(String::from),
            uploader: uploader.map(String::from),
            duration: dur,
            thumbnail: None,
            webpage_url: Some(format!("https://y/{}", title.replace(' ', "_"))),
            album: None,
            genre: None,
            release_year: None,
            description: None,
            track_number: None,
            disc_number: None,
            composer: None,
            language: None,
            tags: None,
            channel_url: None,
            isrc: None,
        }
    }

    #[test]
    fn token_eq_no_short_substring_false_positive() {
        assert!(!token_eq("one", "money"));
        assert!(token_eq("lover", "lovers"));
        assert!(token_eq("hello", "hello"));
    }

    #[test]
    fn rejects_cover_by_wrong_artist() {
        let results = vec![
            vi("Hello (Cover by Someone)", Some("Someone Else"), Some("Someone Else"), Some(295.0)),
            vi("Adele - Hello", Some("Adele"), Some("Adele - Topic"), Some(295.0)),
        ];
        let url = pick_best_match(&results, "Adele - Hello", Some(295.0), false, &HashSet::new());
        assert_eq!(url, Some("https://y/Adele_-_Hello".into()));
    }

    #[test]
    fn fails_when_only_wrong_artist_available() {
        let results = vec![vi("Hello", Some("Random Coverband"), Some("Random Coverband"), Some(200.0))];
        let url = pick_best_match(&results, "Adele - Hello", Some(295.0), false, &HashSet::new());
        assert_eq!(url, None); // fail & flag rather than grab the cover
    }

    #[test]
    fn rejects_live_variant_when_not_requested() {
        let results = vec![vi("Coldplay - Yellow (Live)", Some("Coldplay"), Some("Coldplay"), Some(270.0))];
        assert_eq!(pick_best_match(&results, "Coldplay - Yellow", None, false, &HashSet::new()), None);
    }

    #[test]
    fn allows_live_when_requested() {
        let results = vec![vi("Coldplay - Yellow (Live)", Some("Coldplay"), Some("Coldplay"), Some(270.0))];
        assert!(pick_best_match(&results, "Coldplay - Yellow Live", None, false, &HashSet::new()).is_some());
    }
}
