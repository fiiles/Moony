//! MCP tools: investments. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;
use crate::services::investments as investment_service;

use super::{
    sql_to_json, ticker_value_history, BulkWriteReport, RowError, SkippedRow, MAX_BULK_ROWS,
};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InvestmentDetailArgs {
    #[schemars(description = "The stock investment ID")]
    pub id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StockTransactionsArgs {
    #[schemars(description = "Filter by ticker symbol (e.g. AAPL, MSFT)")]
    pub ticker: Option<String>,
    #[schemars(description = "Max records to return (default 200, max 1000)")]
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StockValueHistoryArgs {
    #[schemars(description = "The stock ticker symbol (e.g. AAPL)")]
    pub ticker: String,
    #[serde(rename = "startDate")]
    #[schemars(description = "Start date as Unix timestamp (seconds)")]
    pub start_date: Option<i64>,
    #[serde(rename = "endDate")]
    #[schemars(description = "End date as Unix timestamp (seconds)")]
    pub end_date: Option<i64>,
}

pub fn investments_list(conn: &Connection) -> Result<Value> {
    let avg_map = crate::services::cost_basis::cost_basis_for_all(
        conn,
        crate::services::cost_basis::TxTable::Stocks,
    )?;
    let mut stmt = conn.prepare(
        "SELECT si.id, si.ticker, si.company_name, si.quantity,
                si.currency,
                CASE WHEN spo.price IS NOT NULL AND (sd.fetched_at IS NULL OR sd.fetched_at <= spo.updated_at) THEN spo.price ELSE sd.original_price END AS current_price,
                COALESCE(CASE WHEN spo.price IS NOT NULL AND (sd.fetched_at IS NULL OR sd.fetched_at <= spo.updated_at) THEN spo.currency ELSE sd.currency END, 'USD') AS price_currency,
                sd.short_name, sd.sector, sd.industry, sd.pe_ratio, sd.market_cap, sd.beta,
                sd.fifty_two_week_high, sd.fifty_two_week_low, sd.fetched_at
         FROM stock_investments si
         LEFT JOIN stock_data sd ON si.ticker = sd.ticker
         LEFT JOIN stock_price_overrides spo ON si.ticker = spo.ticker
         ORDER BY si.company_name",
    )?;
    let rows: Vec<Value> = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "ticker": row.get::<_, String>(1)?,
            "companyName": row.get::<_, String>(2)?,
            "quantity": row.get::<_, String>(3)?,
            "currency": row.get::<_, String>(4)?,
            "currentPrice": sql_to_json(row.get::<_, rusqlite::types::Value>(5).unwrap_or(rusqlite::types::Value::Null)),
            "priceCurrency": row.get::<_, String>(6)?,
            "shortName": sql_to_json(row.get::<_, rusqlite::types::Value>(7).unwrap_or(rusqlite::types::Value::Null)),
            "sector": sql_to_json(row.get::<_, rusqlite::types::Value>(8).unwrap_or(rusqlite::types::Value::Null)),
            "industry": sql_to_json(row.get::<_, rusqlite::types::Value>(9).unwrap_or(rusqlite::types::Value::Null)),
            "peRatio": sql_to_json(row.get::<_, rusqlite::types::Value>(10).unwrap_or(rusqlite::types::Value::Null)),
            "marketCap": sql_to_json(row.get::<_, rusqlite::types::Value>(11).unwrap_or(rusqlite::types::Value::Null)),
            "beta": sql_to_json(row.get::<_, rusqlite::types::Value>(12).unwrap_or(rusqlite::types::Value::Null)),
            "fiftyTwoWeekHigh": sql_to_json(row.get::<_, rusqlite::types::Value>(13).unwrap_or(rusqlite::types::Value::Null)),
            "fiftyTwoWeekLow": sql_to_json(row.get::<_, rusqlite::types::Value>(14).unwrap_or(rusqlite::types::Value::Null)),
            "priceFetchedAt": sql_to_json(row.get::<_, rusqlite::types::Value>(15).unwrap_or(rusqlite::types::Value::Null)),
        }))
    })?.filter_map(|r| r.ok()).collect();
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

