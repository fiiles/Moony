//! Audit M10: a binding that cannot be checked fails closed. The engine shows its own problem and
//! runs no round; a start counts once however often it is retried, and the third failed start in
//! a row re-joins.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use zeroize::Zeroizing;

use super::rekey_fixture::{round_of, sync_on, three};
use super::runtime::{calls_since, device_of};
use super::runtime_loop::{rounds, start, start_in, until, Sealing};
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::binding::{self, BindingFailure, BindingNative};
use crate::services::sync::engine::pull::rejoin_as_new_device;
use crate::services::sync::engine::runtime::Trigger;
use crate::services::sync::identity::trust::{self, TrustState};
use crate::services::sync::identity::{self, CertKind};
use crate::services::sync::setup;

/// `Sealing`, but the first `failures` opens fail as a passing native error would.
struct FlakyOpening {
    failures: AtomicUsize,
}

impl BindingNative for FlakyOpening {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
        Sealing.seal(secret)
    }
    fn open(&self, blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
        let failed = self
            .failures
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if failed {
            return Err(BindingFailure::Failed("the secure storage is busy".into()));
        }
        Sealing.open(blob)
    }
}

/// Audit M10: a native `Failed` at the binding check fails closed. The first start makes no relay
/// request and shows a problem; the next trigger starts again and runs a round.
#[tokio::test]
async fn a_momentary_binding_failure_runs_no_round_and_retries() {
    let vault = super::vault().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let native = FlakyOpening {
        failures: AtomicUsize::new(1),
    };
    binding::write_binding(&vault.a.lock(), dir.path(), &native).expect("binding");
    let from = vault.relay.calls().len();

    let running = start(&vault, &vault.a, dir, Box::new(native));
    until("the failed start showed", || {
        running.handle.runtime().problem == Some(SyncProblem::SecureStorage)
    })
    .await;
    assert!(
        calls_since(&vault.relay, from).is_empty(),
        "no relay request at the failed start"
    );
    assert_eq!(running.handle.runtime().rounds, 0);

    running.handle.wake(Trigger::SyncNow);
    rounds(&running, 1).await;
    assert!(!calls_since(&vault.relay, from).is_empty(), "a round ran");
    assert_eq!(device_of(&vault.a.lock()), A, "no re-join");
    running.handle.stop();
}

/// `Sealing`, but every open fails as a locked secure storage would.
struct AlwaysFailing;

impl BindingNative for AlwaysFailing {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
        Sealing.seal(secret)
    }
    fn open(&self, _blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
        Err(BindingFailure::Failed(
            "the secure storage is locked".into(),
        ))
    }
}

fn failed_starts(conn: &Connection) -> Option<String> {
    conn.query_row(
        "SELECT value FROM app_config WHERE key = 'sync.binding_failed_starts'",
        [],
        |row| row.get(0),
    )
    .ok()
}

