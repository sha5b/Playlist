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
) -> Result<VideoInfo, String> {
    let mut cmd = Command::new(binary);
    low_priority(&mut cmd);
    cmd.args(["--dump-json", "--no-download", "--no-warnings", "--flat-playlist"]);
    if let Some(dir) = ffmpeg_dir {
        cmd.args(["--ffmpeg-location", dir]);
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

    Ok(VideoInfo {
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
    })
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
) -> Result<PlaylistFetchResult, String> {
    let mut cmd = Command::new(binary);
    low_priority(&mut cmd);
    cmd.args(["--dump-json", "--no-download", "--no-warnings", "--flat-playlist"]);
    if let Some(dir) = ffmpeg_dir {
        cmd.args(["--ffmpeg-location", dir]);
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
            entries.push(VideoInfo {
                title: json["title"].as_str().unwrap_or("Unknown").to_string(),
                track: json["track"].as_str().map(|s| s.to_string()),
                artist: json["artist"].as_str()
                    .or_else(|| json["creator"].as_str())
                    .map(|s| s.to_string()),
                uploader: json["uploader"].as_str().map(|s| s.to_string()),
                duration: json["duration"].as_f64(),
                thumbnail: json["thumbnail"].as_str().map(|s| s.to_string()),
                webpage_url: json["webpage_url"]
                    .as_str()
                    .or_else(|| json["url"].as_str())
                    .map(|s| s.to_string()),
                album: json["album"].as_str().map(|s| s.to_string()),
            });
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
    _quality: &str,
    file_stem: &str,
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

    let mut cmd = Command::new(binary);
    low_priority(&mut cmd);
    cmd.args([
        "--extract-audio",
        "--audio-format",
        format,
        "--audio-quality",
        "0",
        "--embed-metadata",
        "--embed-thumbnail",
        "--output",
        &output_template,
        "--newline",
        "--no-warnings",
        "--no-playlist",
    ]);
    if let Some(dir) = ffmpeg_dir {
        cmd.args(["--ffmpeg-location", dir]);
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

        while let Ok(Some(line)) = stderr_lines.next_line().await {
            if let Some(progress) = parse_progress_line(&line) {
                latest = Some(progress);
                if last_emit.elapsed() >= std::time::Duration::from_millis(800) {
                    if let Some(p) = latest.take() {
                        progress_callback(p);
                        last_emit = std::time::Instant::now();
                    }
                }
            }
        }
        // Always emit the final progress
        if let Some(p) = latest {
            progress_callback(p);
        }
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

    let _ = progress_handle.await;
    let _ = stdout_handle.await;

    if !status.success() {
        return Err(format!("yt-dlp exited with status {}", status));
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
