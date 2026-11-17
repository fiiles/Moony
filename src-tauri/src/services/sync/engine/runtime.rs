//! The engine's loop (spec §9.4): one task per unlocked profile that checks the device binding
//! at start (a copy re-joins before anything is pushed), then runs rounds on its triggers (the
//! unlock, a local write, every 5 minutes, "Sync now"), backs off on transient failures and tells
//! the app what changed (`EngineEvents`).
//!
//! The loop never holds the connection across the network (`run_round` takes it step by step),
//! runs one round at a time (one task, and `run_round`'s own lock), and ends quietly when the
//! profile locks: a step that finds the session closed (`AppError::Auth`) ends it without an error
//! status; the next unlock starts a fresh engine.
//!
//! Beside the loop, a second task runs the two transfer lanes of documents and photos (attachments
//! design §5.3, §5.4; ADR 0030): uploads and downloads, one blob at a time each, sharing the
//! round's `BlobPolicy`. They take what the round leaves (content over 8 MiB, content that failed
//! an attempt, every download), wake after every round, on "Sync now" and on `wake_transfers`, and
//! look at their queue at least every `LANE_IDLE`. Nothing they do delays or fails a round. The
//! task first waits on the profile's lane gate (`ConnAccess::lanes_lock`), so an earlier engine's
//! lanes have ended before these start; then, once and before either lane transfers, it sweeps
//! incomplete blobs out of the store and reconciles the whole queue with the store.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::Rng;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::pull::rejoin_as_new_device;
use super::snapshots::{SnapshotPolicy, SNAPSHOT_MAX_DELAY};
use super::status::{self, problem_of_round, Runtime};
use super::{recheck_vault, run_round_into, ConnAccess, EngineEvents, RoundMode, RoundReport};
use crate::db::files_schema;
use crate::error::AppError;
use crate::models::sync::{SyncProblem, SyncStatus};
use crate::services::attachment_store;
use crate::services::sync::binding::{self, BindingNative, StartupCheck};
use crate::services::sync::blobs::policy::{BlobPolicy, Lane};
use crate::services::sync::blobs::queue;
use crate::services::sync::blobs::stream::hold;
use crate::services::sync::blobs::transfer::{run_downloads, run_uploads};
use crate::services::sync::capture::guard;
use crate::services::sync::devices;
use crate::services::sync::error::{SyncError, Transient};
use crate::services::sync::identity;
use crate::services::sync::keys::{self, Credentials};
use crate::services::sync::rekey;
use crate::services::sync::relay::Relay;
use crate::services::sync::state;

/// The first wait after a failed round.
pub const BACKOFF_START: Duration = Duration::from_secs(30);
/// The longest wait after failed rounds.
pub const BACKOFF_MAX: Duration = Duration::from_secs(1_800);
/// How often the outbox is looked at for local writes.
pub const OUTBOX_POLL: Duration = Duration::from_secs(5);
/// A local write runs a round at the latest this long (seconds) after it was first seen, even
/// while writes keep coming.
pub const LOCAL_WRITE_MAX_WAIT: i64 = 60;
/// A round runs at least this often.
pub const PERIODIC: Duration = Duration::from_secs(300);
/// While a fast-poll window is open (a device was just paired) a round runs this often.
pub const FAST_EVERY: Duration = Duration::from_secs(5);
/// How long a fast-poll window stays open at most.
pub const FAST_WINDOW: Duration = Duration::from_secs(300);
/// The longest a round may run before it is dropped as a transient failure.
pub const ROUND_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// The loop never sleeps less than this, whatever its deadlines say.
const MIN_SLEEP: Duration = Duration::from_millis(5);
/// A transfer lane looks at its queue at least this often when nothing wakes it.
pub const LANE_IDLE: Duration = Duration::from_secs(60);
/// A lane run takes no new blob after this long, so the app hears what moved while a long queue
/// is worked through (a transfer in progress is never cut); the lane goes on at once.
pub const LANE_SLICE: Duration = Duration::from_secs(1);
/// At most one `EngineEvents::attachments_changed` per this gap; a change inside it is told at
/// its end.
pub const ATTACHMENTS_EVENT_GAP: Duration = Duration::from_millis(250);

/// The wait after failed rounds: 30 s, doubling up to 30 minutes, back to 30 s after a round that
/// returned.
#[derive(Debug, Clone)]
pub struct Backoff {
    next: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            next: BACKOFF_START,
        }
    }
}

impl Backoff {
    /// The wait before the next try, and the one after it doubled.
    pub fn on_error(&mut self) -> Duration {
        let wait = self.next;
        self.next = (self.next * 2).min(BACKOFF_MAX);
        wait
    }

    pub fn reset(&mut self) {
        self.next = BACKOFF_START;
    }
}

/// The local-write trigger (plan's call P6): the outbox's row count, looked at every 5 s. A count
/// that changed since the last look (or the last round) is a write; the round runs at the first
/// look that sees no further change, or `LOCAL_WRITE_MAX_WAIT` after the write was first seen.
#[derive(Debug, Clone, Default)]
pub struct OutboxWatch {
    last_count: u64,
    first_seen: Option<i64>,
}