/// Retries inside one app start (every due trigger runs `start` again) stop the round each time
/// but count once.
#[tokio::test]
async fn several_failed_attempts_in_one_process_count_once() {
    let vault = super::vault().await;
    let dir = tempfile::tempdir().expect("temp dir");
    binding::write_binding(&vault.a.lock(), dir.path(), &Sealing).expect("binding");
    let from = vault.relay.calls().len();

    let running = start(&vault, &vault.a, dir, Box::new(AlwaysFailing));
    until("the problem showed", || {
        running.handle.runtime().problem == Some(SyncProblem::SecureStorage)
    })
    .await;
    for _ in 0..4 {
        running.handle.wake(Trigger::SyncNow);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    assert_eq!(failed_starts(&vault.a.lock()).as_deref(), Some("1"));
    assert_eq!(device_of(&vault.a.lock()), A, "no re-join");
    assert!(calls_since(&vault.relay, from).is_empty(), "no round ran");
    running.handle.stop();
}

/// Final review A, I2: the mark is the app start's, not the engine's. Every unlock and every sync
/// change spawns a new engine within the same app start; a failed check counts once among all of
/// them.
#[tokio::test]
async fn engines_spawned_in_one_app_start_count_once() {
    let vault = super::vault().await;
    let this_start = Arc::new(AtomicBool::new(false));

    for _ in 0..3 {
        let dir = tempfile::tempdir().expect("temp dir");
        binding::write_binding(&vault.a.lock(), dir.path(), &Sealing).expect("binding");
        let running = start_in(
            &vault,
            &vault.a,
            dir,
            Box::new(AlwaysFailing),
            this_start.clone(),
        );
        until("the problem showed", || {
            running.handle.runtime().problem == Some(SyncProblem::SecureStorage)
        })
        .await;
        running.handle.stop();
        until("the loop ended", || !running.handle.is_running()).await;
    }

    assert_eq!(failed_starts(&vault.a.lock()).as_deref(), Some("1"));
    assert_eq!(device_of(&vault.a.lock()), A, "no re-join");
}

/// Three separate app starts in a row that cannot check the binding re-join. Each start gets a
/// fresh app-start mark (`start` passes a new one): that is what a new process has, and it stands
/// in for resetting the process-wide mark between starts.
#[tokio::test]
async fn three_separate_failed_starts_rejoin() {
    let vault = super::vault().await;

    for count in ["1", "2"] {
        let dir = tempfile::tempdir().expect("temp dir");
        binding::write_binding(&vault.a.lock(), dir.path(), &Sealing).expect("binding");
        let running = start(&vault, &vault.a, dir, Box::new(AlwaysFailing));
        until("the problem showed", || {
            running.handle.runtime().problem == Some(SyncProblem::SecureStorage)
        })
        .await;
        running.handle.stop();
        until("the loop ended", || !running.handle.is_running()).await;
        assert_eq!(failed_starts(&vault.a.lock()).as_deref(), Some(count));
        assert_eq!(device_of(&vault.a.lock()), A, "no re-join yet");
    }

    let dir = tempfile::tempdir().expect("temp dir");
    binding::write_binding(&vault.a.lock(), dir.path(), &Sealing).expect("binding");
    let running = start(&vault, &vault.a, dir, Box::new(AlwaysFailing));
    rounds(&running, 1).await;

    assert!(device_of(&vault.a.lock()) > A, "the third start re-joined");
    assert_eq!(failed_starts(&vault.a.lock()), None, "the count reset");
    running.handle.stop();
}

/// Phase 5 §5.2–§5.3: a re-join gives the new ID a signing key of its own and a re-join
/// certificate signed by the old ID's key; the old ID stays trusted here.
#[tokio::test]
async fn a_rejoin_is_vouched_for_by_the_old_id() {
    let vault = super::vault().await;
    let old = with_genesis(&vault.a.lock(), "Mac");

    let conn = vault.a.lock();
    rejoin_as_new_device(&conn, None, NOW + 60).expect("re-join");

    let new = device_of(&conn);
    assert!(new > A);
    let entry = trust::entry(&conn, new).expect("entry").expect("pinned");
    assert_eq!(entry.kind, CertKind::Rejoin);
    assert_eq!(entry.issuer, A);
    assert_eq!(entry.cert.body.issuer_sign_pub, old.body.sign_pub);
    assert_ne!(entry.sign_pub, old.body.sign_pub, "a new signing key");
    let row_cert: Option<String> = conn
        .query_row(
            "SELECT cert FROM sync_devices WHERE id = ?1",
            [new.to_hex()],
            |r| r.get(0),
        )
        .expect("the new row");
    assert_eq!(row_cert, Some(entry.cert.to_b64()));
    assert_eq!(
        trust::entry(&conn, A).expect("entry").map(|e| e.state),
        Some(TrustState::Trusted),
        "the old ID stays trusted"
    );
    assert!(trust::is_supported(&conn).expect("supported"));
}

/// Phase 5 §5.5: a vault set up before phase 5 re-joins with a new signing key but no
/// certificate, so removal stays unavailable.
#[tokio::test]
async fn a_rejoin_of_a_vault_without_certificates_makes_a_key_and_no_certificate() {
    let vault = super::vault().await;
    let conn = vault.a.lock();
    rejoin_as_new_device(&conn, None, NOW).expect("re-join");

    assert!(identity::own_seed(&conn).expect("seed").is_some());
    assert!(trust::entries(&conn).expect("entries").is_empty());
    let row_cert: Option<String> = conn
        .query_row(
            "SELECT cert FROM sync_devices WHERE id = ?1",
            [device_of(&conn).to_hex()],
            |r| r.get(0),
        )
        .expect("the new row");
    assert_eq!(row_cert, None);
}

/// Phase 5 §5.5: at unlock, a profile whose sync was set up before phase 5 gets its signing key
/// and no certificate; removal stays unavailable and sync goes on.
#[tokio::test]
async fn at_unlock_a_vault_without_certificates_gets_a_signing_key_only() {
    let vault = super::vault().await;
    assert!(identity::own_seed(&vault.a.lock()).expect("seed").is_none());

    let running = start(
        &vault,
        &vault.a,
        tempfile::tempdir().expect("temp dir"),
        Box::new(Sealing),
    );
    rounds(&running, 1).await;
    running.handle.stop();

    let conn = vault.a.lock();
    assert!(identity::own_seed(&conn).expect("seed").is_some());
    assert!(trust::entries(&conn).expect("entries").is_empty());
    assert!(!trust::is_supported(&conn).expect("supported"));
}

/// `Sealing`, but every open says the binding was sealed on another machine.
struct OtherMachine;

impl BindingNative for OtherMachine {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
        Sealing.seal(secret)
    }
    fn open(&self, _blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
        Err(BindingFailure::NotThisDevice)
    }
}

