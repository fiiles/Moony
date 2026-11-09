//! Device sync glue (spec §9.4): starts the engine when a sync-enabled profile unlocks and stops it
//! at lock, restore and account deletion; and the sync commands (thin: the flag, the input, the
//! service in `services::sync::{setup, join}`, its `SyncError` mapped with `to_app_error`).
//!
//! Every command but `sync_feature_enabled` starts with `require_feature()`. The relay is the one
//! at the stored account's address (`keys::account`, ruling SH-2); `default_base_url()` only before
//! an account exists. The commands that change what the engine works on (finishing setup, turning
//! sync off, deleting the synced data, the repair and the re-sync) stop the engine first and start
//! it again afterwards when sync is still on, so no loop keeps a relay or credentials from before.
//! No command logs its arguments; passwords stay in `Zeroizing` once received.

use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use moony_keychain::{KeychainError, KeychainExt};
use tauri::{AppHandle, Emitter, Manager, State};
use zeroize::Zeroizing;

use crate::commands::auth::get_db_path;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::sync::{CheckoutTarget, RecoveryKit, SyncAccount, SyncPlan, SyncStatus};
use crate::services::auth;
use crate::services::sync::binding::{BindingFailure, BindingNative};
use crate::services::sync::capture::bench::{self, Overhead};
use crate::services::sync::engine::runtime::{self, EngineHandle, Trigger};
use crate::services::sync::engine::{status, EngineEvents};
use crate::services::sync::error::SyncError;
use crate::services::sync::merge::DATA_CHANGED_EVENT;
use crate::services::sync::relay::http::{default_base_url, HttpRelay};
use crate::services::sync::{feature, join, keys, setup, state};

const UNAVAILABLE: &str = "validation.syncUnavailable";
const DEV_ONLY: &str = "validation.syncDevOnly";
const REPAIR_IN_PROGRESS: &str = "validation.syncRepairInProgress";
const PASSWORD_REQUIRED: &str = "validation.syncPasswordRequired";

/// How long after `sync_show_recovery_kit` checked the password the code may be copied with sync
/// on (during setup it may be copied at any time).
const KIT_COPY_WINDOW: Duration = Duration::from_secs(5 * 60);

/// How long a copied recovery code stays on the clipboard if nothing else was copied since.
const CLIPBOARD_CLEAR_AFTER: Duration = Duration::from_secs(60);

/// The most rows the developer measurement inserts.
const MAX_BENCH_ROWS: u32 = 50_000;

/// The event that carries `SyncStatus` to the UI.
pub const STATUS_CHANGED_EVENT: &str = "sync-status-changed";

/// The running engine of the open profile, if any.
#[derive(Default)]
pub struct SyncEngineState(pub Mutex<Option<EngineHandle>>);

/// The binding native: the keychain plugin seals to this machine (a Secure Enclave key on macOS,
/// DPAPI in machine scope on Windows) and answers `Unsupported` on Linux and the phones.
struct KeychainBinding {
    app: AppHandle,
}

impl BindingNative for KeychainBinding {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
        self.app
            .keychain()
            .bind_seal(secret)
            .map_err(binding_failure)
    }

    fn open(&self, blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
        self.app.keychain().bind_open(blob).map_err(binding_failure)
    }
}

/// `Invalidated` is the plugin's "sealed on another machine"; anything but that and
/// `Unsupported` is a failure of the moment, which lets this start go on. The plugin's messages
/// name OS error codes only, never a value, a path or a blob.
fn binding_failure(error: KeychainError) -> BindingFailure {
    match error {
        KeychainError::Unsupported => BindingFailure::Unsupported,
        KeychainError::Invalidated => BindingFailure::NotThisDevice,
        other => BindingFailure::Failed(other.to_string()),
    }
}

/// What the engine tells the app: the events, the history queue after a merge, the binding native
/// and the profile's folder.
struct TauriEvents {
    app: AppHandle,
    db: Database,
    data_dir: PathBuf,
    native: KeychainBinding,
}

impl EngineEvents for TauriEvents {
    fn data_changed(&self) {
        if let Err(e) = self.app.emit(DATA_CHANGED_EVENT, ()) {
            log::warn!("[SYNC] Could not emit {DATA_CHANGED_EVENT}: {e}");
        }
        // A merge's repairs queue history recalculations (`repair`): run them.
        crate::commands::mcp_refresh::resume(self.app.clone(), self.db.clone());
    }

