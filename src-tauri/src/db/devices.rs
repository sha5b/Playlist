use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use super::tracks::{row_to_track, TRACK_COLUMNS};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Device {
    pub id: i64,
    pub device_uid: String,
    pub name: String,
    pub device_type: String,
    pub mount_path: Option<String>,
    pub capacity_bytes: Option<i64>,
    pub music_dir: String,
    pub output_format: String,
    pub output_bitrate: String,
    pub generate_m3u: bool,
    pub first_seen_at: String,
    pub last_seen_at: String,
}

const DEVICE_COLUMNS: &str =
    "id, device_uid, name, device_type, mount_path, capacity_bytes,
     music_dir, output_format, output_bitrate, generate_m3u,
     first_seen_at, last_seen_at";

fn row_to_device(row: &Row) -> Result<Device, rusqlite::Error> {
    Ok(Device {
        id: row.get(0)?,
        device_uid: row.get(1)?,
        name: row.get(2)?,
        device_type: row.get(3)?,
        mount_path: row.get(4)?,
        capacity_bytes: row.get(5)?,
        music_dir: row.get::<_, Option<String>>(6)?.unwrap_or_else(|| "Music".to_string()),
        output_format: row.get::<_, Option<String>>(7)?.unwrap_or_else(|| "original".to_string()),
        output_bitrate: row.get::<_, Option<String>>(8)?.unwrap_or_else(|| "320".to_string()),
        generate_m3u: row.get::<_, i64>(9).unwrap_or(1) != 0,
        first_seen_at: row.get(10)?,
        last_seen_at: row.get(11)?,
    })
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DevicePlaylistLink {
    pub playlist_id: i64,
    pub playlist_name: String,
    pub enabled: bool,
    pub last_synced_at: Option<String>,
    pub total_tracks: i64,
    pub synced_tracks: i64,
    pub pending_changes: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DeviceDetail {
    pub device: Device,
    pub playlists: Vec<DevicePlaylistLink>,
    pub synced_track_count: i64,
}

pub fn upsert_device(
    conn: &Connection,
    device_uid: &str,
    name: &str,
    mount_path: &str,
    capacity_bytes: Option<i64>,
) -> Result<Device, Box<dyn std::error::Error>> {
    // This runs on every scan poll (~5s). Avoid constant WAL writes: only write
    // when the device is new or its fields actually changed, and refresh
    // last_seen_at at most once per 60 seconds.
    if let Ok(existing) = get_device_by_uid(conn, device_uid) {
        let fields_changed = existing.name != name
            || existing.mount_path.as_deref() != Some(mount_path)
            || existing.capacity_bytes != capacity_bytes;
        let seen_stale: bool = conn
            .query_row(
                "SELECT last_seen_at <= datetime('now', '-60 seconds') FROM devices WHERE id = ?1",
                params![existing.id],
                |row| row.get(0),
            )
            .unwrap_or(true);
        if fields_changed || seen_stale {
            conn.execute(
                "UPDATE devices SET name = ?2, mount_path = ?3, capacity_bytes = ?4,
                        last_seen_at = datetime('now')
                 WHERE device_uid = ?1",
                params![device_uid, name, mount_path, capacity_bytes],
            )?;
            return get_device_by_uid(conn, device_uid);
        }
        return Ok(existing);
    }

    conn.execute(
        "INSERT INTO devices (device_uid, name, mount_path, capacity_bytes, last_seen_at)
         VALUES (?1, ?2, ?3, ?4, datetime('now'))
         ON CONFLICT(device_uid) DO UPDATE SET
            name = ?2,
            mount_path = ?3,
            capacity_bytes = ?4,
            last_seen_at = datetime('now')",
        params![device_uid, name, mount_path, capacity_bytes],
    )?;
    get_device_by_uid(conn, device_uid)
}

pub fn get_devices(conn: &Connection) -> Result<Vec<Device>, Box<dyn std::error::Error>> {
    let sql = format!(
        "SELECT {} FROM devices ORDER BY last_seen_at DESC",
        DEVICE_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let devices = stmt
        .query_map([], row_to_device)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(devices)
}

pub fn get_device_by_uid(
    conn: &Connection,
    device_uid: &str,
) -> Result<Device, Box<dyn std::error::Error>> {
    let sql = format!(
        "SELECT {} FROM devices WHERE device_uid = ?1",
        DEVICE_COLUMNS
    );
    Ok(conn.query_row(&sql, params![device_uid], row_to_device)?)
}

pub fn get_device_by_id(
    conn: &Connection,
    device_id: i64,
) -> Result<Device, Box<dyn std::error::Error>> {
    let sql = format!(
        "SELECT {} FROM devices WHERE id = ?1",
        DEVICE_COLUMNS
    );
    Ok(conn.query_row(&sql, params![device_id], row_to_device)?)
}

pub fn configure_device(
    conn: &Connection,
    device_id: i64,
    music_dir: &str,
    output_format: &str,
    output_bitrate: &str,
    generate_m3u: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let output_format = output_format.trim().to_ascii_lowercase();
    if !matches!(output_format.as_str(), "original" | "mp3" | "opus" | "flac") {
        return Err(format!("Unsupported device output format: {output_format}").into());
    }
    let music_path = std::path::Path::new(music_dir.trim());
    if music_path.as_os_str().is_empty()
        || music_path.is_absolute()
        || music_path.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err("Music directory must be a relative path inside the device".into());
    }
    let output_bitrate = output_bitrate.trim();
    if output_format != "original"
        && output_format != "flac"
        && (!matches!(output_bitrate, "128" | "192" | "256" | "320"))
    {
        return Err("Unsupported device output bitrate".into());
    }
    // If the music dir changes, the recorded on-device paths no longer point at
    // the files the next sync will write — clear the sync history so stale
    // paths don't poison M3Us and everything re-syncs into the new directory.
    let old_music_dir: Option<String> = conn
        .query_row(
            "SELECT music_dir FROM devices WHERE id = ?1",
            params![device_id],
            |row| row.get(0),
        )
        .ok();
    let changed = conn.execute(
        "UPDATE devices SET music_dir = ?1, output_format = ?2, output_bitrate = ?3, generate_m3u = ?4
         WHERE id = ?5",
        params![music_dir.trim(), output_format, output_bitrate, generate_m3u as i64, device_id],
    )?;
    if changed == 0 {
        return Err(format!("Device not found: {device_id}").into());
    }
    let old_music_dir = old_music_dir.unwrap_or_else(|| "Music".to_string());
    if old_music_dir != music_dir.trim() {
        let cleared = clear_device_sync_history(conn, device_id)?;
        log::info!(
            "Device {} music_dir changed ({:?} -> {:?}); cleared {} sync history rows",
            device_id, old_music_dir, music_dir.trim(), cleared
        );
    }
    Ok(())
}

pub fn add_device_playlist(
    conn: &Connection,
    device_id: i64,
    playlist_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "INSERT OR IGNORE INTO device_playlist_sync (device_id, playlist_id) VALUES (?1, ?2)",
        params![device_id, playlist_id],
    )?;
    Ok(())
}

pub fn remove_device_playlist(
    conn: &Connection,
    device_id: i64,
    playlist_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "DELETE FROM device_playlist_sync WHERE device_id = ?1 AND playlist_id = ?2",
        params![device_id, playlist_id],
    )?;
    Ok(())
}

