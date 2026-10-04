//! MCP tools: crypto. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;
use crate::services::{crypto_investments as crypto_service, dedup};

use super::{sql_to_json, BulkWriteReport, RowError, SkippedRow, MAX_BULK_ROWS};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CryptoTransactionsArgs {
    #[schemars(description = "Filter by ticker symbol (e.g. BTC, ETH)")]
    pub ticker: Option<String>,
    #[schemars(description = "Max records to return (default 200, max 1000)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CryptoValueHistoryArgs {
    #[schemars(description = "The crypto ticker symbol (e.g. BTC, ETH)")]
    pub ticker: String,
    #[serde(rename = "startDate")]
    #[schemars(description = "Start date as Unix timestamp (seconds)")]
    pub start_date: Option<i64>,
    #[serde(rename = "endDate")]
    #[schemars(description = "End date as Unix timestamp (seconds)")]
    pub end_date: Option<i64>,
}

pub fn crypto_list(conn: &Connection) -> Result<Value> {
    let avg_map = crate::services::cost_basis::cost_basis_for_all(
        conn,
        crate::services::cost_basis::TxTable::Crypto,
    )?;
    let mut stmt = conn.prepare(
        "SELECT ci.id, ci.ticker, ci.coingecko_id, ci.name, ci.quantity,
                ci.currency,
                COALESCE(cpo.price, cp.price) AS current_price,
                COALESCE(cpo.currency, cp.currency, 'USD') AS price_currency,
                cp.fetched_at
         FROM crypto_investments ci
         LEFT JOIN crypto_prices cp ON ci.ticker = cp.symbol
         LEFT JOIN crypto_price_overrides cpo ON ci.ticker = cpo.symbol
         ORDER BY ci.name",
    )?;
    let rows: Vec<Value> = stmt.query_map([], |row| Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "ticker": row.get::<_, String>(1)?,
        "coingeckoId": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
        "name": row.get::<_, String>(3)?,
        "quantity": row.get::<_, String>(4)?,
        "currency": row.get::<_, String>(5)?,
        "currentPrice": sql_to_json(row.get::<_, rusqlite::types::Value>(6).unwrap_or(rusqlite::types::Value::Null)),
        "priceCurrency": row.get::<_, String>(7)?,
        "priceFetchedAt": sql_to_json(row.get::<_, rusqlite::types::Value>(8).unwrap_or(rusqlite::types::Value::Null)),
    })))?.filter_map(|r| r.ok()).collect();
    let rows = rows
        .into_iter()
        .map(|mut v| {
            let avg = v["id"]
                .as_str()
                .and_then(|id| avg_map.get(id))
                .map(|p| p.average_price_native().to_string())
                .unwrap_or_else(|| "0".to_string());
            v["averagePrice"] = serde_json::json!(avg);
            v
        })
        .collect();
    Ok(Value::Array(rows))
}

pub fn crypto_transactions(conn: &Connection, ticker: Option<String>, limit: i64) -> Result<Value> {
    let rows: Vec<Value> = if let Some(ref t) = ticker {
        let mut stmt = conn.prepare(
            "SELECT id, investment_id, type, ticker, name, quantity,
                price_per_unit, currency, transaction_date, created_at
         FROM crypto_transactions WHERE ticker = ? ORDER BY transaction_date DESC, created_at DESC, rowid DESC LIMIT ?",
        )?;
        let result = stmt
            .query_map(rusqlite::params![t, limit], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "investmentId": row.get::<_, String>(1)?,
                    "type": row.get::<_, String>(2)?,
                    "ticker": row.get::<_, String>(3)?,
                    "name": row.get::<_, String>(4)?,
                    "quantity": row.get::<_, String>(5)?,
                    "pricePerUnit": row.get::<_, String>(6)?,
                    "currency": row.get::<_, String>(7)?,
                    "transactionDate": row.get::<_, i64>(8)?,
                    "createdAt": row.get::<_, i64>(9)?,
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();
        result
    } else {
        let mut stmt = conn.prepare(
            "SELECT id, investment_id, type, ticker, name, quantity,
                price_per_unit, currency, transaction_date, created_at
         FROM crypto_transactions ORDER BY transaction_date DESC, created_at DESC, rowid DESC LIMIT ?",
        )?;
        let result = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "investmentId": row.get::<_, String>(1)?,
                    "type": row.get::<_, String>(2)?,
                    "ticker": row.get::<_, String>(3)?,
                    "name": row.get::<_, String>(4)?,
                    "quantity": row.get::<_, String>(5)?,
                    "pricePerUnit": row.get::<_, String>(6)?,
                    "currency": row.get::<_, String>(7)?,
                    "transactionDate": row.get::<_, i64>(8)?,
                    "createdAt": row.get::<_, i64>(9)?,
                }))
            })?
            .filter_map(|r| r.ok())
            .collect();
        result
    };
    Ok(Value::Array(rows))
}

