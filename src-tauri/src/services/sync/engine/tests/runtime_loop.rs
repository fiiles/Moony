//! The engine's loop and the round lock: rounds never overlap or hold the profile over the
//! network, the loop's triggers, back-off, re-join at start, pauses and events.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use zeroize::Zeroizing;

use super::runtime::{calls_since, device_of, op_devices, routes, write_and_seal};
use super::*;
use crate::models::sync::{SyncPhase, SyncProblem, SyncStatus};
use crate::services::sync::binding::{self, BindingFailure, BindingNative, NoBinding};
use crate::services::sync::blobs::policy::BlobPolicy;
use crate::services::sync::engine::runtime::{self, EngineHandle, Trigger};
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, EngineEvents, RoundMode};
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{
    AccountStatus, CheckoutPlan, PullPage, PushReceipt, RelayError, RelayResult, SnapshotBlob,
    VaultInfo,
};

// ---------------------------------------------------------------------------------------------
// Serialized rounds, and the lock never held across a relay call (obligations a, h)

/// `FakeRelay` with a delay in every call, counting calls in flight and calls made while the
/// profile's connection was locked.
struct Probe {
    inner: FakeRelay,
    conn: Arc<Mutex<Connection>>,
    in_flight: AtomicUsize,
    most_in_flight: AtomicUsize,
    calls: AtomicUsize,
    locked_calls: AtomicUsize,
    /// Calls that never answer, from the next one on.
    stalls: AtomicUsize,
}

impl Probe {
    fn new(inner: FakeRelay, device: &TestConn) -> Self {
        Self {
            inner,
            conn: device.conn.clone(),
            in_flight: AtomicUsize::new(0),
            most_in_flight: AtomicUsize::new(0),
            calls: AtomicUsize::new(0),
            locked_calls: AtomicUsize::new(0),
            stalls: AtomicUsize::new(0),
        }
    }

    async fn around<T>(&self, call: impl std::future::Future<Output = T>) -> T {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.conn.try_lock().is_err() {
            self.locked_calls.fetch_add(1, Ordering::SeqCst);
        }
        let stalled = self
            .stalls
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if stalled {
            std::future::pending::<()>().await;
        }
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_in_flight.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(5)).await;
        let result = call.await;
        self.in_flight.fetch_sub(1, Ordering::SeqCst);
        result
    }
}

