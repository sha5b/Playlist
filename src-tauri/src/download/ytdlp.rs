use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Apply platform-specific flags to run subprocesses at low priority
/// so they don't starve the UI or audio playback.
#[cfg(windows)]
fn low_priority(cmd: &mut Command) {
    #[allow(unused_imports)]
    use std::os::windows::process::CommandExt;
    // IDLE_PRIORITY_CLASS (0x00000040) + CREATE_NO_WINDOW (0x08000000)
    // Lowest possible priority so downloads + ffmpeg never starve the audio callback.
    cmd.creation_flags(0x00000040 | 0x08000000);
}

#[cfg(not(windows))]
fn low_priority(_cmd: &mut Command) {
    // On non-Windows, no special flags needed for now.
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoInfo {
    pub title: String,
    /// Music track name (from yt-dlp "track" field), preferred over title for music
    pub track: Option<String>,
    /// Artist name (from yt-dlp "artist" field), preferred over uploader for music
    pub artist: Option<String>,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    pub webpage_url: Option<String>,
    pub album: Option<String>,
    pub genre: Option<String>,
    pub release_year: Option<String>,
    pub description: Option<String>,
    pub track_number: Option<i64>,
    pub disc_number: Option<i64>,
    pub composer: Option<String>,
    pub language: Option<String>,
    pub tags: Option<Vec<String>>,
    pub channel_url: Option<String>,
    /// ISRC (International Standard Recording Code) for precise track matching
    pub isrc: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DownloadProgress {
    pub percent: f64,
    pub speed: Option<String>,
    pub eta: Option<String>,
}

/// Check if a yt-dlp binary is working
pub async fn check_available(binary: &str) -> Result<String, String> {
    let output = Command::new(binary)
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .map_err(|_| format!("{} not found or failed to run", binary))?;

    if output.status.success() {
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(version)
    } else {
        Err("yt-dlp returned an error".to_string())
    }
}

/// Fetch metadata for a URL without downloading
pub async fn get_info(
    binary: &str,
    ffmpeg_dir: Option<&str>,
    url: &str,
    cookies_from_browser: Option<&str>,
) -> Result<VideoInfo, String> {
    let mut cmd = Command::new(binary);
    low_priority(&mut cmd);
    cmd.args([
        "--dump-json", 
        "--no-download", 
        "--no-warnings", 
        "--flat-playlist",
        // Anti-bot detection flags
        "--user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36",
        "--extractor-retries", "3",
    ]);
    if let Some(dir) = ffmpeg_dir {
        cmd.args(["--ffmpeg-location", dir]);
    }
    if let Some(browser) = cookies_from_browser {
        if !browser.is_empty() {
            cmd.args(["--cookies-from-browser", browser]);
        }
    }
    cmd.arg(url);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to run yt-dlp: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp error: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let first_line = stdout.lines().next().unwrap_or("");

    let json: serde_json::Value = serde_json::from_str(first_line)
        .map_err(|e| format!("Failed to parse yt-dlp output: {}", e))?;

    Ok(video_info_from_json(&json))
}

fn video_info_from_json(json: &serde_json::Value) -> VideoInfo {
    VideoInfo {
        title: json["title"].as_str().unwrap_or("Unknown").to_string(),
        track: json["track"].as_str().map(|s| s.to_string()),
        artist: json["artist"].as_str()
            .or_else(|| json["creator"].as_str())
            .map(|s| s.to_string()),
        uploader: json["uploader"].as_str().map(|s| s.to_string()),
        duration: json["duration"].as_f64(),
        thumbnail: json["thumbnail"].as_str().map(|s| s.to_string()),
        webpage_url: json["webpage_url"].as_str().map(|s| s.to_string()),
        album: json["album"].as_str().map(|s| s.to_string()),
        genre: json["genre"].as_str().map(|s| s.to_string()),
        release_year: json["release_year"].as_str()
            .or_else(|| json["release_date"].as_str().and_then(|d| if d.len() >= 4 { Some(d) } else { None }))
            .map(|s| s.get(..4).unwrap_or(s).to_string()),
        description: json["description"].as_str()
            .filter(|d| !d.is_empty())
            .map(|d| {
                if d.len() > 500 {
                    // Find a valid char boundary at or before byte 497
                    let end = d.floor_char_boundary(497);
                    format!("{}...", &d[..end])
                } else {
                    d.to_string()
                }
            }),
        track_number: json["track_number"].as_i64(),
        disc_number: json["disc_number"].as_i64(),
        composer: json["composer"].as_str().map(|s| s.to_string()),
        language: json["language"].as_str().map(|s| s.to_string()),
        tags: json["tags"].as_array().map(|arr| {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        }),
        channel_url: json["channel_url"].as_str().map(|s| s.to_string()),
        isrc: json["isrc"].as_str()
            .or_else(|| json["external_ids"]["isrc"].as_str())
            .map(|s| s.to_string()),
    }
}

/// Search YouTube (or other platform) and return multiple result infos for duration matching.
/// Uses `ytsearch5:` to get up to 5 results so we can pick the best duration match.
pub async fn search_info(
    binary: &str,
    ffmpeg_dir: Option<&str>,
    query: &str,
    cookies_from_browser: Option<&str>,
) -> Result<Vec<VideoInfo>, String> {
    let mut cmd = Command::new(binary);
    low_priority(&mut cmd);
    cmd.args([
        "--dump-json",
        "--no-download",
        "--no-warnings",
        // Anti-bot detection flags
        "--user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36",
        "--extractor-retries", "3",
    ]);
    if let Some(dir) = ffmpeg_dir {
        cmd.args(["--ffmpeg-location", dir]);
    }
    if let Some(browser) = cookies_from_browser {
        if !browser.is_empty() {
            cmd.args(["--cookies-from-browser", browser]);
        }
    }
    cmd.arg(query);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to run yt-dlp: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp error: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let results: Vec<VideoInfo> = stdout
        .lines()
        .filter_map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .ok()
                .map(|json| video_info_from_json(&json))
        })
        .collect();

    Ok(results)
}

