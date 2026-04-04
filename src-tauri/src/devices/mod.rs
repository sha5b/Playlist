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

    pub async fn start_sync(
        &self,
        db: Arc<std::sync::Mutex<rusqlite::Connection>>,
        device_id: i64,
        playlist_id: i64,
    ) -> Result<(), String> {
        let mut handle = self.active_sync.lock().await;

        if handle.is_some() {
            self.cancel_sync_inner(&mut handle).await;
        }

        self.cancel_token.store(false, Ordering::Relaxed);
        let cancel = self.cancel_token.clone();
        let app = self.app_handle.clone();

        *handle = Some(tokio::spawn(async move {
            match sync::sync_playlist_to_device(app, db, device_id, playlist_id, cancel).await {
                Ok(result) => log::info!(
                    "Device sync complete: {} synced, {} failed",
                    result.synced,
                    result.failed,
                ),
                Err(e) => log::error!("Device sync failed: {}", e),
            }
        }));

        Ok(())
    }

    pub async fn cancel_sync(&self) {
        let mut handle = self.active_sync.lock().await;
        self.cancel_sync_inner(&mut handle).await;
    }

    async fn cancel_sync_inner(&self, handle: &mut Option<tokio::task::JoinHandle<()>>) {
        self.cancel_token.store(true, Ordering::Relaxed);
        if let Some(h) = handle.take() {
            let _ = h.await;
        }
    }
}
