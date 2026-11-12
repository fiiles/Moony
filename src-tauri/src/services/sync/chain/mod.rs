//! Chain bookkeeping (spec §9.3, §11.4, §14): the verdict on every pulled envelope, from its
//! author's chain head (`sync_heads`) or, for this device's own ID, from `sync_own_log`; the
//! barrier marker found by relay order; and the re-queue when this device crosses a barrier.
//! The re-join after a clone or rollback lives in `rejoin`; held gaps and skipping them in `gaps`.

pub mod gaps;
mod rejoin;

pub use rejoin::*;

use rusqlite::{params, Connection, OptionalExtension};

use super::changeset::Changeset;
use super::hlc::DeviceId;
use super::in_savepoint;
use super::registry::keys::{barrier_number, schema_number};
use super::state::{self, SyncState};
use crate::error::{AppError, Result};

/// What the engine needs of a pulled envelope to judge it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvelopeMeta {
    pub device: DeviceId,
    pub seq: u64,
    pub prev: Option<[u8; 32]>,
    /// SHA-256 of the envelope.
    pub hash: [u8; 32],
    pub schema: u32,
    pub barrier: u32,
    /// The relay's sequence of the op that carried the envelope: its place in relay order, the
    /// same on every device.
    pub position: u64,
}

impl EnvelopeMeta {
    pub fn of(cs: &Changeset, hash: [u8; 32], position: u64) -> Self {
        Self {
            device: cs.device,
            seq: cs.seq,
            prev: cs.prev,
            hash,
            schema: cs.schema,
            barrier: cs.barrier,
            position,
        }
    }
}

/// Records the barrier marker (§14): the envelope earliest in relay order written at or above the
/// barrier this device is past. Called before `judge` on every envelope that decrypts, whatever its
/// verdict: a position in relay order is the same for every device, and the smallest one wins
/// (ruling T8-5), so every device finds the same marker in whatever order it notes positions.
/// `true` when `meta` became the marker (`sync_state.barrier_seq` = its position).
///
/// The marker tells a device below the barrier where to stop; a device past the barrier skips
/// every envelope written below it wherever it lies (`judge`, ruling T8-2).
pub fn note_barrier_marker(conn: &Connection, meta: &EnvelopeMeta) -> Result<bool> {
    let sync = loaded(conn)?;
    let earlier = sync.barrier_seq.is_none_or(|m| meta.position < m);
    if sync.barrier == 0 || meta.barrier < sync.barrier || !earlier {
        return Ok(false);
    }
    state::set_barrier_seq(conn, Some(meta.position))?;
    Ok(true)
}

/// Whether a snapshot heals what this device missed, by skipping below-barrier envelopes (rulings
/// T8-2, T8-6) or by skipping a change that never arrived (`skipped`, Task 17 ruling I1): its
/// header says it was dumped at this device's barrier or later (`header_barrier`), at relay
/// sequence `header_seq` at or past every missed position, by a device with no miss of either
/// kind (`header_missed`, the max of the dumper's two, is `None`), which therefore applied what
/// this one lacks. `false` when nothing is missed.
///
/// The engine (Tasks 11 and 13) restores by merge from the relay's newest snapshot once this
/// holds; `snapshot::restore_merge` then clears both marks. Every device that crosses a
/// barrier uploads a snapshot at its first clean round after crossing (forced, outside the upload
/// policy, but never against `may_upload_snapshot`), so such a snapshot appears. Known limit
/// (ADR 0025): an envelope that no device applied before crossing stays missed by the devices
/// that crossed early; a skipped change no device has stays missed for good.
pub fn snapshot_heals_missed(
    sync: &SyncState,
    skipped: Option<u64>,
    header_barrier: u32,
    header_seq: u64,
    header_missed: Option<u64>,
) -> bool {
    sync.missed_below_barrier
        .max(skipped)
        .is_some_and(|missed| {
            header_missed.is_none() && header_barrier >= sync.barrier && header_seq >= missed
        })
}

