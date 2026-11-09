//! First-device setup (spec §11.5, ruling P4), turning sync off and deleting synced data (§11.6),
//! on the relay the person chose (ruling SH-2: the default relay or a self-hosted one).
//!
//! Setup runs in steps the UI drives: `create_account` (stores the relay address and the account
//! number), `account_status` and `checkout_url` until the account is paid, `recovery_kit` (makes
//! the vault's ID and key once and shows them), then `finish_setup` once the person saved the kit.
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
//! `&dyn` that cannot move to the blocking pool (as in `join::join_by_replace`). Each is at most
//! one keychain or DPAPI open or seal of 32 bytes, a small file read or write and, for a copy,
//! the re-join's one savepoint. (`block_in_place` would need a multi-thread runtime; the tests run
//! on tokio's current-thread one.)
//!
//! `finish_setup`, `delete_synced_data` and `turn_off_between_rounds` hold off the profile's
//! rounds (`engine::exclusive`) while they change what a round works on.

use std::path::{Path, PathBuf};
use std::time::Duration;

use reqwest::Url;
use rusqlite::Connection;

use super::binding::{self, BindingNative};
use super::devices;
use super::engine::snapshots::{off_thread, SnapshotPolicy};
use super::engine::{self, ConnAccess, RoundMode, RoundReport};
use super::error::{SyncError, SyncResult, TooLarge, Transient};
use super::in_savepoint;
use super::join::rejoin_if_copy_under_lock;
use super::keys::{self, Credentials};
use super::recovery::{encode_recovery_code, format_recovery_code, qr_svg, recovery_qr_payload};
use super::relay::http::{is_default_relay, normalize_base_url, validate_base_url};
use super::relay::{CheckoutPlan, Relay, RelayError};
use super::state;
use crate::error::{AppError, Result};
use crate::models::sync::{CheckoutTarget, RecoveryKit, SyncAccount, SyncPlan, SyncProblem};

const KIT_NOT_CONFIRMED: &str = "validation.syncRecoveryKitNotConfirmed";
const SETUP_INCOMPLETE: &str = "validation.syncSetupIncomplete";
const ALREADY_ENABLED: &str = "validation.syncAlreadyEnabled";
const RELAY_ERROR: &str = "validation.syncRelayError";

/// How long Delete Account waits for the relay to delete the vault.
pub const DELETE_BUDGET: Duration = Duration::from_secs(5);

fn refused(key: &str) -> SyncError {
    SyncError::Local(AppError::Validation(key.into()))
}

/// `work` on the profile's connection, off the runtime.
async fn on_db<D: ConnAccess, T: Send + 'static>(
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

/// The stored account as the relay sees it now; `validation.syncSetupIncomplete` without one. The
/// UI polls this until the account is active (paid, or added to a self-hosted relay's
/// `ALLOWED_ACCOUNTS`).
pub async fn account_status<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> SyncResult<SyncAccount> {
    let account = stored_account(db).await?;
    status_of(relay, account).await
}

/// Where to pay for the stored account. The default relay answers a payment page, which must be
/// `https` on `polar.sh` or one of its subdomains (anything else is `validation.syncRelayError`:
/// the URL goes to the system opener). A self-hosted relay takes no payments and answers
/// `CheckoutTarget::SelfHosted`, which is not an error: the UI shows the account number to add to
/// the relay's `ALLOWED_ACCOUNTS` and polls `account_status`.
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

/// The Sync Recovery Kit of the stored account: the vault's ID and key are made the first time
/// (`keys::ensure_vault_secrets`) and reused after, so every call shows the same code. The relay
/// address is in it only when it is not the default relay. Without an account,
/// `validation.syncSetupIncomplete`.
pub fn recovery_kit(conn: &Connection) -> Result<RecoveryKit> {
    let (relay_url, account) =
        keys::account(conn)?.ok_or_else(|| AppError::Validation(SETUP_INCOMPLETE.into()))?;
    let (vault_id, vault_keys) = keys::ensure_vault_secrets(conn)?;
    let code = encode_recovery_code(&vault_id, vault_keys.current());
    let relay_url = (!is_default_relay(&relay_url)).then_some(relay_url);
    let payload = recovery_qr_payload(&account, &code, relay_url.as_deref());
    let qr_svg = qr_svg(&payload)?;
    let grouped = format_recovery_code(&code);
    Ok(RecoveryKit {
        account_number: account,
        recovery_code: grouped.as_str().to_owned(),
        qr_svg,
        relay_url,
    })
}

