//! Investment business logic service
//!
//! All business logic for investments lives here.
//! Commands should only validate input, call these functions, and handle events.
//! This is the SINGLE SOURCE OF TRUTH for investment operations.

use crate::error::{AppError, Result};
use crate::models::{InsertInvestmentTransaction, InvestmentTransaction, StockInvestment};
use crate::services::dedup;
use chrono::DateTime;
use rusqlite::params_from_iter;
use rusqlite::types::Value;
use rusqlite::OptionalExtension;
use std::collections::{BTreeMap, HashMap};
use uuid::Uuid;

/// Recalculate investment quantity and average price from transactions
/// SINGLE SOURCE OF TRUTH for investment metrics calculation
/// NOTE: Average price is calculated in the investment's native currency (set by first transaction)
/// Uses a moving average: buys add to the cost basis, sells remove cost basis proportionally
/// (at the average cost, not the sale price) — mirrors `recalculate_crypto_metrics`, minus
/// currency conversion (stocks enforce a single currency per investment, so none is needed).
///
/// Ordered by `transaction_date, created_at, rowid`. The first two columns can
/// tie within one bulk import (second-granularity `created_at` stamps every
/// row in a batch identically), and the result is order-sensitive: a same-date
/// sell processed before its buy hits the `total_qty > 0` guard and skips the
/// cost-basis reduction, landing on a different average price than
/// buy-then-sell would. `rowid ASC` breaks the tie deterministically instead
/// of leaving it to SQLite's unspecified sort order for equal keys.
/// `investment_transactions` is an ordinary rowid table, and rowid ==
/// insertion order == the bulk importer's buys-before-sells order, so this
/// pins today's observed behavior with zero change for existing data.
pub fn recalculate_investment_metrics(
    conn: &rusqlite::Connection,
    investment_id: &str,
) -> Result<()> {
    // Get all transactions for this investment, ordered chronologically; rowid
    // is the deterministic tiebreak for same-date/same-second rows (see above).
    let mut stmt = conn.prepare(
        "SELECT type, quantity, price_per_unit FROM investment_transactions WHERE investment_id = ?1 ORDER BY transaction_date ASC, created_at ASC, rowid ASC"
    )?;

    let txs: Vec<(String, f64, f64)> = stmt
        .query_map([investment_id], |row| {
            let type_: String = row.get(0)?;
            let qty_str: String = row.get(1)?;
            let price_str: String = row.get(2)?;

            // Tolerant parsing: legacy data may contain suffixes (e.g. a unit string)
            let qty = qty_str
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse::<f64>()
                .unwrap_or(0.0);
            let price = price_str
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .parse::<f64>()
                .unwrap_or(0.0);

            Ok((type_, qty, price))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // No transactions yet (defensive: migration 008 backfills a synthetic buy
    // for legacy manual positions, and creation now requires an initial
    // transaction) — leave the stored quantity as-is instead of zeroing it.
    if txs.is_empty() {
        return Ok(());
    }

    let mut total_qty = 0.0f64;

    for (tx_type, qty, _price) in txs {
        if tx_type == "buy" {
            total_qty += qty;
        } else if tx_type == "sell" {
            total_qty -= qty;
        }
    }

    // Prevent negative values
    if total_qty < 0.0 {
        total_qty = 0.0;
    }

    // Quantity is the only stored derived value; cost basis and average price
    // are derived on read from transactions at their day's exchange rates
    // (services/cost_basis.rs).
    conn.execute(
        "UPDATE stock_investments SET quantity = ?1 WHERE id = ?2",
        rusqlite::params![total_qty.to_string(), investment_id],
    )?;

    Ok(())
}

/// Recalculate quantity for every stock investment.
/// Idempotent self-heal that runs on every database open: corrects any stale quantity
/// without waiting for each investment's next transaction. Investments with no transactions are left untouched (see the early
/// return in `recalculate_investment_metrics`), so manually-entered positions survive.
/// Never aborts on a single investment's failure — logs and continues, so one bad row can't
/// block the database from opening.
pub fn recalculate_all_investment_metrics(conn: &rusqlite::Connection) -> Result<()> {
    let mut stmt = conn.prepare("SELECT id FROM stock_investments")?;
    let ids: Vec<String> = stmt
        .query_map([], |row| row.get(0))?
        .filter_map(|r| r.ok())
        .collect();

    for id in ids {
        if let Err(e) = recalculate_investment_metrics(conn, &id) {
            log::warn!(
                "[Investments] Warning: Failed to recalculate metrics for investment {}: {}",
                id,
                e
            );
        }
    }

    Ok(())
}

/// Delete stock transactions by id, together, in one database transaction.
///
/// Every investment left without transactions is removed with them; the others
/// get their quantity recalculated. Returns, per affected ticker, the earliest
/// date among the deleted transactions — the day its history rebuild must start
/// from — so a bulk delete costs one rebuild per ticker, not one per row
///. An unknown id fails the whole call before anything is deleted.
pub fn delete_transactions(
    conn: &rusqlite::Connection,
    tx_ids: &[String],
) -> Result<Vec<(String, i64)>> {
    if tx_ids.is_empty() {
        return Ok(Vec::new());
    }

    let db_tx = conn.unchecked_transaction()?;

    let mut investment_ids: Vec<String> = Vec::new();
    let mut earliest_by_ticker: BTreeMap<String, i64> = BTreeMap::new();
    for id in tx_ids {
        let (investment_id, ticker, date): (String, String, i64) = db_tx
            .query_row(
                "SELECT investment_id, ticker, transaction_date
                 FROM investment_transactions WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .ok_or_else(|| AppError::NotFound("Transaction not found".into()))?;
        if !investment_ids.contains(&investment_id) {
            investment_ids.push(investment_id);
        }
        earliest_by_ticker
            .entry(ticker)
            .and_modify(|earliest| *earliest = (*earliest).min(date))
            .or_insert(date);
    }

    for id in tx_ids {
        db_tx.execute("DELETE FROM investment_transactions WHERE id = ?1", [id])?;
    }

    for investment_id in &investment_ids {
        let remaining: i64 = db_tx.query_row(
            "SELECT COUNT(*) FROM investment_transactions WHERE investment_id = ?1",
            [investment_id],
            |row| row.get(0),
        )?;
        if remaining == 0 {
            // No transactions left - delete the investment entirely
            db_tx.execute(
                "DELETE FROM stock_investments WHERE id = ?1",
                [investment_id],
            )?;
        } else {
            recalculate_investment_metrics(&db_tx, investment_id)?;
        }
    }

    db_tx.commit()?;
    Ok(earliest_by_ticker.into_iter().collect())
}

/// Where a stock transaction came from. Hand-made and MCP-created ones have
/// neither field; a CSV import sets both (migration 003).
#[derive(Debug, Clone, Copy, Default)]
pub struct TransactionOrigin<'a> {
    /// `stock_import_batches.id` of the import that wrote the row.
    pub import_batch_id: Option<&'a str>,
    /// `<source>:<the broker's own transaction id>`.
    pub external_id: Option<&'a str>,
}

/// Internal function to create a transaction record
/// Used by both single-create, add transaction, and bulk import
/// This is the SINGLE SOURCE OF TRUTH for transaction creation
#[allow(clippy::too_many_arguments)]
pub fn create_transaction_internal(
    conn: &rusqlite::Connection,
    investment_id: &str,
    ticker: &str,
    company_name: &str,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
    currency: &str,
    transaction_date: i64,
    origin: TransactionOrigin<'_>,
) -> Result<InvestmentTransaction> {
    let tx_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let currency_upper = currency.to_uppercase();

    conn.execute(
        "INSERT INTO investment_transactions
         (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at, import_batch_id, external_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            tx_id,
            investment_id,
            tx_type,
            ticker,
            company_name,
            quantity,
            price_per_unit,
            currency_upper,
            transaction_date,
            now,
            origin.import_batch_id,
            origin.external_id,
        ],
    )?;

    Ok(InvestmentTransaction {
        id: tx_id,
        investment_id: investment_id.to_string(),
        tx_type: tx_type.to_string(),
        ticker: ticker.to_string(),
        company_name: company_name.to_string(),
        quantity: quantity.to_string(),
        price_per_unit: price_per_unit.to_string(),
        currency: currency_upper,
        transaction_date,
        created_at: now,
    })
}

/// Get or create an investment by ticker
/// Returns the investment ID
/// This is the SINGLE SOURCE OF TRUTH for investment lookup/creation
pub fn get_or_create_investment(
    conn: &rusqlite::Connection,
    ticker: &str,
    company_name: &str,
    currency: &str,
) -> Result<String> {
    let ticker_upper = ticker.to_uppercase();

    // Check if investment already exists
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM stock_investments WHERE ticker = ?1",
            [&ticker_upper],
            |row| row.get(0),
        )
        .ok();

    match existing {
        Some(id) => Ok(id),
        None => {
            let id = Uuid::new_v4().to_string();
            let currency_upper = currency.to_uppercase();

            conn.execute(
                "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
                 VALUES (?1, ?2, ?3, '0', ?4)",
                rusqlite::params![id, ticker_upper, company_name, currency_upper],
            )?;
            Ok(id)
        }
    }
}

/// Get investment details by ID
pub fn get_investment_by_id(
    conn: &rusqlite::Connection,
    investment_id: &str,
) -> Result<StockInvestment> {
    let mut inv = conn.query_row(
        "SELECT id, ticker, company_name, quantity, currency FROM stock_investments WHERE id = ?1",
        [investment_id],
        |row| {
            Ok(StockInvestment {
                id: row.get(0)?,
                ticker: row.get(1)?,
                company_name: row.get(2)?,
                quantity: row.get(3)?,
                average_price: "0".to_string(),
                currency: row.get(4)?,
            })
        },
    )?;
    // Average price is derived from transactions (weighted average in the
    // position's primary currency), never stored.
    inv.average_price = crate::services::cost_basis::cost_basis_for_all(
        conn,
        crate::services::cost_basis::TxTable::Stocks,
    )?
    .get(investment_id)
    .map(|p| p.average_price_native().to_string())
    .unwrap_or_else(|| "0".to_string());
    Ok(inv)
}

/// Get investment details by ticker
pub fn get_investment_by_ticker(
    conn: &rusqlite::Connection,
    ticker: &str,
) -> Result<Option<(String, String)>> {
    let ticker_upper = ticker.to_uppercase();

    let result: Option<(String, String)> = conn
        .query_row(
            "SELECT id, company_name FROM stock_investments WHERE ticker = ?1",
            [&ticker_upper],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    Ok(result)
}

/// Unified import logic for a single transaction
/// Used by CSV import and creates both investment (if needed) and transaction
/// Returns (description, transaction_date, ticker) on success
#[allow(clippy::too_many_arguments)]
pub fn import_single_transaction(
    conn: &rusqlite::Connection,
    ticker: &str,
    company_name: &str,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
    currency: &str,
    transaction_date: i64,
) -> Result<(String, i64, String)> {
    let written = import_single_transaction_with_origin(
        conn,
        ticker,
        company_name,
        tx_type,
        quantity,
        price_per_unit,
        currency,
        transaction_date,
        TransactionOrigin::default(),
    )?;
    Ok((
        written.description,
        written.transaction_date,
        written.ticker,
    ))
}

/// What [`import_single_transaction_with_origin`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedTransaction {
    pub description: String,
    pub transaction_date: i64,
    /// Uppercase ticker of the position the row went to.
    pub ticker: String,
    /// The row was the first of its position, which it created.
    pub position_created: bool,
}

/// [`import_single_transaction`] that also records where the row came from.
/// Business rules (all fail with `AppError::Validation`): a sell needs an
/// existing position, and a position keeps the currency of its first buy.
#[allow(clippy::too_many_arguments)]
pub fn import_single_transaction_with_origin(
    conn: &rusqlite::Connection,
    ticker: &str,
    company_name: &str,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
    currency: &str,
    transaction_date: i64,
    origin: TransactionOrigin<'_>,
) -> Result<ImportedTransaction> {
    let ticker_upper = ticker.to_uppercase();
    let tx_type_lower = tx_type.to_lowercase();

    // Validate transaction type
    if tx_type_lower != "buy" && tx_type_lower != "sell" {
        return Err(AppError::Validation(format!(
            "Invalid type '{}' (must be 'buy' or 'sell')",
            tx_type
        )));
    }

    // Check if investment exists
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM stock_investments WHERE ticker = ?1",
            [&ticker_upper],
            |row| row.get(0),
        )
        .ok();

    // For sell transactions, require existing investment
    if tx_type_lower == "sell" && existing.is_none() {
        return Err(AppError::Validation(format!(
            "{}: Cannot sell - no existing position found",
            ticker_upper
        )));
    }

    // Get or create investment
    let position_created = existing.is_none();
    let investment_id = match existing {
        Some(id) => {
            // Validate currency matches for existing investment
            let inv_currency: String = conn
                .query_row(
                    "SELECT currency FROM stock_investments WHERE id = ?1",
                    [&id],
                    |row| row.get(0),
                )
                .unwrap_or_else(|_| currency.to_uppercase());

            if inv_currency.to_uppercase() != currency.to_uppercase() {
                return Err(AppError::Validation(format!(
                    "{}: Currency mismatch - investment uses {} but transaction is in {}",
                    ticker_upper, inv_currency, currency
                )));
            }
            id
        }
        None => {
            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
                 VALUES (?1, ?2, ?3, '0', ?4)",
                rusqlite::params![id, ticker_upper, company_name, currency.to_uppercase()],
            )?;
            id
        }
    };

    // Create transaction using the shared function
    create_transaction_internal(
        conn,
        &investment_id,
        &ticker_upper,
        company_name,
        &tx_type_lower,
        quantity,
        price_per_unit,
        currency,
        transaction_date,
        origin,
    )?;

    // Recalculate metrics
    recalculate_investment_metrics(conn, &investment_id)?;

    Ok(ImportedTransaction {
        description: format!(
            "{} {} {} @ {}",
            tx_type_lower.to_uppercase(),
            quantity,
            ticker_upper,
            price_per_unit
        ),
        transaction_date,
        ticker: ticker_upper,
        position_created,
    })
}

