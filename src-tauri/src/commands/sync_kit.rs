//! Saving the Sync Recovery Kit on the desktop (audit M11): the webview renders the file, Rust
//! opens the native save dialog and writes it owner-only (`services::sync::kit_file`: mode 0600 at
//! creation on macOS and Linux; on Windows the chosen folder's permissions), so the webview needs
//! no file system scope for it. Thin: the flag, the copy window of `sync.rs`, the dialog, the
//! service. Phones save through the Files picker (`stage_text_export`): this command refuses there.
//!
//! Every command starts with `require_feature()`. No command logs its arguments; the contents
//! hold the recovery code and are wiped once written.

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

use super::sync::{data_dir, require_feature, with_kit_draft};
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::sync::{KitFileKind, KitSaveOutcome};
use crate::services::sync::rekey::kit::KitStorage;
use crate::services::sync::{kit_file, setup};

const UNAVAILABLE: &str = "validation.syncUnavailable";

/// Desktop only: on a phone the dialog answers a `content://` or security-scoped URL a plain file
/// write cannot use, so a stray call is `validation.syncUnavailable` before anything runs.
fn desktop_only(on_a_phone: bool) -> Result<()> {
    if on_a_phone {
        Err(AppError::Validation(UNAVAILABLE.into()))
    } else {
        Ok(())
    }
}

/// The open Kit draft's code, within the copy window of this profile session, as copying the
/// code; `validation.syncPasswordRequired` otherwise.
fn draft_code(app: &AppHandle, db: &Database) -> Result<Zeroizing<String>> {
    with_kit_draft(app, db, |draft| {
        db.with_conn(|conn| setup::kit_code(conn, draft))
    })
}

/// Saves the Recovery Kit (desktop; `validation.syncUnavailable` on a phone): allowed while a
/// Kit draft is open (`with_kit_draft`, the copy window); Rust opens the native save dialog, writes `contents` (at most
/// 256 KiB, holding this kit's recovery code; a page with no script or handler) owner-only and
/// atomically (`kit_file::write_kit_file`: mode 0600 at creation on macOS and Linux, the folder's
/// permissions on Windows), never into the data folder, wipes it, and says whether the folder
/// looks cloud-synced; the Kit then counts as saved to a file or to a cloud folder
/// (`KitStorage::File` / `Cloud`, revision §2.7). A closed dialog is `saved: false`. The window
/// is checked again after the dialog: a lock while it was open refuses the write.
#[tauri::command]
pub async fn sync_save_recovery_kit(
    app: AppHandle,
    db: State<'_, Database>,
    kind: KitFileKind,
    contents: String,
) -> Result<KitSaveOutcome> {
    let contents = Zeroizing::new(contents);
    require_feature()?;
    desktop_only(cfg!(mobile))?;
    let code = draft_code(&app, &db)?;
    kit_file::check_contents(kind, &contents, &code)?;
    drop(code);

    let (file_name, filter, extension) = kit_file::file_type(kind);
    let (chosen_tx, chosen_rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .set_file_name(file_name)
        .add_filter(filter, &[extension])
        .save_file(move |chosen| {
            // The receiver waits below; it is gone only if the command was dropped.
            let _ = chosen_tx.send(chosen);
        });
    let chosen = chosen_rx
        .await
        .map_err(|_| AppError::Internal("The save dialog did not answer".into()))?;
    let Some(chosen) = chosen else {
        return Ok(KitSaveOutcome {
            saved: false,
            synced_folder: false,
        });
    };
    let path = chosen
        .into_path()
        .map_err(|_| AppError::Internal("The save dialog gave no file path".into()))?;

    let code = draft_code(&app, &db)?;
    let dir = data_dir(&app)?;
    let synced_folder = tauri::async_runtime::spawn_blocking(move || {
        kit_file::write_kit_file(&path, &dir, kind, &contents, &code)
    })
    .await
    .map_err(|e| AppError::Internal(format!("Saving the Recovery Kit did not finish: {e}")))??;
    log::info!("[SYNC] Saved the Recovery Kit to a file (synced folder: {synced_folder})");
    let storage = if synced_folder {
        KitStorage::Cloud
    } else {
        KitStorage::File
    };
    with_kit_draft(&app, &db, |draft| {
        db.with_conn(|conn| setup::note_kit_storage(conn, draft, storage))
    })?;
    Ok(KitSaveOutcome {
        saved: true,
        synced_folder,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phone_is_refused_before_anything_runs() {
        let error = desktop_only(true).expect_err("phone");
        assert!(matches!(&error, AppError::Validation(key) if key == UNAVAILABLE));
        assert!(desktop_only(false).is_ok());
    }
}
