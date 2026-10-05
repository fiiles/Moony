//! Bank account milestones: a term account ending, a promotional rate ending, and one
//! reminder for balances nobody updated for a month.

use rusqlite::Connection;

use super::{dated, milestone, DAY};
use crate::error::Result;
use crate::models::Milestone;
use crate::services::loan_amortization::day_floor;

const TERMINATION_LEAD_DAYS: i64 = 28;
const RATE_END_LEAD_DAYS: i64 = 14;
const STALE_BALANCE_DAYS: i64 = 30;

struct AccountRow {
    id: String,
    name: String,
    termination: Option<i64>,
    rate_end: Option<i64>,
    excluded: bool,
    updated_at: i64,
}

pub(super) fn build(conn: &Connection, today: i64) -> Result<Vec<Milestone>> {
    let mut stmt = conn.prepare(
        "SELECT id, name, termination_date, interest_rate_valid_until, exclude_from_balance, updated_at
         FROM bank_accounts ORDER BY name",
    )?;
    let accounts = stmt
        .query_map([], |r| {
            Ok(AccountRow {
                id: r.get(0)?,
                name: r.get(1)?,
                termination: r.get(2)?,
                rate_end: r.get(3)?,
                excluded: r.get::<_, i32>(4)? != 0,
                updated_at: r.get(5)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let mut out = Vec::new();
    let mut stale = 0_i64;
    let mut oldest: Option<i64> = None;
    for a in accounts {
        let termination = a.termination.map(day_floor);
        if termination.is_some_and(|t| t < today) {
            continue;
        }
        if let Some(t) = termination {
            if let Some(m) = dated(
                "account_termination",
                &a.id,
                &a.name,
                t,
                t - TERMINATION_LEAD_DAYS * DAY,
                today,
            ) {
                out.push(m);
            }
        }
        if let Some(r) = a
            .rate_end
            .map(day_floor)
            .filter(|r| *r >= today && !termination.is_some_and(|t| t < *r))
        {
            if let Some(m) = dated(
                "savings_rate_end",
                &a.id,
                &a.name,
                r,
                r - RATE_END_LEAD_DAYS * DAY,
                today,
            ) {
                out.push(m);
            }
        }
        let updated = day_floor(a.updated_at);
        if !a.excluded && updated < today - STALE_BALANCE_DAYS * DAY {
            stale += 1;
            oldest = Some(oldest.map_or(updated, |o| o.min(updated)));
        }
    }
    if stale > 0 {
        let mut m = milestone("balances_stale", "balances_stale".to_string(), None, "");
        m.count = Some(stale);
        m.since_day = oldest;
        m.can_dismiss = false;
        out.push(m);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{day, setup};
    use super::super::STAGE_NOW;
    use super::*;
    use rusqlite::params;

    fn account(
        conn: &Connection,
        id: &str,
        termination: Option<i64>,
        rate_end: Option<i64>,
        exclude: bool,
        updated: i64,
    ) {
        conn.execute(
            "INSERT INTO bank_accounts (id, name, termination_date, interest_rate_valid_until, exclude_from_balance, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![id, format!("Účet {id}"), termination, rate_end, exclude as i32, updated],
        )
        .expect("account");
    }

    #[test]
    fn termination_and_promo_rate_end_remind_ahead() {
        let conn = setup();
        let today = day(2026, 10, 5);
        account(&conn, "t", Some(day(2026, 10, 30)), None, false, today);
        account(&conn, "r", None, Some(day(2026, 10, 15)), false, today);
        let list = build(&conn, today).unwrap();
        let mut kinds: Vec<_> = list.iter().map(|m| m.kind.as_str()).collect();
        kinds.sort_unstable();
        assert_eq!(kinds, vec!["account_termination", "savings_rate_end"]);
        assert!(list.iter().all(|m| m.stage == STAGE_NOW));
    }

    #[test]
    fn stale_balances_are_one_item_without_excluded_or_terminated_accounts() {
        let conn = setup();
        let today = day(2026, 10, 5);
        account(&conn, "a", None, None, false, day(2026, 8, 1));
        account(&conn, "b", None, None, false, day(2026, 7, 1));
        account(&conn, "fresh", None, None, false, day(2026, 9, 20));
        account(&conn, "excluded", None, None, true, day(2026, 1, 1));
        account(
            &conn,
            "closed",
            Some(day(2026, 5, 1)),
            None,
            false,
            day(2026, 1, 1),
        );
        let list = build(&conn, today).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].kind, "balances_stale");
        assert_eq!(list[0].count, Some(2));
        assert_eq!(list[0].since_day, Some(day(2026, 7, 1)));
        assert!(!list[0].can_dismiss);
    }
}
