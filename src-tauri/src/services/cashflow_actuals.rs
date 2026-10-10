//! Actual cashflow from bank transactions for one calendar period (design system README §11).
//!
//! The shared booked-ledger interpretation (explicit choice, own account, category role,
//! direction) decides whether a row is income, an expense or a money transfer. Expense
//! rows are debit-positive, so a refund lowers its category; income and transfer rows are
//! credit-positive. Totals and groups cover the period; `months` covers a twelve-month
//! chart window ending with the period's last month. Averages name their observed
//! full-month sample; observation never proves statement completeness.

use crate::error::{AppError, Result};
use crate::models::cashflow_actuals::{CashflowActuals, CashflowGroup, CashflowMonth};
use crate::models::FlowKind;
use crate::models::ReportTransaction;
use crate::services::reporting::{parse_money, quality, transactions_in_currency};
use chrono::{DateTime, Datelike, NaiveDate, Utc};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};

const UNCATEGORIZED: &str = "uncategorized";
/// Months the chart shows, ending with the last month of the period.
const CHART_MONTHS: i32 = 12;

/// A period must not end before it starts.
pub fn validate_period(start: i64, end: i64) -> Result<()> {
    if end < start {
        Err(AppError::Validation(
            "validation.cashflowPeriodInvalid".to_string(),
        ))
    } else {
        Ok(())
    }
}

