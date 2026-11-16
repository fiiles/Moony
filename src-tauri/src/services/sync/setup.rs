//! First-device setup (spec §11.5, ruling P4), turning sync off and deleting synced data (§11.6),
//! on the relay the person chose (ruling SH-2: the default relay or a self-hosted one).
//!
//! Setup runs in steps the UI drives: `create_account` (stores the relay address and the account
//! number), `account_status` and `checkout_url` until the account is paid, `prepare_kit` (makes
//! the vault's ID and key once and a Recovery Kit draft, revision §2.2, shown by `kit_view`),
//! then `finish_setup` once the person printed, copied or saved the Kit (`note_kit_storage`).
//! Every step can be repeated after a restart: what is stored is reused, never made again.
//!
//! The relay passed to every function here is the one at the stored address: the caller builds it
//! with `HttpRelay::new` from the address `create_account` was given, and after that from
//! `sync_state.relay_url` (`keys::account`), never from `default_base_url()`.
//!
//! Every database step runs off the runtime (`off_thread`) without a network call inside it, but
//! two in `finish_setup`: the copy check of a resumed setup (`join::rejoin_if_copy_under_lock`)
//! and the binding write (`write_binding_if_due`) run on the runtime, under the connection lock
//! and the round lock, because the platform's sealing (`BindingNative`) comes in as a borrowed
//! `&dyn` that cannot move to the blocking pool (as in `join::write_binding_after_replace`).
//! Each is at most one keychain or DPAPI open or seal of 32 bytes, a small file read or write
//! and, for a copy, the re-join's one savepoint. (`block_in_place` would need a multi-thread
//! runtime; the tests run on tokio's current-thread one.)
//!
//! `finish_setup`, `delete_synced_data` and `turn_off_between_rounds` hold off the profile's
//! rounds (`engine::exclusive`) while they change what a round works on.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Url;
use rusqlite::Connection;
use zeroize::Zeroizing;

use super::binding::{self, BindingNative};
use super::capture::guard;
use super::connection_code::ConnectionCode;
use super::devices;
use super::engine::snapshots::{off_thread, SnapshotPolicy};
use super::engine::{self, ConnAccess, RoundMode, RoundReport};
use super::error::{
    SyncError, SyncResult, TooLarge, Transient, ACCOUNT_BLOCKED, CONNECTION_CODE_REVOKED,
};
use super::hlc::DeviceId;
use super::identity::trust::{self, TrustState};
use super::identity::{self, Issuer};
use super::identity::{cert, CertKind, Certificate, KitKeys, KitSecret};
use super::in_savepoint;
use super::join::rejoin_if_copy_under_lock;
use super::keys::{self, Credentials, VaultId};
use super::recovery::{
    decode_kit_code, encode_kit_code, format_recovery_code, kit_qr_payload, parse_kit_qr_payload,
    qr_svg,
};
use super::rekey::kit::{self as rekey_kit, KitStorage, NewKit};
use super::relay::http::{is_default_relay, normalize_base_url, validate_base_url};
use super::relay::{CheckoutPlan, Relay, RelayError};
use super::server::check_own_server;
use super::state;
use crate::db::files_schema;
use crate::error::{AppError, Result};
use crate::models::sync::{CheckoutTarget, RecoveryKit, SyncAccount, SyncPlan, SyncProblem};
use crate::services::attachment_store;

const KIT_NOT_CONFIRMED: &str = "validation.syncRecoveryKitNotConfirmed";
const SETUP_INCOMPLETE: &str = "validation.syncSetupIncomplete";
const ALREADY_ENABLED: &str = "validation.syncAlreadyEnabled";
const RELAY_ERROR: &str = "validation.syncRelayError";
const SERVER_NEEDS_UPDATE: &str = "validation.syncServerNeedsUpdate";

/// How long Delete Account waits for the relay to delete the vault.
pub const DELETE_BUDGET: Duration = Duration::from_secs(5);

/// How long a device that is turning sync off, or being deleted, spends telling the other devices
/// that it left (`leave_best_effort`), waiting for a running round included.
pub const LEAVE_BUDGET: Duration = Duration::from_secs(5);

fn refused(key: &str) -> SyncError {
    SyncError::Local(AppError::Validation(key.into()))
}

/// `work` on the profile's connection, off the runtime.
pub(super) async fn on_db<D: ConnAccess, T: Send + 'static>(
    db: &D,
    work: impl FnOnce(&Connection) -> Result<T> + Send + 'static,
) -> SyncResult<T> {
    let db = db.clone();
    off_thread(move || db.with_conn(work)).await
}

fn enabled(conn: &Connection) -> Result<bool> {
    Ok(state::load(conn)?.is_some_and(|sync| sync.enabled))
}

/// The sync account at `relay_url` (ruling SH-2: `https`, or `http` only for `localhost` and
/// `127.0.0.1`; anything else is `validation.syncRelayUrlInvalid` before the relay is asked).
///
/// When this profile already stored an account at that address (an earlier try, a restart), the
/// relay is asked for its status and that account is returned. Otherwise the relay creates one,
/// and its number is stored with the address (normalized, `normalize_base_url`) only after the
/// relay answered: a crash in between leaves an unpaid, empty relay account nobody knows, which
/// holds nothing and costs nothing. An account stored at another address is replaced while sync is
/// off (the person changed the relay before finishing); with sync on it is
/// `validation.syncAlreadyEnabled`.
pub async fn create_account<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    relay_url: &str,
) -> SyncResult<SyncAccount> {
    let relay_url = normalize_base_url(relay_url)?;
    let (stored, on) = on_db(db, |conn| Ok((keys::account(conn)?, enabled(conn)?))).await?;
    if let Some((stored_url, account)) = stored {
        if normalize_base_url(&stored_url).is_ok_and(|url| url == relay_url) {
            return status_of(relay, account).await;
        }
    }
    if on {
        return Err(refused(ALREADY_ENABLED));
    }
    let account = relay.create_account().await?;
    let (url, number) = (relay_url, account.clone());
    on_db(db, move |conn| keys::save_account(conn, &url, &number)).await?;
    log::info!("[SYNC] Created a sync account");
    Ok(SyncAccount {
        account_number: account,
        active: false,
        paid_until: None,
    })
}

