//! MusicBrainz lookup for enriching track/album/artist metadata.
//! API docs: https://musicbrainz.org/doc/MusicBrainz_API
//! Rate limit: max 1 request per second, requires User-Agent.

use serde::Deserialize;

const MB_BASE: &str = "https://musicbrainz.org/ws/2";
const USER_AGENT: &str = "Playlist/0.1.0 (https://github.com/sha5b/Playlist)";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(USER_AGENT)
        .build()
        .unwrap_or_default()
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
    #[serde(rename = "primary-type")]
    primary_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MbTag {
    name: Option<String>,
    count: Option<i64>,
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
    pub album_type: Option<String>,
    pub album_release_date: Option<String>,
    pub artist_musicbrainz_id: Option<String>,
    pub artist_sort_name: Option<String>,
    pub artist_type: Option<String>,
    pub artist_country: Option<String>,
    pub artist_begin_year: Option<i64>,
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

/// Search MusicBrainz for a recording and return enrichment data.
pub async fn enrich_track(title: &str, artist: Option<&str>) -> Result<TrackEnrichment, String> {
    // Simple search: just artist + title as plain keywords (most robust for all scripts)
    let query = if let Some(art) = artist {
        format!("{} {}", art, title)
    } else {
        title.to_string()
    };

    let url = format!("{}/recording/?query={}&fmt=json&limit=5", MB_BASE, urlencoding(&query));

    let resp: RecordingSearchResult = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("MusicBrainz request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))?;

    let recordings = resp.recordings.unwrap_or_default();
    let hit = recordings.first().ok_or_else(|| "No MusicBrainz results found".to_string())?;

    let mut enrichment = TrackEnrichment {
        musicbrainz_id: Some(hit.id.clone()),
        release_date: hit.first_release_date.clone(),
        isrc: hit.isrcs.as_ref().and_then(|v| v.first().cloned()),
        description: hit.disambiguation.clone().filter(|s| !s.is_empty()),
        ..Default::default()
    };

    // Genre from tags
    if let Some(tags) = &hit.tags {
        let mut sorted: Vec<_> = tags.iter().filter(|t| t.name.is_some()).collect();
        sorted.sort_by(|a, b| b.count.unwrap_or(0).cmp(&a.count.unwrap_or(0)));
        if let Some(top) = sorted.first() {
            enrichment.genre = top.name.clone();
        }
    }

    // Album info from first release
    if let Some(release) = hit.releases.as_ref().and_then(|r| r.first()) {
        enrichment.album_musicbrainz_id = Some(release.id.clone());
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

    Ok(enrichment)
}

/// Search MusicBrainz for a release (album) and return enrichment data including full tracklist.
pub async fn enrich_album(title: &str, artist: Option<&str>) -> Result<AlbumEnrichment, String> {
    // Simple search: just artist + title as plain keywords
    let query = if let Some(art) = artist {
        format!("{} {}", art, title)
    } else {
        title.to_string()
    };

    let url = format!(
        "{}/release/?query={}&fmt=json&limit=5",
        MB_BASE, urlencoding(&query)
    );

    let resp: ReleaseSearchResult = client()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("MusicBrainz request failed: {}", e))?
        .json()
        .await
        .map_err(|e| format!("Failed to parse MusicBrainz response: {}", e))?;

    let releases = resp.releases.unwrap_or_default();
    let hit = releases.first().ok_or_else(|| "No MusicBrainz results found".to_string())?;

    let mut enrichment = AlbumEnrichment {
        musicbrainz_id: Some(hit.id.clone()),
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
        sorted.sort_by(|a, b| b.count.unwrap_or(0).cmp(&a.count.unwrap_or(0)));
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
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await; // rate limit
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
                        for track in tracks {
                            let num: i64 = track.number.as_ref()
                                .and_then(|n| n.parse::<i64>().ok())
                                .unwrap_or(0);
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
