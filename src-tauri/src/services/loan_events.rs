//! Loan events (redesign phase 2): extra payments, rate changes and balance
//! checks. Each event is stored for the detail page's time trace and applies
//! its side effect to the loan in the same transaction, through the existing
//! balance anchor: the amortization simply continues from the
//! balance on the event day. Deleting an event keeps the anchor it set; the UI
//! says so.
//!
//! Known limitation inherited from the anchor: payments fall due on the
//! anchor's day of month, so an event dated mid-month shifts the schedule's
//! due days to that day.

use crate::error::{AppError, Result};
use crate::models::{InsertLoanEvent, LoanEvent};
use crate::services::loan_amortization::{day_floor, outstanding_balance_at};
use crate::services::loans;
use rusqlite::{Connection, OptionalExtension, Row};
use uuid::Uuid;

fn read_row(row: &Row) -> rusqlite::Result<LoanEvent> {
    Ok(LoanEvent {
        id: row.get(0)?,
        loan_id: row.get(1)?,
        kind: row.get(2)?,
        event_day: row.get(3)?,
        amount: row.get(4)?,
        rate: row.get(5)?,
        monthly_payment: row.get(6)?,
        note: row.get(7)?,
        created_at: row.get(8)?,
    })
}

const COLUMNS: &str =
    "id, loan_id, kind, event_day, amount, rate, monthly_payment, note, created_at";

/// Money TEXT with 2 decimals; never "-0.00".
fn money_text(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
    format!("{:.2}", if rounded == 0.0 { 0.0 } else { rounded })
}

/// Events of one loan, oldest first.
pub fn list_events(conn: &Connection, loan_id: &str) -> Result<Vec<LoanEvent>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM loan_events WHERE loan_id = ?1 ORDER BY event_day ASC, created_at ASC"
    ))?;
    let rows = stmt.query_map([loan_id], read_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

fn set_anchor(conn: &Connection, loan_id: &str, amount: f64, day: i64, now: i64) -> Result<()> {
    conn.execute(
        "UPDATE loans SET balance_anchor_amount = ?1, balance_anchor_date = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![money_text(amount.max(0.0)), day, now, loan_id],
    )?;
    Ok(())
}

/// Record an event and apply its effect on the loan. `today` is the valuation
/// day used to read the loan (UTC).
pub fn add_event(conn: &Connection, data: &InsertLoanEvent, today: i64) -> Result<LoanEvent> {
    data.validate()?;
    let loan = loans::get_loan(conn, &data.loan_id, today)?;
    let terms = loans::loan_terms(&loan)?;
    let day = day_floor(data.event_day);
    if day < terms.start_day {
        return Err(AppError::Validation(
            "validation.loanEventBeforeStart".into(),
        ));
    }
    let now = chrono::Utc::now().timestamp();

    match data.kind.as_str() {
        "extra_payment" => {
            let before = outstanding_balance_at(&terms, day);
            set_anchor(conn, &loan.id, before - data.amount_value(), day, now)?;
            // "Snížit splátku": the bank kept the term and lowered the regular payment.
            if let Some(payment) = data
                .monthly_payment
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty())
            {
                conn.execute(
                    "UPDATE loans SET monthly_payment = ?1, updated_at = ?2 WHERE id = ?3",
                    rusqlite::params![payment, now, loan.id],
                )?;
            }
        }
        "rate_change" => {
            let before = outstanding_balance_at(&terms, day);
            let rate = data.rate.as_deref().map(str::trim).unwrap_or("0");
            let payment = data
                .monthly_payment
                .as_deref()
                .map(str::trim)
                .filter(|p| !p.is_empty());
            conn.execute(
                "UPDATE loans SET interest_rate = ?1, monthly_payment = COALESCE(?2, monthly_payment),
                 interest_rate_validity_date = NULL, updated_at = ?3 WHERE id = ?4",
                rusqlite::params![rate, payment, now, loan.id],
            )?;
            set_anchor(conn, &loan.id, before, day, now)?;
        }
        _ => {
            set_anchor(conn, &loan.id, data.amount_value(), day, now)?;
        }
    }

    let id = Uuid::new_v4().to_string();
    let note = data
        .note
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_string);
    let amount = data
        .amount
        .as_deref()
        .map(str::trim)
        .filter(|a| !a.is_empty())
        .map(str::to_string);
    let rate = data
        .rate
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_string);
    let monthly_payment = data
        .monthly_payment
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string);
    conn.execute(
        "INSERT INTO loan_events (id, loan_id, kind, event_day, amount, rate, monthly_payment, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![id, loan.id, data.kind, day, amount, rate, monthly_payment, note, now],
    )?;
    Ok(LoanEvent {
        id,
        loan_id: loan.id,
        kind: data.kind.clone(),
        event_day: day,
        amount,
        rate,
        monthly_payment,
        note,
        created_at: now,
    })
}

