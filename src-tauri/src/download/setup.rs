use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};
use tokio::io::AsyncWriteExt;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DepsStatus {
    pub ytdlp_available: bool,
    pub ffmpeg_available: bool,
    pub ytdlp_version: Option<String>,
    pub ytdlp_path: Option<String>,
    pub ffmpeg_path: Option<String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct SetupProgress {
    pub step: String,
    pub status: String,
    pub progress: f64,
    pub message: String,
}

/// Check if running inside a Flatpak sandbox
pub fn is_flatpak() -> bool {
    Path::new("/.flatpak-info").exists()
}

pub fn get_bin_dir(app_handle: &tauri::AppHandle) -> PathBuf {
    app_handle
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("bin")
}

pub fn get_ytdlp_path(bin_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        bin_dir.join("yt-dlp.exe")
    } else {
        bin_dir.join("yt-dlp")
    }
}

pub fn get_ffmpeg_path(bin_dir: &Path) -> PathBuf {
    if cfg!(windows) {
        bin_dir.join("ffmpeg.exe")
    } else {
        bin_dir.join("ffmpeg")
    }
}

/// Resolve the yt-dlp binary: local bin first, then PATH
pub fn resolve_ytdlp(bin_dir: &Path) -> Option<String> {
    let local = get_ytdlp_path(bin_dir);
    if local.exists() {
        return Some(local.to_string_lossy().to_string());
    }
    // Try PATH by checking if the command exists
    None
}

/// Resolve ffmpeg directory (for --ffmpeg-location): local bin first, then PATH
pub fn resolve_ffmpeg_dir(bin_dir: &Path) -> Option<String> {
    let local = get_ffmpeg_path(bin_dir);
    if local.exists() {
        log::info!("ffmpeg resolved from local bin dir: {:?}", bin_dir);
        return Some(bin_dir.to_string_lossy().to_string());
    }
    // Fall back to system ffmpeg on PATH
    #[cfg(unix)]
    let which_cmd = "which";
    #[cfg(windows)]
    let which_cmd = "where";
    if let Ok(output) = std::process::Command::new(which_cmd).arg("ffmpeg").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or("").trim().to_string();
            if let Some(parent) = Path::new(&path).parent() {
                log::info!("ffmpeg resolved from system PATH: {} (dir: {:?})", path, parent);
                return Some(parent.to_string_lossy().to_string());
            }
        }
    }
    log::warn!("ffmpeg not found in local bin dir or system PATH");
    None
}

/// Self-update yt-dlp in the background. An outdated yt-dlp silently breaks in
/// subtle ways when YouTube changes (e.g. playlists truncated to 100 entries),
/// so we keep it current instead of installing once and never touching it.
///
/// Standalone yt-dlp binaries support `-U` self-update. If `-U` fails (e.g. a
/// distro/pip install that can't self-update), we download a fresh app-managed
/// binary into `bin_dir`, which `resolve_ytdlp` prefers over PATH from then on.
pub async fn auto_update_ytdlp(bin_dir: &Path) {
    let local = get_ytdlp_path(bin_dir);
    let is_managed = local.exists();
    let binary = if is_managed {
        local.to_string_lossy().to_string()
    } else {
        "yt-dlp".to_string()
    };

    // Make sure the binary exists at all before trying to update it
    if !is_managed && super::ytdlp::check_available(&binary).await.is_err() {
        return;
    }

    log::info!("Checking for yt-dlp updates ({})", binary);
    let update = tokio::process::Command::new(&binary)
        .arg("-U")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .await;

    match update {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            if let Some(line) = stdout.lines().last() {
                log::info!("yt-dlp update: {}", line.trim());
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            log::warn!("yt-dlp self-update failed: {}", stderr.trim());
            // Can't self-update (pip/distro install, or a broken managed
            // binary) — install a fresh app-managed binary instead.
            if let Err(e) = redownload_ytdlp(bin_dir).await {
                log::warn!("yt-dlp re-download failed: {}", e);
            }
        }
        Err(e) => log::warn!("Failed to run yt-dlp -U: {}", e),
    }
}