impl Relay for Probe {
    async fn server_info(&self) -> RelayResult<crate::services::sync::relay::ServerInfo> {
        self.around(self.inner.server_info()).await
    }
    async fn create_account(&self) -> RelayResult<String> {
        self.around(self.inner.create_account()).await
    }
    async fn account_status(&self, account: &str) -> RelayResult<AccountStatus> {
        self.around(self.inner.account_status(account)).await
    }
    async fn checkout(&self, account: &str, plan: CheckoutPlan) -> RelayResult<String> {
        self.around(self.inner.checkout(account, plan)).await
    }
    async fn create_vault(&self, account: &str, vault: &VaultAuth) -> RelayResult<bool> {
        self.around(self.inner.create_vault(account, vault)).await
    }
    async fn vault_info(&self, vault: &VaultAuth) -> RelayResult<VaultInfo> {
        self.around(self.inner.vault_info(vault)).await
    }
    async fn push(&self, vault: &VaultAuth, ops: &[OutgoingOp]) -> RelayResult<PushReceipt> {
        self.around(self.inner.push(vault, ops)).await
    }
    async fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> RelayResult<PullPage> {
        self.around(self.inner.pull(vault, after, limit)).await
    }
    async fn put_snapshot(&self, vault: &VaultAuth, seq: u64, bytes: Vec<u8>) -> RelayResult<()> {
        self.around(self.inner.put_snapshot(vault, seq, bytes))
            .await
    }
    async fn get_snapshot(&self, vault: &VaultAuth) -> RelayResult<SnapshotBlob> {
        self.around(self.inner.get_snapshot(vault)).await
    }
    async fn delete_vault(&self, vault: &VaultAuth) -> RelayResult<()> {
        self.around(self.inner.delete_vault(vault)).await
    }
    async fn rotate_token(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        self.around(self.inner.rotate_token(vault, new_token, bundle))
            .await
    }
    async fn rotate_token_with_kit(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
        kit: crate::services::sync::relay::KitRotation<'_>,
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        self.around(
            self.inner
                .rotate_token_with_kit(vault, new_token, bundle, kit),
        )
        .await
    }
    async fn put_kit(
        &self,
        vault: &VaultAuth,
        kit_token: &str,
        envelope: &[u8],
    ) -> RelayResult<bool> {
        self.around(self.inner.put_kit(vault, kit_token, envelope))
            .await
    }
    async fn get_kit(
        &self,
        vault_id: &str,
        kit_token: &str,
    ) -> RelayResult<crate::services::sync::relay::KitRead> {
        self.around(self.inner.get_kit(vault_id, kit_token)).await
    }
    async fn rekeys(
        &self,
        vault: &VaultAuth,
        after: u32,
        limit: u32,
    ) -> RelayResult<crate::services::sync::relay::RekeyPage> {
        self.around(self.inner.rekeys(vault, after, limit)).await
    }
    async fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sealed_len: u64,
        body: crate::services::sync::relay::BlobBody,
    ) -> RelayResult<()> {
        self.around(self.inner.put_blob(vault, blob, sealed_len, body))
            .await
    }
    async fn head_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<Option<u64>> {
        self.around(self.inner.head_blob(vault, blob)).await
    }
    async fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sink: &mut (dyn crate::services::sync::relay::BlobSink + Send),
    ) -> RelayResult<crate::services::sync::relay::Delivery> {
        self.around(self.inner.get_blob(vault, blob, sink)).await
    }
    async fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<uuid::Uuid>,
        limit: u32,
    ) -> RelayResult<crate::services::sync::relay::BlobPage> {
        self.around(self.inner.list_blobs(vault, after, limit))
            .await
    }
    async fn delete_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<()> {
        self.around(self.inner.delete_blob(vault, blob)).await
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_rounds_of_a_profile_never_overlap_and_never_hold_the_lock_over_the_network() {
    let vault = super::vault().await;
    let b1 = write_and_seal(&vault, &vault.b, "b1");
    push(&vault.relay, &vault.creds, &[b1]).await;
    institution(&vault.a.lock(), "queued");
    let probe = Arc::new(Probe::new(vault.relay.clone(), &vault.a));

    let rounds: Vec<_> = (0..3)
        .map(|_| {
            let (probe, device) = (probe.clone(), vault.a.clone());
            tokio::spawn(async move {
                let mut policy = SnapshotPolicy::default();
                run_round(
                    probe.as_ref(),
                    &device,
                    &mut policy,
                    RoundMode::FirstSnapshot,
                    NOW,
                    0,
                )
                .await
                .map(|report| report.problem)
            })
        })
        .collect();
    for handle in rounds {
        assert_eq!(handle.await.expect("task").expect("round"), None);
    }

    assert!(
        probe.calls.load(Ordering::SeqCst) >= 9,
        "three full rounds ran"
    );
    assert_eq!(
        probe.most_in_flight.load(Ordering::SeqCst),
        1,
        "one round at a time: never two relay calls (or two pushes) at once"
    );
    assert_eq!(
        probe.locked_calls.load(Ordering::SeqCst),
        0,
        "no relay call while the profile's connection is held"
    );
    assert_eq!(vault.relay.ops().len(), 2, "the queued row was pushed once");
}

// ---------------------------------------------------------------------------------------------
// The loop

/// A binding that "seals" by copying (a machine-bound seal in tests).
pub(super) struct Sealing;

impl BindingNative for Sealing {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
        Ok(secret.to_vec())
    }
    fn open(&self, blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
        let value: [u8; 32] = blob
            .try_into()
            .map_err(|_| BindingFailure::Failed("size".into()))?;
        Ok(Zeroizing::new(value))
    }
}

/// What the loop told the app.
#[derive(Clone, Default)]
pub(super) struct Seen {
    data_changed: Arc<AtomicUsize>,
    attachments_changed: Arc<AtomicUsize>,
    statuses: Arc<Mutex<Vec<SyncStatus>>>,
}

impl Seen {
    pub(super) fn attachments_changed(&self) -> usize {
        self.attachments_changed.load(Ordering::SeqCst)
    }
    pub(super) fn statuses(&self) -> Vec<SyncStatus> {
        self.statuses.lock().expect("statuses").clone()
    }
    pub(super) fn last(&self) -> SyncStatus {
        self.statuses().last().cloned().expect("a status")
    }
    fn data_changed(&self) -> usize {
        self.data_changed.load(Ordering::SeqCst)
    }
}

