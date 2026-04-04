use rusqlite::{params, Connection};

pub fn get_setting(conn: &Connection, key: &str) -> Result<Option<String>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
    let mut rows = stmt.query_map(params![key], |row| row.get::<_, String>(0))?;
    match rows.next() {
        Some(Ok(v)) => Ok(Some(v)),
        Some(Err(e)) => Err(e),
        None => Ok(None),
    }
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![key, value],
    )?;
    Ok(())
}

/// Get the cookies-from-browser setting.
/// Returns only the user's explicit choice from settings, or None.
/// No auto-detection — users must explicitly set their browser in settings
/// to avoid issues like Firefox cookies breaking YouTube downloads.
pub fn get_cookies_browser(conn: &Connection) -> Option<String> {
    get_setting(conn, "cookies_from_browser")
        .ok()
        .flatten()
        .filter(|s| !s.is_empty() && s != "none")
}

pub fn get_all_settings(conn: &Connection) -> Result<Vec<(String, String)>, rusqlite::Error> {
    let mut stmt = conn.prepare("SELECT key, value FROM settings ORDER BY key")?;
    let settings = stmt
        .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(settings)
}
