//! Crypto investments business logic service
//!
//! All business logic for crypto investments lives here.
//! Commands should only validate input, call these functions, and handle events.
//! This is the SINGLE SOURCE OF TRUTH for crypto investment operations.

use crate::error::Result;
use crate::models::{CryptoInvestment, CryptoTransaction, InsertCryptoTransaction};
use uuid::Uuid;

/// Recalculate crypto investment metrics from transactions
/// SINGLE SOURCE OF TRUTH for crypto metrics calculation
/// NOTE: Average price is calculated in the investment's native currency (from first transaction)
/// Transactions in other currencies are converted to the native currency before averaging
///
/// Ordered by `transaction_date, created_at, rowid`. The first two columns can
/// tie within one bulk import (second-granularity `created_at` stamps every
/// row in a batch identically), and the result is order-sensitive: a same-date
/// sell processed before its buy hits the `total_qty > 0` guard and skips the
/// cost-basis reduction, and — for crypto specifically — a tie can also flip
/// which row is `txs[0]`, changing the derived `native_currency`. `rowid ASC`
/// breaks the tie deterministically instead of leaving it to SQLite's
/// unspecified sort order for equal keys. `crypto_transactions` is an ordinary
/// rowid table, and rowid == insertion order == the bulk importer's
/// buys-before-sells order, so this pins today's observed behavior with zero
/// change for existing data.
pub fn recalculate_crypto_metrics(conn: &rusqlite::Connection, investment_id: &str) -> Result<()> {
    // Get all transactions for this investment (including currency), sorted by
    // date; rowid is the deterministic tiebreak for same-date/same-second rows
    // (see above).
    let mut stmt = conn.prepare(
        "SELECT type, quantity, price_per_unit, currency FROM crypto_transactions WHERE investment_id = ?1 ORDER BY transaction_date ASC, created_at ASC, rowid ASC"
    )?;

    let txs: Vec<(String, f64, f64, String)> = stmt
        .query_map([investment_id], |row| {
            let tx_type: String = row.get(0)?;
            let qty: String = row.get(1)?;
            let price: String = row.get(2)?;
            let currency: String = row.get(3)?;
            Ok((
                tx_type,
                qty.parse().unwrap_or(0.0),
                price.parse().unwrap_or(0.0),
                currency.to_uppercase(),
            ))
        })?
        .filter_map(|r| r.ok())
        .collect();

    // If no transactions, nothing to recalculate
    if txs.is_empty() {
        return Ok(());
    }

    // Native currency is the currency of the first transaction
    let native_currency = txs[0].3.clone();

    let mut total_qty = 0.0f64;

    for (tx_type, qty, _price, _currency) in txs {
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
    // (services/cost_basis.rs) — never converted at today's rate here.
    conn.execute(
        "UPDATE crypto_investments SET quantity = ?1, currency = ?2 WHERE id = ?3",
        rusqlite::params![total_qty.to_string(), native_currency, investment_id],
    )?;

    Ok(())
}

/// Internal function to create a crypto transaction record
/// Used by both single-create and potential future bulk import
/// This is the SINGLE SOURCE OF TRUTH for crypto transaction creation
#[allow(clippy::too_many_arguments)]
pub fn create_crypto_transaction_internal(
    conn: &rusqlite::Connection,
    investment_id: &str,
    ticker: &str,
    name: &str,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
    currency: &str,
    transaction_date: i64,
) -> Result<CryptoTransaction> {
    let tx_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    conn.execute(
        "INSERT INTO crypto_transactions
         (id, investment_id, type, ticker, name, quantity, price_per_unit, currency, transaction_date, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            tx_id,
            investment_id,
            tx_type,
            ticker,
            name,
            quantity,
            price_per_unit,
            currency,
            transaction_date,
            now,
        ],
    )?;

    Ok(CryptoTransaction {
        id: tx_id,
        investment_id: investment_id.to_string(),
        tx_type: tx_type.to_string(),
        ticker: ticker.to_string(),
        name: name.to_string(),
        quantity: quantity.to_string(),
        price_per_unit: price_per_unit.to_string(),
        currency: currency.to_string(),
        transaction_date,
        created_at: now,
    })
}

/// Get or create a crypto investment by ticker
/// Returns the investment ID
/// This is the SINGLE SOURCE OF TRUTH for crypto investment lookup/creation
pub fn get_or_create_crypto(
    conn: &rusqlite::Connection,
    ticker: &str,
    coingecko_id: Option<&str>,
    name: &str,
) -> Result<String> {
    let ticker_upper = ticker.to_uppercase();

    // Check if investment already exists
    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM crypto_investments WHERE ticker = ?1",
            [&ticker_upper],
            |row| row.get(0),
        )
        .ok();

    match existing {
        Some(id) => Ok(id),
        None => {
            let id = Uuid::new_v4().to_string();

            conn.execute(
                "INSERT INTO crypto_investments (id, ticker, coingecko_id, name, quantity)
                 VALUES (?1, ?2, ?3, ?4, '0')",
                rusqlite::params![id, ticker_upper, coingecko_id, name],
            )?;
            Ok(id)
        }
    }
}

