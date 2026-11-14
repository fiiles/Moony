//! Data upkeep milestones: a backup older than a month and a property not revalued for a year.

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

fn backup(conn: &Connection, today: i64) -> Result<Option<Milestone>> {
    let last: Option<i64> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [FLAG_LAST_BACKUP_AT],
            |r| r.get::<_, String>(0),
        )
        .optional()?
        .and_then(|v| v.trim().parse().ok());
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

/// Sync that stopped or needs the person, or has not finished a round for a week. A problem the
/// engine retries by itself (server out of reach) waits for the week; anything else asks at once,
/// except while a round is running.
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
    let anchor = sync.last_success_at.or(sync.enabled_at);
    let by_time = anchor.is_some_and(|a| day_floor(a) <= today - SYNC_STALE_DAYS * DAY);
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

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut out = Vec::new();
    if let Some(m) = backup(conn, today)? {
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
        let list = build(&conn, today).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "backup_stale");
        assert_eq!(list[0].since_day, Some(day(2026, 8, 24)));
        assert_eq!(list[0].key, format!("backup_stale:{}", day(2026, 8, 24)));
        conn.execute(
            "UPDATE app_config SET value = ?1",
            [day(2026, 9, 20).to_string()],
        )
        .unwrap();
        assert!(build(&conn, today).unwrap().is_empty());
    }

    #[test]
    fn a_profile_without_a_backup_gets_two_weeks_of_grace() {
        let conn = setup();
        conn.execute(
            "INSERT INTO user_profile (name, created_at) VALUES ('F', ?1)",
            [day(2026, 9, 25)],
        )
        .unwrap();
        assert!(build(&conn, day(2026, 10, 5)).unwrap().is_empty());
        let list = build(&conn, day(2026, 10, 9)).unwrap();
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
        let list = build(&conn, day(2026, 10, 5)).unwrap();
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
            dev_tools: false,
            own_server: false,
            server_address: None,
            server_insecure: false,
            enabled_at,
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
        assert!(sync_stalled(today, &old).is_some());
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
}
