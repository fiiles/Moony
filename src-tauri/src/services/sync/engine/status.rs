//! What the UI is told about sync (plan's call P17): the phase and the problem, read from the
//! `sync_state` row (the pauses that outlive a restart) and from the engine's memory of its last
//! round (`Runtime`).

use rusqlite::Connection;

use super::{PullEnd, PullReport, PullStop, RoundReport};
use crate::error::{AppError, Result};
use crate::models::sync::{SyncPhase, SyncProblem, SyncStatus};
use crate::services::sync::capture::guard;
use crate::services::sync::error::{SyncError, TooLarge, Transient};
use crate::services::sync::{feature, keys, state};

/// The relay deletes a lapsed vault this long after the subscription ended (spec §11.6).
pub const DELETE_AFTER_LAPSE: i64 = 90 * 86_400;

/// The key a snapshot from a newer app version is refused with (`snapshots::restore_after_410`).
const NEWER_SCHEMA_KEY: &str = "validation.syncSnapshotNewerSchema";

/// The engine's memory of its rounds, in memory only.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Runtime {
    /// A round is running.
    pub running_round: bool,
    /// The problem of the last round (`None` after a clean one).
    pub problem: Option<SyncProblem>,
    /// The subscription's end, as the relay last reported it.
    pub paid_until: Option<i64>,
    /// Rows too large to send, from the last push.
    pub refused_rows: u32,
    /// Rounds finished since the engine started (diagnostics).
    pub rounds: u64,
}

impl Runtime {
    /// Takes in a round that returned a report: its problem (or none), the vault's `paid_until`
    /// when the round read it, and the rows its push refused.
    pub fn after_round(&mut self, report: &RoundReport) {
        self.problem = report.problem;
        if let Some(vault) = &report.vault {
            self.paid_until = Some(vault.paid_until);
        }
        if let Some(push) = &report.push {
            self.refused_rows = u32::try_from(push.refused).unwrap_or(u32::MAX);
        }
    }
}

/// The status of this profile. A profile without a `sync_state` row, or with a disabled one and no
/// account, is `off`; a disabled row with an account saved is `settingUp`. An enabled profile shows
/// the pauses kept in `sync_state` first (capture broken, vault gone), then a running round, then
/// the last round's problem; `idle` when there is none.
pub fn status(conn: &Connection, runtime: &Runtime) -> Result<SyncStatus> {
    let dev_tools = feature::dev_tools();
    let quiet = |phase| SyncStatus {
        phase,
        problem: None,
        last_success_at: None,
        pending_changes: 0,
        held_devices: 0,
        refused_rows: 0,
        paid_until: None,
        deletes_at: None,
        dev_tools,
    };
    let Some(sync) = state::load(conn)? else {
        return Ok(quiet(SyncPhase::Off));
    };
    if !sync.enabled {
        return Ok(quiet(if keys::account(conn)?.is_some() {
            SyncPhase::SettingUp
        } else {
            SyncPhase::Off
        }));
    }
    let marks = state::marks(conn)?
        .ok_or_else(|| AppError::Internal("sync status: sync is not set up".into()))?;
    let paused = if marks.capture_broken_at.is_some() || guard::changes_paused(conn) {
        Some(SyncProblem::CaptureBroken)
    } else if marks.vault_gone_at.is_some() {
        Some(SyncProblem::VaultGone)
    } else {
        None
    };
    let problem = paused.or(runtime.problem);
    let phase = match (paused, problem) {
        (Some(paused), _) => phase_of(paused),
        (None, _) if runtime.running_round => SyncPhase::Syncing,
        (None, Some(problem)) => phase_of(problem),
        (None, None) => SyncPhase::Idle,
    };
    let (pending, held): (i64, i64) = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM sync_outbox) + (SELECT COUNT(*) FROM sync_unsent), \
                (SELECT COUNT(*) FROM sync_heads WHERE quarantined = 1)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let deletes_at = match problem {
        Some(SyncProblem::Lapsed) => runtime.paid_until.map(|at| at + DELETE_AFTER_LAPSE),
        _ => None,
    };
    Ok(SyncStatus {
        phase,
        problem,
        last_success_at: marks.last_success_at,
        pending_changes: count(pending),
        held_devices: count(held),
        refused_rows: runtime.refused_rows,
        paid_until: runtime.paid_until,
        deletes_at,
        dev_tools,
    })
}

fn count(n: i64) -> u32 {
    u32::try_from(n.max(0)).unwrap_or(u32::MAX)
}

