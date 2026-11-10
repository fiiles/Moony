//! Adding a second device (phase 3, spec §20, ADR 0026): the pairing commands around
//! `services::sync::pairing::session`, the new device's joins (`join::join_new_device` with
//! credentials received by pairing or made from a Sync Recovery Kit) and the device list.
//!
//! - One pairing at a time, in `PAIRING`, bound to the profile session that started it
//!   (`KitOwner`, as the Recovery Kit's copy window). A command of another profile session, or one
//!   past `PairingSession::wiped_at`, abandons it. Lock, restore and Delete Account abandon it too
//!   (`forget_pairing`), and a received session is wiped at `wiped_at` by its own task (review
//!   PF6), so received credentials never outlive `expires_at + RECEIVED_GRACE_SECS`.
//! - Both roles check the device password before a QR is shown or slot 1 is written (`begin`,
//!   review PF1). The role is the profile's own: holder with sync on, joiner without.
//! - The pairing code's text crosses IPC only with the developer tools (review PF2); the QR and
//!   the six digits are not secret, and the received credentials never leave Rust.
//! - Joins run as a sync change (`guarded_change`: one at a time, engine stopped and started).
//!
//! No command logs its arguments; passwords and recovery codes stay in `Zeroizing` once received.

use std::future::Future;
use std::time::Duration;

use rusqlite::Connection;
use tauri::{AppHandle, State};
use zeroize::Zeroizing;

use super::sync::{
    app_error, data_dir, guarded_change, kit_owner, now, require_feature, sync_on, verify_password,
    AppEngine, KeychainBinding, KitOwner,
};
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::sync::{PairingRole, PairingStage, PairingView, SyncDevice};
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::keys::{self, Credentials};
use crate::services::sync::pairing::session::{keeps_session, PairingSession, Stage};
use crate::services::sync::pairing::{pairing_code_text, MailboxId, Role, Side};
use crate::services::sync::recovery::{qr_svg, read_recovery_input};
use crate::services::sync::relay::http::{default_base_url, normalize_base_url, HttpRelay};
use crate::services::sync::relay::Mailbox;
use crate::services::sync::{devices, feature, join, state};

const NOT_STARTED: &str = "validation.syncPairingNotStarted";
const EXPIRED: &str = "validation.syncPairingExpired";
const PASSWORD_REQUIRED: &str = "validation.syncPairingPasswordRequired";
const FAILED: &str = "validation.syncPairingFailed";
const SETUP_INCOMPLETE: &str = "validation.syncSetupIncomplete";
const CHANGE_IN_PROGRESS: &str = "validation.syncChangeInProgress";

/// The pairing of this app, with the profile session that started it and, for a joiner, the
/// device name it joins under.
struct PairingSlot {
    owner: KitOwner,
    session: PairingSession,
    device_name: Option<String>,
}

/// The one pairing in progress, if any.
static PAIRING: tokio::sync::Mutex<Option<PairingSlot>> = tokio::sync::Mutex::const_new(None);

// ============================================================================
// Helpers of the commands
// ============================================================================

