//! The sync engine (spec §9.4): the steps of a sync round against the relay. Every step reads or
//! writes the profile under the connection lock and releases it before any network call
//! (`ConnAccess`). `pull` brings other devices' envelopes in; `push` seals and sends this device's;
//! `snapshots` uploads this device's snapshot and restores the vault's. `run_round` puts them in
//! order; `runtime` runs rounds on their triggers with a back-off and tells the app (`status`).

pub mod pull;
pub mod push;
pub mod runtime;
pub(crate) mod seen;
pub mod snapshots;
pub mod status;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::models::sync::{SyncProblem, SyncStatus};
use crate::services::sync::binding::BindingNative;
use crate::services::sync::blobs::pass;
use crate::services::sync::blobs::policy::{BlobPolicy, Lane};
use crate::services::sync::blobs::queue;
use crate::services::sync::blobs::stream::IncomingBlobs;
use crate::services::sync::blobs::transfer::{
    run_uploads_if_free, PRE_PUSH_BUDGET, PRE_PUSH_MAX_PLAIN,
};
use crate::services::sync::capture::guard;
use crate::services::sync::error::{SyncError, SyncResult, TooLarge};
use crate::services::sync::held_blobs;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::keys::{self, Credentials};
use crate::services::sync::rekey::head::{self, HeadCheck};
use crate::services::sync::rekey::pickup::{self, Pickup};
use crate::services::sync::rekey::{reseal, rotate};
use crate::services::sync::relay::{Relay, RelayError, VaultInfo};
use crate::services::sync::state;
use push::PushReport;
use snapshots::{SnapshotPolicy, Upload, MAX_SNAPSHOT_BYTES};

/// Short access to the profile's connection: the engine takes it for one step at a time and never
/// holds it across a network call. `Database` locks its session mutex; a locked profile answers
/// `AppError::Auth`, which ends the round.
pub trait ConnAccess: Clone + Send + Sync + 'static {
    /// The connection, mutably: what the sync guard needs to set or clear its authorizer
    /// (rusqlite's `Connection::authorizer` takes `&mut self`).
    fn with_conn_mut<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T>;

    /// The connection, for everything else. Defaults to `with_conn_mut`, so an implementation
    /// that counts or logs its holds does so in one place.
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        self.with_conn_mut(|conn| f(conn))
    }

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

    /// The profile's lane gate: one engine's transfer lanes at a time (`runtime`). A new engine's
    /// lanes wait on it until an earlier engine's lanes have ended, their dropped transfers
    /// settled, before they sweep the store and pick work. The app opens one profile at a time, so
    /// the default is one gate for the process.
    fn lanes_lock(&self) -> &tokio::sync::Mutex<()> {
        &LANES
    }
}

