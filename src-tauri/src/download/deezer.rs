use std::io::Write;
use std::path::Path;

use async_trait::async_trait;
use blowfish::cipher::{BlockDecryptMut, KeyIvInit};
use md5::{Digest, Md5};
use reqwest::Client;

use super::source::{AudioSource, SourceError};

/// Deezer API base URL
const DEEZER_API: &str = "https://api.deezer.com";
/// Deezer internal (private) API
const DEEZER_PRIVATE_API: &str = "https://www.deezer.com/ajax/gw-light.php";
/// The secret used to derive per-track Blowfish keys (from deemix/d-fi)
const BLOWFISH_SECRET: &[u8] = b"g4el58wc0zvf9na1";

type BlowfishCbcDec = cbc::Decryptor<blowfish::Blowfish>;

pub struct DeezerSource {
    arl: String,
    client: Client,
}

impl DeezerSource {
    pub fn new(arl: String) -> Self {
        let client = Client::builder()
            .cookie_store(true)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self { arl, client }
    }

    /// Initialize a session with the ARL cookie and get the API token
    async fn get_api_token(&self) -> Result<(String, String), SourceError> {
        // Set the ARL cookie by calling the empty API endpoint
        let resp = self
            .client
            .post(DEEZER_PRIVATE_API)
            .query(&[
                ("method", "deezer.getUserData"),
                ("input", "3"),
                ("api_version", "1.0"),
                ("api_token", ""),
            ])
            .header("Cookie", format!("arl={}", self.arl))
            .send()
            .await
            .map_err(|e| SourceError::NetworkError(format!("Deezer API error: {}", e)))?;

        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SourceError::NetworkError(format!("Failed to parse response: {}", e)))?;

        let api_token = body["results"]["checkForm"]
            .as_str()
            .ok_or_else(|| SourceError::AuthFailed("Invalid ARL — could not get API token".into()))?
            .to_string();

        let user_id_value = &body["results"]["USER"]["USER_ID"];
        let is_valid_user = user_id_value
            .as_str()
            .is_some_and(|s| !s.is_empty() && s != "0")
            || user_id_value.as_i64().is_some_and(|id| id != 0);

        if !is_valid_user {
            return Err(SourceError::AuthFailed(
                "ARL expired or invalid — please update your Deezer ARL cookie".into(),
            ));
        }

