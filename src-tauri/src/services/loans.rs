//! Loans business logic. Extracted from commands/loans.rs so the Tauri
//! command and the MCP write tool share one validated path (ADR 0007).
//!
//! Every loan read goes through here so each `Loan` carries its amortized
//! `outstanding_balance`; the maths lives in `services/loan_amortization.rs`.

use crate::error::{AppError, Result};
use crate::models::loans::anchor_before_start;
use crate::models::{InsertLoan, Loan, LoanSchedule, LoanScheduleRow};
use crate::services::loan_amortization::{
    self as amortization, day_floor, today_utc_day, LoanTerms, ScheduleEnd, MAX_SCHEDULE_ROWS,
};
use rusqlite::{Connection, OptionalExtension, Row};
use std::collections::HashMap;
use uuid::Uuid;

/// Column list of `loans`, qualified with the alias `l` every query uses.
const LOAN_COLUMNS: &str = "l.id, l.name, l.principal, l.currency, l.interest_rate,
    l.interest_rate_validity_date, l.monthly_payment, l.start_date, l.end_date,
    l.balance_anchor_amount, l.balance_anchor_date, l.created_at, l.updated_at";

/// Money TEXT with 2 decimals; never "-0.00".
fn money_text(value: f64) -> String {
    let rounded = (value * 100.0).round() / 100.0;
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

/// Parsed amortization inputs from the stored loan columns.
#[allow(clippy::too_many_arguments)]
fn terms_from_columns(
    principal: &str,
    interest_rate: &str,
    monthly_payment: &str,
    start_date: i64,
    end_date: Option<i64>,
    anchor_amount: Option<&str>,
    anchor_date: Option<i64>,
) -> Result<LoanTerms> {
    let anchor = match (anchor_amount, anchor_date) {
        (Some(amount), Some(day)) if !amount.trim().is_empty() => Some((
            parse_amount(amount, "balance_anchor_amount")?,
            day_floor(day),
        )),
        _ => None,
    };
    Ok(LoanTerms {
        principal: parse_amount(principal, "principal")?,
        annual_rate_pct: parse_amount(interest_rate, "interest_rate")?,
        monthly_payment: parse_amount(monthly_payment, "monthly_payment")?,
        start_day: day_floor(start_date),
        anchor_amount: anchor.map(|(amount, _)| amount),
        anchor_day: anchor.map(|(_, day)| day),
        end_day: end_date.map(day_floor),
    })
}

/// Amortization inputs of a loan.
pub fn loan_terms(loan: &Loan) -> Result<LoanTerms> {
    terms_from_columns(
        &loan.principal,
        &loan.interest_rate,
        &loan.monthly_payment,
        loan.start_date,
        loan.end_date,
        loan.balance_anchor_amount.as_deref(),
        loan.balance_anchor_date,
    )
}

/// Row in `LOAN_COLUMNS` order; `outstanding_balance` is filled by the caller.
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
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
    })
}

fn fill_outstanding(loan: &mut Loan, today: i64) -> Result<()> {
    let terms = loan_terms(loan)?;
    loan.outstanding_balance = money_text(amortization::outstanding_balance_at(&terms, today));
    Ok(())
}

