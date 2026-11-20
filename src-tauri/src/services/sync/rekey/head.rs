//! The signed head of a key change (phase 5 As built 62–65; spec 2026-11-20-sync-audit-2-fixes
//! §2.3 steps 2–5 for the remover, at the end of this comment).
//!
//! The relay position at which an epoch starts (`sync_epochs`) is what the epoch rule (§6.1)
//! checks every op against, and the server reports it. A server colluding with a removed device
//! (which keeps the old key) could take that device's pushes after the rotation and tell another
//! device a head above them: the old-key ops would then fit. So the head is signed: the remover
//! seals (`verify_own`, once it has read the range below the head; see the end of this comment)
//! its first envelope under the new key with no rows and a `HeadRecord` {vault, from, to,
//! rotation, head, base, count, digest} signed by its device key (`queue_signed_head`); it travels through `sync_unsent` like any envelope, so a crash cannot lose it, and only a
//! device holding the remover's signing key can make one.
//!
//! Every other device takes the head the server reports with the bundle as **provisional**
//! (`note_unverified`: an entry of the Local `app_config` key `state::UNVERIFIED_EPOCHS_KEY`, with
//! where this device's cursor stood when it adopted the key). Until the head is verified, the pull
//! stops before any op sealed under an older epoch at a position past that cursor
//! (`awaits_head`, `PullStop::KeyChangeUnconfirmed`, shown as `keyChangeWaiting`), and what was
//! not sent is sealed again under the new key only after a pull that reached the head without a
//! stop (`reseal::may_reseal`); only verified heads are handed on (`verified_epochs`). A restored
//! snapshot naming the same head verifies it (`after_restore`). Each round looks
//! ahead past the provisional head with ordinary pull reads, applying nothing (`verify_heads`):
//! the signed head of the bundle's signer, opened with a key this device holds, either matches
//! (the entry goes, the epoch is verified) or not (the entry is marked `refuted` and stays:
//! `keyChangeUnverified` in every round, nothing past the cursor applied). Not found yet (the
//! remover has not pushed it) keeps waiting; the look-ahead goes on from where it stopped.
//!
//! The remover signs what it found, not the server's word (spec 2026-11-20-sync-audit-2-fixes
//! §2.3). Its commit (`rotate::commit`) makes the new key current and records the head as **not
//! verified** (`note_unverified`, signer = this device, `after` = the base: its cursor when it
//! sent the change), and queues nothing. `verify_own` then reads the range above the base up to
//! the head with the new token (`range::read_range`: nothing applied, nothing opened) and, in one
//! savepoint, stores the notes, queues the version-2 record with the base, the number of ops found
//! and their digest as the first envelope under the new key, and removes the entry: the head
//! counts as verified from that moment. It runs inside the removal call, in the resolution of a
//! pending change and at the start of every round while the entry exists; a read that cannot
//! finish (a refusal, a network error, `410`, more pages than one pull reads) leaves the entry
//! and the next round starts again from the base. While it exists the device sends nothing
//! (`engine::round_pass`, `reseal::may_reseal`, `snapshots::ready_to_dump`) and starts no other
//! change (`rotate::check`). `verify_heads` and `after_restore` leave own entries alone: only
//! `verify_own` settles them.
//!
//! Every other device takes a head as verified only when it has checked what lies between the
//! remover's base and the head against what the remover signed (spec §2.3 steps 7 and 8). Once
//! `look_ahead` finds the signed record, `settle` compares the head, the rotation and the epochs as
//! before; a device whose watermark (the cursor or the last restored snapshot, `seen::watermark`: below it every position was handled) is already at or past the head has nothing to check; any other
//! reads the range `(record.base, record.head]` itself with its own token (`range::read_range`,
//! the base of the **signed record**, never the server's), and takes the head only if the number
//! of ops and their digest are the signed ones: then, in one savepoint, the notes and the base go
//! into the store (`range::store_notes`, the footing of rule 6 in the pull) and the entry goes. A
//! different count or digest refutes the head (`keyChangeUnverified`) and nothing of the range is
//! applied; a read that cannot finish (the relay, more pages than a pull reads, `410`) keeps the
//! entry and the round waits; with `410` a snapshot under the new key that names the head settles
//! it as before (`after_restore`, only when the snapshot lies at or past the head).
//!
//! A device that holds a head from somewhere else than a bundle it picked up (a pairing holder's
//! heads, a snapshot's key list, an earlier epoch before a replace) and whose watermark is below it
//! gets an entry for that epoch too (`require_ranges`): no rotation, the head from `sync_epochs`,
//! `after` and `scanned` at its watermark. Its signer is `sync_epochs.by_device` when recorded, else
//! the maker of the server's bundle for that epoch whose signature verifies under a certificate
//! pinned here (`signer_from_bundle`; it adopts nothing, this device holds the keys); with
//! neither, the entry waits. After a replace every epoch whose head lies above the snapshot's
//! position starts again this way (`snapshot::finish`).
//!
//! An entry this device committed itself and has not signed (`own_pending`, set only by
//! `rotate::commit`) is settled only by `verify_own`; one made by `require_ranges` for an epoch
//! this device signed before a replace (the replace gives it another ID) is settled like any
//! other device's, by finding the record in the log.
//!
//! A device that had already applied ops past the verified head (the server hid the key change
//! from it) learns it here and keeps a notice (`rekey::KeyChangeOverrun`); nothing is undone.

use std::collections::BTreeMap;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::bundle;
use super::range::{self, RangeRead};
use super::{write_config, KeyChangeOverrun};
use crate::error::{AppError, Result};
use crate::services::sync::changeset;
use crate::services::sync::engine::pull::{MAX_PULL_PAGES, PULL_PAGE};
use crate::services::sync::engine::push::seal_into_unsent;
use crate::services::sync::engine::{seen, ConnAccess};
use crate::services::sync::envelope::{envelope_epoch, open_op};
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::{self, trust, verify_sig, SignSeed};
use crate::services::sync::in_savepoint;
use crate::services::sync::journal::build_marker;
use crate::services::sync::keys::{self, Credentials};
use crate::services::sync::relay::{PulledOp, Relay, RelayError, Rotated, MAX_REKEY_PAGE};
use crate::services::sync::state;

