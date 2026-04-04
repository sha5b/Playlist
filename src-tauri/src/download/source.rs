use std::fmt;
use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;

use crate::db::DbPool;

/// Error types for audio source operations
#[derive(Debug)]
pub enum SourceError {
    /// Source is not configured (missing credentials)
    NotConfigured,
    /// Authentication failed (bad credentials)
    AuthFailed(String),
    /// Track not found on this platform
    NotFound,
    /// DRM prevented download
    DrmBlocked(String),
    /// Network / HTTP error
    NetworkError(String),
    /// Generic error
    Other(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SourceError::NotConfigured => write!(f, "Source not configured"),
            SourceError::AuthFailed(msg) => write!(f, "Auth failed: {}", msg),
            SourceError::NotFound => write!(f, "Track not found"),
            SourceError::DrmBlocked(msg) => write!(f, "DRM blocked: {}", msg),
            SourceError::NetworkError(msg) => write!(f, "Network error: {}", msg),
            SourceError::Other(msg) => write!(f, "{}", msg),
        }
    }
}

/// Trait for pluggable audio download sources
#[async_trait]
pub trait AudioSource: Send + Sync {
    /// Check if this source has valid credentials configured
    fn is_available(&self) -> bool;

    /// Platform identifier (e.g. "spotify", "deezer")
    fn platform(&self) -> &str;

    /// Download a track by platform URL, return path to downloaded file
    async fn download(
        &self,
        url: &str,
        output_dir: &Path,
        file_stem: &str,
        format: &str,
        progress: Box<dyn Fn(f64) + Send + Sync>,
    ) -> Result<String, SourceError>;

    /// Search for a track by query and download it
    async fn search_download(
        &self,
        query: &str,
        output_dir: &Path,
        file_stem: &str,
        format: &str,
        progress: Box<dyn Fn(f64) + Send + Sync>,
    ) -> Result<String, SourceError>;

    /// Test the connection / credentials
    async fn test_connection(&self) -> Result<(), SourceError>;
}

/// Status info for a source (serialized to frontend)
#[derive(Debug, serde::Serialize, Clone)]
pub struct SourceStatus {
    pub platform: String,
    pub configured: bool,
    pub available: bool,
    pub max_quality: String,
    pub error: Option<String>,
}

/// Registry of all configured audio sources, ordered by quality preference
pub struct SourceRegistry {
    sources: Vec<Box<dyn AudioSource>>,
}

impl SourceRegistry {
    /// Build the registry from current settings
    pub fn from_settings(db: &Arc<DbPool>) -> Self {
        let mut sources: Vec<Box<dyn AudioSource>> = Vec::new();

        let conn = match crate::db::lock(db) {
            Ok(c) => c,
            Err(_) => return Self { sources },
        };

        // Deezer — FLAC CD quality (highest priority for lossless)
        let deezer_arl = crate::db::settings::get_setting(&conn, "deezer_arl")
            .ok()
            .flatten()
            .filter(|s| !s.is_empty());
        if let Some(arl) = deezer_arl {
            sources.push(Box::new(super::deezer::DeezerSource::new(arl)));
        }

        // Spotify — disabled: Spotify deprecated username/password auth (forced OAuth2 mid-2025).
        // Native librespot downloads no longer work. Spotify URLs are handled by the
        // fallback chain (yt-dlp metadata extraction → Deezer search → YouTube search).
        // Keeping the config fields in settings so credentials aren't lost if OAuth support
        // is added later.

        Self { sources }
    }

    /// Get status of all known sources (configured or not)
    pub fn get_statuses(&self, db: &Arc<DbPool>) -> Vec<SourceStatus> {
        let conn = match crate::db::lock(db) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let has_spotify = crate::db::settings::get_setting(&conn, "spotify_username")
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
            .is_some();
        let has_deezer = crate::db::settings::get_setting(&conn, "deezer_arl")
            .ok()
            .flatten()
            .filter(|s| !s.is_empty())
            .is_some();

        vec![
            SourceStatus {
                platform: "spotify".into(),
                configured: has_spotify,
                available: false,
                max_quality: "OGG 320kbps".into(),
                error: Some("Spotify deprecated password auth. Spotify URLs are searched via Deezer/YouTube instead.".into()),
            },
            SourceStatus {
                platform: "deezer".into(),
                configured: has_deezer,
                available: self.sources.iter().any(|s| s.platform() == "deezer" && s.is_available()),
                max_quality: "FLAC CD (16-bit/44.1kHz)".into(),
                error: None,
            },
        ]
    }

    /// Find a source for the given platform
    pub fn get_for_platform(&self, platform: &str) -> Option<&dyn AudioSource> {
        self.sources
            .iter()
            .find(|s| s.platform() == platform && s.is_available())
            .map(|s| s.as_ref())
    }

    /// Get the best available source for cross-platform search (prefers lossless)
    pub fn get_best_search_source(&self) -> Option<&dyn AudioSource> {
        self.sources
            .iter()
            .find(|s| s.is_available())
            .map(|s| s.as_ref())
    }

    /// Test a specific source's connection
    pub async fn test_source(&self, platform: &str) -> Result<(), SourceError> {
        match self.get_for_platform(platform) {
            Some(source) => source.test_connection().await,
            None => Err(SourceError::NotConfigured),
        }
    }

    /// Fetch playlist entries from a DRM platform using native API.
    /// Returns entries in the same format as ytdlp::PlaylistFetchResult.
    pub async fn fetch_playlist_entries(
        &self,
        platform: &str,
        url: &str,
    ) -> Result<super::ytdlp::PlaylistFetchResult, SourceError> {
        match platform {
            "spotify" => {
                // Spotify's embed API works for public playlists without credentials
                super::spotify::fetch_playlist_entries(url).await
            }
            "deezer" => {
                super::deezer::fetch_playlist_entries(url).await
            }
            _ => Err(SourceError::Other(format!("No native playlist support for {}", platform))),
        }
    }
}
