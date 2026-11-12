//! Task 17 fix round 1. I1: a skipped change's miss never holds a snapshot upload back; it travels
//! in the header, so no device heals from that snapshot and a restorer inherits it, and a clean
//! device's snapshot heals every device. I2: while a skip's miss stands, a change held for a
//! parent is skippable as early as the skip was. I3: the view says how many changes a skip gives
//! up.

use super::gap_skip::{b1_withheld, missed, DAY};
use super::runtime::write_and_seal;
use super::*;
use crate::error::AppError;
use crate::models::sync::HeldGapKind;
use crate::services::sync::chain::gaps::{
    self, held_gap_views, held_gaps, skip_missing_change, GapKind, GAP_SKIP_AFTER_SECS,
};
use crate::services::sync::chain::{may_upload_snapshot, EnvelopeMeta};
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::snapshots::{dump_sealed, heal_missed, SnapshotPolicy};
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::registry::keys::{barrier_number, schema_number};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::VaultInfo;
use crate::services::sync::snapshot::{read_record, Record};

/// A fourth device, joined later from a snapshot.
const D: DeviceId = DeviceId([4; 16]);

const AT: i64 = NOW + GAP_SKIP_AFTER_SECS;

/// `(below-barrier miss, skipped miss)` in the header of `plain`, and the raw text.
fn header_misses(plain: &[u8]) -> (Option<u64>, Option<u64>) {
    let mut input = plain;
    let Some(Record::Devices(devs)) = read_record(&mut input, &[]).expect("devs") else {
        panic!("no devs record");
    };
    match read_record(&mut input, &devs).expect("header") {
        Some(Record::Header {
            missed_below_barrier,
            missed_skipped,
            ..
        }) => (missed_below_barrier, missed_skipped),
        other => panic!("no header: {other:?}"),
    }
}

fn plain_dump(conn: &Connection) -> Vec<u8> {
    let seq = cursor(conn);
    let mut out = Vec::new();
    snapshot::dump(conn, seq, &mut out).expect("dump");
    out
}

async fn info(vault: &Vault) -> VaultInfo {
    vault
        .relay
        .vault_info(&vault.creds.auth())
        .await
        .expect("vault info")
}

/// `A` held B's `b2` (its `b1` is withheld) and skipped it at `AT`; its next pull applied `b2`.
async fn skipped_on_a(vault: &Vault) {
    b1_withheld(vault).await;
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    assert!(skip_missing_change(&vault.a.lock(), B, AT).expect("skip"));
    assert!(pull_all(&vault.relay, &vault.a, &vault.creds, AT)
        .await
        .expect("pull")
        .done());
}

fn not_skippable_yet(error: &AppError) -> bool {
    matches!(error, AppError::Validation(key) if key == "validation.syncGapNotSkippableYet")
}

