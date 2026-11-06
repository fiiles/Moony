//! Splits of stock positions, spec `docs/specs/2026-10-24-stock-splits-design.md`.
//!
//! A `stock_splits` row records one split of a ticker (D4). Detection stores the relevant ones
//! as `pending` (D5, D6); applying a split rewrites the transactions dated before its ex-date
//! into today's share units together with the ticker's per-share side data (D1, D3, D10), and
//! reverting does the same with the ratio turned round. Each rewritten row lists the applied
//! splits its values include in `split_basis` (sync spec D27), and one rewrite,
//! [`reconcile_ticker`], makes the rows match the split statuses for apply, revert and the
//! sync repair alike. Every function takes `&Connection`; the commands call them inside
//! `db.with_conn`.

pub mod classify;
pub mod scale;
pub mod types;

pub use types::{
    ratio_parts, DetectedSplit, InsertStockSplit, StockSplit, StockSplitApplyResult,
    StockSplitSource, StockSplitStatus, StockSplitVerdict,
};

use crate::services::sync::natural_id::natural_id;
use rusqlite::{params, Connection, OptionalExtension, Params};
use std::collections::{BTreeMap, BTreeSet};

use crate::error::{AppError, Result};
use crate::services::investments::{ensure_valid_holdings_history, recalculate_investment_metrics};
use crate::services::stock_import::types::StockSplitOverride;
use classify::{classify_cumulative, verdict_of};
use scale::{scale_amount, Rounding};
use types::ratio_is_valid;

const DAY_SECONDS: i64 = 86_400;

const COLUMNS: &str = "id, ticker, effective_day, numerator, denominator, source, status, \
                       detected_at, resolved_at, applied_count";

/// Per-share side data of a ticker, scaled with its transactions: `(table, column)`.
const SIDE_DATA: [(&str, &str); 3] = [
    ("stock_price_overrides", "price"),
    ("watched_stocks", "target_price"),
    ("dividend_overrides", "yearly_dividend_sum"),
];

/// The ticker's splits that still have transactions dated before them (D6), newest first,
/// with the D7 verdict for the pending ones.
pub fn list_for_ticker(conn: &Connection, ticker: &str) -> Result<Vec<StockSplit>> {
    Ok(query(conn, "ticker = ?1", [ticker])?
        .into_iter()
        .filter(|split| split.pre_split_count > 0)
        .collect())
}

/// Every pending split that still has transactions dated before it (D6), across tickers,
/// newest first, with its verdict. The milestone builder reads these.
pub fn list_pending(conn: &Connection) -> Result<Vec<StockSplit>> {
    Ok(query(conn, "status = 'pending'", [])?
        .into_iter()
        .filter(|split| split.pre_split_count > 0)
        .collect())
}

/// One split by id. Errors: `AppError::NotFound`.
pub fn get(conn: &Connection, id: &str) -> Result<StockSplit> {
    query(conn, "id = ?1", [id])?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::NotFound("Stock split not found".into()))
}

/// The split on record for `(ticker, effective_day)`, if any.
pub fn find(conn: &Connection, ticker: &str, effective_day: i64) -> Result<Option<StockSplit>> {
    Ok(query(
        conn,
        "ticker = ?1 AND effective_day = ?2",
        params![ticker, effective_day],
    )?
    .into_iter()
    .next())
}

/// Records a split the stock import's file spans (D14), source `import`, for a `(ticker, day)`
/// with no row yet (the caller checked with [`find`]). `status` is `dismissed` for a split
/// the file is already adjusted for, or `pending` for one the import goes on to [`apply`]
/// (which rewrites the stored rows dated before it and sets it `applied`); `applied` itself
/// is reached only through [`apply`]. Errors: `validation.stockSplitRatioInvalid`,
/// `validation.stockSplitNotPending` for `applied`.
pub fn insert_from_import(
    conn: &Connection,
    ticker: &str,
    split: &StockSplitOverride,
    status: StockSplitStatus,
) -> Result<StockSplit> {
    if status == StockSplitStatus::Applied {
        return Err(not_pending());
    }
    let (n, d) = ratio_parts(&split.numerator, &split.denominator)?;
    let id = natural_id("stock_splits", &[ticker, &split.effective_day.to_string()]);
    let detected_at = now();
    let resolved_at = (status == StockSplitStatus::Dismissed).then_some(detected_at);
    conn.execute(
        "INSERT INTO stock_splits
             (id, ticker, effective_day, numerator, denominator, source, status, detected_at,
              resolved_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'import', ?6, ?7, ?8)",
        params![
            id,
            ticker,
            split.effective_day,
            n.to_string(),
            d.to_string(),
            status.as_str(),
            detected_at,
            resolved_at,
        ],
    )?;
    get(conn, &id)
}

/// Stores the detected splits that are new and relevant (D5, D6) as `pending` and returns
/// only the rows it inserted; the caller queues a full history rebuild for them (D18). A
/// split already on record keeps its status. A ratio Moony would not accept by hand is
/// skipped.
pub fn record_detected(
    conn: &Connection,
    ticker: &str,
    splits: &[DetectedSplit],
    source: StockSplitSource,
) -> Result<Vec<StockSplit>> {
    let mut inserted = Vec::new();
    for split in splits {
        if !ratio_is_valid(split.numerator, split.denominator) {
            continue;
        }
        let day = split.effective_day.div_euclid(DAY_SECONDS) * DAY_SECONDS;
        if pre_split_count(conn, ticker, day)? == 0 {
            continue;
        }
        let id = natural_id("stock_splits", &[ticker, &day.to_string()]);
        let changed = conn.execute(
            "INSERT INTO stock_splits
                 (id, ticker, effective_day, numerator, denominator, source, status, detected_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'pending', ?7)
             ON CONFLICT(ticker, effective_day) DO NOTHING",
            params![
                id,
                ticker,
                day,
                split.numerator.to_string(),
                split.denominator.to_string(),
                source.as_str(),
                now(),
            ],
        )?;
        if changed == 1 {
            inserted.push(get(conn, &id)?);
        }
    }
    Ok(inserted)
}

/// Applies a pending or dismissed split (D10; a dismissed one means the user changed their
/// mind): marks it `applied` and runs [`reconcile_ticker`], which scales the pre-split
/// transactions and side data into today's units, records the split in their `split_basis`
/// and recomputes the positions. An applied split is refused (`validation.stockSplitNotPending`);
/// a history that would sell more than it holds aborts everything with
/// `validation.stockSplitOversold`.
pub fn apply(conn: &Connection, id: &str) -> Result<StockSplitApplyResult> {
    in_savepoint(conn, || apply_in(conn, id))
}

/// Undoes an applied split (D10 with the ratio turned round): sets it back to `pending` and runs
/// [`reconcile_ticker`], which scales back exactly the rows whose `split_basis` carries it.
/// Errors: `validation.stockSplitNotApplied`, `validation.stockSplitOversold`.
pub fn revert(conn: &Connection, id: &str) -> Result<StockSplitApplyResult> {
    in_savepoint(conn, || {
        let split = get(conn, id)?;
        if split.status != StockSplitStatus::Applied {
            return Err(AppError::Validation(
                "validation.stockSplitNotApplied".into(),
            ));
        }
        conn.execute(
            "UPDATE stock_splits SET status = 'pending', resolved_at = NULL, applied_count = NULL
             WHERE id = ?1",
            [id],
        )?;
        let rewritten = reconcile_ticker(conn, &split.ticker)?.rewritten(id);
        apply_result(conn, id, &split.ticker, rewritten)
    })
}

