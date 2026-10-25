//! Crypto investments business logic service
//!
//! All business logic for crypto investments lives here.
//! Commands should only validate input, call these functions, and handle events.
//! This is the SINGLE SOURCE OF TRUTH for crypto investment operations.

use crate::error::Result;
use crate::models::{CryptoInvestment, CryptoTransaction, InsertCryptoTransaction};
use rusqlite::OptionalExtension;
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
    let native_currency:Option<String>=conn.query_row(
        "SELECT currency FROM crypto_transactions WHERE investment_id=?1 ORDER BY transaction_date,created_at,rowid LIMIT 1",
        [investment_id],|r|r.get(0),
    ).optional()?;
    let Some(native_currency) = native_currency else {
        return Ok(());
    };
    let total_qty = crate::services::investments::exact_held_quantity(
        conn,
        "crypto_transactions",
        investment_id,
    )?;
    let native_currency = native_currency.to_ascii_uppercase();

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
    let quantity = crate::services::dedup::canonical_amount(quantity);
    let price_per_unit = crate::services::dedup::canonical_amount(price_per_unit);
    let tx_type = tx_type.to_ascii_lowercase();
    let currency = currency.to_ascii_uppercase();
    let tx_id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let transaction_date =
        crate::models::validation::utc_day(transaction_date, "validation.dateInvalid")?;

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

/// A symbol supplied by a caller, trimmed and uppercased; `None` when absent or blank.
/// An invalid one is `validation.yahooSymbolInvalid`.
fn provided_symbol(yahoo_symbol: Option<&str>) -> Result<Option<String>> {
    yahoo_symbol
        .map(str::trim)
        .filter(|symbol| !symbol.is_empty())
        .map(crate::services::crypto_symbols::normalize_symbol)
        .transpose()
}

