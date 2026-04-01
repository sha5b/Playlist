use serde::{Deserialize, Serialize};
use std::path::Path;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct VideoInfo {
    pub title: String,
    pub uploader: Option<String>,
    pub duration: Option<f64>,
    pub thumbnail: Option<String>,
    pub webpage_url: Option<String>,
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
        uploader: json["uploader"].as_str().map(|s| s.to_string()),
        duration: json["duration"].as_f64(),
        thumbnail: json["thumbnail"].as_str().map(|s| s.to_string()),
        webpage_url: json["webpage_url"].as_str().map(|s| s.to_string()),
    })
}

/// Fetch playlist entries
pub async fn get_playlist_entries(
    binary: &str,
    ffmpeg_dir: Option<&str>,
    url: &str,
) -> Result<Vec<VideoInfo>, String> {
    let mut cmd = Command::new(binary);
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

    for line in stdout.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(line) {
            entries.push(VideoInfo {
                title: json["title"].as_str().unwrap_or("Unknown").to_string(),
                uploader: json["uploader"].as_str().map(|s| s.to_string()),
                duration: json["duration"].as_f64(),
                thumbnail: json["thumbnail"].as_str().map(|s| s.to_string()),
                webpage_url: json["webpage_url"]
                    .as_str()
                    .or_else(|| json["url"].as_str())
                    .map(|s| s.to_string()),
            });
        }
    }

    Ok(entries)
}

/// Download audio from a URL. Calls progress_callback with percentage updates.
pub async fn download_audio<F>(
    binary: &str,
    ffmpeg_dir: Option<&str>,
    url: &str,
    output_dir: &Path,
    format: &str,
    _quality: &str,
    progress_callback: F,
) -> Result<String, String>
where
    F: Fn(DownloadProgress) + Send + 'static,
{
    let output_template = output_dir
        .join("%(title)s.%(ext)s")
        .to_string_lossy()
        .to_string();

    let mut cmd = Command::new(binary);
    cmd.args([
        "--extract-audio",
        "--audio-format",
        format,
        "--audio-quality",
        "0",
        "--output",
        &output_template,
        "--newline",
        "--no-warnings",
        "--no-playlist",
        "--print",
        "after_move:filepath",
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
    let stderr_reader = BufReader::new(stderr);
    let mut stderr_lines = stderr_reader.lines();

    // Read stderr for progress in background
    let progress_handle = tokio::spawn(async move {
        while let Ok(Some(line)) = stderr_lines.next_line().await {
            if let Some(progress) = parse_progress_line(&line) {
                progress_callback(progress);
            }
        }
    });

    let stdout = child.stdout.take().unwrap();
    let stdout_reader = BufReader::new(stdout);
    let mut stdout_lines = stdout_reader.lines();

    let mut final_path = String::new();
    while let Ok(Some(line)) = stdout_lines.next_line().await {
        let trimmed = line.trim().to_string();
        if !trimmed.is_empty() {
            final_path = trimmed;
        }
    }

    let status = child
        .wait()
        .await
        .map_err(|e| format!("Failed to wait for yt-dlp: {}", e))?;

    let _ = progress_handle.await;

    if !status.success() {
        return Err(format!("yt-dlp exited with status {}", status));
    }

    if final_path.is_empty() {
        return Err("Could not determine output file path".to_string());
    }

    Ok(final_path)
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
