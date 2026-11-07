//! Chain tests: verdicts, the barrier marker and the barrier re-queue, and the helpers the
//! re-join tests share.

use super::*;
use crate::services::sync::capture;
use crate::services::sync::changeset::RowChange;
use crate::services::sync::hlc::Clock;
use crate::services::sync::journal::{build_next, record_sealed};
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
        position: seq,
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

/// `meta` written at `barrier` and placed at relay sequence `position`.
pub(super) fn placed(mut meta: EnvelopeMeta, barrier: u32, position: u64) -> EnvelopeMeta {
    meta.barrier = barrier;
    meta.position = position;
    meta
}

pub(super) fn barrier_seq(conn: &Connection) -> Option<u64> {
    state::load(conn).unwrap().expect("row").barrier_seq
}

/// Through capture: bank account `acc`, then its transaction `t1`, then `acc` renamed, so
/// the account's own clocks after its creation are newer than the transaction's. The outbox
/// is emptied (as if everything had gone out). Returns the account's creation clock.
pub(super) fn account_then_transaction(conn: &Connection) -> u64 {
    conn.execute_batch(
        "DELETE FROM sync_outbox; DELETE FROM sync_clock; \
         INSERT INTO bank_accounts (id, name, currency, balance) \
           VALUES ('acc', 'Main', 'CZK', '125.00');",
    )
    .unwrap();
    let created: i64 = conn
        .query_row(
            "SELECT hlc FROM sync_clock WHERE tbl = 'bank_accounts' AND pk = 'acc' AND col = '*'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    conn.execute_batch(
        "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, \
           booking_date) VALUES ('t1', 'acc', 'debit', '-5.00', 'CZK', 1767225600); \
         UPDATE bank_accounts SET name = 'Renamed' WHERE id = 'acc'; \
         DELETE FROM sync_outbox;",
    )
    .unwrap();
    created as u64
}

/// The re-queue put the transaction ahead of its account by `queued_at`: only the journal's
/// parent-first order keeps the account first.
pub(super) fn assert_child_queued_first(conn: &Connection) {
    let queued_at = |pk: &str| -> i64 {
        conn.query_row(
            "SELECT queued_at FROM sync_outbox WHERE pk = ?1",
            [pk],
            |r| r.get(0),
        )
        .expect(pk)
    };
    assert!(queued_at("t1") < queued_at("acc"));
}

/// Everything queued, sealed as the journal seals it (the hash is `h(seq)`), in order.
pub(super) fn sealed_changesets(conn: &Connection) -> Vec<(Changeset, [u8; 32])> {
    let mut out = Vec::new();
    loop {
        let tx = conn.unchecked_transaction().unwrap();
        let (built, refused) = build_next(&tx).unwrap();
        assert!(refused.is_empty(), "{refused:?}");
        let Some(built) = built else {
            return out;
        };
        let hash = h(built.changeset.seq as u8);
        record_sealed(&tx, &built, hash).unwrap();
        tx.commit().unwrap();
        out.push((built.changeset, hash));
    }
}

/// `sealed_changesets`, each envelope's rows only.
pub(super) fn sealed_rows(conn: &Connection) -> Vec<Vec<RowChange>> {
    sealed_changesets(conn)
        .into_iter()
        .map(|(cs, _)| cs.rows)
        .collect()
}