        Ok((api_token, self.arl.clone()))
    }

    /// Get track info from Deezer's private API
    async fn get_track_info(
        &self,
        api_token: &str,
        track_id: &str,
    ) -> Result<DeezerTrackInfo, SourceError> {
        let body = serde_json::json!({
            "sng_id": track_id,
        });

        let resp = self
            .client
            .post(DEEZER_PRIVATE_API)
            .query(&[
                ("method", "song.getData"),
                ("input", "3"),
                ("api_version", "1.0"),
                ("api_token", api_token),
            ])
            .header("Cookie", format!("arl={}", self.arl))
            .json(&body)
            .send()
            .await
            .map_err(|e| SourceError::NetworkError(format!("Track info error: {}", e)))?;

        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SourceError::NetworkError(format!("Parse error: {}", e)))?;

        let results = &data["results"];
        if results.is_null() {
            return Err(SourceError::NotFound);
        }

        let md5_origin = results["MD5_ORIGIN"].as_str().unwrap_or("").to_string();
        if md5_origin.is_empty() {
            // Without MD5_ORIGIN we cannot derive the CDN URL — happens for
            // tracks that require track tokens or a higher-tier subscription.
            return Err(SourceError::DrmBlocked(
                "Deezer did not return file info for this track (MD5_ORIGIN missing)".into(),
            ));
        }

        Ok(DeezerTrackInfo {
            id: results["SNG_ID"]
                .as_str()
                .unwrap_or(track_id)
                .to_string(),
            md5_origin,
            media_version: results["MEDIA_VERSION"]
                .as_str()
                .unwrap_or("1")
                .to_string(),
        })
    }

    /// Search for a track on Deezer's public API
    async fn search_track(&self, query: &str) -> Result<String, SourceError> {
        let resp = self
            .client
            .get(format!("{}/search/track", DEEZER_API))
            .query(&[("q", query), ("limit", "1")])
            .send()
            .await
            .map_err(|e| SourceError::NetworkError(format!("Search error: {}", e)))?;

        let data: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| SourceError::NetworkError(format!("Parse error: {}", e)))?;

        let track_id = data["data"][0]["id"]
            .as_i64()
            .ok_or(SourceError::NotFound)?;

        Ok(track_id.to_string())
    }

    /// Build the CDN URL for a track (deemix/d-fi "legacy stream URL" scheme).
    ///
    /// The fields are joined with the single latin-1 byte 0xA4 ('¤') — NOT the
    /// UTF-8 encoding of '¤' (0xC2 0xA4), which would produce a different MD5
    /// and AES input and therefore a dead CDN URL.
    fn build_cdn_url(track_info: &DeezerTrackInfo, quality: u8) -> String {
        const SEP: u8 = 0xA4;

        // quality: 1 = MP3 128, 3 = MP3 320, 9 = FLAC
        let quality_str = quality.to_string();
        let parts = [
            track_info.md5_origin.as_bytes(),
            quality_str.as_bytes(),
            track_info.id.as_bytes(),
            track_info.media_version.as_bytes(),
        ];
        let mut step1: Vec<u8> = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                step1.push(SEP);
            }
            step1.extend_from_slice(part);
        }

        let md5_hash = format!("{:x}", Md5::digest(&step1));

        // step2 = md5 ¤ step1 ¤, space-padded to a 16-byte boundary
        let mut aes_input: Vec<u8> = Vec::with_capacity(md5_hash.len() + step1.len() + 18);
        aes_input.extend_from_slice(md5_hash.as_bytes());
        aes_input.push(SEP);
        aes_input.extend_from_slice(&step1);
        aes_input.push(SEP);
        while aes_input.len() % 16 != 0 {
            aes_input.push(b' ');
        }

        use aes::cipher::{BlockEncrypt, KeyInit};
        let key: [u8; 16] = {
            let k = b"jo6aey6haid2Teih";
            let mut arr = [0u8; 16];
            arr.copy_from_slice(k);
            arr
        };
        let cipher = aes::Aes128::new(&key.into());
        let mut encrypted = Vec::new();
        for chunk in aes_input.chunks(16) {
            let mut block = aes::cipher::generic_array::GenericArray::clone_from_slice(chunk);
            cipher.encrypt_block(&mut block);
            encrypted.extend_from_slice(&block);
        }
        let path = hex::encode(&encrypted);

        // md5_origin is guaranteed non-empty by get_track_info
        let cdn_shard = track_info.md5_origin.get(..1).unwrap_or("0");
        format!(
            "https://e-cdns-proxy-{}.dzcdn.net/mobile/1/{}",
            cdn_shard, path
        )
    }

    /// Derive the per-track Blowfish key from the track ID
    fn get_blowfish_key(track_id: &str) -> Vec<u8> {
        let id_md5 = format!("{:x}", Md5::digest(track_id.as_bytes()));
        let id_md5_bytes = id_md5.as_bytes();
        let mut key = Vec::with_capacity(16);
        for i in 0..16 {
            key.push(id_md5_bytes[i] ^ id_md5_bytes[i + 16] ^ BLOWFISH_SECRET[i]);
        }
        key
    }

    /// Decrypt a Deezer audio chunk (2048 bytes, Blowfish CBC)
    fn decrypt_chunk(data: &mut [u8], bf_key: &[u8]) {
        let iv: [u8; 8] = [0, 1, 2, 3, 4, 5, 6, 7];
        if let Ok(cipher) = BlowfishCbcDec::new_from_slices(bf_key, &iv) {
            // Blowfish CBC works on 8-byte blocks
            let _ = cipher.decrypt_padded_mut::<blowfish::cipher::block_padding::NoPadding>(data);
        }
    }

    /// Download and decrypt a track from Deezer CDN
    async fn download_and_decrypt(
        &self,
        track_info: &DeezerTrackInfo,
        output_dir: &Path,
        file_stem: &str,
        format: &str,
        progress: &(dyn Fn(f64) + Send + Sync),
    ) -> Result<String, SourceError> {
        // Try FLAC first, then MP3 320
        let (quality, ext) = match format {
            "flac" => (9u8, "flac"),
            "mp3" => (3u8, "mp3"),
            _ => (9u8, "flac"), // Default to FLAC for best quality
        };

        let cdn_url = Self::build_cdn_url(track_info, quality);
        progress(20.0);

        let resp = self
            .client
            .get(&cdn_url)
            .header("Cookie", format!("arl={}", self.arl))
            .send()
            .await
            .map_err(|e| SourceError::NetworkError(format!("CDN download error: {}", e)))?;

        if resp.status().is_success() {
            return self
                .decrypt_response(resp, track_info, output_dir, file_stem, ext, progress)
                .await;
        }

        // FLAC failed — fall back to MP3 320
        if quality != 9 {
            return Err(SourceError::DrmBlocked(format!(
                "CDN returned {}",
                resp.status()
            )));
        }

        let cdn_url_mp3 = Self::build_cdn_url(track_info, 3);
        let resp = self
            .client
            .get(&cdn_url_mp3)
            .header("Cookie", format!("arl={}", self.arl))
            .send()
            .await
            .map_err(|e| SourceError::NetworkError(format!("CDN fallback error: {}", e)))?;

        if !resp.status().is_success() {
            return Err(SourceError::DrmBlocked(format!(
                "CDN returned {}",
                resp.status()
            )));
        }

        self.decrypt_response(resp, track_info, output_dir, file_stem, "mp3", progress)
            .await
    }

    async fn decrypt_response(
        &self,
        resp: reqwest::Response,
        track_info: &DeezerTrackInfo,
        output_dir: &Path,
        file_stem: &str,
        ext: &str,
        progress: &(dyn Fn(f64) + Send + Sync),
    ) -> Result<String, SourceError> {
        let encrypted_data = resp
            .bytes()
            .await
            .map_err(|e| SourceError::NetworkError(format!("Download error: {}", e)))?;

        progress(60.0);

        let bf_key = Self::get_blowfish_key(&track_info.id);
        let output_path = output_dir.join(format!("{}.{}", file_stem, ext));
        let output_str = output_path.to_string_lossy().to_string();

        // Decrypt in chunks: every 3rd 2048-byte chunk is encrypted
        let mut output_file = std::fs::File::create(&output_path)
            .map_err(|e| SourceError::Other(format!("Failed to create file: {}", e)))?;

        let chunk_size = 2048;
        let total_chunks = encrypted_data.len().div_ceil(chunk_size);

        for (i, chunk) in encrypted_data.chunks(chunk_size).enumerate() {
            let mut chunk_data = chunk.to_vec();

            // Every 3rd chunk (0, 3, 6, ...) is Blowfish encrypted, but only if full size
            if i % 3 == 0 && chunk_data.len() == chunk_size {
                Self::decrypt_chunk(&mut chunk_data, &bf_key);
            }

            output_file
                .write_all(&chunk_data)
                .map_err(|e| SourceError::Other(format!("Write error: {}", e)))?;

            if total_chunks > 0 {
                progress(60.0 + (i as f64 / total_chunks as f64) * 35.0);
            }
        }

        progress(95.0);
        log::info!("Deezer download complete: {}", output_str);
        Ok(output_str)
    }

    /// Extract track ID from a Deezer URL
    fn parse_track_id(url: &str) -> Result<String, SourceError> {
        // https://www.deezer.com/track/12345 or /en/track/12345
        if url.contains("/track/") {
            let after = url.split("/track/").nth(1).unwrap_or("");
            let id = after.split('?').next().unwrap_or("").split('/').next().unwrap_or("");
            if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
                return Ok(id.to_string());
            }
        }
        Err(SourceError::Other(format!(
            "Cannot parse Deezer track ID from: {}",
            url
        )))
    }
}