/// Get crypto investment details by ID
pub fn get_crypto_by_id(
    conn: &rusqlite::Connection,
    investment_id: &str,
) -> Result<CryptoInvestment> {
    let mut inv = conn.query_row(
        "SELECT id, ticker, coingecko_id, name, quantity FROM crypto_investments WHERE id = ?1",
        [investment_id],
        |row| {
            Ok(CryptoInvestment {
                id: row.get(0)?,
                ticker: row.get(1)?,
                coingecko_id: row.get(2)?,
                name: row.get(3)?,
                quantity: row.get(4)?,
                average_price: "0".to_string(),
            })
        },
    )?;
    // Weighted average in the primary (first-transaction) currency, with
    // cross-currency transactions converted at their day's rates — crypto
    // positions may mix transaction currencies.
    inv.average_price = crate::services::cost_basis::cost_basis_for_all(
        conn,
        crate::services::cost_basis::TxTable::Crypto,
    )?
    .get(investment_id)
    .map(|p| p.average_price_native().to_string())
    .unwrap_or_else(|| "0".to_string());
    Ok(inv)
}

/// Create crypto investment with optional initial transaction
/// This is the main entry point for creating crypto investments with transactions
pub fn create_crypto_with_transaction(
    conn: &rusqlite::Connection,
    ticker: &str,
    coingecko_id: Option<&str>,
    name: &str,
    initial_transaction: Option<&InsertCryptoTransaction>,
) -> Result<CryptoInvestment> {
    let ticker_upper = ticker.to_uppercase();

    // Get or create the investment
    let investment_id = get_or_create_crypto(conn, &ticker_upper, coingecko_id, name)?;

    // Create initial transaction if provided
    if let Some(tx) = initial_transaction {
        create_crypto_transaction_internal(
            conn,
            &investment_id,
            &ticker_upper,
            name,
            &tx.tx_type,
            &tx.quantity,
            &tx.price_per_unit,
            &tx.currency,
            tx.transaction_date,
        )?;

        // Recalculate metrics from the transaction
        recalculate_crypto_metrics(conn, &investment_id)?;
    }

    // Return the investment
    get_crypto_by_id(conn, &investment_id)
}