impl ConnAccess for crate::db::Database {
    fn with_conn_mut<T>(&self, f: impl FnOnce(&mut Connection) -> Result<T>) -> Result<T> {
        crate::db::Database::with_conn_mut(self, f)
    }

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
    /// An op under an older epoch past where this device stood when it adopted a key change whose
    /// head the remover's signed head has not confirmed yet (`rekey::head`): it waits for it
    /// (`keyChangeWaiting`, on the device that made the change), nothing past it applied.
    KeyChangeUnconfirmed,
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
    /// Every op at or below this relay position was dealt with, held or looked at for this
    /// device's own envelopes (their echo acknowledged): the cursor, then each op the pull
    /// handled without a stop, and in scan mode each op it scanned. What re-sealing after a key
    /// change waits for (`rekey::reseal::may_reseal`).
    pub through: u64,
    /// The pull stopped on an op whose (unauthenticated) header names this epoch, above this
    /// device's current one: a hint to pick up a new key (phase 5 §7.1), never proof.
    pub key_hint: Option<u32>,
    /// A merged changeset touched the device list: the trust store is brought up to date
    /// (`rekey::refresh_trust`) before the pull goes on.
    devices_changed: bool,
    /// The refresh found a verified record that ends sync on this device (a removal, or a `left`
    /// record made with its key; phase 5 §7.4): sync is off here now, the pull ended there.
    pub removed_here: bool,
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
    /// Documents' or photos' content, or their counts, changed (`sync-attachments-changed`, no
    /// payload): a transfer lane moved something, or the queue changed. Coalesced by the engine:
    /// at most a few calls a second, and the last change always gets one.
    fn attachments_changed(&self);
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
    /// Another device removed this one (a verified notice, phase 5 §7.4): the round turned sync
    /// off here, leaving the binding file to the runtime (`rekey::forget_binding_after_removal`).
    pub removed_here: bool,
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

/// The process's lane gate (`ConnAccess::lanes_lock`; one profile is open at a time).
static LANES: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
///    upload a snapshot if due (forced in `FirstSnapshot`). Then, not in `Drain`, the daily pass of
///    documents and photos (`daily_pass`), which never fails the round.
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
///    success; one with any other problem records its fixed reason. A round with no problem that
///    leaves no change to send (`sync_outbox`, `sync_unsent`) and no document or photo to upload,
///    stuck ones and ones not queued yet included, nor a blob a replace still holds
///    (`queue::holds_content_to_upload`), also records a complete sync
///    (`state::record_complete`, spec 2026-11-14 §7.2).
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
        queue_local_changes(conn, &policy.blobs, now)?;
        // A key change of this device owes the vault a snapshot under the new key (phase 5
        // §6.4), forced like the one after a barrier crossing.
        let epoch_owed = state::snapshot_epoch_owed(conn)?;
        let keys_state = KeyState {
            refresh_owed: crate::services::sync::rekey::refresh_owed(conn)?,
            stuck: rotate::stuck(conn)?,
            pending: rotate::pending(conn)?,
            seen: state::rekey_seen(conn)?,
            unverified: !head::unverified_epochs(conn)?.is_empty(),
            own_unverified: head::own_unverified(conn)?,
        };
        Ok(keys::credentials(conn)?
            .zip(state::marks(conn)?)
            .map(|(creds, marks)| {
                let forced = state::needs_forced_snapshot(&sync, &marks) || epoch_owed;
                (creds, marks, forced, paused, keys_state)
            }))
    })?;
    let Some((creds, marks, forced, paused, keys_state)) = ready else {
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
    if keys_state.stuck {
        // A key change of this device the server took could not complete here (review of
        // Task 17, I2): nothing syncs until sync is turned off and joined again.
        report.problem = Some(SyncProblem::KeyChangeUnverified);
        return Ok(());
    }

    let steps = round_steps(
        relay, db, &creds, keys_state, policy, mode, now, jitter, report,
    );
    if let Err(error) = steps.await {
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
    if let Some(info) = report.vault.as_ref() {
        // Whether the server holds a Recovery Kit: a removal here makes a new one when this
        // device has none to seal to (`rotate::ReplacesKit::Missing`).
        let held = info.kit.is_some();
        db.with_conn(|conn| crate::services::sync::rekey::kit::note_server_kit(conn, held))?;
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
        None => {
            state::record_success(conn, now)?;
            if status::unsent_changes(conn)? == 0 && !content_to_upload(conn)? {
                state::record_complete(conn, now)?;
            }
            Ok(())
        }
        Some(SyncProblem::Gap | SyncProblem::ClockAhead) => state::record_success(conn, now),
        Some(problem) => state::record_error(conn, status::reason(problem)),
    })?;
    Ok(())
}

/// Key changes one round picks up at most (each may walk several bundles).
const MAX_PICKUPS_PER_ROUND: usize = 3;

/// How one pass of `round_pass` ended.
enum Pass {
    Done,
    /// The pull stopped on an op sealed under a later epoch (phase 5 §7.1).
    KeyHint,
}

/// What the round's first connection hold read of the key: a pending change of this device, the
/// newest rotation dealt with (`rekey_seen`), and whether a head waits for its signature.
#[derive(Clone, Copy)]
struct KeyState {
    /// A trust-store refresh failed and is owed (`rekey::refresh_owed`).
    refresh_owed: bool,
    /// A key change of this device was given up (`rotate::stuck`).
    stuck: bool,
    pending: bool,
    seen: u32,
    unverified: bool,
    /// One of the unverified heads is this device's own (`head::own_unverified`).
    own_unverified: bool,
}

