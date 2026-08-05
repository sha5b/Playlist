//! Folder watch / auto-import service.
//!
//! Watches a user-configured list of folders recursively and automatically
//! imports new audio files into the library (e.g. Bandcamp purchases
//! downloaded outside the app). Events are debounced (3s quiet period) and
//! files are checked for size stability before import so files still being
//! written are picked up once complete.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use tauri::{AppHandle, Emitter, Manager};

use crate::db::DbPool;

/// Settings keys (stored in the existing `settings` table).
pub const SETTING_ENABLED: &str = "watch_enabled";
pub const SETTING_FOLDERS: &str = "watch_folders";

/// Quiet period after the last filesystem event before a batch is processed.
const DEBOUNCE_SECS: u64 = 3;

/// Manages the lifetime of the filesystem watcher. Re-created from settings
/// whenever the watched-folder configuration changes.
pub struct WatchManager {
    app: AppHandle,
    debouncer: Mutex<Option<Debouncer<RecommendedWatcher>>>,
}

/// Read the watch configuration (enabled flag + folder list) from settings.
pub fn read_config(conn: &rusqlite::Connection) -> (bool, Vec<String>) {
    let enabled = crate::db::settings::get_setting(conn, SETTING_ENABLED)
        .ok()
        .flatten()
        .map(|v| v == "true")
        .unwrap_or(false);
    let folders = crate::db::settings::get_setting(conn, SETTING_FOLDERS)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str::<Vec<String>>(&json).ok())
        .unwrap_or_default();
    (enabled, folders)
}

impl WatchManager {
    pub fn new(app: AppHandle) -> Self {
        Self {
            app,
            debouncer: Mutex::new(None),
        }
    }

    /// (Re)start watchers from the current settings. Stops all existing
    /// watchers first; does nothing further if the feature is disabled or
    /// no folders are configured.
    ///
    /// NOTE: acquires the DB lock — callers must not hold it.
    pub fn refresh(&self) -> Result<(), String> {
        let (enabled, folders) = {
            let db = self.app.state::<Arc<DbPool>>();
            let conn = crate::db::lock(&db)?;
            read_config(&conn)
        };

        let mut guard = self
            .debouncer
            .lock()
            .map_err(|e| format!("watch lock poisoned: {}", e))?;
        // Drop any existing debouncer (stops its watcher threads).
        *guard = None;

        if !enabled || folders.is_empty() {
            log::info!("Folder watch inactive (enabled: {}, folders: {})", enabled, folders.len());
            return Ok(());
        }

        let app = self.app.clone();
        let mut debouncer = new_debouncer(
            Duration::from_secs(DEBOUNCE_SECS),
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    let mut paths: Vec<PathBuf> =
                        events.into_iter().map(|e| e.path).collect();
                    paths.sort();
                    paths.dedup();
                    paths.retain(|p| p.is_file() && crate::metadata::tags::is_audio_file(p));
                    if paths.is_empty() {
                        return;
                    }
                    let app = app.clone();
                    // The size-stability check sleeps; run it off the
                    // debouncer's callback thread.
                    std::thread::spawn(move || handle_changed_files(&app, paths));
                }
                Err(e) => log::warn!("Folder watch error: {}", e),
            },
        )
        .map_err(|e| format!("failed to create folder watcher: {}", e))?;

        let mut watching = 0usize;
        for folder in &folders {
            let path = Path::new(folder);
            if !path.is_dir() {
                log::warn!("Watched folder does not exist, skipping: {}", folder);
                continue;
            }
            match debouncer.watcher().watch(path, RecursiveMode::Recursive) {
                Ok(()) => watching += 1,
                Err(e) => log::warn!("Failed to watch folder {}: {}", folder, e),
            }
        }

        if watching > 0 {
            *guard = Some(debouncer);
        }
        log::info!("Folder watch active on {}/{} folder(s)", watching, folders.len());
        Ok(())
    }
}

/// Wait until a file's size is non-zero and stable across two consecutive
/// checks 1s apart (i.e. it is no longer being written). Gives up after ~20s.
fn wait_until_stable(path: &Path) -> bool {
    let mut last_size: Option<u64> = None;
    for _ in 0..20 {
        let size = match std::fs::metadata(path) {
            Ok(m) => m.len(),
            Err(_) => return false, // deleted/moved away
        };
        if size > 0 && last_size == Some(size) {
            return true;
        }
        last_size = Some(size);
        std::thread::sleep(Duration::from_secs(1));
    }
    log::warn!("Watched file never stabilized, skipping for now: {:?}", path);
    false
}

/// Import a debounced batch of new/modified audio files. Files already in
/// the library are skipped inside `import_audio_files`.
fn handle_changed_files(app: &AppHandle, paths: Vec<PathBuf>) {
    let stable: Vec<PathBuf> = paths.into_iter().filter(|p| wait_until_stable(p)).collect();
    if stable.is_empty() {
        return;
    }

    let covers_dir = match app.path().app_data_dir() {
        Ok(dir) => dir.join("covers"),
        Err(e) => {
            log::warn!("Folder watch: cannot resolve app data dir: {}", e);
            return;
        }
    };

    let imported = {
        let db = app.state::<Arc<DbPool>>();
        let conn = match crate::db::lock(&db) {
            Ok(c) => c,
            Err(e) => {
                log::warn!("Folder watch: {}", e);
                return;
            }
        };
        match crate::commands::import_audio_files(&conn, &covers_dir, &stable) {
            Ok(n) => n,
            Err(e) => {
                log::warn!("Folder watch import failed: {}", e);
                return;
            }
        }
    };

    if imported > 0 {
        log::info!("Folder watch imported {} new track(s)", imported);
        let _ = app.emit("library-updated", ());
        let _ = app.emit("watch-import", serde_json::json!({ "imported": imported }));
    }
}
