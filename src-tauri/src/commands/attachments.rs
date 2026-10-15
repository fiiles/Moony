//! Attachment commands: exports, opening in another app, wiping the temporary copies, and the
//! background conversion of plain files from earlier versions with its status (thin wrappers over
//! `services::attachments` and `services::attachment_conversion`).

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::db::{files_schema, Database};
use crate::error::{AppError, Result};
use crate::models::{AttachmentKind, AttachmentStatus};
use crate::services::attachment_conversion::{self, ConversionProgress};
use crate::services::{attachment_store, attachments, document_files};

/// Event emitted when a conversion run ends, so the Data settings page and the attachment lists
/// refresh.
pub const CONVERSION_FINISHED_EVENT: &str = "attachments-conversion-finished";

/// Managed state: progress of the conversion run.
#[derive(Default)]
pub struct AttachmentConversion(pub Arc<ConversionProgress>);

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

fn cache_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_cache_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app cache dir: {e}")))
}

/// Write a decrypted copy of a document into the app cache folder and open it in the system app
/// (D3). The copy is wiped on lock, on exit and at the next start.
pub(crate) fn open_elsewhere(
    app: &AppHandle,
    db: &Database,
    kind: AttachmentKind,
    id: &str,
) -> Result<()> {
    let data = data_dir(app)?;
    let cache = cache_dir(app)?;
    let path = db.with_conn(|conn| attachments::open_elsewhere(conn, &data, &cache, kind, id))?;
    document_files::open_with_system(&path)
}

/// Delete the decrypted copies opened in other apps.
pub fn wipe_open_copies(app: &AppHandle) {
    match app.path().app_cache_dir() {
        Ok(cache) => attachments::wipe_open_copies(&cache),
        Err(e) => log::warn!("[ATTACHMENTS] Temporary copies not wiped: {e}"),
    }
}

/// Start converting the plain attachment files of earlier versions in the background (D4), unless
/// a run is already going. Pinned to the current session: a lock or a restore stops it.
pub fn spawn_conversion(app: &AppHandle) {
    let progress = app.state::<AttachmentConversion>().0.clone();
    if progress.running.swap(true, Ordering::SeqCst) {
        return;
    }
    let (db, data) = match (app.state::<Database>().current_session(), data_dir(app)) {
        (Ok(db), Ok(data)) => (db, data),
        _ => {
            progress.running.store(false, Ordering::SeqCst);
            return;
        }
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        if let Err(e) = run_conversion(&db, &data, &progress) {
            log::warn!("[ATTACHMENTS] Conversion stopped: {e}");
        }
        progress.running.store(false, Ordering::SeqCst);
        let _ = app.emit(CONVERSION_FINISHED_EVENT, ());
    });
}

/// One conversion run: finish the last run's quarantine, convert item by item (each file is read
/// without the connection lock), then sweep unreferenced blobs and prune empty legacy folders.
fn run_conversion(db: &Database, data: &Path, progress: &ConversionProgress) -> Result<()> {
    if !db.with_conn(files_schema::is_attached)? {
        return Ok(()); // D11: nothing is converted while the store is unavailable
    }
    db.with_conn(|conn| attachment_conversion::finish_quarantine(conn, data))?;
    let items = db.with_conn(attachment_conversion::pending)?;
    progress.total.store(items.len() as u32, Ordering::SeqCst);
    progress.done.store(0, Ordering::SeqCst);
    progress.failed.store(0, Ordering::SeqCst);
    if !items.is_empty() {
        log::info!(
            "[ATTACHMENTS] Converting {} plain attachment(s)",
            items.len()
        );
    }
    for item in &items {
        let converted = attachment_conversion::read_sources(data, item).and_then(|sources| {
            match sources {
                Some(sources) => db
                    .with_conn(|conn| attachment_conversion::convert(conn, data, item, &sources))
                    .map(Some),
                None => Ok(None), // missing on disk: counted by the status
            }
        });
        match converted {
            Ok(Some(_)) => {
                progress.done.fetch_add(1, Ordering::SeqCst);
            }
            Ok(None) => {}
            Err(AppError::Auth(reason)) => return Err(AppError::Auth(reason)),
            Err(e) => {
                progress.failed.fetch_add(1, Ordering::SeqCst);
                log::warn!("[ATTACHMENTS] An attachment could not be converted: {e}");
            }
        }
    }
    db.with_conn(|conn| {
        let swept = attachment_store::delete_unreferenced(conn)?;
        if swept > 0 {
            log::info!("[ATTACHMENTS] Deleted {swept} unreferenced blob(s)");
        }
        attachment_store::incremental_vacuum(conn)
    })?;
    attachment_conversion::remove_empty_legacy_dirs(data);
    Ok(())
}

/// The encrypted store, the conversion and the leftovers, for the Data settings page.
#[tauri::command]
pub async fn get_attachment_status(
    app: AppHandle,
    db: State<'_, Database>,
    conversion: State<'_, AttachmentConversion>,
) -> Result<AttachmentStatus> {
    let dir = data_dir(&app)?;
    db.with_conn(|conn| attachment_conversion::status(conn, &dir, &conversion.0))
}

/// Show a leftover plain original in the system file manager. Only a path the status currently
/// reports is accepted.
#[tauri::command]
pub async fn reveal_attachment_leftover(
    app: AppHandle,
    db: State<'_, Database>,
    path: String,
) -> Result<()> {
    let dir = data_dir(&app)?;
    let known = db.with_conn(|conn| attachment_conversion::refresh_leftovers(conn, &dir))?;
    if !known.contains(&path) {
        return Err(AppError::NotFound("Not a known leftover".into()));
    }
    tauri_plugin_opener::reveal_item_in_dir(&path)
        .map_err(|e| AppError::Internal(format!("Failed to show the file: {e}")))
}
