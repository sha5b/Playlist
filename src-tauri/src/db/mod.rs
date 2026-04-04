pub mod albums;
pub mod artists;
pub mod devices;
pub mod downloads;
pub mod migrations;
pub mod models;
pub mod monitored;
pub mod playlists;
pub mod settings;
pub mod tracks;

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub type DbPool = Mutex<Connection>;

pub fn init_db(app_data_dir: &Path) -> Result<DbPool, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(app_data_dir)?;
    let db_path = app_data_dir.join("playlist.db");
    let conn = Connection::open(&db_path)?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;

    migrations::run(&conn)?;

    Ok(Mutex::new(conn))
}

/// Open a second connection to the same database for use by the download manager.
/// WAL mode allows concurrent readers/writers without blocking the main connection.
pub fn open_download_conn(app_data_dir: &Path) -> Result<Connection, Box<dyn std::error::Error>> {
    let db_path = app_data_dir.join("playlist.db");
    let conn = Connection::open(&db_path)?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;
    Ok(conn)
}
