//! The second sync audit (2026-11-20), plan 2 task 8: while a device holds a key-change head it
//! has not verified (its own, not signed yet, or another device's, not confirmed yet), it hands no
//! keys on: it pairs nobody, uploads no snapshot and starts no removal or new Recovery Kit; a
//! remover whose own head is unsigned also repairs and re-syncs nothing (ruling R11). The status
//! says what the device waits for (spec `docs/specs/2026-11-20-sync-audit-2-fixes-design.md`
//! §2.3 step 5 and §2.5).

use super::rekey_fixture::*;
use super::*;
use crate::error::AppError;
use crate::models::sync::SyncProblem;
use crate::services::sync::binding::NoBinding;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::{status_at, Runtime};
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::join;
use crate::services::sync::pairing::holder;
use crate::services::sync::rekey::head;
use crate::services::sync::rekey::rotate;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;

const IN_PROGRESS: &str = "validation.syncChangeInProgress";

fn key_of(error: &AppError) -> Option<&str> {
    match error {
        AppError::Validation(key) => Some(key.as_str()),
        _ => None,
    }
}

fn sync_key_of(error: &crate::services::sync::error::SyncError) -> Option<String> {
    match error {
        crate::services::sync::error::SyncError::Local(AppError::Validation(key)) => {
            Some(key.clone())
        }
        _ => None,
    }
}

/// A removed C and A's range read failing for the commit: A holds its own head unsigned. B has
/// picked the change up and holds A's head, which A's signed record has not confirmed.
async fn unsigned_remover() -> Three {
    let three = three().await;
    institution(&three.vault.b.lock(), "b-ahead");
    round_of(&three.vault, &three.vault.b).await;
    three
        .vault
        .relay
        .set_failing(Route::Pull, Some(RelayError::Network("offline".into())));
    remove(&three, A, &[C]).await.expect("the key changed");
    three.vault.relay.set_failing(Route::Pull, None);
    round_of(&three.vault, &three.vault.b).await;
    {
        let a = three.vault.a.lock();
        assert!(head::own_unverified(&a).expect("own"), "A's own head");
        let b = three.vault.b.lock();
        assert!(head::any_unverified(&b).expect("any"), "B holds A's head");
        assert!(!head::own_unverified(&b).expect("own"), "not B's own");
    }
    three
}

/// Rounds of the two devices that stayed until nobody holds an unverified head.
async fn verified(three: &Three) {
    for _ in 0..4 {
        round_of(&three.vault, &three.vault.a).await;
        round_of(&three.vault, &three.vault.b).await;
    }
    for id in [A, B] {
        assert!(
            !head::any_unverified(&three.device(id).lock()).expect("any"),
            "{id:?} verified"
        );
    }
}

/// A snapshot of what `device` holds now, put on the server by hand: what a build without this
/// gate (or a hostile device) uploads while its head is unverified. An honest device does not any
/// more (`a_device_with_an_unverified_head_uploads_no_snapshot`); the tests of what a reader does
/// with such a snapshot make it this way.
pub(super) async fn ungated_snapshot_by(three: &Three, device: &TestConn) {
    let (creds, sealed) = {
        let conn = device.lock();
        let creds = crate::services::sync::keys::credentials(&conn)
            .expect("credentials")
            .expect("sync is on");
        let sealed =
            crate::services::sync::engine::snapshots::dump_sealed(&conn, &creds).expect("dump");
        (creds, sealed)
    };
    three
        .vault
        .relay
        .put_snapshot(&creds.auth(), sealed.seq, sealed.bytes)
        .await
        .expect("the server takes it");
    state::record_snapshot_uploaded(&device.lock(), sealed.seq, sealed.stats.max_hlc, NOW)
        .expect("noted");
}

/// Takes the note of every unverified head away: what a build without this gate looks like to
/// `rotate::check`, for the tests that go on with a change while a head waits.
pub(super) fn set_unverified_aside(conn: &Connection) {
    exec(
        conn,
        &format!(
            "DELETE FROM app_config WHERE key = '{}'",
            state::UNVERIFIED_EPOCHS_KEY
        ),
    );
}

fn unsent(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM sync_unsent", [], |r| r.get(0))
        .expect("count")
}

