//! ReplayGain-style loudness measurement via ffmpeg's ebur128 filter.
//!
//! Measures integrated loudness (LUFS) per track and stores a per-track
//! `gain_db` (target −14 LUFS, clamped to ±12 dB) in the `tracks` table.
//! The audio engine applies `10^(gain_db/20)` as a source amplification
//! when the "Normalize volume" setting is enabled.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tauri::Emitter;

use crate::db::DbPool;

/// Normalization target loudness (streaming-standard).
pub const TARGET_LUFS: f64 = -14.0;
/// Gain is clamped to this magnitude in dB.
pub const MAX_GAIN_DB: f64 = 12.0;

static SCAN_RUNNING: AtomicBool = AtomicBool::new(false);
static SCAN_CANCEL: AtomicBool = AtomicBool::new(false);
/// Guards the compute-on-first-play fallback so rapid track changes don't
/// stack up ffmpeg processes.
static FIRST_PLAY_RUNNING: AtomicBool = AtomicBool::new(false);

/// Convert a measured integrated loudness into a clamped gain in dB.
pub fn gain_from_lufs(lufs: f64) -> f64 {
    (TARGET_LUFS - lufs).clamp(-MAX_GAIN_DB, MAX_GAIN_DB)
}

/// Parse the integrated loudness ("I: -9.8 LUFS") from ffmpeg ebur128 stderr.
/// The summary block is printed last, so scan lines in reverse. Per-frame log
/// lines are prefixed with "[Parsed_ebur128..." and never start with "I:".
fn parse_integrated_lufs(stderr: &str) -> Option<f64> {
    for line in stderr.lines().rev() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("I:") {
            if trimmed.contains("LUFS") {
                if let Some(value) = rest.trim().split_whitespace().next() {
                    if let Ok(lufs) = value.parse::<f64>() {
                        return Some(lufs);
                    }
                }
            }
        }
    }
    None
}

/// Run ffmpeg's ebur128 analysis on a file and return the clamped gain in dB.
/// Blocking — decodes the whole file; run off the audio/UI threads.
pub fn measure_gain_db(ffmpeg_path: &str, file_path: &str) -> Result<f64, String> {
    let mut cmd = Command::new(ffmpeg_path);

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // IDLE_PRIORITY_CLASS | CREATE_NO_WINDOW
        cmd.creation_flags(0x00000040 | 0x08000000);
    }

    cmd.args([
        "-hide_banner",
        "-nostats",
        "-i", file_path,
        "-map", "a:0",
        "-af", "ebur128=framelog=verbose",
        "-f", "null",
        "-",
    ]);
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run ffmpeg for loudness scan: {}", e))?;

    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!(
            "ffmpeg loudness scan failed for '{}': {}",
            file_path,
            stderr.lines().last().unwrap_or("unknown error")
        ));
    }

    let lufs = parse_integrated_lufs(&stderr)
        .ok_or_else(|| format!("Could not parse integrated loudness for '{}'", file_path))?;

    // -70 LUFS is ebur128's silence floor; don't boost digital silence.
    if lufs <= -70.0 {
        return Ok(0.0);
    }

    Ok(gain_from_lufs(lufs))
}

/// Measure a track's gain and persist it. Holds the DB lock only for the UPDATE.
pub fn compute_and_store(db: &DbPool, ffmpeg_path: &str, track_id: i64, file_path: &str) -> Result<f64, String> {
    let gain = measure_gain_db(ffmpeg_path, file_path)?;
    let conn = crate::db::lock(db)?;
    conn.execute(
        "UPDATE tracks SET gain_db = ?1 WHERE id = ?2",
        rusqlite::params![gain, track_id],
    )
    .map_err(|e| e.to_string())?;
    log::info!("[loudness] Track {} gain set to {:.2} dB", track_id, gain);
    Ok(gain)
}

/// Compute-on-first-play fallback: compute a missing gain in the background so
/// it applies on the NEXT play of the track. At most one runs at a time.
pub fn compute_gain_if_missing_async(db: Arc<DbPool>, ffmpeg_path: String, track_id: i64, file_path: String) {
    if FIRST_PLAY_RUNNING.swap(true, Ordering::SeqCst) {
        return; // one background measurement at a time is enough
    }
    std::thread::Builder::new()
        .name("gain-first-play".into())
        .spawn(move || {
            if let Err(e) = compute_and_store(&db, &ffmpeg_path, track_id, &file_path) {
                log::warn!("[loudness] First-play gain compute failed: {}", e);
            }
            FIRST_PLAY_RUNNING.store(false, Ordering::SeqCst);
        })
        .map(|_| ())
        .unwrap_or_else(|e| {
            log::warn!("[loudness] Failed to spawn first-play gain thread: {}", e);
            FIRST_PLAY_RUNNING.store(false, Ordering::SeqCst);
        });
}