struct Events {
    seen: Seen,
    dir: PathBuf,
    native: Box<dyn BindingNative>,
    /// The app start's binding mark (`EngineEvents::binding_failed_this_start`): a fresh one is a
    /// new app start, a shared one engines spawned within one.
    this_start: Arc<AtomicBool>,
}

impl EngineEvents for Events {
    fn data_changed(&self) {
        self.seen.data_changed.fetch_add(1, Ordering::SeqCst);
    }
    fn attachments_changed(&self) {
        self.seen.attachments_changed.fetch_add(1, Ordering::SeqCst);
    }
    fn status_changed(&self, status: &SyncStatus) {
        self.seen
            .statuses
            .lock()
            .expect("statuses")
            .push(status.clone());
    }
    fn binding(&self) -> &dyn BindingNative {
        self.native.as_ref()
    }
    fn data_dir(&self) -> PathBuf {
        self.dir.clone()
    }
    fn binding_failed_this_start(&self) -> &AtomicBool {
        &self.this_start
    }
}

pub(super) struct Running {
    pub(super) handle: EngineHandle,
    pub(super) seen: Seen,
    _dir: tempfile::TempDir,
}

/// The engine in an app start of its own (a fresh binding mark).
pub(super) fn start(
    vault: &Vault,
    device: &TestConn,
    dir: tempfile::TempDir,
    native: Box<dyn BindingNative>,
) -> Running {
    start_in(vault, device, dir, native, Arc::default())
}

/// The engine spawned within the app start whose binding mark is `this_start` (an unlock after a
/// lock, or a sync change, spawns another engine in the same start).
pub(super) fn start_in(
    vault: &Vault,
    device: &TestConn,
    dir: tempfile::TempDir,
    native: Box<dyn BindingNative>,
    this_start: Arc<AtomicBool>,
) -> Running {
    let seen = Seen::default();
    let events = Events {
        seen: seen.clone(),
        dir: dir.path().to_path_buf(),
        native,
        this_start,
    };
    let handle = runtime::spawn(vault.relay.clone(), device.clone(), events);
    Running {
        handle,
        seen,
        _dir: dir,
    }
}

/// The engine on any relay and connection, without a binding.
fn start_on<R: Relay + 'static, D: ConnAccess>(relay: R, db: D) -> Running {
    start_with(relay, db, BlobPolicy::for_this_platform())
}

/// The engine on any relay and connection, without a binding, with the transfer policy `blobs`
/// (a phone's or a desktop's).
pub(super) fn start_with<R: Relay + 'static, D: ConnAccess>(
    relay: R,
    db: D,
    blobs: BlobPolicy,
) -> Running {
    let dir = tempfile::tempdir().expect("temp dir");
    let seen = Seen::default();
    let events = Events {
        seen: seen.clone(),
        dir: dir.path().to_path_buf(),
        native: Box::new(NoBinding),
        this_start: Arc::default(),
    };
    let handle = runtime::spawn_with(relay, db, events, blobs);
    Running {
        handle,
        seen,
        _dir: dir,
    }
}

/// `TestConn`, counting every time the engine takes the connection.
#[derive(Clone)]
struct Counted {
    inner: TestConn,
    takes: Arc<AtomicUsize>,
}

