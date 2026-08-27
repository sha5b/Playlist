//! Last.fm authenticated API: desktop auth flow, now-playing updates and
//! scrobbling with an offline queue (`pending_scrobbles` table).
//!
//! Auth flow (desktop):
//!   1. `get_token()` → token
//!   2. user opens `auth_url(token)` in a browser and authorizes the app
//!   3. `get_session(token)` → (session_key, username), stored in settings
//!
//! All write calls are signed: api_sig = md5(concat(sorted key+value pairs) + secret).

use md5::{Digest, Md5};
use rusqlite::{params, Connection};

use crate::db::{settings, DbPool};

// Re-use the read-only API key from lastfm.rs.
use super::lastfm::LASTFM_API_KEY;

/// Last.fm shared secret for the API account. The bundled community API key
/// has NO known secret — authenticated calls (auth, scrobbling) will fail
/// with "Invalid method signature" until a real key/secret pair is provided
/// at build time via the LASTFM_API_KEY / LASTFM_API_SECRET env vars.
/// TODO: replace with a real secret (create an API account at
/// https://www.last.fm/api/account/create).
pub const LASTFM_API_SECRET: &str = match option_env!("LASTFM_API_SECRET") {
    Some(secret) if !secret.is_empty() => secret,
    _ => "REPLACE_WITH_REAL_LASTFM_API_SECRET",
};

const LASTFM_BASE: &str = "https://ws.audioscrobbler.com/2.0/";

// Settings keys
pub const SETTING_SESSION_KEY: &str = "lastfm_session_key";
pub const SETTING_USERNAME: &str = "lastfm_username";
pub const SETTING_SCROBBLE_ENABLED: &str = "lastfm_scrobble_enabled";

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent("Playlist/0.1.0")
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .unwrap_or_default()
}

// ── Signing ───────────────────────────────────────────────────────────────

/// md5(sorted(key+value)... + secret) as lowercase hex.
/// `format` / `callback` params must NOT be included (callers add `format=json`
/// only after signing).
fn api_sig(params: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = params.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let mut base = String::new();
    for (k, v) in sorted {
        base.push_str(k);
        base.push_str(v);
    }
    base.push_str(LASTFM_API_SECRET);
    let mut hasher = Md5::new();
    hasher.update(base.as_bytes());
    hex::encode(hasher.finalize())
}

/// Perform a signed POST call, returning the parsed JSON body.
/// Last.fm errors ({"error": N, "message": ...}) are returned as Err strings
/// prefixed with "lastfm:N:" so callers can inspect the code.
async fn call_signed(method: &str, mut params: Vec<(String, String)>) -> Result<serde_json::Value, String> {
    params.push(("method".to_string(), method.to_string()));
    params.push(("api_key".to_string(), LASTFM_API_KEY.to_string()));
    let sig = api_sig(&params);
    params.push(("api_sig".to_string(), sig));
    params.push(("format".to_string(), "json".to_string()));

    let resp = client()
        .post(LASTFM_BASE)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Last.fm request failed: {}", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse Last.fm response: {}", e))?;

    if let Some(code) = body.get("error").and_then(|e| e.as_i64()) {
        let msg = body.get("message").and_then(|m| m.as_str()).unwrap_or("unknown error");
        return Err(format!("lastfm:{}:{}", code, msg));
    }
    Ok(body)
}

/// Extract a Last.fm error code from an error string produced by `call_signed`.
fn error_code(err: &str) -> Option<i64> {
    err.strip_prefix("lastfm:")?.split(':').next()?.parse().ok()
}

/// True when the build still carries the placeholder API secret — every signed
/// call fails with error 13 ("Invalid method signature") in this state.
pub fn secret_is_placeholder() -> bool {
    LASTFM_API_SECRET == "REPLACE_WITH_REAL_LASTFM_API_SECRET"
}

