//! What the UI is told about sync (plan's call P17): the phase and the problem, read from the
//! `sync_state` row (the pauses that outlive a restart) and from the engine's memory of its last
//! round (`Runtime`).

use rusqlite::Connection;

use super::{PullEnd, PullReport, PullStop, RoundReport};
use crate::error::{AppError, Result};
use crate::models::sync::{
    SyncAttachments, SyncAttachmentsProblem, SyncPhase, SyncProblem, SyncStatus,
};
use crate::services::loan_amortization::today_utc_day;
use crate::services::milestones::sync_protects;
use crate::services::sync::blobs::queue;
use crate::services::sync::capture::{self, guard};
use crate::services::sync::error::{SyncError, TooLarge, Transient};
use crate::services::sync::hlc::{now_ms, unpack, FUTURE_WARNING_MS};
use crate::services::sync::relay::http::{
    is_default_relay, is_insecure_address, normalize_base_url,
};
use crate::services::sync::{devices, feature, keys, snapshot, state};

/// The relay deletes a lapsed vault this long after the subscription ended (spec §11.6).
pub const DELETE_AFTER_LAPSE: i64 = 90 * 86_400;
/// The last second of year 9999: the latest instant the UI can show as a date.
pub(crate) const MAX_DISPLAY_SECS: i64 = 253_402_300_799;

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
    /// Why documents and photos do not move (`BlobPolicy::problem`), as the engine last read it.
    pub attachments_problem: Option<SyncAttachmentsProblem>,
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
/// the last round's problem; `idle` when there is none. Documents and photos are counted beside
/// the phase (`SyncStatus::attachments`, zeros while sync is off), never in it. The last complete
/// sync and the verdict on it (`milestones::sync_protects`) are `None` and `false` while off.
pub fn status(conn: &Connection, runtime: &Runtime) -> Result<SyncStatus> {
    let dev_tools = feature::dev_tools();
    let address = keys::account(conn)?.map(|(url, _)| url);
    let own_server = address.as_deref().is_some_and(|url| !is_default_relay(url));
    let server_insecure = address
        .as_deref()
        .and_then(|url| normalize_base_url(url).ok())
        .is_some_and(|url| is_insecure_address(&url));
    let quiet = |phase| SyncStatus {
        phase,
        problem: None,
        last_success_at: None,
        pending_changes: 0,
        held_devices: 0,
        refused_rows: 0,
        paid_until: None,
        deletes_at: None,
        clock_ahead_until: None,
        dev_tools,
        own_server,
        server_address: address.clone(),
        server_insecure,
        enabled_at: None,
        attachments: SyncAttachments::default(),
        last_complete_at: None,
        protects_data: false,
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
    let problem = went_back(conn, paused.or(runtime.problem))?;
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
    // A date past year 9999 has no valid `Date` in the UI (a hostile `paidUntil` such as 3e15): it
    // is shown as none, for the deletion date and for `paid_until` alike. Self-hosted servers
    // report year 9000 to 9999, which stays.
    let displayable = |at: i64| (0..=MAX_DISPLAY_SECS).contains(&at);
    let paid_until = runtime.paid_until.filter(|at| displayable(*at));
    // Blocking on a person's own server never deletes the vault (spec §5.4).
    let deletes_at = match problem {
        Some(SyncProblem::Lapsed) if !own_server => paid_until
            .and_then(|at| at.checked_add(DELETE_AFTER_LAPSE))
            .filter(|at| displayable(*at)),
        _ => None,
    };
    // One pass over the attachment rows (`queue::summary`) gives the figures and the verdict's
    // "content left"; an error is logged once and read by both as below.
    let summary = queue::summary(conn)
        .inspect_err(|error| {
            log::warn!("[SYNC] Documents and photos could not be counted for the status: {error}");
        })
        .ok();
    let mut shown = SyncStatus {
        phase,
        problem,
        last_success_at: marks.last_success_at,
        pending_changes: count(pending),
        held_devices: count(held),
        refused_rows: runtime.refused_rows,
        paid_until,
        deletes_at,
        clock_ahead_until: clock_ahead_until(conn, now_ms())?.filter(|at| displayable(*at)),
        dev_tools,
        own_server,
        server_address: address,
        server_insecure,
        enabled_at: devices::this_device_joined_at(conn)?,
        attachments: attachments(summary.as_ref(), runtime),
        last_complete_at: marks.last_complete_at,
        protects_data: false,
    };
    // What is left to upload comes from the same pass as the figures; a summary that cannot be
    // read counts as content left, so the verdict is never "protected" on an error.
    let content_left = summary.as_ref().is_none_or(|s| s.content_left);
    shown.protects_data = sync_protects(today_utc_day(), &shown, content_left);
    Ok(shown)
}

/// Where this device's documents and photos stand (`queue::summary`, read from the queue alone),
/// with the problem the engine last read from its blob policy. A summary that could not be read
/// (`None`) never hides the ordinary status: its figures are empty.
fn attachments(summary: Option<&queue::AttachmentSummary>, runtime: &Runtime) -> SyncAttachments {
    let summary = summary.cloned().unwrap_or_default();
    SyncAttachments {
        waiting_upload: summary.waiting_upload,
        waiting_upload_bytes: summary.waiting_upload_bytes,
        not_here: summary.not_here,
        not_on_server: summary.not_on_server,
        failed: summary.failed,
        not_on_device: summary.not_on_device,
        failed_upload: summary.failed_upload,
        problem: runtime.attachments_problem,
    }
}

/// While this device's clock (`capture::clock_now`) is more than `hlc::FUTURE_WARNING_MS` past
/// `now_ms`: the date it stands at (unix seconds), which every change this device makes carries
/// until then (the local tick is the larger of the wall clock and the clock's high mark). Its own
/// wall clock was ahead once, or it applied a change from a device whose clock was (audit M6).
pub fn clock_ahead_until(conn: &Connection, now_ms: u64) -> Result<Option<i64>> {
    let ms = unpack(capture::clock_now(conn)?).0;
    if ms <= now_ms.saturating_add(FUTURE_WARNING_MS) {
        return Ok(None);
    }
    Ok(i64::try_from(ms / 1000).ok())
}

/// `problem`, unless the sync server went back (`state::vault_went_back_at`): then
/// `ServerWentBack` stands for no problem, a waiting gap or change, a relay error and an invalid
/// chain (the head below this device's place is one), in every round until a replace or turning
/// sync off. Problems that say
/// what to do about something else (a pause, a lapse, a stop) keep their place.
pub fn went_back(conn: &Connection, problem: Option<SyncProblem>) -> Result<Option<SyncProblem>> {
    let outranked = matches!(
        problem,
        None | Some(SyncProblem::Gap)
            | Some(SyncProblem::ClockAhead)
            | Some(SyncProblem::InvalidChain)
            | Some(SyncProblem::RelayError)
    );
    if outranked && state::vault_went_back_at(conn)?.is_some() {
        return Ok(Some(SyncProblem::ServerWentBack));
    }
    Ok(problem)
}

/// Changes recorded but not yet sent: the outbox plus the sealed envelopes waiting for the server
/// (what `SyncStatus::pending_changes` counts).
pub fn unsent_changes(conn: &Connection) -> Result<u32> {
    let n: i64 = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM sync_outbox) + (SELECT COUNT(*) FROM sync_unsent)",
        [],
        |r| r.get(0),
    )?;
    Ok(count(n))
}

