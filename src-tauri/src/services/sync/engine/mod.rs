//! The sync engine (spec §9.4): the steps of a sync round against the relay. Every step reads or
//! writes the profile under the connection lock and releases it before any network call
//! (`ConnAccess`). `pull` brings other devices' envelopes in; `push` seals and sends this device's;
//! `snapshots` uploads this device's snapshot and restores the vault's. `run_round` puts them in
//! order; `runtime` runs rounds on their triggers with a back-off and tells the app (`status`).

pub mod pull;
pub mod push;
pub mod runtime;
pub mod snapshots;
pub mod status;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::models::sync::{SyncProblem, SyncStatus};
use crate::services::sync::binding::BindingNative;
use crate::services::sync::blobs::policy::BlobPolicy;
use crate::services::sync::blobs::stream::IncomingBlobs;
use crate::services::sync::blobs::transfer::{run_uploads, PRE_PUSH_BUDGET, PRE_PUSH_MAX_PLAIN};
use crate::services::sync::capture::guard;
use crate::services::sync::error::{SyncError, SyncResult, TooLarge};
use crate::services::sync::held_blobs;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::keys::{self, Credentials};
use crate::services::sync::relay::{Relay, RelayError, VaultInfo};
use crate::services::sync::state;
use push::PushReport;
use snapshots::{SnapshotPolicy, Upload, MAX_SNAPSHOT_BYTES};

/// Short access to the profile's connection: the engine takes it for one step at a time and never
/// holds it across a network call. `Database` locks its session mutex; a locked profile answers
/// `AppError::Auth`, which ends the round.
pub trait ConnAccess: Clone + Send + Sync + 'static {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T>;

    /// The profile's round lock: one round at a time per profile, whoever runs it (the engine's
    /// loop, setup, the repair), so two `push_all` never interleave. An async lock, never the
    /// connection's: it is held across the network. The app opens one profile at a time, so the
    /// default is one lock for the process.
    fn round_lock(&self) -> &tokio::sync::Mutex<()> {
        &ROUNDS
    }

    /// `with_conn` if the connection is free right now, else `None` without waiting: for a `Drop`
    /// that must never hang (it may run inside a hold of this connection). Required, with no
    /// default: an implementation that never tried would silently leave every cancelled
    /// download's incoming blob behind (`blobs::stream::StoreSink`'s `Drop`).
    fn try_with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Option<Result<T>>;

    /// The blob IDs a download is writing into this profile's attachment store right now
    /// (`blobs::stream::StoreSink`): one download per blob ID at a time. The app opens one profile
    /// at a time, so the default is one set for the process.
    fn incoming_blobs(&self) -> &IncomingBlobs {
        &INCOMING_BLOBS
    }
}

impl ConnAccess for crate::db::Database {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        crate::db::Database::with_conn(self, f)
    }

    fn try_with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Option<Result<T>> {
        crate::db::Database::try_with_conn(self, f)
    }
}

/// Why a pull stopped before the relay's end. The cursor stays before the envelope that stopped
/// it, so the next pull meets it again. A missing envelope or parent is no stop: it is held
/// (`PullReport::gaps`, audit M3), and so is a change dated too far ahead (`PullReport::ahead`,
/// audit M6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullStop {
    /// A malformed chain link, a changeset that does not parse, or a pull page outside the
    /// relay contract: more envelopes than the limit, a sequence not above `after` or not
    /// increasing, a head below the page's last sequence or below `after` (the head never goes
    /// back), an empty page that says there is more.
    Invalid,
    /// An envelope that does not open: a failed authentication, or an epoch this device has no
    /// key for (never read as a rekey: the epoch is not authenticated).
    Undecryptable,
    /// Written by a newer schema than this build knows.
    NewerSchema,
}

