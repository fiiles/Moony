//! Adding a second device (phase 3, spec §20, ADR 0026): the pairing commands around
//! `services::sync::pairing::session`, the new device's joins (`join::join_new_device` with
//! credentials received by pairing or made from a Sync Recovery Kit) and the device list.
//!
//! - One pairing at a time, in `PAIRING`, bound to the profile session that started it
//!   (`KitOwner`, as the Recovery Kit's copy window). A command of another profile session, or one
//!   past `PairingSession::wiped_at`, abandons it. Lock, restore and Delete Account abandon it too,
//!   and so do turning sync off and deleting the synced data while sync was on: with sync off only
//!   a joiner's session exists, which cancelling a half-finished setup must keep for "Try again"
//!   (`forget_pairing`, which never waits behind a running join). A received session is wiped at
//!   `wiped_at` by its own task (review PF6), so received credentials never outlive
//!   `expires_at + RECEIVED_GRACE_SECS`.
//! - New pairings (start, scan) are made one at a time under the slot's lock; a repeated scan of
//!   the code already answered keeps that session.
//! - Both roles check the device password before a QR is shown or slot 1 is written (`begin`,
//!   review PF1). The role is the profile's own: holder with sync on, joiner without.
//! - The pairing code's text crosses IPC with the QR in every build (a person without a camera
//!   types or pastes it; it holds what the QR holds, lives minutes and is guarded by the
//!   comparison step); the received credentials never leave Rust.
//! - Joins run as a sync change (`guarded_change`: one at a time, engine stopped and started).
//!
//! No command logs its arguments; passwords and recovery codes stay in `Zeroizing` once received.

use std::future::Future;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use rusqlite::Connection;
use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use super::sync::{
    app_error, app_error_for, data_dir, guarded_change, kit_owner, mapped_for_server, now,
    require_feature, require_feature_outside_demo, sync_on, verify_password, AppEngine,
    KeychainBinding, KitOwner,
};
use super::sync_backups::{consent_of, outcome_view};
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::sync::{
    PairingRole, PairingStage, PairingView, ReplaceConsentView, ReplaceOutcomeView, SyncDevice,
};
use crate::services::sync::connection_code::server_address_from_input;
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::impact::IMPACT_CHANGED;
use crate::services::sync::keys;
use crate::services::sync::pairing::holder;
use crate::services::sync::pairing::session::{keeps_session, PairingSession, Stage};
use crate::services::sync::pairing::{pairing_code_text, MailboxId, Role, Side};
use crate::services::sync::recovery::{qr_svg, read_kit_input};
use crate::services::sync::relay::http::{
    default_base_url, display_host, is_default_relay, is_insecure_address, normalize_base_url,
    HttpRelay,
};
use crate::services::sync::relay::Mailbox;
use crate::services::sync::{devices, join};

const NOT_STARTED: &str = "validation.syncPairingNotStarted";
const EXPIRED: &str = "validation.syncPairingExpired";
const PASSWORD_NEEDED: &str = "validation.syncPasswordNeeded";
const FAILED: &str = "validation.syncPairingFailed";
const SETUP_INCOMPLETE: &str = "validation.syncSetupIncomplete";
const CHANGE_IN_PROGRESS: &str = "validation.syncChangeInProgress";
const WAITING_FOR_DEVICE: &str = "validation.syncWaitingForDevice";
const JOIN_SETUP_PENDING: &str = "validation.syncJoinSetupPending";

/// The longest the wiper sleeps before it reads the wall clock again (a machine that slept does
/// not advance a monotonic timer).
const WIPER_TICK_SECS: i64 = 30;

/// The pairing of this app, with the profile session that started it and, for a joiner, the
/// device name it joins under.
struct PairingSlot {
    owner: KitOwner,
    session: PairingSession,
    device_name: Option<String>,
}

/// The one pairing slot of the app, and how a pairing that ends is given up (`dispose`).
struct Pairings {
    slot: tokio::sync::Mutex<Option<PairingSlot>>,
    /// Set by `forget` while the slot is busy: whoever holds the lock next, or lets it go, gives
    /// the pairing up.
    forget: AtomicBool,
    dispose: fn(PairingSlot),
}