const HEAD_DOMAIN: &[u8; 19] = b"moony rekey head v2";
const SIGNED_LEN: usize = 19 + 16 + 16 + 4 + 4 + 4 + 8 + 8 + 4 + 32;
const HEAD_LEN: usize = SIGNED_LEN + 64;

/// Pages of ops one look-ahead reads at most per epoch and round (it goes on from there next
/// round).
pub const MAX_LOOKAHEAD_PAGES: usize = 4;

/// Ops per look-ahead page: a pull page, a few in tests (so a test reaches the page cap).
const LOOKAHEAD_PAGE: u32 = if cfg!(test) { 4 } else { PULL_PAGE };

/// `"moony rekey head v2" ‖ vault_id (16) ‖ signer (16) ‖ from_epoch (u32) ‖ to_epoch (u32) ‖
/// rotation (u32) ‖ head (u64) ‖ base (u64) ‖ count (u32) ‖ digest (32) ‖ sig (64)`, `sig`
/// Ed25519 by the signer over every byte before it. `base`, `count` and `digest` bind the range
/// of the server's log above `base` up to `head` (see `range::range_digest`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeadRecord {
    pub vault_id: [u8; 16],
    pub signer: DeviceId,
    pub from_epoch: u32,
    pub to_epoch: u32,
    pub rotation: u32,
    pub head: u64,
    pub base: u64,
    pub count: u32,
    pub digest: [u8; 32],
    pub sig: [u8; 64],
}

fn invalid() -> AppError {
    AppError::Internal("sync: a key change's signed head is not valid".into())
}

impl HeadRecord {
    #[allow(clippy::too_many_arguments)]
    pub fn sign(
        vault_id: [u8; 16],
        signer: DeviceId,
        from_epoch: u32,
        to_epoch: u32,
        rotation: u32,
        head: u64,
        base: u64,
        count: u32,
        digest: [u8; 32],
        key: &SignSeed,
    ) -> Self {
        let mut record = HeadRecord {
            vault_id,
            signer,
            from_epoch,
            to_epoch,
            rotation,
            head,
            base,
            count,
            digest,
            sig: [0; 64],
        };
        record.sig = key.sign(&record.signed_bytes());
        record
    }

    fn signed_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEAD_LEN);
        out.extend_from_slice(HEAD_DOMAIN);
        out.extend_from_slice(&self.vault_id);
        out.extend_from_slice(&self.signer.0);
        out.extend_from_slice(&self.from_epoch.to_be_bytes());
        out.extend_from_slice(&self.to_epoch.to_be_bytes());
        out.extend_from_slice(&self.rotation.to_be_bytes());
        out.extend_from_slice(&self.head.to_be_bytes());
        out.extend_from_slice(&self.base.to_be_bytes());
        out.extend_from_slice(&self.count.to_be_bytes());
        out.extend_from_slice(&self.digest);
        out
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.signed_bytes();
        out.extend_from_slice(&self.sig);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != HEAD_LEN || &bytes[..HEAD_DOMAIN.len()] != HEAD_DOMAIN {
            return Err(invalid());
        }
        let mut at = HEAD_DOMAIN.len();
        let mut take = |n: usize| {
            let part = &bytes[at..at + n];
            at += n;
            part
        };
        let mut array16 = [0u8; 16];
        array16.copy_from_slice(take(16));
        let vault_id = array16;
        array16.copy_from_slice(take(16));
        let signer = DeviceId(array16);
        let u32_of = |b: &[u8]| u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        let from_epoch = u32_of(take(4));
        let to_epoch = u32_of(take(4));
        let rotation = u32_of(take(4));
        let mut head = [0u8; 8];
        head.copy_from_slice(take(8));
        let mut base = [0u8; 8];
        base.copy_from_slice(take(8));
        let count = u32_of(take(4));
        let mut digest = [0u8; 32];
        digest.copy_from_slice(take(32));
        let mut sig = [0u8; 64];
        sig.copy_from_slice(take(64));
        Ok(HeadRecord {
            vault_id,
            signer,
            from_epoch,
            to_epoch,
            rotation,
            head: u64::from_be_bytes(head),
            base: u64::from_be_bytes(base),
            count,
            digest,
            sig,
        })
    }

    /// The signature is by `signer_pub` (the caller looks the signer's pinned key up).
    pub fn verify(&self, signer_pub: &[u8; 32]) -> Result<()> {
        verify_sig(signer_pub, &self.signed_bytes(), &self.sig).map_err(|_| invalid())
    }
}

/// An epoch this device adopted from another device's key change whose head the remover's
/// signed head has not confirmed yet.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnverifiedEpoch {
    #[serde(rename = "epoch")]
    pub epoch: u32,
    /// The head the server reported with the bundle (also in `sync_epochs`); `None` for a Kit
    /// joiner that found no bundle (revision §2.6 step 5): the signed head's own is taken then.
    #[serde(rename = "head")]
    pub head: Option<u64>,
    /// The rotation of that bundle, when there was one.
    #[serde(rename = "rotation")]
    pub rotation: Option<u32>,
    /// The bundle's signer (hex), whose signed head confirms it; empty while this device does
    /// not know it (an epoch it holds a head for without a bundle: `require_ranges`).
    #[serde(rename = "by")]
    pub by: String,
    /// This device's cursor when it adopted the key: what it had pulled before the key change.
    #[serde(rename = "after")]
    pub after: u64,
    /// How far the look-ahead has read.
    #[serde(rename = "scanned")]
    pub scanned: u64,
    /// The signed head was found and differs: `keyChangeUnverified` for good.
    #[serde(rename = "refuted", default)]
    pub refuted: bool,
    /// This device's own key change, committed and not signed yet: only `rotate::commit` sets
    /// it, only `verify_own` settles it. `by == me` says nothing of the kind: a replace gives
    /// this device another ID, and an entry for an epoch it signed before is settled like any
    /// other device's.
    #[serde(rename = "own", default)]
    pub own_pending: bool,
}

