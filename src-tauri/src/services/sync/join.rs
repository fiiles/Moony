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
//!   never leaves it prunable. After the outcome a repair's backup stays kept (what a
//!   capture-broken session may have written before the guard existed) and so does a join's, the
//!   only copy of what it replaced, unless the profile is brand new (`impact::brand_new_profile`:
//!   nothing counted, nothing a person made, created within a day: spec 2026-11-14 §5.4), when it
//!   is plain. A re-sync's settles to plain or stays kept (unsent changes, documents or photos
//!   found nowhere else), and the backup of a replace that never happened becomes
//!   `-unused`.
//!
//! The relay passed in is the one at the credentials' own address (`Credentials::relay_url`, the
//! default or a self-hosted one, ruling SH-2): the caller builds it with
//! `HttpRelay::new(&creds.relay_url)`, never from `default_base_url()` (`MOONY_RELAY_URL`).

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
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
use super::error::{SyncError, SyncResult};
use super::hlc::DeviceId;
use super::identity::KitKeys;
use super::identity::{self, trust, Issuer};
use super::impact::{self, ReplaceConsent, ReplaceKind, DRAIN_INCOMPLETE};
use super::in_savepoint;
use super::keys::{self, Credentials, VaultKeys};
use super::pairing::PairingIdentity;
use super::recovery::KitInput;
use super::rekey::{bundle, envelope, head, pickup, rotate};
use super::relay::http::{normalize_base_url, validate_base_url};
use super::relay::{Relay, RelayError, MAX_REKEY_PAGE};
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
pub(crate) const KIT_REPLACED: &str = "validation.syncRecoveryKitReplaced";
const KIT_UNREADABLE: &str = "validation.syncRecoveryKitUnreadable";
const CODE_INVALID: &str = "validation.syncRecoveryCodeInvalid";
const PAIRING_FAILED: &str = "validation.syncPairingFailed";
const SERVER_NEEDS_UPDATE: &str = "validation.syncServerNeedsUpdate";

/// Drain rounds a repair runs at most; it also stops after two rounds in a row that neither shrank
/// `pending_items + pending_batches` nor moved the pull cursor on.
pub const MAX_DRAIN_ROUNDS: usize = 20;

/// How a new device gets its identity in the vault (phase 5 §5.3, §5.5).
pub enum JoinIdentity {
    /// A pairing (protocol 3): the holder's trust store seeds this device's, and the holder's
    /// one-time ticket signs its paired certificate.
    Paired(PairingIdentity, EpochHeads),
    /// A Recovery Kit join (revision §2.6): the Kit's key signs this device's certificate, the
    /// Kit anchors its store, which trusts on first use what it restores (§5.4) and takes the
    /// envelope's confirmed set; its newest epoch is never left unchecked; it holds its pushes.
    RecoveryKit(Box<KitJoin>),
    /// A pairing from a holder that sent no identity (a vault set up before phase 5): a signing
    /// key and no certificate, so removal stays unavailable there.
    Unvouched(EpochHeads),
}

/// The relay position at which each key epoch started, as the holder of a pairing sent them
/// (slot 2 carries a head for every epoch it knows one for); written to `sync_epochs` at the join.
pub type EpochHeads = std::collections::BTreeMap<u32, u64>;

