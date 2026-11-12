//! Audit M3's way out (Task 17): a held gap is noted with the time this device first held it, is
//! forgotten once its author's head passes it, and after `GAP_SKIP_AFTER_SECS` the person may skip
//! it. The skip moves the author's head so the held envelopes apply at the next pull, records the
//! miss (a clean snapshot heals it), and so releases the cursor that held the refetch tail. A change
//! whose parent never arrives is held and skipped the same way, the envelope itself skipped.

use super::gaps::withhold_one_of_a;
use super::runtime::write_and_seal;
use super::*;
use crate::error::AppError;
use crate::models::sync::{SyncPhase, SyncProblem};
use crate::services::sync::chain::gaps::{
    forget_healed_gaps, held_gaps, note_held_gap, skip_missing_change, GapKind, HeldGap,
    GAP_SKIP_AFTER_SECS,
};
use crate::services::sync::chain::EnvelopeMeta;
use crate::services::sync::changeset::SqlValue;
use crate::services::sync::engine::pull::{pull_all, pull_pages, PULL_PAGE};
use crate::services::sync::engine::snapshots::{dump_sealed, SnapshotPolicy};
use crate::services::sync::engine::status::{problem_of_pull, status, Runtime};
use crate::services::sync::engine::{run_round, PullEnd, RoundMode};
use crate::services::sync::hlc::Clock;

pub(super) const DAY: i64 = 86_400;

/// The skipped change's miss (`state::skipped_miss`); a skip never sets the below-barrier one.
pub(super) fn missed(conn: &Connection) -> Option<u64> {
    let sync = state::load(conn).expect("state").expect("row");
    assert_eq!(
        sync.missed_below_barrier, None,
        "a skip is no below-barrier miss"
    );
    state::skipped_miss(conn).expect("skipped miss")
}

/// `B` writes `b1` and `b2`; the relay withholds `b1` (op 1), so `A` holds `b2` (op 2) as a gap.
pub(super) async fn b1_withheld(vault: &Vault) {
    let b1 = write_and_seal(vault, &vault.b, "b1");
    let b2 = write_and_seal(vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[b1, b2]).await;
    vault.relay.drop_op(1);
}

fn not_skippable_yet(error: &AppError) -> bool {
    matches!(error, AppError::Validation(key) if key == "validation.syncGapNotSkippableYet")
}

#[tokio::test]
async fn a_held_gap_is_noted_once_with_its_first_time() {
    let vault = vault().await;
    b1_withheld(&vault).await;

    let first = pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("first pull");
    let later = NOW + 3 * DAY;
    let second = pull_all(&vault.relay, &vault.a, &vault.creds, later)
        .await
        .expect("second pull");

    assert_eq!(
        (first.end(), second.end()),
        (PullEnd::Waiting, PullEnd::Waiting)
    );
    let gaps = held_gaps(&vault.a.lock()).expect("held gaps");
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    let gap = &gaps[0];
    assert_eq!(gap.device, B);
    assert_eq!(gap.kind, GapKind::Missing);
    assert_eq!((gap.held_seq, gap.position), (2, 2));
    assert!(gap.held_prev.is_some());
    assert_eq!(gap.since, NOW, "the first time is kept");
}

#[tokio::test]
async fn a_lower_held_envelope_of_the_same_author_takes_its_place_and_keeps_the_time() {
    let vault = vault().await;
    let a = vault.a.lock();
    let meta = |seq: u64, position: u64| EnvelopeMeta {
        device: B,
        seq,
        prev: Some([seq as u8; 32]),
        hash: [9; 32],
        schema: 0,
        barrier: 0,
        position,
    };
    note_held_gap(&a, &meta(4, 3), GapKind::Missing, NOW).expect("note 4");
    note_held_gap(&a, &meta(3, 7), GapKind::Missing, NOW + DAY).expect("note 3");
    note_held_gap(&a, &meta(5, 9), GapKind::Missing, NOW + 2 * DAY).expect("note 5");

    let gaps = held_gaps(&a).expect("held gaps");
    assert_eq!(
        gaps,
        vec![HeldGap {
            device: B,
            kind: GapKind::Missing,
            held_seq: 3,
            held_prev: Some([3; 32]),
            held_hash: [9; 32],
            position: 7,
            since: NOW,
        }]
    );
}