/// What sent the round to `pickup::pick_up`.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Trigger {
    /// The vault's info, read before the pull: a `rotation` above `rekey_seen`.
    Info,
    /// The vault's info read by a pass (its snapshot step): the same, after the pull.
    AfterPass,
    /// The pull stopped on an op whose header names a later epoch.
    KeyHint,
    /// A push, a snapshot upload or a read (the pull, the look-ahead) met the retired token
    /// (`ReadOnlyToken`).
    ReadOnly,
}

/// Steps 2–4 of `run_round_into`, after this device's own pending key change is resolved
/// (`rotate::resolve_pending`), with the key pickup (phase 5 §7.1, §7.2). The vault's info, when
/// the round reads it (the policy wants it, or the mode does; a new engine's first round always
/// does), is read **before** the pull too, so a `rotation` above `rekey_seen` is picked up before
/// any op is pulled (the snapshot step reads it again after the push). A pass whose pull met an op
/// under a later epoch, or whose push (or snapshot upload) met a retired token (`ReadOnlyToken`),
/// runs `pickup::pick_up` too. A new key adopted runs the pass again with it (the pull goes on
/// through the rotation point, then what was not sent is sealed again before the push); a key
/// change that gave this device nothing, or that does not verify, ends the round with
/// `KeyNotGiven` or `KeyChangeUnverified` (sync paused, never turned off); a verified removal of
/// this device turns sync off (`RoundReport::removed_here`). A retired token with no key change to
/// pick up yet stays `KeyRotated`; a later epoch's op with none keeps the pull's own problem.
/// Before each pass, every head this device adopted provisionally is looked for among the ops
/// past it (`head::verify_heads`): a signed head that differs ends the round with
/// `KeyChangeUnverified`; one not found yet lets the pass run, whose pull stops before any older
/// op it would need (`PullStop::KeyChangeUnconfirmed`).
#[allow(clippy::too_many_arguments)]
async fn round_steps<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    keys_state: KeyState,
    policy: &mut SnapshotPolicy,
    mode: RoundMode,
    now: i64,
    jitter: i64,
    report: &mut RoundReport,
) -> SyncResult<()> {
    let mut creds = creds.clone();
    let KeyState {
        refresh_owed,
        pending,
        mut seen,
        unverified,
        own_unverified,
        ..
    } = keys_state;
    // A trust-store refresh that failed since the last one is owed first (review of Tasks 20–21,
    // finding 2): a record it would apply came with a merge that is not pulled again.
    if refresh_owed
        && db.with_conn_mut(|conn| crate::services::sync::rekey::refresh_trust(conn, None, now))?
    {
        report.removed_here = true;
        return Ok(());
    }
    // A key change of this device still pending is resolved before anything else (phase 5
    // §6.4 step 6); until it is, nothing is pushed (the error ends the round). A change that was
    // not done leaves its notice there (`rekey::RemovalNotDone`).
    // The resolution of a change that landed also reads its range and signs its head
    // (`head::verify_own`), so the round does not try that a second time.
    let mut tried_own_head = false;
    if pending {
        match rotate::resolve_pending(relay, db, None, now).await? {
            Some(rotate::Outcome::RemovedHere) => {
                report.removed_here = true;
                return Ok(());
            }
            Some(rotate::Outcome::Stuck) => {
                report.problem = Some(SyncProblem::KeyChangeUnverified);
                return Ok(());
            }
            Some(rotate::Outcome::Changed { .. }) => tried_own_head = true,
            _ => {}
        }
        (creds, seen) = db.with_conn(|conn| {
            let creds = keys::credentials(conn)?.ok_or_else(|| {
                AppError::Internal("sync: the credentials went during a key change".into())
            })?;
            Ok((creds, state::rekey_seen(conn)?))
        })?;
    }
    // A head this device signs itself that is not verified yet (the removal committed, its range
    // not read) is finished before the pull; until it is, `round_pass` sends nothing (spec
    // 2026-11-20-sync-audit-2-fixes-design §2.3 steps 3–5).
    if !tried_own_head && own_unverified {
        head::verify_own(relay, db, now).await?;
    }
    // Still unsigned after the tries: `round_pass` sends nothing. Read again only when one of the
    // tries could have changed it, so the common round takes the profile no more often than before.
    let own_waiting = if tried_own_head || own_unverified {
        db.with_conn(head::own_unverified)?
    } else {
        false
    };
    // The vault's info, read before the pull when the round reads it at all (its snapshot step
    // reads it again after the push: the snapshot's sequence is the head then).
    let mut trigger = None;
    // The newest rotation this round already looked at: a pass reacts only to a rotation above
    // it (an info read again with the same counter, or not read again at all, is not news).
    let mut looked_at = None;
    if wants_info(policy, mode, now) {
        let info = relay.vault_info(&creds.auth()).await?;
        if info.rotation > seen {
            trigger = Some(Trigger::Info);
        }
        looked_at = Some(info.rotation);
        report.vault = Some(info);
    }
    // Heads to check: those waiting from an earlier round, and any a pickup adopts now.
    let mut check_heads = unverified;
    let mut pickups = 0;
    loop {
        if let Some(cause) = trigger.take() {
            if pickups == MAX_PICKUPS_PER_ROUND {
                if matches!(cause, Trigger::KeyHint | Trigger::ReadOnly) {
                    report.problem = Some(SyncProblem::KeyRotated);
                }
                return Ok(());
            }
            pickups += 1;
            match pickup::pick_up(relay, db, None, now).await? {
                Pickup::Adopted { .. } => {
                    creds = db.with_conn(keys::credentials)?.ok_or_else(|| {
                        SyncError::Local(AppError::Internal(
                            "sync: the credentials went during a key pickup".into(),
                        ))
                    })?;
                    check_heads = true;
                    // The pickup changed the store (a bundle's recipients and records); the
                    // rows merged so far may now verify too.
                    if db.with_conn_mut(|conn| {
                        crate::services::sync::rekey::refresh_trust(conn, None, now)
                    })? {
                        report.removed_here = true;
                        return Ok(());
                    }
                }
                // Nothing new before the pull: the pass runs as it would have.
                Pickup::Nothing if cause == Trigger::Info => {}
                Pickup::Nothing => {
                    if cause == Trigger::ReadOnly {
                        report.problem = Some(SyncProblem::KeyRotated);
                    }
                    return Ok(());
                }
                Pickup::NotGiven => {
                    report.problem = Some(SyncProblem::KeyNotGiven);
                    return Ok(());
                }
                Pickup::Unverified => {
                    report.problem = Some(SyncProblem::KeyChangeUnverified);
                    return Ok(());
                }
                Pickup::RemovedHere { .. } => {
                    report.removed_here = true;
                    return Ok(());
                }
            }
        }
        if check_heads {
            match head::verify_heads(relay, db, &creds, now).await {
                Ok(HeadCheck::Refuted) => {
                    report.problem = Some(SyncProblem::KeyChangeUnverified);
                    return Ok(());
                }
                Ok(HeadCheck::Clear) => check_heads = false,
                Ok(HeadCheck::Waiting) => {}
                // The look-ahead reads ops: a retired token is refused there like in the pull,
                // and means the same, a key change to pick up (protocol 3).
                Err(SyncError::ReadOnlyToken) => {
                    trigger = Some(Trigger::ReadOnly);
                    continue;
                }
                Err(error) => return Err(error),
            }
        }
        trigger = match round_pass(
            relay,
            db,
            &creds,
            policy,
            mode,
            now,
            jitter,
            own_waiting,
            report,
        )
        .await
        {
            // The vault's info: a rotation above `rekey_seen` and above what this round already
            // looked at is a key change this device has not looked at.
            Ok(Pass::Done) if report.removed_here => return Ok(()),
            Ok(Pass::Done) => {
                let rotation = report.vault.as_ref().map(|info| info.rotation);
                let rotated = match rotation {
                    Some(rotation) if looked_at.is_none_or(|seen| rotation > seen) => {
                        rotation > db.with_conn(state::rekey_seen)?
                    }
                    _ => false,
                };
                if !rotated {
                    return Ok(());
                }
                looked_at = rotation;
                Some(Trigger::AfterPass)
            }
            Ok(Pass::KeyHint) => Some(Trigger::KeyHint),
            Err(SyncError::ReadOnlyToken) => Some(Trigger::ReadOnly),
            Err(error) => return Err(error),
        };
    }
}

