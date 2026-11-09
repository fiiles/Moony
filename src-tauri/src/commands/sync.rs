//! Device sync glue (spec §9.4): starts the engine when a sync-enabled profile unlocks and stops it
//! at lock, restore and account deletion; and the sync commands (thin: the flag, the input, the
//! service in `services::sync::{setup, join}`, its `SyncError` mapped with `to_app_error`).
//!
//! Every command but `sync_feature_enabled` starts with `require_feature()`. The relay is the one
//! at the stored account's address (`keys::account`, ruling SH-2); `default_base_url()` only before
//! an account exists. The commands that change what the engine works on (finishing setup, turning
//! sync off, deleting the synced data, the repair and the re-sync) stop the engine first and start
//! it again afterwards when sync is still on, so no loop keeps a relay or credentials from before;
//! they run one at a time (`guarded_change`), and no unlock starts the engine meanwhile.
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
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::merge::DATA_CHANGED_EVENT;
use crate::services::sync::relay::http::{default_base_url, HttpRelay};
use crate::services::sync::{feature, join, keys, setup, state};

const UNAVAILABLE: &str = "validation.syncUnavailable";
const DEV_ONLY: &str = "validation.syncDevOnly";
const CHANGE_IN_PROGRESS: &str = "validation.syncChangeInProgress";
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
/// While a sync change runs (`guarded_change`: a repair, turning sync off, …) nothing starts here;
/// the change starts the engine itself when it ends.
pub(crate) fn start_engine(app: &AppHandle, db: &Database) {
    if !engine_may_start(
        feature::enabled(),
        SYNC_CHANGE_RUNNING.load(Ordering::SeqCst),
    ) {
        return;
    }
    spawn_engine(app, db);
}

/// `start_engine` without the sync-change check: what a sync change calls when it ends.
fn spawn_engine(app: &AppHandle, db: &Database) {
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

/// Stopping and starting the engine around a sync change (the app's engine, or a test's).
trait EngineControl {
    fn stop(&self);
    /// Starts the engine again; nothing when sync is off by then.
    fn start(&self);
}

/// The app's engine of the open profile.
struct AppEngine<'a> {
    app: &'a AppHandle,
    db: &'a Database,
}

impl EngineControl for AppEngine<'_> {
    fn stop(&self) {
        stop_engine(self.app);
    }

    fn start(&self) {
        spawn_engine(self.app, self.db);
    }
}

/// A change of what the engine works on (finishing setup, turning sync off, deleting the synced
/// data, the repair, the re-sync): one at a time (`validation.syncChangeInProgress` otherwise),
/// with the engine stopped first and started again afterwards when sync is still on, so no loop
/// keeps a relay or credentials from before. The guard is held until the engine has started again
/// (`start_engine` skips while it is held).
async fn guarded_change<T>(
    engine: &impl EngineControl,
    work: impl Future<Output = SyncResult<T>>,
) -> Result<T> {
    let _one = SyncChangeGuard::try_start()?;
    engine.stop();
    let result = work.await;
    engine.start();
    result.map_err(app_error)
}

/// The device password, checked against the profile's key file off the runtime (the key
/// derivation takes about a second).
async fn verify_password(app: &AppHandle, password: Zeroizing<String>) -> Result<()> {
    let db_path = get_db_path(app)?;
    tauri::async_runtime::spawn_blocking(move || auth::verify_password(&db_path, &password))
        .await
        .map_err(|e| AppError::Internal(format!("The password check did not finish: {e}")))?
}

/// One sync change at a time in the process (`guarded_change`).
static SYNC_CHANGE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Holds the one sync-change slot; dropping it frees the slot.
struct SyncChangeGuard;

impl SyncChangeGuard {
    fn try_start() -> Result<Self> {
        SYNC_CHANGE_RUNNING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| SyncChangeGuard)
            .map_err(|_| AppError::Validation(CHANGE_IN_PROGRESS.into()))
    }
}

impl Drop for SyncChangeGuard {
    fn drop(&mut self) {
        SYNC_CHANGE_RUNNING.store(false, Ordering::SeqCst);
    }
}

/// Whether `start_engine` starts the engine: the flag is on and no sync change runs (the change
/// starts the engine itself when it ends).
fn engine_may_start(flag_on: bool, change_running: bool) -> bool {
    flag_on && !change_running
}

/// The profile session a copy window belongs to: the profile's database file and the session
/// (`Database::session_epoch`, which changes at every unlock and lock).
#[derive(Debug, Clone, PartialEq, Eq)]
struct KitOwner {
    db_path: PathBuf,
    session: u64,
}

/// `sync_show_recovery_kit` checked the password of `owner` at `opened`.
#[derive(Debug, Clone)]
struct KitWindow {
    owner: KitOwner,
    opened: Instant,
}

