//! Joining a vault by replace (spec §8, §11.3 D22), a new device joining from pairing or a Sync
//! Recovery Kit (phase 3), repairing a profile whose change capture broke (ruling P3) and
//! re-syncing a device from the vault (ruling T8-4). All of them replace the person's local data
//! with the vault's, so none may lose it silently:
//!
//! - the replace is one savepoint that leaves the profile as it was when any step fails;
//! - the snapshot is fetched and opened first, then one connection hold checks that nothing
//!   changed since the person agreed (`impact::check_consent`), writes the verified backup and
//!   replaces, so no write can slip in between the backup and the replace (audit M8);
//! - the repair replaces nothing unless every change this device captured reached the sync server
//!   first, and the re-sync sends what it can before it replaces;
//! - each returns a `ReplaceOutcome` with what stayed only in the backup. The backup is marked
//!   `-kept` inside the hold, before the replace commits, so a crash before the outcome is known
//!   never leaves it prunable. After the outcome a join's and a repair's backup stay kept (the
//!   only copy of what a join replaced; what a capture-broken session may have written before
//!   the guard existed), a re-sync's settles to plain or stays kept (unsent changes, documents
//!   or photos found nowhere else), and the backup of a replace that never happened becomes
//!   `-unused`.
//!
//! The relay passed in is the one at the credentials' own address (`Credentials::relay_url`, the
//! default or a self-hosted one, ruling SH-2): the caller builds it with
//! `HttpRelay::new(&creds.relay_url)`, never from `default_base_url()` (`MOONY_RELAY_URL`).

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use zeroize::Zeroizing;

use super::backups::{self, BackupMark};
use super::binding::{self, BindingNative};
use super::capture::{self, guard};
use super::devices;
use super::engine::pull::DEFAULT_DEVICE_NAME;
use super::engine::push;
use super::engine::runtime::{rejoin_if_copy, CopyCheck};
use super::engine::snapshots::{off_thread, open_sealed, SnapshotPolicy};
use super::engine::{self, ConnAccess, PullEnd, PullReport, RoundMode};
use super::error::SyncResult;
use super::hlc::DeviceId;
use super::impact::{self, ReplaceConsent, ReplaceKind, DRAIN_INCOMPLETE};
use super::in_savepoint;
use super::keys::{self, Credentials, VaultKeys};
use super::recovery::RecoveryInput;
use super::relay::http::{normalize_base_url, validate_base_url};
use super::relay::{Relay, RelayError};
use super::snapshot::{AttachmentCarry, RestoreReport};
use super::state;
use crate::error::{AppError, Result};
use crate::models::sync::SyncProblem;

pub use super::backups::{PREFIX as REPAIR_BACKUP_PREFIX, SUFFIX as REPAIR_BACKUP_SUFFIX};

const NO_SNAPSHOT: &str = "validation.syncVaultHasNoSnapshot";
const ALREADY_ENABLED: &str = "validation.syncAlreadyEnabled";
const SETUP_PENDING: &str = "validation.syncJoinSetupPending";
const KIT_ACCOUNT_INVALID: &str = "validation.syncRecoveryAccountInvalid";
const KIT_VAULT_NOT_FOUND: &str = "validation.syncRecoveryVaultNotFound";
const KIT_ROTATED: &str = "validation.syncRecoveryKitRotated";

/// Drain rounds a repair runs at most; it also stops after two rounds in a row that neither shrank
/// `pending_items + pending_batches` nor moved the pull cursor on.
pub const MAX_DRAIN_ROUNDS: usize = 20;

/// What a join, repair or re-sync did: its backup (with the mark it settled to), this device's
/// documents and photos it carried over or left only in the backup, and the changes of this
/// device that reached no sync server and stay only in the backup, items (changed rows not sealed
/// yet) and batches (sealed envelopes) apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplaceOutcome {
    pub backup: PathBuf,
    pub attachments: AttachmentCarry,
    pub unsent_items_in_backup_only: u32,
    pub unsent_batches_in_backup_only: u32,
    /// The backup is `-kept`: never pruned, only the person deletes it.
    pub kept: bool,
}

