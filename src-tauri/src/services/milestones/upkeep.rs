//! Data upkeep milestones: a backup older than a month and a property not revalued for a year.

use rusqlite::{Connection, OptionalExtension};

use super::{milestone, DAY};
use crate::error::Result;
use crate::models::Milestone;
use crate::services::loan_amortization::day_floor;
use crate::services::onboarding::FLAG_LAST_BACKUP_AT;

const BACKUP_STALE_DAYS: i64 = 30;
/// A new profile is not asked for its first backup straight away.
const FIRST_BACKUP_GRACE_DAYS: i64 = 14;
const VALUATION_STALE_DAYS: i64 = 365;

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
    let mut m = milestone("backup_stale", "backup_stale".to_string(), None, "");
    m.since_day = since_day;
    m.can_dismiss = false;
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
                format!("valuation_stale:{id}"),
                Some(&id),
                &name,
            );
            m.since_day = Some(last);
            m.can_dismiss = false;
            out.push(m);
        }
    }
    Ok(out)
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
    }
}