/// Marks a pending split as already reflected in the data; nothing else changes.
/// Errors: `validation.stockSplitNotPending`.
pub fn dismiss(conn: &Connection, id: &str) -> Result<StockSplit> {
    let split = get(conn, id)?;
    if split.status != StockSplitStatus::Pending {
        return Err(not_pending());
    }
    conn.execute(
        "UPDATE stock_splits SET status = 'dismissed', resolved_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    get(conn, id)
}

/// Records a split by hand and applies it at once (D11). A row already on record for the
/// same `(ticker, day)` is reused: a pending or dismissed one takes the entered ratio and is
/// applied, keeping its source; an applied one is refused (`validation.stockSplitNotPending`).
pub fn record_manual(conn: &Connection, data: &InsertStockSplit) -> Result<StockSplitApplyResult> {
    data.validate()?;
    let (n, d) = ratio_parts(&data.numerator, &data.denominator)?;
    in_savepoint(conn, || {
        let existing: Option<(String, String)> = conn
            .query_row(
                "SELECT id, status FROM stock_splits WHERE ticker = ?1 AND effective_day = ?2",
                params![data.ticker, data.effective_day],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let id = match existing {
            Some((id, status)) => {
                if StockSplitStatus::parse(&status)? == StockSplitStatus::Applied {
                    return Err(not_pending());
                }
                conn.execute(
                    "UPDATE stock_splits
                     SET status = 'pending', numerator = ?2, denominator = ?3, resolved_at = NULL
                     WHERE id = ?1",
                    params![id, n.to_string(), d.to_string()],
                )?;
                id
            }
            None => {
                let id = natural_id(
                    "stock_splits",
                    &[&data.ticker, &data.effective_day.to_string()],
                );
                conn.execute(
                    "INSERT INTO stock_splits
                         (id, ticker, effective_day, numerator, denominator, source, status,
                          detected_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'manual', 'pending', ?6)",
                    params![
                        id,
                        data.ticker,
                        data.effective_day,
                        n.to_string(),
                        d.to_string(),
                        now(),
                    ],
                )?;
                id
            }
        };
        apply_in(conn, &id)
    })
}

/// Deletes a manual split that is not applied. Errors: `validation.stockSplitNotDeletable`.
pub fn delete_manual(conn: &Connection, id: &str) -> Result<()> {
    let split = get(conn, id)?;
    if split.source != StockSplitSource::Manual || split.status == StockSplitStatus::Applied {
        return Err(AppError::Validation(
            "validation.stockSplitNotDeletable".into(),
        ));
    }
    conn.execute("DELETE FROM stock_splits WHERE id = ?1", [id])?;
    Ok(())
}

/// Applied splits of the ticker dated after `day`, oldest first: a transaction entered for
/// `day` is in today's units across all of them (D20).
pub fn applied_before(conn: &Connection, ticker: &str, day: i64) -> Result<Vec<StockSplit>> {
    let mut splits = query(
        conn,
        "ticker = ?1 AND status = 'applied' AND effective_day > ?2",
        params![ticker, day],
    )?;
    splits.reverse();
    Ok(splits)
}

/// What [`reconcile_ticker`] changed: per split ID, the transactions it scaled by that split
/// (in either direction), and the rows it rewrote across the four tables.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReconcileReport {
    pub per_split: BTreeMap<String, i64>,
    pub rows: i64,
}

impl ReconcileReport {
    /// Transactions scaled by the split `id`.
    fn rewritten(&self, id: &str) -> i64 {
        self.per_split.get(id).copied().unwrap_or(0)
    }
}

/// One split of the ticker as [`reconcile_ticker`] reads it.
struct BasisSplit {
    id: String,
    effective_day: i64,
    numerator: u64,
    denominator: u64,
    applied: bool,
}

/// The canonical `split_basis` (D27) of a row of `ticker` stamped at `day`: the IDs of the
/// ticker's applied splits with `effective_day > day` as a JSON array sorted ascending, `None`
/// when there are none. A transaction is stamped with its date (D20: it is entered in today's
/// units), a price, dividend or watch row with the time it is written.
pub fn basis_for_day(conn: &Connection, ticker: &str, day: i64) -> Result<Option<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM stock_splits
         WHERE ticker = ?1 AND status = 'applied' AND effective_day > ?2",
    )?;
    let ids = stmt
        .query_map(params![ticker, day], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<BTreeSet<_>, _>>()?;
    encode_basis(&ids)
}

/// Makes every row of `ticker` in `investment_transactions` and the [`SIDE_DATA`] tables match
/// the status of the ticker's splits, through its `split_basis` (D27). Apply, revert and the
/// sync repair all run this one rewrite, so every device scales rows identically.
///
/// Per row, ordered by id: each split in the basis that is no longer `applied` is scaled back
/// out, newest `effective_day` first (ties by id descending); then each `applied` split the
/// basis lacks and that affects the row is scaled in, oldest first (ties by id ascending). A
/// transaction is affected when `transaction_date < effective_day`, a side row when
/// `updated_at < effective_day`. The basis is the source of truth: an applied split already in
/// it is never taken out, and an ID with no `stock_splits` row (its ratio unknown) stays. The
/// D3 roundings apply: quantities of buys half-up, of sells down, prices half-up. Then the
/// ticker's `dividend_data` cache row is dropped and its positions are recomputed; a history
/// that would sell more than it holds fails with `validation.stockSplitOversold` and nothing
/// is written.
pub fn reconcile_ticker(conn: &Connection, ticker: &str) -> Result<ReconcileReport> {
    in_savepoint(conn, || reconcile_in(conn, ticker))
}

fn reconcile_in(conn: &Connection, ticker: &str) -> Result<ReconcileReport> {
    let splits = ticker_splits(conn, ticker)?;
    let mut report = ReconcileReport::default();

    let transactions = {
        let mut stmt = conn.prepare(
            "SELECT id, type, quantity, price_per_unit, transaction_date, split_basis
             FROM investment_transactions WHERE ticker = ?1 ORDER BY id",
        )?;
        let rows = stmt
            .query_map([ticker], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, Option<String>>(5)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    for (id, kind, mut quantity, mut price, date, stored) in transactions {
        let mut basis = decode_basis(stored.as_deref())?;
        let (remove, add) = basis_changes(&splits, &basis, |day| date < day);
        if remove.is_empty() && add.is_empty() {
            continue;
        }
        let rounding = if kind.eq_ignore_ascii_case("sell") {
            Rounding::Down
        } else {
            Rounding::HalfUp
        };
        for split in remove {
            quantity = scale_amount(&quantity, split.denominator, split.numerator, rounding)?;
            price = scale_amount(&price, split.numerator, split.denominator, Rounding::HalfUp)?;
            basis.remove(&split.id);
            *report.per_split.entry(split.id.clone()).or_default() += 1;
        }
        for split in add {
            quantity = scale_amount(&quantity, split.numerator, split.denominator, rounding)?;
            price = scale_amount(&price, split.denominator, split.numerator, Rounding::HalfUp)?;
            basis.insert(split.id.clone());
            *report.per_split.entry(split.id.clone()).or_default() += 1;
        }
        conn.execute(
            "UPDATE investment_transactions
             SET quantity = ?2, price_per_unit = ?3, split_basis = ?4 WHERE id = ?1",
            params![id, quantity, price, encode_basis(&basis)?],
        )?;
        report.rows += 1;
    }

    for (table, column) in SIDE_DATA {
        let side_rows = {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, {column}, updated_at, split_basis FROM {table}
                 WHERE ticker = ?1 ORDER BY id"
            ))?;
            let rows = stmt
                .query_map([ticker], |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, Option<String>>(3)?,
                    ))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            rows
        };
        for (id, mut value, updated_at, stored) in side_rows {
            let mut basis = decode_basis(stored.as_deref())?;
            let (remove, add) = basis_changes(&splits, &basis, |day| updated_at < day);
            if remove.is_empty() && add.is_empty() {
                continue;
            }
            // A per-share value: the inverse of the quantity's factor.
            let scale = |value: Option<String>, mul: u64, div: u64| {
                value
                    .map(|v| scale_amount(&v, mul, div, Rounding::HalfUp))
                    .transpose()
            };
            for split in remove {
                value = scale(value, split.numerator, split.denominator)?;
                basis.remove(&split.id);
            }
            for split in add {
                value = scale(value, split.denominator, split.numerator)?;
                basis.insert(split.id.clone());
            }
            conn.execute(
                &format!("UPDATE {table} SET {column} = ?2, split_basis = ?3 WHERE id = ?1"),
                params![id, value, encode_basis(&basis)?],
            )?;
            report.rows += 1;
        }
    }

    conn.execute("DELETE FROM dividend_data WHERE ticker = ?1", [ticker])?;
    refresh_positions(conn, ticker)?;
    Ok(report)
}

/// The ticker's splits, oldest `effective_day` first, ties by id ascending.
fn ticker_splits(conn: &Connection, ticker: &str) -> Result<Vec<BasisSplit>> {
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT id, effective_day, numerator, denominator, status FROM stock_splits
             WHERE ticker = ?1 ORDER BY effective_day, id",
        )?;
        let rows = stmt
            .query_map([ticker], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    rows.into_iter()
        .map(|(id, effective_day, numerator, denominator, status)| {
            let (numerator, denominator) = ratio_parts(&numerator, &denominator)?;
            Ok(BasisSplit {
                id,
                effective_day,
                numerator,
                denominator,
                applied: StockSplitStatus::parse(&status)? == StockSplitStatus::Applied,
            })
        })
        .collect()
}

/// The splits to scale a row out of (in its basis, no longer applied; newest first) and into
/// (applied, missing from its basis, `affects` its `effective_day`; oldest first). `splits`
/// is in [`ticker_splits`] order.
fn basis_changes<'a>(
    splits: &'a [BasisSplit],
    basis: &BTreeSet<String>,
    affects: impl Fn(i64) -> bool,
) -> (Vec<&'a BasisSplit>, Vec<&'a BasisSplit>) {
    let remove = splits
        .iter()
        .rev()
        .filter(|split| !split.applied && basis.contains(&split.id))
        .collect();
    let add = splits
        .iter()
        .filter(|split| split.applied && !basis.contains(&split.id) && affects(split.effective_day))
        .collect();
    (remove, add)
}

/// A JSON array of the IDs in ascending order, `None` for none (the canonical form).
fn encode_basis(ids: &BTreeSet<String>) -> Result<Option<String>> {
    if ids.is_empty() {
        return Ok(None);
    }
    Ok(Some(serde_json::to_string(ids)?))
}