impl Pairings {
    const fn new(dispose: fn(PairingSlot)) -> Self {
        Pairings {
            slot: tokio::sync::Mutex::const_new(None),
            forget: AtomicBool::new(false),
            dispose,
        }
    }

    /// The slot, held until the guard drops. A pending `forget` is honoured first.
    async fn lock(&self) -> PairingGuard<'_> {
        let mut slot = self.slot.lock().await;
        if self.forget.swap(false, Ordering::SeqCst) {
            if let Some(old) = slot.take() {
                (self.dispose)(old);
            }
        }
        PairingGuard {
            pairings: self,
            slot: Some(slot),
        }
    }

    /// Gives the pairing up without waiting: at once while the slot is free, else as soon as its
    /// holder (a running join, say) lets go of it.
    fn forget(&self) {
        self.forget.store(true, Ordering::SeqCst);
        self.forget_if_free();
    }

    fn forget_if_free(&self) {
        if !self.forget.load(Ordering::SeqCst) {
            return;
        }
        if let Ok(mut slot) = self.slot.try_lock() {
            if self.forget.swap(false, Ordering::SeqCst) {
                if let Some(old) = slot.take() {
                    drop(slot);
                    (self.dispose)(old);
                }
            }
        }
    }
}

/// The held slot. Letting go of it honours a `forget` that came meanwhile.
struct PairingGuard<'a> {
    pairings: &'a Pairings,
    /// `None` only inside `drop`.
    slot: Option<tokio::sync::MutexGuard<'a, Option<PairingSlot>>>,
}

impl Deref for PairingGuard<'_> {
    type Target = Option<PairingSlot>;

    fn deref(&self) -> &Self::Target {
        self.slot
            .as_ref()
            .expect("the slot is held until the guard drops")
    }
}

impl DerefMut for PairingGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.slot
            .as_mut()
            .expect("the slot is held until the guard drops")
    }
}

impl Drop for PairingGuard<'_> {
    fn drop(&mut self) {
        // Released first: a `forget` that found the slot busy has set its flag by now.
        drop(self.slot.take());
        self.pairings.forget_if_free();
    }
}

impl PairingGuard<'_> {
    /// Gives the pairing in the slot up, if any.
    fn dispose_current(&mut self) {
        if let Some(slot) = self.take() {
            (self.pairings.dispose)(slot);
        }
    }

    /// Ends the pairing after an error that does not keep the session.
    fn end_unless_kept(&mut self, error: &SyncError) {
        if !keeps_session(error) {
            self.dispose_current();
        }
    }

    /// The live slot of `owner`'s profile session. A stale one (another profile session, or
    /// past `wiped_at`) is given up and answers `validation.syncPairingNotStarted` or
    /// `validation.syncPairingExpired`; no slot is `validation.syncPairingNotStarted`.
    fn live(&mut self, owner: &KitOwner, now: i64) -> Result<&mut PairingSlot> {
        let (kept, stale) = slot_for(owner, now, self.take());
        **self = kept;
        if let Some(stale) = stale {
            let key = if stale.owner == *owner {
                EXPIRED
            } else {
                NOT_STARTED
            };
            (self.pairings.dispose)(stale);
            return Err(AppError::Validation(key.into()));
        }
        self.as_mut()
            .ok_or_else(|| AppError::Validation(NOT_STARTED.into()))
    }

    /// Makes `session` the pairing in progress in place of an older one (given up), unless the
    /// profile session that started it has ended meanwhile (`owner_now`: a lock during the
    /// password check or the announce).
    fn install(
        &mut self,
        owner: &KitOwner,
        owner_now: Option<KitOwner>,
        session: PairingSession,
        device_name: Option<String>,
    ) -> Result<PairingView> {
        let slot = PairingSlot {
            owner: owner.clone(),
            session,
            device_name,
        };
        if owner_now.as_ref() != Some(owner) {
            (self.pairings.dispose)(slot);
            return Err(AppError::Validation(NOT_STARTED.into()));
        }
        let view = match view_of(&slot.session) {
            Ok(view) => view,
            Err(error) => {
                (self.pairings.dispose)(slot);
                return Err(error);
            }
        };
        if let Some(older) = self.replace(slot) {
            (self.pairings.dispose)(older);
        }
        Ok(view)
    }
}

