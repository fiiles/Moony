//! Authentication commands

use crate::commands::demo::DemoLifecycle;
use crate::commands::sync::{password_given, verify_password};
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{InsertUserProfile, UpdateUserProfile, UserProfile};
use crate::services::auth::{self, PendingSetup};
use crate::services::local_api::LocalApiServer;
use crate::services::profile_dir;
use crate::services::session_key::SessionKey;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};
use zeroize::Zeroizing;

// ============================================================================
// 2-Phase Setup Commands
// ============================================================================

/// Response for prepare_setup - the recovery key to show the user. The master key and the salt
/// stay in Rust (`auth::PendingSetup`) until confirm_setup.
#[derive(Serialize)]
pub struct PrepareSetupResponse {
    #[serde(rename = "recoveryKey")]
    pub recovery_key: String,
}

/// Data for confirm_setup
#[derive(Deserialize)]
pub struct ConfirmSetupData {
    pub name: String,
    pub surname: String,
    pub email: String,
    pub password: String,
    pub language: Option<String>,
    /// Display currency chosen in the first-run wizard (defaults to CZK in the service)
    #[serde(default)]
    pub currency: Option<String>,
}

// ============================================================================
// 2-Phase Change Password Commands
// ============================================================================

/// Data for prepare_change_password
#[derive(Deserialize)]
pub struct PrepareChangePasswordData {
    #[serde(rename = "currentPassword")]
    pub current_password: String,
}

/// Response for prepare_change_password
#[derive(Serialize)]
pub struct PrepareChangePasswordResponse {
    #[serde(rename = "recoveryKey")]
    pub recovery_key: String,
}

/// Data for confirm_change_password
#[derive(Deserialize)]
pub struct ConfirmChangePasswordData {
    #[serde(rename = "currentPassword")]
    pub current_password: String,
    #[serde(rename = "newPassword")]
    pub new_password: String,
    #[serde(rename = "recoveryKey")]
    pub recovery_key: String,
}

// ============================================================================
// 2-Phase Recovery Commands
// ============================================================================

/// Data for prepare_recover
#[derive(Deserialize)]
pub struct PrepareRecoverData {
    #[serde(rename = "recoveryKey")]
    pub recovery_key: String,
    #[serde(rename = "newPassword")]
    pub new_password: String,
}

/// Response for prepare_recover
#[derive(Serialize)]
pub struct PrepareRecoverResponse {
    #[serde(rename = "recoveryKey")]
    pub new_recovery_key: String,
}

/// Data for confirm_recover
#[derive(Deserialize)]
pub struct ConfirmRecoverData {
    #[serde(rename = "oldRecoveryKey")]
    pub old_recovery_key: String,
    #[serde(rename = "newPassword")]
    pub new_password: String,
    #[serde(rename = "newRecoveryKey")]
    pub new_recovery_key: String,
}

/// Build the database path from the (fallible) app data dir lookup.
///
/// A failing lookup must surface as an `AppError` instead of panicking the
/// whole command.
fn db_path_from_app_dir<E: std::fmt::Display>(
    app_dir: std::result::Result<PathBuf, E>,
) -> Result<PathBuf> {
    app_dir
        .map(|dir| dir.join("moony.db"))
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {e}")))
}

/// The real profile's database path (`<app data folder>/moony.db`): the key files, unlock, setup,
/// recovery, the password, biometric unlock and sync all belong to the real profile, never to the
/// demo (ADR 0037).
pub(crate) fn get_db_path(app: &AppHandle) -> Result<PathBuf> {
    db_path_from_app_dir(app.path().app_data_dir())
}

/// Check if database exists (user has set up account)
#[tauri::command]
pub async fn check_setup(app: AppHandle) -> Result<bool> {
    let db_path = get_db_path(&app)?;
    Ok(auth::database_exists(&db_path))
}

/// Phase 1: Prepare setup - generates keys but doesn't persist
/// Returns recovery key so user can save it before we create the account
#[tauri::command]
pub async fn prepare_setup(pending: State<'_, PendingSetup>) -> Result<PrepareSetupResponse> {
    Ok(PrepareSetupResponse {
        recovery_key: pending.prepare()?,
    })
}

