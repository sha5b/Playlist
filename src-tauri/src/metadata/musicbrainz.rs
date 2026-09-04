//! MusicBrainz lookup for enriching track/album/artist metadata.
//! API docs: https://musicbrainz.org/doc/MusicBrainz_API
//! Rate limit: max 1 request per second, requires User-Agent.

use serde::Deserialize;

/// MusicBrainz enforces max 1 request/second. We use 1100ms for safety margin.
pub const MB_RATE_LIMIT_MS: u64 = 1100;

const MB_BASE: &str = "https://musicbrainz.org/ws/2";
const USER_AGENT: &str = "Playlist/0.9.2 (https://github.com/sha5b/Playlist)";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

/// Global rate gate: serializes ALL MusicBrainz requests process-wide so the
/// combined traffic of concurrent enrichment paths (startup auto-enrich,
/// manual scans, per-track/album/artist enrich) stays under 1 request/second.
/// Per-call-site sleeps can't do this — two paths sleeping in parallel still
/// hit the API at 2 req/s and get throttled.
async fn rate_gate() {
    use std::sync::OnceLock;
    use tokio::sync::Mutex;
    static LAST_REQUEST: OnceLock<Mutex<Option<std::time::Instant>>> = OnceLock::new();

    let gate = LAST_REQUEST.get_or_init(|| Mutex::new(None));
    let mut last = gate.lock().await;
    if let Some(t) = *last {
        let elapsed = t.elapsed();
        let min_gap = std::time::Duration::from_millis(MB_RATE_LIMIT_MS);
        if elapsed < min_gap {
            tokio::time::sleep(min_gap - elapsed).await;
        }
    }
    *last = Some(std::time::Instant::now());
}

// ── Recording search (for tracks) ──────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct RecordingSearchResult {
    recordings: Option<Vec<RecordingHit>>,
}