/// Re-download the latest yt-dlp release over the app-managed binary.
async fn redownload_ytdlp(bin_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(bin_dir)
        .map_err(|e| format!("Failed to create bin directory: {}", e))?;
    let dest = get_ytdlp_path(bin_dir);
    let url = ytdlp_download_url();

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let bytes = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Download failed: {}", e))?
        .bytes()
        .await
        .map_err(|e| format!("Download stream error: {}", e))?;

    let tmp_path = dest.with_extension("tmp");
    tokio::fs::write(&tmp_path, &bytes)
        .await
        .map_err(|e| format!("Failed to write yt-dlp: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("Failed to set executable permission: {}", e))?;
    }

    // Verify the new binary runs before replacing the old one
    super::ytdlp::check_available(&tmp_path.to_string_lossy())
        .await
        .map_err(|e| {
            let _ = std::fs::remove_file(&tmp_path);
            format!("Downloaded yt-dlp failed to run: {}", e)
        })?;

    tokio::fs::rename(&tmp_path, &dest)
        .await
        .map_err(|e| format!("Failed to replace yt-dlp: {}", e))?;

    log::info!("yt-dlp re-downloaded to {:?}", dest);
    Ok(())
}

/// Check what dependencies are available
pub async fn check_deps(bin_dir: &Path) -> DepsStatus {
    // Check yt-dlp: local binary first, then PATH
    let ytdlp_local = get_ytdlp_path(bin_dir);
    let (ytdlp_available, ytdlp_version, ytdlp_path) = if ytdlp_local.exists() {
        match super::ytdlp::check_available(&ytdlp_local.to_string_lossy()).await {
            Ok(v) => (true, Some(v), Some(ytdlp_local.to_string_lossy().to_string())),
            Err(_) => (false, None, None),
        }
    } else {
        match super::ytdlp::check_available("yt-dlp").await {
            Ok(v) => (true, Some(v), Some("yt-dlp".to_string())),
            Err(_) => (false, None, None),
        }
    };

    // Check ffmpeg: local bundled binary first, then system PATH
    let (ffmpeg_available, ffmpeg_path) = if let Some(dir) = resolve_ffmpeg_dir(bin_dir) {
        (true, Some(dir))
    } else {
        (false, None)
    };

    DepsStatus {
        ytdlp_available,
        ffmpeg_available,
        ytdlp_version,
        ytdlp_path,
        ffmpeg_path,
    }
}

/// Download and install missing dependencies
pub async fn ensure_deps(bin_dir: &Path, app_handle: &tauri::AppHandle) -> Result<(), String> {
    std::fs::create_dir_all(bin_dir)
        .map_err(|e| format!("Failed to create bin directory: {}", e))?;

    let status = check_deps(bin_dir).await;

    // Download yt-dlp if needed
    if !status.ytdlp_available {
        emit_progress(app_handle, "yt-dlp", "downloading", 0.0, "Downloading yt-dlp...");

        let url = ytdlp_download_url();
        let dest = get_ytdlp_path(bin_dir);
        download_file(&url, &dest, app_handle, "yt-dlp").await?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                .map_err(|e| format!("Failed to set executable permission: {}", e))?;
        }

        // Verify it works
        match super::ytdlp::check_available(&dest.to_string_lossy()).await {
            Ok(v) => {
                emit_progress(
                    app_handle,
                    "yt-dlp",
                    "ready",
                    100.0,
                    &format!("yt-dlp {} ready", v),
                );
            }
            Err(e) => {
                let _ = std::fs::remove_file(&dest);
                return Err(format!("Downloaded yt-dlp but it failed to run: {}", e));
            }
        }
    }

    // Download ffmpeg if needed
    if !status.ffmpeg_available {
        #[cfg(windows)]
        {
            emit_progress(
                app_handle,
                "ffmpeg",
                "downloading",
                0.0,
                "Downloading ffmpeg...",
            );

            let url = ffmpeg_download_url();
            let zip_path = bin_dir.join("ffmpeg.zip");
            download_file(&url, &zip_path, app_handle, "ffmpeg").await?;

            emit_progress(
                app_handle,
                "ffmpeg",
                "extracting",
                0.0,
                "Extracting ffmpeg...",
            );
            extract_ffmpeg_zip(&zip_path, bin_dir).await?;

            // Verify ffmpeg works
            let ffmpeg_path = get_ffmpeg_path(bin_dir);
            if !ffmpeg_path.exists() {
                return Err("Failed to extract ffmpeg from archive".to_string());
            }

            emit_progress(app_handle, "ffmpeg", "ready", 100.0, "ffmpeg ready");
        }

        #[cfg(target_os = "macos")]
        {
            emit_progress(
                app_handle,
                "ffmpeg",
                "installing",
                0.0,
                "Installing ffmpeg via Homebrew...",
            );

            // Check if brew is installed, if not install it
            let brew_exists = std::process::Command::new("which")
                .arg("brew")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);

            if !brew_exists {
                emit_progress(
                    app_handle,
                    "ffmpeg",
                    "installing",
                    0.0,
                    "Installing Homebrew...",
                );

                let brew_install = tokio::process::Command::new("bash")
                    .arg("-c")
                    .arg("NONINTERACTIVE=1 /bin/bash -c \"$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"")
                    .output()
                    .await
                    .map_err(|e| format!("Failed to install Homebrew: {}", e))?;

                if !brew_install.status.success() {
                    let stderr = String::from_utf8_lossy(&brew_install.stderr);
                    return Err(format!("Homebrew installation failed: {}", stderr));
                }
            }

            emit_progress(
                app_handle,
                "ffmpeg",
                "installing",
                30.0,
                "Installing ffmpeg via Homebrew (this may take a few minutes)...",
            );

            let brew_path = if Path::new("/opt/homebrew/bin/brew").exists() {
                "/opt/homebrew/bin/brew"
            } else if Path::new("/usr/local/bin/brew").exists() {
                "/usr/local/bin/brew"
            } else {
                "brew"
            };

            let install = tokio::process::Command::new(brew_path)
                .args(["install", "ffmpeg"])
                .output()
                .await
                .map_err(|e| format!("Failed to run brew install ffmpeg: {}", e))?;

            if !install.status.success() {
                let stderr = String::from_utf8_lossy(&install.stderr);
                return Err(format!("brew install ffmpeg failed: {}", stderr));
            }

            // Verify it's now on PATH
            if resolve_ffmpeg_dir(bin_dir).is_none() {
                return Err("ffmpeg was installed but could not be found on PATH".to_string());
            }

            emit_progress(app_handle, "ffmpeg", "ready", 100.0, "ffmpeg ready");
        }

        #[cfg(all(target_os = "linux", not(target_os = "macos")))]
        {
            if is_flatpak() {
                // In Flatpak, ffmpeg should be provided by the runtime or bundled in /app/bin
                return Err("ffmpeg not found. This is unexpected inside the Flatpak sandbox — \
                    please report this as a bug at https://github.com/sha5b/Playlist/issues".to_string());
            }

            emit_progress(
                app_handle,
                "ffmpeg",
                "installing",
                0.0,
                "Installing ffmpeg...",
            );

            // Check if pkexec is available for privilege escalation
            if !Path::new("/usr/bin/pkexec").exists() {
                return Err("ffmpeg not found. Please install it manually with your package manager:\n\
                    - Fedora/RHEL: sudo dnf install ffmpeg-free\n\
                    - Debian/Ubuntu: sudo apt install ffmpeg\n\
                    - Arch: sudo pacman -S ffmpeg".to_string());
            }

            // Try common Linux package managers
            let result = if Path::new("/usr/bin/dnf").exists() {
                tokio::process::Command::new("pkexec")
                    .args(["dnf", "install", "-y", "ffmpeg-free"])
                    .output()
                    .await
            } else if Path::new("/usr/bin/apt-get").exists() {
                tokio::process::Command::new("pkexec")
                    .args(["apt-get", "install", "-y", "ffmpeg"])
                    .output()
                    .await
            } else if Path::new("/usr/bin/pacman").exists() {
                tokio::process::Command::new("pkexec")
                    .args(["pacman", "-S", "--noconfirm", "ffmpeg"])
                    .output()
                    .await
            } else {
                return Err("ffmpeg not found. Please install it with your package manager (e.g. dnf install ffmpeg-free, apt install ffmpeg, pacman -S ffmpeg)".to_string());
            };

            match result {
                Ok(output) if output.status.success() => {
                    if resolve_ffmpeg_dir(bin_dir).is_none() {
                        return Err("ffmpeg was installed but could not be found on PATH".to_string());
                    }
                    emit_progress(app_handle, "ffmpeg", "ready", 100.0, "ffmpeg ready");
                }
                Ok(output) => {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    return Err(format!("ffmpeg installation failed: {}", stderr));
                }
                Err(e) => {
                    return Err(format!("Failed to run package manager: {}", e));
                }
            }
        }
    }

    emit_progress(app_handle, "complete", "ready", 100.0, "Setup complete");
    Ok(())
}