/// The identity a replace gives this device under its new device ID.
enum NewIdentity {
    /// A device new to the vault: its trust store starts over.
    Join(JoinIdentity),
    /// This device again (a repair or a re-sync): its older ID vouches for it, as at a re-join,
    /// and its trust store stays.
    Successor,
}

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
    let who = NewIdentity::Join(JoinIdentity::Unvouched(EpochHeads::new()));
    let report = off_thread(move || {
        joiner.with_conn(|conn| replace_profile(conn, &dir, &creds, &name, &plain, &who, now))
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
    match fetched_snapshot_at(relay, creds).await? {
        Ok((plain, _)) => Ok(plain),
        // Read as a key change these credentials lack (`syncKeyRotated`).
        Err(KeyAhead) => Err(SyncError::ReadOnlyToken),
    }
}

/// The snapshot is sealed under a key epoch the credentials do not hold (As built R-73): a private
/// answer of `fetched_snapshot_at`, not an error of the sync client.
struct KeyAhead;

/// `fetched_snapshot`, with where the snapshot was taken and the epoch it is sealed under (read
/// from the sealed stream's header, which the open authenticated with that epoch's key).
async fn fetched_snapshot_at<R: Relay>(
    relay: &R,
    creds: &Credentials,
) -> SyncResult<std::result::Result<(Zeroizing<Vec<u8>>, SnapshotAt), KeyAhead>> {
    let blob = match relay.get_snapshot(&creds.auth()).await {
        Ok(blob) => blob,
        Err(RelayError::NoSnapshot) => {
            return Err(AppError::Validation(NO_SNAPSHOT.into()).into());
        }
        Err(error) => return Err(error.into()),
    };
    let at = SnapshotAt {
        seq: blob.seq,
        epoch: blob
            .bytes
            .get(4..8)
            .map_or(0, |b| u32::from_be_bytes([b[0], b[1], b[2], b[3]])),
    };
    let sealed_stream = blob.bytes.get(..4) == Some(&crate::services::sync::envelope::MAGIC[..]);
    if sealed_stream && at.epoch > creds.keys.current_epoch() {
        // A key change these credentials do not hold came after them (a pairing a removal
        // overtook): the snapshot is not damaged, this device was never given its key. Bytes
        // that are no sealed stream at all stay `syncSnapshotInvalid`.
        log::warn!("[SYNC] The vault's snapshot is sealed under a key these credentials lack");
        return Ok(Err(KeyAhead));
    }
    let opener = creds.clone();
    let plain = off_thread(move || open_sealed(&opener, &blob)).await?;
    Ok(Ok((plain, at)))
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
    /// A repair, or a join into a profile that held something: its backup stays Kept whatever the
    /// counts (controller ruling); a join into a profile that held nothing leaves it plain. A
    /// re-sync too when, inside its hold, an envelope still waited for its echo (sent or not, the
    /// vault was not seen to hold it: the counts leave out what the push handed over, the mark
    /// does not), or the sync server was seen to go back (`state::vault_went_back_at`: restored
    /// from an older backup, it may have lost envelopes already echoed here).
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
    /// Who this device is after the replace.
    identity: NewIdentity,
    now: i64,
    /// A re-sync: the sequences its push just handed to the relay (`PushReport::acknowledged`).
    /// They wait for an echo the replace never pulls, but the relay took them: they are not counted
    /// as staying only in the backup, nor against the consent.
    acknowledged: Vec<u64>,
}

/// One connection hold, in this order, so nothing can be written between the backup and the
/// replace (audit M8): a join checks again that it may join (`may_join`); the consent check
/// (`impact::check_consent_after_push`: a refusal writes no backup); a re-sync reads what stays
/// only in the backup (`pending_items`, and the `sync_unsent` rows its push did not hand to the
/// relay); the verified backup is written (a repair's was made before its drain); the backup is
/// marked Kept (a crash from here on never leaves it prunable); then `replace_profile`.
fn replace_hold(conn: &Connection, input: &HoldInput) -> Hold {
    let mut backup = match &input.backup {
        HoldBackup::Write => None,
        HoldBackup::Made(path) => Some(path.clone()),
    };
    let replaced = (|| {
        if input.kind == ReplaceKind::Join {
            may_join(conn, &input.creds)?;
        }
        impact::check_consent_after_push(conn, input.kind, &input.consent, &input.acknowledged)?;
        let (unsent_items, unsent_batches, resync_keeps) = if input.kind == ReplaceKind::Resync {
            let impact = impact::replace_impact(conn)?;
            let went_back = state::vault_went_back_at(conn)?.is_some();
            (
                impact.pending_items,
                impact::unsent_batches_besides(conn, &input.acknowledged)?,
                impact.pending_batches > 0 || went_back,
            )
        } else {
            (0, 0, false)
        };
        // A join into a profile that holds nothing still writes its backup (no replace without
        // a verified backup), but the backup is not worth keeping (spec 2026-11-14 §5.4).
        let brand_new =
            input.kind == ReplaceKind::Join && impact::brand_new_profile(conn, input.now)?;
        if brand_new {
            log::info!("[SYNC] The joining profile is brand new: its backup is not kept");
        }
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
            &input.identity,
            input.now,
        )?;
        Ok(Replaced {
            report,
            unsent_items,
            unsent_batches,
            always_kept: match input.kind {
                ReplaceKind::Join => !brand_new,
                ReplaceKind::Repair => true,
                ReplaceKind::Resync => resync_keeps,
            },
        })
    })();
    Hold { backup, replaced }
}

/// `replace_hold` off the runtime.
async fn held<D: ConnAccess>(db: &D, input: HoldInput) -> SyncResult<Hold> {
    let holder = db.clone();
    off_thread(move || holder.with_conn(|conn| Ok(replace_hold(conn, &input)))).await
}