/// Phase 2: Confirm setup - persists all keys and creates account
/// Only called after user confirms they saved the recovery key
#[tauri::command]
pub async fn confirm_setup(
    app: AppHandle,
    db: State<'_, Database>,
    session: State<'_, SessionKey>,
    pending: State<'_, PendingSetup>,
    local_api: State<'_, LocalApiServer>,
    lifecycle: State<'_, DemoLifecycle>,
    data: ConfirmSetupData,
) -> Result<UserProfile> {
    let db_path = get_db_path(&app)?;
    // The wizard closes the demo before it starts; should it still be open, it ends here, before
    // the new profile takes the connection (one demo lifecycle step at a time).
    {
        let _one = lifecycle.0.lock().await;
        if profile_dir::demo_open() {
            end_session(&app, &db, &session, &local_api).await;
        }
    }

    let profile_data = InsertUserProfile {
        name: data.name,
        surname: data.surname,
        email: data.email,
        menu_preferences: None,
        currency: data.currency,
        language: data.language,
        exclude_personal_real_estate: None,
    };

    let profile = pending.confirm(|prepared| {
        auth::confirm_setup(
            &db,
            &session,
            db_path,
            profile_data,
            &data.password,
            prepared,
        )
    })?;
    // A real profile exists now (also on the wizard's sync-join path, which creates it here and
    // joins afterwards): the demo profile goes.
    crate::commands::demo::discard_beside_real_profile(&app);
    // The folder was (re)created for the new profile: keep it out of device backups (phone only).
    #[cfg(mobile)]
    crate::commands::device_backup::exclude_data_dir(&app).await;
    Ok(profile)
}

/// Unlock existing account
#[tauri::command]
pub async fn unlock(
    app: AppHandle,
    db: State<'_, Database>,
    session: State<'_, SessionKey>,
    local_api: State<'_, LocalApiServer>,
    password: String,
) -> Result<UserProfile> {
    let db_path = get_db_path(&app)?;
    let profile = auth::unlock(&db, &session, db_path, &password)?;
    // A key left in the keychain without the "on" marker (a reinstall) goes now.
    crate::commands::biometric::remove_leftover_key(&app).await;
    after_unlock(&app, &db, &local_api, &profile).await;
    Ok(profile)
}

/// What follows every unlock that opens the database, the password and the biometric one alike:
/// the MCP server starts if the profile has it on, old plain attachment files move into the
/// encrypted store, the MCP refresh resumes, and the sync engine starts (a profile with sync on,
/// while the flag allows it). `confirm_recover` does not share it: it never
/// started the MCP server, and this change leaves its behaviour alone.
pub(crate) async fn after_unlock(
    app: &AppHandle,
    db: &Database,
    local_api: &LocalApiServer,
    profile: &UserProfile,
) {
    // The MCP server is a desktop feature; a phone never opens a local port.
    #[cfg(desktop)]
    if profile.mcp_server_enabled && !local_api.is_running() {
        if let Err(e) = local_api.start(db.clone(), app.clone()).await {
            log::error!("[local-api] MCP server failed to start on unlock: {}", e);
        }
    }
    #[cfg(mobile)]
    let _ = (local_api, profile);
    // Plain attachment files from earlier versions move into the encrypted store (D4).
    crate::commands::attachments::spawn_conversion(app);
    crate::commands::mcp_refresh::resume(app.clone(), db.clone());
    crate::commands::sync::start_engine(app, db);
}

/// Phase 1: Prepare recovery - verifies recovery key, generates new one
/// Returns new recovery key so user can save it before we make changes
#[tauri::command]
pub async fn prepare_recover(
    app: AppHandle,
    data: PrepareRecoverData,
) -> Result<PrepareRecoverResponse> {
    let db_path = get_db_path(&app)?;
    let new_recovery_key = auth::prepare_recover(&db_path, &data.recovery_key, &data.new_password)?;
    Ok(PrepareRecoverResponse { new_recovery_key })
}

/// Phase 2: Confirm recovery - persists new password and recovery key
/// Only called after user confirms they saved the new recovery key
#[tauri::command]
pub async fn confirm_recover(
    app: AppHandle,
    db: State<'_, Database>,
    session: State<'_, SessionKey>,
    data: ConfirmRecoverData,
) -> Result<UserProfile> {
    let db_path = get_db_path(&app)?;
    let profile = auth::confirm_recover(
        &db,
        &session,
        db_path,
        &data.old_recovery_key,
        &data.new_password,
        &data.new_recovery_key,
    )?;
    // Plain attachment files from earlier versions move into the encrypted store (D4).
    crate::commands::attachments::spawn_conversion(&app);
    crate::commands::mcp_refresh::resume(app.clone(), (*db).clone());
    crate::commands::sync::start_engine(&app, &db);
    Ok(profile)
}

/// Lock account (logout)
#[tauri::command]
pub async fn logout(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    session: State<'_, SessionKey>,
    local_api: State<'_, LocalApiServer>,
    lifecycle: State<'_, DemoLifecycle>,
) -> Result<()> {
    // Never in the middle of a demo open or reset (uncontended for a real profile).
    let _one = lifecycle.0.lock().await;
    end_session(&app, &db, &session, &local_api).await;
    Ok(())
}

