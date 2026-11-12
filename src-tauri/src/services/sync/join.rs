//! Joining a vault by replace (spec §8, §11.3 D22), a new device joining from pairing or a Sync
//! Recovery Kit (phase 3), repairing a profile whose change capture broke (ruling P3) and
//! re-syncing a device from the vault (ruling T8-4). All of them replace the person's local data
//! with the vault's, so none may lose it silently: a join is one savepoint that leaves the
//! profile as it was when any step fails; the new device's join, the repair and the re-sync
//! write a backup and read it back before anything else; and the repair replaces nothing unless
//! every change this device captured reached the relay first.
//!
//! The relay passed in is the one at the credentials' own address (`Credentials::relay_url`, the
//! default or a self-hosted one, ruling SH-2): the caller builds it with
//! `HttpRelay::new(&creds.relay_url)`, never from `default_base_url()` (`MOONY_RELAY_URL`).

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::binding::{self, BindingNative};
use super::capture::{self, guard};
use super::devices;
use super::engine::pull::DEFAULT_DEVICE_NAME;
use super::engine::runtime::{rejoin_if_copy, CopyCheck};
use super::engine::snapshots::{off_thread, open_sealed, SnapshotPolicy};
use super::engine::{self, ConnAccess, PullReport, RoundMode};
use super::error::SyncResult;
use super::hlc::DeviceId;
use super::in_savepoint;
use super::keys::{self, Credentials, VaultKeys};
use super::recovery::RecoveryInput;
use super::relay::http::{normalize_base_url, validate_base_url};
use super::relay::{Relay, RelayError};
use super::snapshot::RestoreReport;
use super::state;
use crate::error::{AppError, Result};
use crate::services::backup;
use crate::services::crypto;

/// The repair's backups, next to the profile: `sync-repair-<unix>.zip` (`-<n>` added when a backup
/// of that second exists). They stay until Delete Account (`auth::remove_account_files`).
pub const REPAIR_BACKUP_PREFIX: &str = "sync-repair-";
pub const REPAIR_BACKUP_SUFFIX: &str = ".zip";

const NO_SNAPSHOT: &str = "validation.syncVaultHasNoSnapshot";
const ALREADY_ENABLED: &str = "validation.syncAlreadyEnabled";
const SETUP_PENDING: &str = "validation.syncJoinSetupPending";
const KIT_ACCOUNT_INVALID: &str = "validation.syncRecoveryAccountInvalid";
const KIT_VAULT_NOT_FOUND: &str = "validation.syncRecoveryVaultNotFound";
const KIT_ROTATED: &str = "validation.syncRecoveryKitRotated";
const DRAIN_INCOMPLETE: &str = "validation.syncRepairDrainIncomplete";
const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// This device joins the vault of `creds` by replacing its data with the vault's newest snapshot
/// (D22; the caller has made the backup). Holds off every round of the profile
/// (`engine::exclusive`) for the whole join.
///
/// 1. The device name and the relay address are checked before the relay is asked.
/// 2. The snapshot is fetched (none → `validation.syncVaultHasNoSnapshot`) and opened with
///    `open_sealed` (size-capped, authenticated under its sequence `S`) off the runtime, without
///    the connection lock.
/// 3. One savepoint, in this order: the credentials are bound (binding another vault forgets the
///    old one's relay positions); this device's chain starts over and the pauses and counters of
///    the old binding go (`binding_written` among them); `binding::join_leaving_marker` restores
///    the snapshot by replace under a new device ID (the cursor, heads and snapshot bookkeeping
///    become the snapshot's: the cursor is exactly `S`) and installs capture; a new key pair is
///    made and this device recorded with it under `device_name`, queued for the next push. Any
///    failure rolls all of it back and reinstalls the capture that ran before (its clock seeded
///    from this profile's own clocks), so the profile is as it was; the restore marker is left
///    alone.
/// 4. Only after the commit is a leftover restore marker cleared (`binding::join_leaving_marker`;
///    the new device ID has nothing to re-join) and the binding written for the new device ID; a
///    failure of either is logged (an unwritten binding is not checked at the next start).
pub async fn join_by_replace<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: Credentials,
    device_name: &str,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<RestoreReport> {
    devices::validate_device_name(device_name)?;
    validate_base_url(&creds.relay_url)?;
    let _no_round = engine::exclusive(db).await;
    replace_under_lock(relay, db, creds, device_name, data_dir, native, now).await
}