/// The mark a backup settles to after its replace, with whether it is Kept: Kept for a repair and
/// for a join into a profile that held something, and for a re-sync when anything stayed only in
/// it (unsent changes, documents or photos not re-attached), else Plain. A failure is logged, and the
/// backup stays Kept.
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
///    key pair, a signing key and the certificate `identity` calls for, and this device recorded
///    under `device_name`. A failure leaves the profile as it
///    was and the backup, if one was written, `-unused`.
/// 4. The backup stays `-kept`, the only copy of what the join replaced, unless the profile was
///    brand new (`impact::brand_new_profile`), when it settles to plain. The binding is written.
#[allow(clippy::too_many_arguments)]
pub async fn join_new_device<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: Credentials,
    device_name: &str,
    consent: ReplaceConsent,
    mut identity: JoinIdentity,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
) -> SyncResult<ReplaceOutcome> {
    devices::validate_device_name(device_name)?;
    validate_base_url(&creds.relay_url)?;
    let _no_round = engine::exclusive(db).await;
    db.with_conn(|conn| may_join(conn, &creds))?;
    let (plain, at) = match fetched_snapshot_at(relay, &creds).await? {
        // The credentials are a key change behind (As built R-73): pair again, or, for a Kit
        // whose envelope a key change overtook between the two reads, try the Kit again.
        Err(KeyAhead) => {
            let key = match identity {
                JoinIdentity::RecoveryKit(_) => KIT_UNREADABLE,
                _ => PAIRING_FAILED,
            };
            return Err(AppError::Validation(key.into()).into());
        }
        Ok(fetched) => fetched,
    };
    if let JoinIdentity::RecoveryKit(join) = &mut identity {
        join.snapshot = Some(at);
    }
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
            identity: NewIdentity::Join(identity),
            now,
            acknowledged: Vec::new(),
        },
    )
    .await?;
    let outcome = outcome_of(hold)?;
    log::info!("[SYNC] A new device joined the vault under a new device ID");
    write_binding_after_replace(db, data_dir, native);
    Ok(outcome)
}

/// Whether a paired join on this profile has already replaced it (sync is on): its join ticket
/// is spent, so the caller gives the received pairing up whatever error the join answered and
/// never offers "Try again" with it (a second certificate with the same ticket would be refused by
/// every other device as `TicketReused`). The device just goes on syncing, or pairs again.
pub fn pairing_spent(conn: &Connection) -> Result<bool> {
    Ok(state::load(conn)?.is_some_and(|sync| sync.enabled))
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

/// What a Recovery Kit join takes from the Kit and its envelope (revision §2.6): the Kit's keys
/// (derived from the paper; they sign this device's certificate and anchor the store), the heads
/// and the confirmed set the envelope carries, its signer's certificate, the newest epoch it holds
/// a key for (E) and, when the server still lists the bundle of E's key change, the head and the
/// rotation it reported with it. `Debug` prints no secret.
pub struct KitJoin {
    pub kit: KitKeys,
    pub heads: EpochHeads,
    pub confirmed: Vec<(DeviceId, [u8; 32])>,
    pub signer: identity::Certificate,
    pub newest_epoch: u32,
    pub server_head: Option<(u64, u32)>,
    /// Where the snapshot the join restores was taken and the epoch it is sealed under; the join
    /// fills it in once it has fetched the snapshot.
    pub snapshot: Option<SnapshotAt>,
}

/// A fetched snapshot's relay position and the epoch it is sealed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SnapshotAt {
    pub seq: u64,
    pub epoch: u32,
}

impl std::fmt::Debug for KitJoin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KitJoin")
            .field("kit", &self.kit.id())
            .field("newest_epoch", &self.newest_epoch)
            .field("signer", &self.signer.body.device)
            .finish_non_exhaustive()
    }
}

/// The push hold of a Kit joiner (revision §2.6 step 7), the Local `app_config` key
/// `state::AFTER_KIT_JOIN_KEY`: every other active device of what it restored and the time of the
/// newest change in it. While it exists, rounds pull and push nothing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AfterKitJoin {
    #[serde(rename = "devices")]
    pub devices: Vec<String>,
    #[serde(rename = "newestChangeAt")]
    pub newest_change_at: Option<i64>,
}

/// The Kit joiner's push hold, if it holds (`AfterKitJoin`).
pub fn after_kit_join(conn: &Connection) -> Result<Option<AfterKitJoin>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [state::AFTER_KIT_JOIN_KEY],
            |r| r.get(0),
        )
        .optional()?;
    // A value that does not read still holds: only the person lifts it.
    Ok(value.map(|v| {
        serde_json::from_str(&v).unwrap_or(AfterKitJoin {
            devices: Vec::new(),
            newest_change_at: None,
        })
    }))
}