/// Setup on a person's own server from its connection code (spec §9): the server is checked
/// (`check_own_server`: the service does not trust that the UI checked), the account the code names
/// is asked for its status, and only an account that exists and is active is stored with the
/// server's address; the server never creates one and no payment follows. `relay` is the server at
/// the code's address.
///
/// With sync on, a code for another server or account is `validation.syncAlreadyEnabled`; the same
/// one only reads the status. While sync is off an account stored earlier is replaced (the vault's
/// secrets stay, as in `create_account`: the vault is made at `finish_setup`). An account the
/// server does not know is `validation.syncConnectionCodeRevoked`, a blocked one
/// `validation.syncAccountBlocked`; nothing is stored on any refusal.
pub async fn connect_own_server<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    code: &ConnectionCode,
) -> SyncResult<SyncAccount> {
    let (stored, on) = on_db(db, |conn| Ok((keys::account(conn)?, enabled(conn)?))).await?;
    let same = stored.as_ref().is_some_and(|(url, account)| {
        *account == code.account && normalize_base_url(url).is_ok_and(|url| url == code.server_url)
    });
    if on && !same {
        return Err(refused(ALREADY_ENABLED));
    }
    check_own_server(relay, &code.server_url).await?;
    let account = match status_of(relay, code.account.clone()).await {
        Ok(account) => account,
        Err(SyncError::Refused(RelayError::AccountNotFound)) => {
            return Err(refused(CONNECTION_CODE_REVOKED))
        }
        Err(error) => return Err(error),
    };
    if !account.active {
        return Err(refused(ACCOUNT_BLOCKED));
    }
    if !same {
        let (url, number) = (code.server_url.clone(), code.account.clone());
        on_db(db, move |conn| keys::save_account(conn, &url, &number)).await?;
        log::info!("[SYNC] Adopted an account on an own server");
    }
    Ok(account)
}

/// The stored account as the server sees it now; `validation.syncSetupIncomplete` without one. On
/// the Moony cloud the UI polls this until the account is paid; on an own sync server the account
/// is active unless the operator blocked the person in the server's admin interface.
pub async fn account_status<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> SyncResult<SyncAccount> {
    let account = stored_account(db).await?;
    status_of(relay, account).await
}

/// Where to pay for the stored account. The default relay answers a payment page, which must be
/// `https` on `polar.sh` or one of its subdomains (anything else is `validation.syncRelayError`:
/// the URL goes to the system opener). A self-hosted relay takes no payments and answers
/// `CheckoutTarget::SelfHosted`, which is not an error: there is nothing to pay, and an own server
/// is set up from a connection code (`connect_own_server`) instead.
pub async fn checkout_url<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    plan: SyncPlan,
) -> SyncResult<CheckoutTarget> {
    let account = stored_account(db).await?;
    match relay.checkout(&account, checkout_plan(plan)).await {
        Ok(url) if is_polar_checkout(&url) => Ok(CheckoutTarget::Open { url }),
        Ok(_) => {
            log::warn!("[SYNC] The relay offered a payment page outside polar.sh: refused");
            Err(refused(RELAY_ERROR))
        }
        Err(RelayError::SelfHosted) => {
            log::info!("[SYNC] The relay is self-hosted: no payment page");
            Ok(CheckoutTarget::SelfHosted)
        }
        Err(error) => Err(error.into()),
    }
}

/// The Recovery Kit while a flow shows it (revision §2.2): its seed, never stored, the vault it
/// is for, when it was made (its certificate's date, so a retried setup signs the same one) and
/// what the person did with it (`KitStorage`, the strongest kept). The command keeps it in the
/// profile's process-global draft for at most `KIT_DRAFT_LIFETIME` and wipes it at lock, exit,
/// profile switch and when the flow ends. `Debug` prints no secret.
pub struct KitDraft {
    secret: KitSecret,
    vault_id: VaultId,
    created_at: i64,
    storage: Option<KitStorage>,
}

impl std::fmt::Debug for KitDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KitDraft")
            .field("created_at", &self.created_at)
            .field("storage", &self.storage)
            .finish_non_exhaustive()
    }
}

impl KitDraft {
    /// A draft of the Kit `secret` of `vault_id`, made at `created_at`.
    pub fn new(vault_id: VaultId, secret: KitSecret, created_at: i64) -> Self {
        KitDraft {
            secret,
            vault_id,
            created_at,
            storage: None,
        }
    }

    /// A Kit a key change made (`rekey::rotate::Outcome`), for the person to see once.
    pub fn from_new_kit(vault_id: VaultId, kit: NewKit) -> Self {
        KitDraft::new(vault_id, kit.secret, kit.created_at)
    }

    /// What the seed derives for the vault.
    pub fn keys(&self) -> KitKeys {
        self.secret.derive(&self.vault_id.0)
    }

    pub fn vault_id(&self) -> &VaultId {
        &self.vault_id
    }

    pub fn created_at(&self) -> i64 {
        self.created_at
    }

