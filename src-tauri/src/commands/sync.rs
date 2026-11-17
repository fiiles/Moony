//! Device sync glue (spec §9.4): starts the engine when a sync-enabled profile unlocks and stops it
//! at lock, restore and account deletion; and the sync commands (thin: the flag, the input, the
//! service in `services::sync::{setup, join}`, its `SyncError` mapped with `to_app_error`).
//!
//! Every command but three starts with `require_feature()`: `sync_feature_enabled` (it is the flag),
//! `sync_profile_state` (read-only: whether this profile has sync on, so a build without the flag
//! can say so) and `sync_turn_off` (it only removes sync state from this profile; a build without
//! the flag keeps capturing an enabled profile, audit M12, and must let the person end that).
//! The relay is the one
//! at the stored account's address (`keys::account`, ruling SH-2); `default_base_url()` only before
//! an account exists. The commands that change what the engine works on (finishing setup, turning
//! sync off, deleting the synced data, the repair and the re-sync) stop the engine first and start
//! it again afterwards when sync is still on, so no loop keeps a relay or credentials from before;
//! they run one at a time (`guarded_change`), and no unlock starts the engine meanwhile.
//! No command logs its arguments; passwords stay in `Zeroizing` once received.

use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use moony_keychain::{KeychainError, KeychainExt};
use tauri::{AppHandle, Emitter, Manager, State};
use zeroize::Zeroizing;

use crate::commands::auth::get_db_path;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::sync::{
    CheckoutTarget, RecoveryKit, ReplaceConsentView, ReplaceOutcomeView, SyncAccount, SyncPlan,
    SyncProfileState, SyncServerCheck, SyncStatus,
};
use crate::services::auth;
use crate::services::sync::binding::{BindingFailure, BindingNative};
use crate::services::sync::capture::bench::{self, Overhead};
use crate::services::sync::connection_code::{parse_connection_code, server_address_from_input};
use crate::services::sync::engine::runtime::{self, EngineHandle, Trigger};
use crate::services::sync::engine::{status, ConnAccess, EngineEvents};
use crate::services::sync::error::{SyncError, SyncResult, ACCOUNT_BLOCKED};
use crate::services::sync::merge::DATA_CHANGED_EVENT;
use crate::services::sync::rekey::kit::KitStorage;
use crate::services::sync::relay::http::{default_base_url, is_default_relay, HttpRelay};
use crate::services::sync::{feature, join, keys, server, setup, state};

use super::sync_backups::{consent_of, outcome_view};

const UNAVAILABLE: &str = "validation.syncUnavailable";
const DEV_ONLY: &str = "validation.syncDevOnly";
pub(super) const CHANGE_IN_PROGRESS: &str = "validation.syncChangeInProgress";
const PASSWORD_REQUIRED: &str = "validation.syncPasswordRequired";
const PASSWORD_NEEDED: &str = "validation.syncPasswordNeeded";
const COPY_FAILED: &str = "validation.syncCopyFailed";
const KIT_NOT_CONFIRMED: &str = "validation.syncRecoveryKitNotConfirmed";

/// How long after `sync_prepare_recovery_kit` checked the password the Kit draft lives: the code may
/// be copied, the Kit printed or saved, and setup finished with it (audit M11: every showing needs
/// the password; revision §2.2: the draft lives at most ten minutes).
const KIT_COPY_WINDOW: Duration = Duration::from_secs(10 * 60);

/// How long a copied recovery code stays on the clipboard if nothing else was copied since.
const CLIPBOARD_CLEAR_AFTER: Duration = Duration::from_secs(60);

/// The most rows the developer measurement inserts.
const MAX_BENCH_ROWS: u32 = 50_000;

/// The event that carries `SyncStatus` to the UI.
pub const STATUS_CHANGED_EVENT: &str = "sync-status-changed";

/// The event that tells the UI documents' or photos' content or counts changed (no payload).
pub const SYNC_ATTACHMENTS_EVENT: &str = "sync-attachments-changed";

/// How long a sync change, a lock or a restore waits for the stopped engine's tasks to end before
/// it goes on anyway (a task can only be slow to end inside one database hold).
const ENGINE_STOP_WAIT: Duration = Duration::from_secs(5);

/// The running engine of the open profile, if any.
#[derive(Default)]
pub struct SyncEngineState(pub Mutex<Option<EngineHandle>>);

/// The binding native: the keychain plugin seals to this machine (a Secure Enclave key on macOS
/// and on the iPhone, the Xcode 27 simulator included, which has one; DPAPI in machine scope on
/// Windows) and answers `Unsupported` on Linux, Android and without a Secure Enclave.
pub(super) struct KeychainBinding {
    pub(super) app: AppHandle,
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
/// `Unsupported` is a failure of the moment (`Failed`), which stops this start's rounds and
/// counts once per app start; the third such start in a row re-joins (audit M10,
/// `binding::startup_check_counted`). The plugin's messages name OS error codes only, never a
/// value, a path or a blob.
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

    fn attachments_changed(&self) {
        if let Err(e) = self.app.emit(SYNC_ATTACHMENTS_EVENT, ()) {
            log::warn!("[SYNC] Could not emit {SYNC_ATTACHMENTS_EVENT}: {e}");
        }
    }

    fn binding(&self) -> &dyn BindingNative {
        &self.native
    }

    fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    fn binding_failed_this_start(&self) -> &AtomicBool {
        &BINDING_FAILED_THIS_START
    }
}

/// A binding check failed in this process (`EngineEvents::binding_failed_this_start`): the app
/// start's mark, shared by every engine an unlock or a sync change spawns.
static BINDING_FAILED_THIS_START: AtomicBool = AtomicBool::new(false);

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

/// Stops the engine, if one runs, without waiting for its tasks to end (an unlock starting the
/// next one: the new engine's lanes wait for the old ones on the profile's lane gate).
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

/// Stops the engine, if one runs, and returns once its loop and its transfer lanes have ended
/// (at most `ENGINE_STOP_WAIT`): lock, restore, account deletion and every sync change, so no
/// transfer of it still uses the profile while the session closes or its rows are replaced.
pub(crate) async fn stop_engine_and_wait(app: &AppHandle) {
    let handle = app
        .state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take();
    if let Some(handle) = handle {
        if tokio::time::timeout(ENGINE_STOP_WAIT, handle.stopped())
            .await
            .is_err()
        {
            log::warn!("[SYNC] The sync engine was slow to stop; going on");
        }
        log::info!("[SYNC] The sync engine stopped");
    }
}

// ============================================================================
// Helpers of the commands
// ============================================================================