fn count(n: i64) -> u32 {
    u32::try_from(n.max(0)).unwrap_or(u32::MAX)
}

/// The phase a problem puts sync in (P17): `waiting` for a gap (a change from another device has
/// not arrived, and this device keeps pulling and pushing: no stop) and for a change dated more
/// than ten years ahead (held until it is skipped), `paused` while it waits on
/// something outside this device (the vault, the subscription, the quota, an app update, the
/// network, the rate limit), `error` for everything else.
pub fn phase_of(problem: SyncProblem) -> SyncPhase {
    use SyncProblem as P;
    match problem {
        P::Gap | P::ClockAhead => SyncPhase::Waiting,
        P::VaultGone
        | P::Lapsed
        | P::QuotaExceeded
        | P::NewerSchema
        | P::Offline
        | P::RateLimited
        | P::SecureStorage => SyncPhase::Paused,
        P::CaptureBroken
        | P::SnapshotTooLarge
        | P::Undecryptable
        | P::InvalidChain
        | P::MissingParent
        | P::Quarantined
        | P::RelayError
        | P::ServerWentBack
        | P::Unauthorized
        | P::KeyRotated
        | P::Internal => SyncPhase::Error,
    }
}

/// The problem a failed round shows. A relay answer outside the sync states (`Refused`, a
/// snapshot too large to open) is a relay error; a snapshot from a newer app version says so; one
/// dated more than ten years ahead failed verification (`InvalidChain`); any other local failure
/// is internal.
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
        // A snapshot dated more than ten years ahead failed this device's check (audit M6).
        SyncError::Local(AppError::Validation(key)) if key == snapshot::CLOCK_AHEAD_KEY => {
            P::InvalidChain
        }
        SyncError::Local(_) => P::Internal,
    }
}

