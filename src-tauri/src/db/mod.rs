pub mod migrations;
pub mod models;

use rusqlite::Connection;
use std::path::Path;
use std::sync::Mutex;

pub type DbPool = Mutex<Connection>;

pub fn init_db(app_data_dir: &Path) -> Result<DbPool, Box<dyn std::error::Error>> {
    std::fs::create_dir_all(app_data_dir)?;
    let db_path = app_data_dir.join("playlist.db");
    let conn = Connection::open(db_path)?;

    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         PRAGMA busy_timeout = 5000;",
    )?;

    migrations::run(&conn)?;

    Ok(Mutex::new(conn))
}