/// Everything a lock does: the MCP server and the sync engine stop, the sync flows in progress
/// are forgotten, the database closes with the session key, and the decrypted copies go. The demo
/// profile's close is the same (`commands::demo`), so it also clears the demo flag: a lock in the
/// demo ends it rather than leaving the app pointed at the demo folder with nothing open, and takes
/// what the demo left in the process with it (`demo::profile::clear_session_state`: its exchange
/// rates, the categorization engine's layers, queued history work). Callers hold [`DemoLifecycle`].
pub(crate) async fn end_session(
    app: &AppHandle,
    db: &Database,
    session: &SessionKey,
    local_api: &LocalApiServer,
) {
    local_api.stop().await;
    crate::commands::sync::stop_engine_and_wait(app).await;
    crate::commands::sync::forget_kit_window();
    crate::commands::sync_pairing::forget_pairing();
    crate::commands::sync::clear_copied_code(app);
    auth::logout(db, session);
    crate::services::demo::profile::clear_session_state(
        &app.state::<crate::commands::categorization::CategorizationState>()
            .0,
        &app.state::<crate::services::history_recalc::HistoryRecalc>(),
    );
    // Decrypted copies opened in other apps go with the session (D3).
    crate::commands::attachments::wipe_open_copies(app);
    // So do WebKit's plaintext copies of the files picked in the phone's upload sheets.
    #[cfg(mobile)]
    match app.path().temp_dir() {
        Ok(temp_dir) => {
            let removed = crate::services::upload_copies::sweep(&temp_dir);
            log::info!("[UPLOAD] Upload copy sweep at lock removed {removed} file(s)");
        }
        Err(e) => log::warn!("[UPLOAD] Upload copy sweep at lock skipped: no temp dir: {e}"),
    }
    // Android: the WebView's camera captures, plain JPEGs in the app's external Pictures folder.
    #[cfg(target_os = "android")]
    match app.path().picture_dir() {
        Ok(pictures) => {
            let removed = crate::services::upload_copies::sweep_camera_captures(&pictures);
            log::info!("[UPLOAD] Camera capture sweep at lock removed {removed} file(s)");
        }
        Err(e) => log::warn!("[UPLOAD] Camera capture sweep at lock skipped: {e}"),
    }
}

/// Check if currently authenticated
#[tauri::command]
pub async fn is_authenticated() -> Result<bool> {
    Ok(auth::is_authenticated())
}

/// Get current user profile
#[tauri::command]
pub async fn get_user_profile(db: State<'_, Database>) -> Result<Option<UserProfile>> {
    auth::get_user_profile(&db)
}

/// Update user profile
#[tauri::command]
pub async fn update_user_profile(
    db: State<'_, Database>,
    updates: UpdateUserProfile,
) -> Result<UserProfile> {
    auth::update_user_profile(&db, updates)
}

/// Phase 1: Prepare password change - verifies current password, generates new recovery key
/// Returns new recovery key so user can save it before we make changes
#[tauri::command]
pub async fn prepare_change_password(
    app: AppHandle,
    data: PrepareChangePasswordData,
) -> Result<PrepareChangePasswordResponse> {
    let db_path = get_db_path(&app)?;
    let new_recovery_key = auth::prepare_change_password(&db_path, &data.current_password)?;
    Ok(PrepareChangePasswordResponse {
        recovery_key: new_recovery_key,
    })
}

/// Phase 2: Confirm password change - persists new password and recovery key
/// Only called after user confirms they saved the new recovery key
#[tauri::command]
pub async fn confirm_change_password(
    app: AppHandle,
    data: ConfirmChangePasswordData,
) -> Result<()> {
    let db_path = get_db_path(&app)?;
    auth::confirm_change_password(
        &db_path,
        &data.current_password,
        &data.new_password,
        &data.recovery_key,
    )
}

/// Delete account
#[tauri::command]
pub async fn delete_account(
    app: AppHandle,
    db: State<'_, Database>,
    session: State<'_, SessionKey>,
    delete_synced_data: Option<bool>,
    password: Option<String>,
) -> Result<()> {
    // The demo has no account to delete (its own "Obnovit"/"Ukončit" handle it).
    profile_dir::ensure_not_demo()?;
    let delete_synced_data = delete_synced_data.unwrap_or(false);
    if delete_synced_data {
        // Deleting the synced data deletes it for every device: the device password first, as a
        // removal asks for it, before anything stops or is sent (owner decision O-1, As built R-113).
        let password = Zeroizing::new(password.unwrap_or_default());
        password_given(&password)?;
        verify_password(&app, password).await?;
    }
    let db_path = get_db_path(&app)?;
    // The engine stops first, so no round holds the round lock through the vault deletion's
    // budget. Deleting the profile only removes this device: the synced data on the server stays
    // for the other devices unless the owner ticked the opt-in box, and then it goes while the
    // profile is open and its sync row still on (ruling P8). A failure never stops the deletion.
    crate::commands::sync::stop_engine_and_wait(&app).await;
    crate::commands::sync::forget_kit_window();
    crate::commands::sync_pairing::forget_pairing();
    crate::commands::sync::clear_copied_code(&app);
    if delete_synced_data {
        crate::commands::sync::delete_vault_with_account(&db).await;
    } else {
        // The vault stays for the other devices, which learn that this one left.
        crate::commands::sync::say_left(&app, &db).await;
    }
    crate::commands::attachments::wipe_open_copies(&app);
    let deleted = auth::delete_account(&db, &session, &db_path);
    // The marker went with the account files, so the key stored for biometric unlock goes too.
    crate::commands::biometric::remove_leftover_key(&app).await;
    deleted
}