/// How a pull ended (ruling P1), from `PullReport::end`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullEnd {
    /// Every envelope up to the relay's end was dealt with.
    Done,
    /// Every envelope was dealt with except a quarantined author's, which are held (and perhaps
    /// some waiting for an earlier envelope: a quarantine outranks a gap). Also a pull that held a
    /// quarantined author and then stopped on a newer schema: the stop still stops applying
    /// (`PullReport::stop`), but the quarantine is what the round reports (final review A, I3).
    Held,
    /// Every envelope was dealt with except those waiting for an earlier envelope of their author
    /// or for a parent they reference (held; the next pull meets them again, audit M3), and
    /// perhaps some dated too far ahead (a missing change outranks one dated too far ahead).
    Waiting,
    /// Every envelope was dealt with except changes dated more than ten years past this device's
    /// time (`chain::too_far_ahead`) and the later envelopes of their authors, gaps behind them
    /// (held; the next pull meets them again; skippable after 14 days, audit M6).
    Ahead,
    /// The pull reached its page limit before the relay's head; the next round goes on.
    Unfinished,
    /// The pull stopped on an envelope (or a page) and went no further.
    Stopped(PullStop),
}

/// What one pull did.
#[derive(Debug, Default)]
pub struct PullReport {
    /// Envelopes held because their changeset is dated more than ten years ahead
    /// (`chain::too_far_ahead`, audit M6). The same authors' later envelopes are gaps behind them.
    pub ahead: usize,
    /// The authors whose next envelope this pull held as too far ahead.
    ahead_authors: BTreeSet<DeviceId>,
    /// This pull's gaps by author (`gaps`), so the gaps behind a change dated too far ahead are
    /// told apart from missing ones (`missing_gaps`).
    gaps_by_author: BTreeMap<DeviceId, usize>,
    /// Changesets merged.
    pub applied: usize,
    pub stop: Option<PullStop>,
    /// This device re-joined under a new device ID (a clone or a rollback).
    pub rejoined: bool,
    /// Envelopes held for a quarantined author.
    pub held: usize,
    /// Envelopes held because an earlier one of their author, or a parent they reference, has not
    /// arrived.
    pub gaps: usize,
    /// The profile was restored from a snapshot (`snapshots::restore_after_410`).
    pub restored: bool,
    /// The relay's head is past the last envelope it served: it withholds a trailing envelope (or
    /// served none), so this device is behind. Shown in diagnostics; not an error, and the cursor
    /// never moves to the head.
    pub behind: bool,
    /// The pull reached its page limit (`pull::MAX_PULL_PAGES`) before the relay's head.
    pub unfinished: bool,
    /// The head the first page reported: how far this pull looked (`pull::pull_echo`).
    pub reach: Option<u64>,
}

impl PullReport {
    /// The pull reached the relay's end with nothing stopped or held (P23).
    pub fn done(&self) -> bool {
        self.end() == PullEnd::Done
    }

    /// A stop comes first, then an unfinished pull, then a quarantined author's held envelopes,
    /// then envelopes waiting for a missing one (`missing_gaps`), then changes dated too far ahead
    /// (with the gaps of their authors behind them): a quarantine outranks a gap, so a forked
    /// author is shown even while another author's envelope is missing, and a missing change
    /// outranks one dated too far ahead (both are skippable after 14 days, each from its own
    /// alert). A quarantine also outranks a newer-schema stop, which a key holder can write
    /// (final review A, I3): `Held`, while `stop` stays set and still stops applying.
    pub fn end(&self) -> PullEnd {
        match &self.stop {
            Some(PullStop::NewerSchema) if self.held > 0 => PullEnd::Held,
            Some(stop) => PullEnd::Stopped(stop.clone()),
            None if self.unfinished => PullEnd::Unfinished,
            None if self.held > 0 => PullEnd::Held,
            None if self.missing_gaps() > 0 => PullEnd::Waiting,
            None if self.ahead > 0 => PullEnd::Ahead,
            None => PullEnd::Done,
        }
    }