/// True when the failure is transient (network, rate limit, service down)
/// and the scrobble should stay queued for a later retry.
fn is_retryable(err: &str) -> bool {
    match error_code(err) {
        // 11 = service offline, 16 = temporarily unavailable, 29 = rate limit
        Some(11) | Some(16) | Some(29) => true,
        // 13 = invalid signature. With the placeholder secret EVERY signed call
        // fails this way — keep the scrobbles queued (instead of deleting the
        // user's play history one by one) so they submit once a real secret is
        // baked into the build.
        Some(13) if secret_is_placeholder() => true,
        // No lastfm error code → network / parse failure → retry later
        None => true,
        // Everything else (bad signature, invalid session, invalid params…) is permanent
        Some(_) => false,
    }
}

// ── Auth ──────────────────────────────────────────────────────────────────

/// auth.getToken → request token for the desktop auth flow.
pub async fn get_token() -> Result<String, String> {
    if secret_is_placeholder() {
        return Err(
            "Last.fm scrobbling is unavailable in this build: no API secret was compiled in \
             (set LASTFM_API_KEY and LASTFM_API_SECRET at build time)"
                .to_string(),
        );
    }
    let body = call_signed("auth.getToken", Vec::new()).await?;
    body.get("token")
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "Last.fm did not return a token".to_string())
}

/// URL the user must open in a browser to authorize the token.
pub fn auth_url(token: &str) -> String {
    format!(
        "https://www.last.fm/api/auth/?api_key={}&token={}",
        LASTFM_API_KEY, token
    )
}

/// auth.getSession → (session_key, username). Call after the user authorized.
pub async fn get_session(token: &str) -> Result<(String, String), String> {
    let body = call_signed(
        "auth.getSession",
        vec![("token".to_string(), token.to_string())],
    )
    .await?;
    let session = body.get("session").ok_or("Last.fm did not return a session")?;
    let key = session
        .get("key")
        .and_then(|k| k.as_str())
        .ok_or("Last.fm session has no key")?
        .to_string();
    let name = session
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();
    Ok((key, name))
}

// ── Session / settings helpers ────────────────────────────────────────────

/// Returns the stored session key if the user is connected.
pub fn session_key(conn: &Connection) -> Option<String> {
    settings::get_setting(conn, SETTING_SESSION_KEY)
        .ok()
        .flatten()
        .filter(|s| !s.is_empty())
}

/// Returns the session key only if connected AND scrobbling is enabled.
pub fn scrobbling_session(conn: &Connection) -> Option<String> {
    let sk = session_key(conn)?;
    let enabled = settings::get_setting(conn, SETTING_SCROBBLE_ENABLED)
        .ok()
        .flatten()
        .map(|v| v != "false")
        .unwrap_or(true);
    if enabled { Some(sk) } else { None }
}

// ── Now playing / scrobble ────────────────────────────────────────────────

/// track.updateNowPlaying — fire-and-forget notification that a track started.
pub async fn update_now_playing(
    session_key: &str,
    artist: &str,
    track: &str,
    album: Option<&str>,
    duration_secs: Option<i64>,
) -> Result<(), String> {
    let mut params = vec![
        ("sk".to_string(), session_key.to_string()),
        ("artist".to_string(), artist.to_string()),
        ("track".to_string(), track.to_string()),
    ];
    if let Some(album) = album {
        params.push(("album".to_string(), album.to_string()));
    }
    if let Some(d) = duration_secs.filter(|d| *d > 0) {
        params.push(("duration".to_string(), d.to_string()));
    }
    call_signed("track.updateNowPlaying", params).await?;
    Ok(())
}

/// track.scrobble for a single play.
pub async fn scrobble_one(
    session_key: &str,
    artist: &str,
    track: &str,
    album: Option<&str>,
    duration_secs: Option<i64>,
    timestamp: i64,
) -> Result<(), String> {
    let mut params = vec![
        ("sk".to_string(), session_key.to_string()),
        ("artist".to_string(), artist.to_string()),
        ("track".to_string(), track.to_string()),
        ("timestamp".to_string(), timestamp.to_string()),
    ];
    if let Some(album) = album {
        params.push(("album".to_string(), album.to_string()));
    }
    if let Some(d) = duration_secs.filter(|d| *d > 0) {
        params.push(("duration".to_string(), d.to_string()));
    }
    call_signed("track.scrobble", params).await?;
    Ok(())
}