/// Check a supplied symbol against the stored holding: a different symbol is
/// `validation.yahooSymbolConflict`; a symbol for a holding that has none yet is stored.
fn reconcile_symbol(
    conn: &rusqlite::Connection,
    investment_id: &str,
    ticker_upper: &str,
    stored: Option<&str>,
    provided: Option<&str>,
) -> Result<()> {
    match (stored, provided) {
        (Some(stored), Some(provided)) if stored != provided => {
            Err(crate::error::AppError::validation_with(
                "validation.yahooSymbolConflict",
                serde_json::json!({ "ticker": ticker_upper }),
            ))
        }
        (None, Some(provided)) => {
            conn.execute(
                "UPDATE crypto_investments SET yahoo_symbol = ?1 WHERE id = ?2",
                rusqlite::params![provided, investment_id],
            )?;
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Get or create a crypto investment by ticker
/// Returns the investment ID
/// This is the SINGLE SOURCE OF TRUTH for crypto investment lookup/creation
///
/// `yahoo_symbol` is optional: a new coin without one is stored unresolved (`NULL`) and the
/// price refresh resolves it later (spec D4); one that differs from the stored symbol is a
/// conflict.
pub fn get_or_create_crypto(
    conn: &rusqlite::Connection,
    ticker: &str,
    yahoo_symbol: Option<&str>,
    name: &str,
) -> Result<String> {
    let ticker_upper = ticker.to_uppercase();
    let provided = provided_symbol(yahoo_symbol)?;

    // Check if investment already exists
    let existing: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT id, yahoo_symbol FROM crypto_investments WHERE ticker = ?1",
            [&ticker_upper],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    match existing {
        Some((id, stored)) => {
            reconcile_symbol(
                conn,
                &id,
                &ticker_upper,
                stored.as_deref(),
                provided.as_deref(),
            )?;
            Ok(id)
        }
        None => {
            let id = Uuid::new_v4().to_string();

            conn.execute(
                "INSERT INTO crypto_investments (id, ticker, yahoo_symbol, name, quantity)
                 VALUES (?1, ?2, ?3, ?4, '0')",
                rusqlite::params![id, ticker_upper, provided, name],
            )?;
            Ok(id)
        }
    }
}

/// Store the Yahoo symbol of a holding (the symbol picker): trimmed, uppercased and validated
/// (`validation.yahooSymbolInvalid`); `NotFound` for an unknown holding.
pub fn set_symbol(conn: &rusqlite::Connection, investment_id: &str, symbol: &str) -> Result<()> {
    let symbol = crate::services::crypto_symbols::normalize_symbol(symbol)?;
    let changed = conn.execute(
        "UPDATE crypto_investments SET yahoo_symbol = ?1 WHERE id = ?2",
        rusqlite::params![symbol, investment_id],
    )?;
    if changed == 0 {
        return Err(crate::error::AppError::NotFound(
            "Crypto investment not found".into(),
        ));
    }
    Ok(())
}

/// Get crypto investment details by ID
pub fn get_crypto_by_id(
    conn: &rusqlite::Connection,
    investment_id: &str,
) -> Result<CryptoInvestment> {
    let mut inv = conn.query_row(
        "SELECT id, ticker, yahoo_symbol, name, quantity FROM crypto_investments WHERE id = ?1",
        [investment_id],
        |row| {
            Ok(CryptoInvestment {
                id: row.get(0)?,
                ticker: row.get(1)?,
                yahoo_symbol: row.get(2)?,
                name: row.get(3)?,
                quantity: row.get(4)?,
                average_price: "0".to_string(),
            })
        },
    )?;
    // Weighted average in the primary (first-transaction) currency, with
    // cross-currency transactions converted at their day's rates — crypto
    // positions may mix transaction currencies.
    inv.average_price = crate::services::cost_basis::native_average_price(
        conn,
        crate::services::cost_basis::TxTable::Crypto,
        investment_id,
        &crate::services::fx::current(),
    )?
    .to_string();
    Ok(inv)
}

/// Create crypto investment with optional initial transaction
/// This is the main entry point for creating crypto investments with transactions
pub fn create_crypto_with_transaction(
    conn: &rusqlite::Connection,
    ticker: &str,
    yahoo_symbol: Option<&str>,
    name: &str,
    initial_transaction: Option<&InsertCryptoTransaction>,
) -> Result<CryptoInvestment> {
    if let Some(data) = initial_transaction {
        data.validate()?;
    }
    let transaction = conn.unchecked_transaction()?;
    let conn = &transaction;
    let ticker_upper = ticker.to_uppercase();

    // Get or create the investment
    let investment_id = get_or_create_crypto(conn, &ticker_upper, yahoo_symbol, name)?;

    // Create initial transaction if provided
    if let Some(tx) = initial_transaction {
        add_transaction_to_crypto(conn, &investment_id, tx)?;
    }

    // Return the investment
    let investment = get_crypto_by_id(conn, &investment_id)?;
    transaction.commit()?;
    Ok(investment)
}

/// Add a transaction to an existing crypto investment
pub fn add_transaction_to_crypto(
    conn: &rusqlite::Connection,
    investment_id: &str,
    data: &InsertCryptoTransaction,
) -> Result<CryptoTransaction> {
    data.validate()?;
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

/// Delete a crypto transaction and return its date and the holding's ticker for the
/// historical rebuild, even when deleting the holding's last row.
pub fn delete_transaction(conn: &rusqlite::Connection, tx_id: &str) -> Result<(i64, String)> {
    let transaction = conn.unchecked_transaction()?;
    let conn = &transaction;
    let (date, investment_id, ticker): (i64, String, String) = conn
        .query_row(
            "SELECT t.transaction_date, t.investment_id, i.ticker FROM crypto_transactions t
             JOIN crypto_investments i ON i.id = t.investment_id WHERE t.id = ?1",
            [tx_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or_else(|| crate::error::AppError::NotFound("Transaction not found".into()))?;
    conn.execute("DELETE FROM crypto_transactions WHERE id=?1", [tx_id])?;
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM crypto_transactions WHERE investment_id=?1",
        [&investment_id],
        |r| r.get(0),
    )?;
    if count == 0 {
        conn.execute(
            "DELETE FROM crypto_investments WHERE id=?1",
            [&investment_id],
        )?;
    } else {
        crate::services::investments::ensure_valid_holdings_history(
            conn,
            "crypto_transactions",
            &investment_id,
        )?;
        recalculate_crypto_metrics(conn, &investment_id)?;
    }
    transaction.commit()?;
    Ok((date, ticker))
}

/// Unified import logic for a single crypto transaction. Mirrors
/// services/investments.rs::import_single_transaction semantics: type check,
/// sell-requires-position, Yahoo symbol identity, get-or-create, insert, recalc.
/// The symbol is optional: a NEW coin without one is created unresolved and the
/// price refresh resolves it (spec D4); a symbol that differs from the stored
/// one is `validation.yahooSymbolConflict`.
#[allow(clippy::too_many_arguments)]
pub fn import_single_crypto_transaction(
    conn: &rusqlite::Connection,
    ticker: &str,
    name: &str,
    yahoo_symbol: Option<&str>,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
    currency: &str,
    transaction_date: i64,
) -> Result<String> {
    import_crypto_row(
        conn,
        ticker,
        name,
        yahoo_symbol,
        tx_type,
        quantity,
        price_per_unit,
        currency,
        transaction_date,
        true,
    )
}

/// Only the transactional bulk writer may defer holdings replay until every
/// incoming row has been inserted. It must validate the final history before commit.
#[allow(clippy::too_many_arguments)]
pub(crate) fn import_crypto_transaction_deferred(
    conn: &rusqlite::Connection,
    ticker: &str,
    name: &str,
    yahoo_symbol: Option<&str>,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
    currency: &str,
    transaction_date: i64,
) -> Result<String> {
    import_crypto_row(
        conn,
        ticker,
        name,
        yahoo_symbol,
        tx_type,
        quantity,
        price_per_unit,
        currency,
        transaction_date,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn import_crypto_row(
    conn: &rusqlite::Connection,
    ticker: &str,
    name: &str,
    yahoo_symbol: Option<&str>,
    tx_type: &str,
    quantity: &str,
    price_per_unit: &str,
    currency: &str,
    transaction_date: i64,
    validate_holdings: bool,
) -> Result<String> {
    use crate::error::AppError;
    InsertCryptoTransaction {
        investment_id: None,
        tx_type: tx_type.into(),
        ticker: ticker.into(),
        name: name.into(),
        quantity: quantity.into(),
        price_per_unit: price_per_unit.into(),
        currency: currency.into(),
        transaction_date,
    }
    .validate()?;
    let ticker_upper = ticker.to_uppercase();
    let tx_type_lower = tx_type.to_lowercase();

    if tx_type_lower != "buy" && tx_type_lower != "sell" {
        return Err(AppError::Validation(
            "validation.transactionTypeInvalid".into(),
        ));
    }

    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM crypto_investments WHERE ticker = ?1)",
        [&ticker_upper],
        |row| row.get(0),
    )?;

    if tx_type_lower == "sell" && !exists {
        return Err(AppError::validation_with(
            "validation.sellWithoutPosition",
            serde_json::json!({ "ticker": ticker_upper }),
        ));
    }

    let investment_id = get_or_create_crypto(conn, &ticker_upper, yahoo_symbol, name)?;

    if validate_holdings {
        crate::services::investments::ensure_sell_within_holdings(
            conn,
            "crypto_transactions",
            &investment_id,
            &tx_type_lower,
            quantity,
            transaction_date,
        )?;
    }

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

    if validate_holdings {
        recalculate_crypto_metrics(conn, &investment_id)?;
    }

    Ok(investment_id)
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
                yahoo_symbol TEXT,
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
            "INSERT INTO crypto_investments (id, ticker, yahoo_symbol, name, quantity, currency)
             VALUES ('btc-1', 'BTC', 'BTC-USD', 'Bitcoin', '0', 'USD')",
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

    fn stored_symbol(conn: &Connection, ticker: &str) -> Option<String> {
        conn.query_row(
            "SELECT yahoo_symbol FROM crypto_investments WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn a_new_coin_without_a_symbol_is_stored_unresolved() {
        let conn = setup_test_db();
        get_or_create_crypto(&conn, "sui", None, "Sui").unwrap();
        assert_eq!(stored_symbol(&conn, "SUI"), None);
        get_or_create_crypto(&conn, "ARB", Some("  "), "Arbitrum").unwrap();
        assert_eq!(stored_symbol(&conn, "ARB"), None);
        get_or_create_crypto(&conn, "BTC", Some(" btc-usd "), "Bitcoin").unwrap();
        assert_eq!(stored_symbol(&conn, "BTC").as_deref(), Some("BTC-USD"));
    }

    #[test]
    fn a_symbol_that_differs_from_the_stored_one_is_a_conflict() {
        let conn = setup_test_db();
        let id = get_or_create_crypto(&conn, "BTC", Some("BTC-USD"), "Bitcoin").unwrap();
        let err = get_or_create_crypto(&conn, "BTC", Some("ETH-USD"), "Bitcoin").unwrap_err();
        assert_eq!(
            err.keyed().expect("keyed"),
            (
                "validation.yahooSymbolConflict".to_string(),
                serde_json::json!({ "ticker": "BTC" })
            )
        );
        // the same symbol, any spelling, or none at all is the existing holding
        assert_eq!(
            get_or_create_crypto(&conn, "btc", Some("btc-usd"), "Bitcoin").unwrap(),
            id
        );
        assert_eq!(
            get_or_create_crypto(&conn, "BTC", None, "Bitcoin").unwrap(),
            id
        );
        // an unresolved holding takes a supplied symbol
        get_or_create_crypto(&conn, "SUI", None, "Sui").unwrap();
        get_or_create_crypto(&conn, "SUI", Some("SUI20947-USD"), "Sui").unwrap();
        assert_eq!(stored_symbol(&conn, "SUI").as_deref(), Some("SUI20947-USD"));
    }

    #[test]
    fn set_symbol_normalizes_and_stores() {
        let conn = setup_test_db();
        let id = get_or_create_crypto(&conn, "SUI", None, "Sui").unwrap();
        set_symbol(&conn, &id, " sui20947-usd ").unwrap();
        assert_eq!(stored_symbol(&conn, "SUI").as_deref(), Some("SUI20947-USD"));
        assert!(matches!(
            set_symbol(&conn, "missing", "SUI20947-USD"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn set_symbol_rejects_an_invalid_symbol() {
        let conn = setup_test_db();
        let id = get_or_create_crypto(&conn, "BTC", Some("BTC-USD"), "Bitcoin").unwrap();
        let err = set_symbol(&conn, &id, "bitcoin").unwrap_err();
        assert!(matches!(err, AppError::Validation(k) if k == "validation.yahooSymbolInvalid"));
        assert_eq!(stored_symbol(&conn, "BTC").as_deref(), Some("BTC-USD"));
    }

    #[test]
    fn import_creates_coin_without_symbol() {
        let conn = setup_test_db();
        import_single_crypto_transaction(
            &conn, "xyz", "Mystery", None, "buy", "1", "10", "CZK", 1700000000,
        )
        .unwrap();
        import_single_crypto_transaction(
            &conn,
            "btc",
            "Bitcoin",
            Some("BTC-USD"),
            "buy",
            "0.5",
            "900000",
            "CZK",
            1700000000,
        )
        .unwrap();
        assert_eq!(stored_symbol(&conn, "XYZ"), None);
        assert_eq!(stored_symbol(&conn, "BTC").as_deref(), Some("BTC-USD"));
    }

    #[test]
    fn import_rejects_conflicting_symbol() {
        let conn = setup_test_db();
        import_single_crypto_transaction(
            &conn,
            "BTC",
            "Bitcoin",
            Some("BTC-USD"),
            "buy",
            "1",
            "900000",
            "CZK",
            1700000000,
        )
        .unwrap();
        let err = import_single_crypto_transaction(
            &conn,
            "BTC",
            "Bitcoin",
            Some("WBTC-USD"),
            "buy",
            "1",
            "900000",
            "CZK",
            1700000001,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("validation.yahooSymbolConflict"),
            "err was: {err}"
        );
        let n: i64 = conn
            .query_row("SELECT COUNT(*) FROM crypto_transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn import_existing_coin_ignores_missing_symbol() {
        let conn = setup_test_db();
        import_single_crypto_transaction(
            &conn,
            "BTC",
            "Bitcoin",
            Some("BTC-USD"),
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
        assert_eq!(stored_symbol(&conn, "BTC").as_deref(), Some("BTC-USD"));
    }

    #[test]
    fn import_rejects_sell_without_position_and_bad_type() {
        let conn = setup_test_db();
        assert!(import_single_crypto_transaction(
            &conn,
            "ETH",
            "Ethereum",
            Some("ETH-USD"),
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
            Some("ETH-USD"),
            "hold",
            "1",
            "80000",
            "CZK",
            1700000000,
        )
        .is_err());
    }

    #[test]
    fn import_accepts_another_currency_for_existing_coin() {
        let conn = setup_test_db();
        import_single_crypto_transaction(
            &conn,
            "BTC",
            "Bitcoin",
            Some("BTC-USD"),
            "buy",
            "1",
            "900000",
            "CZK",
            1700000000,
        )
        .unwrap();
        import_single_crypto_transaction(
            &conn, "BTC", "Bitcoin", None, "buy", "1", "40000", "USD", 1700000001,
        )
        .expect("crypto transactions retain their native price currency");
        let currencies: Vec<String> = conn
            .prepare("SELECT currency FROM crypto_transactions ORDER BY rowid")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|row| row.unwrap())
            .collect();
        assert_eq!(currencies, vec!["CZK", "USD"]);
    }
    #[test]
    fn initial_sale_is_rejected_without_leaving_an_empty_crypto_position() {
        let conn = setup_test_db();
        let data = InsertCryptoTransaction {
            investment_id: None,
            tx_type: "sell".into(),
            ticker: "BTC".into(),
            name: "Bitcoin".into(),
            quantity: "1".into(),
            price_per_unit: "100".into(),
            currency: "USD".into(),
            transaction_date: 1_700_000_000,
        };
        assert!(create_crypto_with_transaction(
            &conn,
            "BTC",
            Some("BTC-USD"),
            "Bitcoin",
            Some(&data)
        )
        .is_err());
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM crypto_investments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0);
    }
    #[test]
    fn decimal_liquidation_stores_exact_zero_crypto_quantity() {
        let conn = setup_test_db();
        let id = get_or_create_crypto(&conn, "BTC", Some("BTC-USD"), "Bitcoin").unwrap();
        for (kind, qty, day) in [("buy", "0.1", 0), ("buy", "0.2", 1), ("sell", "0.3", 2)] {
            let data = InsertCryptoTransaction {
                investment_id: Some(id.clone()),
                tx_type: kind.into(),
                ticker: "BTC".into(),
                name: "Bitcoin".into(),
                quantity: qty.into(),
                price_per_unit: "100".into(),
                currency: "USD".into(),
                transaction_date: 1_700_000_000 + day * 86400,
            };
            add_transaction_to_crypto(&conn, &id, &data).unwrap();
        }
        let quantity: String = conn
            .query_row(
                "SELECT quantity FROM crypto_investments WHERE id=?1",
                [&id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(quantity, "0");
    }

    fn deletion_fixture(conn: &Connection) -> (String, CryptoTransaction, CryptoTransaction) {
        let id = get_or_create_crypto(conn, "BTC", Some("BTC-USD"), "Bitcoin").unwrap();
        let data = |kind: &str, qty: &str, day: i64| InsertCryptoTransaction {
            investment_id: Some(id.clone()),
            tx_type: kind.into(),
            ticker: "BTC".into(),
            name: "Bitcoin".into(),
            quantity: qty.into(),
            price_per_unit: "100".into(),
            currency: "USD".into(),
            transaction_date: 1_700_000_000 + day * 86400,
        };
        let buy = add_transaction_to_crypto(conn, &id, &data("buy", "10", 0)).unwrap();
        let sale = add_transaction_to_crypto(conn, &id, &data("sell", "8", 1)).unwrap();
        (id, buy, sale)
    }

    #[test]
    fn crypto_delete_cannot_remove_a_buy_needed_by_a_future_sale() {
        let conn = setup_test_db();
        let (id, buy, _) = deletion_fixture(&conn);
        let err = delete_transaction(&conn, &buy.id).expect_err("cannot orphan future sale");
        assert!(matches!(err,AppError::Validation(key) if key=="validation.sellExceedsHoldings"));
        let count: i64 = conn
            .query_row("SELECT count(*) FROM crypto_transactions", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
        assert_eq!(get_crypto_by_id(&conn, &id).unwrap().quantity, "2");
    }

    #[test]
    fn crypto_delete_returns_the_history_ticker_and_cleans_up_last_row() {
        let conn = setup_test_db();
        let (id, buy, sale) = deletion_fixture(&conn);
        assert_eq!(
            delete_transaction(&conn, &sale.id).unwrap(),
            (sale.transaction_date, "BTC".into())
        );
        assert_eq!(get_crypto_by_id(&conn, &id).unwrap().quantity, "10");
        assert_eq!(
            delete_transaction(&conn, &buy.id).unwrap(),
            (buy.transaction_date, "BTC".into())
        );
        let count: i64 = conn
            .query_row("SELECT count(*) FROM crypto_investments", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 0);
        assert!(matches!(
            delete_transaction(&conn, "missing"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn crypto_rule_failures_carry_keys_and_their_values() {
        let conn = setup_test_db();
        let key_of = |err: AppError| err.keyed().expect("keyed validation error");
        let err = import_single_crypto_transaction(
            &conn,
            "ETH",
            "Ethereum",
            Some("ETH-USD"),
            "sell",
            "1",
            "10",
            "CZK",
            1700000000,
        )
        .unwrap_err();
        assert_eq!(
            key_of(err),
            (
                "validation.sellWithoutPosition".to_string(),
                serde_json::json!({ "ticker": "ETH" })
            )
        );
        import_single_crypto_transaction(
            &conn,
            "BTC",
            "Bitcoin",
            Some("BTC-USD"),
            "buy",
            "1",
            "900000",
            "CZK",
            1700000000,
        )
        .unwrap();
        let err = import_single_crypto_transaction(
            &conn,
            "BTC",
            "Bitcoin",
            Some("WBTC-USD"),
            "buy",
            "1",
            "900000",
            "CZK",
            1700000001,
        )
        .unwrap_err();
        assert_eq!(
            key_of(err),
            (
                "validation.yahooSymbolConflict".to_string(),
                serde_json::json!({ "ticker": "BTC" })
            )
        );
    }
}