impl OutboxWatch {
    /// Takes in the outbox count at `now` (unix seconds); `true` when a round should run.
    pub fn observe(&mut self, pending: u64, now: i64) -> bool {
        if pending == 0 {
            *self = Self::default();
            return false;
        }
        if pending == self.last_count {
            // Quiet: a round is due if a write was seen before this look.
            return self.first_seen.take().is_some();
        }
        self.last_count = pending;
        let first = *self.first_seen.get_or_insert(now);
        if now - first >= LOCAL_WRITE_MAX_WAIT {
            self.first_seen = None;
            return true;
        }
        false
    }

    /// A round ran: what is queued now is not a new write (a round that could not push leaves
    /// rows queued, and they wait for the next trigger).
    pub fn after_round(&mut self, pending: u64) {
        self.last_count = pending;
        self.first_seen = None;
    }
}

/// How long after a round at `now` the next periodic round is due: `FAST_EVERY` while the fast-poll
/// window (`fast_until`) is open, `PERIODIC` otherwise.
pub fn periodic_gap(now: Instant, fast_until: Option<Instant>) -> Duration {
    if fast_window_open(now, fast_until) {
        FAST_EVERY
    } else {
        PERIODIC
    }
}

/// Whether the fast-poll window set to end at `fast_until` is still open at `now`.
pub fn fast_window_open(now: Instant, fast_until: Option<Instant>) -> bool {
    fast_until.is_some_and(|until| now < until)
}

/// Whether the fast-poll window is done: it ran out, or the device list holds more devices than it
/// did when the window opened (`baseline`; unknown on either side, it cannot have grown).
pub fn fast_window_done(
    now: Instant,
    fast_until: Option<Instant>,
    baseline: Option<usize>,
    count: Option<usize>,
) -> bool {
    let grown = matches!((baseline, count), (Some(before), Some(now_count)) if now_count > before);
    grown || !fast_window_open(now, fast_until)
}

/// Why a round is asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Unlock,
    LocalWrite,
    Periodic,
    /// The person asked: runs at once, past any back-off, and first asks the relay for a vault it
    /// paused on (ruling P9).
    SyncNow,
}

struct Shared {
    wake: Notify,
    stop: AtomicBool,
    /// The loop's task lives.
    running: AtomicBool,
    /// The lanes' task lives.
    lanes_running: AtomicBool,
    requested: AtomicBool,
    sync_now: AtomicBool,
    runtime: Mutex<Runtime>,
    /// What the round's upload step and the lanes share (the round's `SnapshotPolicy::blobs`).
    blobs: BlobPolicy,
    /// The start check passed: the lanes may transfer.
    started: AtomicBool,
    /// The last round failed and the loop backs off before the next (offline, rate-limited, a
    /// failing server): the lanes rest until a round runs again.
    backing_off: AtomicBool,
    /// Something the lanes moved changed the status: the loop publishes it without a round.
    status_dirty: AtomicBool,
    /// The end of the open fast-poll window (`EngineHandle::fast_poll`), if any.
    fast_until: Mutex<Option<Instant>>,
    /// The number of devices when the window opened (taken by the caller of `fast_poll`, before
    /// any round that pulls the new device can run).
    fast_baseline: Mutex<Option<usize>>,
}

impl Shared {
    fn runtime(&self) -> Runtime {
        self.runtime
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    fn update(&self, change: impl FnOnce(&mut Runtime)) {
        change(
            &mut self
                .runtime
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
    }
}

/// The running engine of one unlocked profile: its loop and its transfer lanes. Dropping it stops
/// the engine.
pub struct EngineHandle {
    shared: Arc<Shared>,
    task: JoinHandle<()>,
    lanes: JoinHandle<()>,
}

impl EngineHandle {
    pub fn wake(&self, trigger: Trigger) {
        let flag = match trigger {
            Trigger::SyncNow => &self.shared.sync_now,
            Trigger::Unlock | Trigger::LocalWrite | Trigger::Periodic => &self.shared.requested,
        };
        flag.store(true, Ordering::SeqCst);
        self.shared.wake.notify_one();
    }

    /// Opens a fast-poll window of `FAST_WINDOW`: a round every `FAST_EVERY` until it ends or the
    /// device list grows (a device that was just paired shows up when this device pulls, which
    /// otherwise waits for the next periodic round). Starts a round now. `baseline` is the number of
    /// devices at this moment, counted by the caller: a round already in flight that pulls the new
    /// device then still closes the window.
    pub fn fast_poll(&self, baseline: Option<usize>) {
        *self
            .shared
            .fast_baseline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = baseline;
        *self
            .shared
            .fast_until
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(Instant::now() + FAST_WINDOW);
        self.wake(Trigger::Periodic);
    }

    /// Wakes both transfer lanes: they look at their queue now (content added or wanted here).
    pub fn wake_transfers(&self) {
        self.shared.blobs.wake();
    }

    /// Stops the loop and the lanes at their next await (a round in flight is dropped there; every
    /// step it took is committed, and the next round goes on from it as after a crash). A transfer
    /// in flight is dropped too: its entry counts a failed attempt and its incoming blob is
    /// abandoned when the connection is free at that instant, else by the next lane start.
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.task.abort();
        self.lanes.abort();
    }

    /// Stops the engine and returns once its loop and its lanes have ended: every transfer they
    /// had in flight is dropped and settled, so nothing of this engine uses the connection after.
    pub async fn stopped(mut self) {
        self.stop();
        let _ = (&mut self.task).await;
        let _ = (&mut self.lanes).await;
    }

    /// The loop or the lanes still run.
    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst)
            || self.shared.lanes_running.load(Ordering::SeqCst)
    }

    /// The engine's memory of its rounds (the status's input).
    pub fn runtime(&self) -> Runtime {
        self.shared.runtime()
    }
}

