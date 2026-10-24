//! Keeps what the phone app stores out of iCloud and computer backups: the glue between
//! `services::device_backup` and the `moony-keychain` plugin's `exclude_from_backup`. Phones only
//! (the module is `#[cfg(mobile)]`).
//!
//! Not a webview command: it runs at every start (`lib.rs`), after a first-run setup and a restore,
//! right after a file is staged in Documents for the Files export, and when a received file is
//! taken. The plugin call is Rust-only (not in the plugin's default permission set). Every function
//! runs the plugin on a blocking thread (the call waits for the native side), logs the outcome
//! without a path and never fails: a path that cannot be excluded is logged and the app carries on.

use std::path::{Path, PathBuf};

use moony_keychain::{Keychain, KeychainExt};
use tauri::{AppHandle, Manager};

use crate::error::{AppError, Result};
use crate::services::device_backup::{self, BackupExcluder, StartFolders, Target};

/// The plugin as the service sees it.
struct PluginExcluder<'a>(&'a Keychain<tauri::Wry>);

impl BackupExcluder for PluginExcluder<'_> {
    fn exclude_from_backup(&self, path: &Path) -> Result<bool> {
        self.0
            .exclude_from_backup(path)
            .map_err(|e| AppError::Internal(e.to_string()))
    }
}

/// Run `work` with the plugin on a blocking thread.
async fn with_excluder(app: &AppHandle, work: impl FnOnce(&PluginExcluder<'_>) + Send + 'static) {
    let app = app.clone();
    let task =
        tauri::async_runtime::spawn_blocking(move || work(&PluginExcluder(app.keychain()))).await;
    if let Err(e) = task {
        log::warn!("[BACKUP] Device backup exclusion task failed: {e}");
    }
}

/// The app data folder, or `None` (logged) when it cannot be worked out.
fn data_dir(app: &AppHandle) -> Option<PathBuf> {
    match app.path().app_data_dir() {
        Ok(dir) => Some(dir),
        Err(e) => {
            log::warn!("[BACKUP] Device backup exclusion skipped: no app data dir: {e}");
            None
        }
    }
}

/// `Documents`, or `None` (logged) when it cannot be worked out.
fn document_dir(app: &AppHandle) -> Option<PathBuf> {
    match app.path().document_dir() {
        Ok(dir) => Some(dir),
        Err(e) => {
            log::warn!("[BACKUP] Device backup exclusion skipped: no documents dir: {e}");
            None
        }
    }
}

/// `Library/WebKit` under the app's container (the iOS home folder). Tauri has no path API for the
/// `Library` folder, so it comes from the home folder. Android has no such folder (its webview data
/// sits in the app's own folders, which the manifest rules of Phase 7 cover).
#[cfg(target_os = "ios")]
fn webkit_dir(app: &AppHandle) -> Option<PathBuf> {
    match app.path().home_dir() {
        Ok(home) => Some(device_backup::webkit_dir(&home)),
        Err(e) => {
            log::warn!("[BACKUP] WebKit storage exclusion skipped: no home dir: {e}");
            None
        }
    }
}

#[cfg(not(target_os = "ios"))]
fn webkit_dir(_app: &AppHandle) -> Option<PathBuf> {
    None
}

/// Every start: the app data folder (created first), the webview's `Library/WebKit` folder (created
/// when missing) and `Documents/Inbox` when it exists.
pub(crate) async fn exclude_at_start(app: &AppHandle) {
    let Some(data_dir) = data_dir(app) else {
        return;
    };
    let folders = StartFolders {
        data_dir,
        webkit_dir: webkit_dir(app),
        received_files_dir: document_dir(app)
            .map(|documents| device_backup::received_files_dir(&documents)),
    };
    with_excluder(app, move |excluder| {
        device_backup::exclude_at_start(excluder, &folders);
    })
    .await;
}

/// After a first-run setup and a restore: the app data folder again, in case it was recreated.
pub(crate) async fn exclude_data_dir(app: &AppHandle) {
    let Some(data_dir) = data_dir(app) else {
        return;
    };
    with_excluder(app, move |excluder| {
        device_backup::exclude_dir_and_log(excluder, Target::Profile, &data_dir);
    })
    .await;
}

/// Right after a file is written to Documents for the Files export (the backup archive, the
/// recovery key's text): the staged copy must not reach a device backup either.
pub(crate) async fn exclude_staged_file(app: &AppHandle, file: &Path) {
    let file = file.to_path_buf();
    with_excluder(app, move |excluder| {
        device_backup::exclude_existing_and_log(excluder, Target::StagedExport, &file);
    })
    .await;
}

/// When a received file is taken: `Documents/Inbox`, in case iOS created (or recreated) it since
/// the start. Nothing happens while the folder does not exist.
pub(crate) async fn exclude_received_files_dir(app: &AppHandle) {
    let Some(documents) = document_dir(app) else {
        return;
    };
    let inbox = device_backup::received_files_dir(&documents);
    with_excluder(app, move |excluder| {
        device_backup::exclude_existing_and_log(excluder, Target::ReceivedFiles, &inbox);
    })
    .await;
}