fn ytdlp_download_url() -> String {
    if cfg!(windows) {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp.exe".to_string()
    } else if cfg!(target_os = "macos") {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_macos".to_string()
    } else {
        "https://github.com/yt-dlp/yt-dlp/releases/latest/download/yt-dlp_linux".to_string()
    }
}

#[cfg(windows)]
fn ffmpeg_download_url() -> String {
    "https://github.com/yt-dlp/FFmpeg-Builds/releases/download/latest/ffmpeg-master-latest-win64-gpl.zip".to_string()
}

async fn download_file(
    url: &str,
    dest: &Path,
    app_handle: &tauri::AppHandle,
    step: &str,
) -> Result<(), String> {
    use futures_util::StreamExt;

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(10))
        .connect_timeout(std::time::Duration::from_secs(30))
        .timeout(std::time::Duration::from_secs(600))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("Download request failed: {}", e))?;

    if !response.status().is_success() {
        return Err(format!("Download failed with status: {}", response.status()));
    }

    let total_size = response.content_length().unwrap_or(0);
    let mut stream = response.bytes_stream();

    let tmp_path = dest.with_extension("tmp");
    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .map_err(|e| format!("Failed to create file: {}", e))?;

    let mut downloaded: u64 = 0;
    let mut last_emitted: f64 = -5.0;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("Download stream error: {}", e))?;
        file.write_all(&chunk)
            .await
            .map_err(|e| format!("Failed to write chunk: {}", e))?;

        downloaded += chunk.len() as u64;

        if total_size > 0 {
            let progress = (downloaded as f64 / total_size as f64) * 100.0;
            if progress - last_emitted >= 2.0 {
                last_emitted = progress;
                let size_mb = downloaded as f64 / (1024.0 * 1024.0);
                let total_mb = total_size as f64 / (1024.0 * 1024.0);
                emit_progress(
                    app_handle,
                    step,
                    "downloading",
                    progress,
                    &format!(
                        "Downloading {}... {:.1} / {:.1} MB",
                        step, size_mb, total_mb
                    ),
                );
            }
        }
    }

    file.flush().await.map_err(|e| format!("Flush failed: {}", e))?;
    drop(file);

    // Rename temp file to final destination
    tokio::fs::rename(&tmp_path, dest)
        .await
        .map_err(|e| format!("Failed to finalize download: {}", e))?;

    Ok(())
}