/// Steps 2 to 4 of `join_by_replace`, for a caller that holds off every round
/// (`engine::exclusive`) and has checked the device name and the relay address.
async fn replace_under_lock<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: Credentials,
    device_name: &str,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<RestoreReport> {
    let blob = match relay.get_snapshot(&creds.auth()).await {
        Ok(blob) => blob,
        Err(RelayError::NoSnapshot) => {
            return Err(AppError::Validation(NO_SNAPSHOT.into()).into());
        }
        Err(error) => return Err(error.into()),
    };
    let opener = creds.clone();
    let plain = off_thread(move || open_sealed(&opener, &blob)).await?;

    let (joiner, dir, name) = (db.clone(), data_dir.to_path_buf(), device_name.to_string());
    let report = off_thread(move || {
        joiner.with_conn(|conn| replace_profile(conn, &dir, &creds, &name, &plain, now))
    })
    .await?;
    log::info!("[SYNC] This device joined the vault under a new device ID");

    if let Err(error) = db.with_conn(|conn| binding::write_binding(conn, data_dir, native)) {
        log::warn!("[SYNC] The device binding could not be written after the join");
        log::debug!("[SYNC] The device binding could not be written after the join: {error}");
    }
    Ok(report)
}

/// A new device joins the vault of `creds` (phase 3: credentials received by pairing, or made
/// from a Sync Recovery Kit by `recovery_credentials`); returns the path of the backup. The
/// caller passes a copy of the credentials and keeps its own, so "Try again" needs no new
/// pairing (review PF6).
///
/// 1. Before any backup or relay call: the device name and the relay address are checked; then,
///    holding off every round of the profile (`engine::exclusive`) until the join ends, sync must
///    be off (`validation.syncAlreadyEnabled`), and a sync setup this device started with another
///    account or on another sync server (`keys::account`) stops the join with
///    `validation.syncJoinSetupPending` (review PF5); a setup that stopped at the same account on
///    the same server may go on into the join.
/// 2. The verified backup of the repair (`sync-repair-<now>.zip`, flushed and read back).
/// 3. `join_by_replace`'s replace: the snapshot is fetched and opened, and in one savepoint the
///    credentials are bound, the profile replaced under a new device ID with a new key pair and
///    this device recorded under `device_name`; then the binding is written. A failure leaves
///    the profile as it was; the backup stays.
pub async fn join_new_device<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: Credentials,
    device_name: &str,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<PathBuf> {
    devices::validate_device_name(device_name)?;
    validate_base_url(&creds.relay_url)?;
    let _no_round = engine::exclusive(db).await;
    db.with_conn(|conn| may_join(conn, &creds))?;
    let zip = backed_up(db, data_dir, now).await?;
    replace_under_lock(relay, db, creds, device_name, data_dir, native, now).await?;
    log::info!("[SYNC] A new device joined the vault");
    Ok(zip)
}

/// Step 1 of `join_new_device`: sync is off, and no setup with another account, or the same
/// account on another sync server, is under way. Both addresses compare normalized; a stored one
/// that does not read as an address counts as another server.
fn may_join(conn: &Connection, creds: &Credentials) -> Result<()> {
    if state::load(conn)?.is_some_and(|sync| sync.enabled) {
        return Err(AppError::Validation(ALREADY_ENABLED.into()));
    }
    if let Some((stored_url, stored_account)) = keys::account(conn)? {
        let same_server = match (
            normalize_base_url(&stored_url),
            normalize_base_url(&creds.relay_url),
        ) {
            (Ok(stored), Ok(joined)) => stored == joined,
            _ => false,
        };
        if !same_server || stored_account != creds.account {
            return Err(AppError::Validation(SETUP_PENDING.into()));
        }
    }
    Ok(())
}

/// The credentials of a Sync Recovery Kit (review PF7), from the relay at `input.relay_url` (the
/// caller builds it from that address). In this order:
///
/// 1. Sync on → `validation.syncAlreadyEnabled`, before any relay call.
/// 2. The relay must know the account (`account_status`; `AccountNotFound` →
///    `validation.syncRecoveryAccountInvalid`).
/// 3. The kit's key is the vault's key of epoch 0; the vault must answer its token (`vault_info`;
///    `Unauthorized` or `VaultNotFound` → `validation.syncRecoveryVaultNotFound`).
/// 4. A vault whose token was rotated (`rotation != 0`) is refused with
///    `validation.syncRecoveryKitRotated`: a kit holds the first key only, and finding a later
///    epoch is phase 5.
///
/// Any other relay answer comes back as its `SyncError`. Nothing is stored.
pub async fn recovery_credentials<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    input: RecoveryInput,
) -> SyncResult<Credentials> {
    let enabled = db.with_conn(|conn| Ok(state::load(conn)?.is_some_and(|sync| sync.enabled)))?;
    if enabled {
        return Err(AppError::Validation(ALREADY_ENABLED.into()).into());
    }
    match relay.account_status(&input.account).await {
        Ok(_) => {}
        Err(RelayError::AccountNotFound) => {
            return Err(AppError::Validation(KIT_ACCOUNT_INVALID.into()).into());
        }
        Err(error) => return Err(error.into()),
    }
    let creds = Credentials {
        relay_url: input.relay_url,
        account: input.account,
        vault_id: input.vault_id,
        keys: VaultKeys::single(0, input.key.into_secret()),
    };
    let info = match relay.vault_info(&creds.auth()).await {
        Ok(info) => info,
        Err(RelayError::Unauthorized | RelayError::VaultNotFound) => {
            return Err(AppError::Validation(KIT_VAULT_NOT_FOUND.into()).into());
        }
        Err(error) => return Err(error.into()),
    };
    if info.rotation != 0 {
        log::info!("[SYNC] The vault was re-keyed; a Recovery Kit join is not supported yet");
        return Err(AppError::Validation(KIT_ROTATED.into()).into());
    }
    Ok(creds)
}