#[tokio::test]
async fn a_device_with_an_unverified_head_pairs_nobody() {
    let three = unsigned_remover().await;
    for id in [A, B] {
        let conn = three.device(id).lock();
        let before = unsent(&conn);
        let held = holder::announced_payload(&conn, NOW).expect_err("refused");
        assert_eq!(key_of(&held), Some(IN_PROGRESS), "{id:?}");
        let held = holder::holder_payload(&conn, NOW).expect_err("refused");
        assert_eq!(key_of(&held), Some(IN_PROGRESS), "{id:?}");
        assert_eq!(unsent(&conn), before, "{id:?}: no ticket was announced");
        assert_eq!(
            holder::refuse_while_unverified(&conn)
                .expect_err("refused")
                .to_string(),
            held.to_string()
        );
    }

    verified(&three).await;

    let b = three.vault.b.lock();
    let payload = holder::announced_payload(&b, NOW).expect("payload");
    assert!(payload.is_some(), "verified, B pairs again");
    holder::refuse_while_unverified(&b).expect("allowed");
}

#[tokio::test]
async fn a_device_with_an_unverified_head_uploads_no_snapshot() {
    let three = unsigned_remover().await;
    let from = three.vault.relay.calls().len();

    let report = run_round(
        &three.vault.relay,
        &three.vault.b,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
        NOW,
        0,
    )
    .await
    .expect("round");

    assert!(!report.snapshot_uploaded, "{report:?}");
    let calls = three.vault.relay.calls()[from..].to_vec();
    assert!(!calls.contains(&Route::PutSnapshot), "{calls:?}");

    verified(&three).await;
    let report = run_round(
        &three.vault.relay,
        &three.vault.b,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
        NOW,
        0,
    )
    .await
    .expect("round");
    assert!(report.snapshot_uploaded, "verified, it uploads: {report:?}");
}

#[tokio::test]
async fn a_device_with_an_unverified_head_starts_no_removal() {
    let three = unsigned_remover().await;
    for id in [A, B] {
        let conn = three.device(id).lock();
        for (removing, new_kit) in [(vec![A, B], false), (vec![], true)] {
            let removing: Vec<_> = removing.into_iter().filter(|d| *d != id).collect();
            let error = rotate::plan(&conn, &removing, &[], new_kit, NOW).expect_err("refused");
            assert_eq!(key_of(&error), Some(IN_PROGRESS), "{id:?}: {error}");
        }
    }
    // The call itself answers the same, before anything is sent (the consent is never read).
    let before = three.vault.relay.ops().len();
    for (removing, new_kit) in [(vec![A], false), (vec![], true)] {
        let error = rotate::remove(
            &three.vault.relay,
            &three.vault.b,
            rotate::Removal {
                removing: &removing,
                new_kit,
                consent: "not read",
            },
            three.dir.path(),
            &|| {},
            NOW,
        )
        .await
        .expect_err("refused");
        assert_eq!(
            sync_key_of(&error).as_deref(),
            Some(IN_PROGRESS),
            "{error:?}"
        );
    }
    assert_eq!(three.vault.relay.ops().len(), before);
    assert_eq!(key_epoch(&three.vault.b.lock()), 1, "no second change");

    verified(&three).await;
    rotate::plan(&three.vault.b.lock(), &[A], &[], false, NOW).expect("a plan again");
}

/// A device that waits for another's head still pushes its ordinary changes (only the remover
/// holds still completely): the gate takes keys and snapshots, not rows.
#[tokio::test]
async fn a_device_waiting_for_another_head_still_sends_its_changes() {
    let three = unsigned_remover().await;
    institution(&three.vault.b.lock(), "b-while-waiting");
    let before = three.vault.relay.ops().len();

    round_of(&three.vault, &three.vault.b).await;

    assert!(three.vault.relay.ops().len() > before, "B's row went out");
}

