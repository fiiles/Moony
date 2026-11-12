//! Audit M3: an envelope the relay withholds or delivers out of order, or one whose parent has not
//! arrived, holds the cursor before it while everything after it is still handled and this device
//! keeps pushing (`PullEnd::Waiting`, the `Gap` problem).

use super::join::{break_capture, device_of, snapshot_of_a, validation_key};
use super::repair::relay_has_institution;
use super::runtime::{routes, write_and_seal};
use super::*;
use crate::models::sync::{SyncPhase, SyncProblem};
use crate::services::sync::binding::NoBinding;
use crate::services::sync::changeset::SqlValue;
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::{status, Runtime};
use crate::services::sync::engine::{run_round, PullEnd, RoundMode, RoundReport};
use crate::services::sync::hlc::Clock;
use crate::services::sync::join::repair_capture;
use crate::services::sync::relay::fake::Route;

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

fn unsent(conn: &Connection) -> i64 {
    count(conn, "SELECT COUNT(*) FROM sync_unsent")
}

#[tokio::test]
async fn a_withheld_envelope_holds_the_cursor_while_others_sync_and_pushes_go_on() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    push(&vault.relay, &vault.creds, &[b1, b2, c1]).await;
    vault.relay.drop_op(1);
    institution(&vault.a.lock(), "a1");

    let report = round(&vault, &vault.a, RoundMode::Normal).await;

    let pull = report.pull.as_ref().expect("pull");
    assert_eq!(pull.end(), PullEnd::Waiting);
    assert_eq!((pull.applied, pull.gaps, pull.held), (1, 1, 0));
    assert!(pull.may_push() && !pull.may_snapshot());
    assert_eq!(report.push.as_ref().map(|p| p.pushed), Some(1));
    assert_eq!(report.problem, Some(SyncProblem::Gap));
    let mut runtime = Runtime::default();
    runtime.after_round(&report);
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 0, "the cursor stays before B's held envelope");
    assert!(head(&a, C).is_some(), "C's envelope after the gap applied");
    assert!(!has_institution(&a, "b2"));
    assert_eq!(head(&a, B), None);
    let shown = status(&a, &runtime).expect("status");
    assert_eq!(
        (shown.phase, shown.problem),
        (SyncPhase::Waiting, Some(SyncProblem::Gap)),
        "a waiting gap is no stop"
    );
}

#[tokio::test]
async fn a_dependent_envelope_waits_for_its_resent_parent() {
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
    // C's change adds a transaction under B's account X.
    let g = Clock {
        hlc: 1_000 << 16,
        device: C,
    };
    let payment = row(
        "bank_transactions",
        "t1",
        g,
        &[
            ("bank_account_id", text("acc-x")),
            ("tx_type", text("debit")),
            ("amount", text("-10.00")),
            ("currency", text("CZK")),
            ("booking_date", SqlValue::Integer(1_767_225_600)),
        ],
    );
    let c2 = forge(&vault.creds, &changeset(C, 1, None, vec![payment]));
    push(&vault.relay, &vault.creds, &[b1, c2]).await;
    // The relay withholds B1 at its place and serves it again after C's change.
    vault.relay.drop_op(1);
    vault.relay.replay_op(1);

    let first = pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("first pull");

    assert_eq!(first.end(), PullEnd::Waiting);
    assert_eq!(
        (first.applied, first.gaps),
        (1, 1),
        "B1 applied, C's change held"
    );
    assert!(first.may_push());
    {
        let a = vault.a.lock();
        assert_eq!(cursor(&a), 0);
        assert_eq!(
            count(&a, "SELECT COUNT(*) FROM bank_accounts WHERE id = 'acc-x'"),
            1
        );
        assert_eq!(
            count(&a, "SELECT COUNT(*) FROM bank_transactions WHERE id = 't1'"),
            0
        );
    }

    let second = pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("second pull");

    assert!(second.done(), "{second:?}");
    assert_eq!(second.applied, 1);
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 3, "B1's replay is the last op");
    assert_eq!(
        count(&a, "SELECT COUNT(*) FROM bank_transactions WHERE id = 't1'"),
        1
    );
}

