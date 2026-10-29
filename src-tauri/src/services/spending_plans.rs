//! Spending plans: CRUD, closing and tagging payments to a plan.
//!
//! A plan is unique per (year, name) compared case-insensitively over Unicode, so the check is
//! done in Rust (SQLite `lower()` only folds ASCII). Deleting a plan clears the tag on its
//! payments but never touches the payments themselves.

use crate::error::{AppError, Result};
use crate::models::fx::ConversionQuality;
use crate::models::{
    BankTransaction, InsertSpendingPlan, PlanCandidateFilters, PlanCandidatesResult, SpendingPlan,
    SpendingPlanAssignment, SpendingPlanDetail, SpendingPlanSummary, SpendingPlansOverview,
};
use crate::models::{
    SpendingPlanCategory, SpendingPlanCell, SpendingPlanMonth, SpendingPlanSpent,
    SpendingPlanTotals,
};
use crate::services::bank_history::{transaction_from_row, TRANSACTION_COLUMNS};
use crate::services::categorization::tokenizer::fold_text;
use crate::services::fx;
use crate::services::reporting::money_in_currency;
use chrono::{DateTime, Datelike, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

const COLUMNS: &str = "id, name, year, amount, currency, icon, color, note, closed_at, created_at";

fn plan_row(row: &rusqlite::Row) -> rusqlite::Result<SpendingPlan> {
    Ok(SpendingPlan {
        id: row.get(0)?,
        name: row.get(1)?,
        year: row.get(2)?,
        amount: row.get(3)?,
        currency: row.get(4)?,
        icon: row.get(5)?,
        color: row.get(6)?,
        note: row.get(7)?,
        closed_at: row.get(8)?,
        created_at: row.get(9)?,
    })
}

/// Comparison key for plan names: trimmed, Unicode-lowercased.
pub(crate) fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

fn not_found(id: &str) -> AppError {
    AppError::NotFound(format!("Spending plan {id}"))
}

fn ensure_name_free(conn: &Connection, year: i32, name: &str, except: Option<&str>) -> Result<()> {
    let key = name_key(name);
    let mut stmt = conn.prepare("SELECT id, name FROM spending_plans WHERE year = ?1")?;
    let rows = stmt.query_map([year], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in rows {
        let (id, existing) = row?;
        if Some(id.as_str()) != except && name_key(&existing) == key {
            return Err(AppError::Validation(
                "validation.spendingPlanNameTaken".into(),
            ));
        }
    }
    Ok(())
}

fn insert_plan(conn: &Connection, data: &InsertSpendingPlan) -> Result<SpendingPlan> {
    let data = data.validate()?;
    ensure_name_free(conn, data.year, &data.name, None)?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO spending_plans (id, name, year, amount, currency, icon, color, note)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            id,
            data.name,
            data.year,
            data.amount,
            data.currency,
            data.icon,
            data.color,
            data.note
        ],
    )?;
    get_plan(conn, &id)
}

pub fn create_plan(conn: &Connection, data: &InsertSpendingPlan) -> Result<SpendingPlan> {
    insert_plan(conn, data)
}

/// Create several plans in one transaction; any failure leaves nothing behind.
pub fn create_plans(conn: &Connection, items: &[InsertSpendingPlan]) -> Result<Vec<SpendingPlan>> {
    let tx = conn.unchecked_transaction()?;
    let mut created = Vec::with_capacity(items.len());
    for item in items {
        created.push(insert_plan(&tx, item)?);
    }
    tx.commit()?;
    Ok(created)
}

pub fn get_plan(conn: &Connection, id: &str) -> Result<SpendingPlan> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM spending_plans WHERE id = ?1"),
        [id],
        plan_row,
    )
    .optional()?
    .ok_or_else(|| not_found(id))
}

pub fn update_plan(conn: &Connection, id: &str, data: &InsertSpendingPlan) -> Result<SpendingPlan> {
    let data = data.validate()?;
    get_plan(conn, id)?;
    ensure_name_free(conn, data.year, &data.name, Some(id))?;
    conn.execute(
        "UPDATE spending_plans SET name = ?2, year = ?3, amount = ?4, currency = ?5, icon = ?6,
             color = ?7, note = ?8, updated_at = unixepoch() WHERE id = ?1",
        params![
            id,
            data.name,
            data.year,
            data.amount,
            data.currency,
            data.icon,
            data.color,
            data.note
        ],
    )?;
    get_plan(conn, id)
}

pub fn set_plan_closed(conn: &Connection, id: &str, closed: bool) -> Result<SpendingPlan> {
    get_plan(conn, id)?;
    conn.execute(
        "UPDATE spending_plans
         SET closed_at = CASE WHEN ?2 THEN unixepoch() ELSE NULL END, updated_at = unixepoch()
         WHERE id = ?1",
        params![id, closed],
    )?;
    get_plan(conn, id)
}

/// Delete a plan; its payments stay, untagged.
pub fn delete_plan(conn: &Connection, id: &str) -> Result<()> {
    get_plan(conn, id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE bank_transactions SET spending_plan_id = NULL WHERE spending_plan_id = ?1",
        [id],
    )?;
    tx.execute("DELETE FROM spending_plans WHERE id = ?1", [id])?;
    tx.commit()?;
    Ok(())
}

