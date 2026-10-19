//! Statement files the system opened in Moony (the share sheet, "Open in Moony"): thin commands
//! over `services::received_files`. Phones only (the module and its handlers are `#[cfg(mobile)]`).
//!
//! `RunEvent::Opened` queues the paths in the managed [`ReceivedFiles`] and emits
//! [`RECEIVED_FILES_EVENT`] (no payload); the webview drains the queue with `take_received_files`
//! at startup and on every event, and removes each plaintext copy with `discard_received_file` once
//! it is imported or abandoned. The paths name the person's bank files and are never logged.

use std::path::PathBuf;

use tauri::{AppHandle, Emitter, Manager, State, Url};

use crate::error::{AppError, Result};
use crate::services::received_files::{self, ReceivedFiles};

/// The event the webview listens to: the queue got new paths, drain it with `take_received_files`.
pub const RECEIVED_FILES_EVENT: &str = "received-files";

/// The two folders iOS copies a received file into (`received_files::inbox_dirs`).
fn inbox_dirs(app: &AppHandle) -> Result<[PathBuf; 2]> {
    let document_dir = app
        .path()
        .document_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get documents dir: {e}")))?;
    let temp_dir = app
        .path()
        .temp_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get temp dir: {e}")))?;
    Ok(received_files::inbox_dirs(
        &document_dir,
        &temp_dir,
        &app.config().identifier,
    ))
}

/// `RunEvent::Opened`: queue the opened files and tell the webview when there is something new.
/// Only the count is logged.
pub fn on_opened(app: &AppHandle, urls: &[Url]) {
    let added = app.state::<ReceivedFiles>().push_urls(urls);
    log::info!(
        "[IMPORT] The system opened {} item(s) in Moony, {added} queued",
        urls.len()
    );
    if added > 0 {
        if let Err(e) = app.emit(RECEIVED_FILES_EVENT, ()) {
            log::warn!("[IMPORT] Could not announce the received files: {e}");
        }
    }
}

/// Remove the copies an interrupted import left in the Inbox folders, off the calling thread,
/// except the paths in `keep`. Best effort: a problem is logged and never fails the caller.
fn spawn_startup_sweep(app: &AppHandle, keep: Vec<PathBuf>) {
    let dirs = match inbox_dirs(app) {
        Ok(dirs) => dirs,
        Err(e) => {
            log::warn!("[IMPORT] Received files sweep skipped: {e}");
            return;
        }
    };
    tauri::async_runtime::spawn_blocking(move || match received_files::sweep(&dirs, &keep) {
        Ok(count) => log::info!("[IMPORT] Received files sweep removed {count} file(s)"),
        Err(e) => log::warn!("[IMPORT] Received files sweep failed: {e}"),
    });
}

/// Phones only. Hand the files the system opened in Moony to the webview, oldest first, and forget
/// them; a second call is empty until something new is opened. The first call of a run also sweeps
/// the Inbox folders of what an earlier run left behind (`ReceivedFiles::claim_startup_sweep`),
/// keeping the files being handed out now.
#[tauri::command]
pub async fn take_received_files(
    app: AppHandle,
    received: State<'_, ReceivedFiles>,
) -> Result<Vec<String>> {
    let paths = received.take();
    if received.claim_startup_sweep() {
        spawn_startup_sweep(&app, paths.clone());
    }
    let taken = paths.len();
    let strings: Vec<String> = paths
        .into_iter()
        .filter_map(|path| path.into_os_string().into_string().ok())
        .collect();
    if strings.len() < taken {
        log::warn!(
            "[IMPORT] {} received file(s) skipped: the path is not valid UTF-8",
            taken - strings.len()
        );
    }
    Ok(strings)
}

/// Phones only. Delete a received file's copy in the app's Inbox folders (a file that is already
/// gone is fine). Refuses any other path with `validation.receivedFileOutside`: the webview has no
/// fs commands, and this is not a way to delete files elsewhere.
#[tauri::command]
pub async fn discard_received_file(app: AppHandle, path: String) -> Result<()> {
    received_files::discard(&PathBuf::from(path), &inbox_dirs(&app)?)
}
