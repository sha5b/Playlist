pub mod detect;
pub mod sync;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct DeviceManager {
    app_handle: tauri::AppHandle,
    /// Serializes syncs: a new sync waits for the current one to finish instead of
    /// cancelling it. This is what makes "Sync all" (a loop of start_sync calls, one
    /// per playlist) actually sync every playlist rather than only the last.
    sync_lock: Mutex<()>,
    cancel_token: Arc<AtomicBool>,
}

impl DeviceManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            sync_lock: Mutex::new(()),
            cancel_token: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Sync one playlist to a device, running to completion. Multiple calls queue behind
    /// the `sync_lock` and run one at a time. Returns `Err("Sync cancelled")` if the user
    /// cancelled, so a frontend "sync all" loop can stop the whole batch.
    pub async fn start_sync(
        &self,
        db: Arc<std::sync::Mutex<rusqlite::Connection>>,
        device_id: i64,
        playlist_id: i64,
    ) -> Result<(), String> {
        // Hold the lock for the whole sync so concurrent requests serialize.
        let _guard = self.sync_lock.lock().await;

        // Clear any stale cancel flag before starting: cancel_sync sets the flag
        // unconditionally (even when nothing is running), and a leftover flag
        // would otherwise silently kill this fresh sync.
        self.cancel_token.swap(false, Ordering::Relaxed);

        let cancel = self.cancel_token.clone();
        let app = self.app_handle.clone();
        match sync::sync_playlist_to_device(app, db, device_id, playlist_id, cancel).await {
            Ok(result) => {
                log::info!(
                    "Device sync complete: {} synced, {} failed",
                    result.synced,
                    result.failed,
                );
                // Consume the flag (swap, not load) so a cancel that stopped this
                // sync can't leak into the next one.
                if self.cancel_token.swap(false, Ordering::Relaxed) {
                    return Err("Sync cancelled".to_string());
                }
                Ok(())
            }
            Err(e) => {
                log::error!("Device sync failed: {}", e);
                Err(e)
            }
        }
    }

    pub async fn cancel_sync(&self) {
        self.cancel_token.store(true, Ordering::Relaxed);
    }
}
