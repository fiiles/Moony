//! Actual cashflow from bank transactions (design system README §11).
//!
//! Shared booked ledger interpretation preserves refunds and reversals, separates
//! nonconsumption flows and reports historical FX gaps. Averages name their
//! observed full-month sample; observation never proves statement completeness.

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::cashflow_actuals::{CashflowActuals, CashflowGroup, CashflowMonth};
use crate::models::FlowKind;
use crate::services::reporting::{parse_money, quality, transactions_in_currency};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
#[cfg(test)]
use rusqlite::params;
use rusqlite::Connection;
use std::collections::HashMap;

const DAY: i64 = 86_400;
const UNCATEGORIZED: &str = "uncategorized";

/// Ranges the report offers: whole months ending today, or the current year so far.
pub const RANGES: [&str; 3] = ["6m", "12m", "ytd"];

pub fn validate_range(range: &str) -> Result<()> {
    if RANGES.contains(&range) {
        Ok(())
    } else {
        Err(AppError::Validation(
            "validation.cashflowRangeInvalid".to_string(),
        ))
    }
}

/// Bounds of the range and of the preceding range, as UTC unix seconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RangeBounds {
    pub start: i64,
    pub end: i64,
    pub previous_start: i64,
    pub previous_end: i64,
}

fn month_start(year: i32, month: u32) -> i64 {
    NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
        .unwrap_or(0)
}

/// First day of the month `offset` months before (negative) or after the given one.
fn shift_month(year: i32, month: u32, offset: i32) -> (i32, u32) {
    let index = year * 12 + (month as i32 - 1) + offset;
    (index.div_euclid(12), (index.rem_euclid(12) + 1) as u32)
}

/// `6m`/`12m`: that many whole months ending with today's month, the previous
/// range ending on the corresponding day that many months earlier. `ytd`: January 1 to today, the
/// previous-year range through the corresponding calendar day.
pub fn range_bounds(range: &str, today: i64) -> Result<RangeBounds> {
    validate_range(range)?;
    let day_start = today.div_euclid(DAY) * DAY;
    let end = day_start + DAY - 1;
    let date = DateTime::<Utc>::from_timestamp(day_start, 0)
        .ok_or_else(|| AppError::Internal("invalid timestamp".to_string()))?;
    let (year, month) = (date.year(), date.month());

    if range == "ytd" {
        let start = month_start(year, 1);
        let previous_day = NaiveDate::from_ymd_opt(year - 1, month, date.day())
            .or_else(|| NaiveDate::from_ymd_opt(year - 1, month, 28))
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .ok_or_else(|| AppError::Internal("invalid comparison date".into()))?;
        return Ok(RangeBounds {
            start,
            end,
            previous_start: month_start(year - 1, 1),
            previous_end: previous_day.and_utc().timestamp() + DAY - 1,
        });
    }

    let months: i32 = if range == "6m" { 6 } else { 12 };
    let (sy, sm) = shift_month(year, month, -(months - 1));
    let start = month_start(sy, sm);
    let (py, pm) = shift_month(sy, sm, -months);
    Ok(RangeBounds {
        start,
        end,
        previous_start: month_start(py, pm),
        previous_end: {
            let (ey, em) = shift_month(year, month, -months);
            let mut day = date.day();
            let comparison = loop {
                if let Some(d) = NaiveDate::from_ymd_opt(ey, em, day) {
                    break d;
                }
                day -= 1;
            };
            comparison
                .and_hms_opt(0, 0, 0)
                .expect("midnight")
                .and_utc()
                .timestamp()
                + DAY
                - 1
        },
    })
}

/// First days of every month from `start` to `end`, oldest first.
fn months_between(start: i64, end: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let Some(from) = DateTime::<Utc>::from_timestamp(start, 0) else {
        return out;
    };
    let (mut year, mut month) = (from.year(), from.month());
    loop {
        let ts = month_start(year, month);
        if ts > end {
            break;
        }
        out.push(ts);
        let next = shift_month(year, month, 1);
        year = next.0;
        month = next.1;
    }
    out
}

