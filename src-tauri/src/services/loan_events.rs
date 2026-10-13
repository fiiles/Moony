//! Loan events: extra payments, rate changes and manual balance records.
//!
//! The events are the facts of the loan ledger (`loan_amortization::loan_balance_at`
//! walks them); nothing is applied to the loan besides the stored row:
//! - `extra_payment` and `rate_change` store the event only. A rate change (and an
//!   extra payment that lowers the regular payment) also records the terms in
//!   force before it (`previous_rate`, `previous_monthly_payment`) and moves the
//!   loan's current rate / payment to the new ones.
//! - `balance_check` is the manual balance record: it holds exactly from its day
//!   until the user changes or removes it. One record per day; the same day
//!   replaces. `loans.balance_anchor_amount` / `balance_anchor_date` mirror the
//!   newest record (NULL without one) and are written only here
//!   ([`refresh_anchor_mirror`]), like `bank_accounts.balance` mirrors the newest
//!   balance record.
//! - Deleting an event leaves nothing of it behind: the walk is re-derived from the
//!   remaining rows, the mirror follows the newest record and the loan's current
//!   terms return to what the remaining events (or the deleted event's recorded
//!   previous terms) say.

use crate::error::{AppError, Result};
use crate::models::{InsertLoanEvent, LoanEvent};
use crate::services::loan_amortization::{day_floor, event_terms_at, loan_balance_at};
use crate::services::loans;
use rusqlite::{Connection, OptionalExtension, Row};
use uuid::Uuid;

const COLUMNS: &str = "id, loan_id, kind, event_day, amount, rate, monthly_payment, \
     previous_rate, previous_monthly_payment, note, created_at";

fn read_row(row: &Row) -> rusqlite::Result<LoanEvent> {
    Ok(LoanEvent {
        id: row.get(0)?,
        loan_id: row.get(1)?,
        kind: row.get(2)?,
        event_day: row.get(3)?,
        amount: row.get(4)?,
        rate: row.get(5)?,
        monthly_payment: row.get(6)?,
        previous_rate: row.get(7)?,
        previous_monthly_payment: row.get(8)?,
        note: row.get(9)?,
        created_at: row.get(10)?,
    })
}

