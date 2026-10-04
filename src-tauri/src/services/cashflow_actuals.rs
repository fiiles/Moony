//! Actual cashflow from bank transactions (design system README §11).
//!
//! Sums the bank transactions of a range by month and direction, net expenses by
//! category and income by counterparty, and repeats the sums for the preceding
//! range of the same length. Internal transfers are left out, like the budgeting
//! report does; refunds (credits in an expense category) reduce that category;
//! credits in the income category and uncategorized credits count as income.
//! Every amount converts to CZK with the exchange rate of its booking day.

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::cashflow_actuals::{CashflowActuals, CashflowGroup, CashflowMonth};
use crate::services::currency::{
    convert_to_czk_with_rates, get_all_rates, get_rates_for_date_range,
    resolve_rates_for_day_from_range,
};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use rusqlite::{params, Connection};
use std::collections::HashMap;

const DAY: i64 = 86_400;
const INTERNAL_TRANSFERS: &str = "cat_internal_transfers";
const INCOME_CATEGORY: &str = "cat_income";
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
/// range being the same number of months before. `ytd`: January 1 to today, the
/// previous range the same number of days before.
pub fn range_bounds(range: &str, today: i64) -> Result<RangeBounds> {
    validate_range(range)?;
    let day_start = today.div_euclid(DAY) * DAY;
    let end = day_start + DAY - 1;
    let date = DateTime::<Utc>::from_timestamp(day_start, 0)
        .ok_or_else(|| AppError::Internal("invalid timestamp".to_string()))?;
    let (year, month) = (date.year(), date.month());

    if range == "ytd" {
        let start = month_start(year, 1);
        let length = end - start + 1;
        return Ok(RangeBounds {
            start,
            end,
            previous_start: start - length,
            previous_end: start - 1,
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
        previous_end: start - 1,
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

struct Row {
    booking_date: i64,
    is_credit: bool,
    currency: String,
    amount: f64,
    category_id: Option<String>,
    category_name: Option<String>,
    counterparty: Option<String>,
    account_id: String,
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

fn money(v: f64) -> String {
    format!("{:.2}", if v.abs() < 0.005 { 0.0 } else { v })
}

fn source_key(counterparty: &Option<String>) -> (String, String) {
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
    let bounds = range_bounds(range, today)?;

    let mut stmt = conn.prepare(
        "SELECT bt.booking_date, bt.tx_type, bt.currency, CAST(bt.amount AS REAL),
                bt.category_id, tc.name, bt.counterparty_name, bt.bank_account_id
         FROM bank_transactions bt
         LEFT JOIN transaction_categories tc ON tc.id = bt.category_id
         WHERE bt.booking_date >= ?1 AND bt.booking_date <= ?2
           AND COALESCE(bt.category_id, '') != ?3
         ORDER BY bt.booking_date",
    )?;
    let rows: Vec<Row> = stmt
        .query_map(
            params![bounds.previous_start, bounds.end, INTERNAL_TRANSFERS],
            |row| {
                Ok(Row {
                    booking_date: row.get(0)?,
                    is_credit: row.get::<_, String>(1)?.eq_ignore_ascii_case("credit"),
                    currency: row.get(2)?,
                    amount: row.get::<_, f64>(3)?.abs(),
                    category_id: row.get(4)?,
                    category_name: row.get(5)?,
                    counterparty: row.get(6)?,
                    account_id: row.get(7)?,
                })
            },
        )?
        .filter_map(|r| r.ok())
        .collect();

    // One FX lookup for the whole span; a day without a snapshot walks back up to
    // ten days, then falls back to the current in-memory rates.
    let rates_by_day = get_rates_for_date_range(conn, bounds.previous_start, bounds.end);
    let current_rates = get_all_rates();
    let to_czk = |amount: f64, currency: &str, day: i64| {
        let rates = resolve_rates_for_day_from_range(&rates_by_day, day).unwrap_or(&current_rates);
        convert_to_czk_with_rates(amount, currency, rates)
    };

    let mut months: HashMap<i64, (f64, f64)> = months_between(bounds.start, bounds.end)
        .into_iter()
        .map(|m| (m, (0.0, 0.0)))
        .collect();
    let mut expenses: HashMap<String, GroupAcc> = HashMap::new();
    let mut sources: HashMap<String, GroupAcc> = HashMap::new();
    let mut total_income = 0.0;
    let mut total_expenses = 0.0;
    let mut previous_income = 0.0;
    let mut previous_expenses = 0.0;
    let mut accounts: Vec<String> = Vec::new();
    let mut transaction_count = 0;

    for row in rows {
        let in_current = row.booking_date >= bounds.start;
        let czk = to_czk(row.amount, &row.currency, row.booking_date);
        let category = row.category_id.clone().unwrap_or_default();
        let is_income = row.is_credit && (category.is_empty() || category == INCOME_CATEGORY);

        if in_current {
            transaction_count += 1;
            if !accounts.contains(&row.account_id) {
                accounts.push(row.account_id.clone());
            }
        }

        if is_income {
            let (key, name) = source_key(&row.counterparty);
            let acc = sources.entry(key).or_default();
            acc.name_from(name, in_current);
            if in_current {
                acc.amount += czk;
                acc.count += 1;
                total_income += czk;
                if let Some(m) = month_of(&mut months, row.booking_date) {
                    m.0 += czk;
                }
            } else {
                acc.previous += czk;
                previous_income += czk;
            }
            continue;
        }

        // Debits in the income category are not expenses of any category: skip them
        // like the budgeting report does.
        if !row.is_credit && category == INCOME_CATEGORY {
            continue;
        }

        let key = if category.is_empty() {
            UNCATEGORIZED.to_string()
        } else {
            category
        };
        let acc = expenses.entry(key).or_default();
        acc.name_from(row.category_name.clone().unwrap_or_default(), in_current);
        // A credit in an expense category is a refund: it reduces the category
        let signed = if row.is_credit { -czk } else { czk };
        if in_current {
            acc.amount += signed;
            acc.count += 1;
            total_expenses += signed;
            if let Some(m) = month_of(&mut months, row.booking_date) {
                m.1 += signed;
            }
        } else {
            acc.previous += signed;
            previous_expenses += signed;
        }
    }

    let mut month_list: Vec<CashflowMonth> = months
        .into_iter()
        .map(|(month, (income, expense))| CashflowMonth {
            month,
            income: money(income),
            expenses: money(expense.max(0.0)),
        })
        .collect();
    month_list.sort_by_key(|m| m.month);

    let to_groups = |map: HashMap<String, GroupAcc>| -> Vec<CashflowGroup> {
        let mut groups: Vec<CashflowGroup> = map
            .into_iter()
            .filter(|(_, acc)| acc.amount.abs() >= 0.005 || acc.previous.abs() >= 0.005)
            .map(|(key, acc)| CashflowGroup {
                key,
                name: acc.name,
                amount: money(acc.amount),
                previous_amount: money(acc.previous),
                count: acc.count,
            })
            .collect();
        groups.sort_by(|a, b| {
            let av: f64 = a.amount.parse().unwrap_or(0.0);
            let bv: f64 = b.amount.parse().unwrap_or(0.0);
            bv.partial_cmp(&av)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.name.cmp(&b.name))
        });
        groups
    };

    Ok(CashflowActuals {
        range: range.to_string(),
        range_start: bounds.start,
        range_end: bounds.end,
        previous_start: bounds.previous_start,
        previous_end: bounds.previous_end,
        months: month_list,
        expense_categories: to_groups(expenses),
        income_sources: to_groups(sources),
        total_income: money(total_income),
        total_expenses: money(total_expenses.max(0.0)),
        previous_income: money(previous_income),
        previous_expenses: money(previous_expenses.max(0.0)),
        account_count: accounts.len() as i32,
        transaction_count,
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
                name TEXT NOT NULL
            );
            CREATE TABLE bank_accounts (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL
            );
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY,
                bank_account_id TEXT NOT NULL,
                booking_date INTEGER NOT NULL,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL,
                counterparty_name TEXT,
                category_id TEXT,
                tx_type TEXT NOT NULL
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

    #[test]
    fn six_months_end_today_and_start_on_a_month_boundary() {
        let b = range_bounds("6m", TODAY).unwrap();
        assert_eq!(b.start, ts(2026, 5, 1));
        assert_eq!(b.end, ts(2026, 10, 4) - 1);
        assert_eq!(b.previous_start, ts(2025, 11, 1));
        assert_eq!(b.previous_end, ts(2026, 5, 1) - 1);
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
        assert_eq!(b.previous_end, b.start - 1);
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
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'EUR', 25.0)",
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
        assert_eq!(report.transaction_count, 6);

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