/// The problem a round that failed with `error` shows: `problem_of`, except that a quarantine its
/// pull held outranks a relay error, the network and the rate limit (final review A, I3). A relay
/// that refuses a later step (the push, the echo pull, the vault's info, the snapshot) must not
/// hide a forked author, which only this problem shows.
pub fn problem_of_round(error: &SyncError, pull: Option<&PullReport>) -> SyncProblem {
    use SyncProblem as P;
    match problem_of(error) {
        P::RelayError | P::Offline | P::RateLimited if pull.is_some_and(|p| p.held > 0) => {
            P::Quarantined
        }
        problem => problem,
    }
}

/// The problem a pull leaves: its stop, envelopes waiting for a missing one or a missing parent
/// (`Gap`: this device keeps syncing and pushing), a quarantined author's held envelopes, or
/// changes dated too far ahead (`ClockAhead`, also no stop); `None` for a pull that ended done,
/// or unfinished with nothing held (the next round goes on).
/// `MissingParent` is no longer produced: a missing parent waits like a gap.
///
/// An unfinished pull that held something says so (`Quarantined`, else `Gap`, else `ClockAhead`):
/// while an envelope
/// is held every pull fetches again from it, so a tail behind it longer than the page limit
/// (`pull::MAX_PULL_PAGES`) ends every pull unfinished at the same place, which is no "up to date"
/// (Task 15 review, minor 8). Skipping the gap (`chain::gaps`) releases it.
pub fn problem_of_pull(pull: &PullReport) -> Option<SyncProblem> {
    use SyncProblem as P;
    match pull.end() {
        PullEnd::Unfinished if pull.held > 0 => Some(P::Quarantined),
        PullEnd::Unfinished if pull.missing_gaps() > 0 => Some(P::Gap),
        PullEnd::Unfinished if pull.ahead > 0 => Some(P::ClockAhead),
        PullEnd::Done | PullEnd::Unfinished => None,
        PullEnd::Waiting => Some(P::Gap),
        PullEnd::Ahead => Some(P::ClockAhead),
        PullEnd::Held => Some(P::Quarantined),
        PullEnd::Stopped(stop) => Some(match stop {
            PullStop::Invalid => P::InvalidChain,
            PullStop::Undecryptable => P::Undecryptable,
            PullStop::NewerSchema => P::NewerSchema,
        }),
    }
}

