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

use std::path::PathBuf;

use rusqlite::Connection;

use crate::error::{AppError, Result};
use crate::models::sync::{SyncProblem, SyncStatus};
use crate::services::sync::binding::BindingNative;
use crate::services::sync::capture::guard;
use crate::services::sync::error::{SyncError, SyncResult, TooLarge};
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
}

impl ConnAccess for crate::db::Database {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        crate::db::Database::with_conn(self, f)
    }
}

/// Why a pull stopped before the relay's end. The cursor stays before the envelope that stopped
/// it, so the next pull meets it again. A missing envelope or parent is no stop: it is held
/// (`PullReport::gaps`, audit M3).
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
    /// some waiting for an earlier envelope: a quarantine outranks a gap).
    Held,
    /// Every envelope was dealt with except those waiting for an earlier envelope of their author
    /// or for a parent they reference (held; the next pull meets them again, audit M3).
    Waiting,
    /// The pull reached its page limit before the relay's head; the next round goes on.
    Unfinished,
    /// The pull stopped on an envelope (or a page) and went no further.
    Stopped(PullStop),
}

/// What one pull did.
#[derive(Debug, Default)]
pub struct PullReport {
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
    /// then envelopes waiting for an earlier one (gaps): a quarantine outranks a gap, so a forked
    /// author is shown even while another author's envelope is missing.
    pub fn end(&self) -> PullEnd {
        match &self.stop {
            Some(stop) => PullEnd::Stopped(stop.clone()),
            None if self.unfinished => PullEnd::Unfinished,
            None if self.held > 0 => PullEnd::Held,
            None if self.gaps > 0 => PullEnd::Waiting,
            None => PullEnd::Done,
        }
    }

    /// Whether the round may push after this pull (ruling P1): the pull ended done, with only
    /// held envelopes (a quarantined author's, or ones waiting for a missing envelope or parent:
    /// a withheld envelope never stalls this device, audit M3), or stopped only on a newer schema
    /// (an older device keeps pushing, spec §14). An unfinished pull does not push: a device that
    /// is behind catches up before adding more (§9.4).
    pub fn may_push(&self) -> bool {
        matches!(
            self.end(),
            PullEnd::Done
                | PullEnd::Held
                | PullEnd::Waiting
                | PullEnd::Stopped(PullStop::NewerSchema)
        )
    }

    /// Whether the round may dump and upload a snapshot after this pull (rulings P4, P14): only
    /// after a pull that ended done. Unlike `may_push`, held or waiting envelopes and a newer
    /// schema rule it out too: the snapshot's sequence is the cursor this device applied, and a
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
/// 3. Push only when the pull allows it (ruling P1, `PullReport::may_push`). A push that sent
///    anything is followed by a second pull, which brings what it sent back (the echo, audit M4):
///    only that removes an envelope from `sync_unsent`; a failure of that pull is logged and ends
///    the round without an error. After an echo pull that was not stopped or unfinished, the
///    round's problem is a relay error when an envelope sent in an earlier round is still not
///    back, or when sealing stopped at `push::MAX_UNECHOED` and the pull brought none of it back
///    (a pull-stop problem is never replaced).
/// 4. Not in `Drain`: when the mode asks for it or the policy wants it, read the vault's info,
///    heal a below-barrier miss with it, and, after pulls that ended done (rulings P4, P14),
///    upload a snapshot if due (forced in `FirstSnapshot`).
/// 5. A sync state is a problem, not an error (ruling P8): the vault gone (paused in
///    `sync_state`), a lapse (with the vault's `paid_until` when the relay still answers), the
///    quota, a snapshot too large, a rotated token or an unauthorized one. A transient, rate-limit,
///    refusal or local failure is returned as an error; nothing inside a round is retried, so a
///    round lasts at most one relay timeout past its last answered call.
/// 6. A sync server that went back (`state::vault_went_back_at`) is the round's problem in place
///    of none, a gap or an invalid chain (`status::went_back`). A round with no problem, or only
///    a waiting gap (`Gap`), records its success; one with any other problem records its fixed
///    reason.
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
    // stays the problem.
    let stalled = report.push.is_none()
        && report
            .pull
            .as_ref()
            .is_some_and(|pull| pull.end() == PullEnd::Unfinished);
    db.with_conn(|conn| match report.problem {
        Some(SyncProblem::Gap) if stalled => {
            state::record_error(conn, status::reason(SyncProblem::Gap))
        }
        None | Some(SyncProblem::Gap) => state::record_success(conn, now),
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
        // A pull that holds something keeps its own problem (a quarantine, else the gap): the
        // person's way out of a held gap (the skip) is offered for `Gap` only.
        if pulled.held == 0 && pulled.gaps == 0 {
            report.problem = Some(SyncProblem::RelayError);
        }
    }
    report.pull = Some(pulled);
    if !may_push {
        return Ok(());
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
            report.problem = Some(SyncProblem::RelayError);
            return Ok(());
        }
    }
    after_push(relay, db, creds, policy, mode, now, jitter, report).await
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
/// pull's ending stands.
fn absorb(first: &mut PullReport, later: PullReport) {
    first.applied += later.applied;
    first.rejoined |= later.rejoined;
    first.restored |= later.restored;
    first.held = later.held;
    first.gaps = later.gaps;
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