    fn status_changed(&self, status: &SyncStatus) {
        if let Err(e) = self.app.emit(STATUS_CHANGED_EVENT, status) {
            log::warn!("[SYNC] Could not emit {STATUS_CHANGED_EVENT}: {e}");
        }
    }

    fn binding(&self) -> &dyn BindingNative {
        &self.native
    }

    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }
}

/// Starts the engine for the profile just unlocked. Nothing at all while the flag is off
/// (`feature::enabled`); a profile without sync costs one `SELECT` (G5). A running engine is
/// stopped first. Never fails: a problem is logged and the profile opens without sync.
pub(crate) fn start_engine(app: &AppHandle, db: &Database) {
    if !feature::enabled() {
        return;
    }
    stop_engine(app);
    let session = match db.current_session() {
        Ok(session) => session,
        Err(e) => {
            log::warn!("[SYNC] The engine did not start: {e}");
            return;
        }
    };
    let relay_url = session.with_conn(|conn| {
        if !state::load(conn)?.is_some_and(|sync| sync.enabled) {
            return Ok(None);
        }
        Ok(keys::account(conn)?.map(|(relay_url, _)| relay_url))
    });
    let relay_url = match relay_url {
        Ok(Some(url)) => url,
        Ok(None) => return,
        Err(e) => {
            log::warn!("[SYNC] The engine did not start: {e}");
            return;
        }
    };
    let relay = match HttpRelay::new(&relay_url) {
        Ok(relay) => relay,
        Err(_) => {
            log::warn!("[SYNC] The engine did not start: the stored relay address is not valid");
            return;
        }
    };
    let data_dir = match app.path().app_data_dir() {
        Ok(dir) => dir,
        Err(e) => {
            log::warn!("[SYNC] The engine did not start: no app data dir: {e}");
            return;
        }
    };
    let events = TauriEvents {
        app: app.clone(),
        db: session.clone(),
        data_dir,
        native: KeychainBinding { app: app.clone() },
    };
    // The engine's task goes on Tauri's runtime, whoever calls this.
    let _runtime = tauri::async_runtime::handle().inner().enter();
    let handle = runtime::spawn(relay, session, events);
    *app.state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
    log::info!("[SYNC] The sync engine started");
}

/// Stops the engine, if one runs (lock, restore, account deletion).
pub(crate) fn stop_engine(app: &AppHandle) {
    let handle = app
        .state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(handle) = handle {
        handle.stop();
        log::info!("[SYNC] The sync engine stopped");
    }
}

// ============================================================================
// Helpers of the commands
// ============================================================================

/// `validation.syncUnavailable` while the flag is off.
fn require_feature() -> Result<()> {
    require_feature_with(feature::enabled)
}

fn require_feature_with(enabled: impl Fn() -> bool) -> Result<()> {
    if enabled() {
        Ok(())
    } else {
        Err(AppError::Validation(UNAVAILABLE.into()))
    }
}

/// `validation.syncDevOnly` unless the developer tools are offered (a debug build with sync on).
fn require_dev_tools() -> Result<()> {
    if feature::dev_tools() {
        Ok(())
    } else {
        Err(AppError::Validation(DEV_ONLY.into()))
    }
}

fn app_error(error: SyncError) -> AppError {
    error.to_app_error()
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {e}")))
}

/// The relay of this profile: the stored account's address (ruling SH-2), or the default relay
/// while no account is stored.
fn profile_relay(db: &Database) -> Result<HttpRelay> {
    let stored = db.with_conn(|conn| Ok(keys::account(conn)?.map(|(url, _)| url)))?;
    HttpRelay::new(&stored.unwrap_or_else(default_base_url))
}

fn sync_on(db: &Database) -> Result<bool> {
    db.with_conn(|conn| Ok(state::load(conn)?.is_some_and(|sync| sync.enabled)))
}

/// The status of the open profile, with the running engine's memory of its rounds.
fn current_status(app: &AppHandle, db: &Database) -> Result<SyncStatus> {
    let runtime = app
        .state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        .map(EngineHandle::runtime)
        .unwrap_or_default();
    db.with_conn(|conn| status::status(conn, &runtime))
}

/// Runs `work` with the engine stopped, then starts it again (`start_engine` starts nothing when
/// sync is off by then).
async fn with_engine_stopped<T>(
    app: &AppHandle,
    db: &Database,
    work: impl Future<Output = T>,
) -> T {
    stop_engine(app);
    let result = work.await;
    start_engine(app, db);
    result
}