impl Counted {
    fn new(inner: TestConn) -> Self {
        Self {
            inner,
            takes: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn takes(&self) -> usize {
        self.takes.load(Ordering::SeqCst)
    }
}

impl ConnAccess for Counted {
    fn with_conn_mut<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        self.takes.fetch_add(1, Ordering::SeqCst);
        self.inner.with_conn_mut(f)
    }
    fn try_with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Option<Result<T>> {
        self.inner.try_with_conn(f)
    }
    fn round_lock(&self) -> &tokio::sync::Mutex<()> {
        self.inner.round_lock()
    }
    fn lanes_lock(&self) -> &tokio::sync::Mutex<()> {
        self.inner.lanes_lock()
    }
    fn incoming_blobs(&self) -> &crate::services::sync::blobs::stream::IncomingBlobs {
        self.inner.incoming_blobs()
    }
}

pub(super) fn start_plain(vault: &Vault, device: &TestConn) -> Running {
    start(
        vault,
        device,
        tempfile::tempdir().expect("temp dir"),
        Box::new(NoBinding),
    )
}

pub(super) async fn until(what: &str, done: impl Fn() -> bool) {
    for _ in 0..6_000 {
        if done() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    panic!("timed out waiting until {what}");
}

pub(super) async fn rounds(running: &Running, n: u64) {
    until(&format!("{n} rounds ran"), || {
        running.handle.runtime().rounds >= n
    })
    .await;
}

#[tokio::test]
async fn data_changed_fires_only_when_something_was_applied() {
    let vault = super::vault().await;
    let running = start_plain(&vault, &vault.a);
    rounds(&running, 1).await;
    assert_eq!(running.seen.data_changed(), 0, "nothing was applied");

    let b1 = write_and_seal(&vault, &vault.b, "b1");
    push(&vault.relay, &vault.creds, &[b1]).await;
    running.handle.wake(Trigger::SyncNow);
    rounds(&running, 2).await;
    assert_eq!(running.seen.data_changed(), 1);
    assert!(has_institution(&vault.a.lock(), "b1"));

    running.handle.wake(Trigger::SyncNow);
    rounds(&running, 3).await;
    assert_eq!(running.seen.data_changed(), 1);
    let last = running.seen.last();
    assert_eq!((last.phase, last.problem), (SyncPhase::Idle, None));
    assert!(running
        .seen
        .statuses()
        .iter()
        .any(|s| s.phase == SyncPhase::Syncing));
    running.handle.stop();
    until("the loop ended", || !running.handle.is_running()).await;
}

#[tokio::test]
async fn a_vault_gone_answer_pauses_until_sync_now_finds_it_again() {
    let vault = super::vault().await;
    vault.relay.wipe_vault(&vault.creds.vault_id.to_b64());
    let running = start_plain(&vault, &vault.a);
    rounds(&running, 1).await;
    let paused = running.seen.last();
    assert_eq!(
        (paused.phase, paused.problem),
        (SyncPhase::Paused, Some(SyncProblem::VaultGone))
    );
    let calls = vault.relay.calls().len();

    running.handle.wake(Trigger::Periodic);
    rounds(&running, 2).await;
    assert_eq!(
        vault.relay.calls().len(),
        calls,
        "a paused vault is not asked"
    );

    running.handle.wake(Trigger::SyncNow);
    rounds(&running, 3).await;
    assert_eq!(
        vault.relay.calls()[calls..].to_vec(),
        [Route::VaultInfo],
        "Sync now asks once"
    );
    assert_eq!(
        running.handle.runtime().problem,
        Some(SyncProblem::VaultGone)
    );
    assert!(
        keys::credentials(&vault.a.lock()).expect("keys").is_some(),
        "no key is deleted"
    );

    // The vault is back (the relay answered 404 falsely, or the outage ended).
    assert!(vault
        .relay
        .create_vault(&vault.creds.account, &vault.creds.auth())
        .await
        .expect("vault again"));
    institution(&vault.a.lock(), "a1");
    running.handle.wake(Trigger::SyncNow);
    rounds(&running, 4).await;

    let marks = state::marks(&vault.a.lock()).expect("marks").expect("row");
    assert_eq!(marks.vault_gone_at, None);
    assert_eq!(vault.relay.ops().len(), 1, "syncing again");
    let last = running.seen.last();
    assert_eq!((last.phase, last.problem), (SyncPhase::Idle, None));
    running.handle.stop();
}

#[tokio::test]
async fn a_transient_error_backs_off_until_sync_now() {
    let vault = super::vault().await;
    vault
        .relay
        .fail_next(Route::Pull, RelayError::Network("timeout".into()));
    let running = start_plain(&vault, &vault.a);
    rounds(&running, 1).await;
    assert_eq!(running.handle.runtime().problem, Some(SyncProblem::Offline));
    assert_eq!(running.seen.last().phase, SyncPhase::Paused);

    running.handle.wake(Trigger::Periodic);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        running.handle.runtime().rounds,
        1,
        "the back-off holds a periodic round"
    );

    running.handle.wake(Trigger::SyncNow);
    rounds(&running, 2).await;
    assert_eq!(running.handle.runtime().problem, None);
    running.handle.stop();
}

/// Final review A, I3: a round whose pull held a quarantined author and whose push then failed on
/// the relay shows the quarantine, not a relay error; it still backs off.
#[tokio::test]
async fn a_failed_push_does_not_hide_a_quarantine() {
    let vault = super::vault().await;
    let c1 = forge(&vault.creds, &changeset(C, 1, None, vec![]));
    let c2 = forge(&vault.creds, &changeset(C, 2, Some([9; 32]), vec![]));
    push(&vault.relay, &vault.creds, &[c1, c2]).await;
    institution(&vault.a.lock(), "a1");
    vault.relay.fail_next(Route::Push, RelayError::Server(503));

    let running = start_plain(&vault, &vault.a);
    rounds(&running, 1).await;

    assert_eq!(
        running.handle.runtime().problem,
        Some(SyncProblem::Quarantined)
    );
    let last = running.seen.last();
    assert_eq!(
        (last.phase, last.problem),
        (SyncPhase::Error, Some(SyncProblem::Quarantined))
    );
    let last_error: Option<String> = vault
        .a
        .lock()
        .query_row("SELECT last_error FROM sync_state", [], |r| r.get(0))
        .expect("last_error");
    assert_eq!(last_error.as_deref(), Some("sync stopped: quarantined"));
    running.handle.stop();
}

#[tokio::test]
async fn a_restore_marker_rejoins_before_the_first_round() {
    let vault = super::vault().await;
    let dir = tempfile::tempdir().expect("temp dir");
    binding::write_rejoin_marker(dir.path()).expect("marker");
    let marker = dir.path().join(binding::REJOIN_MARKER);
    let bound = dir.path().join(binding::BINDING_FILE);
    institution(&vault.a.lock(), "restored");
    let from = vault.relay.calls().len();

    let running = start(&vault, &vault.a, dir, Box::new(Sealing));
    rounds(&running, 1).await;

    let new_id = device_of(&vault.a.lock());
    assert!(new_id > A, "re-joined under a new ID");
    assert!(!marker.exists(), "the marker is cleared");
    assert!(bound.exists(), "the binding is written for the new ID");
    assert!(
        state::marks(&vault.a.lock())
            .expect("marks")
            .expect("row")
            .binding_written
    );
    // Nothing before the re-join: the first round's first calls are its vault info (read before
    // the pull, phase 5 As built 64) and its pull.
    assert_eq!(
        calls_since(&vault.relay, from)[..2],
        [Route::VaultInfo, Route::Pull]
    );
    assert_eq!(
        op_devices(&vault, 0),
        [new_id],
        "nothing went out under the old ID"
    );
    running.handle.stop();
}

/// `Sealing`, but the first `failures` seals fail as a passing native error would.
struct FlakySealing {
    failures: AtomicUsize,
}

impl BindingNative for FlakySealing {
    fn seal(&self, secret: &[u8; 32]) -> std::result::Result<Vec<u8>, BindingFailure> {
        let failed = self
            .failures
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok();
        if failed {
            return Err(BindingFailure::Failed("the secure storage is busy".into()));
        }
        Sealing.seal(secret)
    }
    fn open(&self, blob: &[u8]) -> std::result::Result<Zeroizing<[u8; 32]>, BindingFailure> {
        Sealing.open(blob)
    }
}

/// Ruling (fix round 1, item 7): a seal that failed when sync was turned on leaves
/// `binding_written = 0`; the next engine start writes it, so the check is not off for good.
#[tokio::test]
async fn a_binding_that_failed_to_seal_at_enable_is_written_at_the_next_start() {
    let vault = super::vault().await;
    let dir = tempfile::tempdir().expect("temp dir");
    let bound = dir.path().join(binding::BINDING_FILE);
    let native = FlakySealing {
        failures: AtomicUsize::new(1),
    };
    let written = |conn: &Connection| {
        state::marks(conn)
            .expect("marks")
            .expect("row")
            .binding_written
    };

    // Turning sync on: the seal fails, nothing is written.
    let error = binding::write_binding(&vault.a.lock(), dir.path(), &native);
    assert!(matches!(error, Err(AppError::Internal(_))), "{error:?}");
    assert!(!written(&vault.a.lock()));
    assert!(!bound.exists());

    let running = start(&vault, &vault.a, dir, Box::new(native));
    rounds(&running, 1).await;

    assert!(written(&vault.a.lock()), "the start wrote the binding");
    assert!(bound.exists());
    assert_eq!(
        device_of(&vault.a.lock()),
        A,
        "no re-join for an unwritten binding"
    );
    running.handle.stop();
}

/// Obligation (c): if the marker cannot be removed after the re-join, sync stops with a problem
/// rather than re-joining again on the next round.
#[tokio::test]
async fn a_marker_that_cannot_be_cleared_stops_sync_after_one_rejoin() {
    let vault = super::vault().await;
    let dir = tempfile::tempdir().expect("temp dir");
    // A folder where the marker file would be: it exists, and removing it as a file fails.
    std::fs::create_dir_all(dir.path().join(binding::REJOIN_MARKER).join("inside"))
        .expect("marker folder");
    let from = vault.relay.calls().len();

    let running = start(&vault, &vault.a, dir, Box::new(NoBinding));
    until("the loop ended", || !running.handle.is_running()).await;

    assert!(device_of(&vault.a.lock()) > A, "one re-join");
    assert!(calls_since(&vault.relay, from).is_empty(), "no round ran");
    assert_eq!(
        running.handle.runtime().problem,
        Some(SyncProblem::Internal)
    );
    let last = running.seen.last();
    assert_eq!(
        (last.phase, last.problem),
        (SyncPhase::Error, Some(SyncProblem::Internal))
    );
    let error = state::marks(&vault.a.lock())
        .expect("marks")
        .expect("row")
        .last_error
        .expect("noted");
    assert!(!error.contains('/'), "a fixed text, no path: {error}");
}

#[tokio::test]
async fn a_locked_session_ends_the_loop_without_an_error() {
    let vault = super::vault().await;
    let running = start_plain(&vault, &vault.a);
    rounds(&running, 1).await;

    vault
        .a
        .locked
        .store(true, std::sync::atomic::Ordering::SeqCst);
    running.handle.wake(Trigger::SyncNow);
    until("the loop ended", || !running.handle.is_running()).await;

    assert!(running
        .seen
        .statuses()
        .iter()
        .all(|s| s.phase != SyncPhase::Error && s.problem.is_none()));

    // Locked before it ever started: it ends without telling anything.
    let locked = super::vault().await;
    locked
        .a
        .locked
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let from = locked.relay.calls().len();
    let running = start_plain(&locked, &locked.a);
    until("the loop ended", || !running.handle.is_running()).await;
    assert!(running.seen.statuses().is_empty());
    assert!(calls_since(&locked.relay, from).is_empty());
}

/// Ruling T8-2, obligation (b): a device that crossed a barrier uploads a snapshot at its first
/// clean round, once: the force ends with the upload, and with `409 snapshot_outdated` too.
#[tokio::test]
async fn a_forced_snapshot_after_a_barrier_crossing_is_tried_once() {
    for outdated in [false, true] {
        let vault = super::vault().await;
        // Both devices crossed a barrier (`chain::cross_barrier` at their unlock); `B` already
        // pushed at it, so its envelopes carry it and mark it for `A`.
        let crossed = "UPDATE sync_state SET barrier = barrier + 1, barrier_seq = NULL";
        exec(&vault.a.lock(), crossed);
        exec(&vault.b.lock(), crossed);
        institution(&vault.a.lock(), "queued");
        let sealed: Vec<Vec<u8>> = ["b1", "b2"]
            .iter()
            .map(|id| write_and_seal(&vault, &vault.b, id))
            .collect();
        push(&vault.relay, &vault.creds, &sealed).await;
        // Another device's snapshot at 2: the ordinary policy has nothing due after it.
        vault
            .relay
            .put_snapshot(&vault.creds.auth(), 2, vec![7; 100])
            .await
            .expect("other snapshot");
        if outdated {
            vault
                .relay
                .fail_next(Route::PutSnapshot, RelayError::SnapshotOutdated);
        }
        let puts_before = routes(&vault.relay, Route::PutSnapshot);

        let running = start_plain(&vault, &vault.a);
        rounds(&running, 1).await;
        running.handle.wake(Trigger::SyncNow);
        rounds(&running, 2).await;
        running.handle.wake(Trigger::SyncNow);
        rounds(&running, 3).await;
        running.handle.stop();

        assert_eq!(
            routes(&vault.relay, Route::PutSnapshot) - puts_before,
            1,
            "outdated {outdated}"
        );
        let expected = if outdated { Some(2) } else { Some(3) };
        assert_eq!(vault.relay.snapshot_seq(), expected, "outdated {outdated}");
    }
}

/// Runs `test` on a paused current-thread runtime on a thread of its own, and fails when it has
/// not finished within `limit` of real time. Paused time moves only while the runtime is idle, so
/// a loop that spins would hang a plain `start_paused` test forever instead of failing it.
fn within_real_time<F, Fut>(limit: Duration, test: F)
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()>,
{
    let (done, finished) = std::sync::mpsc::channel();
    let thread = std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .expect("paused runtime")
            .block_on(test());
        // The receiver is gone only after a timeout, which has already failed the test.
        let _ = done.send(());
    });
    match finished.recv_timeout(limit) {
        Ok(()) => thread.join().expect("test thread"),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
            panic!("the test did not finish within {limit:?} of real time: the loop spins")
        }
        // The test panicked before it sent: pass its panic on.
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => match thread.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => unreachable!("the test thread ended without sending"),
        },
    }
}