/// The phase a problem puts sync in (P17): `waiting` for a gap (a change from another device has
/// not arrived, and this device keeps pulling and pushing: no stop), `paused` while it waits on
/// something outside this device (the vault, the subscription, the quota, an app update, the
/// network, the rate limit), `error` for everything else.
pub fn phase_of(problem: SyncProblem) -> SyncPhase {
    use SyncProblem as P;
    match problem {
        P::Gap => SyncPhase::Waiting,
        P::VaultGone
        | P::Lapsed
        | P::QuotaExceeded
        | P::NewerSchema
        | P::Offline
        | P::RateLimited => SyncPhase::Paused,
        P::CaptureBroken
        | P::SnapshotTooLarge
        | P::Undecryptable
        | P::InvalidChain
        | P::MissingParent
        | P::Quarantined
        | P::RelayError
        | P::Unauthorized
        | P::KeyRotated
        | P::Internal => SyncPhase::Error,
    }
}

/// The problem a failed round shows. A relay answer outside the sync states (`Refused`, a
/// snapshot too large to open) is a relay error; a snapshot from a newer app version says so; any
/// other local failure is internal.
pub fn problem_of(error: &SyncError) -> SyncProblem {
    use SyncProblem as P;
    match error {
        SyncError::Transient(Transient::Offline) => P::Offline,
        SyncError::Transient(Transient::Relay)
        | SyncError::Refused(_)
        | SyncError::SnapshotRequired
        | SyncError::TooLarge(TooLarge::Body) => P::RelayError,
        SyncError::RateLimited => P::RateLimited,
        SyncError::VaultNotFound => P::VaultGone,
        SyncError::Lapsed => P::Lapsed,
        SyncError::Unauthorized => P::Unauthorized,
        SyncError::ReadOnlyToken => P::KeyRotated,
        SyncError::TooLarge(TooLarge::Quota) => P::QuotaExceeded,
        SyncError::TooLarge(TooLarge::Snapshot) => P::SnapshotTooLarge,
        SyncError::Local(AppError::Validation(key)) if key == NEWER_SCHEMA_KEY => P::NewerSchema,
        SyncError::Local(_) => P::Internal,
    }
}

/// The problem a pull leaves: its stop, envelopes waiting for a missing one or a missing parent
/// (`Gap`: this device keeps syncing and pushing), or a quarantined author's held envelopes; `None`
/// for a pull that ended done or unfinished (the next round goes on). `MissingParent` is no longer
/// produced: a missing parent waits like a gap.
pub fn problem_of_pull(pull: &PullReport) -> Option<SyncProblem> {
    use SyncProblem as P;
    match pull.end() {
        PullEnd::Done | PullEnd::Unfinished => None,
        PullEnd::Waiting => Some(P::Gap),
        PullEnd::Held => Some(P::Quarantined),
        PullEnd::Stopped(stop) => Some(match stop {
            PullStop::Invalid => P::InvalidChain,
            PullStop::Undecryptable => P::Undecryptable,
            PullStop::NewerSchema => P::NewerSchema,
        }),
    }
}

/// The fixed `last_error` text of a problem: its wire name, never an error's own text (which may
/// carry a path or a URL). A round whose only problem is a waiting gap records success instead
/// (`run_round_into`), so the `Gap` text is a fallback that says it is no stop.
pub fn reason(problem: SyncProblem) -> &'static str {
    use SyncProblem as P;
    match problem {
        P::CaptureBroken => "sync stopped: captureBroken",
        P::VaultGone => "sync stopped: vaultGone",
        P::Lapsed => "sync stopped: lapsed",
        P::QuotaExceeded => "sync stopped: quotaExceeded",
        P::SnapshotTooLarge => "sync stopped: snapshotTooLarge",
        P::NewerSchema => "sync stopped: newerSchema",
        P::Gap => "sync waiting: gap",
        P::Undecryptable => "sync stopped: undecryptable",
        P::InvalidChain => "sync stopped: invalidChain",
        P::MissingParent => "sync stopped: missingParent",
        P::Quarantined => "sync stopped: quarantined",
        P::Offline => "sync stopped: offline",
        P::RelayError => "sync stopped: relayError",
        P::RateLimited => "sync stopped: rateLimited",
        P::Unauthorized => "sync stopped: unauthorized",
        P::KeyRotated => "sync stopped: keyRotated",
        P::Internal => "sync stopped: internal",
    }
}