#[tokio::test]
async fn a_gap_cannot_be_skipped_before_fourteen_days() {
    let vault = vault().await;
    b1_withheld(&vault).await;
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    let before = sync_rows(&vault.a.lock());
    let gaps_before = held_gaps(&vault.a.lock()).expect("held gaps");

    let error =
        skip_missing_change(&vault.a.lock(), B, NOW + 13 * DAY).expect_err("13 days is too early");
    let just_before = skip_missing_change(&vault.a.lock(), B, NOW + GAP_SKIP_AFTER_SECS - 1)
        .expect_err("one second before");

    assert!(not_skippable_yet(&error), "{error:?}");
    assert!(not_skippable_yet(&just_before), "{just_before:?}");
    let a = vault.a.lock();
    assert_eq!(sync_rows(&a), before, "nothing changed");
    assert_eq!(held_gaps(&a).expect("held gaps"), gaps_before);
}

#[tokio::test]
async fn skipping_applies_the_held_envelopes_and_records_the_miss() {
    let vault = vault().await;
    b1_withheld(&vault).await;
    let first = pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    assert!(!first.may_snapshot());
    let at = NOW + GAP_SKIP_AFTER_SECS;

    assert!(skip_missing_change(&vault.a.lock(), B, at).expect("skip"));
    {
        let a = vault.a.lock();
        assert!(held_gaps(&a).expect("held gaps").is_empty());
        assert_eq!(missed(&a), Some(2), "the miss is the held position");
        assert_eq!(head(&a, B).map(|h| h.0), Some(1));
    }
    let next = pull_all(&vault.relay, &vault.a, &vault.creds, at)
        .await
        .expect("next pull");

    assert!(next.done(), "{next:?}");
    assert_eq!(next.applied, 1);
    assert!(next.may_snapshot(), "the gap no longer holds the pull");
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 2, "the cursor moved past the held envelope");
    assert!(has_institution(&a, "b2"));
    assert!(
        !has_institution(&a, "b1"),
        "the skipped change stays missing"
    );
    assert_eq!(head(&a, B).map(|h| h.0), Some(2));
    assert_eq!(missed(&a), Some(2), "until a clean snapshot heals it");
    assert!(held_gaps(&a).expect("held gaps").is_empty());
}

#[tokio::test]
async fn skipping_a_gap_that_healed_meanwhile_changes_nothing() {
    let vault = vault().await;
    b1_withheld(&vault).await;
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    vault.relay.replay_op(1);
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW + DAY)
        .await
        .expect("pull");
    let before = sync_rows(&vault.a.lock());

    let skipped = skip_missing_change(&vault.a.lock(), B, NOW + GAP_SKIP_AFTER_SECS)
        .expect("nothing to skip");
    assert!(!skipped, "it arrived meanwhile: nothing was skipped");
    assert!(!skip_missing_change(&vault.a.lock(), C, NOW + GAP_SKIP_AFTER_SECS).expect("unknown"));

    let a = vault.a.lock();
    assert_eq!(sync_rows(&a), before);
    assert_eq!(missed(&a), None);
}

#[tokio::test]
async fn a_gap_that_heals_forgets_its_record() {
    let vault = vault().await;
    b1_withheld(&vault).await;
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    assert_eq!(held_gaps(&vault.a.lock()).expect("held gaps").len(), 1);

    // The relay serves the missing envelope after all (at a later position).
    vault.relay.replay_op(1);
    let first = pull_all(&vault.relay, &vault.a, &vault.creds, NOW + DAY)
        .await
        .expect("pull");
    assert_eq!(
        first.end(),
        PullEnd::Waiting,
        "b2 waited before b1's replay"
    );
    assert!(
        held_gaps(&vault.a.lock()).expect("held gaps").is_empty(),
        "b1 applied: the head reached the gap"
    );
    let second = pull_all(&vault.relay, &vault.a, &vault.creds, NOW + DAY)
        .await
        .expect("pull");

    assert!(second.done(), "{second:?}");
    let a = vault.a.lock();
    assert!(has_institution(&a, "b1") && has_institution(&a, "b2"));
    assert!(held_gaps(&a).expect("held gaps").is_empty());
    assert_eq!(missed(&a), None);
}