struct DeezerTrackInfo {
    id: String,
    md5_origin: String,
    media_version: String,
}

/// Fetch playlist/album entries from Deezer's public API (no auth needed).
pub async fn fetch_playlist_entries(url: &str) -> Result<super::ytdlp::PlaylistFetchResult, super::source::SourceError> {
    use super::source::SourceError;

    let url = url.trim();
    let client = reqwest::Client::new();

    // Extract type and ID from URL
    let (item_type, item_id) = if url.contains("/playlist/") {
        let id = url.split("/playlist/").nth(1).unwrap_or("")
            .split('?').next().unwrap_or("").split('/').next().unwrap_or("");
        ("playlist", id)
    } else if url.contains("/album/") {
        let id = url.split("/album/").nth(1).unwrap_or("")
            .split('?').next().unwrap_or("").split('/').next().unwrap_or("");
        ("album", id)
    } else {
        return Err(SourceError::Other("URL is not a Deezer playlist or album".into()));
    };

    if item_id.is_empty() {
        return Err(SourceError::Other("Could not extract ID from Deezer URL".into()));
    }

    // Fetch from Deezer's public API
    let api_url = format!("{}/{}/{}", DEEZER_API, item_type, item_id);
    let resp = client.get(&api_url)
        .send()
        .await
        .map_err(|e| SourceError::NetworkError(format!("Deezer API error: {}", e)))?;

    let data: serde_json::Value = resp.json().await
        .map_err(|e| SourceError::NetworkError(format!("Parse error: {}", e)))?;

    if data["error"].is_object() {
        let msg = data["error"]["message"].as_str().unwrap_or("Unknown error");
        return Err(SourceError::Other(format!("Deezer API: {}", msg)));
    }

    let playlist_title = data["title"].as_str().map(|s| s.to_string());

    // Get tracks — paginated, fetch all pages
    let mut entries = Vec::new();
    let mut tracks_url = format!("{}/{}/{}/tracks?limit=100", DEEZER_API, item_type, item_id);

    loop {
        let resp = client.get(&tracks_url)
            .send()
            .await
            .map_err(|e| SourceError::NetworkError(format!("Deezer tracks error: {}", e)))?;

        let page: serde_json::Value = resp.json().await
            .map_err(|e| SourceError::NetworkError(format!("Parse error: {}", e)))?;

        if let Some(tracks) = page["data"].as_array() {
            for track in tracks {
                let title = track["title"].as_str().unwrap_or("Unknown").to_string();
                let artist_name = track["artist"]["name"].as_str().map(|s| s.to_string());
                let duration = track["duration"].as_f64();
                let track_id = track["id"].as_i64().unwrap_or(0);
                let album_title = track["album"]["title"].as_str()
                    .map(|s| s.to_string())
                    .or_else(|| playlist_title.clone());

                // Build Deezer track URL for native downloading
                let track_url = if track_id > 0 {
                    format!("https://www.deezer.com/track/{}", track_id)
                } else {
                    String::new()
                };

                entries.push(super::ytdlp::VideoInfo {
                    title: title.clone(),
                    track: Some(title),
                    artist: artist_name.clone(),
                    uploader: artist_name,
                    duration,
                    thumbnail: track["album"]["cover_medium"].as_str().map(|s| s.to_string()),
                    webpage_url: if track_url.is_empty() { None } else { Some(track_url) },
                    album: album_title,
                    genre: None,
                    release_year: None,
                    description: None,
                    track_number: track["track_position"].as_i64(),
                    disc_number: track["disk_number"].as_i64(),
                    composer: None,
                    language: None,
                    tags: None,
                    channel_url: None,
                    isrc: track["isrc"].as_str().map(|s| s.to_string()),
                });
            }
        }

        // Check for next page
        if let Some(next) = page["next"].as_str() {
            tracks_url = next.to_string();
        } else {
            break;
        }
    }

    if entries.is_empty() {
        return Err(SourceError::Other("No tracks found in Deezer playlist".into()));
    }

    Ok(super::ytdlp::PlaylistFetchResult {
        playlist_title,
        entries,
    })
}

