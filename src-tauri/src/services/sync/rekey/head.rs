//! The signed head of a key change (phase 5 As built 62–65).
//!
//! The relay position at which an epoch starts (`sync_epochs`) is what the epoch rule (§6.1)
//! checks every op against, and the server reports it. A server colluding with a removed device
//! (which keeps the old key) could take that device's pushes after the rotation and tell another
//! device a head above them: the old-key ops would then fit. So the head is signed: the remover
//! seals, in the savepoint of its commit, its first envelope under the new key with no rows and
//! a `HeadRecord` {vault, from, to, rotation, head} signed by its device key (`queue_signed_head`);
//! it travels through `sync_unsent` like any envelope, so a crash cannot lose it, and only a
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
//! A device that had already applied ops past the verified head (the server hid the key change
//! from it) learns it here and keeps a notice (`rekey::KeyChangeOverrun`); nothing is undone.

use std::collections::BTreeMap;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::range;
use super::{read_config, write_config, KeyChangeOverrun};
use crate::error::{AppError, Result};
use crate::services::sync::changeset;
use crate::services::sync::engine::pull::PULL_PAGE;
use crate::services::sync::engine::push::seal_into_unsent;
use crate::services::sync::engine::ConnAccess;
use crate::services::sync::envelope::{envelope_epoch, open_op};
use crate::services::sync::error::SyncResult;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::{self, trust, verify_sig, SignSeed};
use crate::services::sync::journal::build_marker;
use crate::services::sync::keys::{self, Credentials};
use crate::services::sync::relay::{PulledOp, Relay, RelayError, Rotated};
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
    /// The bundle's signer (hex), whose signed head confirms it.
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
}