#[derive(Debug, Deserialize)]
struct RecordingHit {
    id: String,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<ArtistCredit>>,
    releases: Option<Vec<ReleaseRef>>,
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
    isrcs: Option<Vec<String>>,
    tags: Option<Vec<MbTag>>,
    disambiguation: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ArtistCredit {
    artist: ArtistRef,
}

#[derive(Debug, Deserialize)]
struct ArtistRef {
    id: String,
    name: Option<String>,
    #[serde(rename = "sort-name")]
    sort_name: Option<String>,
    #[serde(rename = "type")]
    artist_type: Option<String>,
    country: Option<String>,
    #[serde(rename = "life-span")]
    life_span: Option<LifeSpan>,
}

#[derive(Debug, Deserialize)]
struct LifeSpan {
    begin: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReleaseRef {
    id: String,
    date: Option<String>,
    #[serde(rename = "release-group")]
    release_group: Option<ReleaseGroupRef>,
}

#[derive(Debug, Deserialize)]
struct ReleaseGroupRef {
    id: Option<String>,
    #[serde(rename = "primary-type")]
    primary_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbTag {
    name: Option<String>,
    count: Option<i64>,
}

// ── Recording URL relations (for music video detection) ───────────────────

#[derive(Debug, Deserialize)]
struct RecordingDetail {
    relations: Option<Vec<MbRelation>>,
}

#[derive(Debug, Deserialize)]
struct MbRelation {
    #[serde(rename = "type")]
    relation_type: Option<String>,
    url: Option<MbUrl>,
}

#[derive(Debug, Deserialize)]
struct MbUrl {
    resource: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RecordingArtistRels {
    relations: Option<Vec<MbArtistRelation>>,
}

#[derive(Debug, Deserialize)]
struct MbArtistRelation {
    #[serde(rename = "type")]
    relation_type: Option<String>,
    artist: Option<MbRelArtist>,
}

#[derive(Debug, Deserialize)]
struct MbRelArtist {
    name: Option<String>,
}

/// Look up a recording's artist relations to find the composer.
async fn lookup_composer(recording_mbid: &str) -> Option<String> {
    rate_gate().await;

    let url = format!(
        "{}/recording/{}?inc=artist-rels&fmt=json",
        MB_BASE, recording_mbid
    );
    let resp = client().get(&url).send().await.ok()?;
    let detail: RecordingArtistRels = resp.json().await.ok()?;

    let relations = detail.relations?;
    for rel in &relations {
        if rel.relation_type.as_deref() == Some("composer") {
            if let Some(artist) = &rel.artist {
                return artist.name.clone();
            }
        }
    }
    None
}

/// Look up a recording's URL relations to find a confirmed music video link.
pub async fn lookup_music_video_url(recording_mbid: &str) -> Option<String> {
    // Rate limit: 1 req/sec
    rate_gate().await;

    let url = format!(
        "{}/recording/{}?inc=url-rels&fmt=json",
        MB_BASE, recording_mbid
    );
    let resp = client().get(&url).send().await.ok()?;
    let detail: RecordingDetail = resp.json().await.ok()?;

    let relations = detail.relations?;
    // Look for streaming/video relations pointing to video platforms.
    // Real MusicBrainz relation types are "streaming" and "free streaming"
    // ("streaming music" doesn't exist and never matched).
    for rel in &relations {
        let rtype = rel.relation_type.as_deref().unwrap_or("");
        let resource = rel.url.as_ref().and_then(|u| u.resource.as_deref()).unwrap_or("");

        // Check if the URL points to a video platform
        let is_video_url = resource.contains("youtube.com/watch")
            || resource.contains("youtu.be/")
            || resource.contains("vimeo.com/");

        if is_video_url && (rtype == "free streaming" || rtype == "streaming") {
            return Some(resource.to_string());
        }
    }

    // Fallback: a YouTube link typed as a video/stream of THIS recording.
    // Returning any untyped YouTube URL grabbed lyric videos and audio
    // uploads, which then got persisted as the "official" music video.
    for rel in &relations {
        let rtype = rel.relation_type.as_deref().unwrap_or("");
        let resource = rel.url.as_ref().and_then(|u| u.resource.as_deref()).unwrap_or("");
        let is_video_url = resource.contains("youtube.com/watch") || resource.contains("youtu.be/");
        if is_video_url
            && matches!(rtype, "streaming" | "free streaming" | "video" | "lyric video" | "official video")
        {
            return Some(resource.to_string());
        }
    }

    None
}

/// Look up an artist's URL relations to find their official website.
async fn lookup_artist_website(artist_mbid: &str) -> Option<String> {
    rate_gate().await;
    let url = format!("{}/artist/{}?inc=url-rels&fmt=json", MB_BASE, artist_mbid);
    let resp = client().get(&url).send().await.ok()?;
    let detail: RecordingDetail = resp.json().await.ok()?;
    let relations = detail.relations?;

    // Prefer "official homepage", fall back to any social/streaming profile
    for rel in &relations {
        let rtype = rel.relation_type.as_deref().unwrap_or("");
        if rtype == "official homepage" {
            return rel.url.as_ref().and_then(|u| u.resource.clone());
        }
    }
    // Fallback: social media or wiki
    for rel in &relations {
        let rtype = rel.relation_type.as_deref().unwrap_or("");
        let resource = rel.url.as_ref().and_then(|u| u.resource.as_deref()).unwrap_or("");
        if rtype == "social network" && (resource.contains("instagram.com") || resource.contains("twitter.com") || resource.contains("x.com") || resource.contains("facebook.com")) {
            return Some(resource.to_string());
        }
    }
    None
}

/// Look up a release's URL relations to find purchase/streaming links.
async fn lookup_album_purchase_url(release_mbid: &str) -> Option<String> {
    rate_gate().await;
    let url = format!("{}/release/{}?inc=url-rels&fmt=json", MB_BASE, release_mbid);
    let resp = client().get(&url).send().await.ok()?;
    let detail: RecordingDetail = resp.json().await.ok()?;
    let relations = detail.relations?;

    // Prefer purchase links, then streaming
    for rel in &relations {
        let rtype = rel.relation_type.as_deref().unwrap_or("");
        if rtype == "purchase for download" || rtype == "purchase for mail-order" || rtype == "mail-order" {
            return rel.url.as_ref().and_then(|u| u.resource.clone());
        }
    }
    // Fallback: Amazon, Bandcamp, or any streaming link
    for rel in &relations {
        let resource = rel.url.as_ref().and_then(|u| u.resource.as_deref()).unwrap_or("");
        if resource.contains("amazon.") || resource.contains("bandcamp.com") {
            return Some(resource.to_string());
        }
    }
    for rel in &relations {
        let rtype = rel.relation_type.as_deref().unwrap_or("");
        if rtype == "streaming" || rtype == "free streaming" {
            return rel.url.as_ref().and_then(|u| u.resource.clone());
        }
    }
    None
}

// ── Release lookup (for albums) ────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ReleaseSearchResult {
    releases: Option<Vec<ReleaseHit>>,
}

#[derive(Debug, Deserialize)]
struct ReleaseHit {
    id: String,
    date: Option<String>,
    #[serde(rename = "label-info")]
    label_info: Option<Vec<LabelInfo>>,
    #[serde(rename = "artist-credit")]
    artist_credit: Option<Vec<ArtistCredit>>,
    #[serde(rename = "release-group")]
    release_group: Option<ReleaseGroupRef>,
    #[serde(rename = "track-count")]
    track_count: Option<i64>,
    tags: Option<Vec<MbTag>>,
    media: Option<Vec<MbMedia>>,
}

#[derive(Debug, Deserialize)]
struct LabelInfo {
    label: Option<LabelRef>,
}

#[derive(Debug, Deserialize)]
struct LabelRef {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbMedia {
    position: Option<i64>,
    tracks: Option<Vec<MbTrack>>,
}

#[derive(Debug, Deserialize)]
struct MbTrack {
    /// Numeric position within the medium — always an integer, unlike
    /// `number`, which is free-form ("A1", "B2" on vinyl).
    position: Option<i64>,
    number: Option<String>,
    title: Option<String>,
    length: Option<i64>,
}

// ── Public result types ────────────────────────────────────────────────────

#[derive(Debug, Default, Clone)]
pub struct TrackEnrichment {
    pub musicbrainz_id: Option<String>,
    pub genre: Option<String>,
    pub release_date: Option<String>,
    pub isrc: Option<String>,
    pub description: Option<String>,
    pub label: Option<String>,
    pub language: Option<String>,
    pub album_musicbrainz_id: Option<String>,
    /// Release-group MBID for the album (what belongs in `albums.musicbrainz_id`)
    pub album_release_group_mbid: Option<String>,
    pub album_type: Option<String>,
    pub album_release_date: Option<String>,
    pub artist_musicbrainz_id: Option<String>,
    pub artist_sort_name: Option<String>,
    pub artist_type: Option<String>,
    pub artist_country: Option<String>,
    pub artist_begin_year: Option<i64>,
    pub tags: Option<Vec<String>>,
    /// Music video URL from MusicBrainz URL relations (confirmed to exist)
    pub music_video_url: Option<String>,
    /// Artist official homepage from MusicBrainz URL relations
    pub artist_website_url: Option<String>,
    /// Album purchase URL from MusicBrainz URL relations
    pub album_purchase_url: Option<String>,
    /// Composer name from MusicBrainz artist relations
    pub composer: Option<String>,
}

#[derive(Debug, Default, Clone)]
pub struct AlbumEnrichment {
    pub musicbrainz_id: Option<String>,
    pub release_date: Option<String>,
    pub label: Option<String>,
    pub album_type: Option<String>,
    pub genre: Option<String>,
    pub total_tracks: Option<i64>,
    pub total_discs: Option<i64>,
    pub tracklist: Vec<AlbumTrackInfo>,
    pub artist_musicbrainz_id: Option<String>,
    pub artist_sort_name: Option<String>,
    pub artist_type: Option<String>,
    pub artist_country: Option<String>,
    pub artist_begin_year: Option<i64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AlbumTrackInfo {
    pub disc_number: i64,
    pub track_number: i64,
    pub title: String,
    pub duration_ms: Option<i64>,
}

// ── Search & enrich functions ──────────────────────────────────────────────

/// Strip Lucene special characters from search terms so field-specific queries
/// don't break on titles containing parentheses, colons, etc.
fn strip_lucene_special(s: &str) -> String {
    s.chars()
        .filter(|c| !matches!(c, '+' | '-' | '!' | '(' | ')' | '{' | '}' | '[' | ']'
                                  | '^' | '"' | '~' | '*' | '?' | ':' | '\\' | '/'))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Normalize a string for fuzzy comparison: lowercase, strip punctuation/whitespace.
fn normalize_name(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Check if two artist names are a fuzzy match.
fn artist_matches(query_artist: &str, result_artist: &str) -> bool {
    let q = normalize_name(query_artist);
    let r = normalize_name(result_artist);
    if q.is_empty() || r.is_empty() {
        return false;
    }
    // Exact match or one contains the other (handles "Artist" vs "Artist feat. X")
    q == r || r.contains(&q) || q.contains(&r)
}

/// Pick the best recording from search results by verifying the artist name matches.
fn best_recording<'a>(recordings: &'a [RecordingHit], artist: Option<&str>) -> Option<&'a RecordingHit> {
    if recordings.is_empty() {
        return None;
    }
    if let Some(query_artist) = artist {
        // Only accept results whose artist matches the query. Falling back to
        // the first hit would attach ANOTHER artist's recording: wrong MBID,
        // ISRC and release date get written permanently (fields only fill
        // when NULL), and the wrong artist's data lands on the artist row.
        for rec in recordings {
            if let Some(ac) = rec.artist_credit.as_ref().and_then(|v| v.first()) {
                if let Some(ref name) = ac.artist.name {
                    if artist_matches(query_artist, name) {
                        return Some(rec);
                    }
                }
            }
        }
        return None;
    }
    // No artist filter — first result is the best we can do
    Some(&recordings[0])
}

/// Pick the best release from search results by verifying the artist name matches.
fn best_release<'a>(releases: &'a [ReleaseHit], artist: Option<&str>) -> Option<&'a ReleaseHit> {
    if releases.is_empty() {
        return None;
    }
    if let Some(query_artist) = artist {
        // See best_recording: never fall back to another artist's release.
        for rel in releases {
            if let Some(ac) = rel.artist_credit.as_ref().and_then(|v| v.first()) {
                if let Some(ref name) = ac.artist.name {
                    if artist_matches(query_artist, name) {
                        return Some(rel);
                    }
                }
            }
        }
        return None;
    }
    Some(&releases[0])
}

/// Search MusicBrainz for a recording and return enrichment data, including
/// the per-recording detail lookups (composer, website, purchase URLs, video).
pub async fn enrich_track(title: &str, artist: Option<&str>) -> Result<TrackEnrichment, String> {
    enrich_track_inner(title, artist, true).await
}

/// Search-only variant for bulk scans: skips the four rate-gated detail
/// lookups (≈4.4s/track of gate time) whose results the scans never store.
pub async fn enrich_track_search_only(title: &str, artist: Option<&str>) -> Result<TrackEnrichment, String> {
    enrich_track_inner(title, artist, false).await
}

async fn enrich_track_inner(title: &str, artist: Option<&str>, include_details: bool) -> Result<TrackEnrichment, String> {
    // Use field-specific Lucene query when artist is known for better precision.
    // No phrase quotes — allows fuzzy word matching within each field.
    let query = if let Some(art) = artist {
        format!("recording:({}) AND artist:({})", strip_lucene_special(title), strip_lucene_special(art))
    } else {
        strip_lucene_special(title)
    };

    let url = format!("{}/recording/?query={}&fmt=json&limit=5", MB_BASE, urlencoding(&query));

    rate_gate().await;
    let resp: RecordingSearchResult = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("MusicBrainz request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))?;

    let recordings = resp.recordings.unwrap_or_default();
    let hit = best_recording(&recordings, artist)
        .ok_or_else(|| "No MusicBrainz results found".to_string())?;

    let mut enrichment = TrackEnrichment {
        musicbrainz_id: Some(hit.id.clone()),
        release_date: hit.first_release_date.clone(),
        isrc: hit.isrcs.as_ref().and_then(|v| v.first().cloned()),
        description: hit.disambiguation.clone().filter(|s| !s.is_empty()),
        ..Default::default()
    };

    // Genre and tags from MusicBrainz tags
    if let Some(tags) = &hit.tags {
        let mut sorted: Vec<_> = tags.iter().filter(|t| t.name.is_some()).collect();
        sorted.sort_by_key(|tag| std::cmp::Reverse(tag.count.unwrap_or(0)));
        if let Some(top) = sorted.first() {
            enrichment.genre = top.name.clone();
        }
        let tag_names: Vec<String> = sorted.iter()
            .filter_map(|t| t.name.clone())
            .collect();
        if !tag_names.is_empty() {
            enrichment.tags = Some(tag_names);
        }
    }

    // Album info: MusicBrainz orders a recording's releases arbitrarily, so
    // "first" could be a compilation. Prefer a proper Album release, break
    // ties by earliest date (the original release), fall back to the first.
    let picked_release = hit.releases.as_ref().and_then(|releases| {
        releases
            .iter()
            .filter(|r| {
                r.release_group.as_ref()
                    .and_then(|rg| rg.primary_type.as_deref())
                    == Some("Album")
            })
            .min_by(|a, b| {
                a.date.as_deref().unwrap_or("9999")
                    .cmp(b.date.as_deref().unwrap_or("9999"))
            })
            .or_else(|| releases.first())
    });
    if let Some(release) = picked_release {
        // Keep the release id for the purchase-URL lookup, but expose the
        // release-group id for the albums table (see enrich_album note).
        enrichment.album_musicbrainz_id = Some(release.id.clone());
        enrichment.album_release_group_mbid = release
            .release_group
            .as_ref()
            .and_then(|rg| rg.id.clone());
        enrichment.album_release_date = release.date.clone();
        enrichment.album_type = release.release_group.as_ref()
            .and_then(|rg| rg.primary_type.clone());
    }

    // Artist info
    if let Some(ac) = hit.artist_credit.as_ref().and_then(|v| v.first()) {
        let a = &ac.artist;
        enrichment.artist_musicbrainz_id = Some(a.id.clone());
        enrichment.artist_sort_name = a.sort_name.clone();
        enrichment.artist_type = a.artist_type.clone();
        enrichment.artist_country = a.country.clone();
        enrichment.artist_begin_year = a.life_span.as_ref()
            .and_then(|ls| ls.begin.as_ref())
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse::<i64>().ok());
    }

    // Detail lookups — one rate-gated MusicBrainz request each. Skipped by
    // bulk scans, which don't store these fields.
    if include_details {
        // Check for confirmed music video via MusicBrainz URL relations
        enrichment.music_video_url = lookup_music_video_url(&hit.id).await;

        // Look up composer from artist relations
        enrichment.composer = lookup_composer(&hit.id).await;

        // Look up artist website if we have an artist MBID
        if let Some(ref artist_mbid) = enrichment.artist_musicbrainz_id {
            enrichment.artist_website_url = lookup_artist_website(artist_mbid).await;
        }

        // Look up album purchase URL if we have a release MBID
        if let Some(ref album_mbid) = enrichment.album_musicbrainz_id {
            enrichment.album_purchase_url = lookup_album_purchase_url(album_mbid).await;
        }
    }

    Ok(enrichment)
}

/// Search MusicBrainz for a release (album) and return enrichment data including full tracklist.
pub async fn enrich_album(title: &str, artist: Option<&str>) -> Result<AlbumEnrichment, String> {
    // Use field-specific Lucene query when artist is known for better precision.
    // No phrase quotes — allows fuzzy word matching within each field.
    let query = if let Some(art) = artist {
        format!("release:({}) AND artist:({})", strip_lucene_special(title), strip_lucene_special(art))
    } else {
        strip_lucene_special(title)
    };

    let url = format!(
        "{}/release/?query={}&fmt=json&limit=5",
        MB_BASE, urlencoding(&query)
    );

    rate_gate().await;
    let resp: ReleaseSearchResult = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("MusicBrainz request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))?;

    let releases = resp.releases.unwrap_or_default();
    let hit = best_release(&releases, artist)
        .ok_or_else(|| "No MusicBrainz results found".to_string())?;

    let mut enrichment = AlbumEnrichment {
        // Store the RELEASE-GROUP mbid, not the release id — "missing albums"
        // detection compares albums.musicbrainz_id against release-group ids
        // from the discography, so a release id here never matches and the
        // album would be re-downloaded forever.
        musicbrainz_id: hit.release_group.as_ref().and_then(|rg| rg.id.clone())
            .or_else(|| Some(hit.id.clone())),
        release_date: hit.date.clone(),
        album_type: hit.release_group.as_ref().and_then(|rg| rg.primary_type.clone()),
        total_tracks: hit.track_count,
        ..Default::default()
    };

    // Label
    if let Some(li) = hit.label_info.as_ref().and_then(|v| v.first()) {
        enrichment.label = li.label.as_ref().and_then(|l| l.name.clone());
    }

    // Genre from tags
    if let Some(tags) = &hit.tags {
        let mut sorted: Vec<_> = tags.iter().filter(|t| t.name.is_some()).collect();
        sorted.sort_by_key(|tag| std::cmp::Reverse(tag.count.unwrap_or(0)));
        if let Some(top) = sorted.first() {
            enrichment.genre = top.name.clone();
        }
    }

    // Artist info
    if let Some(ac) = hit.artist_credit.as_ref().and_then(|v| v.first()) {
        let a = &ac.artist;
        enrichment.artist_musicbrainz_id = Some(a.id.clone());
        enrichment.artist_sort_name = a.sort_name.clone();
        enrichment.artist_type = a.artist_type.clone();
        enrichment.artist_country = a.country.clone();
        enrichment.artist_begin_year = a.life_span.as_ref()
            .and_then(|ls| ls.begin.as_ref())
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse::<i64>().ok());
    }

    // Now fetch full release with media for tracklist
    rate_gate().await;
    let detail_url = format!(
        "{}/release/{}?inc=recordings+artist-credits&fmt=json",
        MB_BASE, hit.id
    );
    if let Ok(detail_resp) = client().get(&detail_url).send().await {
        if let Ok(detail) = detail_resp.json::<ReleaseHit>().await {
            let mut tracklist = Vec::new();
            let mut total_discs = 0i64;
            let mut total_tracks = 0i64;
            if let Some(media) = detail.media {
                for medium in &media {
                    let disc = medium.position.unwrap_or(1);
                    if disc > total_discs { total_discs = disc; }
                    if let Some(tracks) = &medium.tracks {
                        for (idx, track) in tracks.iter().enumerate() {
                            let num: i64 = track.position
                                .or_else(|| track.number.as_ref()
                                    .and_then(|n| n.parse::<i64>().ok()))
                                .unwrap_or(idx as i64 + 1);
                            tracklist.push(AlbumTrackInfo {
                                disc_number: disc,
                                track_number: num,
                                title: track.title.clone().unwrap_or_default(),
                                duration_ms: track.length,
                            });
                            total_tracks += 1;
                        }
                    }
                }
            }
            enrichment.tracklist = tracklist;
            if total_discs > 0 { enrichment.total_discs = Some(total_discs); }
            if total_tracks > 0 { enrichment.total_tracks = Some(total_tracks); }
        }
    }

    Ok(enrichment)
}

// ── Artist discography ────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ReleaseGroupSearchResult {
    #[serde(rename = "release-groups")]
    release_groups: Option<Vec<ReleaseGroupHit>>,
}

#[derive(Debug, Deserialize)]
struct ReleaseGroupHit {
    id: String,
    title: Option<String>,
    #[serde(rename = "primary-type")]
    primary_type: Option<String>,
    #[serde(rename = "first-release-date")]
    first_release_date: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct ArtistDiscographyEntry {
    pub mbid: String,
    pub title: String,
    pub album_type: Option<String>,
    pub year: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct ArtistSearchResult {
    artists: Option<Vec<ArtistSearchHit>>,
}

#[derive(Debug, Deserialize)]
struct ArtistSearchHit {
    id: String,
}

/// Search MusicBrainz for an artist by name and return their MBID.
pub async fn search_artist(name: &str) -> Result<String, String> {
    let url = format!("{}/artist/?query={}&fmt=json&limit=5", MB_BASE, urlencoding(name));
    rate_gate().await;
    let resp: ArtistSearchResult = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("MusicBrainz request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let artists = resp.artists.unwrap_or_default();
    let hit = artists.first().ok_or_else(|| "No MusicBrainz artist results".to_string())?;
    Ok(hit.id.clone())
}

/// Fetch an artist's full discography (albums, EPs, singles) from MusicBrainz.
pub async fn get_artist_discography(mbid: &str) -> Result<Vec<ArtistDiscographyEntry>, String> {
    let url = format!(
        "{}/release-group?artist={}&type=album|ep|single&fmt=json&limit=100",
        MB_BASE, mbid
    );
    rate_gate().await;
    let resp: ReleaseGroupSearchResult = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("MusicBrainz request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let groups = resp.release_groups.unwrap_or_default();
    let entries: Vec<ArtistDiscographyEntry> = groups.into_iter().map(|rg| {
        let year = rg.first_release_date.as_ref()
            .and_then(|d| d.get(..4))
            .and_then(|y| y.parse::<i64>().ok());
        ArtistDiscographyEntry {
            mbid: rg.id,
            title: rg.title.unwrap_or_default(),
            album_type: rg.primary_type,
            year,
        }
    }).collect();

    Ok(entries)
}

// ── Cover Art Archive ─────────────────────────────────────────────────────

/// Download cover art bytes from Cover Art Archive.
pub async fn download_cover_art(release_mbid: &str) -> Option<Vec<u8>> {
    let url = format!("https://coverartarchive.org/release/{}/front-500", release_mbid);
    match client().get(&url).send().await {
        Ok(resp) if resp.status().is_success() => {
            resp.bytes().await.ok().map(|b| b.to_vec())
        }
        _ => None,
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn urlencoding(s: &str) -> String {
    // Encode each UTF-8 byte (not Unicode code point) for correct percent-encoding
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b' ' => {
                if b == b' ' { "+".to_string() } else { (b as char).to_string() }
            }
            _ => format!("%{:02X}", b),
        })
        .collect()
}
