//! Authentication commands

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{InsertUserProfile, UpdateUserProfile, UserProfile};
use crate::services::auth;
use crate::services::local_api::LocalApiServer;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

// ============================================================================
// 2-Phase Setup Commands
// ============================================================================

/// Response for prepare_setup - contains generated keys to show user
#[derive(Serialize)]
pub struct PrepareSetupResponse {
    #[serde(rename = "recoveryKey")]
    pub recovery_key: String,
    #[serde(rename = "masterKeyHex")]
    pub master_key_hex: String,
    pub salt: Vec<u8>,
}

/// Data for confirm_setup
#[derive(Deserialize)]
pub struct ConfirmSetupData {
    pub name: String,
    pub surname: String,
    pub email: String,
    pub password: String,
    #[serde(rename = "masterKeyHex")]
    pub master_key_hex: String,
    #[serde(rename = "recoveryKey")]
    pub recovery_key: String,
    pub salt: Vec<u8>,
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

/// Get the database path for the app
fn get_db_path(app: &AppHandle) -> Result<PathBuf> {
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
pub async fn prepare_setup() -> Result<PrepareSetupResponse> {
    let prepared = auth::prepare_setup()?;
    Ok(PrepareSetupResponse {
        recovery_key: prepared.recovery_key,
        master_key_hex: prepared.master_key_hex,
        salt: prepared.salt,
    })
}

/// Phase 2: Confirm setup - persists all keys and creates account
/// Only called after user confirms they saved the recovery key
#[tauri::command]
pub async fn confirm_setup(
    app: AppHandle,
    db: State<'_, Database>,
    data: ConfirmSetupData,
) -> Result<UserProfile> {
    let db_path = get_db_path(&app)?;

    let profile_data = InsertUserProfile {
        name: data.name,
        surname: data.surname,
        email: data.email,
        menu_preferences: None,
        currency: data.currency,
        language: data.language,
        exclude_personal_real_estate: None,
    };

    let prepared = auth::PreparedSetup {
        master_key_hex: data.master_key_hex,
        recovery_key: data.recovery_key,
        salt: data.salt,
    };

    auth::confirm_setup(&db, db_path, profile_data, &data.password, &prepared)
}

/// Unlock existing account
#[tauri::command]
pub async fn unlock(
    app: AppHandle,
    db: State<'_, Database>,
    local_api: State<'_, LocalApiServer>,
    password: String,
) -> Result<UserProfile> {
    let db_path = get_db_path(&app)?;
    let profile = auth::unlock(&db, db_path, &password)?;

    if profile.mcp_server_enabled && !local_api.is_running() {
        if let Err(e) = local_api.start((*db).clone(), app.clone()).await {
            log::error!("[local-api] MCP server failed to start on unlock: {}", e);
        }
    }
    Ok(profile)
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
    data: ConfirmRecoverData,
) -> Result<UserProfile> {
    let db_path = get_db_path(&app)?;
    auth::confirm_recover(
        &db,
        db_path,
        &data.old_recovery_key,
        &data.new_password,
        &data.new_recovery_key,
    )
}

/// Lock account (logout)
#[tauri::command]
pub async fn logout(db: State<'_, Database>, local_api: State<'_, LocalApiServer>) -> Result<()> {
    local_api.stop().await;
    auth::logout(&db);
    Ok(())
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
pub async fn delete_account(app: AppHandle, db: State<'_, Database>) -> Result<()> {
    let db_path = get_db_path(&app)?;
    auth::delete_account(&db, &db_path)
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
        coingecko_modal_dismissed: None,
        mcp_server_enabled: Some(enabled),
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
