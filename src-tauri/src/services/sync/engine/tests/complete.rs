//! A complete sync (spec 2026-11-14 §7.2): a round with no problem at all that leaves no change
//! to send and no document or photo to upload records `sync_state.last_complete_at` beside its
//! success; a round with a problem, or with anything left here, records its success at most.

use super::runtime::write_and_seal;
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;
use crate::services::sync::state::EngineMarks;
use crate::services::sync::test_support::noise;

async fn round_at(vault: &Vault, device: &TestConn, now: i64) -> RoundReport {
    run_round(
        &vault.relay,
        device,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        now,
        0,
    )
    .await
    .expect("round")
}

fn marks(device: &TestConn) -> EngineMarks {
    state::marks(&device.lock())
        .expect("marks")
        .expect("sync is on")
}

#[tokio::test]
async fn a_clean_round_with_nothing_left_records_a_complete_sync() {
    // proves the happy path
    let vault = vault().await;
    institution(&vault.a.lock(), "a1");

    let report = round_at(&vault, &vault.a, NOW).await;

    assert_eq!(report.problem, None);
    let marks = marks(&vault.a);
    assert_eq!(marks.last_complete_at, Some(NOW));
    assert_eq!(marks.last_complete_at, marks.last_success_at);
}

#[tokio::test]
async fn a_round_that_leaves_a_refused_row_is_not_complete() {
    // proves a row too large to send (it stays in the outbox) keeps the sync incomplete
    let vault = vault().await;
    {
        let a = vault.a.lock();
        exec(
            &a,
            "INSERT INTO watched_stocks (id, ticker) VALUES ('w1', 'AAPL')",
        );
        a.execute(
            "UPDATE watched_stocks SET notes = ?1 WHERE id = 'w1'",
            [noise(300 * 1024, 29)],
        )
        .expect("notes");
        institution(&a, "a1");
    }

    let report = round_at(&vault, &vault.a, NOW).await;

    assert_eq!(report.push.as_ref().map(|push| push.refused), Some(1));
    assert_eq!(
        count(&vault.a.lock(), "SELECT COUNT(*) FROM sync_outbox"),
        1,
        "the refused row stays queued"
    );
    let marks = marks(&vault.a);
    assert_eq!(marks.last_success_at, Some(NOW));
    assert_eq!(marks.last_complete_at, None);
}

#[tokio::test]
async fn a_round_that_holds_a_gap_records_success_but_not_complete() {
    // proves a waiting gap (`sync_stalled` raises it at once) is not a complete sync
    let vault = vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    push(&vault.relay, &vault.creds, &[b1, b2, c1]).await;
    vault.relay.drop_op(1);
    institution(&vault.a.lock(), "a1");

    let report = round_at(&vault, &vault.a, NOW).await;

    assert_eq!(report.problem, Some(SyncProblem::Gap));
    let marks = marks(&vault.a);
    assert_eq!(marks.last_success_at, Some(NOW));
    assert_eq!(marks.last_complete_at, None);
}

#[tokio::test]
async fn a_failed_round_records_neither() {
    // proves an unreachable server leaves both times alone
    let vault = vault().await;
    institution(&vault.a.lock(), "a1");
    round_at(&vault, &vault.a, NOW).await;
    assert_eq!(marks(&vault.a).last_complete_at, Some(NOW));
    for route in [
        Route::ServerInfo,
        Route::VaultInfo,
        Route::Push,
        Route::Pull,
        Route::PutSnapshot,
        Route::GetSnapshot,
        Route::PutBlob,
        Route::HeadBlob,
        Route::GetBlob,
        Route::ListBlobs,
        Route::DeleteBlob,
    ] {
        vault
            .relay
            .set_failing(route, Some(RelayError::Network("offline".into())));
    }

    let failed = run_round(
        &vault.relay,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW + 600,
        0,
    )
    .await;

    assert!(failed.is_err(), "{failed:?}");
    let marks = marks(&vault.a);
    assert_eq!(
        (marks.last_success_at, marks.last_complete_at),
        (Some(NOW), Some(NOW))
    );
}

/// A document added after the round's queue update (here: the update fails, so its blob gets no
/// entry) is pushed with the round and leaves the outbox empty, but its content exists only here:
/// the round records its success and no complete sync.
#[tokio::test]
async fn a_document_whose_blob_has_no_queue_entry_keeps_the_sync_incomplete() {
    // proves a row naming a blob that has no queue entry counts as content left to upload
    let vault = vault().await;
    with_store(&vault.a);
    {
        let a = vault.a.lock();
        exec(
            &a,
            "INSERT INTO insurance_policies \
               (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
             VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly'); \
             CREATE TEMP TRIGGER no_entries BEFORE INSERT ON sync_blobs \
             BEGIN SELECT RAISE(ABORT, 'no queue entries in this test'); END;",
        );
        let blob = crate::services::attachment_store::put_bytes(&a, b"%PDF-1.7 contract")
            .expect("blob")
            .id;
        a.execute(
            "INSERT INTO insurance_documents (id, insurance_id, name, blob_id, uploaded_at) \
             VALUES ('d1', 'p1', 'Contract', ?1, 1767225600)",
            [&blob],
        )
        .expect("document");
    }

    let report = round_at(&vault, &vault.a, NOW).await;

    assert_eq!(report.problem, None);
    assert_eq!(
        count(&vault.a.lock(), "SELECT COUNT(*) FROM sync_outbox"),
        0,
        "the row was pushed"
    );
    assert_eq!(
        count(&vault.a.lock(), "SELECT COUNT(*) FROM sync_blobs"),
        0,
        "its blob has no entry"
    );
    let marks = marks(&vault.a);
    assert_eq!(
        (marks.last_success_at, marks.last_complete_at),
        (Some(NOW), None)
    );
}