/// Ruling R11: a repair and a re-sync give the device a new ID, and the others wait for a record
/// signed by the old one.
#[tokio::test]
async fn a_remover_with_an_unsigned_head_cannot_repair() {
    let three = unsigned_remover().await;
    let device = state::load(&three.vault.a.lock())
        .expect("state")
        .expect("row")
        .device;

    let error = join::repair_capture(
        &three.vault.relay,
        &three.vault.a,
        three.dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect_err("refused");

    assert_eq!(
        sync_key_of(&error).as_deref(),
        Some(IN_PROGRESS),
        "{error:?}"
    );
    let a = three.vault.a.lock();
    assert_eq!(state::load(&a).expect("state").expect("row").device, device);
    assert!(head::own_unverified(&a).expect("own"), "nothing replaced");
}

#[tokio::test]
async fn a_remover_with_an_unsigned_head_cannot_resync() {
    let three = unsigned_remover().await;
    let device = state::load(&three.vault.a.lock())
        .expect("state")
        .expect("row")
        .device;
    let consent = agreed(&three.vault.a);

    let error = join::resync_by_replace(
        &three.vault.relay,
        &three.vault.a,
        consent,
        three.dir.path(),
        &NoBinding,
        NOW,
    )
    .await
    .expect_err("refused");

    assert_eq!(
        sync_key_of(&error).as_deref(),
        Some(IN_PROGRESS),
        "{error:?}"
    );
    let a = three.vault.a.lock();
    assert_eq!(state::load(&a).expect("state").expect("row").device, device);
    assert!(head::own_unverified(&a).expect("own"), "nothing replaced");
    assert!(
        std::fs::read_dir(three.dir.path())
            .expect("dir")
            .all(|entry| !entry
                .expect("entry")
                .file_name()
                .to_string_lossy()
                .starts_with("sync-repair")),
        "no backup was written"
    );
}

/// The status of a remover and of a device waiting for the remover, with the round's problem.
fn shown(three: &Three, id: DeviceId, runtime: &Runtime, now: i64) -> crate::models::SyncStatus {
    status_at(&three.device(id).lock(), runtime, now).expect("status")
}

#[tokio::test]
async fn the_status_says_a_remover_waits_for_the_sync_server() {
    let three = unsigned_remover().await;
    let mut runtime = Runtime::default();
    // A device that waits for the remover shows no "finishing".
    let b = shown(&three, B, &Runtime::default(), NOW);
    assert!(!b.own_change_finishing);

    // Local failure of the signature: its table is gone, the range reads fine, the store fails.
    // Only a log line per round; the status still says the removal is finishing.
    three
        .vault
        .a
        .lock()
        .execute_batch("DROP TABLE sync_range_notes")
        .expect("drop");
    for _ in 0..3 {
        let report = round_of(&three.vault, &three.vault.a).await;
        runtime.after_round(&report);
        assert_eq!(report.problem, Some(SyncProblem::KeyChangeWaiting));
    }
    let a = shown(&three, A, &runtime, NOW);
    assert_eq!(a.problem, Some(SyncProblem::KeyChangeWaiting));
    assert!(a.own_change_finishing);
    assert!(head::own_unverified(&three.vault.a.lock()).expect("own"));
}

#[tokio::test]
async fn a_server_head_below_the_stored_base_shows_the_finishing_status_and_names_nobody() {
    let three = unsigned_remover().await;
    {
        // The server's head for A's change is below A's place: `verify_own` waits for ever.
        let a = three.vault.a.lock();
        let own = head::unverified_epochs(&a)
            .expect("heads")
            .into_iter()
            .find(|e| e.own_pending)
            .expect("own head");
        head::note_own_unverified(
            &a,
            own.epoch,
            own.after.saturating_sub(1),
            own.rotation.expect("rotation"),
            state::load(&a).expect("state").expect("row").device,
            own.after,
        )
        .expect("note");
    }
    let mut runtime = Runtime::default();
    for _ in 0..3 {
        let report = round_of(&three.vault, &three.vault.a).await;
        runtime.after_round(&report);
        assert_eq!(report.problem, Some(SyncProblem::KeyChangeWaiting));
    }

    let two_days = NOW + 2 * 86_400;
    let a = shown(&three, A, &runtime, two_days);

    assert_eq!(a.problem, Some(SyncProblem::KeyChangeWaiting));
    assert!(a.own_change_finishing);
    assert_eq!(
        a.waiting_for_remover, None,
        "this device is not waiting for itself"
    );
    // The other device does wait for the remover after a day, and does not finish anything.
    let runtime = Runtime {
        problem: Some(SyncProblem::KeyChangeWaiting),
        ..Runtime::default()
    };
    let b = shown(&three, B, &runtime, two_days);
    assert!(!b.own_change_finishing);
    assert!(b.waiting_for_remover.is_some(), "{b:?}");
}