/// The loop sleeps through a back-off (review finding 1): a passed poll or periodic time never
/// leaves its deadline in the past. Every pull fails offline; over 31 s and then 30 minutes of
/// (paused) time the rounds follow the back-off and the profile is taken a few times per round.
/// A real-time watchdog fails the test if the loop spins instead (`within_real_time`).
#[test]
fn a_back_off_sleeps_instead_of_spinning() {
    within_real_time(Duration::from_secs(60), back_off_sleeps_instead_of_spinning);
}

async fn back_off_sleeps_instead_of_spinning() {
    let vault = super::vault().await;
    for _ in 0..20 {
        vault
            .relay
            .fail_next(Route::Pull, RelayError::Network("connect".into()));
    }
    let counted = Counted::new(vault.a.clone());
    let running = start_on(vault.relay.clone(), counted.clone());

    tokio::time::sleep(Duration::from_secs(31)).await;
    let (rounds_31s, takes_31s) = (running.handle.runtime().rounds, counted.takes());
    tokio::time::sleep(Duration::from_secs(30 * 60)).await;
    let (rounds, takes) = (running.handle.runtime().rounds, counted.takes());
    running.handle.stop();

    assert_eq!(rounds_31s, 2, "the first round, then the retry after 30 s");
    // Retries at 30, 90, 210, 450 and 930 s; the next is due at 1,890 s.
    assert_eq!(rounds, 6);
    assert_eq!(running.handle.runtime().problem, Some(SyncProblem::Offline));
    assert!(takes_31s <= 2 * 12, "{takes_31s} takes in 31 s");
    // Each round takes the profile about a dozen times, and the unlock's check of the device
    // list (`rekey::refresh_trust`) once more.
    assert!(
        takes <= 6 * 12 + 1,
        "{takes} takes over 31 minutes: the loop spun"
    );
}

