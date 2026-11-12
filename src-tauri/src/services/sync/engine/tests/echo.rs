//! Audit M4: a sealed envelope stays in `sync_unsent` until this device pulls it back from the
//! relay (its echo). A push answer alone never removes it, so a relay that acknowledges a push and
//! then loses it cannot make the change disappear: the next round sends it again.

use std::collections::BTreeSet;

use super::join::{break_capture, device_of, repair_backups, snapshot_of_a, validation_key};
use super::repair::relay_has_institution;
use super::*;
use crate::models::sync::{SyncPhase, SyncProblem};
use crate::services::sync::backups::{self, BackupMark};
use crate::services::sync::binding::NoBinding;
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::push::{push_all, MAX_UNECHOED};
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::{status, Runtime};
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::impact::{self, DRAIN_INCOMPLETE};
use crate::services::sync::join::{repair_capture, resync_by_replace};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;
use crate::services::sync::test_support::noise;

async fn round(vault: &Vault, device: &TestConn, mode: RoundMode) -> RoundReport {
    run_round(
        &vault.relay,
        device,
        &mut SnapshotPolicy::default(),
        mode,
        NOW,
        0,
    )
    .await
    .expect("round")
}

/// The sequences waiting in `sync_unsent`, in order.
fn unsent(conn: &Connection) -> Vec<i64> {
    conn.prepare("SELECT seq FROM sync_unsent ORDER BY seq")
        .expect("unsent")
        .query_map([], |r| r.get(0))
        .expect("unsent")
        .collect::<std::result::Result<_, _>>()
        .expect("unsent")
}

fn outbox(conn: &Connection) -> i64 {
    count(conn, "SELECT COUNT(*) FROM sync_outbox")
}

#[tokio::test]
async fn a_push_answer_alone_never_empties_sync_unsent() {
    let vault = vault().await;
    institution(&vault.a.lock(), "a1");

    let report = push_all(&vault.relay, &vault.a, &vault.creds)
        .await
        .expect("push");

    assert_eq!((report.pushed, report.acknowledged.clone()), (1, vec![1]));
    assert!(!report.unechoed_cap);
    assert_eq!(vault.relay.ops().len(), 1, "the relay took it");
    assert_eq!(
        unsent(&vault.a.lock()),
        vec![1],
        "it waits for its echo, not for the answer"
    );

    let pulled = pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    assert!(pulled.done());
    assert_eq!(pulled.applied, 0, "an echo is acknowledged, not applied");
    assert!(unsent(&vault.a.lock()).is_empty());
}

#[tokio::test]
async fn one_push_all_sends_each_envelope_once() {
    let vault = vault().await;
    {
        let a = vault.a.lock();
        for i in 0..150 {
            institution(&a, &format!("i{i:03}"));
            seal_from(&a, &vault.creds);
        }
        let barrier = state::load(&a).expect("state").expect("row").barrier;
        state::note_sealed_at_barrier(&a, barrier).expect("barrier");
        institution(&a, "late");
    }

    for call in ["first", "again, nothing echoed yet"] {
        let from = vault.relay.pushed_ids().len();

        let report = push_all(&vault.relay, &vault.a, &vault.creds)
            .await
            .expect(call);

        let sent: Vec<String> = vault.relay.pushed_ids()[from..].concat();
        let distinct: BTreeSet<&String> = sent.iter().collect();
        assert_eq!(sent.len(), 151, "{call}: every waiting envelope");
        assert_eq!(distinct.len(), sent.len(), "{call}: each one once");
        assert_eq!(report.pushed, 151, "{call}");
        assert_eq!(
            report.acknowledged,
            (1..=151).collect::<Vec<u64>>(),
            "{call}"
        );
    }
    assert_eq!(vault.relay.ops().len(), 151, "the relay keeps each op once");
    assert_eq!(unsent(&vault.a.lock()).len(), 151);
}

#[tokio::test]
async fn a_large_outbox_drains_in_one_round_on_an_honest_relay() {
    let vault = vault().await;
    {
        let a = vault.a.lock();
        // 200 rows of 40 KiB incompressible notes: one row per 60 KiB envelope.
        for i in 0..200u64 {
            a.execute(
                "INSERT INTO institutions (id, name) VALUES (?1, ?2)",
                params![format!("i{i:03}"), noise(40 * 1024, i)],
            )
            .expect("institution");
        }
    }

    let report = push_all(&vault.relay, &vault.a, &vault.creds)
        .await
        .expect("push");

    assert_eq!(report.pushed, 200);
    assert!(!report.unechoed_cap);
    assert_eq!(vault.relay.ops().len(), 200);
    {
        let a = vault.a.lock();
        assert_eq!(outbox(&a), 0);
        assert_eq!(unsent(&a).len(), 200);
    }

    pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    assert!(unsent(&vault.a.lock()).is_empty());
}