// ============================================================================
// MCP Server commands
// ============================================================================

/// Enable or disable the local MCP HTTP server
#[tauri::command]
pub async fn set_mcp_server_enabled(
    app: AppHandle,
    db: State<'_, Database>,
    local_api: State<'_, LocalApiServer>,
    enabled: bool,
) -> Result<UserProfile> {
    if enabled {
        // The demo's synthetic data never leaves the device through the MCP server.
        profile_dir::ensure_not_demo()?;
    }
    if enabled && !local_api.is_running() {
        local_api.start((*db).clone(), app.clone()).await?;
    } else if !enabled && local_api.is_running() {
        local_api.stop().await;
    }
    let updates = UpdateUserProfile {
        name: None,
        surname: None,
        email: None,
        menu_preferences: None,
        currency: None,
        language: None,
        exclude_personal_real_estate: None,
        mcp_server_enabled: Some(enabled),
        country: None,
        insurance_notice_days: None,
        theme: None,
    };
    auth::update_user_profile(&db, updates)
}

#[derive(Serialize, specta::Type)]
pub struct McpServerStatus {
    pub running: bool,
    pub port: u16,
    pub url: String,
    #[serde(rename = "tokenSet")]
    pub token_set: bool,
    #[serde(rename = "lastError")]
    pub last_error: Option<String>,
}

/// Get current MCP server status (running, configured port/url, token presence, last error)
#[tauri::command]
pub async fn get_mcp_server_status(
    db: State<'_, Database>,
    local_api: State<'_, LocalApiServer>,
) -> Result<McpServerStatus> {
    use crate::services::mcp::config;
    let (port, token_set) = if db.is_open() {
        db.with_conn(|conn| Ok((config::get_port(conn)?, config::get_token(conn)?.is_some())))?
    } else {
        (config::DEFAULT_PORT, false)
    };
    Ok(McpServerStatus {
        running: local_api.is_running(),
        port,
        url: format!("http://127.0.0.1:{}/mcp", port),
        token_set,
        last_error: local_api.get_last_error(),
    })
}

/// Read the stable MCP bearer token for display in Settings (None until first enable)
#[tauri::command]
pub async fn get_mcp_server_token(db: State<'_, Database>) -> Result<Option<String>> {
    db.with_conn(crate::services::mcp::config::get_token)
}

/// Change the MCP port (1024–65535); restarts the server if it is running
#[tauri::command]
pub async fn set_mcp_server_port(
    app: AppHandle,
    db: State<'_, Database>,
    local_api: State<'_, LocalApiServer>,
    port: u16,
) -> Result<()> {
    db.with_conn(|conn| crate::services::mcp::config::set_port(conn, port))?;
    if local_api.is_running() {
        local_api.stop().await;
        local_api.start((*db).clone(), app.clone()).await?;
    }
    Ok(())
}

/// Replace the bearer token, invalidating existing client configs; restarts the server if running
#[tauri::command]
pub async fn regenerate_mcp_token(
    app: AppHandle,
    db: State<'_, Database>,
    local_api: State<'_, LocalApiServer>,
) -> Result<String> {
    let token = db.with_conn(crate::services::mcp::config::regenerate_token)?;
    if local_api.is_running() {
        local_api.stop().await;
        local_api.start((*db).clone(), app.clone()).await?;
    }
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_path_is_moony_db_inside_app_data_dir() {
        let path =
            db_path_from_app_dir(Ok::<_, String>(PathBuf::from("/data/moony-app"))).expect("path");
        assert_eq!(path, PathBuf::from("/data/moony-app/moony.db"));
    }

    #[test]
    fn failing_app_data_dir_lookup_becomes_internal_error_not_a_panic() {
        let err = db_path_from_app_dir(Err("no home dir".to_string())).unwrap_err();
        match err {
            AppError::Internal(msg) => {
                assert_eq!(msg, "Failed to get app data dir: no home dir")
            }
            other => panic!("expected AppError::Internal, got {other:?}"),
        }
    }
}