/// The kit while setup runs (`sync_prepare_recovery_kit`, no password): `recovery_kit`, refused
/// with `validation.syncAlreadyEnabled` once sync is on. After setup the kit is shown only once
/// the device password was checked (`sync_show_recovery_kit`).
pub fn setup_recovery_kit(conn: &Connection) -> Result<RecoveryKit> {
    if enabled(conn)? {
        return Err(AppError::Validation(ALREADY_ENABLED.into()));
    }
    recovery_kit(conn)
}

/// Finishes setup on the first device of a new vault. Resumable: a step that already happened is
/// not repeated.
///
/// 1. `kit_saved` false is `validation.syncRecoveryKitNotConfirmed` (the UI's confirmation is
///    trusted: the person ticked it after a save or a copy); then the device name is checked.
/// 2. Under the profile's round lock: without the account and the vault's secrets,
///    `validation.syncSetupIncomplete`; with sync on and a snapshot already taken,
///    `validation.syncAlreadyEnabled`. With sync already on (a resumed setup), the engine start's
///    copy check runs first (`runtime::rejoin_if_copy`): a copy re-joins under a new device ID
///    before the first round pushes, and a check that cannot be made stops setup with its
///    error. The stored relay address is checked, and the relay creates the vault (an existing
///    one of this account and token is fine; `Lapsed` and `VaultLimitReached` come back as they
///    are and read `validation.syncPaymentRequired` and `validation.syncVaultLimitReached` at the
///    command).
/// 3. In one savepoint: sync is turned on for the device the row holds (`binding::enable`, which
///    also clears a stale restore marker), the key pair made once, and this device recorded under
///    `device_name`. The device ID does not change, so a written binding stays valid. Then the
///    binding is written when due; a failed seal is logged, and the engine's start tries again
///    (`binding::binding_due`).
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
    kit_saved: bool,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<()> {
    if !kit_saved {
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
        if relay.create_vault(&creds.account, &creds.auth()).await? {
            log::info!("[SYNC] Created the vault on the relay");
        }

        let (dir, name) = (data_dir.to_path_buf(), device_name.to_string());
        on_db(db, move |conn| turn_on(conn, &dir, &name, !on, now)).await?;
        if !on {
            log::info!("[SYNC] Sync is on for this device");
        }
        write_binding_if_due(db, data_dir, native);
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
        devices::record_this_device(conn, name, &public, now)?;
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
        SyncProblem::KeyRotated => SyncError::ReadOnlyToken,
        SyncProblem::Unauthorized => SyncError::Unauthorized,
        SyncProblem::RateLimited => SyncError::RateLimited,
        SyncProblem::Offline => SyncError::Transient(Transient::Offline),
        SyncProblem::CaptureBroken
        | SyncProblem::NewerSchema
        | SyncProblem::Gap
        | SyncProblem::Undecryptable
        | SyncProblem::InvalidChain
        | SyncProblem::MissingParent
        | SyncProblem::Quarantined
        | SyncProblem::RelayError
        | SyncProblem::Internal => SyncError::Transient(Transient::Relay),
    }
}

/// Turns sync off on this device (spec §11.6): the capture triggers go, and `sync_state` (the
/// relay address, the account, the vault's keys, this device's key pair), the clocks, the outbox
/// and the chain bookkeeping are deleted (`state::disable`), and so is the device list
/// (`sync_devices`: sync metadata of the old vault, not finance data; a later setup into a new
/// vault would otherwise publish the old vault's devices and their public keys), in one
/// transaction; then the binding file and the restore marker are removed
/// (`binding::forget_binding`). Local data stays, and so does the vault on the
/// relay for the other devices. Repair backups (`sync-repair-*.zip`) are copies of local data and
/// stay until Delete Account.
pub fn turn_off(conn: &Connection, data_dir: &Path) -> Result<()> {
    in_savepoint(conn, "sync_turn_off", || {
        // `disable` drops the capture first, so deleting the device list queues nothing.
        state::disable(conn)?;
        conn.execute("DELETE FROM sync_devices", [])?;
        Ok(())
    })?;
    binding::forget_binding(data_dir)?;
    log::info!("[SYNC] Sync is off on this device; local data stays");
    Ok(())
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