    /// A gap of `device` held by this pull (an ordinary gap, also behind the author's change dated
    /// too far ahead).
    pub(crate) fn note_gap(&mut self, device: DeviceId) {
        self.gaps += 1;
        *self.gaps_by_author.entry(device).or_insert(0) += 1;
    }

    /// `device`'s next envelope is dated too far ahead (audit M6): held, and its author's gaps
    /// wait behind it rather than for a missing change.
    pub(crate) fn note_ahead(&mut self, device: DeviceId) {
        self.ahead += 1;
        self.ahead_authors.insert(device);
    }

    /// The gaps that wait for a missing envelope or parent: every gap except those of an author
    /// whose next envelope this pull held as too far ahead (they wait behind it, nothing of that
    /// author is missing, and its `Ahead` record covers them).
    pub fn missing_gaps(&self) -> usize {
        let behind_ahead: usize = self
            .gaps_by_author
            .iter()
            .filter(|(device, _)| self.ahead_authors.contains(*device))
            .map(|(_, n)| n)
            .sum();
        self.gaps.saturating_sub(behind_ahead)
    }

    /// Whether this pull held anything: a quarantined author's envelopes, a gap, or a change dated
    /// too far ahead. Such a pull keeps its own problem over a relay error (endless pulls, the
    /// echo check): the person's way out is offered under that problem.
    pub fn holds_something(&self) -> bool {
        self.held > 0 || self.gaps > 0 || self.ahead > 0
    }

    /// Whether the round may push after this pull (ruling P1): the pull ended done, with only
    /// held envelopes (a quarantined author's, ones waiting for a missing envelope or parent: a
    /// withheld envelope never stalls this device, audit M3, or ones dated too far ahead, audit
    /// M6), or stopped only on a newer schema
    /// (an older device keeps pushing, spec §14). An unfinished pull does not push: a device that
    /// is behind catches up before adding more (§9.4).
    pub fn may_push(&self) -> bool {
        matches!(
            self.end(),
            PullEnd::Done
                | PullEnd::Held
                | PullEnd::Waiting
                | PullEnd::Ahead
                | PullEnd::Stopped(PullStop::NewerSchema)
        )
    }

    /// Whether the round may dump and upload a snapshot after this pull (rulings P4, P14): only
    /// after a pull that ended done. Unlike `may_push`, held or waiting envelopes (changes dated
    /// too far ahead too) and a newer schema rule it out: the snapshot's sequence is the cursor this device applied, and a
    /// restorer skips everything up to it.
    pub fn may_snapshot(&self) -> bool {
        self.done()
    }
}

/// What the engine tells the app, and what it needs from it (the Tauri adapter in
/// `commands::sync`).
pub trait EngineEvents: Send + Sync + 'static {
    /// A round merged other devices' changes or restored a snapshot (`sync-data-changed`).
    fn data_changed(&self);
    /// The status differs from the last one sent (`sync-status-changed`).
    fn status_changed(&self, status: &SyncStatus);
    /// The platform's machine-bound sealing for the device binding (Task 14).
    fn binding(&self) -> &dyn BindingNative;
    /// The folder next to the profile: the restore marker and the binding live there.
    fn data_dir(&self) -> PathBuf;
    /// Whether a binding check already failed in this app start (audit M10, final review A I2):
    /// one mark for the whole process, which outlives any one engine. Every unlock and every sync
    /// change spawns a new engine, and a failed check counts once per app start among all of them.
    fn binding_failed_this_start(&self) -> &AtomicBool;
}

/// What a round is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundMode {
    Normal,
    /// The vault's info is read and a snapshot uploaded at the first clean pull, outside the
    /// upload policy: setup's first snapshot, and the first clean round after a barrier crossing
    /// (ruling T8-2).
    FirstSnapshot,
    /// The repair's drain (Task 15): runs on a capture-broken profile, takes no snapshot.
    Drain,
}

