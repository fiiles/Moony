//! Backup, restore, export and data-location commands (thin wrappers over
//! `services::backup`).

use std::path::PathBuf;

#[cfg(mobile)]
use tauri::Manager;
use tauri::{AppHandle, State};

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    BackupInspection, BackupManifest, DataLocation, ExportSummary, IntegrityReport,
};
use crate::services::backup;
use crate::services::local_api::LocalApiServer;
use crate::services::profile_dir;
use crate::services::session_key::SessionKey;

const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The open profile's folder (ADR 0037): what a backup reads and what the data location shows.
/// A restore always writes the real profile's folder.
fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    profile_dir::open_profile_dir(app)
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

/// Phones only: where an export waits for the save picker. iOS: the app's Documents folder, where
/// the Files export looks for the file. Android: `exports` in the app's private cache folder,
/// created when missing; there the save picker hands back the URI of an empty document and
/// `write_staged_export` copies the staged file into it (Tauri's Documents on Android is on shared
/// external storage, no place for a recovery key's plain text).
#[cfg(mobile)]
pub(crate) fn staging_dir(app: &AppHandle) -> Result<PathBuf> {
    #[cfg(target_os = "android")]
    {
        let dir = app
            .path()
            .app_cache_dir()
            .map_err(|e| AppError::Internal(format!("Failed to get cache dir: {e}")))?
            .join("exports");
        std::fs::create_dir_all(&dir)
            .map_err(|e| AppError::Internal(format!("Failed to create the exports folder: {e}")))?;
        Ok(dir)
    }
    #[cfg(not(target_os = "android"))]
    app.path()
        .document_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get documents dir: {e}")))
}

/// Phones only. Android: copy a staged export (`stage_backup_export`, `stage_text_export`,
/// `stage_data_export`, `stage_sync_backup_export`, `stage_attachment_export`) into the document the save picker created,
/// `uri` (a `content://` URI). The caller then finishes or discards the staged file as on iOS.
/// Refuses any path that is not a staged export, and is refused on iOS, whose picker exports the
/// staged file itself.
#[cfg(mobile)]
#[tauri::command]
pub async fn write_staged_export(app: AppHandle, path: String, uri: String) -> Result<()> {
    let path = PathBuf::from(path);
    let staging = staging_dir(&app)?;
    let staged_document =
        crate::services::attachment_export::is_staged_path(&data_dir(&app)?, &staging, &path)?;
    if !(backup::is_staged_export_path(&staging, &path)
        || backup::is_staged_text_export_path(&staging, &path)
        || staged_document)
        || !uri.starts_with("content://")
    {
        return Err(AppError::Validation(
            "validation.backupExportInvalid".into(),
        ));
    }
    #[cfg(target_os = "android")]
    {
        use moony_keychain::KeychainExt;
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || app.keychain().copy_to_uri(&path, &uri))
            .await
            .map_err(|e| AppError::Internal(format!("The export did not finish: {e}")))?
            .map_err(|e| AppError::Internal(format!("Failed to write the export: {e}")))
    }
    #[cfg(not(target_os = "android"))]
    Err(AppError::Validation(
        "validation.backupExportInvalid".into(),
    ))
}

/// Phones only: a staged path as the string the webview hands back to the finishing command.
#[cfg(mobile)]
pub(super) fn path_string(path: &std::path::Path) -> Result<String> {
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
    let dest = staging_dir(&app)?.join(&file_name);
    if let Err(e) = db.with_conn(|conn| backup::create_backup(conn, &dir, &dest, APP_VERSION)) {
        if let Err(removal) = backup::remove_staged_file(&dest) {
            log::warn!("[BACKUP] Could not remove the partial staged backup: {removal}");
        }
        return Err(e);
    }
    // The staged archive must not reach a device backup while it waits for the Files sheet.
    crate::commands::device_backup::exclude_staged_file(&app, &dest).await;
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
    if !backup::is_staged_export_path(&staging_dir(&app)?, &path) {
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

/// Phones only. Write a text (the recovery key, or a class's CSV export) to
/// `Documents/<file_name>` for the Files export to pick up; only the names on
/// `services::backup`'s allow-list are accepted and an existing file is replaced. The webview has
/// no fs commands: this and `discard_text_export` are its only way to put a file in Documents.
/// Returns the path to hand to `discard_text_export`.
#[cfg(mobile)]
#[tauri::command]
pub async fn stage_text_export(
    app: AppHandle,
    file_name: String,
    contents: String,
) -> Result<String> {
    let dest = backup::write_text_export(&staging_dir(&app)?, &file_name, &contents)?;
    // The recovery key's text is plain: keep it out of a device backup while it waits.
    crate::commands::device_backup::exclude_staged_file(&app, &dest).await;
    path_string(&dest)
}

/// Phones only. Remove a staged text export (a missing one is fine). Refuses any path that is not
/// an allow-listed file directly inside Documents.
#[cfg(mobile)]
#[tauri::command]
pub async fn discard_text_export(app: AppHandle, path: String) -> Result<()> {
    let path = PathBuf::from(path);
    if !backup::is_staged_text_export_path(&staging_dir(&app)?, &path) {
        return Err(AppError::Validation(
            "validation.backupExportInvalid".into(),
        ));
    }
    backup::remove_staged_file(&path)
}

/// Phones only. Write the JSON of every user-data table (`export_all_data`) to
/// `Documents/moony-export.json` for the Files export to pick up; an existing file is replaced. The
/// file is plain: it is kept out of a device backup while it waits, and the frontend removes it
/// with `discard_text_export` whatever the person chose (the name is on the text allow-list, so the
/// sweep of an interrupted export removes it too). The summary's `path` is the staged file.
#[cfg(mobile)]
#[tauri::command]
pub async fn stage_data_export(app: AppHandle, db: State<'_, Database>) -> Result<ExportSummary> {
    let dest = staging_dir(&app)?.join(backup::DATA_EXPORT_FILE_NAME);
    let summary = match db.with_conn(|conn| backup::export_all_data(conn, &dest, APP_VERSION)) {
        Ok(summary) => summary,
        Err(e) => {
            if let Err(removal) = backup::remove_staged_file(&dest) {
                log::warn!("[BACKUP] Could not remove the partial data export: {removal}");
            }
            return Err(e);
        }
    };
    crate::commands::device_backup::exclude_staged_file(&app, &dest).await;
    Ok(summary)
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
    // Before anything stops: the demo is never locked or replaced by a restore.
    profile_dir::ensure_not_demo()?;
    let dir = profile_dir::real_dir(&app)?;
    local_api.stop().await;
    crate::commands::sync::stop_engine_and_wait(&app).await;
    crate::commands::sync::forget_kit_window();
    crate::commands::sync_pairing::forget_pairing();
    crate::commands::sync::clear_copied_code(&app);
    crate::services::auth::logout(&db, &session);
    crate::commands::attachments::wipe_open_copies(&app);
    backup::restore_backup(&dir, &PathBuf::from(path), &known_migrations())?;
    // A real profile exists now: the demo profile goes.
    crate::commands::demo::discard_beside_real_profile(&app);
    // The archive carries its own master key, so the key biometric unlock holds no longer opens
    // the profile: switch it off (the phone's frontend also does so first; twice is harmless).
    crate::commands::biometric::switch_off(&app).await;
    // The restore may have recreated folders: keep the profile out of device backups (phone only).
    #[cfg(mobile)]
    crate::commands::device_backup::exclude_data_dir(&app).await;
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
