//! A sync server that went back (Task 16 review, round 2): a page whose head is below this
//! device's place marks it (`sync.vault_went_back_at`), sticky through every round until a
//! completed replace or turning sync off; meanwhile the status shows `ServerWentBack` and every
//! re-sync keeps its backup.

use super::join::{break_capture, snapshot_of_a};
use super::pull::{write_and_seal, Mangled};
use super::*;
use crate::models::sync::{SyncPhase, SyncProblem};
use crate::services::sync::backups::{self, BackupMark};
use crate::services::sync::binding::NoBinding;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::{status, Runtime};
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::join::{repair_capture, resync_by_replace};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{PullPage, RelayError};
use crate::services::sync::setup;

async fn round(vault: &Vault, device: &TestConn) -> RoundReport {
    run_round(
        &vault.relay,
        device,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("round")
}

fn went_back_at(device: &TestConn) -> Option<i64> {
    state::vault_went_back_at(&device.lock()).expect("mark")
}

/// `B` syncs, then meets a sync server whose head is below its cursor (restored from an older
/// backup).
async fn went_back(vault: &Vault) -> RoundReport {
    snapshot_of_a(vault, "a1").await;
    round(vault, &vault.b).await;
    assert!(cursor(&vault.b.lock()) > 0);
    let restored = Mangled {
        inner: vault.relay.clone(),
        mangle: |_, mut page: PullPage| {
            page.head = 0;
            page
        },
    };
    run_round(
        &restored,
        &vault.b,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("round")
}

fn shown(device: &TestConn, report: Option<&RoundReport>) -> (SyncPhase, Option<SyncProblem>) {
    let mut runtime = Runtime::default();
    if let Some(report) = report {
        runtime.after_round(report);
    }
    let status = status(&device.lock(), &runtime).expect("status");
    (status.phase, status.problem)
}

#[tokio::test]
async fn a_head_below_the_cursor_marks_the_server_as_gone_back() {
    let vault = vault().await;

    let report = went_back(&vault).await;

    assert_eq!(went_back_at(&vault.b), Some(NOW));
    assert_eq!(report.problem, Some(SyncProblem::ServerWentBack));
    assert_eq!(
        shown(&vault.b, Some(&report)),
        (SyncPhase::Error, Some(SyncProblem::ServerWentBack))
    );
    // A fresh engine (no round yet) shows it too.
    assert_eq!(shown(&vault.b, None).1, Some(SyncProblem::ServerWentBack));
    assert_eq!(went_back_at(&vault.a), None, "A never saw it");
}

#[tokio::test]
async fn the_status_shows_the_server_went_back_after_the_device_resumes() {
    let vault = vault().await;
    went_back(&vault).await;

    // The head is past B's place again: B syncs on, and still says what happened.
    let report = round(&vault, &vault.b).await;

    assert!(report.pull.as_ref().is_some_and(|p| p.done()), "{report:?}");
    assert_eq!(report.problem, Some(SyncProblem::ServerWentBack));
    assert_eq!(
        shown(&vault.b, Some(&report)),
        (SyncPhase::Error, Some(SyncProblem::ServerWentBack))
    );
}

/// Final review A, I1: while the mark stands this device uploads no snapshot, the forced one after
/// a barrier crossing included. The restored server may have renumbered a change below this
/// device's cursor, so a snapshot at its cursor could claim a change it never saw. A forced round
/// still asks for the vault's info, and keeps asking.
#[tokio::test]
async fn a_device_that_saw_the_server_go_back_uploads_no_snapshot() {
    let vault = vault().await;
    went_back(&vault).await;
    // The head passes B's place again, so B's pull ends done.
    let a2 = write_and_seal(&vault, &vault.a, "a2");
    push(&vault.relay, &vault.creds, &[a2]).await;
    let before = vault.relay.snapshot_seq();
    let from = vault.relay.calls().len();

    let report = run_round(
        &vault.relay,
        &vault.b,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
        NOW,
        0,
    )
    .await
    .expect("round");

    assert!(report.pull.as_ref().is_some_and(|p| p.done()), "{report:?}");
    assert!(!report.snapshot_uploaded, "{report:?}");
    let calls = vault.relay.calls()[from..].to_vec();
    assert!(calls.contains(&Route::VaultInfo), "{calls:?}");
    assert!(!calls.contains(&Route::PutSnapshot), "{calls:?}");
    assert_eq!(vault.relay.snapshot_seq(), before);
}

#[tokio::test]
async fn the_mark_survives_a_gap_round_and_an_offline_round_and_a_resync_keeps_its_backup() {
    let vault = vault().await;
    went_back(&vault).await;
    // A gap: the relay withholds A's next envelope, so B waits for it.
    let cts = vec![
        write_and_seal(&vault, &vault.a, "a2"),
        write_and_seal(&vault, &vault.a, "a3"),
    ];
    push(&vault.relay, &vault.creds, &cts).await;
    let withheld = vault.relay.ops().last().expect("ops").seq - 1;
    vault.relay.drop_op(withheld);

    let gap = round(&vault, &vault.b).await;

    assert!(gap.pull.as_ref().is_some_and(|p| p.gaps > 0), "{gap:?}");
    assert_eq!(
        gap.problem,
        Some(SyncProblem::ServerWentBack),
        "outranks the gap"
    );
    assert_eq!(went_back_at(&vault.b), Some(NOW));

    vault
        .relay
        .fail_next(Route::Pull, RelayError::Network("offline".into()));
    let offline = run_round(
        &vault.relay,
        &vault.b,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await;
    assert!(offline.is_err());
    assert_eq!(went_back_at(&vault.b), Some(NOW));

    let consent = agreed(&vault.b);
    let dir = tempfile::tempdir().expect("dir");
    let outcome = resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &NoBinding, NOW)
        .await
        .expect("resync");

    assert_eq!(
        (
            outcome.unsent_items_in_backup_only,
            outcome.unsent_batches_in_backup_only
        ),
        (0, 0)
    );
    assert!(
        outcome.kept,
        "the server went back: kept whatever the counts"
    );
    assert_eq!(
        backups::list(dir.path()).expect("list")[0].mark,
        BackupMark::Kept
    );
    assert_eq!(
        went_back_at(&vault.b),
        None,
        "a completed replace clears it"
    );
}

#[tokio::test]
async fn a_repair_completes_keeps_its_backup_and_clears_the_mark() {
    let vault = vault().await;
    went_back(&vault).await;
    institution(&vault.b.lock(), "b1");
    break_capture(&vault.b);
    let dir = tempfile::tempdir().expect("dir");

    let outcome = repair_capture(&vault.relay, &vault.b, dir.path(), &NoBinding, NOW)
        .await
        .expect("the drain judges the pull itself");

    assert!(outcome.kept);
    assert_eq!(went_back_at(&vault.b), None);
}

#[tokio::test]
async fn turning_sync_off_clears_the_mark() {
    let vault = vault().await;
    went_back(&vault).await;
    let dir = tempfile::tempdir().expect("dir");

    setup::turn_off(&mut vault.b.lock(), dir.path()).expect("turn off");

    assert_eq!(went_back_at(&vault.b), None);
}

/// Rule 9a and the server that went back: a restored backup hands out positions again, honestly
/// and with other ops. The notes of what this device had handled go with the mark, so the pull
/// that follows is not read as a replaced position; the message stays the server's own.
#[tokio::test]
async fn a_server_that_went_back_clears_the_notes_and_keeps_its_own_message() {
    let vault = vault().await;
    let b0 = write_and_seal(&vault, &vault.b, "b0");
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    let b2 = write_and_seal(&vault, &vault.b, "b2");
    push(&vault.relay, &vault.creds, &[b0, b1, b2]).await;
    vault.relay.drop_op(2);
    round(&vault, &vault.a).await;
    {
        let a = vault.a.lock();
        assert_eq!(cursor(&a), 1, "A holds its cursor before the withheld op");
        assert_eq!(
            count(&a, "SELECT COUNT(*) FROM sync_seen_ops"),
            1,
            "b2 at 3 is noted"
        );
    }

    // The server is restored from a backup older than A's cursor; the log then fills again.
    vault
        .relay
        .roll_back_head(&vault.creds.vault_id.to_b64(), 0);
    let first = round(&vault, &vault.a).await;
    assert_eq!(first.problem, Some(SyncProblem::ServerWentBack));
    assert_eq!(
        count(&vault.a.lock(), "SELECT COUNT(*) FROM sync_seen_ops"),
        0,
        "the notes went with the mark"
    );
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    let c2 = forge(
        &vault.creds,
        &changeset(C, 2, Some(envelope_hash(&c1)), vec![]),
    );
    let c3 = forge(
        &vault.creds,
        &changeset(C, 3, Some(envelope_hash(&c2)), vec![]),
    );
    push(&vault.relay, &vault.creds, &[c1, c2, c3]).await;

    let second = round(&vault, &vault.a).await;

    assert_eq!(
        second.problem,
        Some(SyncProblem::ServerWentBack),
        "{second:?}"
    );
    assert_ne!(
        second.pull.as_ref().and_then(|pull| pull.stop.clone()),
        Some(crate::services::sync::engine::PullStop::Invalid),
        "a reused position is not a replaced one: {second:?}"
    );
}