    /// Where the person keeps it, as far as this flow saw (`None`: nothing done yet).
    pub fn storage(&self) -> Option<KitStorage> {
        self.storage
    }

    /// The seed (the e2e tests type the Kit's code from it; the app shows it by `kit_view`).
    #[cfg(test)]
    pub(crate) fn secret(&self) -> &KitSecret {
        &self.secret
    }
}

/// The Kit draft for setup (`sync_prepare_recovery_kit`, after the device password was checked):
/// the vault's ID and key are made once (`keys::ensure_vault_secrets`); `draft` is the one still
/// open, reused while it is this vault's, so a person who reopens the step sees the same Kit;
/// otherwise a new Kit. Refused with `validation.syncAlreadyEnabled` once sync is on (a Kit after
/// setup comes from a key change), and `validation.syncSetupIncomplete` without an account.
pub fn prepare_kit(conn: &Connection, draft: Option<KitDraft>, now: i64) -> Result<KitDraft> {
    if enabled(conn)? {
        return Err(AppError::Validation(ALREADY_ENABLED.into()));
    }
    if keys::account(conn)?.is_none() {
        return Err(AppError::Validation(SETUP_INCOMPLETE.into()));
    }
    let (vault_id, _) = keys::ensure_vault_secrets(conn)?;
    match draft {
        Some(draft) if draft.vault_id == vault_id => Ok(draft),
        _ => Ok(KitDraft::new(vault_id, KitSecret::generate()?, now)),
    }
}

/// The Kit as the person sees it (revision §2.1, §2.2): the v2 code in 19 groups of five, its QR
/// and the server address when it is not the default one. The account is inside the code (the
/// wire's `accountNumber` still carries it until the wire task drops it). The view holds the
/// secret: the caller treats it like the code.
pub fn kit_view(conn: &Connection, draft: &KitDraft) -> Result<RecoveryKit> {
    let (relay_url, account) =
        keys::account(conn)?.ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?;
    let code = encode_kit_code(&draft.vault_id, &draft.secret, &account)?;
    let relay_url = (!is_default_relay(&relay_url)).then_some(relay_url);
    let qr_svg = qr_svg(&kit_qr_payload(&code, relay_url.as_deref()))?;
    let grouped = format_recovery_code(&code);
    Ok(RecoveryKit {
        account_number: account,
        recovery_code: grouped.as_str().to_owned(),
        qr_svg,
        relay_url,
    })
}

/// The Kit's code in groups (`kit_view`'s), for copying it or checking a file against it.
pub fn kit_code(conn: &Connection, draft: &KitDraft) -> Result<Zeroizing<String>> {
    let (_, account) =
        keys::account(conn)?.ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?;
    let code = encode_kit_code(&draft.vault_id, &draft.secret, &account)?;
    Ok(format_recovery_code(&code))
}

/// The person printed, copied or saved the Kit (revision §2.7): the draft keeps the strongest
/// value, and so does the Kit's row when it exists already (a Kit a key change made; a setup's
/// row is written with the value at `finish_setup`). Captured, so it syncs.
pub fn note_kit_storage(
    conn: &Connection,
    draft: &mut KitDraft,
    storage: KitStorage,
) -> Result<()> {
    draft.storage = draft.storage.max(Some(storage));
    note_row_storage(conn, draft.keys().id(), storage)
}

/// What "Ověřit sadu" found (revision §2.8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KitCheck {
    /// The Kit is this vault's and `trusted` here.
    Valid,
    /// A newer Kit replaced it at `at` (when this device learnt of it, if known); the valid one
    /// was made at `current_created_at`.
    Replaced {
        at: Option<i64>,
        current_created_at: Option<i64>,
    },
    /// The code is of another vault.
    OtherSync,
    /// This vault has no such Kit (one whose setup never finished, say).
    Unknown,
}

/// "Ověřit sadu" (revision §2.8, owner decision D1): reads a typed code or a scanned QR, with no
/// password and no network; the seed is wiped before it answers. A code that does not read is
/// `validation.syncRecoveryCodeInvalid`; with sync off, `validation.syncSetupIncomplete`.
pub fn check_kit(conn: &Connection, input: &str) -> Result<KitCheck> {
    let creds =
        keys::credentials(conn)?.ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?;
    let input = input.trim();
    let code = if input.starts_with("moony-recover:") {
        parse_kit_qr_payload(input)?.code
    } else {
        decode_kit_code(input)?
    };
    if code.vault_id != creds.vault_id {
        return Ok(KitCheck::OtherSync);
    }
    let keys = code.seed.derive(&code.vault_id.0);
    let (id, sign_pub) = (keys.id(), *keys.sign_pub());
    drop(keys);
    drop(code);
    let entries = trust::entries(conn)?;
    let found = entries
        .iter()
        .find(|e| e.device == id && e.kind == CertKind::Kit && e.sign_pub == sign_pub);
    Ok(match found.map(|e| e.state) {
        Some(TrustState::Trusted) => KitCheck::Valid,
        Some(TrustState::Removed) => KitCheck::Replaced {
            at: found.and_then(|e| e.state_at),
            current_created_at: entries
                .iter()
                .find(|e| e.kind == CertKind::Kit && e.state == TrustState::Trusted)
                .map(|e| e.cert.body.issued_at),
        },
        _ => KitCheck::Unknown,
    })
}

/// What `register_kit` puts on the sync server: the Kit's token, its certificate and the envelope
/// sealed to it. Never printed (the token reads the envelope).
struct KitUpload {
    token: Zeroizing<String>,
    cert: Certificate,
    envelope: Vec<u8>,
}

