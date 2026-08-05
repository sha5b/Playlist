//! Last.fm scrobbling commands: auth flow, status, enable/disable.

use std::sync::Arc;
use serde::Serialize;
use tauri::State;
use tauri_plugin_opener::OpenerExt;

use crate::db::{settings, DbPool};
use crate::metadata::scrobble;

#[derive(Debug, Serialize)]
pub struct LastfmAuth {
    pub token: String,
    pub url: String,
}

#[derive(Debug, Serialize)]
pub struct LastfmStatus {
    pub connected: bool,
    pub username: Option<String>,
    pub scrobbling_enabled: bool,
    pub pending_scrobbles: i64,
}

fn read_status(conn: &rusqlite::Connection) -> LastfmStatus {
    let connected = scrobble::session_key(conn).is_some();
    let username = settings::get_setting(conn, scrobble::SETTING_USERNAME)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty());
    let scrobbling_enabled = settings::get_setting(conn, scrobble::SETTING_SCROBBLE_ENABLED)
        .ok()
        .flatten()
        .map(|v| v != "false")
        .unwrap_or(true);
    LastfmStatus {
        connected,
        username,
        scrobbling_enabled,
        pending_scrobbles: scrobble::pending_count(conn),
    }
}

/// Step 1 of the auth flow: request a token and open the authorization page
/// in the user's browser.
#[tauri::command]
pub async fn lastfm_start_auth(app: tauri::AppHandle) -> Result<LastfmAuth, String> {
    let token = scrobble::get_token().await?;
    let url = scrobble::auth_url(&token);
    if let Err(e) = app.opener().open_url(&url, None::<&str>) {
        log::warn!("[lastfm] Failed to open browser: {}", e);
    }
    Ok(LastfmAuth { token, url })
}

/// Step 2: after the user authorized in the browser, exchange the token for
/// a session key and persist it.
#[tauri::command]
pub async fn lastfm_finish_auth(
    db: State<'_, Arc<DbPool>>,
    token: String,
) -> Result<LastfmStatus, String> {
    let (session_key, username) = scrobble::get_session(&token).await?;
    let status = {
        let conn = crate::db::lock(&db)?;
        settings::set_setting(&conn, scrobble::SETTING_SESSION_KEY, &session_key)
            .map_err(|e| e.to_string())?;
        settings::set_setting(&conn, scrobble::SETTING_USERNAME, &username)
            .map_err(|e| e.to_string())?;
        settings::set_setting(&conn, scrobble::SETTING_SCROBBLE_ENABLED, "true")
            .map_err(|e| e.to_string())?;
        read_status(&conn)
    };
    // Anything queued while offline/disconnected can go out now.
    let db_arc = db.inner().clone();
    tauri::async_runtime::spawn(async move {
        scrobble::flush_pending(&db_arc).await;
    });
    Ok(status)
}

#[tauri::command]
pub fn lastfm_get_status(db: State<'_, Arc<DbPool>>) -> Result<LastfmStatus, String> {
    let conn = crate::db::lock(&db)?;
    Ok(read_status(&conn))
}

#[tauri::command]
pub fn lastfm_disconnect(db: State<'_, Arc<DbPool>>) -> Result<LastfmStatus, String> {
    let conn = crate::db::lock(&db)?;
    settings::set_setting(&conn, scrobble::SETTING_SESSION_KEY, "").map_err(|e| e.to_string())?;
    settings::set_setting(&conn, scrobble::SETTING_USERNAME, "").map_err(|e| e.to_string())?;
    // Drop the offline queue — it belongs to the disconnected account.
    let _ = conn.execute("DELETE FROM pending_scrobbles", []);
    Ok(read_status(&conn))
}

#[tauri::command]
pub fn lastfm_set_scrobbling(
    db: State<'_, Arc<DbPool>>,
    enabled: bool,
) -> Result<LastfmStatus, String> {
    let conn = crate::db::lock(&db)?;
    settings::set_setting(
        &conn,
        scrobble::SETTING_SCROBBLE_ENABLED,
        if enabled { "true" } else { "false" },
    )
    .map_err(|e| e.to_string())?;
    Ok(read_status(&conn))
}