/// Whether the round reads the vault's info (step 4): never in `Drain`, always in
/// `FirstSnapshot`, in `Normal` when the snapshot policy wants it.
fn wants_info(policy: &SnapshotPolicy, mode: RoundMode, now: i64) -> bool {
    match mode {
        RoundMode::Drain => false,
        RoundMode::FirstSnapshot => true,
        RoundMode::Normal => policy.wants_info(now),
    }
}

/// One pass of steps 2–4 of `run_round_into` (`round_steps`). `own_waiting`: this device's own
/// key-change head is still not signed (`head::verify_own`): the pass pulls and sends nothing.
#[allow(clippy::too_many_arguments)]
async fn round_pass<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &mut SnapshotPolicy,
    mode: RoundMode,
    now: i64,
    jitter: i64,
    own_waiting: bool,
    report: &mut RoundReport,
) -> SyncResult<Pass> {
    let pulled = match pull::pull_all(relay, db, creds, now).await {
        Err(SyncError::SnapshotRequired) => {
            if !policy.restore_allowed(now) {
                log::warn!(
                    "[SYNC] The relay asked for a snapshot restore again within the back-off"
                );
                report.problem = Some(SyncProblem::RelayError);
                return Ok(Pass::Done);
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
            // The restore merged the device list too.
            if db.with_conn_mut(|conn| {
                crate::services::sync::rekey::refresh_trust(conn, None, now)
            })? {
                report.removed_here = true;
                return Ok(Pass::Done);
            }
            match pull::pull_all(relay, db, creds, now).await {
                Err(SyncError::SnapshotRequired) => {
                    log::warn!("[SYNC] The relay asked for a snapshot restore twice in a round");
                    report.problem = Some(SyncProblem::RelayError);
                    return Ok(Pass::Done);
                }
                other => other?,
            }
        }
        other => other?,
    };
    if pulled.removed_here {
        // A verified record ended sync on this device during the pull: nothing more happens
        // (no held blob is released, no lane woken; review of Tasks 20–21, minor 5).
        report.removed_here = true;
        match report.pull.as_mut() {
            Some(first) => absorb(first, pulled),
            None => report.pull = Some(pulled),
        }
        return Ok(Pass::Done);
    }
    if pulled.done() && !pulled.behind {
        // Caught up to the server's head: the rows a replace's snapshot lacked have come back by
        // now (ADR 0030). A pull behind the head may not have served them yet.
        if let Err(error) = db.with_conn(held_blobs::release) {
            log::warn!("[SYNC] Could not release the blobs a replace held: {error}");
        }
    }
    if pulled.applied > 0 {
        // The pull made content the server lacked at its last try due again
        // (`queue::content_may_have_landed`): the download lane looks now, not after the round.
        policy.blobs.wake_lane(Lane::Down);
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
    let key_hint = pulled.key_hint.is_some() && pulled.stop == Some(PullStop::Undecryptable);
    // How far this pass's own pull got when it stopped nowhere (`reseal::may_reseal`).
    let clean_through = pulled.stop.is_none().then_some(pulled.through);
    match report.pull.as_mut() {
        // A pass after a key pickup: its pull goes on from where the first one stopped.
        Some(first) => absorb(first, pulled),
        None => report.pull = Some(pulled),
    }
    if key_hint {
        return Ok(Pass::KeyHint);
    }
    if !may_push {
        return Ok(Pass::Done);
    }
    // This device's own head is still not signed (`head::verify_own` could not read its range):
    // nothing is sent, sealed again or uploaded until it is (§2.3 step 5). The next round tries
    // again.
    if own_waiting {
        log::info!("[SYNC] Nothing is sent until this device has signed its own key change");
        report.problem = Some(SyncProblem::KeyChangeWaiting);
        return Ok(Pass::Done);
    }
    // A Recovery Kit joiner sends nothing, not even its own row, until the person answers
    // "Data jsou zpět" (revision §2.6 step 7): its pulls go on.
    if db
        .with_conn(crate::services::sync::join::after_kit_join)?
        .is_some()
    {
        log::info!("[SYNC] Nothing is sent until the person decides about the Kit join");
        return Ok(Pass::Done);
    }
    // Nothing sealed under an older key goes out after a key change (phase 5 §7.2): it is sealed
    // again once the pull has reached the current epoch's start (a start only the server
    // reported: reached by this pass's pull without a stop), and until then nothing is pushed
    // (`KeyRotated`, "picking up the new key").
    let through = report.pull.as_ref().map_or(0, |pull| pull.through);
    let sendable = db.with_conn(|conn| {
        if !reseal::holds_older(conn, creds)? {
            return Ok(true);
        }
        if !reseal::may_reseal(conn, creds, through, clean_through)? {
            return Ok(false);
        }
        let resealed = reseal::reseal_unsent(conn, creds)?;
        log::info!("[SYNC] Sealed {resealed} unsent changes again under the new key");
        Ok(true)
    })?;
    if !sendable {
        log::info!("[SYNC] Unsent changes wait for the pull to reach the key change");
        report.problem = Some(SyncProblem::KeyRotated);
        return Ok(Pass::Done);
    }
    if mode != RoundMode::Drain {
        // Not ordered after the lanes' start (`runtime::Lanes::start`), and harmless: it uploads
        // only complete blobs a syncing row names that never failed an attempt (`attempts = 0`),
        // and never writes an incoming blob. Its `fail_abandoned(Up)` can count one failed
        // attempt on an old engine's upload that is still live only after the 5 s stop wait ran
        // out, or after a stop without a wait; that blob then has an attempt, so this round never
        // uploads it again.
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
        after_push(relay, db, creds, policy, mode, now, jitter, report).await?;
        daily_pass(relay, db, creds, &policy.blobs, mode, now, report).await?;
        return Ok(Pass::Done);
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
            return Ok(Pass::Done);
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
            return Ok(Pass::Done);
        }
    }
    after_push(relay, db, creds, policy, mode, now, jitter, report).await?;
    daily_pass(relay, db, creds, &policy.blobs, mode, now, report).await?;
    Ok(Pass::Done)
}

/// The daily pass of documents and photos (`blobs::pass`, ADR 0030), after a `Normal` or
/// `FirstSnapshot` round's last step went through: once a day, a caught-up device deletes the
/// server's unreferenced blobs and puts back what the server lost. A retired token (`403`) is
/// returned as `ReadOnlyToken`, so the round picks the key up; nothing else it meets fails or
/// changes the round: it ends within `pass::PASS_BUDGET`, a sync server error ends it (logged
/// there), a local error is logged here, and a locked profile ends the round at its next step.
async fn daily_pass<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    blobs: &BlobPolicy,
    mode: RoundMode,
    now: i64,
    report: &RoundReport,
) -> SyncResult<()> {
    if !matches!(mode, RoundMode::Normal | RoundMode::FirstSnapshot) {
        return Ok(());
    }
    let Some(pulled) = report.pull.as_ref() else {
        return Ok(());
    };
    // A key change this round has seen the vault announce and not picked up yet (the pickup
    // follows the pass, `Trigger::AfterPass`): the token in hand is retired, so the pass's
    // deletes would be refused and its listing would count as a failed pass. It waits for the
    // next round, with the new token (phase 5 revision Task 21).
    // The rotation is the server's unauthenticated word, so the wait is bounded (review of Tasks
    // 20–21, minor 8): at most a day per claimed rotation (`state::pass_waits`), then the pass runs
    // anyway, and a dishonest server cannot hold it off for ever.
    if let Some(info) = report.vault.as_ref() {
        let waits = db.with_conn(|conn| {
            let seen = state::rekey_seen(conn)?;
            state::pass_waits(conn, (info.rotation > seen).then_some(info.rotation), now)
        });
        match waits {
            Ok(true) => {
                log::debug!("[SYNC] The daily pass of documents and photos waits for a key pickup");
                return Ok(());
            }
            Ok(false) => {}
            Err(AppError::Auth(_)) => return Ok(()),
            Err(error) => {
                log::debug!("[SYNC] The daily pass could not read the key epoch: {error}");
                return Ok(());
            }
        }
    }
    match pass::run_if_due(relay, db, creds, blobs, pulled, now).await {
        // A retired token: the round picks the key up (`Trigger::ReadOnly`), as for a push.
        Ok(pass) if pass.retired_token => Err(SyncError::ReadOnlyToken),
        Ok(_) | Err(AppError::Auth(_)) => Ok(()),
        Err(error) => {
            log::warn!("[SYNC] The daily pass of documents and photos failed on this device");
            log::debug!("[SYNC] The daily pass of documents and photos failed: {error}");
            Ok(())
        }
    }
}

