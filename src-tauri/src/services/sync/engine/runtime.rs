//! The engine's loop (spec §9.4): one task per unlocked profile that checks the device binding
//! at start (a copy re-joins before anything is pushed), then runs rounds on its triggers (the
//! unlock, a local write, every 5 minutes, "Sync now"), backs off on transient failures and tells
//! the app what changed (`EngineEvents`).
//!
//! The loop never holds the connection across the network (`run_round` takes it step by step),
//! runs one round at a time (one task, and `run_round`'s own lock), and ends quietly when the
//! profile locks: a step that finds the session closed (`AppError::Auth`) ends it without an error
//! status; the next unlock starts a fresh engine.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use rand::Rng;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use super::pull::rejoin_as_new_device;
use super::snapshots::{SnapshotPolicy, SNAPSHOT_MAX_DELAY};
use super::status::{self, problem_of, Runtime};
use super::{recheck_vault, run_round_into, ConnAccess, EngineEvents, RoundMode, RoundReport};
use crate::error::AppError;
use crate::models::sync::{SyncProblem, SyncStatus};
use crate::services::sync::binding::{self, StartupCheck};
use crate::services::sync::error::{SyncError, Transient};
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
/// The longest a round may run before it is dropped as a transient failure.
pub const ROUND_TIMEOUT: Duration = Duration::from_secs(15 * 60);
/// The loop never sleeps less than this, whatever its deadlines say.
const MIN_SLEEP: Duration = Duration::from_millis(5);

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
    running: AtomicBool,
    requested: AtomicBool,
    sync_now: AtomicBool,
    runtime: Mutex<Runtime>,
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

/// The running engine of one unlocked profile. Dropping it stops the engine.
pub struct EngineHandle {
    shared: Arc<Shared>,
    task: JoinHandle<()>,
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

    /// Stops the loop at its next await (a round in flight is dropped there; every step it took
    /// is committed, and the next round goes on from it as after a crash).
    pub fn stop(&self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.task.abort();
    }

    pub fn is_running(&self) -> bool {
        self.shared.running.load(Ordering::SeqCst)
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
/// it first ran.
struct RunningFlag(Arc<Shared>);

impl Drop for RunningFlag {
    fn drop(&mut self) {
        self.0.running.store(false, Ordering::SeqCst);
    }
}

/// Starts the engine of an unlocked, sync-enabled profile on the current tokio runtime; its first
/// round follows the unlock trigger.
pub fn spawn<R: Relay + 'static, D: ConnAccess, E: EngineEvents>(
    relay: R,
    db: D,
    events: E,
) -> EngineHandle {
    let shared = Arc::new(Shared {
        wake: Notify::new(),
        stop: AtomicBool::new(false),
        running: AtomicBool::new(true),
        requested: AtomicBool::new(false),
        sync_now: AtomicBool::new(false),
        runtime: Mutex::new(Runtime::default()),
    });
    let flag = RunningFlag(shared.clone());
    let engine = Engine {
        relay,
        db,
        events,
        shared: shared.clone(),
        policy: SnapshotPolicy::default(),
        backoff: Backoff::default(),
        watch: OutboxWatch::default(),
        started: false,
        last_sent: None,
        retry_at: None,
        next_periodic: Instant::now() + PERIODIC,
        next_poll: Instant::now() + OUTBOX_POLL,
    };
    let task = tokio::spawn(async move {
        let _flag = flag;
        engine.run().await;
    });
    let handle = EngineHandle { shared, task };
    handle.wake(Trigger::Unlock);
    handle
}

/// The profile locked (or the loop must stop): the loop ends without telling anything.
struct Ended;

struct Engine<R, D, E> {
    relay: R,
    db: D,
    events: E,
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
            let periodic_due = now >= self.next_periodic;
            if periodic_due {
                self.next_periodic = now + PERIODIC;
            }
            let mut due = false;
            if sync_now {
                self.retry_at = None;
                due = true;
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
                }
                if self.started {
                    if sync_now {
                        self.recheck_vault().await?;
                    }
                    self.round().await?;
                }
            }
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
        let checked = self
            .db
            .with_conn(|conn| binding::startup_check(conn, &dir, self.events.binding()));
        let check = match checked {
            Ok(check) => check,
            Err(error) => {
                return self.start_failed(error, "The device binding could not be checked");
            }
        };
        if let StartupCheck::Rejoin(reason) = check {
            log::info!("[SYNC] This profile is a copy ({reason:?}): it re-joins under a new ID");
            let rejoined = self
                .db
                .with_conn(|conn| rejoin_as_new_device(conn, None, unix_now()));
            if let Err(error) = rejoined {
                return self.start_failed(error, "The re-join at unlock failed");
            }
            if let Err(error) = binding::clear_rejoin_marker(&dir) {
                log::warn!(
                    "[SYNC] The restore marker could not be removed after the re-join ({}): \
                     sync stops for this session",
                    error.kind()
                );
                self.note_problem(SyncProblem::Internal)?;
                return Err(Ended);
            }
            self.write_binding()?;
        } else {
            self.write_binding_if_due()?;
        }
        Ok(true)
    }

    /// Writes the binding at start when sync is on and none is written yet (a seal that failed
    /// for a passing reason earlier), so the check does not stay off for good.
    fn write_binding_if_due(&self) -> Result<(), Ended> {
        match self.db.with_conn(binding::binding_due) {
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

    /// "Sync now" on a paused vault: one question to the relay (ruling P9).
    async fn recheck_vault(&mut self) -> Result<(), Ended> {
        match recheck_vault(&self.relay, &self.db).await {
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
                &self.relay,
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
        self.shared.update(|r| r.after_round(&report));
        match result {
            Ok(()) => self.backoff.reset(),
            Err(error) => {
                let problem = problem_of(&error);
                log::warn!("[SYNC] A sync round failed: {}", status::reason(problem));
                log::debug!("[SYNC] A sync round failed: {error}");
                self.shared.update(|r| r.problem = Some(problem));
                self.note_reason(problem)?;
                if error.backs_off() {
                    self.retry_at = Some(Instant::now() + self.backoff.on_error());
                }
            }
        }
        self.next_periodic = Instant::now() + PERIODIC;
        let pending = self.outbox_count()?;
        self.watch.after_round(pending);
        self.shared.update(|r| r.running_round = false);
        self.publish()?;
        self.shared.update(|r| r.rounds += 1);
        Ok(())
    }

    /// Sends the status when it differs from the last one sent.
    fn publish(&mut self) -> Result<(), Ended> {
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