#[tokio::test]
async fn a_skipper_uploads_with_the_miss_and_only_a_clean_snapshot_heals() {
    let vault = vault().await;
    skipped_on_a(&vault).await;
    assert_eq!(missed(&vault.a.lock()), Some(2));
    assert!(may_upload_snapshot(
        &state::load(&vault.a.lock()).expect("load").expect("row")
    ));

    // No device is clean yet: the skipper uploads anyway, the miss in the header.
    let report = run_round(
        &vault.relay,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
        AT,
        0,
    )
    .await
    .expect("round");
    assert!(report.snapshot_uploaded, "{report:?}");
    let plain = plain_dump(&vault.a.lock());
    assert_eq!(header_misses(&plain), (None, Some(2)));
    let text = String::from_utf8_lossy(&plain);
    assert!(
        text.contains(r#""missedBelowBarrier":2"#),
        "older readers see a miss"
    );
    assert!(text.contains(r#""missedSkipped":2"#));

    // A device joining from it inherits the miss, as a skipped one: it may upload too.
    let j = {
        let conn = crate::services::sync::test_support::migrated();
        keys::save_credentials(&conn, &vault.creds).expect("credentials");
        snapshot::join(&conn, D, &mut plain.as_slice(), AT).expect("join");
        TestConn::new(conn)
    };
    {
        let j = j.lock();
        assert_eq!(missed(&j), Some(2), "inherited");
        assert!(!has_institution(&j, "b1"));
        assert!(may_upload_snapshot(
            &state::load(&j).expect("load").expect("row")
        ));
    }

    // That device is not healed by the skipper's snapshot: it lacks the change too.
    let healed = heal_missed(
        &vault.relay,
        &j,
        &vault.creds,
        &mut SnapshotPolicy::default(),
        &info(&vault).await,
        AT,
    )
    .await
    .expect("heal");
    assert!(healed.is_none());
    assert_eq!(missed(&j.lock()), Some(2));

    // B wrote b1 and has no miss: its later snapshot heals both.
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    push(&vault.relay, &vault.creds, &[c1]).await;
    pull_all(&vault.relay, &vault.b, &vault.creds, AT)
        .await
        .expect("B pulls");
    // b1 never comes back to B either; the dump waits for an empty `sync_unsent`.
    exec(&vault.b.lock(), "DELETE FROM sync_unsent");
    let sealed = dump_sealed(&vault.b.lock(), &vault.creds).expect("dump B");
    assert_eq!(sealed.seq, 3);
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), sealed.seq, sealed.bytes)
        .await
        .expect("put snapshot");
    for device in [&vault.a, &j] {
        let healed = heal_missed(
            &vault.relay,
            device,
            &vault.creds,
            &mut SnapshotPolicy::default(),
            &info(&vault).await,
            AT,
        )
        .await
        .expect("heal");
        assert!(healed.is_some());
        let conn = device.lock();
        assert_eq!(missed(&conn), None, "healed");
        assert!(has_institution(&conn, "b1"));
    }
}

/// Final review A, minor 3: a device never downloads its own newest snapshot to heal a miss: the
/// snapshot carries that very miss and cannot heal it.
#[tokio::test]
async fn a_skipper_does_not_download_its_own_snapshot_to_heal() {
    let vault = vault().await;
    skipped_on_a(&vault).await;
    let report = run_round(
        &vault.relay,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
        AT,
        0,
    )
    .await
    .expect("round");
    assert!(report.snapshot_uploaded, "{report:?}");
    let info = info(&vault).await;
    let from = vault.relay.calls().len();

    let healed = heal_missed(
        &vault.relay,
        &vault.a,
        &vault.creds,
        &mut SnapshotPolicy::default(),
        &info,
        AT,
    )
    .await
    .expect("heal");

    assert!(healed.is_none());
    assert!(
        !vault.relay.calls()[from..].contains(&Route::GetSnapshot),
        "its own snapshot is not fetched"
    );
    assert_eq!(missed(&vault.a.lock()), Some(2));
}

#[tokio::test]
async fn a_merge_inherits_a_skipped_miss_above_its_cursor() {
    let vault = vault().await;
    skipped_on_a(&vault).await;
    let plain = plain_dump(&vault.a.lock());

    // B's cursor is still 0: the merge moves it past position 2, so B now lacks what A lacks.
    snapshot::restore_merge(&vault.b.lock(), &mut plain.as_slice(), AT).expect("merge");

    let b = vault.b.lock();
    assert_eq!(missed(&b), Some(2));
    assert_eq!(
        state::local_skip(&b).expect("local skip"),
        None,
        "B skipped nothing itself"
    );
}

/// A Parent hold of `device`'s first envelope at relay position 9.
fn parent_meta(device: DeviceId) -> EnvelopeMeta {
    EnvelopeMeta {
        device,
        seq: 1,
        prev: None,
        hash: [7; 32],
        schema: schema_number(),
        barrier: barrier_number(),
        position: 9,
    }
}

fn since_of(conn: &Connection, device: DeviceId) -> i64 {
    held_gaps(conn)
        .expect("held gaps")
        .into_iter()
        .find(|gap| gap.device == device)
        .expect("held")
        .since
}

/// Final review A, minor 2 (ruling): a miss inherited from a snapshot never makes a Parent hold
/// skippable at once. Two weeks after a join, an unrelated author whose parent is merely late
/// still waits its own 14 days.
#[tokio::test]
async fn an_inherited_miss_leaves_a_parent_hold_its_own_wait() {
    let vault = vault().await;
    skipped_on_a(&vault).await;
    let plain = plain_dump(&vault.a.lock());
    snapshot::restore_merge(&vault.b.lock(), &mut plain.as_slice(), AT).expect("merge");
    let later = AT + GAP_SKIP_AFTER_SECS + DAY;
    let b = vault.b.lock();

    gaps::note_held_gap(&b, &parent_meta(C), GapKind::Parent, later).expect("hold");

    assert_eq!(since_of(&b, C), later);
    let early = skip_missing_change(&b, C, later).expect_err("waits");
    assert!(not_skippable_yet(&early));
}

/// Final review A, minor 2 (ruling): a Parent hold takes a local skip's first time only when it
/// is noted within 14 days of that skip; a later one waits its own 14 days.
#[tokio::test]
async fn only_a_parent_hold_soon_after_a_local_skip_inherits_its_time() {
    let vault = vault().await;
    skipped_on_a(&vault).await;
    let a = vault.a.lock();
    let within = AT + GAP_SKIP_AFTER_SECS;
    let after = within + 1;

    gaps::note_held_gap(&a, &parent_meta(C), GapKind::Parent, within).expect("hold C");
    gaps::note_held_gap(&a, &parent_meta(D), GapKind::Parent, after).expect("hold D");

    assert_eq!(since_of(&a, C), NOW, "the skipped gap's first time");
    assert_eq!(since_of(&a, D), after);
    assert!(skip_missing_change(&a, C, within).expect("skippable at once"));
    let early = skip_missing_change(&a, D, after).expect_err("D waits");
    assert!(not_skippable_yet(&early));
}

#[tokio::test]
async fn turning_sync_off_or_a_clean_replace_forgets_a_skipped_miss() {
    let vault = vault().await;
    let clean = plain_dump(&vault.a.lock());
    skipped_on_a(&vault).await;

    snapshot::restore_replace(&vault.a.lock(), &mut clean.as_slice(), AT).expect("replace");
    assert_eq!(
        missed(&vault.a.lock()),
        None,
        "a replace from a clean snapshot"
    );

    state::note_skipped_miss(&vault.a.lock(), 2).expect("miss");
    state::note_local_skip(&vault.a.lock(), NOW, AT).expect("skip");
    state::disable(&vault.a.lock()).expect("disable");
    assert_eq!(state::skipped_miss(&vault.a.lock()).expect("miss"), None);
    assert_eq!(state::local_skip(&vault.a.lock()).expect("skip"), None);
}

/// Ruling I2: after a skip, B's later change under the account the skipped one created waits for
/// a parent that is not coming; it may be skipped at once. C's unrelated gap waits its 14 days.
#[tokio::test]
async fn a_change_built_on_a_skipped_one_is_skippable_at_once() {
    let vault = vault().await;
    let b1 = {
        let b = vault.b.lock();
        exec(
            &b,
            "INSERT INTO bank_accounts (id, name, currency, balance) \
             VALUES ('acc-x', 'Main', 'CZK', '0')",
        );
        seal_from(&b, &vault.creds)
    };
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[b1, b2]).await;
    vault.relay.drop_op(1);
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");
    assert!(skip_missing_change(&vault.a.lock(), B, AT).expect("skip"));
    pull_all(&vault.relay, &vault.a, &vault.creds, AT)
        .await
        .expect("pull");

    let b3 = {
        let b = vault.b.lock();
        exec(
            &b,
            "INSERT INTO bank_transactions \
               (id, bank_account_id, tx_type, amount, currency, booking_date) \
             VALUES ('t1', 'acc-x', 'debit', '-10.00', 'CZK', 1767225600)",
        );
        seal_from(&b, &vault.creds)
    };
    let c2 = forge(&vault.creds, &changeset(C, 2, Some([5; 32]), vec![]));
    push(&vault.relay, &vault.creds, &[b3, c2]).await;
    let later = AT + DAY;
    pull_all(&vault.relay, &vault.a, &vault.creds, later)
        .await
        .expect("pull");

    let gaps = held_gaps(&vault.a.lock()).expect("held gaps");
    let of = |device| gaps.iter().find(|g| g.device == device).expect("gap");
    assert_eq!((of(B).kind, of(B).since), (GapKind::Parent, NOW));
    assert_eq!((of(C).kind, of(C).since), (GapKind::Missing, later));
    let views = held_gap_views(&vault.a.lock()).expect("views");
    let b_view = views.iter().find(|v| v.device_id == B.to_hex()).expect("B");
    assert_eq!(
        (b_view.kind, b_view.missing_changes),
        (HeldGapKind::Parent, 1)
    );

    let early = skip_missing_change(&vault.a.lock(), C, later).expect_err("C waits");
    assert!(not_skippable_yet(&early));
    assert!(skip_missing_change(&vault.a.lock(), B, later).expect("skippable at once"));
}

/// Ruling I3: a skip of a missing range gives up every change between the head and the held one.
#[tokio::test]
async fn the_view_counts_the_changes_a_skip_gives_up() {
    let vault = vault().await;
    let cts: Vec<Vec<u8>> = ["b1", "b2", "b3"]
        .iter()
        .map(|id| write_and_seal(&vault, &vault.b, id))
        .collect();
    push(&vault.relay, &vault.creds, &cts).await;
    vault.relay.drop_op(1);
    vault.relay.drop_op(2);
    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    let views = held_gap_views(&vault.a.lock()).expect("views");

    assert_eq!(views.len(), 1);
    assert_eq!(
        (views[0].kind, views[0].missing_changes),
        (HeldGapKind::Missing, 2)
    );
}