/// `n` envelopes this device sealed earlier that the relay never served back: own-log entries and
/// `sync_unsent` rows `1..=n`, the chain moved on past them.
fn unechoed(conn: &Connection, n: usize) {
    exec(
        conn,
        &format!(
            "WITH RECURSIVE s(n) AS (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < {n}) \
             INSERT INTO sync_own_log (seq, hash, min_hlc, max_hlc) \
               SELECT n, randomblob(32), 1, 1 FROM s; \
             INSERT INTO sync_unsent (seq, envelope) \
               SELECT seq, randomblob(64) FROM sync_own_log; \
             UPDATE sync_state SET next_seq = {n} + 1, \
               last_hash = (SELECT hash FROM sync_own_log WHERE seq = {n}) WHERE id = 1;"
        ),
    );
}

#[tokio::test]
async fn unechoed_envelopes_past_the_cap_are_reported() {
    let vault = vault().await;
    // A relay that acknowledges every push and never serves anything back.
    vault.relay.set_swallow_pushes(true);
    unechoed(&vault.a.lock(), MAX_UNECHOED - 1);
    institution(&vault.a.lock(), "a1");

    let first = round(&vault, &vault.a, RoundMode::Normal).await;

    let push = first.push.as_ref().expect("push");
    assert!(push.unechoed_cap);
    assert_eq!(push.pushed, MAX_UNECHOED, "the waiting ones, then a1");
    assert_eq!(first.problem, Some(SyncProblem::RelayError));
    {
        let a = vault.a.lock();
        assert_eq!(unsent(&a).len(), MAX_UNECHOED);
        assert_eq!(outbox(&a), 0, "a1 was the last one sealed");
    }

    institution(&vault.a.lock(), "a2");
    let second = round(&vault, &vault.a, RoundMode::Normal).await;

    assert!(second.push.as_ref().expect("push").unechoed_cap);
    assert_eq!(second.problem, Some(SyncProblem::RelayError));
    let a = vault.a.lock();
    assert_eq!(unsent(&a).len(), MAX_UNECHOED, "nothing more is sealed");
    assert_eq!(outbox(&a), 1, "a2 waits in the outbox");
    assert!(vault.relay.ops().is_empty());
    let mut runtime = Runtime::default();
    runtime.after_round(&second);
    let shown = status(&a, &runtime).expect("status");
    assert_eq!(
        (shown.phase, shown.problem),
        (SyncPhase::Error, Some(SyncProblem::RelayError))
    );
}

/// On an honest relay a round that pushed pulls its echo, so nothing it sent is left counted as
/// pending.
#[tokio::test]
async fn a_round_that_pushed_pulls_its_echo() {
    let vault = vault().await;
    institution(&vault.a.lock(), "a1");

    let report = round(&vault, &vault.a, RoundMode::Normal).await;

    assert_eq!(report.problem, None);
    assert_eq!(report.push.as_ref().map(|p| p.pushed), Some(1));
    let a = vault.a.lock();
    assert!(unsent(&a).is_empty());
    let shown = status(&a, &Runtime::default()).expect("status");
    assert_eq!(shown.pending_changes, 0);
}

/// Task 6's count: a re-sync replaces after its best-effort push, without a pull, so what stays
/// only in the backup is what the relay did not take in that push. An envelope the relay took
/// earlier but never served back is sent again by that push.
#[tokio::test]
async fn a_resync_counts_only_what_the_relay_did_not_take() {
    for refused in [false, true] {
        let vault = vault().await;
        snapshot_of_a(&vault, "a1").await;
        round(&vault, &vault.b, RoundMode::Normal).await;
        institution(&vault.b.lock(), "b1");
        push_all(&vault.relay, &vault.b, &vault.creds)
            .await
            .expect("b1 pushed, never pulled back");
        institution(&vault.b.lock(), "b2");
        let consent = agreed(&vault.b);
        assert_eq!(
            impact::replace_impact(&vault.b.lock())
                .expect("impact")
                .pending_batches,
            1,
            "b1 waits for its echo"
        );
        if refused {
            vault.relay.fail_next(Route::Push, RelayError::Server(503));
        }
        let dir = tempfile::tempdir().expect("dir");

        let outcome =
            resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
                .await
                .expect("resync");

        let counts = (
            outcome.unsent_items_in_backup_only,
            outcome.unsent_batches_in_backup_only,
        );
        let listed = backups::list(dir.path()).expect("list");
        if refused {
            assert_eq!(counts, (1, 1), "b2 queued, b1 unconfirmed");
            assert!(outcome.kept);
            assert_eq!(listed[0].mark, BackupMark::Kept);
        } else {
            assert_eq!(counts, (0, 0), "the push took both");
            assert!(!outcome.kept);
            assert_eq!(listed[0].mark, BackupMark::Plain);
            assert!(relay_has_institution(&vault, "b1") && relay_has_institution(&vault, "b2"));
        }
    }
}