/// `validation.syncUnavailable` while the flag is off.
pub(super) fn require_feature() -> Result<()> {
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

pub(super) fn app_error(error: SyncError) -> AppError {
    error.to_app_error()
}

/// `app_error` for a change against the server at `relay_url`: the Moony cloud takes payment, any
/// other server (the person's own) does not, so a blocked account never reads "subscription" copy
/// there. Only `Lapsed` is remapped; every other error keeps `to_app_error` (an unknown account is
/// `syncAccountNotFound`: no connection code is involved in these changes).
pub(super) fn app_error_for(relay_url: &str, error: SyncError) -> AppError {
    match error {
        SyncError::Lapsed if !is_default_relay(relay_url) => {
            AppError::Validation(ACCOUNT_BLOCKED.into())
        }
        other => other.to_app_error(),
    }
}

/// `work`, its error already mapped for the server at `relay_url` (`app_error_for`); the mapped
/// error passes `guarded_change` unchanged (`SyncError::Local`).
pub(super) async fn mapped_for_server<T>(
    relay_url: String,
    work: impl Future<Output = SyncResult<T>>,
) -> SyncResult<T> {
    work.await
        .map_err(|error| SyncError::Local(app_error_for(&relay_url, error)))
}

/// Setup's mapping: as `mapped_for_server`, and an account the own server does not know means the
/// connection code just entered was revoked (`to_own_server_app_error`).
async fn mapped_for_setup<T>(
    relay_url: String,
    work: impl Future<Output = SyncResult<T>>,
) -> SyncResult<T> {
    work.await.map_err(|error| {
        SyncError::Local(if is_default_relay(&relay_url) {
            error.to_app_error()
        } else {
            error.to_own_server_app_error()
        })
    })
}

/// The server address this profile syncs against (the Moony cloud while there is no account).
pub(super) fn stored_relay_url(db: &Database) -> Result<String> {
    let stored = db.with_conn(|conn| Ok(keys::account(conn)?.map(|(url, _)| url)))?;
    Ok(stored.unwrap_or_else(default_base_url))
}

pub(super) fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

pub(super) fn data_dir(app: &AppHandle) -> Result<PathBuf> {
    app.path()
        .app_data_dir()
        .map_err(|e| AppError::Internal(format!("Failed to get app data dir: {e}")))
}

/// The relay of this profile: the stored account's address (ruling SH-2), or the default relay
/// while no account is stored.
pub(super) fn profile_relay(db: &Database) -> Result<HttpRelay> {
    HttpRelay::new(&stored_relay_url(db)?)
}

/// Whether sync is on, a damaged `sync_state` row counting as on (`state::on_or_unreadable`).
pub(super) fn sync_on(db: &Database) -> Result<bool> {
    db.with_conn(state::on_or_unreadable)
}

/// The status of the open profile, with the running engine's memory of its rounds.
pub(crate) fn current_status(app: &AppHandle, db: &Database) -> Result<SyncStatus> {
    // Every status poll also wipes a Kit draft whose window ran out (review of Tasks 18–19, M1).
    expire_kit_draft(Instant::now());
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
pub(super) trait EngineControl {
    /// Stops the engine and returns once its tasks have ended.
    fn stop(&self) -> impl Future<Output = ()> + Send;
    /// Starts the engine again; nothing when sync is off by then.
    fn start(&self);
}

/// The app's engine of the open profile.
pub(super) struct AppEngine<'a> {
    pub(super) app: &'a AppHandle,
    pub(super) db: &'a Database,
}

impl EngineControl for AppEngine<'_> {
    async fn stop(&self) {
        stop_engine_and_wait(self.app).await;
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
pub(super) async fn guarded_change<T>(
    engine: &impl EngineControl,
    work: impl Future<Output = SyncResult<T>>,
) -> Result<T> {
    let _one = SyncChangeGuard::try_start()?;
    engine.stop().await;
    let result = work.await;
    engine.start();
    result.map_err(app_error)
}

/// The device password, checked against the profile's key file off the runtime (the key
/// derivation takes about a second).
pub(super) async fn verify_password(app: &AppHandle, password: Zeroizing<String>) -> Result<()> {
    let db_path = get_db_path(app)?;
    tauri::async_runtime::spawn_blocking(move || auth::verify_password(&db_path, &password))
        .await
        .map_err(|e| AppError::Internal(format!("The password check did not finish: {e}")))?
}

/// One sync change at a time in the process (`guarded_change`).
static SYNC_CHANGE_RUNNING: AtomicBool = AtomicBool::new(false);

/// Whether a sync change (`guarded_change`: a join, repair, re-sync, turning off, …) runs now.
pub(super) fn sync_change_running() -> bool {
    SYNC_CHANGE_RUNNING.load(Ordering::SeqCst)
}

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
pub(super) struct KitOwner {
    pub(super) db_path: PathBuf,
    pub(super) session: u64,
}

/// `sync_prepare_recovery_kit` checked the password of `owner` at `opened` (or a key change made a
/// new Kit then): the Kit draft of that moment is open.
#[derive(Debug, Clone)]
pub(super) struct KitWindow {
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

pub(super) fn kit_window() -> Option<KitWindow> {
    kit_window_slot().clone()
}

fn open_kit_window(owner: KitOwner, opened: Instant) {
    *kit_window_slot() = Some(KitWindow { owner, opened });
}

/// The Recovery Kit draft (revision §2.2): the Kit's seed while a flow shows it, never stored.
/// It lives with the copy window and goes with it (`forget_kit_window`).
static KIT_DRAFT: Mutex<Option<setup::KitDraft>> = Mutex::new(None);

fn kit_draft_slot() -> std::sync::MutexGuard<'static, Option<setup::KitDraft>> {
    KIT_DRAFT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Opens the copy window for `owner` with `draft` as the Kit it shows.
pub(super) fn open_kit_draft(owner: KitOwner, draft: setup::KitDraft) {
    *kit_draft_slot() = Some(draft);
    open_kit_window(owner, Instant::now());
}

/// `work` on the open Kit draft of this profile session, within the copy window
/// (`validation.syncPasswordRequired` otherwise, or when no draft is open).
pub(super) fn with_kit_draft<T>(
    app: &AppHandle,
    db: &Database,
    work: impl FnOnce(&mut setup::KitDraft) -> Result<T>,
) -> Result<T> {
    if !copy_allowed(kit_window().as_ref(), &kit_owner(app, db)?, Instant::now()) {
        expire_kit_draft(Instant::now());
        return Err(AppError::Validation(PASSWORD_REQUIRED.into()));
    }
    let mut slot = kit_draft_slot();
    let draft = slot
        .as_mut()
        .ok_or_else(|| AppError::Validation(PASSWORD_REQUIRED.into()))?;
    work(draft)
}

/// Wipes the Kit draft (its seed) and closes the window once `KIT_COPY_WINDOW` has run out, or
/// when no window is open: the seed never outlives its ten minutes in memory (review of Tasks
/// 18–19, M1). Called lazily by `with_kit_draft` and by every status poll.
pub(super) fn expire_kit_draft(now: Instant) {
    if window_ran_out(kit_window().as_ref(), now) {
        forget_kit_window();
    }
}

/// Whether the Kit draft must go: no window is open, or it ran out (`expire_kit_draft`).
fn window_ran_out(window: Option<&KitWindow>, now: Instant) -> bool {
    window.is_none_or(|window| now.saturating_duration_since(window.opened) > KIT_COPY_WINDOW)
}

/// Closes the copy window and wipes the Kit draft (lock, restore, account deletion, sync off,
/// synced data deleted, the flow's end).
pub(crate) fn forget_kit_window() {
    *kit_window_slot() = None;
    *kit_draft_slot() = None;
}

pub(super) fn kit_owner(app: &AppHandle, db: &Database) -> Result<KitOwner> {
    Ok(KitOwner {
        db_path: get_db_path(app)?,
        session: db.session_epoch(),
    })
}

/// The code is copied (and the Kit saved) only within `KIT_COPY_WINDOW` after the password of
/// this very profile session was checked, during setup too (audit M11: a person at an unlocked
/// app cannot take the Kit by starting setup).
pub(super) fn copy_allowed(window: Option<&KitWindow>, owner: &KitOwner, now: Instant) -> bool {
    window.is_some_and(|window| {
        window.owner == *owner && now.saturating_duration_since(window.opened) <= KIT_COPY_WINDOW
    })
}

/// What a native copy's answer means: a change token to clear later, `None` when this platform
/// has no native copy (Linux, Android: the UI copies itself and clears it), and
/// `validation.syncCopyFailed` for any other failure, so a failed native copy never falls back
/// to a plain clipboard write (audit M11).
fn copy_outcome(result: std::result::Result<u64, KeychainError>) -> Result<Option<u64>> {
    match result {
        Ok(token) => Ok(Some(token)),
        Err(KeychainError::Unsupported) => Ok(None),
        Err(e) => {
            log::warn!("[SYNC] The recovery code could not be copied natively: {e}");
            Err(AppError::Validation(COPY_FAILED.into()))
        }
    }
}

/// An empty password is `validation.syncPasswordNeeded` before any check runs (it would only
/// count as a wrong one).
pub(super) fn password_given(password: &str) -> Result<()> {
    if password.is_empty() {
        return Err(AppError::Validation(PASSWORD_NEEDED.into()));
    }
    Ok(())
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

/// A device leaving says so (phase 5 plan Task 11): while sync is on here, `setup::leave_best_effort`
/// signs a `left` record and sends it within `setup::LEAVE_BUDGET`. Called for turning sync off
/// and for Delete Account without the "delete the synced data" box (the vault stays for the other
/// devices); the engine has stopped, so no round holds the round lock. Never blocks and never
/// fails the change: every failure is logged there.
pub(crate) async fn say_left(db: &Database) {
    if !feature::enabled() {
        return;
    }
    let Ok(session) = db.current_session() else {
        return;
    };
    if !sync_on(&session).unwrap_or(false) {
        return;
    }
    let Ok(relay) = profile_relay(&session) else {
        log::warn!("[SYNC] This device did not say that it left: no valid sync server address");
        return;
    };
    setup::leave_best_effort(&relay, &session, setup::LEAVE_BUDGET).await;
}

// ============================================================================
// Commands
// ============================================================================

/// Whether sync runs in this build and process (`feature::enabled`).
#[tauri::command]
pub async fn sync_feature_enabled() -> Result<bool> {
    Ok(feature::enabled())
}

/// Works without the flag, read-only: whether this profile has sync on (`enabled`) and whether this
/// build runs sync (`featureOn`). A build without the flag shows a way out when the profile is on.
#[tauri::command]
pub async fn sync_profile_state(db: State<'_, Database>) -> Result<SyncProfileState> {
    db.with_conn(profile_state)
}

/// `sync_profile_state`'s answer.
fn profile_state(conn: &rusqlite::Connection) -> Result<SyncProfileState> {
    Ok(SyncProfileState {
        enabled: state::on_or_unreadable(conn)?,
        feature_on: feature::enabled(),
        pending_changes: status::unsent_changes(conn)?,
    })
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

/// Asks the server at an address, or in a connection code, what it is (`server::check_own_server`).
/// The input is never logged.
#[tauri::command]
pub async fn sync_check_server(input: String) -> Result<SyncServerCheck> {
    require_feature()?;
    let address = server_address_from_input(&input)?;
    let relay = HttpRelay::new(&address)?;
    server::check_own_server(&relay, &address)
        .await
        .map_err(app_error)
}

/// Sets up on a person's own server from its connection code (`setup::connect_own_server`): the
/// account is the one the code names; nothing is created or paid.
#[tauri::command]
pub async fn sync_connect_own_server(
    db: State<'_, Database>,
    connection_code: String,
) -> Result<SyncAccount> {
    require_feature()?;
    let code = parse_connection_code(&connection_code)?;
    let session = db.current_session()?;
    let relay = HttpRelay::new(&code.server_url)?;
    setup::connect_own_server(&relay, &session, &code)
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

/// The Kit during setup, every time behind the device password (the first showing included,
/// audit M11): makes the Kit draft, or shows the one still open for this vault again (revision
/// §2.2), and opens `KIT_COPY_WINDOW` for copying, printing, saving and finishing setup with it.
/// With sync on, `validation.syncAlreadyEnabled` (a Kit after setup comes from a key change).
#[tauri::command]
pub async fn sync_prepare_recovery_kit(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
) -> Result<RecoveryKit> {
    let password = Zeroizing::new(password);
    require_feature()?;
    password_given(&password)?;
    verify_password(&app, password).await?;
    let owner = kit_owner(&app, &db)?;
    let open = copy_allowed(kit_window().as_ref(), &owner, Instant::now())
        .then(|| kit_draft_slot().take())
        .flatten();
    let draft = db.with_conn(|conn| setup::prepare_kit(conn, open, now()))?;
    let kit = db.with_conn(|conn| setup::kit_view(conn, &draft))?;
    open_kit_draft(owner, draft);
    Ok(kit)
}

/// Copies the Kit's code natively (hidden from clipboard managers) and clears the clipboard
/// after `CLIPBOARD_CLEAR_AFTER`, or earlier at exit or lock (`clear_copied_code`), if nothing
/// else was copied since. The code is read here from the open Kit draft and never crosses IPC.
/// Only within `KIT_COPY_WINDOW` (`validation.syncPasswordRequired` otherwise). The Kit counts as
/// copied (`KitStorage::Copied`, revision §2.7) either way. `false`: no native copy on this
/// platform, so the UI copies itself and clears it; any other failure is
/// `validation.syncCopyFailed` (`copy_outcome`).
#[tauri::command]
pub async fn sync_copy_recovery_code(app: AppHandle, db: State<'_, Database>) -> Result<bool> {
    require_feature()?;
    let code = with_kit_draft(&app, &db, |draft| {
        db.with_conn(|conn| setup::kit_code(conn, draft))
    })?;
    let copied = copy_outcome(app.keychain().copy_secret_text(&code))?;
    drop(code);
    with_kit_draft(&app, &db, |draft| {
        db.with_conn(|conn| setup::note_kit_storage(conn, draft, KitStorage::Copied))
    })?;
    let Some(token) = copied else {
        return Ok(false);
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

/// "Vytisknout" (revision §6.3): the window's print dialog, while a print stylesheet shows only
/// the Kit sheet; Moony writes nothing to disk. Within `KIT_COPY_WINDOW`; the Kit counts as
/// printed (`KitStorage::Paper`) once the dialog was opened.
#[tauri::command]
pub async fn sync_print_recovery_kit(
    app: AppHandle,
    db: State<'_, Database>,
    window: tauri::WebviewWindow,
) -> Result<()> {
    require_feature()?;
    with_kit_draft(&app, &db, |_| Ok(()))?;
    window
        .print()
        .map_err(|e| AppError::Internal(format!("The print dialog did not open: {e}")))?;
    with_kit_draft(&app, &db, |draft| {
        db.with_conn(|conn| setup::note_kit_storage(conn, draft, KitStorage::Paper))
    })
}

/// Finishes setup (`setup::finish_setup`) and starts the engine; the status after it.
#[tauri::command]
pub async fn sync_finish_setup(
    app: AppHandle,
    db: State<'_, Database>,
    device_name: String,
) -> Result<SyncStatus> {
    require_feature()?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    // The open Kit draft (revision §2.2); none, or one the person did nothing with, is
    // `validation.syncRecoveryKitNotConfirmed`.
    let owner = kit_owner(&app, &db)?;
    let draft = copy_allowed(kit_window().as_ref(), &owner, Instant::now())
        .then(|| kit_draft_slot().take())
        .flatten()
        .ok_or_else(|| AppError::Validation(KIT_NOT_CONFIRMED.into()))?;
    let finish = setup::finish_setup(&relay, &session, &device_name, &draft, &dir, &native, now());
    // A person blocked between the steps of an own server never reads "subscription" copy.
    let finish = mapped_for_setup(stored_relay_url(&db)?, finish);
    match guarded_change(&AppEngine { app: &app, db: &db }, finish).await {
        Ok(setup::FinishSetup::Done) => forget_kit_window(),
        Ok(setup::FinishSetup::KitNotRegistered) => {
            // Sync is on without a Kit (the server holds an earlier try's): the status says so
            // (no `recoveryKit`) and the person makes a new one (Tasks 24, 27).
            log::warn!("[SYNC] Setup finished, but the Recovery Kit is not the vault's");
            forget_kit_window();
        }
        Err(error) => {
            // A retry finishes with the same Kit while the window is open.
            let mut slot = kit_draft_slot();
            if slot.is_none() && copy_allowed(kit_window().as_ref(), &owner, Instant::now()) {
                *slot = Some(draft);
            }
            return Err(error);
        }
    }
    current_status(&app, &db)
}

/// A round now, past any back-off. Nothing while no engine runs.
#[tauri::command]
pub async fn sync_now(app: AppHandle) -> Result<()> {
    require_feature()?;
    wake_engine(&app);
    Ok(())
}

/// Asks the running engine for a round now, past any back-off; nothing while no engine runs.
pub(super) fn wake_engine(app: &AppHandle) {
    if let Some(handle) = app
        .state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        handle.wake(Trigger::SyncNow);
    }
}

/// Opens the running engine's fast-poll window (a round every few seconds for a few minutes, or
/// until the device list grows): a device the holder just paired shows up in its list when it
/// pulls. Nothing while no engine runs.
pub(super) fn fast_poll_engine(app: &AppHandle) {
    // The count is taken here, before the window opens: a round already in flight may pull the new
    // device at any moment
    let baseline = app
        .state::<Database>()
        .with_conn(crate::services::sync::devices::count_devices)
        .ok();
    if let Some(handle) = app
        .state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        handle.fast_poll(baseline);
    }
}

/// Asks the running engine's transfer lanes to look at their queue now (content was wanted or
/// reset here); nothing while no engine runs (locked, sync off).
pub(crate) fn wake_transfers(app: &AppHandle) {
    if let Some(handle) = app
        .state::<SyncEngineState>()
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
    {
        handle.wake_transfers();
    }
}

/// Whether turning sync off (or deleting the synced data) ends a running pairing. A holder's
/// session needs sync on; with sync off only a joiner's can exist, and cancelling a half-finished
/// setup must keep it, so that the join which the setup refused can be tried again.
fn forgets_pairing_on_turn_off(sync_was_on: bool) -> bool {
    sync_was_on
}

/// Turning sync off with sync on needs the device password (audit L10):
/// `validation.syncPasswordNeeded` without one that was checked. Cancelling a setup that has not
/// finished (sync not on yet) needs none.
fn turn_off_allowed(sync_is_on: bool, password_checked: bool) -> Result<()> {
    if sync_is_on && !password_checked {
        return Err(AppError::Validation(PASSWORD_NEEDED.into()));
    }
    Ok(())
}

/// `sync_turn_off`'s work once no other sync change runs: `turn_off_allowed` again on the profile
/// as it is now (a setup may have finished since the first check), then
/// `setup::turn_off_between_rounds`.
async fn turn_off_rechecked<D: ConnAccess>(
    db: &D,
    data_dir: &Path,
    password_checked: bool,
) -> SyncResult<()> {
    let on = db.with_conn(state::on_or_unreadable)?;
    turn_off_allowed(on, password_checked)?;
    setup::turn_off_between_rounds(db, data_dir).await
}

/// Turns sync off on this device; local data stays. Works without the flag: it only removes sync
/// state (and the guard or `query_only`) from this profile. With sync on, only once the device password
/// is checked (`turn_off_allowed`, checked again once no other sync change runs, in case a setup
/// finished meanwhile); cancelling a setup that has not finished needs none.
#[tauri::command]
pub async fn sync_turn_off(
    app: AppHandle,
    db: State<'_, Database>,
    password: Option<String>,
) -> Result<()> {
    let password = password.map(Zeroizing::new);
    let checked = match password {
        Some(password) => {
            verify_password(&app, password).await?;
            true
        }
        None => false,
    };
    let session = db.current_session()?;
    let dir = data_dir(&app)?;
    let was_on = sync_on(&db)?;
    turn_off_allowed(was_on, checked)?;
    forget_kit_window();
    if forgets_pairing_on_turn_off(was_on) {
        super::sync_pairing::forget_pairing();
    }
    guarded_change(&AppEngine { app: &app, db: &db }, async {
        // Turning sync off with sync on says so to the other devices first (best effort, a few
        // seconds at most); a setup that never finished has nobody to tell.
        if was_on && checked {
            say_left(&db).await;
        }
        turn_off_rechecked(&session, &dir, checked).await
    })
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
    password_given(&password)?;
    verify_password(&app, password).await?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    forget_kit_window();
    if forgets_pairing_on_turn_off(sync_on(&db)?) {
        super::sync_pairing::forget_pairing();
    }
    guarded_change(
        &AppEngine { app: &app, db: &db },
        setup::delete_synced_data(&relay, &session, &dir),
    )
    .await
}

/// Repairs a capture-broken profile (`join::repair_capture`) once the device password is checked;
/// its backup and what stayed only in it.
#[tauri::command]
pub async fn sync_repair_device(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
) -> Result<ReplaceOutcomeView> {
    let password = Zeroizing::new(password);
    require_feature()?;
    password_given(&password)?;
    verify_password(&app, password).await?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    let repair = mapped_for_server(
        stored_relay_url(&db)?,
        join::repair_capture(&relay, &session, &dir, &native, now()),
    );
    let outcome = guarded_change(&AppEngine { app: &app, db: &db }, repair).await?;
    Ok(outcome_view(&outcome))
}

/// "Re-sync this device" (ruling T8-4): backup, then join by replace without a drain, once the
/// device password is checked, within what the person agreed to (`consent`: a change after it
/// stops the re-sync with `validation.syncReplaceImpactChanged`). Changes not sent yet survive
/// only in the backup; the outcome says how many.
#[tauri::command]
pub async fn sync_resync_device(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
    consent: ReplaceConsentView,
) -> Result<ReplaceOutcomeView> {
    let password = Zeroizing::new(password);
    require_feature()?;
    let consent = consent_of(&consent)?;
    password_given(&password)?;
    verify_password(&app, password).await?;
    let session = db.current_session()?;
    let relay = profile_relay(&session)?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    let resync = mapped_for_server(
        stored_relay_url(&db)?,
        join::resync_by_replace(&relay, &session, consent, &dir, &native, now()),
    );
    let outcome = guarded_change(&AppEngine { app: &app, db: &db }, resync).await?;
    Ok(outcome_view(&outcome))
}

/// Changes the sync server address of a device that syncs (`server::change_server_address`) once
/// the device password is checked. Only the ADDRESS is used: a connection code's account is
/// ignored, the stored account, vault and token stay. The engine is stopped for the swap and
/// starts again on the new stored address, its first round running at once (`runtime::spawn`).
#[tauri::command]
pub async fn sync_change_server_address(
    app: AppHandle,
    db: State<'_, Database>,
    password: String,
    address: String,
) -> Result<SyncStatus> {
    let password = Zeroizing::new(password);
    require_feature()?;
    password_given(&password)?;
    verify_password(&app, password).await?;
    let address = server_address_from_input(&address)?;
    forget_kit_window();
    super::sync_pairing::forget_pairing();
    let session = db.current_session()?;
    // The service builds the client from the address it normalised and stores
    let change = mapped_for_server(
        stored_relay_url(&db)?,
        server::change_server_address(
            |normalized| HttpRelay::new(normalized).map_err(SyncError::Local),
            &session,
            &address,
        ),
    );
    guarded_change(&AppEngine { app: &app, db: &db }, change).await?;
    current_status(&app, &db)
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

    #[test]
    fn turning_sync_off_with_sync_on_needs_the_password() {
        assert_eq!(
            validation_key(turn_off_allowed(true, false)),
            "validation.syncPasswordNeeded"
        );
        assert!(turn_off_allowed(true, true).is_ok());
        assert!(
            turn_off_allowed(false, false).is_ok(),
            "cancelling a setup that has not finished"
        );
    }

    #[tokio::test]
    async fn a_setup_that_finished_after_the_first_check_still_needs_the_password() {
        let db = TestConn::new(migrated());
        let dir = tempfile::tempdir().expect("dir");
        assert!(
            turn_off_allowed(sync_is_on(&db), false).is_ok(),
            "the first check: a setup that has not finished"
        );
        state::enable(&db.lock(), DeviceId::random()).expect("the setup finished meanwhile");

        let refused = turn_off_rechecked(&db, dir.path(), false).await;
        assert_eq!(
            validation_key(refused.map_err(app_error)),
            "validation.syncPasswordNeeded"
        );
        assert!(sync_is_on(&db), "nothing was turned off");

        turn_off_rechecked(&db, dir.path(), true)
            .await
            .expect("turned off with the password checked");
        assert!(!sync_is_on(&db));
    }

    /// Final review B, minor 1: a `sync_state` row that does not load (a damaged `device_id`)
    /// counts as on, as `sync::on_open` guards it: turning sync off still works, behind the
    /// password, and the profile state says sync is on.
    #[tokio::test]
    async fn an_unreadable_sync_row_can_still_be_turned_off() {
        let db = TestConn::new(migrated());
        state::enable(&db.lock(), DeviceId::random()).expect("sync on");
        db.lock()
            .execute("UPDATE sync_state SET device_id = X'01'", [])
            .expect("damage the row");
        assert!(state::load(&db.lock()).is_err(), "the row does not load");
        let dir = tempfile::tempdir().expect("dir");

        assert!(profile_state(&db.lock()).expect("profile state").enabled);
        let refused = turn_off_rechecked(&db, dir.path(), false).await;
        assert_eq!(
            validation_key(refused.map_err(app_error)),
            "validation.syncPasswordNeeded"
        );

        turn_off_rechecked(&db, dir.path(), true)
            .await
            .expect("turned off with the password checked");
        assert!(!sync_is_on(&db));
        assert!(!profile_state(&db.lock()).expect("profile state").enabled);
    }

    #[test]
    fn only_turning_sync_off_from_on_ends_the_pairing() {
        assert!(forgets_pairing_on_turn_off(true));
        assert!(!forgets_pairing_on_turn_off(false));
    }

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
        async fn stop(&self) {
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
    fn the_code_is_copied_only_shortly_after_the_password() {
        let now = Instant::now();
        let a = owner("/profiles/a/moony.db", 3);
        let shown = |opened| KitWindow {
            owner: a.clone(),
            opened,
        };
        // Setup included: no window, no copy (audit M11).
        assert!(!copy_allowed(None, &a, now));
        assert!(copy_allowed(Some(&shown(now)), &a, now));
        let edge = now + KIT_COPY_WINDOW;
        assert!(copy_allowed(Some(&shown(now)), &a, edge));
        assert!(!copy_allowed(
            Some(&shown(now)),
            &a,
            edge + Duration::from_secs(1)
        ));
    }

    /// Review of Tasks 18–19, M1: the draft's seed goes once the window ran out, not only at the
    /// flow's end or a lock.
    #[test]
    fn an_expired_window_wipes_the_kit_draft() {
        // The pure rule (the slots are process globals other tests touch in parallel).
        let now = Instant::now();
        let window = KitWindow {
            owner: owner("/profiles/a/moony.db", 3),
            opened: now,
        };
        assert!(window_ran_out(None, now), "no window: nothing to keep");
        assert!(!window_ran_out(Some(&window), now + KIT_COPY_WINDOW));
        assert!(window_ran_out(
            Some(&window),
            now + KIT_COPY_WINDOW + Duration::from_secs(1)
        ));
    }

    #[test]
    fn copy_outcome_falls_back_only_when_unsupported() {
        assert_eq!(copy_outcome(Ok(7)).expect("copied"), Some(7));
        assert_eq!(
            copy_outcome(Err(KeychainError::Unsupported)).expect("the UI copies"),
            None
        );
        for failed in [
            KeychainError::Failed("NSPasteboard refused".into()),
            KeychainError::Unavailable,
            KeychainError::Cancelled,
            KeychainError::Invalidated,
        ] {
            let error = copy_outcome(Err(failed)).expect_err("no fallback");
            assert!(
                matches!(&error, AppError::Validation(key) if key == COPY_FAILED),
                "{error:?}"
            );
        }
    }

    #[test]
    fn password_given_refuses_an_empty_password() {
        let error = password_given("").expect_err("empty");
        assert!(matches!(&error, AppError::Validation(key) if key == PASSWORD_NEEDED));
        assert!(password_given("correct horse").is_ok());
    }

    #[test]
    fn the_copy_window_belongs_to_one_profile_session() {
        let now = Instant::now();
        let a = owner("/profiles/a/moony.db", 3);
        let window = KitWindow {
            owner: a.clone(),
            opened: now,
        };
        assert!(copy_allowed(Some(&window), &a, now));
        assert!(
            !copy_allowed(Some(&window), &owner("/profiles/b/moony.db", 3), now),
            "another profile"
        );
        assert!(
            !copy_allowed(Some(&window), &owner("/profiles/a/moony.db", 5), now),
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

    /// The sync commands (name → body up to its third statement), read from the sources of this
    /// file, `sync_pairing.rs`, `sync_backups.rs`, `sync_kit.rs`, `sync_gaps.rs` and `sync_attachments.rs`.
    fn commands_and_their_first_statements() -> Vec<(String, String)> {
        [
            include_str!("sync.rs"),
            include_str!("sync_pairing.rs"),
            include_str!("sync_backups.rs"),
            include_str!("sync_kit.rs"),
            include_str!("sync_gaps.rs"),
            include_str!("sync_attachments.rs"),
        ]
        .into_iter()
        .flat_map(commands_of)
        .collect()
    }

    /// The commands of one source file.
    fn commands_of(source: &str) -> Vec<(String, String)> {
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
            .filter_map(|line| {
                let line = line.trim();
                line.strip_prefix("commands::sync::")
                    .or_else(|| line.strip_prefix("commands::sync_pairing::"))
                    .or_else(|| line.strip_prefix("commands::sync_backups::"))
                    .or_else(|| line.strip_prefix("commands::sync_kit::"))
                    .or_else(|| line.strip_prefix("commands::sync_gaps::"))
                    .or_else(|| line.strip_prefix("commands::sync_attachments::"))
            })
            .map(|name| name.trim_end_matches(','))
            .collect();
        assert_eq!(found, registered);
        assert_eq!(found.len(), 38);
        for (name, first_three) in &commands {
            // The flag itself, a read-only question, and the way out of an enabled profile.
            if [
                "sync_feature_enabled",
                "sync_profile_state",
                "sync_turn_off",
            ]
            .contains(&name.as_str())
            {
                continue;
            }
            assert!(
                first_three.contains("require_feature()?"),
                "`{name}` must call `require_feature()?` within its first three statements"
            );
        }
    }

    /// The body of `name` in this file's code above the tests, up to its closing brace.
    fn command_body(name: &str) -> &'static str {
        command_body_in(include_str!("sync.rs"), name)
    }

    /// The body of `name` in `source`'s code above its tests, up to its closing brace.
    fn command_body_in(source: &'static str, name: &str) -> &'static str {
        let code = source
            .split("\n#[cfg(test)]\nmod tests {")
            .next()
            .expect("code");
        let start = code
            .find(&format!("pub async fn {name}("))
            .unwrap_or_else(|| panic!("`{name}` is in the source"));
        let body = &code[start..];
        &body[..body.find("\n}\n").expect("the end of the command")]
    }

    /// Final review B, minor 6 (audit L10): the repair, the re-sync, deleting the synced data and
    /// the skip check the device password, and refuse an empty one before the key check runs.
    #[test]
    fn every_password_command_refuses_an_empty_password_then_checks_it() {
        for (source, name) in [
            (include_str!("sync.rs"), "sync_repair_device"),
            (include_str!("sync.rs"), "sync_resync_device"),
            (include_str!("sync.rs"), "sync_delete_synced_data"),
            (include_str!("sync.rs"), "sync_change_server_address"),
            (include_str!("sync_gaps.rs"), "sync_skip_missing_change"),
        ] {
            let body = command_body_in(source, name);
            let given = body
                .find("password_given(&password)?")
                .unwrap_or_else(|| panic!("`{name}` must call `password_given(`"));
            let checked = body
                .find("verify_password(&app, password).await?")
                .unwrap_or_else(|| panic!("`{name}` must call `verify_password(`"));
            assert!(given < checked, "`{name}` refuses an empty password first");
            let work = body.find("guarded_change(").expect("a sync change");
            assert!(
                checked < work,
                "`{name}` checks the password before the change"
            );
        }
    }

    /// Final fix I1: a blocked person on their own server never reads the payment message, on
    /// any change; the Moony cloud keeps it.
    #[test]
    fn a_blocked_account_reads_the_server_it_is_on() {
        let key = |error: AppError| match error {
            AppError::Validation(key) => key,
            other => panic!("a validation key, not {other:?}"),
        };
        assert_eq!(
            key(app_error_for(&default_base_url(), SyncError::Lapsed)),
            "validation.syncPaymentRequired"
        );
        for own in ["https://sync.example.org", "http://192.168.1.21:8787"] {
            assert_eq!(
                key(app_error_for(own, SyncError::Lapsed)),
                "validation.syncAccountBlocked"
            );
            // No connection code is involved: an unknown account is not a revoked code
            assert_eq!(
                key(app_error_for(
                    own,
                    SyncError::Refused(crate::services::sync::relay::RelayError::AccountNotFound)
                )),
                "validation.syncAccountNotFound"
            );
        }
    }

    /// Final fix I1: every change that talks to the person's server maps its error for that server.
    #[test]
    fn every_server_change_maps_its_error_for_the_server() {
        for (source, name) in [
            (include_str!("sync.rs"), "sync_repair_device"),
            (include_str!("sync.rs"), "sync_resync_device"),
            (include_str!("sync.rs"), "sync_change_server_address"),
            (
                include_str!("sync_pairing.rs"),
                "sync_join_with_recovery_kit",
            ),
        ] {
            let body = command_body_in(source, name);
            let mapped = body
                .find("mapped_for_server(")
                .unwrap_or_else(|| panic!("`{name}` maps for the server"));
            assert!(mapped < body.find("guarded_change(").expect("a change"));
        }
        let setup = command_body("sync_finish_setup");
        assert!(setup.contains("mapped_for_setup("));
        let join = command_body_in(include_str!("sync_pairing.rs"), "sync_pairing_join");
        assert!(join.contains("app_error_for(&relay_url, error)"));
    }

    /// Audit L10: the one command that works without the flag keeps the password rule.
    #[test]
    fn turning_sync_off_keeps_the_password_rule() {
        let body = command_body("sync_turn_off");
        assert!(body.contains("turn_off_allowed(was_on, checked)?"));
    }

    /// Audit M11: every showing of the Kit checks the device password, setup's first included,
    /// before the kit is read.
    #[test]
    fn every_kit_showing_checks_the_password_first() {
        {
            let name = "sync_prepare_recovery_kit";
            let body = command_body(name);
            let checked = body
                .find("verify_password(&app, password).await?")
                .unwrap_or_else(|| panic!("`{name}` must call `verify_password(`"));
            let read = body
                .find("prepare_kit(")
                .unwrap_or_else(|| panic!("`{name}` reads the kit"));
            assert!(
                checked < read,
                "`{name}` checks the password before reading the kit"
            );
            assert!(body.contains("password_given(&password)?"), "{name}");
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