/// The epochs whose head is not verified yet: empty when there is no row. A row that exists and
/// does not read is an error, never "nothing unverified": every key-change gate reads this, and
/// a value that cannot be read must keep them closed (audit-2 final review, I2).
pub fn unverified_epochs(conn: &Connection) -> Result<Vec<UnverifiedEpoch>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [state::UNVERIFIED_EPOCHS_KEY],
            |r| r.get(0),
        )
        .optional()?;
    match value {
        None => Ok(Vec::new()),
        Some(value) => serde_json::from_str(&value).map_err(|e| {
            AppError::Internal(format!(
                "sync: the list of unverified key-change heads does not read ({:?})",
                e.classify()
            ))
        }),
    }
}

fn store(conn: &Connection, epochs: &[UnverifiedEpoch]) -> Result<()> {
    if epochs.is_empty() {
        conn.execute(
            "DELETE FROM app_config WHERE key = ?1",
            [state::UNVERIFIED_EPOCHS_KEY],
        )?;
        return Ok(());
    }
    write_config(conn, state::UNVERIFIED_EPOCHS_KEY, &epochs)
}

/// The pickup adopted `epoch` at the server's `head` (rotation `rotation`, signed by `by`), with
/// this device's cursor at `after`: the head is provisional until verified. A Kit joiner with no
/// bundle to read them from passes neither (revision §2.6 step 5): the signed head of `by` then
/// gives them, once its signature verifies, and until then every op under an older epoch past
/// `after` waits.
pub(crate) fn note_unverified(
    conn: &Connection,
    epoch: u32,
    head: Option<u64>,
    rotation: Option<u32>,
    by: DeviceId,
    after: u64,
) -> Result<()> {
    note(conn, epoch, head, rotation, by.to_hex(), after, false)
}

/// `rotate::commit`: this device's own key change is committed and its head not signed yet
/// (`verify_own`), `after` being the base.
pub(crate) fn note_own_unverified(
    conn: &Connection,
    epoch: u32,
    head: u64,
    rotation: u32,
    me: DeviceId,
    base: u64,
) -> Result<()> {
    note(
        conn,
        epoch,
        Some(head),
        Some(rotation),
        me.to_hex(),
        base,
        true,
    )
}

fn note(
    conn: &Connection,
    epoch: u32,
    head: Option<u64>,
    rotation: Option<u32>,
    by: String,
    after: u64,
    own_pending: bool,
) -> Result<()> {
    let mut epochs = unverified_epochs(conn)?;
    epochs.retain(|e| e.epoch != epoch);
    epochs.push(UnverifiedEpoch {
        epoch,
        head,
        rotation,
        by,
        after,
        scanned: head.unwrap_or(after),
        refuted: false,
        own_pending,
    });
    epochs.sort_by_key(|e| e.epoch);
    store(conn, &epochs)
}

/// Whether this device holds a head it signs itself that is not verified yet (its own key
/// change, committed provisionally, whose range it has not read: `verify_own`).
pub fn own_unverified(conn: &Connection) -> Result<bool> {
    Ok(unverified_epochs(conn)?.iter().any(|e| e.own_pending))
}

/// Whether any head, this device's own or another's, is not verified yet.
pub fn any_unverified(conn: &Connection) -> Result<bool> {
    Ok(!unverified_epochs(conn)?.is_empty())
}

/// A re-join under a new device ID (`pull::rejoin_as_new_device`, in its savepoint): this device's
/// own unsigned head stays noted as signed by the OLD ID (`by`, which `note_own_unverified` set),
/// but is no longer "own": the new identity never signs a change the old one made. From then on
/// it waits for the original's signed record like any other device's head (`after` and `scanned`
/// stay: the record lies above the head, which is where the look-ahead reads from).
pub(crate) fn release_own_pending(conn: &Connection) -> Result<()> {
    let mut epochs = unverified_epochs(conn)?;
    if epochs.iter().any(|e| e.own_pending) {
        for entry in epochs.iter_mut() {
            entry.own_pending = false;
        }
        store(conn, &epochs)?;
    }
    Ok(())
}

/// Every signed head record waiting in `sync_unsent`, by ascending epoch (a re-join under a new
/// ID collects them before `chain::rejoin` empties the queue; audit-2 reading A, A-2). The
/// record's signature is the signer's, not the envelope author's.
pub(crate) fn unsent_records(conn: &Connection) -> Result<Vec<Vec<u8>>> {
    let Some(creds) = keys::credentials(conn)? else {
        return Ok(Vec::new());
    };
    let rows: Vec<Vec<u8>> = conn
        .prepare("SELECT envelope FROM sync_unsent ORDER BY seq")?
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()
        .map_err(|_| AppError::Internal("sync: the unsent envelopes do not read".into()))?;
    let mut records = Vec::new();
    for envelope in rows {
        let json = open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &envelope)
            .map_err(|_| AppError::Internal("sync: an unsent envelope does not open".into()))?;
        let changes = changeset::from_json(&json).map_err(|_| {
            AppError::Internal("sync: an unsent envelope does not hold a change set".into())
        })?;
        if let Some(bytes) = changes.rekey_head {
            let epoch = HeadRecord::decode(&bytes)
                .map_err(|_| {
                    AppError::Internal("sync: an unsent head record does not decode".into())
                })?
                .to_epoch;
            records.push((epoch, bytes));
        }
    }
    records.sort_by_key(|(epoch, _)| *epoch);
    Ok(records.into_iter().map(|(_, bytes)| bytes).collect())
}