/// Add a transaction to an existing crypto investment
pub fn add_transaction_to_crypto(
    conn: &rusqlite::Connection,
    investment_id: &str,
    data: &InsertCryptoTransaction,
) -> Result<CryptoTransaction> {
    // Get crypto investment info
    let (ticker, name): (String, String) = conn.query_row(
        "SELECT ticker, name FROM crypto_investments WHERE id = ?1",
        [investment_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;

    crate::services::investments::ensure_sell_within_holdings(
        conn,
        "crypto_transactions",
        investment_id,
        &data.tx_type,
        &data.quantity,
        data.transaction_date,
    )?;

    let tx = create_crypto_transaction_internal(
        conn,
        investment_id,
        &ticker,
        &name,
        &data.tx_type,
        &data.quantity,
        &data.price_per_unit,
        &data.currency,
        data.transaction_date,
    )?;

    recalculate_crypto_metrics(conn, investment_id)?;

    Ok(tx)
}

/// Unified import logic for a single crypto transaction. Mirrors
/// services/investments.rs::import_single_transaction semantics: type check,
/// sell-requires-position, currency match, get-or-create, insert, recalc.
/// A NEW coin additionally requires a CoinGecko id (NOT NULL column, needed
/// for price fetching).
#[allow(clippy::too_many_arguments)]
pub fn import_single_crypto_transaction(
    conn: &rusqlite::Connection,
    ticker: &str,
    name: &str,
    coingecko_id: Option<&str>,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
    currency: &str,
    transaction_date: i64,
) -> Result<String> {
    use crate::error::AppError;
    let ticker_upper = ticker.to_uppercase();
    let tx_type_lower = tx_type.to_lowercase();

    if tx_type_lower != "buy" && tx_type_lower != "sell" {
        return Err(AppError::Validation(format!(
            "Invalid type '{}' (must be 'buy' or 'sell')",
            tx_type
        )));
    }

    let existing: Option<String> = conn
        .query_row(
            "SELECT id FROM crypto_investments WHERE ticker = ?1",
            [&ticker_upper],
            |row| row.get(0),
        )
        .ok();

    if tx_type_lower == "sell" && existing.is_none() {
        return Err(AppError::Validation(format!(
            "{}: Cannot sell - no existing position found",
            ticker_upper
        )));
    }

    let investment_id = match existing {
        Some(id) => {
            let tx_count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM crypto_transactions WHERE investment_id = ?1",
                [&id],
                |row| row.get(0),
            )?;
            if tx_count > 0 {
                let inv_currency: String = conn
                    .query_row(
                        "SELECT currency FROM crypto_investments WHERE id = ?1",
                        [&id],
                        |row| row.get(0),
                    )
                    .unwrap_or_else(|_| currency.to_uppercase());
                if inv_currency.to_uppercase() != currency.to_uppercase() {
                    return Err(AppError::Validation(format!(
                        "{}: Currency mismatch - holding uses {} but transaction is in {}",
                        ticker_upper, inv_currency, currency
                    )));
                }
            }
            id
        }
        None => {
            let cg = coingecko_id.filter(|s| !s.is_empty()).ok_or_else(|| {
                AppError::Validation(format!(
                    "{}: new coin requires a coingeckoId (e.g. \"bitcoin\") for price fetching",
                    ticker_upper
                ))
            })?;
            get_or_create_crypto(conn, &ticker_upper, Some(cg), name)?
        }
    };

    create_crypto_transaction_internal(
        conn,
        &investment_id,
        &ticker_upper,
        name,
        &tx_type_lower,
        quantity,
        price_per_unit,
        currency,
        transaction_date,
    )?;

    recalculate_crypto_metrics(conn, &investment_id)?;

    Ok(investment_id)
}

/// Get value history for a specific crypto ticker
pub fn get_value_history(
    conn: &rusqlite::Connection,
    ticker: &str,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<crate::models::TickerValueHistory>> {
    let ticker_upper = ticker.to_uppercase();

    let mut query = String::from(
        "SELECT ticker, recorded_at, value_czk, quantity, price, currency 
         FROM crypto_value_history 
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("Failed to open in-memory db");

        conn.execute_batch(
            r#"
            CREATE TABLE crypto_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                coingecko_id TEXT NOT NULL,
                name TEXT NOT NULL,
                quantity TEXT NOT NULL,

                currency TEXT NOT NULL DEFAULT 'CZK'
            );

            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL REFERENCES crypto_investments(id) ON DELETE CASCADE,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            "#,
        )
        .expect("Failed to create schema");

        conn
    }

    /// For crypto: the shared holdings check applies to the UI write path.
    #[test]
    fn crypto_sell_above_holdings_is_rejected() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO crypto_investments (id, ticker, coingecko_id, name, quantity, currency)
             VALUES ('btc-1', 'BTC', 'bitcoin', 'Bitcoin', '0', 'USD')",
            [],
        )
        .expect("coin");
        let tx = |tx_type: &str, qty: &str, date: i64| InsertCryptoTransaction {
            investment_id: Some("btc-1".to_string()),
            tx_type: tx_type.to_string(),
            ticker: "BTC".to_string(),
            name: "Bitcoin".to_string(),
            quantity: qty.to_string(),
            price_per_unit: "50000".to_string(),
            currency: "USD".to_string(),
            transaction_date: date,
        };
        add_transaction_to_crypto(&conn, "btc-1", &tx("buy", "0.5", 1_700_000_000)).expect("buy");
        let err = add_transaction_to_crypto(&conn, "btc-1", &tx("sell", "0.75", 1_700_086_400))
            .expect_err("oversell");
        assert!(matches!(err, AppError::Validation(k) if k == "validation.sellExceedsHoldings"));
        add_transaction_to_crypto(&conn, "btc-1", &tx("sell", "0.5", 1_700_086_400))
            .expect("sell all");
    }

    #[test]
    fn import_creates_coin_when_coingecko_id_present() {
        let conn = setup_test_db();
        import_single_crypto_transaction(
            &conn,
            "btc",
            "Bitcoin",
            Some("bitcoin"),
            "buy",
            "0.5",
            "900000",
            "CZK",
            1700000000,
        )
        .unwrap();
        let (ticker, cg): (String, String) = conn
            .query_row(
                "SELECT ticker, coingecko_id FROM crypto_investments",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(ticker, "BTC");
        assert_eq!(cg, "bitcoin");
    }

    #[test]
    fn import_rejects_new_coin_without_coingecko_id() {
        let conn = setup_test_db();
        let err = import_single_crypto_transaction(
            &conn, "XYZ", "Mystery", None, "buy", "1", "10", "CZK", 1700000000,
        )
        .unwrap_err();
        assert!(err.to_string().contains("coingeckoId"), "err was: {err}");
    }

    #[test]
    fn import_existing_coin_ignores_missing_coingecko_id() {
        let conn = setup_test_db();
        import_single_crypto_transaction(
            &conn,
            "BTC",
            "Bitcoin",
            Some("bitcoin"),
            "buy",
            "1",
            "900000",
            "CZK",
            1700000000,
        )
        .unwrap();
        import_single_crypto_transaction(
            &conn, "BTC", "Bitcoin", None, "buy", "1", "950000", "CZK", 1700000001,
        )
        .unwrap();
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM crypto_transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn import_rejects_sell_without_position_and_bad_type() {
        let conn = setup_test_db();
        assert!(import_single_crypto_transaction(
            &conn,
            "ETH",
            "Ethereum",
            Some("ethereum"),
            "sell",
            "1",
            "80000",
            "CZK",
            1700000000,
        )
        .is_err());
        assert!(import_single_crypto_transaction(
            &conn,
            "ETH",
            "Ethereum",
            Some("ethereum"),
            "hold",
            "1",
            "80000",
            "CZK",
            1700000000,
        )
        .is_err());
    }

    #[test]
    fn import_rejects_currency_mismatch_with_existing_coin() {
        let conn = setup_test_db();
        import_single_crypto_transaction(
            &conn,
            "BTC",
            "Bitcoin",
            Some("bitcoin"),
            "buy",
            "1",
            "900000",
            "CZK",
            1700000000,
        )
        .unwrap();
        assert!(import_single_crypto_transaction(
            &conn, "BTC", "Bitcoin", None, "buy", "1", "40000", "USD", 1700000001,
        )
        .is_err());
    }

    #[test]
    fn test_get_value_history_sql_construction() {
        let conn = Connection::open_in_memory().unwrap();

        // Create table
        conn.execute(
            "CREATE TABLE crypto_value_history (
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
            "INSERT INTO crypto_value_history (ticker, recorded_at, value_czk, quantity, price, currency)
             VALUES ('BTC', 1000, '1000', '1', '10000', 'USD')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO crypto_value_history (ticker, recorded_at, value_czk, quantity, price, currency)
             VALUES ('BTC', 2000, '2000', '1', '20000', 'USD')",
            [],
        ).unwrap();
        conn.execute(
            "INSERT INTO crypto_value_history (ticker, recorded_at, value_czk, quantity, price, currency)
             VALUES ('BTC', 3000, '3000', '1', '30000', 'USD')",
            [],
        ).unwrap();

        // Test 1: No dates (All) - Should NOT fail
        let history = get_value_history(&conn, "BTC", None, None).unwrap();
        assert_eq!(history.len(), 3);

        // Test 2: Start date only
        let history = get_value_history(&conn, "BTC", Some(2000), None).unwrap();
        assert_eq!(history.len(), 2); // 2000 and 3000

        // Test 3: End date only
        let history = get_value_history(&conn, "BTC", None, Some(2000)).unwrap();
        assert_eq!(history.len(), 2); // 1000 and 2000

        // Test 4: Both dates
        let history = get_value_history(&conn, "BTC", Some(2000), Some(2500)).unwrap();
        assert_eq!(history.len(), 1); // Only 2000
    }
}