/// Step 3 of `join_by_replace`.
fn replace_profile(
    conn: &Connection,
    data_dir: &Path,
    creds: &Credentials,
    device_name: &str,
    plain: &[u8],
    now: i64,
) -> Result<RestoreReport> {
    // The device whose capture runs on this connection now, if any, and whether the guard of a
    // capture-broken profile is on (ADR 0027): a failed join puts back whichever it was.
    let installed = if capture::is_installed(conn)? {
        state::load(conn)?.map(|sync| sync.device)
    } else {
        None
    };
    let was_guarded = guard::is_imposed(conn);
    // The guard is a connection setting no rollback undoes, so it is lifted outside the
    // savepoint (the replace writes synced tables) and imposed again on every failure.
    let joined = guard::lift(conn).and_then(|()| {
        in_savepoint(conn, "sync_join_by_replace", || {
            keys::save_credentials(conn, creds)?;
            state::reset_chain(conn)?;
            state::reset_engine_marks(conn)?;
            let report =
                binding::join_leaving_marker(conn, DeviceId::random(), &mut &plain[..], now)?;
            let public = keys::new_device_keypair(conn)?;
            devices::record_this_device(conn, device_name, &public, now)?;
            Ok(report)
        })
    });
    let joined = match joined {
        Ok(report) => report,
        Err(error) => {
            // The rollback restored the old triggers, but not the clock functions the join's
            // capture install registered: they were seeded from the snapshot's clocks. Installing
            // again seeds them from this profile's own `sync_clock`; failing that, the guard.
            if let Some(device) = installed {
                if let Err(e) = capture::install_or_guard(conn, device) {
                    log::warn!("[SYNC] Could not reinstall capture after a failed join: {e}");
                }
            } else if was_guarded {
                super::guard_or_close(conn, guard::impose);
            }
            return Err(error);
        }
    };
    // Only now that the join has committed: the new device ID has nothing to re-join.
    if let Err(e) = binding::clear_rejoin_marker(data_dir) {
        log::warn!("[SYNC] Could not clear the restore marker after the join: {e}");
    }
    Ok(joined)
}

