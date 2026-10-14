//! Loans business logic. Extracted from commands/loans.rs so the Tauri
//! command and the MCP write tool share one validated path (ADR 0007).
//!
//! Every loan read goes through here so each `Loan` carries its events and the
//! balance of today's ledger walk (`loan_amortization::loan_balance_at`,
//! `balance_mode`); the maths lives in `services/loan_amortization.rs`. A manual
//! balance is a `balance_check` record of `loan_events`; the anchor columns of
//! `loans` only mirror the newest one (`loan_events::refresh_anchor_mirror`).

use crate::error::{AppError, Result};
use crate::models::loans::anchor_before_start;
use crate::models::{InsertLoan, Loan, LoanEvent, LoanSchedule, LoanScheduleRow};
use crate::services::loan_amortization::{
    self as amortization, day_floor, loan_balance_at, plan_from, today_utc_day, LedgerEvent,
    LedgerEventKind, LedgerMode, LoanLedger, LoanTerms, ScheduleEnd, MAX_SCHEDULE_ROWS,
};
use crate::services::loan_events;
use rusqlite::{Connection, OptionalExtension, Row};
use std::collections::HashMap;
use uuid::Uuid;

/// Column list of `loans`, qualified with the alias `l` every query uses.
const LOAN_COLUMNS: &str = "l.id, l.name, l.principal, l.currency, l.interest_rate,
    l.interest_rate_validity_date, l.monthly_payment, l.start_date, l.end_date,
    l.balance_anchor_amount, l.balance_anchor_date, l.created_at, l.updated_at";

/// Money TEXT with 2 decimals; never "-0.00".
fn money_text(value: f64) -> String {
    let scaled = value * 100.0;
    let rounded = if scaled.is_finite() {
        scaled.round() / 100.0
    } else {
        value
    };
    format!("{:.2}", if rounded == 0.0 { 0.0 } else { rounded })
}

/// A stored money/percent text as a number. Empty means 0 (a loan without a
/// rate or payment); anything else that is not a finite number is corrupt data
/// and fails loudly instead of becoming a plausible-looking 0 (ADR 0001).
fn parse_amount(value: &str, field: &str) -> Result<f64> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(0.0);
    }
    match trimmed.parse::<f64>() {
        Ok(number) if number.is_finite() => Ok(number),
        _ => Err(AppError::Internal(format!("loans.{field}: corrupt value"))),
    }
}

/// `Some(number)` for a stored optional money/percent text; `None` when absent
/// or blank. Corrupt text is an error (ADR 0001), never a silent 0.
fn parse_optional(value: Option<&str>, field: &str) -> Result<Option<f64>> {
    match value.map(str::trim).filter(|text| !text.is_empty()) {
        Some(text) => parse_amount(text, field).map(Some),
        None => Ok(None),
    }
}

/// Amortization inputs from the stored loan columns: the loan's current rate and
/// payment, no anchor (the events are the source of every manual balance).
fn terms_from_columns(
    principal: &str,
    interest_rate: &str,
    monthly_payment: &str,
    start_date: i64,
    end_date: Option<i64>,
) -> Result<LoanTerms> {
    Ok(LoanTerms {
        principal: parse_amount(principal, "principal")?,
        annual_rate_pct: parse_amount(interest_rate, "interest_rate")?,
        monthly_payment: parse_amount(monthly_payment, "monthly_payment")?,
        start_day: day_floor(start_date),
        anchor_amount: None,
        anchor_day: None,
        end_day: end_date.map(day_floor),
    })
}

fn ledger_event(event: &LoanEvent) -> Result<LedgerEvent> {
    let kind = match event.kind.as_str() {
        "extra_payment" => LedgerEventKind::ExtraPayment,
        "rate_change" => LedgerEventKind::RateChange,
        "balance_check" => LedgerEventKind::BalanceCheck,
        other => {
            return Err(AppError::Internal(format!(
                "loan_events.kind: unknown value {other}"
            )))
        }
    };
    Ok(LedgerEvent {
        day: day_floor(event.event_day),
        created_at: event.created_at,
        kind,
        amount: parse_optional(event.amount.as_deref(), "loan_events.amount")?,
        rate: parse_optional(event.rate.as_deref(), "loan_events.rate")?,
        monthly_payment: parse_optional(
            event.monthly_payment.as_deref(),
            "loan_events.monthly_payment",
        )?,
        previous_rate: parse_optional(event.previous_rate.as_deref(), "loan_events.previous_rate")?,
        previous_monthly_payment: parse_optional(
            event.previous_monthly_payment.as_deref(),
            "loan_events.previous_monthly_payment",
        )?,
    })
}

/// The ledger of a loan from its stored terms and its events (oldest first).
fn ledger_from_parts(
    principal: &str,
    interest_rate: &str,
    monthly_payment: &str,
    start_date: i64,
    end_date: Option<i64>,
    events: &[LoanEvent],
) -> Result<LoanLedger> {
    Ok(LoanLedger {
        terms: terms_from_columns(
            principal,
            interest_rate,
            monthly_payment,
            start_date,
            end_date,
        )?,
        events: events
            .iter()
            .map(ledger_event)
            .collect::<Result<Vec<_>>>()?,
    })
}

/// The ledger of a loan: its current terms and its events (`loan.events`).
pub fn loan_ledger(loan: &Loan) -> Result<LoanLedger> {
    ledger_from_parts(
        &loan.principal,
        &loan.interest_rate,
        &loan.monthly_payment,
        loan.start_date,
        loan.end_date,
        &loan.events,
    )
}

/// A loan's id and currency with its ledger (net worth, projection, history).
#[derive(Debug, Clone)]
pub struct LoanLedgerEntry {
    pub id: String,
    pub currency: String,
    pub ledger: LoanLedger,
}