impl Drop for EngineHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Clears `running` when the loop's future is dropped: it ended, or it was aborted, even before
/// it first ran. The lanes are woken, so they end with it.
struct RunningFlag(Arc<Shared>);

impl Drop for RunningFlag {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::SeqCst);
        self.0.blobs.wake();
    }
}

/// Clears `lanes_running` when the lanes' future is dropped.
struct LanesFlag(Arc<Shared>);

impl Drop for LanesFlag {
    fn drop(&mut self) {
        self.0.lanes_running.store(false, Ordering::SeqCst);
    }
}

/// Starts the engine of an unlocked, sync-enabled profile on the current tokio runtime, with the
/// transfer policy of this platform (`BlobPolicy::for_this_platform`); its first round follows the
/// unlock trigger.
pub fn spawn<R: Relay + 'static, D: ConnAccess, E: EngineEvents>(
    relay: R,
    db: D,
    events: E,
) -> EngineHandle {
    spawn_with(relay, db, events, BlobPolicy::for_this_platform())
}

/// `spawn` with the given transfer policy (a phone's or a desktop's), shared by the round's upload
/// step and the two lanes.
pub fn spawn_with<R: Relay + 'static, D: ConnAccess, E: EngineEvents>(
    relay: R,
    db: D,
    events: E,
    blobs: BlobPolicy,
) -> EngineHandle {
    let shared = Arc::new(Shared {
        wake: Notify::new(),
        stop: AtomicBool::new(false),
        running: AtomicBool::new(true),
        lanes_running: AtomicBool::new(true),
        requested: AtomicBool::new(false),
        sync_now: AtomicBool::new(false),
        runtime: Mutex::new(Runtime::default()),
        blobs: blobs.clone(),
        started: AtomicBool::new(false),
        backing_off: AtomicBool::new(false),
        status_dirty: AtomicBool::new(false),
        fast_until: Mutex::new(None),
        fast_baseline: Mutex::new(None),
    });
    let (relay, events) = (Arc::new(relay), Arc::new(events));
    let mut policy = SnapshotPolicy::default();
    policy.blobs = blobs;
    let lanes = Lanes {
        relay: relay.clone(),
        db: db.clone(),
        events: events.clone(),
        shared: shared.clone(),
    };
    let flag = RunningFlag(shared.clone());
    let engine = Engine {
        relay,
        db,
        events,
        shared: shared.clone(),
        policy,
        backoff: Backoff::default(),
        watch: OutboxWatch::default(),
        started: false,
        last_sent: None,
        retry_at: None,
        next_periodic: Instant::now() + PERIODIC,
        next_poll: Instant::now() + OUTBOX_POLL,
    };
    let lanes_flag = LanesFlag(shared.clone());
    let lanes = tokio::spawn(async move {
        let _flag = lanes_flag;
        reporting_panic(lanes.run()).await;
    });
    let task = tokio::spawn(async move {
        let _flag = flag;
        engine.run().await;
    });
    let handle = EngineHandle {
        shared,
        task,
        lanes,
    };
    handle.wake(Trigger::Unlock);
    handle
}

/// The profile locked (or the loop must stop): the loop ends without telling anything.
struct Ended;

struct Engine<R, D, E> {
    relay: Arc<R>,
    db: D,
    events: Arc<E>,
    shared: Arc<Shared>,
    policy: SnapshotPolicy,
    backoff: Backoff,
    watch: OutboxWatch,
    /// The binding check passed (and any re-join is done).
    started: bool,
    last_sent: Option<SyncStatus>,
    retry_at: Option<Instant>,
    next_periodic: Instant,
    next_poll: Instant,
}

impl<R: Relay, D: ConnAccess, E: EngineEvents> Engine<R, D, E> {
    async fn run(mut self) {
        let _ = self.serve().await;
    }

    async fn serve(&mut self) -> Result<(), Ended> {
        loop {
            if self.shared.stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            let now = Instant::now();
            let sync_now = self.shared.sync_now.swap(false, Ordering::SeqCst);
            let requested = self.shared.requested.swap(false, Ordering::SeqCst);
            // A timer that passed moves on whether or not its action runs, so no deadline below
            // is ever in the past.
            let poll_due = now >= self.next_poll;
            if poll_due {
                self.next_poll = now + OUTBOX_POLL;
            }
            let fast_until = self.fast_until();
            let periodic_due = now >= self.next_periodic;
            if periodic_due {
                self.next_periodic = now + periodic_gap(now, fast_until);
            }
            let mut due = false;
            if sync_now {
                self.retry_at = None;
                due = true;
                self.try_transfers_again()?;
            } else if let Some(at) = self.retry_at {
                // During a back-off, Unlock, LocalWrite and Periodic requests (and the outbox
                // poll) are dropped on purpose: the retry runs a round anyway.
                if now >= at {
                    self.retry_at = None;
                    due = true;
                }
            } else {
                due = requested || periodic_due;
                if poll_due {
                    let pending = self.outbox_count()?;
                    due |= self.watch.observe(pending, unix_now());
                }
            }
            if due {
                if !self.started {
                    self.started = self.start().await?;
                    self.shared.started.store(self.started, Ordering::SeqCst);
                }
                if self.started {
                    if sync_now {
                        self.recheck_vault().await?;
                    }
                    self.round().await?;
                }
            }
            if self.shared.status_dirty.swap(false, Ordering::SeqCst) {
                self.publish()?;
            }
            self.end_fast_poll_if_done(now);
            // While backing off, only the retry (or a message: stop, Sync now) wakes the loop.
            let deadline = match self.retry_at {
                Some(at) => at,
                None => self.next_poll.min(self.next_periodic),
            };
            let deadline = deadline.max(Instant::now() + MIN_SLEEP);
            tokio::select! {
                _ = self.shared.wake.notified() => {}
                _ = tokio::time::sleep_until(deadline) => {}
            }
        }
    }