#[derive(Default)]
struct GroupAcc {
    name: String,
    /// The name comes from the current range when it has rows there (spelling can drift).
    named_in_current: bool,
    amount: f64,
    previous: f64,
    count: i32,
}

impl GroupAcc {
    fn name_from(&mut self, name: String, in_current: bool) {
        if self.name.is_empty() || (in_current && !self.named_in_current) {
            self.name = name;
            self.named_in_current = in_current;
        }
    }
}

pub fn source_key(counterparty: &Option<String>) -> (String, String) {
    let name = counterparty
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("");
    let key = name
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if key.is_empty() {
        ("other".to_string(), String::new())
    } else {
        (key, name.to_string())
    }
}

/// The report for `range`, with `today` as the reference day (unix seconds).
pub fn get_actuals(conn: &Connection, range: &str, today: i64) -> Result<CashflowActuals> {
    get_actuals_scoped(conn, range, today, None)
}
pub fn get_actuals_scoped(
    conn: &Connection,
    range: &str,
    today: i64,
    accounts: Option<&[String]>,
) -> Result<CashflowActuals> {
    get_actuals_in_currency(conn, range, today, accounts, "CZK")
}
pub fn get_actuals_in_currency(
    conn: &Connection,
    range: &str,
    today: i64,
    accounts: Option<&[String]>,
    report_currency: &str,
) -> Result<CashflowActuals> {
    let report_currency = crate::services::reporting::normalize_currency(report_currency)?;
    let money = |v| crate::services::reporting::money_in_currency(v, &report_currency);
    let bounds = range_bounds(range, today)?;
    let rows =
        transactions_in_currency(conn, bounds.start, bounds.end, accounts, &report_currency)?;
    let previous = transactions_in_currency(
        conn,
        bounds.previous_start,
        bounds.previous_end,
        accounts,
        &report_currency,
    )?;
    let current_quality = quality(&rows, bounds.start, bounds.end, today);
    let previous_quality = quality(&previous, bounds.previous_start, bounds.previous_end, today);
    let mut months: HashMap<i64, (f64, f64)> = months_between(bounds.start, bounds.end)
        .into_iter()
        .map(|m| (m, (0.0, 0.0)))
        .collect();
    let mut expenses: HashMap<String, GroupAcc> = HashMap::new();
    let mut sources: HashMap<String, GroupAcc> = HashMap::new();
    let mut other: HashMap<String, GroupAcc> = HashMap::new();
    let mut monthly_other: HashMap<(i64, String), (f64, i32)> = HashMap::new();
    let (mut income, mut expense, mut prev_income, mut prev_expense) = (0.0, 0.0, 0.0, 0.0);
    for (row, current) in rows
        .iter()
        .map(|r| (r, true))
        .chain(previous.iter().map(|r| (r, false)))
    {
        let Some(amount) = row.signed_amount.as_deref() else {
            continue;
        };
        let amount = parse_money(amount)?;
        let kind = row.effective_flow_kind;
        let (key, name, map) = match kind {
            FlowKind::Income => {
                let (k, n) = source_key(&row.counterparty_name);
                (k, n, &mut sources)
            }
            FlowKind::Expense => {
                let id = row
                    .category_id
                    .clone()
                    .unwrap_or_else(|| UNCATEGORIZED.into());
                let name = if let Some(category) = &row.category_id {
                    conn.query_row(
                        "SELECT name FROM transaction_categories WHERE id=?1",
                        [category],
                        |r| r.get(0),
                    )?
                } else {
                    String::new()
                };
                (id, name, &mut expenses)
            }
            _ => (kind.to_string(), kind.to_string(), &mut other),
        };
        let acc = map.entry(key).or_default();
        acc.name_from(name, current);
        if current {
            acc.amount += amount;
        } else {
            acc.previous += amount;
        }
        match (kind, current) {
            (FlowKind::Income, true) => {
                income += amount;
                if let Some(m) = month_of(&mut months, row.booking_date) {
                    m.0 += amount;
                }
            }
            (FlowKind::Expense, true) => {
                expense += amount;
                if let Some(m) = month_of(&mut months, row.booking_date) {
                    m.1 += amount;
                }
            }
            (FlowKind::Income, false) => prev_income += amount,
            (FlowKind::Expense, false) => prev_expense += amount,
            (_, true) => {
                if let Some(month) = crate::services::reporting::month_start(row.booking_date) {
                    let acc = monthly_other.entry((month, kind.to_string())).or_default();
                    acc.0 += amount;
                    acc.1 += 1;
                }
            }
            _ => {}
        }
    }
    // Counts match the drilldown and include rows whose report conversion is
    // missing; a group with only unconverted rows is not added as a zero.
    for row in &rows {
        let (key, map) = match row.effective_flow_kind {
            FlowKind::Income => (source_key(&row.counterparty_name).0, &mut sources),
            FlowKind::Expense => (
                row.category_id
                    .clone()
                    .unwrap_or_else(|| UNCATEGORIZED.into()),
                &mut expenses,
            ),
            kind => (kind.to_string(), &mut other),
        };
        if let Some(acc) = map.get_mut(&key) {
            acc.count += 1;
        }
    }
    let mut months: Vec<_> = months
        .into_iter()
        .map(|(month, (income, expenses))| CashflowMonth {
            bank_cash_movement: money(
                income - expenses
                    + monthly_other
                        .iter()
                        .filter(|((m, _), _)| *m == month)
                        .map(|(_, a)| a.0)
                        .sum::<f64>(),
            ),
            nonconsumption_flows: monthly_other
                .iter()
                .filter(|((m, _), _)| *m == month)
                .map(|((_, key), (amount, count))| CashflowGroup {
                    key: key.clone(),
                    name: key.clone(),
                    amount: money(*amount),
                    previous_amount: "0.00".into(),
                    count: *count,
                })
                .collect(),
            month,
            income: money(income),
            expenses: money(expenses),
        })
        .collect();
    months.sort_by_key(|m| m.month);
    let to_groups = |map: HashMap<String, GroupAcc>| {
        let mut groups: Vec<_> = map
            .into_iter()
            .map(|(key, a)| CashflowGroup {
                key,
                name: a.name,
                amount: money(a.amount),
                previous_amount: money(a.previous),
                count: a.count,
            })
            .collect();
        groups.sort_by(|a, b| {
            b.amount
                .parse::<f64>()
                .expect("formatted amount")
                .total_cmp(&a.amount.parse::<f64>().expect("formatted amount"))
        });
        groups
    };
    let completed: Vec<_> = current_quality
        .observed_completed_months
        .iter()
        .copied()
        .filter(|month| {
            !rows.iter().any(|r| {
                r.fx_quality == crate::models::FxQuality::Missing
                    && crate::services::reporting::month_start(r.booking_date) == Some(*month)
            })
        })
        .collect();
    let averages = if completed.is_empty() {
        (None, None)
    } else {
        let (i, e) =
            months
                .iter()
                .filter(|m| completed.contains(&m.month))
                .fold((0.0, 0.0), |(i, e), m| {
                    (
                        i + m.income.parse::<f64>().expect("amount"),
                        e + m.expenses.parse::<f64>().expect("amount"),
                    )
                });
        (
            Some(money(i / completed.len() as f64)),
            Some(money(e / completed.len() as f64)),
        )
    };
    let account_count = rows
        .iter()
        .map(|r| &r.bank_account_id)
        .collect::<std::collections::HashSet<_>>()
        .len() as i32;
    let nonconsumption_total: f64 = other.values().map(|g| g.amount).sum();
    Ok(CashflowActuals {
        report_currency: report_currency.clone(),
        net_balance: money(income - expense),
        bank_cash_movement: money(income - expense + nonconsumption_total),
        average_completed_months: completed,
        range: range.into(),
        range_start: bounds.start,
        range_end: bounds.end,
        previous_start: bounds.previous_start,
        previous_end: bounds.previous_end,
        months,
        expense_categories: to_groups(expenses),
        income_sources: to_groups(sources),
        nonconsumption_flows: to_groups(other),
        total_income: money(income),
        total_expenses: money(expense),
        previous_income: money(prev_income),
        previous_expenses: money(prev_expense),
        account_count,
        transaction_count: rows.len() as i32,
        quality: current_quality,
        previous_quality,
        average_income: averages.0,
        average_expenses: averages.1,
    })
}