/// The open copy window, if any. Closed at lock, restore, account deletion, turning sync off and
/// deleting the synced data (`forget_kit_window`).
static KIT_WINDOW: Mutex<Option<KitWindow>> = Mutex::new(None);

fn kit_window_slot() -> std::sync::MutexGuard<'static, Option<KitWindow>> {
    KIT_WINDOW
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn kit_window() -> Option<KitWindow> {
    kit_window_slot().clone()
}

fn open_kit_window(owner: KitOwner, opened: Instant) {
    *kit_window_slot() = Some(KitWindow { owner, opened });
}

/// Closes the copy window (lock, restore, account deletion, sync off, synced data deleted).
pub(crate) fn forget_kit_window() {
    *kit_window_slot() = None;
}

fn kit_owner(app: &AppHandle, db: &Database) -> Result<KitOwner> {
    Ok(KitOwner {
        db_path: get_db_path(app)?,
        session: db.session_epoch(),
    })
}

/// The code is copied without a password only during setup (sync not on yet, the kit is being
/// shown the first time); with sync on, only within `KIT_COPY_WINDOW` after the password of this
/// very profile session was checked.
fn copy_allowed(sync_on: bool, window: Option<&KitWindow>, owner: &KitOwner, now: Instant) -> bool {
    !sync_on
        || window.is_some_and(|window| {
            window.owner == *owner
                && now.saturating_duration_since(window.opened) <= KIT_COPY_WINDOW
        })
}

/// The clipboard clear still owed for the newest recovery code copied natively: its change token
/// (`copy_secret_text`). The 60 s timer, an exit and a lock each take it once, so whichever comes
/// first clears, and only while nothing else was copied since.
#[derive(Debug, Default)]
struct PendingClear(Option<u64>);

impl PendingClear {
    /// A new copy: its token replaces an older one (that copy is no longer on the clipboard).
    fn set(&mut self, token: u64) {
        self.0 = Some(token);
    }

    /// The owed token, once (exit, lock).
    fn take(&mut self) -> Option<u64> {
        self.0.take()
    }

    /// Takes the owed token only if it is still `token` (the timer of that very copy).
    fn take_if(&mut self, token: u64) -> bool {
        let owed = self.0 == Some(token);
        if owed {
            self.0 = None;
        }
        owed
    }
}

static PENDING_CLEAR: Mutex<PendingClear> = Mutex::new(PendingClear(None));