/// The one pairing in progress, if any.
static PAIRING: Pairings = Pairings::new(abandon_later);

// ============================================================================
// Helpers of the commands
// ============================================================================

/// The gate in front of every new pairing and every Kit join (review PF1): an empty password is
/// `validation.syncPasswordNeeded`, without awaiting `verify`; `verify` (the device
/// password check) must succeed before `then` is polled at all, so no QR is rendered and nothing
/// is written to a mailbox before it did.
async fn begin<T>(
    password: Zeroizing<String>,
    verify: impl Future<Output = Result<()>>,
    then: impl Future<Output = SyncResult<T>>,
) -> Result<T> {
    if password.is_empty() {
        return Err(AppError::Validation(PASSWORD_NEEDED.into()));
    }
    drop(password);
    verify.await?;
    then.await.map_err(app_error)
}

/// A holder with a key-change head it has not verified (`holder::refuse_while_unverified`) shows
/// no code and scans none: `validation.syncChangeInProgress` (its own) or
/// `validation.syncWaitingForDevice` (another's), before the password is asked.
fn holder_may_hand_keys_on(session: &Database, role: Role) -> Result<()> {
    match role {
        Role::Holder => session.with_conn(holder::refuse_while_unverified),
        Role::Joiner => Ok(()),
    }
}

/// The slot a command may use: kept while it belongs to `owner`'s profile session and `now` is
/// before its `wiped_at`; otherwise it is handed back to be abandoned.
fn slot_for(
    owner: &KitOwner,
    now: i64,
    slot: Option<PairingSlot>,
) -> (Option<PairingSlot>, Option<PairingSlot>) {
    match slot {
        Some(slot) if slot.owner == *owner && now < slot.session.wiped_at() => (Some(slot), None),
        other => (None, other),
    }
}

/// Whether a failed join keeps the received session for "Try again": a momentary relay failure,
/// a rate limit, another sync change running, or a sync setup of another account still pending
/// (the person can cancel it and try again within the grace period), or what the confirmation
/// showed no longer holding (`IMPACT_CHANGED`: nothing was replaced, the confirmation reads the
/// new numbers and asks again within the grace period). Anything else ends the pairing.
fn after_join_error(error: &SyncError) -> bool {
    keeps_session(error)
        || matches!(
            error,
            SyncError::Local(AppError::Validation(key))
                if key == CHANGE_IN_PROGRESS
                    || key == WAITING_FOR_DEVICE
                    || key == JOIN_SETUP_PENDING
                    || key == IMPACT_CHANGED
        )
}

/// The session as the UI shows it. The QR and the same code as text (to copy and type on the other
/// device when it cannot scan) while this device shows the code and waits for the scan.
fn view_of(session: &PairingSession) -> Result<PairingView> {
    let showing = session.side() == Side::Shower && session.stage() == Stage::WaitingForScan;
    let code = showing.then(|| pairing_code_text(&session.code()));
    let qr = code.as_deref().map(qr_svg).transpose()?;
    // The server of the received credentials, once they are here: where the account is used.
    let joined = session
        .received()
        .filter(|_| session.role() == Role::Joiner && session.stage() == Stage::Received)
        .map(|creds| creds.relay_url.as_str());
    Ok(PairingView {
        stage: stage_of(session.stage()),
        role: match session.role() {
            Role::Holder => PairingRole::Holder,
            Role::Joiner => PairingRole::Joiner,
        },
        qr_svg: qr,
        pairing_code: code,
        comparison_code: session.comparison_code(),
        contested: session.contested(),
        expires_at: session.wiped_at(),
        server_host: server_host(session.relay_url()),
        join_server: joined.and_then(display_host),
        join_server_insecure: joined.is_some_and(is_insecure_address),
        attempts_left: (session.role() == Role::Holder && session.stage() == Stage::Compare)
            .then(|| session.attempts_left()),
    })
}

/// `host[:port]` of a sync server that is not the default one.
fn server_host(relay_url: &str) -> Option<String> {
    if is_default_relay(relay_url) {
        return None;
    }
    display_host(relay_url)
}