/// The setup's Kit, sealed (revision §2.2–§2.4): its kind-5 certificate, issued by this device at
/// the draft's time (so a retry signs the same bytes), and the envelope of every key this device
/// holds, its confirmed set this device and the Kit.
fn seal_setup_kit(conn: &Connection, draft: &KitDraft, now: i64) -> Result<KitUpload> {
    let me = state::load(conn)?
        .ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?
        .device;
    let creds =
        keys::credentials(conn)?.ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?;
    if creds.vault_id != draft.vault_id {
        return Err(AppError::Validation(KIT_NOT_CONFIRMED.into()));
    }
    let seed = identity::own_seed(conn)?
        .ok_or_else(|| AppError::Internal("sync setup: no signing key".into()))?;
    let own = trust::entry(conn, me)?
        .ok_or_else(|| AppError::Internal("sync setup: this device is not pinned".into()))?;
    let keys = draft.keys();
    let cert = cert::issue_kit(&seed, me, &keys, draft.created_at)?;
    let confirmed = rekey_kit::confirmed_after(
        &trust::entries(conn)?,
        std::slice::from_ref(&own),
        &[],
        None,
        &cert,
    );
    let envelope = rekey_kit::seal(conn, &creds.keys, &cert, &seed, &own.cert, &confirmed, now)?;
    Ok(KitUpload {
        token: Zeroizing::new(keys.token().to_string()),
        cert,
        envelope,
    })
}

/// Puts the setup's Kit on the sync server and registers it here (revision §2.2, §2.3), after
/// sync is on and this device is pinned: `PUT /kit` first (a retried setup with the same draft
/// replaces the envelope), then, in one savepoint, the Kit's row with where it is kept, its
/// certificate pinned `trusted` and `confirmed` with this device (the envelope's confirmed set).
/// The Kit is registered here only once the server holds it, so this device never seals to a Kit
/// the server does not know; a Kit pinned here already (an earlier try) is not put again. A server that holds another Kit of this vault (`409 kit_exists`: an
/// earlier try registered a draft that has since expired) is left as it is: nothing is
/// registered, and the person makes a new Kit later (As built R-37).
async fn register_kit<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    auth: &keys::VaultAuth,
    draft: &KitDraft,
    now: i64,
) -> SyncResult<()> {
    // Registered by an earlier try (the server held it before it was pinned here, and a copy
    // that re-joined since keeps it): only where it is kept may have grown.
    let id = draft.keys().id();
    let registered = db.with_conn(|conn| {
        let pinned = trust::entry(conn, id)?.is_some();
        if pinned {
            if let Some(storage) = draft.storage {
                note_row_storage(conn, id, storage)?;
            }
        }
        Ok(pinned)
    })?;
    if registered {
        return Ok(());
    }
    let upload = db.with_conn(|conn| seal_setup_kit(conn, draft, now))?;
    match relay.put_kit(auth, &upload.token, &upload.envelope).await {
        Ok(created) => {
            if created {
                log::info!("[SYNC] The Recovery Kit is on the sync server");
            }
        }
        Err(RelayError::KitExists) => {
            log::warn!("[SYNC] The sync server holds another Recovery Kit of this vault; kept");
            return Ok(());
        }
        Err(error) => return Err(error.into()),
    }
    let me = upload.cert.body.issuer_device;
    let storage = draft.storage;
    db.with_conn(|conn| {
        in_savepoint(conn, "sync_setup_kit", || {
            rekey_kit::write_row(conn, &upload.cert, storage, now)?;
            if let Some(storage) = storage {
                note_row_storage(conn, upload.cert.body.device, storage)?;
            }
            trust::pin_created_kit(conn, &upload.cert, now)?;
            trust::mark_confirmed(conn, &[me])
        })
    })?;
    Ok(())
}

/// Writes `storage` into the Kit's row when it is stronger than what the row holds.
fn note_row_storage(conn: &Connection, kit: DeviceId, storage: KitStorage) -> Result<()> {
    if rekey_kit::storage(conn, kit)?.is_some_and(|stored| stored >= storage) {
        return Ok(());
    }
    conn.execute(
        "UPDATE sync_devices SET kit_storage = ?1 WHERE id = ?2",
        rusqlite::params![storage.as_str(), kit.to_hex()],
    )?;
    Ok(())
}