/// `acc` goes out before `t1` (by envelope, then by place in it), and `t1`'s `"p"` carries
/// the account's current generation.
pub(super) fn assert_parent_first(conn: &Connection, sent: &[Vec<RowChange>]) {
    let place = |pk: &str| {
        sent.iter()
            .enumerate()
            .find_map(|(e, rows)| {
                rows.iter()
                    .position(|r| r.key.to_pk_text().unwrap() == pk)
                    .map(|i| (e, i))
            })
            .expect(pk)
    };
    assert!(
        place("acc") < place("t1"),
        "{:?} {:?}",
        place("acc"),
        place("t1")
    );
    let (e, i) = place("t1");
    let generation: (i64, Vec<u8>) = conn
        .query_row(
            "SELECT hlc, device FROM sync_clock \
             WHERE tbl = 'bank_accounts' AND pk = 'acc' AND col = '*'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert_eq!(
        sent[e][i].parents["bank_account_id"],
        Clock {
            hlc: generation.0 as u64,
            device: DeviceId(generation.1.try_into().unwrap()),
        }
    );
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
fn below_barrier_envelope_before_the_marker_is_skipped_and_missed() {
    // A device that crossed at unlock with a stale cursor has no marker yet (ruling T8-2).
    let conn = profile();
    conn.execute_batch("UPDATE sync_state SET barrier = 25")
        .unwrap();
    let lagging = placed(meta(A, 1, None, 1), 19, 5);
    assert_eq!(judge(&conn, &lagging).unwrap(), Verdict::BelowBarrier);

    skip_below_barrier(&conn, &lagging).unwrap();

    let sync = state::load(&conn).unwrap().expect("row");
    assert_eq!(sync.missed_below_barrier, Some(5));
    assert_eq!(sync.barrier_seq, None);
    assert_eq!(heads(&conn), vec![(A.0.to_vec(), 1, h(1).to_vec(), false)]);
    // Placed before a marker noted later, it is still not applied.
    assert!(note_barrier_marker(&conn, &placed(meta(B, 1, None, 11), 25, 6)).unwrap());
    let next = placed(meta(A, 2, Some(1), 2), 19, 4);
    assert_eq!(judge(&conn, &next).unwrap(), Verdict::BelowBarrier);
}

#[test]
fn below_barrier_envelope_after_the_marker_is_skipped() {
    let conn = profile();
    conn.execute_batch("UPDATE sync_state SET barrier = 25")
        .unwrap();
    assert!(note_barrier_marker(&conn, &placed(meta(B, 1, None, 11), 25, 6)).unwrap());
    let old = placed(meta(A, 1, None, 1), 19, 7);
    assert_eq!(judge(&conn, &old).unwrap(), Verdict::BelowBarrier);

    skip_below_barrier(&conn, &old).unwrap();

    assert_eq!(heads(&conn), vec![(A.0.to_vec(), 1, h(1).to_vec(), false)]);
    let next = placed(meta(A, 2, Some(1), 2), 25, 8);
    assert_eq!(judge(&conn, &next).unwrap(), Verdict::Apply);
    let below = placed(next.clone(), 24, 9);
    assert_eq!(judge(&conn, &below).unwrap(), Verdict::BelowBarrier);
    skip_below_barrier(&conn, &below).unwrap();
    // The newest skipped position is kept.
    let sync = state::load(&conn).unwrap().expect("row");
    assert_eq!(sync.missed_below_barrier, Some(9));
}

#[test]
fn the_marker_is_the_smallest_position_noted_in_any_order() {
    let conn = profile();
    conn.execute_batch("UPDATE sync_state SET barrier = 25")
        .unwrap();
    assert!(note_barrier_marker(&conn, &placed(meta(B, 2, Some(11), 12), 25, 9)).unwrap());
    assert!(note_barrier_marker(&conn, &placed(meta(B, 1, None, 11), 25, 4)).unwrap());
    assert!(!note_barrier_marker(&conn, &placed(meta(A, 1, None, 1), 25, 6)).unwrap());
    assert!(!note_barrier_marker(&conn, &placed(meta(A, 2, Some(1), 2), 19, 2)).unwrap());
    assert_eq!(barrier_seq(&conn), Some(4));
}

#[test]
fn a_snapshot_heals_a_miss_only_from_a_clean_dumper_at_the_barrier_and_past_the_miss() {
    let conn = profile();
    conn.execute_batch("UPDATE sync_state SET barrier = 25")
        .unwrap();
    let sync = state::load(&conn).unwrap().expect("row");
    assert!(
        !snapshot_heals_missed(&sync, 25, 100, None),
        "nothing missed"
    );
    assert!(may_upload_snapshot(&sync));

    state::note_missed_below_barrier(&conn, 7).unwrap();
    let sync = state::load(&conn).unwrap().expect("row");
    assert!(
        !may_upload_snapshot(&sync),
        "a device with a miss never uploads"
    );
    assert!(snapshot_heals_missed(&sync, 25, 7, None));
    assert!(snapshot_heals_missed(&sync, 26, 30, None));
    assert!(
        !snapshot_heals_missed(&sync, 25, 30, Some(7)),
        "the dumper skipped it too"
    );
    assert!(
        !snapshot_heals_missed(&sync, 25, 30, Some(12)),
        "the dumper has a miss of its own"
    );
    assert!(
        !snapshot_heals_missed(&sync, 25, 6, None),
        "dumped before the miss"
    );
    assert!(
        !snapshot_heals_missed(&sync, 19, 30, None),
        "dumped below the barrier"
    );
}

#[test]
fn the_first_envelope_at_or_above_the_barrier_is_the_marker() {
    let conn = profile();
    conn.execute_batch("UPDATE sync_state SET barrier = 25")
        .unwrap();
    // Below the barrier: not the marker.
    assert!(!note_barrier_marker(&conn, &placed(meta(A, 1, None, 1), 19, 3)).unwrap());
    assert_eq!(barrier_seq(&conn), None);
    // The first at the barrier is; later ones (even above it) never move it.
    assert!(note_barrier_marker(&conn, &placed(meta(B, 1, None, 11), 25, 4)).unwrap());
    assert!(!note_barrier_marker(&conn, &placed(meta(B, 2, Some(11), 12), 25, 5)).unwrap());
    assert!(!note_barrier_marker(&conn, &placed(meta(A, 2, Some(1), 2), 30, 9)).unwrap());
    assert_eq!(barrier_seq(&conn), Some(4));
    assert!(heads(&conn).is_empty(), "noting a marker moves no head");

    // A profile that never crossed a barrier has no marker to find.
    let none = profile();
    none.execute_batch("UPDATE sync_state SET barrier = 0")
        .unwrap();
    assert!(!note_barrier_marker(&none, &placed(meta(A, 1, None, 1), 25, 1)).unwrap());
    assert_eq!(barrier_seq(&none), None);
}

#[test]
fn crossing_a_barrier_forgets_the_old_marker() {
    let conn = migrated();
    state::enable(&conn, ME).unwrap();
    conn.execute_batch("UPDATE sync_state SET schema_seen = 24, barrier = 19, barrier_seq = 12")
        .unwrap();

    crate::services::sync::on_open(&conn);

    let sync = state::load(&conn).unwrap().expect("row");
    assert_eq!(sync.barrier, 25);
    assert_eq!(sync.barrier_seq, None);
    // The new barrier's marker is the next envelope at or above 25.
    assert!(note_barrier_marker(&conn, &placed(meta(A, 1, None, 1), 25, 40)).unwrap());
    assert_eq!(barrier_seq(&conn), Some(40));
}

#[test]
fn barrier_requeue_ships_parents_before_children() {
    let conn = migrated();
    state::enable(&conn, ME).unwrap();
    let created = account_then_transaction(&conn);
    conn.execute(
        "UPDATE sync_state SET snapshot_hlc = ?1, schema_seen = 24, barrier = 19",
        [created as i64],
    )
    .unwrap();

    crate::services::sync::on_open(&conn);

    assert_child_queued_first(&conn);
    assert_parent_first(&conn, &sealed_rows(&conn));
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
    conn.execute_batch("UPDATE sync_state SET snapshot_hlc = 260, schema_seen = 24, barrier = 19")
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
    assert_eq!(sync.schema_seen, 31);
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
    conn.execute_batch("UPDATE sync_state SET snapshot_hlc = 0, schema_seen = 25, barrier = 25")
        .unwrap();

    crate::services::sync::on_open(&conn);

    assert!(outbox(&conn).is_empty());
    assert_eq!(state::load(&conn).unwrap().expect("row").schema_seen, 31);
}
