//! Biometric unlock on the phone and the desktop (spec §8.1): thin commands over
//! `services::biometric_unlock`, with the `moony-keychain` plugin behind the service's
//! `KeychainLike` trait. Where the plugin has nothing to offer (Linux) every keychain call answers
//! `Unsupported` and the status reports `supported: false`.
//!
//! The master key never reaches the webview: these commands call the plugin from Rust. The phone's
//! webview permission set allows only the plugin's `status`, `set_privacy_screen` and
//! `set_status_bar_style`; the desktop webview gets none of the plugin's permissions at all.

use std::path::{Path, PathBuf};

use moony_keychain::{KeychainError, KeychainExt};
use tauri::{AppHandle, Manager, State};

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{BiometricStatus, BiometryType, UserProfile};
use crate::services::auth;
use crate::services::biometric_unlock::{self, KeychainFailure, KeychainLike, KeychainStatus};
use crate::services::local_api::LocalApiServer;
use crate::services::session_key::SessionKey;

/// The plugin as the service sees it.
struct PluginKeychain<'a>(&'a moony_keychain::Keychain<tauri::Wry>);

impl KeychainLike for PluginKeychain<'_> {
    fn status(&self) -> std::result::Result<KeychainStatus, KeychainFailure> {
        let status = self.0.status().map_err(failure)?;
        Ok(KeychainStatus {
            available: status.available,
            biometry_type: biometry(status.biometry_type),
        })
    }

    fn store(
        &self,
        key: &str,
        value_hex: &str,
        reason: &str,
    ) -> std::result::Result<(), KeychainFailure> {
        self.0.store(key, value_hex, reason).map_err(failure)
    }

    fn retrieve(
        &self,
        key: &str,
        reason: &str,
    ) -> std::result::Result<Option<String>, KeychainFailure> {
        self.0.retrieve(key, reason).map_err(failure)
    }

    fn remove(&self, key: &str) -> std::result::Result<(), KeychainFailure> {
        self.0.remove(key).map_err(failure)
    }
}

fn failure(error: KeychainError) -> KeychainFailure {
    match error {
        KeychainError::Invalidated => KeychainFailure::Invalidated,
        KeychainError::Cancelled => KeychainFailure::Cancelled,
        KeychainError::Unavailable => KeychainFailure::Unavailable,
        KeychainError::Failed(message) => KeychainFailure::Failed(message),
        KeychainError::Unsupported => KeychainFailure::Unsupported,
    }
}

fn biometry(kind: moony_keychain::BiometryType) -> BiometryType {
    match kind {
        moony_keychain::BiometryType::FaceId => BiometryType::FaceId,
        moony_keychain::BiometryType::TouchId => BiometryType::TouchId,
        moony_keychain::BiometryType::Biometric => BiometryType::Biometric,
        moony_keychain::BiometryType::WindowsHello => BiometryType::WindowsHello,
        moony_keychain::BiometryType::None => BiometryType::None,
    }
}

/// Run `work` with the keychain and the database path on a blocking thread: a keychain call waits
/// for the native side (and for the person, behind a prompt), so it must never run on the main
/// thread or hold up the async runtime. That holds on every platform; Android additionally runs
/// the call on its UI thread.
async fn with_keychain<T: Send + 'static>(
    app: &AppHandle,
    work: impl FnOnce(&AppHandle, &PluginKeychain<'_>, &Path, PathBuf) -> Result<T> + Send + 'static,
) -> Result<T> {
    let db_path = crate::commands::auth::get_db_path(app)?;
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let data_dir = db_path
            .parent()
            .ok_or_else(|| AppError::Internal("Invalid database path".into()))?
            .to_path_buf();
        work(&app, &PluginKeychain(app.keychain()), &data_dir, db_path)
    })
    .await
    .map_err(|e| AppError::Internal(format!("Biometric unlock task failed: {e}")))?
}

/// Whether biometrics can be used now, whether they are switched on, and which biometry the
/// phone has (no prompt; the database may still be locked).
#[tauri::command]
pub async fn biometric_unlock_status(app: AppHandle) -> Result<BiometricStatus> {
    with_keychain(&app, |_, keychain, data_dir, _| {
        Ok(biometric_unlock::biometric_status(keychain, data_dir))
    })
    .await
}

/// Store the session's master key behind biometry. `reason` is the localized text of the
/// biometric prompt both platforms show before they store.
#[tauri::command]
pub async fn enable_biometric_unlock(app: AppHandle, reason: String) -> Result<()> {
    with_keychain(&app, move |app, keychain, data_dir, _| {
        biometric_unlock::enable(keychain, &app.state::<SessionKey>(), data_dir, &reason)
    })
    .await
}

/// Switch biometric unlock off and remove the stored key.
#[tauri::command]
pub async fn disable_biometric_unlock(app: AppHandle) -> Result<()> {
    with_keychain(&app, |_, keychain, data_dir, _| {
        biometric_unlock::disable(keychain, data_dir)
    })
    .await
}

/// Unlock with the key stored behind biometry; `reason` is the localized prompt text. The same
/// follow-ups as the password `unlock` (`auth::after_unlock`).
#[tauri::command]
pub async fn unlock_with_biometrics(
    app: AppHandle,
    local_api: State<'_, LocalApiServer>,
    reason: String,
) -> Result<UserProfile> {
    let profile = with_keychain(&app, move |app, keychain, data_dir, db_path| {
        biometric_unlock::unlock_with_biometrics(keychain, data_dir, &reason, |master_key| {
            auth::unlock_with_master_key(
                &app.state::<Database>(),
                &app.state::<SessionKey>(),
                db_path,
                master_key,
            )
        })
    })
    .await?;

    let db = (*app.state::<Database>()).clone();
    crate::commands::auth::after_unlock(&app, &db, &local_api, &profile).await;
    Ok(profile)
}

/// Switch biometric unlock off after the key it holds stopped being the profile's key (a restored
/// backup carries another one). Best effort, like [`remove_leftover_key`]: a failure is logged.
pub(crate) async fn switch_off(app: &AppHandle) {
    let result = with_keychain(app, |_, keychain, data_dir, _| {
        biometric_unlock::disable(keychain, data_dir)
    })
    .await;
    if let Err(e) = result {
        log::warn!("[BIOMETRIC] Switching biometric unlock off skipped: {e}");
    }
}

/// After a password unlock or an account deletion: remove a stored key the marker does not
/// vouch for (a reinstalled app never offers a stale key). Best effort.
pub(crate) async fn remove_leftover_key(app: &AppHandle) {
    let result = with_keychain(app, |_, keychain, data_dir, _| {
        biometric_unlock::remove_leftover_key(keychain, data_dir);
        Ok(())
    })
    .await;
    if let Err(e) = result {
        log::warn!("[BIOMETRIC] Leftover key check skipped: {e}");
    }
}