/// Whether this device may upload a snapshot (ruling T8-6): never while it has skipped a
/// below-barrier envelope it has not healed, since its snapshot lacks that envelope's rows. The
/// engine's upload policy checks it before every upload, the forced one after a barrier crossing
/// included. A skipped change that never arrived (`state::skipped_miss`) does not hold the upload
/// back (Task 17 ruling I1): its position goes into the snapshot's header instead.
pub fn may_upload_snapshot(sync: &SyncState) -> bool {
    sync.missed_below_barrier.is_none()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The author's next envelope: merge it and move the head.
    Apply,
    /// Already applied (a replay): skip it.
    SkipReplay,
    /// This device's own envelope, as sealed here, back from the relay: acknowledged
    /// (`acknowledge_own`).
    OwnAcknowledged,
    /// This device's ID on an envelope it never sealed: a clone or a rollback. Re-join.
    Clone,
    /// The author's chain forked: quarantine that author (`quarantine`), everything else syncs.
    Quarantine,
    /// The author is quarantined: keep its envelopes (replays included) and do not advance the
    /// cursor past them.
    Held,
    /// An envelope of the author is missing (the relay dropped it): stop until it arrives.
    Gap,
    /// Written by a newer schema than this build knows: stop pulling, keep pushing.
    NewerSchema,
    /// Written below the barrier this device is past: not applied, the head moves and the miss is
    /// recorded (`skip_below_barrier`), wherever the relay placed it (ruling T8-2).
    BelowBarrier,
    /// The chain link itself is malformed (`seq` 0, a first envelope with a `prev`, a later one
    /// without): stop.
    Invalid,
}

/// The verdict on `meta`, from `sync_state`, `sync_heads` and `sync_own_log`. Writes nothing.
pub fn judge(conn: &Connection, meta: &EnvelopeMeta) -> Result<Verdict> {
    let sync = loaded(conn)?;
    if meta.seq == 0 || (meta.seq == 1) != meta.prev.is_none() {
        return Ok(Verdict::Invalid);
    }
    if meta.device == sync.device {
        let sealed: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sync_own_log WHERE seq = ?1 AND hash = ?2)",
            params![seq_param(meta.seq)?, meta.hash.to_vec()],
            |r| r.get(0),
        )?;
        return Ok(if sealed {
            Verdict::OwnAcknowledged
        } else {
            Verdict::Clone
        });
    }
    if meta.schema > schema_number() {
        return Ok(Verdict::NewerSchema);
    }
    let head = head_of(conn, meta.device)?;
    // `Held` means keep the envelope and do not advance the cursor past it, even when it is a
    // replay of the quarantined author: the quarantine is resolved first.
    if head.as_ref().is_some_and(|h| h.quarantined) {
        return Ok(Verdict::Held);
    }
    // Below the barrier is never applied, even before the marker (ruling T8-2): a device that
    // crossed with a stale cursor would apply it raw while the others skip it. The skip is
    // recorded and healed by a snapshot (`snapshot_heals_missed`).
    let next = || {
        if meta.barrier < sync.barrier {
            Verdict::BelowBarrier
        } else {
            Verdict::Apply
        }
    };
    let Some(head) = head else {
        return Ok(if meta.seq == 1 { next() } else { Verdict::Gap });
    };
    Ok(match meta.seq {
        seq if seq < head.seq => Verdict::SkipReplay,
        seq if seq == head.seq && meta.hash == head.hash => Verdict::SkipReplay,
        seq if seq == head.seq => Verdict::Quarantine,
        seq if seq == head.seq + 1 && meta.prev == Some(head.hash) => next(),
        seq if seq == head.seq + 1 => Verdict::Quarantine,
        _ => Verdict::Gap,
    })
}

/// This device pulled its own envelope `seq` back from the relay (its echo): the relay holds it,
/// and it no longer waits in `sync_unsent`. With `acknowledge_own_through`, the only way a row
/// leaves `sync_unsent` while the chain goes on (audit M4): a push answer alone removes nothing,
/// so a relay that acknowledges and then loses an envelope gets it again.
pub fn acknowledge_own(conn: &Connection, seq: u64) -> Result<()> {
    conn.execute("DELETE FROM sync_unsent WHERE seq = ?1", [seq_param(seq)?])?;
    Ok(())
}