/// Queues each of `records` again as the marker envelope of this device's current ID, under the
/// current key, with the record's bytes as they were (the look-ahead checks the record's signer
/// and signature, never the envelope's author).
pub(crate) fn requeue_records(conn: &Connection, records: &[Vec<u8>]) -> Result<()> {
    if records.is_empty() {
        return Ok(());
    }
    let creds = keys::credentials(conn)?
        .ok_or_else(|| AppError::Internal("sync: the signed head has no credentials".into()))?;
    for record in records {
        let mut built = build_marker(conn)?;
        built.changeset.rekey_head = Some(record.clone());
        seal_into_unsent(conn, &creds, &built)?;
    }
    let sync =
        state::load(conn)?.ok_or_else(|| AppError::Internal("sync: sync is not set up".into()))?;
    state::note_sealed_at_barrier(conn, sync.barrier)
}

/// `sync_epochs` without the heads not verified yet: what this device vouches for (re-sealing,
/// what a holder hands a joiner, a snapshot's `Keys` record, a bundle's older keys).
pub fn verified_epochs(conn: &Connection) -> Result<BTreeMap<u32, u64>> {
    let mut heads = state::epochs(conn)?;
    for unverified in unverified_epochs(conn)? {
        heads.remove(&unverified.epoch);
    }
    Ok(heads)
}

/// Whether an op sealed under `epoch` at relay `position` needs a head not verified yet: an
/// epoch above it whose provisional head it would be judged against, past where this device's
/// cursor stood when it adopted that epoch.
pub(crate) fn awaits_head(conn: &Connection, epoch: u32, position: u64) -> Result<bool> {
    Ok(unverified_epochs(conn)?
        .iter()
        .any(|e| epoch < e.epoch && position > e.after))
}

/// The remover's signature (`verify_own`, in its savepoint): its next envelope, with no rows,
/// carries the signed head of the change, sealed under the new key into `sync_unsent` (so it goes
/// out with the next push, a crash or not). The record binds the range `(base, head]` as `notes`
/// found it: their number and digest.
pub(crate) fn queue_signed_head(
    conn: &Connection,
    me: DeviceId,
    from: u32,
    to: u32,
    rotated: Rotated,
    base: u64,
    notes: &[range::RangeNote],
) -> Result<()> {
    let creds = keys::credentials(conn)?
        .ok_or_else(|| AppError::Internal("sync: the key change has no credentials".into()))?;
    // Sealed under the current key: a later key change adopted while the head was unsigned makes
    // that a key above `to`, which `signed_head_in` accepts (an envelope of the record's epoch or
    // later). A current key below `to` cannot be.
    if creds.keys.current_epoch() < to {
        return Err(AppError::Internal(
            "sync: the signed head is sealed under an older key".into(),
        ));
    }
    let seed = identity::own_seed(conn)?
        .ok_or_else(|| AppError::Internal("sync: no signing key for the signed head".into()))?;
    let count = u32::try_from(notes.len())
        .map_err(|_| AppError::Internal("sync: the range of a key change is too long".into()))?;
    let digest = range::range_digest(&creds.vault_id.0, to, base, rotated.head, notes)?;
    let record = HeadRecord::sign(
        creds.vault_id.0,
        me,
        from,
        to,
        rotated.rotation,
        rotated.head,
        base,
        count,
        digest,
        &seed,
    );
    let mut built = build_marker(conn)?;
    built.changeset.rekey_head = Some(record.encode());
    seal_into_unsent(conn, &creds, &built)?;
    let sync =
        state::load(conn)?.ok_or_else(|| AppError::Internal("sync: sync is not set up".into()))?;
    state::note_sealed_at_barrier(conn, sync.barrier)
}

/// What `verify_own` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnHead {
    /// No head of this device's own is waiting.
    None,
    /// The range was read and the head is signed and queued (verified from now on).
    Signed,
    /// The range could not be read to its end (or the server's head is below this device's
    /// cursor): the head stays unverified, and the next call starts again from the base.
    Waiting,
}

/// Settles this device's own unverified heads (module doc): for each, reads the range with the
/// new token, then, in one savepoint, stores the notes, queues the signed record and removes the
/// entry. A read that cannot finish, whatever the reason (the relay refused, the network, `410`,
/// more pages than a pull reads), is `Waiting`: nothing is kept of a partial read, and the round's
/// pull meets the same error and names it. A local failure is the error. Under the round lock or
/// `engine::exclusive`.
pub async fn verify_own<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> SyncResult<OwnHead> {
    let loaded = db.with_conn(|conn| {
        let Some(me) = state::load(conn)?.map(|sync| sync.device) else {
            return Ok(None);
        };
        let mine: Vec<UnverifiedEpoch> = unverified_epochs(conn)?
            .into_iter()
            .filter(|e| e.own_pending)
            .collect();
        if mine.is_empty() {
            return Ok(None);
        }
        let creds = keys::credentials(conn)?.ok_or_else(|| {
            AppError::Internal("sync: the credentials went during a key change".into())
        })?;
        Ok(Some((me, mine, creds)))
    })?;
    let Some((me, mine, creds)) = loaded else {
        return Ok(OwnHead::None);
    };
    for entry in mine {
        let (Some(head), Some(rotation)) = (entry.head, entry.rotation) else {
            log::warn!("[SYNC] This device's own key change has no head recorded");
            return Ok(OwnHead::Waiting);
        };
        if head < entry.after {
            log::warn!(
                "[SYNC] The sync server's head for this device's key change is below its place"
            );
            return Ok(OwnHead::Waiting);
        }
        let notes = match range::read_range(relay, &creds.auth(), entry.after, head, MAX_PULL_PAGES)
            .await
        {
            Ok(RangeRead::Complete(notes)) => notes,
            Ok(RangeRead::More { scanned }) => {
                log::info!(
                    "[SYNC] The range of this device's key change is longer than one read ({scanned})"
                );
                return Ok(OwnHead::Waiting);
            }
            Err(error) => {
                log::info!("[SYNC] The range of this device's key change was not read: {error}");
                return Ok(OwnHead::Waiting);
            }
        };
        db.with_conn(|conn| {
            in_savepoint(conn, "sync_sign_own_head", || {
                let mut epochs = unverified_epochs(conn)?;
                let before = epochs.len();
                epochs.retain(|e| !(e.epoch == entry.epoch && e.own_pending));
                if epochs.len() == before {
                    // Settled meanwhile: nothing to sign twice.
                    return Ok(());
                }
                range::store_notes(conn, entry.epoch, entry.after, &notes)?;
                queue_signed_head(
                    conn,
                    me,
                    entry.epoch.saturating_sub(1),
                    entry.epoch,
                    Rotated { rotation, head },
                    entry.after,
                    &notes,
                )?;
                store(conn, &epochs)?;
                state::owe_snapshot(conn, entry.epoch)?;
                log::info!(
                    "[SYNC] This device signed the start of key epoch {} with {} changes below it",
                    entry.epoch,
                    notes.len()
                );
                Ok(())
            })
        })?;
    }
    Ok(OwnHead::Signed)
}

