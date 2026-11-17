//! Data upkeep milestones: no safe copy for a month, a property not revalued for a year, and a
//! sync that stalled. The newest safe copy (spec 2026-11-14 §7.1) is the later of the last manual
//! backup and, while sync is on, the last complete sync; the backup reminder follows it, and
//! `sync_protects` gives the verdict the sync status carries (§7.3). Without a sync status (the
//! flag off, or the status could not be read) the backup reminder reads the backup alone, as before.

use rusqlite::{Connection, OptionalExtension};

use super::{milestone, DAY};
use crate::error::Result;
use crate::models::sync::{SyncPhase, SyncProblem, SyncStatus};
use crate::models::Milestone;
use crate::services::loan_amortization::day_floor;
use crate::services::onboarding::FLAG_LAST_BACKUP_AT;

const BACKUP_STALE_DAYS: i64 = 30;
/// A new profile is not asked for its first backup straight away.
const FIRST_BACKUP_GRACE_DAYS: i64 = 14;
const VALUATION_STALE_DAYS: i64 = 365;
/// A sync without a successful round for this many whole UTC days asks for a look (spec 2026-11-14 §6.4).
pub(crate) const SYNC_STALE_DAYS: i64 = 7;
/// Problems the engine retries by itself: the server is out of reach for now.
pub(crate) const TRANSIENT_PROBLEMS: [SyncProblem; 3] = [
    SyncProblem::Offline,
    SyncProblem::RelayError,
    SyncProblem::RateLimited,
];

/// Sync is on for the profile: any phase but `Off` and `SettingUp`.
fn sync_on(sync: &SyncStatus) -> bool {
    !matches!(sync.phase, SyncPhase::Off | SyncPhase::SettingUp)
}

/// The last complete sync while sync is on, unless it is dated after tomorrow (UTC): a clock that
/// was far ahead at one complete round would otherwise count as a fresh safe copy for as long as
/// it was ahead. Up to tomorrow it counts (a clock a little ahead, a time zone), as today's.
fn trusted_complete(today: i64, sync: &SyncStatus) -> Option<i64> {
    sync.last_complete_at
        .filter(|at| sync_on(sync) && day_floor(*at) <= today + DAY)
}

/// The newest safe copy (spec §7.1): the later of the last manual backup and the trusted last
/// complete sync (`trusted_complete`). A backup time is not capped; a time in the future counts as
/// fresh, never as a negative age, since callers compare whole UTC days.
pub(super) fn newest_safe_copy(
    today: i64,
    last_backup: Option<i64>,
    sync: Option<&SyncStatus>,
) -> Option<i64> {
    last_backup.max(sync.and_then(|s| trusted_complete(today, s)))
}

/// Sync protects this device's data now (spec §7.3): on, `sync_stalled` quiet, no problem on
/// record that needs the person (a round running after one keeps it, and `sync_stalled` is quiet
/// while a round runs; only `TRANSIENT_PROBLEMS` may stand), a trusted complete sync
/// (`trusted_complete`, the same one the backup reminder reads, so the two never overlap) on one of
/// the last `SYNC_STALE_DAYS` UTC days, and nothing held here that exists nowhere else: no unsent
/// change (`pending_changes`), no document or photo waiting to upload (`attachments`) and none that
/// cannot or is not queued yet (`content_to_upload`, read from the queue by the caller). The
/// complete time is not lowered by what is made after it, so these "right now" checks are what
/// withdraw the verdict when something new is made here.
pub(crate) fn sync_protects(today: i64, sync: &SyncStatus, content_to_upload: bool) -> bool {
    sync_on(sync)
        && sync_stalled(today, sync).is_none()
        && sync.problem.is_none_or(|p| TRANSIENT_PROBLEMS.contains(&p))
        && trusted_complete(today, sync)
            .is_some_and(|at| day_floor(at) > today - SYNC_STALE_DAYS * DAY)
        && sync.pending_changes == 0
        && sync.attachments.waiting_upload == 0
        && !content_to_upload
}