pub fn crypto_value_history(
    conn: &Connection,
    ticker: &str,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Value> {
    let mut conditions = vec!["ticker = ?".to_string()];
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(ticker.to_string())];
    if let Some(s) = start_date {
        conditions.push("recorded_at >= ?".into());
        p.push(Box::new(s));
    }
    if let Some(e) = end_date {
        conditions.push("recorded_at <= ?".into());
        p.push(Box::new(e));
    }
    let sql = format!(
        "SELECT id, ticker, recorded_at, value_czk, quantity, price, currency
         FROM crypto_value_history WHERE {} ORDER BY recorded_at ASC",
        conditions.join(" AND ")
    );
    let refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|x| x.as_ref()).collect();
    let mut stmt = conn.prepare(&sql)?;
    let rows: Vec<Value> = stmt.query_map(refs.as_slice(), |row| Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "ticker": row.get::<_, String>(1)?,
        "recordedAt": row.get::<_, i64>(2)?,
        "valueCzk": sql_to_json(row.get::<_, rusqlite::types::Value>(3).unwrap_or(rusqlite::types::Value::Null)),
        "quantity": sql_to_json(row.get::<_, rusqlite::types::Value>(4).unwrap_or(rusqlite::types::Value::Null)),
        "price": sql_to_json(row.get::<_, rusqlite::types::Value>(5).unwrap_or(rusqlite::types::Value::Null)),
        "currency": sql_to_json(row.get::<_, rusqlite::types::Value>(6).unwrap_or(rusqlite::types::Value::Null)),
    })))?.filter_map(|r| r.ok()).collect();
    Ok(Value::Array(rows))
}

// ============================================================================
// Write tools (Task 9: mirrors the canonical bulk engine in services/mcp/bank_accounts.rs
// and services/mcp/investments.rs::stock_transactions_create)
// ============================================================================

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CryptoTransactionRow {
    #[schemars(description = "Ticker symbol, e.g. BTC")]
    pub ticker: String,
    #[schemars(
        description = "Coin name; used when the holding is created. Defaults to the ticker"
    )]
    pub name: Option<String>,
    #[serde(rename = "coingeckoId")]
    #[schemars(
        description = "CoinGecko id, e.g. \"bitcoin\" — REQUIRED when this import creates a new holding (needed for price fetching)"
    )]
    pub coingecko_id: Option<String>,
    #[serde(rename = "type")]
    #[schemars(description = "buy or sell")]
    pub tx_type: String,
    #[schemars(description = "Positive quantity as a numeric string")]
    pub quantity: String,
    #[serde(rename = "pricePerUnit")]
    #[schemars(description = "Positive price per unit as a numeric string, in the given currency")]
    pub price_per_unit: String,
    #[schemars(description = "3-letter ISO currency code of the price")]
    pub currency: String,
    #[serde(rename = "transactionDate")]
    #[schemars(description = "Unix timestamp (seconds)")]
    pub transaction_date: i64,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CryptoTransactionsCreateArgs {
    #[schemars(length(max = MAX_BULK_ROWS))]
    pub transactions: Vec<CryptoTransactionRow>,
}

fn validate_crypto_row(row: &CryptoTransactionRow) -> Result<()> {
    let insert = crate::models::InsertCryptoTransaction {
        investment_id: None,
        tx_type: row.tx_type.clone(),
        ticker: row.ticker.clone(),
        name: row.name.clone().unwrap_or_else(|| row.ticker.clone()),
        quantity: row.quantity.clone(),
        price_per_unit: row.price_per_unit.clone(),
        currency: row.currency.clone(),
        transaction_date: row.transaction_date,
    };
    insert.validate()
}