    /// The check at unlock (obligation c), holding off every round (`engine::exclusive`):
    /// `binding::startup_check`; on `Rejoin`, the re-join commits, then the restore marker goes,
    /// then the binding is written for the new ID; otherwise a binding not yet written (sync on,
    /// `binding_written = 0`) is written now. Only then may a round run. `Ok(false)` when the
    /// check could not be made (an unreadable binding is a sync error, never an unlock failure):
    /// tried again on the next trigger.
    ///
    /// A marker that cannot be removed after the re-join stops sync for this session only, with
    /// a problem, so the profile does not re-join again on every round. The next unlock finds the
    /// marker and re-joins once more under another new ID: that churns this device's local IDs but
    /// changes nothing on the relay (nothing was pushed under the abandoned ID). No state keeps it.
    async fn start(&mut self) -> Result<bool, Ended> {
        let db = self.db.clone();
        let _no_round = super::exclusive(&db).await;
        let dir = self.events.data_dir();
        // The app start's mark, not this engine's: an unlock or a sync change spawns a new engine
        // in the same start, and a failed check counts once per start (final review A, I2).
        let this_start = self.events.binding_failed_this_start();
        let first_failure = !this_start.load(Ordering::SeqCst);
        match rejoin_if_copy(
            &self.db,
            &dir,
            self.events.binding(),
            unix_now(),
            first_failure,
        ) {
            Ok(CopyCheck::Rejoined) => self.write_binding()?,
            Ok(CopyCheck::Fine) => self.write_binding_if_due()?,
            Err(CopyCheckFailure::Check(error)) if binding::is_unchecked(&error) => {
                // Counted once per app start: later attempts, by this engine or another one
                // spawned in the same start, still stop the round.
                self.events
                    .binding_failed_this_start()
                    .store(true, Ordering::SeqCst);
                log::warn!("[SYNC] The device binding could not be checked; sync waits");
                self.note_problem(SyncProblem::SecureStorage)?;
                return Ok(false);
            }
            Err(CopyCheckFailure::Check(error)) => {
                return self.start_failed(error, "The device binding could not be checked");
            }
            Err(CopyCheckFailure::Rejoin(error)) => {
                return self.start_failed(error, "The re-join at unlock failed");
            }
            Err(CopyCheckFailure::Marker(error)) => {
                log::warn!(
                    "[SYNC] The restore marker could not be removed after the re-join ({}): \
                     sync stops for this session",
                    error.kind()
                );
                self.note_problem(SyncProblem::Internal)?;
                return Err(Ended);
            }
        }
        // The trust store catches up with the device list at unlock (phase 5 plan Task 11): a
        // merge made while no refresh ran, or by an older build, may have left records and
        // certificates unapplied. A record that removes this device ends sync before any round.
        match self
            .db
            .with_conn(|conn| rekey::refresh_trust(conn, Some(&dir), unix_now()))
        {
            Ok(true) => return self.removed_here().map(|()| false),
            Ok(false) => {}
            Err(error) if locked(&error) => return Err(Ended),
            Err(error) => {
                log::warn!("[SYNC] The device list could not be checked at unlock");
                log::debug!("[SYNC] The device list check failed: {error}");
            }
        }
        Ok(true)
    }

    /// Another device removed this one and sync is off here now (`rekey::removed_here`): the
    /// status says so, and the loop ends.
    fn removed_here(&mut self) -> Result<(), Ended> {
        self.shared.update(|r| r.problem = None);
        self.publish()?;
        Err(Ended)
    }

    /// Writes the binding at start when sync is on and none is written yet (a seal that failed
    /// for a passing reason earlier), so the check does not stay off for good.
    ///
    /// In the same connection hold (phase 5 §5.5): a profile whose sync was set up before phase 5
    /// gets its signing key, and no certificate. A re-join made one already, so only this path
    /// needs it. A failure is logged; the next start tries again, and sync goes on without it
    /// (removal stays unavailable there anyway).
    fn write_binding_if_due(&self) -> Result<(), Ended> {
        let checked = self.db.with_conn(|conn| {
            match identity::ensure_seed(conn) {
                Ok(true) => log::info!(
                    "[SYNC] This device made its signing key (no certificate: older vault)"
                ),
                Ok(false) => {}
                Err(error) => {
                    log::warn!("[SYNC] This device's signing key could not be made");
                    log::debug!("[SYNC] This device's signing key could not be made: {error}");
                }
            }
            binding::binding_due(conn)
        });
        match checked {
            Ok(true) => self.write_binding(),
            Ok(false) => Ok(()),
            Err(error) if locked(&error) => Err(Ended),
            Err(error) => {
                log::warn!("[SYNC] Could not tell whether the device binding is written");
                log::debug!("[SYNC] Could not tell whether the device binding is written: {error}");
                Ok(())
            }
        }
    }