/// What one round did. A round that stopped on a sync state (the vault gone, a lapse, a quota, a
/// rotated key, a stopped pull) returns its report with `problem` set.
#[derive(Debug, Default)]
pub struct RoundReport {
    pub pull: Option<PullReport>,
    pub push: Option<PushReport>,
    /// A snapshot was restored by merge (after `410`, or to heal a below-barrier miss).
    pub restored: bool,
    pub snapshot_uploaded: bool,
    /// The upload found a newer snapshot on the relay (`409 snapshot_outdated`).
    pub snapshot_outdated: bool,
    /// The vault's info, when the round read it.
    pub vault: Option<VaultInfo>,
    pub problem: Option<SyncProblem>,
}

impl RoundReport {
    /// Other devices' changes reached the profile: a merge applied something, or a restore ran.
    pub fn data_changed(&self) -> bool {
        self.restored || self.pull.as_ref().is_some_and(|pull| pull.applied > 0)
    }

    /// This device re-joined under a new ID during the round: its binding is written again.
    pub fn rejoined(&self) -> bool {
        self.pull.as_ref().is_some_and(|pull| pull.rejoined)
    }
}

/// The envelopes waiting in `sync_unsent` for their echo, and the oldest one's sequence.
fn unechoed<D: ConnAccess>(db: &D) -> Result<(usize, Option<u64>)> {
    db.with_conn(|conn| {
        let (n, oldest): (i64, Option<i64>) =
            conn.query_row("SELECT COUNT(*), MIN(seq) FROM sync_unsent", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?;
        Ok((
            usize::try_from(n).unwrap_or(usize::MAX),
            oldest.and_then(|seq| u64::try_from(seq).ok()),
        ))
    })
}

/// The process's round lock (`ConnAccess::round_lock`; one profile is open at a time).
static ROUNDS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The process's downloads in progress (`ConnAccess::incoming_blobs`; one profile is open at a
/// time).
static INCOMING_BLOBS: IncomingBlobs = IncomingBlobs::new();

/// Holds off every round of `db`'s profile while the guard lives: the engine's start check (the
/// binding, a re-join, the marker, the binding write) takes it, so a setup or repair round cannot
/// interleave with a re-join.
pub async fn exclusive<D: ConnAccess>(db: &D) -> tokio::sync::MutexGuard<'_, ()> {
    db.round_lock().lock().await
}

/// One sync round (spec §9.4): `run_round_into` on a fresh report.
pub async fn run_round<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    policy: &mut SnapshotPolicy,
    mode: RoundMode,
    now: i64,
    jitter: i64,
) -> SyncResult<RoundReport> {
    let mut report = RoundReport::default();
    run_round_into(relay, db, policy, mode, now, jitter, &mut report).await?;
    Ok(report)
}