/// Turning sync off asks the copy check before the device says it left (D10, first audit M10): a
/// profile whose binding answers `native` signs and sends nothing, and the original device keeps
/// syncing as before.
async fn nothing_is_said_when_the_check_answers(native: Arc<dyn BindingNative>) {
    let three = three().await;
    binding::write_binding(&three.vault.a.lock(), three.dir.path(), &Sealing).expect("binding");
    let ops = three.vault.relay.ops().len();

    setup::leave_best_effort(
        &three.vault.relay,
        &three.vault.a,
        three.dir.path(),
        native,
        setup::LEAVE_BUDGET,
    )
    .await;

    let left: Option<String> = three
        .vault
        .a
        .lock()
        .query_row(
            "SELECT left_record FROM sync_devices WHERE id = ?1",
            [A.to_hex()],
            |r| r.get(0),
        )
        .expect("the row");
    assert_eq!(left, None, "no left record was signed");
    assert_eq!(three.vault.relay.ops().len(), ops, "nothing was sent");
    assert_eq!(
        failed_starts(&three.vault.a.lock()),
        None,
        "the failed-starts count is not touched"
    );
    let report = round_of(&three.vault, &three.vault.b).await;
    assert_eq!(report.problem, None, "{report:?}");
    assert_eq!(
        trust::entry(&three.vault.b.lock(), A)
            .expect("entry")
            .map(|e| e.state),
        Some(TrustState::Trusted),
        "the others still see A as a member"
    );
    let report = round_of(&three.vault, &three.vault.a).await;
    assert_eq!(report.problem, None, "{report:?}");
    assert!(sync_on(&three.vault.a.lock()), "the original keeps syncing");
}

#[tokio::test]
async fn a_copy_whose_binding_cannot_be_checked_does_not_say_left() {
    nothing_is_said_when_the_check_answers(Arc::new(AlwaysFailing)).await;
}

#[tokio::test]
async fn a_copy_whose_binding_says_not_this_device_does_not_say_left() {
    nothing_is_said_when_the_check_answers(Arc::new(OtherMachine)).await;
}

/// The positive control of the two tests above: with a binding written and a check that opens it,
/// turning sync off signs and sends the `left` record (final review 1, M1).
#[tokio::test]
async fn a_device_whose_binding_checks_out_says_left() {
    let three = three().await;
    binding::write_binding(&three.vault.a.lock(), three.dir.path(), &Sealing).expect("binding");
    let ops = three.vault.relay.ops().len();

    setup::leave_best_effort(
        &three.vault.relay,
        &three.vault.a,
        three.dir.path(),
        Arc::new(Sealing),
        setup::LEAVE_BUDGET,
    )
    .await;

    let left: Option<String> = three
        .vault
        .a
        .lock()
        .query_row(
            "SELECT left_record FROM sync_devices WHERE id = ?1",
            [A.to_hex()],
            |r| r.get(0),
        )
        .expect("the row");
    assert!(left.is_some(), "the left record was signed");
    assert!(three.vault.relay.ops().len() > ops, "and sent");
    assert_eq!(failed_starts(&three.vault.a.lock()), None);
}