/// Finishes setup on the first device of a new vault. Resumable: a step that already happened is
/// not repeated.
///
/// 1. A Kit the person did nothing with (`kit.storage()` `None`: not printed, copied or saved) is
///    `validation.syncRecoveryKitNotConfirmed`; then the device name is checked.
/// 2. Under the profile's round lock: without the account and the vault's secrets,
///    `validation.syncSetupIncomplete`; with sync on and a snapshot already taken,
///    `validation.syncAlreadyEnabled`. With sync already on (a resumed setup), the engine start's
///    copy check runs first (`runtime::rejoin_if_copy`): a copy re-joins under a new device ID
///    before the first round pushes, and a check that cannot be made stops setup with its
///    error. The stored relay address is checked, a server without the Recovery Kit
///    (`ServerInfo::kit`) is `validation.syncServerNeedsUpdate`, and the relay creates the vault (an existing
///    one of this account and token is fine; `Lapsed` and `VaultLimitReached` come back as they
///    are and read `validation.syncPaymentRequired` and `validation.syncVaultLimitReached` at the
///    command).
/// 3. In one savepoint: sync is turned on for the device the row holds (`binding::enable`, which
///    also clears a stale restore marker), the key pair made once, this device's signing key and
///    genesis certificate made once and pinned (phase 5 §5.3), and this device recorded under
///    `device_name` with that certificate. The device ID does not change, so a written binding stays valid. Then the
///    binding is written when due; a failed seal is logged, and the engine's start tries again
///    (`binding::binding_due`). Then the Kit goes to the server and is registered here
///    (`register_kit`; idempotent).
/// 4. After the lock, a `FirstSnapshot` round (ruling P4): pulls (nothing yet), pushes the rule
///    packs and the device row, pulls them back as own envelopes and uploads the first snapshot at
///    `S` = that cursor. A round that pushed nothing fails setup, whether it failed (offline, a
///    relay error) or ended on a problem (a stopped pull, a lapse, the quota, a refused token:
///    `problem_error` gives the same key as the relay answer would). Sync stays on, and the next
///    call resumes at step 2 (a pause for a vault the relay lost is lifted once it answers the
///    vault's creation). A failure after the push (the vault's info, the second pull, the upload)
///    does not: the upload policy uploads when the vault has no snapshot.
pub async fn finish_setup<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    device_name: &str,
    kit: &KitDraft,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<()> {
    if kit.storage().is_none() {
        return Err(refused(KIT_NOT_CONFIRMED));
    }
    devices::validate_device_name(device_name)?;
    {
        let _no_round = engine::exclusive(db).await;
        let (creds, on) = on_db(db, |conn| {
            let creds = keys::credentials(conn)?
                .ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?;
            let on = enabled(conn)?;
            let snapshot: bool = conn.query_row(
                "SELECT snapshot_seq IS NOT NULL FROM sync_state WHERE id = 1",
                [],
                |r| r.get(0),
            )?;
            if on && snapshot {
                return Err(AppError::Validation(ALREADY_ENABLED.into()));
            }
            Ok((creds, on))
        })
        .await?;
        if on {
            // A resumed setup pushes in its first round: a copy re-joins first, as at the
            // engine's start.
            rejoin_if_copy_under_lock(db, data_dir, native, now)?;
        }
        validate_base_url(&creds.relay_url)?;
        if &creds.vault_id != kit.vault_id() {
            return Err(refused(KIT_NOT_CONFIRMED));
        }
        // The Kit is every vault's (revision §2.9): a server without it is refused first.
        match relay.server_info().await {
            Ok(info) if info.kit => {}
            Ok(_) | Err(RelayError::BadRequest(_)) => return Err(refused(SERVER_NEEDS_UPDATE)),
            Err(error) => return Err(error.into()),
        }
        if relay.create_vault(&creds.account, &creds.auth()).await? {
            log::info!("[SYNC] Created the vault on the relay");
        }

        let (dir, name) = (data_dir.to_path_buf(), device_name.to_string());
        on_db(db, move |conn| turn_on(conn, &dir, &name, !on, now)).await?;
        if !on {
            log::info!("[SYNC] Sync is on for this device");
        }
        write_binding_if_due(db, data_dir, native);
        register_kit(relay, db, &creds.auth(), kit, now).await?;
    }
    first_round(relay, db, now).await
}

/// Step 3 of `finish_setup`.
fn turn_on(conn: &Connection, data_dir: &Path, name: &str, enable: bool, now: i64) -> Result<()> {
    in_savepoint(conn, "sync_finish_setup", || {
        if enable {
            let device = state::load(conn)?
                .ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?
                .device;
            binding::enable(conn, data_dir, device)?;
        }
        let public = keys::ensure_device_keypair(conn)?;
        // The vault's first device vouches for itself (phase 5 §5.3); a resumed setup keeps the
        // certificate it signed the first time.
        let cert = identity::create_own(conn, Issuer::Genesis, now)?;
        devices::record_this_device(conn, name, &public, Some(&cert), now)?;
        // The relay has just created or confirmed the vault: a pause from a first round that found
        // it gone (`vault_gone_at`) no longer holds, or the retry's round would not run.
        state::set_vault_gone(conn, None)
    })
}

/// Writes the binding when it is due; a failure is logged (`binding_due` retries at the next
/// start).
fn write_binding_if_due<D: ConnAccess>(db: &D, data_dir: &Path, native: &dyn BindingNative) {
    let written = db.with_conn(|conn| {
        if binding::binding_due(conn)? {
            binding::write_binding(conn, data_dir, native)?;
        }
        Ok(())
    });
    if let Err(error) = written {
        log::warn!("[SYNC] The device binding could not be written at setup");
        log::debug!("[SYNC] The device binding could not be written at setup: {error}");
    }
}

