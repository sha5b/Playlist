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
            "-J", "-b", "-o",
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
        let dev_name = device.get("name").and_then(|v| v.as_str()).unwrap_or("");
        // Removable media isn't only USB: SD cards show up as tran "mmc" (or an
        // mmcblk* device with no tran), and desktop automounts land under
        // /run/media regardless of transport.
        let is_removable_tran = tran.map(|t| t == "usb" || t == "mmc").unwrap_or(false)
            || dev_name.starts_with("mmcblk")
            || mountpoint.map(|mp| mp.starts_with("/run/media/")).unwrap_or(false);

        let mp = match mountpoint {
            Some(mp) if hotplug && is_removable_tran && !mp.is_empty() => mp,
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
        // With `lsblk -b` newer versions emit sizes as JSON numbers, older as strings.
        let size_str = device
            .get("size")
            .map(|v| match v {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Number(n) => n.to_string(),
                _ => String::new(),
            })
            .unwrap_or_default();

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

        let capacity = parse_lsblk_size(device.get("fssize"));
        let free = parse_lsblk_size(device.get("fsavail"));

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
fn parse_lsblk_size(value: Option<&serde_json::Value>) -> Option<i64> {
    let value = value?;
    // With -b, lsblk reports bytes — as a JSON number on newer versions,
    // as a numeric string on older ones. Handle both, keeping the
    // human-readable suffix parser only as a fallback.
    if let Some(n) = value.as_i64() {
        return Some(n);
    }
    let s = value.as_str()?.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(n) = s.parse::<i64>() {
        return Some(n);
    }
    // Fallback: lsblk SIZE can be like "14.9G", "500M", "1T", etc.
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
                // Parse the value ADJACENT to each key — a bare `contains("<true/>")`
                // matches anywhere in the plist, so internal volumes passed the check.
                let is_removable = plist_bool_after_key(&info_str, "Removable") == Some(true);
                // Internal = false means external
                let is_external = plist_bool_after_key(&info_str, "Internal") == Some(false);

                if is_removable || is_external {
                    let (capacity, free) = get_fs_stats(&mount_path);
                    // Key identity on the VolumeUUID when present — the volume
                    // name/capacity combo isn't stable across renames/reformats.
                    let uid = match plist_string_after_key(&info_str, "VolumeUUID") {
                        Some(uuid) if !uuid.is_empty() => format!("uuid:{}", uuid),
                        _ => format!("vol:{}:{}", name, capacity.unwrap_or(0)),
                    };
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

/// Find `<key>KEY</key>` in a plist and return the boolean tag that immediately
/// follows it (`<true/>` / `<false/>`), or None if the key is absent or followed
/// by something else.
#[cfg(target_os = "macos")]
fn plist_bool_after_key(info: &str, key: &str) -> Option<bool> {
    let needle = format!("<key>{}</key>", key);
    let pos = info.find(&needle)?;
    let rest = info[pos + needle.len()..].trim_start();
    if rest.starts_with("<true/>") {
        Some(true)
    } else if rest.starts_with("<false/>") {
        Some(false)
    } else {
        None
    }
}

/// Find `<key>KEY</key>` in a plist and return the `<string>` value that
/// immediately follows it, if any.
#[cfg(target_os = "macos")]
fn plist_string_after_key(info: &str, key: &str) -> Option<String> {
    let needle = format!("<key>{}</key>", key);
    let pos = info.find(&needle)?;
    let rest = info[pos + needle.len()..].trim_start();
    let value = rest.strip_prefix("<string>")?;
    let end = value.find("</string>")?;
    Some(value[..end].to_string())
}

#[cfg(target_os = "windows")]
async fn detect_windows() -> Result<Vec<DetectedDevice>, String> {
    let mut cmd = tokio::process::Command::new("powershell");
    cmd.args([
        "-NoProfile",
        "-Command",
        "Get-Volume | Where-Object { $_.DriveType -eq 'Removable' -and $_.DriveLetter } | Select-Object DriveLetter, FileSystemLabel, Size, SizeRemaining, UniqueId | ConvertTo-Json",
    ]);
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW: the device page polls this every few seconds, and
        // without the flag each scan opens a console window that steals focus.
        cmd.creation_flags(0x08000000);
    }
    let output = cmd
        .output()
        .await
        .map_err(|e| format!("PowerShell failed: {}", e))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let json_str = String::from_utf8_lossy(&output.stdout);
    // With no removable volumes plugged in, ConvertTo-Json emits nothing at all —
    // that's "no devices", not a parse error.
    let json_str = json_str.trim();
    if json_str.is_empty() {
        return Ok(Vec::new());
    }
    let parsed: serde_json::Value = serde_json::from_str(json_str)
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
        // Drive letters change between plugs — key identity on the volume's
        // UniqueId when available so the same stick keeps one device row.
        let unique_id = vol
            .get("UniqueId")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim();
        let uid = if !unique_id.is_empty() {
            format!("winvol:{}", unique_id)
        } else {
            format!("{}:{}:{}", drive_letter, label, size.unwrap_or(0))
        };

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