#[tokio::test]
async fn a_held_gap_never_uploads_a_snapshot() {
    let vault = vault().await;
    let sealed: Vec<Vec<u8>> = ["b1", "b2"]
        .iter()
        .map(|id| write_and_seal(&vault, &vault.b, id))
        .collect();
    push(&vault.relay, &vault.creds, &sealed).await;
    vault.relay.drop_op(1);
    institution(&vault.a.lock(), "a1");

    // A forced upload is due in `FirstSnapshot`.
    let report = round(&vault, &vault.a, RoundMode::FirstSnapshot).await;

    let pull = report.pull.as_ref().expect("pull");
    assert_eq!(pull.gaps, 1);
    assert!(report.push.is_some(), "the round still pushes");
    assert!(!report.snapshot_uploaded);
    assert_eq!(routes(&vault.relay, Route::PutSnapshot), 0);
    assert_eq!(report.problem, Some(SyncProblem::Gap));
}

#[tokio::test]
async fn own_envelopes_after_a_gap_are_still_acknowledged() {
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    let a1 = write_and_seal(&vault, &vault.a, "a1");
    push(&vault.relay, &vault.creds, &[b1, b2, a1]).await;
    vault.relay.drop_op(1);
    assert_eq!(unsent(&vault.a.lock()), 1);

    let report = pull_all(&vault.relay, &vault.a, &vault.creds, NOW)
        .await
        .expect("pull");

    assert_eq!(report.end(), PullEnd::Waiting);
    let a = vault.a.lock();
    assert_eq!(cursor(&a), 0);
    assert_eq!(unsent(&a), 0, "the relay holds A's envelope");
}

/// `A` writes two envelopes; the relay withholds the first, so a pull holds the second (a gap).
pub(super) async fn withhold_one_of_a(vault: &Vault) {
    let cts = {
        let a = vault.a.lock();
        institution(&a, "a2");
        let first = seal_from(&a, &vault.creds);
        institution(&a, "a3");
        vec![first, seal_from(&a, &vault.creds)]
    };
    push(&vault.relay, &vault.creds, &cts).await;
    let dropped = vault.relay.ops().last().expect("ops").seq - 1;
    vault.relay.drop_op(dropped);
}

/// Coordinator ruling on Task 15: the drain makes sure this device's changes were sent. A pull
/// that waits on another device's missing envelope, with nothing left to send, has drained: the
/// repair goes on and replaces (its backup is kept regardless).
#[tokio::test]
async fn a_repair_with_a_waiting_gap_finishes_once_everything_is_sent() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    {
        let b = vault.b.lock();
        institution(&b, "b1");
        seal_from(&b, &vault.creds);
    }
    break_capture(&vault.b);
    withhold_one_of_a(&vault).await;
    let dir = tempfile::tempdir().expect("dir");

    let outcome = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect("the gap waits, everything of B's was sent");

    assert!(relay_has_institution(&vault, "b1"), "the drain pushed");
    assert_eq!(
        (
            outcome.unsent_items_in_backup_only,
            outcome.unsent_batches_in_backup_only
        ),
        (0, 0)
    );
    assert!(outcome.kept);
    let b = vault.b.lock();
    assert_ne!(device_of(&b), B, "replaced under a new device ID");
    assert!(state::marks(&b)
        .expect("marks")
        .expect("row")
        .capture_broken_at
        .is_none());
}

/// `C`'s chain forks (its second envelope does not follow its first) next to `B`'s withheld one.
async fn a_fork_beside_a_gap(vault: &Vault) {
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    let c2 = forge(&vault.creds, &changeset(C, 2, Some([9; 32]), vec![]));
    let b1 = write_and_seal(vault, &vault.b, "b1");
    let b2 = write_and_seal(vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[c1, c2, b1, b2]).await;
    vault.relay.drop_op(3);
}