/// Step 4 of `finish_setup`.
async fn first_round<R: Relay, D: ConnAccess>(relay: &R, db: &D, now: i64) -> SyncResult<()> {
    let mut policy = SnapshotPolicy::default();
    let mut report = RoundReport::default();
    let round = engine::run_round_into(
        relay,
        db,
        &mut policy,
        RoundMode::FirstSnapshot,
        now,
        0,
        &mut report,
    )
    .await;
    match round {
        Ok(()) if report.push.is_none() && report.problem.is_some() => {
            // Nothing was pushed: setup is not done. Sync stays on, so a retry resumes here.
            let problem = report.problem.unwrap_or(SyncProblem::RelayError);
            log::info!("[SYNC] Setup's first round pushed nothing: {problem:?}");
            Err(problem_error(problem))
        }
        Ok(()) => {
            if let Some(problem) = report.problem {
                log::info!("[SYNC] Setup's first round ended with {problem:?}");
            }
            if !report.snapshot_uploaded {
                log::info!("[SYNC] Setup's first snapshot is not uploaded yet; the policy will");
            }
            Ok(())
        }
        Err(error) if report.push.is_some() => {
            log::info!("[SYNC] Setup's first snapshot failed; the policy uploads it later");
            log::debug!("[SYNC] Setup's first snapshot failed: {error}");
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// The error a first round that pushed nothing ends setup with: the `SyncError` class the problem
/// came from, so a command reads the same key as for that relay answer (`Lapsed` →
/// `validation.syncPaymentRequired`). A stopped pull, which no relay answer stands for, reads as
/// a relay error.
fn problem_error(problem: SyncProblem) -> SyncError {
    match problem {
        SyncProblem::Lapsed => SyncError::Lapsed,
        SyncProblem::VaultGone => SyncError::VaultNotFound,
        SyncProblem::QuotaExceeded => SyncError::TooLarge(TooLarge::Quota),
        SyncProblem::SnapshotTooLarge => SyncError::TooLarge(TooLarge::Snapshot),
        SyncProblem::KeyRotated
        | SyncProblem::KeyNotGiven
        | SyncProblem::KeyChangeUnverified
        | SyncProblem::KeyChangeWaiting => SyncError::ReadOnlyToken,
        SyncProblem::Unauthorized => SyncError::Unauthorized,
        SyncProblem::RateLimited => SyncError::RateLimited,
        SyncProblem::Offline => SyncError::Transient(Transient::Offline),
        SyncProblem::CaptureBroken
        | SyncProblem::NewerSchema
        | SyncProblem::Gap
        | SyncProblem::ClockAhead
        | SyncProblem::Undecryptable
        | SyncProblem::InvalidChain
        | SyncProblem::MissingParent
        | SyncProblem::Quarantined
        | SyncProblem::RelayError
        | SyncProblem::SecureStorage
        | SyncProblem::ServerWentBack
        | SyncProblem::Internal => SyncError::Transient(Transient::Relay),
    }
}

/// Turns sync off on this device (spec §11.6): the capture triggers go, and `sync_state` (the
/// relay address, the account, the vault's keys, this device's key pair), the clocks, the outbox
/// and the chain bookkeeping are deleted (`state::disable`), and so is the device list
/// (`sync_devices`: sync metadata of the old vault, not finance data; a later setup into a new
/// vault would otherwise publish the old vault's devices and their public keys), in one
/// transaction; then the incoming blobs of cut-off downloads are swept
/// (`sweep_unfinished_downloads`, best effort) and the binding file and the restore marker are
/// removed (`binding::forget_binding`). Local data stays, and so does the vault on the
/// relay for the other devices. Repair backups (`sync-repair-*.zip`) are copies of local data and
/// stay until Delete Account.
///
/// Turning sync off is the way out of a profile whose capture cannot be installed (ADR 0027): the
/// guard (`capture::guard`) and a read-only session (`query_only`) are lifted before the
/// savepoint, so local data is writable again once sync is off; a failure puts back whichever of
/// them the profile had.
pub fn turn_off(conn: &Connection, data_dir: &Path) -> Result<()> {
    turn_off_keeping_binding_file(conn)?;
    binding::forget_binding(data_dir)?;
    log::info!("[SYNC] Sync is off on this device; local data stays");
    Ok(())
}

/// `turn_off` without removing the binding file and the restore marker: for a caller inside a
/// round that does not know the data folder (a removed device, phase 5 §7.4), which leaves them to
/// the engine's runtime (`binding::forget_binding`).
pub(crate) fn turn_off_keeping_binding_file(conn: &Connection) -> Result<()> {
    let was_guarded = guard::is_imposed(conn);
    let was_read_only: bool = conn.pragma_query_value(None, "query_only", |r| r.get(0))?;
    let off = guard::lift(conn)
        .and_then(|()| Ok(conn.execute_batch("PRAGMA query_only = 0")?))
        .and_then(|()| {
            in_savepoint(conn, "sync_turn_off", || {
                // `disable` drops the capture first, so deleting the device list queues nothing.
                state::disable(conn)?;
                conn.execute("DELETE FROM sync_devices", [])?;
                Ok(())
            })
        });
    if let Err(error) = off {
        if was_guarded {
            super::guard_or_close(conn, guard::impose);
        }
        if was_read_only {
            if let Err(pragma) = conn.execute_batch("PRAGMA query_only = 1") {
                log::error!("[SYNC] Could not make the profile read-only again: {pragma}");
            }
        }
        return Err(error);
    }
    truncate_wal(conn);
    sweep_unfinished_downloads(conn);
    Ok(())
}

/// After sync went off: the incoming blobs of downloads that were cut off go
/// (`attachment_store::sweep_incomplete`). The lanes sweep them when they start, which never
/// happens again with sync off; each is invisible but as large as its file. No transfer runs
/// now (the engine was stopped before the change). Best effort: a failure is logged and sync
/// stays off.
fn sweep_unfinished_downloads(conn: &Connection) {
    match files_schema::is_attached(conn) {
        Ok(true) => match attachment_store::sweep_incomplete(conn) {
            Ok(0) => {}
            Ok(swept) => {
                log::info!("[SYNC] Removed {swept} unfinished downloads of documents or photos")
            }
            Err(e) => log::warn!("[SYNC] Unfinished downloads could not be removed: {e}"),
        },
        Ok(false) => {}
        Err(e) => log::warn!("[SYNC] Unfinished downloads could not be looked for: {e}"),
    }
}

/// After the sync secrets were deleted (`secure_delete` zeroes their cells, `db::configure`): in
/// WAL mode the log still holds the pages as they were, so they are checkpointed into the
/// database file and the log is truncated to nothing (audit M11). Profiles use a rollback journal,
/// where this is a no-op; best effort, since sync is already off.
fn truncate_wal(conn: &Connection) {
    let mode = conn.pragma_query_value(None, "journal_mode", |r| r.get::<_, String>(0));
    match mode {
        Ok(mode) if mode.eq_ignore_ascii_case("wal") => {
            match conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
                r.get::<_, i64>(0)
            }) {
                Ok(0) => {}
                Ok(_) => log::warn!("[SYNC] The write-ahead log could not be truncated: busy"),
                Err(e) => log::warn!("[SYNC] The write-ahead log could not be truncated: {e}"),
            }
        }
        Ok(_) => {}
        Err(e) => log::warn!("[SYNC] Could not read the journal mode: {e}"),
    }
}

/// `turn_off` once no round of the profile runs (`engine::exclusive`), off the runtime: what the
/// commands call.
pub async fn turn_off_between_rounds<D: ConnAccess>(db: &D, data_dir: &Path) -> SyncResult<()> {
    let _no_round = engine::exclusive(db).await;
    let dir = data_dir.to_path_buf();
    on_db(db, move |conn| turn_off(conn, &dir)).await
}

/// "Delete synced data" (spec §11.6; the command asks for the device password first): under the
/// round lock, the relay deletes the vault (the stored credentials hold its token, so this comes
/// first; a vault the relay no longer knows counts as deleted), then `turn_off`. Any other answer
/// is returned and nothing changes here.
pub async fn delete_synced_data<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    data_dir: &Path,
) -> SyncResult<()> {
    let _no_round = engine::exclusive(db).await;
    if let Some(creds) = on_db(db, keys::credentials).await? {
        delete_vault(relay, &creds).await?;
        log::info!("[SYNC] Deleted the synced data on the relay");
    }
    let dir: PathBuf = data_dir.to_path_buf();
    on_db(db, move |conn| turn_off(conn, &dir)).await
}