/// Two-phase bulk import. Phase 1: every row field-validates. Phase 2: one SQL
/// transaction; rows sorted by date (buys before sells within a date — a sell
/// may need a position created earlier in the same batch); duplicates skipped;
/// business-rule failures mid-batch (sell without position, currency mismatch,
/// new coin missing a coingeckoId) roll the WHOLE batch back and are reported
/// as that row's error.
pub fn crypto_transactions_create(
    conn: &mut rusqlite::Connection,
    args: &CryptoTransactionsCreateArgs,
) -> Result<BulkWriteReport> {
    super::check_bulk_size(args.transactions.len())?;

    let mut errors: Vec<RowError> = Vec::new();
    for (i, row) in args.transactions.iter().enumerate() {
        if let Err(e) = validate_crypto_row(row) {
            errors.push(RowError {
                index: i,
                message: e.to_string(),
            });
        }
    }
    if !errors.is_empty() {
        return Ok(BulkWriteReport {
            created: 0,
            skipped_duplicates: vec![],
            errors,
        });
    }

    let mut order: Vec<usize> = (0..args.transactions.len()).collect();
    order.sort_by_key(|&i| {
        let r = &args.transactions[i];
        (
            r.transaction_date,
            if r.tx_type.to_lowercase() == "buy" {
                0
            } else {
                1
            },
        )
    });

    let tx = conn.transaction()?;
    let mut created = 0usize;
    let mut skipped: Vec<SkippedRow> = Vec::new();
    for &i in &order {
        let row = &args.transactions[i];
        let ticker_upper = row.ticker.to_uppercase();
        if let Some(reason) = dedup::find_duplicate_crypto_transaction(
            &tx,
            &ticker_upper,
            row.transaction_date,
            &row.tx_type.to_lowercase(),
            &row.quantity,
            &row.price_per_unit,
        )? {
            skipped.push(SkippedRow::new(i, reason));
            continue;
        }
        let name_or_ticker = row.name.as_deref().unwrap_or(&row.ticker);
        if let Err(e) = crypto_service::import_single_crypto_transaction(
            &tx,
            &row.ticker,
            name_or_ticker,
            row.coingecko_id.as_deref(),
            &row.tx_type,
            &row.quantity,
            &row.price_per_unit,
            &row.currency,
            row.transaction_date,
        ) {
            // roll back everything; report the failing row
            drop(tx);
            return Ok(BulkWriteReport {
                created: 0,
                skipped_duplicates: vec![],
                errors: vec![RowError {
                    index: i,
                    message: e.to_string(),
                }],
            });
        }
        created += 1;
    }
    tx.commit()?;

    Ok(BulkWriteReport {
        created,
        skipped_duplicates: skipped,
        errors: vec![],
    })
}

/// Distinct tickers touched by this batch, paired with a CoinGecko id (the
/// recalculation trigger needs it). If the same ticker appears with and
/// without an id in one batch, the Some(id) entry wins.
///
/// Note: if a batch supplies two *different* CoinGecko ids for one new
/// ticker, this picks the first one encountered in input order, while the
/// id actually persisted to `crypto_investments` is whichever row phase 2
/// processes first (buys before sells within a date, by rowid within a
/// tie — see `recalculate_crypto_metrics`). Those two "first"s can disagree.
/// Such input is malformed (one ticker cannot have two CoinGecko ids); this
/// is noted, not fixed — the two-phase validator doesn't currently reject it.
pub fn distinct_tickers(args: &CryptoTransactionsCreateArgs) -> Vec<(String, Option<String>)> {
    let mut map: std::collections::BTreeMap<String, Option<String>> =
        std::collections::BTreeMap::new();
    for row in &args.transactions {
        let ticker_upper = row.ticker.to_uppercase();
        let entry = map.entry(ticker_upper).or_insert(None);
        if entry.is_none() {
            if let Some(id) = &row.coingecko_id {
                *entry = Some(id.clone());
            }
        }
    }
    map.into_iter().collect()
}

pub fn earliest_date(args: &CryptoTransactionsCreateArgs) -> Option<i64> {
    args.transactions.iter().map(|r| r.transaction_date).min()
}