/// `backup_stale` when the newest safe copy is older than 30 whole UTC days; with neither copy,
/// after the profile's 14 days of grace. Key `backup_stale:<day of the newest safe copy>` or
/// `backup_stale:never`; `since_day` = that day. `sync: None` (flag off, or the status could not
/// be read) is the backup alone, as before sync.
pub(super) fn backup(
    conn: &Connection,
    today: i64,
    sync: Option<&SyncStatus>,
) -> Result<Option<Milestone>> {
    let last_backup: Option<i64> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [FLAG_LAST_BACKUP_AT],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|v| v.trim().parse().ok());
    let last = newest_safe_copy(today, last_backup, sync);
    let since_day = match last {
        Some(at) => {
            let last_day = day_floor(at);
            if last_day >= today - BACKUP_STALE_DAYS * DAY {
                return Ok(None);
            }
            Some(last_day)
        }
        None => {
            let created: Option<i64> =
                conn.query_row("SELECT MIN(created_at) FROM user_profile", [], |r| r.get(0))?;
            match created {
                Some(c) if day_floor(c) <= today - FIRST_BACKUP_GRACE_DAYS * DAY => None,
                _ => return Ok(None),
            }
        }
    };
    let key = format!(
        "backup_stale:{}",
        since_day.map_or_else(|| "never".to_string(), |d| d.to_string())
    );
    let mut m = milestone("backup_stale", key, None, "");
    m.since_day = since_day;
    Ok(Some(m))
}

fn valuations(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT r.id, r.name,
                (SELECT MAX(v.valued_at) FROM real_estate_valuations v WHERE v.real_estate_id = r.id),
                r.purchase_date, r.created_at
         FROM real_estate r ORDER BY r.name",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<i64>>(2)?,
                r.get::<_, Option<i64>>(3)?,
                r.get::<_, i64>(4)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let mut out = Vec::new();
    for (id, name, valued, purchased, created) in rows {
        let last = day_floor(valued.or(purchased).unwrap_or(created));
        if last < today - VALUATION_STALE_DAYS * DAY {
            let mut m = milestone(
                "valuation_stale",
                format!("valuation_stale:{id}:{last}"),
                Some(&id),
                &name,
            );
            m.since_day = Some(last);
            out.push(m);
        }
    }
    Ok(out)
}

/// Sync that stopped or needs the person, or whose server has been out of reach for a week. A
/// problem the engine retries by itself (`TRANSIENT_PROBLEMS`: server out of reach) waits for the
/// week, counted from the last success (or from `enabled_at` without one); anything else asks at
/// once, except while a round is running. A status without such a problem never stalls by time,
/// however old its last success (the app opened after a break, before its first round).
pub(super) fn sync_stalled(today: i64, sync: &SyncStatus) -> Option<Milestone> {
    let at_once = match sync.phase {
        SyncPhase::Off | SyncPhase::SettingUp => return None,
        SyncPhase::Waiting | SyncPhase::Paused | SyncPhase::Error => {
            sync.problem.map_or(sync.phase != SyncPhase::Waiting, |p| {
                !TRANSIENT_PROBLEMS.contains(&p)
            })
        }
        SyncPhase::Idle | SyncPhase::Syncing => false,
    };
    // A last success dated after tomorrow (a clock that was far ahead) is no anchor.
    let success = sync
        .last_success_at
        .filter(|a| day_floor(*a) <= today + DAY);
    let anchor = success.or(sync.enabled_at);
    let out_of_reach = sync
        .problem
        .is_some_and(|p| TRANSIENT_PROBLEMS.contains(&p));
    let by_time =
        out_of_reach && anchor.is_some_and(|a| day_floor(a) <= today - SYNC_STALE_DAYS * DAY);
    if !at_once && !by_time {
        return None;
    }
    let key = format!(
        "sync_stalled:{}",
        anchor.map_or_else(|| "never".to_string(), |a| day_floor(a).to_string())
    );
    let mut m = milestone("sync_stalled", key, None, "");
    m.since_day = sync.last_success_at.map(day_floor);
    Some(m)
}

