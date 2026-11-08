//! Joining a vault by replace (spec §8, §11.3 D22) and repairing a profile whose change capture
//! broke (ruling P3). Both replace the person's local data with the vault's, so neither may lose
//! it silently: a join is one savepoint that leaves the profile as it was when any step fails, and
//! the repair writes a backup and reads it back before anything else, and replaces nothing unless
//! every change this device captured reached the relay first.
//!
//! The relay passed in is the one at the credentials' own address (`Credentials::relay_url`, the
//! default or a self-hosted one, ruling SH-2): the caller builds it with
//! `HttpRelay::new(&creds.relay_url)`, never from `default_base_url()` (`MOONY_RELAY_URL`).

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::binding::{self, BindingNative, REJOIN_MARKER};
use super::devices;
use super::engine::pull::DEFAULT_DEVICE_NAME;
use super::engine::snapshots::{off_thread, open_sealed, SnapshotPolicy};
use super::engine::{self, ConnAccess, PullReport, RoundMode};
use super::error::SyncResult;
use super::hlc::DeviceId;
use super::in_savepoint;
use super::keys::{self, Credentials};
use super::relay::http::validate_base_url;
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
///    the old binding go (`binding_written` among them); `binding::join` restores the snapshot by
///    replace under a new device ID (the cursor, heads and snapshot bookkeeping become the
///    snapshot's: the cursor is exactly `S`), installs capture and clears a leftover restore
///    marker; a new key pair is made and this device recorded with it under `device_name`, queued
///    for the next push. Any failure rolls all of it back, and a restore marker the join removed
///    is put back, so the profile is as it was.
/// 4. Only then is the binding written for the new device ID; a failure there is logged (the
///    binding stays unwritten, so the next start does not check it).
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

/// Step 3 of `join_by_replace`.
fn replace_profile(
    conn: &Connection,
    data_dir: &Path,
    creds: &Credentials,
    device_name: &str,
    plain: &[u8],
    now: i64,
) -> Result<RestoreReport> {
    let had_marker = data_dir.join(REJOIN_MARKER).try_exists()?;
    let joined = in_savepoint(conn, "sync_join_by_replace", || {
        keys::save_credentials(conn, creds)?;
        state::reset_chain(conn)?;
        state::reset_engine_marks(conn)?;
        let report = binding::join(conn, data_dir, DeviceId::random(), &mut &plain[..], now)?;
        let public = keys::new_device_keypair(conn)?;
        devices::record_this_device(conn, device_name, &public, now)?;
        Ok(report)
    });
    if joined.is_err() && had_marker {
        // `binding::join` clears the marker before the savepoint ends; the old profile still
        // needs it.
        if let Err(e) = binding::write_rejoin_marker(data_dir) {
            log::warn!("[SYNC] Could not put the restore marker back after a failed join: {e}");
        }
    }
    joined
}

/// Repairs a profile whose change capture broke (ruling P3); returns the path of the backup.
///
/// 1. A backup of the whole profile goes to `data_dir/sync-repair-<now>.zip` (never over an
///    earlier one), flushed to disk and read back to its last byte. Nothing goes on without it.
/// 2. A drain round (`RoundMode::Drain`) pulls, and pushes what capture recorded before it broke.
/// 3. Only a clean drain goes on: the round ended without an error or a problem, its pull reached
///    the relay's end (`PullReport::done`), and `sync_outbox` and `sync_unsent` are empty. Anything
///    else is `validation.syncRepairDrainIncomplete`, with nothing replaced (the backup stays).
/// 4. `join_by_replace` with the stored credentials (their relay address included) under this
///    device's current name, then the capture mark is cleared.
///
/// The backup is never deleted here; it stays until Delete Account.
pub async fn repair_capture<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<PathBuf> {
    let (creds, name) = db.with_conn(|conn| {
        let enabled = state::load(conn)?.is_some_and(|sync| sync.enabled);
        let creds = keys::credentials(conn)?
            .filter(|_| enabled)
            .ok_or_else(|| AppError::Internal("sync repair: sync is not set up".into()))?;
        let name = devices::this_device(conn)?
            .map_or_else(|| DEFAULT_DEVICE_NAME.to_string(), |(name, _)| name);
        Ok((creds, name))
    })?;

    let (backer, dir) = (db.clone(), data_dir.to_path_buf());
    let zip = off_thread(move || back_up(&backer, &dir, now)).await?;
    log::info!("[SYNC] The repair backed this profile up");

    if !drained(relay, db, now).await? {
        log::warn!("[SYNC] The repair stopped: this device's changes did not all reach the relay");
        return Err(AppError::Validation(DRAIN_INCOMPLETE.into()).into());
    }
    join_by_replace(relay, db, creds, &name, data_dir, native, now).await?;
    db.with_conn(state::clear_capture_broken)?;
    log::info!("[SYNC] The repair re-joined the vault; change capture is back");
    Ok(zip)
}

/// Steps 2 and 3 of `repair_capture`: whether the drain round ended clean.
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

/// Step 1 of `repair_capture`: writes the backup to a free name, flushes it and reads it back.
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