/// Remove an event from the trace. The loan keeps the anchor the event set.
pub fn delete_event(conn: &Connection, id: &str) -> Result<()> {
    let exists: Option<String> = conn
        .query_row("SELECT id FROM loan_events WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?;
    if exists.is_none() {
        return Err(AppError::NotFound("Loan event not found".into()));
    }
    conn.execute("DELETE FROM loan_events WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    const DAY: i64 = 86_400;
    /// 2024-01-01 (UTC) as a day number × seconds.
    const START: i64 = 19_723 * DAY;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            CREATE TABLE loans (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, principal TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK', interest_rate TEXT NOT NULL DEFAULT '0',
                interest_rate_validity_date INTEGER, monthly_payment TEXT NOT NULL DEFAULT '0',
                start_date INTEGER NOT NULL DEFAULT (unixepoch()), end_date INTEGER,
                balance_anchor_amount TEXT, balance_anchor_date INTEGER,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()), updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE real_estate_loans (real_estate_id TEXT NOT NULL, loan_id TEXT NOT NULL, PRIMARY KEY (real_estate_id, loan_id));
            CREATE TABLE loan_events (
                id TEXT PRIMARY KEY,
                loan_id TEXT NOT NULL REFERENCES loans(id) ON DELETE CASCADE,
                kind TEXT NOT NULL CHECK (kind IN ('extra_payment', 'rate_change', 'balance_check')),
                event_day INTEGER NOT NULL, amount TEXT, rate TEXT, monthly_payment TEXT, note TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            "#,
        )
        .expect("schema");
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date) VALUES ('l1', 'Mortgage', '1000000', '0', '10000', ?1)",
            [START],
        )
        .expect("seed loan");
        conn
    }

    fn event(kind: &str, day: i64) -> InsertLoanEvent {
        InsertLoanEvent {
            loan_id: "l1".into(),
            kind: kind.into(),
            event_day: day,
            amount: None,
            rate: None,
            monthly_payment: None,
            note: None,
        }
    }

    fn anchor(conn: &Connection) -> (Option<String>, Option<i64>) {
        conn.query_row(
            "SELECT balance_anchor_amount, balance_anchor_date FROM loans WHERE id = 'l1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    #[test]
    fn extra_payment_anchors_the_reduced_balance_on_its_day() {
        let conn = setup_test_db();
        // Zero interest, 10 000 a month: after 12 payments the balance is 880 000.
        let day = START + 366 * DAY; // one year later (2024 is a leap year)
        let mut data = event("extra_payment", day + 3_600);
        data.amount = Some("100000".into());
        let saved = add_event(&conn, &data, day + 30 * DAY).unwrap();
        assert_eq!(saved.event_day, day);
        assert_eq!(saved.amount.as_deref(), Some("100000"));
        let (amount, anchor_day) = anchor(&conn);
        assert_eq!(amount.as_deref(), Some("780000.00"));
        assert_eq!(anchor_day, Some(day));
        assert_eq!(list_events(&conn, "l1").unwrap().len(), 1);
    }

    #[test]
    fn extra_payment_can_lower_the_regular_payment() {
        let conn = setup_test_db();
        let day = START + 366 * DAY;
        let mut data = event("extra_payment", day);
        data.amount = Some("100000".into());
        data.monthly_payment = Some("8500".into());
        add_event(&conn, &data, day).unwrap();
        let payment: String = conn
            .query_row(
                "SELECT monthly_payment FROM loans WHERE id = 'l1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(payment, "8500");
        assert_eq!(anchor(&conn), (Some("780000.00".into()), Some(day)));
    }

    #[test]
    fn rate_change_updates_the_rate_and_continues_from_that_days_balance() {
        let conn = setup_test_db();
        let day = START + 366 * DAY;
        let mut data = event("rate_change", day);
        data.rate = Some("4.1".into());
        data.monthly_payment = Some("15200".into());
        add_event(&conn, &data, day).unwrap();
        let (rate, payment): (String, String) = conn
            .query_row(
                "SELECT interest_rate, monthly_payment FROM loans WHERE id = 'l1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(rate, "4.1");
        assert_eq!(payment, "15200");
        let (amount, anchor_day) = anchor(&conn);
        assert_eq!(amount.as_deref(), Some("880000.00"));
        assert_eq!(anchor_day, Some(day));
    }

    #[test]
    fn balance_check_sets_the_anchor_verbatim() {
        let conn = setup_test_db();
        let day = START + 100 * DAY;
        let mut data = event("balance_check", day);
        data.amount = Some("950000".into());
        data.note = Some("  statement ".into());
        let saved = add_event(&conn, &data, day).unwrap();
        assert_eq!(saved.note.as_deref(), Some("statement"));
        assert_eq!(anchor(&conn), (Some("950000.00".into()), Some(day)));
    }

    #[test]
    fn events_before_the_start_and_bad_kinds_are_rejected() {
        let conn = setup_test_db();
        let mut data = event("extra_payment", START - DAY);
        data.amount = Some("1000".into());
        assert!(matches!(
            add_event(&conn, &data, START),
            Err(AppError::Validation(msg)) if msg == "validation.loanEventBeforeStart"
        ));
        assert!(matches!(
            add_event(&conn, &event("refinance", START), START),
            Err(AppError::Validation(msg)) if msg == "validation.loanEventKindInvalid"
        ));
        assert!(matches!(
            add_event(&conn, &event("extra_payment", START), START),
            Err(AppError::Validation(msg)) if msg == "validation.invalidAmount"
        ));
        assert!(matches!(
            add_event(&conn, &event("rate_change", START), START),
            Err(AppError::Validation(msg)) if msg == "validation.interestRatePositive"
        ));
    }

    #[test]
    fn delete_removes_the_row_but_keeps_the_anchor() {
        let conn = setup_test_db();
        let day = START + 100 * DAY;
        let mut data = event("balance_check", day);
        data.amount = Some("950000".into());
        let saved = add_event(&conn, &data, day).unwrap();
        delete_event(&conn, &saved.id).unwrap();
        assert!(list_events(&conn, "l1").unwrap().is_empty());
        assert_eq!(anchor(&conn).1, Some(day));
        assert!(matches!(
            delete_event(&conn, &saved.id),
            Err(AppError::NotFound(_))
        ));
    }
}
