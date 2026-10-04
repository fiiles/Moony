//! System commands: access to the application log.
//!
//! Thin wrapper over `services::logging`; the folder-opening itself mirrors
//! `commands::backup::open_data_folder`.

use tauri::AppHandle;

use crate::error::{AppError, Result};
use crate::services::logging;

/// Reveal the log folder (with `moony.log`) in the system file manager.
#[tauri::command]
pub async fn open_logs_folder(app: AppHandle) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = logging::logs_dir(&app)?;
    app.opener()
        .open_path(dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| AppError::Internal(format!("Failed to open log folder: {e}")))
}
