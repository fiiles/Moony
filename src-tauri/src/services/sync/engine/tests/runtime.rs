//! The engine's rounds: pull before push, the snapshot step, the guards and the relay's refusals
//! as problems; the back-off and local-write trigger arithmetic; the status. The loop itself is
//! tested in `runtime_loop`.

use std::time::Duration;

use super::*;
use crate::models::sync::{SyncPhase, SyncProblem, SyncStatus};
use crate::services::sync::capture;
use crate::services::sync::changeset::from_json;
use crate::services::sync::engine::runtime::{Backoff, OutboxWatch};
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::{phase_of, problem_of, status, Runtime};
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::envelope::open_op;
use crate::services::sync::error::{SyncError, Transient};
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;
use crate::services::sync::test_support::assert_converged;

const DAY: i64 = 86_400;

async fn round_with(
    vault: &Vault,
    device: &TestConn,
    policy: &mut SnapshotPolicy,
    mode: RoundMode,
) -> RoundReport {
    run_round(&vault.relay, device, policy, mode, NOW, 0)
        .await
        .expect("round")
}

async fn round(vault: &Vault, device: &TestConn) -> RoundReport {
    round_with(
        vault,
        device,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
    )
    .await
}

pub(super) fn write_and_seal(vault: &Vault, device: &TestConn, id: &str) -> Vec<u8> {
    let conn = device.lock();
    institution(&conn, id);
    seal_from(&conn, &vault.creds)
}

/// The relay calls made after the first `from`.
pub(super) fn calls_since(relay: &FakeRelay, from: usize) -> Vec<Route> {
    relay.calls()[from..].to_vec()
}

pub(super) fn routes(relay: &FakeRelay, route: Route) -> usize {
    relay.calls().iter().filter(|r| **r == route).count()
}

fn outbox(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare("SELECT tbl || ' ' || pk FROM sync_outbox ORDER BY tbl, pk")
        .expect("outbox");
    stmt.query_map([], |r| r.get(0))
        .expect("outbox")
        .map(|r| r.expect("row"))
        .collect()
}

pub(super) fn device_of(conn: &Connection) -> DeviceId {
    state::load(conn).expect("state").expect("row").device
}

/// The changeset device of every op the relay holds from position `after` on.
pub(super) fn op_devices(vault: &Vault, after: u64) -> Vec<DeviceId> {
    vault
        .relay
        .ops()
        .iter()
        .filter(|op| op.seq > after)
        .map(|op| {
            let json = open_op(
                |epoch| vault.creds.keys.get(epoch),
                &vault.creds.vault_id.0,
                &op.ct,
            )
            .expect("open");
            from_json(&json).expect("changeset").device
        })
        .collect()
}

fn real_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs() as i64
}

// ---------------------------------------------------------------------------------------------
// The pure parts

#[test]
fn backoff_doubles_from_30_seconds_to_30_minutes_and_resets() {
    let mut backoff = Backoff::default();
    let waits: Vec<u64> = (0..8).map(|_| backoff.on_error().as_secs()).collect();
    assert_eq!(waits, [30, 60, 120, 240, 480, 960, 1_800, 1_800]);
    backoff.reset();
    assert_eq!(backoff.on_error(), Duration::from_secs(30));
}

#[test]
fn a_local_write_runs_a_round_after_one_quiet_poll_or_after_60_seconds() {
    let mut watch = OutboxWatch::default();
    assert!(!watch.observe(0, 0), "an empty outbox runs nothing");
    assert!(!watch.observe(3, 5), "a new write waits for one quiet poll");
    assert!(watch.observe(3, 10), "the next poll saw no change");
    // The round drained the outbox; nothing new since.
    watch.after_round(0);
    assert!(!watch.observe(0, 15));

    // Writes that never stop: the round runs 60 s after the first was seen.
    let fired: Vec<i64> = (1..=16)
        .map(|i| (i, 100 + 5 * (i - 1)))
        .filter(|&(i, now)| watch.observe(i as u64, now))
        .map(|(_, now)| now)
        .collect();
    assert_eq!(
        fired,
        [160],
        "forced after 60 s, then the count keeps changing"
    );

    // A round that could not push leaves rows queued: they run nothing until a new write.
    watch.after_round(16);
    assert!(!watch.observe(16, 200));
    assert!(!watch.observe(16, 205));
    assert!(!watch.observe(17, 210));
    assert!(watch.observe(17, 215));
}