fn stage_of(stage: Stage) -> PairingStage {
    match stage {
        Stage::Announcing => PairingStage::Announcing,
        Stage::WaitingForScan => PairingStage::WaitingForScan,
        // Answered, but the digits wait for the shower's reveal (protocol 4): the scanner reads
        // slot 3, the shower writes it.
        Stage::AwaitingAck | Stage::Acknowledging => PairingStage::Connecting,
        Stage::Compare => PairingStage::Compare,
        Stage::WaitingForOther => PairingStage::WaitingForOther,
        Stage::Sent => PairingStage::Sent,
        Stage::Received => PairingStage::Received,
    }
}

/// A joiner's device name, checked before the password; a holder's is ignored.
fn joiner_name(role: Role, name: Option<String>) -> Result<Option<String>> {
    match role {
        Role::Holder => Ok(None),
        Role::Joiner => {
            let name = name.unwrap_or_default();
            devices::validate_device_name(&name)?;
            Ok(Some(name))
        }
    }
}

/// The sync server of the mailbox this device shows (plan correction 2): a holder's stored
/// account address (`validation.syncSetupIncomplete` without one); a joiner's typed address or
/// connection code (`server_address_from_input`: only the code's address is used), else its
/// stored account address, else `default_base_url()`. Normalized.
fn shower_relay(conn: &Connection, role: Role, typed: Option<&str>) -> Result<String> {
    let stored = keys::account(conn)?.map(|(url, _)| url);
    let url = match role {
        Role::Holder => stored.ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?,
        Role::Joiner => {
            return match typed.filter(|text| !text.trim().is_empty()) {
                Some(text) => server_address_from_input(text),
                None => normalize_base_url(&stored.unwrap_or_else(default_base_url)),
            }
        }
    };
    normalize_base_url(&url)
}

/// The scanner's announce right after the scan: a `keeps_session` error leaves the session in
/// Announcing (the next poll retries); any other abandons it and is returned.
async fn announced<M: Mailbox>(
    mut session: PairingSession,
    mailbox: &M,
    now: i64,
) -> SyncResult<PairingSession> {
    match session.announce(mailbox, now).await {
        Ok(_) => Ok(session),
        Err(error) if keeps_session(&error) => Ok(session),
        Err(error) => {
            session.abandon(mailbox).await;
            Err(error)
        }
    }
}

fn mailbox_of(session: &PairingSession) -> MailboxId {
    session.code().mailbox
}

/// Whether two sessions answer the same pairing code (the same mailbox, commitment and server).
fn same_code(a: &PairingSession, b: &PairingSession) -> bool {
    pairing_code_text(&a.code()) == pairing_code_text(&b.code())
}

/// Gives a pairing up: cancel markers and the delete within `ABANDON_BUDGET`, best effort (a
/// holder in Sent sends nothing); the session's secrets go with it.
async fn abandon(slot: PairingSlot) {
    match HttpRelay::new(slot.session.relay_url()) {
        Ok(relay) => slot.session.abandon(&relay).await,
        Err(_) => drop(slot),
    }
}

/// `abandon` on the app's runtime, so nothing waits for it (`PAIRING`'s `dispose`).
fn abandon_later(slot: PairingSlot) {
    tauri::async_runtime::spawn(abandon(slot));
}

fn current_view(pairing: &Option<PairingSlot>) -> Result<PairingView> {
    let slot = pairing
        .as_ref()
        .ok_or_else(|| AppError::Validation(NOT_STARTED.into()))?;
    view_of(&slot.session)
}

/// A new shown pairing, made under the slot's lock (one start or scan at a time): the older
/// pairing is given up first.
async fn start_under_lock(
    pairings: &Pairings,
    owner: &KitOwner,
    owner_now: impl FnOnce() -> Option<KitOwner>,
    session: PairingSession,
    device_name: Option<String>,
) -> SyncResult<PairingView> {
    let mut pairing = pairings.lock().await;
    pairing.dispose_current();
    Ok(pairing.install(owner, owner_now(), session, device_name)?)
}

