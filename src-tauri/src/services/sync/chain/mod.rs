//! Chain bookkeeping (spec §9.3, §11.4, §14): the verdict on every pulled envelope, from its
//! author's chain head (`sync_heads`) or, for this device's own ID, from `sync_own_log`; the
//! re-join after a clone or rollback; and the re-queue when this device crosses a barrier.

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
}

impl EnvelopeMeta {
    pub fn of(cs: &Changeset, hash: [u8; 32]) -> Self {
        Self {
            device: cs.device,
            seq: cs.seq,
            prev: cs.prev,
            hash,
            schema: cs.schema,
            barrier: cs.barrier,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The author's next envelope: merge it and move the head.
    Apply,
    /// Already applied (a replay): skip it.
    SkipReplay,
    /// This device's own envelope, as sealed here: acknowledged (`acknowledge_own`).
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
    /// Written below the barrier this device is past: not applied, the head moves
    /// (`skip_below_barrier`).
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

/// The relay holds this device's envelope `seq`: it no longer waits in `sync_unsent`.
pub fn acknowledge_own(conn: &Connection, seq: u64) -> Result<()> {
    conn.execute("DELETE FROM sync_unsent WHERE seq = ?1", [seq_param(seq)?])?;
    Ok(())
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

/// Moves the author's head past an envelope written below the barrier, without applying it.
pub fn skip_below_barrier(conn: &Connection, meta: &EnvelopeMeta) -> Result<()> {
    set_head(conn, meta.device, meta.seq, meta.hash)
}

/// At unlock (§14): when this build is past a barrier the profile was last opened below, the rows
/// this device changed after the newest snapshot it restored or uploaded are queued again (in
/// the new format), and the barrier is recorded. Returns how many keys were queued.
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
mod tests {
    use super::*;
    use crate::services::sync::capture;
    use crate::services::sync::test_support::migrated;

    pub(super) const ME: DeviceId = DeviceId([7; 16]);
    pub(super) const A: DeviceId = DeviceId([1; 16]);
    pub(super) const B: DeviceId = DeviceId([2; 16]);
    pub(super) const NEW: DeviceId = DeviceId([9; 16]);

    pub(super) fn h(n: u8) -> [u8; 32] {
        [n; 32]
    }

    /// A sync-enabled profile of `ME` without capture, nothing queued.
    pub(super) fn profile() -> Connection {
        let conn = migrated();
        state::ensure_row(&conn, ME).unwrap();
        conn
    }

    pub(super) fn meta(device: DeviceId, seq: u64, prev: Option<u8>, hash: u8) -> EnvelopeMeta {
        EnvelopeMeta {
            device,
            seq,
            prev: prev.map(h),
            hash: h(hash),
            schema: schema_number(),
            barrier: barrier_number(),
        }
    }

    pub(super) fn head(conn: &Connection, device: DeviceId, seq: u64, hash: u8) {
        set_head(conn, device, seq, h(hash)).unwrap();
    }

    pub(super) fn heads(conn: &Connection) -> Vec<(Vec<u8>, i64, Vec<u8>, bool)> {
        conn.prepare("SELECT device, seq, hash, quarantined FROM sync_heads ORDER BY device")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    /// Own envelope `seq` in the log, with its own clocks spanning `min_hlc..=max_hlc`.
    pub(super) fn sealed(
        conn: &Connection,
        seq: i64,
        hash: u8,
        min_hlc: i64,
        max_hlc: i64,
        unsent: bool,
    ) {
        conn.execute(
            "INSERT INTO sync_own_log (seq, hash, min_hlc, max_hlc) VALUES (?1, ?2, ?3, ?4)",
            params![seq, h(hash).to_vec(), min_hlc, max_hlc],
        )
        .unwrap();
        if unsent {
            conn.execute(
                "INSERT INTO sync_unsent (seq, envelope) VALUES (?1, x'00')",
                [seq],
            )
            .unwrap();
        }
    }

    pub(super) fn clock(conn: &Connection, pk: &str, col: &str, hlc: i64, device: DeviceId) {
        conn.execute(
            "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES ('institutions', ?1, ?2, ?3, ?4)",
            params![pk, col, hlc, device.0.to_vec()],
        )
        .unwrap();
    }

    pub(super) fn outbox(conn: &Connection) -> Vec<(String, String, i64)> {
        conn.prepare("SELECT tbl, pk, queued_at FROM sync_outbox ORDER BY tbl, pk")
            .unwrap()
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    pub(super) fn queued(pk: &str, at: i64) -> (String, String, i64) {
        ("institutions".to_string(), pk.to_string(), at)
    }

    pub(super) fn unsent(conn: &Connection) -> Vec<i64> {
        conn.prepare("SELECT seq FROM sync_unsent ORDER BY seq")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    pub(super) fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).expect(sql)
    }

    /// Own clocks at 150, 250 (and 270 on a column of the same key) and 350, one other
    /// device's clock at 400, a tombstone of this device at 360; nothing queued.
    pub(super) fn own_clocks(conn: &Connection) {
        clock(conn, "i150", "*", 150, ME);
        clock(conn, "i250", "*", 250, ME);
        clock(conn, "i250", "name", 270, ME);
        clock(conn, "i350", "*", 350, ME);
        clock(conn, "dead", "-", 360, ME);
        clock(conn, "theirs", "*", 400, A);
        conn.execute_batch("DELETE FROM sync_outbox").unwrap();
    }

    #[test]
    fn the_next_sequence_with_the_right_prev_is_applied() {
        let conn = profile();
        assert_eq!(judge(&conn, &meta(A, 1, None, 1)).unwrap(), Verdict::Apply);
        head(&conn, A, 1, 1);
        assert_eq!(
            judge(&conn, &meta(A, 2, Some(1), 2)).unwrap(),
            Verdict::Apply
        );
        assert_eq!(
            judge(&conn, &meta(A, 1, None, 1)).unwrap(),
            Verdict::SkipReplay
        );
    }

    #[test]
    fn a_replay_is_skipped() {
        let conn = profile();
        head(&conn, A, 5, 5);
        assert_eq!(
            judge(&conn, &meta(A, 3, Some(2), 3)).unwrap(),
            Verdict::SkipReplay
        );
        assert_eq!(
            judge(&conn, &meta(A, 5, Some(4), 5)).unwrap(),
            Verdict::SkipReplay
        );
        assert_eq!(
            heads(&conn),
            vec![(A.0.to_vec(), 5, h(5).to_vec(), false)],
            "judging writes nothing"
        );
    }

    #[test]
    fn a_forked_author_is_quarantined_and_held_while_others_apply() {
        let conn = profile();
        head(&conn, A, 3, 3);
        head(&conn, B, 1, 11);
        // Same sequence, another hash; and the next sequence on another prev.
        assert_eq!(
            judge(&conn, &meta(A, 3, Some(2), 33)).unwrap(),
            Verdict::Quarantine
        );
        assert_eq!(
            judge(&conn, &meta(A, 4, Some(33), 4)).unwrap(),
            Verdict::Quarantine
        );

        quarantine(&conn, A).unwrap();

        assert_eq!(
            judge(&conn, &meta(A, 4, Some(3), 4)).unwrap(),
            Verdict::Held
        );
        assert_eq!(
            judge(&conn, &meta(A, 9, Some(8), 9)).unwrap(),
            Verdict::Held
        );
        assert_eq!(
            judge(&conn, &meta(B, 2, Some(11), 12)).unwrap(),
            Verdict::Apply
        );
        assert_eq!(
            heads(&conn),
            vec![
                (A.0.to_vec(), 3, h(3).to_vec(), true),
                (B.0.to_vec(), 1, h(11).to_vec(), false),
            ]
        );
        assert!(quarantine(&conn, NEW).is_err(), "no head, nothing forked");
    }

    #[test]
    fn a_gap_stops_until_the_missing_envelope_arrives() {
        let conn = profile();
        assert_eq!(judge(&conn, &meta(A, 2, Some(1), 2)).unwrap(), Verdict::Gap);
        head(&conn, A, 2, 2);
        assert_eq!(judge(&conn, &meta(A, 4, Some(3), 4)).unwrap(), Verdict::Gap);
        assert_eq!(
            judge(&conn, &meta(A, 3, Some(2), 3)).unwrap(),
            Verdict::Apply
        );
        head(&conn, A, 3, 3);
        assert_eq!(
            judge(&conn, &meta(A, 4, Some(3), 4)).unwrap(),
            Verdict::Apply
        );
    }

    #[test]
    fn an_own_envelope_in_the_log_is_acknowledged() {
        let conn = profile();
        sealed(&conn, 1, 1, 90, 100, false);
        sealed(&conn, 2, 2, 150, 200, true);
        sealed(&conn, 3, 3, 250, 300, true);
        assert_eq!(
            judge(&conn, &meta(ME, 2, Some(1), 2)).unwrap(),
            Verdict::OwnAcknowledged
        );

        acknowledge_own(&conn, 2).unwrap();

        assert_eq!(unsent(&conn), vec![3]);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_own_log"), 3);
        assert!(heads(&conn).is_empty(), "own envelopes never make a head");
    }

    #[test]
    fn an_own_id_envelope_never_sealed_here_means_a_clone() {
        let conn = profile();
        sealed(&conn, 1, 1, 90, 100, false);
        // The original sealed seq 2 after the copy was taken; or a different seq 1.
        assert_eq!(
            judge(&conn, &meta(ME, 2, Some(1), 2)).unwrap(),
            Verdict::Clone
        );
        assert_eq!(
            judge(&conn, &meta(ME, 1, None, 21)).unwrap(),
            Verdict::Clone
        );
        // Even from a newer schema: the own-ID check comes first.
        let mut newer = meta(ME, 2, Some(1), 2);
        newer.schema = schema_number() + 1;
        assert_eq!(judge(&conn, &newer).unwrap(), Verdict::Clone);
    }

    #[test]
    fn an_envelope_from_a_newer_schema_is_not_applied() {
        let conn = profile();
        let mut next = meta(A, 1, None, 1);
        next.schema = schema_number() + 1;
        assert_eq!(judge(&conn, &next).unwrap(), Verdict::NewerSchema);
        next.schema = schema_number();
        assert_eq!(judge(&conn, &next).unwrap(), Verdict::Apply);
    }

    #[test]
    fn an_envelope_below_the_barrier_is_skipped_and_the_head_moves() {
        let conn = profile();
        conn.execute_batch("UPDATE sync_state SET barrier = 25")
            .unwrap();
        let mut old = meta(A, 1, None, 1);
        old.barrier = 19;
        assert_eq!(judge(&conn, &old).unwrap(), Verdict::BelowBarrier);

        skip_below_barrier(&conn, &old).unwrap();

        assert_eq!(heads(&conn), vec![(A.0.to_vec(), 1, h(1).to_vec(), false)]);
        let mut next = meta(A, 2, Some(1), 2);
        next.barrier = 25;
        assert_eq!(judge(&conn, &next).unwrap(), Verdict::Apply);
        next.barrier = 24;
        assert_eq!(judge(&conn, &next).unwrap(), Verdict::BelowBarrier);
    }

    #[test]
    fn a_malformed_chain_link_is_invalid() {
        let conn = profile();
        head(&conn, A, 1, 1);
        // A first envelope with a prev, a later one without, and sequence 0.
        assert_eq!(
            judge(&conn, &meta(B, 1, Some(0), 1)).unwrap(),
            Verdict::Invalid
        );
        assert_eq!(
            judge(&conn, &meta(A, 2, None, 2)).unwrap(),
            Verdict::Invalid
        );
        assert_eq!(
            judge(&conn, &meta(A, 0, None, 2)).unwrap(),
            Verdict::Invalid
        );
        assert_eq!(
            judge(&conn, &meta(ME, 1, Some(0), 1)).unwrap(),
            Verdict::Invalid
        );
        assert_eq!(
            judge(&conn, &meta(ME, 3, None, 1)).unwrap(),
            Verdict::Invalid
        );
    }

    #[test]
    fn crossing_a_barrier_requeues_own_rows_newer_than_the_snapshot() {
        let conn = migrated();
        state::enable(&conn, ME).unwrap();
        conn.execute_batch("DELETE FROM sync_outbox; DELETE FROM sync_clock;")
            .unwrap();
        own_clocks(&conn);
        conn.execute_batch(
            "UPDATE sync_state SET snapshot_hlc = 260, schema_seen = 24, barrier = 19",
        )
        .unwrap();

        crate::services::sync::on_open(&conn);

        assert_eq!(
            outbox(&conn),
            vec![
                queued("dead", 360),
                queued("i250", 270),
                queued("i350", 350)
            ]
        );
        let sync = state::load(&conn).unwrap().expect("row");
        assert_eq!(sync.barrier, 25);
        assert_eq!(sync.schema_seen, 30);
        assert_eq!(sync.barrier, barrier_number());
        assert_eq!(sync.schema_seen, schema_number());
        assert!(capture::is_installed(&conn).unwrap());
    }

    #[test]
    fn an_open_that_crosses_no_barrier_queues_nothing() {
        let conn = migrated();
        state::enable(&conn, ME).unwrap();
        conn.execute_batch("DELETE FROM sync_outbox; DELETE FROM sync_clock;")
            .unwrap();
        own_clocks(&conn);
        conn.execute_batch(
            "UPDATE sync_state SET snapshot_hlc = 0, schema_seen = 25, barrier = 25",
        )
        .unwrap();

        crate::services::sync::on_open(&conn);

        assert!(outbox(&conn).is_empty());
        assert_eq!(state::load(&conn).unwrap().expect("row").schema_seen, 30);
    }
}