/// One row of a bulk stock write ([`bulk_create_stock_transactions`]).
#[derive(Debug, Clone)]
pub struct BulkStockRow {
    pub ticker: String,
    /// Used when the write creates the position; defaults to the ticker.
    pub company_name: Option<String>,
    /// `buy` or `sell` (any case).
    pub tx_type: String,
    /// Positive decimal text.
    pub quantity: String,
    /// Positive decimal text, in `currency`.
    pub price_per_unit: String,
    /// 3-letter ISO code.
    pub currency: String,
    /// Unix seconds (UTC midnight of the trade day for imports).
    pub transaction_date: i64,
    /// `<source>:<the broker's own transaction id>` of a CSV import row.
    pub external_id: Option<String>,
    /// Write the row even when an identical transaction exists. The CSV import
    /// decides duplicates itself (its preview shows them) and sets this on
    /// every row it hands over.
    pub allow_duplicate: bool,
}

/// The recorded import a bulk write belongs to (`stock_import_batches`).
#[derive(Debug, Clone)]
pub struct NewStockImportBatch {
    pub file_name: String,
    /// `xtb`, `trading212`, `degiro`, `ibkr`, `moony`, `custom` or `format:<uuid>`.
    pub source: String,
}

/// A row of a bulk write that was left out or rejected: `index` is its
/// position in the rows handed to [`bulk_create_stock_transactions`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkRowIssue {
    pub index: usize,
    pub message: String,
}

/// What [`bulk_create_stock_transactions`] did. `errors` non-empty implies
/// `created == 0` and nothing was written, whether a row failed field
/// validation or a business rule rolled the whole batch back. (The MCP tool
/// turns it into its own wire type, `services::mcp::BulkWriteReport`.)
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BulkWriteReport {
    pub created: usize,
    pub skipped_duplicates: Vec<BulkRowIssue>,
    pub errors: Vec<BulkRowIssue>,
    /// The recorded batch: only when one was requested and a row was written.
    pub batch_id: Option<String>,
    /// Tickers whose position this write created, in creation order.
    pub created_positions: Vec<String>,
    /// Every ticker that received a row, in first-write order.
    pub written_tickers: Vec<String>,
}

