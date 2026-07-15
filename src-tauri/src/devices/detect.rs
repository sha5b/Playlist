use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DetectedDevice {
    pub device_uid: String,
    pub name: String,
    pub mount_path: String,
    pub capacity_bytes: Option<i64>,
    pub free_bytes: Option<i64>,
    pub vendor: Option<String>,
    pub model: Option<String>,
}

pub async fn detect_devices() -> Result<Vec<DetectedDevice>, String> {
    #[cfg(target_os = "linux")]
    {
        detect_linux().await
    }
    #[cfg(target_os = "macos")]
    {
        detect_macos().await
    }
    #[cfg(target_os = "windows")]
    {
        detect_windows().await
    }
}

#[cfg(target_os = "linux")]
async fn detect_linux() -> Result<Vec<DetectedDevice>, String> {
    // Check if we're in Flatpak — lsblk may not be available
    if Path::new("/.flatpak-info").exists() {
        return detect_linux_scan_mounts().await;
    }

    let output = tokio::process::Command::new("lsblk")
        .args([
            "-J", "-o",
            "NAME,LABEL,SIZE,MOUNTPOINT,HOTPLUG,TRAN,MODEL,SERIAL,UUID,FSSIZE,FSAVAIL",
        ])
        .output()
        .await
        .map_err(|e| {
            log::warn!("lsblk failed, falling back to mount scan: {}", e);
            e.to_string()
        });

    match output {
        Ok(out) if out.status.success() => {
            let json_str = String::from_utf8_lossy(&out.stdout);
            parse_lsblk_json(&json_str)
        }
        _ => detect_linux_scan_mounts().await,
    }
}

#[cfg(target_os = "linux")]
fn parse_lsblk_json(json_str: &str) -> Result<Vec<DetectedDevice>, String> {
    let parsed: serde_json::Value =
        serde_json::from_str(json_str).map_err(|e| format!("Failed to parse lsblk JSON: {}", e))?;

    let mut devices = Vec::new();

    fn collect_devices(
        device: &serde_json::Value,
        parent_tran: Option<&str>,
        parent_model: Option<&str>,
        parent_serial: Option<&str>,
        devices: &mut Vec<DetectedDevice>,
    ) {
        let hotplug = device.get("hotplug").and_then(|v| {
            v.as_bool().or_else(|| v.as_str().map(|s| s == "1"))
        }).unwrap_or(false);
        let tran = device.get("tran").and_then(|v| v.as_str()).or(parent_tran);
        let model = device.get("model").and_then(|v| v.as_str()).or(parent_model);
        let serial = device.get("serial").and_then(|v| v.as_str()).or(parent_serial);
        let mountpoint = device.get("mountpoint").and_then(|v| v.as_str());
        let is_usb = tran.map(|t| t == "usb").unwrap_or(false);

        let mp = match mountpoint {
            Some(mp) if hotplug && is_usb && !mp.is_empty() => mp,
            _ => {
                // Recurse into children (partitions) even if this device doesn't match
                if let Some(children) = device.get("children").and_then(|v| v.as_array()) {
                    for child in children {
                        collect_devices(child, tran, model, serial, devices);
                    }
                }
                return;
            }
        };

        let label = device.get("label").and_then(|v| v.as_str()).unwrap_or("");
        let uuid = device.get("uuid").and_then(|v| v.as_str()).unwrap_or("");
        let size_str = device.get("size").and_then(|v| v.as_str()).unwrap_or("");

        // Key device identity on the filesystem UUID first — it's stable and matches
        // what the mount-scan fallback derives, so the same stick keeps ONE identity
        // regardless of which detection path ran (fixes recurring full re-syncs).
        let uid = if !uuid.is_empty() {
            format!("uuid:{}", uuid)
        } else if !serial.unwrap_or("").is_empty() {
            format!("serial:{}", serial.unwrap_or(""))
        } else {
            format!("label:{}:{}", label, size_str)
        };

        let name = if !label.is_empty() {
            label.to_string()
        } else if let Some(m) = model {
            m.trim().to_string()
        } else {
            mp.rsplit('/').next().unwrap_or("USB Device").to_string()
        };

        let capacity = parse_lsblk_size(device.get("fssize").and_then(|v| v.as_str()));
        let free = parse_lsblk_size(device.get("fsavail").and_then(|v| v.as_str()));

        devices.push(DetectedDevice {
            device_uid: uid,
            name,
            mount_path: mp.to_string(),
            capacity_bytes: capacity,
            free_bytes: free,
            vendor: None,
            model: model.map(|m| m.trim().to_string()),
        });

        // Recurse into children (partitions)
        if let Some(children) = device.get("children").and_then(|v| v.as_array()) {
            for child in children {
                collect_devices(child, tran, model, serial, devices);
            }
        }
    }

    if let Some(blockdevices) = parsed.get("blockdevices").and_then(|v| v.as_array()) {
        for dev in blockdevices {
            collect_devices(dev, None, None, None, &mut devices);
        }
    }

    Ok(devices)
}

