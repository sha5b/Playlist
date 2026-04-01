use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Track {
    pub id: i64,
    pub title: String,
    pub artist_id: Option<i64>,
    pub album_id: Option<i64>,
    pub album_artist: Option<String>,
    pub duration_ms: Option<i64>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub genre: Option<String>,
    pub year: Option<i64>,
    pub file_path: String,
    pub file_size: Option<i64>,
    pub format: Option<String>,
    pub bitrate: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub cover_art_path: Option<String>,
    pub source_platform: Option<String>,
    pub source_url: Option<String>,
    pub play_count: i64,
    pub last_played_at: Option<String>,
    pub date_added: String,
    pub description: Option<String>,
    pub label: Option<String>,
    pub release_date: Option<String>,
    pub composer: Option<String>,
    pub language: Option<String>,
    pub metadata_completeness: i64,
    // Denormalized for display
    pub artist_name: Option<String>,
    pub album_title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrackPage {
    pub tracks: Vec<Track>,
    pub total: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Artist {
    pub id: i64,
    pub name: String,
    pub sort_name: Option<String>,
    pub musicbrainz_id: Option<String>,
    pub image_path: Option<String>,
    pub bio: Option<String>,
    pub country: Option<String>,
    pub begin_year: Option<i64>,
    pub artist_type: Option<String>,
    pub track_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Album {
    pub id: i64,
    pub title: String,
    pub artist_id: Option<i64>,
    pub album_artist: Option<String>,
    pub year: Option<i64>,
    pub genre: Option<String>,
    pub total_tracks: Option<i64>,
    pub total_discs: Option<i64>,
    pub musicbrainz_id: Option<String>,
    pub cover_art_path: Option<String>,
    pub label: Option<String>,
    pub release_date: Option<String>,
    pub description: Option<String>,
    pub album_type: Option<String>,
    pub artist_name: Option<String>,
    pub track_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Playlist {
    pub id: i64,
    pub name: String,
    pub description: Option<String>,
    pub cover_art_path: Option<String>,
    pub source_platform: Option<String>,
    pub source_url: Option<String>,
    pub track_count: i64,
    pub total_duration_ms: i64,
    pub is_synced: bool,
    pub last_synced_at: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LibraryStats {
    pub total_tracks: i64,
    pub total_albums: i64,
    pub total_artists: i64,
    pub total_playlists: i64,
    pub total_duration_ms: i64,
    pub total_size_bytes: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SearchResults {
    pub tracks: Vec<Track>,
    pub albums: Vec<Album>,
    pub artists: Vec<Artist>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PlaylistDetail {
    pub playlist: Playlist,
    pub tracks: Vec<Track>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Download {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    pub platform: String,
    pub status: String,
    pub progress: f64,
    pub error_message: Option<String>,
    pub file_path: Option<String>,
    pub track_id: Option<i64>,
    pub playlist_id: Option<i64>,
    pub format: String,
    pub quality: String,
    pub created_at: String,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UrlInfo {
    pub platform: String,
    pub url_type: String,
    pub clean_url: String,
    pub title: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SyncResult {
    pub playlist_id: i64,
    pub new_count: i64,
    pub total_count: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct BatchDownloadResult {
    pub queued: i64,
    pub playlist_id: i64,
}