/// "Mám je", or "Odebrat je" once the removal is done (revision §2.6 step 7): the push hold is
/// lifted and the next round sends what waited.
pub fn lift_kit_join_hold(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM app_config WHERE key = ?1",
        [state::AFTER_KIT_JOIN_KEY],
    )?;
    Ok(())
}

/// A removal of this device is still running (`rotate::pending`), or the devices the person said
/// to remove are still active: the push hold stays and the dialog says so
/// ("Odebrání ještě probíhá").
pub const REMOVAL_PENDING: &str = "validation.syncRemovalPending";

/// "Mám je" / "Odebrat je" (`lift_kit_join_hold`), checked: nothing is lifted while a key change
/// of this device is pending, or, with `remove_them`, while any device the hold names is still
/// listed active (the UI removes them first; review of Task 24, M4).
pub fn decide_kit_join(conn: &Connection, remove_them: bool) -> Result<()> {
    if rotate::pending(conn)? {
        return Err(AppError::Validation(REMOVAL_PENDING.into()));
    }
    if remove_them {
        if let Some(hold) = after_kit_join(conn)? {
            let entries = trust::entries(conn)?;
            for id in &hold.devices {
                let Ok(device) = DeviceId::from_hex(id) else {
                    continue;
                };
                let trusted = entries
                    .iter()
                    .any(|e| e.device == device && e.state == trust::TrustState::Trusted);
                let removed: bool = conn.query_row(
                    "SELECT COALESCE((SELECT removed != 0 FROM sync_devices WHERE id = ?1), 1)",
                    [id],
                    |r| r.get(0),
                )?;
                if trusted && !removed {
                    return Err(AppError::Validation(REMOVAL_PENDING.into()));
                }
            }
        }
    }
    lift_kit_join_hold(conn)
}

fn unreadable() -> SyncError {
    AppError::Validation(KIT_UNREADABLE.into()).into()
}

/// What `GET /kit` refused, as the person reads it (revision §2.6 step 3; review of Task 16):
/// mapped on the relay's answer, never through `SyncError::Unauthorized` (which reads "the key
/// changed"). A wrong code and a vault without a Kit look alike to the server (`401`), as does a
/// Kit replaced too long ago: one text covers them, and an unknown vault too
/// (`syncRecoveryVaultNotFound`). A Kit replaced since says when (`syncRecoveryKitReplaced`). A
/// Kit token or vault ID the server's rules refuse can only come from a code that does not read
/// (`syncRecoveryCodeInvalid`). A rate limit, a lapsed subscription and the network keep their
/// own readings.
fn kit_read_refused(error: RelayError) -> SyncError {
    match error {
        RelayError::Unauthorized | RelayError::VaultNotFound => {
            AppError::Validation(KIT_VAULT_NOT_FOUND.into()).into()
        }
        RelayError::KitReplaced { replaced_at } => {
            AppError::validation_with(KIT_REPLACED, serde_json::json!({ "at": replaced_at })).into()
        }
        RelayError::BadRequest(_) => AppError::Validation(CODE_INVALID.into()).into(),
        other => other.into(),
    }
}