#[test]
fn status_phases_follow_the_problem() {
    use SyncPhase::{Error, Paused};
    use SyncProblem as P;
    let table = [
        (P::CaptureBroken, Error),
        (P::VaultGone, Paused),
        (P::Lapsed, Paused),
        (P::QuotaExceeded, Paused),
        (P::SnapshotTooLarge, Error),
        (P::NewerSchema, Paused),
        (P::Gap, Error),
        (P::Undecryptable, Error),
        (P::InvalidChain, Error),
        (P::MissingParent, Error),
        (P::Quarantined, Error),
        (P::Offline, Paused),
        (P::RelayError, Error),
        (P::RateLimited, Paused),
        (P::Unauthorized, Error),
        (P::KeyRotated, Error),
        (P::Internal, Error),
    ];
    for (problem, phase) in table {
        assert_eq!(phase_of(problem), phase, "{problem:?}");
    }
}

#[test]
fn sync_errors_map_to_problems() {
    use crate::error::AppError;
    use crate::services::sync::error::TooLarge;
    use SyncProblem as P;
    let table: Vec<(SyncError, SyncProblem)> = vec![
        (SyncError::Transient(Transient::Offline), P::Offline),
        (SyncError::Transient(Transient::Relay), P::RelayError),
        (SyncError::Refused(RelayError::NoSnapshot), P::RelayError),
        (SyncError::SnapshotRequired, P::RelayError),
        (SyncError::RateLimited, P::RateLimited),
        (SyncError::VaultNotFound, P::VaultGone),
        (SyncError::Lapsed, P::Lapsed),
        (SyncError::Unauthorized, P::Unauthorized),
        (SyncError::ReadOnlyToken, P::KeyRotated),
        (SyncError::TooLarge(TooLarge::Quota), P::QuotaExceeded),
        (SyncError::TooLarge(TooLarge::Snapshot), P::SnapshotTooLarge),
        (SyncError::TooLarge(TooLarge::Body), P::RelayError),
        (
            SyncError::Local(AppError::Validation(
                "validation.syncSnapshotNewerSchema".into(),
            )),
            P::NewerSchema,
        ),
        (
            SyncError::Local(AppError::Database("disk I/O error".into())),
            P::Internal,
        ),
    ];
    for (error, problem) in table {
        assert_eq!(problem_of(&error), problem, "{error:?}");
    }
}

#[test]
fn the_status_of_a_profile_without_sync_is_off_and_with_an_account_setting_up() {
    let conn = crate::services::sync::test_support::migrated();
    let off = status(&conn, &Runtime::default()).expect("status");
    assert_eq!((off.phase, off.problem), (SyncPhase::Off, None));
    assert_eq!(off.pending_changes, 0);

    keys::save_account(&conn, "https://relay.test", "MOONY-ACCOUNT").expect("account");
    let setting_up = status(&conn, &Runtime::default()).expect("status");
    assert_eq!(setting_up.phase, SyncPhase::SettingUp);
}

#[tokio::test]
async fn the_status_counts_pending_changes_and_held_devices() {
    let vault = super::vault().await;
    {
        let a = vault.a.lock();
        institution(&a, "queued-1");
        let _sealed = seal_from(&a, &vault.creds);
        institution(&a, "queued-2");
        institution(&a, "queued-3");
        exec(
            &a,
            "INSERT INTO sync_heads (device, seq, hash, quarantined) \
             VALUES (x'03030303030303030303030303030303', 1, zeroblob(32), 1)",
        );
    }
    let runtime = Runtime {
        running_round: true,
        problem: None,
        paid_until: Some(NOW),
        refused_rows: 2,
        rounds: 0,
    };

    let status = status(&vault.a.lock(), &runtime).expect("status");

    assert_eq!(
        status,
        SyncStatus {
            phase: SyncPhase::Syncing,
            problem: None,
            last_success_at: None,
            pending_changes: 3,
            held_devices: 1,
            refused_rows: 2,
            paid_until: Some(NOW),
            deletes_at: None,
            dev_tools: crate::services::sync::feature::dev_tools(),
        }
    );
}

