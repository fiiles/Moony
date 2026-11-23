//! The demo profile (ADR 0037): thin commands over `services::demo::profile`. The demo opens with
//! no password, lives in `<app data folder>/demo`, and is offered only while no real profile exists.
//!
//! The lifecycle commands (open, reset, close, discard) and every other end of a session (`logout`,
//! `confirm_setup` ending a demo still open) run one at a time behind [`DemoLifecycle`], so a close
//! can never slip into a reset and leave a demo database open with the flag cleared.

use tauri::{AppHandle, State};

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{DemoStatus, UserProfile};
use crate::services::demo::profile::{self, DEMO_UNAVAILABLE};
use crate::services::local_api::LocalApiServer;
use crate::services::session_key::SessionKey;
use crate::services::{auth, profile_dir};

/// Managed state: held for the whole of a demo lifecycle command and of a session end.
#[derive(Default)]
pub struct DemoLifecycle(pub tauri::async_runtime::Mutex<()>);

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Run blocking profile work (generating the demo takes a second or two) off the async runtime.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| AppError::Internal(format!("The demo profile task failed: {e}")))?
}

/// The follow-ups of an unlock that make sense in the demo: plain attachment files of earlier
/// versions (none) move into the store, and the durable recalculation queue drains. Not the MCP
/// server, not the sync engine, no biometric key check (`auth::after_unlock`).
fn after_open(app: &AppHandle, db: &Database) {
    crate::commands::attachments::spawn_conversion(app);
    crate::commands::mcp_refresh::resume(app.clone(), db.clone());
}

/// Whether a complete demo is stored, when it was last opened, and whether it is open now (what
/// the frontend reads at start, also after a webview reload while the demo is open).
#[tauri::command]
pub async fn demo_status(app: AppHandle) -> Result<DemoStatus> {
    let real = profile_dir::real_dir(&app)?;
    Ok(profile::status(&real, &profile_dir::demo_dir_in(&real)))
}

/// Open the demo, generating it first when none is stored, in `language` (the profile's language
/// switches to it on a stored demo). Refused while a real profile exists or is unlocked; the demo
/// already open answers its profile.
#[tauri::command]
pub async fn demo_open(
    app: AppHandle,
    db: State<'_, Database>,
    lifecycle: State<'_, DemoLifecycle>,
    language: String,
) -> Result<UserProfile> {
    let _one = lifecycle.0.lock().await;
    let real = profile_dir::real_dir(&app)?;
    if profile_dir::demo_open() {
        return auth::get_user_profile(&db)?
            .ok_or_else(|| AppError::Internal("The demo profile has no profile row".into()));
    }
    let worker = (*db).clone();
    let authenticated = auth::is_authenticated();
    let profile =
        blocking(move || profile::open_session(&worker, &real, &language, now(), authenticated))
            .await?;
    auth::set_authenticated(true);
    after_open(&app, &db);
    Ok(profile)
}

/// Throw the open demo away and generate it again in its current language. A failure leaves the
/// demo closed (the frontend returns to the choice screen).
#[tauri::command]
pub async fn demo_reset(
    app: AppHandle,
    db: State<'_, Database>,
    session: State<'_, SessionKey>,
    local_api: State<'_, LocalApiServer>,
    lifecycle: State<'_, DemoLifecycle>,
) -> Result<UserProfile> {
    let _one = lifecycle.0.lock().await;
    if !profile_dir::demo_open() {
        return Err(AppError::Auth(DEMO_UNAVAILABLE.into()));
    }
    let demo_dir = profile_dir::demo_dir(&app)?;
    // Decrypted copies of the old demo's documents go with it.
    crate::commands::attachments::wipe_open_copies(&app);
    let worker = (*db).clone();
    match blocking(move || profile::reset(&worker, &demo_dir, now())).await {
        Ok(profile) => {
            after_open(&app, &db);
            Ok(profile)
        }
        Err(e) => {
            crate::commands::auth::end_session(&app, &db, &session, &local_api).await;
            Err(e)
        }
    }
}

/// Close the demo as a lock closes a profile (`auth::end_session`); the demo stays on disk. Nothing
/// happens while the demo is not the open profile (a real profile is never locked by this).
#[tauri::command]
pub async fn demo_close(
    app: AppHandle,
    db: State<'_, Database>,
    session: State<'_, SessionKey>,
    local_api: State<'_, LocalApiServer>,
    lifecycle: State<'_, DemoLifecycle>,
) -> Result<()> {
    let _one = lifecycle.0.lock().await;
    if profile_dir::demo_open() {
        crate::commands::auth::end_session(&app, &db, &session, &local_api).await;
    }
    Ok(())
}

/// Delete the stored demo ("Začít znovu s původními daty"); an open demo is closed first, so a
/// failed delete still leaves it closed.
#[tauri::command]
pub async fn demo_discard(
    app: AppHandle,
    db: State<'_, Database>,
    session: State<'_, SessionKey>,
    local_api: State<'_, LocalApiServer>,
    lifecycle: State<'_, DemoLifecycle>,
) -> Result<()> {
    let _one = lifecycle.0.lock().await;
    if profile_dir::demo_open() {
        crate::commands::auth::end_session(&app, &db, &session, &local_api).await;
    }
    let real = profile_dir::real_dir(&app)?;
    let demo_dir = profile_dir::demo_dir_in(&real);
    blocking(move || profile::discard(&real, &demo_dir)).await
}

/// Delete a demo folder left next to a real profile (at startup, after the first-run setup and
/// after a restore). Best effort: a failure is logged and the next start tries again.
pub(crate) fn discard_beside_real_profile(app: &AppHandle) {
    let real = match profile_dir::real_dir(app) {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("[DEMO] Leftover demo check skipped: {e}");
            return;
        }
    };
    let demo_dir = profile_dir::demo_dir_in(&real);
    match profile::discard_beside_real_profile(&real, &demo_dir) {
        Ok(true) => log::info!("[DEMO] A real profile exists: the demo profile was deleted"),
        Ok(false) => {}
        Err(e) => log::warn!("[DEMO] The demo profile next to the real one was not deleted: {e}"),
    }
}