#[cfg(windows)]
async fn extract_ffmpeg_zip(zip_path: &Path, bin_dir: &Path) -> Result<(), String> {
    let zip_path = zip_path.to_path_buf();
    let bin_dir = bin_dir.to_path_buf();

    tokio::task::spawn_blocking(move || {
        let file =
            std::fs::File::open(&zip_path).map_err(|e| format!("Failed to open zip: {}", e))?;
        let mut archive =
            zip::ZipArchive::new(file).map_err(|e| format!("Invalid zip archive: {}", e))?;

        let targets = ["ffmpeg.exe", "ffprobe.exe"];

        for i in 0..archive.len() {
            let mut entry = archive
                .by_index(i)
                .map_err(|e| format!("Zip entry error: {}", e))?;
            let name = entry.name().to_string();

            // Look for ffmpeg.exe and ffprobe.exe inside any bin/ directory
            for target in &targets {
                if name.ends_with(&format!("bin/{}", target)) {
                    let out_path = bin_dir.join(target);
                    // Validate against zip-slip: output must stay inside bin_dir
                    if !out_path.starts_with(&bin_dir) {
                        return Err(format!("Unsafe zip entry path: {}", name));
                    }
                    let mut out_file = std::fs::File::create(&out_path)
                        .map_err(|e| format!("Failed to create {}: {}", target, e))?;
                    std::io::copy(&mut entry, &mut out_file)
                        .map_err(|e| format!("Failed to extract {}: {}", target, e))?;
                    log::info!("Extracted {} to {:?}", target, out_path);
                }
            }
        }

        // Clean up the zip file
        let _ = std::fs::remove_file(&zip_path);

        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("Extract task failed: {}", e))?
}

fn emit_progress(
    app_handle: &tauri::AppHandle,
    step: &str,
    status: &str,
    progress: f64,
    message: &str,
) {
    let _ = app_handle.emit(
        "setup-progress",
        SetupProgress {
            step: step.to_string(),
            status: status.to_string(),
            progress,
            message: message.to_string(),
        },
    );
}