/// The fixed `last_error` text of a problem: its wire name, never an error's own text (which may
/// carry a path or a URL). A round whose only problem is a waiting gap or a change held for its
/// date records success instead (`run_round_into`), so the `Gap` and `ClockAhead` texts are used
/// only by a stalled round (an unfinished pull that pushed nothing) and say it is no stop.
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
        P::ClockAhead => "sync waiting: clockAhead",
        P::Undecryptable => "sync stopped: undecryptable",
        P::InvalidChain => "sync stopped: invalidChain",
        P::MissingParent => "sync stopped: missingParent",
        P::Quarantined => "sync stopped: quarantined",
        P::Offline => "sync stopped: offline",
        P::RelayError => "sync stopped: relayError",
        P::RateLimited => "sync stopped: rateLimited",
        P::Unauthorized => "sync stopped: unauthorized",
        P::KeyRotated => "sync stopped: keyRotated",
        P::SecureStorage => "sync stopped: secureStorage",
        P::ServerWentBack => "sync stopped: serverWentBack",
        P::Internal => "sync stopped: internal",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::relay::http::DEFAULT_RELAY_URL;
    use crate::services::sync::test_support::migrated;

    fn lapsed() -> Runtime {
        Runtime {
            problem: Some(SyncProblem::Lapsed),
            paid_until: Some(1_800_000_000),
            ..Runtime::default()
        }
    }

    /// An enabled profile with its account stored at `address`.
    fn enabled_at(address: &str) -> Connection {
        let conn = migrated();
        keys::save_account(&conn, address, "4000000000000001").expect("account");
        conn.execute("UPDATE sync_state SET enabled = 1 WHERE id = 1", [])
            .expect("enable");
        conn
    }

    #[test]
    fn an_own_server_lapse_has_no_deletion_date() {
        let conn = enabled_at("https://relay.example.org");
        let shown = status(&conn, &lapsed()).expect("status");
        assert!(shown.own_server);
        assert_eq!(shown.deletes_at, None);
        assert_eq!(shown.paid_until, Some(1_800_000_000));
        assert_eq!(
            shown.server_address.as_deref(),
            Some("https://relay.example.org")
        );
        assert!(!shown.server_insecure);

        let lan = enabled_at("http://192.168.1.20:8787");
        let shown = status(&lan, &lapsed()).expect("status");
        assert!(shown.server_insecure);
        assert!(shown.own_server);

        let cloud = enabled_at(DEFAULT_RELAY_URL);
        let shown = status(&cloud, &lapsed()).expect("status");
        assert!(!shown.own_server);
        assert!(!shown.server_insecure);
        assert_eq!(shown.deletes_at, Some(1_800_000_000 + DELETE_AFTER_LAPSE));
    }

    #[test]
    fn a_profile_with_sync_off_and_no_account_names_no_server() {
        let off = status(&migrated(), &Runtime::default()).expect("status");
        assert_eq!(off.phase, SyncPhase::Off);
        assert!(!off.own_server);
        assert_eq!(off.server_address, None);
        assert!(!off.server_insecure);
        assert_eq!(off.enabled_at, None);
        assert_eq!(off.last_complete_at, None);
        assert!(!off.protects_data);
    }

    #[test]
    fn the_status_carries_the_last_complete_sync_and_its_verdict() {
        let conn = enabled_at("https://relay.example.org");
        let before = status(&conn, &Runtime::default()).expect("status");
        assert_eq!(before.phase, SyncPhase::Idle);
        assert_eq!(before.last_complete_at, None);
        assert!(!before.protects_data);

        let now = chrono::Utc::now().timestamp();
        state::record_complete(&conn, now).expect("complete");
        let after = status(&conn, &Runtime::default()).expect("status");
        assert_eq!(after.last_complete_at, Some(now));
        assert!(after.protects_data);

        // A problem that needs the person withdraws the verdict; the time stays.
        let stopped = Runtime {
            problem: Some(SyncProblem::Unauthorized),
            ..Runtime::default()
        };
        let shown = status(&conn, &stopped).expect("status");
        assert_eq!(shown.last_complete_at, Some(now));
        assert!(!shown.protects_data);

        // Sync off (an account kept): no time, no verdict.
        conn.execute("UPDATE sync_state SET enabled = 0 WHERE id = 1", [])
            .expect("disable");
        let setting_up = status(&conn, &Runtime::default()).expect("status");
        assert_eq!(setting_up.phase, SyncPhase::SettingUp);
        assert_eq!(setting_up.last_complete_at, None);
        assert!(!setting_up.protects_data);
    }

    #[test]
    fn an_enabled_profile_reports_when_this_device_joined() {
        let conn = enabled_at("https://relay.example.org");
        devices::record_this_device(&conn, "MacBook", &[0x01; 32], 1_700_000_000).unwrap();
        let shown = status(&conn, &Runtime::default()).expect("status");
        assert_eq!(shown.enabled_at, Some(1_700_000_000));
    }

    #[test]
    fn setting_up_shows_the_stored_server() {
        let conn = migrated();
        keys::save_account(&conn, "https://relay.example.org", "4000000000000001")
            .expect("account");
        let setting_up = status(&conn, &Runtime::default()).expect("status");
        assert_eq!(setting_up.phase, SyncPhase::SettingUp);
        assert!(setting_up.own_server);
        assert_eq!(
            setting_up.server_address.as_deref(),
            Some("https://relay.example.org")
        );
        assert_eq!(setting_up.enabled_at, None);
    }
}