/// The epochs whose head is not verified yet (empty when none or when the value does not read).
pub fn unverified_epochs(conn: &Connection) -> Result<Vec<UnverifiedEpoch>> {
    Ok(read_config(conn, state::UNVERIFIED_EPOCHS_KEY)?.unwrap_or_default())
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
    let mut epochs = unverified_epochs(conn)?;
    epochs.retain(|e| e.epoch != epoch);
    epochs.push(UnverifiedEpoch {
        epoch,
        head,
        rotation,
        by: by.to_hex(),
        after,
        scanned: head.unwrap_or(after),
        refuted: false,
    });
    epochs.sort_by_key(|e| e.epoch);
    store(conn, &epochs)
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

/// The remover's commit (in its savepoint, the new key already current): its next envelope,
/// with no rows, carries the signed head of the change, sealed under the new key into
/// `sync_unsent` (so it goes out with the next push, a crash or not).
pub(crate) fn queue_signed_head(
    conn: &Connection,
    me: DeviceId,
    from: u32,
    to: u32,
    rotated: Rotated,
) -> Result<()> {
    let creds = keys::credentials(conn)?
        .ok_or_else(|| AppError::Internal("sync: the key change has no credentials".into()))?;
    if creds.keys.current_epoch() != to {
        return Err(AppError::Internal(
            "sync: the signed head is sealed under another key".into(),
        ));
    }
    let seed = identity::own_seed(conn)?
        .ok_or_else(|| AppError::Internal("sync: no signing key for the signed head".into()))?;
    // Placeholder: an empty range at the head; a later task (the remover's range read) replaces it.
    let digest = range::range_digest(&creds.vault_id.0, to, rotated.head, rotated.head, &[])?;
    let record = HeadRecord::sign(
        creds.vault_id.0,
        me,
        from,
        to,
        rotated.rotation,
        rotated.head,
        rotated.head,
        0,
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
        match look_ahead(relay, db, creds, &entry).await? {
            Found::Record(record) => {
                let verified = db.with_conn(|conn| settle(conn, &entry, &record, now))?;
                if !verified {
                    return Ok(HeadCheck::Refuted);
                }
            }
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
    let mut after = entry
        .head
        .map_or(entry.scanned, |head| entry.scanned.max(head));
    let Ok(by) = DeviceId::from_hex(&entry.by) else {
        return Ok(Found::NotYet(after));
    };
    let Some(signer_pub) = db
        .with_conn(|conn| trust::entry(conn, by))?
        .map(|e| e.sign_pub)
    else {
        return Ok(Found::NotYet(after));
    };
    let auth = creds.auth();
    for _ in 0..MAX_LOOKAHEAD_PAGES {
        let page = match relay.pull(&auth, after, LOOKAHEAD_PAGE).await {
            Ok(page) => page,
            Err(RelayError::SnapshotRequired) => {
                // Pruned below a snapshot: go on from this device's place, past the floor once
                // its own pull has restored (`after_restore` moves `scanned` there too).
                let cursor =
                    db.with_conn(|conn| Ok(state::load(conn)?.map_or(0, |sync| sync.cursor)))?;
                log::info!("[SYNC] The ops after a key change were pruned: looking past them");
                after = after.max(cursor);
                break;
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
    Ok(Found::NotYet(after))
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
/// snapshot's `Keys` record names alike is verified (only a holder of that epoch's key can name it
/// there, since the record carries the key, and a dumper names only heads it verified); the
/// look-ahead of every other one starts at `seq`, below which the server pruned the ops.
pub(crate) fn after_restore(conn: &Connection, seq: u64, heads: &[(u32, u64)]) -> Result<()> {
    let epochs = unverified_epochs(conn)?;
    if epochs.is_empty() {
        return Ok(());
    }
    let mut left = Vec::new();
    for mut entry in epochs {
        let named = heads
            .iter()
            .find(|(epoch, head)| *epoch == entry.epoch && entry.head.is_none_or(|h| h == *head));
        if let Some((_, head)) = named.filter(|_| !entry.refuted) {
            note_verified(conn, &entry, *head)?;
            continue;
        }
        entry.scanned = entry.scanned.max(seq);
        left.push(entry);
    }
    store(conn, &left)
}

/// A signed head found for `entry`: it matches (the entry goes) or not (the entry is refuted).
/// An entry with no server head (a Kit joiner's, revision §2.6 step 5) takes the record's head
/// and rotation, recorded in `sync_epochs` (a different head there refutes). Whether it matched.
fn settle(
    conn: &Connection,
    entry: &UnverifiedEpoch,
    record: &HeadRecord,
    now: i64,
) -> Result<bool> {
    let mut epochs = unverified_epochs(conn)?;
    let mut matches = record.from_epoch.checked_add(1) == Some(record.to_epoch)
        && entry.head.is_none_or(|head| head == record.head)
        && entry
            .rotation
            .is_none_or(|rotation| rotation == record.rotation);
    if matches && entry.head.is_none() {
        let recorded =
            state::record_epoch(conn, entry.epoch, record.head, Some(record.signer), now)?;
        matches = recorded != state::EpochRecord::Differs;
    }
    if matches {
        epochs.retain(|e| e.epoch != entry.epoch);
        note_verified(conn, entry, record.head)?;
    } else {
        log::warn!(
            "[SYNC] The sync server reported another start for key epoch {} than its signer",
            entry.epoch
        );
        for e in epochs.iter_mut().filter(|e| e.epoch == entry.epoch) {
            e.refuted = true;
        }
    }
    store(conn, &epochs)?;
    Ok(matches)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::identity::{new_sign_seed, sign_pub};

    fn sample(seed: &SignSeed) -> HeadRecord {
        HeadRecord::sign(
            [0x22; 16],
            DeviceId([1; 16]),
            3,
            4,
            9,
            1234,
            1200,
            2,
            [0x5c; 32],
            seed,
        )
    }

    #[test]
    fn a_signed_head_v2_round_trips_and_a_changed_byte_does_not_verify() {
        let seed = new_sign_seed().expect("seed");
        let record = sample(&seed);
        let bytes = record.encode();
        assert_eq!(bytes.len(), HEAD_LEN);
        assert_eq!(&bytes[..19], b"moony rekey head v2");
        assert_eq!(
            bytes[..19]
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
            "6d6f6f6e792072656b65792068656164207632"
        );
        let back = HeadRecord::decode(&bytes).expect("decodes");
        assert_eq!(back, record);
        back.verify(&sign_pub(&seed)).expect("verifies");
        // inside the older fields, the new ones (base, count, digest) and the last signed byte
        for at in [
            20,
            40,
            60,
            70,
            83,
            92,
            100,
            120,
            SIGNED_LEN - 1,
            HEAD_LEN - 1,
        ] {
            let mut changed = bytes.clone();
            changed[at] ^= 1;
            let decoded = HeadRecord::decode(&changed).expect("decodes");
            assert!(decoded.verify(&sign_pub(&seed)).is_err(), "byte {at}");
        }
        assert!(HeadRecord::decode(&bytes[1..]).is_err());
        let mut longer = bytes.clone();
        longer.push(0);
        assert!(HeadRecord::decode(&longer).is_err());
        let mut other_domain = bytes.clone();
        other_domain[0] ^= 1;
        assert!(HeadRecord::decode(&other_domain).is_err());
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