/// Review finding 3: a relay that never answers cannot pin the loop. The round is dropped at its
/// 15-minute deadline as a transient failure, and "Sync now" runs the next one at once.
#[tokio::test(start_paused = true)]
async fn a_stalled_round_is_dropped_at_its_deadline() {
    let vault = super::vault().await;
    // A snapshot already on the relay: the policy has nothing due, so no dump runs off the
    // runtime (paused time would jump past it).
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), 0, vec![7; 100])
        .await
        .expect("snapshot");
    let probe = Probe::new(vault.relay.clone(), &vault.a);
    probe.stalls.store(1, Ordering::SeqCst);
    let running = start_on(probe, vault.a.clone());

    tokio::time::sleep(runtime::ROUND_TIMEOUT + Duration::from_secs(1)).await;
    assert_eq!(running.handle.runtime().rounds, 1);
    assert_eq!(
        running.handle.runtime().problem,
        Some(SyncProblem::RelayError)
    );

    running.handle.wake(Trigger::SyncNow);
    rounds(&running, 2).await;
    assert_eq!(running.handle.runtime().problem, None);
    running.handle.stop();
}

/// Review finding 4: the start check (binding, re-join, marker, binding write) holds off every
/// round, and waits for a round that runs elsewhere (setup, repair).
#[tokio::test]
async fn the_start_check_waits_for_any_running_round() {
    let vault = super::vault().await;
    let dir = tempfile::tempdir().expect("temp dir");
    binding::write_rejoin_marker(dir.path()).expect("marker");
    let marker = dir.path().join(binding::REJOIN_MARKER);
    let round_elsewhere = crate::services::sync::engine::exclusive(&vault.a).await;

    let running = start(&vault, &vault.a, dir, Box::new(NoBinding));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        device_of(&vault.a.lock()),
        A,
        "no re-join while a round runs"
    );
    assert!(marker.exists());

    drop(round_elsewhere);
    rounds(&running, 1).await;
    assert!(device_of(&vault.a.lock()) > A);
    assert!(!marker.exists());
    running.handle.stop();
}