/// One sync round, filling `report` as it goes, so a caller still sees what the round did when it
/// ends in an error (a merge that applied changes before a later step failed). Waits for any other
/// round of the profile to finish first (`ConnAccess::round_lock`).
///
/// 1. A profile without sync, or without credentials, does nothing. A capture-broken profile
///    syncs only in `Drain`; a vault the relay no longer knows (`vault_gone_at`) is not asked. A
///    `Normal` round of a device that crossed a barrier and owes the vault a snapshot at it
///    (`state::needs_forced_snapshot`, ruling T8-2) runs as `FirstSnapshot`, until an upload goes
///    through or finds a newer snapshot (`409`, noted with `state::note_snapshot_barrier`).
/// 2. Pull. `410` restores the vault's snapshot by merge, then pulls once more (a second `410` is
///    a relay error).
/// 3. Push only when the pull allows it (ruling P1, `PullReport::may_push`), after sending small
///    documents and photos within a budget (`upload_before_push`, not in `Drain`). A push that sent
///    anything is followed by a second pull, which brings what it sent back (the echo, audit M4):
///    only that removes an envelope from `sync_unsent`; a failure of that pull is logged and ends
///    the round without an error. After an echo pull that was not stopped or unfinished, the
///    round's problem is a relay error when an envelope sent in an earlier round is still not
///    back, or when sealing stopped at `push::MAX_UNECHOED` and the pull brought none of it back
///    (a pull-stop problem is never replaced, and a pull that holds something keeps its own
///    problem: `PullReport::holds_something`).
/// 4. Not in `Drain`: when the mode asks for it or the policy wants it, read the vault's info,
///    heal a below-barrier miss with it, and, after pulls that ended done (rulings P4, P14),
///    upload a snapshot if due (forced in `FirstSnapshot`).
/// 5. A sync state is a problem, not an error (ruling P8): the vault gone (paused in
///    `sync_state`), a lapse (with the vault's `paid_until` when the relay still answers), the
///    quota, a snapshot too large, a rotated token or an unauthorized one. A transient, rate-limit,
///    refusal or local failure is returned as an error (the runtime shows it with
///    `status::problem_of_round`, where a quarantine the pull held outranks a relay error, the
///    network and the rate limit); nothing inside a round is retried, so a round lasts at most
///    one relay timeout past its last answered call.
/// 6. A sync server that went back (`state::vault_went_back_at`) is the round's problem in place
///    of none, a gap, a change held for its date, an invalid chain or a relay error
///    (`status::went_back`). A
///    round with no problem, or only a waiting gap (`Gap`) or change (`ClockAhead`), records its
///    success; one with any other problem records its fixed reason.
#[allow(clippy::too_many_arguments)]
pub async fn run_round_into<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    policy: &mut SnapshotPolicy,
    mode: RoundMode,
    now: i64,
    jitter: i64,
    report: &mut RoundReport,
) -> SyncResult<()> {
    let _one_round = db.round_lock().lock().await;
    let ready = db.with_conn(|conn| {
        let Some(sync) = state::load(conn)?.filter(|sync| sync.enabled) else {
            return Ok(None);
        };
        // A guarded or read-only connection is capture-broken whatever the mark says (it may
        // not have been written): nothing runs against it but the repair's drain.
        let paused = guard::changes_paused(conn);
        Ok(keys::credentials(conn)?
            .zip(state::marks(conn)?)
            .map(|(creds, marks)| {
                let forced = state::needs_forced_snapshot(&sync, &marks);
                (creds, marks, forced, paused)
            }))
    })?;
    let Some((creds, marks, forced, paused)) = ready else {
        return Ok(());
    };
    let mode = if mode == RoundMode::Normal && forced {
        RoundMode::FirstSnapshot
    } else {
        mode
    };
    if (marks.capture_broken_at.is_some() || paused) && mode != RoundMode::Drain {
        report.problem = Some(SyncProblem::CaptureBroken);
        return Ok(());
    }
    if marks.vault_gone_at.is_some() {
        report.problem = Some(SyncProblem::VaultGone);
        return Ok(());
    }

    if let Err(error) = round_steps(relay, db, &creds, policy, mode, now, jitter, report).await {
        let problem = match error {
            SyncError::VaultNotFound => {
                db.with_conn(|conn| state::set_vault_gone(conn, Some(now)))?;
                log::warn!("[SYNC] The relay does not know this vault: sync is paused");
                SyncProblem::VaultGone
            }
            SyncError::Lapsed => {
                if let Ok(info) = relay.vault_info(&creds.auth()).await {
                    report.vault = Some(info);
                }
                SyncProblem::Lapsed
            }
            SyncError::TooLarge(TooLarge::Quota) => SyncProblem::QuotaExceeded,
            SyncError::TooLarge(TooLarge::Snapshot) => SyncProblem::SnapshotTooLarge,
            SyncError::ReadOnlyToken => SyncProblem::KeyRotated,
            SyncError::Unauthorized => SyncProblem::Unauthorized,
            error => return Err(error),
        };
        report.problem = Some(problem);
    }
    if report.problem.is_none() {
        report.problem = report.pull.as_ref().and_then(status::problem_of_pull);
    }
    // A sync server that went back stays visible in every round until a replace (no silent
    // divergence once the head has passed this device's place again).
    let problem = report.problem;
    report.problem = db.with_conn(|conn| status::went_back(conn, problem))?;
    // A gap alone is no failure: the round synced everything it could and keeps pushing, so it
    // counts for "Last synced" (the `Gap` problem and the `waiting` phase are still reported).
    // An unfinished pull behind a held gap that pushed nothing did not sync: no success, the gap
    // stays the problem. A change held for its date (`ClockAhead`) counts alike.
    let stalled = report.push.is_none()
        && report
            .pull
            .as_ref()
            .is_some_and(|pull| pull.end() == PullEnd::Unfinished);
    db.with_conn(|conn| match report.problem {
        Some(problem @ (SyncProblem::Gap | SyncProblem::ClockAhead)) if stalled => {
            state::record_error(conn, status::reason(problem))
        }
        None | Some(SyncProblem::Gap | SyncProblem::ClockAhead) => state::record_success(conn, now),
        Some(problem) => state::record_error(conn, status::reason(problem)),
    })?;
    Ok(())
}

