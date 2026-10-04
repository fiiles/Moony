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
    local_api: State<'_, LocalApiServer>,
    path: String,
) -> Result<()> {
    let dir = data_dir(&app)?;
    local_api.stop().await;
    crate::services::auth::logout(&db);
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