pub fn investment_detail(conn: &Connection, id: &str) -> Result<Value> {
    let id = id.to_string();
    let derived_avg = crate::services::cost_basis::cost_basis_for_all(
        conn,
        crate::services::cost_basis::TxTable::Stocks,
    )?
    .get(&id)
    .map(|p| p.average_price_native().to_string())
    .unwrap_or_else(|| "0".to_string());
    let inv = conn.query_row(
        "SELECT si.id, si.ticker, si.company_name, si.quantity,
                si.currency,
                CASE WHEN spo.price IS NOT NULL AND (sd.fetched_at IS NULL OR sd.fetched_at <= spo.updated_at) THEN spo.price ELSE sd.original_price END AS current_price,
                COALESCE(CASE WHEN spo.price IS NOT NULL AND (sd.fetched_at IS NULL OR sd.fetched_at <= spo.updated_at) THEN spo.currency ELSE sd.currency END, 'USD') AS price_currency
         FROM stock_investments si
         LEFT JOIN stock_data sd ON si.ticker = sd.ticker
         LEFT JOIN stock_price_overrides spo ON si.ticker = spo.ticker
         WHERE si.id = ?",
        [&id],
        |row| Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "ticker": row.get::<_, String>(1)?,
            "companyName": row.get::<_, String>(2)?,
            "quantity": row.get::<_, String>(3)?,
            "averagePrice": "",
            "currency": row.get::<_, String>(4)?,
            "currentPrice": sql_to_json(row.get::<_, rusqlite::types::Value>(5).unwrap_or(rusqlite::types::Value::Null)),
            "priceCurrency": row.get::<_, String>(6)?,
        })),
    );
    match inv {
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Ok(serde_json::json!({ "error": format!("Investment {} not found", id) }))
        }
        Err(e) => Err(e.into()),
        Ok(mut obj) => {
            obj["averagePrice"] = serde_json::json!(derived_avg);
            let txs: Vec<Value> = {
                let mut s = conn.prepare(
                    "SELECT id, type, ticker, company_name, quantity, price_per_unit,
                            currency, transaction_date, created_at
                     FROM investment_transactions WHERE investment_id = ?
                     ORDER BY transaction_date DESC, created_at DESC, rowid DESC",
                )?;
                let result = s
                    .query_map([&id], |row| {
                        Ok(serde_json::json!({
                            "id": row.get::<_, String>(0)?,
                            "type": row.get::<_, String>(1)?,
                            "ticker": row.get::<_, String>(2)?,
                            "companyName": row.get::<_, String>(3)?,
                            "quantity": row.get::<_, String>(4)?,
                            "pricePerUnit": row.get::<_, String>(5)?,
                            "currency": row.get::<_, String>(6)?,
                            "transactionDate": row.get::<_, i64>(7)?,
                            "createdAt": row.get::<_, i64>(8)?,
                        }))
                    })?
                    .filter_map(|r| r.ok())
                    .collect();
                result
            };
            obj["transactions"] = Value::Array(txs);
            Ok(obj)
        }
    }
}