/// Loans matching `FROM loans l <sql_tail>`, valued as of `today`.
fn query_loans(
    conn: &Connection,
    sql_tail: &str,
    params: impl rusqlite::Params,
    today: i64,
) -> Result<Vec<Loan>> {
    let sql = format!("SELECT {LOAN_COLUMNS} FROM loans l {sql_tail}");
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params, read_loan)?;
    let mut loans = Vec::new();
    for row in rows {
        let mut loan = row?;
        fill_outstanding(&mut loan, today)?;
        loans.push(loan);
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
    let currency = data.currency.clone().unwrap_or_else(|| "CZK".to_string());
    let interest_rate = data
        .interest_rate
        .clone()
        .unwrap_or_else(|| "0".to_string());
    let monthly_payment = data
        .monthly_payment
        .clone()
        .unwrap_or_else(|| "0".to_string());
    let start_date = data.start_date.unwrap_or(now);
    let anchor = data.balance_anchor();

    // InsertLoan::validate() only checks ordering when BOTH start_date and
    // end_date are provided by the caller; start_date defaults to `now` right
    // above, so an end_date-only input (end_date <= now) would otherwise
    // persist an end-before-start loan. Re-check against the effective date.
    check_dates(
        start_date,
        data.end_date,
        anchor.as_ref().map(|(_, day)| *day),
    )?;

    conn.execute(
        "INSERT INTO loans (id, name, principal, currency, interest_rate, interest_rate_validity_date,
         monthly_payment, start_date, end_date, balance_anchor_amount, balance_anchor_date,
         created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12)",
        rusqlite::params![
            id,
            data.name,
            data.principal,
            currency,
            interest_rate,
            data.interest_rate_validity_date,
            monthly_payment,
            start_date,
            data.end_date,
            anchor.as_ref().map(|(amount, _)| amount),
            anchor.as_ref().map(|(_, day)| day),
            now
        ],
    )?;

    get_loan(conn, &id, today_utc_day())
}

/// Update a loan. A missing manual balance clears a previous one (the loan
/// goes back to amortizing from its principal).
pub fn update_loan(conn: &Connection, id: &str, data: &InsertLoan) -> Result<Loan> {
    data.validate()?;

    let existing_start: i64 = conn
        .query_row("SELECT start_date FROM loans WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .optional()?
        .ok_or_else(|| AppError::NotFound("Loan not found".into()))?;
    let anchor = data.balance_anchor();
    check_dates(
        data.start_date.unwrap_or(existing_start),
        data.end_date,
        anchor.as_ref().map(|(_, day)| *day),
    )?;

    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE loans SET name = ?1, principal = ?2,
         currency = COALESCE(?3, currency), interest_rate = COALESCE(?4, interest_rate),
         interest_rate_validity_date = ?5, monthly_payment = COALESCE(?6, monthly_payment),
         start_date = COALESCE(?7, start_date), end_date = ?8,
         balance_anchor_amount = ?9, balance_anchor_date = ?10, updated_at = ?11
         WHERE id = ?12",
        rusqlite::params![
            data.name,
            data.principal,
            data.currency,
            data.interest_rate,
            data.interest_rate_validity_date,
            data.monthly_payment,
            data.start_date,
            data.end_date,
            anchor.as_ref().map(|(amount, _)| amount),
            anchor.as_ref().map(|(_, day)| day),
            now,
            id
        ],
    )?;

    get_loan(conn, id, today_utc_day())
}

/// Every loan's currency and amortization inputs (net worth, projection).
pub fn loan_terms_with_currency(conn: &Connection) -> Result<Vec<(String, LoanTerms)>> {
    let mut stmt = conn.prepare(
        "SELECT currency, principal, interest_rate, monthly_payment, start_date, end_date,
                balance_anchor_amount, balance_anchor_date
         FROM loans",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<i64>>(7)?,
        ))
    })?;
    let mut loans = Vec::new();
    for row in rows {
        let (currency, principal, rate, payment, start, end, anchor_amount, anchor_date) = row?;
        let terms = terms_from_columns(
            &principal,
            &rate,
            &payment,
            start,
            end,
            anchor_amount.as_deref(),
            anchor_date,
        )?;
        loans.push((currency, terms));
    }
    Ok(loans)
}

/// Outstanding balance per currency as of `today`: the liabilities of net
/// worth and snapshots (unrounded, like every other native breakdown).
pub fn outstanding_by_currency(conn: &Connection, today: i64) -> Result<HashMap<String, f64>> {
    let mut by_currency: HashMap<String, f64> = HashMap::new();
    for (currency, terms) in loan_terms_with_currency(conn)? {
        *by_currency.entry(currency).or_insert(0.0) +=
            amortization::outstanding_balance_at(&terms, today);
    }
    Ok(by_currency)
}