pub(super) fn build(
    conn: &Connection,
    today: i64,
    sync: Option<&SyncStatus>,
) -> Result<Vec<Milestone>> {
    let mut out = Vec::new();
    if let Some(m) = backup(conn, today, sync)? {
        out.push(m);
    }
    out.extend(valuations(conn, today)?);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{day, setup};
    use super::*;
    use crate::models::sync::{SyncPhase, SyncProblem, SyncStatus};
    use rusqlite::params;

    #[test]
    fn a_backup_older_than_thirty_days_is_stale() {
        let conn = setup();
        let today = day(2026, 10, 5);
        conn.execute(
            "INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)",
            [day(2026, 1, 1)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES ('backup.lastCreatedAt', ?1)",
            [(day(2026, 8, 24) + 3_600).to_string()],
        )
        .unwrap();
        let list = build(&conn, today, None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "backup_stale");
        assert_eq!(list[0].since_day, Some(day(2026, 8, 24)));
        assert_eq!(list[0].key, format!("backup_stale:{}", day(2026, 8, 24)));
        conn.execute(
            "UPDATE app_config SET value = ?1",
            [day(2026, 9, 20).to_string()],
        )
        .unwrap();
        assert!(build(&conn, today, None).unwrap().is_empty());
    }

    #[test]
    fn a_profile_without_a_backup_gets_two_weeks_of_grace() {
        let conn = setup();
        conn.execute(
            "INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)",
            [day(2026, 9, 25)],
        )
        .unwrap();
        assert!(build(&conn, day(2026, 10, 5), None).unwrap().is_empty());
        let list = build(&conn, day(2026, 10, 9), None).unwrap();
        assert_eq!(list[0].kind, "backup_stale");
        assert_eq!(list[0].since_day, None);
        assert_eq!(list[0].key, "backup_stale:never");
    }

    #[test]
    fn a_property_valued_more_than_a_year_ago_is_stale() {
        let conn = setup();
        conn.execute(
            "INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)",
            [day(2026, 10, 1)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO real_estate (id, name, purchase_date, created_at) VALUES
                ('old', 'Byt', ?1, ?1), ('new', 'Chata', NULL, ?2)",
            params![day(2020, 5, 1), day(2026, 1, 1)],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO real_estate_valuations (id, real_estate_id, value, valued_at) VALUES ('v1', 'old', '1', ?1)",
            [day(2025, 3, 12)],
        )
        .unwrap();
        let list = build(&conn, day(2026, 10, 5), None).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "valuation_stale");
        assert_eq!(list[0].source_id.as_deref(), Some("old"));
        assert_eq!(list[0].since_day, Some(day(2025, 3, 12)));
        assert_eq!(
            list[0].key,
            format!("valuation_stale:old:{}", day(2025, 3, 12))
        );
    }

    fn sync(
        phase: SyncPhase,
        problem: Option<SyncProblem>,
        last_success: Option<i64>,
        enabled_at: Option<i64>,
    ) -> SyncStatus {
        SyncStatus {
            phase,
            problem,
            last_success_at: last_success,
            pending_changes: 0,
            held_devices: 0,
            refused_rows: 0,
            paid_until: None,
            deletes_at: None,
            clock_ahead_until: None,
            own_server: false,
            server_address: None,
            server_insecure: false,
            enabled_at,
            attachments: Default::default(),
            last_complete_at: None,
            protects_data: false,
            ..Default::default()
        }
    }

    /// A status whose last complete round (and last success) was at `last_complete`.
    fn complete(
        phase: SyncPhase,
        problem: Option<SyncProblem>,
        last_complete: Option<i64>,
    ) -> SyncStatus {
        SyncStatus {
            last_complete_at: last_complete,
            ..sync(phase, problem, last_complete, None)
        }
    }

    #[test]
    fn sync_off_or_being_set_up_raises_nothing() {
        let today = day(2026, 11, 14);
        for phase in [SyncPhase::Off, SyncPhase::SettingUp] {
            let s = sync(phase, None, Some(day(2026, 1, 1)), Some(day(2026, 1, 1)));
            assert!(sync_stalled(today, &s).is_none());
        }
    }

    #[test]
    fn a_healthy_sync_raises_nothing() {
        let today = day(2026, 11, 14);
        let s = sync(SyncPhase::Idle, None, Some(today + 3_600), None);
        assert!(sync_stalled(today, &s).is_none());
    }

    #[test]
    fn an_unreachable_server_is_quiet_for_six_days_and_stalls_on_the_seventh() {
        let today = day(2026, 11, 14);
        let quiet = sync(
            SyncPhase::Paused,
            Some(SyncProblem::Offline),
            Some(day(2026, 11, 8) + 82_800),
            None,
        );
        assert!(sync_stalled(today, &quiet).is_none());
        let stalled = sync(
            SyncPhase::Paused,
            Some(SyncProblem::Offline),
            Some(day(2026, 11, 7) + 60),
            None,
        );
        let m = sync_stalled(today, &stalled).expect("stalled");
        assert_eq!(m.kind, "sync_stalled");
        assert_eq!(m.since_day, Some(day(2026, 11, 7)));
        assert_eq!(m.key, format!("sync_stalled:{}", day(2026, 11, 7)));
    }

    #[test]
    fn relay_errors_and_rate_limits_count_as_out_of_reach() {
        let today = day(2026, 11, 14);
        for (phase, problem) in [
            (SyncPhase::Error, SyncProblem::RelayError),
            (SyncPhase::Paused, SyncProblem::RateLimited),
        ] {
            let s = sync(phase, Some(problem), Some(day(2026, 11, 12)), None);
            assert!(sync_stalled(today, &s).is_none());
        }
    }

    #[test]
    fn a_problem_that_needs_the_person_stalls_at_once() {
        let today = day(2026, 11, 14);
        let cases = [
            (SyncPhase::Error, Some(SyncProblem::Unauthorized)),
            (SyncPhase::Error, Some(SyncProblem::CaptureBroken)),
            (SyncPhase::Waiting, Some(SyncProblem::Gap)),
            (SyncPhase::Paused, Some(SyncProblem::Lapsed)),
            (SyncPhase::Paused, None),
        ];
        for (phase, problem) in cases {
            let s = sync(phase, problem, Some(today), None);
            assert!(
                sync_stalled(today, &s).is_some(),
                "{phase:?} {problem:?} should stall"
            );
        }
    }

    #[test]
    fn a_running_round_does_not_stall_at_once() {
        let today = day(2026, 11, 14);
        let fresh = sync(
            SyncPhase::Syncing,
            Some(SyncProblem::Lapsed),
            Some(today),
            None,
        );
        assert!(sync_stalled(today, &fresh).is_none());
        let old = sync(
            SyncPhase::Syncing,
            Some(SyncProblem::Lapsed),
            Some(day(2026, 11, 6)),
            None,
        );
        assert!(sync_stalled(today, &old).is_none());
    }

    #[test]
    fn an_old_success_without_a_problem_never_stalls_by_time() {
        let today = day(2026, 11, 14);
        for phase in [SyncPhase::Idle, SyncPhase::Syncing, SyncPhase::Waiting] {
            let s = sync(phase, None, Some(today - 30 * DAY), None);
            assert!(sync_stalled(today, &s).is_none(), "{phase:?}");
        }
    }

    #[test]
    fn a_retry_round_of_an_unreachable_server_keeps_the_reminder() {
        // The top bar shows the running round (`syncState`); the reminder stays so it does not
        // flicker off between rounds.
        let today = day(2026, 11, 14);
        let s = sync(
            SyncPhase::Syncing,
            Some(SyncProblem::Offline),
            Some(today - 30 * DAY),
            None,
        );
        assert!(sync_stalled(today, &s).is_some());
    }

    #[test]
    fn the_seventh_utc_day_of_an_out_of_reach_server_stalls_the_sixth_does_not() {
        let today = day(2026, 11, 14);
        let at = |days: i64| {
            sync(
                SyncPhase::Idle,
                Some(SyncProblem::Offline),
                Some(today - days * DAY + 3_600),
                None,
            )
        };
        assert!(sync_stalled(today, &at(7)).is_some());
        assert!(sync_stalled(today, &at(6)).is_none());
    }

    #[test]
    fn a_device_that_never_synced_counts_from_when_it_joined() {
        let today = day(2026, 11, 14);
        let recent = sync(
            SyncPhase::Paused,
            Some(SyncProblem::Offline),
            None,
            Some(day(2026, 11, 11)),
        );
        assert!(sync_stalled(today, &recent).is_none());
        let old = sync(
            SyncPhase::Paused,
            Some(SyncProblem::Offline),
            None,
            Some(day(2026, 11, 7)),
        );
        let m = sync_stalled(today, &old).expect("stalled");
        assert_eq!(m.since_day, None);
        assert_eq!(m.key, format!("sync_stalled:{}", day(2026, 11, 7)));
    }

    #[test]
    fn without_a_success_or_a_join_record_nothing_stalls_by_time() {
        let today = day(2026, 11, 14);
        let quiet = sync(SyncPhase::Paused, Some(SyncProblem::Offline), None, None);
        assert!(sync_stalled(today, &quiet).is_none());
        let broken = sync(SyncPhase::Error, Some(SyncProblem::Internal), None, None);
        let m = sync_stalled(today, &broken).expect("stalled");
        assert_eq!(m.key, "sync_stalled:never");
    }

    #[test]
    fn a_last_success_in_the_future_does_not_stall() {
        let today = day(2026, 11, 14);
        let s = sync(
            SyncPhase::Paused,
            Some(SyncProblem::Offline),
            Some(day(2026, 12, 1)),
            None,
        );
        assert!(sync_stalled(today, &s).is_none());
    }

    #[test]
    fn a_new_stall_after_a_success_has_a_new_key() {
        let today = day(2026, 12, 30);
        let stall = |last: i64| {
            let s = sync(
                SyncPhase::Paused,
                Some(SyncProblem::Offline),
                Some(last),
                None,
            );
            sync_stalled(today, &s).expect("stalled").key
        };
        assert_ne!(stall(day(2026, 11, 14)), stall(day(2026, 11, 24)));
    }

    // -----------------------------------------------------------------------------------------
    // The newest safe copy (spec 2026-11-14 §7.1) and the verdict (§7.3)

    /// The one profile row, created on `created`.
    fn profile(conn: &Connection, created: i64) {
        conn.execute("DELETE FROM user_profile", []).unwrap();
        conn.execute(
            "INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)",
            [created],
        )
        .unwrap();
    }

    /// The last manual backup at `at`; `None` forgets it.
    fn backed_up(conn: &Connection, at: Option<i64>) {
        conn.execute(
            "DELETE FROM app_config WHERE key = 'backup.lastCreatedAt'",
            [],
        )
        .unwrap();
        if let Some(at) = at {
            conn.execute(
                "INSERT INTO app_config (key, value) VALUES ('backup.lastCreatedAt', ?1)",
                [at.to_string()],
            )
            .unwrap();
        }
    }

    #[test]
    fn without_sync_the_backup_reminder_is_unchanged() {
        // Review Focus 1: no status (flag off, or unreadable) and a status of a sync that is not
        // on (even one carrying a complete time) give today's milestone, key for key.
        let conn = setup();
        let same_as_today = |today: i64| {
            let before = backup(&conn, today, None).unwrap();
            for phase in [SyncPhase::Off, SyncPhase::SettingUp] {
                let s = complete(phase, None, Some(today));
                assert_eq!(backup(&conn, today, Some(&s)).unwrap(), before, "{phase:?}");
            }
            before.map(|m| (m.key, m.since_day))
        };
        // The cases of `a_backup_older_than_thirty_days_is_stale`.
        let today = day(2026, 10, 5);
        profile(&conn, day(2026, 1, 1));
        backed_up(&conn, Some(day(2026, 8, 24) + 3_600));
        assert_eq!(
            same_as_today(today),
            Some((
                format!("backup_stale:{}", day(2026, 8, 24)),
                Some(day(2026, 8, 24))
            ))
        );
        backed_up(&conn, Some(day(2026, 9, 20)));
        assert_eq!(same_as_today(today), None);
        // The cases of `a_profile_without_a_backup_gets_two_weeks_of_grace`.
        profile(&conn, day(2026, 9, 25));
        backed_up(&conn, None);
        assert_eq!(same_as_today(day(2026, 10, 5)), None);
        assert_eq!(
            same_as_today(day(2026, 10, 9)),
            Some(("backup_stale:never".to_string(), None))
        );
    }

    #[test]
    fn a_complete_sync_counts_as_a_safe_copy() {
        let conn = setup();
        let today = day(2026, 10, 5);
        profile(&conn, day(2026, 1, 1));
        backed_up(&conn, Some(day(2026, 8, 24)));
        let recent = complete(SyncPhase::Idle, None, Some(day(2026, 10, 1)));
        assert_eq!(backup(&conn, today, Some(&recent)).unwrap(), None);
        let old = complete(SyncPhase::Idle, None, Some(day(2026, 9, 4)));
        let m = backup(&conn, today, Some(&old)).unwrap().expect("stale");
        assert_eq!(m.kind, "backup_stale");
        assert_eq!(m.since_day, Some(day(2026, 9, 4)));
        assert_eq!(m.key, format!("backup_stale:{}", day(2026, 9, 4)));
    }

    #[test]
    fn the_newer_of_backup_and_sync_wins() {
        let conn = setup();
        let today = day(2026, 10, 5);
        profile(&conn, day(2026, 1, 1));
        backed_up(&conn, Some(day(2026, 9, 20)));
        let older_sync = complete(SyncPhase::Idle, None, Some(day(2026, 8, 1)));
        assert_eq!(backup(&conn, today, Some(&older_sync)).unwrap(), None);
        backed_up(&conn, Some(day(2026, 8, 1)));
        let newer_sync = complete(SyncPhase::Idle, None, Some(day(2026, 8, 2) + 60));
        let m = backup(&conn, today, Some(&newer_sync))
            .unwrap()
            .expect("stale");
        assert_eq!(m.since_day, Some(day(2026, 8, 2)));
        assert_eq!(m.key, format!("backup_stale:{}", day(2026, 8, 2)));
    }

    #[test]
    fn the_thirtieth_utc_day_is_fresh_the_thirty_first_is_not() {
        // Review Focus 3, for either copy.
        let conn = setup();
        let today = day(2026, 10, 5);
        profile(&conn, day(2026, 1, 1));
        let thirty = today - 30 * DAY + 82_800;
        let thirty_one = today - 31 * DAY + 82_800;
        let at = |t: i64| complete(SyncPhase::Idle, None, Some(t));
        assert_eq!(backup(&conn, today, Some(&at(thirty))).unwrap(), None);
        assert!(backup(&conn, today, Some(&at(thirty_one)))
            .unwrap()
            .is_some());
        // The same day count for a backup beside an older complete sync.
        let older = at(today - 60 * DAY);
        backed_up(&conn, Some(thirty));
        assert_eq!(backup(&conn, today, Some(&older)).unwrap(), None);
        backed_up(&conn, Some(thirty_one));
        assert!(backup(&conn, today, Some(&older)).unwrap().is_some());
    }

    #[test]
    fn a_new_profile_with_sync_but_no_complete_round_keeps_its_grace() {
        let conn = setup();
        profile(&conn, day(2026, 9, 25));
        let s = SyncStatus {
            last_success_at: Some(day(2026, 10, 4)),
            ..complete(SyncPhase::Idle, None, None)
        };
        assert_eq!(backup(&conn, day(2026, 10, 5), Some(&s)).unwrap(), None);
        let m = backup(&conn, day(2026, 10, 9), Some(&s))
            .unwrap()
            .expect("grace over");
        assert_eq!(m.key, "backup_stale:never");
        assert_eq!(m.since_day, None);
    }

    #[test]
    fn a_stalled_sync_brings_the_backup_reminder_back_after_thirty_days() {
        // Spec §7.1 consequences: the stall first, then (no backup) the backup reminder.
        let conn = setup();
        let today = day(2026, 10, 5);
        profile(&conn, day(2026, 1, 1));
        let s = complete(
            SyncPhase::Paused,
            Some(SyncProblem::Offline),
            Some(today - 35 * DAY),
        );
        assert!(backup(&conn, today, Some(&s)).unwrap().is_some());
        assert!(sync_stalled(today, &s).is_some());
    }

    #[test]
    fn a_complete_sync_is_trusted_up_to_tomorrow() {
        // Review Focus 4, as ruled in the final review: a complete time up to tomorrow (UTC) is
        // fresh, never a negative age; one dated later is ignored, so one round with a clock far
        // ahead cannot silence the reminder for as long as the clock was ahead. A manual backup
        // time is not capped.
        let conn = setup();
        let today = day(2026, 10, 5);
        profile(&conn, day(2026, 1, 1));
        let tomorrow = complete(SyncPhase::Idle, None, Some(today + DAY + 3_600));
        assert_eq!(backup(&conn, today, Some(&tomorrow)).unwrap(), None);
        assert_eq!(
            newest_safe_copy(today, None, Some(&tomorrow)),
            Some(today + DAY + 3_600)
        );
        assert!(sync_protects(today, &tomorrow, false));

        let ahead = complete(SyncPhase::Idle, None, Some(today + 40 * DAY));
        assert_eq!(newest_safe_copy(today, None, Some(&ahead)), None);
        assert!(!sync_protects(today, &ahead, false));
        // The reminder follows the backup alone: none, an old profile → asked for one.
        let alone = backup(&conn, today, None).unwrap();
        assert!(alone.is_some());
        assert_eq!(backup(&conn, today, Some(&ahead)).unwrap(), alone);
        backed_up(&conn, Some(today - 3 * DAY));
        assert_eq!(backup(&conn, today, Some(&ahead)).unwrap(), None);

        backed_up(&conn, Some(today + 40 * DAY));
        assert_eq!(backup(&conn, today, None).unwrap(), None);
        assert_eq!(backup(&conn, today, Some(&ahead)).unwrap(), None);
    }

    #[test]
    fn a_running_round_does_not_protect_over_a_problem_that_needs_the_person() {
        // A round running after one that ended on such a problem keeps it (`sync_stalled` is
        // quiet while a round runs): the verdict stays withdrawn for the length of the round.
        let today = day(2026, 11, 14);
        for problem in [
            SyncProblem::Unauthorized,
            SyncProblem::Lapsed,
            SyncProblem::Gap,
            SyncProblem::ServerWentBack,
        ] {
            let s = complete(SyncPhase::Syncing, Some(problem), Some(today - 3 * DAY));
            assert!(sync_stalled(today, &s).is_none(), "{problem:?}");
            assert!(!sync_protects(today, &s, false), "{problem:?}");
        }
        // A problem the engine retries by itself (server out of reach) changes nothing.
        for problem in [None].into_iter().chain(TRANSIENT_PROBLEMS.map(Some)) {
            let s = complete(SyncPhase::Syncing, problem, Some(today - 3 * DAY));
            assert!(sync_protects(today, &s, false), "{problem:?}");
        }
    }

    #[test]
    fn a_last_success_dated_after_tomorrow_does_not_hold_off_a_stall() {
        // Out of reach since a clock far ahead stamped the last success: the week counts from
        // when this device joined, as without a success.
        let today = day(2026, 11, 14);
        let s = sync(
            SyncPhase::Paused,
            Some(SyncProblem::Offline),
            Some(today + 40 * DAY),
            Some(today - 40 * DAY),
        );
        let m = sync_stalled(today, &s).expect("stalled");
        assert_eq!(m.key, format!("sync_stalled:{}", today - 40 * DAY));
        let tomorrow = SyncStatus {
            last_success_at: Some(today + DAY + 3_600),
            ..s
        };
        assert!(sync_stalled(today, &tomorrow).is_none());
    }

    #[test]
    fn sync_protects_only_while_complete_within_a_week_and_nothing_needs_the_person() {
        let today = day(2026, 11, 14);
        let cases = [
            (SyncPhase::Idle, None, Some(today), true),
            (SyncPhase::Idle, None, Some(today - 6 * DAY), true),
            (SyncPhase::Idle, None, Some(today - 7 * DAY), false),
            (
                SyncPhase::Paused,
                Some(SyncProblem::Offline),
                Some(today - 2 * DAY),
                true,
            ),
            (
                SyncPhase::Error,
                Some(SyncProblem::Unauthorized),
                Some(today),
                false,
            ),
            (
                SyncPhase::Waiting,
                Some(SyncProblem::Gap),
                Some(today),
                false,
            ),
            (SyncPhase::Idle, None, None, false),
            (SyncPhase::Off, None, Some(today), false),
            (SyncPhase::SettingUp, None, Some(today), false),
            (SyncPhase::Idle, None, Some(today + DAY + 3_600), true),
            (SyncPhase::Idle, None, Some(today + 2 * DAY), false),
            (
                SyncPhase::Syncing,
                Some(SyncProblem::Unauthorized),
                Some(today),
                false,
            ),
        ];
        for (phase, problem, last_complete, expected) in cases {
            let s = complete(phase, problem, last_complete);
            assert_eq!(
                sync_protects(today, &s, false),
                expected,
                "{phase:?} {problem:?} {last_complete:?}"
            );
        }
    }

    #[test]
    fn sync_protects_nothing_while_this_device_holds_something_only_it_has() {
        // A device that completed yesterday and has since made a change, or holds a document or
        // photo to upload (waiting, or stuck: the caller reads that from the queue), is not
        // protected now; its backup reminder still follows the newest safe copy.
        let today = day(2026, 11, 14);
        let yesterday = complete(SyncPhase::Idle, None, Some(today - DAY));
        assert!(sync_protects(today, &yesterday, false));
        let unsent = SyncStatus {
            pending_changes: 1,
            ..yesterday.clone()
        };
        assert!(!sync_protects(today, &unsent, false));
        let mut waiting = yesterday.clone();
        waiting.attachments.waiting_upload = 1;
        assert!(!sync_protects(today, &waiting, false));
        assert!(!sync_protects(today, &yesterday, true));

        let conn = setup();
        profile(&conn, day(2026, 1, 1));
        for s in [&unsent, &waiting] {
            assert_eq!(backup(&conn, today, Some(s)).unwrap(), None);
        }
    }

    #[test]
    fn protection_and_the_backup_reminder_never_overlap() {
        // Review Focus 5: whenever the verdict is "protected", there is no backup reminder.
        let today = day(2026, 11, 14);
        let conn = setup();
        let ages = |days: std::ops::RangeInclusive<i64>| {
            std::iter::once(None)
                .chain(days.map(|d| Some(today - d * DAY + 3_600)))
                .chain([1, 5, 40].map(|d| Some(today + d * DAY + 3_600)))
                .collect::<Vec<_>>()
        };
        let phases = [
            (SyncPhase::Idle, None),
            (SyncPhase::Syncing, None),
            (SyncPhase::Syncing, Some(SyncProblem::Offline)),
            (SyncPhase::Syncing, Some(SyncProblem::Unauthorized)),
            (SyncPhase::Syncing, Some(SyncProblem::Lapsed)),
            (SyncPhase::Syncing, Some(SyncProblem::Gap)),
            (SyncPhase::Syncing, Some(SyncProblem::ServerWentBack)),
            (SyncPhase::Paused, Some(SyncProblem::Offline)),
            (SyncPhase::Error, Some(SyncProblem::RelayError)),
            (SyncPhase::Error, Some(SyncProblem::Unauthorized)),
            (SyncPhase::Waiting, Some(SyncProblem::Gap)),
            (SyncPhase::Paused, None),
            (SyncPhase::Off, None),
            (SyncPhase::SettingUp, None),
        ];
        let enabled = [
            None,
            Some(today),
            Some(today - 3 * DAY),
            Some(today - 40 * DAY),
        ];
        let mut protected = 0;
        let mut reminded = 0;
        for created in [day(2026, 1, 1), today - 5 * DAY] {
            profile(&conn, created);
            for last_backup in [
                None,
                Some(0),
                Some(29),
                Some(30),
                Some(31),
                Some(40),
                Some(-5),
            ] {
                backed_up(&conn, last_backup.map(|d| today - d * DAY + 3_600));
                for last_complete in ages(0..=40) {
                    for (phase, problem) in phases {
                        for enabled_at in enabled {
                            let s = SyncStatus {
                                enabled_at,
                                ..complete(phase, problem, last_complete)
                            };
                            let protects = sync_protects(today, &s, false);
                            let stale = backup(&conn, today, Some(&s)).unwrap().is_some();
                            assert!(
                                !(protects && stale),
                                "{phase:?} {problem:?} complete {last_complete:?} \
                                 backup {last_backup:?} enabled {enabled_at:?}"
                            );
                            protected += i32::from(protects);
                            reminded += i32::from(stale);
                        }
                    }
                }
            }
        }
        // The grid reaches both sides.
        assert!(protected > 0 && reminded > 0, "{protected} {reminded}");
    }
}
