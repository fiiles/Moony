//! Stock split milestones: a split Moony detected that is still waiting for the user's
//! decision and still has transactions dated before it (D8).

use rusqlite::{Connection, OptionalExtension};

use super::{milestone, TONE_INFO};
use crate::error::Result;
use crate::models::Milestone;
use crate::services::stock_splits;

/// One `info` row per pending split whose ticker has a position. The key is
/// `stock_split:<split id>` without a day: the split stays until applied or dismissed, and a
/// day segment would cap a snooze at a past day.
pub(super) fn build(conn: &Connection, _today: i64) -> Result<Vec<Milestone>> {
    let mut out = Vec::new();
    for split in stock_splits::list_pending(conn)? {
        let investment_id: Option<String> = conn
            .query_row(
                "SELECT id FROM stock_investments WHERE ticker = ?1",
                [&split.ticker],
                |r| r.get(0),
            )
            .optional()?;
        let Some(investment_id) = investment_id else {
            continue;
        };
        let mut m = milestone(
            "stock_split",
            format!("stock_split:{}", split.id),
            Some(&investment_id),
            &split.ticker,
        );
        m.tone = TONE_INFO.to_string();
        m.count = Some(split.pre_split_count);
        m.direction = Some(format!("{}:{}", split.numerator, split.denominator));
        m.action_day = Some(split.effective_day);
        out.push(m);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{day, setup};
    use super::*;

    fn position(conn: &Connection, id: &str, ticker: &str) {
        conn.execute(
            "INSERT INTO stock_investments (id, ticker) VALUES (?1, ?2)",
            [id, ticker],
        )
        .unwrap();
    }

    fn buy(conn: &Connection, ticker: &str, at: i64) {
        conn.execute(
            "INSERT INTO investment_transactions (id, investment_id, ticker, transaction_date)
             VALUES (?1, ?2, ?2, ?3)",
            rusqlite::params![format!("t{ticker}{at}"), ticker, at],
        )
        .unwrap();
    }

    fn split(conn: &Connection, id: &str, ticker: &str, at: i64, status: &str) {
        conn.execute(
            "INSERT INTO stock_splits (id, ticker, effective_day, numerator, denominator, status)
             VALUES (?1, ?2, ?3, '10', '1', ?4)",
            rusqlite::params![id, ticker, at, status],
        )
        .unwrap();
    }

    #[test]
    fn pending_split_with_pre_split_rows_is_listed() {
        let conn = setup();
        position(&conn, "inv-nvda", "NVDA");
        buy(&conn, "NVDA", day(2024, 1, 10));
        buy(&conn, "NVDA", day(2024, 3, 1));
        buy(&conn, "NVDA", day(2024, 7, 1));
        split(&conn, "s1", "NVDA", day(2024, 6, 10), "pending");
        let list = build(&conn, day(2024, 8, 1)).unwrap();
        assert_eq!(list.len(), 1);
        let m = &list[0];
        assert_eq!(m.kind, "stock_split");
        assert_eq!(m.key, "stock_split:s1");
        assert_eq!(m.source_id.as_deref(), Some("inv-nvda"));
        assert_eq!(m.title, "NVDA");
        assert_eq!(m.tone, "info");
        assert_eq!(m.stage, "now");
        assert_eq!(m.count, Some(2));
        assert_eq!(m.direction.as_deref(), Some("10:1"));
        assert_eq!(m.action_day, Some(day(2024, 6, 10)));
        assert_eq!(m.due_day, None);
    }

    #[test]
    fn applied_or_dismissed_or_irrelevant_are_not() {
        let conn = setup();
        for t in ["APPL", "DISM", "NOPRE", "NOPOS"] {
            buy(&conn, t, day(2024, 1, 10));
        }
        for t in ["APPL", "DISM", "NOPRE"] {
            position(&conn, &format!("inv-{t}"), t);
        }
        split(&conn, "a", "APPL", day(2024, 6, 10), "applied");
        split(&conn, "d", "DISM", day(2024, 6, 10), "dismissed");
        // Every transaction is dated on or after the split: nothing to rewrite (D6).
        split(&conn, "n", "NOPRE", day(2023, 6, 10), "pending");
        // A pending split of a ticker without a position is skipped.
        split(&conn, "p", "NOPOS", day(2024, 6, 10), "pending");
        assert!(build(&conn, day(2024, 8, 1)).unwrap().is_empty());
    }

    #[test]
    fn listed_through_list_milestones_and_snoozable() {
        let conn = setup();
        position(&conn, "inv-x", "X");
        buy(&conn, "X", day(2024, 1, 10));
        split(&conn, "s9", "X", day(2024, 6, 10), "pending");
        let today = day(2024, 8, 1);
        assert_eq!(
            super::super::list_milestones(&conn, today).unwrap().len(),
            1
        );
        super::super::set_milestone_state(&conn, "stock_split:s9", "snoozed", today).unwrap();
        assert!(super::super::list_milestones(&conn, today)
            .unwrap()
            .is_empty());
        assert_eq!(
            super::super::list_milestones(&conn, today + 8 * super::super::DAY)
                .unwrap()
                .len(),
            1
        );
    }
}