/// A scanned pairing, made under the slot's lock. A repeated scan of the code this profile
/// session already answered (the camera reports a QR twice, two scans race) answers that
/// session's view and writes nothing; any other code gives the older pairing up and announces.
#[allow(clippy::too_many_arguments)]
async fn scan_under_lock<M: Mailbox>(
    pairings: &Pairings,
    owner: &KitOwner,
    owner_now: impl FnOnce() -> Option<KitOwner>,
    scanned: PairingSession,
    mailbox: &M,
    device_name: Option<String>,
    now: i64,
) -> SyncResult<PairingView> {
    let mut pairing = pairings.lock().await;
    if let Some(slot) = pairing.as_ref() {
        let alive = slot.owner == *owner && now < slot.session.wiped_at();
        if alive && slot.session.side() == Side::Scanner && same_code(&slot.session, &scanned) {
            return Ok(view_of(&slot.session)?);
        }
    }
    pairing.dispose_current();
    let session = announced(scanned, mailbox, now).await?;
    Ok(pairing.install(owner, owner_now(), session, device_name)?)
}

/// How long the wiper sleeps before it looks at the wall clock again: at most
/// `WIPER_TICK_SECS`; `None` once `wiped_at` has passed.
fn wiper_wait(now: i64, wiped_at: i64) -> Option<Duration> {
    let left = wiped_at.saturating_sub(now);
    (left > 0).then(|| Duration::from_secs(left.min(WIPER_TICK_SECS).unsigned_abs()))
}

/// Received credentials never outlive `wiped_at` (review PF6): the session answers nothing past
/// it, and this task gives it up then if it is still the pairing in progress. It sleeps in short
/// ticks against the wall clock, so a machine that slept does not keep the credentials longer.
fn wipe_when_due(mailbox: MailboxId, wiped_at: i64) {
    tauri::async_runtime::spawn(async move {
        while let Some(wait) = wiper_wait(now(), wiped_at) {
            tokio::time::sleep(wait).await;
        }
        let mut pairing = PAIRING.lock().await;
        if pairing
            .as_ref()
            .is_some_and(|slot| mailbox_of(&slot.session) == mailbox)
        {
            pairing.dispose_current();
        }
    });
}

/// Gives the pairing in progress up (lock, restore, Delete Account, and turning sync off or
/// deleting the synced data while sync was on) without waiting: a running join gives it up as
/// soon as it ends.
pub(crate) fn forget_pairing() {
    PAIRING.forget();
}

// ============================================================================
// Commands
// ============================================================================

/// Shows a pairing code once the device password is checked: a holder (sync on) to send its sync
/// details, a joiner (sync off; `deviceName` required) to receive them. An older pairing is
/// abandoned first.
#[tauri::command]
pub async fn sync_pairing_start(
    app: AppHandle,
    db: State<'_, Database>,
    relay_url: Option<String>,
    device_name: Option<String>,
    password: String,
) -> Result<PairingView> {
    let password = Zeroizing::new(password);
    require_feature_outside_demo()?;
    let session = db.current_session()?;
    let role = Role::of(sync_on(&session)?);
    holder_may_hand_keys_on(&session, role)?;
    let device_name = joiner_name(role, device_name)?;
    let relay_url = session.with_conn(|conn| shower_relay(conn, role, relay_url.as_deref()))?;
    let owner = kit_owner(&app, &db)?;
    let verify = verify_password(&app, password.clone());
    begin(password, verify, async {
        let shown = PairingSession::show(role, &relay_url, now())?;
        let owner_now = || kit_owner(&app, &db).ok();
        start_under_lock(&PAIRING, &owner, owner_now, shown, device_name).await
    })
    .await
}

/// Answers a scanned pairing code once the device password is checked (slot 1 is written then);
/// `deviceName` as for `sync_pairing_start`. A momentary relay failure leaves the view in
/// `announcing`, and polling retries. Scanning the code already answered answers its view.
#[tauri::command]
pub async fn sync_pairing_scan(
    app: AppHandle,
    db: State<'_, Database>,
    code: String,
    device_name: Option<String>,
    password: String,
) -> Result<PairingView> {
    let password = Zeroizing::new(password);
    require_feature_outside_demo()?;
    let session = db.current_session()?;
    let role = Role::of(sync_on(&session)?);
    holder_may_hand_keys_on(&session, role)?;
    let device_name = joiner_name(role, device_name)?;
    let owner = kit_owner(&app, &db)?;
    let verify = verify_password(&app, password.clone());
    begin(password, verify, async {
        let now = now();
        let scanned = PairingSession::scan(role, &code, now)?;
        let relay = HttpRelay::new(scanned.relay_url())?;
        let owner_now = || kit_owner(&app, &db).ok();
        scan_under_lock(
            &PAIRING,
            &owner,
            owner_now,
            scanned,
            &relay,
            device_name,
            now,
        )
        .await
    })
    .await
}

