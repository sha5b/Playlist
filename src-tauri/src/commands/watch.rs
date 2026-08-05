//! Tauri commands for the folder watch / auto-import feature.

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::db::DbPool;
use crate::watch::{WatchManager, SETTING_ENABLED, SETTING_FOLDERS};

#[derive(Debug, Serialize)]
pub struct WatchStatus {
    pub enabled: bool,
    pub folders: Vec<String>,
}

fn get_status(conn: &rusqlite::Connection) -> WatchStatus {
    let (enabled, folders) = crate::watch::read_config(conn);
    WatchStatus { enabled, folders }
}

fn save_folders(conn: &rusqlite::Connection, folders: &[String]) -> Result<(), String> {
    let json = serde_json::to_string(folders).map_err(|e| e.to_string())?;
    crate::db::settings::set_setting(conn, SETTING_FOLDERS, &json).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn watch_get_status(db: State<'_, Arc<DbPool>>) -> Result<WatchStatus, String> {
    let conn = crate::db::lock(&db)?;
    Ok(get_status(&conn))
}

#[tauri::command]
pub fn watch_set_enabled(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<WatchManager>>,
    enabled: bool,
) -> Result<WatchStatus, String> {
    let status = {
        let conn = crate::db::lock(&db)?;
        crate::db::settings::set_setting(&conn, SETTING_ENABLED, if enabled { "true" } else { "false" })
            .map_err(|e| e.to_string())?;
        get_status(&conn)
        // conn guard dropped here — refresh() re-acquires the DB lock.
    };
    manager.refresh()?;
    Ok(status)
}

#[tauri::command]
pub fn watch_add_folder(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<WatchManager>>,
    path: String,
) -> Result<WatchStatus, String> {
    let status = {
        let conn = crate::db::lock(&db)?;
        let (_, mut folders) = crate::watch::read_config(&conn);
        if !folders.contains(&path) {
            folders.push(path);
            save_folders(&conn, &folders)?;
        }
        get_status(&conn)
    };
    manager.refresh()?;
    Ok(status)
}

#[tauri::command]
pub fn watch_remove_folder(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<WatchManager>>,
    path: String,
) -> Result<WatchStatus, String> {
    let status = {
        let conn = crate::db::lock(&db)?;
        let (_, mut folders) = crate::watch::read_config(&conn);
        folders.retain(|f| f != &path);
        save_folders(&conn, &folders)?;
        get_status(&conn)
    };
    manager.refresh()?;
    Ok(status)
}