// ---------------------------------------------------------------------------------------------
// Rounds

#[tokio::test]
async fn two_devices_converge_through_rounds() {
    let vault = super::vault().await;
    let (mut policy_a, mut policy_b) = (SnapshotPolicy::default(), SnapshotPolicy::default());
    institution(&vault.a.lock(), "a1");
    exec(
        &vault.a.lock(),
        "INSERT INTO institutions (id, name) VALUES ('shared', 'From A')",
    );

    let first = round_with(&vault, &vault.a, &mut policy_a, RoundMode::Normal).await;
    institution(&vault.b.lock(), "b1");
    let second = round_with(&vault, &vault.b, &mut policy_b, RoundMode::Normal).await;
    exec(
        &vault.b.lock(),
        "UPDATE institutions SET name = 'From B' WHERE id = 'shared'",
    );
    let third = round_with(&vault, &vault.b, &mut policy_b, RoundMode::Normal).await;
    let fourth = round_with(&vault, &vault.a, &mut policy_a, RoundMode::Normal).await;

    for report in [&first, &second, &third, &fourth] {
        assert_eq!(report.problem, None, "{report:?}");
    }
    assert_eq!(first.push.as_ref().map(|p| p.pushed), Some(1));
    assert_eq!(second.pull.as_ref().map(|p| p.applied), Some(1));
    assert_eq!(fourth.pull.as_ref().map(|p| p.applied), Some(2));
    let (a, b) = (vault.a.lock(), vault.b.lock());
    assert_converged(&[&a, &b]);
    assert!(has_institution(&a, "b1") && has_institution(&b, "a1"));
    let name: String = a
        .query_row(
            "SELECT name FROM institutions WHERE id = 'shared'",
            [],
            |r| r.get(0),
        )
        .expect("shared");
    assert_eq!(name, "From B");
    assert!(state::marks(&a)
        .expect("marks")
        .expect("row")
        .last_success_at
        .is_some_and(|at| at == NOW));
}

/// A copy of `A` (`VACUUM INTO`) pushes under `A`'s ID. `A`'s next round re-joins in its pull,
/// before anything is pushed, and pushes only under its new ID.
#[tokio::test]
async fn a_rejoin_happens_before_any_push_in_the_round() {
    let vault = super::vault().await;
    institution(&vault.a.lock(), "a1");
    round(&vault, &vault.a).await;
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("copy.db");
    vault
        .a
        .lock()
        .execute("VACUUM INTO ?1", [path.to_str().expect("utf-8")])
        .expect("copy A");
    let copy = Connection::open(&path).expect("open the copy");
    exec(&copy, "PRAGMA foreign_keys = ON");
    capture::install(&copy, A).expect("capture on the copy");
    institution(&copy, "theirs");
    let copied = seal_from(&copy, &vault.creds);
    push(&vault.relay, &vault.creds, &[copied]).await;
    institution(&vault.a.lock(), "mine");
    let calls_before = vault.relay.calls().len();
    let head_before = vault.relay.ops().len() as u64;

    let report = round(&vault, &vault.a).await;

    assert!(report.pull.as_ref().is_some_and(|p| p.rejoined));
    let calls = vault.relay.calls()[calls_before..].to_vec();
    let first_push = calls
        .iter()
        .position(|r| *r == Route::Push)
        .expect("a push");
    assert!(
        calls[..first_push].contains(&Route::Pull),
        "the pull (and its re-join) comes before any push: {calls:?}"
    );
    let new_id = device_of(&vault.a.lock());
    assert!(new_id > A);
    let pushed = op_devices(&vault, head_before);
    assert!(!pushed.is_empty());
    assert!(
        pushed.iter().all(|device| *device == new_id),
        "every op pushed in the round is the new ID's"
    );
    let a = vault.a.lock();
    assert!(has_institution(&a, "theirs") && has_institution(&a, "mine"));
}