/// Repairs a profile whose change capture broke (ruling P3); returns the path of the backup.
///
/// A read-only session (`refuse_read_only`) is `validation.syncCaptureStillBroken` before step 0.
///
/// 0. Holding off every round, the engine start's copy check (`runtime::rejoin_if_copy`): a
///    restored or copied profile whose engine start did not re-join (its check failed) re-joins
///    under a new device ID now, so the drain never pushes under the original device's ID. A
///    check that cannot be made (an unreadable binding) stops the repair with that error, before
///    the backup.
/// 1. A backup of the whole profile goes to `data_dir/sync-repair-<now>.zip` (never over an
///    earlier one), flushed to disk and read back to its last byte. Nothing goes on without it.
/// 2. Capture is installed again on the connection (whatever broke it at unlock may have passed):
///    the drain's merge needs it. When it still fails, the repair stops with
///    `validation.syncCaptureStillBroken` (its message points to Turn off sync); nothing but the
///    backup has changed, and the guard stays (ADR 0027). Once capture is installed the guard is
///    lifted, before the drain: every change from here on is captured.
/// 3. A drain round (`RoundMode::Drain`) pulls, and pushes what capture recorded before it broke.
/// 4. Only a clean drain goes on: the round ended without an error or a problem, its pull reached
///    the relay's end (`PullReport::done`), and `sync_outbox` and `sync_unsent` are empty. Anything
///    else is `validation.syncRepairDrainIncomplete`, with nothing replaced (the backup stays).
/// 5. `join_by_replace` with the stored credentials (their relay address included) under this
///    device's current name; its fresh engine marks clear the capture mark.
///
/// The backup is never deleted here; it stays until Delete Account.
pub async fn repair_capture<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<PathBuf> {
    refuse_read_only(db)?;
    rejoin_if_copy_between_rounds(db, data_dir, native, now).await?;
    let (creds, name) = stored_identity(db)?;
    let zip = backed_up(db, data_dir, now).await?;

    let reinstalled = db.with_conn(|conn| {
        let device = state::load(conn)?
            .ok_or_else(|| AppError::Internal("sync repair: sync is not set up".into()))?
            .device;
        Ok(capture::install(conn, device))
    })?;
    if let Err(error) = reinstalled {
        // The guard stays: nothing may change uncaptured. Turning sync off is the way on.
        log::warn!("[SYNC] The repair stopped: change capture still does not install");
        log::debug!("[SYNC] The repair stopped: change capture still does not install: {error}");
        return Err(AppError::Validation(guard::CAPTURE_STILL_BROKEN.into()).into());
    }
    // Capture records every change again, so the guard goes before the drain (whose merge writes
    // synced tables); from here on a failure leaves capture installed.
    db.with_conn(guard::lift)?;
    if !drained(relay, db, now).await? {
        log::warn!("[SYNC] The repair stopped: this device's changes did not all reach the relay");
        return Err(AppError::Validation(DRAIN_INCOMPLETE.into()).into());
    }
    join_by_replace(relay, db, creds, &name, data_dir, native, now).await?;
    // `join_by_replace` cleared the mark with the other engine marks; this only makes sure, and a
    // failure never hides the backup of a repair that is done.
    if let Err(error) = db.with_conn(state::clear_capture_broken) {
        log::warn!("[SYNC] Could not clear the capture mark after the repair");
        log::debug!("[SYNC] Could not clear the capture mark after the repair: {error}");
    }
    log::info!("[SYNC] The repair re-joined the vault; change capture is back");
    Ok(zip)
}

/// "Re-sync this device" (ruling T8-4): the way out for a device whose sync history is
/// inconsistent, or whose drain cannot finish. Returns the path of the backup.
///
/// The same verified backup as `repair_capture`, then `join_by_replace` with the stored
/// credentials under this device's current name, **without a drain**: changes this device has not
/// sent (`sync_outbox`, `sync_unsent`) are not pushed and survive only in the backup. The UI must
/// say so and have the person confirm before it calls this.
pub async fn resync_by_replace<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<PathBuf> {
    refuse_read_only(db)?;
    let (creds, name) = stored_identity(db)?;
    let zip = backed_up(db, data_dir, now).await?;
    join_by_replace(relay, db, creds, &name, data_dir, native, now).await?;
    log::info!("[SYNC] This device re-synced from the vault's snapshot");
    Ok(zip)
}