/// The credentials of a Recovery Kit and what its envelope says (revision §2.6 steps 1–5), from
/// the sync server at `input.relay_url` (the caller builds it from that address). In this order:
///
/// 1. Sync on → `validation.syncAlreadyEnabled`, before any relay call.
/// 2. The server must know the account in the code (`AccountNotFound` →
///    `validation.syncRecoveryAccountInvalid`).
/// 3. A server whose `/v1/server` lacks the `kit` feature → `validation.syncServerNeedsUpdate`;
///    then `GET /kit` with the Kit's token (`kit_read_refused` reads its refusals).
/// 4. The envelope opens with the Kit's keys and its signature verifies with the certificate it
///    bundles (`rekey::envelope::open`); any failure is `validation.syncRecoveryKitUnreadable`.
///    Whether its signer may speak is the store's question, at the join (step 6).
/// 5. The credentials hold every key it carries, the newest current; the vault must answer that
///    key's token (`vault_info`: an unknown token means the envelope does not hold the vault's
///    key, `syncRecoveryKitUnreadable`). The bundle of the newest key change, when the server
///    still lists it (30 days), gives that epoch's head as the server reports it.
///
/// Nothing is stored.
pub async fn recovery_credentials<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    input: KitInput,
) -> SyncResult<(Credentials, KitJoin)> {
    let enabled = db.with_conn(|conn| Ok(state::load(conn)?.is_some_and(|sync| sync.enabled)))?;
    if enabled {
        return Err(AppError::Validation(ALREADY_ENABLED.into()).into());
    }
    let KitInput { relay_url, code } = input;
    match relay.account_status(&code.account).await {
        Ok(_) => {}
        Err(RelayError::AccountNotFound) => {
            return Err(AppError::Validation(KIT_ACCOUNT_INVALID.into()).into());
        }
        Err(error) => return Err(error.into()),
    }
    // A server without Recovery Kits says so, not "the server doesn't know this kit" (review of
    // Tasks 18–19, M4). Its info failing is left to the Kit read.
    if relay.server_info().await.is_ok_and(|info| !info.kit) {
        return Err(AppError::Validation(SERVER_NEEDS_UPDATE.into()).into());
    }
    let kit = code.seed.derive(&code.vault_id.0);
    let read = relay
        .get_kit(&code.vault_id.to_b64(), kit.token())
        .await
        .map_err(kit_read_refused)?;
    let envelope = envelope::open(&read.envelope, &kit).map_err(|_| {
        log::warn!("[SYNC] The Recovery Kit's envelope does not open or verify");
        unreadable()
    })?;
    let newest_epoch = envelope.header.epoch;
    let mut held = std::collections::BTreeMap::new();
    let mut heads = EpochHeads::new();
    for old in &envelope.keys {
        held.insert(old.epoch, *old.key);
        if let Some(head) = old.head {
            heads.insert(old.epoch, head);
        }
    }
    let keys = VaultKeys::from_parts(newest_epoch, held).map_err(|_| unreadable())?;
    let creds = Credentials {
        relay_url,
        account: code.account.clone(),
        vault_id: code.vault_id,
        keys,
    };
    match relay.vault_info(&creds.auth()).await {
        Ok(_) => {}
        Err(RelayError::Unauthorized) => {
            log::warn!("[SYNC] The Recovery Kit's envelope does not hold the vault's key");
            return Err(unreadable());
        }
        Err(RelayError::VaultNotFound) => {
            return Err(AppError::Validation(KIT_VAULT_NOT_FOUND.into()).into());
        }
        Err(error) => return Err(error.into()),
    }
    let server_head = newest_bundle_head(relay, &creds, newest_epoch).await?;
    let join = KitJoin {
        kit,
        heads,
        confirmed: envelope
            .confirmed
            .iter()
            .map(|c| (c.device, c.cert_hash))
            .collect(),
        signer: envelope.signer,
        newest_epoch,
        server_head,
        snapshot: None,
    };
    Ok((creds, join))
}

/// The head and rotation the server reports with the bundle of the key change to `epoch`, when it
/// still lists one (it keeps bundles for 30 days): `None` otherwise, or for epoch 0.
async fn newest_bundle_head<R: Relay>(
    relay: &R,
    creds: &Credentials,
    epoch: u32,
) -> SyncResult<Option<(u64, u32)>> {
    if epoch == 0 {
        return Ok(None);
    }
    let auth = creds.auth();
    let mut after = 0;
    for _ in 0..pickup::MAX_BUNDLES_PER_WALK {
        // The envelope's newest key reads the vault (`vault_info` said so a moment ago): a refusal
        // now is no "key changed" (review of Tasks 18–19, M4), the Kit does not hold up.
        let page = match relay.rekeys(&auth, after, MAX_REKEY_PAGE).await {
            Ok(page) => page,
            Err(RelayError::Unauthorized | RelayError::TokenReadOnly) => return Err(unreadable()),
            Err(error) => return Err(error.into()),
        };
        for rekey in &page.rekeys {
            if rekey.rotation <= after {
                return Err(
                    RelayError::Protocol("the list of key changes does not end".into()).into(),
                );
            }
            after = rekey.rotation;
            let to = bundle::peek(&rekey.bundle, &creds.vault_id.0).map(|h| h.to_epoch);
            if to.is_ok_and(|to| to == epoch) {
                return Ok(Some((rekey.head, rekey.rotation)));
            }
        }
        if !page.more || page.rekeys.is_empty() {
            break;
        }
    }
    Ok(None)
}