/// Steps 2–4 of `run_round_into`.
#[allow(clippy::too_many_arguments)]
async fn round_steps<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &mut SnapshotPolicy,
    mode: RoundMode,
    now: i64,
    jitter: i64,
    report: &mut RoundReport,
) -> SyncResult<()> {
    let pulled = match pull::pull_all(relay, db, creds, now).await {
        Err(SyncError::SnapshotRequired) => {
            if !policy.restore_allowed(now) {
                log::warn!(
                    "[SYNC] The relay asked for a snapshot restore again within the back-off"
                );
                report.problem = Some(SyncProblem::RelayError);
                return Ok(());
            }
            let restored = snapshots::restore_after_410(relay, db, creds, now).await;
            // Only a restore that ran, or was refused on the snapshot's own bytes, uses up the
            // back-off window; a transient failure leaves the next round free to try again.
            if matches!(
                restored,
                Ok(_)
                    | Err(SyncError::Local(AppError::Validation(_)))
                    | Err(SyncError::TooLarge(TooLarge::Body))
            ) {
                policy.note_restore(now);
            }
            restored?;
            report.restored = true;
            log::info!("[SYNC] Restored the vault's snapshot after the relay pruned this cursor");
            match pull::pull_all(relay, db, creds, now).await {
                Err(SyncError::SnapshotRequired) => {
                    log::warn!("[SYNC] The relay asked for a snapshot restore twice in a round");
                    report.problem = Some(SyncProblem::RelayError);
                    return Ok(());
                }
                other => other?,
            }
        }
        other => other?,
    };
    if pulled.done() && !pulled.behind {
        // Caught up to the server's head: the rows a replace's snapshot lacked have come back by
        // now (ADR 0030). A pull behind the head may not have served them yet.
        if let Err(error) = db.with_conn(held_blobs::release) {
            log::warn!("[SYNC] Could not release the blobs a replace held: {error}");
        }
    }
    if pulled.gaps > 0 {
        // Once per round, not per envelope: every later envelope of a waiting author waits too.
        log::info!(
            "[SYNC] {} envelopes wait for an earlier envelope or a parent that has not arrived",
            pulled.gaps
        );
    }
    // A round that fails on a transient error returns before this point: it neither counts as an
    // unfinished pull nor resets the run. An unfinished pull that holds nothing and made progress
    // still records success (pull progress is syncing); only the run of `UNFINISHED_BEFORE_PUSH`
    // turns it into a relay error.
    // Endless pulls (audit L3): an unfinished pull does not push, until `UNFINISHED_BEFORE_PUSH` of
    // them in a row. The round then pushes and reports a relay error, so it records no success; a
    // held gap behind a long tail is such a pull too. The push still stops at
    // `push::MAX_UNECHOED`, and the echo pull's unechoed check waits for a pull that reached the
    // relay's end.
    let endless =
        policy.note_pull(&pulled) >= snapshots::UNFINISHED_BEFORE_PUSH && !pulled.may_push();
    let may_push = pulled.may_push() || endless;
    if endless {
        log::warn!("[SYNC] Pulls keep ending unfinished: pushing anyway");
        // A pull that holds something keeps its own problem (a quarantine, else the gap, else a
        // change held for its date): the person's way out (the skip) is offered under `Gap` and
        // `ClockAhead` only.
        if !pulled.holds_something() {
            report.problem = Some(SyncProblem::RelayError);
        }
    }
    report.pull = Some(pulled);
    if !may_push {
        return Ok(());
    }
    if mode != RoundMode::Drain {
        upload_before_push(relay, db, creds, &policy.blobs, now).await;
    }
    let pushed = push::push_all(relay, db, creds).await?;
    let (sent, capped, resent_through) = (
        pushed.pushed > 0,
        pushed.unechoed_cap,
        pushed.resent_through,
    );
    report.push = Some(pushed);
    if !sent {
        return after_push(relay, db, creds, policy, mode, now, jitter, report).await;
    }
    // The echo (audit M4): what the push sent leaves `sync_unsent` only when this device pulls it
    // back, so the round pulls once more. On an honest relay nothing it sent stays pending, and a
    // snapshot's `S` covers it (ruling P4). A transient failure of the echo pull (a `410`, a `429`,
    // the network between the two pulls) does not fail a round whose push went through: the next
    // round's pull is the echo. Any other error is the round's, as for the first pull.
    let reach = report.pull.as_ref().and_then(|pull| pull.reach);
    match pull::pull_echo(relay, db, creds, now, reach).await {
        Ok(echo) => {
            if let Some(pull) = report.pull.as_mut() {
                absorb(pull, echo);
            }
        }
        Err(
            error
            @ (SyncError::RateLimited | SyncError::Transient(_) | SyncError::SnapshotRequired),
        ) => {
            log::info!("[SYNC] The pull after the push failed; the next round pulls the echo");
            log::debug!("[SYNC] The pull after the push failed: {error}");
            return Ok(());
        }
        Err(error) => return Err(error),
    }
    // Only a pull that reached the relay's end (or got past a hold or a gap, where own envelopes
    // are still acknowledged) says what came back; a pull stop keeps its own problem.
    let reached = report
        .pull
        .as_ref()
        .is_some_and(|pull| !matches!(pull.end(), PullEnd::Stopped(_) | PullEnd::Unfinished));
    if reached {
        let (left, oldest) = unechoed(db)?;
        let lost_earlier = resent_through.is_some_and(|r| oldest.is_some_and(|o| o <= r));
        if (capped && left >= push::MAX_UNECHOED) || lost_earlier {
            // The relay takes envelopes and never serves them back, even right after the push.
            log::warn!("[SYNC] The relay does not serve this device's envelopes back");
            // As after endless pulls, a pull that holds something keeps its own problem: a
            // relay cannot hide a quarantine behind this (final review A, I3).
            if !report
                .pull
                .as_ref()
                .is_some_and(PullReport::holds_something)
            {
                report.problem = Some(SyncProblem::RelayError);
            }
            return Ok(());
        }
    }
    after_push(relay, db, creds, policy, mode, now, jitter, report).await
}