/// The replace without a backup: tests and the e2e harness only. Fetches and opens the snapshot,
/// then `replace_profile` and the binding, holding off every round.
#[cfg(test)]
pub(crate) async fn join_by_replace<R: Relay, D: ConnAccess>(
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
    let plain = fetched_snapshot(relay, &creds).await?;
    let (joiner, dir, name) = (db.clone(), data_dir.to_path_buf(), device_name.to_string());
    let report = off_thread(move || {
        joiner.with_conn(|conn| replace_profile(conn, &dir, &creds, &name, &plain, now))
    })
    .await?;
    write_binding_after_replace(db, data_dir, native);
    Ok(report)
}

/// The vault's newest snapshot (none → `validation.syncVaultHasNoSnapshot`), opened with
/// `open_sealed` (size-capped, authenticated under its sequence) off the runtime, without the
/// connection lock.
async fn fetched_snapshot<R: Relay>(
    relay: &R,
    creds: &Credentials,
) -> SyncResult<Zeroizing<Vec<u8>>> {
    let blob = match relay.get_snapshot(&creds.auth()).await {
        Ok(blob) => blob,
        Err(RelayError::NoSnapshot) => {
            return Err(AppError::Validation(NO_SNAPSHOT.into()).into());
        }
        Err(error) => return Err(error.into()),
    };
    let opener = creds.clone();
    off_thread(move || open_sealed(&opener, &blob)).await
}

/// After the replace committed: the binding is written for the new device ID; a failure is logged
/// (an unwritten binding is not checked at the next start).
fn write_binding_after_replace<D: ConnAccess>(db: &D, data_dir: &Path, native: &dyn BindingNative) {
    if let Err(error) = db.with_conn(|conn| binding::write_binding(conn, data_dir, native)) {
        log::warn!("[SYNC] The device binding could not be written after the join");
        log::debug!("[SYNC] The device binding could not be written after the join: {error}");
    }
}

/// The backup a replace hold works with.
enum HoldBackup {
    /// A join or re-sync: the hold writes it (`backups::write_verified`) once consent holds.
    Write,
    /// A repair: the backup it made before its drain.
    Made(PathBuf),
}

/// What a hold's replace left to report.
struct Replaced {
    report: RestoreReport,
    unsent_items: u32,
    unsent_batches: u32,
    /// A join or a repair: its backup stays Kept whatever the counts (controller ruling).
    always_kept: bool,
}

/// One replace hold: the backup it wrote or used (marked Kept once it is about to be replaced
/// over), and the replace's result.
struct Hold {
    backup: Option<PathBuf>,
    replaced: Result<Replaced>,
}

/// The inputs of a replace hold.
struct HoldInput {
    kind: ReplaceKind,
    consent: ReplaceConsent,
    backup: HoldBackup,
    data_dir: PathBuf,
    creds: Credentials,
    device_name: String,
    plain: Zeroizing<Vec<u8>>,
    now: i64,
}