/// One step of waiting (the UI polls every 2 s while the view waits). A momentary relay failure
/// answers the unchanged view.
#[tauri::command]
pub async fn sync_pairing_poll(app: AppHandle, db: State<'_, Database>) -> Result<PairingView> {
    require_feature()?;
    let owner = kit_owner(&app, &db)?;
    let mut pairing = PAIRING.lock().await;
    let polled = {
        let slot = pairing.live(&owner, now())?;
        let relay = HttpRelay::new(slot.session.relay_url())?;
        let before = slot.session.stage();
        slot.session
            .poll(&relay, now())
            .await
            .map(|stage| (before, stage))
    };
    match polled {
        Ok((before, Stage::Received)) if before != Stage::Received => {
            if let Some(slot) = pairing.as_ref() {
                wipe_when_due(mailbox_of(&slot.session), slot.session.wiped_at());
            }
        }
        Ok(_) => {}
        Err(error) if keeps_session(&error) => {}
        Err(error) => {
            pairing.end_unless_kept(&error);
            return Err(app_error(error));
        }
    }
    current_view(&pairing)
}

/// The person confirmed. A holder passes the six digits typed from the joining device's screen
/// and, if they are the joiner's, seals its sync details for it; nothing typed, or text that is
/// not six digits, is `validation.syncPairingCodeIncomplete` (no attempt spent), a wrong code
/// `validation.syncPairingCodeMismatch` (the pairing stays) until the third, which ends it with
/// `validation.syncPairingCodeMismatchFinal`. A joiner starts waiting for the details (`typed_code`
/// is ignored). A contested scan answers `validation.syncPairingCodeUsed`.
#[tauri::command]
pub async fn sync_pairing_confirm(
    app: AppHandle,
    db: State<'_, Database>,
    typed_code: Option<String>,
) -> Result<PairingView> {
    let typed_code = typed_code.map(Zeroizing::new);
    require_feature()?;
    let owner = kit_owner(&app, &db)?;
    let session = db.current_session()?;
    let mut pairing = PAIRING.lock().await;
    let confirmed = {
        let slot = pairing.live(&owner, now())?;
        let relay = HttpRelay::new(slot.session.relay_url())?;
        // The vault keys are read only for text that can be a code, holding off every round;
        // the join ticket's secret goes as soon as slot 2 is sealed (or not).
        holder::confirm(
            &mut slot.session,
            &relay,
            &session,
            typed_code.as_deref().map(String::as_str),
            now(),
        )
        .await
    };
    if let Err(error) = confirmed {
        pairing.end_unless_kept(&error);
        return Err(app_error(error));
    }
    // The holder confirmed: the new device joins within minutes and shows in the device list when
    // this device pulls, so the engine pulls often for a while instead of every 5 minutes.
    if typed_code.is_some() {
        super::sync::fast_poll_engine(&app);
    }
    current_view(&pairing)
}

/// Gives the pairing in progress up (Cancel, closing the dialog).
#[tauri::command]
pub async fn sync_pairing_cancel() -> Result<()> {
    require_feature()?;
    PAIRING.lock().await.dispose_current();
    Ok(())
}