#[cfg(test)]
mod bulk_tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
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
        .expect("schema");
        conn
    }

    fn row(
        ticker: &str,
        coingecko_id: Option<&str>,
        tx_type: &str,
        quantity: &str,
        price: &str,
        currency: &str,
        date: i64,
    ) -> CryptoTransactionRow {
        CryptoTransactionRow {
            ticker: ticker.to_string(),
            name: None,
            coingecko_id: coingecko_id.map(String::from),
            tx_type: tx_type.to_string(),
            quantity: quantity.to_string(),
            price_per_unit: price.to_string(),
            currency: currency.to_string(),
            transaction_date: date,
        }
    }

    fn holding_quantity(conn: &Connection, ticker: &str) -> Option<String> {
        conn.query_row(
            "SELECT quantity FROM crypto_investments WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .ok()
    }

    fn holding_average_price(conn: &Connection, ticker: &str) -> Option<String> {
        let id: String = conn
            .query_row(
                "SELECT id FROM crypto_investments WHERE ticker = ?1",
                [ticker],
                |r| r.get(0),
            )
            .ok()?;
        crate::services::cost_basis::cost_basis_for_all(
            conn,
            crate::services::cost_basis::TxTable::Crypto,
        )
        .ok()?
        .get(&id)
        .map(|p| p.average_price_native().to_string())
    }

    fn holding_currency(conn: &Connection, ticker: &str) -> Option<String> {
        conn.query_row(
            "SELECT currency FROM crypto_investments WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .ok()
    }

    fn holding_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM crypto_investments", [], |r| r.get(0))
            .unwrap()
    }

    fn tx_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM crypto_transactions", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn happy_path_auto_creates_holding_and_recalculates() {
        let mut conn = setup_test_db();
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row(
                    "btc",
                    Some("bitcoin"),
                    "buy",
                    "1",
                    "900000",
                    "CZK",
                    1_700_000_000,
                ),
                row("btc", None, "buy", "2", "910000", "CZK", 1_700_100_000),
            ],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 2);
        assert!(report.skipped_duplicates.is_empty());
        assert!(report.errors.is_empty());
        assert_eq!(holding_count(&conn), 1);
        assert_eq!(tx_count(&conn), 2);
        assert_eq!(holding_quantity(&conn, "BTC"), Some("3".to_string()));
    }

    /// Carry-forward note 3: mirrors the stock test — `created_at` ties
    /// within a batch, and `recalculate_crypto_metrics` breaks the tie with
    /// `rowid ASC` (insertion order == this module's buys-before-sells sort),
    /// so the recalc processes the buy before the sell regardless of input
    /// order. `average_price` (and, for crypto specifically, the derived
    /// `native_currency` — a tie can flip which row is `txs[0]`) are the
    /// order-sensitive observables; quantity alone is commutative and
    /// wouldn't catch a tie-order regression.
    #[test]
    fn intra_batch_same_day_buy_then_sell_computes_correct_average_price() {
        let mut conn = setup_test_db();
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row("btc", None, "sell", "4", "910000", "CZK", 1_700_000_000),
                row(
                    "btc",
                    Some("bitcoin"),
                    "buy",
                    "10",
                    "900000",
                    "CZK",
                    1_700_000_000,
                ),
            ],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 2, "errors: {:?}", report.errors);
        assert!(report.errors.is_empty());
        assert_eq!(holding_quantity(&conn, "BTC"), Some("6".to_string()));
        // Buy-first: cost basis 10*900000=9,000,000, avg=900000; sell 4 at
        // avg cost reduces cost basis by 4*900000=3,600,000 -> 5,400,000 over
        // 6 units -> avg still 900000. Sell-first would have hit the
        // `total_qty > 0` guard (no existing position yet) and skipped the
        // cost-basis reduction, landing on a different average.
        assert_eq!(
            holding_average_price(&conn, "BTC"),
            Some("900000".to_string())
        );
        assert_eq!(holding_currency(&conn, "BTC"), Some("CZK".to_string()));
    }

    #[test]
    fn sell_without_position_rolls_back_whole_batch() {
        let mut conn = setup_test_db();
        let seed = CryptoTransactionsCreateArgs {
            transactions: vec![row(
                "btc",
                Some("bitcoin"),
                "buy",
                "5",
                "900000",
                "CZK",
                1_699_000_000,
            )],
        };
        assert_eq!(
            crypto_transactions_create(&mut conn, &seed)
                .unwrap()
                .created,
            1
        );

        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row("btc", None, "buy", "3", "950000", "CZK", 1_700_000_000), // would succeed alone
                row("eth", None, "sell", "2", "60000", "CZK", 1_700_100_000), // no position -> fails
            ],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert!(report.skipped_duplicates.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 1);
        assert_eq!(holding_count(&conn), 1);
        assert_eq!(holding_quantity(&conn, "BTC"), Some("5".to_string()));
        assert_eq!(tx_count(&conn), 1);
    }

    #[test]
    fn new_coin_without_coingecko_id_rejects_at_phase2_and_rolls_back() {
        let mut conn = setup_test_db();
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row(
                    "btc",
                    Some("bitcoin"),
                    "buy",
                    "1",
                    "900000",
                    "CZK",
                    1_700_000_000,
                ), // would succeed alone
                row("xyz", None, "buy", "10", "5", "CZK", 1_700_100_000), // new coin, no id -> fails
            ],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 1);
        assert!(report.errors[0].message.contains("coingeckoId"));
        assert_eq!(holding_count(&conn), 0, "BTC buy must be rolled back too");
        assert_eq!(tx_count(&conn), 0);
    }

    #[test]
    fn duplicate_row_is_skipped_not_inserted() {
        let mut conn = setup_test_db();
        let seed = CryptoTransactionsCreateArgs {
            transactions: vec![row(
                "btc",
                Some("bitcoin"),
                "buy",
                "10",
                "900000",
                "CZK",
                1_700_000_000,
            )],
        };
        assert_eq!(
            crypto_transactions_create(&mut conn, &seed)
                .unwrap()
                .created,
            1
        );

        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row("btc", None, "buy", "10", "900000", "CZK", 1_700_000_000), // exact duplicate
                row("btc", None, "buy", "5", "910000", "CZK", 1_700_100_000),  // distinct
            ],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 1);
        assert!(report.errors.is_empty());
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert_eq!(report.skipped_duplicates[0].index, 0);
        assert_eq!(tx_count(&conn), 2);
        assert_eq!(holding_quantity(&conn, "BTC"), Some("15".to_string()));
    }

    #[test]
    fn empty_batch_creates_nothing() {
        let mut conn = setup_test_db();
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert!(report.skipped_duplicates.is_empty());
        assert!(report.errors.is_empty());
        assert_eq!(holding_count(&conn), 0);
        assert_eq!(tx_count(&conn), 0);
    }

    #[test]
    fn create_rejects_oversized_batch() {
        let mut conn = setup_test_db();
        let oversized_count = MAX_BULK_ROWS + 1;
        let mut transactions = Vec::with_capacity(oversized_count);
        for i in 0..oversized_count {
            transactions.push(row(
                "btc",
                if i == 0 { Some("bitcoin") } else { None },
                "buy",
                "1",
                "900000",
                "CZK",
                1_700_000_000 + i as i64,
            ));
        }
        let args = CryptoTransactionsCreateArgs { transactions };
        let err = crypto_transactions_create(&mut conn, &args).unwrap_err();
        match err {
            crate::error::AppError::Validation(msg) => {
                assert!(
                    msg.contains(&oversized_count.to_string()),
                    "message must state the actual count: {msg}"
                );
                assert!(
                    msg.contains(&MAX_BULK_ROWS.to_string()),
                    "message must state the cap: {msg}"
                );
            }
            other => panic!("expected Validation error, got {other:?}"),
        }
        assert_eq!(
            holding_count(&conn),
            0,
            "an oversized batch must write nothing"
        );
        assert_eq!(tx_count(&conn), 0);
    }

    #[test]
    fn phase1_validation_rejection_writes_nothing() {
        let mut conn = setup_test_db();
        let mut bad = row(
            "btc",
            Some("bitcoin"),
            "buy",
            "10",
            "900000",
            "CZK",
            1_700_000_000,
        );
        bad.tx_type = "banana".into();
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                bad,
                row(
                    "eth",
                    Some("ethereum"),
                    "buy",
                    "5",
                    "60000",
                    "CZK",
                    1_700_100_000,
                ),
            ],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 0);
        assert_eq!(
            holding_count(&conn),
            0,
            "good row must not be inserted either"
        );
    }

    #[test]
    fn distinct_tickers_prefers_some_coingecko_id_when_deduping() {
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row("btc", None, "buy", "1", "900000", "CZK", 1),
                row("btc", Some("bitcoin"), "sell", "1", "900000", "CZK", 2),
                row("eth", Some("ethereum"), "buy", "1", "60000", "CZK", 3),
            ],
        };
        let tickers = distinct_tickers(&args);
        assert_eq!(
            tickers,
            vec![
                ("BTC".to_string(), Some("bitcoin".to_string())),
                ("ETH".to_string(), Some("ethereum".to_string())),
            ]
        );
    }

    #[test]
    fn earliest_date_picks_the_minimum() {
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row("btc", None, "buy", "1", "900000", "CZK", 500),
                row("btc", None, "buy", "1", "900000", "CZK", 100),
                row("btc", None, "buy", "1", "900000", "CZK", 300),
            ],
        };
        assert_eq!(earliest_date(&args), Some(100));
    }
}
