pub mod detect;
pub mod sync;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct DeviceManager {
    app_handle: tauri::AppHandle,
    active_sync: Mutex<Option<tokio::task::JoinHandle<()>>>,
    cancel_token: Arc<AtomicBool>,
}

impl DeviceManager {
    pub fn new(app_handle: tauri::AppHandle) -> Self {
        Self {
            app_handle,
            active_sync: Mutex::new(None),
            cancel_token: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_syncing(&self) -> bool {
        // Check via cancel token — if there's an active handle, it's syncing
        // This is a quick check; the actual handle check requires async
        !self.cancel_token.load(Ordering::Relaxed)
            && self.active_sync.try_lock().map(|g| g.is_some()).unwrap_or(true)
    }

    pub async fn start_sync(
        &self,
        db: Arc<std::sync::Mutex<rusqlite::Connection>>,
        device_id: i64,
        playlist_id: i64,
    ) -> Result<(), String> {
        let mut handle = self.active_sync.lock().await;

        // Cancel any existing sync first
        if let Some(h) = handle.take() {
            self.cancel_token.store(true, Ordering::Relaxed);
            let _ = h.await;
        }

        self.cancel_token.store(false, Ordering::Relaxed);
        let cancel = self.cancel_token.clone();
        let app = self.app_handle.clone();

        let join_handle = tokio::spawn(async move {
            match sync::sync_playlist_to_device(app, db, device_id, playlist_id, cancel).await {
                Ok(result) => {
                    log::info!(
                        "Device sync complete: {} synced, {} failed",
                        result.synced,
                        result.failed
                    );
                }
                Err(e) => {
                    log::error!("Device sync failed: {}", e);
                }
            }
        });

        *handle = Some(join_handle);
        Ok(())
    }

    pub async fn cancel_sync(&self) {
        self.cancel_token.store(true, Ordering::Relaxed);
        let mut handle = self.active_sync.lock().await;
        if let Some(h) = handle.take() {
            let _ = h.await;
        }
    }
}