/// Ruling P1: a pull that stopped on an envelope that does not open or a broken chain link pushes
/// nothing in that round; the queued rows stay where they are. (A gap holds and pushes: `gaps`.)
#[tokio::test]
async fn a_stopped_pull_skips_the_push() {
    for (what, expected) in [
        ("undecryptable", SyncProblem::Undecryptable),
        ("invalid chain", SyncProblem::InvalidChain),
    ] {
        let vault = super::vault().await;
        match what {
            "undecryptable" => {
                let mut tampered = write_and_seal(&vault, &vault.b, "b1");
                *tampered.last_mut().expect("tag") ^= 1;
                push(&vault.relay, &vault.creds, &[tampered]).await;
            }
            _ => {
                let broken = forge(&vault.creds, &changeset(C, 2, None, vec![]));
                push(&vault.relay, &vault.creds, &[broken]).await;
            }
        }
        institution(&vault.a.lock(), "queued");
        let queued = outbox(&vault.a.lock());
        assert!(!queued.is_empty());
        let from = vault.relay.calls().len();

        let report = round(&vault, &vault.a).await;

        assert!(report.push.is_none(), "{what}");
        assert!(
            !calls_since(&vault.relay, from).contains(&Route::Push),
            "{what}"
        );
        assert_eq!(outbox(&vault.a.lock()), queued, "{what}");
        assert_eq!(
            count(&vault.a.lock(), "SELECT COUNT(*) FROM sync_unsent"),
            0,
            "{what}"
        );
        assert_eq!(report.problem, Some(expected), "{what}");
        assert!(!report.snapshot_uploaded, "{what}");
    }
}

#[tokio::test]
async fn newer_schema_and_held_envelopes_still_push() {
    for (what, expected) in [
        ("newer schema", SyncProblem::NewerSchema),
        ("held", SyncProblem::Quarantined),
    ] {
        let vault = super::vault().await;
        if what == "newer schema" {
            let mut newer = changeset(C, 1, None, vec![]);
            newer.schema = schema_number() + 1;
            push(&vault.relay, &vault.creds, &[forge(&vault.creds, &newer)]).await;
        } else {
            let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
            let c2 = forge(&vault.creds, &changeset(C, 2, Some([9; 32]), vec![]));
            push(&vault.relay, &vault.creds, &[c1, c2]).await;
        }
        institution(&vault.a.lock(), "queued");
        let head_before = vault.relay.ops().len() as u64;

        let report = round(&vault, &vault.a).await;

        assert_eq!(report.push.as_ref().map(|p| p.pushed), Some(1), "{what}");
        assert_eq!(op_devices(&vault, head_before), [A], "{what}");
        assert!(outbox(&vault.a.lock()).is_empty(), "{what}");
        assert_eq!(report.problem, Some(expected), "{what}");
        assert!(
            !report.snapshot_uploaded,
            "a snapshot needs a clean pull: {what}"
        );
        assert_eq!(vault.relay.snapshot_seq(), None, "{what}");
    }
}

