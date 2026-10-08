//! Budgeting service - aggregates transactions by category for spending analysis
//!
//! This service provides:
//! - Income/expense aggregation by category across all bank accounts
//! - Budget goal CRUD operations
//! - Transaction retrieval for category drill-down

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::budgeting::{
    BudgetGoal, BudgetGoalVersion, BudgetingReport, BudgetingTransaction, CategorySpendingSummary,
    InsertBudgetGoal,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;
use uuid::Uuid;

// ========================== Budget Goals CRUD ==========================

fn goal_row(row: &rusqlite::Row) -> rusqlite::Result<BudgetGoal> {
    Ok(BudgetGoal {
        id: row.get(0)?,
        category_id: row.get(1)?,
        timeframe: row.get(2)?,
        amount: row.get(3)?,
        currency: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        effective_month: None,
        versions: Vec::new(),
    })
}

fn goal_versions(
    conn: &Connection,
    category_id: &str,
    timeframe: &str,
) -> Result<Vec<BudgetGoalVersion>> {
    let mut stmt = conn.prepare(
        "SELECT effective_month, amount, currency FROM budget_goal_versions
         WHERE category_id = ?1 AND timeframe = ?2 ORDER BY effective_month",
    )?;
    let versions = stmt
        .query_map(params![category_id, timeframe], |r| {
            Ok(BudgetGoalVersion {
                effective_month: r.get(0)?,
                amount: r.get(1)?,
                currency: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(versions)
}

/// The version that sets the limit for `month`: the latest one starting on
/// or before it, if that one is not a removal.
fn version_in_effect(versions: &[BudgetGoalVersion], month: i64) -> Option<&BudgetGoalVersion> {
    versions
        .iter()
        .rev()
        .find(|v| v.effective_month <= month)
        .filter(|v| v.amount.is_some())
}

/// The version a goal shows as its limit: the one in effect now, otherwise
/// the first scheduled one (a goal that only starts in a later month).
fn displayed_version(versions: &[BudgetGoalVersion], now_month: i64) -> Option<&BudgetGoalVersion> {
    version_in_effect(versions, now_month).or_else(|| {
        versions
            .iter()
            .find(|v| v.effective_month > now_month && v.amount.is_some())
    })
}

/// Attach the versions and expose the displayed version as the goal's amount.
fn with_versions(conn: &Connection, mut goal: BudgetGoal, now: i64) -> Result<BudgetGoal> {
    let versions = goal_versions(conn, &goal.category_id, &goal.timeframe)?;
    let now_month = crate::services::reporting::month_start(now)
        .ok_or_else(|| AppError::Validation("validation.effectiveMonthInvalid".into()))?;
    if let Some(v) = displayed_version(&versions, now_month) {
        goal.amount = v.amount.clone().expect("displayed versions have an amount");
        goal.currency = v.currency.clone();
        goal.effective_month = Some(v.effective_month);
    }
    goal.versions = versions;
    Ok(goal)
}

/// Get all budget goals
pub fn get_all_goals(conn: &Connection) -> Result<Vec<BudgetGoal>> {
    get_all_goals_at(conn, chrono::Utc::now().timestamp())
}

/// All goals as seen at `now`: each shows the limit in effect in that month.
pub fn get_all_goals_at(conn: &Connection, now: i64) -> Result<Vec<BudgetGoal>> {
    let mut stmt = conn.prepare(
        "SELECT id, category_id, timeframe, amount, currency, created_at, updated_at
         FROM budget_goals
         ORDER BY category_id, timeframe",
    )?;
    let goals = stmt
        .query_map([], goal_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    goals
        .into_iter()
        .map(|goal| with_versions(conn, goal, now))
        .collect()
}

/// Get budget goals for a specific timeframe
pub fn get_goals_by_timeframe(conn: &Connection, timeframe: &str) -> Result<Vec<BudgetGoal>> {
    Ok(get_all_goals(conn)?
        .into_iter()
        .filter(|goal| goal.timeframe == timeframe)
        .collect())
}

/// Upsert (create or update) a budget goal
pub fn upsert_goal(conn: &Connection, data: InsertBudgetGoal) -> Result<BudgetGoal> {
    upsert_goal_at(conn, data, chrono::Utc::now().timestamp())
}

/// Save a limit version effective from `data.effective_month` (default: the
/// month of `now`). Only the versions decide the limit of a month; the
/// `budget_goals` row mirrors the version in effect at `now`, so a scheduled
/// or back-dated change never replaces the current limit.
pub fn upsert_goal_at(
    conn: &Connection,
    mut data: InsertBudgetGoal,
    now: i64,
) -> Result<BudgetGoal> {
    data.validate()?;

    let currency = data.currency.clone().unwrap_or_else(|| "CZK".to_string());
    let effective = data
        .effective_month
        .or_else(|| crate::services::reporting::month_start(now))
        .ok_or_else(|| AppError::Validation("validation.effectiveMonthInvalid".into()))?;
    let tx = conn.unchecked_transaction()?;
    let conn = &tx;

    // Check if goal already exists for this category + timeframe
    let existing: Option<(String, String, String)> = conn
        .query_row(
            "SELECT id, amount, currency FROM budget_goals WHERE category_id = ?1 AND timeframe = ?2",
            params![data.category_id, data.timeframe],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?;

    // An unchanged value keeps the stored spelling of the limit the user saw
    // for that month (the version in effect then).
    let versions = goal_versions(conn, &data.category_id, &data.timeframe)?;
    let previous = version_in_effect(&versions, effective)
        .map(|v| (v.amount.clone().expect("in effect"), v.currency.clone()))
        .or_else(|| existing.as_ref().map(|(_, a, c)| (a.clone(), c.clone())));
    if let Some((old_amount, old_currency)) = previous {
        if currency == old_currency
            && crate::services::reporting::parse_money(&old_amount).ok()
                == Some(crate::services::reporting::parse_money(&data.amount)?)
        {
            data.amount = old_amount;
        }
    }

    let id = match existing {
        Some((existing_id, _, _)) => existing_id,
        None => {
            let new_id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO budget_goals (id, category_id, timeframe, amount, currency, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![new_id, data.category_id, data.timeframe, data.amount, currency, now],
            )?;
            new_id
        }
    };

    conn.execute("INSERT INTO budget_goal_versions(id,category_id,timeframe,effective_month,amount,currency,created_at) VALUES (?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(category_id,timeframe,effective_month) DO UPDATE SET amount=excluded.amount,currency=excluded.currency,created_at=excluded.created_at",
        params![Uuid::new_v4().to_string(),data.category_id,data.timeframe,effective,data.amount,currency,now])?;

    // Mirror the version in effect now (or the first scheduled one).
    let base = conn.query_row(
        "SELECT id, category_id, timeframe, amount, currency, created_at, updated_at
         FROM budget_goals WHERE id = ?1",
        [&id],
        goal_row,
    )?;
    let result = with_versions(conn, base, now)?;
    conn.execute(
        "UPDATE budget_goals SET amount = ?1, currency = ?2, updated_at = ?3 WHERE id = ?4",
        params![result.amount, result.currency, now, id],
    )?;
    let result = BudgetGoal {
        updated_at: now,
        ..result
    };
    tx.commit()?;
    Ok(result)
}

/// Delete a budget goal
pub fn delete_goal(conn: &Connection, id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let goal = tx
        .query_row(
            "SELECT category_id,timeframe,currency FROM budget_goals WHERE id=?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            },
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound(format!("Budget goal {id} not found")))?;
    let month = crate::services::reporting::month_start(chrono::Utc::now().timestamp())
        .expect("current month");
    tx.execute("INSERT INTO budget_goal_versions(id,category_id,timeframe,effective_month,amount,currency,created_at) VALUES (?1,?2,?3,?4,NULL,?5,?6) ON CONFLICT(category_id,timeframe,effective_month) DO UPDATE SET amount=NULL",params![Uuid::new_v4().to_string(),goal.0,goal.1,month,goal.2,chrono::Utc::now().timestamp()])?;
    // Also cancel future limits when deleting a scheduled goal.
    tx.execute("DELETE FROM budget_goal_versions WHERE category_id=?1 AND timeframe=?2 AND effective_month>?3",params![goal.0,goal.1,month])?;
    tx.execute("DELETE FROM budget_goals WHERE id=?1", [id])?;
    tx.commit()?;
    Ok(())
}

// ========================== Budgeting Report ==========================

/// Get the full budgeting report for a time period
pub fn get_report(
    conn: &Connection,
    start_date: i64,
    end_date: i64,
    timeframe: &str,
) -> Result<BudgetingReport> {
    get_report_scoped(conn, start_date, end_date, timeframe, None)
}
pub fn get_report_scoped(
    conn: &Connection,
    start_date: i64,
    end_date: i64,
    timeframe: &str,
    accounts: Option<&[String]>,
) -> Result<BudgetingReport> {
    get_report_in_currency(conn, start_date, end_date, timeframe, accounts, "CZK")
}
pub fn get_report_in_currency(
    conn: &Connection,
    start_date: i64,
    end_date: i64,
    timeframe: &str,
    accounts: Option<&[String]>,
    report_currency: &str,
) -> Result<BudgetingReport> {
    let report_currency = crate::services::reporting::normalize_currency(report_currency)?;
    let money = |v| crate::services::reporting::money_in_currency(v, &report_currency);
    use crate::models::{CashflowGroup, FlowKind};
    use crate::services::reporting::{parse_money, quality, transactions_in_currency};
    let rows = transactions_in_currency(conn, start_date, end_date, accounts, &report_currency)?;
    let mut incomes: HashMap<String, CategorySpendingSummary> = HashMap::new();
    let mut expenses: HashMap<String, CategorySpendingSummary> = HashMap::new();
    let mut others: HashMap<String, (f64, i32)> = HashMap::new();
    let (mut income, mut expense, mut un_income, mut un_expense, mut un_count) =
        (0.0, 0.0, 0.0, 0.0, 0);
    for row in &rows {
        let Some(value) = row.signed_amount.as_deref() else {
            continue;
        };
        let value = parse_money(value)?;
        let kind = row.effective_flow_kind;
        if kind != FlowKind::Income && kind != FlowKind::Expense {
            let v = others.entry(kind.to_string()).or_default();
            v.0 += value;
            v.1 += 1;
            continue;
        }
        if kind == FlowKind::Income {
            income += value;
        } else {
            expense += value;
        }
        if let Some(category) = &row.category_id {
            let map = if kind == FlowKind::Income {
                &mut incomes
            } else {
                &mut expenses
            };
            if !map.contains_key(category) {
                map.insert(category.clone(), empty_summary(conn, category)?);
            }
            let summary = map.get_mut(category).expect("inserted");
            summary.total_amount = (parse_money(&summary.total_amount)? + value).to_string();
        } else {
            if kind == FlowKind::Income {
                un_income += value;
            } else {
                un_expense += value;
            }
            un_count += 1;
        }
    }
    let limits = period_limits(conn, start_date, end_date, timeframe, &report_currency)?;
    // The latest active version within this period chooses the budget's own
    // evaluation currency. Profile/display changes never change utilization.
    let mut native_limits = HashMap::new();
    let mut native_spending = HashMap::new();
    for (_, goal) in limits.values() {
        let currency = crate::services::reporting::normalize_currency(&goal.currency)?;
        if native_limits.contains_key(&currency) {
            continue;
        }
        native_limits.insert(
            currency.clone(),
            period_limits(conn, start_date, end_date, timeframe, &currency)?,
        );
        let mut spending: HashMap<String, Option<f64>> = HashMap::new();
        for row in rows
            .iter()
            .filter(|r| r.effective_flow_kind == FlowKind::Expense)
        {
            let Some(category) = &row.category_id else {
                continue;
            };
            let contribution = match parse_money(&row.amount) {
                Ok(amount) => crate::services::reporting::historical_amount(
                    conn,
                    amount.abs(),
                    &row.currency,
                    &currency,
                    row.booking_date,
                )?
                .map(|value| {
                    let signed = crate::services::financial_semantics::signed_flow_amount(
                        row.effective_flow_kind,
                        row.tx_type.eq_ignore_ascii_case("credit"),
                        value,
                    );
                    parse_money(&crate::services::reporting::money_in_currency(
                        signed, &currency,
                    ))
                    .expect("formatted money")
                }),
                Err(_) => None,
            };
            let total = spending.entry(category.clone()).or_insert(Some(0.0));
            *total = match (*total, contribution) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
        }
        native_spending.insert(currency, spending);
    }
    for (category, (limit, goal)) in limits {
        if !expenses.contains_key(&category) {
            expenses.insert(category.clone(), empty_summary(conn, &category)?);
        }
        let summary = expenses.get_mut(&category).expect("inserted");
        let currency = crate::services::reporting::normalize_currency(&goal.currency)?;
        let native_money = |value| crate::services::reporting::money_in_currency(value, &currency);
        let spent = native_spending
            .get(&currency)
            .and_then(|m| m.get(&category))
            .copied()
            .unwrap_or(Some(0.0));
        let native_limit = native_limits
            .get(&currency)
            .and_then(|m| m.get(&category))
            .and_then(|(amount, _)| *amount);
        summary.budget_currency = Some(currency.clone());
        summary.budget_spent = spent.map(native_money);
        summary.budget_limit_native = native_limit.map(native_money);
        summary.budget_percentage = match (spent, native_limit) {
            (Some(spent), Some(limit)) if limit > 0.0 => Some(spent / limit * 100.0),
            _ => None,
        };
        summary.budget_goal = Some(goal);
        summary.budget_limit_fx_quality = Some(crate::models::FxQuality::Missing);
        if let Some(limit) = limit {
            summary.budget_limit = Some(money(limit));
            summary.budget_limit_fx_quality = Some(crate::models::FxQuality::Historical);
        }
    }
    // Counts (and drilldown availability) include rows whose report conversion
    // is missing; a row with only unconverted spending is not added as a zero.
    for row in &rows {
        let map = match row.effective_flow_kind {
            FlowKind::Income => &mut incomes,
            FlowKind::Expense => &mut expenses,
            _ => continue,
        };
        if let Some(summary) = row.category_id.as_ref().and_then(|c| map.get_mut(c)) {
            summary.transaction_count += 1;
        }
    }
    let sort = |map: HashMap<String, CategorySpendingSummary>| {
        let mut out: Vec<_> = map
            .into_values()
            .map(|mut v| {
                v.total_amount = money(v.total_amount.parse::<f64>().expect("money"));
                v
            })
            .collect();
        out.sort_by(|a, b| {
            b.total_amount
                .parse::<f64>()
                .expect("money")
                .total_cmp(&a.total_amount.parse::<f64>().expect("money"))
        });
        out
    };
    let other_total: f64 = others.values().map(|v| v.0).sum();
    Ok(BudgetingReport {
        report_currency: report_currency.clone(),
        bank_cash_movement: money(income - expense + other_total),
        period_start: start_date,
        period_end: end_date,
        timeframe: timeframe.into(),
        total_income: money(income),
        total_expenses: money(expense),
        net_balance: money(income - expense),
        income_categories: sort(incomes),
        expense_categories: sort(expenses),
        uncategorized_income: money(un_income),
        uncategorized_expenses: money(un_expense),
        uncategorized_transaction_count: un_count,
        uncategorized_expense_transaction_count: rows
            .iter()
            .filter(|row| row.category_id.is_none() && row.effective_flow_kind == FlowKind::Expense)
            .count() as i32,
        quality: quality(&rows, start_date, end_date, chrono::Utc::now().timestamp()),
        nonconsumption_flows: others
            .into_iter()
            .map(|(key, (amount, count))| CashflowGroup {
                key: key.clone(),
                name: key,
                amount: money(amount),
                previous_amount: "0.00".into(),
                count,
            })
            .collect(),
    })
}
fn empty_summary(conn: &Connection, id: &str) -> Result<CategorySpendingSummary> {
    let (name, icon, color) = conn.query_row(
        "SELECT name,icon,color FROM transaction_categories WHERE id=?1",
        [id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    Ok(CategorySpendingSummary {
        category_id: id.into(),
        category_name: name,
        category_icon: icon,
        category_color: color,
        total_amount: "0.00".into(),
        transaction_count: 0,
        budget_goal: None,
        budget_currency: None,
        budget_spent: None,
        budget_limit_native: None,
        budget_percentage: None,
        budget_limit: None,
        budget_limit_fx_quality: None,
    })
}
/// Integrate the effective versions of each calendar month. A quarterly/yearly
/// goal supplies its monthly equivalent; no version exists before its creation.
#[allow(clippy::type_complexity)]
fn period_limits(
    conn: &Connection,
    start: i64,
    end: i64,
    timeframe: &str,
    report_currency: &str,
) -> Result<HashMap<String, (Option<f64>, BudgetGoal)>> {
    use crate::services::reporting::{historical_amount, month_start, parse_money};
    use chrono::{DateTime, Datelike, NaiveDate, Utc};
    let mut stmt=conn.prepare("SELECT id,category_id,timeframe,amount,currency,effective_month,created_at FROM budget_goal_versions ORDER BY effective_month")?;
    let versions = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let Some(mut month) = month_start(start) else {
        return Ok(HashMap::new());
    };
    let count = match timeframe {
        "quarterly" => 3,
        "yearly" => 12,
        _ => 1,
    };
    let mut out: HashMap<String, (Option<f64>, BudgetGoal)> = HashMap::new();
    for _ in 0..count {
        if month > end {
            break;
        }
        let mut selected = HashMap::new();
        for v in versions.iter().filter(|v| v.5 <= month) {
            selected.insert((v.1.clone(), v.2.clone()), v);
        }
        // Monthly limits have priority over the less granular alternatives.
        let mut active: HashMap<String, _> = HashMap::new();
        for (_, v) in selected {
            if v.3.is_some() {
                let priority = match v.2.as_str() {
                    "monthly" => 3,
                    "quarterly" => 2,
                    _ => 1,
                };
                if active.get(&v.1).is_none_or(|(p, _)| *p < priority) {
                    active.insert(v.1.clone(), (priority, v));
                }
            }
        }
        for (category, (_, v)) in active {
            // A legacy non-finite limit is shown as unavailable, not fatal.
            let native = parse_money(v.3.as_deref().expect("active"))
                .ok()
                .map(|amount| {
                    amount
                        / match v.2.as_str() {
                            "quarterly" => 3.0,
                            "yearly" => 12.0,
                            _ => 1.0,
                        }
                });
            let czk = match native {
                Some(native) => historical_amount(conn, native, &v.4, report_currency, month)?,
                None => None,
            };
            let goal = BudgetGoal {
                id: v.0.clone(),
                category_id: category.clone(),
                timeframe: v.2.clone(),
                amount: v.3.clone().expect("active"),
                currency: v.4.clone(),
                created_at: v.6,
                updated_at: v.6,
                effective_month: Some(v.5),
                versions: Vec::new(),
            };
            let e = out.entry(category).or_insert((Some(0.0), goal.clone()));
            e.1 = goal;
            e.0 = match (e.0, czk) {
                (Some(a), Some(b)) => Some(a + b),
                _ => None,
            };
        }
        let d = DateTime::<Utc>::from_timestamp(month, 0).expect("month");
        let (y, m) = if d.month() == 12 {
            (d.year() + 1, 1)
        } else {
            (d.year(), d.month() + 1)
        };
        month = NaiveDate::from_ymd_opt(y, m, 1)
            .expect("month")
            .and_hms_opt(0, 0, 0)
            .expect("midnight")
            .and_utc()
            .timestamp();
    }
    // Prorated monthly shares retain precision until the complete period.
    // Round the denominator once so wire limits and utilization use one value.
    for (limit, _) in out.values_mut() {
        if let Some(value) = limit {
            *value = parse_money(&crate::services::reporting::money_in_currency(
                *value,
                report_currency,
            ))?;
        }
    }
    Ok(out)
}
pub fn get_category_transactions(
    conn: &Connection,
    category_id: &str,
    start: i64,
    end: i64,
) -> Result<Vec<BudgetingTransaction>> {
    get_category_transactions_in_currency(conn, category_id, start, end, "CZK")
}
pub fn get_category_transactions_in_currency(
    conn: &Connection,
    category_id: &str,
    start: i64,
    end: i64,
    report_currency: &str,
) -> Result<Vec<BudgetingTransaction>> {
    Ok(crate::services::reporting::transactions_in_currency(
        conn,
        start,
        end,
        None,
        report_currency,
    )?
    .into_iter()
    .filter(|r| {
        r.category_id.as_deref().unwrap_or("uncategorized") == category_id
            && matches!(
                r.effective_flow_kind,
                crate::models::FlowKind::Income | crate::models::FlowKind::Expense
            )
    })
    .collect())
}

// ========================== Async wrappers for Database ==========================

/// Get budgeting report (async wrapper)
pub async fn get_budgeting_report(
    db: &Database,
    start_date: i64,
    end_date: i64,
    timeframe: String,
) -> Result<BudgetingReport> {
    db.with_conn(|conn| get_report(conn, start_date, end_date, &timeframe))
}

/// Get category transactions (async wrapper)
pub async fn get_transactions_for_category(
    db: &Database,
    category_id: String,
    start_date: i64,
    end_date: i64,
) -> Result<Vec<BudgetingTransaction>> {
    db.with_conn(|conn| get_category_transactions(conn, &category_id, start_date, end_date))
}

/// Get all budget goals (async wrapper)
pub async fn get_budget_goals(db: &Database) -> Result<Vec<BudgetGoal>> {
    db.with_conn(get_all_goals)
}

/// Upsert budget goal (async wrapper)
pub async fn upsert_budget_goal(db: &Database, data: InsertBudgetGoal) -> Result<BudgetGoal> {
    db.with_conn(|conn| upsert_goal(conn, data))
}

/// Delete budget goal (async wrapper)
pub async fn delete_budget_goal(db: &Database, id: String) -> Result<()> {
    db.with_conn(|conn| delete_goal(conn, &id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("Failed to open in-memory db");

        // Create minimal schema for testing
        conn.execute_batch(
            r#"
            CREATE TABLE transaction_categories (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                icon TEXT,
                color TEXT, kind TEXT NOT NULL DEFAULT 'expense'
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
                description TEXT,
                counterparty_name TEXT,
                category_id TEXT,
                tx_type TEXT NOT NULL, status TEXT NOT NULL DEFAULT 'booked', flow_kind TEXT, counterparty_iban TEXT, counterparty_bank_code TEXT, counterparty_country TEXT
            );

            CREATE TABLE budget_goals (
                id TEXT PRIMARY KEY,
                category_id TEXT NOT NULL,
                timeframe TEXT NOT NULL,
                amount TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                UNIQUE(category_id, timeframe)
            );

            CREATE TABLE budget_goal_versions (id TEXT PRIMARY KEY,category_id TEXT NOT NULL,timeframe TEXT NOT NULL,effective_month INTEGER NOT NULL,amount TEXT,currency TEXT NOT NULL,created_at INTEGER NOT NULL,UNIQUE(category_id,timeframe,effective_month));
            CREATE TABLE exchange_rate_history (date INTEGER NOT NULL,currency TEXT NOT NULL,rate REAL NOT NULL,PRIMARY KEY(date,currency));
            INSERT INTO transaction_categories (id, name, icon, color) VALUES
                ('cat_groceries', 'Groceries', 'shopping-cart', '#4CAF50'),
                ('cat_dining', 'Dining', 'utensils', '#FF9800'),
                ('cat_income', 'Income', 'trending-up', '#8BC34A');

            INSERT INTO bank_accounts (id, name) VALUES
                ('acc1', 'Main Account');
            "#,
        )
        .expect("Failed to create test schema");

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

    const JAN: i64 = 1767225600;
    const MAR: i64 = 1772323200;
    const JUN: i64 = 1780272000;
    const OCT: i64 = 1790812800;
    const NOV: i64 = 1793491200;
    const DEC: i64 = 1796083200;
    /// 2026-10-15 12:00 UTC.
    const NOW: i64 = 1792065600;

    fn goal_input(amount: &str, month: i64) -> InsertBudgetGoal {
        InsertBudgetGoal {
            category_id: "cat_groceries".into(),
            timeframe: "monthly".into(),
            amount: amount.into(),
            currency: Some("CZK".into()),
            effective_month: Some(month),
        }
    }

    fn stored_amount(conn: &Connection) -> String {
        conn.query_row(
            "SELECT amount FROM budget_goals WHERE category_id='cat_groceries'",
            [],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn limit_for(conn: &Connection, month: i64, next: i64) -> Option<String> {
        get_report(conn, month, next - 1, "monthly")
            .unwrap()
            .expense_categories[0]
            .budget_limit
            .clone()
    }

    fn assert_native_period_limit(currency: &str, timeframe: &str, end: i64, expected: &str) {
        let conn = setup_test_db();
        let mut input = goal_input("100.00", JAN);
        input.currency = Some(currency.into());
        input.timeframe = timeframe.into();
        upsert_goal_at(&conn, input, JAN).unwrap();
        conn.execute("INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,category_id,tx_type) VALUES ('period','acc1',?1,'100',?2,'cat_groceries','debit')", params![JAN,currency]).unwrap();
        let report = get_report_in_currency(&conn, JAN, end, timeframe, None, currency).unwrap();
        let category = &report.expense_categories[0];
        assert_eq!(category.budget_limit.as_deref(), Some(expected));
        assert_eq!(category.budget_limit_native.as_deref(), Some(expected));
        assert_eq!(category.budget_spent.as_deref(), Some(expected));
        assert_eq!(category.budget_percentage, Some(100.0));
        assert_eq!(stored_amount(&conn), "100.00");
        assert_eq!(report.quality.missing_fx_count, 0);
    }
    #[test]
    fn target_period_limit_yearly_jpy_preserves_full_goal_before_rounding() {
        assert_native_period_limit("JPY", "yearly", 1798761600 - 1, "100");
    }
    #[test]
    fn target_period_limit_quarterly_eur_preserves_fractional_monthly_shares() {
        assert_native_period_limit("EUR", "quarterly", 1775001600 - 1, "100.00");
    }
    #[test]
    fn target_period_limit_yearly_eur_preserves_fractional_monthly_shares() {
        assert_native_period_limit("EUR", "yearly", 1798761600 - 1, "100.00");
    }

    /// Final review I-1: a limit scheduled for December must not become the
    /// current limit, and the version in effect now is what the goal exposes.
    #[test]
    fn target_budget_missing_native_spend_is_unavailable_not_false_underspend() {
        let conn = setup_test_db();
        let mut input = goal_input("100", OCT);
        input.currency = Some("EUR".into());
        upsert_goal_at(&conn, input, NOW).unwrap();
        conn.execute("INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,category_id,tx_type) VALUES ('mixed','acc1',?1,'50','USD','cat_groceries','debit')",[OCT]).unwrap();
        let report = get_report_in_currency(&conn, OCT, NOV - 1, "monthly", None, "USD").unwrap();
        assert_eq!(report.total_expenses, "50.00");
        assert!(report.expense_categories[0].budget_spent.is_none());
        assert!(report.expense_categories[0].budget_percentage.is_none());
        assert_eq!(
            report.expense_categories[0].budget_limit_native.as_deref(),
            Some("100.00")
        );
    }
    #[test]
    fn target_budget_latest_active_version_selects_native_evaluation_currency() {
        let conn = setup_test_db();
        for (amount, currency, month) in [("100.00", "EUR", OCT), ("110.00", "USD", NOV)] {
            let mut input = goal_input(amount, month);
            input.currency = Some(currency.into());
            upsert_goal_at(&conn, input, NOW).unwrap();
        }
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'EUR',27.5)",
            [OCT],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'USD',25)",
            [OCT],
        )
        .unwrap();
        let report = get_report_in_currency(&conn, OCT, DEC - 1, "quarterly", None, "EUR").unwrap();
        let cat = &report.expense_categories[0];
        assert_eq!(cat.budget_currency.as_deref(), Some("USD"));
        assert_eq!(cat.budget_limit_native.as_deref(), Some("220.00"));
        assert_eq!(cat.budget_goal.as_ref().unwrap().currency, "USD");
    }
    #[test]
    fn target_budget_utilization_is_independent_of_display_fx_changes() {
        let conn = setup_test_db();
        let mut input = goal_input("100.00", OCT);
        input.currency = Some("EUR".into());
        upsert_goal_at(&conn, input, NOW).unwrap();
        conn.execute("INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,category_id,tx_type) VALUES ('eur','acc1',?1,'50','EUR','cat_groceries','debit')",[OCT+86400]).unwrap();
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'EUR',25)",
            [OCT],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'USD',25)",
            [OCT],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'EUR',30)",
            [OCT + 86400],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO exchange_rate_history VALUES (?1,'USD',25)",
            [OCT + 86400],
        )
        .unwrap();
        let eur = get_report_in_currency(&conn, OCT, NOV - 1, "monthly", None, "EUR").unwrap();
        let usd = get_report_in_currency(&conn, OCT, NOV - 1, "monthly", None, "USD").unwrap();
        assert_eq!(eur.expense_categories[0].budget_percentage, Some(50.0));
        assert_eq!(usd.expense_categories[0].budget_percentage, Some(50.0));
        assert_eq!(usd.total_expenses, "60.00");
        assert_eq!(
            usd.expense_categories[0].budget_currency.as_deref(),
            Some("EUR")
        );
        assert_eq!(
            usd.expense_categories[0].budget_spent.as_deref(),
            Some("50.00")
        );
        assert_eq!(
            usd.expense_categories[0].budget_limit_native.as_deref(),
            Some("100.00")
        );
    }
    /// A native EUR budget stays evaluable without USD quotes; its count (and
    /// thus the drilldown) reflects the relevant transactions, not conversions.
    #[test]
    fn budget_transaction_count_does_not_depend_on_report_conversion() {
        let conn = setup_test_db();
        let mut input = goal_input("200.00", OCT);
        input.currency = Some("EUR".into());
        upsert_goal_at(&conn, input, NOW).unwrap();
        conn.execute_batch(&format!(
            "INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,category_id,tx_type) VALUES
             ('e1','acc1',{d},'60','EUR','cat_groceries','debit'),
             ('e2','acc1',{d},'40','EUR','cat_groceries','debit'),
             ('salary','acc1',{d},'500','EUR','cat_income','credit');",
            d = OCT + 86400
        ))
        .unwrap();
        let usd = get_report_in_currency(&conn, OCT, NOV - 1, "monthly", None, "USD").unwrap();
        assert_eq!(usd.quality.missing_fx_count, 3);
        let groceries = &usd.expense_categories[0];
        assert_eq!(groceries.budget_spent.as_deref(), Some("100.00"));
        assert_eq!(groceries.budget_percentage, Some(50.0));
        assert_eq!(groceries.transaction_count, 2);
        // Unconverted spending is not reported as a healthy zero row.
        assert!(usd.income_categories.is_empty());
    }
    #[test]
    fn target_budget_versions_retain_native_text_after_target_switch() {
        let conn = setup_test_db();
        for (amount, month) in [("100.00", OCT), ("200.00", NOV)] {
            let mut input = goal_input(amount, month);
            input.currency = Some("EUR".into());
            upsert_goal_at(&conn, input, NOW).unwrap();
        }
        let october = get_report_in_currency(&conn, OCT, NOV - 1, "monthly", None, "EUR").unwrap();
        assert_eq!(
            october.expense_categories[0].budget_limit.as_deref(),
            Some("100.00")
        );
        let quarter =
            get_report_in_currency(&conn, OCT, DEC - 1, "quarterly", None, "EUR").unwrap();
        assert_eq!(
            quarter.expense_categories[0].budget_limit.as_deref(),
            Some("300.00")
        );
        let usd = get_report_in_currency(&conn, OCT, NOV - 1, "monthly", None, "USD").unwrap();
        assert!(usd.expense_categories[0].budget_limit.is_none());
        assert_eq!(usd.expense_categories[0].budget_percentage, Some(0.0));
        assert_eq!(
            usd.expense_categories[0]
                .budget_goal
                .as_ref()
                .unwrap()
                .amount,
            "100.00"
        );
        let native: String = conn
            .query_row(
                "SELECT amount FROM budget_goal_versions WHERE effective_month=?1",
                [OCT],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(native, "100.00");
    }
    #[test]
    fn target_budget_native_limit_and_spend_need_no_fx() {
        let conn = setup_test_db();
        let mut input = goal_input("100.00", OCT);
        input.currency = Some("EUR".into());
        upsert_goal_at(&conn, input, NOW).unwrap();
        conn.execute("INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,category_id,tx_type) VALUES ('eur','acc1',1790812800,'25','EUR','cat_groceries','debit')", []).unwrap();
        let report = get_report_in_currency(&conn, OCT, NOV - 1, "monthly", None, "EUR").unwrap();
        assert_eq!(report.total_expenses, "25.00");
        assert_eq!(report.expense_categories[0].budget_percentage, Some(25.0));
        assert_eq!(stored_amount(&conn), "100.00");
    }
    #[test]
    fn scheduled_future_limit_keeps_the_current_limit_in_effect() {
        let conn = setup_test_db();
        upsert_goal_at(&conn, goal_input("1000", OCT), NOW).unwrap();
        let saved = upsert_goal_at(&conn, goal_input("2000", DEC), NOW).unwrap();

        assert_eq!(saved.amount, "1000", "current limit, not the scheduled one");
        assert_eq!(saved.effective_month, Some(OCT));
        assert_eq!(stored_amount(&conn), "1000");
        let goals = get_all_goals_at(&conn, NOW).unwrap();
        assert_eq!(goals[0].amount, "1000");
        assert_eq!(goals[0].effective_month, Some(OCT));
        let months: Vec<_> = goals[0]
            .versions
            .iter()
            .map(|v| (v.effective_month, v.amount.clone()))
            .collect();
        assert_eq!(
            months,
            vec![(OCT, Some("1000".into())), (DEC, Some("2000".into()))]
        );

        // Reopening the dialog prefills the October version; saving it
        // unchanged for October keeps October at 1 000 and December at 2 000.
        upsert_goal_at(&conn, goal_input("1000", OCT), NOW).unwrap();
        assert_eq!(limit_for(&conn, OCT, NOV).as_deref(), Some("1000.00"));
        assert_eq!(
            limit_for(&conn, DEC, DEC + 31 * 86400).as_deref(),
            Some("2000.00")
        );
        assert_eq!(stored_amount(&conn), "1000");
    }

    #[test]
    fn back_dated_edit_does_not_replace_the_limit_in_effect_now() {
        let conn = setup_test_db();
        upsert_goal_at(&conn, goal_input("1000", JAN), NOW).unwrap();
        upsert_goal_at(&conn, goal_input("1500", JUN), NOW).unwrap();
        let saved = upsert_goal_at(&conn, goal_input("1200", MAR), NOW).unwrap();

        assert_eq!(saved.amount, "1500");
        assert_eq!(saved.effective_month, Some(JUN));
        assert_eq!(stored_amount(&conn), "1500");
        assert_eq!(
            limit_for(&conn, MAR, MAR + 31 * 86400).as_deref(),
            Some("1200.00")
        );
        assert_eq!(limit_for(&conn, OCT, NOV).as_deref(), Some("1500.00"));
    }

    #[test]
    fn goal_scheduled_only_for_the_future_exposes_its_first_version() {
        let conn = setup_test_db();
        let saved = upsert_goal_at(&conn, goal_input("2000", DEC), NOW).unwrap();
        assert_eq!(saved.amount, "2000");
        assert_eq!(saved.effective_month, Some(DEC));
        assert_eq!(saved.versions.len(), 1);
    }

    #[test]
    fn unchanged_amount_keeps_the_text_of_the_version_in_effect_for_that_month() {
        let conn = setup_test_db();
        upsert_goal_at(&conn, goal_input("1000.123", JAN), NOW).unwrap();
        upsert_goal_at(&conn, goal_input("2000", DEC), NOW).unwrap();
        // Same value as January's, spelled differently: keep the stored text.
        upsert_goal_at(&conn, goal_input("1000.1230", MAR), NOW).unwrap();
        let mar: String = conn
            .query_row(
                "SELECT amount FROM budget_goal_versions WHERE effective_month=?1",
                [MAR],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mar, "1000.123");
    }

    #[test]
    fn changing_or_deleting_budget_preserves_prior_month_limit() {
        let conn = setup_test_db();
        let jan = 1704067200;
        let feb = 1706745600;
        let goal = upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".into(),
                timeframe: "monthly".into(),
                amount: "1000.123".into(),
                currency: Some("CZK".into()),
                effective_month: Some(jan),
            },
        )
        .unwrap();
        upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".into(),
                timeframe: "monthly".into(),
                amount: "2000".into(),
                currency: Some("CZK".into()),
                effective_month: Some(feb),
            },
        )
        .unwrap();
        let report = get_report(&conn, jan, feb - 1, "monthly").unwrap();
        assert_eq!(
            report.expense_categories.len(),
            1,
            "zero-spend budget remains visible"
        );
        assert_eq!(
            report.expense_categories[0].budget_limit.as_deref(),
            Some("1000.12")
        );
        delete_goal(&conn, &goal.id).unwrap();
        let report = get_report(&conn, jan, feb - 1, "monthly").unwrap();
        assert_eq!(
            report.expense_categories[0].budget_limit.as_deref(),
            Some("1000.12")
        );
    }

    #[test]
    fn signed_report_conserves_cross_category_refunds_income_reversals_and_pending() {
        let conn = setup_test_db();
        conn.execute_batch("INSERT INTO transaction_categories(id,name,kind) VALUES ('custom-income','Consulting','income');
          INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,category_id,tx_type) VALUES
          ('d','acc1',1790812800,'1000','CZK','cat_dining','debit'),
          ('r','acc1',1790899200,'500','CZK','cat_groceries','credit'),
          ('i','acc1',1790812800,'1000','CZK','cat_income','credit'),
          ('ir','acc1',1790812800,'100','CZK','cat_income','debit'),
          ('ci','acc1',1790812800,'2000','CZK','custom-income','credit');").unwrap();
        conn.execute("INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,tx_type,status) VALUES ('pending','acc1',1790812800,'9999','CZK','debit','pending')",[]).unwrap();
        let report = get_report(&conn, 1790812800, 1791158400, "monthly").unwrap();
        assert_eq!(report.total_expenses, "500.00");
        assert_eq!(report.total_income, "2900.00");
        let cashflow =
            crate::services::cashflow_actuals::get_actuals(&conn, "6m", 1_791_000_000).unwrap();
        assert_eq!(cashflow.total_expenses, "500.00");
        assert_eq!(cashflow.total_income, "2900.00");
        let drilldown =
            get_category_transactions(&conn, "cat_groceries", 1790812800, 1791158400).unwrap();
        assert_eq!(drilldown.len(), 1);
        assert_eq!(drilldown[0].signed_amount.as_deref(), Some("-500.00"));

        assert!(report
            .expense_categories
            .iter()
            .any(|g| g.total_amount == "-500.00"));
    }

    #[test]
    fn quarterly_limits_integrate_versions_currency_and_deleted_months() {
        let conn = setup_test_db();
        let jan = 1704067200;
        let feb = 1706745600;
        let march = 1709251200;
        for (month, amount, currency) in [
            (jan, "1000.123", "CZK"),
            (feb, "100", "EUR"),
            (march, "3000", "CZK"),
        ] {
            upsert_goal(
                &conn,
                InsertBudgetGoal {
                    category_id: "cat_groceries".into(),
                    timeframe: "monthly".into(),
                    amount: amount.into(),
                    currency: Some(currency.into()),
                    effective_month: Some(month),
                },
            )
            .unwrap();
        }
        add_rate(&conn, feb, "EUR", 25.0);
        let quarter = get_report(&conn, jan, 1711929599, "quarterly").unwrap();
        assert_eq!(
            quarter.expense_categories[0].budget_limit.as_deref(),
            Some("6500.12")
        );
        let old = get_report(&conn, jan, feb - 1, "monthly").unwrap();
        assert_eq!(
            old.expense_categories[0].budget_limit.as_deref(),
            Some("1000.12")
        );
        let before = get_report(&conn, 1672531200, 1675209599, "monthly").unwrap();
        assert!(before.expense_categories.is_empty());
        let future_goal = get_all_goals(&conn).unwrap().remove(0);
        delete_goal(&conn, &future_goal.id).unwrap();
        let old = get_report(&conn, jan, feb - 1, "monthly").unwrap();
        assert_eq!(
            old.expense_categories[0].budget_limit.as_deref(),
            Some("1000.12")
        );
    }

    #[test]
    fn test_budget_goal_crud() {
        let conn = setup_test_db();

        // Create a goal
        let goal = upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".to_string(),
                timeframe: "monthly".to_string(),
                amount: "5000".to_string(),
                currency: Some("CZK".to_string()),
                effective_month: None,
            },
        )
        .expect("Failed to create goal");

        assert_eq!(goal.category_id, "cat_groceries");
        assert_eq!(goal.amount, "5000");
        assert_eq!(goal.timeframe, "monthly");

        // Update the goal
        let updated = upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".to_string(),
                timeframe: "monthly".to_string(),
                amount: "6000".to_string(),
                currency: Some("CZK".to_string()),
                effective_month: None,
            },
        )
        .expect("Failed to update goal");

        assert_eq!(updated.id, goal.id); // Same ID
        assert_eq!(updated.amount, "6000");

        // Delete the goal
        delete_goal(&conn, &goal.id).expect("Failed to delete goal");

        let goals = get_all_goals(&conn).expect("Failed to get goals");
        assert!(goals.is_empty());
    }

    #[test]
    fn test_get_report_empty() {
        let conn = setup_test_db();

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("Failed to get report");

        assert_eq!(report.total_income, "0.00");
        assert_eq!(report.total_expenses, "0.00");
        assert_eq!(report.net_balance, "0.00");
        assert!(report.income_categories.is_empty());
        assert!(report.expense_categories.is_empty());
    }

    #[test]
    fn test_get_report_with_transactions() {
        let conn = setup_test_db();

        // Add some test transactions
        conn.execute_batch(
            r#"
            INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type) VALUES
                ('tx1', 'acc1', 1704067200, '100.50', 'CZK', 'cat_groceries', 'debit'),
                ('tx2', 'acc1', 1704067200, '50.00', 'CZK', 'cat_groceries', 'debit'),
                ('tx3', 'acc1', 1704067200, '1000.00', 'CZK', 'cat_income', 'credit');
            "#,
        )
        .expect("Failed to insert transactions");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("Failed to get report");

        assert_eq!(report.total_income, "1000.00");
        assert_eq!(report.total_expenses, "150.50");
        assert_eq!(report.net_balance, "849.50");
        assert_eq!(report.income_categories.len(), 1);
        assert_eq!(report.expense_categories.len(), 1);
        assert_eq!(report.expense_categories[0].transaction_count, 2);
    }

    #[test]
    fn test_validate_budget_goal() {
        // Valid goal
        let valid = InsertBudgetGoal {
            category_id: "cat_groceries".to_string(),
            timeframe: "monthly".to_string(),
            amount: "5000".to_string(),
            currency: None,
            effective_month: None,
        };
        assert!(valid.validate().is_ok());

        // Invalid timeframe
        let invalid_timeframe = InsertBudgetGoal {
            category_id: "cat_groceries".to_string(),
            timeframe: "weekly".to_string(),
            amount: "5000".to_string(),
            currency: None,
            effective_month: None,
        };
        assert!(invalid_timeframe.validate().is_err());

        // Invalid amount
        let invalid_amount = InsertBudgetGoal {
            category_id: "cat_groceries".to_string(),
            timeframe: "monthly".to_string(),
            amount: "-100".to_string(),
            currency: None,
            effective_month: None,
        };
        assert!(invalid_amount.validate().is_err());
    }

    #[test]
    fn test_get_all_goals_multiple_categories() {
        let conn = setup_test_db();

        upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".to_string(),
                timeframe: "monthly".to_string(),
                amount: "3000".to_string(),
                currency: Some("CZK".to_string()),
                effective_month: None,
            },
        )
        .unwrap();

        upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_dining".to_string(),
                timeframe: "monthly".to_string(),
                amount: "1500".to_string(),
                currency: Some("CZK".to_string()),
                effective_month: None,
            },
        )
        .unwrap();

        let goals = get_all_goals(&conn).expect("get goals");
        assert_eq!(goals.len(), 2);
    }

    #[test]
    fn test_validate_budget_goal_zero_amount_invalid() {
        let invalid = InsertBudgetGoal {
            category_id: "cat_groceries".to_string(),
            timeframe: "monthly".to_string(),
            amount: "0".to_string(),
            currency: None,
            effective_month: None,
        };
        assert!(invalid.validate().is_err());
    }

    /// Booking dates are stored as UTC midnight of the calendar day (ADR 0008).
    /// The report's inclusive `end` bound must therefore be the last second of
    /// the period's last day in UTC — the local-time boundaries the frontend
    /// used to send dropped the last day for UTC+ users.
    #[test]
    fn last_day_of_month_at_utc_midnight_is_inside_the_month() {
        let conn = setup_test_db();
        const SEPT_30_UTC: i64 = 1_790_726_400; // 2026-09-30T00:00:00Z
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type)
             VALUES ('tx1', 'acc1', ?1, '100.00', 'CZK', 'cat_groceries', 'debit')",
            [SEPT_30_UTC],
        )
        .expect("insert");

        // UTC period bounds: 2026-09-01T00:00:00Z ..= 2026-09-30T23:59:59Z
        let report = get_report(&conn, 1_788_220_800, 1_790_812_799, "monthly").expect("report");
        assert_eq!(report.total_expenses, "100.00");

        // Local-time bounds from a UTC+2 client (the old frontend behaviour):
        // 2026-08-31T22:00:00Z ..= 2026-09-29T22:00:00Z — the transaction is lost.
        let local = get_report(&conn, 1_788_213_600, 1_790_719_200, "monthly").expect("report");
        assert_eq!(local.total_expenses, "0.00");

        // ...and the following month (starting 2026-09-30T22:00:00Z) misses it too.
        let next = get_report(&conn, 1_790_805_600, 1_793_397_600, "monthly").expect("report");
        assert_eq!(next.total_expenses, "0.00");
    }

    #[test]
    fn year_end_at_utc_midnight_is_inside_the_year() {
        let conn = setup_test_db();
        const DEC_31_UTC: i64 = 1_798_675_200; // 2026-12-31T00:00:00Z
        conn.execute(
            "INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type)
             VALUES ('tx1', 'acc1', ?1, '250.00', 'CZK', 'cat_income', 'credit')",
            [DEC_31_UTC],
        )
        .expect("insert");
        // 2026-01-01T00:00:00Z ..= 2026-12-31T23:59:59Z
        let report = get_report(&conn, 1_767_225_600, 1_798_761_599, "yearly").expect("report");
        assert_eq!(report.total_income, "250.00");
    }

    #[test]
    fn budget_percentage_converts_goal_currency_to_czk() {
        // XTS is an ISO test code, so it cannot collide with real rates used
        // by other tests sharing the global rate cache.
        crate::services::currency::update_exchange_rates(HashMap::from([(
            "XTS".to_string(),
            10.0,
        )]));
        let conn = setup_test_db();
        upsert_goal(
            &conn,
            InsertBudgetGoal {
                category_id: "cat_groceries".to_string(),
                timeframe: "monthly".to_string(),
                amount: "100".to_string(), // 100 XTS = 1 000 CZK
                currency: Some("XTS".to_string()),
                effective_month: Some(1704067200),
            },
        )
        .expect("goal");
        add_rate(&conn, 1704067200, "XTS", 10.0);
        conn.execute_batch(
            r#"INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type)
               VALUES ('tx1', 'acc1', 1704067200, '250', 'CZK', 'cat_groceries', 'debit');"#,
        )
        .expect("insert");

        let report = get_report(&conn, 1704067200, 1706745599, "monthly").expect("report");
        let groceries = &report.expense_categories[0];
        // 250 CZK spent of a 1 000 CZK budget = 25 % (not 250 % as with a CZK assumption)
        assert!((groceries.budget_percentage.expect("pct") - 25.0).abs() < 1e-9);
    }

    fn add_fx_history(_conn: &Connection) {}

    fn add_rate(conn: &Connection, day: i64, currency: &str, rate: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            params![day, currency, rate],
        )
        .expect("rate");
    }

    fn add_expense(conn: &Connection, id: &str, booked: i64, amount: &str, currency: &str) {
        conn.execute(
            "INSERT INTO bank_transactions
             (id, bank_account_id, booking_date, amount, currency, category_id, tx_type)
             VALUES (?1, 'acc1', ?2, ?3, ?4, 'cat_groceries', 'debit')",
            params![id, booked, amount, currency],
        )
        .expect("tx");
    }

    /// ADR 0001: a foreign-currency transaction is converted at the
    /// rate of its booking day, not at today's.
    #[test]
    fn foreign_currency_transactions_convert_at_the_booking_day_rate() {
        // XTB: an ISO test code that no other test touches in the shared cache.
        crate::services::currency::update_exchange_rates(HashMap::from([(
            "XTB".to_string(),
            25.2, // today
        )]));
        let conn = setup_test_db();
        add_fx_history(&conn);
        let january: i64 = 1_705_276_800; // 2024-01-15
        let june: i64 = 1_718_409_600; // 2024-06-15
        add_rate(&conn, january, "XTB", 24.5);
        add_rate(&conn, june, "XTB", 24.0);
        add_expense(&conn, "tx1", january, "100", "XTB");
        add_expense(&conn, "tx2", june, "100", "XTB");

        let report = get_report(&conn, 0, i64::MAX, "yearly").expect("report");
        // 100 * 24.5 + 100 * 24.0 — not 2 * 100 * 25.2
        assert_eq!(report.total_expenses, "4850.00");
        assert_eq!(report.expense_categories[0].total_amount, "4850.00");
        assert_eq!(report.expense_categories[0].transaction_count, 2);
    }

    #[test]
    fn same_day_transactions_share_one_rate_and_czk_is_untouched() {
        crate::services::currency::update_exchange_rates(HashMap::from([(
            "XTB".to_string(),
            25.2,
        )]));
        let conn = setup_test_db();
        add_fx_history(&conn);
        let day: i64 = 1_705_276_800;
        add_rate(&conn, day, "XTB", 24.5);
        add_expense(&conn, "tx1", day + 3600, "10", "XTB");
        add_expense(&conn, "tx2", day + 7200, "30", "XTB");
        add_expense(&conn, "tx3", day, "500", "CZK");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        // 40 XTB at 24.5 = 980, plus 500 CZK
        assert_eq!(report.total_expenses, "1480.00");
    }

    #[test]
    fn a_day_without_history_is_explicitly_missing_not_current_rate() {
        crate::services::currency::update_exchange_rates(HashMap::from([(
            "XTB".to_string(),
            25.2,
        )]));
        // No exchange_rate_history table rows at all.
        let conn = setup_test_db();
        add_fx_history(&conn);
        add_expense(&conn, "tx1", 1_705_276_800, "100", "XTB");
        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        assert_eq!(report.total_expenses, "0.00");
        assert_eq!(report.quality.missing_fx_count, 1);
        assert_eq!(report.quality.missing_fx_currencies, vec!["XTB"]);

        // History that only starts after the booking day: also the latest rate.
        let conn = setup_test_db();
        add_fx_history(&conn);
        add_rate(&conn, 1_705_276_800 + 30 * 86_400, "XTB", 20.0);
        add_expense(&conn, "tx1", 1_705_276_800, "100", "XTB");
        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        assert_eq!(report.total_expenses, "0.00");
        assert_eq!(report.quality.missing_fx_count, 1);
        assert_eq!(report.quality.missing_fx_currencies, vec!["XTB"]);
    }

    /// a credit nobody categorized yet is still money that
    /// came in. Uncategorized debits already counted toward `total_expenses`;
    /// the credit side must be symmetric.
    #[test]
    fn resolved_expenses_include_income_and_transfer_category_ids() {
        let conn = setup_test_db();
        conn.execute_batch("INSERT INTO transaction_categories(id,name,kind) VALUES ('cat_internal_transfers','Transfers','transfer');
          INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,tx_type,flow_kind,category_id) VALUES
          ('income-as-expense','acc1',1704067200,'100','CZK','debit','expense','cat_income'),
          ('transfer-as-expense','acc1',1704067200,'50','CZK','debit','expense','cat_internal_transfers');").unwrap();
        let report = get_report(&conn, 1704067200, 1704153599, "monthly").unwrap();
        assert_eq!(report.total_expenses, "150.00");
        assert_eq!(report.expense_categories.len(), 2);
        assert!(report
            .expense_categories
            .iter()
            .any(|group| group.category_id == "cat_income"
                && group.total_amount == "100.00"
                && group.transaction_count == 1));
        assert!(report
            .expense_categories
            .iter()
            .any(|group| group.category_id == "cat_internal_transfers"
                && group.total_amount == "50.00"
                && group.transaction_count == 1));
        conn.execute_batch("UPDATE transaction_categories SET kind='expense' WHERE id='cat_income'; UPDATE bank_transactions SET flow_kind=NULL WHERE id='income-as-expense';").unwrap();
        let by_role = get_report(&conn, 1704067200, 1704153599, "monthly").unwrap();
        assert_eq!(by_role.total_expenses, "150.00");
        assert_eq!(by_role.expense_categories.len(), 2);
    }

    #[test]
    fn zero_net_uncategorized_expenses_have_their_own_drilldown_count() {
        let conn = setup_test_db();
        conn.execute_batch("INSERT INTO bank_transactions(id,bank_account_id,booking_date,amount,currency,tx_type,flow_kind,status) VALUES
          ('paid','acc1',1704067200,'100','CZK','debit',NULL,'booked'),
          ('refund','acc1',1704067200,'100','CZK','credit','expense','booked'),
          ('unknown-fx','acc1',1704067200,'10','AED','debit',NULL,'booked'),
          ('income1','acc1',1704067200,'200','CZK','credit',NULL,'booked'),
          ('income2','acc1',1704067200,'300','CZK','credit',NULL,'booked'),
          ('pending','acc1',1704067200,'100','CZK','debit',NULL,'pending');").unwrap();
        let report = get_report(&conn, 1704067200, 1704153599, "monthly").unwrap();
        assert_eq!(report.uncategorized_expenses, "0.00");
        assert_eq!(report.uncategorized_transaction_count, 4);
        assert_eq!(
            serde_json::to_value(&report).unwrap()["uncategorizedExpenseTransactionCount"],
            3
        );
        let drilldown =
            crate::services::reporting::transactions(&conn, 1704067200, 1704153599, None).unwrap();
        assert_eq!(
            drilldown
                .iter()
                .filter(|row| row.category_id.is_none()
                    && row.effective_flow_kind == crate::models::FlowKind::Expense)
                .count(),
            3
        );
    }

    #[test]
    fn uncategorized_credits_count_as_income() {
        let conn = setup_test_db();
        conn.execute_batch(
            r#"
            INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type) VALUES
                ('tx1', 'acc1', 1704067200, '30000.00', 'CZK', NULL, 'credit'),
                ('tx2', 'acc1', 1704067200, '50000.00', 'CZK', 'cat_income', 'credit'),
                ('tx3', 'acc1', 1704067200, '20000.00', 'CZK', 'cat_groceries', 'debit'),
                ('tx4', 'acc1', 1704067200, '5000.00', 'CZK', NULL, 'debit');
            "#,
        )
        .expect("insert");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");

        assert_eq!(report.total_income, "80000.00");
        assert_eq!(report.total_expenses, "25000.00");
        assert_eq!(report.net_balance, "55000.00");
        // Still reported separately so the UI can show an "Uncategorized income" row.
        assert_eq!(report.uncategorized_income, "30000.00");
        assert_eq!(report.uncategorized_expenses, "5000.00");
        assert_eq!(report.uncategorized_transaction_count, 2);
        // The per-category income list is unchanged: uncategorized is not a category.
        assert_eq!(report.income_categories.len(), 1);
        assert_eq!(report.income_categories[0].total_amount, "50000.00");
    }

    #[test]
    fn only_uncategorized_credits_still_produce_income_and_a_surplus() {
        let conn = setup_test_db();
        conn.execute_batch(
            r#"
            INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type) VALUES
                ('tx1', 'acc1', 1704067200, '3200.00', 'CZK', NULL, 'credit');
            "#,
        )
        .expect("insert");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        assert_eq!(report.total_income, "3200.00");
        assert_eq!(report.net_balance, "3200.00");
    }

    #[test]
    fn test_get_report_only_income() {
        let conn = setup_test_db();

        conn.execute_batch(
            r#"
            INSERT INTO bank_transactions (id, bank_account_id, booking_date, amount, currency, category_id, tx_type) VALUES
                ('tx1', 'acc1', 1704067200, '500.00', 'CZK', 'cat_income', 'credit');
            "#,
        )
        .expect("insert");

        let report = get_report(&conn, 0, i64::MAX, "monthly").expect("report");
        assert_eq!(report.total_income, "500.00");
        assert_eq!(report.total_expenses, "0.00");
        assert_eq!(report.net_balance, "500.00");
    }
}