/// Ruling on concern 1: the forced upload after a crossing survives a restart. The first engine
/// seals at the new barrier (so `marker_barrier` catches up) but its upload fails; it stops (the
/// app quits). The next engine still forces the upload.
#[tokio::test]
async fn a_forced_snapshot_survives_a_restart() {
    let vault = super::vault().await;
    let crossed = "UPDATE sync_state SET barrier = barrier + 1, barrier_seq = NULL";
    exec(&vault.a.lock(), crossed);
    exec(&vault.b.lock(), crossed);
    institution(&vault.a.lock(), "queued");
    let sealed: Vec<Vec<u8>> = ["b1", "b2"]
        .iter()
        .map(|id| write_and_seal(&vault, &vault.b, id))
        .collect();
    push(&vault.relay, &vault.creds, &sealed).await;
    vault
        .relay
        .put_snapshot(&vault.creds.auth(), 2, vec![7; 100])
        .await
        .expect("other snapshot");
    vault
        .relay
        .fail_next(Route::PutSnapshot, RelayError::Network("timeout".into()));
    let puts_before = routes(&vault.relay, Route::PutSnapshot);

    let first = start_plain(&vault, &vault.a);
    rounds(&first, 1).await;
    first.handle.stop();
    until("the first engine ended", || !first.handle.is_running()).await;
    {
        let a = vault.a.lock();
        let sync = state::load(&a).expect("state").expect("row");
        let marks = state::marks(&a).expect("marks").expect("row");
        assert_eq!(marks.marker_barrier, sync.barrier, "the barrier is marked");
        assert!(state::needs_forced_snapshot(&sync, &marks), "still owed");
    }
    assert_eq!(vault.relay.snapshot_seq(), Some(2));

    let second = start_plain(&vault, &vault.a);
    rounds(&second, 1).await;
    second.handle.stop();

    assert_eq!(routes(&vault.relay, Route::PutSnapshot) - puts_before, 2);
    assert_eq!(vault.relay.snapshot_seq(), Some(3));
    let a = vault.a.lock();
    let sync = state::load(&a).expect("state").expect("row");
    let marks = state::marks(&a).expect("marks").expect("row");
    assert_eq!(marks.snapshot_barrier, sync.barrier);
    assert!(!state::needs_forced_snapshot(&sync, &marks));
}