fn month_of(months: &mut HashMap<i64, (f64, f64)>, booking_date: i64) -> Option<&mut (f64, f64)> {
    let date = DateTime::<Utc>::from_timestamp(booking_date, 0)?;
    months.get_mut(&month_start(date.year(), date.month()))
}

// ========================== Async wrapper for Database ==========================

pub async fn get_cashflow_actuals(db: &Database, range: String) -> Result<CashflowActuals> {
    let today = chrono::Utc::now().timestamp();
    db.with_conn(|conn| get_actuals(conn, &range, today))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE transaction_categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL, kind TEXT NOT NULL DEFAULT 'expense'
            );
            CREATE TABLE bank_accounts (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL, iban TEXT, bban TEXT, institution_id TEXT
            );
            CREATE TABLE institutions(id TEXT,country TEXT);
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL,
                booking_date INTEGER NOT NULL,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                counterparty_name TEXT, description TEXT,
                category_id TEXT,
                tx_type TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'booked', flow_kind TEXT, counterparty_iban TEXT, counterparty_bank_code TEXT, counterparty_country TEXT
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            INSERT INTO transaction_categories (id, name) VALUES
                ('cat_income', 'Income'),
                ('cat_groceries', 'Groceries'),
                ('cat_housing', 'Housing'),
                ('cat_internal_transfers', 'Internal Transfers');
            INSERT INTO bank_accounts (id, name) VALUES ('acc1', 'Main'), ('acc2', 'Savings');
            "#,
        )
        .expect("schema");
        conn.execute(
            "UPDATE transaction_categories SET kind='income' WHERE id='cat_income'",
            [],
        )
        .unwrap();
        conn.execute(
            "UPDATE transaction_categories SET kind='transfer' WHERE id='cat_internal_transfers'",
            [],
        )
        .unwrap();
        conn
    }

    fn ts(year: i32, month: u32, day: u32) -> i64 {
        NaiveDate::from_ymd_opt(year, month, day)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_tx(
        conn: &Connection,
        id: &str,
        account: &str,
        date: i64,
        amount: &str,
        currency: &str,
        counterparty: Option<&str>,
        category: Option<&str>,
        tx_type: &str,
    ) {
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, counterparty_name, category_id, tx_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![id, account, date, amount, currency, counterparty, category, tx_type],
        )
        .unwrap();
    }

    const TODAY: i64 = 1_791_000_000; // 2026-10-03 (UTC)

    /// Group counts match the drilldown (all relevant rows), even when a row
    /// lacks a report quote; its amount stays out of the partial total.
    #[test]
    fn group_counts_include_rows_without_report_conversion() {
        let conn = setup_test_db();
        for (id, currency) in [("czk", "CZK"), ("aed", "AED")] {
            insert_tx(
                &conn,
                id,
                "acc1",
                ts(2026, 9, 1),
                "100",
                currency,
                None,
                Some("cat_groceries"),
                "debit",
            );
        }
        // Only unconverted spending: no healthy-looking zero group is added.
        insert_tx(
            &conn,
            "aed-only",
            "acc1",
            ts(2026, 9, 1),
            "50",
            "AED",
            None,
            None,
            "debit",
        );
        let report = get_actuals_in_currency(&conn, "6m", TODAY, None, "CZK").unwrap();
        assert_eq!(report.quality.missing_fx_count, 2);
        assert_eq!(report.expense_categories.len(), 1);
        assert_eq!(report.expense_categories[0].amount, "100.00");
        assert_eq!(report.expense_categories[0].count, 2);
    }
    #[test]
    fn target_actuals_aed_and_jpy_work_without_quotes() {
        for (currency, amount, expected) in [
            ("AED", "100", "100.00"),
            ("JPY", "100.6", "101"),
            ("KWD", "0.123", "0.123"),
        ] {
            let conn = setup_test_db();
            insert_tx(
                &conn,
                "native",
                "acc1",
                ts(2026, 9, 1),
                amount,
                currency,
                None,
                Some("cat_income"),
                "credit",
            );
            let report = get_actuals_in_currency(&conn, "6m", TODAY, None, currency).unwrap();
            assert_eq!(report.total_income, expected);
            assert_eq!(report.quality.missing_fx_count, 0);
        }
    }
    #[test]
    fn target_actuals_comparison_is_historical_and_conserves_cross_rows() {
        let conn = setup_test_db();
        for (id, day) in [("current", ts(2026, 9, 1)), ("previous", ts(2026, 3, 1))] {
            insert_tx(
                &conn,
                id,
                "acc1",
                day,
                "110",
                "XAC",
                None,
                Some("cat_income"),
                "credit",
            );
            conn.execute(
                "INSERT INTO exchange_rate_history VALUES (?1,'XAC',1.1)",
                [day],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO exchange_rate_history VALUES (?1,'XAT',1.0)",
                [day],
            )
            .unwrap();
        }
        let first = get_actuals_in_currency(&conn, "6m", TODAY, None, "XAT").unwrap();
        assert_eq!(first.total_income, "100.00");
        assert_eq!(first.previous_income, "100.00");
        crate::services::fx::set_current_for_test(&[("XAC", 0.125), ("XAT", 1.0)]);
        let second = get_actuals_in_currency(&conn, "6m", TODAY, None, "XAT").unwrap();
        assert_eq!(first.total_income, second.total_income);
        assert_eq!(first.previous_income, second.previous_income);
    }
    #[test]
    fn target_actuals_native_eur_refunds_and_drilldowns_conserve() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "eur",
            "acc1",
            ts(2026, 9, 1),
            "100",
            "EUR",
            None,
            Some("cat_groceries"),
            "debit",
        );
        insert_tx(
            &conn,
            "refund",
            "acc1",
            ts(2026, 9, 2),
            "25",
            "EUR",
            None,
            Some("cat_groceries"),
            "credit",
        );
        let report = get_actuals_in_currency(&conn, "6m", TODAY, None, "EUR").unwrap();
        assert_eq!(report.total_expenses, "75.00");
        assert_eq!(report.quality.missing_fx_count, 0);
        assert_eq!(report.report_currency, "EUR");
        let rows = crate::services::reporting::filtered_transactions_in_currency(
            &conn,
            report.range_start,
            report.range_end,
            None,
            Some(FlowKind::Expense),
            Some("cat_groceries"),
            None,
            "EUR",
        )
        .unwrap();
        assert_eq!(
            rows.iter()
                .filter_map(|r| r.signed_amount.as_deref())
                .map(|a| parse_money(a).unwrap())
                .sum::<f64>(),
            75.0
        );
    }
    #[test]
    fn signed_report_conserves_cross_category_refunds_income_reversals_and_pending() {
        let conn = setup_test_db();
        conn.execute_batch("INSERT INTO transaction_categories(id,name,kind) VALUES ('custom-income','Consulting','income');
          INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,category_id,tx_type) VALUES
          ('d','acc1',1790812800,'1000','CZK','cat_groceries','debit'),
          ('r','acc1',1790899200,'500','CZK','cat_housing','credit'),
          ('i','acc1',1790812800,'1000','CZK','cat_income','credit'),
          ('ir','acc1',1790812800,'100','CZK','cat_income','debit'),
          ('ci','acc1',1790812800,'2000','CZK','custom-income','credit');").unwrap();
        conn.execute("INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,tx_type,status) VALUES ('pending','acc1',1790812800,'9999','CZK','debit','pending')",[]).unwrap();
        let report = get_actuals(&conn, "6m", TODAY).unwrap();
        assert_eq!(report.total_expenses, "500.00");
        assert_eq!(report.total_income, "2900.00");
        assert!(report
            .expense_categories
            .iter()
            .any(|g| g.amount == "-500.00"));
        let expense_sum: f64 = report
            .months
            .iter()
            .map(|m| m.expenses.parse::<f64>().unwrap())
            .sum();
        assert_eq!(expense_sum, 500.0);
    }

    #[test]
    fn calendar_ytd_clamps_leap_day_and_keeps_corresponding_march_day() {
        let leap = range_bounds("ytd", ts(2024, 2, 29)).unwrap();
        assert_eq!(leap.previous_start, ts(2023, 1, 1));
        assert_eq!(leap.previous_end, ts(2023, 3, 1) - 1);
        let march = range_bounds("ytd", ts(2025, 3, 1)).unwrap();
        assert_eq!(march.previous_end, ts(2024, 3, 2) - 1);
    }
    #[test]
    fn negative_months_nonconsumption_and_observed_averages_reconcile() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "d",
            "acc1",
            ts(2026, 8, 1),
            "1000",
            "CZK",
            None,
            Some("cat_housing"),
            "debit",
        );
        insert_tx(
            &conn,
            "r",
            "acc1",
            ts(2026, 9, 1),
            "1500",
            "CZK",
            None,
            Some("cat_groceries"),
            "credit",
        );
        insert_tx(
            &conn,
            "t",
            "acc2",
            ts(2026, 9, 2),
            "700",
            "CZK",
            None,
            Some("cat_internal_transfers"),
            "credit",
        );
        let report = get_actuals(&conn, "6m", TODAY).unwrap();
        assert_eq!(report.total_expenses, "-500.00");
        assert_eq!(report.net_balance, "500.00");
        assert_eq!(report.bank_cash_movement, "1200.00");
        assert_eq!(report.nonconsumption_flows[0].amount, "700.00");
        let expense: f64 = report
            .months
            .iter()
            .map(|m| m.expenses.parse::<f64>().unwrap())
            .sum();
        assert_eq!(expense, -500.0);
        let movement: f64 = report
            .months
            .iter()
            .map(|m| m.bank_cash_movement.parse::<f64>().unwrap())
            .sum();
        assert_eq!(movement, 1200.0);
        assert_eq!(report.average_expenses.as_deref(), Some("-250.00"));
        assert_eq!(
            report.average_completed_months,
            vec![ts(2026, 8, 1), ts(2026, 9, 1)]
        );
        assert_eq!(report.quality.unobserved_month_count, 3);
        assert_eq!(report.quality.partial_month_count, 1);
    }

    #[test]
    fn six_months_end_today_and_start_on_a_month_boundary() {
        let b = range_bounds("6m", TODAY).unwrap();
        assert_eq!(b.start, ts(2026, 5, 1));
        assert_eq!(b.end, ts(2026, 10, 4) - 1);
        assert_eq!(b.previous_start, ts(2025, 11, 1));
        assert_eq!(b.previous_end, ts(2026, 4, 4) - 1);
        assert_eq!(months_between(b.start, b.end).len(), 6);
    }

    #[test]
    fn twelve_months_span_the_year_change() {
        let b = range_bounds("12m", TODAY).unwrap();
        assert_eq!(b.start, ts(2025, 11, 1));
        assert_eq!(b.previous_start, ts(2024, 11, 1));
        assert_eq!(months_between(b.start, b.end).len(), 12);
    }

    #[test]
    fn year_to_date_compares_to_the_same_number_of_days_before() {
        let b = range_bounds("ytd", TODAY).unwrap();
        assert_eq!(b.start, ts(2026, 1, 1));
        assert_eq!(b.previous_start, ts(2025, 1, 1));
        assert_eq!(b.previous_end, ts(2025, 10, 4) - 1);
        assert_eq!(b.end - b.start, b.previous_end - b.previous_start);
    }

    #[test]
    fn rejects_unknown_ranges() {
        assert!(matches!(
            range_bounds("3m", TODAY),
            Err(AppError::Validation(_))
        ));
        assert!(validate_range("ytd").is_ok());
    }

    #[test]
    fn empty_database_gives_zero_filled_months() {
        let conn = setup_test_db();
        let report = get_actuals(&conn, "6m", TODAY).unwrap();
        assert_eq!(report.months.len(), 6);
        assert!(report
            .months
            .iter()
            .all(|m| m.income == "0.00" && m.expenses == "0.00"));
        assert_eq!(report.total_income, "0.00");
        assert_eq!(report.account_count, 0);
        assert!(report.expense_categories.is_empty());
    }

    #[test]
    fn sums_by_month_category_and_source_with_previous_range_and_dated_fx() {
        let conn = setup_test_db();
        // Current range (May–Oct 2026)
        insert_tx(
            &conn,
            "t1",
            "acc1",
            ts(2026, 9, 30),
            "50000",
            "CZK",
            Some("Northwind a.s."),
            Some("cat_income"),
            "credit",
        );
        insert_tx(
            &conn,
            "t2",
            "acc1",
            ts(2026, 9, 5),
            "1200",
            "CZK",
            Some("Albert"),
            Some("cat_groceries"),
            "debit",
        );
        insert_tx(
            &conn,
            "t3",
            "acc1",
            ts(2026, 9, 7),
            "200",
            "CZK",
            Some("Albert"),
            Some("cat_groceries"),
            "credit",
        ); // refund
        insert_tx(
            &conn,
            "t4",
            "acc1",
            ts(2026, 8, 1),
            "15000",
            "CZK",
            Some("Banka"),
            Some("cat_housing"),
            "debit",
        );
        insert_tx(
            &conn,
            "t5",
            "acc2",
            ts(2026, 8, 20),
            "3000",
            "CZK",
            Some("Jan Novák"),
            None,
            "credit",
        ); // uncategorized credit = income
        insert_tx(
            &conn,
            "t6",
            "acc1",
            ts(2026, 8, 25),
            "100",
            "EUR",
            Some("Revolut"),
            None,
            "debit",
        ); // converted with the day's rate
        insert_tx(
            &conn,
            "t7",
            "acc1",
            ts(2026, 8, 26),
            "9999",
            "CZK",
            None,
            Some("cat_internal_transfers"),
            "debit",
        ); // excluded
           // Previous range (Nov 2025–Apr 2026)
        insert_tx(
            &conn,
            "p1",
            "acc1",
            ts(2026, 3, 30),
            "48000",
            "CZK",
            Some("NORTHWIND  a.s."),
            Some("cat_income"),
            "credit",
        );
        insert_tx(
            &conn,
            "p2",
            "acc1",
            ts(2026, 3, 2),
            "14000",
            "CZK",
            Some("Banka"),
            Some("cat_housing"),
            "debit",
        );
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'CZK', 25.0)",
            params![ts(2026, 8, 24)],
        )
        .unwrap();

        let report = get_actuals(&conn, "6m", TODAY).unwrap();

        assert_eq!(report.total_income, "53000.00");
        // 1200 − 200 refund + 15000 + 100 EUR × 25 (rate of the nearest earlier day)
        assert_eq!(report.total_expenses, "18500.00");
        assert_eq!(report.previous_income, "48000.00");
        assert_eq!(report.previous_expenses, "14000.00");
        assert_eq!(report.account_count, 2);
        assert_eq!(report.transaction_count, 7);

        let sep = report
            .months
            .iter()
            .find(|m| m.month == ts(2026, 9, 1))
            .unwrap();
        assert_eq!(sep.income, "50000.00");
        assert_eq!(sep.expenses, "1000.00");
        let aug = report
            .months
            .iter()
            .find(|m| m.month == ts(2026, 8, 1))
            .unwrap();
        assert_eq!(aug.income, "3000.00");
        assert_eq!(aug.expenses, "17500.00");

        let housing = report
            .expense_categories
            .iter()
            .find(|g| g.key == "cat_housing")
            .unwrap();
        assert_eq!(housing.amount, "15000.00");
        assert_eq!(housing.previous_amount, "14000.00");
        assert_eq!(housing.name, "Housing");
        assert_eq!(report.expense_categories[0].key, "cat_housing");
        let uncategorized = report
            .expense_categories
            .iter()
            .find(|g| g.key == "uncategorized")
            .unwrap();
        assert_eq!(uncategorized.amount, "2500.00");
        assert!(report
            .expense_categories
            .iter()
            .all(|g| g.key != "cat_internal_transfers"));

        // Sources: counterparty normalized (case and spacing), largest first
        assert_eq!(report.income_sources[0].key, "northwind a.s.");
        assert_eq!(report.income_sources[0].name, "Northwind a.s.");
        assert_eq!(report.income_sources[0].amount, "50000.00");
        assert_eq!(report.income_sources[0].previous_amount, "48000.00");
        assert_eq!(report.income_sources[1].key, "jan novák");
    }
}