#[test]
fn forget_healed_gaps_drops_only_authors_whose_head_reached_the_gap() {
    let conn = crate::services::sync::test_support::migrated();
    state::enable(&conn, A).expect("enable");
    let meta = |device, seq: u64| EnvelopeMeta {
        device,
        seq,
        prev: Some([1; 32]),
        hash: [2; 32],
        schema: 0,
        barrier: 0,
        position: 5,
    };
    note_held_gap(&conn, &meta(B, 3), GapKind::Missing, NOW).expect("B");
    note_held_gap(&conn, &meta(C, 3), GapKind::Missing, NOW).expect("C");
    conn.execute(
        "INSERT INTO sync_heads (device, seq, hash) VALUES (?1, 2, ?2), (?3, 1, ?2)",
        params![B.0.to_vec(), vec![1u8; 32], C.0.to_vec()],
    )
    .expect("heads");

    assert_eq!(forget_healed_gaps(&conn).expect("forget"), 1);

    let left: Vec<DeviceId> = held_gaps(&conn)
        .expect("held gaps")
        .into_iter()
        .map(|g| g.device)
        .collect();
    assert_eq!(left, vec![C]);
}

#[tokio::test]
async fn a_clean_snapshot_heals_a_skipped_change() {
    let vault = vault().await;
    b1_withheld(&vault).await;
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    let at = NOW + GAP_SKIP_AFTER_SECS;
    skip_missing_change(&vault.a.lock(), B, at).expect("skip");
    pull_all(&vault.relay, &vault.a, &vault.creds, at)
        .await
        .expect("pull");
    assert_eq!(missed(&vault.a.lock()), Some(2));

    // B wrote b1, so it has no miss: its snapshot dumped past position 2 heals A's.
    pull_all(&vault.relay, &vault.b, &vault.creds, at)
        .await
        .expect("B pulls its own echo");
    // b1 never comes back to B either (the relay withholds it), so it stays unsent there; the
    // dump waits for an empty `sync_unsent`, which this stand-in for a clean device skips.
    exec(&vault.b.lock(), "DELETE FROM sync_unsent");
    let sealed = dump_sealed(&vault.b.lock(), &vault.creds).expect("dump B");
    assert!(sealed.seq >= 2, "dumped past the position: {}", sealed.seq);
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), sealed.seq, sealed.bytes)
        .await
        .expect("put snapshot");

    let report = run_round(
        &vault.relay,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        at,
        0,
    )
    .await
    .expect("round");

    assert!(report.restored, "{report:?}");
    let a = vault.a.lock();
    assert_eq!(missed(&a), None, "healed");
    assert!(
        has_institution(&a, "b1"),
        "the skipped change came with the snapshot"
    );
}

/// A change whose parent never arrives waits like a gap; after 14 days the change itself is
/// skipped (its author's later envelopes then apply) and recorded as missed.
#[tokio::test]
async fn a_change_whose_parent_never_arrives_can_be_skipped_itself() {
    let vault = vault().await;
    let g = Clock {
        hlc: 1_000 << 16,
        device: C,
    };
    let payment = row(
        "bank_transactions",
        "t1",
        g,
        &[
            ("bank_account_id", text("acc-gone")),
            ("tx_type", text("debit")),
            ("amount", text("-10.00")),
            ("currency", text("CZK")),
            ("booking_date", SqlValue::Integer(1_767_225_600)),
        ],
    );
    let c1_cs = changeset(C, 1, None, vec![payment]);
    let c1 = forge(&vault.creds, &c1_cs);
    let c1_hash = crate::services::sync::envelope::envelope_hash(&c1);
    let c2 = forge(&vault.creds, &changeset(C, 2, Some(c1_hash), vec![]));
    push(&vault.relay, &vault.creds, &[c1, c2]).await;

    let first = pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    assert_eq!(first.end(), PullEnd::Waiting);
    assert_eq!(first.gaps, 2, "C1 waits for its parent, C2 for C1");
    let gaps = held_gaps(&vault.a.lock()).expect("held gaps");
    assert_eq!(gaps.len(), 1, "{gaps:?}");
    assert_eq!(
        (
            gaps[0].device,
            gaps[0].kind,
            gaps[0].held_seq,
            gaps[0].position
        ),
        (C, GapKind::Parent, 1, 1)
    );

    let early = skip_missing_change(&vault.a.lock(), C, NOW + 13 * DAY).expect_err("early");
    assert!(not_skippable_yet(&early));
    let at = NOW + GAP_SKIP_AFTER_SECS;
    skip_missing_change(&vault.a.lock(), C, at).expect("skip");
    let next = pull_all(&vault.relay, &vault.a, &vault.creds, at)
        .await
        .expect("pull");

    assert!(next.done(), "{next:?}");
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 2);
    assert_eq!(
        head(&a, C).map(|h| h.0),
        Some(2),
        "C2 applied after the skip"
    );
    assert_eq!(
        count(&a, "SELECT COUNT(*) FROM bank_transactions WHERE id = 't1'"),
        0
    );
    assert_eq!(missed(&a), Some(1), "the skipped change is the miss");
    assert!(held_gaps(&a).expect("held gaps").is_empty());
}