/// A trimmed text, `None` when absent or blank.
fn clean(text: Option<&str>) -> Option<String> {
    text.map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

/// A rate or payment of the ledger as stored text (`4.1`, `15200`).
fn number_text(value: f64) -> String {
    if value.is_finite() {
        format!("{value}")
    } else {
        "0".to_string()
    }
}

/// Events of one loan, oldest first: by day, then creation time, then insertion
/// (the order the ledger walk applies them in).
pub fn list_events(conn: &Connection, loan_id: &str) -> Result<Vec<LoanEvent>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM loan_events WHERE loan_id = ?1
         ORDER BY event_day ASC, created_at ASC, rowid ASC"
    ))?;
    let rows = stmt.query_map([loan_id], read_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Write the anchor columns of `loans` as a mirror of the newest manual balance
/// record (day, creation time, insertion), NULL when the loan has none.
pub fn refresh_anchor_mirror(conn: &Connection, loan_id: &str, now: i64) -> Result<()> {
    let newest: Option<(Option<String>, i64)> = conn
        .query_row(
            "SELECT amount, event_day FROM loan_events
             WHERE loan_id = ?1 AND kind = 'balance_check'
             ORDER BY event_day DESC, created_at DESC, rowid DESC LIMIT 1",
            [loan_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    let (amount, day) = match newest {
        Some((amount, day)) => (amount, Some(day)),
        None => (None, None),
    };
    conn.execute(
        "UPDATE loans SET balance_anchor_amount = ?1, balance_anchor_date = ?2, updated_at = ?3
         WHERE id = ?4",
        rusqlite::params![amount, day, now, loan_id],
    )?;
    Ok(())
}

/// Create the manual balance record of `day` (UTC midnight is taken from any
/// time of that day); a record of the same day is replaced. Re-mirrors the
/// anchor columns. The caller validates the loan, the amount and the day and
/// owns the transaction.
pub fn add_manual_balance(
    conn: &Connection,
    loan_id: &str,
    day: i64,
    amount: &str,
    note: Option<&str>,
    now: i64,
) -> Result<LoanEvent> {
    let day = day_floor(day);
    let amount = amount.trim().to_string();
    let note = clean(note);
    conn.execute(
        "DELETE FROM loan_events WHERE loan_id = ?1 AND kind = 'balance_check' AND event_day = ?2",
        rusqlite::params![loan_id, day],
    )?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO loan_events (id, loan_id, kind, event_day, amount, note, created_at)
         VALUES (?1, ?2, 'balance_check', ?3, ?4, ?5, ?6)",
        rusqlite::params![id, loan_id, day, amount, note, now],
    )?;
    refresh_anchor_mirror(conn, loan_id, now)?;
    Ok(LoanEvent {
        id,
        loan_id: loan_id.to_string(),
        kind: "balance_check".to_string(),
        event_day: day,
        amount: Some(amount),
        rate: None,
        monthly_payment: None,
        previous_rate: None,
        previous_monthly_payment: None,
        note,
        created_at: now,
    })
}

/// The loan form's manual balance: the record of `day` is created or replaced,
/// older and newer records stay. A record that already says the same is left
/// alone, so saving an unchanged form does not reorder the events of its day.
/// The caller validates and owns the transaction.
pub fn upsert_form_balance(
    conn: &Connection,
    loan_id: &str,
    day: i64,
    amount: &str,
    now: i64,
) -> Result<()> {
    let day = day_floor(day);
    let existing: Option<String> = conn
        .query_row(
            "SELECT amount FROM loan_events
             WHERE loan_id = ?1 AND kind = 'balance_check' AND event_day = ?2
             ORDER BY created_at DESC, rowid DESC LIMIT 1",
            rusqlite::params![loan_id, day],
            |row| row.get(0),
        )
        .optional()?;
    let same = existing
        .as_deref()
        .and_then(|text| text.trim().parse::<f64>().ok())
        .zip(amount.trim().parse::<f64>().ok())
        .is_some_and(|(stored, new)| stored == new);
    if same {
        return refresh_anchor_mirror(conn, loan_id, now);
    }
    add_manual_balance(conn, loan_id, day, amount, None, now).map(|_| ())
}

/// Switch the loan back to the estimate: delete every manual balance record
/// (D8) and clear the mirror. The caller owns the transaction.
pub fn delete_manual_balances(conn: &Connection, loan_id: &str, now: i64) -> Result<()> {
    conn.execute(
        "DELETE FROM loan_events WHERE loan_id = ?1 AND kind = 'balance_check'",
        [loan_id],
    )?;
    refresh_anchor_mirror(conn, loan_id, now)
}

/// A legacy anchor and the estimate must agree within this (currency units) to count as the
/// same balance: rounding, not a value the user set.
const LEGACY_ANCHOR_TOLERANCE: f64 = 1.0;

/// Does the balance a pre-019 anchor held on `day` differ from what the ledger estimates there,
/// events as they are? A loan that is gone, or an amount that is not a number, decides nothing.
fn legacy_anchor_overrides_estimate(
    conn: &Connection,
    loan_id: &str,
    amount: &str,
    day: i64,
) -> Result<bool> {
    let Ok(legacy) = amount.trim().parse::<f64>() else {
        return Ok(false);
    };
    let entry = match loans::load_ledger(conn, loan_id) {
        Ok(entry) => entry,
        Err(AppError::NotFound(_)) => return Ok(false),
        Err(e) => return Err(e),
    };
    let estimate = loan_balance_at(&entry.ledger, day).balance;
    Ok((estimate - legacy).abs() > LEGACY_ANCHOR_TOLERANCE)
}

/// Follow up migration 019 once the database is open. Before 019 an extra payment or a rate
/// change re-anchored the loan, overwriting a balance the user had entered by hand; the
/// migration cannot tell the two apart in SQL, so it keeps every anchor that sits on such an
/// event day in `loan_legacy_anchors` (and the ledger now replays the event from the principal).
/// Each kept anchor is compared with the ledger's estimate at its day: when they differ by more
/// than a currency unit the user had overridden the estimate, and a manual record of the legacy
/// amount is added on that day (a manual value holds, P2); when they agree it was only the
/// estimate and nothing is added. The processed row is deleted, so a later run finds nothing.
/// Returns the number of records added. The caller logs a failure and never blocks on it: the
/// unprocessed rows are retried at the next open.
pub fn reconcile_legacy_anchors(conn: &Connection, now: i64) -> Result<usize> {
    let rows: Vec<(String, String, i64)> = conn
        .prepare("SELECT loan_id, amount, day FROM loan_legacy_anchors ORDER BY loan_id")?
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let mut restored = 0;
    for (loan_id, amount, day) in rows {
        let day = day_floor(day);
        let tx = conn.unchecked_transaction()?;
        if legacy_anchor_overrides_estimate(&tx, &loan_id, &amount, day)? {
            // The anchor was the balance after the day's events: the record must apply after
            // them, whatever the clock says.
            let newest: Option<i64> = tx.query_row(
                "SELECT MAX(created_at) FROM loan_events WHERE loan_id = ?1 AND event_day = ?2",
                rusqlite::params![loan_id, day],
                |row| row.get(0),
            )?;
            let created_at = newest.map_or(now, |newest| now.max(newest + 1));
            add_manual_balance(&tx, &loan_id, day, &amount, None, created_at)?;
            restored += 1;
        }
        tx.execute(
            "DELETE FROM loan_legacy_anchors WHERE loan_id = ?1",
            [&loan_id],
        )?;
        tx.commit()?;
    }
    Ok(restored)
}

/// Move the loan's current rate and / or payment to what the ledger holds after
/// its last event, for the fields an event just added or removed carries. A
/// backdated event therefore leaves the current terms of a later one alone.
fn sync_current_terms(
    conn: &Connection,
    loan_id: &str,
    rate: bool,
    payment: bool,
    now: i64,
) -> Result<()> {
    if !rate && !payment {
        return Ok(());
    }
    let entry = loans::load_ledger(conn, loan_id)?;
    let Some(last_day) = entry.ledger.events.iter().map(|event| event.day).max() else {
        return Ok(());
    };
    let terms = &entry.ledger.terms;
    // The chain of events alone: the walk itself runs its last segment on the very columns
    // this function is about to move. An event dated before the start is still applied.
    let (event_rate, event_payment) = event_terms_at(&entry.ledger, last_day.max(terms.start_day));
    if rate && event_rate != terms.annual_rate_pct {
        conn.execute(
            "UPDATE loans SET interest_rate = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![number_text(event_rate), now, loan_id],
        )?;
    }
    if payment && event_payment != terms.monthly_payment {
        conn.execute(
            "UPDATE loans SET monthly_payment = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![number_text(event_payment), now, loan_id],
        )?;
    }
    Ok(())
}

/// Record an event. `today` is the valuation day used to read the loan (UTC).
///
/// - `balance_check`: a manual balance record ([`add_manual_balance`]).
/// - `rate_change`: stores the rate (and payment) with the terms in force on its
///   day, clears the fixation date and moves the loan's current terms.
/// - `extra_payment`: stores the payment; a new regular payment ("Snížit splátku")
///   is stored with the one it replaced and becomes the loan's current payment.
///
/// None of them writes the anchor columns except through the manual record.
pub fn add_event(conn: &Connection, data: &InsertLoanEvent, today: i64) -> Result<LoanEvent> {
    data.validate()?;
    let loan = loans::get_loan(conn, &data.loan_id, today)?;
    let day = day_floor(data.event_day);
    if day < day_floor(loan.start_date) {
        return Err(AppError::Validation(
            "validation.loanEventBeforeStart".into(),
        ));
    }
    let now = chrono::Utc::now().timestamp();
    let note = clean(data.note.as_deref());
    let amount = clean(data.amount.as_deref());

    let tx = conn.unchecked_transaction()?;
    let saved = if data.kind == "balance_check" {
        add_manual_balance(
            &tx,
            &loan.id,
            day,
            amount.as_deref().unwrap_or("0"),
            note.as_deref(),
            now,
        )?
    } else {
        let rate = clean(data.rate.as_deref());
        let monthly_payment = clean(data.monthly_payment.as_deref());
        let is_rate_change = data.kind == "rate_change";
        // The terms in force on the event's day, before this event.
        let in_force = loan_balance_at(&loans::loan_ledger(&loan)?, day);
        let previous_rate = is_rate_change.then(|| number_text(in_force.annual_rate_pct));
        let previous_monthly_payment = (is_rate_change || monthly_payment.is_some())
            .then(|| number_text(in_force.monthly_payment));
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO loan_events (id, loan_id, kind, event_day, amount, rate, monthly_payment,
             previous_rate, previous_monthly_payment, note, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                id,
                loan.id,
                data.kind,
                day,
                amount,
                rate,
                monthly_payment,
                previous_rate,
                previous_monthly_payment,
                note,
                now
            ],
        )?;
        if is_rate_change {
            tx.execute(
                "UPDATE loans SET interest_rate_validity_date = NULL, updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now, loan.id],
            )?;
        }
        sync_current_terms(
            &tx,
            &loan.id,
            is_rate_change,
            monthly_payment.is_some(),
            now,
        )?;
        LoanEvent {
            id,
            loan_id: loan.id.clone(),
            kind: data.kind.clone(),
            event_day: day,
            amount,
            rate,
            monthly_payment,
            previous_rate,
            previous_monthly_payment,
            note,
            created_at: now,
        }
    };
    tx.commit()?;
    Ok(saved)
}

/// Remove an event. Nothing of it stays: a manual record re-mirrors the anchor
/// columns, and an event that changed the loan's rate or payment hands the
/// terms it replaced on and returns the loan's current terms to what is left. Returns
/// the day of the removed event: the first day the loan's history changes.
pub fn delete_event(conn: &Connection, id: &str) -> Result<i64> {
    let event: LoanEvent = conn
        .query_row(
            &format!("SELECT {COLUMNS} FROM loan_events WHERE id = ?1"),
            [id],
            read_row,
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("Loan event not found".into()))?;
    let now = chrono::Utc::now().timestamp();
    let events = list_events(conn, &event.loan_id)?;
    let position = events.iter().position(|other| other.id == event.id);

    let tx = conn.unchecked_transaction()?;
    // The ledger starts from the previous terms of the earliest event that
    // carries them. When that is this event, the next carrier takes them over,
    // or, with none left, the loan row itself becomes the source.
    for is_rate in [true, false] {
        let previous = |e: &LoanEvent| {
            if is_rate {
                e.previous_rate.clone()
            } else {
                e.previous_monthly_payment.clone()
            }
        };
        let Some(value) = previous(&event) else {
            continue;
        };
        if events.iter().position(|e| previous(e).is_some()) != position {
            continue;
        }
        let next =
            position.and_then(|at| events.iter().skip(at + 1).find(|e| previous(e).is_some()));
        match next {
            Some(next) => {
                let column = if is_rate {
                    "previous_rate"
                } else {
                    "previous_monthly_payment"
                };
                tx.execute(
                    &format!("UPDATE loan_events SET {column} = ?1 WHERE id = ?2"),
                    rusqlite::params![value, next.id],
                )?;
            }
            None => {
                let carries_new = if is_rate {
                    event.kind == "rate_change"
                } else {
                    event.monthly_payment.is_some()
                };
                if carries_new {
                    let column = if is_rate {
                        "interest_rate"
                    } else {
                        "monthly_payment"
                    };
                    tx.execute(
                        &format!("UPDATE loans SET {column} = ?1, updated_at = ?2 WHERE id = ?3"),
                        rusqlite::params![value, now, event.loan_id],
                    )?;
                }
            }
        }
    }
    tx.execute("DELETE FROM loan_events WHERE id = ?1", [id])?;
    if event.kind == "balance_check" {
        refresh_anchor_mirror(&tx, &event.loan_id, now)?;
    }
    sync_current_terms(
        &tx,
        &event.loan_id,
        event.kind == "rate_change",
        event.monthly_payment.is_some(),
        now,
    )?;
    tx.commit()?;
    Ok(event.event_day)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::loan_amortization::LedgerMode;
    use rusqlite::Connection;

    const DAY: i64 = 86_400;
    /// 2024-01-01 (UTC) as a day number × seconds.
    const START: i64 = 19_723 * DAY;
    /// One year later (2024 is a leap year): 2025-01-01, twelve payments in.
    const YEAR_ON: i64 = START + 366 * DAY;

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
            CREATE TABLE loan_legacy_anchors (
                loan_id TEXT PRIMARY KEY, amount TEXT NOT NULL, day INTEGER NOT NULL
            );
            CREATE TABLE loan_events (
                id TEXT PRIMARY KEY,
                loan_id TEXT NOT NULL REFERENCES loans(id) ON DELETE CASCADE,
                kind TEXT NOT NULL CHECK (kind IN ('extra_payment', 'rate_change', 'balance_check')),
                event_day INTEGER NOT NULL, amount TEXT, rate TEXT, monthly_payment TEXT, note TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                previous_rate TEXT, previous_monthly_payment TEXT
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

    fn extra(day: i64, amount: &str) -> InsertLoanEvent {
        let mut data = event("extra_payment", day);
        data.amount = Some(amount.into());
        data
    }

    fn rate_change(day: i64, rate: &str) -> InsertLoanEvent {
        let mut data = event("rate_change", day);
        data.rate = Some(rate.into());
        data
    }

    fn record(day: i64, amount: &str) -> InsertLoanEvent {
        let mut data = event("balance_check", day);
        data.amount = Some(amount.into());
        data
    }

    fn anchor(conn: &Connection) -> (Option<String>, Option<i64>) {
        conn.query_row(
            "SELECT balance_anchor_amount, balance_anchor_date FROM loans WHERE id = 'l1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    fn terms(conn: &Connection) -> (String, String) {
        conn.query_row(
            "SELECT interest_rate, monthly_payment FROM loans WHERE id = 'l1'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    /// Balance and mode of the loan on `day`, from the ledger.
    fn balance_on(conn: &Connection, day: i64) -> (String, LedgerMode) {
        let loan = loans::get_loan(conn, "l1", day).unwrap();
        let state = loan_balance_at(&loans::loan_ledger(&loan).unwrap(), day);
        (loan.outstanding_balance, state.mode)
    }

    #[test]
    fn extra_payment_is_stored_and_never_writes_the_anchor() {
        let conn = setup_test_db();
        // Zero interest, 10 000 a month: after 12 payments the balance is 880 000.
        let saved =
            add_event(&conn, &extra(YEAR_ON + 3_600, "100000"), YEAR_ON + 30 * DAY).unwrap();
        assert_eq!(saved.event_day, YEAR_ON);
        assert_eq!(saved.amount.as_deref(), Some("100000"));
        assert_eq!(saved.previous_monthly_payment, None);
        assert_eq!(list_events(&conn, "l1").unwrap().len(), 1);
        assert_eq!(anchor(&conn), (None, None));
        // The ledger takes the payment off the estimate.
        assert_eq!(
            balance_on(&conn, YEAR_ON - DAY),
            ("890000.00".into(), LedgerMode::Estimated)
        );
        assert_eq!(
            balance_on(&conn, YEAR_ON + 30 * DAY),
            ("780000.00".into(), LedgerMode::Estimated)
        );
    }

    #[test]
    fn extra_payment_can_lower_the_regular_payment_and_remembers_the_old_one() {
        let conn = setup_test_db();
        let mut data = extra(YEAR_ON, "100000");
        data.monthly_payment = Some("8500".into());
        let saved = add_event(&conn, &data, YEAR_ON).unwrap();
        assert_eq!(terms(&conn), ("0".into(), "8500".into()));
        assert_eq!(saved.previous_monthly_payment.as_deref(), Some("10000"));
        assert_eq!(saved.previous_rate, None);
        let stored = list_events(&conn, "l1").unwrap();
        assert_eq!(stored[0].previous_monthly_payment.as_deref(), Some("10000"));
        // Before the event the old payment applies, after it the new one.
        assert_eq!(balance_on(&conn, YEAR_ON - DAY).0, "890000.00");
        assert_eq!(balance_on(&conn, YEAR_ON).0, "780000.00");
        assert_eq!(balance_on(&conn, YEAR_ON + 31 * DAY).0, "771500.00");
        assert_eq!(anchor(&conn), (None, None));
    }

    #[test]
    fn rate_change_records_the_previous_terms_and_moves_the_current_ones() {
        let conn = setup_test_db();
        conn.execute(
            "UPDATE loans SET interest_rate_validity_date = ?1 WHERE id = 'l1'",
            [YEAR_ON],
        )
        .unwrap();
        let mut data = rate_change(YEAR_ON, "4.1");
        data.monthly_payment = Some("15200".into());
        let saved = add_event(&conn, &data, YEAR_ON).unwrap();
        assert_eq!(terms(&conn), ("4.1".into(), "15200".into()));
        assert_eq!(saved.previous_rate.as_deref(), Some("0"));
        assert_eq!(saved.previous_monthly_payment.as_deref(), Some("10000"));
        let validity: Option<i64> = conn
            .query_row(
                "SELECT interest_rate_validity_date FROM loans WHERE id = 'l1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(validity, None, "a rate change resolves the fixation date");
        assert_eq!(anchor(&conn), (None, None));
        // Estimated: the day becomes the segment anchor and the new terms apply from it.
        assert_eq!(balance_on(&conn, YEAR_ON - DAY).0, "890000.00");
        assert_eq!(balance_on(&conn, YEAR_ON).0, "880000.00");
        let after_one = 880_000.0 * (1.0 + 0.041 / 12.0) - 15_200.0;
        assert_eq!(
            balance_on(&conn, YEAR_ON + 31 * DAY).0,
            format!("{after_one:.2}")
        );
    }

    #[test]
    fn a_rate_change_without_a_payment_keeps_the_payment_and_records_it() {
        let conn = setup_test_db();
        let saved = add_event(&conn, &rate_change(YEAR_ON, "3"), YEAR_ON).unwrap();
        assert_eq!(terms(&conn), ("3".into(), "10000".into()));
        assert_eq!(saved.previous_monthly_payment.as_deref(), Some("10000"));
    }

    #[test]
    fn a_manual_balance_is_a_record_and_the_anchor_mirrors_it() {
        let conn = setup_test_db();
        let day = START + 100 * DAY;
        let mut data = record(day, "950000");
        data.note = Some("  statement ".into());
        let saved = add_event(&conn, &data, day).unwrap();
        assert_eq!(saved.kind, "balance_check");
        assert_eq!(saved.note.as_deref(), Some("statement"));
        assert_eq!(anchor(&conn), (Some("950000".into()), Some(day)));
        assert_eq!(
            balance_on(&conn, day + 90 * DAY),
            ("950000.00".into(), LedgerMode::Manual)
        );
        // Nothing before the record is overridden.
        assert_eq!(balance_on(&conn, day - DAY).1, LedgerMode::Estimated);
    }

    #[test]
    fn a_manual_balance_of_the_same_day_replaces_the_record() {
        let conn = setup_test_db();
        let day = START + 100 * DAY;
        add_event(&conn, &record(day, "950000"), day).unwrap();
        add_event(&conn, &record(day + 3_600, "940000"), day).unwrap();
        let stored = list_events(&conn, "l1").unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].amount.as_deref(), Some("940000"));
        assert_eq!(anchor(&conn), (Some("940000".into()), Some(day)));
    }

    #[test]
    fn the_anchor_mirrors_the_newest_record_not_the_last_entered() {
        let conn = setup_test_db();
        let (older, newer) = (START + 100 * DAY, START + 200 * DAY);
        add_event(&conn, &record(newer, "900000"), newer).unwrap();
        add_event(&conn, &record(older, "950000"), newer).unwrap();
        assert_eq!(anchor(&conn), (Some("900000".into()), Some(newer)));
    }

    #[test]
    fn deleting_a_manual_record_restores_the_previous_one_then_the_estimate() {
        let conn = setup_test_db();
        let (first, second) = (START + 100 * DAY, START + 200 * DAY);
        let kept = add_event(&conn, &record(first, "950000"), second).unwrap();
        let removed = add_event(&conn, &record(second, "900000"), second).unwrap();
        assert_eq!(anchor(&conn), (Some("900000".into()), Some(second)));
        assert_eq!(balance_on(&conn, second + 30 * DAY).0, "900000.00");

        delete_event(&conn, &removed.id).unwrap();
        assert_eq!(anchor(&conn), (Some("950000".into()), Some(first)));
        assert_eq!(
            balance_on(&conn, second + 30 * DAY),
            ("950000.00".into(), LedgerMode::Manual)
        );

        delete_event(&conn, &kept.id).unwrap();
        assert_eq!(anchor(&conn), (None, None));
        assert_eq!(
            balance_on(&conn, second + 30 * DAY).1,
            LedgerMode::Estimated
        );
        assert!(matches!(
            delete_event(&conn, &kept.id),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn an_extra_payment_in_manual_mode_is_the_record_minus_the_payment() {
        let conn = setup_test_db();
        let day = START + 100 * DAY;
        add_event(&conn, &record(day, "900000"), day).unwrap();
        add_event(&conn, &extra(day + 10 * DAY, "50000"), day + 60 * DAY).unwrap();
        assert_eq!(
            balance_on(&conn, day + 5 * DAY),
            ("900000.00".into(), LedgerMode::Manual)
        );
        assert_eq!(
            balance_on(&conn, day + 60 * DAY),
            ("850000.00".into(), LedgerMode::Manual)
        );
        // The mirror still says what the user entered.
        assert_eq!(anchor(&conn), (Some("900000".into()), Some(day)));
    }

    #[test]
    fn a_rate_change_in_manual_mode_leaves_the_balance_alone() {
        let conn = setup_test_db();
        let day = START + 100 * DAY;
        add_event(&conn, &record(day, "900000"), day).unwrap();
        add_event(&conn, &rate_change(day + 10 * DAY, "5"), day + 60 * DAY).unwrap();
        assert_eq!(
            balance_on(&conn, day + 60 * DAY),
            ("900000.00".into(), LedgerMode::Manual)
        );
        assert_eq!(terms(&conn).0, "5");
        assert_eq!(anchor(&conn), (Some("900000".into()), Some(day)));
    }

    #[test]
    fn a_backdated_rate_change_gets_the_terms_in_force_and_keeps_the_later_ones_current() {
        let conn = setup_test_db();
        let (older, newer) = (START + 100 * DAY, START + 200 * DAY);
        add_event(&conn, &rate_change(newer, "4"), newer).unwrap();
        let backdated = add_event(&conn, &rate_change(older, "3"), newer).unwrap();
        // Before the older change the loan still had its original rate.
        assert_eq!(backdated.previous_rate.as_deref(), Some("0"));
        // The newer change stays the current rate.
        assert_eq!(terms(&conn).0, "4");
        let loan = loans::get_loan(&conn, "l1", newer + 40 * DAY).unwrap();
        let ledger = loans::loan_ledger(&loan).unwrap();
        assert_eq!(loan_balance_at(&ledger, START).annual_rate_pct, 0.0);
        assert_eq!(loan_balance_at(&ledger, older + DAY).annual_rate_pct, 3.0);
        assert_eq!(loan_balance_at(&ledger, newer + DAY).annual_rate_pct, 4.0);
    }

    #[test]
    fn deleting_the_only_rate_change_gives_the_loan_its_previous_terms_back() {
        let conn = setup_test_db();
        let mut data = rate_change(YEAR_ON, "4.1");
        data.monthly_payment = Some("15200".into());
        let saved = add_event(&conn, &data, YEAR_ON).unwrap();
        delete_event(&conn, &saved.id).unwrap();
        assert_eq!(terms(&conn), ("0".into(), "10000".into()));
        assert_eq!(balance_on(&conn, YEAR_ON + 31 * DAY).0, "870000.00");
    }

    #[test]
    fn deleting_the_latest_change_returns_to_the_earlier_one() {
        let conn = setup_test_db();
        let (first, second) = (START + 100 * DAY, START + 200 * DAY);
        add_event(&conn, &rate_change(first, "4"), second).unwrap();
        let latest = add_event(&conn, &rate_change(second, "3"), second).unwrap();
        delete_event(&conn, &latest.id).unwrap();
        assert_eq!(terms(&conn).0, "4");
    }

    #[test]
    fn deleting_the_earliest_change_hands_the_initial_terms_to_the_next() {
        let conn = setup_test_db();
        let (first, second) = (START + 100 * DAY, START + 200 * DAY);
        let earliest = add_event(&conn, &rate_change(first, "4"), second).unwrap();
        add_event(&conn, &rate_change(second, "3"), second).unwrap();
        delete_event(&conn, &earliest.id).unwrap();
        // The remaining change recorded 4 % as its previous rate; the original 0 % is
        // the loan's initial rate again.
        let remaining = list_events(&conn, "l1").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].previous_rate.as_deref(), Some("0"));
        assert_eq!(terms(&conn).0, "3");
        let loan = loans::get_loan(&conn, "l1", second).unwrap();
        let ledger = loans::loan_ledger(&loan).unwrap();
        assert_eq!(loan_balance_at(&ledger, first + DAY).annual_rate_pct, 0.0);
        assert_eq!(loan_balance_at(&ledger, second + DAY).annual_rate_pct, 3.0);
    }

    #[test]
    fn deleting_the_earliest_lowering_of_the_payment_restores_the_original_payment() {
        let conn = setup_test_db();
        let mut data = extra(YEAR_ON, "1000");
        data.monthly_payment = Some("8500".into());
        let saved = add_event(&conn, &data, YEAR_ON).unwrap();
        delete_event(&conn, &saved.id).unwrap();
        assert_eq!(terms(&conn).1, "10000");
        assert!(list_events(&conn, "l1").unwrap().is_empty());
        assert_eq!(anchor(&conn), (None, None));
    }

    #[test]
    fn deleting_a_plain_extra_payment_changes_no_terms() {
        let conn = setup_test_db();
        let saved = add_event(&conn, &extra(YEAR_ON, "1000"), YEAR_ON).unwrap();
        delete_event(&conn, &saved.id).unwrap();
        assert_eq!(terms(&conn), ("0".into(), "10000".into()));
        assert_eq!(balance_on(&conn, YEAR_ON).0, "880000.00");
    }

    #[test]
    fn events_before_the_start_and_bad_kinds_are_rejected() {
        let conn = setup_test_db();
        assert!(matches!(
            add_event(&conn, &extra(START - DAY, "1000"), START),
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
        let mut unknown = extra(START, "1000");
        unknown.loan_id = "nope".into();
        assert!(matches!(
            add_event(&conn, &unknown, START),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn the_form_balance_creates_replaces_and_keeps_the_other_days() {
        let conn = setup_test_db();
        let (first, second) = (START + 100 * DAY, START + 200 * DAY);
        upsert_form_balance(&conn, "l1", first, "950000", 10).unwrap();
        upsert_form_balance(&conn, "l1", second, "900000", 20).unwrap();
        upsert_form_balance(&conn, "l1", first + 3_600, "960000", 30).unwrap();
        let stored: Vec<(i64, Option<String>)> = list_events(&conn, "l1")
            .unwrap()
            .into_iter()
            .map(|e| (e.event_day, e.amount))
            .collect();
        assert_eq!(
            stored,
            vec![
                (first, Some("960000".to_string())),
                (second, Some("900000".to_string()))
            ]
        );
        // The newest record stays the mirror.
        assert_eq!(anchor(&conn), (Some("900000".into()), Some(second)));
    }

    #[test]
    fn saving_an_unchanged_form_balance_leaves_the_record_alone() {
        let conn = setup_test_db();
        let day = START + 100 * DAY;
        let saved = add_manual_balance(&conn, "l1", day, "950000", Some("statement"), 10).unwrap();
        upsert_form_balance(&conn, "l1", day, "950000.00", 99).unwrap();
        let stored = list_events(&conn, "l1").unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].id, saved.id);
        assert_eq!(stored[0].created_at, 10);
        assert_eq!(stored[0].note.as_deref(), Some("statement"));
    }

    #[test]
    fn switching_the_form_off_deletes_every_manual_record_and_only_those() {
        let conn = setup_test_db();
        let day = START + 100 * DAY;
        add_manual_balance(&conn, "l1", day, "950000", None, 10).unwrap();
        add_manual_balance(&conn, "l1", day + 30 * DAY, "900000", None, 11).unwrap();
        add_event(&conn, &extra(day + 5 * DAY, "1000"), day + 60 * DAY).unwrap();
        delete_manual_balances(&conn, "l1", 20).unwrap();
        let kinds: Vec<String> = list_events(&conn, "l1")
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, vec!["extra_payment"]);
        assert_eq!(anchor(&conn), (None, None));
    }
    // --- the legacy anchors migration 019 kept ----------------------------------------------

    fn legacy_anchor(conn: &Connection, loan_id: &str, amount: &str, day: i64) {
        conn.execute(
            "INSERT INTO loan_legacy_anchors (loan_id, amount, day) VALUES (?1, ?2, ?3)",
            rusqlite::params![loan_id, amount, day],
        )
        .unwrap();
    }

    fn legacy_rows(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM loan_legacy_anchors", [], |r| r.get(0))
            .unwrap()
    }

    /// The balance 100 000 below the estimate holds as a manual record: the extra payment of
    /// the day had re-anchored a balance the user had set by hand.
    #[test]
    fn a_legacy_anchor_that_differs_from_the_estimate_becomes_a_manual_record() {
        let conn = setup_test_db();
        // 880 000 on YEAR_ON (twelve payments), 780 000 after the extra payment of the day.
        add_event(&conn, &extra(YEAR_ON, "100000"), YEAR_ON).unwrap();
        legacy_anchor(&conn, "l1", "700000.00", YEAR_ON);

        assert_eq!(reconcile_legacy_anchors(&conn, YEAR_ON + 5).unwrap(), 1);

        let events = list_events(&conn, "l1").unwrap();
        let kinds: Vec<&str> = events.iter().map(|e| e.kind.as_str()).collect();
        assert_eq!(kinds, vec!["extra_payment", "balance_check"]);
        assert_eq!(events[1].event_day, YEAR_ON);
        assert_eq!(events[1].amount.as_deref(), Some("700000.00"));
        assert_eq!(anchor(&conn), (Some("700000.00".into()), Some(YEAR_ON)));
        assert_eq!(
            balance_on(&conn, YEAR_ON),
            ("700000.00".into(), LedgerMode::Manual),
            "the record applies after the day's extra payment"
        );
        assert_eq!(
            balance_on(&conn, YEAR_ON + 60 * DAY),
            ("700000.00".into(), LedgerMode::Manual),
            "and holds"
        );
        assert_eq!(legacy_rows(&conn), 0, "the processed row is gone");
    }

    #[test]
    fn a_legacy_anchor_that_matches_the_estimate_adds_no_record() {
        let conn = setup_test_db();
        add_event(&conn, &extra(YEAR_ON, "100000"), YEAR_ON).unwrap();
        // a pure estimate with the extra payment: 780 000
        legacy_anchor(&conn, "l1", "780000.00", YEAR_ON);

        assert_eq!(reconcile_legacy_anchors(&conn, YEAR_ON + 5).unwrap(), 0);

        let events = list_events(&conn, "l1").unwrap();
        assert_eq!(events.len(), 1, "only the extra payment");
        assert_eq!(anchor(&conn), (None, None), "the mirror is left alone");
        assert_eq!(
            balance_on(&conn, YEAR_ON + 30 * DAY),
            ("780000.00".into(), LedgerMode::Estimated)
        );
        assert_eq!(legacy_rows(&conn), 0);
    }

    #[test]
    fn a_legacy_anchor_within_a_currency_unit_of_the_estimate_is_the_estimate() {
        let conn = setup_test_db();
        add_event(&conn, &extra(YEAR_ON, "100000"), YEAR_ON).unwrap();
        legacy_anchor(&conn, "l1", "780000.99", YEAR_ON);
        assert_eq!(reconcile_legacy_anchors(&conn, YEAR_ON + 5).unwrap(), 0);

        // …and one unit and a bit away is an override
        let conn = setup_test_db();
        add_event(&conn, &extra(YEAR_ON, "100000"), YEAR_ON).unwrap();
        legacy_anchor(&conn, "l1", "780001.01", YEAR_ON);
        assert_eq!(reconcile_legacy_anchors(&conn, YEAR_ON + 5).unwrap(), 1);
    }

    #[test]
    fn the_reconciliation_runs_once_and_a_second_run_changes_nothing() {
        let conn = setup_test_db();
        add_event(&conn, &extra(YEAR_ON, "100000"), YEAR_ON).unwrap();
        legacy_anchor(&conn, "l1", "700000.00", YEAR_ON);
        assert_eq!(reconcile_legacy_anchors(&conn, YEAR_ON + 5).unwrap(), 1);
        let ids = |conn: &Connection| -> Vec<String> {
            list_events(conn, "l1")
                .unwrap()
                .into_iter()
                .map(|event| event.id)
                .collect()
        };
        let after_first = ids(&conn);

        assert_eq!(reconcile_legacy_anchors(&conn, YEAR_ON + 99).unwrap(), 0);
        assert_eq!(ids(&conn), after_first);
        assert_eq!(anchor(&conn), (Some("700000.00".into()), Some(YEAR_ON)));
    }

    #[test]
    fn a_legacy_anchor_of_a_deleted_loan_or_an_unreadable_amount_is_dropped() {
        let conn = setup_test_db();
        legacy_anchor(&conn, "gone", "1000", YEAR_ON);
        legacy_anchor(&conn, "l1", "n/a", YEAR_ON);

        assert_eq!(reconcile_legacy_anchors(&conn, YEAR_ON).unwrap(), 0);

        assert_eq!(legacy_rows(&conn), 0);
        assert!(list_events(&conn, "l1").unwrap().is_empty());
    }
}