/// Search YouTube for the official music video of a track.
/// Returns the webpage URL of the best match, or None.
/// Check if a YouTube result is likely a real music video (not a static-image auto-upload).
/// Filters out: Topic channel auto-uploads, "Provided to YouTube by" distributor uploads,
/// lyric videos, and audio-only uploads.
fn is_real_music_video(v: &VideoInfo) -> bool {
    let title_lower = v.title.to_lowercase();
    let uploader_lower = v.uploader.as_deref().unwrap_or("").to_lowercase();
    let desc_lower = v.description.as_deref().unwrap_or("").to_lowercase();

    // Reject auto-generated Topic channel uploads (always static album art)
    if uploader_lower.ends_with("- topic") {
        return false;
    }

    // Reject distributor auto-uploads ("Provided to YouTube by ...")
    if desc_lower.starts_with("provided to youtube by") {
        return false;
    }

    // Reject YouTube auto-generated uploads (Topic channels with static album art)
    if desc_lower.contains("auto-generated by youtube") {
        return false;
    }

    // Reject lyric videos and audio-only uploads
    if title_lower.contains("lyric") || title_lower.contains("audio only") || title_lower.contains("audio)") {
        return false;
    }

    // Accept: title says official video/music video, or uploader is VEVO
    let has_official = title_lower.contains("official video")
        || title_lower.contains("official music video")
        || title_lower.contains("music video")
        || title_lower.contains("official mv")
        || title_lower.contains("m/v");
    let is_vevo = uploader_lower.contains("vevo");

    has_official || is_vevo
}

