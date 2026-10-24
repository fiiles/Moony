//! Keeps the phone's profile folder out of iCloud and computer backups: the glue between
//! `services::device_backup` and the `moony-keychain` plugin's `exclude_from_backup`. Phones only
//! (the module is `#[cfg(mobile)]`).
//!
//! Not a webview command: it runs at every start (`lib.rs`) and after a first-run setup and a
//! restore, and the plugin call is Rust-only (not in the plugin's default permission set).

use std::path::Path;

use moony_keychain::{Keychain, KeychainExt};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, Result};
use crate::services::device_backup::{self, BackupExcluder};

/// The plugin as the service sees it.
struct PluginExcluder<'a>(&'a Keychain<tauri::Wry>);

impl BackupExcluder for PluginExcluder<'_> {
    fn exclude_from_backup(&self, folder: &Path) -> Result<bool> {
        self.0
            .exclude_from_backup(folder)
            .map_err(|e| AppError::Internal(e.to_string()))
    }
}

/// Make sure the app data folder exists and is excluded from device backups, on a blocking thread
/// (the plugin call waits for the native side), and log the outcome. Never fails: a folder that
/// cannot be excluded is logged and the app carries on.
pub(crate) async fn exclude_data_dir(app: &AppHandle) {
    let data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("[BACKUP] Device backup exclusion skipped: no app data dir: {e}");
            return;
        }
    };
    let app = app.clone();
    let task = tauri::async_runtime::spawn_blocking(move || {
        device_backup::exclude_data_dir_and_log(&PluginExcluder(app.keychain()), &data_dir);
    })
    .await;
    if let Err(e) = task {
        log::warn!("[BACKUP] Device backup exclusion task failed: {e}");
    }
}