// ── Offline queue ─────────────────────────────────────────────────────────

struct PendingScrobble {
    id: i64,
    artist: String,
    track: String,
    album: Option<String>,
    duration_secs: Option<i64>,
    played_at: i64,
}

/// Queue a scrobble in the `pending_scrobbles` table (flushed later).
pub fn queue_scrobble(
    conn: &Connection,
    artist: &str,
    track: &str,
    album: Option<&str>,
    duration_secs: Option<i64>,
    played_at_unix: i64,
) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO pending_scrobbles (artist, track, album, duration_secs, played_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![artist, track, album, duration_secs, played_at_unix],
    )?;
    Ok(())
}

pub fn pending_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM pending_scrobbles", [], |row| row.get(0))
        .unwrap_or(0)
}

/// Flush the pending scrobble queue. Sends oldest-first, one at a time.
/// Stops on the first transient failure (so the queue is retried on the next
/// play); drops entries that fail permanently (bad params, invalid session
/// covered by later reconnect).
pub async fn flush_pending(db: &DbPool) {
    // Snapshot the queue and session key while holding the lock, then release
    // it before any network I/O.
    let (sk, batch): (String, Vec<PendingScrobble>) = {
        let conn = match db.lock() {
            Ok(c) => c,
            Err(_) => return,
        };
        let sk = match scrobbling_session(&conn) {
            Some(sk) => sk,
            None => return,
        };
        let mut stmt = match conn.prepare(
            "SELECT id, artist, track, album, duration_secs, played_at
             FROM pending_scrobbles ORDER BY played_at ASC LIMIT 50",
        ) {
            Ok(s) => s,
            Err(_) => return,
        };
        let rows = stmt
            .query_map([], |row| {
                Ok(PendingScrobble {
                    id: row.get(0)?,
                    artist: row.get(1)?,
                    track: row.get(2)?,
                    album: row.get(3)?,
                    duration_secs: row.get(4)?,
                    played_at: row.get(5)?,
                })
            })
            .map(|r| r.filter_map(|x| x.ok()).collect::<Vec<_>>())
            .unwrap_or_default();
        (sk, rows)
    };

    for item in batch {
        let result = scrobble_one(
            &sk,
            &item.artist,
            &item.track,
            item.album.as_deref(),
            item.duration_secs,
            item.played_at,
        )
        .await;

        match result {
            Ok(()) => {
                if let Ok(conn) = db.lock() {
                    let _ = conn.execute("DELETE FROM pending_scrobbles WHERE id = ?1", params![item.id]);
                }
                log::info!("[lastfm] Scrobbled: {} — {}", item.artist, item.track);
            }
            Err(e) if is_retryable(&e) => {
                log::warn!("[lastfm] Scrobble deferred (will retry): {}", e);
                return; // keep this and all later entries queued
            }
            Err(e) => {
                log::warn!("[lastfm] Dropping unscrobbleable entry ({} — {}): {}", item.artist, item.track, e);
                if let Ok(conn) = db.lock() {
                    let _ = conn.execute("DELETE FROM pending_scrobbles WHERE id = ?1", params![item.id]);
                }
            }
        }
    }
}

/// Hook for track-start events from the audio engine: sends
/// track.updateNowPlaying in the background when connected + enabled.
pub fn on_track_started(db: std::sync::Arc<DbPool>, track: crate::audio::queue::QueueTrack) {
    tauri::async_runtime::spawn(async move {
        let sk = {
            let conn = match db.lock() {
                Ok(c) => c,
                Err(_) => return,
            };
            match scrobbling_session(&conn) {
                Some(sk) => sk,
                None => return,
            }
        };
        let artist = match &track.artist_name {
            Some(a) if !a.is_empty() => a.clone(),
            _ => return, // Last.fm requires an artist
        };
        let duration_secs = track.duration_ms.map(|ms| ms / 1000);
        if let Err(e) = update_now_playing(
            &sk,
            &artist,
            &track.title,
            track.album_title.as_deref(),
            duration_secs,
        )
        .await
        {
            log::debug!("[lastfm] updateNowPlaying failed: {}", e);
        }
    });
}