fn decode_basis(text: Option<&str>) -> Result<BTreeSet<String>> {
    let Some(text) = text else {
        return Ok(BTreeSet::new());
    };
    serde_json::from_str::<Vec<String>>(text)
        .map(|ids| ids.into_iter().collect())
        .map_err(|e| AppError::Database(format!("invalid split_basis {text}: {e}")))
}

/// [`apply`] without its savepoint, for callers that hold one.
fn apply_in(conn: &Connection, id: &str) -> Result<StockSplitApplyResult> {
    let split = get(conn, id)?;
    if split.status == StockSplitStatus::Applied {
        return Err(not_pending());
    }
    ratio_parts(&split.numerator, &split.denominator)?;
    conn.execute(
        "UPDATE stock_splits SET status = 'applied', resolved_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    let rewritten = reconcile_ticker(conn, &split.ticker)?.rewritten(id);
    conn.execute(
        "UPDATE stock_splits SET applied_count = ?2 WHERE id = ?1",
        params![id, rewritten],
    )?;
    apply_result(conn, id, &split.ticker, rewritten)
}

/// Re-checks the holdings of every position with transactions of `ticker` and rewrites its
/// cached quantity. An oversold history becomes `validation.stockSplitOversold`.
fn refresh_positions(conn: &Connection, ticker: &str) -> Result<()> {
    let investment_ids = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT investment_id FROM investment_transactions WHERE ticker = ?1",
        )?;
        let ids = stmt
            .query_map([ticker], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ids
    };
    let oversold = |error: AppError| match error {
        AppError::Validation(key) if key == "validation.sellExceedsHoldings" => {
            AppError::Validation("validation.stockSplitOversold".into())
        }
        other => other,
    };
    for investment_id in investment_ids {
        ensure_valid_holdings_history(conn, "investment_transactions", &investment_id)
            .map_err(oversold)?;
        recalculate_investment_metrics(conn, &investment_id).map_err(oversold)?;
    }
    Ok(())
}

fn apply_result(
    conn: &Connection,
    id: &str,
    ticker: &str,
    rewritten: i64,
) -> Result<StockSplitApplyResult> {
    let earliest: Option<i64> = conn.query_row(
        "SELECT MIN(transaction_date) FROM investment_transactions WHERE ticker = ?1",
        [ticker],
        |r| r.get(0),
    )?;
    Ok(StockSplitApplyResult {
        split: get(conn, id)?,
        rewritten,
        earliest_day: earliest.map(|t| t.div_euclid(DAY_SECONDS) * DAY_SECONDS),
    })
}

/// Runs `work` inside `SAVEPOINT split_apply`, rolled back when it fails. Savepoints nest, so
/// this composes with an outer transaction.
fn in_savepoint<T>(conn: &Connection, work: impl FnOnce() -> Result<T>) -> Result<T> {
    conn.execute_batch("SAVEPOINT split_apply")?;
    match work() {
        Ok(value) => {
            conn.execute_batch("RELEASE split_apply")?;
            Ok(value)
        }
        Err(error) => {
            conn.execute_batch("ROLLBACK TO split_apply; RELEASE split_apply")?;
            Err(error)
        }
    }
}