/// Delete Account's attempt to delete the vault too (spec §11.6), within `DELETE_BUDGET`
/// (waiting for a running round included). `true` when no vault of this profile is left on the
/// relay as far as this device knows: deleted now, already gone, or never made (no credentials).
/// `false` on any failure or when the budget ran out; the profile is not changed either way.
pub async fn delete_vault_best_effort<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> bool {
    let attempt = async {
        let _no_round = engine::exclusive(db).await;
        match on_db(db, keys::credentials).await? {
            Some(creds) => delete_vault(relay, &creds).await,
            None => Ok(()),
        }
    };
    match tokio::time::timeout(DELETE_BUDGET, attempt).await {
        Ok(Ok(())) => true,
        Ok(Err(error)) => {
            log::warn!("[SYNC] The synced data could not be deleted with the account");
            log::debug!("[SYNC] The synced data could not be deleted with the account: {error}");
            false
        }
        Err(_) => {
            log::warn!("[SYNC] The relay did not delete the synced data in time");
            false
        }
    }
}

/// Signs this device's `left` record and writes it into its own row, which the capture queues
/// (phase 5 plan Task 11, revision Task 20). `false` when there is nothing to write: sync is
/// off, the device has no signing key (a vault from before phase 5) or no row, or the row holds
/// a `left` record already (the column is write-once; a turn-off that failed after the first
/// leave keeps it).
pub(crate) fn write_left_record(conn: &Connection, now: i64) -> Result<bool> {
    let Some(sync) = state::load(conn)?.filter(|sync| sync.enabled) else {
        return Ok(false);
    };
    let (Some(creds), Some(seed)) = (keys::credentials(conn)?, identity::own_seed(conn)?) else {
        return Ok(false);
    };
    let record = identity::Record::sign(
        identity::RecordType::Left,
        creds.vault_id.0,
        sync.device,
        sync.device,
        now,
        creds.keys.current_epoch(),
        &seed,
    )?;
    let written = conn.execute(
        "UPDATE sync_devices SET left_record = ?1 WHERE id = ?2 AND left_record IS NULL",
        rusqlite::params![record.to_b64(), sync.device.to_hex()],
    )?;
    Ok(written == 1)
}

/// A device that turns sync off or is deleted says so (phase 5 plan Task 11): under the round
/// lock it signs a `left` record, writes it into its row and sends it in one push round without
/// a pull, all within `budget` (waiting for a running round counts). Every failure, and running
/// out of time, is ignored: the person's turn-off never waits for the server, and a device that
/// could not say it left is simply cut off at the next key change. Nothing happens when sync is
/// off (a device another one removed signs nothing: its key went with its sync state). What was
/// queued before, a pairing's ticket announcement included, goes out first, since the push
/// sends envelopes in sequence order: a holder's announcement always reaches the server before
/// its `left` record (revision §3.1).
pub async fn leave_best_effort<R: Relay, D: ConnAccess>(relay: &R, db: &D, budget: Duration) {
    let attempt = async {
        let _no_round = engine::exclusive(db).await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        if !on_db(db, move |conn| write_left_record(conn, now)).await? {
            return Ok(());
        }
        let Some(creds) = on_db(db, keys::credentials).await? else {
            return Ok(());
        };
        engine::push::push_all(relay, db, &creds).await.map(|_| ())
    };
    match tokio::time::timeout(budget, attempt).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => {
            log::info!("[SYNC] This device could not tell the others that it left");
            log::debug!("[SYNC] The leave record was not sent: {error}");
        }
        Err(_) => log::info!("[SYNC] The sync server did not take the leave record in time"),
    }
}

/// `DELETE /v1/vaults/:id` at the credentials' own relay address; `VaultNotFound` is done.
async fn delete_vault<R: Relay>(relay: &R, creds: &Credentials) -> SyncResult<()> {
    validate_base_url(&creds.relay_url)?;
    match relay.delete_vault(&creds.auth()).await {
        Ok(()) | Err(RelayError::VaultNotFound) => Ok(()),
        Err(error) => Err(error.into()),
    }
}

