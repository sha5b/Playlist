use std::sync::Arc;
use tauri::State;

use crate::db::DbPool;
use crate::db::devices as db_devices;
use crate::devices::detect;
use crate::devices::DeviceManager;

#[derive(serde::Serialize)]
pub struct ScannedDevice {
    pub device_uid: String,
    pub name: String,
    pub mount_path: String,
    pub capacity_bytes: Option<i64>,
    pub free_bytes: Option<i64>,
    pub vendor: Option<String>,
    pub model: Option<String>,
    pub is_known: bool,
    pub device_id: Option<i64>,
}

#[tauri::command]
pub async fn devices_scan(db: State<'_, Arc<DbPool>>) -> Result<Vec<ScannedDevice>, String> {
    let detected = detect::detect_devices().await?;

    let conn = crate::db::lock(&db)?;
    let known = db_devices::get_devices(&conn).map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for d in detected {
        let known_device = known.iter().find(|k| k.device_uid == d.device_uid);

        // Upsert into DB so we track this device
        let _ = db_devices::upsert_device(
            &conn,
            &d.device_uid,
            &d.name,
            &d.mount_path,
            d.capacity_bytes,
        );

        let device_id = known_device.map(|k| k.id).or_else(|| {
            db_devices::get_device_by_uid(&conn, &d.device_uid)
                .ok()
                .map(|dev| dev.id)
        });

        result.push(ScannedDevice {
            device_uid: d.device_uid,
            name: d.name,
            mount_path: d.mount_path,
            capacity_bytes: d.capacity_bytes,
            free_bytes: d.free_bytes,
            vendor: d.vendor,
            model: d.model,
            is_known: known_device.is_some(),
            device_id,
        });
    }

    Ok(result)
}

#[tauri::command]
pub fn devices_get_all(db: State<'_, Arc<DbPool>>) -> Result<Vec<db_devices::Device>, String> {
    let conn = crate::db::lock(&db)?;
    db_devices::get_devices(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn devices_get_detail(
    db: State<'_, Arc<DbPool>>,
    device_id: i64,
) -> Result<db_devices::DeviceDetail, String> {
    let conn = crate::db::lock(&db)?;
    db_devices::get_device_detail(&conn, device_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn devices_configure(
    db: State<'_, Arc<DbPool>>,
    device_id: i64,
    music_dir: String,
    output_format: String,
    output_bitrate: String,
    generate_m3u: bool,
) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    db_devices::configure_device(&conn, device_id, &music_dir, &output_format, &output_bitrate, generate_m3u)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn devices_add_playlist(
    db: State<'_, Arc<DbPool>>,
    device_id: i64,
    playlist_id: i64,
) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    db_devices::add_device_playlist(&conn, device_id, playlist_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn devices_remove_playlist(
    db: State<'_, Arc<DbPool>>,
    device_id: i64,
    playlist_id: i64,
) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    db_devices::remove_device_playlist(&conn, device_id, playlist_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn devices_sync(
    db: State<'_, Arc<DbPool>>,
    device_manager: State<'_, Arc<DeviceManager>>,
    device_id: i64,
    playlist_id: i64,
) -> Result<(), String> {
    let db_arc = db.inner().clone();
    device_manager
        .start_sync(db_arc, device_id, playlist_id)
        .await
}

#[tauri::command]
pub async fn devices_cancel_sync(
    device_manager: State<'_, Arc<DeviceManager>>,
) -> Result<(), String> {
    device_manager.cancel_sync().await;
    Ok(())
}

#[tauri::command]
pub fn devices_clear_history(
    db: State<'_, Arc<DbPool>>,
    device_id: i64,
) -> Result<i64, String> {
    let conn = crate::db::lock(&db)?;
    db_devices::clear_device_sync_history(&conn, device_id).map_err(|e| e.to_string())
}