/// `stock_splits` rows matching `filter`, newest first, with the derived fields filled in.
fn query(conn: &Connection, filter: &str, values: impl Params) -> Result<Vec<StockSplit>> {
    let raw = {
        let mut stmt = conn.prepare(&format!(
            "SELECT {COLUMNS} FROM stock_splits WHERE {filter}
             ORDER BY effective_day DESC, ticker"
        ))?;
        let raw = stmt
            .query_map(values, |r| {
                Ok((
                    (
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, i64>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ),
                    (
                        r.get::<_, String>(5)?,
                        r.get::<_, String>(6)?,
                        r.get::<_, i64>(7)?,
                        r.get::<_, Option<i64>>(8)?,
                        r.get::<_, Option<i64>>(9)?,
                    ),
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        raw
    };
    raw.into_iter()
        .map(
            |(
                (id, ticker, effective_day, numerator, denominator),
                (source, status, detected_at, resolved_at, applied_count),
            )| {
                let status = StockSplitStatus::parse(&status)?;
                let verdict = if status == StockSplitStatus::Pending {
                    stored_verdict(conn, &ticker, effective_day)?
                } else {
                    StockSplitVerdict::Unknown
                };
                Ok(StockSplit {
                    pre_split_count: pre_split_count(conn, &ticker, effective_day)?,
                    id,
                    ticker,
                    effective_day,
                    numerator,
                    denominator,
                    source: StockSplitSource::parse(&source)?,
                    status,
                    detected_at,
                    resolved_at,
                    applied_count,
                    verdict,
                })
            },
        )
        .collect()
}

/// Transactions of `ticker` dated before `effective_day` (D6).
pub(crate) fn pre_split_count(conn: &Connection, ticker: &str, effective_day: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM investment_transactions WHERE ticker = ?1 AND transaction_date < ?2",
        params![ticker, effective_day],
        |r| r.get(0),
    )?)
}

/// D7 for a stored split: each pre-split transaction against the stored close of its UTC day
/// (D18: adjusted once a rebuild covered that day), with the cumulative factor of the ticker's
/// pending splits dated after the transaction (applied ones are already in the rows, dismissed
/// ones count as adjusted). `unknown` unless the split is pending.
fn stored_verdict(
    conn: &Connection,
    ticker: &str,
    effective_day: i64,
) -> Result<StockSplitVerdict> {
    let pending = pending_ratios(conn, ticker)?;
    if !pending.iter().any(|&(day, _)| day == effective_day) {
        return Ok(StockSplitVerdict::Unknown);
    }
    classify_stored(conn, ticker, effective_day, &pending)
}

/// [`stored_verdict`] for a split `n:d` of `effective_day` the import wizard found, which Moony
/// may hold as `pending` or not know at all: the ticker's stored transactions dated before it,
/// judged as the position page would once the split is pending (the ticker's pending splits
/// plus this one when it has no row). The caller does not ask about applied or dismissed ones.
pub fn stored_verdict_for(
    conn: &Connection,
    ticker: &str,
    effective_day: i64,
    numerator: u64,
    denominator: u64,
) -> Result<StockSplitVerdict> {
    let mut pending = pending_ratios(conn, ticker)?;
    if !pending.iter().any(|&(day, _)| day == effective_day) {
        pending.push((effective_day, (numerator, denominator)));
    }
    classify_stored(conn, ticker, effective_day, &pending)
}

/// `(effective_day, (n, d))` of the ticker's pending splits with a valid ratio.
fn pending_ratios(conn: &Connection, ticker: &str) -> Result<Vec<(i64, (u64, u64))>> {
    let mut stmt = conn.prepare(
        "SELECT effective_day, numerator, denominator FROM stock_splits
         WHERE ticker = ?1 AND status = 'pending'",
    )?;
    let rows = stmt
        .query_map(params![ticker], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(day, n, d)| ratio_parts(&n, &d).ok().map(|ratio| (day, ratio)))
        .collect())
}

/// D7 over the ticker's transactions dated before `effective_day`, each against the stored
/// close of its day with the cumulative factor of the `pending` splits dated after it.
fn classify_stored(
    conn: &Connection,
    ticker: &str,
    effective_day: i64,
    pending: &[(i64, (u64, u64))],
) -> Result<StockSplitVerdict> {
    let mut stmt = conn.prepare(
        "SELECT t.price_per_unit, t.currency, t.transaction_date, h.price, h.currency
         FROM investment_transactions t
         LEFT JOIN stock_value_history h
             ON h.ticker = t.ticker
            AND h.recorded_at = t.transaction_date - ((t.transaction_date % 86400) + 86400) % 86400
         WHERE t.ticker = ?1 AND t.transaction_date < ?2",
    )?;
    let verdicts = stmt
        .query_map(params![ticker, effective_day], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .map(|row| {
            let (price, currency, day, close, close_currency) = row?;
            let close = close
                .and_then(|c| c.trim().parse::<f64>().ok())
                .zip(close_currency);
            let ratios: Vec<(u64, u64)> = pending
                .iter()
                .filter(|&&(split_day, _)| split_day > day)
                .map(|&(_, ratio)| ratio)
                .collect();
            Ok(match price.trim().parse::<f64>() {
                Ok(price) => classify_cumulative(
                    price,
                    &currency,
                    close.as_ref().map(|(c, cur)| (*c, cur.as_str())),
                    &ratios,
                ),
                Err(_) => StockSplitVerdict::Unknown,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(verdict_of(&verdicts))
}

fn not_pending() -> AppError {
    AppError::Validation("validation.stockSplitNotPending".into())
}

fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    const DAY: i64 = 86_400;

    fn day(n: i64) -> i64 {
        n * DAY
    }

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                import_batch_id TEXT,
                external_id TEXT,
                split_basis TEXT
            );
            CREATE TABLE stock_splits (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL,
                effective_day INTEGER NOT NULL,
                numerator TEXT NOT NULL,
                denominator TEXT NOT NULL,
                source TEXT NOT NULL CHECK (source IN ('yahoo', 'manual', 'import')),
                status TEXT NOT NULL CHECK (status IN ('pending', 'applied', 'dismissed')),
                detected_at INTEGER NOT NULL DEFAULT (unixepoch()),
                resolved_at INTEGER,
                applied_count INTEGER,
                UNIQUE (ticker, effective_day)
            );
            CREATE TABLE stock_value_history (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL,
                recorded_at INTEGER NOT NULL,
                quantity TEXT NOT NULL,
                price TEXT NOT NULL,
                currency TEXT NOT NULL,
                is_stale INTEGER NOT NULL DEFAULT 0,
                UNIQUE(ticker, recorded_at)
            );
            CREATE TABLE stock_price_overrides (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                split_basis TEXT
            );
            CREATE TABLE watched_stocks (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                target_price TEXT,
                notes TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                split_basis TEXT
            );
            CREATE TABLE dividend_overrides (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                yearly_dividend_sum TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                split_basis TEXT
            );
            CREATE TABLE dividend_data (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                yearly_dividend_sum TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'USD',
                last_fetched_at INTEGER NOT NULL DEFAULT (unixepoch()),
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            "#,
        )
        .expect("schema");
        conn
    }

    /// Inserts a transaction (and the position on first use); returns the transaction id.
    fn tx(conn: &Connection, ticker: &str, kind: &str, qty: &str, price: &str, day: i64) -> String {
        let investment_id = format!("inv-{ticker}");
        conn.execute(
            "INSERT OR IGNORE INTO stock_investments (id, ticker, company_name, quantity, currency)
             VALUES (?1, ?2, ?2, '0', 'USD')",
            rusqlite::params![investment_id, ticker],
        )
        .expect("position");
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date)
             VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, 'USD', ?7)",
            rusqlite::params![id, investment_id, kind, ticker, qty, price, day],
        )
        .expect("transaction");
        id
    }

    fn split(day_number: i64, numerator: u64, denominator: u64) -> DetectedSplit {
        DetectedSplit {
            effective_day: day(day_number),
            numerator,
            denominator,
        }
    }

    /// `(quantity, price_per_unit)` of a transaction.
    fn row(conn: &Connection, id: &str) -> (String, String) {
        conn.query_row(
            "SELECT quantity, price_per_unit FROM investment_transactions WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row")
    }

    fn pair(quantity: &str, price: &str) -> (String, String) {
        (quantity.to_string(), price.to_string())
    }

    fn quantity_of_position(conn: &Connection, ticker: &str) -> String {
        conn.query_row(
            "SELECT quantity FROM stock_investments WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .expect("position quantity")
    }

    fn status_in_table(conn: &Connection, id: &str) -> String {
        conn.query_row("SELECT status FROM stock_splits WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .expect("split row")
    }

    fn validation_key(err: AppError) -> String {
        match err {
            AppError::Validation(key) => key,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    /// NVDA: a buy of 3 @ 950 before the 10:1 split on day 200 and 20 @ 120 after it.
    fn nvda(conn: &Connection) -> (String, String, String) {
        let pre = tx(conn, "NVDA", "buy", "3", "950", day(100));
        let post = tx(conn, "NVDA", "buy", "20", "120", day(400));
        let new = record_detected(conn, "NVDA", &[split(200, 10, 1)], StockSplitSource::Yahoo)
            .expect("record");
        (pre, post, new[0].id.clone())
    }

    fn today() -> i64 {
        chrono::Utc::now().timestamp().div_euclid(DAY) * DAY
    }

    #[test]
    fn record_detected_stores_only_relevant_new_splits_and_reports_them() {
        let conn = setup_test_db();
        tx(&conn, "NVDA", "buy", "3", "950", day(100));
        tx(&conn, "NVDA", "buy", "20", "120", day(400));

        let new = record_detected(
            &conn,
            "NVDA",
            &[split(200, 10, 1), split(50, 4, 1)],
            StockSplitSource::Yahoo,
        )
        .unwrap();
        assert_eq!(new.len(), 1);
        assert_eq!(new[0].status, StockSplitStatus::Pending);
        assert_eq!(new[0].source, StockSplitSource::Yahoo);
        assert_eq!(new[0].effective_day, day(200));
        assert_eq!(
            (new[0].numerator.as_str(), new[0].denominator.as_str()),
            ("10", "1")
        );
        assert_eq!(new[0].pre_split_count, 1);
        let stored: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_splits", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stored, 1, "the split before the first buy is not stored");

        let again =
            record_detected(&conn, "NVDA", &[split(200, 10, 1)], StockSplitSource::Yahoo).unwrap();
        assert!(again.is_empty(), "an existing row is not reported again");
    }

    #[test]
    fn record_detected_skips_ratios_moony_does_not_store() {
        let conn = setup_test_db();
        tx(&conn, "NVDA", "buy", "3", "950", day(100));
        let new = record_detected(
            &conn,
            "NVDA",
            &[split(200, 0, 1), split(210, 2, 2), split(220, 2_000_000, 1)],
            StockSplitSource::Yahoo,
        )
        .unwrap();
        assert!(new.is_empty());
    }

    #[test]
    fn record_detected_never_changes_an_existing_status() {
        let conn = setup_test_db();
        let (_, _, id) = nvda(&conn);
        dismiss(&conn, &id).unwrap();

        let again =
            record_detected(&conn, "NVDA", &[split(200, 10, 1)], StockSplitSource::Yahoo).unwrap();
        assert!(again.is_empty());
        assert_eq!(get(&conn, &id).unwrap().status, StockSplitStatus::Dismissed);
    }

    #[test]
    fn list_hides_splits_without_pre_split_rows() {
        let conn = setup_test_db();
        let (pre, _, id) = nvda(&conn);
        let listed = list_for_ticker(&conn, "NVDA").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(list_pending(&conn).unwrap().len(), 1);

        conn.execute("DELETE FROM investment_transactions WHERE id = ?1", [&pre])
            .unwrap();
        assert!(list_for_ticker(&conn, "NVDA").unwrap().is_empty());
        assert!(list_pending(&conn).unwrap().is_empty());
        assert_eq!(
            status_in_table(&conn, &id),
            "pending",
            "the row stays in the table"
        );
    }

    #[test]
    fn list_is_newest_first_and_pending_only_lists_pending() {
        let conn = setup_test_db();
        tx(&conn, "NVDA", "buy", "3", "950", day(100));
        let new = record_detected(
            &conn,
            "NVDA",
            &[split(200, 10, 1), split(300, 4, 1)],
            StockSplitSource::Yahoo,
        )
        .unwrap();
        let listed = list_for_ticker(&conn, "NVDA").unwrap();
        let days: Vec<i64> = listed.iter().map(|s| s.effective_day).collect();
        assert_eq!(days, vec![day(300), day(200)]);

        dismiss(&conn, &new[0].id).unwrap();
        let pending = list_pending(&conn).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, new[1].id);
    }

    #[test]
    fn verdict_from_value_history() {
        let close = |conn: &Connection, price: &str| {
            conn.execute(
                "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
                 VALUES ('h1', 'NVDA', ?1, '3', ?2, 'USD')",
                rusqlite::params![day(100), price],
            )
            .unwrap();
        };
        let verdict = |price: &str, with_close: bool| {
            let conn = setup_test_db();
            if with_close {
                close(&conn, "96");
            }
            tx(&conn, "NVDA", "buy", "3", price, day(100));
            record_detected(&conn, "NVDA", &[split(200, 10, 1)], StockSplitSource::Yahoo).unwrap();
            let listed = list_for_ticker(&conn, "NVDA").unwrap();
            assert_eq!(listed.len(), 1);
            listed[0].verdict
        };
        assert_eq!(verdict("950", true), StockSplitVerdict::Unadjusted);
        assert_eq!(verdict("96", true), StockSplitVerdict::Adjusted);
        assert_eq!(verdict("950", false), StockSplitVerdict::Unknown);
    }

    #[test]
    fn verdict_uses_the_cumulative_factor_of_the_pending_splits() {
        let conn = setup_test_db();
        for (id, day_number) in [("h1", 100), ("h2", 250)] {
            conn.execute(
                "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
                 VALUES (?1, 'NVDA', ?2, '1', '95', 'USD')",
                rusqlite::params![id, day(day_number)],
            )
            .unwrap();
        }
        // Bought before both splits at 40 × the adjusted close, and between them at 10 ×.
        tx(&conn, "NVDA", "buy", "1", "3800", day(100));
        tx(&conn, "NVDA", "buy", "1", "950", day(250));
        record_detected(
            &conn,
            "NVDA",
            &[split(200, 4, 1), split(300, 10, 1)],
            StockSplitSource::Yahoo,
        )
        .unwrap();
        let verdicts = |conn: &Connection| -> Vec<(i64, StockSplitVerdict)> {
            list_for_ticker(conn, "NVDA")
                .unwrap()
                .iter()
                .map(|s| (s.effective_day, s.verdict))
                .collect()
        };
        assert_eq!(
            verdicts(&conn),
            vec![
                (day(300), StockSplitVerdict::Unadjusted),
                (day(200), StockSplitVerdict::Unadjusted),
            ]
        );

        // A dismissed split counts as adjusted: the 3800 buy is then 4 × the expected price.
        let four = find(&conn, "NVDA", day(200)).unwrap().unwrap();
        dismiss(&conn, &four.id).unwrap();
        assert_eq!(
            verdicts(&conn),
            vec![
                (day(300), StockSplitVerdict::Unadjusted),
                (day(200), StockSplitVerdict::Unknown),
            ]
        );
    }

    #[test]
    fn verdict_is_unknown_once_resolved() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
             VALUES ('h1', 'NVDA', ?1, '3', '96', 'USD')",
            [day(100)],
        )
        .unwrap();
        let (_, _, id) = nvda(&conn);
        assert_eq!(
            get(&conn, &id).unwrap().verdict,
            StockSplitVerdict::Unadjusted
        );
        dismiss(&conn, &id).unwrap();
        assert_eq!(get(&conn, &id).unwrap().verdict, StockSplitVerdict::Unknown);
    }

    #[test]
    fn stored_verdict_for_judges_a_split_moony_does_not_hold_yet() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
             VALUES ('h1', 'NVDA', ?1, '3', '96', 'USD')",
            [day(100)],
        )
        .unwrap();
        tx(&conn, "NVDA", "buy", "3", "950", day(100));

        // No row for the split: the given ratio is what the rows are judged against.
        assert_eq!(
            stored_verdict_for(&conn, "NVDA", day(200), 10, 1).unwrap(),
            StockSplitVerdict::Unadjusted
        );
        assert_eq!(
            stored_verdict_for(&conn, "NVDA", day(200), 2, 1).unwrap(),
            StockSplitVerdict::Unknown,
            "a 2:1 factor cannot tell the cases apart"
        );
        // A pending row: the same answer as the position page gives.
        record_detected(&conn, "NVDA", &[split(200, 10, 1)], StockSplitSource::Yahoo).unwrap();
        assert_eq!(
            stored_verdict_for(&conn, "NVDA", day(200), 10, 1).unwrap(),
            list_for_ticker(&conn, "NVDA").unwrap()[0].verdict
        );
        // Another pending split after the rows counts in the factor.
        record_detected(&conn, "NVDA", &[split(300, 4, 1)], StockSplitSource::Yahoo).unwrap();
        assert_eq!(
            stored_verdict_for(&conn, "NVDA", day(200), 10, 1).unwrap(),
            StockSplitVerdict::Unknown,
            "950 is neither 40 × nor 1 × the close"
        );
        // No rows before the split: nothing to judge.
        assert_eq!(
            stored_verdict_for(&conn, "NVDA", day(50), 10, 1).unwrap(),
            StockSplitVerdict::Unknown
        );
    }

    #[test]
    fn apply_rewrites_only_pre_split_rows_and_keeps_cost_basis() {
        let conn = setup_test_db();
        let (pre, post, id) = nvda(&conn);

        let r = apply(&conn, &id).unwrap();
        assert_eq!(r.rewritten, 1);
        assert_eq!(row(&conn, &pre), pair("30", "95"));
        assert_eq!(row(&conn, &post), pair("20", "120"));
        assert_eq!(r.split.status, StockSplitStatus::Applied);
        assert_eq!(r.split.applied_count, Some(1));
        assert!(r.split.resolved_at.is_some());
        assert_eq!(r.earliest_day, Some(day(100)));
        assert_eq!(quantity_of_position(&conn, "NVDA"), "50");
    }

    #[test]
    fn apply_refuses_a_non_pending_split() {
        let conn = setup_test_db();
        let (pre, _, id) = nvda(&conn);
        apply(&conn, &id).unwrap();
        let err = apply(&conn, &id).unwrap_err();
        assert_eq!(validation_key(err), "validation.stockSplitNotPending");
        assert_eq!(row(&conn, &pre), pair("30", "95"), "not rewritten twice");

        let err = apply(&conn, "missing").unwrap_err();
        assert!(matches!(err, AppError::NotFound(_)), "{err:?}");
    }

    #[test]
    fn apply_accepts_a_dismissed_split() {
        let conn = setup_test_db();
        let (pre, post, id) = nvda(&conn);
        dismiss(&conn, &id).unwrap();

        let r = apply(&conn, &id).unwrap();
        assert_eq!(r.rewritten, 1);
        assert_eq!(r.split.status, StockSplitStatus::Applied);
        assert_eq!(r.split.applied_count, Some(1));
        assert_eq!(row(&conn, &pre), pair("30", "95"));
        assert_eq!(row(&conn, &post), pair("20", "120"));
        assert_eq!(quantity_of_position(&conn, "NVDA"), "50");

        let err = apply(&conn, &id).unwrap_err();
        assert_eq!(validation_key(err), "validation.stockSplitNotPending");
    }

    #[test]
    fn apply_scales_side_data_older_than_the_ex_date_only() {
        let conn = setup_test_db();
        let (_, _, id) = nvda(&conn);
        conn.execute_batch(&format!(
            "INSERT INTO stock_price_overrides (id, ticker, price, currency, updated_at)
                 VALUES ('o1', 'NVDA', '1000', 'USD', {old});
             INSERT INTO watched_stocks (id, ticker, target_price, updated_at)
                 VALUES ('w1', 'NVDA', '1200', {new});
             INSERT INTO dividend_overrides (id, ticker, yearly_dividend_sum, currency, updated_at)
                 VALUES ('d1', 'NVDA', '20', 'USD', {old});
             INSERT INTO dividend_data (id, ticker, yearly_dividend_sum, currency)
                 VALUES ('c1', 'NVDA', '16', 'USD');
             INSERT INTO stock_price_overrides (id, ticker, price, currency, updated_at)
                 VALUES ('o2', 'AAPL', '1000', 'USD', {old});
             INSERT INTO dividend_data (id, ticker, yearly_dividend_sum, currency)
                 VALUES ('c2', 'AAPL', '1', 'USD');",
            old = day(150),
            new = day(250),
        ))
        .unwrap();

        apply(&conn, &id).unwrap();

        let text = |sql: &str| -> String { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        let number = |sql: &str| -> i64 { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        assert_eq!(
            text("SELECT price FROM stock_price_overrides WHERE id = 'o1'"),
            "100"
        );
        assert_eq!(
            number("SELECT updated_at FROM stock_price_overrides WHERE id = 'o1'"),
            day(150),
            "updated_at is not touched"
        );
        assert_eq!(
            text("SELECT target_price FROM watched_stocks WHERE id = 'w1'"),
            "1200"
        );
        assert_eq!(
            text("SELECT yearly_dividend_sum FROM dividend_overrides WHERE id = 'd1'"),
            "2"
        );
        assert_eq!(
            number("SELECT COUNT(*) FROM dividend_data WHERE ticker = 'NVDA'"),
            0
        );
        assert_eq!(
            text("SELECT price FROM stock_price_overrides WHERE id = 'o2'"),
            "1000"
        );
        assert_eq!(
            number("SELECT COUNT(*) FROM dividend_data WHERE ticker = 'AAPL'"),
            1
        );
    }

    #[test]
    fn apply_with_sells_after_a_reverse_split_stays_valid() {
        let conn = setup_test_db();
        let a = tx(&conn, "XYZ", "buy", "1", "3", day(100));
        let b = tx(&conn, "XYZ", "buy", "1", "3", day(110));
        let s = tx(&conn, "XYZ", "sell", "2", "3", day(120));
        let new =
            record_detected(&conn, "XYZ", &[split(200, 1, 3)], StockSplitSource::Yahoo).unwrap();

        let r = apply(&conn, &new[0].id).unwrap();
        assert_eq!(r.rewritten, 3);
        assert_eq!(row(&conn, &a), pair("0.33333333", "9"));
        assert_eq!(row(&conn, &b), pair("0.33333333", "9"));
        assert_eq!(row(&conn, &s), pair("0.66666666", "9"));
        assert_eq!(quantity_of_position(&conn, "XYZ"), "0");
    }

    #[test]
    fn apply_aborts_when_the_history_is_oversold() {
        let conn = setup_test_db();
        let buy = tx(&conn, "XYZ", "buy", "1", "40", day(100));
        let sell = tx(&conn, "XYZ", "sell", "2", "40", day(150));
        let new =
            record_detected(&conn, "XYZ", &[split(200, 4, 1)], StockSplitSource::Yahoo).unwrap();
        conn.execute(
            "INSERT INTO stock_price_overrides (id, ticker, price, currency, updated_at)
             VALUES ('o1', 'XYZ', '40', 'USD', ?1)",
            [day(150)],
        )
        .unwrap();

        let err = apply(&conn, &new[0].id).unwrap_err();
        assert_eq!(validation_key(err), "validation.stockSplitOversold");
        assert_eq!(row(&conn, &buy), pair("1", "40"));
        assert_eq!(row(&conn, &sell), pair("2", "40"));
        assert_eq!(status_in_table(&conn, &new[0].id), "pending");
        let price: String = conn
            .query_row(
                "SELECT price FROM stock_price_overrides WHERE id = 'o1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(price, "40", "side data rolled back too");
    }

    #[test]
    fn revert_restores_terminating_ratios_exactly() {
        let conn = setup_test_db();
        let buy = tx(&conn, "AAPL", "buy", "15", "165.3", day(100));
        let new =
            record_detected(&conn, "AAPL", &[split(200, 4, 1)], StockSplitSource::Yahoo).unwrap();
        let id = new[0].id.clone();

        apply(&conn, &id).unwrap();
        assert_eq!(row(&conn, &buy), pair("60", "41.325"));
        assert_eq!(quantity_of_position(&conn, "AAPL"), "60");

        let r = revert(&conn, &id).unwrap();
        assert_eq!(r.rewritten, 1);
        assert_eq!(row(&conn, &buy), pair("15", "165.3"));
        assert_eq!(r.split.status, StockSplitStatus::Pending);
        assert_eq!(r.split.applied_count, None);
        assert_eq!(r.earliest_day, Some(day(100)));
        assert_eq!(quantity_of_position(&conn, "AAPL"), "15");

        let err = revert(&conn, &id).unwrap_err();
        assert_eq!(validation_key(err), "validation.stockSplitNotApplied");
    }

    #[test]
    fn revert_restores_side_data_and_leaves_other_tickers_alone() {
        let conn = setup_test_db();
        let buy = tx(&conn, "AAPL", "buy", "15", "165.3", day(100));
        let other_buy = tx(&conn, "MSFT", "buy", "7", "310.5", day(90));
        let other_sell = tx(&conn, "MSFT", "sell", "2", "320", day(120));
        let new =
            record_detected(&conn, "AAPL", &[split(200, 4, 1)], StockSplitSource::Yahoo).unwrap();
        let id = new[0].id.clone();
        conn.execute_batch(&format!(
            "INSERT INTO stock_price_overrides (id, ticker, price, currency, updated_at)
                 VALUES ('o1', 'AAPL', '170.2', 'USD', {old});
             INSERT INTO watched_stocks (id, ticker, target_price, updated_at)
                 VALUES ('w1', 'AAPL', '180', {old});
             INSERT INTO dividend_overrides (id, ticker, yearly_dividend_sum, currency, updated_at)
                 VALUES ('d1', 'AAPL', '0.92', 'USD', {old});",
            old = day(150),
        ))
        .unwrap();

        let text = |sql: &str| -> String { conn.query_row(sql, [], |r| r.get(0)).unwrap() };
        let side_data = || {
            (
                text("SELECT price FROM stock_price_overrides WHERE id = 'o1'"),
                text("SELECT target_price FROM watched_stocks WHERE id = 'w1'"),
                text("SELECT yearly_dividend_sum FROM dividend_overrides WHERE id = 'd1'"),
            )
        };
        let original = side_data();

        apply(&conn, &id).unwrap();
        assert_eq!(
            side_data(),
            ("42.55".to_string(), "45".to_string(), "0.23".to_string())
        );
        assert_eq!(row(&conn, &other_buy), pair("7", "310.5"));
        assert_eq!(row(&conn, &other_sell), pair("2", "320"));

        revert(&conn, &id).unwrap();
        assert_eq!(side_data(), original);
        assert_eq!(row(&conn, &buy), pair("15", "165.3"));
        assert_eq!(row(&conn, &other_buy), pair("7", "310.5"));
        assert_eq!(row(&conn, &other_sell), pair("2", "320"));
        assert_eq!(quantity_of_position(&conn, "MSFT"), "0", "never recomputed");
    }

    #[test]
    fn dismiss_and_delete_rules() {
        let conn = setup_test_db();
        let (_, _, yahoo) = nvda(&conn);
        assert_eq!(
            dismiss(&conn, &yahoo).unwrap().status,
            StockSplitStatus::Dismissed
        );
        let err = dismiss(&conn, &yahoo).unwrap_err();
        assert_eq!(validation_key(err), "validation.stockSplitNotPending");
        let err = delete_manual(&conn, &yahoo).unwrap_err();
        assert_eq!(validation_key(err), "validation.stockSplitNotDeletable");

        let manual = record_manual(
            &conn,
            &InsertStockSplit {
                ticker: "NVDA".into(),
                effective_day: day(300),
                numerator: "2".into(),
                denominator: "1".into(),
            },
        )
        .unwrap();
        assert_eq!(manual.split.source, StockSplitSource::Manual);
        assert_eq!(manual.split.status, StockSplitStatus::Applied);
        let err = delete_manual(&conn, &manual.split.id).unwrap_err();
        assert_eq!(
            validation_key(err),
            "validation.stockSplitNotDeletable",
            "an applied manual split is reverted first"
        );

        revert(&conn, &manual.split.id).unwrap();
        delete_manual(&conn, &manual.split.id).unwrap();
        assert!(matches!(
            get(&conn, &manual.split.id),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn record_manual_reuses_a_detected_row() {
        let conn = setup_test_db();
        let (pre, _, id) = nvda(&conn);

        let r = record_manual(
            &conn,
            &InsertStockSplit {
                ticker: "NVDA".into(),
                effective_day: day(200),
                numerator: "10".into(),
                denominator: "1".into(),
            },
        )
        .unwrap();
        assert_eq!(r.split.id, id);
        assert_eq!(r.split.status, StockSplitStatus::Applied);
        assert_eq!(r.split.source, StockSplitSource::Yahoo);
        assert_eq!(r.rewritten, 1);
        assert_eq!(row(&conn, &pre), pair("30", "95"));
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_splits", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 1);

        let err = record_manual(
            &conn,
            &InsertStockSplit {
                ticker: "NVDA".into(),
                effective_day: day(200),
                numerator: "10".into(),
                denominator: "1".into(),
            },
        )
        .unwrap_err();
        assert_eq!(validation_key(err), "validation.stockSplitNotPending");
    }

    #[test]
    fn record_manual_validation() {
        let conn = setup_test_db();
        tx(&conn, "NVDA", "buy", "3", "950", day(100));
        let attempt = |numerator: &str, denominator: &str, effective_day: i64| {
            let err = record_manual(
                &conn,
                &InsertStockSplit {
                    ticker: "NVDA".into(),
                    effective_day,
                    numerator: numerator.into(),
                    denominator: denominator.into(),
                },
            )
            .unwrap_err();
            validation_key(err)
        };
        let ratio = "validation.stockSplitRatioInvalid";
        let date = "validation.stockSplitDayInvalid";
        assert_eq!(attempt("4", "4", day(200)), ratio);
        assert_eq!(attempt("0", "1", day(200)), ratio);
        assert_eq!(attempt("4", "0", day(200)), ratio);
        assert_eq!(attempt("1000001", "1", day(200)), ratio);
        assert_eq!(attempt("1.5", "1", day(200)), ratio);
        assert_eq!(attempt("abc", "1", day(200)), ratio);
        assert_eq!(attempt("4", "1", today() + DAY), date);
        assert_eq!(attempt("4", "1", day(200) + 3_600), date);
        let none: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_splits", [], |r| r.get(0))
            .unwrap();
        assert_eq!(none, 0);

        let empty_ticker = InsertStockSplit {
            ticker: " ".into(),
            effective_day: day(200),
            numerator: "4".into(),
            denominator: "1".into(),
        };
        assert_eq!(
            validation_key(empty_ticker.validate().unwrap_err()),
            "validation.tickerRequired"
        );
        let today_is_fine = InsertStockSplit {
            ticker: "NVDA".into(),
            effective_day: today(),
            numerator: "4".into(),
            denominator: "1".into(),
        };
        today_is_fine.validate().unwrap();
    }

    #[test]
    fn applied_before_lists_applied_splits_after_the_day() {
        let conn = setup_test_db();
        let (_, _, id) = nvda(&conn);
        assert!(
            applied_before(&conn, "NVDA", day(100)).unwrap().is_empty(),
            "pending"
        );

        apply(&conn, &id).unwrap();
        let before = applied_before(&conn, "NVDA", day(100)).unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].id, id);
        assert!(applied_before(&conn, "NVDA", day(300)).unwrap().is_empty());
        assert!(applied_before(&conn, "AAPL", day(100)).unwrap().is_empty());
    }

    fn import_split(effective_day: i64, n: &str, d: &str) -> StockSplitOverride {
        StockSplitOverride {
            effective_day,
            numerator: n.into(),
            denominator: d.into(),
            action: crate::services::stock_import::types::StockSplitAction::Convert,
            apply_stored: true,
        }
    }

    #[test]
    fn find_returns_the_row_of_the_ticker_and_day() {
        let conn = setup_test_db();
        let (_, _, id) = nvda(&conn);
        assert_eq!(find(&conn, "NVDA", day(200)).unwrap().unwrap().id, id);
        assert!(find(&conn, "NVDA", day(201)).unwrap().is_none());
        assert!(find(&conn, "AAPL", day(200)).unwrap().is_none());
    }

    #[test]
    fn insert_from_import_records_a_dismissed_or_a_pending_split() {
        let conn = setup_test_db();
        tx(&conn, "NVDA", "buy", "3", "950", day(100));

        let kept = insert_from_import(
            &conn,
            "NVDA",
            &import_split(day(200), " 10", "1"),
            StockSplitStatus::Dismissed,
        )
        .unwrap();
        assert_eq!(kept.source, StockSplitSource::Import);
        assert_eq!(kept.status, StockSplitStatus::Dismissed);
        assert_eq!(
            (kept.numerator.as_str(), kept.denominator.as_str()),
            ("10", "1")
        );
        assert!(kept.resolved_at.is_some());
        assert_eq!(kept.applied_count, None);

        let pending = insert_from_import(
            &conn,
            "NVDA",
            &import_split(day(300), "2", "1"),
            StockSplitStatus::Pending,
        )
        .unwrap();
        assert_eq!(pending.status, StockSplitStatus::Pending);
        assert_eq!(pending.resolved_at, None);
        let applied = apply(&conn, &pending.id).unwrap();
        assert_eq!(applied.split.status, StockSplitStatus::Applied);
        assert_eq!(applied.split.source, StockSplitSource::Import);
        assert_eq!(applied.rewritten, 1);
    }

    #[test]
    fn insert_from_import_refuses_applied_and_bad_ratios() {
        let conn = setup_test_db();
        let err = insert_from_import(
            &conn,
            "NVDA",
            &import_split(day(200), "10", "1"),
            StockSplitStatus::Applied,
        )
        .unwrap_err();
        assert_eq!(validation_key(err), "validation.stockSplitNotPending");
        let err = insert_from_import(
            &conn,
            "NVDA",
            &import_split(day(200), "1", "1"),
            StockSplitStatus::Dismissed,
        )
        .unwrap_err();
        assert_eq!(validation_key(err), "validation.stockSplitRatioInvalid");
        let none: i64 = conn
            .query_row("SELECT COUNT(*) FROM stock_splits", [], |r| r.get(0))
            .unwrap();
        assert_eq!(none, 0);
    }

    #[test]
    fn a_detected_split_id_follows_ticker_and_day() {
        let natural = crate::services::sync::natural_id::natural_id;
        let conn = setup_test_db();
        tx(&conn, "NVDA", "buy", "3", "950", day(100));
        let rows =
            record_detected(&conn, "NVDA", &[split(150, 10, 1)], StockSplitSource::Yahoo).unwrap();
        let expected = natural("stock_splits", &["NVDA", &day(150).to_string()]);
        assert_eq!(rows.len(), 1, "the split is relevant to NVDA");
        assert_eq!(rows[0].id, expected);

        let imported = insert_from_import(
            &conn,
            "NVDA",
            &import_split(day(160), "2", "1"),
            StockSplitStatus::Dismissed,
        )
        .unwrap();
        assert_eq!(
            imported.id,
            natural("stock_splits", &["NVDA", &day(160).to_string()])
        );

        record_manual(
            &conn,
            &InsertStockSplit {
                ticker: "NVDA".into(),
                effective_day: day(170),
                numerator: "3".into(),
                denominator: "1".into(),
            },
        )
        .unwrap();
        let id: String = conn
            .query_row(
                "SELECT id FROM stock_splits WHERE effective_day = ?1",
                [day(170)],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            id,
            natural("stock_splits", &["NVDA", &day(170).to_string()])
        );
    }
}

/// `split_basis` (D27) on the real schema: apply, revert, reconcile and the writes that stamp it.
#[cfg(test)]
mod split_basis_tests {
    use super::*;
    use crate::models::InsertInvestmentTransaction;
    use crate::services::investments::{
        create_transaction_internal, update_transaction, TransactionOrigin,
    };
    use crate::services::sync::test_support::migrated;
    use crate::services::{price_overrides, stock_monitor};
    use std::collections::BTreeMap;

    const DAY: i64 = 86_400;

    /// UTC midnight `n` days after 2022-01-08 (a plausible trading history).
    fn day(n: i64) -> i64 {
        (19_000 + n) * DAY
    }

    fn position(conn: &Connection) {
        conn.execute(
            "INSERT OR IGNORE INTO stock_investments (id, ticker, company_name, quantity, currency)
             VALUES ('inv-AAPL', 'AAPL', 'Apple', '0', 'USD')",
            [],
        )
        .expect("position");
    }

    /// A transaction written straight to the table with the given basis (what a merge produces).
    fn raw_tx(conn: &Connection, kind: &str, qty: &str, price: &str, on: i64) -> String {
        position(conn);
        let id = uuid::Uuid::new_v4().to_string();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency,
              transaction_date)
             VALUES (?1, 'inv-AAPL', ?2, 'AAPL', 'Apple', ?3, ?4, 'USD', ?5)",
            params![id, kind, qty, price, on],
        )
        .expect("transaction");
        id
    }

    /// A transaction written by the service, as the app writes one.
    fn new_tx(conn: &Connection, qty: &str, price: &str, on: i64) -> String {
        position(conn);
        create_transaction_internal(
            conn,
            "inv-AAPL",
            "AAPL",
            "Apple",
            "buy",
            qty,
            price,
            "USD",
            on,
            None,
            TransactionOrigin::default(),
        )
        .expect("create")
        .id
    }

    fn split_row(conn: &Connection, on: i64, n: u64, d: u64, status: &str) -> String {
        let id = natural_id("stock_splits", &["AAPL", &on.to_string()]);
        conn.execute(
            "INSERT INTO stock_splits
                 (id, ticker, effective_day, numerator, denominator, source, status)
             VALUES (?1, 'AAPL', ?2, ?3, ?4, 'manual', ?5)",
            params![id, on, n.to_string(), d.to_string(), status],
        )
        .expect("split");
        id
    }

    fn tx_values(conn: &Connection, id: &str) -> (String, String) {
        conn.query_row(
            "SELECT quantity, price_per_unit FROM investment_transactions WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("transaction row")
    }

    fn pair(quantity: &str, price: &str) -> (String, String) {
        (quantity.to_string(), price.to_string())
    }

    fn basis(conn: &Connection, table: &str, id: &str) -> Option<String> {
        conn.query_row(
            &format!("SELECT split_basis FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .expect("basis")
    }

    fn side_value(conn: &Connection, table: &str, column: &str, id: &str) -> Option<String> {
        conn.query_row(
            &format!("SELECT {column} FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .expect("side row")
    }

    fn held(conn: &Connection) -> String {
        conn.query_row(
            "SELECT quantity FROM stock_investments WHERE ticker = 'AAPL'",
            [],
            |r| r.get(0),
        )
        .expect("position quantity")
    }

    fn one(id: &str) -> Option<String> {
        Some(format!(r#"["{id}"]"#))
    }

    #[test]
    fn apply_records_the_split_in_the_basis_of_every_rewritten_row() {
        let conn = migrated();
        let pre = raw_tx(&conn, "buy", "15", "165.3", day(100));
        let post = raw_tx(&conn, "buy", "4", "50", day(300));
        let s = split_row(&conn, day(200), 4, 1, "pending");
        conn.execute_batch(&format!(
            "INSERT INTO stock_price_overrides (id, ticker, price, currency, updated_at)
                 VALUES ('o1', 'AAPL', '170', 'USD', {new});
             INSERT INTO watched_stocks (id, ticker, target_price, updated_at)
                 VALUES ('w1', 'AAPL', '180', {old});
             INSERT INTO dividend_overrides (id, ticker, yearly_dividend_sum, currency, updated_at)
                 VALUES ('d1', 'AAPL', '0.92', 'USD', {old});",
            old = day(150),
            new = day(250),
        ))
        .unwrap();

        let r = apply(&conn, &s).unwrap();

        assert_eq!(r.rewritten, 1);
        assert_eq!(r.split.applied_count, Some(1));
        assert_eq!(tx_values(&conn, &pre), pair("60", "41.325"));
        assert_eq!(basis(&conn, "investment_transactions", &pre), one(&s));
        assert_eq!(tx_values(&conn, &post), pair("4", "50"));
        assert_eq!(basis(&conn, "investment_transactions", &post), None);
        assert_eq!(
            side_value(&conn, "watched_stocks", "target_price", "w1"),
            Some("45".into())
        );
        assert_eq!(basis(&conn, "watched_stocks", "w1"), one(&s));
        assert_eq!(
            side_value(&conn, "dividend_overrides", "yearly_dividend_sum", "d1"),
            Some("0.23".into())
        );
        assert_eq!(basis(&conn, "dividend_overrides", "d1"), one(&s));
        assert_eq!(
            side_value(&conn, "stock_price_overrides", "price", "o1"),
            Some("170".into()),
            "set after the ex-date: already in today's units"
        );
        assert_eq!(basis(&conn, "stock_price_overrides", "o1"), None);
        assert_eq!(held(&conn), "64");
    }

    #[test]
    fn revert_scales_back_exactly_the_rows_that_carry_the_split() {
        let conn = migrated();
        let pre = raw_tx(&conn, "buy", "15", "165.3", day(100));
        let s = split_row(&conn, day(200), 4, 1, "pending");
        conn.execute(
            "INSERT INTO dividend_overrides (id, ticker, yearly_dividend_sum, currency, updated_at)
             VALUES ('d1', 'AAPL', '0.92', 'USD', ?1)",
            [day(150)],
        )
        .unwrap();
        apply(&conn, &s).unwrap();

        // Entered after the apply, in today's units, for a day before the split.
        let added = new_tx(&conn, "8", "40", day(150));
        assert_eq!(basis(&conn, "investment_transactions", &added), one(&s));
        // Edited after the apply: today's units, no basis.
        price_overrides::upsert_stock_price(&conn, "AAPL", "42", "USD", day(250)).unwrap();
        let override_id = natural_id("stock_price_overrides", &["AAPL"]);
        // A row that claims an old date but carries no basis is not the split's to undo.
        conn.execute(
            "INSERT INTO watched_stocks (id, ticker, target_price, updated_at)
             VALUES ('w1', 'AAPL', '30', ?1)",
            [day(150)],
        )
        .unwrap();

        let r = revert(&conn, &s).unwrap();

        assert_eq!(r.rewritten, 2);
        assert_eq!(r.split.status, StockSplitStatus::Pending);
        assert_eq!(tx_values(&conn, &pre), pair("15", "165.3"));
        assert_eq!(basis(&conn, "investment_transactions", &pre), None);
        assert_eq!(tx_values(&conn, &added), pair("2", "160"));
        assert_eq!(basis(&conn, "investment_transactions", &added), None);
        assert_eq!(
            side_value(&conn, "dividend_overrides", "yearly_dividend_sum", "d1"),
            Some("0.92".into())
        );
        assert_eq!(basis(&conn, "dividend_overrides", "d1"), None);
        assert_eq!(
            side_value(&conn, "stock_price_overrides", "price", &override_id),
            Some("42".into())
        );
        assert_eq!(basis(&conn, "stock_price_overrides", &override_id), None);
        assert_eq!(
            side_value(&conn, "watched_stocks", "target_price", "w1"),
            Some("30".into())
        );
        assert_eq!(basis(&conn, "watched_stocks", "w1"), None);
        assert_eq!(held(&conn), "17");
    }

    #[test]
    fn reconcile_scales_a_pre_split_row_that_missed_the_apply() {
        let conn = migrated();
        let pre = raw_tx(&conn, "buy", "15", "165.3", day(100));
        let s = split_row(&conn, day(200), 4, 1, "pending");
        apply(&conn, &s).unwrap();
        let missed = raw_tx(&conn, "buy", "3", "160", day(120));
        assert_eq!(basis(&conn, "investment_transactions", &missed), None);

        let report = reconcile_ticker(&conn, "AAPL").unwrap();

        assert_eq!(report.per_split, BTreeMap::from([(s.clone(), 1)]));
        assert_eq!(report.rows, 1);
        assert_eq!(tx_values(&conn, &missed), pair("12", "40"));
        assert_eq!(basis(&conn, "investment_transactions", &missed), one(&s));
        assert_eq!(tx_values(&conn, &pre), pair("60", "41.325"));
        assert_eq!(basis(&conn, "investment_transactions", &pre), one(&s));
        assert_eq!(held(&conn), "72");
    }

    #[test]
    fn reconcile_removes_a_split_that_is_no_longer_applied() {
        let conn = migrated();
        let pre = raw_tx(&conn, "buy", "15", "165.3", day(100));
        let s = split_row(&conn, day(200), 4, 1, "pending");
        conn.execute(
            "INSERT INTO dividend_overrides (id, ticker, yearly_dividend_sum, currency, updated_at)
             VALUES ('d1', 'AAPL', '0.92', 'USD', ?1)",
            [day(150)],
        )
        .unwrap();
        apply(&conn, &s).unwrap();
        conn.execute(
            "UPDATE stock_splits SET status = 'dismissed' WHERE id = ?1",
            [&s],
        )
        .unwrap();

        let report = reconcile_ticker(&conn, "AAPL").unwrap();

        assert_eq!(report.per_split, BTreeMap::from([(s.clone(), 1)]));
        assert_eq!(report.rows, 2);
        assert_eq!(tx_values(&conn, &pre), pair("15", "165.3"));
        assert_eq!(basis(&conn, "investment_transactions", &pre), None);
        assert_eq!(
            side_value(&conn, "dividend_overrides", "yearly_dividend_sum", "d1"),
            Some("0.92".into())
        );
        assert_eq!(basis(&conn, "dividend_overrides", "d1"), None);
        assert_eq!(held(&conn), "15");
    }

    #[test]
    fn reconcile_is_idempotent() {
        let conn = migrated();
        let early = raw_tx(&conn, "buy", "7", "10", day(100));
        let between = raw_tx(&conn, "buy", "5", "4", day(250));
        let a = split_row(&conn, day(200), 3, 1, "applied");
        let b = split_row(&conn, day(300), 1, 2, "applied");
        let mut both = [a.clone(), b.clone()];
        both.sort();

        let first = reconcile_ticker(&conn, "AAPL").unwrap();
        assert_eq!(
            first.per_split,
            BTreeMap::from([(a.clone(), 1), (b.clone(), 2)])
        );
        assert_eq!(first.rows, 2);
        // 3:1 first (oldest), then 1:2: 7 × 3 / 2, 10 / 3 (half-up) × 2.
        assert_eq!(tx_values(&conn, &early), pair("10.5", "6.66666666"));
        assert_eq!(
            basis(&conn, "investment_transactions", &early),
            Some(format!(r#"["{}","{}"]"#, both[0], both[1]))
        );
        assert_eq!(tx_values(&conn, &between), pair("2.5", "8"));
        assert_eq!(basis(&conn, "investment_transactions", &between), one(&b));
        assert_eq!(held(&conn), "13");

        let second = reconcile_ticker(&conn, "AAPL").unwrap();
        assert_eq!(second.per_split, BTreeMap::new());
        assert_eq!(second.rows, 0);
        assert_eq!(tx_values(&conn, &early), pair("10.5", "6.66666666"));
        assert_eq!(tx_values(&conn, &between), pair("2.5", "8"));
        assert_eq!(held(&conn), "13");
    }

    #[test]
    fn a_new_transaction_dated_before_an_applied_split_carries_it() {
        let conn = migrated();
        let s = split_row(&conn, day(200), 4, 1, "pending");
        apply(&conn, &s).unwrap();

        let id = new_tx(&conn, "8", "40", day(150));
        assert_eq!(
            basis(&conn, "investment_transactions", &id),
            Some(format!(r#"["{s}"]"#))
        );
        let later = new_tx(&conn, "2", "45", day(250));
        assert_eq!(basis(&conn, "investment_transactions", &later), None);

        let edit = |on: i64| InsertInvestmentTransaction {
            investment_id: Some("inv-AAPL".into()),
            tx_type: "buy".into(),
            ticker: "AAPL".into(),
            company_name: "Apple".into(),
            quantity: "2".into(),
            price_per_unit: "45".into(),
            currency: "USD".into(),
            transaction_date: on,
            fee: None,
        };
        update_transaction(&conn, &later, &edit(day(120))).unwrap();
        assert_eq!(basis(&conn, "investment_transactions", &later), one(&s));
        update_transaction(&conn, &later, &edit(day(260))).unwrap();
        assert_eq!(basis(&conn, "investment_transactions", &later), None);
    }

    #[test]
    fn an_override_set_after_the_split_carries_no_basis() {
        let conn = migrated();
        let s = split_row(&conn, day(200), 4, 1, "pending");
        apply(&conn, &s).unwrap();

        let price =
            price_overrides::upsert_stock_price(&conn, "AAPL", "42", "USD", day(300)).unwrap();
        assert_eq!(basis(&conn, "stock_price_overrides", &price.id), None);
        let dividend =
            price_overrides::upsert_dividend(&conn, "AAPL", "0.25", "USD", day(300)).unwrap();
        assert_eq!(basis(&conn, "dividend_overrides", &dividend.id), None);
        let watched = stock_monitor::follow_stock(
            &conn,
            &crate::models::stock_monitor::InsertWatchedStock {
                ticker: "AAPL".into(),
            },
        )
        .unwrap();
        stock_monitor::set_target_price(&conn, "AAPL", Some("50".into()), Some("above".into()))
            .unwrap();
        assert_eq!(basis(&conn, "watched_stocks", &watched.id), None);

        // The basis follows the write's own stamp: one dated before the split carries it.
        price_overrides::upsert_dividend(&conn, "AAPL", "1", "USD", day(150)).unwrap();
        assert_eq!(basis(&conn, "dividend_overrides", &dividend.id), one(&s));
    }
}