/// What `verify_heads` found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeadCheck {
    /// No head waits for its signature.
    Clear,
    /// A head waits: its signed head is not on the server yet (or not readable).
    Waiting,
    /// A signed head differs from what the server reported (`keyChangeUnverified`).
    Refuted,
}

/// The look-ahead (module doc): for each provisional head, reads ops past it (from where the
/// last look-ahead stopped, at most `MAX_LOOKAHEAD_PAGES` pages), applying nothing, until it
/// finds the signed head of the bundle's signer. A relay failure is returned as its `SyncError`
/// (the round's pull would meet it too), except `410` (the ops were pruned), which only waits.
pub async fn verify_heads<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    now: i64,
) -> SyncResult<HeadCheck> {
    let pending = db.with_conn(unverified_epochs)?;
    if pending.iter().any(|e| e.refuted) {
        return Ok(HeadCheck::Refuted);
    }
    for entry in pending {
        // This device's own head has no signed record to look for yet: only `verify_own`
        // settles it.
        if entry.own_pending {
            continue;
        }
        match look_ahead(relay, db, creds, &entry).await? {
            Found::Record(record) => match settle(relay, db, creds, &entry, &record, now).await? {
                Settled::Verified | Settled::Waiting => {}
                Settled::Refuted => return Ok(HeadCheck::Refuted),
            },
            Found::NotYet(scanned) => db.with_conn(|conn| {
                let mut epochs = unverified_epochs(conn)?;
                for e in epochs.iter_mut().filter(|e| e.epoch == entry.epoch) {
                    e.scanned = e.scanned.max(scanned);
                }
                store(conn, &epochs)
            })?,
        }
    }
    let left = db.with_conn(unverified_epochs)?;
    Ok(if left.is_empty() {
        HeadCheck::Clear
    } else if left.iter().any(|e| e.refuted) {
        HeadCheck::Refuted
    } else {
        HeadCheck::Waiting
    })
}

enum Found {
    Record(HeadRecord),
    NotYet(u64),
}