/// The stored account number, or `validation.syncSetupIncomplete`.
async fn stored_account<D: ConnAccess>(db: &D) -> SyncResult<String> {
    on_db(db, |conn| {
        keys::account(conn)?
            .map(|(_, account)| account)
            .ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))
    })
    .await
}

async fn status_of<R: Relay>(relay: &R, account: String) -> SyncResult<SyncAccount> {
    let status = relay.account_status(&account).await?;
    Ok(SyncAccount {
        account_number: account,
        active: status.active,
        paid_until: status.paid_until,
    })
}

fn checkout_plan(plan: SyncPlan) -> CheckoutPlan {
    match plan {
        SyncPlan::Monthly => CheckoutPlan::Monthly,
        SyncPlan::Yearly => CheckoutPlan::Yearly,
    }
}

/// An `https` page on `polar.sh` or a subdomain of it, on the default port, without user info.
fn is_polar_checkout(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url) else {
        return false;
    };
    let polar = parsed
        .host_str()
        .is_some_and(|host| host == "polar.sh" || host.ends_with(".polar.sh"));
    parsed.scheme() == "https"
        && polar
        && parsed.port().is_none()
        && parsed.username().is_empty()
        && parsed.password().is_none()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A vault key nobody else writes: its bytes must be gone from the files after turning off.
    const MARKER: &str = "M11-VAULT-KEY-MARKER-5a3c9e17d2b84f60a1e7c3d95b2f8e46";

    fn contains(haystack: &[u8], needle: &str) -> bool {
        haystack
            .windows(needle.len())
            .any(|window| window == needle.as_bytes())
    }

    fn read_or_empty(path: &Path) -> Vec<u8> {
        std::fs::read(path).unwrap_or_default()
    }

    /// An unkeyed file database (its pages readable as plain bytes), configured and migrated like
    /// a profile, with an account and vault keys holding `MARKER`.
    fn profile_with_marked_keys(path: &Path, wal: bool) -> Connection {
        let conn = Connection::open(path).expect("open");
        crate::db::configure(&conn).expect("configure");
        if wal {
            let mode: String = conn
                .query_row("PRAGMA journal_mode = WAL", [], |r| r.get(0))
                .expect("wal");
            assert_eq!(mode, "wal");
        }
        crate::db::run_migrations(&conn).expect("migrations");
        keys::save_account(&conn, "https://sync.example.com", "1234567812345678").expect("account");
        conn.execute(
            "UPDATE sync_state SET vault_id = 'v', key_epoch = 1, vault_keys = ?1 WHERE id = 1",
            [MARKER],
        )
        .expect("marked keys");
        conn
    }

    /// The profile here is also what an unfinished setup leaves (sync off, the account and the
    /// vault keys stored): "Cancel setup" is `sync_turn_off` without a password, which runs this.
    #[test]
    fn turning_sync_off_leaves_no_key_bytes_in_a_plain_database_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let conn = profile_with_marked_keys(&path, false);
        assert!(
            contains(&read_or_empty(&path), MARKER),
            "the key is on disk before"
        );

        turn_off(&conn, dir.path()).expect("turn off");

        assert!(state::load(&conn).expect("state").is_none());
        assert!(keys::account(&conn).expect("account").is_none());
        assert!(!contains(&read_or_empty(&path), MARKER));
    }

    #[test]
    fn turning_sync_off_in_wal_mode_leaves_no_key_bytes_in_the_file_or_its_log() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let wal = dir.path().join("moony.db-wal");
        let conn = profile_with_marked_keys(&path, true);
        assert!(
            contains(&read_or_empty(&path), MARKER) || contains(&read_or_empty(&wal), MARKER),
            "the key is on disk before"
        );

        turn_off(&conn, dir.path()).expect("turn off");

        assert!(!contains(&read_or_empty(&path), MARKER), "database file");
        assert!(!contains(&read_or_empty(&wal), MARKER), "write-ahead log");
    }

    fn incomplete_blobs(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM files.blobs WHERE sha256 = ''",
            [],
            |r| r.get(0),
        )
        .expect("count")
    }

    /// A download cancelled before sync went off leaves an incoming blob (invisible, but as large
    /// as the file); with sync off no lane starts again to sweep it, so turning off sweeps it.
    #[test]
    fn turning_sync_off_removes_unfinished_downloads_and_keeps_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::services::sync::test_support::migrated();
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        crate::db::files_schema::migrate(&conn).expect("files schema");
        state::enable(&conn, crate::services::sync::hlc::DeviceId([7; 16])).expect("enable");
        let kept = attachment_store::put_bytes(&conn, b"kept")
            .expect("blob")
            .id;
        attachment_store::begin_incoming(&conn, "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60", 1024)
            .expect("left over");
        assert_eq!(incomplete_blobs(&conn), 1);

        turn_off(&conn, dir.path()).expect("turn off");

        assert_eq!(incomplete_blobs(&conn), 0);
        assert_eq!(
            attachment_store::read_all(&conn, &kept).expect("kept"),
            b"kept".to_vec()
        );
    }

    /// Turning off works without the attachment store (it is swept at the next turn-off or lane
    /// start of a profile that has it).
    #[test]
    fn turning_sync_off_without_the_store_still_turns_off() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = crate::services::sync::test_support::migrated();
        state::enable(&conn, crate::services::sync::hlc::DeviceId([7; 16])).expect("enable");

        turn_off(&conn, dir.path()).expect("turn off");

        assert!(!state::on_or_unreadable(&conn).expect("state"));
    }
}
