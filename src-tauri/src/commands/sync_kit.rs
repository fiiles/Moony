//! Saving the Sync Recovery Kit on the desktop (audit M11): the webview renders the file, Rust
//! opens the native save dialog and writes it owner-only (`services::sync::kit_file`: mode 0600 at
//! creation on macOS and Linux; on Windows the chosen folder's permissions), so the webview needs
//! no file system scope for it. Thin: the flag, the copy window of `sync.rs`, the dialog, the
//! service. Phones save through the Files picker (`stage_text_export`): this command refuses there.
//!
//! Every command starts with `require_feature()`. No command logs its arguments; the contents
//! hold the recovery code and are wiped once written.

use std::time::Instant;

use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

use super::sync::{copy_allowed, data_dir, kit_owner, kit_window, require_feature};
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::sync::{KitFileKind, KitSaveOutcome};
use crate::services::sync::{kit_file, setup};

const PASSWORD_REQUIRED: &str = "validation.syncPasswordRequired";
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

/// Within the copy window of this profile session (`copy_allowed`), as copying the code;
/// `validation.syncPasswordRequired` otherwise.
fn require_copy_window(app: &AppHandle, db: &Database) -> Result<()> {
    if copy_allowed(kit_window().as_ref(), &kit_owner(app, db)?, Instant::now()) {
        Ok(())
    } else {
        Err(AppError::Validation(PASSWORD_REQUIRED.into()))
    }
}

/// Saves the Recovery Kit (desktop; `validation.syncUnavailable` on a phone): allowed while
/// `copy_allowed` allows the copy; Rust opens the native save dialog, writes `contents` (at most
/// 256 KiB, holding this kit's recovery code; a page with no script or handler) owner-only and
/// atomically (`kit_file::write_kit_file`: mode 0600 at creation on macOS and Linux, the folder's
/// permissions on Windows), never into the data folder, wipes it, and says whether the folder
/// looks cloud-synced. A closed dialog is `saved: false`. The window is checked again after the
/// dialog: a lock while it was open refuses the write.
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
    require_copy_window(&app, &db)?;
    // The kit wipes its code when dropped.
    let kit = db.with_conn(setup::recovery_kit)?;
    kit_file::check_contents(kind, &contents, &kit.recovery_code)?;

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

    require_copy_window(&app, &db)?;
    let dir = data_dir(&app)?;
    let code = Zeroizing::new(kit.recovery_code.clone());
    drop(kit);
    let synced_folder = tauri::async_runtime::spawn_blocking(move || {
        kit_file::write_kit_file(&path, &dir, kind, &contents, &code)
    })
    .await
    .map_err(|e| AppError::Internal(format!("Saving the Recovery Kit did not finish: {e}")))??;
    log::info!("[SYNC] Saved the Recovery Kit to a file (synced folder: {synced_folder})");
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