/// The round's upload step (attachments design §5.3, ADR 0030): before the push, content of at
/// most `PRE_PUSH_MAX_PLAIN` bytes goes up, so another device that merges a row seldom has to wait
/// for its content. The queue is reconciled by its rows first (content added here since the last
/// merge is queued and counted; the store is asked about new blobs only, never every entry); an
/// upload lane busy with a long transfer skips the rest at once. The whole step ends within
/// `PRE_PUSH_BUDGET`: an upload still running then is dropped and counts a failed attempt, so its
/// backoff keeps the next rounds from spending their budget on it again. Nothing the step meets
/// fails or changes the round: transfer errors stay on their entries, and a local error is
/// logged and dropped (a locked profile ends the round at its next step).
/// In the round's first hold, before any network call: the transfer queue learns of documents and
/// photos added or removed here (`queue::reconcile_rows`: only newly named blobs are looked up in
/// the store), so the status counts them even when the round then cannot reach the server; the
/// app is told when the queue changed. A local failure is logged and the round goes on (the
/// upload step reconciles again); a locked profile ends the round.
fn queue_local_changes(conn: &Connection, blobs: &BlobPolicy, now: i64) -> Result<()> {
    match queue::reconcile_rows(conn, now) {
        Ok(reconciled) => {
            if reconciled != queue::Reconciled::default() {
                blobs.note_changed();
            }
            Ok(())
        }
        Err(error @ AppError::Auth(_)) => Err(error),
        Err(error) => {
            log::warn!("[SYNC] The transfer queue could not be brought up to date");
            log::debug!("[SYNC] The transfer queue could not be brought up to date: {error}");
            Ok(())
        }
    }
}

