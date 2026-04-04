use rusqlite::{params, Connection, Row};
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
    conn.execute(
        "UPDATE devices SET music_dir = ?1, output_format = ?2, output_bitrate = ?3, generate_m3u = ?4
         WHERE id = ?5",
        params![music_dir, output_format, output_bitrate, generate_m3u as i64, device_id],
    )?;
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
    let mut stmt = conn.prepare(
        "SELECT dps.playlist_id, p.name, dps.enabled, dps.last_synced_at,
                (SELECT COUNT(*) FROM playlist_tracks WHERE playlist_id = dps.playlist_id) as total_tracks,
                (SELECT COUNT(*) FROM device_track_sync WHERE device_id = ?1 AND playlist_id = dps.playlist_id) as synced_tracks
         FROM device_playlist_sync dps
         JOIN playlists p ON p.id = dps.playlist_id
         WHERE dps.device_id = ?1",
    )?;
    let links = stmt
        .query_map(params![device_id], |row| {
            Ok(DevicePlaylistLink {
                playlist_id: row.get(0)?,
                playlist_name: row.get(1)?,
                enabled: row.get::<_, i64>(2)? != 0,
                last_synced_at: row.get(3)?,
                total_tracks: row.get(4)?,
                synced_tracks: row.get(5)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(links)
}

pub fn get_device_detail(
    conn: &Connection,
    device_id: i64,
) -> Result<DeviceDetail, Box<dyn std::error::Error>> {
    let device = get_device_by_id(conn, device_id)?;
    let playlists = get_device_playlists(conn, device_id)?;
    let synced_track_count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM device_track_sync WHERE device_id = ?1",
        params![device_id],
        |row| row.get(0),
    )?;
    Ok(DeviceDetail {
        device,
        playlists,
        synced_track_count,
    })
}

pub fn record_synced_track(
    conn: &Connection,
    device_id: i64,
    track_id: i64,
    playlist_id: i64,
    file_path_on_device: &str,
    format: &str,
    source_hash: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    conn.execute(
        "INSERT OR REPLACE INTO device_track_sync
         (device_id, track_id, playlist_id, file_path_on_device, format, synced_at, source_hash)
         VALUES (?1, ?2, ?3, ?4, ?5, datetime('now'), ?6)",
        params![device_id, track_id, playlist_id, file_path_on_device, format, source_hash],
    )?;
    Ok(())
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

/// Get tracks from a playlist that haven't been synced to a device yet
pub fn get_unsynced_tracks(
    conn: &Connection,
    device_id: i64,
    playlist_id: i64,
) -> Result<Vec<crate::db::models::Track>, Box<dyn std::error::Error>> {
    let sql = format!(
        "SELECT {}
         FROM playlist_tracks pt
         JOIN tracks t ON t.id = pt.track_id
         LEFT JOIN artists a ON a.id = t.artist_id
         LEFT JOIN albums al ON al.id = t.album_id
         WHERE pt.playlist_id = ?1
           AND t.id NOT IN (SELECT track_id FROM device_track_sync WHERE device_id = ?2 AND playlist_id = ?1)
         ORDER BY pt.position",
        TRACK_COLUMNS
    );
    let mut stmt = conn.prepare(&sql)?;
    let tracks = stmt
        .query_map(params![playlist_id, device_id], row_to_track)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tracks)
}