/// The replace itself, in one savepoint: the credentials are bound (binding another vault
/// forgets the old one's relay positions); this device's chain starts over and the pauses and
/// counters of the old binding go (`binding_written` among them); `binding::join_leaving_marker`
/// restores the snapshot by replace under a new device ID (the cursor, heads and snapshot
/// bookkeeping become the snapshot's) and installs capture; a new key pair and signing key are
/// made and this device recorded with them under `device_name`, queued for the next push, with
/// the certificate `who` calls for (phase 5 §5.3): a joining device's trust store starts over, a
/// Kit joiner signs its own certificate and trusts what it restored on first use, a holder
/// without an identity leaves it without one, and a repair or re-sync is vouched for by the ID it
/// had before. Any failure rolls all of it
/// back and reinstalls the capture (or the guard) that ran before. Only after the commit is a
/// leftover restore marker cleared.
fn replace_profile(
    conn: &Connection,
    data_dir: &Path,
    creds: &Credentials,
    device_name: &str,
    plain: &[u8],
    who: &NewIdentity,
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
            // Read before the device ID changes: what vouches for this device's next ID.
            let predecessor = match who {
                NewIdentity::Successor => identity::predecessor(conn)?,
                NewIdentity::Join(_) => {
                    conn.execute_batch(
                        "DELETE FROM sync_trust; DELETE FROM sync_epochs; \
                         DELETE FROM sync_trust_seen; DELETE FROM sync_tickets;",
                    )?;
                    for key in [
                        state::UNVERIFIED_EPOCHS_KEY,
                        state::KIT_READ_SEEN_KEY,
                        state::AFTER_KIT_JOIN_KEY,
                        state::KIT_EPOCH_FLOOR_KEY,
                        trust::NEW_DEVICES_KEY,
                    ]
                    .into_iter()
                    .chain(state::REVIEW_KEYS)
                    {
                        conn.execute("DELETE FROM app_config WHERE key = ?1", [key])?;
                    }
                    None
                }
            };
            keys::save_credentials(conn, creds)?;
            state::reset_chain(conn)?;
            state::reset_engine_marks(conn)?;
            let report =
                binding::join_leaving_marker(conn, DeviceId::random(), &mut &plain[..], now)?;
            let public = keys::new_device_keypair(conn)?;
            let cert = match who {
                NewIdentity::Successor => identity::successor(conn, predecessor, now)?,
                NewIdentity::Join(JoinIdentity::Paired(paired, _)) => {
                    Some(paired_identity(conn, paired, now)?)
                }
                NewIdentity::Join(JoinIdentity::RecoveryKit(join)) => {
                    // The Kit's row anchors the store (revision §2.6 step 6); its key signs.
                    trust::pin_kit_anchor(
                        conn,
                        join.kit.id(),
                        join.kit.sign_pub(),
                        &join.confirmed,
                        now,
                    )
                    .map_err(|error| kit_refused("its row", error))?;
                    Some(identity::create_own(
                        conn,
                        Issuer::Kit { kit: &join.kit },
                        now,
                    )?)
                }
                NewIdentity::Join(JoinIdentity::Unvouched(_)) => {
                    identity::create_uncertified(conn)?;
                    None
                }
            };
            devices::record_this_device(conn, device_name, &public, cert.as_ref(), now)?;
            if let NewIdentity::Join(joining) = who {
                record_joined_epochs(conn, joining, now)?;
            }
            if let NewIdentity::Join(JoinIdentity::RecoveryKit(join)) = who {
                kit_join_store(conn, join, now)?;
            }
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

/// A joiner's key epochs (phase 5 §6.1): the heads its holder sent for the epochs it holds keys
/// for, with epoch 0 at relay position 0 when it holds that key; what the snapshot's `Keys` record
/// already recorded must agree. The keys are read from the profile after the snapshot's restore,
/// not from the credentials the join started with: the restore added the older epochs' keys. A head that does not is `validation.syncPairingFailed`: two
/// authenticated sources contradict each other.
pub(super) fn record_joined_epochs(
    conn: &Connection,
    joining: &JoinIdentity,
    now: i64,
) -> Result<()> {
    let held = keys::vault_keys(conn)?
        .ok_or_else(|| AppError::Internal("sync join: no vault keys after the restore".into()))?;
    let mut heads = match joining {
        JoinIdentity::Paired(_, heads) | JoinIdentity::Unvouched(heads) => heads.clone(),
        JoinIdentity::RecoveryKit(join) => join.heads.clone(),
    };
    if held.get(0).is_some() {
        heads.entry(0).or_insert(0);
    }
    for (epoch, head) in heads {
        if held.get(epoch).is_none() {
            continue;
        }
        if state::record_epoch(conn, epoch, head, None, now)? == state::EpochRecord::Differs {
            log::warn!("[SYNC] The holder's key epochs disagree with the vault's snapshot");
            let key = match joining {
                JoinIdentity::RecoveryKit(_) => KIT_UNREADABLE,
                _ => PAIRING_FAILED,
            };
            return Err(AppError::Validation(key.into()));
        }
    }
    Ok(())
}

/// A Kit join that does not hold up: `validation.syncRecoveryKitUnreadable`, which rolls the
/// replace back.
fn kit_refused(what: &str, error: AppError) -> AppError {
    log::warn!("[SYNC] The Recovery Kit join was refused: {what}");
    log::debug!("[SYNC] The Recovery Kit join was refused: {what}: {error}");
    AppError::Validation(KIT_UNREADABLE.into())
}

/// Steps 5–7 of a Kit join (revision §2.6), in the replace's savepoint once this device's row is
/// recorded and the Kit anchored:
///
/// - the store: trust on first use of what was restored, a genesis only when the Kit's chain
///   names it (As built R-28); then the envelope's signer must be pinned with the certificate the
///   envelope bundles, not removed, and not left before the envelope's newest epoch (its `left`
///   record's epoch is at least the envelope's, As built R-75; the envelope's own `at` is the
///   signer's claim and is not consulted, R-28), or the Kit is unreadable; and only when the
///   signer speaks on that store (`trust::kit_join_speaker`: `trusted`, not superseded, not left
///   out for its issuer unless it is the Kit's creator; As built R-106, generalising R-78) is the
///   confirmed set applied and any genesis it names trusted; otherwise the Kit opens and nothing
///   of the set is taken;
/// - the newest epoch E is never left unchecked: its head is known when the snapshot's `Keys`
///   record named it; a snapshot sealed under E bounds older ops past it
///   (`state::KitEpochFloor`); an older snapshot leaves E provisional (`head::note_unverified`,
///   with the server's head when it still lists the bundle) until the signer's signed head is
///   verified;
/// - the push hold (`AfterKitJoin`), with every other active device restored, unless there is
///   none.
fn kit_join_store(conn: &Connection, join: &KitJoin, now: i64) -> Result<()> {
    let me = state::load(conn)?
        .ok_or_else(|| AppError::Internal("sync join: no sync state after the replace".into()))?
        .device;
    let signer = join.signer.body.device;
    // Only what the Kit's own issuer chain vouches for: the envelope's confirmed set is the
    // signer's word, and is taken (its genesis certificates too) only from a signer that speaks
    // on this store (below).
    let anchored = trust::kit_join_genesis(conn, join.kit.id(), &[])?;
    trust::tofu_from_rows(conn, now, &anchored)?;
    let speaks = trust::entry(conn, signer)?.is_some_and(|e| {
        e.cert == join.signer
            && match e.state {
                trust::TrustState::Trusted => true,
                // Leaving seals no envelope, so the last one a device sealed stays the Kit's
                // until the next key change (As built R-75), and the Kit must keep working then.
                // The epoch of its `left` record is the signer's own claim and is not trusted
                // for anything (R-78): the envelope opens, and its confirmed set is ignored.
                trust::TrustState::Left => e
                    .state_record
                    .as_ref()
                    .is_some_and(|r| r.signer == signer && r.epoch >= join.newest_epoch),
                _ => false,
            }
    });
    if !speaks {
        return Err(kit_refused(
            "its envelope's signer",
            AppError::Internal(
                "not pinned with that certificate, removed, or left before the envelope".into(),
            ),
        ));
    }
    // The confirmed set, and any genesis it names, only from a signer that speaks here (final
    // review I-3, As built R-106, generalising R-78): `trusted`, not superseded and not left out
    // for its issuer, the Kit's creator aside (`trust::kit_join_speaker`). A vault-key holder can
    // seal an envelope with any key it holds: a genesis of its own, a superseded or left copy, a
    // late vouch; none of them makes the joiner trust what it names. The Kit still opens.
    if trust::kit_join_speaker(conn, &join.signer, join.kit.id())? {
        let genesis = trust::kit_join_genesis(conn, join.kit.id(), &join.confirmed)?;
        if genesis != anchored {
            trust::tofu_from_rows(conn, now, &genesis)?;
        }
        trust::apply_confirmed_set(conn, &join.confirmed)?;
    } else {
        log::warn!("[SYNC] The Recovery Kit's envelope signer does not speak here; its confirmed set is not taken");
    }
    let newest = join.newest_epoch;
    if !state::epochs(conn)?.contains_key(&newest) {
        let at = join.snapshot.ok_or_else(|| {
            AppError::Internal("sync join: the snapshot's place is unknown".into())
        })?;
        if at.epoch == newest {
            state::set_kit_epoch_floor(
                conn,
                state::KitEpochFloor {
                    epoch: newest,
                    after: at.seq,
                },
            )?;
        } else {
            if let Some((head, _)) = join.server_head {
                if state::record_epoch(conn, newest, head, Some(signer), now)?
                    == state::EpochRecord::Differs
                {
                    return Err(kit_refused(
                        "the newest epoch's start",
                        AppError::Internal("recorded with another head".into()),
                    ));
                }
            }
            head::note_unverified(
                conn,
                newest,
                join.server_head.map(|(head, _)| head),
                join.server_head.map(|(_, rotation)| rotation),
                signer,
                at.seq,
            )?;
        }
    }
    let others: Vec<String> = trust::entries(conn)?
        .into_iter()
        .filter(|e| {
            e.state == trust::TrustState::Trusted
                && e.kind != identity::CertKind::Kit
                && e.device != me
        })
        .map(|e| e.device.to_hex())
        .collect();
    if !others.is_empty() {
        let hold = AfterKitJoin {
            devices: others,
            newest_change_at: trust::newest_change_secs(conn, me)?,
        };
        let json = serde_json::to_string(&hold)
            .map_err(|e| AppError::Internal(format!("sync join: the push hold: {e}")))?;
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            rusqlite::params![state::AFTER_KIT_JOIN_KEY, json],
        )?;
    }
    Ok(())
}