pub fn get_device_playlists(
    conn: &Connection,
    device_id: i64,
) -> Result<Vec<DevicePlaylistLink>, Box<dyn std::error::Error>> {
    let output_format: String = conn.query_row(
        "SELECT COALESCE(output_format, 'original') FROM devices WHERE id = ?1",
        params![device_id],
        |row| row.get(0),
    )?;
    let mut stmt = conn.prepare(
        "SELECT dps.playlist_id, p.name, dps.enabled, dps.last_synced_at,
                (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = dps.playlist_id) as total_tracks,
                (SELECT COUNT(*) FROM device_track_sync WHERE device_id = ?1 AND playlist_id = dps.playlist_id) as synced_tracks
         FROM device_playlist_sync dps
         JOIN playlists p ON p.id = dps.playlist_id
         WHERE dps.device_id = ?1",
    )?;
    let mut links = stmt
        .query_map(params![device_id], |row| {
            Ok(DevicePlaylistLink {
                playlist_id: row.get(0)?,
                playlist_name: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                last_synced_at: row.get(3)?,
                total_tracks: row.get(4)?,
                synced_tracks: row.get(5)?,
                pending_changes: 0,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    // A sync row is only current when it matches the device's requested format
    // and the source file has not been replaced. A raw COUNT(*) made the UI say
    // "Synced" after changing Opus back to Original, preventing re-sync entirely.
    for link in &mut links {
        let pending = get_unsynced_tracks(conn, device_id, link.playlist_id, &output_format)?;
        let stale = get_stale_synced_tracks(conn, device_id, link.playlist_id)?;
        link.synced_tracks = link.total_tracks.saturating_sub(pending.len() as i64);
        link.pending_changes = (pending.len() + stale.len()) as i64;
    }
    Ok(links)
}

pub fn get_device_detail(
    conn: &Connection,
    device_id: i64,
) -> Result<DeviceDetail, Box<dyn std::error::Error>> {
    let device = get_device_by_id(conn, device_id)?;
    let playlists = get_device_playlists(conn, device_id)?;
    let synced_track_count: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT track_id) FROM device_track_sync WHERE device_id = ?1",
        params![device_id],
        |row| row.get(0),
    )?;
    Ok(DeviceDetail {
        device,
        playlists,
        synced_track_count,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn record_synced_track(
    conn: &Connection,
    device_id: i64,
    track_id: i64,
    playlist_id: i64,
    file_path_on_device: &str,
    format: &str,
    file_size: Option<i64>,
    source_hash: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    // PK is (device_id, track_id, playlist_id): a track shared by several
    // playlists keeps one row per playlist.
    conn.execute(
        "INSERT OR REPLACE INTO device_track_sync
         (device_id, track_id, playlist_id, file_path_on_device, format, file_size, synced_at, source_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'), ?7)",
        params![device_id, track_id, playlist_id, file_path_on_device, format, file_size, source_hash],
    )?;
    Ok(())
}

/// True if this track already has a synced file at `file_path_on_device` on the
/// device (under any playlist) — the physical copy can then be skipped.
pub fn is_file_already_on_device(
    conn: &Connection,
    device_id: i64,
    track_id: i64,
    file_path_on_device: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM device_track_sync
            WHERE device_id = ?1 AND track_id = ?2 AND file_path_on_device = ?3
         )",
        params![device_id, track_id, file_path_on_device],
        |row| row.get(0),
    )?;
    Ok(exists)
}

pub fn get_synced_track_path(
    conn: &Connection,
    device_id: i64,
    track_id: i64,
    playlist_id: i64,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    let path = conn
        .query_row(
            "SELECT file_path_on_device FROM device_track_sync
             WHERE device_id = ?1 AND track_id = ?2 AND playlist_id = ?3",
            params![device_id, track_id, playlist_id],
            |row| row.get(0),
        )
        .optional()?;
    Ok(path)
}

/// Sync rows for (device, playlist) whose track is no longer in the playlist.
/// Returns (track_id, file_path_on_device) pairs.
pub fn get_stale_synced_tracks(
    conn: &Connection,
    device_id: i64,
    playlist_id: i64,
) -> Result<Vec<(i64, String)>, Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare(
        "SELECT track_id, file_path_on_device FROM device_track_sync
         WHERE device_id = ?1 AND playlist_id = ?2
           AND track_id NOT IN (SELECT track_id FROM playlist_tracks WHERE playlist_id = ?2)",
    )?;
    let rows = stmt
        .query_map(params![device_id, playlist_id], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn remove_synced_track(
    conn: &Connection,
    device_id: i64,
    track_id: i64,
    playlist_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "DELETE FROM device_track_sync WHERE device_id = ?1 AND track_id = ?2 AND playlist_id = ?3",
        params![device_id, track_id, playlist_id],
    )?;
    Ok(())
}

/// True if any sync row on this device still references the given on-device path
/// (e.g. the same track synced via another playlist).
pub fn is_device_file_referenced(
    conn: &Connection,
    device_id: i64,
    file_path_on_device: &str,
) -> Result<bool, Box<dyn std::error::Error>> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(
            SELECT 1 FROM device_track_sync WHERE device_id = ?1 AND file_path_on_device = ?2
         )",
        params![device_id, file_path_on_device],
        |row| row.get(0),
    )?;
    Ok(exists)
}

pub fn update_playlist_sync_time(
    conn: &Connection,
    device_id: i64,
    playlist_id: i64,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "UPDATE device_playlist_sync SET last_synced_at = datetime('now')
         WHERE device_id = ?1 AND playlist_id = ?2",
        params![device_id, playlist_id],
    )?;
    Ok(())
}

pub fn clear_device_sync_history(
    conn: &Connection,
    device_id: i64,
) -> Result<i64, Box<dyn std::error::Error>> {
    let deleted = conn.execute(
        "DELETE FROM device_track_sync WHERE device_id = ?1",
        params![device_id],
    )?;
    conn.execute(
        "UPDATE device_playlist_sync SET last_synced_at = NULL WHERE device_id = ?1",
        params![device_id],
    )?;
    Ok(deleted as i64)
}

/// Get tracks from a playlist that need syncing to a device.
///
/// A track is considered unsynced when it has no sync row for this
/// (device, playlist), when the recorded format differs from what the device's
/// current output format would produce, or when the recorded source file size
/// differs from the file on disk (re-download / replacement).
pub fn get_unsynced_tracks(
    conn: &Connection,
    device_id: i64,
    playlist_id: i64,
    output_format: &str,
) -> Result<Vec<crate::db::models::Track>, Box<dyn std::error::Error>> {
    let sql = format!(
        "SELECT {},
                dts.format AS sync_format,
                dts.file_size AS sync_file_size
         FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         LEFT JOIN artists a ON a.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         LEFT JOIN device_track_sync dts
             ON dts.device_id = ?2 AND dts.playlist_id = ?1 AND dts.track_id = t.id
         WHERE pt.playlist_id = ?1
         ORDER BY pt.position",
        TRACK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params![playlist_id, device_id], |row| {
            let track = row_to_track(row)?;
            let sync_format: Option<String> = row.get("sync_format")?;
            let sync_file_size: Option<i64> = row.get("sync_file_size")?;
            Ok((track, sync_format, sync_file_size))
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let mut tracks = Vec::new();
    for (track, sync_format, sync_file_size) in rows {
        let synced_format = match sync_format {
            None => {
                // Never synced for this playlist on this device.
                tracks.push(track);
                continue;
            }
            Some(f) => f,
        };
        // What format would a sync produce right now?
        let expected_format = if output_format.eq_ignore_ascii_case("original") {
            std::path::Path::new(&track.file_path)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("mp3")
                .to_ascii_lowercase()
        } else {
            output_format.to_ascii_lowercase()
        };
        if !synced_format.eq_ignore_ascii_case(&expected_format) {
            tracks.push(track);
            continue;
        }
        // Source file replaced (re-download, different rip)? Compare recorded
        // source size against the current local file. Rows synced before the
        // size column existed (NULL) are treated as up to date.
        if let Some(recorded) = sync_file_size {
            let current = std::fs::metadata(&track.file_path).ok().map(|m| m.len() as i64);
            if let Some(current) = current {
                if current != recorded {
                    tracks.push(track);
                    continue;
                }
            }
        }
    }
    Ok(tracks)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        crate::db::migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO devices (id, device_uid, name, output_format)
             VALUES (1, 'test-device', 'Test device', 'opus')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO playlists (id, name) VALUES (1, 'Test')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO tracks (id, title, file_path, format)
             VALUES (1, 'Song', '/music/Song.FLAC', 'opus')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO playlist_tracks (playlist_id, track_id, position) VALUES (1, 1, 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO device_playlist_sync (device_id, playlist_id) VALUES (1, 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO device_track_sync
             (device_id, track_id, playlist_id, file_path_on_device, format)
             VALUES (1, 1, 1, 'Artist/Album/Song.opus', 'opus')",
            [],
        )
        .unwrap();
        conn
    }

    #[test]
    fn changing_to_original_marks_transcoded_tracks_pending() {
        let conn = device_db();
        assert_eq!(get_device_playlists(&conn, 1).unwrap()[0].synced_tracks, 1);

        configure_device(&conn, 1, "Music", "original", "320", true).unwrap();

        let links = get_device_playlists(&conn, 1).unwrap();
        assert_eq!(links[0].synced_tracks, 0);
        assert_eq!(links[0].pending_changes, 1);
        let pending = get_unsynced_tracks(&conn, 1, 1, "original").unwrap();
        assert_eq!(pending.len(), 1);
        // The real extension, not stale track.format metadata, is expected.
        assert_eq!(pending[0].file_path, "/music/Song.FLAC");
    }

    #[test]
    fn device_configuration_rejects_paths_outside_mount_and_unknown_formats() {
        let conn = device_db();
        assert!(configure_device(&conn, 1, "../Music", "original", "320", true).is_err());
        assert!(configure_device(&conn, 1, "/Music", "original", "320", true).is_err());
        assert!(configure_device(&conn, 1, "Music", "aac", "320", true).is_err());
    }

    #[test]
    fn removed_tracks_remain_pending_until_device_reconciliation() {
        let conn = device_db();
        conn.execute("DELETE FROM playlist_tracks WHERE playlist_id = 1", [])
            .unwrap();

        let link = &get_device_playlists(&conn, 1).unwrap()[0];
        assert_eq!(link.total_tracks, 0);
        assert_eq!(link.synced_tracks, 0);
        assert_eq!(link.pending_changes, 1);
    }
}