/// Whether the round leaves content here to upload (`queue::holds_content_to_upload`, the
/// summary's `content_left`: the same definition the status's verdict reads), for the complete
/// sync of step 6. A queue that cannot be read is logged and counts as content left (no
/// complete sync is recorded, the round's success stands); a locked profile ends the round.
fn content_to_upload(conn: &Connection) -> Result<bool> {
    match queue::holds_content_to_upload(conn) {
        Err(error @ AppError::Auth(_)) => Err(error),
        Err(error) => {
            log::warn!("[SYNC] The transfer queue could not be read: no complete sync recorded");
            log::debug!("[SYNC] The transfer queue could not be read: {error}");
            Ok(true)
        }
        held => held,
    }
}

async fn upload_before_push<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    now: i64,
) {
    let until = tokio::time::Instant::now() + PRE_PUSH_BUDGET;
    let uploads = run_uploads_if_free(
        relay,
        db,
        creds,
        policy,
        now,
        Some(until),
        Some(PRE_PUSH_MAX_PLAIN),
    );
    match tokio::time::timeout_at(until, uploads).await {
        Ok(Ok(Some(sent))) if sent.uploaded > 0 || sent.failed > 0 => log::debug!(
            "[SYNC] Before the push: {} documents or photos uploaded, {} failed",
            sent.uploaded,
            sent.failed
        ),
        Ok(Ok(Some(_))) => {}
        Ok(Ok(None)) => log::debug!("[SYNC] The upload lane is busy: the round sends no content"),
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
    if !wants_info(policy, mode, now) {
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
    first.through = first.through.max(later.through);
    first.key_hint = later.key_hint;
    first.removed_here |= later.removed_here;
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