/// Tag, move or untag payments in one transaction. Tagging to a closed plan is refused unless
/// `restore` (an undo putting earlier tags back).
pub fn set_transaction_plans(
    conn: &Connection,
    assignments: &[SpendingPlanAssignment],
    restore: bool,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for a in assignments {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM bank_transactions WHERE id = ?1)",
            [&a.transaction_id],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(AppError::NotFound(format!(
                "Transaction {}",
                a.transaction_id
            )));
        }
        if let Some(plan_id) = &a.plan_id {
            let plan = get_plan(&tx, plan_id)?;
            if plan.closed_at.is_some() && !restore {
                return Err(AppError::Validation("validation.spendingPlanClosed".into()));
            }
        }
        tx.execute(
            "UPDATE bank_transactions SET spending_plan_id = ?2 WHERE id = ?1",
            params![a.transaction_id, a.plan_id],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// A tagged payment, reduced to what the sums need.
struct Payment {
    plan_id: String,
    account_id: String,
    booking_date: i64,
    /// Debit positive, credit negative; `None` when the stored amount is not a finite number.
    signed: Option<f64>,
    currency: String,
    category_id: Option<String>,
    booked: bool,
}

/// Payments tagged to any plan matching `plan_filter` (a SQL condition on `spending_plans sp`).
fn load_payments(
    conn: &Connection,
    plan_filter: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Payment>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT t.spending_plan_id, t.bank_account_id, t.booking_date, t.tx_type, t.amount,
                t.currency, t.category_id, t.status
         FROM bank_transactions t JOIN spending_plans sp ON sp.id = t.spending_plan_id
         WHERE {plan_filter}"
    ))?;
    let rows = stmt.query_map(args, |r| {
        let tx_type: String = r.get(3)?;
        let amount: String = r.get(4)?;
        let status: String = r.get(7)?;
        let signed = amount
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .map(|v| if tx_type == "credit" { -v } else { v });
        Ok(Payment {
            plan_id: r.get(0)?,
            account_id: r.get(1)?,
            booking_date: r.get(2)?,
            signed,
            currency: r.get(5)?,
            category_id: r.get(6)?,
            booked: status == "booked",
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// One History::load over the booking days of the booked payments.
fn history_for(conn: &Connection, payments: &[Payment]) -> Result<fx::History> {
    let days = payments.iter().filter(|p| p.booked).map(|p| p.booking_date);
    match (days.clone().min(), days.max()) {
        (Some(min), Some(max)) => fx::History::load(conn, min, max),
        _ => Ok(fx::History::from_days(Default::default())),
    }
}

/// A booked payment's signed amount in the plan currency.
struct Converted<'a> {
    payment: &'a Payment,
    amount: f64,
}

/// Convert the booked payments of one plan at their booking day; failures go to `quality`.
fn convert_booked<'a>(
    plan: &SpendingPlan,
    payments: &[&'a Payment],
    history: &fx::History,
    quality: &mut fx::Quality,
) -> Vec<Converted<'a>> {
    let mut out = Vec::new();
    for p in payments.iter().filter(|p| p.booked) {
        let amount = match p.signed {
            Some(v) => {
                history.convert_on_tracked(quality, v, &p.currency, &plan.currency, p.booking_date)
            }
            None => quality.track(&p.currency, None),
        };
        if let Some(amount) = amount {
            out.push(Converted { payment: p, amount });
        }
    }
    out
}

fn summarize<'a>(
    plan: SpendingPlan,
    payments: &[&'a Payment],
    history: &fx::History,
) -> (SpendingPlanSummary, fx::Quality, Vec<Converted<'a>>) {
    let mut quality = fx::Quality::default();
    let converted = convert_booked(&plan, payments, history, &mut quality);
    let booked = payments.iter().filter(|p| p.booked).count();
    let spent = if booked == 0 {
        Some(money_in_currency(0.0, &plan.currency))
    } else if converted.is_empty() {
        None
    } else {
        let sum: f64 = converted.iter().map(|c| c.amount).sum();
        Some(money_in_currency(sum, &plan.currency))
    };
    let summary = SpendingPlanSummary {
        spent,
        payment_count: payments.len() as i64,
        last_payment_date: payments.iter().map(|p| p.booking_date).max(),
        fx_quality: ConversionQuality::from(&quality),
        plan,
    };
    (summary, quality, converted)
}

fn plans_where(
    conn: &Connection,
    filter: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<SpendingPlan>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM spending_plans sp WHERE {filter} ORDER BY created_at, name, id"
    ))?;
    let rows = stmt.query_map(args, plan_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// Each plan's summary, its conversion quality and its booked payments in the plan currency.
fn summaries<'a>(
    conn: &Connection,
    plans: Vec<SpendingPlan>,
    payments: &'a [Payment],
) -> Result<Vec<(SpendingPlanSummary, fx::Quality, Vec<Converted<'a>>)>> {
    let history = history_for(conn, payments)?;
    let mut by_plan: HashMap<&str, Vec<&Payment>> = HashMap::new();
    for p in payments {
        by_plan.entry(p.plan_id.as_str()).or_default().push(p);
    }
    Ok(plans
        .into_iter()
        .map(|plan| {
            let own = by_plan.get(plan.id.as_str()).map_or(&[][..], Vec::as_slice);
            summarize(plan, own, &history)
        })
        .collect())
}

/// Every plan of every year with its spent (ledger chips, plan picker).
pub fn plan_index(conn: &Connection) -> Result<Vec<SpendingPlanSummary>> {
    let plans = plans_where(conn, "1 = 1", &[])?;
    let payments = load_payments(conn, "1 = 1", &[])?;
    Ok(summaries(conn, plans, &payments)?
        .into_iter()
        .map(|(summary, _, _)| summary)
        .collect())
}

/// (sum, count) per category of converted payments.
fn sum_by_category(converted: &[Converted]) -> HashMap<Option<String>, (f64, i64)> {
    let mut sums: HashMap<Option<String>, (f64, i64)> = HashMap::new();
    for c in converted {
        let entry = sums.entry(c.payment.category_id.clone()).or_default();
        entry.0 += c.amount;
        entry.1 += 1;
    }
    sums
}

/// Category rows in `currency`, largest spend first (then more payments, then by id).
fn category_rows(
    sums: HashMap<Option<String>, (f64, i64)>,
    currency: &str,
) -> Vec<SpendingPlanCategory> {
    let mut rows: Vec<(f64, SpendingPlanCategory)> = sums
        .into_iter()
        .map(|(category_id, (sum, count))| {
            let category = SpendingPlanCategory {
                category_id,
                spent: money_in_currency(sum, currency),
                count,
            };
            (sum, category)
        })
        .collect();
    rows.sort_by(|(a_sum, a), (b_sum, b)| {
        b_sum
            .total_cmp(a_sum)
            .then(b.count.cmp(&a.count))
            .then_with(|| a.category_id.cmp(&b.category_id))
    });
    rows.into_iter().map(|(_, c)| c).collect()
}

/// Sum of values still convertible; `None` when `attempted > 0` and nothing contributed.
struct Total {
    sum: f64,
    contributed: usize,
    attempted: usize,
}

impl Total {
    fn new() -> Self {
        Total {
            sum: 0.0,
            contributed: 0,
            attempted: 0,
        }
    }

    fn add(&mut self, value: Option<f64>) {
        self.attempted += 1;
        if let Some(v) = value {
            self.sum += v;
            self.contributed += 1;
        }
    }

    fn money(&self, currency: &str) -> Option<String> {
        (self.attempted == 0 || self.contributed > 0).then(|| money_in_currency(self.sum, currency))
    }
}

fn plan_spent_row(s: &SpendingPlanSummary, spent: Option<f64>, report: &str) -> SpendingPlanSpent {
    SpendingPlanSpent {
        plan_id: s.plan.id.clone(),
        spent: spent.map(|v| money_in_currency(v, report)),
    }
}

/// The plans of `year` with totals in `report_currency` (converted with `quotes`).
pub fn overview(
    conn: &Connection,
    year: i32,
    report_currency: &str,
    quotes: &fx::Quotes,
) -> Result<SpendingPlansOverview> {
    let report = fx::normalize(report_currency)
        .ok_or_else(|| AppError::Validation("validation.currencyInvalid".into()))?;
    let plans = plans_where(conn, "sp.year = ?1", &[&year])?;
    let payments = load_payments(conn, "sp.year = ?1", &[&year])?;
    let summaries = summaries(conn, plans, &payments)?;

    let mut quality = fx::Quality::default();
    let (mut planned, mut spent, mut remaining) = (Total::new(), Total::new(), Total::new());
    // (spent in the report currency for ordering, the row)
    let mut plan_spent: Vec<(Option<f64>, SpendingPlanSpent)> = Vec::new();
    for (s, plan_quality, _) in &summaries {
        let to_report = |amount: f64, quality: &mut fx::Quality| {
            let result = quotes.convert(amount, &s.plan.currency, &report);
            quality.track_conversion(quotes, &s.plan.currency, &report, result)
        };
        quality.merge(plan_quality);
        let amount = s.plan.amount.trim().parse::<f64>().ok();
        let planned_in_report = match amount {
            Some(a) => to_report(a, &mut quality),
            None => quality.track(&s.plan.currency, None),
        };
        // A plan that cannot be converted at all is left out of every total.
        let Some(planned_in_report) = planned_in_report else {
            planned.add(None);
            spent.add(None);
            if s.plan.closed_at.is_none() {
                remaining.add(None);
            }
            plan_spent.push((None, plan_spent_row(s, None, &report)));
            continue;
        };
        planned.add(Some(planned_in_report));
        let spent_native = s.spent.as_deref().and_then(|v| v.parse::<f64>().ok());
        let spent_in_report = spent_native.and_then(|v| to_report(v, &mut quality));
        spent.add(spent_in_report);
        plan_spent.push((spent_in_report, plan_spent_row(s, spent_in_report, &report)));
        if s.plan.closed_at.is_none() {
            let left = amount
                .zip(spent_native)
                .and_then(|(a, v)| to_report(a - v, &mut quality));
            remaining.add(left);
        }
    }

    // Largest spent first, unconvertible last; ties keep the plan order (stable sort).
    plan_spent.sort_by(|(a, _), (b, _)| match (a, b) {
        (Some(a), Some(b)) => b.total_cmp(a),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });

    let mut years_stmt = conn.prepare("SELECT DISTINCT year FROM spending_plans ORDER BY year")?;
    let years_with_plans = years_stmt
        .query_map([], |r| r.get::<_, i32>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(SpendingPlansOverview {
        year,
        totals: SpendingPlanTotals {
            planned: planned.money(&report),
            spent: spent.money(&report),
            remaining_active: remaining.money(&report),
        },
        plan_spent: plan_spent.into_iter().map(|(_, row)| row).collect(),
        currency: report,
        fx_quality: ConversionQuality::from(&quality),
        plans: summaries.into_iter().map(|(s, _, _)| s).collect(),
        years_with_plans,
    })
}

fn month_key(ts: i64) -> String {
    DateTime::<Utc>::from_timestamp(ts, 0)
        .map(|d| format!("{:04}-{:02}", d.year(), d.month()))
        .unwrap_or_else(|| "1970-01".into())
}

/// A plan's numbers: months, categories and the month x category cells, all converted into the
/// plan currency at each payment's booking day.
pub fn detail(conn: &Connection, id: &str) -> Result<SpendingPlanDetail> {
    let plan = get_plan(conn, id)?;
    let currency = plan.currency.clone();
    let payments = load_payments(conn, "sp.id = ?1", &[&id])?;
    let history = history_for(conn, &payments)?;
    let refs: Vec<&Payment> = payments.iter().collect();
    let (summary, _, converted) = summarize(plan, &refs, &history);

    // (sum, count) per month and per month x category; categories alone below.
    let mut months: BTreeMap<String, f64> = BTreeMap::new();
    let mut cells: BTreeMap<(String, Option<String>), (f64, i64)> = BTreeMap::new();
    for c in &converted {
        let month = month_key(c.payment.booking_date);
        *months.entry(month.clone()).or_default() += c.amount;
        let cell = cells
            .entry((month, c.payment.category_id.clone()))
            .or_default();
        cell.0 += c.amount;
        cell.1 += 1;
    }

    let accounts: HashSet<&str> = payments.iter().map(|p| p.account_id.as_str()).collect();
    Ok(SpendingPlanDetail {
        summary,
        months: months
            .into_iter()
            .map(|(month, sum)| SpendingPlanMonth {
                month,
                spent: money_in_currency(sum, &currency),
            })
            .collect(),
        categories: category_rows(sum_by_category(&converted), &currency),
        cells: cells
            .into_iter()
            .map(|((month, category_id), (sum, count))| SpendingPlanCell {
                month,
                category_id,
                spent: money_in_currency(sum, &currency),
                count,
            })
            .collect(),
        account_count: accounts.len() as i64,
    })
}

/// The payments tagged to a plan, newest first, pending included.
pub fn plan_transactions(conn: &Connection, id: &str) -> Result<Vec<BankTransaction>> {
    get_plan(conn, id)?;
    let mut stmt = conn.prepare(&format!(
        "SELECT {TRANSACTION_COLUMNS} FROM bank_transactions WHERE spending_plan_id = ?1
         ORDER BY booking_date DESC, created_at DESC, id DESC"
    ))?;
    let rows = stmt.query_map([id], transaction_from_row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

const DEFAULT_CANDIDATE_LIMIT: i64 = 500;
const MAX_CANDIDATE_LIMIT: i64 = 2000;

/// Payments from any account that could be tagged to a plan (its own are excluded), newest
/// first. `query` is matched in Rust, folded like the categorization engine (case and
/// diacritics: "kavarna" finds "Kavárna"), because SQLite folds case for ASCII only. A row also
/// matches it when its category is one of `category_ids`: the dialog lists the categories whose
/// translated name matches, as seeded ones are stored in English.
pub fn candidate_transactions(
    conn: &Connection,
    f: &PlanCandidateFilters,
) -> Result<PlanCandidatesResult> {
    let limit = f.limit.unwrap_or(DEFAULT_CANDIDATE_LIMIT);
    if !(1..=MAX_CANDIDATE_LIMIT).contains(&limit) {
        return Err(AppError::Validation("validation.paginationInvalid".into()));
    }
    get_plan(conn, &f.plan_id)?;
    let mut sql = format!(
        "SELECT {TRANSACTION_COLUMNS},
                (SELECT name FROM transaction_categories WHERE id = bank_transactions.category_id)
         FROM bank_transactions
         WHERE (spending_plan_id IS NULL OR spending_plan_id != ?)"
    );
    let mut args: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(f.plan_id.clone())];
    if f.only_unassigned {
        sql.push_str(" AND spending_plan_id IS NULL");
    }
    if let Some(from) = f.date_from {
        sql.push_str(" AND booking_date >= ?");
        args.push(Box::new(from));
    }
    if let Some(to) = f.date_to {
        sql.push_str(" AND booking_date <= ?");
        args.push(Box::new(to));
    }
    if let Some(account) = &f.account_id {
        sql.push_str(" AND bank_account_id = ?");
        args.push(Box::new(account.clone()));
    }
    sql.push_str(" ORDER BY booking_date DESC, created_at DESC, id DESC");

    let needle = f
        .query
        .as_deref()
        .map(str::trim)
        .filter(|q| !q.is_empty())
        .map(fold_text);
    let contains = |value: &Option<String>, needle: &str| {
        value
            .as_deref()
            .is_some_and(|v| fold_text(v).contains(needle))
    };
    let category_ids: HashSet<&str> = f
        .category_ids
        .iter()
        .flatten()
        .map(String::as_str)
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params_from_iter(args.iter().map(|a| a.as_ref())))?;
    let mut transactions = Vec::new();
    let mut truncated = false;
    while let Some(row) = rows.next()? {
        let tx = transaction_from_row(row)?;
        if let Some(needle) = &needle {
            let category_name: Option<String> = row.get(26)?;
            let listed_category = tx
                .category_id
                .as_deref()
                .is_some_and(|id| category_ids.contains(id));
            if !(listed_category
                || contains(&tx.counterparty_name, needle)
                || contains(&tx.description, needle)
                || contains(&category_name, needle))
            {
                continue;
            }
        }
        if transactions.len() as i64 == limit {
            truncated = true;
            break;
        }
        transactions.push(tx);
    }
    Ok(PlanCandidatesResult {
        transactions,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::params;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("fk");
        conn.execute_batch(
            r#"
            CREATE TABLE spending_plans (
                id TEXT PRIMARY KEY, name TEXT NOT NULL, year INTEGER NOT NULL,
                amount TEXT NOT NULL, currency TEXT NOT NULL, icon TEXT NOT NULL,
                color TEXT NOT NULL, note TEXT, closed_at INTEGER,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE bank_transactions (
                id TEXT PRIMARY KEY, bank_account_id TEXT NOT NULL DEFAULT 'a',
                transaction_id TEXT, tx_type TEXT NOT NULL DEFAULT 'debit',
                amount TEXT NOT NULL DEFAULT '10', currency TEXT NOT NULL DEFAULT 'EUR',
                description TEXT, counterparty_name TEXT, counterparty_iban TEXT,
                booking_date INTEGER NOT NULL DEFAULT 0, value_date INTEGER,
                category_id TEXT, merchant_category_code TEXT, remittance_info TEXT,
                suggested_category_id TEXT, status TEXT NOT NULL DEFAULT 'booked',
                data_source TEXT NOT NULL DEFAULT 'manual',
                created_at INTEGER NOT NULL DEFAULT 0, flow_kind TEXT,
                counterparty_bank_code TEXT, counterparty_country TEXT,
                import_batch_id TEXT, import_line INTEGER, row_balance TEXT,
                spending_plan_id TEXT REFERENCES spending_plans(id) ON DELETE SET NULL
            );
            CREATE TABLE csv_import_batches (id TEXT PRIMARY KEY, file_name TEXT);
            CREATE TABLE transaction_categories (id TEXT PRIMARY KEY, name TEXT NOT NULL);
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn plan(name: &str, year: i32) -> InsertSpendingPlan {
        InsertSpendingPlan {
            name: name.into(),
            year,
            amount: "1000".into(),
            currency: "eur".into(),
            icon: "plane".into(),
            color: "#3366cc".into(),
            note: None,
        }
    }

    fn add_tx(conn: &Connection, id: &str) {
        conn.execute("INSERT INTO bank_transactions (id) VALUES (?1)", [id])
            .unwrap();
    }

    fn tag(conn: &Connection, id: &str) -> Option<String> {
        conn.query_row(
            "SELECT spending_plan_id FROM bank_transactions WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .unwrap()
    }

    fn assign(tx: &str, plan: Option<&str>) -> SpendingPlanAssignment {
        SpendingPlanAssignment {
            transaction_id: tx.into(),
            plan_id: plan.map(String::from),
        }
    }

    fn validation_key(r: Result<impl std::fmt::Debug>) -> String {
        match r {
            Err(AppError::Validation(k)) => k,
            other => panic!("expected validation error, got {other:?}"),
        }
    }

    #[test]
    fn create_and_get_plan_roundtrip() {
        let conn = setup_test_db();
        let mut data = plan("  Dovolená ", 2026);
        data.note = Some("Řecko".into());
        let created = create_plan(&conn, &data).unwrap();
        assert_eq!(created.name, "Dovolená");
        assert_eq!(created.currency, "EUR");
        assert_eq!(created.closed_at, None);
        assert_eq!(get_plan(&conn, &created.id).unwrap(), created);
        assert_eq!(created.note.as_deref(), Some("Řecko"));
        assert!(matches!(
            get_plan(&conn, "nope"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn validate_rejects_each_bad_field() {
        let bad = |f: fn(&mut InsertSpendingPlan), key: &str| {
            let mut p = plan("Ok", 2026);
            f(&mut p);
            assert_eq!(validation_key(p.validate()), format!("validation.{key}"));
        };
        bad(|p| p.name = "   ".into(), "spendingPlanNameRequired");
        bad(|p| p.name = "x".repeat(101), "spendingPlanNameTooLong");
        bad(|p| p.year = 1999, "spendingPlanYearInvalid");
        bad(|p| p.year = 2101, "spendingPlanYearInvalid");
        bad(|p| p.amount = "abc".into(), "spendingPlanAmountInvalid");
        bad(|p| p.amount = "NaN".into(), "spendingPlanAmountInvalid");
        bad(|p| p.amount = "0".into(), "spendingPlanAmountInvalid");
        bad(|p| p.amount = "-5".into(), "spendingPlanAmountInvalid");
        bad(|p| p.currency = "EURO".into(), "currencyInvalid");
        bad(|p| p.icon = "".into(), "spendingPlanIconInvalid");
        bad(|p| p.icon = "Plane".into(), "spendingPlanIconInvalid");
        bad(|p| p.icon = "a".repeat(41), "spendingPlanIconInvalid");
        bad(|p| p.color = "3366cc".into(), "spendingPlanColorInvalid");
        bad(|p| p.color = "#33g6cc".into(), "spendingPlanColorInvalid");
        bad(
            |p| p.note = Some("n".repeat(501)),
            "spendingPlanNoteTooLong",
        );
        assert!(plan("Ok", 2026).validate().is_ok());
    }

    #[test]
    fn name_unique_per_year_unicode_case() {
        let conn = setup_test_db();
        create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        assert_eq!(
            validation_key(create_plan(&conn, &plan("DOVOLENÁ", 2026))),
            "validation.spendingPlanNameTaken"
        );
        create_plan(&conn, &plan("Dovolená", 2027)).unwrap();
        create_plan(&conn, &plan("Dovolena", 2026)).unwrap();
        assert_eq!(name_key("  ŠKOLA "), "škola");
    }

    #[test]
    fn update_keeps_own_name() {
        let conn = setup_test_db();
        let a = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        let b = create_plan(&conn, &plan("Kolo", 2026)).unwrap();
        let mut data = plan("DOVOLENÁ", 2026);
        data.amount = "2500".into();
        let updated = update_plan(&conn, &a.id, &data).unwrap();
        assert_eq!(updated.name, "DOVOLENÁ");
        assert_eq!(updated.amount, "2500");
        assert_eq!(
            validation_key(update_plan(&conn, &b.id, &plan("dovolená", 2026))),
            "validation.spendingPlanNameTaken"
        );
        assert!(matches!(
            update_plan(&conn, "nope", &plan("X", 2026)),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn create_plans_is_all_or_nothing() {
        let conn = setup_test_db();
        let r = create_plans(&conn, &[plan("Dovolená", 2026), plan("dovolená", 2026)]);
        assert_eq!(validation_key(r), "validation.spendingPlanNameTaken");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM spending_plans", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let ok = create_plans(&conn, &[plan("A", 2026), plan("B", 2026)]).unwrap();
        assert_eq!(ok.len(), 2);
    }

    #[test]
    fn close_and_reopen() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        assert!(set_plan_closed(&conn, &p.id, true)
            .unwrap()
            .closed_at
            .is_some());
        assert_eq!(
            set_plan_closed(&conn, &p.id, false).unwrap().closed_at,
            None
        );
        assert!(matches!(
            set_plan_closed(&conn, "nope", true),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn delete_clears_tags() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        add_tx(&conn, "t1");
        add_tx(&conn, "t2");
        set_transaction_plans(&conn, &[assign("t1", Some(&p.id))], false).unwrap();
        delete_plan(&conn, &p.id).unwrap();
        assert!(matches!(get_plan(&conn, &p.id), Err(AppError::NotFound(_))));
        assert_eq!(tag(&conn, "t1"), None);
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM bank_transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(remaining, 2);
        assert!(matches!(
            delete_plan(&conn, &p.id),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn delete_plan_leaves_payments_in_place() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        add_tx(&conn, "t1");
        set_transaction_plans(&conn, &[assign("t1", Some(&p.id))], false).unwrap();
        // Even a bare DELETE (FK action only) keeps the payment and nulls the tag.
        conn.execute("DELETE FROM spending_plans WHERE id = ?1", [&p.id])
            .unwrap();
        let (count, tagged): (i64, Option<String>) = conn
            .query_row(
                "SELECT COUNT(*), MAX(spending_plan_id) FROM bank_transactions WHERE id = 't1'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((count, tagged), (1, None));
    }

    #[test]
    fn assign_move_remove() {
        let conn = setup_test_db();
        let a = create_plan(&conn, &plan("A", 2026)).unwrap();
        let b = create_plan(&conn, &plan("B", 2026)).unwrap();
        add_tx(&conn, "t1");
        add_tx(&conn, "t2");
        set_transaction_plans(
            &conn,
            &[assign("t1", Some(&a.id)), assign("t2", Some(&a.id))],
            false,
        )
        .unwrap();
        assert_eq!(tag(&conn, "t2").as_deref(), Some(a.id.as_str()));
        set_transaction_plans(&conn, &[assign("t1", Some(&b.id))], false).unwrap();
        assert_eq!(tag(&conn, "t1").as_deref(), Some(b.id.as_str()));
        set_transaction_plans(&conn, &[assign("t1", None), assign("t2", None)], false).unwrap();
        assert_eq!((tag(&conn, "t1"), tag(&conn, "t2")), (None, None));
    }

    #[test]
    fn assign_to_closed_rejected_unless_restore() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        add_tx(&conn, "t1");
        set_plan_closed(&conn, &p.id, true).unwrap();
        assert_eq!(
            validation_key(set_transaction_plans(
                &conn,
                &[assign("t1", Some(&p.id))],
                false
            )),
            "validation.spendingPlanClosed"
        );
        assert_eq!(tag(&conn, "t1"), None);
        set_transaction_plans(&conn, &[assign("t1", Some(&p.id))], true).unwrap();
        assert_eq!(tag(&conn, "t1").as_deref(), Some(p.id.as_str()));
        // Untagging from a closed plan is always allowed.
        set_transaction_plans(&conn, &[assign("t1", None)], false).unwrap();
    }

    #[test]
    fn assign_unknown_ids_not_found() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("A", 2026)).unwrap();
        add_tx(&conn, "t1");
        assert!(matches!(
            set_transaction_plans(&conn, &[assign("ghost", Some(&p.id))], false),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            set_transaction_plans(&conn, &[assign("t1", Some("ghost"))], false),
            Err(AppError::NotFound(_))
        ));
        // All-or-nothing: a good first assignment is rolled back by a bad second one.
        let r = set_transaction_plans(
            &conn,
            &[assign("t1", Some(&p.id)), assign("ghost", None)],
            false,
        );
        assert!(matches!(r, Err(AppError::NotFound(_))));
        assert_eq!(tag(&conn, "t1"), None);
    }

    // ---- spend, overview, detail, candidates ----

    use chrono::NaiveDate;
    fn ts(y: i32, m: u32, d: u32) -> i64 {
        NaiveDate::from_ymd_opt(y, m, d)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp()
    }

    fn plan_in(name: &str, year: i32, amount: &str, currency: &str) -> InsertSpendingPlan {
        InsertSpendingPlan {
            amount: amount.into(),
            currency: currency.into(),
            ..plan(name, year)
        }
    }

    struct Pay<'a> {
        id: &'a str,
        plan: Option<&'a str>,
        tx_type: &'a str,
        amount: &'a str,
        currency: &'a str,
        date: i64,
        status: &'a str,
        category: Option<&'a str>,
        account: &'a str,
    }

    fn pay<'a>(id: &'a str, plan: Option<&'a str>, amount: &'a str, date: i64) -> Pay<'a> {
        Pay {
            id,
            plan,
            tx_type: "debit",
            amount,
            currency: "EUR",
            date,
            status: "booked",
            category: None,
            account: "a",
        }
    }

    fn put(conn: &Connection, p: Pay) {
        conn.execute(
            "INSERT INTO bank_transactions (id, spending_plan_id, tx_type, amount, currency,
                 booking_date, status, category_id, bank_account_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                p.id, p.plan, p.tx_type, p.amount, p.currency, p.date, p.status, p.category,
                p.account
            ],
        )
        .unwrap();
    }

    fn rate(conn: &Connection, day: i64, currency: &str, per_eur: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            params![day, currency, per_eur],
        )
        .unwrap();
    }

    fn summary(conn: &Connection, id: &str) -> SpendingPlanSummary {
        plan_index(conn)
            .unwrap()
            .into_iter()
            .find(|s| s.plan.id == id)
            .unwrap()
    }

    #[test]
    fn spent_nets_credits() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan_in("Byt", 2026, "50000", "CZK")).unwrap();
        let d = ts(2026, 3, 1);
        let czk = |id, amount, tx_type| Pay {
            currency: "CZK",
            tx_type,
            ..pay(id, Some(&p.id), amount, d)
        };
        put(&conn, czk("t1", "6000", "debit"));
        put(&conn, czk("t2", "12000", "debit"));
        put(&conn, czk("t3", "3500", "credit"));
        let s = summary(&conn, &p.id);
        assert_eq!(s.spent.as_deref(), Some("14500.00"));
        assert_eq!(s.payment_count, 3);
        assert!(s.fx_quality.missing_currencies.is_empty());
    }

    #[test]
    fn no_payments_is_zero_spent() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan_in("Prázdný", 2026, "100", "CZK")).unwrap();
        let s = summary(&conn, &p.id);
        assert_eq!(s.spent.as_deref(), Some("0.00"));
        assert_eq!((s.payment_count, s.last_payment_date), (0, None));
    }

    #[test]
    fn pending_not_counted_but_listed() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        put(&conn, pay("t1", Some(&p.id), "40", ts(2026, 5, 1)));
        put(
            &conn,
            Pay {
                status: "pending",
                ..pay("t2", Some(&p.id), "25", ts(2026, 5, 9))
            },
        );
        let s = summary(&conn, &p.id);
        assert_eq!(s.payment_count, 2);
        assert_eq!(s.last_payment_date, Some(ts(2026, 5, 9)));
        assert_eq!(s.spent.as_deref(), Some("40.00"));
        let listed = plan_transactions(&conn, &p.id).unwrap();
        let ids: Vec<&str> = listed.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["t2", "t1"]);
        assert!(matches!(
            plan_transactions(&conn, "nope"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn only_pending_payments_spent_is_zero() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        put(
            &conn,
            Pay {
                status: "pending",
                ..pay("t1", Some(&p.id), "25", ts(2026, 5, 9))
            },
        );
        let s = summary(&conn, &p.id);
        assert_eq!((s.spent.as_deref(), s.payment_count), (Some("0.00"), 1));
    }

    #[test]
    fn foreign_payment_converted_at_its_day() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        let day = ts(2026, 6, 10);
        rate(&conn, day, "CZK", 25.0);
        rate(&conn, day + 86_400, "CZK", 20.0);
        put(
            &conn,
            Pay {
                currency: "CZK",
                ..pay("t1", Some(&p.id), "2500", day + 3_600)
            },
        );
        put(&conn, pay("t2", Some(&p.id), "10", day));
        assert_eq!(summary(&conn, &p.id).spent.as_deref(), Some("110.00"));
    }

    #[test]
    fn missing_rate_excluded_and_reported() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        let day = ts(2026, 6, 10);
        put(
            &conn,
            Pay {
                currency: "XYZ",
                ..pay("t1", Some(&p.id), "999", day)
            },
        );
        let only = summary(&conn, &p.id);
        assert_eq!(only.spent, None);
        assert_eq!(only.payment_count, 1);
        assert_eq!(only.fx_quality.missing_currencies, ["XYZ"]);
        put(&conn, pay("t2", Some(&p.id), "10", day));
        let mixed = summary(&conn, &p.id);
        assert_eq!(mixed.spent.as_deref(), Some("10.00"));
        assert_eq!(mixed.fx_quality.missing_currencies, ["XYZ"]);
        assert_eq!(mixed.fx_quality.missing_count, 1);
    }

    #[test]
    fn months_bucket_including_next_year() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        put(
            &conn,
            pay("t1", Some(&p.id), "10", ts(2026, 12, 31) + 86_399),
        );
        put(&conn, pay("t2", Some(&p.id), "5", ts(2027, 2, 10)));
        put(&conn, pay("t3", Some(&p.id), "7", ts(2026, 3, 2)));
        put(&conn, pay("t4", Some(&p.id), "3", ts(2026, 3, 20)));
        let d = detail(&conn, &p.id).unwrap();
        let months: Vec<(&str, &str)> = d
            .months
            .iter()
            .map(|m| (m.month.as_str(), m.spent.as_str()))
            .collect();
        assert_eq!(
            months,
            [
                ("2026-03", "10.00"),
                ("2026-12", "10.00"),
                ("2027-02", "5.00")
            ]
        );
        assert_eq!(d.summary.spent.as_deref(), Some("25.00"));
        assert_eq!(d.account_count, 1);
    }

    #[test]
    fn categories_group_uncategorized() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        let d = ts(2026, 4, 1);
        let c = |id, amount, cat| Pay {
            category: cat,
            ..pay(id, Some(&p.id), amount, d)
        };
        put(&conn, c("t1", "10", Some("food")));
        put(&conn, c("t2", "30", Some("food")));
        put(&conn, c("t3", "15", None));
        put(&conn, c("t4", "5", None));
        put(&conn, c("t5", "100", Some("fun")));
        put(
            &conn,
            Pay {
                status: "pending",
                ..c("t6", "999", Some("fun"))
            },
        );
        let d = detail(&conn, &p.id).unwrap();
        let cats: Vec<(Option<&str>, &str, i64)> = d
            .categories
            .iter()
            .map(|c| (c.category_id.as_deref(), c.spent.as_str(), c.count))
            .collect();
        assert_eq!(
            cats,
            [
                (Some("fun"), "100.00", 1),
                (Some("food"), "40.00", 2),
                (None, "20.00", 2)
            ]
        );
    }

    #[test]
    fn cells_sum_to_months_and_categories() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        let pays = [
            ("t1", "10", 3, Some("food")),
            ("t2", "20", 3, None),
            ("t3", "30", 4, Some("food")),
            ("t4", "40", 4, Some("fun")),
        ];
        for (id, amount, month, cat) in pays {
            put(
                &conn,
                Pay {
                    category: cat,
                    ..pay(id, Some(&p.id), amount, ts(2026, month, 5))
                },
            );
        }
        let d = detail(&conn, &p.id).unwrap();
        let sum = |f: &dyn Fn(&SpendingPlanCell) -> bool| -> f64 {
            d.cells
                .iter()
                .filter(|c| f(c))
                .map(|c| c.spent.parse::<f64>().unwrap())
                .sum()
        };
        for m in &d.months {
            let total = sum(&|c| c.month == m.month);
            assert_eq!(total, m.spent.parse::<f64>().unwrap());
        }
        for c in &d.categories {
            let total = sum(&|x| x.category_id == c.category_id);
            assert_eq!(total, c.spent.parse::<f64>().unwrap());
            let count: i64 = d
                .cells
                .iter()
                .filter(|x| x.category_id == c.category_id)
                .map(|x| x.count)
                .sum();
            assert_eq!(count, c.count);
        }
        assert_eq!(d.cells.len(), 4);
    }

    #[test]
    fn detail_counts_accounts() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        put(&conn, pay("t1", Some(&p.id), "1", ts(2026, 1, 1)));
        put(
            &conn,
            Pay {
                account: "b",
                ..pay("t2", Some(&p.id), "1", ts(2026, 1, 1))
            },
        );
        assert_eq!(detail(&conn, &p.id).unwrap().account_count, 2);
        assert!(matches!(detail(&conn, "nope"), Err(AppError::NotFound(_))));
    }

    #[test]
    fn plan_index_lists_all_years() {
        let conn = setup_test_db();
        let a = create_plan(&conn, &plan("A", 2025)).unwrap();
        let b = create_plan(&conn, &plan("B", 2026)).unwrap();
        put(&conn, pay("t1", Some(&a.id), "5", ts(2025, 1, 1)));
        let index = plan_index(&conn).unwrap();
        assert_eq!(index.len(), 2);
        let find = |id: &str| index.iter().find(|s| s.plan.id == id).unwrap();
        assert_eq!(find(&a.id).spent.as_deref(), Some("5.00"));
        assert_eq!(find(&b.id).spent.as_deref(), Some("0.00"));
    }

    fn czk_quotes() -> fx::Quotes {
        fx::Quotes::from_stored([("CZK", 25.0)])
    }

    #[test]
    fn overview_totals_in_report_currency() {
        let conn = setup_test_db();
        let czk = create_plan(&conn, &plan_in("Byt", 2026, "10000", "CZK")).unwrap();
        let eur = create_plan(&conn, &plan_in("Dovolená", 2026, "100", "EUR")).unwrap();
        let closed = create_plan(&conn, &plan_in("Kolo", 2026, "1000", "CZK")).unwrap();
        create_plan(&conn, &plan_in("Jiný rok", 2025, "777", "CZK")).unwrap();
        set_plan_closed(&conn, &closed.id, true).unwrap();
        let d = ts(2026, 2, 2);
        rate(&conn, d, "CZK", 25.0);
        put(
            &conn,
            Pay {
                currency: "CZK",
                ..pay("t1", Some(&czk.id), "2000", d)
            },
        );
        put(&conn, pay("t2", Some(&eur.id), "40", d));
        put(
            &conn,
            Pay {
                currency: "CZK",
                ..pay("t3", Some(&closed.id), "400", d)
            },
        );
        let o = overview(&conn, 2026, "czk", &czk_quotes()).unwrap();
        assert_eq!((o.year, o.currency.as_str()), (2026, "CZK"));
        assert_eq!(o.plans.len(), 3);
        // planned 10000 + 100 EUR * 25 + 1000; spent 2000 + 40 * 25 + 400; left (active) 8000 + 1500
        assert_eq!(o.totals.planned.as_deref(), Some("13500.00"));
        assert_eq!(o.totals.spent.as_deref(), Some("3400.00"));
        assert_eq!(o.totals.remaining_active.as_deref(), Some("9500.00"));
        assert!(o.fx_quality.missing_currencies.is_empty());
        // Per-plan values stay in the plan currency.
        let eur_row = o.plans.iter().find(|s| s.plan.id == eur.id).unwrap();
        assert_eq!(eur_row.spent.as_deref(), Some("40.00"));
    }

    #[test]
    fn overview_leaves_unconvertible_plans_out() {
        let conn = setup_test_db();
        create_plan(&conn, &plan_in("Byt", 2026, "1000", "CZK")).unwrap();
        create_plan(&conn, &plan_in("Exotika", 2026, "500", "XYZ")).unwrap();
        let o = overview(&conn, 2026, "CZK", &czk_quotes()).unwrap();
        assert_eq!(o.totals.planned.as_deref(), Some("1000.00"));
        assert_eq!(o.totals.spent.as_deref(), Some("0.00"));
        assert_eq!(o.totals.remaining_active.as_deref(), Some("1000.00"));
        assert_eq!(o.fx_quality.missing_currencies, ["XYZ"]);
    }

    #[test]
    fn overview_only_unconvertible_plan_has_no_totals() {
        let conn = setup_test_db();
        create_plan(&conn, &plan_in("Exotika", 2026, "500", "XYZ")).unwrap();
        let o = overview(&conn, 2026, "CZK", &czk_quotes()).unwrap();
        assert_eq!(o.totals.planned, None);
        assert_eq!(o.totals.spent, None);
        assert_eq!(o.totals.remaining_active, None);
        assert_eq!(o.fx_quality.missing_currencies, ["XYZ"]);
    }

    #[test]
    fn overview_plan_without_convertible_spend_skips_spent_and_remaining() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan_in("Byt", 2026, "1000", "CZK")).unwrap();
        rate(&conn, ts(2026, 2, 2), "CZK", 25.0);
        put(
            &conn,
            Pay {
                currency: "XYZ",
                ..pay("t1", Some(&p.id), "5", ts(2026, 2, 2))
            },
        );
        let o = overview(&conn, 2026, "CZK", &czk_quotes()).unwrap();
        assert_eq!(o.totals.planned.as_deref(), Some("1000.00"));
        assert_eq!(o.totals.spent, None);
        assert_eq!(o.totals.remaining_active, None);
        assert_eq!(o.fx_quality.missing_currencies, ["XYZ"]);
    }

    fn overview_plan_spent(o: &SpendingPlansOverview) -> Vec<(&str, Option<&str>)> {
        o.plan_spent
            .iter()
            .map(|p| (p.plan_id.as_str(), p.spent.as_deref()))
            .collect()
    }

    #[test]
    fn overview_plan_spent_in_report_currency_largest_first() {
        let conn = setup_test_db();
        let czk = create_plan(&conn, &plan_in("Byt", 2026, "10000", "CZK")).unwrap();
        let eur = create_plan(&conn, &plan_in("Dovolená", 2026, "200", "EUR")).unwrap();
        let idle = create_plan(&conn, &plan_in("Kolo", 2026, "1000", "CZK")).unwrap();
        let d = ts(2026, 2, 2);
        rate(&conn, d, "CZK", 25.0);
        put(
            &conn,
            Pay {
                currency: "CZK",
                ..pay("t1", Some(&czk.id), "2000", d)
            },
        );
        put(&conn, pay("t2", Some(&eur.id), "100", d));
        put(
            &conn,
            Pay {
                status: "pending",
                ..pay("t3", Some(&eur.id), "999", d)
            },
        );
        let o = overview(&conn, 2026, "CZK", &czk_quotes()).unwrap();
        // 100 EUR * 25 = 2500 CZK ahead of 2000 CZK; the plan without payments last; pending out
        assert_eq!(
            overview_plan_spent(&o),
            [
                (eur.id.as_str(), Some("2500.00")),
                (czk.id.as_str(), Some("2000.00")),
                (idle.id.as_str(), Some("0.00")),
            ]
        );
        assert!(o.fx_quality.missing_currencies.is_empty());
    }

    #[test]
    fn overview_plan_spent_none_without_a_rate() {
        let conn = setup_test_db();
        let xyz = create_plan(&conn, &plan_in("Exotika", 2026, "500", "XYZ")).unwrap();
        let czk = create_plan(&conn, &plan_in("Byt", 2026, "10000", "CZK")).unwrap();
        let foreign = create_plan(&conn, &plan_in("Zahraničí", 2026, "100", "CZK")).unwrap();
        let d = ts(2026, 2, 2);
        rate(&conn, d, "CZK", 25.0);
        put(
            &conn,
            Pay {
                currency: "CZK",
                ..pay("t1", Some(&czk.id), "1000", d)
            },
        );
        // No current quote for XYZ: the plan cannot be converted into the report currency.
        put(
            &conn,
            Pay {
                currency: "XYZ",
                ..pay("t2", Some(&xyz.id), "7", d)
            },
        );
        // No rate for XYZ on its day: the CZK plan's only payment cannot be converted.
        put(
            &conn,
            Pay {
                currency: "XYZ",
                ..pay("t3", Some(&foreign.id), "5", d)
            },
        );
        let o = overview(&conn, 2026, "CZK", &czk_quotes()).unwrap();
        assert_eq!(
            overview_plan_spent(&o),
            [
                (czk.id.as_str(), Some("1000.00")),
                (xyz.id.as_str(), None),
                (foreign.id.as_str(), None),
            ]
        );
        assert_eq!(o.fx_quality.missing_currencies, ["XYZ"]);
    }

    #[test]
    fn overview_empty_year_and_bad_currency() {
        let conn = setup_test_db();
        let o = overview(&conn, 2026, "CZK", &czk_quotes()).unwrap();
        assert!(o.plans.is_empty());
        assert_eq!(o.totals.planned.as_deref(), Some("0.00"));
        assert_eq!(
            validation_key(overview(&conn, 2026, "CZKK", &czk_quotes())),
            "validation.currencyInvalid"
        );
    }

    #[test]
    fn overview_years_with_plans() {
        let conn = setup_test_db();
        create_plan(&conn, &plan("A", 2027)).unwrap();
        create_plan(&conn, &plan("B", 2025)).unwrap();
        create_plan(&conn, &plan("C", 2025)).unwrap();
        let o = overview(&conn, 2026, "EUR", &fx::Quotes::from_stored([])).unwrap();
        assert_eq!(o.years_with_plans, [2025, 2027]);
        assert!(o.plans.is_empty());
    }

    fn candidate_filters(plan_id: &str) -> PlanCandidateFilters {
        PlanCandidateFilters {
            plan_id: plan_id.into(),
            date_from: None,
            date_to: None,
            account_id: None,
            only_unassigned: false,
            query: None,
            category_ids: None,
            limit: None,
        }
    }

    fn candidate_ids(conn: &Connection, f: &PlanCandidateFilters) -> Vec<String> {
        candidate_transactions(conn, f)
            .unwrap()
            .transactions
            .into_iter()
            .map(|t| t.id)
            .collect()
    }

    #[test]
    fn candidates_filters() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        let other = create_plan(&conn, &plan("Jiný", 2026)).unwrap();
        conn.execute(
            "INSERT INTO transaction_categories (id, name) VALUES ('c1', 'Řecké taverny')",
            [],
        )
        .unwrap();
        put(&conn, pay("own", Some(&p.id), "1", ts(2026, 5, 1)));
        put(&conn, pay("free", None, "1", ts(2026, 5, 2)));
        put(&conn, pay("taken", Some(&other.id), "1", ts(2026, 5, 3)));
        put(
            &conn,
            Pay {
                account: "b",
                category: Some("c1"),
                status: "pending",
                ..pay("acc_b", None, "1", ts(2026, 6, 1))
            },
        );
        conn.execute(
            "UPDATE bank_transactions SET counterparty_name = 'ŠKODA Auto', description = 'servis'
             WHERE id = 'free'",
            [],
        )
        .unwrap();

        // Excludes the plan's own rows, newest first, keeps other plans' rows and pending.
        let f = candidate_filters(&p.id);
        assert_eq!(candidate_ids(&conn, &f), ["acc_b", "taken", "free"]);

        let only = PlanCandidateFilters {
            only_unassigned: true,
            ..f.clone()
        };
        assert_eq!(candidate_ids(&conn, &only), ["acc_b", "free"]);

        let acct = PlanCandidateFilters {
            account_id: Some("b".into()),
            ..f.clone()
        };
        assert_eq!(candidate_ids(&conn, &acct), ["acc_b"]);

        let period = PlanCandidateFilters {
            date_from: Some(ts(2026, 5, 2)),
            date_to: Some(ts(2026, 5, 3)),
            ..f.clone()
        };
        assert_eq!(candidate_ids(&conn, &period), ["taken", "free"]);

        // Case-insensitive over Unicode: counterparty, description, category name.
        for (q, expected) in [
            ("škoda", vec!["free"]),
            ("SERVIS", vec!["free"]),
            ("řecké", vec!["acc_b"]),
        ] {
            let by_query = PlanCandidateFilters {
                query: Some(q.into()),
                ..f.clone()
            };
            assert_eq!(candidate_ids(&conn, &by_query), expected, "query {q}");
        }
    }

    #[test]
    fn candidates_query_ignores_diacritics() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        put(&conn, pay("cafe", None, "1", ts(2026, 5, 1)));
        put(&conn, pay("other", None, "1", ts(2026, 5, 2)));
        conn.execute(
            "UPDATE bank_transactions SET counterparty_name = 'Kavárna Slavia' WHERE id = 'cafe'",
            [],
        )
        .unwrap();
        for q in ["kavarna", "KAVARNA", "kavárna"] {
            let f = PlanCandidateFilters {
                query: Some(q.into()),
                ..candidate_filters(&p.id)
            };
            assert_eq!(candidate_ids(&conn, &f), ["cafe"], "query {q}");
        }
    }

    #[test]
    fn candidates_query_matches_listed_categories() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        // Seeded categories are stored in English and shown translated ("Potraviny")
        conn.execute(
            "INSERT INTO transaction_categories (id, name) VALUES ('groceries', 'Groceries')",
            [],
        )
        .unwrap();
        put(
            &conn,
            Pay {
                category: Some("groceries"),
                ..pay("food", None, "1", ts(2026, 5, 1))
            },
        );
        put(&conn, pay("other", None, "1", ts(2026, 5, 2)));
        let f = PlanCandidateFilters {
            query: Some("potraviny".into()),
            ..candidate_filters(&p.id)
        };
        assert!(candidate_ids(&conn, &f).is_empty());
        let by_category = PlanCandidateFilters {
            category_ids: Some(vec!["groceries".into()]),
            ..f.clone()
        };
        assert_eq!(candidate_ids(&conn, &by_category), ["food"]);
    }

    #[test]
    fn candidates_truncate_at_limit() {
        let conn = setup_test_db();
        let p = create_plan(&conn, &plan("Dovolená", 2026)).unwrap();
        for i in 0..5 {
            put(&conn, pay(&format!("t{i}"), None, "1", ts(2026, 5, 1 + i)));
        }
        let f = PlanCandidateFilters {
            limit: Some(3),
            ..candidate_filters(&p.id)
        };
        let r = candidate_transactions(&conn, &f).unwrap();
        assert!(r.truncated);
        let ids: Vec<&str> = r.transactions.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, ["t4", "t3", "t2"]);
        let exact = PlanCandidateFilters {
            limit: Some(5),
            ..f.clone()
        };
        assert!(!candidate_transactions(&conn, &exact).unwrap().truncated);
        for bad in [0, 2001, -1] {
            let f = PlanCandidateFilters {
                limit: Some(bad),
                ..f.clone()
            };
            assert_eq!(
                validation_key(candidate_transactions(&conn, &f)),
                "validation.paginationInvalid"
            );
        }
        assert!(matches!(
            candidate_transactions(&conn, &candidate_filters("nope")),
            Err(AppError::NotFound(_))
        ));
    }
}
