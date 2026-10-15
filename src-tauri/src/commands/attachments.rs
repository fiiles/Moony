//! Attachment export commands (thin wrappers over `services::attachments`).

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::AttachmentKind;
use crate::services::attachments;

fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {e}")))
}

/// Save a decrypted copy of an attachment to `dest_path` (chosen by the user in a save dialog).
#[tauri::command]
pub async fn export_attachment(
    app: AppHandle,
    db: State<'_, Database>,
    kind: AttachmentKind,
    id: String,
    dest_path: String,
) -> Result<()> {
    let dir = data_dir(&app)?;
    db.with_conn(|conn| attachments::export(conn, &dir, kind, &id, &PathBuf::from(&dest_path)))
}

/// Save every photo of a batch into `dest_dir` (chosen in a folder dialog); returns how many.
#[tauri::command]
pub async fn export_photo_batch(
    app: AppHandle,
    db: State<'_, Database>,
    batch_id: String,
    dest_dir: String,
) -> Result<u32> {
    let dir = data_dir(&app)?;
    db.with_conn(|conn| {
        attachments::export_photo_batch(conn, &dir, &batch_id, &PathBuf::from(&dest_dir))
    })
}