/// One connection hold, in this order, so nothing can be written between the backup and the
/// replace (audit M8): a join checks again that it may join (`may_join`); the consent check
/// (`impact::check_consent`: a refusal writes no backup); a re-sync reads what stays only in the
/// backup (`pending_items`, `pending_batches`); the verified backup is written (a repair's was
/// made before its drain); the backup is marked Kept (a crash from here on never leaves it
/// prunable); then `replace_profile`.
fn replace_hold(conn: &Connection, input: &HoldInput) -> Hold {
    let mut backup = match &input.backup {
        HoldBackup::Write => None,
        HoldBackup::Made(path) => Some(path.clone()),
    };
    let replaced = (|| {
        if input.kind == ReplaceKind::Join {
            may_join(conn, &input.creds)?;
        }
        impact::check_consent(conn, input.kind, &input.consent)?;
        let (unsent_items, unsent_batches) = if input.kind == ReplaceKind::Resync {
            let impact = impact::replace_impact(conn)?;
            (impact.pending_items, impact.pending_batches)
        } else {
            (0, 0)
        };
        let path = match backup.take() {
            Some(path) => path,
            None => {
                let path = backups::write_verified(conn, &input.data_dir, input.now)?;
                log::info!("[SYNC] Backed this profile up before replacing it");
                path
            }
        };
        let kept = backups::mark(&path, BackupMark::Kept);
        backup = Some(kept.as_ref().map_or(path, Clone::clone));
        kept?;
        let report = replace_profile(
            conn,
            &input.data_dir,
            &input.creds,
            &input.device_name,
            &input.plain,
            input.now,
        )?;
        Ok(Replaced {
            report,
            unsent_items,
            unsent_batches,
            always_kept: input.kind != ReplaceKind::Resync,
        })
    })();
    Hold { backup, replaced }
}

/// `replace_hold` off the runtime.
async fn held<D: ConnAccess>(db: &D, input: HoldInput) -> SyncResult<Hold> {
    let holder = db.clone();
    off_thread(move || holder.with_conn(|conn| Ok(replace_hold(conn, &input)))).await
}

/// The mark a backup settles to after its replace, with whether it is Kept: Kept for a join and
/// a repair, and for a re-sync when anything stayed only in it (unsent changes, documents or
/// photos not re-attached), else Plain. A failure is logged, and the backup stays Kept.
fn settled(backup: PathBuf, replaced: &Replaced) -> (PathBuf, bool) {
    let only_in_backup = replaced.unsent_items > 0
        || replaced.unsent_batches > 0
        || replaced.report.attachments.orphaned > 0
        || replaced.always_kept;
    let mark = if only_in_backup {
        BackupMark::Kept
    } else {
        BackupMark::Plain
    };
    match backups::mark(&backup, mark) {
        Ok(path) => (path, mark == BackupMark::Kept),
        Err(error) => {
            log::warn!("[SYNC] A sync backup could not be marked after its replace: {error}");
            (backup, true)
        }
    }
}

/// A backup whose replace never happened becomes Unused; a failure is logged.
fn mark_unused(backup: &Path) {
    if let Err(error) = backups::mark(backup, BackupMark::Unused) {
        log::warn!("[SYNC] A sync backup could not be marked unused: {error}");
    }
}

/// A join's or re-sync's hold, settled: the outcome after a replace; the backup it wrote marked
/// Unused and the error returned when the replace did not happen.
fn outcome_of(hold: Hold) -> SyncResult<ReplaceOutcome> {
    match (hold.replaced, hold.backup) {
        (Ok(replaced), Some(backup)) => Ok(outcome(settled(backup, &replaced), &replaced)),
        (Ok(_), None) => Err(AppError::Internal("sync replace: no backup".into()).into()),
        (Err(error), backup) => {
            if let Some(backup) = backup {
                mark_unused(&backup);
            }
            Err(error.into())
        }
    }
}

fn outcome((backup, kept): (PathBuf, bool), replaced: &Replaced) -> ReplaceOutcome {
    ReplaceOutcome {
        backup,
        attachments: replaced.report.attachments,
        unsent_items_in_backup_only: replaced.unsent_items,
        unsent_batches_in_backup_only: replaced.unsent_batches,
        kept,
    }
}

