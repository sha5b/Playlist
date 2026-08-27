//! Key/value app settings.

use std::sync::Arc;
use tauri::State;

use crate::db::DbPool;

// --- Settings ---

#[tauri::command]
pub fn settings_get(
    db: State<'_, Arc<DbPool>>,
    key: String,
) -> Result<Option<String>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::settings::get_setting(&conn, &key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_set(
    db: State<'_, Arc<DbPool>>,
    key: String,
    value: String,
) -> Result<(), String> {
    let conn = crate::db::lock(&db)?;
    crate::db::settings::set_setting(&conn, &key, &value).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn settings_get_all(
    db: State<'_, Arc<DbPool>>,
) -> Result<Vec<(String, String)>, String> {
    let conn = crate::db::lock(&db)?;
    crate::db::settings::get_all_settings(&conn).map_err(|e| e.to_string())
}