pub fn stock_transactions(conn: &Connection, ticker: Option<String>, limit: i64) -> Result<Value> {
    let sql = if ticker.is_some() {
        "SELECT id, investment_id, type, ticker, company_name, quantity,
            price_per_unit, currency, transaction_date, created_at
     FROM investment_transactions WHERE ticker = ? ORDER BY transaction_date DESC, created_at DESC, rowid DESC LIMIT ?"
    } else {
        "SELECT id, investment_id, type, ticker, company_name, quantity,
            price_per_unit, currency, transaction_date, created_at
     FROM investment_transactions ORDER BY transaction_date DESC, created_at DESC, rowid DESC LIMIT ?"
    };
    let rows: Vec<Value> = if let Some(ref t) = ticker {
        let mut stmt = conn.prepare(sql)?;
        let result = stmt
            .query_map(rusqlite::params![t, limit], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "investmentId": row.get::<_, String>(1)?,
                    "type": row.get::<_, String>(2)?,
                    "ticker": row.get::<_, String>(3)?,
                    "companyName": row.get::<_, String>(4)?,
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
        let mut stmt = conn.prepare(sql)?;
        let result = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "investmentId": row.get::<_, String>(1)?,
                    "type": row.get::<_, String>(2)?,
                    "ticker": row.get::<_, String>(3)?,
                    "companyName": row.get::<_, String>(4)?,
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

pub fn stock_value_history(
    conn: &Connection,
    ticker: &str,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Value> {
    ticker_value_history(conn, "stock_value_history", ticker, start_date, end_date)
}

// ============================================================================
// Write tools (Task 9: mirrors the canonical bulk engine in services/mcp/bank_accounts.rs)
// ============================================================================

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StockTransactionRow {
    #[schemars(description = "Ticker symbol, e.g. AAPL")]
    pub ticker: String,
    #[serde(rename = "companyName")]
    #[schemars(
        description = "Company name; used when the holding is created by this import. Defaults to the ticker"
    )]
    pub company_name: Option<String>,
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
pub struct StockTransactionsCreateArgs {
    #[schemars(length(max = MAX_BULK_ROWS))]
    pub transactions: Vec<StockTransactionRow>,
}

/// Two-phase bulk import through the shared writer
/// (`services::investments::bulk_create_stock_transactions`, ADR 0007). Phase
/// 1: every row field-validates. Phase 2: one SQL transaction; rows sorted by
/// date (buys before sells within a date — a sell may need a position created
/// earlier in the same batch); duplicates skipped; business-rule failures
/// mid-batch (sell without position, currency mismatch) roll the WHOLE batch
/// back and are reported as that row's error.
pub fn stock_transactions_create(
    conn: &mut rusqlite::Connection,
    args: &StockTransactionsCreateArgs,
) -> Result<BulkWriteReport> {
    super::check_bulk_size(args.transactions.len())?;

    let rows: Vec<investment_service::BulkStockRow> = args
        .transactions
        .iter()
        .map(|row| investment_service::BulkStockRow {
            ticker: row.ticker.clone(),
            company_name: row.company_name.clone(),
            tx_type: row.tx_type.clone(),
            quantity: row.quantity.clone(),
            price_per_unit: row.price_per_unit.clone(),
            currency: row.currency.clone(),
            transaction_date: row.transaction_date,
            external_id: None,
            allow_duplicate: false,
        })
        .collect();
    let report = investment_service::bulk_create_stock_transactions(conn, &rows, None)?;

    Ok(BulkWriteReport {
        created: report.created,
        skipped_duplicates: report
            .skipped_duplicates
            .into_iter()
            .map(|row| SkippedRow::new(row.index, row.message))
            .collect(),
        errors: report
            .errors
            .into_iter()
            .map(|row| RowError {
                index: row.index,
                message: row.message,
            })
            .collect(),
    })
}

/// Distinct tickers touched by this batch, for the data-changed event payload.
/// The second element (a per-ticker CoinGecko id) is always `None` for stocks;
/// it exists so the event helper shares one shape with the crypto tool.
pub fn distinct_tickers(args: &StockTransactionsCreateArgs) -> Vec<(String, Option<String>)> {
    let mut v: Vec<(String, Option<String>)> = args
        .transactions
        .iter()
        .map(|r| (r.ticker.to_uppercase(), None))
        .collect();
    v.sort();
    v.dedup();
    v
}

pub fn earliest_date(args: &StockTransactionsCreateArgs) -> Option<i64> {
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
            CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
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
                investment_id TEXT NOT NULL REFERENCES stock_investments(id) ON DELETE CASCADE,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                company_name TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                import_batch_id TEXT REFERENCES stock_import_batches(id) ON DELETE SET NULL,
                external_id TEXT
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn row(
        ticker: &str,
        tx_type: &str,
        quantity: &str,
        price: &str,
        currency: &str,
        date: i64,
    ) -> StockTransactionRow {
        StockTransactionRow {
            ticker: ticker.to_string(),
            company_name: None,
            tx_type: tx_type.to_string(),
            quantity: quantity.to_string(),
            price_per_unit: price.to_string(),
            currency: currency.to_string(),
            transaction_date: date,
        }
    }

    fn holding_quantity(conn: &Connection, ticker: &str) -> Option<String> {
        conn.query_row(
            "SELECT quantity FROM stock_investments WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .ok()
    }

    fn holding_average_price(conn: &Connection, ticker: &str) -> Option<String> {
        let id: String = conn
            .query_row(
                "SELECT id FROM stock_investments WHERE ticker = ?1",
                [ticker],
                |r| r.get(0),
            )
            .ok()?;
        crate::services::cost_basis::cost_basis_for_all(
            conn,
            crate::services::cost_basis::TxTable::Stocks,
        )
        .ok()?
        .get(&id)
        .map(|p| p.average_price_native().to_string())
    }

    fn holding_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM stock_investments", [], |r| r.get(0))
            .unwrap()
    }

    fn tx_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM investment_transactions", [], |r| {
            r.get(0)
        })
        .unwrap()
    }

    #[test]
    fn happy_path_auto_creates_holding_and_recalculates() {
        let mut conn = setup_test_db();
        let args = StockTransactionsCreateArgs {
            transactions: vec![
                row("aapl", "buy", "1", "100", "USD", 1_700_000_000),
                row("aapl", "buy", "2", "110", "USD", 1_700_100_000),
                row("aapl", "buy", "3", "120", "USD", 1_700_200_000),
            ],
        };
        let report = stock_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 3);
        assert!(report.skipped_duplicates.is_empty());
        assert!(report.errors.is_empty());
        assert_eq!(holding_count(&conn), 1);
        assert_eq!(tx_count(&conn), 3);
        assert_eq!(holding_quantity(&conn, "AAPL"), Some("6".to_string()));
    }

    /// Carry-forward note 3: `created_at` is second-granularity, so a bulk
    /// batch stamps every row identically, tying with `transaction_date` for a
    /// same-date buy+sell pair. `recalculate_investment_metrics` breaks that
    /// tie with `rowid ASC` — and rowid is insertion order, which is this
    /// module's buys-before-sells sort — so the recalc processes the buy
    /// before the sell regardless of input order. `average_price` is the
    /// order-sensitive observable here (unlike quantity, which is commutative):
    /// buy-then-sell keeps the average at the buy price; sell-then-buy would
    /// have hit the `total_qty > 0` guard and skipped the cost-basis
    /// reduction entirely, landing on a different average.
    #[test]
    fn intra_batch_same_day_buy_then_sell_computes_correct_average_price() {
        let mut conn = setup_test_db();
        // Sell listed BEFORE buy in the input to prove our sort (and the
        // recalc's rowid tiebreak) — not argument order — controls processing
        // order. Buy 10 @ 100, sell 4 @ 110, both dated the same second.
        let args = StockTransactionsCreateArgs {
            transactions: vec![
                row("aapl", "sell", "4", "110", "USD", 1_700_000_000),
                row("aapl", "buy", "10", "100", "USD", 1_700_000_000),
            ],
        };
        let report = stock_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 2, "errors: {:?}", report.errors);
        assert!(report.errors.is_empty());
        assert_eq!(holding_quantity(&conn, "AAPL"), Some("6".to_string()));
        // Buy-first: cost basis 10*100=1000, avg=100; sell 4 at avg cost
        // reduces cost basis by 4*100=400 -> 600 over 6 units -> avg still
        // 100. Sell-first would have left the average at ~166.67 instead.
        assert_eq!(
            holding_average_price(&conn, "AAPL"),
            Some("100".to_string())
        );
    }

    #[test]
    fn sell_without_position_rolls_back_whole_batch() {
        let mut conn = setup_test_db();
        // Seed an existing AAPL position so we can prove it's untouched too.
        let seed = StockTransactionsCreateArgs {
            transactions: vec![row("aapl", "buy", "5", "100", "USD", 1_699_000_000)],
        };
        assert_eq!(
            stock_transactions_create(&mut conn, &seed).unwrap().created,
            1
        );

        let args = StockTransactionsCreateArgs {
            transactions: vec![
                row("aapl", "buy", "3", "150", "USD", 1_700_000_000), // would succeed alone
                row("msft", "sell", "2", "300", "USD", 1_700_100_000), // no position -> fails
            ],
        };
        let report = stock_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert!(report.skipped_duplicates.is_empty());
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 1);
        // Only the pre-seeded AAPL holding exists; MSFT was never created and
        // AAPL's quantity was NOT bumped by the buy that got rolled back.
        assert_eq!(holding_count(&conn), 1);
        assert_eq!(holding_quantity(&conn, "AAPL"), Some("5".to_string()));
        assert_eq!(tx_count(&conn), 1);
    }

    #[test]
    fn duplicate_row_is_skipped_not_inserted() {
        let mut conn = setup_test_db();
        let seed = StockTransactionsCreateArgs {
            transactions: vec![row("aapl", "buy", "10", "100", "USD", 1_700_000_000)],
        };
        assert_eq!(
            stock_transactions_create(&mut conn, &seed).unwrap().created,
            1
        );

        let args = StockTransactionsCreateArgs {
            transactions: vec![
                row("aapl", "buy", "10", "100", "USD", 1_700_000_000), // exact duplicate
                row("aapl", "buy", "5", "105", "USD", 1_700_100_000),  // distinct
            ],
        };
        let report = stock_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 1);
        assert!(report.errors.is_empty());
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert_eq!(report.skipped_duplicates[0].index, 0);
        assert_eq!(tx_count(&conn), 2);
        assert_eq!(holding_quantity(&conn, "AAPL"), Some("15".to_string()));
    }

    #[test]
    fn empty_batch_creates_nothing() {
        let mut conn = setup_test_db();
        let args = StockTransactionsCreateArgs {
            transactions: vec![],
        };
        let report = stock_transactions_create(&mut conn, &args).unwrap();
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
                "aapl",
                "buy",
                "1",
                "100",
                "USD",
                1_700_000_000 + i as i64,
            ));
        }
        let args = StockTransactionsCreateArgs { transactions };
        let err = stock_transactions_create(&mut conn, &args).unwrap_err();
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
        let mut bad = row("aapl", "buy", "10", "100", "USD", 1_700_000_000);
        bad.tx_type = "banana".into();
        let args = StockTransactionsCreateArgs {
            transactions: vec![bad, row("msft", "buy", "5", "200", "USD", 1_700_100_000)],
        };
        let report = stock_transactions_create(&mut conn, &args).unwrap();
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
    fn currency_mismatch_rolls_back_whole_batch() {
        let mut conn = setup_test_db();
        let seed = StockTransactionsCreateArgs {
            transactions: vec![row("aapl", "buy", "5", "100", "USD", 1_699_000_000)],
        };
        assert_eq!(
            stock_transactions_create(&mut conn, &seed).unwrap().created,
            1
        );

        let args = StockTransactionsCreateArgs {
            transactions: vec![row("aapl", "buy", "1", "100", "EUR", 1_700_000_000)],
        };
        let report = stock_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 1);
        assert!(report.errors[0].message.contains("Currency mismatch"));
        assert_eq!(holding_quantity(&conn, "AAPL"), Some("5".to_string()));
    }

    #[test]
    fn distinct_tickers_dedupes_and_uppercases() {
        let args = StockTransactionsCreateArgs {
            transactions: vec![
                row("aapl", "buy", "1", "100", "USD", 1),
                row("AAPL", "sell", "1", "100", "USD", 2),
                row("msft", "buy", "1", "100", "USD", 3),
            ],
        };
        let tickers = distinct_tickers(&args);
        assert_eq!(
            tickers,
            vec![("AAPL".to_string(), None), ("MSFT".to_string(), None),]
        );
    }

    #[test]
    fn earliest_date_picks_the_minimum() {
        let args = StockTransactionsCreateArgs {
            transactions: vec![
                row("aapl", "buy", "1", "100", "USD", 500),
                row("aapl", "buy", "1", "100", "USD", 100),
                row("aapl", "buy", "1", "100", "USD", 300),
            ],
        };
        assert_eq!(earliest_date(&args), Some(100));
    }
}