fn validate_bulk_row(row: &BulkStockRow) -> Result<()> {
    InsertInvestmentTransaction {
        investment_id: None,
        tx_type: row.tx_type.clone(),
        ticker: row.ticker.clone(),
        company_name: row
            .company_name
            .clone()
            .unwrap_or_else(|| row.ticker.clone()),
        quantity: row.quantity.clone(),
        price_per_unit: row.price_per_unit.clone(),
        currency: row.currency.clone(),
        transaction_date: row.transaction_date,
    }
    .validate()
}

/// The one bulk writer for stock transactions (ADR 0007): the MCP tool
/// `stock_transactions_create` and the CSV import both call it.
///
/// Two phases. Phase 1: every row is field-validated, nothing is written and
/// every failure is reported when any row is invalid. Phase 2: one SQL
/// transaction; rows are written in date order, buys before sells within a day
/// (a sell may need a position created earlier in the same batch); a row that
/// is identical to an existing transaction is skipped and reported unless it
/// has `allow_duplicate`; a business-rule failure (sell without position,
/// currency mismatch) rolls the WHOLE batch back and is reported as that
/// row's error.
///
/// With a `batch`, its `stock_import_batches` row is written in the same
/// transaction and every written row carries its id; no batch is recorded when
/// no row ends up written. Every row stores its `external_id`.
pub fn bulk_create_stock_transactions(
    conn: &mut rusqlite::Connection,
    rows: &[BulkStockRow],
    batch: Option<&NewStockImportBatch>,
) -> Result<BulkWriteReport> {
    let errors: Vec<BulkRowIssue> = rows
        .iter()
        .enumerate()
        .filter_map(|(index, row)| {
            validate_bulk_row(row).err().map(|e| BulkRowIssue {
                index,
                message: e.to_string(),
            })
        })
        .collect();
    if !errors.is_empty() {
        return Ok(BulkWriteReport {
            errors,
            ..BulkWriteReport::default()
        });
    }

    // Stable sort: rows of one day and direction keep the order they came in.
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by_key(|&i| {
        let row = &rows[i];
        (
            row.transaction_date,
            if row.tx_type.eq_ignore_ascii_case("buy") {
                0
            } else {
                1
            },
        )
    });

    let tx = conn.transaction()?;
    // The batch row goes in first: the transactions reference it.
    let batch_id = match batch {
        Some(batch) if !rows.is_empty() => {
            let id = Uuid::new_v4().to_string();
            tx.execute(
                "INSERT INTO stock_import_batches (id, file_name, source, trade_count)
                 VALUES (?1, ?2, ?3, 0)",
                rusqlite::params![id, batch.file_name, batch.source],
            )?;
            Some(id)
        }
        _ => None,
    };

    let mut report = BulkWriteReport::default();
    for &i in &order {
        let row = &rows[i];
        if !row.allow_duplicate {
            if let Some(reason) = dedup::find_duplicate_stock_transaction(
                &tx,
                &row.ticker.to_uppercase(),
                row.transaction_date,
                &row.tx_type.to_lowercase(),
                &row.quantity,
                &row.price_per_unit,
            )? {
                report.skipped_duplicates.push(BulkRowIssue {
                    index: i,
                    message: reason,
                });
                continue;
            }
        }
        let external_id = row
            .external_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty());
        match import_single_transaction_with_origin(
            &tx,
            &row.ticker,
            row.company_name.as_deref().unwrap_or(&row.ticker),
            &row.tx_type,
            &row.quantity,
            &row.price_per_unit,
            &row.currency,
            row.transaction_date,
            TransactionOrigin {
                import_batch_id: batch_id.as_deref(),
                external_id,
            },
        ) {
            Ok(written) => {
                report.created += 1;
                if written.position_created {
                    report.created_positions.push(written.ticker.clone());
                }
                if !report.written_tickers.contains(&written.ticker) {
                    report.written_tickers.push(written.ticker);
                }
            }
            Err(e) => {
                // Roll everything back (batch row included); report the failing row.
                drop(tx);
                return Ok(BulkWriteReport {
                    errors: vec![BulkRowIssue {
                        index: i,
                        message: e.to_string(),
                    }],
                    ..BulkWriteReport::default()
                });
            }
        }
    }

    if let Some(id) = batch_id {
        if report.created == 0 {
            tx.execute("DELETE FROM stock_import_batches WHERE id = ?1", [&id])?;
        } else {
            tx.execute(
                "UPDATE stock_import_batches SET trade_count = ?1 WHERE id = ?2",
                rusqlite::params![report.created as i64, id],
            )?;
            report.batch_id = Some(id);
        }
    }
    tx.commit()?;
    Ok(report)
}

/// Create investment with optional initial transaction
/// This is the main entry point for creating investments with transactions
pub fn create_investment_with_transaction(
    conn: &rusqlite::Connection,
    ticker: &str,
    company_name: &str,
    initial_transaction: Option<&InsertInvestmentTransaction>,
) -> Result<StockInvestment> {
    let ticker_upper = ticker.to_uppercase();

    // Determine currency from initial transaction (first transaction sets the currency)
    let currency = initial_transaction
        .map(|tx| tx.currency.as_str())
        .unwrap_or("CZK");

    // Get or create the investment
    let investment_id = get_or_create_investment(conn, &ticker_upper, company_name, currency)?;

    // Create initial transaction if provided
    if let Some(tx) = initial_transaction {
        create_transaction_internal(
            conn,
            &investment_id,
            &ticker_upper,
            company_name,
            &tx.tx_type,
            &tx.quantity,
            &tx.price_per_unit,
            &tx.currency,
            tx.transaction_date,
            TransactionOrigin::default(),
        )?;

        // Recalculate metrics from the transaction
        recalculate_investment_metrics(conn, &investment_id)?;
    }

    // Return the investment
    get_investment_by_id(conn, &investment_id)
}

/// Add a transaction to an existing investment
/// Validates that transaction currency matches investment currency
pub fn add_transaction_to_investment(
    conn: &rusqlite::Connection,
    investment_id: &str,
    data: &InsertInvestmentTransaction,
) -> Result<InvestmentTransaction> {
    // Get investment info including currency
    let (ticker, company_name, inv_currency): (String, String, String) = conn.query_row(
        "SELECT ticker, company_name, currency FROM stock_investments WHERE id = ?1",
        [investment_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    )?;

    // Validate currency matches
    if inv_currency.to_uppercase() != data.currency.to_uppercase() {
        return Err(AppError::Validation(format!(
            "Currency mismatch: {} uses {} but transaction is in {}. All transactions for a stock must use the same currency.",
            ticker, inv_currency, data.currency
        )));
    }

    ensure_sell_within_holdings(
        conn,
        "investment_transactions",
        investment_id,
        &data.tx_type,
        &data.quantity,
        data.transaction_date,
    )?;

    let tx = create_transaction_internal(
        conn,
        investment_id,
        &ticker,
        &company_name,
        &data.tx_type,
        &data.quantity,
        &data.price_per_unit,
        &data.currency,
        data.transaction_date,
        TransactionOrigin::default(),
    )?;

    recalculate_investment_metrics(conn, investment_id)?;

    Ok(tx)
}

/// Quantity of `investment_id` held at the end of `date` (buys and sells up to
/// and including that day).
pub fn held_quantity_at(
    conn: &rusqlite::Connection,
    tx_table: &str,
    investment_id: &str,
    date: i64,
) -> Result<f64> {
    let held: f64 = conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(CASE WHEN type = 'buy' THEN CAST(quantity AS REAL)
                                      ELSE -CAST(quantity AS REAL) END), 0.0)
             FROM {tx_table} WHERE investment_id = ?1 AND transaction_date <= ?2"
        ),
        rusqlite::params![investment_id, date],
        |row| row.get(0),
    )?;
    Ok(held.max(0.0))
}