    fn start_failed(&mut self, error: AppError, what: &str) -> Result<bool, Ended> {
        if locked(&error) {
            return Err(Ended);
        }
        log::warn!("[SYNC] {what}; sync waits for the next try");
        log::debug!("[SYNC] {what}: {error}");
        self.note_problem(SyncProblem::Internal)?;
        Ok(false)
    }

    /// A problem outside a round: remembered, noted with its fixed reason, shown.
    fn note_problem(&mut self, problem: SyncProblem) -> Result<(), Ended> {
        self.shared.update(|r| r.problem = Some(problem));
        self.note_reason(problem)?;
        self.publish()
    }

    fn note_reason(&self, problem: SyncProblem) -> Result<(), Ended> {
        match self
            .db
            .with_conn(|conn| state::record_error(conn, status::reason(problem)))
        {
            Err(error) if locked(&error) => Err(Ended),
            Err(error) => {
                log::warn!("[SYNC] Could not note the sync problem");
                log::debug!("[SYNC] Could not note the sync problem: {error}");
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// Writes the binding for this device's current ID; a failure is logged (the binding stays
    /// unwritten, so the next start does not check it).
    fn write_binding(&self) -> Result<(), Ended> {
        let dir = self.events.data_dir();
        match self
            .db
            .with_conn(|conn| binding::write_binding(conn, &dir, self.events.binding()))
        {
            Err(error) if locked(&error) => Err(Ended),
            Err(error) => {
                log::warn!("[SYNC] The device binding could not be written");
                log::debug!("[SYNC] The device binding could not be written: {error}");
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// "Sync now" for documents and photos: the server is asked again about its blob routes, the
    /// pause for room ends (`BlobPolicy::try_again`), and every transfer waiting for its backoff is
    /// due now (`queue::retry_now`); the lanes are woken after the round.
    fn try_transfers_again(&self) -> Result<(), Ended> {
        self.policy.blobs.try_again();
        match self.db.with_conn(queue::retry_now) {
            Err(error) if locked(&error) => Err(Ended),
            Err(error) => {
                log::warn!("[SYNC] Documents and photos could not be made due again");
                log::debug!("[SYNC] Documents and photos could not be made due again: {error}");
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }

    /// "Sync now" on a paused vault: one question to the relay (ruling P9).
    async fn recheck_vault(&mut self) -> Result<(), Ended> {
        match recheck_vault(self.relay.as_ref(), &self.db).await {
            Ok(_) => Ok(()),
            Err(SyncError::Local(error)) if locked(&error) => Err(Ended),
            Err(error) => {
                log::info!("[SYNC] Sync now could not reach the vault");
                log::debug!("[SYNC] Sync now could not reach the vault: {error}");
                Ok(())
            }
        }
    }

    async fn round(&mut self) -> Result<(), Ended> {
        self.shared.update(|r| r.running_round = true);
        self.publish()?;
        let mut report = RoundReport::default();
        // A relay that stalls cannot pin the loop: the round is dropped at its deadline (every
        // step it took is committed) and counts as a transient failure, so "Sync now" and the
        // back-off take over.
        let result = match tokio::time::timeout(
            ROUND_TIMEOUT,
            run_round_into(
                self.relay.as_ref(),
                &self.db,
                &mut self.policy,
                RoundMode::Normal,
                unix_now(),
                jitter(),
                &mut report,
            ),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => {
                log::warn!("[SYNC] A sync round ran past its deadline and was stopped");
                Err(SyncError::Transient(Transient::Relay))
            }
        };
        if let Err(SyncError::Local(error)) = &result {
            if locked(error) {
                return Err(Ended);
            }
        }
        if report.rejoined() {
            self.write_binding()?;
        }
        if report.data_changed() {
            self.events.data_changed();
        }
        if report.removed_here {
            // The round turned sync off (a verified removal, phase 5 §7.4); the binding file
            // goes as `setup::turn_off` removes it.
            if let Err(error) = rekey::forget_binding_after_removal(&self.events.data_dir()) {
                log::warn!("[SYNC] The device binding could not be removed after the removal");
                log::debug!("[SYNC] The device binding could not be removed: {error}");
            }
            self.shared.update(|r| r.running_round = false);
            return self.removed_here();
        }
        self.shared.update(|r| r.after_round(&report));
        match result {
            Ok(()) => self.backoff.reset(),
            Err(error) => {
                let problem = problem_of_round(&error, report.pull.as_ref());
                log::warn!("[SYNC] A sync round failed: {}", status::reason(problem));
                log::debug!("[SYNC] A sync round failed: {error}");
                self.shared.update(|r| r.problem = Some(problem));
                self.note_reason(problem)?;
                if error.backs_off() {
                    self.retry_at = Some(Instant::now() + self.backoff.on_error());
                }
            }
        }
        self.shared
            .backing_off
            .store(self.retry_at.is_some(), Ordering::SeqCst);
        let after = Instant::now();
        self.next_periodic = after + periodic_gap(after, self.fast_until());
        let pending = self.outbox_count()?;
        self.watch.after_round(pending);
        self.shared.update(|r| r.running_round = false);
        self.publish()?;
        self.shared.update(|r| r.rounds += 1);
        // What the round merged, queued or left behind is the lanes' now.
        self.policy.blobs.wake();
        Ok(())
    }

    /// Sends the status when it differs from the last one sent, with the attachments problem as
    /// the blob policy holds it now.
    fn publish(&mut self) -> Result<(), Ended> {
        let attachments = self.policy.blobs.problem();
        self.shared.update(|r| r.attachments_problem = attachments);
        let runtime = self.shared.runtime();
        match self.db.with_conn(|conn| status::status(conn, &runtime)) {
            Ok(status) => {
                if self.last_sent.as_ref() != Some(&status) {
                    self.events.status_changed(&status);
                    self.last_sent = Some(status);
                }
                Ok(())
            }
            Err(error) if locked(&error) => Err(Ended),
            Err(error) => {
                log::warn!("[SYNC] The sync status could not be read");
                log::debug!("[SYNC] The sync status could not be read: {error}");
                Ok(())
            }
        }
    }

    fn fast_until(&self) -> Option<Instant> {
        *self
            .shared
            .fast_until
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The number of devices in the local list; `None` when it cannot be read.
    fn device_count(&self) -> Option<usize> {
        self.db.with_conn(devices::count_devices).ok()
    }

    /// Closes the fast-poll window once it ran out or the device list has a device that was not
    /// there when it opened.
    fn end_fast_poll_if_done(&mut self, now: Instant) {
        let until = self.fast_until();
        if until.is_none() {
            return;
        }
        let baseline = *self
            .shared
            .fast_baseline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The count is only asked while the window is open and could still grow
        let count = if fast_window_open(now, until) && baseline.is_some() {
            self.device_count()
        } else {
            None
        };
        if fast_window_done(now, until, baseline, count) {
            *self
                .shared
                .fast_until
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            *self
                .shared
                .fast_baseline
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
            // The periodic round after the window is the ordinary one again
            self.next_periodic = Instant::now() + PERIODIC;
        }
    }

    fn outbox_count(&self) -> Result<u64, Ended> {
        let counted = self.db.with_conn(|conn| {
            let n: i64 = conn.query_row("SELECT COUNT(*) FROM sync_outbox", [], |r| r.get(0))?;
            Ok(n)
        });
        match counted {
            Ok(n) => Ok(u64::try_from(n).unwrap_or(0)),
            Err(error) if locked(&error) => Err(Ended),
            Err(error) => {
                log::debug!("[SYNC] The outbox could not be counted: {error}");
                Ok(0)
            }
        }
    }
}

/// The transfer lanes of one engine (attachments design §5.3, §5.4): what they share with its
/// loop.
struct Lanes<R, D, E> {
    relay: Arc<R>,
    db: D,
    events: Arc<E>,
    shared: Arc<Shared>,
}

impl<R: Relay, D: ConnAccess, E: EngineEvents> Lanes<R, D, E> {
    /// Waits for the profile's lane gate (an earlier engine's lanes end first: no entry of theirs
    /// is still being transferred, no incoming blob of theirs is still being written), starts the
    /// lanes (`start`), then runs the upload and the download lane side by side, and tells the app
    /// what they change (`forward_changes`). No lane transfers anything before a start succeeded:
    /// a failed start is tried again at the next wake of the download lane (after every round,
    /// "Sync now", `wake_transfers`) or after `LANE_IDLE`, while the rounds go on. Ends when both
    /// lanes ended (a locked profile, or the loop ended); an abort drops everything here before
    /// the gate is released.
    async fn run(self) {
        let _gate = self.db.lanes_lock().lock().await;
        let mut retried = false;
        loop {
            match self.start() {
                Err(Ended) => return,
                Ok(true) => break,
                Ok(false) => {}
            }
            self.shared.blobs.wait(Lane::Down, LANE_IDLE).await;
            if !self.shared.running.load(Ordering::SeqCst) {
                return;
            }
            retried = true;
        }
        if retried {
            // The wake that retried the start was the lanes' own: they look at their queue now.
            self.shared.blobs.wake();
        }
        let both = async { tokio::join!(self.lane(Lane::Up), self.lane(Lane::Down)) };
        // What the round's upload step and the lanes change (`BlobPolicy::note_changed`).
        let events = forward_changes(self.shared.blobs.changes(), ATTACHMENTS_EVENT_GAP, || {
            self.events.attachments_changed();
            self.shared.status_dirty.store(true, Ordering::SeqCst);
            self.shared.wake.notify_one();
        });
        tokio::select! {
            _ = both => {}
            _ = events => {}
        }
    }

    /// Once per engine, before either lane transfers: the incomplete blobs earlier transfers left
    /// in the store go (`attachment_store::sweep_incomplete`: no transfer runs now, the gate is
    /// held and the lanes have not begun, and a local add writes its blob within one hold), and the
    /// whole queue is reconciled with the store (`queue::reconcile`: content lost or found outside
    /// a merge), in one hold. Whether it succeeded: a store that is not open, or a local failure,
    /// is not a start (logged once per attempt; the caller tries again later); a locked profile
    /// ends the lanes.
    fn start(&self) -> Result<bool, Ended> {
        let now = unix_now();
        let started = hold(&self.db, |conn| {
            if !files_schema::is_attached(conn)? {
                return Ok(None);
            }
            let swept = attachment_store::sweep_incomplete(conn)?;
            Ok(Some((swept, queue::reconcile(conn, now)?)))
        });
        match started {
            Ok(Some((swept, reconciled))) => {
                if swept > 0 {
                    log::info!(
                        "[SYNC] Removed {swept} unfinished downloads of documents or photos"
                    );
                }
                if reconciled != queue::Reconciled::default() {
                    self.shared.blobs.note_changed();
                }
                Ok(true)
            }
            Ok(None) => {
                log::warn!("[SYNC] Documents and photos wait: the attachment store is not open");
                Ok(false)
            }
            Err(error) if locked(&error) => Err(Ended),
            Err(error) => {
                log::warn!(
                    "[SYNC] Documents and photos wait: the transfer queue could not be prepared"
                );
                log::debug!("[SYNC] The transfer queue could not be prepared: {error}");
                Ok(false)
            }
        }
    }

    /// One lane: waits for a wake (after every round, "Sync now", `wake_transfers`) or
    /// `LANE_IDLE`, then runs until a run moves nothing more. Ends with the loop, or when the
    /// profile is locked.
    async fn lane(&self, lane: Lane) {
        loop {
            self.shared.blobs.wait(lane, LANE_IDLE).await;
            loop {
                if !self.shared.running.load(Ordering::SeqCst) {
                    return;
                }
                match self.run_once(lane).await {
                    Err(Ended) => return,
                    Ok(true) => {}
                    Ok(false) => break,
                }
            }
        }
    }

    /// One run of `lane` over what is due now, taking new blobs for `LANE_SLICE`; whether it moved
    /// any blob (more may be due). Nothing runs while the engine's start check has not passed,
    /// while the loop backs off after a failed round (the retry round wakes the lanes), or while
    /// the profile does not sync now (`lane_credentials`, `lane_may_run`). A local error is logged
    /// and waits for the next wake; a locked profile ends the lane.
    async fn run_once(&self, lane: Lane) -> Result<bool, Ended> {
        if !self.shared.started.load(Ordering::SeqCst)
            || self.shared.backing_off.load(Ordering::SeqCst)
            || !lane_may_run(lane, self.shared.runtime().problem)
        {
            return Ok(false);
        }
        // Nothing goes up while a Kit joiner's hold stands, a key change of this device is pending
        // or was given up (As built R-45, R-53; review of Tasks 18–19): the round would push
        // nothing either.
        if lane == Lane::Up {
            match hold(&self.db, uploads_held) {
                Ok(false) => {}
                Ok(true) => return Ok(false),
                Err(error) if locked(&error) => return Err(Ended),
                Err(error) => {
                    log::debug!("[SYNC] The upload lane could not read the hold: {error}");
                    return Ok(false);
                }
            }
        }
        let creds = match hold(&self.db, lane_credentials) {
            Ok(Some(creds)) => creds,
            Ok(None) => return Ok(false),
            Err(error) if locked(&error) => return Err(Ended),
            Err(error) => {
                log::warn!("[SYNC] Documents and photos could not be transferred");
                log::debug!("[SYNC] The transfer lane could not read the vault: {error}");
                return Ok(false);
            }
        };
        let policy = &self.shared.blobs;
        let problem = policy.problem();
        let (relay, now) = (self.relay.as_ref(), unix_now());
        let until = Some(Instant::now() + LANE_SLICE);
        let ran = match lane {
            Lane::Up => run_uploads(relay, &self.db, &creds, policy, now, until, None).await,
            Lane::Down => run_downloads(relay, &self.db, &creds, policy, now, until).await,
        };
        let report = match ran {
            Ok(report) => report,
            Err(error) if locked(&error) => return Err(Ended),
            Err(error) => {
                log::warn!("[SYNC] Documents and photos could not be transferred");
                log::debug!("[SYNC] A transfer lane failed: {error}");
                return Ok(false);
            }
        };
        if report.changed || policy.problem() != problem {
            self.shared.blobs.note_changed();
        }
        Ok(report.uploaded + report.stored > 0)
    }
}

/// Whether uploads of documents and photos wait (the lane's own gate, as the round's): a Recovery
/// Kit joiner's push hold (`join::after_kit_join`), this device's key change pending
/// (`rotate::pending`: its token may be retired already) or given up (`rotate::stuck`).
pub(crate) fn uploads_held(conn: &rusqlite::Connection) -> crate::error::Result<bool> {
    use crate::services::sync::rekey::rotate;
    Ok(crate::services::sync::join::after_kit_join(conn)?.is_some()
        || rotate::pending(conn)?
        || rotate::stuck(conn)?)
}

/// The credentials a lane transfers with, `None` while the profile does not sync: sync off, no
/// credentials, capture broken (marked, or a guarded connection: the round runs only the repair's
/// drain, and a repair replaces the rows), or the vault gone (the round asks nothing until "Sync
/// now" finds it again). Read in one hold.
fn lane_credentials(conn: &rusqlite::Connection) -> crate::error::Result<Option<Credentials>> {
    if !state::load(conn)?.is_some_and(|sync| sync.enabled) {
        return Ok(None);
    }
    let Some(marks) = state::marks(conn)? else {
        return Ok(None);
    };
    if marks.capture_broken_at.is_some()
        || marks.vault_gone_at.is_some()
        || guard::changes_paused(conn)
    {
        return Ok(None);
    }
    keys::credentials(conn)
}

/// Whether `lane` runs after a round whose problem is `problem`, as the round itself goes on: a
/// lapse or a rotated token stops writes (pushes, uploads) while reads go on as long as the server
/// allows (pulls, downloads); a token the server refuses, a vault gone or broken capture stop
/// both. Any other problem (a quota of changes, a gap, a newer schema) leaves the lanes to their
/// own per-blob backoff; while the round itself backs off (offline, rate-limited, a failing
/// server) the lanes rest too (`Shared::backing_off`).
pub(crate) fn lane_may_run(lane: Lane, problem: Option<SyncProblem>) -> bool {
    match problem {
        Some(SyncProblem::CaptureBroken | SyncProblem::VaultGone | SyncProblem::Unauthorized) => {
            false
        }
        Some(
            SyncProblem::Lapsed
            | SyncProblem::KeyRotated
            | SyncProblem::KeyNotGiven
            | SyncProblem::KeyChangeUnverified
            | SyncProblem::KeyChangeWaiting,
        ) => lane == Lane::Down,
        _ => true,
    }
}

/// Runs `work` (the lanes) and reports a panic in it as one error line with its message, instead
/// of losing it with the task; the panic's text, if there was one. Nothing restarts the lanes.
pub(crate) async fn reporting_panic(work: impl std::future::Future<Output = ()>) -> Option<String> {
    use futures_util::FutureExt;
    match std::panic::AssertUnwindSafe(work).catch_unwind().await {
        Ok(()) => None,
        Err(panic) => {
            let message = crate::services::logging::panic_payload_text(panic.as_ref()).to_string();
            log::error!("[SYNC] Document and photo transfers stopped after a panic: {message}");
            Some(message)
        }
    }
}

/// Tells `emit` about the changes `changes` is notified of, at most once per `gap`: a change
/// within the gap after an emit is told at its end (the `Notify` keeps it), so a burst of changes
/// gives a few calls and the last change always one. Never returns.
pub(crate) async fn forward_changes(changes: &Notify, gap: Duration, mut emit: impl FnMut()) {
    loop {
        changes.notified().await;
        emit();
        tokio::time::sleep(gap).await;
    }
}

/// What [`rejoin_if_copy`] did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CopyCheck {
    /// Not a copy: nothing changed.
    Fine,
    /// A copy: it re-joined under a new device ID, and the restore marker is gone. The binding of
    /// the new ID is not written yet (`binding_written = 0`).
    Rejoined,
}

/// Why [`rejoin_if_copy`] stopped.
#[derive(Debug)]
pub enum CopyCheckFailure {
    /// The check could not be made (an unreadable binding): nothing changed.
    Check(AppError),
    /// The re-join failed and rolled back: nothing changed.
    Rejoin(AppError),
    /// The re-join committed, but the restore marker could not be removed.
    Marker(std::io::Error),
}

impl CopyCheckFailure {
    /// The error a person's action (the repair, a resumed setup) stops with: an unreadable binding
    /// stays a sync error, as at the engine's start.
    pub fn into_app_error(self) -> AppError {
        match self {
            Self::Check(error) | Self::Rejoin(error) => error,
            Self::Marker(error) => error.into(),
        }
    }
}

/// The engine start's copy check (obligation c), for every path that may push: the engine's
/// start, the repair's drain (`join::repair_capture`) and a resumed setup's first round
/// (`setup::finish_setup`). `binding::startup_check`; on `Rejoin`, the re-join commits
/// (`rejoin_as_new_device`), then the restore marker goes. `count_failure` is true only for the
/// first failed binding check of an app start (the engine). The caller holds off every round
/// (`engine::exclusive`) and writes the new ID's binding afterwards.
pub fn rejoin_if_copy<D: ConnAccess>(
    db: &D,
    data_dir: &Path,
    native: &dyn BindingNative,
    now: i64,
    count_failure: bool,
) -> Result<CopyCheck, CopyCheckFailure> {
    let check = db
        .with_conn(|conn| binding::startup_check_counted(conn, data_dir, native, count_failure))
        .map_err(CopyCheckFailure::Check)?;
    let StartupCheck::Rejoin(reason) = check else {
        return Ok(CopyCheck::Fine);
    };
    log::info!("[SYNC] This profile is a copy ({reason:?}): it re-joins under a new ID");
    db.with_conn(|conn| rejoin_as_new_device(conn, None, now))
        .map_err(CopyCheckFailure::Rejoin)?;
    binding::clear_rejoin_marker(data_dir).map_err(CopyCheckFailure::Marker)?;
    Ok(CopyCheck::Rejoined)
}

/// The session is closed (locked, or another profile opened): the engine has nothing to do.
fn locked(error: &AppError) -> bool {
    matches!(error, AppError::Auth(_))
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The random delay before a due snapshot upload, `0..=SNAPSHOT_MAX_DELAY` seconds.
fn jitter() -> i64 {
    let span = SNAPSHOT_MAX_DELAY as u64 + 1;
    (rand::rng().next_u64() % span) as i64
}