/// Task 15 review ruling: a quarantine outranks a gap, so the forked author's error is shown even
/// while another author's envelope is missing; the round still pushes.
#[tokio::test]
async fn a_gap_does_not_hide_a_quarantine() {
    let vault = vault().await;
    a_fork_beside_a_gap(&vault).await;
    institution(&vault.a.lock(), "a1");

    let report = round(&vault, &vault.a, RoundMode::Normal).await;

    let pull = report.pull.as_ref().expect("pull");
    assert_eq!((pull.gaps, pull.held), (1, 1));
    assert_eq!(pull.end(), PullEnd::Held);
    assert!(pull.may_push());
    assert_eq!(report.push.as_ref().map(|p| p.pushed), Some(1));
    assert_eq!(report.problem, Some(SyncProblem::Quarantined));
    let mut runtime = Runtime::default();
    runtime.after_round(&report);
    let shown = status(&vault.a.lock(), &runtime).expect("status");
    assert_eq!(
        (shown.phase, shown.problem),
        (SyncPhase::Error, Some(SyncProblem::Quarantined))
    );
}

/// Task 15 review ruling: a round whose only problem is a waiting gap synced everything it could:
/// it counts for "Last synced" and leaves no error, while the gap is still reported.
#[tokio::test]
async fn a_gap_only_round_records_success() {
    let vault = vault().await;
    let sealed: Vec<Vec<u8>> = ["b1", "b2"]
        .iter()
        .map(|id| write_and_seal(&vault, &vault.b, id))
        .collect();
    push(&vault.relay, &vault.creds, &sealed).await;
    vault.relay.drop_op(1);
    exec(
        &vault.a.lock(),
        "UPDATE sync_state SET last_error = 'sync stopped: offline', last_success_at = NULL",
    );

    let report = round(&vault, &vault.a, RoundMode::Normal).await;

    assert_eq!(report.problem, Some(SyncProblem::Gap));
    {
        let a = vault.a.lock();
        let marks = state::marks(&a).expect("marks").expect("row");
        assert_eq!(marks.last_success_at, Some(NOW));
        let last_error: Option<String> = a
            .query_row("SELECT last_error FROM sync_state", [], |r| r.get(0))
            .expect("last_error");
        assert_eq!(last_error, None);
        let mut runtime = Runtime::default();
        runtime.after_round(&report);
        let shown = status(&a, &runtime).expect("status");
        assert_eq!(
            (shown.phase, shown.problem, shown.last_success_at),
            (SyncPhase::Waiting, Some(SyncProblem::Gap), Some(NOW))
        );
    }

    // A quarantine is still an error: it records its reason.
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    let c2 = forge(&vault.creds, &changeset(C, 2, Some([9; 32]), vec![]));
    push(&vault.relay, &vault.creds, &[c1, c2]).await;
    let report = round(&vault, &vault.a, RoundMode::Normal).await;
    assert_eq!(report.problem, Some(SyncProblem::Quarantined));
    let last_error: Option<String> = vault
        .a
        .lock()
        .query_row("SELECT last_error FROM sync_state", [], |r| r.get(0))
        .expect("last_error");
    assert_eq!(last_error.as_deref(), Some("sync stopped: quarantined"));
}

/// The repair's drain: a quarantine is not clean, with or without a gap beside it.
#[tokio::test]
async fn a_repair_with_a_gap_and_a_quarantine_does_not_finish() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b-local");
    break_capture(&vault.b);
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    let c2 = forge(&vault.creds, &changeset(C, 2, Some([9; 32]), vec![]));
    push(&vault.relay, &vault.creds, &[c1, c2]).await;
    withhold_one_of_a(&vault).await;
    let dir = tempfile::tempdir().expect("dir");

    let error = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect_err("a quarantined author is not a clean drain");

    assert_eq!(
        validation_key(&error),
        Some("validation.syncRepairDrainIncomplete"),
        "{error:?}"
    );
    assert!(relay_has_institution(&vault, "b-local"), "the drain pushed");
    assert_eq!(device_of(&vault.b.lock()), B, "nothing replaced");
}