/// Rulings P4, P14: the snapshot's sequence is the relay's head after the pull that follows the
/// push, and a held envelope or a gap rules the upload out.
#[tokio::test]
async fn a_snapshot_is_dumped_only_after_a_clean_pull() {
    let vault = super::vault().await;
    let _first = write_and_seal(&vault, &vault.a, "a1");
    let _second = write_and_seal(&vault, &vault.a, "a2");
    institution(&vault.a.lock(), "a3");
    let mut policy = SnapshotPolicy::default();

    let report = round_with(&vault, &vault.a, &mut policy, RoundMode::FirstSnapshot).await;

    assert_eq!(report.push.as_ref().map(|p| p.pushed), Some(3));
    assert!(report.snapshot_uploaded);
    assert_eq!(vault.relay.ops().len(), 3);
    assert_eq!(vault.relay.snapshot_seq(), Some(3), "S is the relay's head");
    assert_eq!(cursor(&vault.a.lock()), 3);
    assert_eq!(report.vault.as_ref().map(|v| v.head), Some(3));

    // A held envelope: no upload.
    let held = super::vault().await;
    let c1 = forge(&held.creds, &changeset(C, 1, None, vec![]));
    let c2 = forge(&held.creds, &changeset(C, 2, Some([9; 32]), vec![]));
    push(&held.relay, &held.creds, &[c1, c2]).await;
    institution(&held.a.lock(), "a1");
    let report = round_with(
        &held,
        &held.a,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
    )
    .await;
    assert!(report.push.is_some() && !report.snapshot_uploaded);
    assert_eq!(routes(&held.relay, Route::PutSnapshot), 0);

    // A gap: a push, but no upload.
    let gap = super::vault().await;
    let sealed: Vec<Vec<u8>> = ["b1", "b2"]
        .iter()
        .map(|id| write_and_seal(&gap, &gap.b, id))
        .collect();
    push(&gap.relay, &gap.creds, &sealed).await;
    gap.relay.drop_op(1);
    let report = round_with(
        &gap,
        &gap.a,
        &mut SnapshotPolicy::default(),
        RoundMode::FirstSnapshot,
    )
    .await;
    assert_eq!(report.problem, Some(SyncProblem::Gap));
    assert_eq!(routes(&gap.relay, Route::PutSnapshot), 0);
}

#[tokio::test]
async fn a_capture_broken_profile_never_syncs() {
    let vault = super::vault().await;
    {
        let a = vault.a.lock();
        state::mark_capture_broken(&a, NOW).expect("broken");
        institution(&a, "queued");
    }
    let from = vault.relay.calls().len();

    let report = round(&vault, &vault.a).await;

    assert_eq!(report.problem, Some(SyncProblem::CaptureBroken));
    assert!(report.pull.is_none() && report.push.is_none());
    assert!(
        calls_since(&vault.relay, from).is_empty(),
        "no relay call at all"
    );
    let mut runtime = Runtime::default();
    runtime.after_round(&report);
    let shown = status(&vault.a.lock(), &runtime).expect("status");
    assert_eq!(
        (shown.phase, shown.problem),
        (SyncPhase::Error, Some(SyncProblem::CaptureBroken))
    );

    // The repair's drain round (Task 15) still pulls and pushes, but takes no snapshot.
    let drain = round_with(
        &vault,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::Drain,
    )
    .await;
    assert_eq!(drain.push.as_ref().map(|p| p.pushed), Some(1));
    assert_eq!(routes(&vault.relay, Route::VaultInfo), 0);
    assert_eq!(routes(&vault.relay, Route::PutSnapshot), 0);
}