#[async_trait]
impl AudioSource for DeezerSource {
    fn is_available(&self) -> bool {
        !self.arl.is_empty()
    }

    fn platform(&self) -> &str {
        "deezer"
    }

    async fn download(
        &self,
        url: &str,
        output_dir: &Path,
        file_stem: &str,
        format: &str,
        progress: Box<dyn Fn(f64) + Send + Sync>,
    ) -> Result<String, SourceError> {
        let track_id = Self::parse_track_id(url)?;
        let (api_token, _) = self.get_api_token().await?;

        progress(10.0);
        let track_info = self.get_track_info(&api_token, &track_id).await?;

        self.download_and_decrypt(&track_info, output_dir, file_stem, format, &*progress)
            .await
    }

    async fn search_download(
        &self,
        query: &str,
        output_dir: &Path,
        file_stem: &str,
        format: &str,
        progress: Box<dyn Fn(f64) + Send + Sync>,
    ) -> Result<String, SourceError> {
        let track_id = self.search_track(query).await?;
        let (api_token, _) = self.get_api_token().await?;

        progress(10.0);
        let track_info = self.get_track_info(&api_token, &track_id).await?;

        self.download_and_decrypt(&track_info, output_dir, file_stem, format, &*progress)
            .await
    }

    async fn test_connection(&self) -> Result<(), SourceError> {
        let _ = self.get_api_token().await?;
        Ok(())
    }
}

/// Search Deezer's public API for a track and return metadata (ISRC, duration, title, artist).
/// This requires NO authentication — works for anyone.
pub async fn search_public_metadata(query: &str) -> Option<DeezerPublicResult> {
    let client = Client::new();
    let resp = client
        .get(format!("{}/search/track", DEEZER_API))
        .query(&[("q", query), ("limit", "1")])
        .send()
        .await
        .ok()?;

    let data: serde_json::Value = resp.json().await.ok()?;
    let track = data["data"].as_array()?.first()?;

    let track_id = track["id"].as_i64()?;
    let title = track["title"].as_str()?.to_string();
    let artist = track["artist"]["name"].as_str().map(|s| s.to_string());
    let duration = track["duration"].as_f64();
    let album = track["album"]["title"].as_str().map(|s| s.to_string());

    // The public search API doesn't return ISRC directly,
    // but the individual track endpoint does (no auth needed).
    let isrc = match client
        .get(format!("{}/track/{}", DEEZER_API, track_id))
        .send()
        .await
    {
        Ok(resp) => {
            if let Ok(track_data) = resp.json::<serde_json::Value>().await {
                track_data["isrc"].as_str().map(|s| s.to_string())
            } else {
                None
            }
        }
        Err(_) => None,
    };

    Some(DeezerPublicResult {
        title,
        artist,
        duration,
        album,
        isrc,
    })
}

/// Metadata from Deezer's public API (no authentication required)
pub struct DeezerPublicResult {
    pub title: String,
    pub artist: Option<String>,
    pub duration: Option<f64>,
    pub album: Option<String>,
    pub isrc: Option<String>,
}