#[cfg(target_os = "linux")]
fn parse_lsblk_size(size_str: Option<&str>) -> Option<i64> {
    let s = size_str?.trim();
    if s.is_empty() {
        return None;
    }
    // lsblk SIZE can be like "14.9G", "500M", "1T", etc.
    let (num_part, suffix) = if let Some(n) = s.strip_suffix('G') {
        (n, 1_073_741_824i64)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1_048_576i64)
    } else if let Some(n) = s.strip_suffix('T') {
        (n, 1_099_511_627_776i64)
    } else if let Some(n) = s.strip_suffix('K') {
        (n, 1024i64)
    } else {
        // Try parsing as bytes directly
        return s.parse::<i64>().ok();
    };
    num_part.parse::<f64>().ok().map(|n| (n * suffix as f64) as i64)
}

#[cfg(target_os = "linux")]
async fn detect_linux_scan_mounts() -> Result<Vec<DetectedDevice>, String> {
    let mut devices = Vec::new();
    let user = std::env::var("USER").unwrap_or_default();

    let scan_dirs = vec![
        format!("/run/media/{}", user),
        format!("/media/{}", user),
    ];

    for dir in scan_dirs {
        let path = Path::new(&dir);
        if !path.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let mount_path = entry.path();
                if mount_path.is_dir() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let (capacity, free) = get_fs_stats(&mount_path);
                    // Prefer the filesystem UUID so this matches the lsblk path's identity;
                    // only fall back to the (stable) mount name if the UUID can't be read.
                    let uid = fs_uuid_for_mount(&mount_path.to_string_lossy())
                        .map(|u| format!("uuid:{}", u))
                        .unwrap_or_else(|| format!("mount:{}", name));
                    devices.push(DetectedDevice {
                        device_uid: uid,
                        name,
                        mount_path: mount_path.to_string_lossy().to_string(),
                        capacity_bytes: capacity,
                        free_bytes: free,
                        vendor: None,
                        model: None,
                    });
                }
            }
        }
    }

    Ok(devices)
}