fn month_start(year: i32, month: u32) -> Option<i64> {
    NaiveDate::from_ymd_opt(year, month, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .map(|dt| dt.and_utc().timestamp())
}

/// The month `offset` months before (negative) or after the given one.
fn shift_month(year: i32, month: u32, offset: i32) -> (i32, u32) {
    let index = year * 12 + (month as i32 - 1) + offset;
    (index.div_euclid(12), (index.rem_euclid(12) + 1) as u32)
}

/// First day of the chart window: eleven months before the month of `end`, or the first
/// day of the month of `start` when a longer period begins earlier.
pub fn chart_window_start(start: i64, end: i64) -> i64 {
    let Some(last) = DateTime::<Utc>::from_timestamp(end, 0) else {
        return start;
    };
    let (year, month) = shift_month(last.year(), last.month(), -(CHART_MONTHS - 1));
    let first = crate::services::reporting::month_start(start);
    match (month_start(year, month), first) {
        (Some(window), Some(first)) => window.min(first),
        (window, first) => window.or(first).unwrap_or(start),
    }
}

/// First days of every month from `start` to `end`, oldest first.
fn months_between(start: i64, end: i64) -> Vec<i64> {
    let mut out = Vec::new();
    let Some(from) = DateTime::<Utc>::from_timestamp(start, 0) else {
        return out;
    };
    let (mut year, mut month) = (from.year(), from.month());
    // A month outside chrono's calendar ends the list instead of looping forever.
    while let Some(ts) = month_start(year, month) {
        if ts > end {
            break;
        }
        out.push(ts);
        (year, month) = shift_month(year, month, 1);
    }
    out
}

#[derive(Default)]
struct GroupAcc {
    amount: f64,
    count: i32,
}

fn group_key(row: &ReportTransaction) -> (FlowKind, String) {
    (
        row.effective_flow_kind,
        row.category_id
            .clone()
            .unwrap_or_else(|| UNCATEGORIZED.into()),
    )
}

/// The report for the period `[start, end]` (unix seconds, inclusive) with `today` as the
/// reference day, in `report_currency`.
pub fn get_actuals_in_currency(
    conn: &Connection,
    start: i64,
    end: i64,
    today: i64,
    accounts: Option<&[String]>,
    report_currency: &str,
) -> Result<CashflowActuals> {
    validate_period(start, end)?;
    let report_currency = crate::services::reporting::normalize_currency(report_currency)?;
    let money = |v| crate::services::reporting::money_in_currency(v, &report_currency);
    let window_start = chart_window_start(start, end);
    let window = transactions_in_currency(conn, window_start, end, accounts, &report_currency)?;
    let rows: Vec<&ReportTransaction> = window
        .iter()
        .filter(|r| r.booking_date >= start && r.booking_date <= end)
        .collect();

    let mut months: HashMap<i64, (f64, f64)> = months_between(window_start, end)
        .into_iter()
        .map(|m| (m, (0.0, 0.0)))
        .collect();
    for row in &window {
        let Some(amount) = row.signed_amount.as_deref() else {
            continue;
        };
        let amount = parse_money(amount)?;
        let Some(month) = crate::services::reporting::month_start(row.booking_date)
            .and_then(|m| months.get_mut(&m))
        else {
            continue;
        };
        match row.effective_flow_kind {
            FlowKind::Income => month.0 += amount,
            FlowKind::Expense => month.1 += amount,
            FlowKind::Transfer => {}
        }
    }

    let mut groups: HashMap<(FlowKind, String), GroupAcc> = HashMap::new();
    let (mut income, mut expense) = (0.0, 0.0);
    for row in &rows {
        let Some(amount) = row.signed_amount.as_deref() else {
            continue;
        };
        let amount = parse_money(amount)?;
        groups.entry(group_key(row)).or_default().amount += amount;
        match row.effective_flow_kind {
            FlowKind::Income => income += amount,
            FlowKind::Expense => expense += amount,
            FlowKind::Transfer => {}
        }
    }
    // Counts match the drilldown and include rows whose report conversion is
    // missing; a group with only unconverted rows is not added as a zero.
    for row in &rows {
        if let Some(acc) = groups.get_mut(&group_key(row)) {
            acc.count += 1;
        }
    }

    let names: HashMap<String, String> = {
        let mut stmt = conn.prepare("SELECT id, name FROM transaction_categories")?;
        let pairs = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        pairs.collect::<std::result::Result<_, _>>()?
    };
    let mut expense_categories = Vec::new();
    let mut income_categories = Vec::new();
    let mut transfers = Vec::new();
    let mut transfer_total = 0.0;
    for ((kind, key), acc) in groups {
        let target = match kind {
            FlowKind::Income => &mut income_categories,
            FlowKind::Expense => &mut expense_categories,
            FlowKind::Transfer => {
                transfer_total += acc.amount;
                &mut transfers
            }
        };
        target.push((
            acc.amount,
            CashflowGroup {
                name: names.get(&key).cloned().unwrap_or_default(),
                key,
                flow_kind: kind,
                amount: money(acc.amount),
                count: acc.count,
            },
        ));
    }
    let largest_first = |mut list: Vec<(f64, CashflowGroup)>, by_size: bool| {
        list.sort_by(|(a, ga), (b, gb)| {
            let (a, b) = if by_size {
                (a.abs(), b.abs())
            } else {
                (*a, *b)
            };
            b.total_cmp(&a).then_with(|| ga.key.cmp(&gb.key))
        });
        list.into_iter().map(|(_, g)| g).collect::<Vec<_>>()
    };

    let period_rows: Vec<ReportTransaction> = rows.iter().map(|r| (*r).clone()).collect();
    let period_quality = quality(&period_rows, start, end, today);
    let completed: Vec<i64> = period_quality
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
    let (average_income, average_expenses) = if completed.is_empty() {
        (None, None)
    } else {
        let (i, e) = completed
            .iter()
            .filter_map(|m| months.get(m))
            .fold((0.0, 0.0), |(i, e), (mi, me)| (i + mi, e + me));
        let n = completed.len() as f64;
        (Some(money(i / n)), Some(money(e / n)))
    };

    let mut months: Vec<CashflowMonth> = months
        .into_iter()
        .map(|(month, (income, expenses))| CashflowMonth {
            month,
            income: money(income),
            expenses: money(expenses),
        })
        .collect();
    months.sort_by_key(|m| m.month);
    let account_count = rows
        .iter()
        .map(|r| &r.bank_account_id)
        .collect::<HashSet<_>>()
        .len() as i32;

    Ok(CashflowActuals {
        report_currency: report_currency.clone(),
        period_start: start,
        period_end: end,
        quality: period_quality,
        months,
        total_income: money(income),
        total_expenses: money(expense),
        net_balance: money(income - expense),
        bank_cash_movement: money(income - expense + transfer_total),
        average_income,
        average_expenses,
        average_completed_months: completed,
        expense_categories: largest_first(expense_categories, false),
        income_categories: largest_first(income_categories, false),
        transfers: largest_first(transfers, true),
        account_count,
        transaction_count: rows.len() as i32,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::{params, Connection};

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
                tx_type TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'booked', flow_kind TEXT, counterparty_iban TEXT, counterparty_bank_code TEXT, counterparty_country TEXT, remittance_info TEXT
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            INSERT INTO transaction_categories (id, name, kind) VALUES
                ('cat_income', 'Salary', 'income'),
                ('cat_investment_income', 'Investment income', 'income'),
                ('cat_groceries', 'Groceries', 'expense'),
                ('cat_dining', 'Dining & Restaurants', 'expense'),
                ('cat_housing', 'Housing', 'expense'),
                ('cat_savings', 'Savings', 'transfer'),
                ('cat_investments', 'Investments', 'transfer'),
                ('cat_internal_transfers', 'Internal Transfers', 'transfer');
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
        category: Option<&str>,
        tx_type: &str,
    ) {
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, account, date, amount, currency, category, tx_type],
        )
        .unwrap();
    }

    const TODAY: i64 = 1_791_000_000; // 2026-10-03 (UTC)
    /// September 2026, a completed month before TODAY.
    fn september() -> (i64, i64) {
        (ts(2026, 9, 1), ts(2026, 10, 1) - 1)
    }
    fn year_2026() -> (i64, i64) {
        (ts(2026, 1, 1), ts(2027, 1, 1) - 1)
    }
    fn report(conn: &Connection, (start, end): (i64, i64)) -> CashflowActuals {
        get_actuals_in_currency(conn, start, end, TODAY, None, "CZK").unwrap()
    }
    fn group<'a>(groups: &'a [CashflowGroup], key: &str) -> &'a CashflowGroup {
        groups.iter().find(|g| g.key == key).unwrap()
    }

    #[test]
    fn chart_window_is_twelve_months_ending_with_the_period() {
        let year = year_2026();
        assert_eq!(chart_window_start(year.0, year.1), ts(2026, 1, 1));
        let q3 = (ts(2026, 7, 1), ts(2026, 10, 1) - 1);
        assert_eq!(chart_window_start(q3.0, q3.1), ts(2025, 10, 1));
        let october = (ts(2026, 10, 1), ts(2026, 11, 1) - 1);
        assert_eq!(chart_window_start(october.0, october.1), ts(2025, 11, 1));
        let january = (ts(2026, 1, 1), ts(2026, 2, 1) - 1);
        assert_eq!(chart_window_start(january.0, january.1), ts(2025, 2, 1));
        for (start, end) in [year, q3, october, january] {
            let report = report(&setup_test_db(), (start, end));
            assert_eq!(report.months.len(), 12);
            assert_eq!(
                report.months[11].month,
                crate::services::reporting::month_start(end).unwrap()
            );
        }
    }

    #[test]
    fn a_longer_custom_period_widens_the_window() {
        let (start, end) = (ts(2025, 1, 1), ts(2026, 7, 1) - 1);
        assert_eq!(chart_window_start(start, end), start);
        assert_eq!(report(&setup_test_db(), (start, end)).months.len(), 18);
    }

    #[test]
    fn rejects_a_period_that_ends_before_it_starts() {
        let conn = setup_test_db();
        let result =
            get_actuals_in_currency(&conn, ts(2026, 9, 2), ts(2026, 9, 1), TODAY, None, "CZK");
        assert!(
            matches!(result, Err(AppError::Validation(key)) if key == "validation.cashflowPeriodInvalid")
        );
    }

    #[test]
    fn window_rows_outside_the_period_feed_the_chart_but_not_the_totals() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "aug",
            "acc1",
            ts(2026, 8, 15),
            "1000",
            "CZK",
            Some("cat_income"),
            "credit",
        );
        insert_tx(
            &conn,
            "sep-in",
            "acc1",
            ts(2026, 9, 15),
            "500",
            "CZK",
            Some("cat_income"),
            "credit",
        );
        insert_tx(
            &conn,
            "sep-out",
            "acc1",
            ts(2026, 9, 16),
            "200",
            "CZK",
            Some("cat_groceries"),
            "debit",
        );
        let report = report(&conn, september());
        assert_eq!(report.total_income, "500.00");
        assert_eq!(report.total_expenses, "200.00");
        assert_eq!(report.transaction_count, 2);
        let month = |m: i64| report.months.iter().find(|x| x.month == m).unwrap();
        assert_eq!(month(ts(2026, 8, 1)).income, "1000.00");
        assert_eq!(month(ts(2026, 9, 1)).income, "500.00");
        assert_eq!(month(ts(2026, 9, 1)).expenses, "200.00");
        assert_eq!(report.period_start, september().0);
        assert_eq!(report.period_end, september().1);
    }

    #[test]
    fn refunds_net_inside_expense_categories_and_never_enter_income() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "dinner",
            "acc1",
            ts(2026, 9, 5),
            "1000",
            "CZK",
            Some("cat_dining"),
            "debit",
        );
        insert_tx(
            &conn,
            "friend",
            "acc1",
            ts(2026, 9, 6),
            "400",
            "CZK",
            Some("cat_dining"),
            "credit",
        );
        insert_tx(
            &conn,
            "late",
            "acc1",
            ts(2026, 9, 7),
            "500",
            "CZK",
            Some("cat_housing"),
            "credit",
        );
        let report = report(&conn, september());
        assert_eq!(report.total_income, "0.00");
        assert!(report.income_categories.is_empty());
        assert_eq!(report.total_expenses, "100.00");
        let dining = group(&report.expense_categories, "cat_dining");
        assert_eq!((dining.amount.as_str(), dining.count), ("600.00", 2));
        assert_eq!(dining.flow_kind, FlowKind::Expense);
        assert_eq!(dining.name, "Dining & Restaurants");
        assert_eq!(
            group(&report.expense_categories, "cat_housing").amount,
            "-500.00"
        );
        assert_eq!(report.expense_categories[0].key, "cat_dining");
    }

    #[test]
    fn income_is_grouped_by_category_with_uncategorized_credits() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "salary",
            "acc1",
            ts(2026, 9, 10),
            "50000",
            "CZK",
            Some("cat_income"),
            "credit",
        );
        insert_tx(
            &conn,
            "dividend",
            "acc1",
            ts(2026, 9, 11),
            "300",
            "CZK",
            Some("cat_investment_income"),
            "credit",
        );
        insert_tx(
            &conn,
            "friend",
            "acc1",
            ts(2026, 9, 12),
            "3000",
            "CZK",
            None,
            "credit",
        );
        insert_tx(
            &conn,
            "reversal",
            "acc1",
            ts(2026, 9, 13),
            "1000",
            "CZK",
            Some("cat_income"),
            "debit",
        );
        let report = report(&conn, september());
        assert_eq!(report.total_income, "52300.00");
        let keys: Vec<_> = report
            .income_categories
            .iter()
            .map(|g| g.key.as_str())
            .collect();
        assert_eq!(
            keys,
            ["cat_income", "uncategorized", "cat_investment_income"]
        );
        let salary = group(&report.income_categories, "cat_income");
        assert_eq!((salary.amount.as_str(), salary.count), ("49000.00", 2));
        assert_eq!(salary.name, "Salary");
        let uncategorized = group(&report.income_categories, "uncategorized");
        assert_eq!(
            (uncategorized.name.as_str(), uncategorized.flow_kind),
            ("", FlowKind::Income)
        );
    }

    #[test]
    fn transfers_are_grouped_by_category_and_reconcile_bank_movement() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "salary",
            "acc1",
            ts(2026, 9, 1),
            "10000",
            "CZK",
            Some("cat_income"),
            "credit",
        );
        insert_tx(
            &conn,
            "food",
            "acc1",
            ts(2026, 9, 2),
            "1000",
            "CZK",
            Some("cat_groceries"),
            "debit",
        );
        insert_tx(
            &conn,
            "save",
            "acc1",
            ts(2026, 9, 3),
            "5000",
            "CZK",
            Some("cat_savings"),
            "debit",
        );
        insert_tx(
            &conn,
            "etf",
            "acc1",
            ts(2026, 9, 4),
            "2000",
            "CZK",
            Some("cat_investments"),
            "debit",
        );
        insert_tx(
            &conn,
            "in",
            "acc2",
            ts(2026, 9, 5),
            "700",
            "CZK",
            Some("cat_internal_transfers"),
            "credit",
        );
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, tx_type, flow_kind)
             VALUES ('marked', 'acc1', ?1, '100', 'CZK', 'debit', 'transfer')",
            [ts(2026, 9, 6)],
        )
        .unwrap();
        let report = report(&conn, september());
        assert_eq!(report.net_balance, "9000.00");
        assert_eq!(report.total_expenses, "1000.00");
        let keys: Vec<_> = report.transfers.iter().map(|g| g.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "cat_savings",
                "cat_investments",
                "cat_internal_transfers",
                "uncategorized"
            ]
        );
        assert_eq!(group(&report.transfers, "cat_savings").amount, "-5000.00");
        assert_eq!(
            group(&report.transfers, "cat_internal_transfers").amount,
            "700.00"
        );
        assert!(report
            .transfers
            .iter()
            .all(|g| g.flow_kind == FlowKind::Transfer));
        // 10000 − 1000 − 5000 − 2000 + 700 − 100
        assert_eq!(report.bank_cash_movement, "2600.00");
        let months_income: f64 = report
            .months
            .iter()
            .map(|m| m.income.parse::<f64>().unwrap())
            .sum();
        assert_eq!(months_income, 10000.0);
    }

    #[test]
    fn averages_use_completed_observed_months_inside_the_period() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "d",
            "acc1",
            ts(2026, 8, 1),
            "1000",
            "CZK",
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
            Some("cat_internal_transfers"),
            "credit",
        );
        let report = report(&conn, year_2026());
        assert_eq!(report.total_expenses, "-500.00");
        assert_eq!(report.bank_cash_movement, "1200.00");
        assert_eq!(report.average_expenses.as_deref(), Some("-250.00"));
        assert_eq!(
            report.average_completed_months,
            vec![ts(2026, 8, 1), ts(2026, 9, 1)]
        );
        // January–July unobserved; October (current) to December partial.
        assert_eq!(report.quality.unobserved_month_count, 7);
        assert_eq!(report.quality.partial_month_count, 3);
        let (start, end) = september();
        let month = get_actuals_in_currency(&conn, start, end, TODAY, None, "CZK").unwrap();
        assert_eq!(month.average_completed_months, vec![ts(2026, 9, 1)]);
    }

    #[test]
    fn account_scope_limits_the_period_rows() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "a",
            "acc1",
            ts(2026, 9, 1),
            "100",
            "CZK",
            Some("cat_groceries"),
            "debit",
        );
        insert_tx(
            &conn,
            "b",
            "acc2",
            ts(2026, 9, 1),
            "40",
            "CZK",
            Some("cat_groceries"),
            "debit",
        );
        let scope = ["acc2".to_string()];
        let (start, end) = september();
        let report =
            get_actuals_in_currency(&conn, start, end, TODAY, Some(&scope), "CZK").unwrap();
        assert_eq!(report.total_expenses, "40.00");
        assert_eq!((report.account_count, report.transaction_count), (1, 1));
    }

    #[test]
    fn empty_period_gives_zero_filled_months() {
        let report = report(&setup_test_db(), year_2026());
        assert!(report
            .months
            .iter()
            .all(|m| m.income == "0.00" && m.expenses == "0.00"));
        assert_eq!(report.total_income, "0.00");
        assert_eq!((report.account_count, report.transaction_count), (0, 0));
        assert!(report.expense_categories.is_empty() && report.transfers.is_empty());
        assert_eq!(report.average_income, None);
    }

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
            "debit",
        );
        let report = report(&conn, september());
        assert_eq!(report.quality.missing_fx_count, 2);
        assert_eq!(report.expense_categories.len(), 1);
        assert_eq!(report.expense_categories[0].amount, "100.00");
        assert_eq!(report.expense_categories[0].count, 2);
        assert_eq!(report.average_completed_months, Vec::<i64>::new());
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
                Some("cat_income"),
                "credit",
            );
            let (start, end) = september();
            let report = get_actuals_in_currency(&conn, start, end, TODAY, None, currency).unwrap();
            assert_eq!(report.total_income, expected);
            assert_eq!(report.report_currency, currency);
            assert_eq!(report.quality.missing_fx_count, 0);
        }
    }

    #[test]
    fn target_actuals_use_historical_cross_rates_and_ignore_live_rates() {
        let conn = setup_test_db();
        let day = ts(2026, 9, 1);
        insert_tx(
            &conn,
            "current",
            "acc1",
            day,
            "110",
            "XAC",
            Some("cat_income"),
            "credit",
        );
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'XAC',25)",
            [day],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'XAT',27.5)",
            [day],
        )
        .unwrap();
        let (start, end) = september();
        let first = get_actuals_in_currency(&conn, start, end, TODAY, None, "XAT").unwrap();
        assert_eq!(first.total_income, "100.00");
        crate::services::currency::update_exchange_rates(HashMap::from([
            ("XAC".into(), 80.0),
            ("XAT".into(), 10.0),
        ]));
        let second = get_actuals_in_currency(&conn, start, end, TODAY, None, "XAT").unwrap();
        assert_eq!(first.total_income, second.total_income);
    }

    #[test]
    fn native_eur_refunds_and_drilldowns_conserve() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "eur",
            "acc1",
            ts(2026, 9, 1),
            "100",
            "EUR",
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
            Some("cat_groceries"),
            "credit",
        );
        let (start, end) = september();
        let report = get_actuals_in_currency(&conn, start, end, TODAY, None, "EUR").unwrap();
        assert_eq!(report.total_expenses, "75.00");
        assert_eq!(report.quality.missing_fx_count, 0);
        let rows = crate::services::reporting::filtered_transactions_in_currency(
            &conn,
            start,
            end,
            None,
            Some(FlowKind::Expense),
            Some("cat_groceries"),
            "EUR",
        )
        .unwrap();
        let sum: f64 = rows
            .iter()
            .filter_map(|r| r.signed_amount.as_deref())
            .map(|a| parse_money(a).unwrap())
            .sum();
        assert_eq!(sum, 75.0);
    }

    #[test]
    fn signed_report_conserves_refunds_income_reversals_and_skips_pending() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO transaction_categories(id,name,kind) VALUES ('custom-income','Consulting','income');
             INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,category_id,tx_type) VALUES
              ('d','acc1',1790812800,'1000','CZK','cat_groceries','debit'),
              ('r','acc1',1790899200,'500','CZK','cat_housing','credit'),
              ('i','acc1',1790812800,'1000','CZK','cat_income','credit'),
              ('ir','acc1',1790812800,'100','CZK','cat_income','debit'),
              ('ci','acc1',1790812800,'2000','CZK','custom-income','credit');
             INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,tx_type,status) VALUES
              ('pending','acc1',1790812800,'9999','CZK','debit','pending');",
        )
        .unwrap();
        // The rows are booked on 1 and 2 October 2026.
        let report = report(&conn, (ts(2026, 10, 1), ts(2026, 11, 1) - 1));
        assert_eq!(report.total_expenses, "500.00");
        assert_eq!(report.total_income, "2900.00");
        assert_eq!(
            group(&report.expense_categories, "cat_housing").amount,
            "-500.00"
        );
        let expense_sum: f64 = report
            .months
            .iter()
            .map(|m| m.expenses.parse::<f64>().unwrap())
            .sum();
        assert_eq!(expense_sum, 500.0);
    }

    #[test]
    fn sums_by_month_and_category_with_dated_fx() {
        let conn = setup_test_db();
        insert_tx(
            &conn,
            "t1",
            "acc1",
            ts(2026, 9, 30),
            "50000",
            "CZK",
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
            Some("cat_groceries"),
            "credit",
        );
        insert_tx(
            &conn,
            "t4",
            "acc1",
            ts(2026, 8, 1),
            "15000",
            "CZK",
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
            None,
            "credit",
        );
        insert_tx(
            &conn,
            "t6",
            "acc1",
            ts(2026, 8, 25),
            "100",
            "EUR",
            None,
            "debit",
        );
        insert_tx(
            &conn,
            "t7",
            "acc1",
            ts(2026, 8, 26),
            "9999",
            "CZK",
            Some("cat_internal_transfers"),
            "debit",
        );
        insert_tx(
            &conn,
            "old",
            "acc1",
            ts(2025, 3, 30),
            "48000",
            "CZK",
            Some("cat_income"),
            "credit",
        );
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'EUR', 25.0)",
            params![ts(2026, 8, 24)],
        )
        .unwrap();

        let report = report(&conn, year_2026());
        assert_eq!(report.total_income, "53000.00");
        // 1200 − 200 refund + 15000 + 100 EUR × 25 (rate of the nearest earlier day)
        assert_eq!(report.total_expenses, "18500.00");
        assert_eq!(report.account_count, 2);
        assert_eq!(report.transaction_count, 7);
        let month = |m: i64| report.months.iter().find(|x| x.month == m).unwrap();
        assert_eq!(month(ts(2026, 9, 1)).income, "50000.00");
        assert_eq!(month(ts(2026, 9, 1)).expenses, "1000.00");
        assert_eq!(month(ts(2026, 8, 1)).income, "3000.00");
        assert_eq!(month(ts(2026, 8, 1)).expenses, "17500.00");
        assert_eq!(report.expense_categories[0].key, "cat_housing");
        assert_eq!(
            group(&report.expense_categories, "uncategorized").amount,
            "2500.00"
        );
        assert_eq!(
            group(&report.transfers, "cat_internal_transfers").amount,
            "-9999.00"
        );
    }
}