/// The round's upload step (attachments design §5.3, ADR 0030): before the push, content of at most
/// `PRE_PUSH_MAX_PLAIN` bytes goes up (the queue reconciled first, so content added here since the
/// last merge is queued), so another device that merges a row seldom has to wait for its content.
/// The whole step, waiting for an upload lane busy with a long transfer included, ends within
/// `PRE_PUSH_BUDGET` (an upload still running then is dropped and tried again later), and nothing
/// it meets fails or changes the round: transfer errors stay on their entries, and a local error
/// is logged and dropped (a locked profile ends the round at its next step).
async fn upload_before_push<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    now: i64,
) {
    let until = tokio::time::Instant::now() + PRE_PUSH_BUDGET;
    let uploads = run_uploads(
        relay,
        db,
        creds,
        policy,
        now,
        Some(until),
        Some(PRE_PUSH_MAX_PLAIN),
    );
    match tokio::time::timeout_at(until, uploads).await {
        Ok(Ok(sent)) if sent.uploaded > 0 || sent.failed > 0 => log::debug!(
            "[SYNC] Before the push: {} documents or photos uploaded, {} failed",
            sent.uploaded,
            sent.failed
        ),
        Ok(Ok(_)) => {}
        Ok(Err(AppError::Auth(_))) => {}
        Ok(Err(error)) => log::warn!("[SYNC] Documents and photos were not uploaded: {error}"),
        Err(_) => log::info!("[SYNC] Uploads before the push ran out of time; the rest waits"),
    }
}

