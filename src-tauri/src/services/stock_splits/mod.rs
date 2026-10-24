//! Splits of stock positions, spec `docs/specs/2026-10-24-stock-splits-design.md`.
//!
//! A `stock_splits` row records one split of a ticker (D4). Detection stores the relevant ones
//! as `pending` (D5, D6); applying a split rewrites the transactions dated before its ex-date
//! into today's share units together with the ticker's per-share side data (D1, D3, D10), and
//! reverting does the same with the ratio turned round. Every function takes `&Connection`;
//! the commands call them inside `db.with_conn`.

pub mod classify;
pub mod scale;
pub mod types;

pub use types::{
    ratio_parts, DetectedSplit, InsertStockSplit, StockSplit, StockSplitApplyResult,
    StockSplitSource, StockSplitStatus, StockSplitVerdict,
};

use rusqlite::{params, Connection, OptionalExtension, Params};
use uuid::Uuid;

use crate::error::{AppError, Result};
use crate::services::investments::{ensure_valid_holdings_history, recalculate_investment_metrics};
use crate::services::stock_import::types::StockSplitOverride;
use classify::{classify, verdict_of};
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
    let id = Uuid::new_v4().to_string();
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
        let id = Uuid::new_v4().to_string();
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

/// Applies a pending split (D10): rewrites the pre-split transactions and side data, marks it
/// `applied`, and recomputes the positions. A history that would sell more than it holds
/// aborts everything with `validation.stockSplitOversold`.
pub fn apply(conn: &Connection, id: &str) -> Result<StockSplitApplyResult> {
    in_savepoint(conn, || apply_in(conn, id))
}

/// Undoes an applied split (D10 with the ratio turned round) and sets it back to `pending`.
/// Errors: `validation.stockSplitNotApplied`, `validation.stockSplitOversold`.
pub fn revert(conn: &Connection, id: &str) -> Result<StockSplitApplyResult> {
    in_savepoint(conn, || {
        let split = get(conn, id)?;
        if split.status != StockSplitStatus::Applied {
            return Err(AppError::Validation(
                "validation.stockSplitNotApplied".into(),
            ));
        }
        let (n, d) = ratio_parts(&split.numerator, &split.denominator)?;
        let rewritten = rewrite_pre_split_rows(conn, &split.ticker, split.effective_day, d, n)?;
        conn.execute(
            "UPDATE stock_splits SET status = 'pending', resolved_at = NULL, applied_count = NULL
             WHERE id = ?1",
            [id],
        )?;
        refresh_positions(conn, &split.ticker)?;
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
                let id = Uuid::new_v4().to_string();
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

/// The D1/D3 rewrite: every transaction of `ticker` dated before `effective_day` gets
/// `quantity × mul / div` (buys half-up, sells down) and `price × div / mul` (half-up); the
/// side data older than `effective_day` is scaled by `div / mul` without touching its
/// `updated_at`, and the ticker's `dividend_data` cache row is dropped. Returns the number of
/// transactions rewritten. Apply passes `(n, d)`, revert `(d, n)`.
pub(crate) fn rewrite_pre_split_rows(
    conn: &Connection,
    ticker: &str,
    effective_day: i64,
    mul: u64,
    div: u64,
) -> Result<i64> {
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT id, type, quantity, price_per_unit FROM investment_transactions
             WHERE ticker = ?1 AND transaction_date < ?2 ORDER BY rowid",
        )?;
        let rows = stmt
            .query_map(params![ticker, effective_day], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows
    };
    for (id, kind, quantity, price) in &rows {
        let rounding = if kind.eq_ignore_ascii_case("sell") {
            Rounding::Down
        } else {
            Rounding::HalfUp
        };
        conn.execute(
            "UPDATE investment_transactions SET quantity = ?2, price_per_unit = ?3 WHERE id = ?1",
            params![
                id,
                scale_amount(quantity, mul, div, rounding)?,
                scale_amount(price, div, mul, Rounding::HalfUp)?,
            ],
        )?;
    }

    for (table, column) in SIDE_DATA {
        let values = {
            let mut stmt = conn.prepare(&format!(
                "SELECT id, {column} FROM {table}
                 WHERE ticker = ?1 AND updated_at < ?2 AND {column} IS NOT NULL"
            ))?;
            let values = stmt
                .query_map(params![ticker, effective_day], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            values
        };
        for (id, value) in values {
            conn.execute(
                &format!("UPDATE {table} SET {column} = ?2 WHERE id = ?1"),
                params![id, scale_amount(&value, div, mul, Rounding::HalfUp)?],
            )?;
        }
    }
    conn.execute("DELETE FROM dividend_data WHERE ticker = ?1", [ticker])?;

    Ok(rows.len() as i64)
}

/// [`apply`] without its savepoint, for callers that hold one.
fn apply_in(conn: &Connection, id: &str) -> Result<StockSplitApplyResult> {
    let split = get(conn, id)?;
    if split.status != StockSplitStatus::Pending {
        return Err(not_pending());
    }
    let (n, d) = ratio_parts(&split.numerator, &split.denominator)?;
    let rewritten = rewrite_pre_split_rows(conn, &split.ticker, split.effective_day, n, d)?;
    conn.execute(
        "UPDATE stock_splits SET status = 'applied', resolved_at = ?2, applied_count = ?3
         WHERE id = ?1",
        params![id, now(), rewritten],
    )?;
    refresh_positions(conn, &split.ticker)?;
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
                    stored_verdict(conn, &ticker, effective_day, &numerator, &denominator)?
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
fn pre_split_count(conn: &Connection, ticker: &str, effective_day: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM investment_transactions WHERE ticker = ?1 AND transaction_date < ?2",
        params![ticker, effective_day],
        |r| r.get(0),
    )?)
}

/// D7 for a stored split: each pre-split transaction against the stored close of its UTC day
/// (D18: adjusted once a rebuild covered that day).
fn stored_verdict(
    conn: &Connection,
    ticker: &str,
    effective_day: i64,
    numerator: &str,
    denominator: &str,
) -> Result<StockSplitVerdict> {
    let Ok((n, d)) = ratio_parts(numerator, denominator) else {
        return Ok(StockSplitVerdict::Unknown);
    };
    let mut stmt = conn.prepare(
        "SELECT t.price_per_unit, t.currency, h.price, h.currency
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
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?
        .map(|row| {
            let (price, currency, close, close_currency) = row?;
            let close = close
                .and_then(|c| c.trim().parse::<f64>().ok())
                .zip(close_currency);
            Ok(match price.trim().parse::<f64>() {
                Ok(price) => classify(
                    price,
                    &currency,
                    close.as_ref().map(|(c, cur)| (*c, cur.as_str())),
                    n,
                    d,
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
                external_id TEXT
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
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE watched_stocks (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                target_price TEXT,
                notes TEXT NOT NULL DEFAULT '',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE dividend_overrides (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                yearly_dividend_sum TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
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
}