/// The joiner joins with the received sync details (stage `received` only, else
/// `validation.syncPairingFailed`): backup, then join by replace, as a sync change, within what
/// the person agreed to (`consent`: more documents and photos than it showed stop the join with
/// `validation.syncReplaceImpactChanged`); the outcome. A failure that may pass keeps the
/// received session for "Try again"; success ends it. A lock meanwhile (`forget_pairing`) gives
/// the session up when the join ends.
#[tauri::command]
pub async fn sync_pairing_join(
    app: AppHandle,
    db: State<'_, Database>,
    consent: ReplaceConsentView,
) -> Result<ReplaceOutcomeView> {
    require_feature_outside_demo()?;
    let consent = consent_of(&consent)?;
    let owner = kit_owner(&app, &db)?;
    let session = db.current_session()?;
    let dir = data_dir(&app)?;
    let mut pairing = PAIRING.lock().await;
    let slot = pairing.live(&owner, now())?;
    // The slot keeps its own copy of the credentials and the holder's identity for "Try again"
    // (review PF6).
    let (Some(creds), Some(device_name)) =
        (slot.session.received().cloned(), slot.device_name.clone())
    else {
        return Err(AppError::Validation(FAILED.into()));
    };
    let heads = slot.session.received_heads().cloned().unwrap_or_default();
    let identity = match slot.session.received_identity() {
        Some(paired) => join::JoinIdentity::Paired(paired.duplicate(), heads),
        None => join::JoinIdentity::Unvouched(heads),
    };
    let relay = HttpRelay::new(&creds.relay_url)?;
    let relay_url = creds.relay_url.clone();
    let native = KeychainBinding { app: app.clone() };
    let join = join::join_new_device(
        &relay,
        &session,
        creds,
        &device_name,
        consent,
        identity,
        &dir,
        &native,
        now(),
    );
    // The join's own error stays a `SyncError` for `after_join_error`; the outer one is the
    // change guard's.
    let joined = guarded_change(&AppEngine { app: &app, db: &db }, async { Ok(join.await) })
        .await
        .map_err(SyncError::Local)
        .and_then(|joined| joined);
    match joined {
        Ok(outcome) => {
            pairing.take();
            Ok(outcome_view(&outcome))
        }
        Err(error) => {
            // A join whose replace committed has spent its ticket: no "Try again" with it.
            let spent = session.with_conn(join::pairing_spent).unwrap_or_else(|e| {
                log::warn!("[SYNC] Could not tell whether the join replaced the profile: {e}");
                true
            });
            if spent || !after_join_error(&error) {
                pairing.dispose_current();
            }
            Err(app_error_for(&relay_url, error))
        }
    }
}

/// Joins with a Recovery Kit (its QR text or typed v2 code, which holds the account, with an
/// optional sync server for a typed one) once the device password is checked: backup, then join
/// by replace, as a sync change, within what the person agreed to (`consent`, as
/// `sync_pairing_join`); the outcome. The code carries the account.
#[allow(clippy::too_many_arguments)]
#[tauri::command]
pub async fn sync_join_with_recovery_kit(
    app: AppHandle,
    db: State<'_, Database>,
    code: String,
    relay_url: Option<String>,
    device_name: String,
    password: String,
    consent: ReplaceConsentView,
) -> Result<ReplaceOutcomeView> {
    let password = Zeroizing::new(password);
    let code = Zeroizing::new(code);
    require_feature_outside_demo()?;
    let consent = consent_of(&consent)?;
    devices::validate_device_name(&device_name)?;
    let verify = verify_password(&app, password.clone());
    begin(password, verify, std::future::ready(Ok(()))).await?;
    let input = read_kit_input(&code, relay_url.as_deref(), &default_base_url())?;
    drop(code);
    let relay = HttpRelay::new(&input.relay_url)?;
    let relay_url = input.relay_url.clone();
    let session = db.current_session()?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    let join = async {
        let (creds, kit) = join::recovery_credentials(&relay, &session, input).await?;
        join::join_new_device(
            &relay,
            &session,
            creds,
            &device_name,
            consent,
            join::JoinIdentity::RecoveryKit(Box::new(kit)),
            &dir,
            &native,
            now(),
        )
        .await
    };
    let join = mapped_for_server(relay_url, join);
    let outcome = guarded_change(&AppEngine { app: &app, db: &db }, join).await?;
    Ok(outcome_view(&outcome))
}

/// The devices of the sync account.
#[tauri::command]
pub async fn sync_list_devices(db: State<'_, Database>) -> Result<Vec<SyncDevice>> {
    require_feature()?;
    db.with_conn(|conn| devices::list_devices(conn, now()))
}

/// Renames this device (1 to 60 characters); the new name syncs to the other devices.
#[tauri::command]
pub async fn sync_rename_device(db: State<'_, Database>, name: String) -> Result<()> {
    require_feature()?;
    db.with_conn(|conn| devices::rename_this_device(conn, &name))
}

#[cfg(test)]
#[path = "sync_pairing_tests.rs"]
mod tests;