pub async fn search_music_video(
    binary: &str,
    ffmpeg_dir: Option<&str>,
    artist: &str,
    title: &str,
    cookies_from_browser: Option<&str>,
) -> Option<String> {
    // Clean the title: strip parenthetical suffixes like "(Official Music Video)" that are
    // already part of the downloaded filename but would confuse the search
    let clean_title = title
        .split(&['(', '['][..])
        .next()
        .unwrap_or(title)
        .trim();

    let query = format!("ytsearch8:{} {} official music video", artist, clean_title);
    let results = search_info(binary, ffmpeg_dir, &query, cookies_from_browser)
        .await
        .ok()?;

    let lower_artist = artist.to_lowercase();
    let lower_title = clean_title.to_lowercase();

    // Extract key words from the title (3+ chars) for fuzzy matching
    let title_words: Vec<&str> = lower_title
        .split_whitespace()
        .filter(|w| w.len() >= 3)
        .collect();

    // Check if a result's title contains the song title (or enough key words)
    let title_matches = |video_title: &str| -> bool {
        let vt = video_title.to_lowercase();
        // Exact substring match
        if vt.contains(&lower_title) {
            return true;
        }
        // Fuzzy: at least 60% of title words present
        if !title_words.is_empty() {
            let matched = title_words.iter().filter(|w| vt.contains(**w)).count();
            return matched * 100 / title_words.len() >= 60;
        }
        false
    };

    let artist_matches = |v: &VideoInfo| -> bool {
        let t = v.title.to_lowercase();
        t.contains(&lower_artist)
            || v.artist.as_ref().map_or(false, |a| a.to_lowercase().contains(&lower_artist))
            || v.uploader.as_ref().map_or(false, |u| u.to_lowercase().contains(&lower_artist))
    };

    // Best: real music video + artist match + title match
    let best = results
        .iter()
        .filter(|v| is_real_music_video(v))
        .find(|v| artist_matches(v) && title_matches(&v.title));

    // Good: any video + artist match + title match (might not say "official" in title)
    let good = || results
        .iter()
        .find(|v| artist_matches(v) && title_matches(&v.title));

    // Acceptable: real music video + title match (artist name might differ e.g. feat.)
    let acceptable = || results
        .iter()
        .filter(|v| is_real_music_video(v))
        .find(|v| title_matches(&v.title));

    best.or_else(good)
        .or_else(acceptable)
        .and_then(|v| v.webpage_url.clone())
}

/// Result of fetching playlist entries, including the playlist title
pub struct PlaylistFetchResult {
    pub playlist_title: Option<String>,
    pub entries: Vec<VideoInfo>,
}

/// Fetch playlist entries
pub async fn get_playlist_entries(
    binary: &str,
    ffmpeg_dir: Option<&str>,
    url: &str,
    cookies_from_browser: Option<&str>,
) -> Result<PlaylistFetchResult, String> {
    let mut cmd = Command::new(binary);
    low_priority(&mut cmd);
    cmd.args([
        "--dump-json", 
        "--no-download", 
        "--no-warnings", 
        "--flat-playlist",
        // Anti-bot detection flags
        "--user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36",
        "--extractor-retries", "3",
    ]);
    if let Some(dir) = ffmpeg_dir {
        cmd.args(["--ffmpeg-location", dir]);
    }
    if let Some(browser) = cookies_from_browser {
        if !browser.is_empty() {
            cmd.args(["--cookies-from-browser", browser]);
        }
    }
    cmd.arg(url);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .await
        .map_err(|e| format!("Failed to run yt-dlp: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("yt-dlp error: {}", stderr.trim()));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    let mut playlist_title: Option<String> = None;

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            // Extract playlist_title from the first entry (yt-dlp includes it in each entry)
            if playlist_title.is_none() {
                playlist_title = json["playlist_title"].as_str().map(|s| s.to_string());
            }
            let mut info = video_info_from_json(&json);

            // Debug: log raw yt-dlp fields for URL resolution
            let raw_url = json["url"].as_str().unwrap_or("<missing>");
            let raw_id = json["id"].as_str().unwrap_or("<missing>");
            let raw_webpage = json["webpage_url"].as_str().unwrap_or("<missing>");
            log::info!(
                "[playlist-entry] title='{}' webpage_url='{}' url='{}' id='{}'",
                info.title, raw_webpage, raw_url, raw_id
            );

            // For playlist entries, resolve the video URL from multiple fields.
            // --flat-playlist often returns just a video ID in "url"/"id", not a full URL.
            if info.webpage_url.is_none() || info.webpage_url.as_deref() == Some("") {
                let url_field = json["url"].as_str().unwrap_or("");
                if url_field.starts_with("http") {
                    info.webpage_url = Some(url_field.to_string());
                } else if url_field.starts_with("spotify:track:") {
                    // Spotify URI — convert to a proper Spotify URL
                    let track_id = url_field.strip_prefix("spotify:track:").unwrap_or("");
                    info.webpage_url = Some(format!("https://open.spotify.com/track/{}", track_id));
                } else if !url_field.is_empty() {
                    info.webpage_url = Some(format!("https://www.youtube.com/watch?v={}", url_field));
                } else if let Some(id) = json["id"].as_str().filter(|s| !s.is_empty()) {
                    info.webpage_url = Some(format!("https://www.youtube.com/watch?v={}", id));
                }
                log::info!("[playlist-entry] resolved URL -> {:?}", info.webpage_url);
            }
            entries.push(info);
        }
    }

    Ok(PlaylistFetchResult {
        playlist_title,
        entries,
    })
}