async fn look_ahead<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    entry: &UnverifiedEpoch,
) -> SyncResult<Found> {
    // `scanned` is where the last look-ahead stopped: the bundle's head for an entry made by the
    // pickup, this device's watermark for one made by `require_ranges` (the record may sit below
    // the head a lying server reported, so it is looked for from there, never below the floor).
    let mut after = entry.scanned;
    // What the look-ahead remembers advances only over an unbroken run of positions (A-4): a
    // server that withholds one op, the signed record perhaps, must not make the next rounds start
    // above it. Positions up to the entry's head may have holes (the server lifts the head for a
    // removal and nothing sits in the free range), and no record lies there.
    let mut stored = entry.scanned;
    let free_to = entry.head.unwrap_or(entry.scanned);
    let mut broken = false;
    let Some(by) = signer_of(relay, db, creds, entry).await? else {
        return Ok(Found::NotYet(stored));
    };
    let Some(signer_pub) = db
        .with_conn(|conn| trust::entry(conn, by))?
        .map(|e| e.sign_pub)
    else {
        return Ok(Found::NotYet(stored));
    };
    let auth = creds.auth();
    for _ in 0..MAX_LOOKAHEAD_PAGES {
        let page = match relay.pull(&auth, after, LOOKAHEAD_PAGE).await {
            Ok(page) => page,
            Err(RelayError::SnapshotRequired) => {
                // Pruned below a snapshot: go on from the server's floor (and never below this
                // device's place), where the next read is allowed.
                let floor = relay.vault_info(&auth).await?.floor;
                let mark = db.with_conn(|conn| Ok(seen::watermark(conn)?.unwrap_or(0)))?;
                log::info!("[SYNC] The ops after a key change were pruned: looking past them");
                after = after.max(floor).max(mark);
                if !broken {
                    stored = stored.max(after);
                }
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        let mut contract = true;
        for op in &page.ops {
            if op.seq <= after {
                contract = false;
                break;
            }
            after = op.seq;
            if !broken && (op.seq <= free_to || op.seq == stored.max(free_to) + 1) {
                stored = op.seq;
            } else {
                broken = true;
            }
            if let Some(record) = signed_head_in(creds, op, entry.epoch) {
                if record.signer == by
                    && record.vault_id == creds.vault_id.0
                    && record.to_epoch == entry.epoch
                    && record.verify(&signer_pub).is_ok()
                {
                    return Ok(Found::Record(record));
                }
            }
        }
        if !contract || !page.more || page.ops.is_empty() {
            break;
        }
    }
    Ok(Found::NotYet(stored))
}

/// Who signs the record of `entry` (ruling R8): the entry's signer when it has one (the bundle's
/// maker, or what `sync_epochs` recorded); else the maker of the server's bundle for that epoch
/// that verifies under a certificate pinned here, recorded for next time; else nobody yet and
/// the entry waits (the server lists a bundle for 30 days only).
async fn signer_of<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    entry: &UnverifiedEpoch,
) -> SyncResult<Option<DeviceId>> {
    if let Ok(by) = DeviceId::from_hex(&entry.by) {
        return Ok(Some(by));
    }
    if let Some(by) = db.with_conn(|conn| state::epoch_signer(conn, entry.epoch))? {
        return Ok(Some(by));
    }
    let Some(by) = signer_from_bundle(relay, db, creds, entry.epoch).await? else {
        return Ok(None);
    };
    db.with_conn(|conn| {
        state::set_epoch_signer(conn, entry.epoch, by)?;
        let mut epochs = unverified_epochs(conn)?;
        for e in epochs
            .iter_mut()
            .filter(|e| e.epoch == entry.epoch && e.by.is_empty())
        {
            e.by = by.to_hex();
        }
        store(conn, &epochs)
    })?;
    Ok(Some(by))
}

/// The maker of the bundle the server lists for `epoch`: the bundle opens with the key this device
/// holds for that epoch and its signature verifies with the key of the certificate it names, which
/// this store has pinned for a device that speaks and is not known to be gone (as the pickup
/// checks a bundle). Nothing is adopted from
/// it: this device holds every key it carries.
async fn signer_from_bundle<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    epoch: u32,
) -> SyncResult<Option<DeviceId>> {
    let Some(key) = creds.keys.get(epoch) else {
        return Ok(None);
    };
    let (mut after, mut walked) = (0u32, 0usize);
    loop {
        let page = relay
            .rekeys(&creds.auth(), after, MAX_REKEY_PAGE)
            .await
            .map_err(SyncError::from)?;
        for rekey in &page.rekeys {
            walked += 1;
            if walked > super::pickup::MAX_BUNDLES_PER_WALK || rekey.rotation <= after {
                return Ok(None);
            }
            after = rekey.rotation;
            let Ok(header) = bundle::peek(&rekey.bundle, &creds.vault_id.0) else {
                continue;
            };
            if header.to_epoch != epoch {
                continue;
            }
            let Ok(opened) = bundle::open_body_with_key(&rekey.bundle, &creds.vault_id.0, &key)
            else {
                continue;
            };
            let cert = opened.signer().clone();
            // As the pickup checks a bundle's signer: pinned with this certificate, trusted and
            // not left out for its issuer (`trust::speaker`), and not known here to be gone.
            let by = cert.body.device;
            let pinned = db.with_conn(|conn| {
                if super::pickup::known_gone(conn, by)? {
                    return Ok(None);
                }
                trust::speaker(conn, &cert)
            })?;
            if let Some(pinned) = pinned {
                if opened.verify(&pinned.sign_pub).is_ok() {
                    return Ok(Some(by));
                }
            }
        }
        if !page.more || page.rekeys.is_empty() {
            return Ok(None);
        }
    }
}

/// The signed head an op carries, if it is sealed under `epoch` or later, opens with a key this
/// device holds and holds a changeset with one.
fn signed_head_in(creds: &Credentials, op: &PulledOp, epoch: u32) -> Option<HeadRecord> {
    if envelope_epoch(&op.ct).is_none_or(|sealed| sealed < epoch) {
        return None;
    }
    let json = open_op(|e| creds.keys.get(e), &creds.vault_id.0, &op.ct).ok()?;
    let cs = changeset::from_json(&json).ok()?;
    HeadRecord::decode(cs.rekey_head.as_deref()?).ok()
}

/// `entry`'s head is verified at `head`: a cursor that had already passed it when the key was
/// adopted is reported (the server hid the key change from this device).
fn note_verified(conn: &Connection, entry: &UnverifiedEpoch, head: u64) -> Result<()> {
    log::info!("[SYNC] The start of key epoch {} is verified", entry.epoch);
    if entry.after > head {
        log::warn!(
            "[SYNC] This device applied changes past the start of key epoch {}: the server hid \
             the key change from it",
            entry.epoch
        );
        super::write_key_change_overrun(
            conn,
            &KeyChangeOverrun {
                epoch: entry.epoch,
                head,
                through: entry.after,
            },
        )?;
    }
    Ok(())
}

/// After a snapshot restore at relay position `seq` (in its savepoint): a provisional head the
/// snapshot's `Keys` record names alike is verified when the snapshot lies at or past it (only a
/// holder of that epoch's key can name it there, since the record carries the key, a dumper names
/// only heads it verified, and a device at or past the head has no range to check); the look-ahead
/// of every other one starts at `seq`, below which the server pruned the ops.
pub(crate) fn after_restore(conn: &Connection, seq: u64, heads: &[(u32, u64)]) -> Result<()> {
    let epochs = unverified_epochs(conn)?;
    if epochs.is_empty() {
        return Ok(());
    }
    let mut left = Vec::new();
    for mut entry in epochs {
        // Only `verify_own` settles this device's own head: it has to sign it.
        if entry.own_pending {
            left.push(entry);
            continue;
        }
        let named = heads
            .iter()
            .find(|(epoch, head)| *epoch == entry.epoch && entry.head.is_none_or(|h| h == *head));
        if let Some((_, head)) = named.filter(|(_, head)| !entry.refuted && seq >= *head) {
            note_verified(conn, &entry, *head)?;
            continue;
        }
        entry.scanned = entry.scanned.max(seq);
        left.push(entry);
    }
    store(conn, &left)
}

