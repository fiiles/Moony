//! Audit M10: a binding that cannot be checked fails closed. The engine shows its own problem and
//! runs no round; a start counts once however often it is retried, and the third failed start in
//! a row re-joins.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use zeroize::Zeroizing;

use super::runtime::{calls_since, device_of};
use super::runtime_loop::{rounds, start, start_in, until, Sealing};
use super::*;
use crate::models::sync::SyncProblem;
use crate::services::sync::binding::{self, BindingFailure, BindingNative};
use crate::services::sync::engine::runtime::Trigger;

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
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
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
