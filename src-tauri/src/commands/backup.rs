//! Backup, restore, export and data-location commands (thin wrappers over
//! `services::backup`).

use std::path::PathBuf;

use tauri::{AppHandle, Manager, State};

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    BackupInspection, BackupManifest, DataLocation, ExportSummary, IntegrityReport,
};
use crate::services::backup;
use crate::services::local_api::LocalApiServer;
use crate::services::session_key::SessionKey;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {e}")))
}

fn known_migrations() -> Vec<&'static str> {
    crate::db::known_migration_names()
}

/// Write a backup archive to `dest_path` (chosen by the user in a save dialog).
#[tauri::command]
pub async fn create_backup(
    app: AppHandle,
    db: State<'_, Database>,
    dest_path: String,
) -> Result<BackupManifest> {
    let dir = data_dir(&app)?;
    db.with_conn(|conn| {
        let manifest = backup::create_backup(conn, &dir, &PathBuf::from(&dest_path), APP_VERSION)?;
        // Feeds the dashboard "Getting started" checklist; never fails the backup.
        if let Err(e) =
            crate::services::onboarding::record_backup_created(conn, manifest.created_at)
        {
            log::warn!("[BACKUP] Could not record last backup time: {e}");
        }
        Ok(manifest)
    })
}

/// Phones only: the app's Documents folder, where the system's Files export looks for the file.
#[cfg(mobile)]
fn documents_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .document_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get documents dir: {e}")))
}

/// Phones only: a staged path as the string the webview hands back to the finishing command.
#[cfg(mobile)]
fn path_string(path: &std::path::Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| AppError::Internal("Documents path is not valid UTF-8".into()))
}

/// Phones only. Write a backup archive to `Documents/<file_name>` for the Files export to pick up:
/// the dialog plugin's save picker on iOS exports the file that already sits under that name.
/// Nothing is recorded here; `finish_backup_export` does that once the person has chosen a place.
/// A backup that fails leaves no partial file behind.
#[cfg(mobile)]
#[tauri::command]
pub async fn stage_backup_export(
    app: AppHandle,
    db: State<'_, Database>,
    file_name: String,
) -> Result<String> {
    if !backup::is_export_file_name(&file_name) {
        return Err(AppError::Validation(
            "validation.backupExportInvalid".into(),
        ));
    }
    let dir = data_dir(&app)?;
    let dest = documents_dir(&app)?.join(&file_name);
    if let Err(e) = db.with_conn(|conn| backup::create_backup(conn, &dir, &dest, APP_VERSION)) {
        if let Err(removal) = backup::remove_staged_file(&dest) {
            log::warn!("[BACKUP] Could not remove the partial staged backup: {removal}");
        }
        return Err(e);
    }
    path_string(&dest)
}

/// Phones only. Remove the staged archive (a missing one is fine) and, when the export reached a
/// place the person chose, record the backup for the "backup is due" reminder and the Settings
/// row. A cancelled export records nothing. A failed removal is the error; a failed record is only
/// logged, as on the desktop: the export itself has happened.
#[cfg(mobile)]
#[tauri::command]
pub async fn finish_backup_export(
    app: AppHandle,
    db: State<'_, Database>,
    path: String,
    exported: bool,
) -> Result<()> {
    let path = PathBuf::from(path);
    if !backup::is_staged_export_path(&documents_dir(&app)?, &path) {
        return Err(AppError::Validation(
            "validation.backupExportInvalid".into(),
        ));
    }
    let removed = backup::remove_staged_file(&path);
    if exported {
        if let Err(e) = db.with_conn(|conn| {
            crate::services::onboarding::record_backup_created(conn, backup::unix_now())
        }) {
            log::warn!("[BACKUP] Could not record last backup time: {e}");
        }
    }
    removed
}

/// Phones only. Write a short text (the recovery key's) to `Documents/<file_name>` for the Files
/// export to pick up; only the names on `services::backup`'s allow-list are accepted and an
/// existing file is replaced. The webview has no fs commands: this and `discard_text_export` are
/// its only way to put a file in Documents. Returns the path to hand to `discard_text_export`.
#[cfg(mobile)]
#[tauri::command]
pub async fn stage_text_export(
    app: AppHandle,
    file_name: String,
    contents: String,
) -> Result<String> {
    let dest = backup::write_text_export(&documents_dir(&app)?, &file_name, &contents)?;
    path_string(&dest)
}

/// Phones only. Remove a staged text export (a missing one is fine). Refuses any path that is not
/// an allow-listed file directly inside Documents.
#[cfg(mobile)]
#[tauri::command]
pub async fn discard_text_export(app: AppHandle, path: String) -> Result<()> {
    let path = PathBuf::from(path);
    if !backup::is_staged_text_export_path(&documents_dir(&app)?, &path) {
        return Err(AppError::Validation(
            "validation.backupExportInvalid".into(),
        ));
    }
    backup::remove_staged_file(&path)
}

/// Validate an archive and return its manifest for the confirmation dialog.
#[tauri::command]
pub async fn inspect_backup(path: String) -> Result<BackupInspection> {
    backup::inspect_backup(&PathBuf::from(path), &known_migrations())
}

/// Lock the app and replace the current account with the archive's contents.
/// The frontend reloads afterwards; the user unlocks with the backup's password.
#[tauri::command]
pub async fn restore_backup(
    app: AppHandle,
    db: State<'_, Database>,
    session: State<'_, SessionKey>,
    local_api: State<'_, LocalApiServer>,
    path: String,
) -> Result<()> {
    let dir = data_dir(&app)?;
    local_api.stop().await;
    crate::services::auth::logout(&db, &session);
    crate::commands::attachments::wipe_open_copies(&app);
    backup::restore_backup(&dir, &PathBuf::from(path), &known_migrations())?;
    Ok(())
}

/// Dump every user-data table as JSON to `dest_path`.
#[tauri::command]
pub async fn export_all_data(db: State<'_, Database>, dest_path: String) -> Result<ExportSummary> {
    db.with_conn(|conn| backup::export_all_data(conn, &PathBuf::from(&dest_path), APP_VERSION))
}

/// `PRAGMA quick_check` (+ `cipher_integrity_check`).
#[tauri::command]
pub async fn verify_database(db: State<'_, Database>) -> Result<IntegrityReport> {
    db.with_conn(backup::verify_database)
}

/// Where the data lives, with the folder's total size.
#[tauri::command]
pub async fn get_data_location(app: AppHandle) -> Result<DataLocation> {
    let dir = data_dir(&app)?;
    let db_path = dir.join(backup::DB_FILE_NAME);
    backup::data_location(&dir, &db_path)
}

/// Reveal the data folder in the system file manager.
#[tauri::command]
pub async fn open_data_folder(app: AppHandle) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = data_dir(&app)?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| AppError::Internal(format!("Failed to create data dir: {e}")))?;
    app.opener()
        .open_path(dir.to_string_lossy().into_owned(), None::<&str>)
        .map_err(|e| AppError::Internal(format!("Failed to open data folder: {e}")))
}