/// Row in `LOAN_COLUMNS` order; events and the balance fields are filled by the caller.
fn read_loan(row: &Row) -> rusqlite::Result<Loan> {
    Ok(Loan {
        id: row.get(0)?,
        name: row.get(1)?,
        principal: row.get(2)?,
        currency: row.get(3)?,
        interest_rate: row.get(4)?,
        interest_rate_validity_date: row.get(5)?,
        monthly_payment: row.get(6)?,
        start_date: row.get(7)?,
        end_date: row.get(8)?,
        balance_anchor_amount: row.get(9)?,
        balance_anchor_date: row.get(10)?,
        outstanding_balance: String::new(),
        balance_mode: String::new(),
        events: Vec::new(),
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

/// Name of a ledger mode on the wire.
pub fn balance_mode_name(mode: LedgerMode) -> &'static str {
    match mode {
        LedgerMode::Estimated => "estimated",
        LedgerMode::Manual => "manual",
    }
}

/// `outstanding_balance` and `balance_mode` of `today` from the loan's ledger.
fn fill_balance(loan: &mut Loan, today: i64) -> Result<()> {
    let state = loan_balance_at(&loan_ledger(loan)?, today);
    loan.outstanding_balance = money_text(state.balance);
    loan.balance_mode = balance_mode_name(state.mode).to_string();
    Ok(())
}

/// Loans matching `FROM loans l <sql_tail>` with their events attached (oldest
/// first); the balance fields are still empty.
fn query_loans_with_events(
    conn: &Connection,
    sql_tail: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<Loan>> {
    let sql = format!("SELECT {LOAN_COLUMNS} FROM loans l {sql_tail}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params, read_loan)?;
    let mut loans = Vec::new();
    for row in rows {
        let mut loan = row?;
        loan.events = loan_events::list_events(conn, &loan.id)?;
        loans.push(loan);
    }
    Ok(loans)
}

/// Loans matching `FROM loans l <sql_tail>`, valued as of `today`.
fn query_loans(
    conn: &Connection,
    sql_tail: &str,
    params: impl rusqlite::Params,
    today: i64,
) -> Result<Vec<Loan>> {
    let mut loans = query_loans_with_events(conn, sql_tail, params)?;
    for loan in &mut loans {
        fill_balance(loan, today)?;
    }
    Ok(loans)
}

/// All loans by name, valued as of `today` (UTC day).
pub fn list_loans(conn: &Connection, today: i64) -> Result<Vec<Loan>> {
    query_loans(conn, "ORDER BY l.name", [], today)
}

/// Loans not linked to any real estate.
pub fn list_available_loans(conn: &Connection, today: i64) -> Result<Vec<Loan>> {
    query_loans(
        conn,
        "WHERE l.id NOT IN (SELECT loan_id FROM real_estate_loans) ORDER BY l.name",
        [],
        today,
    )
}

/// Loans linked to one real estate.
pub fn list_loans_for_real_estate(
    conn: &Connection,
    real_estate_id: &str,
    today: i64,
) -> Result<Vec<Loan>> {
    query_loans(
        conn,
        "INNER JOIN real_estate_loans rel ON l.id = rel.loan_id WHERE rel.real_estate_id = ?1",
        [real_estate_id],
        today,
    )
}

/// One loan, valued as of `today`.
pub fn get_loan(conn: &Connection, id: &str, today: i64) -> Result<Loan> {
    query_loans(conn, "WHERE l.id = ?1", [id], today)?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Loan not found".into()))
}

/// The start day of a loan: the first day its history changes when the loan is edited or
/// deleted. `None` for a loan that does not exist.
pub fn loan_start_date(conn: &Connection, id: &str) -> Result<Option<i64>> {
    Ok(conn
        .query_row("SELECT start_date FROM loans WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?)
}

/// Dates that depend on the effective start (which may be defaulted or kept).
fn check_dates(start: i64, end: Option<i64>, anchor_date: Option<i64>) -> Result<()> {
    if let Some(end) = end {
        if end <= start {
            return Err(AppError::Validation("validation.endDateAfterStart".into()));
        }
    }
    if let Some(anchor) = anchor_date {
        if anchor_before_start(anchor, start) {
            return Err(AppError::Validation(
                "validation.balanceAnchorBeforeStart".into(),
            ));
        }
    }
    Ok(())
}

/// Create a new loan. SINGLE SOURCE OF TRUTH for loan creation.
pub fn create_loan(conn: &Connection, data: &InsertLoan) -> Result<Loan> {
    data.validate()?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let currency = data
        .currency
        .clone()
        .unwrap_or_else(|| crate::constants::STORAGE_DEFAULT_CURRENCY.to_string());
    let interest_rate = data
        .interest_rate
        .clone()
        .unwrap_or_else(|| "0".to_string());
    let monthly_payment = data
        .monthly_payment
        .clone()
        .unwrap_or_else(|| "0".to_string());
    let start_date = crate::models::validation::utc_day(
        data.start_date.unwrap_or(now),
        "validation.dateInvalid",
    )?;
    let end_date = data
        .end_date
        .map(|date| crate::models::validation::utc_day(date, "validation.dateInvalid"))
        .transpose()?;
    let validity_date = data
        .interest_rate_validity_date
        .map(|date| crate::models::validation::utc_day(date, "validation.dateInvalid"))
        .transpose()?;
    let anchor = data
        .balance_anchor()
        .map(|(amount, day)| {
            crate::models::validation::utc_day(day, "validation.dateInvalid")
                .map(|day| (amount, day))
        })
        .transpose()?;

    // InsertLoan::validate() only checks ordering when BOTH start_date and
    // end_date are provided by the caller; start_date defaults to `now` right
    // above, so an end_date-only input (end_date <= now) would otherwise
    // persist an end-before-start loan. Re-check against the effective date.
    check_dates(start_date, end_date, anchor.as_ref().map(|(_, day)| *day))?;

    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO loans (id, name, principal, currency, interest_rate, interest_rate_validity_date,
         monthly_payment, start_date, end_date, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        rusqlite::params![
            id,
            data.name,
            data.principal,
            currency,
            interest_rate,
            validity_date,
            monthly_payment,
            start_date,
            end_date,
            now
        ],
    )?;
    // The anchor of the form is a manual balance record; the anchor columns follow it.
    if let Some((amount, day)) = &anchor {
        loan_events::add_manual_balance(&tx, &id, *day, amount, None, now)?;
    }
    tx.commit()?;

    get_loan(conn, &id, today_utc_day())
}

/// Update a loan. The form's manual balance is a record: with a balance and a day
/// the record of that day is created or replaced (older records stay); without
/// one every manual record of the loan is deleted and the loan is estimated
/// again (D8). The anchor columns follow the newest record.
pub fn update_loan(conn: &Connection, id: &str, data: &InsertLoan) -> Result<Loan> {
    data.validate()?;

    let existing_start: i64 = conn
        .query_row("SELECT start_date FROM loans WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or_else(|| AppError::NotFound("Loan not found".into()))?;
    let start_date = data
        .start_date
        .map(|date| crate::models::validation::utc_day(date, "validation.dateInvalid"))
        .transpose()?;
    let end_date = data
        .end_date
        .map(|date| crate::models::validation::utc_day(date, "validation.dateInvalid"))
        .transpose()?;
    let validity_date = data
        .interest_rate_validity_date
        .map(|date| crate::models::validation::utc_day(date, "validation.dateInvalid"))
        .transpose()?;
    let anchor = data
        .balance_anchor()
        .map(|(amount, day)| {
            crate::models::validation::utc_day(day, "validation.dateInvalid")
                .map(|day| (amount, day))
        })
        .transpose()?;
    check_dates(
        start_date.unwrap_or(existing_start),
        end_date,
        anchor.as_ref().map(|(_, day)| *day),
    )?;

    let now = chrono::Utc::now().timestamp();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE loans SET name = ?1, principal = ?2,
         currency = COALESCE(?3, currency), interest_rate = COALESCE(?4, interest_rate),
         interest_rate_validity_date = ?5, monthly_payment = COALESCE(?6, monthly_payment),
         start_date = COALESCE(?7, start_date), end_date = ?8, updated_at = ?9
         WHERE id = ?10",
        rusqlite::params![
            data.name,
            data.principal,
            data.currency,
            data.interest_rate,
            validity_date,
            data.monthly_payment,
            start_date,
            end_date,
            now,
            id
        ],
    )?;
    match &anchor {
        Some((amount, day)) => loan_events::upsert_form_balance(&tx, id, *day, amount, now)?,
        None => loan_events::delete_manual_balances(&tx, id, now)?,
    }
    tx.commit()?;

    get_loan(conn, id, today_utc_day())
}

/// Loans matching `FROM loans l <sql_tail>` as ledgers. Only the columns the maths
/// needs are read, so the callers that value many loans stay light.
fn query_ledgers(
    conn: &Connection,
    sql_tail: &str,
    params: impl rusqlite::Params,
) -> Result<Vec<LoanLedgerEntry>> {
    let sql = format!(
        "SELECT l.id, l.currency, l.principal, l.interest_rate, l.monthly_payment,
                l.start_date, l.end_date
         FROM loans l {sql_tail}"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params, |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, i64>(5)?,
            row.get::<_, Option<i64>>(6)?,
        ))
    })?;
    let mut entries = Vec::new();
    for row in rows {
        let (id, currency, principal, rate, payment, start, end) = row?;
        let events = loan_events::list_events(conn, &id)?;
        entries.push(LoanLedgerEntry {
            ledger: ledger_from_parts(&principal, &rate, &payment, start, end, &events)?,
            id,
            currency,
        });
    }
    Ok(entries)
}

/// One loan with its ledger, or `NotFound`.
pub fn load_ledger(conn: &Connection, id: &str) -> Result<LoanLedgerEntry> {
    query_ledgers(conn, "WHERE l.id = ?1", [id])?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Loan not found".into()))
}

/// Every loan with its ledger (net worth, projection, cashflow, history).
pub fn load_ledgers(conn: &Connection) -> Result<Vec<LoanLedgerEntry>> {
    query_ledgers(conn, "ORDER BY l.id", [])
}

/// Outstanding balance per currency as of `today`: the liabilities of net
/// worth and snapshots (unrounded, like every other native breakdown). Each
/// loan's value is `loan_balance_at(today)`, the figure of its own page.
pub fn outstanding_by_currency(conn: &Connection, today: i64) -> Result<HashMap<String, f64>> {
    let mut by_currency: HashMap<String, f64> = HashMap::new();
    for entry in load_ledgers(conn)? {
        *by_currency.entry(entry.currency).or_insert(0.0) +=
            loan_balance_at(&entry.ledger, today).balance;
    }
    Ok(by_currency)
}

/// Repayment schedule and totals of one loan as of `today` (loan detail page).
///
/// The schedule is the plan ahead of the ledger (`plan_from`): from the current
/// estimated segment, or, when a manual balance holds, from today's balance
/// with the terms in force. `anchor_*`, `principal_repaid`, `interest_paid` and
/// `payments_made` describe that plan's start; with a manual balance nothing
/// has been paid since it (the balance holds), so they are today and 0.
pub fn loan_schedule(conn: &Connection, id: &str, today: i64) -> Result<LoanSchedule> {
    let loan = get_loan(conn, id, today)?;
    let ledger = loan_ledger(&loan)?;
    let state = loan_balance_at(&ledger, today);
    let plan = plan_from(&ledger, today);
    let manual = state.mode == LedgerMode::Manual;
    let (plan_anchor_amount, plan_anchor_day) = plan.anchor();
    let elapsed = amortization::elapsed_to(&plan, today);
    let schedule = amortization::schedule(&plan, MAX_SCHEDULE_ROWS);
    // A manual balance holds as of today; the plan's anchor day is only the
    // payment cycle it continues on.
    let (anchor_amount, anchor_day) = if manual {
        (state.balance, day_floor(today))
    } else {
        (plan_anchor_amount, plan_anchor_day)
    };

    let mut warnings = Vec::new();
    if plan.monthly_payment <= 0.0 && plan_anchor_amount > 0.0 {
        warnings.push("noPayment".to_string());
    }
    if amortization::payment_below_interest(&plan) {
        warnings.push("paymentBelowInterest".to_string());
    }
    match schedule.end {
        ScheduleEnd::EndDate if !schedule.rows.is_empty() => {
            warnings.push("balanceAtEndDate".to_string())
        }
        ScheduleEnd::Capped => warnings.push("scheduleTruncated".to_string()),
        _ => {}
    }

    let (payments_remaining, payoff_day) = if elapsed.balance <= 0.0 {
        (
            Some(0),
            schedule
                .rows
                .last()
                .filter(|row| row.balance_after <= 0.0)
                .map(|row| row.due_day),
        )
    } else if schedule.end == ScheduleEnd::Paid {
        let remaining = schedule
            .rows
            .iter()
            .filter(|row| row.due_day > today)
            .count() as u32;
        (Some(remaining), schedule.rows.last().map(|row| row.due_day))
    } else {
        (None, None)
    };

    Ok(LoanSchedule {
        loan_id: loan.id,
        as_of_day: day_floor(today),
        anchor_amount: money_text(anchor_amount),
        anchor_day,
        anchor_is_manual: manual,
        outstanding_balance: money_text(elapsed.balance),
        principal_repaid: money_text(anchor_amount - elapsed.balance),
        interest_paid: money_text(elapsed.interest_paid),
        payments_made: elapsed.payments_made,
        payments_remaining,
        payoff_day,
        rows: schedule
            .rows
            .iter()
            .map(|row| LoanScheduleRow {
                due_day: row.due_day,
                payment: money_text(row.payment),
                interest: money_text(row.interest),
                principal_part: money_text(row.principal_part),
                balance_after: money_text(row.balance_after),
            })
            .collect(),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    /// UTC-midnight unix seconds of an ISO date.
    fn day(iso: &str) -> i64 {
        let date = chrono::NaiveDate::parse_from_str(iso, "%Y-%m-%d").unwrap();
        let epoch = chrono::NaiveDate::from_ymd_opt(1970, 1, 1).unwrap();
        date.signed_duration_since(epoch).num_days() * 86_400
    }

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE loans (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                principal TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                interest_rate TEXT NOT NULL DEFAULT '0',
                interest_rate_validity_date INTEGER,
                monthly_payment TEXT NOT NULL DEFAULT '0',
                start_date INTEGER NOT NULL DEFAULT (unixepoch()),
                end_date INTEGER,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                balance_anchor_amount TEXT,
                balance_anchor_date INTEGER
            );
            CREATE TABLE real_estate_loans (
                real_estate_id TEXT NOT NULL,
                loan_id TEXT NOT NULL,
                PRIMARY KEY (real_estate_id, loan_id)
            );
            CREATE TABLE loan_events (
                id TEXT PRIMARY KEY,
                loan_id TEXT NOT NULL REFERENCES loans(id) ON DELETE CASCADE,
                kind TEXT NOT NULL CHECK (kind IN ('extra_payment', 'rate_change', 'balance_check')),
                event_day INTEGER NOT NULL,
                amount TEXT,
                rate TEXT,
                monthly_payment TEXT,
                note TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                previous_rate TEXT,
                previous_monthly_payment TEXT
            );
            "#,
        )
        .expect("schema");
        conn
    }

    /// Insert a loan event row directly (kind, day, amount / rate / payment, creation time).
    fn put_event(
        conn: &Connection,
        loan_id: &str,
        kind: &str,
        event_day: i64,
        amount: Option<&str>,
        created_at: i64,
    ) {
        conn.execute(
            "INSERT INTO loan_events (id, loan_id, kind, event_day, amount, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                loan_id,
                kind,
                event_day,
                amount,
                created_at
            ],
        )
        .expect("insert event");
    }

    /// Manual balance records of a loan: (day, amount), oldest first.
    fn records(conn: &Connection, loan_id: &str) -> Vec<(i64, String)> {
        loan_events::list_events(conn, loan_id)
            .unwrap()
            .into_iter()
            .filter(|event| event.kind == "balance_check")
            .map(|event| (event.event_day, event.amount.unwrap()))
            .collect()
    }

    fn valid_insert() -> InsertLoan {
        InsertLoan {
            name: "Test loan".into(),
            principal: "10000".into(),
            currency: None,
            interest_rate: None,
            interest_rate_validity_date: None,
            monthly_payment: None,
            start_date: None,
            end_date: None,
            balance_anchor_amount: None,
            balance_anchor_date: None,
        }
    }

    /// The 30-year annuity of the spec: 1 000 000 at 5 %, payment 5 368.22.
    fn mortgage(conn: &Connection, id: &str, currency: &str) {
        conn.execute(
            "INSERT INTO loans (id, name, principal, currency, interest_rate, monthly_payment, start_date)
             VALUES (?1, ?1, '1000000', ?2, '5', '5368.22', ?3)",
            rusqlite::params![id, currency, day("2020-01-15") + 9 * 3_600],
        )
        .expect("insert loan");
    }

    #[test]
    fn the_start_date_of_a_loan_is_readable_before_it_changes() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.start_date = Some(day("2021-03-04"));
        let loan = create_loan(&conn, &data).unwrap();
        assert_eq!(
            loan_start_date(&conn, &loan.id).unwrap(),
            Some(day("2021-03-04"))
        );
        assert_eq!(loan_start_date(&conn, "missing").unwrap(), None);
    }

    #[test]
    fn create_loan_applies_defaults() {
        let conn = setup_test_db();
        let before = chrono::Utc::now().timestamp();
        let loan = create_loan(&conn, &valid_insert()).unwrap();
        assert_eq!(loan.currency, "CZK");
        assert_eq!(loan.interest_rate, "0");
        assert_eq!(loan.monthly_payment, "0");
        assert_eq!(loan.start_date, before.div_euclid(86_400) * 86_400);
        assert_eq!(loan.balance_anchor_amount, None);
        assert_eq!(loan.balance_anchor_date, None);
        // no payment: the balance is the principal
        assert_eq!(loan.outstanding_balance, "10000.00");
    }

    #[test]
    fn large_finite_principal_remains_finite_when_formatting_its_balance() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.principal = "1e308".into();
        let loan = create_loan(&conn, &data).expect("loan");
        assert_eq!(
            loan.outstanding_balance.parse::<f64>().expect("balance"),
            1e308
        );
    }

    #[test]
    fn blank_loan_numbers_are_rejected_without_mutating_existing_facts() {
        let conn = setup_test_db();
        let existing = create_loan(&conn, &valid_insert()).expect("existing loan");
        for blank in ["", "  ", " 1.25 "] {
            for field in ["interest", "payment"] {
                let mut data = valid_insert();
                if field == "interest" {
                    data.interest_rate = Some(blank.into());
                } else {
                    data.monthly_payment = Some(blank.into());
                }
                assert!(
                    create_loan(&conn, &data).is_err(),
                    "create accepted {field}={blank:?}"
                );
                assert!(
                    update_loan(&conn, &existing.id, &data).is_err(),
                    "update accepted {field}={blank:?}"
                );
            }
        }
        let facts: (i64, String, String) = conn
            .query_row(
                "SELECT count(*), interest_rate, monthly_payment FROM loans",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("facts");
        assert_eq!(facts, (1, "0".into(), "0".into()));
    }

    #[test]
    fn create_loan_rejects_invalid() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.principal = "0".into();
        assert!(create_loan(&conn, &data).is_err());
    }

    #[test]
    fn create_loan_rejects_end_date_before_defaulted_start_date() {
        // start_date is omitted (defaults to `now`) and end_date is in the
        // past relative to `now` — validate() alone can't catch this because
        // it only compares the two fields when both are Some.
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.end_date = Some(1); // 1970-01-01, long before "now"
        let err = create_loan(&conn, &data).unwrap_err();
        match err {
            AppError::Validation(msg) => assert_eq!(msg, "validation.endDateAfterStart"),
            other => panic!("expected Validation error, got {other:?}"),
        }
    }

    #[test]
    fn create_loan_accepts_end_date_after_defaulted_start_date() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.end_date = Some(chrono::Utc::now().timestamp() + 3_600 * 24 * 365);
        assert!(create_loan(&conn, &data).is_ok());
    }

    #[test]
    fn create_loan_rejects_a_manual_balance_before_the_defaulted_start_date() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.balance_anchor_amount = Some("9000".into());
        data.balance_anchor_date = Some(day("2001-01-01"));
        match create_loan(&conn, &data).unwrap_err() {
            AppError::Validation(msg) => assert_eq!(msg, "validation.balanceAnchorBeforeStart"),
            other => panic!("expected Validation error, got {other:?}"),
        }
    }

    #[test]
    fn create_loan_stores_the_form_balance_as_a_record_that_holds() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.start_date = Some(day("2020-01-15"));
        data.interest_rate = Some("5".into());
        data.monthly_payment = Some("5368.22".into());
        data.principal = "1000000".into();
        data.balance_anchor_amount = Some("800000".into());
        data.balance_anchor_date = Some(day("2025-06-15"));
        let loan = create_loan(&conn, &data).unwrap();
        // The anchor columns mirror the record.
        assert_eq!(loan.balance_anchor_amount.as_deref(), Some("800000"));
        assert_eq!(loan.balance_anchor_date, Some(day("2025-06-15")));
        assert_eq!(
            records(&conn, &loan.id),
            vec![(day("2025-06-15"), "800000".to_string())]
        );
        assert_eq!(loan.events.len(), 1);
        assert_eq!(loan.events[0].kind, "balance_check");
        // Nothing is amortized after it: the balance is what the user entered.
        assert_eq!(loan.outstanding_balance, "800000.00");
        assert_eq!(loan.balance_mode, "manual");
    }

    #[test]
    fn a_loan_without_a_manual_balance_is_estimated() {
        let conn = setup_test_db();
        mortgage(&conn, "m1", "CZK");
        let loan = get_loan(&conn, "m1", day("2021-01-15")).unwrap();
        assert_eq!(loan.balance_mode, "estimated");
        assert!(loan.events.is_empty());
    }

    #[test]
    fn list_values_each_loan_as_of_the_given_day() {
        let conn = setup_test_db();
        mortgage(&conn, "m1", "CZK");

        let before_first_payment = list_loans(&conn, day("2020-02-14")).unwrap();
        assert_eq!(before_first_payment[0].outstanding_balance, "1000000.00");
        assert_eq!(before_first_payment[0].principal, "1000000");

        let after_a_year = list_loans(&conn, day("2021-01-15")).unwrap();
        assert_eq!(after_a_year[0].outstanding_balance, "985246.30");
        // principal stays the original amount
        assert_eq!(after_a_year[0].principal, "1000000");

        let paid_off = list_loans(&conn, day("2050-01-15")).unwrap();
        assert_eq!(paid_off[0].outstanding_balance, "0.00");
    }

    #[test]
    fn an_empty_rate_and_payment_read_as_zero() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date)
             VALUES ('l1', 'Interest free', '12000', '', '', ?1)",
            [day("2024-01-10")],
        )
        .unwrap();
        let loans = list_loans(&conn, day("2030-01-01")).unwrap();
        assert_eq!(loans[0].outstanding_balance, "12000.00");
    }

    #[test]
    fn a_corrupt_stored_amount_is_an_error_not_a_zero() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO loans (id, name, principal, start_date) VALUES ('l1', 'Broken', 'abc', 0)",
            [],
        )
        .unwrap();
        assert!(list_loans(&conn, day("2024-01-01")).is_err());
        assert!(outstanding_by_currency(&conn, day("2024-01-01")).is_err());
    }

    #[test]
    fn get_loan_reports_missing_loans() {
        let conn = setup_test_db();
        match get_loan(&conn, "nope", day("2024-01-01")).unwrap_err() {
            AppError::NotFound(_) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn real_estate_linking_filters_the_lists() {
        let conn = setup_test_db();
        mortgage(&conn, "linked", "CZK");
        mortgage(&conn, "free", "CZK");
        conn.execute(
            "INSERT INTO real_estate_loans (real_estate_id, loan_id) VALUES ('re1', 'linked')",
            [],
        )
        .unwrap();
        let today = day("2021-01-15");
        let available = list_available_loans(&conn, today).unwrap();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].id, "free");
        let linked = list_loans_for_real_estate(&conn, "re1", today).unwrap();
        assert_eq!(linked.len(), 1);
        assert_eq!(linked[0].id, "linked");
        assert_eq!(linked[0].outstanding_balance, "985246.30");
    }

    fn mortgage_data() -> InsertLoan {
        let mut data = valid_insert();
        data.start_date = Some(day("2020-01-15"));
        data.principal = "1000000".into();
        data.interest_rate = Some("5".into());
        data.monthly_payment = Some("5368.22".into());
        data
    }

    #[test]
    fn update_switches_the_manual_balance_on_and_off() {
        let conn = setup_test_db();
        let mut data = mortgage_data();
        let created = create_loan(&conn, &data).unwrap();
        assert_eq!(created.balance_anchor_amount, None);
        assert_eq!(created.balance_mode, "estimated");

        // On: a record of that day, and it holds long after (the payments do not amortize it).
        data.balance_anchor_amount = Some("700000".into());
        data.balance_anchor_date = Some(day("2026-01-01"));
        let with_anchor = update_loan(&conn, &created.id, &data).unwrap();
        assert_eq!(with_anchor.balance_anchor_amount.as_deref(), Some("700000"));
        assert_eq!(with_anchor.balance_anchor_date, Some(day("2026-01-01")));
        assert_eq!(with_anchor.balance_mode, "manual");
        assert_eq!(records(&conn, &created.id).len(), 1);
        let later = get_loan(&conn, &created.id, day("2030-01-01")).unwrap();
        assert_eq!(later.outstanding_balance, "700000.00");
        let before = get_loan(&conn, &created.id, day("2025-12-31")).unwrap();
        assert_eq!(before.balance_mode, "estimated");

        // Off: every manual record goes and the loan is estimated again.
        data.balance_anchor_amount = None;
        data.balance_anchor_date = None;
        let cleared = update_loan(&conn, &created.id, &data).unwrap();
        assert_eq!(cleared.balance_anchor_amount, None);
        assert_eq!(cleared.balance_anchor_date, None);
        assert_eq!(cleared.balance_mode, "estimated");
        assert!(records(&conn, &created.id).is_empty());
    }

    #[test]
    fn update_replaces_the_record_of_the_same_day_and_keeps_the_older_ones() {
        let conn = setup_test_db();
        let mut data = mortgage_data();
        let created = create_loan(&conn, &data).unwrap();

        data.balance_anchor_amount = Some("700000".into());
        data.balance_anchor_date = Some(day("2025-01-01"));
        update_loan(&conn, &created.id, &data).unwrap();
        // The same day with a new amount replaces it.
        data.balance_anchor_amount = Some("690000".into());
        update_loan(&conn, &created.id, &data).unwrap();
        assert_eq!(
            records(&conn, &created.id),
            vec![(day("2025-01-01"), "690000".to_string())]
        );
        // A later day adds a record; the older one stays and is no longer the mirror.
        data.balance_anchor_amount = Some("650000".into());
        data.balance_anchor_date = Some(day("2025-07-01"));
        let loan = update_loan(&conn, &created.id, &data).unwrap();
        assert_eq!(
            records(&conn, &created.id),
            vec![
                (day("2025-01-01"), "690000".to_string()),
                (day("2025-07-01"), "650000".to_string())
            ]
        );
        assert_eq!(loan.balance_anchor_amount.as_deref(), Some("650000"));
        assert_eq!(loan.balance_anchor_date, Some(day("2025-07-01")));
        // Between the records the first holds.
        let between = get_loan(&conn, &created.id, day("2025-04-01")).unwrap();
        assert_eq!(between.outstanding_balance, "690000.00");
        // Saving the unchanged form again changes nothing.
        let again = update_loan(&conn, &created.id, &data).unwrap();
        assert_eq!(again.events.len(), 2);
        assert_eq!(again.events[1].id, loan.events[1].id);
    }

    #[test]
    fn switching_off_deletes_every_manual_record_but_not_the_other_events() {
        let conn = setup_test_db();
        let mut data = mortgage_data();
        data.balance_anchor_amount = Some("700000".into());
        data.balance_anchor_date = Some(day("2025-01-01"));
        let created = create_loan(&conn, &data).unwrap();
        put_event(
            &conn,
            &created.id,
            "balance_check",
            day("2025-06-01"),
            Some("650000"),
            5,
        );
        put_event(
            &conn,
            &created.id,
            "extra_payment",
            day("2025-03-01"),
            Some("10000"),
            6,
        );
        data.balance_anchor_amount = None;
        data.balance_anchor_date = None;
        let cleared = update_loan(&conn, &created.id, &data).unwrap();
        assert!(records(&conn, &created.id).is_empty());
        assert_eq!(cleared.events.len(), 1);
        assert_eq!(cleared.events[0].kind, "extra_payment");
        assert_eq!(cleared.balance_anchor_amount, None);
        assert_eq!(cleared.balance_mode, "estimated");
    }

    #[test]
    fn update_checks_the_manual_balance_against_the_stored_start() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.start_date = Some(day("2020-01-15"));
        let created = create_loan(&conn, &data).unwrap();

        // start_date omitted on update: the stored start (2020-01-15) applies
        data.start_date = None;
        data.balance_anchor_amount = Some("5000".into());
        data.balance_anchor_date = Some(day("2019-12-31"));
        match update_loan(&conn, &created.id, &data).unwrap_err() {
            AppError::Validation(msg) => assert_eq!(msg, "validation.balanceAnchorBeforeStart"),
            other => panic!("expected Validation error, got {other:?}"),
        }
    }

    #[test]
    fn update_of_an_unknown_loan_is_not_found() {
        let conn = setup_test_db();
        match update_loan(&conn, "nope", &valid_insert()).unwrap_err() {
            AppError::NotFound(_) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[test]
    fn outstanding_by_currency_sums_the_amortized_balances() {
        let conn = setup_test_db();
        mortgage(&conn, "czk-1", "CZK");
        mortgage(&conn, "czk-2", "CZK");
        mortgage(&conn, "eur-1", "EUR");
        let totals = outstanding_by_currency(&conn, day("2021-01-15")).unwrap();
        assert!((totals["CZK"] - 2.0 * 985_246.30).abs() < 0.1);
        assert!((totals["EUR"] - 985_246.30).abs() < 0.1);
        assert_eq!(totals.len(), 2);
    }

    #[test]
    fn schedule_reports_progress_and_the_payoff() {
        let conn = setup_test_db();
        mortgage(&conn, "m1", "CZK");
        let schedule = loan_schedule(&conn, "m1", day("2021-01-15")).unwrap();
        assert_eq!(schedule.as_of_day, day("2021-01-15"));
        assert_eq!(schedule.anchor_amount, "1000000.00");
        assert_eq!(schedule.anchor_day, day("2020-01-15"));
        assert!(!schedule.anchor_is_manual);
        assert_eq!(schedule.outstanding_balance, "985246.30");
        assert_eq!(schedule.principal_repaid, "14753.70");
        assert_eq!(schedule.payments_made, 12);
        assert_eq!(schedule.payments_remaining, Some(348));
        assert_eq!(schedule.payoff_day, Some(day("2050-01-15")));
        assert_eq!(schedule.rows.len(), 360);
        assert_eq!(schedule.rows[0].due_day, day("2020-02-15"));
        assert_eq!(schedule.rows[0].balance_after, "998798.45");
        assert_eq!(schedule.rows[359].balance_after, "0.00");
        assert!(schedule.warnings.is_empty());
        // 12 payments of 5368.22 = principal + interest
        let interest: f64 = schedule.interest_paid.parse().unwrap();
        assert!((interest + 14_753.70 - 12.0 * 5_368.22).abs() < 0.05);
    }

    #[test]
    fn schedule_warns_when_there_is_no_payment() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date)
             VALUES ('l1', 'No payment', '50000', '4', '0', ?1)",
            [day("2024-01-10")],
        )
        .unwrap();
        let schedule = loan_schedule(&conn, "l1", day("2025-01-10")).unwrap();
        assert_eq!(schedule.warnings, vec!["noPayment"]);
        assert!(schedule.rows.is_empty());
        assert_eq!(schedule.outstanding_balance, "50000.00");
        assert_eq!(schedule.payments_remaining, None);
        assert_eq!(schedule.payoff_day, None);
    }

    #[test]
    fn schedule_warns_when_the_payment_does_not_cover_the_interest() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date)
             VALUES ('l1', 'Underpaid', '1000000', '6', '3000', ?1)",
            [day("2024-01-10")],
        )
        .unwrap();
        let schedule = loan_schedule(&conn, "l1", day("2025-01-10")).unwrap();
        assert!(schedule
            .warnings
            .contains(&"paymentBelowInterest".to_string()));
        assert!(schedule.warnings.contains(&"scheduleTruncated".to_string()));
        assert_eq!(schedule.outstanding_balance, "1024671.12");
        assert_eq!(schedule.principal_repaid, "-24671.12");
        assert_eq!(schedule.rows.len(), MAX_SCHEDULE_ROWS);
    }

    #[test]
    fn schedule_warns_when_a_balance_is_left_at_the_end_date() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date, end_date)
             VALUES ('l1', 'Short', '100000', '5', '2000', ?1, ?2)",
            rusqlite::params![day("2024-01-15"), day("2025-01-15")],
        )
        .unwrap();
        let schedule = loan_schedule(&conn, "l1", day("2024-06-01")).unwrap();
        assert_eq!(schedule.warnings, vec!["balanceAtEndDate"]);
        assert_eq!(schedule.rows.len(), 12);
        assert_eq!(schedule.payments_remaining, None);
    }

    #[test]
    fn schedule_of_a_paid_off_loan_has_no_payments_left() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date)
             VALUES ('l1', 'Small', '10000', '12', '3000', ?1)",
            [day("2024-01-31")],
        )
        .unwrap();
        let schedule = loan_schedule(&conn, "l1", day("2030-01-01")).unwrap();
        assert_eq!(schedule.outstanding_balance, "0.00");
        assert_eq!(schedule.payments_remaining, Some(0));
        assert_eq!(schedule.payoff_day, Some(day("2024-05-31")));
        assert_eq!(schedule.payments_made, 4);
        assert_eq!(schedule.rows.len(), 4);
        assert_eq!(schedule.rows[3].payment, "1224.84");
    }

    #[test]
    fn schedule_after_a_manual_balance_holds_it_and_plans_from_today() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date)
             VALUES ('l1', 'Overridden', '1000000', '4.2', '6000', ?1)",
            [day("2020-01-15")],
        )
        .unwrap();
        put_event(
            &conn,
            "l1",
            "balance_check",
            day("2025-06-15"),
            Some("800000"),
            1,
        );
        let schedule = loan_schedule(&conn, "l1", day("2025-09-20")).unwrap();
        assert!(schedule.anchor_is_manual);
        // The balance holds: no payment has amortized it since the record.
        assert_eq!(schedule.outstanding_balance, "800000.00");
        assert_eq!(schedule.anchor_amount, "800000.00");
        assert_eq!(schedule.anchor_day, day("2025-09-20"));
        assert_eq!(schedule.payments_made, 0);
        assert_eq!(schedule.principal_repaid, "0.00");
        assert_eq!(schedule.interest_paid, "0.00");
        // The plan ahead pays on the loan's 15th, starting after today.
        assert_eq!(schedule.rows[0].due_day, day("2025-10-15"));
        assert_eq!(schedule.rows[0].balance_after, "796800.00");
        assert_eq!(
            schedule.payments_remaining,
            Some(schedule.rows.len() as u32)
        );
    }

    #[test]
    fn schedule_continues_from_an_estimated_segment_after_an_extra_payment() {
        let conn = setup_test_db();
        mortgage(&conn, "m1", "CZK");
        put_event(
            &conn,
            "m1",
            "extra_payment",
            day("2021-01-20"),
            Some("100000"),
            1,
        );
        let schedule = loan_schedule(&conn, "m1", day("2021-03-25")).unwrap();
        assert!(!schedule.anchor_is_manual);
        // 12 payments to 15 Jan 2021, then the extra payment on 20 Jan starts a segment
        // whose payments fall on the 20th.
        assert_eq!(schedule.anchor_day, day("2021-01-20"));
        assert_eq!(schedule.anchor_amount, "885246.30");
        assert_eq!(schedule.payments_made, 2);
        assert_eq!(schedule.rows[0].due_day, day("2021-02-20"));
    }

    #[test]
    fn a_loan_that_has_not_started_is_worth_nothing_today() {
        let conn = setup_test_db();
        mortgage(&conn, "m1", "CZK");
        let early = get_loan(&conn, "m1", day("2019-12-31")).unwrap();
        assert_eq!(early.outstanding_balance, "0.00");
        assert_eq!(early.balance_mode, "estimated");
        assert_eq!(
            outstanding_by_currency(&conn, day("2019-12-31")).unwrap()["CZK"],
            0.0
        );
    }

    #[test]
    fn events_come_with_the_loan_oldest_first() {
        let conn = setup_test_db();
        mortgage(&conn, "m1", "CZK");
        put_event(
            &conn,
            "m1",
            "balance_check",
            day("2022-01-10"),
            Some("900000"),
            3,
        );
        put_event(
            &conn,
            "m1",
            "extra_payment",
            day("2021-01-20"),
            Some("1000"),
            9,
        );
        put_event(
            &conn,
            "m1",
            "extra_payment",
            day("2021-01-20"),
            Some("2000"),
            4,
        );
        let loans = list_loans(&conn, day("2023-01-01")).unwrap();
        let order: Vec<(i64, i64)> = loans[0]
            .events
            .iter()
            .map(|e| (e.event_day, e.created_at))
            .collect();
        assert_eq!(
            order,
            vec![
                (day("2021-01-20"), 4),
                (day("2021-01-20"), 9),
                (day("2022-01-10"), 3)
            ]
        );
        assert_eq!(loans[0].balance_mode, "manual");
        // The lists that filter loans carry the events too.
        assert_eq!(
            list_available_loans(&conn, day("2023-01-01")).unwrap()[0]
                .events
                .len(),
            3
        );
    }

    #[test]
    fn todays_value_everywhere_is_the_ledger_balance() {
        let conn = setup_test_db();
        mortgage(&conn, "m1", "CZK");
        mortgage(&conn, "m2", "EUR");
        put_event(
            &conn,
            "m1",
            "extra_payment",
            day("2021-01-20"),
            Some("100000"),
            1,
        );
        put_event(
            &conn,
            "m2",
            "balance_check",
            day("2021-06-01"),
            Some("500000"),
            1,
        );
        let today = day("2023-05-05");
        let totals = outstanding_by_currency(&conn, today).unwrap();
        for loan in list_loans(&conn, today).unwrap() {
            let ledger = loan_ledger(&loan).unwrap();
            let balance = loan_balance_at(&ledger, today).balance;
            assert_eq!(loan.outstanding_balance, money_text(balance));
            assert_eq!(totals[&loan.currency], balance);
        }
        assert_eq!(totals["EUR"], 500_000.0);
    }

    #[test]
    fn a_corrupt_event_amount_is_an_error_not_a_zero() {
        let conn = setup_test_db();
        mortgage(&conn, "m1", "CZK");
        put_event(
            &conn,
            "m1",
            "extra_payment",
            day("2021-01-20"),
            Some("abc"),
            1,
        );
        assert!(list_loans(&conn, day("2023-01-01")).is_err());
        assert!(outstanding_by_currency(&conn, day("2023-01-01")).is_err());
    }
}