/// Repayment schedule and totals of one loan as of `today` (loan detail page).
pub fn loan_schedule(conn: &Connection, id: &str, today: i64) -> Result<LoanSchedule> {
    let loan = get_loan(conn, id, today)?;
    let terms = loan_terms(&loan)?;
    let (anchor_amount, anchor_day) = terms.anchor();
    let elapsed = amortization::elapsed_to(&terms, today);
    let schedule = amortization::schedule(&terms, MAX_SCHEDULE_ROWS);

    let mut warnings = Vec::new();
    if terms.monthly_payment <= 0.0 && anchor_amount > 0.0 {
        warnings.push("noPayment".to_string());
    }
    if amortization::payment_below_interest(&terms) {
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
        anchor_is_manual: terms.is_manual_anchor(),
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
            "#,
        )
        .expect("schema");
        conn
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
    fn create_loan_applies_defaults() {
        let conn = setup_test_db();
        let before = chrono::Utc::now().timestamp();
        let loan = create_loan(&conn, &valid_insert()).unwrap();
        assert_eq!(loan.currency, "CZK");
        assert_eq!(loan.interest_rate, "0");
        assert_eq!(loan.monthly_payment, "0");
        assert!(loan.start_date >= before);
        assert_eq!(loan.balance_anchor_amount, None);
        assert_eq!(loan.balance_anchor_date, None);
        // no payment: the balance is the principal
        assert_eq!(loan.outstanding_balance, "10000.00");
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
    fn create_loan_stores_and_returns_the_manual_balance() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.start_date = Some(day("2020-01-15"));
        data.interest_rate = Some("5".into());
        data.monthly_payment = Some("5368.22".into());
        data.principal = "1000000".into();
        data.balance_anchor_amount = Some("800000".into());
        data.balance_anchor_date = Some(day("2025-06-15"));
        let loan = create_loan(&conn, &data).unwrap();
        assert_eq!(loan.balance_anchor_amount.as_deref(), Some("800000"));
        assert_eq!(loan.balance_anchor_date, Some(day("2025-06-15")));
        // amortized from the anchor, not from the principal at the start
        let from_anchor =
            amortization::outstanding_balance_at(&loan_terms(&loan).unwrap(), today_utc_day());
        assert_eq!(loan.outstanding_balance, money_text(from_anchor));
        assert!(from_anchor < 800_000.0);
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

    #[test]
    fn update_sets_and_clears_the_manual_balance() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.start_date = Some(day("2020-01-15"));
        data.principal = "1000000".into();
        data.interest_rate = Some("5".into());
        data.monthly_payment = Some("5368.22".into());
        let created = create_loan(&conn, &data).unwrap();
        assert_eq!(created.balance_anchor_amount, None);

        data.balance_anchor_amount = Some("700000".into());
        data.balance_anchor_date = Some(day("2026-01-01"));
        let with_anchor = update_loan(&conn, &created.id, &data).unwrap();
        assert_eq!(with_anchor.balance_anchor_amount.as_deref(), Some("700000"));
        assert_eq!(with_anchor.balance_anchor_date, Some(day("2026-01-01")));
        let stored = get_loan(&conn, &created.id, day("2026-01-01")).unwrap();
        assert_eq!(stored.outstanding_balance, "700000.00");

        data.balance_anchor_amount = None;
        data.balance_anchor_date = None;
        let cleared = update_loan(&conn, &created.id, &data).unwrap();
        assert_eq!(cleared.balance_anchor_amount, None);
        assert_eq!(cleared.balance_anchor_date, None);
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
    fn schedule_after_a_manual_balance_starts_from_it() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date,
                                balance_anchor_amount, balance_anchor_date)
             VALUES ('l1', 'Overridden', '1000000', '4.2', '6000', ?1, '800000', ?2)",
            rusqlite::params![day("2020-01-15"), day("2025-06-15")],
        )
        .unwrap();
        let schedule = loan_schedule(&conn, "l1", day("2025-09-15")).unwrap();
        assert!(schedule.anchor_is_manual);
        assert_eq!(schedule.anchor_amount, "800000.00");
        assert_eq!(schedule.anchor_day, day("2025-06-15"));
        assert_eq!(schedule.outstanding_balance, "790366.36");
        assert_eq!(schedule.payments_made, 3);
        assert_eq!(schedule.rows[0].due_day, day("2025-07-15"));
    }
}