/// A paired joiner's identity, inside the join's savepoint after the replace (spec §5.3, §5.4):
/// the holder's trust store seeds this device's empty one (`trust::import_export`: it must hold
/// the holder, trusted, and verify throughout), then this device signs its paired certificate
/// with the holder's one-time ticket, which must be signed by the holder's pinned key. A
/// payload that fails either is `validation.syncPairingFailed`: an authenticated holder sent
/// something this device cannot trust, and joining unverified would hide it.
fn paired_identity(
    conn: &Connection,
    paired: &PairingIdentity,
    now: i64,
) -> Result<identity::Certificate> {
    let refused = |what: &str, error: AppError| {
        log::warn!("[SYNC] The holder's identity was refused: {what}");
        log::debug!("[SYNC] The holder's identity was refused: {what}: {error}");
        AppError::Validation(PAIRING_FAILED.into())
    };
    trust::import_export(conn, &paired.export, paired.holder, now)
        .map_err(|error| refused("its trust store", error))?;
    let holder_sign_pub = trust::entry(conn, paired.holder)?
        .ok_or_else(|| AppError::Internal("sync join: the holder is not pinned".into()))?
        .sign_pub;
    identity::create_own(
        conn,
        Issuer::Ticket {
            secret: &paired.ticket_secret,
            ticket: &paired.ticket,
            holder: paired.holder,
            holder_sign_pub,
        },
        now,
    )
    .map_err(|error| refused("its join ticket", error))
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
                identity: NewIdentity::Successor,
                now,
                acknowledged: Vec::new(),
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
/// the outcome says how many. An envelope the push handed to the relay is not counted although
/// its echo is never pulled here (audit M4's echo needs a pull the replace does not wait for),
/// but the backup stays `-kept` whenever any envelope still waited for its echo in the hold, and
/// whenever the sync server was seen to go back (`state::vault_went_back_at`, sticky until a
/// replace): it may be the only copy of what such a server lost.
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
    let acknowledged = match push::push_all(relay, db, &creds).await {
        Ok(report) => {
            if report.pushed > 0 {
                log::info!(
                    "[SYNC] The re-sync sent {} envelopes before replacing",
                    report.pushed
                );
            }
            report.acknowledged
        }
        Err(error) => {
            // What an earlier batch of this push handed over is counted as unsent: the backup
            // stays kept.
            log::warn!("[SYNC] The re-sync could not send this device's changes first");
            log::debug!("[SYNC] The re-sync could not send this device's changes first: {error}");
            Vec::new()
        }
    };
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
            identity: NewIdentity::Successor,
            now,
            acknowledged,
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
    match rejoin_if_copy(db, data_dir, native, now, false) {
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
/// (`PullEnd::Waiting`, problem `Gap`) or for another device's change dated too far ahead
/// (`PullEnd::Ahead`, problem `ClockAhead`): the drain makes sure this device's changes were sent,
/// and a waiting pull still pushes. A quarantined author outranks a gap (`PullEnd::Held`, problem
/// `Quarantined`): not clean, with or without a gap. The caller asks for nothing left pending as
/// well.
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
                Some(SyncProblem::ClockAhead) => pull.is_some_and(|p| p.end() == PullEnd::Ahead),
                // A sync server that went back stays the problem of every round until the replace
                // this drain leads to: the pull itself decides.
                Some(SyncProblem::ServerWentBack) => pull.is_some_and(|p| {
                    matches!(p.end(), PullEnd::Done | PullEnd::Waiting | PullEnd::Ahead)
                }),
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