/// The device password, checked against the profile's key file.
fn verify_password(app: &AppHandle, password: &Zeroizing<String>) -> Result<()> {
    auth::verify_password(&get_db_path(app)?, password)
}

/// One repair or re-sync at a time in the process (`validation.syncRepairInProgress`).
static REPAIR_RUNNING: AtomicBool = AtomicBool::new(false);

/// Holds the one repair slot; dropping it frees the slot.
struct RepairGuard;

impl RepairGuard {
    fn try_start() -> Result<Self> {
        REPAIR_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| RepairGuard)
            .map_err(|_| AppError::Validation(REPAIR_IN_PROGRESS.into()))
    }
}

impl Drop for RepairGuard {
    fn drop(&mut self) {
        REPAIR_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// When `sync_show_recovery_kit` last checked the password.
static KIT_SHOWN_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// The code is copied without a password only during setup (sync not on yet, the kit is being
/// shown the first time); with sync on, within `KIT_COPY_WINDOW` after the password was checked.
fn copy_allowed(sync_on: bool, shown_at: Option<Instant>, now: Instant) -> bool {
    !sync_on
        || shown_at.is_some_and(|shown| now.saturating_duration_since(shown) <= KIT_COPY_WINDOW)
}

fn format_overhead(rows: u32, overhead: &Overhead) -> String {
    let without = overhead.without.as_secs_f64() * 1e3;
    let with = overhead.with.as_secs_f64() * 1e3;
    let pct = (with / without - 1.0) * 100.0;
    format!("{rows} rows: {without:.1} ms without capture, {with:.1} ms with ({pct:.0}% more)")
}

/// Delete Account's attempt to delete the synced data too (spec §11.6, ruling P8), while the
/// profile is still open and sync is on; called before the engine stops. Never blocks the
/// deletion: `delete_vault_best_effort` gives up after `setup::DELETE_BUDGET` and logs why.
pub(crate) async fn delete_vault_with_account(db: &Database) {
    if !feature::enabled() {
        return;
    }
    let Ok(session) = db.current_session() else {
        return;
    };
    if !sync_on(&session).unwrap_or(false) {
        return;
    }
    let relay = match profile_relay(&session) {
        Ok(relay) => relay,
        Err(_) => {
            log::warn!("[SYNC] The synced data was not deleted with the account: no valid relay");
            return;
        }
    };
    if setup::delete_vault_best_effort(&relay, &session).await {
        log::info!("[SYNC] Deleted the synced data with the account");
    }
}

// ============================================================================
// Commands
// ============================================================================

/// Whether sync runs in this build and process (`feature::enabled`).
#[tauri::command]
pub async fn sync_feature_enabled() -> Result<bool> {
    Ok(feature::enabled())
}

#[tauri::command]
pub async fn get_sync_status(app: AppHandle, db: State<'_, Database>) -> Result<SyncStatus> {
    require_feature()?;
    current_status(&app, &db)
}

/// Creates the sync account at `relay_url` (`None` or empty: the default relay), or returns the
/// one already stored at that address.
#[tauri::command]
pub async fn sync_create_account(
    db: State<'_, Database>,
    relay_url: Option<String>,
) -> Result<SyncAccount> {
    require_feature()?;
    let session = db.current_session()?;
    let url = relay_url
        .filter(|url| !url.trim().is_empty())
        .unwrap_or_else(default_base_url);
    let relay = HttpRelay::new(&url)?;
    setup::create_account(&relay, &session, &url)
        .await
        .map_err(app_error)
}

#[tauri::command]
pub async fn sync_get_account(db: State<'_, Database>) -> Result<SyncAccount> {
    require_feature()?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    setup::account_status(&relay, &session)
        .await
        .map_err(app_error)
}

/// Where to pay: a page the frontend opens with the opener plugin, or `selfHosted`.
#[tauri::command]
pub async fn sync_start_checkout(
    db: State<'_, Database>,
    plan: SyncPlan,
) -> Result<CheckoutTarget> {
    require_feature()?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    setup::checkout_url(&relay, &session, plan)
        .await
        .map_err(app_error)
}

/// The kit during setup; with sync on, `validation.syncAlreadyEnabled` (see
/// `sync_show_recovery_kit`).
#[tauri::command]
pub async fn sync_prepare_recovery_kit(db: State<'_, Database>) -> Result<RecoveryKit> {
    require_feature()?;
    db.with_conn(setup::setup_recovery_kit)
}

/// Copies the recovery code natively (hidden from clipboard managers) and clears the clipboard
/// after `CLIPBOARD_CLEAR_AFTER` if nothing else was copied since. The code is read here from
/// `sync_state` and never crosses IPC. `false`: no native copy on this platform (or it failed);
/// the UI copies itself and asks the person to clear the clipboard.
#[tauri::command]
pub async fn sync_copy_recovery_code(app: AppHandle, db: State<'_, Database>) -> Result<bool> {
    require_feature()?;
    let shown_at = *KIT_SHOWN_AT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if !copy_allowed(sync_on(&db)?, shown_at, Instant::now()) {
        return Err(AppError::Validation(PASSWORD_REQUIRED.into()));
    }
    // The kit wipes its code when dropped.
    let kit = db.with_conn(setup::recovery_kit)?;
    let token = match app.keychain().copy_secret_text(&kit.recovery_code) {
        Ok(token) => token,
        Err(KeychainError::Unsupported) => return Ok(false),
        Err(e) => {
            log::warn!("[SYNC] The recovery code could not be copied natively: {e}");
            return Ok(false);
        }
    };
    let clearer = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CLIPBOARD_CLEAR_AFTER).await;
        if let Err(e) = clearer.keychain().clear_clipboard_if_unchanged(token) {
            log::warn!("[SYNC] The clipboard could not be cleared: {e}");
        }
    });
    Ok(true)
}