// ---------------------------------------------------------------------------------------------
// The fast-poll window after a device was paired

#[test]
fn rounds_run_every_few_seconds_only_while_the_fast_poll_window_is_open() {
    use crate::services::sync::engine::runtime::{
        fast_window_open, periodic_gap, FAST_EVERY, PERIODIC,
    };
    let now = tokio::time::Instant::now();
    let until = now + Duration::from_secs(60);

    assert_eq!(
        periodic_gap(now, None),
        PERIODIC,
        "no window: the ordinary pace"
    );
    assert_eq!(
        periodic_gap(now, Some(until)),
        FAST_EVERY,
        "open: the fast pace"
    );
    assert_eq!(
        periodic_gap(until, Some(until)),
        PERIODIC,
        "the window is over at its end"
    );
    assert!(fast_window_open(
        until - Duration::from_millis(1),
        Some(until)
    ));
    assert!(!fast_window_open(until, Some(until)));
    assert!(!fast_window_open(now, None));
    assert!(FAST_EVERY < PERIODIC);
}

#[test]
fn the_fast_poll_window_closes_when_it_runs_out_or_a_device_was_added() {
    use crate::services::sync::engine::runtime::fast_window_done;
    let now = tokio::time::Instant::now();
    let until = Some(now + Duration::from_secs(60));

    assert!(!fast_window_done(now, until, Some(2), Some(2)), "unchanged");
    assert!(
        fast_window_done(now, until, Some(2), Some(3)),
        "a device joined"
    );
    assert!(
        !fast_window_done(now, until, Some(2), Some(1)),
        "fewer is no growth"
    );
    assert!(!fast_window_done(now, until, None, Some(3)), "no baseline");
    assert!(
        !fast_window_done(now, until, Some(2), None),
        "count unreadable"
    );
    assert!(
        fast_window_done(now + Duration::from_secs(60), until, Some(2), Some(2)),
        "the window ran out"
    );
}