/// What `settle` made of a signed record.
enum Settled {
    Verified,
    Refuted,
    /// The range could not be read to its end: the entry stays.
    Waiting,
}

/// What a found record asks of this device.
enum Plan {
    Refute,
    /// This device's watermark is at or past the head: nothing to read.
    PastHead,
    /// Read the range `(record.base, record.head]` and compare it.
    ReadRange,
}

/// The numbers of `record` against `entry` and this device's watermark (no network).
fn plan_of(conn: &Connection, entry: &UnverifiedEpoch, record: &HeadRecord) -> Result<Plan> {
    let mut matches = record.from_epoch.checked_add(1) == Some(record.to_epoch)
        && record.base <= record.head
        && entry.head.is_none_or(|head| head == record.head)
        && entry
            .rotation
            .is_none_or(|rotation| rotation == record.rotation);
    if matches && entry.head.is_none() {
        // A Kit joiner takes the record's head; one recorded here differently refutes it.
        matches = state::epochs(conn)?
            .get(&entry.epoch)
            .is_none_or(|held| *held == record.head);
    }
    if !matches {
        return Ok(Plan::Refute);
    }
    // The watermark, not the place: a held op above the head says nothing of the positions
    // between the cursor and it (audit-2 reading A, A-1).
    let mark = seen::watermark(conn)?.unwrap_or(0);
    Ok(if mark >= record.head {
        Plan::PastHead
    } else {
        Plan::ReadRange
    })
}

/// A signed head found for `entry` (module doc): its numbers match and the range, read by this
/// device from the signed base, has the signed count and digest (the entry goes, with the notes
/// stored), or not (the entry is refuted and nothing of the range is applied). A device whose
/// watermark is at or past the head reads nothing. The read is a network call, made outside any
/// connection hold; notes, base and the entry's removal go in one savepoint only after count and
/// digest matched.
async fn settle<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    entry: &UnverifiedEpoch,
    record: &HeadRecord,
    now: i64,
) -> SyncResult<Settled> {
    let refuted = || {
        log::warn!(
            "[SYNC] The sync server reported another start for key epoch {} than its signer",
            entry.epoch
        );
        db.with_conn(|conn| mark_refuted(conn, entry.epoch))
            .map(|()| Settled::Refuted)
            .map_err(SyncError::from)
    };
    let notes = match db.with_conn(|conn| plan_of(conn, entry, record))? {
        Plan::Refute => return refuted(),
        Plan::PastHead => None,
        Plan::ReadRange => {
            let read = range::read_range(
                relay,
                &creds.auth(),
                record.base,
                record.head,
                MAX_PULL_PAGES,
            )
            .await;
            match read {
                Ok(RangeRead::Complete(notes)) => {
                    let fits = u32::try_from(notes.len()).is_ok_and(|n| n == record.count)
                        && range::range_digest(
                            &creds.vault_id.0,
                            record.to_epoch,
                            record.base,
                            record.head,
                            &notes,
                        )
                        .is_ok_and(|digest| digest == record.digest);
                    if !fits {
                        log::warn!(
                            "[SYNC] The changes below the start of key epoch {} are not the ones its signer saw",
                            entry.epoch
                        );
                        return refuted();
                    }
                    Some(notes)
                }
                Ok(RangeRead::More { scanned }) => {
                    log::info!(
                        "[SYNC] The range of key epoch {} is longer than one read ({scanned})",
                        entry.epoch
                    );
                    return Ok(Settled::Waiting);
                }
                // The server pruned above the base: a snapshot under the new key that names the
                // head settles it (`after_restore`).
                Err(SyncError::SnapshotRequired) => {
                    log::info!("[SYNC] The range of key epoch {} was pruned", entry.epoch);
                    return Ok(Settled::Waiting);
                }
                Err(error) => return Err(error),
            }
        }
    };
    let verified = db.with_conn(|conn| commit(conn, entry, record, notes.as_deref(), now))?;
    Ok(if verified {
        Settled::Verified
    } else {
        Settled::Refuted
    })
}

/// Marks `epoch`'s entry refuted: `keyChangeUnverified` for good.
fn mark_refuted(conn: &Connection, epoch: u32) -> Result<()> {
    let mut epochs = unverified_epochs(conn)?;
    for e in epochs.iter_mut().filter(|e| e.epoch == epoch) {
        e.refuted = true;
    }
    store(conn, &epochs)
}

/// The head of `entry` is verified (`notes`: the range this device read, `None` when it was at or
/// past the head): in one savepoint the epoch's start is recorded (a Kit joiner's has none yet),
/// the notes and base stored, the signer recorded and the entry removed. `false` when the record's
/// head contradicts one recorded meanwhile: the entry is refuted then.
fn commit(
    conn: &Connection,
    entry: &UnverifiedEpoch,
    record: &HeadRecord,
    notes: Option<&[range::RangeNote]>,
    now: i64,
) -> Result<bool> {
    in_savepoint(conn, "sync_settle_head", || {
        let mut epochs = unverified_epochs(conn)?;
        let waiting = |e: &UnverifiedEpoch| e.epoch == entry.epoch && !e.own_pending;
        if !epochs.iter().any(waiting) {
            // Settled meanwhile.
            return Ok(true);
        }
        let recorded =
            state::record_epoch(conn, entry.epoch, record.head, Some(record.signer), now)?;
        if recorded == state::EpochRecord::Differs {
            mark_refuted(conn, entry.epoch)?;
            return Ok(false);
        }
        state::set_epoch_signer(conn, entry.epoch, record.signer)?;
        if let Some(notes) = notes {
            range::store_notes(conn, entry.epoch, record.base, notes)?;
        }
        epochs.retain(|e| !waiting(e));
        store(conn, &epochs)?;
        note_verified(conn, entry, record.head)?;
        // Every device that verified a key change owes the vault a snapshot under the new key
        // when the vault has none above its head (rule 10).
        state::owe_snapshot(conn, entry.epoch)?;
        Ok(true)
    })
}