/// Finishes setup (`setup::finish_setup`) and starts the engine; the status after it.
#[tauri::command]
pub async fn sync_finish_setup(
    app: AppHandle,
    db: State<'_, Database>,
    device_name: String,
    kit_saved: bool,
) -> Result<SyncStatus> {
    require_feature()?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    let finish = setup::finish_setup(
        &relay,
        &session,
        &device_name,
        kit_saved,
        &dir,
        &native,
        now(),
    );
    with_engine_stopped(&app, &db, finish)
        .await
        .map_err(app_error)?;
    current_status(&app, &db)
}

/// A round now, past any back-off. Nothing while no engine runs.
#[tauri::command]
pub async fn sync_now(app: AppHandle) -> Result<()> {
    require_feature()?;
    if let Some(handle) = app
        .state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        handle.wake(Trigger::SyncNow);
    }
    Ok(())
}

/// The kit after setup, once the device password is checked; opens `KIT_COPY_WINDOW` for
/// `sync_copy_recovery_code`.
#[tauri::command]
pub async fn sync_show_recovery_kit(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
) -> Result<RecoveryKit> {
    let password = Zeroizing::new(password);
    require_feature()?;
    verify_password(&app, &password)?;
    let kit = db.with_conn(setup::recovery_kit)?;
    *KIT_SHOWN_AT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Instant::now());
    Ok(kit)
}

/// Turns sync off on this device; local data stays.
#[tauri::command]
pub async fn sync_turn_off(app: AppHandle, db: State<'_, Database>) -> Result<()> {
    require_feature()?;
    let session = db.current_session()?;
    let dir = data_dir(&app)?;
    with_engine_stopped(&app, &db, setup::turn_off_between_rounds(&session, &dir))
        .await
        .map_err(app_error)
}

/// Deletes the vault on the relay and turns sync off, once the device password is checked.
#[tauri::command]
pub async fn sync_delete_synced_data(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
) -> Result<()> {
    let password = Zeroizing::new(password);
    require_feature()?;
    verify_password(&app, &password)?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    with_engine_stopped(&app, &db, setup::delete_synced_data(&relay, &session, &dir))
        .await
        .map_err(app_error)
}

/// Repairs a capture-broken profile (`join::repair_capture`); the backup's path.
#[tauri::command]
pub async fn sync_repair_device(app: AppHandle, db: State<'_, Database>) -> Result<String> {
    require_feature()?;
    let _one = RepairGuard::try_start()?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    let repair = join::repair_capture(&relay, &session, &dir, &native, now());
    let backup = with_engine_stopped(&app, &db, repair)
        .await
        .map_err(app_error)?;
    Ok(backup.display().to_string())
}

/// "Re-sync this device" (ruling T8-4): backup, then join by replace without a drain, once the
/// device password is checked (changes not sent yet survive only in the backup); the backup's
/// path.
#[tauri::command]
pub async fn sync_resync_device(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
) -> Result<String> {
    let password = Zeroizing::new(password);
    require_feature()?;
    let _one = RepairGuard::try_start()?;
    verify_password(&app, &password)?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    let resync = join::resync_by_replace(&relay, &session, &dir, &native, now());
    let backup = with_engine_stopped(&app, &db, resync)
        .await
        .map_err(app_error)?;
    Ok(backup.display().to_string())
}