/// Spec §11.6: a lapsed subscription keeps pulling (the relay's read grace), the push waits, and
/// the status shows the date the relay deletes the vault.
#[tokio::test]
async fn lapsed_subscription_keeps_pulling_and_reports_the_deletion_date() {
    let vault = super::vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    push(&vault.relay, &vault.creds, &[b1]).await;
    let paid_until = real_now() - DAY;
    vault.relay.set_paid_until(&vault.creds.account, paid_until);
    institution(&vault.a.lock(), "queued");

    let report = run_round(
        &vault.relay,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("a lapse is a problem, not an error");

    assert_eq!(report.problem, Some(SyncProblem::Lapsed));
    assert_eq!(report.pull.as_ref().map(|p| p.applied), Some(1));
    assert_eq!(
        report.vault.as_ref().map(|v| v.paid_until),
        Some(paid_until)
    );
    let a = vault.a.lock();
    assert!(has_institution(&a, "b1"), "pulls go on");
    assert_eq!(
        count(&a, "SELECT COUNT(*) FROM sync_unsent"),
        1,
        "the push waits"
    );
    let mut runtime = Runtime::default();
    runtime.after_round(&report);
    let shown = status(&a, &runtime).expect("status");
    assert_eq!(shown.phase, SyncPhase::Paused);
    assert_eq!(shown.paid_until, Some(paid_until));
    assert_eq!(shown.deletes_at, Some(paid_until + 90 * DAY));
    assert_eq!(shown.last_success_at, None);
}

#[tokio::test]
async fn relay_refusals_become_problems_without_a_back_off() {
    for (route, error, expected) in [
        (
            Route::Push,
            RelayError::TokenReadOnly,
            SyncProblem::KeyRotated,
        ),
        (
            Route::Pull,
            RelayError::Unauthorized,
            SyncProblem::Unauthorized,
        ),
        (
            Route::Push,
            RelayError::QuotaExceeded,
            SyncProblem::QuotaExceeded,
        ),
    ] {
        let vault = super::vault().await;
        institution(&vault.a.lock(), "queued");
        vault.relay.fail_next(route, error);

        let report = round(&vault, &vault.a).await;

        assert_eq!(report.problem, Some(expected));
        assert!(
            state::load(&vault.a.lock())
                .expect("state")
                .expect("row")
                .enabled,
            "nothing is turned off or deleted"
        );
    }
}

/// Spec §9.4, obligation (f): a transient failure ends the round at once (no retry inside it, so a
/// round lasts at most one ops timeout per call it reached); the loop backs off.
#[tokio::test]
async fn a_transient_error_ends_the_round_without_retrying() {
    let vault = super::vault().await;
    vault
        .relay
        .fail_next(Route::Pull, RelayError::Network("timeout".into()));
    let from = vault.relay.calls().len();

    let error = run_round(
        &vault.relay,
        &vault.a,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect_err("transient");

    assert!(matches!(error, SyncError::Transient(Transient::Offline)));
    assert!(error.backs_off());
    assert_eq!(calls_since(&vault.relay, from), [Route::Pull]);
}

#[tokio::test]
async fn a_vault_gone_during_the_pull_pauses_too() {
    let vault = super::vault().await;
    vault.relay.wipe_vault(&vault.creds.vault_id.to_b64());
    let from = vault.relay.calls().len();

    let report = round(&vault, &vault.a).await;

    assert_eq!(report.problem, Some(SyncProblem::VaultGone));
    assert_eq!(calls_since(&vault.relay, from), [Route::Pull]);
    let marks = state::marks(&vault.a.lock()).expect("marks").expect("row");
    assert_eq!(marks.vault_gone_at, Some(NOW));
    assert!(keys::credentials(&vault.a.lock()).expect("keys").is_some());

    let again = round(&vault, &vault.a).await;
    assert_eq!(again.problem, Some(SyncProblem::VaultGone));
    assert_eq!(
        calls_since(&vault.relay, from),
        [Route::Pull],
        "a paused vault is not asked"
    );
}

/// Fix round 1 (2): a guarded (or read-only) profile counts as capture-broken even when its mark
/// could not be written: no round runs against it, and the status offers the repair.
#[tokio::test]
async fn a_guarded_profile_never_syncs_even_without_its_mark() {
    for read_only in [false, true] {
        let vault = super::vault().await;
        {
            let a = vault.a.lock();
            institution(&a, "queued");
            capture::uninstall(&a).expect("uninstall");
            if read_only {
                exec(&a, "PRAGMA query_only = 1");
            } else {
                capture::guard::impose(&a).expect("guard");
            }
            let marks = state::marks(&a).expect("marks").expect("row");
            assert_eq!(marks.capture_broken_at, None, "no mark");
        }
        let from = vault.relay.calls().len();

        let report = round(&vault, &vault.a).await;

        assert_eq!(
            report.problem,
            Some(SyncProblem::CaptureBroken),
            "{read_only}"
        );
        assert!(calls_since(&vault.relay, from).is_empty(), "{read_only}");
        let mut runtime = Runtime::default();
        runtime.after_round(&report);
        let shown = status(&vault.a.lock(), &runtime).expect("status");
        assert_eq!(
            (shown.phase, shown.problem),
            (SyncPhase::Error, Some(SyncProblem::CaptureBroken)),
            "{read_only}"
        );
    }
}