/// Download audio from a URL. Calls progress_callback with percentage updates.
/// `file_stem` is used as the output filename (without extension) to avoid encoding issues.
pub async fn download_audio<F>(
    binary: &str,
    ffmpeg_dir: Option<&str>,
    url: &str,
    output_dir: &Path,
    format: &str,
    quality: &str,
    file_stem: &str,
    cookies_from_browser: Option<&str>,
    progress_callback: F,
) -> Result<String, String>
where
    F: Fn(DownloadProgress) + Send + 'static,
{
    // Use a safe, deterministic filename based on the download ID.
    // This avoids all encoding issues with non-ASCII titles, special characters
    // like '/' in titles, and Windows code page mismatches with yt-dlp stdout.
    let output_template = output_dir
        .join(format!("{}.%(ext)s", file_stem))
        .to_string_lossy()
        .to_string();

    // Use the quality setting from user preferences, default to "0" (best)
    let audio_quality = if quality.is_empty() { "0" } else { quality };

    let mut cmd = Command::new(binary);
    low_priority(&mut cmd);
    cmd.args([
        "--extract-audio",
        "--audio-format",
        format,
        "--audio-quality",
        audio_quality,
        // No -f filter: let yt-dlp pick the best available format.
        // --extract-audio + --audio-format + ffmpeg handle conversion.
        // This avoids "Requested format is not available" on Topic channels etc.
        "--embed-metadata",
        "--embed-thumbnail",
        "--output",
        &output_template,
        "--newline",
        "--no-playlist",
        // Anti-bot detection flags
        "--user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36",
        "--extractor-retries", "3",
    ]);
    if let Some(dir) = ffmpeg_dir {
        cmd.args(["--ffmpeg-location", dir]);
    }
    if let Some(browser) = cookies_from_browser {
        if !browser.is_empty() {
            cmd.args(["--cookies-from-browser", browser]);
        }
    }
    cmd.arg(url);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn yt-dlp: {}", e))?;

    let stderr = child.stderr.take().unwrap();
    let stdout = child.stdout.take().unwrap();

    // Read both streams concurrently to avoid pipe deadlocks.
    // Throttle progress callbacks to at most once per second to avoid flooding
    // the frontend with IPC events (yt-dlp emits many lines per second).
    let stderr_reader = BufReader::new(stderr);
    let mut stderr_lines = stderr_reader.lines();
    let progress_handle = tokio::spawn(async move {
        let mut last_emit = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(2))
            .unwrap_or_else(std::time::Instant::now);
        let mut latest: Option<DownloadProgress> = None;
        let mut error_lines: Vec<String> = Vec::new();

        while let Ok(Some(line)) = stderr_lines.next_line().await {
            if let Some(progress) = parse_progress_line(&line) {
                latest = Some(progress);
                if last_emit.elapsed() >= std::time::Duration::from_millis(800) {
                    if let Some(p) = latest.take() {
                        progress_callback(p);
                        last_emit = std::time::Instant::now();
                    }
                }
            } else if line.starts_with("ERROR:") || line.contains("ERROR:") {
                error_lines.push(line);
            }
        }
        // Always emit the final progress
        if let Some(p) = latest {
            progress_callback(p);
        }
        error_lines
    });

    // Drain stdout to prevent pipe deadlock
    let stdout_reader = BufReader::new(stdout);
    let mut stdout_lines = stdout_reader.lines();
    let stdout_handle = tokio::spawn(async move {
        while let Ok(Some(_)) = stdout_lines.next_line().await {}
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to wait for yt-dlp: {}", e))?;

    let error_lines = progress_handle.await.unwrap_or_default();
    let _ = stdout_handle.await;

    if !status.success() {
        let detail = if error_lines.is_empty() {
            format!("yt-dlp exited with status {}", status)
        } else {
            format!("yt-dlp failed: {}", error_lines.join(" | "))
        };
        return Err(detail);
    }

    // Construct the expected output path deterministically
    let expected_path = output_dir.join(format!("{}.{}", file_stem, format));
    if expected_path.exists() {
        return Ok(expected_path.to_string_lossy().to_string());
    }

    // Fallback: scan the output directory for files matching this stem
    // (yt-dlp may choose a different extension than requested)
    if let Ok(entries) = std::fs::read_dir(output_dir) {
        let prefix = format!("{}.", file_stem);
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(&prefix) {
                    return Ok(entry.path().to_string_lossy().to_string());
                }
            }
        }
    }

    Err(format!("Download completed but output file not found for {}", file_stem))
}