fn pending_clear() -> std::sync::MutexGuard<'static, PendingClear> {
    PENDING_CLEAR
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Empties the clipboard now if it still holds a recovery code copied natively (nothing copied
/// since), instead of waiting for the 60 s timer: at app exit, lock, restore and account deletion.
/// A crash or a force-quit runs nothing, so the code can then stay on the clipboard.
pub(crate) fn clear_copied_code(app: &AppHandle) {
    let Some(token) = pending_clear().take() else {
        return;
    };
    match app.keychain().clear_clipboard_if_unchanged(token) {
        Ok(true) => log::info!("[SYNC] Cleared the copied recovery code from the clipboard"),
        Ok(false) => {}
        Err(e) => log::warn!("[SYNC] The clipboard could not be cleared: {e}"),
    }
}

fn format_overhead(rows: u32, overhead: &Overhead) -> String {
    let without = overhead.without.as_secs_f64() * 1e3;
    let with = overhead.with.as_secs_f64() * 1e3;
    let pct = (with / without - 1.0) * 100.0;
    format!("{rows} rows: {without:.1} ms without capture, {with:.1} ms with ({pct:.0}% more)")
}

/// Delete Account's attempt to delete the synced data too (spec §11.6, ruling P8), while the
/// profile is still open and sync is on; called right after the engine stopped, so no round holds
/// the round lock through the budget. Never blocks the deletion: `delete_vault_best_effort` gives
/// up after `setup::DELETE_BUDGET` and logs why.
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
/// after `CLIPBOARD_CLEAR_AFTER`, or earlier at exit or lock (`clear_copied_code`), if nothing
/// else was copied since. The code is read here from `sync_state` and never crosses IPC. `false`: no native copy on this platform (or it failed);
/// the UI copies itself and asks the person to clear the clipboard.
#[tauri::command]
pub async fn sync_copy_recovery_code(app: AppHandle, db: State<'_, Database>) -> Result<bool> {
    require_feature()?;
    let owner = kit_owner(&app, &db)?;
    let window = kit_window();
    if !copy_allowed(sync_on(&db)?, window.as_ref(), &owner, Instant::now()) {
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
    // Owed until the timer, an exit or a lock clears it (`clear_copied_code`).
    pending_clear().set(token);
    let clearer = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(CLIPBOARD_CLEAR_AFTER).await;
        if !pending_clear().take_if(token) {
            // Cleared at an exit or lock already, or a newer copy owns the clipboard.
            return;
        }
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
    guarded_change(&AppEngine { app: &app, db: &db }, finish).await?;
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
    verify_password(&app, password).await?;
    let kit = db.with_conn(setup::recovery_kit)?;
    open_kit_window(kit_owner(&app, &db)?, Instant::now());
    Ok(kit)
}

/// Turns sync off on this device; local data stays.
#[tauri::command]
pub async fn sync_turn_off(app: AppHandle, db: State<'_, Database>) -> Result<()> {
    require_feature()?;
    let session = db.current_session()?;
    let dir = data_dir(&app)?;
    forget_kit_window();
    guarded_change(
        &AppEngine { app: &app, db: &db },
        setup::turn_off_between_rounds(&session, &dir),
    )
    .await
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
    verify_password(&app, password).await?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    forget_kit_window();
    guarded_change(
        &AppEngine { app: &app, db: &db },
        setup::delete_synced_data(&relay, &session, &dir),
    )
    .await
}

/// Repairs a capture-broken profile (`join::repair_capture`); the backup's path.
#[tauri::command]
pub async fn sync_repair_device(app: AppHandle, db: State<'_, Database>) -> Result<String> {
    require_feature()?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    let repair = join::repair_capture(&relay, &session, &dir, &native, now());
    let backup = guarded_change(&AppEngine { app: &app, db: &db }, repair).await?;
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
    verify_password(&app, password).await?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    let resync = join::resync_by_replace(&relay, &session, &dir, &native, now());
    let backup = guarded_change(&AppEngine { app: &app, db: &db }, resync).await?;
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
    use std::collections::BTreeSet;
    use std::sync::atomic::AtomicU32;

    use super::*;
    use crate::services::sync::engine::tests::TestConn;
    use crate::services::sync::hlc::DeviceId;
    use crate::services::sync::test_support::migrated;

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

    /// The tests that take the process-wide change guard run one at a time.
    static GUARD_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn only_one_sync_change_runs_at_a_time() {
        let _serial = GUARD_TESTS.lock().await;
        let first = SyncChangeGuard::try_start().expect("the first change starts");
        assert_eq!(
            validation_key(SyncChangeGuard::try_start().map(|_| ())),
            "validation.syncChangeInProgress"
        );
        drop(first);
        let again = SyncChangeGuard::try_start().expect("a change starts once the first ended");
        drop(again);
    }

    /// Counts the stops and starts a guarded change asks for.
    #[derive(Default)]
    struct FakeEngine {
        stops: AtomicU32,
        starts: AtomicU32,
    }

    impl EngineControl for FakeEngine {
        fn stop(&self) {
            self.stops.fetch_add(1, Ordering::SeqCst);
        }

        fn start(&self) {
            self.starts.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn sync_is_on(db: &TestConn) -> bool {
        state::load(&db.lock())
            .expect("state")
            .is_some_and(|sync| sync.enabled)
    }

    #[tokio::test]
    async fn turning_sync_off_is_refused_while_another_change_runs() {
        let _serial = GUARD_TESTS.lock().await;
        let db = TestConn::new(migrated());
        state::enable(&db.lock(), DeviceId::random()).expect("sync on");
        let dir = tempfile::tempdir().expect("dir");
        let engine = FakeEngine::default();

        let running = SyncChangeGuard::try_start().expect("a repair runs");
        let refused =
            guarded_change(&engine, setup::turn_off_between_rounds(&db, dir.path())).await;
        assert_eq!(validation_key(refused), "validation.syncChangeInProgress");
        assert!(sync_is_on(&db), "nothing was turned off");
        assert_eq!(
            engine.stops.load(Ordering::SeqCst),
            0,
            "the engine was left alone"
        );
        assert_eq!(engine.starts.load(Ordering::SeqCst), 0);

        drop(running);
        guarded_change(&engine, setup::turn_off_between_rounds(&db, dir.path()))
            .await
            .expect("turned off once the repair ended");
        assert!(!sync_is_on(&db));
        assert_eq!(engine.stops.load(Ordering::SeqCst), 1);
        assert_eq!(engine.starts.load(Ordering::SeqCst), 1);
        assert!(
            SyncChangeGuard::try_start().is_ok(),
            "the guard is free after the change"
        );
    }

    #[test]
    fn the_engine_does_not_start_during_a_sync_change() {
        assert!(engine_may_start(true, false));
        assert!(
            !engine_may_start(true, true),
            "an unlock during a repair starts nothing: the repair restarts the engine itself"
        );
        assert!(!engine_may_start(false, false), "the flag is off");
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

    fn owner(path: &str, session: u64) -> KitOwner {
        KitOwner {
            db_path: PathBuf::from(path),
            session,
        }
    }

    #[test]
    fn the_code_is_copied_during_setup_or_shortly_after_the_password() {
        let now = Instant::now();
        let a = owner("/profiles/a/moony.db", 3);
        let shown = |opened| KitWindow {
            owner: a.clone(),
            opened,
        };
        // During setup (sync not on yet) the kit is being shown for the first time.
        assert!(copy_allowed(false, None, &a, now));
        // With sync on, only within the window after `sync_show_recovery_kit` checked the password.
        assert!(!copy_allowed(true, None, &a, now));
        assert!(copy_allowed(true, Some(&shown(now)), &a, now));
        let edge = now + KIT_COPY_WINDOW;
        assert!(copy_allowed(true, Some(&shown(now)), &a, edge));
        assert!(!copy_allowed(
            true,
            Some(&shown(now)),
            &a,
            edge + Duration::from_secs(1)
        ));
    }

    #[test]
    fn the_copy_window_belongs_to_one_profile_session() {
        let now = Instant::now();
        let a = owner("/profiles/a/moony.db", 3);
        let window = KitWindow {
            owner: a.clone(),
            opened: now,
        };
        assert!(copy_allowed(true, Some(&window), &a, now));
        assert!(
            !copy_allowed(true, Some(&window), &owner("/profiles/b/moony.db", 3), now),
            "another profile"
        );
        assert!(
            !copy_allowed(true, Some(&window), &owner("/profiles/a/moony.db", 5), now),
            "the same profile after a lock and an unlock"
        );
    }

    #[test]
    fn a_pending_clear_is_taken_once_and_a_newer_copy_replaces_it() {
        let mut pending = PendingClear::default();
        assert_eq!(pending.take(), None, "nothing copied");

        pending.set(7);
        assert_eq!(pending.take(), Some(7), "exit or lock clears it");
        assert_eq!(pending.take(), None, "once");

        // A newer copy replaces the older token: the older copy's timer takes nothing, the newer
        // one's takes its own, and exit then has nothing left.
        pending.set(7);
        pending.set(9);
        assert!(!pending.take_if(7), "the older copy's timer");
        assert!(pending.take_if(9), "the newer copy's timer");
        assert_eq!(pending.take(), None);

        // The timer of a copy already cleared at exit or lock finds nothing.
        pending.set(11);
        assert_eq!(pending.take(), Some(11));
        assert!(!pending.take_if(11));
    }

    #[test]
    fn a_lock_closes_the_copy_window() {
        let a = owner("/profiles/a/moony.db", 3);
        open_kit_window(a.clone(), Instant::now());
        assert!(kit_window().is_some_and(|w| w.owner == a));
        forget_kit_window();
        assert!(kit_window().is_none());

        // The session the window is keyed to changes at every lock, too.
        let db = Database::new();
        let before = db.session_epoch();
        db.close();
        assert_ne!(db.session_epoch(), before);
    }

    /// The commands of this file (name → body up to its third statement), read from the source.
    fn commands_and_their_first_statements() -> Vec<(String, String)> {
        let source = include_str!("sync.rs");
        // Only the code above the tests: the tests' own text names the attribute too.
        let code = source
            .split("\n#[cfg(test)]\nmod tests {")
            .next()
            .expect("code");
        let mut commands = Vec::new();
        let mut lines = code.lines();
        while let Some(line) = lines.next() {
            if line.trim() != "#[tauri::command]" {
                continue;
            }
            let rest: String = lines.clone().collect::<Vec<_>>().join("\n");
            let signature = rest
                .trim_start()
                .strip_prefix("pub async fn ")
                .unwrap_or_else(|| panic!("a command is a `pub async fn`: {rest:.60}"));
            let name: String = signature
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let body_start = signature.find(") -> ").expect("a return type");
            let body = &signature[body_start..];
            let body = &body[body.find('{').expect("a body") + 1..];
            let first_three: String = body.splitn(4, ';').take(3).collect::<Vec<_>>().join(";");
            commands.push((name, first_three));
        }
        commands
    }

    #[test]
    fn every_sync_command_checks_the_flag_first() {
        let commands = commands_and_their_first_statements();
        let found: BTreeSet<&str> = commands.iter().map(|(name, _)| name.as_str()).collect();
        // Every command registered in `lib.rs` is one this scan found, and the other way round.
        let registered: BTreeSet<&str> = include_str!("../lib.rs")
            .lines()
            .filter_map(|line| line.trim().strip_prefix("commands::sync::"))
            .map(|name| name.trim_end_matches(','))
            .collect();
        assert_eq!(found, registered);
        assert_eq!(found.len(), 16);
        for (name, first_three) in &commands {
            if name == "sync_feature_enabled" {
                continue;
            }
            assert!(
                first_three.contains("require_feature()?"),
                "`{name}` must call `require_feature()?` within its first three statements"
            );
        }
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