/// `runtime::rejoin_if_copy` once no round of the profile runs (`engine::exclusive`), for a
/// person's action that pushes next (the repair's drain, a resumed setup's first round). After a
/// re-join the binding stays unwritten until the next binding write (the join that ends the
/// repair, setup's `write_binding_if_due`, or the engine's next start: `binding::binding_due`).
pub(super) async fn rejoin_if_copy_between_rounds<D: ConnAccess>(
    db: &D,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<()> {
    let _no_round = engine::exclusive(db).await;
    rejoin_if_copy_under_lock(db, data_dir, native, now)
}

/// `runtime::rejoin_if_copy` for a caller that already holds off every round.
pub(super) fn rejoin_if_copy_under_lock<D: ConnAccess>(
    db: &D,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<()> {
    match rejoin_if_copy(db, data_dir, native, now) {
        Ok(CopyCheck::Fine) => Ok(()),
        Ok(CopyCheck::Rejoined) => {
            log::info!("[SYNC] This profile re-joined under a new device ID before pushing");
            Ok(())
        }
        Err(failure) => {
            log::warn!("[SYNC] The copy check before pushing failed; nothing is pushed");
            log::debug!("[SYNC] The copy check before pushing failed: {failure:?}");
            Err(failure.into_app_error().into())
        }
    }
}

/// A read-only session (`query_only`, the last resort when the guard could not be imposed: ADR
/// 0027) can write nothing, a backup included, and capture needs temp writes to install: the
/// repair and the re-sync answer `validation.syncCaptureStillBroken` (turning sync off is the way
/// on) before they try anything.
fn refuse_read_only<D: ConnAccess>(db: &D) -> SyncResult<()> {
    if db.with_conn(|conn| Ok(guard::is_read_only(conn)))? {
        log::warn!("[SYNC] The profile is read-only: change capture cannot be repaired");
        return Err(AppError::Validation(guard::CAPTURE_STILL_BROKEN.into()).into());
    }
    Ok(())
}

/// The stored credentials of an enabled profile and this device's current name.
fn stored_identity<D: ConnAccess>(db: &D) -> SyncResult<(Credentials, String)> {
    Ok(db.with_conn(|conn| {
        let enabled = state::load(conn)?.is_some_and(|sync| sync.enabled);
        let creds = keys::credentials(conn)?
            .filter(|_| enabled)
            .ok_or_else(|| AppError::Internal("sync repair: sync is not set up".into()))?;
        let name = devices::this_device(conn)?
            .map_or_else(|| DEFAULT_DEVICE_NAME.to_string(), |(name, _)| name);
        Ok((creds, name))
    })?)
}

/// The verified backup (`back_up`), off the runtime.
async fn backed_up<D: ConnAccess>(db: &D, data_dir: &Path, now: i64) -> SyncResult<PathBuf> {
    let (backer, dir) = (db.clone(), data_dir.to_path_buf());
    let zip = off_thread(move || back_up(&backer, &dir, now)).await?;
    log::info!("[SYNC] Backed this profile up before replacing it");
    Ok(zip)
}

/// Steps 3 and 4 of `repair_capture`: whether the drain round ended clean.
async fn drained<R: Relay, D: ConnAccess>(relay: &R, db: &D, now: i64) -> SyncResult<bool> {
    let mut policy = SnapshotPolicy::default();
    let clean = match engine::run_round(relay, db, &mut policy, RoundMode::Drain, now, 0).await {
        Ok(report) => {
            if let Some(problem) = report.problem {
                log::info!("[SYNC] The repair's drain round ended with {problem:?}");
            }
            report.problem.is_none() && report.pull.as_ref().is_some_and(PullReport::done)
        }
        Err(error) => {
            log::info!("[SYNC] The repair's drain round failed");
            log::debug!("[SYNC] The repair's drain round failed: {error}");
            false
        }
    };
    if !clean {
        return Ok(false);
    }
    let pending: i64 = db.with_conn(|conn| {
        Ok(conn.query_row(
            "SELECT (SELECT COUNT(*) FROM sync_outbox) + (SELECT COUNT(*) FROM sync_unsent)",
            [],
            |r| r.get(0),
        )?)
    })?;
    Ok(pending == 0)
}

/// Step 1 of `repair_capture` and `resync_by_replace`, step 2 of `join_new_device`: writes the
/// backup to a free name, flushes it and reads it back.
fn back_up<D: ConnAccess>(db: &D, data_dir: &Path, now: i64) -> Result<PathBuf> {
    let dest = free_backup_path(data_dir, now)?;
    db.with_conn(|conn| backup::create_backup(conn, data_dir, &dest, APP_VERSION))?;
    File::open(&dest)?.sync_all()?;
    crypto::sync_parent_dir(&dest)?;
    read_back(&dest)?;
    Ok(dest)
}

/// `sync-repair-<now>.zip`, or `sync-repair-<now>-<n>.zip` with the first `n` from 2 that is free.
fn free_backup_path(data_dir: &Path, now: i64) -> Result<PathBuf> {
    let first = data_dir.join(format!("{REPAIR_BACKUP_PREFIX}{now}{REPAIR_BACKUP_SUFFIX}"));
    if !first.try_exists()? {
        return Ok(first);
    }
    for n in 2u32.. {
        let next = data_dir.join(format!(
            "{REPAIR_BACKUP_PREFIX}{now}-{n}{REPAIR_BACKUP_SUFFIX}"
        ));
        if !next.try_exists()? {
            return Ok(next);
        }
    }
    Err(AppError::Internal(
        "sync repair: no free backup name".into(),
    ))
}

/// The archive is one the app restores (`backup::inspect_backup`: its layout, its manifest, every
/// listed file present, not newer than this build), and every entry reads to its end, so the zip
/// checks each one's CRC.
fn read_back(path: &Path) -> Result<()> {
    let unreadable = || AppError::Internal("sync repair: the backup does not read back".into());
    let inspection = backup::inspect_backup(path, &crate::db::known_migration_names())?;
    if inspection.newer_than_app {
        return Err(unreadable());
    }
    let mut archive = zip::ZipArchive::new(File::open(path)?).map_err(|_| unreadable())?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|_| unreadable())?;
        io::copy(&mut entry, &mut io::sink()).map_err(|_| unreadable())?;
    }
    Ok(())
}