/// A sell may not exceed what is held on its date. Shared by the
/// stock and crypto write paths; imports keep their own ordering semantics.
pub fn ensure_sell_within_holdings(
    conn: &rusqlite::Connection,
    tx_table: &str,
    investment_id: &str,
    tx_type: &str,
    quantity: &str,
    date: i64,
) -> Result<()> {
    if tx_type != "sell" {
        return Ok(());
    }
    let qty: f64 = quantity.trim().parse().unwrap_or(0.0);
    let held = held_quantity_at(conn, tx_table, investment_id, date)?;
    if qty > held + 1e-9 {
        return Err(AppError::Validation(
            "validation.sellExceedsHoldings".into(),
        ));
    }
    Ok(())
}

/// Get value history for a specific stock ticker
pub fn get_value_history(
    conn: &rusqlite::Connection,
    ticker: &str,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<crate::models::TickerValueHistory>> {
    let ticker_upper = ticker.to_uppercase();

    let mut query = String::from(
        "SELECT ticker, recorded_at, value_czk, quantity, price, currency 
         FROM stock_value_history 
         WHERE ticker = ?",
    );

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    params.push(Box::new(ticker_upper));

    if let Some(start) = start_date {
        query.push_str(" AND recorded_at >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_date {
        query.push_str(" AND recorded_at <= ?");
        params.push(Box::new(end));
    }

    query.push_str(" ORDER BY recorded_at DESC");

    let mut stmt = conn.prepare(&query)?;

    let rows = stmt.query_map(rusqlite::params_from_iter(params), |row| {
        Ok(crate::models::TickerValueHistory {
            ticker: row.get(0)?,
            recorded_at: row.get(1)?,
            value_czk: row.get(2)?,
            quantity: row.get(3)?,
            price: row.get(4)?,
            currency: row.get(5)?,
        })
    })?;

    let histories: Vec<crate::models::TickerValueHistory> = rows.filter_map(|r| r.ok()).collect();
    Ok(histories)
}

/// Convert a Unix timestamp to a "YYYY-MM-DD" date string (UTC).
fn ts_to_date_str(ts: i64) -> String {
    DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// Find the most recent (value_czk, quantity) for a ticker at or before `target_ts`.
///
/// Uses binary search — O(log N) — so entries must be sorted ascending by timestamp
/// (as guaranteed by the SQL `ORDER BY ticker, recorded_at ASC` in `compute_twr_for_tickers`).
fn lookup_ticker_at(
    history: &HashMap<String, Vec<(i64, f64, f64)>>,
    ticker: &str,
    target_ts: i64,
) -> (f64, f64) {
    history
        .get(ticker)
        .and_then(|entries| {
            // partition_point returns the index of the first element where ts > target_ts,
            // so the last element at or before target_ts is at idx-1 (if idx > 0).
            let idx = entries.partition_point(|(ts, _, _)| *ts <= target_ts);
            if idx > 0 {
                let (_, val, qty) = entries[idx - 1];
                Some((val, qty))
            } else {
                None
            }
        })
        .unwrap_or((0.0, 0.0))
}

/// Compute daily chain-linked time-weighted return for a group of tickers.
///
/// Returns one `TwrDataPoint` per calendar day from `from_ts` to `to_ts` inclusive.
/// The first point is always `{ date: from_ts, twr: 0.0 }`.
/// Cash flows are estimated from quantity changes in `stock_value_history`.
///
/// `from_ts` and `to_ts` must be Unix timestamps (seconds, midnight UTC).
pub fn compute_twr_for_tickers(
    conn: &rusqlite::Connection,
    tickers: &[String],
    from_ts: i64,
    to_ts: i64,
) -> Result<Vec<crate::models::TwrDataPoint>> {
    if tickers.is_empty() {
        return Ok(vec![crate::models::TwrDataPoint {
            date: ts_to_date_str(from_ts),
            twr: 0.0,
        }]);
    }

    // Build SQL with two IN clause copies (tickers appear twice: once for main range, once for pre-range lookup)
    // params: ?1=from_ts, ?2=to_ts, ?3..?(n+2)=tickers (main range), ?(n+3)..?(2n+2)=tickers (pre-range)
    let n = tickers.len();
    let main_placeholders: Vec<String> = (3..=n + 2).map(|i| format!("?{i}")).collect();
    let pre_placeholders: Vec<String> = (n + 3..=2 * n + 2).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT ticker, recorded_at, CAST(value_czk AS REAL), CAST(quantity AS REAL) \
         FROM stock_value_history \
         WHERE ticker IN ({main}) AND recorded_at >= ?1 AND recorded_at <= ?2 \
         UNION ALL \
         SELECT s.ticker, s.recorded_at, CAST(s.value_czk AS REAL), CAST(s.quantity AS REAL) \
         FROM stock_value_history s \
         WHERE s.ticker IN ({pre}) \
           AND s.recorded_at = ( \
               SELECT MAX(recorded_at) FROM stock_value_history \
               WHERE ticker = s.ticker AND recorded_at < ?1 \
           ) \
         ORDER BY ticker, recorded_at ASC",
        main = main_placeholders.join(", "),
        pre = pre_placeholders.join(", "),
    );

    let params: Vec<Value> = std::iter::once(Value::Integer(from_ts))
        .chain(std::iter::once(Value::Integer(to_ts)))
        .chain(tickers.iter().map(|t| Value::Text(t.clone()))) // main range
        .chain(tickers.iter().map(|t| Value::Text(t.clone()))) // pre-range
        .collect();

    let mut history: HashMap<String, Vec<(i64, f64, f64)>> = HashMap::new();
    {
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_from_iter(params), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
                row.get::<_, f64>(3)?,
            ))
        })?;
        for row in rows {
            let (ticker, ts, val, qty) = row?;
            history.entry(ticker).or_default().push((ts, val, qty));
        }
    }

    // Date inputs fire per keystroke, so transient absurd ranges reach this
    // function (typing "2024" passes through year 0002 → from_ts ≈ -62e9).
    // Clamp the loop to the actual data span: nothing exists to chain-link
    // before the first known row or after the last one, and an unclamped loop
    // runs for millions of iterations while holding the global DB mutex.
    let earliest = history
        .values()
        .filter_map(|rows| rows.first().map(|(ts, _, _)| *ts))
        .min();
    let latest = history
        .values()
        .filter_map(|rows| rows.last().map(|(ts, _, _)| *ts))
        .max();
    let (Some(earliest), Some(latest)) = (earliest, latest) else {
        return Ok(vec![crate::models::TwrDataPoint {
            date: ts_to_date_str(from_ts),
            twr: 0.0,
        }]);
    };
    let from_ts = from_ts.max(earliest);
    let to_ts = to_ts.min(latest).max(from_ts);

    // Determine emit interval so we never return more than MAX_POINTS data points.
    // Chain-linking still runs every day for accuracy; we just skip emitting most points.
    const MAX_POINTS: i64 = 300;
    let total_days = (to_ts - from_ts) / 86400;
    let emit_every = ((total_days + MAX_POINTS - 1) / MAX_POINTS).max(1);

    let mut result = vec![crate::models::TwrDataPoint {
        date: ts_to_date_str(from_ts),
        twr: 0.0,
    }];
    let mut twr_factor = 1.0f64;
    let mut day = from_ts + 86400;
    let mut day_index: i64 = 0;

    while day <= to_ts {
        day_index += 1;
        let prev_day = day - 86400;
        let mut v_curr = 0.0f64;
        let mut v_prev = 0.0f64;
        let mut cf = 0.0f64;

        for ticker in tickers {
            let (val_curr, qty_curr) = lookup_ticker_at(&history, ticker, day);
            let (val_prev, qty_prev) = lookup_ticker_at(&history, ticker, prev_day);
            v_curr += val_curr;
            v_prev += val_prev;

            let delta_qty = qty_curr - qty_prev;
            if delta_qty.abs() > 1e-9 {
                let price_czk = if qty_curr > 1e-9 {
                    val_curr / qty_curr
                } else if qty_prev > 1e-9 {
                    val_prev / qty_prev
                } else {
                    0.0
                };
                cf += delta_qty * price_czk;
            }
        }

        let denominator = v_prev + cf;
        if denominator > 1e-9 {
            let daily_r = (v_curr - v_prev - cf) / denominator;
            twr_factor *= 1.0 + daily_r;
        }

        // Emit this point if it falls on the sampling interval or is the last day.
        if day_index % emit_every == 0 || day == to_ts {
            result.push(crate::models::TwrDataPoint {
                date: ts_to_date_str(day),
                twr: (twr_factor - 1.0) * 100.0,
            });
        }

        day += 86400;
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_get_value_history_sql_construction() {
        let conn = Connection::open_in_memory().unwrap();

        // Create table
        conn.execute(
            "CREATE TABLE stock_value_history (
                ticker TEXT NOT NULL,
                recorded_at INTEGER NOT NULL,
                value_czk TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price TEXT NOT NULL,
                currency TEXT NOT NULL
            )",
            [],
        )
        .unwrap();

        // Insert dummy data
        conn.execute(
            "INSERT INTO stock_value_history (ticker, recorded_at, value_czk, quantity, price, currency)
             VALUES ('AAPL', 1000, '1000', '10', '100', 'USD')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO stock_value_history (ticker, recorded_at, value_czk, quantity, price, currency)
             VALUES ('AAPL', 2000, '2000', '10', '200', 'USD')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO stock_value_history (ticker, recorded_at, value_czk, quantity, price, currency)
             VALUES ('AAPL', 3000, '3000', '10', '300', 'USD')",
            [],
        ).unwrap();

        // Test 1: No dates (All) - Should NOT fail
        let history = get_value_history(&conn, "AAPL", None, None).unwrap();
        assert_eq!(history.len(), 3);

        // Test 2: Start date only
        let history = get_value_history(&conn, "AAPL", Some(2000), None).unwrap();
        assert_eq!(history.len(), 2); // 2000 and 3000

        // Test 3: End date only
        let history = get_value_history(&conn, "AAPL", None, Some(2000)).unwrap();
        assert_eq!(history.len(), 2); // 1000 and 2000

        // Test 4: Both dates
        let history = get_value_history(&conn, "AAPL", Some(2000), Some(2500)).unwrap();
        assert_eq!(history.len(), 1); // Only 2000
    }

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'CZK'
            );

            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );

            CREATE TABLE stock_import_batches (
                id TEXT PRIMARY KEY,
                file_name TEXT NOT NULL,
                source TEXT NOT NULL,
                trade_count INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                import_batch_id TEXT REFERENCES stock_import_batches(id) ON DELETE SET NULL,
                external_id TEXT
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn tx(investment_id: &str, tx_type: &str, qty: &str, date: i64) -> InsertInvestmentTransaction {
        InsertInvestmentTransaction {
            investment_id: Some(investment_id.to_string()),
            tx_type: tx_type.to_string(),
            ticker: "AAPL".to_string(),
            company_name: "Apple Inc.".to_string(),
            quantity: qty.to_string(),
            price_per_unit: "100".to_string(),
            currency: "USD".to_string(),
            transaction_date: date,
        }
    }

    /// a sell above the quantity held on its date is rejected at the
    /// trust boundary instead of silently corrupting the cost basis.
    #[test]
    fn sell_above_holdings_is_rejected() {
        let conn = setup_test_db();
        let id = get_or_create_investment(&conn, "AAPL", "Apple Inc.", "USD").expect("create");
        add_transaction_to_investment(&conn, &id, &tx(&id, "buy", "10", 1_700_000_000))
            .expect("buy");

        let err = add_transaction_to_investment(&conn, &id, &tx(&id, "sell", "15", 1_700_086_400))
            .expect_err("oversell");
        assert!(matches!(err, AppError::Validation(k) if k == "validation.sellExceedsHoldings"));

        // A sell dated BEFORE the buy holds nothing yet either.
        let err = add_transaction_to_investment(&conn, &id, &tx(&id, "sell", "1", 1_699_900_000))
            .expect_err("sell before buy");
        assert!(matches!(err, AppError::Validation(_)));

        // Selling exactly what is held (same day as the buy) is fine.
        add_transaction_to_investment(&conn, &id, &tx(&id, "sell", "10", 1_700_000_000))
            .expect("sell all");
        assert!(
            (held_quantity_at(&conn, "investment_transactions", &id, 1_700_000_000).expect("held"))
                .abs()
                < 1e-9
        );
    }

    /// deleting several rows at once reports one rebuild start per
    /// ticker (its earliest deleted date), and the bookkeeping matches the
    /// single-row delete: emptied investments go, the others are recalculated.
    #[test]
    fn bulk_delete_reports_one_start_date_per_ticker_and_fixes_the_positions() {
        let conn = setup_test_db();
        let aapl = get_or_create_investment(&conn, "AAPL", "Apple Inc.", "USD").expect("aapl");
        let msft = get_or_create_investment(&conn, "MSFT", "Microsoft", "USD").expect("msft");
        let mut ids = Vec::new();
        for (investment, ticker, kind, qty, date) in [
            (&aapl, "AAPL", "buy", "10", 1_700_000_000_i64),
            (&aapl, "AAPL", "buy", "5", 1_700_100_000),
            (&aapl, "AAPL", "buy", "2", 1_700_200_000),
            (&msft, "MSFT", "buy", "4", 1_700_300_000),
            (&msft, "MSFT", "sell", "1", 1_700_400_000),
        ] {
            let mut data = tx(investment, kind, qty, date);
            data.ticker = ticker.to_string();
            ids.push(
                add_transaction_to_investment(&conn, investment, &data)
                    .expect("tx")
                    .id,
            );
        }

        // Two AAPL rows (the earlier one is not first in the list) and both MSFT rows
        let to_delete = vec![
            ids[2].clone(),
            ids[0].clone(),
            ids[3].clone(),
            ids[4].clone(),
        ];
        let jobs = delete_transactions(&conn, &to_delete).expect("bulk delete");

        assert_eq!(
            jobs,
            vec![
                ("AAPL".to_string(), 1_700_000_000),
                ("MSFT".to_string(), 1_700_300_000)
            ],
            "one start date per ticker, the earliest deleted"
        );
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM investment_transactions", [], |r| {
                r.get(0)
            })
            .expect("count");
        assert_eq!(remaining, 1);
        // MSFT lost every transaction and is gone; AAPL keeps its 5-share row
        let investments: Vec<String> = conn
            .prepare("SELECT ticker FROM stock_investments ORDER BY ticker")
            .expect("prepare")
            .query_map([], |r| r.get(0))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        assert_eq!(investments, vec!["AAPL".to_string()]);
        let quantity: String = conn
            .query_row(
                "SELECT quantity FROM stock_investments WHERE ticker = 'AAPL'",
                [],
                |r| r.get(0),
            )
            .expect("quantity");
        assert_eq!(quantity, "5");
    }

    #[test]
    fn bulk_delete_with_an_unknown_id_deletes_nothing() {
        let conn = setup_test_db();
        let id = get_or_create_investment(&conn, "AAPL", "Apple Inc.", "USD").expect("create");
        let real = add_transaction_to_investment(&conn, &id, &tx(&id, "buy", "10", 1_700_000_000))
            .expect("tx")
            .id;

        let err =
            delete_transactions(&conn, &[real, "missing".to_string()]).expect_err("unknown id");
        assert!(matches!(err, AppError::NotFound(_)));
        let remaining: i64 = conn
            .query_row("SELECT COUNT(*) FROM investment_transactions", [], |r| {
                r.get(0)
            })
            .expect("count");
        assert_eq!(remaining, 1, "the lookup fails before anything is deleted");
        assert!(delete_transactions(&conn, &[]).expect("empty").is_empty());
    }

    #[test]
    fn test_get_or_create_investment_creates_new() {
        let conn = setup_test_db();
        let id = get_or_create_investment(&conn, "AAPL", "Apple Inc.", "USD").expect("create");
        assert!(!id.is_empty());

        // Second call returns same id
        let id2 =
            get_or_create_investment(&conn, "AAPL", "Apple Inc.", "USD").expect("get existing");
        assert_eq!(id, id2);
    }

    #[test]
    fn test_get_or_create_investment_upcases_ticker() {
        let conn = setup_test_db();
        let id1 = get_or_create_investment(&conn, "aapl", "Apple", "USD").expect("lowercase");
        let id2 = get_or_create_investment(&conn, "AAPL", "Apple", "USD").expect("uppercase");
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_recalculate_metrics_buy_only() {
        let conn = setup_test_db();
        let investment_id =
            get_or_create_investment(&conn, "MSFT", "Microsoft", "USD").expect("create");

        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx1', ?1, 'buy', 'MSFT', 'Microsoft', '10', '300', 'USD', 0, 0)",
            [&investment_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx2', ?1, 'buy', 'MSFT', 'Microsoft', '5', '360', 'USD', 0, 0)",
            [&investment_id],
        )
        .unwrap();

        recalculate_investment_metrics(&conn, &investment_id).expect("recalc");

        let inv = get_investment_by_id(&conn, &investment_id).expect("get");
        let qty: f64 = inv.quantity.parse().unwrap();
        let avg: f64 = inv.average_price.parse().unwrap();
        assert_eq!(qty, 15.0);
        // avg = (10*300 + 5*360) / 15 = 4800/15 = 320
        assert!((avg - 320.0).abs() < 0.01);
    }

    #[test]
    fn test_recalculate_metrics_buy_then_sell() {
        let conn = setup_test_db();
        let investment_id =
            get_or_create_investment(&conn, "TSLA", "Tesla", "USD").expect("create");

        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx1', ?1, 'buy', 'TSLA', 'Tesla', '20', '200', 'USD', 0, 0)",
            [&investment_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx2', ?1, 'sell', 'TSLA', 'Tesla', '8', '250', 'USD', 1, 0)",
            [&investment_id],
        )
        .unwrap();

        recalculate_investment_metrics(&conn, &investment_id).expect("recalc");

        let inv = get_investment_by_id(&conn, &investment_id).expect("get");
        let qty: f64 = inv.quantity.parse().unwrap();
        assert_eq!(qty, 12.0);
        let avg: f64 = inv.average_price.parse().unwrap();
        assert!((avg - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_recalculate_metrics_sell_then_rebuy() {
        // buy 10@100, sell 10, buy 10@200 -> average should reset to 200, not blend with the past
        let conn = setup_test_db();
        let investment_id =
            get_or_create_investment(&conn, "SNAP", "Snap Inc.", "USD").expect("create");

        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx1', ?1, 'buy', 'SNAP', 'Snap Inc.', '10', '100', 'USD', 0, 0)",
            [&investment_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx2', ?1, 'sell', 'SNAP', 'Snap Inc.', '10', '150', 'USD', 1, 0)",
            [&investment_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx3', ?1, 'buy', 'SNAP', 'Snap Inc.', '10', '200', 'USD', 2, 0)",
            [&investment_id],
        )
        .unwrap();

        recalculate_investment_metrics(&conn, &investment_id).expect("recalc");

        let inv = get_investment_by_id(&conn, &investment_id).expect("get");
        let qty: f64 = inv.quantity.parse().unwrap();
        let avg: f64 = inv.average_price.parse().unwrap();
        assert_eq!(qty, 10.0);
        assert!((avg - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_recalculate_metrics_uses_transaction_date_not_insertion_order() {
        // Same buy/sell/buy sequence as test_recalculate_metrics_sell_then_rebuy (qty 10,
        // avg 200), but the rows are INSERTed out of chronological order. If the ORDER BY
        // clause were dropped, the query would fall back to insertion (rowid) order and
        // produce a different (wrong) result — this exercises that the ORDER BY is load-bearing.
        let conn = setup_test_db();
        let investment_id =
            get_or_create_investment(&conn, "SNAP", "Snap Inc.", "USD").expect("create");

        // Inserted first, but chronologically last (transaction_date = 2)
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx3', ?1, 'buy', 'SNAP', 'Snap Inc.', '10', '200', 'USD', 2, 0)",
            [&investment_id],
        )
        .unwrap();
        // Inserted second, but chronologically first (transaction_date = 0)
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx1', ?1, 'buy', 'SNAP', 'Snap Inc.', '10', '100', 'USD', 0, 0)",
            [&investment_id],
        )
        .unwrap();
        // Inserted third, but chronologically in the middle (transaction_date = 1)
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx2', ?1, 'sell', 'SNAP', 'Snap Inc.', '10', '150', 'USD', 1, 0)",
            [&investment_id],
        )
        .unwrap();

        recalculate_investment_metrics(&conn, &investment_id).expect("recalc");

        let inv = get_investment_by_id(&conn, &investment_id).expect("get");
        let qty: f64 = inv.quantity.parse().unwrap();
        let avg: f64 = inv.average_price.parse().unwrap();
        assert_eq!(qty, 10.0);
        assert!(
            (avg - 200.0).abs() < 0.01,
            "expected avg 200 from chronological ordering, got {avg} — ORDER BY may be missing"
        );
    }

    #[test]
    fn test_recalculate_metrics_partial_sell() {
        // buy 10@100, sell 5, buy 5@200 -> average should be a moving average: 150
        let conn = setup_test_db();
        let investment_id =
            get_or_create_investment(&conn, "AMZN", "Amazon", "USD").expect("create");

        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx1', ?1, 'buy', 'AMZN', 'Amazon', '10', '100', 'USD', 0, 0)",
            [&investment_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx2', ?1, 'sell', 'AMZN', 'Amazon', '5', '150', 'USD', 1, 0)",
            [&investment_id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx3', ?1, 'buy', 'AMZN', 'Amazon', '5', '200', 'USD', 2, 0)",
            [&investment_id],
        )
        .unwrap();

        recalculate_investment_metrics(&conn, &investment_id).expect("recalc");

        let inv = get_investment_by_id(&conn, &investment_id).expect("get");
        let qty: f64 = inv.quantity.parse().unwrap();
        let avg: f64 = inv.average_price.parse().unwrap();
        assert_eq!(qty, 10.0);
        assert!((avg - 150.0).abs() < 0.01);
    }

    #[test]
    fn test_recalculate_all_investment_metrics_corrects_all_investments() {
        // Two investments, both seeded with stale/incorrect stored metrics (as the old
        // lifetime-average bug would have left behind). A one-time recalculation over
        // all stock_investments should self-heal both from their transaction history.
        let conn = setup_test_db();

        let inv1 =
            get_or_create_investment(&conn, "SNAP", "Snap Inc.", "USD").expect("create inv1");
        let inv2 = get_or_create_investment(&conn, "AMZN", "Amazon", "USD").expect("create inv2");

        conn.execute(
            "UPDATE stock_investments SET quantity = '999' WHERE id = ?1",
            [&inv1],
        )
        .unwrap();
        conn.execute(
            "UPDATE stock_investments SET quantity = '999' WHERE id = ?1",
            [&inv2],
        )
        .unwrap();

        // inv1: sell-then-rebuy -> qty 10, avg 200
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx1', ?1, 'buy', 'SNAP', 'Snap Inc.', '10', '100', 'USD', 0, 0)",
            [&inv1],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx2', ?1, 'sell', 'SNAP', 'Snap Inc.', '10', '150', 'USD', 1, 0)",
            [&inv1],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx3', ?1, 'buy', 'SNAP', 'Snap Inc.', '10', '200', 'USD', 2, 0)",
            [&inv1],
        )
        .unwrap();

        // inv2: partial-sell -> qty 10, avg 150
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx4', ?1, 'buy', 'AMZN', 'Amazon', '10', '100', 'USD', 0, 0)",
            [&inv2],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx5', ?1, 'sell', 'AMZN', 'Amazon', '5', '150', 'USD', 1, 0)",
            [&inv2],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO investment_transactions
             (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES ('tx6', ?1, 'buy', 'AMZN', 'Amazon', '5', '200', 'USD', 2, 0)",
            [&inv2],
        )
        .unwrap();

        recalculate_all_investment_metrics(&conn).expect("recalc all");

        let got1 = get_investment_by_id(&conn, &inv1).expect("get inv1");
        let qty1: f64 = got1.quantity.parse().unwrap();
        let avg1: f64 = got1.average_price.parse().unwrap();
        assert_eq!(qty1, 10.0);
        assert!((avg1 - 200.0).abs() < 0.01);

        let got2 = get_investment_by_id(&conn, &inv2).expect("get inv2");
        let qty2: f64 = got2.quantity.parse().unwrap();
        let avg2: f64 = got2.average_price.parse().unwrap();
        assert_eq!(qty2, 10.0);
        assert!((avg2 - 150.0).abs() < 0.01);
    }

    #[test]
    fn test_recalculate_all_investment_metrics_preserves_transaction_less_investment() {
        // Defensive: a position with zero rows in investment_transactions (legacy
        // data before migration 008 backfills a synthetic buy). The startup sweep
        // runs on every DB open, so it must leave the stored quantity untouched
        // rather than zeroing it out. Average price derives to 0 — there is no
        // transaction record to derive from.
        let conn = setup_test_db();
        let investment_id = get_or_create_investment(&conn, "IBM", "IBM", "USD").expect("create");
        conn.execute(
            "UPDATE stock_investments SET quantity = '42' WHERE id = ?1",
            [&investment_id],
        )
        .unwrap();

        recalculate_all_investment_metrics(&conn).expect("recalc all");

        let inv = get_investment_by_id(&conn, &investment_id).expect("get");
        let qty: f64 = inv.quantity.parse().unwrap();
        assert_eq!(qty, 42.0);
        let avg: f64 = inv.average_price.parse().unwrap();
        assert_eq!(avg, 0.0);
    }

    #[test]
    fn test_import_single_transaction_invalid_type() {
        let conn = setup_test_db();
        let result =
            import_single_transaction(&conn, "AAPL", "Apple", "hold", "10", "150", "USD", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_import_single_transaction_sell_without_position() {
        let conn = setup_test_db();
        let result =
            import_single_transaction(&conn, "GOOG", "Google", "sell", "5", "100", "USD", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_import_single_transaction_buy_creates_investment() {
        let conn = setup_test_db();
        let result = import_single_transaction(
            &conn,
            "nvda",
            "NVIDIA",
            "buy",
            "3",
            "500",
            "USD",
            1_700_000_000,
        );
        assert!(result.is_ok());
        let (desc, _date, ticker) = result.unwrap();
        assert_eq!(ticker, "NVDA");
        assert!(desc.contains("BUY"));
    }

    #[test]
    fn test_import_single_transaction_currency_mismatch() {
        let conn = setup_test_db();
        import_single_transaction(&conn, "AMD", "AMD", "buy", "10", "100", "USD", 0).unwrap();
        let result = import_single_transaction(&conn, "AMD", "AMD", "buy", "5", "90", "EUR", 1);
        assert!(result.is_err());
    }

    fn setup_twr_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE stock_value_history (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL,
                recorded_at INTEGER NOT NULL,
                value_czk TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price TEXT NOT NULL,
                currency TEXT NOT NULL,
                UNIQUE(ticker, recorded_at)
            );",
        )
        .unwrap();
        conn
    }

    #[test]
    fn test_twr_buy_does_not_inflate_return() {
        // Day 0: 10 AAPL @ 100 CZK = 1000 CZK
        // Day 1: buy 10 more @ 100 CZK → 20 @ 100 = 2000 (CF = 1000, pure cash-in, no price gain)
        // Day 2: price rises to 110 → 20 @ 110 = 2200 (no transaction, pure gain = 10%)
        // Expected TWR: day0=0%, day1=0%, day2=10%
        let conn = setup_twr_db();
        let day0: i64 = 1_700_000_000 / 86400 * 86400;
        let day1 = day0 + 86400;
        let day2 = day0 + 2 * 86400;
        conn.execute_batch(&format!(
            "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency) VALUES
             ('a', 'AAPL', {day0}, '1000.0', '10.0', '100.0', 'USD'),
             ('b', 'AAPL', {day1}, '2000.0', '20.0', '100.0', 'USD'),
             ('c', 'AAPL', {day2}, '2200.0', '20.0', '110.0', 'USD');"
        ))
        .unwrap();

        let result = compute_twr_for_tickers(&conn, &["AAPL".to_string()], day0, day2).unwrap();
        assert_eq!(result.len(), 3);
        assert!((result[0].twr - 0.0).abs() < 0.01, "day0 should be 0%");
        assert!(
            (result[1].twr - 0.0).abs() < 0.01,
            "buy should not inflate TWR"
        );
        assert!(
            (result[2].twr - 10.0).abs() < 0.1,
            "price gain should be ~10%"
        );
    }

    #[test]
    fn test_twr_pure_price_appreciation() {
        // No transactions: daily chain TWR equals simple cumulative return
        let conn = setup_twr_db();
        let day0: i64 = 1_700_200_000 / 86400 * 86400;
        let day1 = day0 + 86400;
        let day2 = day0 + 2 * 86400;
        conn.execute_batch(&format!(
            "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency) VALUES
             ('a', 'AAPL', {day0}, '1000.0', '10.0', '100.0', 'USD'),
             ('b', 'AAPL', {day1}, '1050.0', '10.0', '105.0', 'USD'),
             ('c', 'AAPL', {day2}, '1100.0', '10.0', '110.0', 'USD');"
        ))
        .unwrap();

        let result = compute_twr_for_tickers(&conn, &["AAPL".to_string()], day0, day2).unwrap();
        assert_eq!(result.len(), 3);
        assert!((result[0].twr - 0.0).abs() < 0.01);
        // day1: (1050-1000)/1000 = 5%
        assert!((result[1].twr - 5.0).abs() < 0.1);
        // day2: 1.05 * (1100/1050) - 1 ≈ 10%
        assert!((result[2].twr - 10.0).abs() < 0.5);
    }

    #[test]
    fn test_twr_empty_tickers_returns_single_zero_point() {
        let conn = setup_twr_db();
        let day0: i64 = 1_700_400_000 / 86400 * 86400;
        let day1 = day0 + 86400;
        let result = compute_twr_for_tickers(&conn, &[], day0, day1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].twr, 0.0);
    }

    // Date inputs fire per keystroke, so the command receives transient
    // absurd ranges (year 0002 while typing "2024" → from_ts ≈ -62e9). The
    // day loop must be clamped to the actual data span, or it runs for
    // ~738 000 iterations per keystroke while holding the global DB mutex —
    // the observed app-wide freeze.
    #[test]
    fn test_twr_range_is_clamped_to_data_span() {
        let conn = setup_twr_db();
        let day0: i64 = 1_700_000_000 / 86400 * 86400;
        let day1 = day0 + 86400;
        let day2 = day0 + 2 * 86400;
        conn.execute_batch(&format!(
            "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency) VALUES
             ('a', 'AAPL', {day0}, '1000.0', '10.0', '100.0', 'USD'),
             ('b', 'AAPL', {day1}, '1050.0', '10.0', '105.0', 'USD'),
             ('c', 'AAPL', {day2}, '1100.0', '10.0', '110.0', 'USD');"
        ))
        .unwrap();

        // Year ~0002 to year ~4064: without clamping this loop would run
        // ~24 million days. The result must start at the data span instead.
        let absurd_from: i64 = -62_000_000_000;
        let absurd_to: i64 = 66_000_000_000;
        let result =
            compute_twr_for_tickers(&conn, &["AAPL".to_string()], absurd_from, absurd_to).unwrap();

        assert_eq!(result[0].date, ts_to_date_str(day0), "starts at first data");
        let last = result.last().unwrap();
        assert!(
            (last.twr - 10.0).abs() < 0.5,
            "final TWR equals the in-range computation, got {}",
            last.twr
        );
        assert!(result.len() <= 301);
    }

    #[test]
    fn test_twr_no_history_rows_returns_single_flat_point() {
        let conn = setup_twr_db();
        // Ticker exists but has no value-history rows in or before the range:
        // no loop, one flat point (nothing can be computed).
        let result = compute_twr_for_tickers(
            &conn,
            &["AAPL".to_string()],
            -62_000_000_000,
            66_000_000_000,
        )
        .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].twr, 0.0);
    }

    // ---- the shared bulk writer (ADR 0007) --------------------------------

    const DAY: i64 = 86_400;
    const D0: i64 = 1_700_000_000 - 1_700_000_000 % DAY;

    fn bulk_row(ticker: &str, kind: &str, qty: &str, price: &str, date: i64) -> BulkStockRow {
        BulkStockRow {
            ticker: ticker.to_string(),
            company_name: None,
            tx_type: kind.to_string(),
            quantity: qty.to_string(),
            price_per_unit: price.to_string(),
            currency: "USD".to_string(),
            transaction_date: date,
            external_id: None,
            allow_duplicate: false,
        }
    }

    fn batch(file: &str) -> NewStockImportBatch {
        NewStockImportBatch {
            file_name: file.to_string(),
            source: "xtb".to_string(),
        }
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    /// `(import_batch_id, external_id)` of every transaction, oldest rowid first.
    fn provenance(conn: &Connection) -> Vec<(Option<String>, Option<String>)> {
        let mut stmt = conn
            .prepare(
                "SELECT import_batch_id, external_id FROM investment_transactions ORDER BY rowid",
            )
            .unwrap();
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    #[test]
    fn a_bulk_write_with_a_batch_records_it_and_stamps_every_row() {
        let mut conn = setup_test_db();
        let mut a = bulk_row("aapl", "buy", "10", "100", D0);
        a.external_id = Some("xtb:111".into());
        let mut b = bulk_row("aapl", "sell", "4", "110", D0 + DAY);
        b.external_id = Some("xtb:112".into());
        let c = bulk_row("msft", "buy", "2", "300", D0 + DAY); // no broker id

        let report =
            bulk_create_stock_transactions(&mut conn, &[a, b, c], Some(&batch("xtb.csv"))).unwrap();

        assert_eq!(report.created, 3, "{report:?}");
        assert!(report.errors.is_empty());
        let batch_id = report.batch_id.clone().expect("a batch is recorded");
        assert_eq!(report.created_positions, vec!["AAPL", "MSFT"]);
        assert_eq!(report.written_tickers, vec!["AAPL", "MSFT"]);

        let (file, source, trades): (String, String, i64) = conn
            .query_row(
                "SELECT file_name, source, trade_count FROM stock_import_batches WHERE id = ?1",
                [&batch_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(
            (file.as_str(), source.as_str(), trades),
            ("xtb.csv", "xtb", 3)
        );
        assert_eq!(count(&conn, "stock_import_batches"), 1);
        // Written in date order, buys first within a day: a, then the MSFT buy, then the sell.
        assert_eq!(
            provenance(&conn),
            vec![
                (Some(batch_id.clone()), Some("xtb:111".to_string())),
                (Some(batch_id.clone()), None),
                (Some(batch_id), Some("xtb:112".to_string())),
            ]
        );
    }

    #[test]
    fn a_bulk_write_without_a_batch_leaves_both_columns_empty() {
        let mut conn = setup_test_db();
        let report = bulk_create_stock_transactions(
            &mut conn,
            &[bulk_row("aapl", "buy", "1", "1", D0)],
            None,
        )
        .unwrap();
        assert_eq!(report.created, 1);
        assert_eq!(report.batch_id, None);
        assert_eq!(provenance(&conn), vec![(None, None)]);
        assert_eq!(count(&conn, "stock_import_batches"), 0);
    }

    #[test]
    fn a_blank_external_id_is_stored_as_none() {
        let mut conn = setup_test_db();
        let mut row = bulk_row("aapl", "buy", "1", "1", D0);
        row.external_id = Some("   ".into());
        bulk_create_stock_transactions(&mut conn, &[row], None).unwrap();
        assert_eq!(provenance(&conn), vec![(None, None)]);
    }

    #[test]
    fn identical_rows_are_skipped_unless_they_allow_a_duplicate() {
        let mut conn = setup_test_db();
        let first = bulk_row("aapl", "buy", "10", "100", D0);
        bulk_create_stock_transactions(&mut conn, std::slice::from_ref(&first), None).unwrap();

        let skipped =
            bulk_create_stock_transactions(&mut conn, std::slice::from_ref(&first), None).unwrap();
        assert_eq!(skipped.created, 0);
        assert_eq!(skipped.skipped_duplicates.len(), 1);
        assert_eq!(skipped.skipped_duplicates[0].index, 0);
        assert_eq!(count(&conn, "investment_transactions"), 1);

        let mut forced = first;
        forced.allow_duplicate = true;
        let written = bulk_create_stock_transactions(&mut conn, &[forced], None).unwrap();
        assert_eq!(written.created, 1);
        assert!(written.skipped_duplicates.is_empty());
        assert_eq!(count(&conn, "investment_transactions"), 2);
    }

    #[test]
    fn no_batch_is_recorded_when_every_row_is_skipped() {
        let mut conn = setup_test_db();
        let row = bulk_row("aapl", "buy", "10", "100", D0);
        bulk_create_stock_transactions(&mut conn, std::slice::from_ref(&row), None).unwrap();

        let report =
            bulk_create_stock_transactions(&mut conn, &[row], Some(&batch("again.csv"))).unwrap();

        assert_eq!(report.created, 0);
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert_eq!(report.batch_id, None);
        assert_eq!(
            count(&conn, "stock_import_batches"),
            0,
            "no empty batch is left behind"
        );
    }

    #[test]
    fn an_empty_write_records_no_batch() {
        let mut conn = setup_test_db();
        let report =
            bulk_create_stock_transactions(&mut conn, &[], Some(&batch("empty.csv"))).unwrap();
        assert_eq!(report, BulkWriteReport::default());
        assert_eq!(count(&conn, "stock_import_batches"), 0);
    }

    #[test]
    fn a_rule_failure_rolls_back_the_rows_and_the_batch() {
        let mut conn = setup_test_db();
        let rows = [
            bulk_row("aapl", "buy", "10", "100", D0),
            bulk_row("msft", "sell", "1", "300", D0 + DAY), // no MSFT position
        ];

        let report =
            bulk_create_stock_transactions(&mut conn, &rows, Some(&batch("bad.csv"))).unwrap();

        assert_eq!(report.created, 0);
        assert_eq!(report.batch_id, None);
        assert!(report.created_positions.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 1);
        assert!(
            report.errors[0].message.contains("Cannot sell"),
            "{:?}",
            report.errors
        );
        assert_eq!(count(&conn, "stock_investments"), 0);
        assert_eq!(count(&conn, "investment_transactions"), 0);
        assert_eq!(count(&conn, "stock_import_batches"), 0);
    }

    #[test]
    fn field_validation_reports_every_bad_row_and_writes_nothing() {
        let mut conn = setup_test_db();
        let mut bad_type = bulk_row("aapl", "banana", "1", "1", D0);
        bad_type.tx_type = "banana".into();
        let bad_quantity = bulk_row("msft", "buy", "0", "1", D0);
        let good = bulk_row("goog", "buy", "1", "1", D0);

        let report = bulk_create_stock_transactions(
            &mut conn,
            &[bad_type, bad_quantity, good],
            Some(&batch("x.csv")),
        )
        .unwrap();

        assert_eq!(report.created, 0);
        let indexes: Vec<usize> = report.errors.iter().map(|e| e.index).collect();
        assert_eq!(indexes, vec![0, 1]);
        assert_eq!(count(&conn, "stock_investments"), 0);
        assert_eq!(count(&conn, "stock_import_batches"), 0);
    }

    #[test]
    fn rows_are_written_in_date_order_with_buys_first_within_a_day() {
        let mut conn = setup_test_db();
        // The sell is listed first, on the same day as the buy it depends on.
        let rows = [
            bulk_row("aapl", "sell", "4", "110", D0),
            bulk_row("aapl", "buy", "10", "100", D0),
        ];
        let report = bulk_create_stock_transactions(&mut conn, &rows, None).unwrap();
        assert_eq!(report.created, 2, "{report:?}");
        assert_eq!(
            report.created_positions,
            vec!["AAPL"],
            "only the first buy creates it"
        );
        let types: Vec<String> = conn
            .prepare("SELECT type FROM investment_transactions ORDER BY rowid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(types, vec!["buy", "sell"]);
    }

    #[test]
    fn a_later_write_to_an_existing_position_reports_no_created_position() {
        let mut conn = setup_test_db();
        bulk_create_stock_transactions(
            &mut conn,
            &[bulk_row("aapl", "buy", "10", "100", D0)],
            None,
        )
        .unwrap();
        let report = bulk_create_stock_transactions(
            &mut conn,
            &[bulk_row("aapl", "buy", "5", "105", D0 + DAY)],
            Some(&batch("more.csv")),
        )
        .unwrap();
        assert_eq!(report.created, 1);
        assert!(report.created_positions.is_empty());
        assert_eq!(report.written_tickers, vec!["AAPL"]);
    }
}
