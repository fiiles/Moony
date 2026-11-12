//! Audit M3: an envelope the relay withholds or delivers out of order, or one whose parent has not
//! arrived, holds the cursor before it while everything after it is still handled and this device
//! keeps pushing (`PullEnd::Waiting`, the `Gap` problem).

use super::runtime::{routes, write_and_seal};
use super::*;
use crate::models::sync::{SyncPhase, SyncProblem};
use crate::services::sync::changeset::SqlValue;
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::{status, Runtime};
use crate::services::sync::engine::{run_round, PullEnd, RoundMode, RoundReport};
use crate::services::sync::hlc::Clock;
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
    institution(&vault.a.lock(), "a1");

    let first = round(&vault, &vault.a, RoundMode::Normal).await;

    let pull = first.pull.as_ref().expect("pull");
    assert_eq!(pull.end(), PullEnd::Waiting);
    assert_eq!(
        (pull.applied, pull.gaps),
        (1, 1),
        "B1 applied, C's change held"
    );
    assert_eq!(first.push.as_ref().map(|p| p.pushed), Some(1));
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
    assert_eq!(cursor(&a), 4);
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
