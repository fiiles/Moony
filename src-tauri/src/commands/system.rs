//! System commands: access to the application log and build facts.
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

/// Whether this build was compiled with an analytics key (`VITE_APTABASE_KEY`).
/// Without one the consent prompt and the settings switch stay hidden: there is
/// nothing that could be sent.
#[tauri::command]
pub fn has_analytics() -> bool {
    option_env!("VITE_APTABASE_KEY").is_some()
}
