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

use crate::error::Result;
use crate::models::sync::{SyncProblem, SyncStatus};
use crate::services::sync::binding::BindingNative;
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
}

impl ConnAccess for crate::db::Database {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        crate::db::Database::with_conn(self, f)
    }
}

/// Why a pull stopped before the relay's end. The cursor stays before the envelope that stopped
/// it, so the next pull meets it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullStop {
    /// An envelope of an author is missing (the relay dropped it).
    Gap,
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
    /// A changeset references a parent this device has never seen; the merge rolled back.
    MissingParent,
}

/// How a pull ended (ruling P1), from `PullReport::end`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PullEnd {
    /// Every envelope up to the relay's end was dealt with.
    Done,
    /// Every envelope was dealt with except a quarantined author's, which are held.
    Held,
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
    /// The profile was restored from a snapshot (`snapshots::restore_after_410`).
    pub restored: bool,
    /// The relay's head is past the last envelope it served: it withholds a trailing envelope (or
    /// served none), so this device is behind. Shown in diagnostics; not an error, and the cursor
    /// never moves to the head.
    pub behind: bool,
    /// The pull reached its page limit (`pull::MAX_PULL_PAGES`) before the relay's head.
    pub unfinished: bool,
}

impl PullReport {
    /// The pull reached the relay's end with nothing stopped or held (P23).
    pub fn done(&self) -> bool {
        self.end() == PullEnd::Done
    }

    /// A stop comes first, then an unfinished pull, then held envelopes.
    pub fn end(&self) -> PullEnd {
        match &self.stop {
            Some(stop) => PullEnd::Stopped(stop.clone()),
            None if self.unfinished => PullEnd::Unfinished,
            None if self.held > 0 => PullEnd::Held,
            None => PullEnd::Done,
        }
    }

    /// Whether the round may push after this pull (ruling P1): the pull ended done, with only a
    /// quarantined author's envelopes held, or stopped only on a newer schema (an older device
    /// keeps pushing, spec §14). An unfinished pull does not push: a device that is behind
    /// catches up before adding more (§9.4).
    pub fn may_push(&self) -> bool {
        matches!(
            self.end(),
            PullEnd::Done | PullEnd::Held | PullEnd::Stopped(PullStop::NewerSchema)
        )
    }

    /// Whether the round may dump and upload a snapshot after this pull (rulings P4, P14): only
    /// after a pull that ended done. Unlike `may_push`, held envelopes and a newer schema rule it
    /// out too: the snapshot's sequence is the cursor this device applied, and a restorer skips
    /// everything up to it.
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

/// One round at a time in this process, whoever runs it (the engine's loop, setup, the repair):
/// two rounds of the profile never run at once, so two `push_all` never interleave (one profile is
/// open at a time). An async lock, never the connection's: it is held across the network.
static ROUNDS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
/// round to finish first (`ROUNDS`).
///
/// 1. A profile without sync, or without credentials, does nothing. A capture-broken profile
///    syncs only in `Drain`; a vault the relay no longer knows (`vault_gone_at`) is not asked.
/// 2. Pull. `410` restores the vault's snapshot by merge, then pulls once more (a second `410` is
///    a relay error).
/// 3. Push only when the pull allows it (ruling P1, `PullReport::may_push`).
/// 4. Not in `Drain`: when the mode asks for it or the policy wants it, read the vault's info,
///    heal a below-barrier miss with it, and, after a pull that ended done (rulings P4, P14) and
///    a second one if the push sent anything, upload a snapshot if due (forced in
///    `FirstSnapshot`).
/// 5. A sync state is a problem, not an error (ruling P8): the vault gone (paused in
///    `sync_state`), a lapse (with the vault's `paid_until` when the relay still answers), the
///    quota, a snapshot too large, a rotated token or an unauthorized one. A transient, rate-limit,
///    refusal or local failure is returned as an error; nothing inside a round is retried, so a
///    round lasts at most one relay timeout past its last answered call.
/// 6. A round with no problem records its success; one with a problem records its fixed reason.
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
    let _one_round = ROUNDS.lock().await;
    let ready = db.with_conn(|conn| {
        if !state::load(conn)?.is_some_and(|sync| sync.enabled) {
            return Ok(None);
        }
        Ok(keys::credentials(conn)?.zip(state::marks(conn)?))
    })?;
    let Some((creds, marks)) = ready else {
        return Ok(());
    };
    if marks.capture_broken_at.is_some() && mode != RoundMode::Drain {
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
    db.with_conn(|conn| match report.problem {
        None => state::record_success(conn, now),
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
            snapshots::restore_after_410(relay, db, creds, now).await?;
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
    let may_push = pulled.may_push();
    report.pull = Some(pulled);
    if !may_push {
        return Ok(());
    }
    let pushed = push::push_all(relay, db, creds).await?;
    let sent = pushed.pushed > 0;
    report.push = Some(pushed);

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
    if sent {
        // The pushed envelopes are acknowledged, so the dump's `S` covers them (ruling P4).
        let again = pull::pull_all(relay, db, creds, now).await?;
        let done = again.may_snapshot();
        if let Some(pull) = report.pull.as_mut() {
            absorb(pull, again);
        }
        if !done {
            return Ok(());
        }
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
        Upload::Outdated => report.snapshot_outdated = true,
        Upload::Skipped => {}
    }
    Ok(())
}

/// Adds a later pull of the same round to the first (which ended done): the counts add up, the
/// later pull's ending stands.
fn absorb(first: &mut PullReport, later: PullReport) {
    first.applied += later.applied;
    first.rejoined |= later.rejoined;
    first.restored |= later.restored;
    first.held = later.held;
    first.stop = later.stop;
    first.behind = later.behind;
    first.unfinished = later.unfinished;
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