/// Download a video (with audio) from a URL, returning the path to the .mp4 file.
pub async fn download_video<F>(
    binary: &str,
    ffmpeg_dir: Option<&str>,
    url: &str,
    output_dir: &Path,
    file_stem: &str,
    cookies_from_browser: Option<&str>,
    progress_callback: F,
) -> Result<String, String>
where
    F: Fn(DownloadProgress) + Send + 'static,
{
    let output_template = output_dir
        .join(format!("{}.%(ext)s", file_stem))
        .to_string_lossy()
        .to_string();

    let mut cmd = Command::new(binary);
    low_priority(&mut cmd);
    cmd.args([
        // Prefer h264+aac (widely compatible), fall back to best available
        "-f", "bv*[vcodec~='^(h264|avc)'][height<=1080]+ba[acodec~='^(mp4a|aac)'] / bv*[height<=1080]+ba / b[height<=1080] / b",
        "--merge-output-format", "mp4",
        "--embed-metadata",
        "--output", &output_template,
        "--newline",
        "--no-playlist",
        "--user-agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/136.0.0.0 Safari/537.36",
        "--extractor-retries", "3",
    ]);
    if let Some(dir) = ffmpeg_dir {
        cmd.args(["--ffmpeg-location", dir]);
    }
    if let Some(browser) = cookies_from_browser {
        if !browser.is_empty() {
            cmd.args(["--cookies-from-browser", browser]);
        }
    }
    cmd.arg(url);
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn yt-dlp: {}", e))?;

    // Drain both pipes to avoid deadlock, parse progress from stderr
    let stderr = child.stderr.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let stderr_handle = tokio::spawn(async move {
        let mut last_emit = std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(2))
            .unwrap_or_else(std::time::Instant::now);
        let mut latest: Option<DownloadProgress> = None;
        let mut error_lines: Vec<String> = Vec::new();
        let mut lines = BufReader::new(stderr).lines();
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(progress) = parse_progress_line(&line) {
                latest = Some(progress);
                if last_emit.elapsed() >= std::time::Duration::from_millis(800) {
                    if let Some(p) = latest.take() {
                        progress_callback(p);
                        last_emit = std::time::Instant::now();
                    }
                }
            } else if line.starts_with("ERROR:") || line.contains("ERROR:") {
                error_lines.push(line);
            }
        }
        // Always emit the final progress
        if let Some(p) = latest {
            progress_callback(p);
        }
        error_lines
    });
    let stdout_handle = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        while let Ok(Some(_)) = lines.next_line().await {}
    });

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to wait for yt-dlp: {}", e))?;

    let error_lines = stderr_handle.await.unwrap_or_default();
    let _ = stdout_handle.await;

    if !status.success() {
        let detail = if error_lines.is_empty() {
            format!("yt-dlp video download exited with status {}", status)
        } else {
            format!("yt-dlp video download failed: {}", error_lines.join(" | "))
        };
        return Err(detail);
    }

    let expected_path = output_dir.join(format!("{}.mp4", file_stem));
    if expected_path.exists() {
        return Ok(expected_path.to_string_lossy().to_string());
    }

    // Fallback: scan for any file with this stem
    if let Ok(entries) = std::fs::read_dir(output_dir) {
        let prefix = format!("{}.", file_stem);
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with(&prefix) {
                    return Ok(entry.path().to_string_lossy().to_string());
                }
            }
        }
    }

    Err(format!("Video download completed but output file not found for {}", file_stem))
}

fn parse_progress_line(line: &str) -> Option<DownloadProgress> {
    // yt-dlp progress lines look like:
    // [download]  45.2% of  3.45MiB at  1.23MiB/s ETA 00:02
    if !line.contains("[download]") || !line.contains('%') {
        return None;
    }

    let percent = line
        .split('%')
        .next()?
        .split_whitespace()
        .last()?
        .parse::<f64>()
        .ok()?;

    let speed = line
        .split("at")
        .nth(1)
        .and_then(|s| s.split("ETA").next())
        .map(|s| s.trim().to_string());

    let eta = line.split("ETA").nth(1).map(|s| s.trim().to_string());

    Some(DownloadProgress {
        percent,
        speed,
        eta,
    })
}