/// Task 15 review (minor 8): while a gap is held, every pull fetches again from the held envelope;
/// a tail past the page limit ends the pull unfinished, which must not read as "up to date".
#[tokio::test]
async fn an_unfinished_pull_behind_a_gap_shows_the_gap() {
    let vault = vault().await;
    b1_withheld(&vault).await;
    // More than one page after the held envelope, of C's.
    let mut prev = None;
    let mut cts = Vec::new();
    for seq in 1..=(PULL_PAGE as u64 + 5) {
        let ct = forge(&vault.creds, &changeset(C, seq, prev, vec![]));
        prev = Some(crate::services::sync::envelope::envelope_hash(&ct));
        cts.push(ct);
    }
    push(&vault.relay, &vault.creds, &cts).await;

    let pull = pull_pages(&vault.relay, &vault.a, &vault.creds, NOW, 1)
        .await
        .expect("pull");

    assert_eq!(pull.end(), PullEnd::Unfinished);
    assert!(pull.gaps > 0);
    assert_eq!(problem_of_pull(&pull), Some(SyncProblem::Gap));
    let runtime = Runtime {
        problem: problem_of_pull(&pull),
        ..Runtime::default()
    };
    let shown = status(&vault.a.lock(), &runtime).expect("status");
    assert_eq!(shown.phase, SyncPhase::Waiting);
    assert_eq!(
        cursor(&vault.a.lock()),
        0,
        "the held envelope keeps the cursor"
    );

    // Skipping releases it: the next full pull reaches the head.
    let at = NOW + GAP_SKIP_AFTER_SECS;
    skip_missing_change(&vault.a.lock(), B, at).expect("skip");
    let next = pull_all(&vault.relay, &vault.a, &vault.creds, at)
        .await
        .expect("pull");
    assert!(next.done(), "{next:?}");
    assert_eq!(cursor(&vault.a.lock()), PULL_PAGE as u64 + 7);
}

/// A pull that reaches the relay's end with nothing waiting leaves no gap behind, even one whose
/// author's head never moved (the relay no longer serves anything held).
#[tokio::test]
async fn a_pull_with_nothing_waiting_forgets_every_gap() {
    let vault = vault().await;
    withhold_one_of_a(&vault).await;
    pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect("pull");
    assert_eq!(held_gaps(&vault.b.lock()).expect("held gaps").len(), 1);
    let last = vault.relay.ops().last().expect("ops").seq;
    vault.relay.forget_op(last);

    let pull = pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect("pull");

    assert!(pull.done(), "{pull:?}");
    assert!(held_gaps(&vault.b.lock()).expect("held gaps").is_empty());
}

/// Turning sync off forgets the held gaps with the rest of the sync rows.
#[tokio::test]
async fn turning_sync_off_forgets_held_gaps() {
    let vault = vault().await;
    b1_withheld(&vault).await;
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    state::disable(&vault.a.lock()).expect("disable");

    assert!(held_gaps(&vault.a.lock()).expect("held gaps").is_empty());
}

/// A completed replace starts the chain over: the held gaps go with the other engine marks.
#[tokio::test]
async fn a_replace_forgets_held_gaps() {
    let vault = vault().await;
    b1_withheld(&vault).await;
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    state::reset_engine_marks(&vault.a.lock()).expect("reset");

    assert!(held_gaps(&vault.a.lock()).expect("held gaps").is_empty());
}

/// Settings sees the author's name and when the skip becomes possible.
#[tokio::test]
async fn the_view_names_the_device_and_the_day_it_may_be_skipped() {
    let vault = vault().await;
    b1_withheld(&vault).await;
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    exec(
        &vault.a.lock(),
        &format!(
            "INSERT INTO sync_devices (id, name, platform, app_version, joined_at) \
             VALUES ('{}', 'Work laptop', 'macos', '1.3.0', 0)",
            B.to_hex()
        ),
    );

    let views = crate::services::sync::chain::gaps::held_gap_views(&vault.a.lock()).expect("views");

    assert_eq!(views.len(), 1);
    let view = &views[0];
    assert_eq!(view.device_id, B.to_hex());
    assert_eq!(view.device_name.as_deref(), Some("Work laptop"));
    assert_eq!(view.kind, crate::models::sync::HeldGapKind::Missing);
    assert_eq!((view.since, view.skippable_at), (NOW, NOW + 14 * DAY));
}