/// A test subscription of `days` days on a local relay started with `DEV_GRANT=1` (debug builds
/// with the developer tools only).
#[tauri::command]
pub async fn sync_dev_grant(db: State<'_, Database>, days: u32) -> Result<SyncAccount> {
    require_feature()?;
    require_dev_tools()?;
    dev_grant(&db, days).await
}

#[cfg(debug_assertions)]
async fn dev_grant(db: &Database, days: u32) -> Result<SyncAccount> {
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let account = session.with_conn(|conn| {
        keys::account(conn)?
            .map(|(_, account)| account)
            .ok_or_else(|| AppError::Validation("validation.syncSetupIncomplete".into()))
    })?;
    relay
        .dev_grant(&account, days)
        .await
        .map_err(|e| app_error(e.into()))?;
    setup::account_status(&relay, &session)
        .await
        .map_err(app_error)
}

#[cfg(not(debug_assertions))]
async fn dev_grant(_db: &Database, _days: u32) -> Result<SyncAccount> {
    Err(AppError::Validation(DEV_ONLY.into()))
}

/// The capture overhead of `rows` inserts on two scratch profiles (`capture::bench`; developer
/// tools only).
#[tauri::command]
pub async fn sync_debug_capture_overhead(rows: u32) -> Result<String> {
    require_feature()?;
    require_dev_tools()?;
    let rows = rows.clamp(1, MAX_BENCH_ROWS);
    let overhead =
        tauri::async_runtime::spawn_blocking(move || bench::insert_overhead(rows as usize))
            .await
            .map_err(|e| AppError::Internal(format!("The measurement did not finish: {e}")))??;
    Ok(format_overhead(rows, &overhead))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validation_key(result: Result<impl std::fmt::Debug>) -> String {
        match result.expect_err("refused") {
            AppError::Validation(key) => key,
            other => panic!("not a validation key: {other:?}"),
        }
    }

    #[test]
    fn sync_commands_refuse_while_the_feature_is_off() {
        assert_eq!(
            validation_key(require_feature_with(|| false)),
            "validation.syncUnavailable"
        );
        assert!(require_feature_with(|| true).is_ok());
    }

    #[test]
    fn only_one_repair_runs_at_a_time() {
        let first = RepairGuard::try_start().expect("the first repair starts");
        assert_eq!(
            validation_key(RepairGuard::try_start().map(|_| ())),
            "validation.syncRepairInProgress"
        );
        drop(first);
        let again = RepairGuard::try_start().expect("a repair starts once the first ended");
        drop(again);
    }

    #[test]
    fn the_measurement_reads_as_one_line() {
        let overhead = Overhead {
            without: Duration::from_micros(12_340),
            with: Duration::from_micros(14_808),
            queued: 1_001,
        };
        assert_eq!(
            format_overhead(1_000, &overhead),
            "1000 rows: 12.3 ms without capture, 14.8 ms with (20% more)"
        );
    }

    #[test]
    fn the_code_is_copied_during_setup_or_shortly_after_the_password() {
        let now = Instant::now();
        // During setup (sync not on yet) the kit is being shown for the first time.
        assert!(copy_allowed(false, None, now));
        // With sync on, only within the window after `sync_show_recovery_kit` checked the password.
        assert!(!copy_allowed(true, None, now));
        assert!(copy_allowed(true, Some(now), now));
        let edge = now + KIT_COPY_WINDOW;
        assert!(copy_allowed(true, Some(now), edge));
        assert!(!copy_allowed(
            true,
            Some(now),
            edge + Duration::from_secs(1)
        ));
    }

    #[test]
    fn keychain_errors_map_to_binding_failures() {
        assert!(matches!(
            binding_failure(KeychainError::Unsupported),
            BindingFailure::Unsupported
        ));
        assert!(matches!(
            binding_failure(KeychainError::Invalidated),
            BindingFailure::NotThisDevice
        ));
        for passing in [
            KeychainError::Failed("DPAPI could not open the binding: 0x800706BA".into()),
            KeychainError::Cancelled,
            KeychainError::Unavailable,
        ] {
            assert!(matches!(
                binding_failure(passing),
                BindingFailure::Failed(_)
            ));
        }
    }
}