#[cfg(target_os = "macos")]
async fn detect_macos() -> Result<Vec<DetectedDevice>, String> {
    let output = tokio::process::Command::new("diskutil")
        .args(["list", "-plist", "external"])
        .output()
        .await
        .map_err(|e| format!("diskutil failed: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    // Get list of external disk identifiers, then check /Volumes
    let mut devices = Vec::new();

    // Simpler approach: scan /Volumes for non-system volumes
    if let Ok(entries) = std::fs::read_dir("/Volumes") {
        for entry in entries.flatten() {
            let mount_path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip the main system volume
            if name == "Macintosh HD" || name == "Macintosh HD - Data" {
                continue;
            }

            // Check if it's a removable volume via diskutil
            let info_output = tokio::process::Command::new("diskutil")
                .args(["info", "-plist", &mount_path.to_string_lossy()])
                .output()
                .await;

            if let Ok(info) = info_output {
                let info_str = String::from_utf8_lossy(&info.stdout);
                let is_removable = info_str.contains("<key>Removable</key>") &&
                    info_str.contains("<true/>");
                let is_external = info_str.contains("<key>Internal</key>") &&
                    // Internal = false means external
                    info_str.find("<key>Internal</key>")
                        .and_then(|pos| info_str[pos..].find("<false/>"))
                        .map(|p| p < 100) // <false/> should be close after the key
                        .unwrap_or(false);

                if is_removable || is_external {
                    let (capacity, free) = get_fs_stats(&mount_path);
                    let uid = format!("vol:{}:{}", name, capacity.unwrap_or(0));
                    devices.push(DetectedDevice {
                        device_uid: uid,
                        name,
                        mount_path: mount_path.to_string_lossy().to_string(),
                        capacity_bytes: capacity,
                        free_bytes: free,
                        vendor: None,
                        model: None,
                    });
                }
            }
        }
    }

    Ok(devices)
}

#[cfg(target_os = "windows")]
async fn detect_windows() -> Result<Vec<DetectedDevice>, String> {
    let output = tokio::process::Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-Volume | Where-Object { $_.DriveType -eq 'Removable' -and $_.DriveLetter } | Select-Object DriveLetter, FileSystemLabel, Size, SizeRemaining | ConvertTo-Json",
        ])
        .output()
        .await
        .map_err(|e| format!("PowerShell failed: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    let parsed: serde_json::Value = serde_json::from_str(&json_str)
        .map_err(|e| format!("Failed to parse PowerShell JSON: {}", e))?;

    let mut devices = Vec::new();
    let volumes = if parsed.is_array() {
        parsed.as_array().cloned().unwrap_or_default()
    } else {
        vec![parsed]
    };

    for vol in volumes {
        let drive_letter = vol.get("DriveLetter").and_then(|v| v.as_str()).unwrap_or("");
        if drive_letter.is_empty() {
            continue;
        }
        let label = vol.get("FileSystemLabel").and_then(|v| v.as_str()).unwrap_or("");
        let size = vol.get("Size").and_then(|v| v.as_i64());
        let free = vol.get("SizeRemaining").and_then(|v| v.as_i64());

        let mount_path = format!("{}:\\", drive_letter);
        let name = if !label.is_empty() {
            label.to_string()
        } else {
            format!("Removable Drive ({}:)", drive_letter)
        };
        let uid = format!("{}:{}:{}", drive_letter, label, size.unwrap_or(0));

        devices.push(DetectedDevice {
            device_uid: uid,
            name,
            mount_path,
            capacity_bytes: size,
            free_bytes: free,
            vendor: None,
            model: None,
        });
    }

    Ok(devices)
}

/// Resolve the filesystem UUID backing a mount point (Linux only) by matching the
/// mounted device from /proc/mounts against the symlinks in /dev/disk/by-uuid.
/// Used so the mount-scan fallback produces the same device identity as the lsblk path.
#[cfg(target_os = "linux")]
fn fs_uuid_for_mount(mount_path: &str) -> Option<String> {
    let mounts = std::fs::read_to_string("/proc/mounts").ok()?;
    let mut device: Option<String> = None;
    for line in mounts.lines() {
        let mut parts = line.split_whitespace();
        let dev = match parts.next() {
            Some(d) => d,
            None => continue,
        };
        // /proc/mounts escapes spaces as \040 in the mount point field.
        let mp = parts.next().unwrap_or("").replace("\\040", " ");
        if mp == mount_path {
            device = Some(dev.to_string());
            break;
        }
    }
    let canon_dev = std::fs::canonicalize(device?).ok()?;
    for entry in std::fs::read_dir("/dev/disk/by-uuid").ok()?.flatten() {
        if let Ok(target) = std::fs::canonicalize(entry.path()) {
            if target == canon_dev {
                return entry.file_name().to_str().map(|s| s.to_string());
            }
        }
    }
    None
}

/// Returns (total_bytes, free_bytes) for the filesystem containing `path`.
/// Unix-only; returns (None, None) on other platforms or on error.
pub fn get_fs_stats(path: &Path) -> (Option<i64>, Option<i64>) {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let c_path = match CString::new(path.to_string_lossy().as_bytes()) {
            Ok(p) => p,
            Err(_) => return (None, None),
        };
        unsafe {
            let mut stat: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(c_path.as_ptr(), &mut stat) == 0 {
                let total = stat.f_blocks as i64 * stat.f_frsize as i64;
                let free = stat.f_bavail as i64 * stat.f_frsize as i64;
                (Some(total), Some(free))
            } else {
                (None, None)
            }
        }
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        (None, None)
    }
}