/// Step 4 of `run_round_into`: the vault's info, healing a below-barrier miss, and the snapshot.
#[allow(clippy::too_many_arguments)]
async fn after_push<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &mut SnapshotPolicy,
    mode: RoundMode,
    now: i64,
    jitter: i64,
    report: &mut RoundReport,
) -> SyncResult<()> {
    if mode == RoundMode::Drain || (mode == RoundMode::Normal && !policy.wants_info(now)) {
        return Ok(());
    }
    let info = relay.vault_info(&creds.auth()).await?;
    report.vault = Some(info.clone());
    if snapshots::heal_missed(relay, db, creds, policy, &info, now)
        .await?
        .is_some()
    {
        report.restored = true;
    }
    if !report.pull.as_ref().is_some_and(PullReport::may_snapshot) {
        return Ok(());
    }
    let force = mode == RoundMode::FirstSnapshot;
    match snapshots::upload_if_due(
        relay,
        db,
        creds,
        policy,
        &info,
        now,
        jitter,
        force,
        MAX_SNAPSHOT_BYTES,
    )
    .await?
    {
        Upload::Uploaded => report.snapshot_uploaded = true,
        Upload::Outdated => {
            report.snapshot_outdated = true;
            if force {
                db.with_conn(state::note_snapshot_barrier)?;
            }
        }
        Upload::Skipped => {}
    }
    Ok(())
}

/// Adds a later pull of the same round (the echo pull) to the first: the counts add up, the later
/// pull's ending stands, its gaps and its authors held for their date included (final review A,
/// minor 1: `missing_gaps` reads one pull, not a mix of two).
fn absorb(first: &mut PullReport, later: PullReport) {
    first.applied += later.applied;
    first.rejoined |= later.rejoined;
    first.restored |= later.restored;
    first.held = later.held;
    first.gaps = later.gaps;
    first.gaps_by_author = later.gaps_by_author;
    first.ahead = later.ahead;
    first.ahead_authors = later.ahead_authors;
    first.stop = later.stop;
    first.behind = later.behind;
    first.unfinished = later.unfinished;
    first.reach = later.reach.or(first.reach);
}

/// "Sync now" on a paused vault (ruling P9): asks the relay for the vault once and lifts the pause
/// when it answers. `Ok(false)` while it still does not know the vault (the pause stays; keys are
/// never deleted on a `404`), and when nothing is paused or sync is off.
pub async fn recheck_vault<R: Relay, D: ConnAccess>(relay: &R, db: &D) -> SyncResult<bool> {
    let creds = db.with_conn(|conn| {
        let paused = state::marks(conn)?.is_some_and(|marks| marks.vault_gone_at.is_some());
        if paused {
            keys::credentials(conn)
        } else {
            Ok(None)
        }
    })?;
    let Some(creds) = creds else {
        return Ok(false);
    };
    match relay.vault_info(&creds.auth()).await {
        Ok(_) => {
            db.with_conn(|conn| state::set_vault_gone(conn, None))?;
            log::info!("[SYNC] The relay knows this vault again: sync resumes");
            Ok(true)
        }
        Err(RelayError::VaultNotFound) => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
pub(crate) mod tests;
