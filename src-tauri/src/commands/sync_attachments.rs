//! Fetching documents and photos from other devices on request (ADR 0030): "Download" on one
//! that is not here, "Try again" on one that failed, and "Download everything to this phone".
//! Thin: the flag, one service call under the database lock (`services::attachments`), a wake of
//! the transfer lanes, and the row's content now.
//!
//! Every command starts with `require_feature()`. No command logs its arguments.

use tauri::{AppHandle, State};

use super::sync::{require_feature, wake_transfers};
use crate::db::Database;
use crate::error::Result;
use crate::models::{AttachmentContent, AttachmentKind};
use crate::services::attachments;

/// Asks for the full content of a document or photo that is not here. Content that is here or
/// cannot arrive as things stand is left as it is; the answer is its content state.
#[tauri::command]
pub async fn sync_attachment_download(
    app: AppHandle,
    db: State<'_, Database>,
    kind: AttachmentKind,
    id: String,
) -> Result<AttachmentContent> {
    require_feature()?;
    let content = db.with_conn(|conn| attachments::request_download(conn, kind, &id))?;
    wake_transfers(&app);
    Ok(content)
}

/// "Try again" on one document or photo: what failed to move starts again, up or down. The answer
/// is its content state.
#[tauri::command]
pub async fn sync_attachment_retry(
    app: AppHandle,
    db: State<'_, Database>,
    kind: AttachmentKind,
    id: String,
) -> Result<AttachmentContent> {
    require_feature()?;
    let content = db.with_conn(|conn| attachments::retry(conn, kind, &id))?;
    wake_transfers(&app);
    Ok(content)
}

/// "Download everything": every full content not here is asked for. How many rows are asked for.
#[tauri::command]
pub async fn sync_download_everything(app: AppHandle, db: State<'_, Database>) -> Result<u32> {
    require_feature()?;
    let wanted = db.with_conn(attachments::download_everything)?;
    wake_transfers(&app);
    Ok(wanted)
}