/// A new device joins the vault of `creds` (phase 3: credentials received by pairing, or made
/// from a Sync Recovery Kit by `recovery_credentials`). The caller passes a copy of the
/// credentials and keeps its own, so "Try again" needs no new pairing (review PF6).
///
/// 1. Before any backup or relay call: the device name and the relay address are checked; then,
///    holding off every round of the profile (`engine::exclusive`) until the join ends, sync must
///    be off (`validation.syncAlreadyEnabled`), and a sync setup this device started with another
///    account or on another sync server (`keys::account`) stops the join with
///    `validation.syncJoinSetupPending` (review PF5); a setup that stopped at the same account on
///    the same server may go on into the join.
/// 2. The snapshot is fetched and opened (`fetched_snapshot`).
/// 3. One connection hold (`replace_hold`): step 1's check again, documents and photos other
///    than the person agreed to (`consent.attachments`) or a watermark past this profile's newest
///    clock → `validation.syncReplaceImpactChanged`, the verified backup marked kept
///    (`sync-repair-<now>-kept.zip`) before the replace commits, then `replace_profile`: in one
///    savepoint the credentials are bound, the profile replaced under a new device ID with a new
///    key pair and this device recorded under `device_name`. A failure leaves the profile as it
///    was and the backup, if one was written, `-unused`.
/// 4. The backup stays `-kept`: it is the only copy of what the join replaced. The binding is
///    written.
#[allow(clippy::too_many_arguments)]
pub async fn join_new_device<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: Credentials,
    device_name: &str,
    consent: ReplaceConsent,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<ReplaceOutcome> {
    devices::validate_device_name(device_name)?;
    validate_base_url(&creds.relay_url)?;
    let _no_round = engine::exclusive(db).await;
    db.with_conn(|conn| may_join(conn, &creds))?;
    let plain = fetched_snapshot(relay, &creds).await?;
    let hold = held(
        db,
        HoldInput {
            kind: ReplaceKind::Join,
            consent,
            backup: HoldBackup::Write,
            data_dir: data_dir.to_path_buf(),
            creds,
            device_name: device_name.to_string(),
            plain,
            now,
        },
    )
    .await?;
    let outcome = outcome_of(hold)?;
    log::info!("[SYNC] A new device joined the vault under a new device ID");
    write_binding_after_replace(db, data_dir, native);
    Ok(outcome)
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

/// The replace itself, in one savepoint: the credentials are bound (binding another vault
/// forgets the old one's relay positions); this device's chain starts over and the pauses and
/// counters of the old binding go (`binding_written` among them); `binding::join_leaving_marker`
/// restores the snapshot by replace under a new device ID (the cursor, heads and snapshot
/// bookkeeping become the snapshot's) and installs capture; a new key pair is made and this device
/// recorded with it under `device_name`, queued for the next push. Any failure rolls all of it
/// back and reinstalls the capture (or the guard) that ran before. Only after the commit is a
/// leftover restore marker cleared.
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

/// Repairs a profile whose change capture broke (ruling P3).
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
///    It holds what a capture-broken session may have written uncaptured before the guard existed
///    (audit correction 2).
/// 2. Capture is installed again on the connection (whatever broke it at unlock may have passed):
///    the drain's merge needs it. When it still fails, the repair stops with
///    `validation.syncCaptureStillBroken` (its message points to Turn off sync), and the guard
///    stays (ADR 0027). Once capture is installed the guard is lifted, before the drain: every
///    change from here on is captured.
/// 3. Drain rounds (`RoundMode::Drain`) pull, and push what capture recorded, until one ends clean:
///    no error or problem, a pull that reached the relay's end, and `sync_outbox` and
///    `sync_unsent` empty. At most `MAX_DRAIN_ROUNDS` in all, and two rounds in a row that neither
///    shrink what is pending nor move the pull cursor on (a device several pages behind catches up
///    a page or more per round) end it: `validation.syncRepairDrainIncomplete`, nothing replaced.
/// 4. Holding off every round, the snapshot is fetched and opened, then one connection hold: a
///    change written since the drain (`impact::check_consent`, `ReplaceKind::Repair`) sends the
///    repair back to step 3 within the same budget; else the backup is marked and
///    `replace_profile` replaces with the stored credentials under this device's current name;
///    its fresh engine marks clear the capture mark.
///
/// A repair that ends without a replace leaves its backup `-unused`; after a replace it stays
/// `-kept` (it may hold what a capture-broken session wrote before the guard existed). It is never
/// deleted here.
pub async fn repair_capture<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<ReplaceOutcome> {
    refuse_read_only(db)?;
    rejoin_if_copy_between_rounds(db, data_dir, native, now).await?;
    let (creds, name) = stored_identity(db)?;
    let mut backup = backed_up(db, data_dir, now).await?;
    let replaced =
        match repair_after_backup(relay, db, &creds, &name, &mut backup, data_dir, now).await {
            Ok(replaced) => replaced,
            Err(error) => {
                mark_unused(&backup);
                return Err(error);
            }
        };
    let outcome = outcome(settled(backup, &replaced), &replaced);
    // `replace_profile` cleared the mark with the other engine marks; this only makes sure, and a
    // failure never hides the backup of a repair that is done.
    if let Err(error) = db.with_conn(state::clear_capture_broken) {
        log::warn!("[SYNC] Could not clear the capture mark after the repair");
        log::debug!("[SYNC] Could not clear the capture mark after the repair: {error}");
    }
    log::info!("[SYNC] The repair re-joined the vault; change capture is back");
    write_binding_after_replace(db, data_dir, native);
    Ok(outcome)
}

/// Steps 2 to 4 of `repair_capture`; `backup` follows the pre-drain backup's renames.
async fn repair_after_backup<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    name: &str,
    backup: &mut PathBuf,
    data_dir: &Path,
    now: i64,
) -> SyncResult<Replaced> {
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
    let mut rounds = 0;
    loop {
        if !drained(relay, db, &mut rounds, now).await? {
            log::warn!(
                "[SYNC] The repair stopped: this device's changes did not all reach the relay"
            );
            return Err(AppError::Validation(DRAIN_INCOMPLETE.into()).into());
        }
        let _no_round = engine::exclusive(db).await;
        let plain = fetched_snapshot(relay, creds).await?;
        let hold = held(
            db,
            HoldInput {
                kind: ReplaceKind::Repair,
                consent: ReplaceConsent::default(),
                backup: HoldBackup::Made(backup.clone()),
                data_dir: data_dir.to_path_buf(),
                creds: creds.clone(),
                device_name: name.to_string(),
                plain,
                now,
            },
        )
        .await?;
        if let Some(path) = hold.backup {
            *backup = path;
        }
        match hold.replaced {
            Ok(replaced) => return Ok(replaced),
            Err(AppError::Validation(key)) if key == DRAIN_INCOMPLETE => {
                log::info!("[SYNC] A change arrived after the repair's drain: it drains again");
            }
            Err(error) => return Err(error.into()),
        }
    }
}