/// A device that holds a head for an epoch without a range stored for it and whose watermark
/// (`mark`, `seen::watermark`) is below that head has not checked what lies between (spec §2.3
/// step 8): an entry for the epoch, no rotation, the head from `sync_epochs`, `after` and
/// `scanned` at the watermark. Epochs that
/// already have an entry are left alone (a refuted one always; with `replaced`, a waiting one
/// starts again at the new place and its range is forgotten with it). `replaced`: the profile was
/// replaced, so for every epoch whose head lies above the watermark the range stored before goes
/// (`range::forget`) and the check starts again (rule 6, spec §2.3 step 6).
pub(crate) fn require_ranges(conn: &Connection, mark: u64, replaced: bool) -> Result<()> {
    let mut epochs = unverified_epochs(conn)?;
    let mut changed = false;
    for (epoch, head) in state::epochs(conn)? {
        if epoch == 0 || head <= mark {
            continue;
        }
        let existing = epochs.iter().find(|e| e.epoch == epoch);
        if existing.is_some_and(|e| e.refuted || e.own_pending) {
            continue;
        }
        if existing.is_some() && !replaced {
            continue;
        }
        if !replaced && range::base_of(conn, epoch)?.is_some() {
            continue;
        }
        range::forget(conn, epoch)?;
        epochs.retain(|e| e.epoch != epoch);
        epochs.push(UnverifiedEpoch {
            epoch,
            head: Some(head),
            rotation: None,
            by: state::epoch_signer(conn, epoch)?
                .map(|by| by.to_hex())
                .unwrap_or_default(),
            after: mark,
            scanned: mark,
            refuted: false,
            own_pending: false,
        });
        changed = true;
    }
    if changed {
        epochs.sort_by_key(|e| e.epoch);
        store(conn, &epochs)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::identity::{new_sign_seed, sign_pub};
    use range::RangeNote;

    /// The real digest of the two-note range, tied to the frozen constant of `range.rs`.
    fn two_note_digest() -> [u8; 32] {
        let notes = [
            RangeNote {
                pos: 11,
                hash: [0xaa; 32],
            },
            RangeNote {
                pos: 13,
                hash: [0xbb; 32],
            },
        ];
        let digest = range::range_digest(&[0x22; 16], 4, 10, 13, &notes).expect("digest");
        assert_eq!(hex(&digest), range::FROZEN_TWO_NOTE_DIGEST);
        digest
    }

    fn sample(seed: &SignSeed) -> HeadRecord {
        HeadRecord::sign(
            [0x22; 16],
            DeviceId([1; 16]),
            3,
            4,
            9,
            13,
            10,
            2,
            two_note_digest(),
            seed,
        )
    }

    fn hex(bytes: &[u8]) -> String {
        bytes.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_signed_head_is_frozen_byte_for_byte() {
        let seed = SignSeed::from_bytes(zeroize::Zeroizing::new([0x77; 32]));
        let record = sample(&seed);
        let signed = record.signed_bytes();
        assert_eq!(signed.len(), SIGNED_LEN);
        assert_eq!(signed[83..115], two_note_digest());
        assert_eq!(
            hex(&signed),
            "6d6f6f6e792072656b657920686561642076322222222222222222222222222222222201010101010101010101010101010101000000030000000400000009000000000000000d000000000000000a000000020c4975dbc30e2f9866ff89c16bdd9d42330faf65f2e5e009a7b2ddd98b08e870"
        );
        assert_eq!(
            hex(&record.encode()),
            "6d6f6f6e792072656b657920686561642076322222222222222222222222222222222201010101010101010101010101010101000000030000000400000009000000000000000d000000000000000a000000020c4975dbc30e2f9866ff89c16bdd9d42330faf65f2e5e009a7b2ddd98b08e870ac8071ec7de095282ab9578ff132bfeed6dc43fb646fb8fd8492bdcb32dde5ababe67e57dc0abb264debd694dedb58253a9e63210661ea155883288e43233a00"
        );
    }

    #[test]
    fn a_signed_head_v2_round_trips_and_a_changed_byte_does_not_verify() {
        let seed = new_sign_seed().expect("seed");
        let record = sample(&seed);
        let bytes = record.encode();
        assert_eq!(bytes.len(), HEAD_LEN);
        let back = HeadRecord::decode(&bytes).expect("decodes");
        assert_eq!(back, record);
        back.verify(&sign_pub(&seed)).expect("verifies");
        // every signed byte (the domain ones do not even decode) and every signature byte
        for at in 0..HEAD_LEN {
            let mut changed = bytes.clone();
            changed[at] ^= 1;
            match HeadRecord::decode(&changed) {
                Err(_) => assert!(at < HEAD_DOMAIN.len(), "byte {at} must decode"),
                Ok(decoded) => {
                    assert!(decoded.verify(&sign_pub(&seed)).is_err(), "byte {at}")
                }
            }
        }
        assert!(HeadRecord::decode(&bytes[1..]).is_err());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(HeadRecord::decode(&longer).is_err());
        let other = new_sign_seed().expect("seed");
        assert!(back.verify(&sign_pub(&other)).is_err());
    }

    #[test]
    fn a_version_1_record_does_not_decode() {
        let seed = new_sign_seed().expect("seed");
        let mut bytes = sample(&seed).encode();
        bytes[18] = b'1';
        assert!(HeadRecord::decode(&bytes).is_err());
        // a real version-1 record: shorter, other domain
        let mut v1 = b"moony rekey head v1".to_vec();
        v1.extend_from_slice(&[0u8; 16 + 16 + 4 + 4 + 4 + 8 + 64]);
        assert!(HeadRecord::decode(&v1).is_err());
    }
}