/// The gate in front of every new pairing and every Kit join (review PF1): an empty password is
/// `validation.syncPairingPasswordRequired`, without awaiting `verify`; `verify` (the device
/// password check) must succeed before `then` is polled at all, so no QR is rendered and nothing
/// is written to a mailbox before it did.
async fn begin<T>(
    password: Zeroizing<String>,
    verify: impl Future<Output = Result<()>>,
    then: impl Future<Output = SyncResult<T>>,
) -> Result<T> {
    if password.is_empty() {
        return Err(AppError::Validation(PASSWORD_REQUIRED.into()));
    }
    drop(password);
    verify.await?;
    then.await.map_err(app_error)
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
/// a rate limit, or another sync change running. Anything else ends the pairing.
fn after_join_error(error: &SyncError) -> bool {
    keeps_session(error)
        || matches!(error, SyncError::Local(AppError::Validation(key)) if key == CHANGE_IN_PROGRESS)
}

/// The session as the UI shows it. The QR (and, with the developer tools only, the same code as
/// text) while this device shows the code and waits for the scan.
fn view_of(session: &PairingSession, dev_tools: bool) -> Result<PairingView> {
    let showing = session.side() == Side::Shower && session.stage() == Stage::WaitingForScan;
    let code = showing.then(|| pairing_code_text(&session.code()));
    let qr = code.as_deref().map(qr_svg).transpose()?;
    Ok(PairingView {
        stage: stage_of(session.stage()),
        role: match session.role() {
            Role::Holder => PairingRole::Holder,
            Role::Joiner => PairingRole::Joiner,
        },
        qr_svg: qr,
        pairing_code: code.filter(|_| dev_tools),
        comparison_code: session.comparison_code(),
        contested: session.contested(),
        expires_at: session.wiped_at(),
    })
}

fn stage_of(stage: Stage) -> PairingStage {
    match stage {
        Stage::Announcing => PairingStage::Announcing,
        Stage::WaitingForScan => PairingStage::WaitingForScan,
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
/// account address (`validation.syncSetupIncomplete` without one); a joiner's typed address, else
/// its stored account address, else `default_base_url()`. Normalized.
fn shower_relay(conn: &Connection, role: Role, typed: Option<&str>) -> Result<String> {
    let stored = keys::account(conn)?.map(|(url, _)| url);
    let url = match role {
        Role::Holder => stored.ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?,
        Role::Joiner => typed
            .map(str::trim)
            .filter(|url| !url.is_empty())
            .map(str::to_string)
            .or(stored)
            .unwrap_or_else(default_base_url),
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

/// The holder's credentials when its confirm will seal them (holder, in Compare, not contested),
/// read with sync on only; `None` for every other confirm.
fn holder_credentials(session: &PairingSession, db: &Database) -> SyncResult<Option<Credentials>> {
    if session.role() != Role::Holder || session.stage() != Stage::Compare || session.contested() {
        return Ok(None);
    }
    let creds = db.with_conn(|conn| {
        if !state::load(conn)?.is_some_and(|sync| sync.enabled) {
            return Ok(None);
        }
        keys::credentials(conn)
    })?;
    match creds {
        Some(creds) => Ok(Some(creds)),
        None => Err(AppError::Validation(FAILED.into()).into()),
    }
}

fn mailbox_of(session: &PairingSession) -> MailboxId {
    session.code().mailbox
}

/// Gives a pairing up: cancel markers and the delete within `ABANDON_BUDGET`, best effort (a
/// holder in Sent sends nothing); the session's secrets go with it.
async fn abandon(slot: PairingSlot) {
    match HttpRelay::new(slot.session.relay_url()) {
        Ok(relay) => slot.session.abandon(&relay).await,
        Err(_) => drop(slot),
    }
}

/// `abandon` on the app's runtime, so the command that ends a pairing answers at once.
fn abandon_later(slot: PairingSlot) {
    tauri::async_runtime::spawn(abandon(slot));
}

/// Takes the pairing in progress, if any, and abandons it.
async fn abandon_current() {
    if let Some(slot) = PAIRING.lock().await.take() {
        abandon_later(slot);
    }
}

/// Ends the pairing (abandoned) after an error that does not keep the session.
fn end_unless_kept(pairing: &mut Option<PairingSlot>, error: &SyncError) {
    if !keeps_session(error) {
        if let Some(slot) = pairing.take() {
            abandon_later(slot);
        }
    }
}

/// The live slot of `owner`'s profile session. A stale one (another profile session, or past
/// `wiped_at`) is abandoned and answers `validation.syncPairingNotStarted` or
/// `validation.syncPairingExpired`; no slot is `validation.syncPairingNotStarted`.
fn live_slot<'a>(
    pairing: &'a mut Option<PairingSlot>,
    owner: &KitOwner,
    now: i64,
) -> Result<&'a mut PairingSlot> {
    let (kept, stale) = slot_for(owner, now, pairing.take());
    *pairing = kept;
    if let Some(stale) = stale {
        let key = if stale.owner == *owner {
            EXPIRED
        } else {
            NOT_STARTED
        };
        abandon_later(stale);
        return Err(AppError::Validation(key.into()));
    }
    pairing
        .as_mut()
        .ok_or_else(|| AppError::Validation(NOT_STARTED.into()))
}

fn current_view(pairing: &Option<PairingSlot>) -> Result<PairingView> {
    let slot = pairing
        .as_ref()
        .ok_or_else(|| AppError::Validation(NOT_STARTED.into()))?;
    view_of(&slot.session, feature::dev_tools())
}

/// Makes `session` the pairing in progress, unless the profile session that started it has
/// ended meanwhile (a lock during the password check); an older pairing is abandoned.
async fn install(
    app: &AppHandle,
    db: &Database,
    owner: KitOwner,
    session: PairingSession,
    device_name: Option<String>,
) -> Result<PairingView> {
    let slot = PairingSlot {
        owner,
        session,
        device_name,
    };
    let mut pairing = PAIRING.lock().await;
    if kit_owner(app, db).ok().as_ref() != Some(&slot.owner) {
        abandon_later(slot);
        return Err(AppError::Validation(NOT_STARTED.into()));
    }
    let view = match view_of(&slot.session, feature::dev_tools()) {
        Ok(view) => view,
        Err(error) => {
            abandon_later(slot);
            return Err(error);
        }
    };
    if let Some(older) = pairing.replace(slot) {
        abandon_later(older);
    }
    Ok(view)
}

/// Received credentials never outlive `wiped_at` (review PF6): the session answers nothing past
/// it, and this task abandons it then if it is still the pairing in progress.
fn wipe_when_due(mailbox: MailboxId, wiped_at: i64) {
    tauri::async_runtime::spawn(async move {
        let wait = u64::try_from(wiped_at.saturating_sub(now())).unwrap_or(0);
        tokio::time::sleep(Duration::from_secs(wait)).await;
        let mut pairing = PAIRING.lock().await;
        if pairing
            .as_ref()
            .is_some_and(|slot| mailbox_of(&slot.session) == mailbox)
        {
            if let Some(slot) = pairing.take() {
                drop(pairing);
                abandon(slot).await;
            }
        }
    });
}

/// Abandons the pairing in progress, if any (lock, restore, Delete Account).
pub(crate) fn forget_pairing() {
    tauri::async_runtime::spawn(async {
        if let Some(slot) = PAIRING.lock().await.take() {
            abandon(slot).await;
        }
    });
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
    require_feature()?;
    let session = db.current_session()?;
    let role = Role::of(sync_on(&session)?);
    let device_name = joiner_name(role, device_name)?;
    let relay_url = session.with_conn(|conn| shower_relay(conn, role, relay_url.as_deref()))?;
    let owner = kit_owner(&app, &db)?;
    let verify = verify_password(&app, password.clone());
    let pairing = begin(password, verify, async {
        abandon_current().await;
        Ok(PairingSession::show(role, &relay_url, now())?)
    })
    .await?;
    install(&app, &db, owner, pairing, device_name).await
}

/// Answers a scanned pairing code once the device password is checked (slot 1 is written then);
/// `deviceName` as for `sync_pairing_start`. A momentary relay failure leaves the view in
/// `announcing`, and polling retries.
#[tauri::command]
pub async fn sync_pairing_scan(
    app: AppHandle,
    db: State<'_, Database>,
    code: String,
    device_name: Option<String>,
    password: String,
) -> Result<PairingView> {
    let password = Zeroizing::new(password);
    require_feature()?;
    let session = db.current_session()?;
    let role = Role::of(sync_on(&session)?);
    let device_name = joiner_name(role, device_name)?;
    let owner = kit_owner(&app, &db)?;
    let verify = verify_password(&app, password.clone());
    let pairing = begin(password, verify, async {
        abandon_current().await;
        let scanned = PairingSession::scan(role, &code, now())?;
        let relay = HttpRelay::new(scanned.relay_url())?;
        announced(scanned, &relay, now()).await
    })
    .await?;
    install(&app, &db, owner, pairing, device_name).await
}

/// One step of waiting (the UI polls every 2 s while the view waits). A momentary relay failure
/// answers the unchanged view.
#[tauri::command]
pub async fn sync_pairing_poll(app: AppHandle, db: State<'_, Database>) -> Result<PairingView> {
    require_feature()?;
    let owner = kit_owner(&app, &db)?;
    let mut pairing = PAIRING.lock().await;
    let polled = {
        let slot = live_slot(&mut pairing, &owner, now())?;
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
            end_unless_kept(&mut pairing, &error);
            return Err(app_error(error));
        }
    }
    current_view(&pairing)
}

/// The person confirmed the six digits match: a holder seals its sync details for the other
/// device, a joiner starts waiting for them. A contested scan answers
/// `validation.syncPairingCodeUsed`.
#[tauri::command]
pub async fn sync_pairing_confirm(app: AppHandle, db: State<'_, Database>) -> Result<PairingView> {
    require_feature()?;
    let owner = kit_owner(&app, &db)?;
    let session = db.current_session()?;
    let mut pairing = PAIRING.lock().await;
    let confirmed = {
        let slot = live_slot(&mut pairing, &owner, now())?;
        let relay = HttpRelay::new(slot.session.relay_url())?;
        match holder_credentials(&slot.session, &session) {
            Ok(creds) => slot.session.confirm(&relay, creds.as_ref(), now()).await,
            Err(error) => Err(error),
        }
    };
    if let Err(error) = confirmed {
        end_unless_kept(&mut pairing, &error);
        return Err(app_error(error));
    }
    current_view(&pairing)
}

/// Gives the pairing in progress up ("They don't match", closing the dialog).
#[tauri::command]
pub async fn sync_pairing_cancel() -> Result<()> {
    require_feature()?;
    abandon_current().await;
    Ok(())
}

/// The joiner joins with the received sync details (stage `received` only): backup, then join by
/// replace, as a sync change; the backup's path. A failure that may pass keeps the received
/// session for "Try again"; success ends it.
#[tauri::command]
pub async fn sync_pairing_join(app: AppHandle, db: State<'_, Database>) -> Result<String> {
    require_feature()?;
    let owner = kit_owner(&app, &db)?;
    let session = db.current_session()?;
    let dir = data_dir(&app)?;
    let mut pairing = PAIRING.lock().await;
    let slot = live_slot(&mut pairing, &owner, now())?;
    // The slot keeps its own copy of the credentials for "Try again" (review PF6).
    let (Some(creds), Some(device_name)) =
        (slot.session.received().cloned(), slot.device_name.clone())
    else {
        return Err(AppError::Validation(NOT_STARTED.into()));
    };
    let relay = HttpRelay::new(&creds.relay_url)?;
    let native = KeychainBinding { app: app.clone() };
    let join = join::join_new_device(&relay, &session, creds, &device_name, &dir, &native, now());
    // The join's own error stays a `SyncError` for `after_join_error`; the outer one is the
    // change guard's.
    let joined = guarded_change(&AppEngine { app: &app, db: &db }, async { Ok(join.await) })
        .await
        .map_err(SyncError::Local)
        .and_then(|joined| joined);
    match joined {
        Ok(backup) => {
            pairing.take();
            Ok(backup.display().to_string())
        }
        Err(error) => {
            if !after_join_error(&error) {
                if let Some(slot) = pairing.take() {
                    abandon_later(slot);
                }
            }
            Err(app_error(error))
        }
    }
}

/// Joins with a Sync Recovery Kit (its QR text or typed code, with the account number and an
/// optional sync server for a typed one) once the device password is checked: backup, then join
/// by replace, as a sync change; the backup's path.
#[tauri::command]
pub async fn sync_join_with_recovery_kit(
    app: AppHandle,
    db: State<'_, Database>,
    code: String,
    account: Option<String>,
    relay_url: Option<String>,
    device_name: String,
    password: String,
) -> Result<String> {
    let password = Zeroizing::new(password);
    let code = Zeroizing::new(code);
    require_feature()?;
    devices::validate_device_name(&device_name)?;
    let verify = verify_password(&app, password.clone());
    begin(password, verify, std::future::ready(Ok(()))).await?;
    let input = read_recovery_input(
        &code,
        account.as_deref(),
        relay_url.as_deref(),
        &default_base_url(),
    )?;
    drop(code);
    let relay = HttpRelay::new(&input.relay_url)?;
    let session = db.current_session()?;
    let dir = data_dir(&app)?;
    let native = KeychainBinding { app: app.clone() };
    let join = async {
        let creds = join::recovery_credentials(&relay, &session, input).await?;
        join::join_new_device(&relay, &session, creds, &device_name, &dir, &native, now()).await
    };
    let backup = guarded_change(&AppEngine { app: &app, db: &db }, join).await?;
    Ok(backup.display().to_string())
}

/// The devices of the sync account.
#[tauri::command]
pub async fn sync_list_devices(db: State<'_, Database>) -> Result<Vec<SyncDevice>> {
    require_feature()?;
    db.with_conn(devices::list_devices)
}

/// Renames this device (1 to 60 characters); the new name syncs to the other devices.
#[tauri::command]
pub async fn sync_rename_device(db: State<'_, Database>, name: String) -> Result<()> {
    require_feature()?;
    db.with_conn(|conn| devices::rename_this_device(conn, &name))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;
    use crate::services::sync::error::Transient;
    use crate::services::sync::relay::fake::FakeRelay;
    use crate::services::sync::relay::RelayError;
    use crate::services::sync::test_support::migrated;

    const T0: i64 = 1_800_000_000;
    const HOME: &str = "https://sync.home.example.com";

    fn owner(path: &str, session: u64) -> KitOwner {
        KitOwner {
            db_path: PathBuf::from(path),
            session,
        }
    }

    fn validation(key: &str) -> SyncError {
        SyncError::Local(AppError::Validation(key.into()))
    }

    fn key_of(result: Result<impl std::fmt::Debug>) -> String {
        match result.expect_err("refused") {
            AppError::Validation(key) | AppError::Auth(key) => key,
            other => panic!("not a key: {other:?}"),
        }
    }

    fn wrong_password() -> Result<()> {
        Err(AppError::Auth("auth.wrongPassword".into()))
    }

    #[tokio::test]
    async fn pairing_needs_the_password_before_anything_is_shown_or_written() {
        // A joiner shows a code; this device scans it as the holder.
        let shower = PairingSession::show(Role::Joiner, HOME, T0).expect("shower");
        let text = pairing_code_text(&shower.code());
        let mailbox = mailbox_of(&shower).to_b64();

        for (password, verify, expected) in [
            ("", Ok(()), "validation.syncPairingPasswordRequired"),
            ("wrong", wrong_password(), "auth.wrongPassword"),
        ] {
            let relay = FakeRelay::new();
            let polled = AtomicBool::new(false);
            let scan = async {
                polled.store(true, Ordering::SeqCst);
                let scanned = PairingSession::scan(Role::Holder, &text, T0)?;
                announced(scanned, &relay, T0).await
            };
            let refused = begin(
                Zeroizing::new(password.to_string()),
                std::future::ready(verify),
                scan,
            )
            .await;
            assert_eq!(key_of(refused), expected);
            assert!(!polled.load(Ordering::SeqCst), "the scan never started");
            assert!(relay.calls().is_empty(), "nothing was sent: {password:?}");
            assert_eq!(relay.peek_slot(&mailbox, 1), None);

            // A holder's show: no pairing code is made.
            let shown = AtomicBool::new(false);
            let show = async {
                shown.store(true, Ordering::SeqCst);
                Ok(PairingSession::show(Role::Holder, HOME, T0)?)
            };
            let verify = if password.is_empty() {
                Ok(())
            } else {
                wrong_password()
            };
            let refused = begin(
                Zeroizing::new(password.to_string()),
                std::future::ready(verify),
                show,
            )
            .await;
            assert_eq!(key_of(refused), expected);
            assert!(!shown.load(Ordering::SeqCst), "no code was made");
        }

        // The right password: the scan answers, and slot 1 is written.
        let relay = FakeRelay::new();
        let scan = async {
            let scanned = PairingSession::scan(Role::Holder, &text, T0)?;
            announced(scanned, &relay, T0).await
        };
        let scanned = begin(
            Zeroizing::new("right".to_string()),
            std::future::ready(Ok(())),
            scan,
        )
        .await
        .expect("scanned");
        assert_eq!(scanned.stage(), Stage::Compare);
        assert!(relay.peek_slot(&mailbox, 1).is_some(), "slot 1 is written");
    }

    fn slot_of(owner: KitOwner) -> PairingSlot {
        PairingSlot {
            owner,
            session: PairingSession::show(Role::Holder, HOME, T0).expect("session"),
            device_name: None,
        }
    }

    #[test]
    fn a_pairing_session_belongs_to_one_profile_session() {
        let a = owner("/profiles/a/moony.db", 1);
        let now = T0 + 10;

        let (kept, stale) = slot_for(&a, now, Some(slot_of(a.clone())));
        assert!(kept.is_some_and(|slot| slot.owner == a));
        assert!(stale.is_none());

        for other in [
            owner("/profiles/a/moony.db", 2),
            owner("/profiles/b/moony.db", 1),
        ] {
            let (kept, stale) = slot_for(&other, now, Some(slot_of(a.clone())));
            assert!(kept.is_none(), "{other:?} does not get it");
            assert!(stale.is_some_and(|slot| slot.owner == a), "it is abandoned");
        }

        // The right profile session, past `wiped_at`.
        let slot = slot_of(a.clone());
        let wiped_at = slot.session.wiped_at();
        assert_eq!(wiped_at, T0 + 600);
        let (kept, stale) = slot_for(&a, wiped_at - 1, Some(slot));
        assert!(kept.is_some() && stale.is_none(), "alive until wiped_at");
        let (kept, stale) = slot_for(&a, wiped_at, kept);
        assert!(kept.is_none());
        assert!(stale.is_some(), "abandoned at wiped_at");

        let (kept, stale) = slot_for(&a, now, None);
        assert!(kept.is_none() && stale.is_none());
    }

    #[test]
    fn a_transient_join_failure_keeps_the_received_session() {
        for kept in [
            SyncError::Transient(Transient::Offline),
            SyncError::Transient(Transient::Relay),
            SyncError::RateLimited,
            validation("validation.syncChangeInProgress"),
        ] {
            assert!(after_join_error(&kept), "{kept:?} keeps the session");
        }
        for ended in [
            validation("validation.syncAlreadyEnabled"),
            validation("validation.syncJoinSetupPending"),
            validation("validation.syncVaultHasNoSnapshot"),
            SyncError::Refused(RelayError::AccountNotFound),
            SyncError::Refused(RelayError::SlotConsumed),
            SyncError::VaultNotFound,
            SyncError::Local(AppError::Internal("disk full".into())),
        ] {
            assert!(!after_join_error(&ended), "{ended:?} ends the pairing");
        }
    }

    #[test]
    fn the_pairing_code_text_is_a_developer_tool() {
        let session = PairingSession::show(Role::Holder, HOME, T0).expect("session");
        let text = pairing_code_text(&session.code());
        assert!(text.starts_with("moony-pair:1:"));

        let release = view_of(&session, false).expect("view");
        assert_eq!(
            release.pairing_code, None,
            "a release build shows the QR only"
        );
        assert!(release
            .qr_svg
            .as_deref()
            .is_some_and(|svg| svg.contains("<svg")));
        assert_eq!(release.stage, PairingStage::WaitingForScan);
        assert_eq!(release.role, PairingRole::Holder);
        assert_eq!(release.comparison_code, None);
        assert!(!release.contested);
        assert_eq!(release.expires_at, T0 + 600);

        let dev = view_of(&session, true).expect("view");
        assert_eq!(dev.pairing_code.as_deref(), Some(text.as_str()));
        assert_eq!(dev.qr_svg, release.qr_svg);

        // A scanner shows no code of its own, with or without the developer tools.
        let scanner = PairingSession::scan(Role::Joiner, &text, T0).expect("scanner");
        let view = view_of(&scanner, true).expect("view");
        assert_eq!(view.stage, PairingStage::Announcing);
        assert_eq!(view.role, PairingRole::Joiner);
        assert_eq!((view.qr_svg, view.pairing_code), (None, None));
    }

    #[test]
    fn the_shown_mailbox_is_on_the_server_of_the_role() {
        let conn = migrated();
        let default = normalize_base_url(&default_base_url()).expect("default");
        // Without an account: a holder cannot show, a joiner uses the typed address or the
        // default.
        assert_eq!(
            key_of(shower_relay(&conn, Role::Holder, Some(HOME))),
            "validation.syncSetupIncomplete"
        );
        assert_eq!(
            shower_relay(&conn, Role::Joiner, None).expect("joiner"),
            default
        );
        assert_eq!(
            shower_relay(&conn, Role::Joiner, Some("  ")).expect("joiner"),
            default
        );
        assert_eq!(
            shower_relay(&conn, Role::Joiner, Some(" https://sync.example.com/ ")).expect("typed"),
            normalize_base_url("https://sync.example.com").expect("typed")
        );
        assert_eq!(
            key_of(shower_relay(&conn, Role::Joiner, Some("ftp://nope"))),
            "validation.syncRelayUrlInvalid"
        );

        // With an account: a holder always its own; a joiner the typed one first.
        keys::save_account(&conn, HOME, "1234567890123456").expect("account");
        assert_eq!(
            shower_relay(&conn, Role::Holder, Some("https://sync.example.com")).expect("holder"),
            HOME
        );
        assert_eq!(
            shower_relay(&conn, Role::Joiner, None).expect("joiner"),
            HOME
        );
        assert_eq!(
            shower_relay(&conn, Role::Joiner, Some("https://sync.example.com")).expect("typed"),
            normalize_base_url("https://sync.example.com").expect("typed")
        );
    }

    #[test]
    fn only_a_joiner_needs_a_device_name() {
        assert_eq!(joiner_name(Role::Holder, None).expect("holder"), None);
        assert_eq!(
            joiner_name(Role::Holder, Some(String::new())).expect("ignored"),
            None
        );
        assert_eq!(
            joiner_name(Role::Joiner, Some("Work laptop".into())).expect("joiner"),
            Some("Work laptop".to_string())
        );
        for invalid in [None, Some("   ".to_string()), Some("x".repeat(61))] {
            assert_eq!(
                key_of(joiner_name(Role::Joiner, invalid)),
                "validation.syncDeviceNameInvalid"
            );
        }
    }
}