/// "Re-sync this device" (ruling T8-4): the way out for a device whose sync history is
/// inconsistent, or whose drain cannot finish. The person agreed to what stays only in the backup
/// (`consent`); a change made after that stops it with `validation.syncReplaceImpactChanged`.
///
/// Holding off every round: the copy check (`rejoin_if_copy_under_lock`, so nothing is pushed
/// under a copied profile's original ID); a best-effort `push::push_all` of what this device has
/// not sent (a failure is logged; the hold decides); the snapshot is fetched and opened; then one
/// connection hold checks the consent, counts what is still unsent, writes the verified backup and
/// replaces (`replace_hold`). Unsent changes survive only in the backup, which then stays `-kept`;
/// the outcome says how many.
pub async fn resync_by_replace<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    consent: ReplaceConsent,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<ReplaceOutcome> {
    refuse_read_only(db)?;
    let (creds, name) = stored_identity(db)?;
    let _no_round = engine::exclusive(db).await;
    rejoin_if_copy_under_lock(db, data_dir, native, now)?;
    match push::push_all(relay, db, &creds).await {
        Ok(report) if report.pushed > 0 => {
            log::info!(
                "[SYNC] The re-sync sent {} envelopes before replacing",
                report.pushed
            );
        }
        Ok(_) => {}
        Err(error) => {
            log::warn!("[SYNC] The re-sync could not send this device's changes first");
            log::debug!("[SYNC] The re-sync could not send this device's changes first: {error}");
        }
    }
    let plain = fetched_snapshot(relay, &creds).await?;
    let hold = held(
        db,
        HoldInput {
            kind: ReplaceKind::Resync,
            consent,
            backup: HoldBackup::Write,
            data_dir: data_dir.to_path_buf(),
            creds,
            device_name: name,
            plain,
            now,
        },
    )
    .await?;
    let outcome = outcome_of(hold)?;
    log::info!("[SYNC] This device re-synced from the vault's snapshot");
    write_binding_after_replace(db, data_dir, native);
    Ok(outcome)
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

/// The repair's pre-drain backup (`backups::write_verified`), off the runtime.
async fn backed_up<D: ConnAccess>(db: &D, data_dir: &Path, now: i64) -> SyncResult<PathBuf> {
    let (backer, dir) = (db.clone(), data_dir.to_path_buf());
    let zip = off_thread(move || backer.with_conn(|conn| backups::write_verified(conn, &dir, now)))
        .await?;
    log::info!("[SYNC] Backed this profile up before repairing it");
    Ok(zip)
}

/// Step 3 of `repair_capture`: drain rounds until one ends clean (`drain_round`; a pull that
/// only waits for another device's missing envelope counts) with nothing pending (`true`), or the
/// budget
/// (`rounds`, shared by every drain of the repair, up to `MAX_DRAIN_ROUNDS`) or two rounds in a
/// row without progress (nothing less pending, the pull cursor where it was) end it (`false`).
async fn drained<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    rounds: &mut usize,
    now: i64,
) -> SyncResult<bool> {
    let (mut last, mut last_cursor) = pending(db)?;
    let mut stalled = 0;
    while *rounds < MAX_DRAIN_ROUNDS {
        *rounds += 1;
        let clean = drain_round(relay, db, now).await;
        let (left, cursor) = pending(db)?;
        if clean && left == 0 {
            return Ok(true);
        }
        let progress = left < last || cursor > last_cursor;
        stalled = if progress { 0 } else { stalled + 1 };
        (last, last_cursor) = (left, cursor);
        if stalled >= 2 {
            log::info!("[SYNC] The repair's drain made no progress in two rounds");
            return Ok(false);
        }
    }
    log::info!("[SYNC] The repair's drain used all its rounds");
    Ok(false)
}