/// An edit after consent that the re-sync's own push delivered is not left only in the backup:
/// the hold does not count it against the consent (Task 6), although it waits for its echo.
#[tokio::test]
async fn an_edit_after_consent_that_the_resync_sent_does_not_stop_it() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    let consent = agreed(&vault.b);
    institution(&vault.b.lock(), "b-late");
    let dir = tempfile::tempdir().expect("dir");

    let outcome = resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
        .await
        .expect("b-late reached the relay");

    assert!(relay_has_institution(&vault, "b-late"));
    assert_eq!(
        (
            outcome.unsent_items_in_backup_only,
            outcome.unsent_batches_in_backup_only
        ),
        (0, 0)
    );
    assert_ne!(device_of(&vault.b.lock()), B);
}

#[tokio::test]
async fn the_repair_drains_until_its_envelopes_come_back() {
    // An honest relay: the drain's round pulls the echo, so one push is enough.
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    let from = vault.relay.calls().len();
    let dir = tempfile::tempdir().expect("dir");

    repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect("repaired once the echo came back");

    assert!(relay_has_institution(&vault, "b1"));
    let pushes = vault.relay.calls()[from..]
        .iter()
        .filter(|route| **route == Route::Push)
        .count();
    assert_eq!(pushes, 1);
    assert_ne!(device_of(&vault.b.lock()), B);

    // A relay that acknowledges every push and keeps nothing: the envelope never comes back, two
    // rounds without progress end the drain and nothing is replaced.
    let vault = super::vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    vault.relay.set_swallow_pushes(true);
    let dir = tempfile::tempdir().expect("dir");

    let error = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("the envelope never came back");

    assert_eq!(validation_key(&error), Some(DRAIN_INCOMPLETE), "{error:?}");
    assert!(!relay_has_institution(&vault, "b1"));
    let b = vault.b.lock();
    assert_eq!(device_of(&b), B, "nothing replaced");
    assert_eq!(unsent(&b).len(), 1, "b1 still waits to be sent");
    assert_eq!(
        repair_backups(dir.path()),
        vec![format!("sync-repair-{NOW}-unused.zip")]
    );
}

/// A restore after `410` moves the cursor to the snapshot's sequence, past this device's own
/// envelopes at or below it: the snapshot's head for this device says which of them the vault
/// holds, and those stop waiting for an echo that will never be served to this device.
#[tokio::test]
async fn a_restore_after_410_acknowledges_what_the_snapshot_holds() {
    let vault = vault().await;
    let b0 = write_and_seal_b(&vault, "b0");
    push(&vault.relay, &vault.creds, &[b0]).await;
    institution(&vault.a.lock(), "a1");
    push_all(&vault.relay, &vault.a, &vault.creds)
        .await
        .expect("A pushes a1 and does not pull");
    assert_eq!(unsent(&vault.a.lock()), vec![1]);
    let uploaded = round(&vault, &vault.b, RoundMode::FirstSnapshot).await;
    assert!(uploaded.snapshot_uploaded, "{uploaded:?}");
    assert_eq!(vault.relay.snapshot_seq(), Some(2));
    // The relay prunes below A's envelope, which stays at sequence 2 under its op ID.
    vault.relay.prune_to(1);

    let report = round(&vault, &vault.a, RoundMode::Normal).await;

    assert!(report.restored, "{report:?}");
    assert_eq!(report.problem, None);
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 2);
    assert!(unsent(&a).is_empty(), "the snapshot holds a1");
    assert_eq!(vault.relay.ops().len(), 1, "a1 kept once");
}

fn write_and_seal_b(vault: &Vault, id: &str) -> Vec<u8> {
    let b = vault.b.lock();
    institution(&b, id);
    seal_from(&b, &vault.creds)
}