/// Request cancellation of a running library gain scan (best effort — the
/// currently analyzed file finishes first).
pub fn cancel_scan() {
    SCAN_CANCEL.store(true, Ordering::SeqCst);
}

#[derive(serde::Serialize, Clone)]
struct GainScanProgress {
    scanned: usize,
    failed: usize,
    total: usize,
    done: bool,
}

/// Start a background scan filling `gain_db` for every track that is missing
/// it. Returns immediately; progress is emitted as `gain-scan-progress`
/// events. No-op error if a scan is already running.
pub fn start_scan(db: Arc<DbPool>, ffmpeg_path: String, app: tauri::AppHandle) -> Result<(), String> {
    if SCAN_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("Loudness scan already running".to_string());
    }
    SCAN_CANCEL.store(false, Ordering::SeqCst);

    std::thread::Builder::new()
        .name("gain-scan".into())
        .spawn(move || {
            // Snapshot pending tracks once so files that fail to analyze
            // are not retried forever within the same scan.
            let pending: Vec<(i64, String)> = {
                match crate::db::lock(&db) {
                    Ok(conn) => {
                        let result = conn
                            .prepare("SELECT id, file_path FROM tracks WHERE gain_db IS NULL")
                            .and_then(|mut stmt| {
                                stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
                                    .collect::<Result<Vec<_>, _>>()
                            });
                        match result {
                            Ok(rows) => rows,
                            Err(e) => {
                                log::error!("[loudness] Scan query failed: {}", e);
                                vec![]
                            }
                        }
                    }
                    Err(e) => {
                        log::error!("[loudness] Scan DB lock failed: {}", e);
                        vec![]
                    }
                }
            };

            let total = pending.len();
            log::info!("[loudness] Gain scan started: {} tracks pending", total);
            let mut scanned = 0usize;
            let mut failed = 0usize;

            for (track_id, file_path) in pending {
                if SCAN_CANCEL.load(Ordering::SeqCst) {
                    log::info!("[loudness] Gain scan cancelled");
                    break;
                }
                match compute_and_store(&db, &ffmpeg_path, track_id, &file_path) {
                    Ok(_) => scanned += 1,
                    Err(e) => {
                        failed += 1;
                        log::warn!("[loudness] {}", e);
                    }
                }
                if (scanned + failed) % 5 == 0 {
                    let _ = app.emit("gain-scan-progress", GainScanProgress { scanned, failed, total, done: false });
                }
            }

            let _ = app.emit("gain-scan-progress", GainScanProgress { scanned, failed, total, done: true });
            log::info!("[loudness] Gain scan finished: {} scanned, {} failed of {}", scanned, failed, total);
            SCAN_RUNNING.store(false, Ordering::SeqCst);
        })
        .map(|_| ())
        .map_err(|e| {
            SCAN_RUNNING.store(false, Ordering::SeqCst);
            format!("Failed to spawn gain scan thread: {}", e)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_summary_integrated_loudness() {
        let stderr = "\
[Parsed_ebur128_0 @ 0x55] t: 2.5  TARGET:-23 LUFS  M: -12.1 S: -13.0  I: -12.5 LUFS  LRA: 0.0 LU
[Parsed_ebur128_0 @ 0x55] Summary:

  Integrated loudness:
    I:         -9.8 LUFS
    Threshold: -20.6 LUFS

  Loudness range:
    LRA:        5.0 LU
";
        assert_eq!(parse_integrated_lufs(stderr), Some(-9.8));
    }

    #[test]
    fn gain_is_clamped() {
        assert_eq!(gain_from_lufs(-14.0), 0.0);
        assert_eq!(gain_from_lufs(-9.0), -5.0);
        assert_eq!(gain_from_lufs(-40.0), MAX_GAIN_DB);
        assert_eq!(gain_from_lufs(0.0), -MAX_GAIN_DB);
    }

    #[test]
    fn parse_returns_none_without_summary() {
        assert_eq!(parse_integrated_lufs("no loudness here"), None);
    }
}