/// One drain round: whether it ended without an error or a problem and its pull reached the
/// relay's end (`PullReport::done`), or only waits for another device's missing envelope or parent
/// (`PullEnd::Waiting`, problem `Gap`): the drain makes sure this device's changes were sent, and
/// a waiting pull still pushes. The caller asks for nothing left pending as well.
async fn drain_round<R: Relay, D: ConnAccess>(relay: &R, db: &D, now: i64) -> bool {
    let mut policy = SnapshotPolicy::default();
    match engine::run_round(relay, db, &mut policy, RoundMode::Drain, now, 0).await {
        Ok(report) => {
            if let Some(problem) = report.problem {
                log::info!("[SYNC] The repair's drain round ended with {problem:?}");
            }
            let pull = report.pull.as_ref();
            match report.problem {
                None => pull.is_some_and(PullReport::done),
                Some(SyncProblem::Gap) => pull.is_some_and(|p| p.end() == PullEnd::Waiting),
                Some(_) => false,
            }
        }
        Err(error) => {
            log::info!("[SYNC] The repair's drain round failed");
            log::debug!("[SYNC] The repair's drain round failed: {error}");
            false
        }
    }
}

/// `sync_outbox` rows plus `sync_unsent` rows, and the pull cursor.
fn pending<D: ConnAccess>(db: &D) -> Result<(u64, u64)> {
    db.with_conn(|conn| {
        let impact = impact::replace_impact(conn)?;
        let cursor = state::load(conn)?.map_or(0, |sync| sync.cursor);
        Ok((
            u64::from(impact.pending_items) + u64::from(impact.pending_batches),
            cursor,
        ))
    })
}