#[cfg(test)]
mod value_history_tests {
    use super::*;

    const DAY: i64 = 86_400;

    /// Production column types: every money cell is TEXT.
    fn setup_db(main_currency: &str) -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE exchange_rates (
                currency TEXT PRIMARY KEY,
                rate REAL NOT NULL,
                fetched_at INTEGER NOT NULL
            );
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            CREATE TABLE stock_value_history (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL,
                recorded_at INTEGER NOT NULL,
                value_czk TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price TEXT NOT NULL,
                currency TEXT NOT NULL,
                is_stale INTEGER NOT NULL DEFAULT 0,
                UNIQUE(ticker, recorded_at)
            );
            INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('EUR', 25.0, 0);
            INSERT INTO exchange_rate_history (date, currency, rate) VALUES
                (100 * 86400, 'EUR', 24.0), (101 * 86400, 'EUR', 26.0);
            -- the same 2400 CZK value on two days: only the day rate differs
            INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                VALUES ('v1', 'AAPL', 100 * 86400, '2400', '10', '12', 'USD'),
                       ('v2', 'AAPL', 101 * 86400, '2400', '10', '12', 'USD'),
                       ('v3', 'OTHER', 101 * 86400, '9999', '1', '1', 'USD');
            "#,
        )
        .expect("schema + seed");
        conn.execute(
            "INSERT INTO user_profile (currency) VALUES (?1)",
            [main_currency],
        )
        .expect("profile");
        conn
    }

    #[test]
    fn value_history_converts_each_row_at_its_own_day_rate() {
        let conn = setup_db("EUR");

        let result = stock_value_history(&conn, "AAPL", None, None).expect("history");

        assert_eq!(result["mainCurrency"], "EUR");
        let rows = result["history"].as_array().expect("history array");
        assert_eq!(rows.len(), 2);
        // Oldest first, as before.
        assert_eq!(rows[0]["recordedAt"], 100 * DAY);
        assert_eq!(rows[0]["value"], "100.00");
        assert_eq!(rows[1]["recordedAt"], 101 * DAY);
        assert_eq!(rows[1]["value"], "92.31");
    }

    #[test]
    fn value_history_keeps_the_native_price_and_currency() {
        let conn = setup_db("EUR");

        let result = stock_value_history(&conn, "AAPL", None, None).expect("history");

        let first = &result["history"][0];
        assert_eq!(first["price"], "12");
        assert_eq!(first["currency"], "USD");
        assert_eq!(first["quantity"], "10");
        assert!(first.get("valueCzk").is_none());
    }

    #[test]
    fn value_history_stays_in_czk_for_a_czk_main_currency() {
        let conn = setup_db("CZK");

        let result = stock_value_history(&conn, "AAPL", None, None).expect("history");

        assert_eq!(result["mainCurrency"], "CZK");
        assert_eq!(result["history"][0]["value"], "2400");
        assert_eq!(result["history"][1]["value"], "2400");
    }

    #[test]
    fn value_history_applies_the_date_window() {
        let conn = setup_db("EUR");

        let result = stock_value_history(&conn, "AAPL", Some(101 * DAY), None).expect("history");

        let rows = result["history"].as_array().expect("history array");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["recordedAt"], 101 * DAY);
    }

    #[test]
    fn value_history_of_an_unknown_ticker_is_empty_but_names_the_currency() {
        let conn = setup_db("EUR");

        let result = stock_value_history(&conn, "NOPE", None, None).expect("history");

        assert_eq!(result["mainCurrency"], "EUR");
        assert_eq!(result["history"], serde_json::json!([]));
    }
}