/// A snapshot restored by merge carries the dumper's head for this device, `(seq, hash)`: when it
/// names this device's own envelope `seq` (`sync_own_log`), the dumper applied this device's chain
/// up to it, so the vault holds every own envelope up to `seq`, and they stop waiting for an echo
/// the restore's cursor jump has passed. A head that names no own envelope (another chain under
/// this ID) acknowledges nothing. Returns how many rows left `sync_unsent`.
pub fn acknowledge_own_through(conn: &Connection, seq: u64, hash: [u8; 32]) -> Result<usize> {
    let ours: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sync_own_log WHERE seq = ?1 AND hash = ?2)",
        params![seq_param(seq)?, hash.to_vec()],
        |r| r.get(0),
    )?;
    if !ours {
        return Ok(0);
    }
    Ok(conn.execute("DELETE FROM sync_unsent WHERE seq <= ?1", [seq_param(seq)?])?)
}

/// Holds every further envelope of `device` (its chain forked). Only a device with a head can
/// fork, so a device without one is an error.
pub fn quarantine(conn: &Connection, device: DeviceId) -> Result<()> {
    let marked = conn.execute(
        "UPDATE sync_heads SET quarantined = 1 WHERE device = ?1",
        [device.0.to_vec()],
    )?;
    if marked != 1 {
        return Err(AppError::Internal(format!(
            "sync chain: no head to quarantine for device {}",
            device.to_hex()
        )));
    }
    Ok(())
}

/// Moves the author's head past an envelope written below the barrier, without applying it, and
/// records its relay position as missed (`state::note_missed_below_barrier`, the newest kept).
pub fn skip_below_barrier(conn: &Connection, meta: &EnvelopeMeta) -> Result<()> {
    in_savepoint(conn, "sync_skip_below_barrier", || {
        set_head(conn, meta.device, meta.seq, meta.hash)?;
        state::note_missed_below_barrier(conn, meta.position)
    })
}

/// At unlock (§14): when this build is past a barrier the profile was last opened below, the rows
/// this device changed after the newest snapshot it restored or uploaded are queued again (in
/// the new format), and the barrier is recorded; the old barrier's marker is forgotten, so
/// `note_barrier_marker` looks for the new one. Returns how many keys were queued.
pub fn cross_barrier(conn: &Connection, sync: &SyncState) -> Result<usize> {
    let barrier = barrier_number();
    if sync.schema_seen >= barrier {
        return Ok(0);
    }
    in_savepoint(conn, "sync_cross_barrier", || {
        let queued = requeue_own_since(conn, sync.device, sync.snapshot_hlc)?;
        conn.execute(
            "UPDATE sync_state SET barrier = MAX(barrier, ?1), barrier_seq = NULL WHERE id = 1",
            [barrier],
        )?;
        Ok(queued)
    })
}

struct Head {
    seq: u64,
    hash: [u8; 32],
    quarantined: bool,
}

fn head_of(conn: &Connection, device: DeviceId) -> Result<Option<Head>> {
    let raw: Option<(i64, Vec<u8>, bool)> = conn
        .query_row(
            "SELECT seq, hash, quarantined FROM sync_heads WHERE device = ?1",
            [device.0.to_vec()],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    raw.map(|(seq, hash, quarantined)| {
        Ok(Head {
            seq: unsigned(seq, "seq")?,
            hash: hash
                .as_slice()
                .try_into()
                .map_err(|_| AppError::Internal("sync_heads.hash is not 32 bytes".into()))?,
            quarantined,
        })
    })
    .transpose()
}

fn set_head(conn: &Connection, device: DeviceId, seq: u64, hash: [u8; 32]) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_heads (device, seq, hash) VALUES (?1, ?2, ?3) \
         ON CONFLICT(device) DO UPDATE SET seq = excluded.seq, hash = excluded.hash",
        params![device.0.to_vec(), seq_param(seq)?, hash.to_vec()],
    )?;
    Ok(())
}

fn loaded(conn: &Connection) -> Result<SyncState> {
    state::load(conn)?.ok_or_else(|| AppError::Internal("sync chain: sync is not set up".into()))
}

fn seq_param(seq: u64) -> Result<i64> {
    i64::try_from(seq)
        .map_err(|_| AppError::Internal(format!("sync chain: sequence {seq} is out of range")))
}

fn unsigned(value: i64, column: &str) -> Result<u64> {
    u64::try_from(value)
        .map_err(|_| AppError::Internal(format!("sync chain: {column} {value} is out of range")))
}

#[cfg(test)]
mod tests;
