//! Full library reset: wipe tables, delete download files and cover art.

use std::sync::Arc;
use tauri::{Emitter, Manager, State};

use crate::db::DbPool;

fn download_file_paths(conn: &rusqlite::Connection) -> Result<Vec<String>, String> {
    // The configured download directory may be the user's OS Music folder.
    // Recursively deleting it would also erase files Playlist never created.
    // Only remove exact files that the database identifies as downloads.
    let mut stmt = conn
        .prepare(
            "SELECT file_path FROM downloads WHERE file_path IS NOT NULL
             UNION
             SELECT file_path FROM tracks WHERE source_platform = 'download'",
        )
        .map_err(|e| e.to_string())?;
    let paths = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    Ok(paths)
}

fn cleanup_download_files(paths: Vec<String>) {
    for path in paths {
        if let Err(e) = std::fs::remove_file(&path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                log::warn!("Failed to remove downloaded file {}: {}", path, e);
            }
        }
    }
}

fn reset_database_tables(conn: &rusqlite::Connection) -> Result<(), String> {
    let result = conn.execute_batch(
        "BEGIN IMMEDIATE;
         DELETE FROM pending_scrobbles;
         DELETE FROM monitored_playlist_entries;
         DELETE FROM playlist_tracks;
         DELETE FROM downloads;
         DELETE FROM tracks;
         DELETE FROM albums;
         DELETE FROM artists;
         DELETE FROM playlists;
         DROP TABLE IF EXISTS tracks_fts;
         CREATE VIRTUAL TABLE tracks_fts USING fts5(
             title, artist_name, album_title, album_artist, genre,
             content='',
             contentless_delete=1,
             tokenize='unicode61 remove_diacritics 2'
         );
         COMMIT;"
    );
    if let Err(e) = result {
        let _ = conn.execute_batch("ROLLBACK");
        return Err(e.to_string());
    }
    Ok(())
}

fn cleanup_cover_art(app_handle: &tauri::AppHandle) {
    if let Ok(covers_dir) = app_handle.path().app_data_dir().map(|d| d.join("covers")) {
        if covers_dir.exists() {
            if let Err(e) = std::fs::remove_dir_all(&covers_dir) {
                log::warn!("Failed to remove covers directory: {}", e);
            }
            if let Err(e) = std::fs::create_dir_all(&covers_dir) {
                log::warn!("Failed to recreate covers directory: {}", e);
            }
        }
    }
}

/// Delete all library data: tracks (and their files), albums, artists, playlists, downloads, monitored entries.
/// Settings are preserved.
#[tauri::command]
pub async fn library_reset(
    db: State<'_, Arc<DbPool>>,
    manager: State<'_, Arc<crate::download::DownloadManager>>,
    app_handle: tauri::AppHandle,
    delete_files: bool,
) -> Result<(), String> {
    // Cancel all active downloads before wiping data
    manager.cancel_all().await;

    let conn = crate::db::lock(&db)?;

    // Capture exact paths before clearing their rows, but only delete the files
    // after the database reset succeeds. A database error must not leave intact
    // rows pointing at files that were already destroyed.
    let files_to_delete = if delete_files {
        download_file_paths(&conn)?
    } else {
        Vec::new()
    };
    reset_database_tables(&conn)?;
    cleanup_download_files(files_to_delete);
    cleanup_cover_art(&app_handle);

    // The frontend listens for "library-updated" — "library-changed" was a
    // dead event, so pages kept showing the wiped library after a reset.
    let _ = app_handle.emit("library-updated", ());
    log::info!("Library reset complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cleanup_removes_only_recorded_downloads() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE downloads (file_path TEXT);
             CREATE TABLE tracks (file_path TEXT NOT NULL, source_platform TEXT);",
        )
        .unwrap();

        let dir = std::env::temp_dir().join(format!(
            "playlist-reset-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let downloaded = dir.join("playlist-download.mp3");
        let unrelated = dir.join("personal-recording.wav");
        std::fs::write(&downloaded, b"download").unwrap();
        std::fs::write(&unrelated, b"personal").unwrap();
        conn.execute(
            "INSERT INTO downloads (file_path) VALUES (?1)",
            [downloaded.to_string_lossy().as_ref()],
        )
        .unwrap();

        let paths = download_file_paths(&conn).unwrap();
        cleanup_download_files(paths);

        assert!(!downloaded.exists());
        assert!(unrelated.exists());
        std::fs::remove_file(unrelated).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }
}
