//! MCP tools: investments. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use std::collections::HashMap;

use crate::error::Result;
use crate::services::investments as investment_service;
use crate::services::positions::{AssetKind, PositionValue};

use super::money::{add_position_fields, main_currency_positions, positions_response};
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

/// Every stock position with its native price and average cost plus the value, cost basis
/// and gains in the user's main currency (the app's positions service and current snapshot).
pub fn investments_list(conn: &Connection) -> Result<Value> {
    let positions = main_currency_positions(conn, AssetKind::Stock)?;
    let by_id: HashMap<&str, &PositionValue> = positions
        .items
        .iter()
        .map(|item| (item.investment_id.as_str(), item))
        .collect();
    let mut stmt = conn.prepare(
        "SELECT si.id, si.ticker, si.company_name, si.quantity,
                si.currency,
                sd.short_name, sd.sector, sd.industry, sd.pe_ratio, sd.market_cap, sd.beta,
                sd.fifty_two_week_high, sd.fifty_two_week_low, sd.fetched_at
         FROM stock_investments si
         LEFT JOIN stock_data sd ON si.ticker = sd.ticker
         ORDER BY si.company_name",
    )?;
    let rows: Vec<Value> = stmt.query_map([], |row| {
        Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "ticker": row.get::<_, String>(1)?,
            "companyName": row.get::<_, String>(2)?,
            "quantity": row.get::<_, String>(3)?,
            "currency": row.get::<_, String>(4)?,
            "shortName": sql_to_json(row.get::<_, rusqlite::types::Value>(5).unwrap_or(rusqlite::types::Value::Null)),
            "sector": sql_to_json(row.get::<_, rusqlite::types::Value>(6).unwrap_or(rusqlite::types::Value::Null)),
            "industry": sql_to_json(row.get::<_, rusqlite::types::Value>(7).unwrap_or(rusqlite::types::Value::Null)),
            "peRatio": sql_to_json(row.get::<_, rusqlite::types::Value>(8).unwrap_or(rusqlite::types::Value::Null)),
            "marketCap": sql_to_json(row.get::<_, rusqlite::types::Value>(9).unwrap_or(rusqlite::types::Value::Null)),
            "beta": sql_to_json(row.get::<_, rusqlite::types::Value>(10).unwrap_or(rusqlite::types::Value::Null)),
            "fiftyTwoWeekHigh": sql_to_json(row.get::<_, rusqlite::types::Value>(11).unwrap_or(rusqlite::types::Value::Null)),
            "fiftyTwoWeekLow": sql_to_json(row.get::<_, rusqlite::types::Value>(12).unwrap_or(rusqlite::types::Value::Null)),
            "priceFetchedAt": sql_to_json(row.get::<_, rusqlite::types::Value>(13).unwrap_or(rusqlite::types::Value::Null)),
        }))
    })?.filter_map(|r| r.ok()).collect();
    let rows: Vec<Value> = rows
        .into_iter()
        .map(|mut v| {
            let item = v["id"].as_str().and_then(|id| by_id.get(id).copied());
            add_position_fields(&mut v, item, &positions.currency);
            v
        })
        .collect();
    Ok(positions_response(&positions, "investments", rows))
}

pub fn investment_detail(conn: &Connection, id: &str) -> Result<Value> {
    let id = id.to_string();
    let positions = main_currency_positions(conn, AssetKind::Stock)?;
    let item = positions.items.iter().find(|item| item.investment_id == id);
    let inv = conn.query_row(
        "SELECT si.id, si.ticker, si.company_name, si.quantity, si.currency
         FROM stock_investments si
         WHERE si.id = ?",
        [&id],
        |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "ticker": row.get::<_, String>(1)?,
                "companyName": row.get::<_, String>(2)?,
                "quantity": row.get::<_, String>(3)?,
                "averagePrice": "",
                "currency": row.get::<_, String>(4)?,
            }))
        },
    );
    match inv {
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Ok(serde_json::json!({ "error": format!("Investment {} not found", id) }))
        }
        Err(e) => Err(e.into()),
        Ok(mut obj) => {
            add_position_fields(&mut obj, item, &positions.currency);
            obj["mainCurrency"] = serde_json::json!(positions.currency);
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
    ticker_value_history(
        conn,
        crate::services::pricing::AssetKind::Stock,
        ticker,
        start_date,
        end_date,
    )
}

// ============================================================================
// Write tools (Task 9: mirrors the canonical bulk engine in services/mcp/bank_accounts.rs)
// ============================================================================

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
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
    #[serde(rename = "externalId")]
    #[schemars(
        description = "Stable source transaction identifier; repeated IDs are deduplicated"
    )]
    pub external_id: Option<String>,
    #[serde(rename = "importAnyway", default)]
    #[schemars(
        description = "Import a genuine identical fill even when its values match an existing transaction"
    )]
    pub import_anyway: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct StockTransactionsCreateArgs {
    #[schemars(length(max = MAX_BULK_ROWS))]
    pub transactions: Vec<StockTransactionRow>,
    #[serde(rename = "fileName")]
    #[schemars(description = "Name of the source file for the undoable stock import history")]
    pub file_name: Option<String>,
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
            external_id: row
                .external_id
                .as_deref()
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(|id| format!("mcp:{id}")),
            allow_duplicate: row.import_anyway,
        })
        .collect::<Vec<_>>();
    let batch = investment_service::NewStockImportBatch {
        file_name: args
            .file_name
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .unwrap_or("MCP import")
            .to_string(),
        source: "mcp".to_string(),
    };
    let report = investment_service::bulk_create_stock_transactions(conn, &rows, Some(&batch))?;

    Ok(BulkWriteReport {
        import_batch_id: report.batch_id,
        promoted: 0,
        created: report.created,
        skipped_duplicates: report
            .skipped_duplicates
            .into_iter()
            .map(|row| {
                if row.possible_duplicate {
                    SkippedRow::possible_duplicate(row.index, row.message)
                } else {
                    SkippedRow::new(row.index, row.message)
                }
            })
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
            external_id: None,
            import_anyway: false,
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
            "CZK",
            &crate::services::fx::current(),
        )
        .ok()?
        .0
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
            file_name: None,
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
            file_name: None,
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
            file_name: None,
            transactions: vec![row("aapl", "buy", "5", "100", "USD", 1_699_000_000)],
        };
        assert_eq!(
            stock_transactions_create(&mut conn, &seed).unwrap().created,
            1
        );

        let args = StockTransactionsCreateArgs {
            file_name: None,
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
            file_name: None,
            transactions: vec![row("aapl", "buy", "10", "100", "USD", 1_700_000_000)],
        };
        assert_eq!(
            stock_transactions_create(&mut conn, &seed).unwrap().created,
            1
        );

        let args = StockTransactionsCreateArgs {
            file_name: None,
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
            file_name: None,
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
        let args = StockTransactionsCreateArgs {
            transactions,
            file_name: None,
        };
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
            file_name: None,
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
            file_name: None,
            transactions: vec![row("aapl", "buy", "5", "100", "USD", 1_699_000_000)],
        };
        assert_eq!(
            stock_transactions_create(&mut conn, &seed).unwrap().created,
            1
        );

        let args = StockTransactionsCreateArgs {
            file_name: None,
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
            file_name: None,
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
    fn integrity_oversell_rolls_back_even_when_later_buy_covers_it() {
        let mut conn = setup_test_db();
        let args = StockTransactionsCreateArgs {
            file_name: None,
            transactions: vec![
                row("AAPL", "buy", "1", "100", "USD", 1_700_000_000),
                row("AAPL", "sell", "2", "100", "USD", 1_700_100_000),
                row("AAPL", "buy", "3", "100", "USD", 1_700_200_000),
            ],
        };
        let report = stock_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert!(!report.errors.is_empty());
        assert_eq!(tx_count(&conn), 0);
    }

    #[test]
    fn integrity_backdated_sell_protects_existing_future_sales() {
        let mut conn = setup_test_db();
        stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: None,
                transactions: vec![
                    row("AAPL", "buy", "10", "100", "USD", 1_700_000_000),
                    row("AAPL", "sell", "8", "100", "USD", 1_700_200_000),
                ],
            },
        )
        .unwrap();
        let report = stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: None,
                transactions: vec![row("AAPL", "sell", "5", "100", "USD", 1_700_100_000)],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert!(!report.errors.is_empty());
        assert_eq!(holding_quantity(&conn, "AAPL"), Some("2".into()));
    }

    #[test]
    fn integrity_duplicate_uses_numeric_values_and_currency() {
        let mut conn = setup_test_db();
        stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: None,
                transactions: vec![row("AAPL", "buy", "10", "100", "USD", 1_700_000_000)],
            },
        )
        .unwrap();
        let report = stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: None,
                transactions: vec![row("AAPL", "buy", "10.0", "1e2", "USD", 1_700_000_000)],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.skipped_duplicates.len(), 1);
        let report = stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: None,
                transactions: vec![
                    row("AAPL", "buy", "10", "100", "EUR", 1_700_000_000),
                    row("AAPL", "buy", "1", "100", "USD", 1_700_100_000),
                ],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert!(!report.errors.is_empty());
        assert_eq!(tx_count(&conn), 1);
    }

    #[test]
    fn integrity_mcp_stock_import_records_owned_batch() {
        let mut conn = setup_test_db();
        let report = stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: None,
                transactions: vec![row("AAPL", "buy", "1", "100", "USD", 1_700_000_000)],
            },
        )
        .unwrap();
        assert_eq!(report.created, 1);
        let (source, count, owned): (String, i64, i64) = conn.query_row(
            "SELECT source, trade_count, (SELECT count(*) FROM investment_transactions WHERE import_batch_id = stock_import_batches.id) FROM stock_import_batches", [],
            |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?))).unwrap();
        assert_eq!((source, count, owned), ("mcp".into(), 1, 1));
    }

    #[test]
    fn integrity_identical_fills_need_explicit_override_and_external_ids_are_stable() {
        let mut conn = setup_test_db();
        let mut first = row("AAPL", "buy", "1", "100", "USD", 1_700_000_000);
        first.external_id = Some("trade-1".into());
        let report = stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: Some("broker.csv".into()),
                transactions: vec![first.clone()],
            },
        )
        .unwrap();
        assert!(report.import_batch_id.is_some());
        let duplicate = stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: None,
                transactions: vec![first.clone()],
            },
        )
        .unwrap();
        assert_eq!(duplicate.created, 0);
        assert_eq!(duplicate.skipped_duplicates.len(), 1);
        first.import_anyway = true;
        first.external_id = Some("trade-2".into());
        let second = stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: None,
                transactions: vec![first.clone()],
            },
        )
        .unwrap();
        assert_eq!(second.created, 1);
        assert_eq!(holding_quantity(&conn, "AAPL"), Some("2".into()));
        first.price_per_unit = "101".into();
        let conflict = stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                file_name: None,
                transactions: vec![first],
            },
        )
        .unwrap();
        assert_eq!(conflict.created, 0);
        assert_eq!(conflict.errors.len(), 1);
    }

    #[test]
    fn earliest_date_picks_the_minimum() {
        let args = StockTransactionsCreateArgs {
            file_name: None,
            transactions: vec![
                row("aapl", "buy", "1", "100", "USD", 500),
                row("aapl", "buy", "1", "100", "USD", 100),
                row("aapl", "buy", "1", "100", "USD", 300),
            ],
        };
        assert_eq!(earliest_date(&args), Some(100));
    }
    #[test]
    fn source_ids_preserve_distinct_identical_fills_and_id_retries_are_certain() {
        let mut conn = setup_test_db();
        let mut a = row("AAPL", "buy", "1", "100", "USD", 1_700_000_000);
        a.external_id = Some("fill-1".into());
        let mut b = a.clone();
        b.external_id = Some("fill-2".into());
        let args = StockTransactionsCreateArgs {
            transactions: vec![a, b],
            file_name: Some("fills.csv".into()),
        };
        let report = stock_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 2, "{report:?}");
        let batch_id = report.import_batch_id.unwrap();
        let (name, count): (String, i64) = conn
            .query_row(
                "SELECT file_name,trade_count FROM stock_import_batches WHERE id=?1",
                [&batch_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((name.as_str(), count), ("fills.csv", 2));
        let retry = stock_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(retry.created, 0);
        assert_eq!(retry.skipped_duplicates.len(), 2);
        assert!(retry
            .skipped_duplicates
            .iter()
            .all(|row| !row.possible_duplicate));
        assert!(retry.import_batch_id.is_none());
        let undone =
            crate::services::stock_import::batches::undo_batch(&mut conn, &batch_id).unwrap();
        assert_eq!(undone.removed, 2);
        assert_eq!(tx_count(&conn), 0);
        assert_eq!(holding_count(&conn), 0);
    }

    #[test]
    fn heuristic_duplicates_are_possible_and_can_be_overridden() {
        let mut conn = setup_test_db();
        let a = row("AAPL", "buy", "1", "100", "USD", 1_700_000_000);
        stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                transactions: vec![a.clone()],
                file_name: None,
            },
        )
        .unwrap();
        let report = stock_transactions_create(
            &mut conn,
            &StockTransactionsCreateArgs {
                transactions: vec![a.clone()],
                file_name: None,
            },
        )
        .unwrap();
        assert!(report.skipped_duplicates[0].possible_duplicate);
        let mut forced = a;
        forced.import_anyway = true;
        assert_eq!(
            stock_transactions_create(
                &mut conn,
                &StockTransactionsCreateArgs {
                    transactions: vec![forced],
                    file_name: None
                }
            )
            .unwrap()
            .created,
            1
        );
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
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0
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
                quantity TEXT NOT NULL,
                price TEXT NOT NULL,
                currency TEXT NOT NULL,
                is_stale INTEGER NOT NULL DEFAULT 0,
                UNIQUE(ticker, recorded_at)
            );
            INSERT INTO exchange_rate_history (date, currency, rate) VALUES
                (100 * 86400, 'CZK', 24.0), (100 * 86400, 'USD', 1.2), (101 * 86400, 'CZK', 26.0), (101 * 86400, 'USD', 1.3);
            -- the same 120 USD value on two days: only the day rate of the main currency differs
            INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
                VALUES ('v1', 'AAPL', 100 * 86400, '10', '12', 'USD'),
                       ('v2', 'AAPL', 101 * 86400, '10', '12', 'USD'),
                       ('v3', 'OTHER', 101 * 86400, '1', '1', 'USD');
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
        assert_eq!(result["fxEstimated"], false);
    }

    #[test]
    fn value_history_keeps_the_native_price_and_currency() {
        let conn = setup_db("EUR");

        let result = stock_value_history(&conn, "AAPL", None, None).expect("history");

        let first = &result["history"][0];
        assert_eq!(first["price"], "12");
        assert_eq!(first["currency"], "USD");
        assert_eq!(first["quantity"], "10");
        assert_eq!(first["isStale"], false);
    }

    #[test]
    fn value_history_stays_in_czk_for_a_czk_main_currency() {
        let conn = setup_db("CZK");

        let result = stock_value_history(&conn, "AAPL", None, None).expect("history");

        assert_eq!(result["mainCurrency"], "CZK");
        assert_eq!(result["history"][0]["value"], "2400.00");
        assert_eq!(result["history"][1]["value"], "2400.00");
        assert_eq!(result["missingCurrencies"], serde_json::json!([]));
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

    #[test]
    fn value_history_names_the_currency_it_cannot_convert() {
        let conn = setup_db("EUR");
        conn.execute(
            "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
             VALUES ('v4', 'AAPL', 102 * 86400, '1', '5', 'XAU')",
            [],
        )
        .expect("row");

        let result = stock_value_history(&conn, "AAPL", None, None).expect("history");

        assert_eq!(result["history"][2]["value"], serde_json::Value::Null);
        assert_eq!(result["history"][2]["price"], "5");
        assert_eq!(result["missingCurrencies"], serde_json::json!(["XAU"]));
    }
}

#[cfg(test)]
mod list_missing_currency_tests {
    use super::*;

    fn list_db(main_currency: &str) -> Connection {
        let conn = Connection::open_in_memory().expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_investments (id TEXT PRIMARY KEY, ticker TEXT, company_name TEXT, quantity TEXT, currency TEXT);
            CREATE TABLE stock_data (ticker TEXT PRIMARY KEY, original_price TEXT, currency TEXT, short_name TEXT, sector TEXT, industry TEXT, pe_ratio TEXT, market_cap TEXT, beta TEXT, fifty_two_week_high TEXT, fifty_two_week_low TEXT, fetched_at INTEGER);
            CREATE TABLE stock_price_overrides (ticker TEXT PRIMARY KEY, price TEXT, currency TEXT, updated_at INTEGER);
            CREATE TABLE dividend_data (ticker TEXT PRIMARY KEY, yearly_dividend_sum TEXT, currency TEXT);
            CREATE TABLE dividend_overrides (ticker TEXT PRIMARY KEY, yearly_dividend_sum TEXT, currency TEXT);
            CREATE TABLE investment_transactions (id TEXT PRIMARY KEY, investment_id TEXT, type TEXT, ticker TEXT, company_name TEXT, quantity TEXT, price_per_unit TEXT, currency TEXT, transaction_date INTEGER, created_at INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE exchange_rate_history (date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL, PRIMARY KEY (date, currency));
            CREATE TABLE user_profile (id INTEGER PRIMARY KEY AUTOINCREMENT, currency TEXT NOT NULL);
            "#,
        )
        .expect("schema");
        conn.execute(
            "INSERT INTO user_profile (currency) VALUES (?1)",
            [main_currency],
        )
        .expect("profile");
        conn
    }

    #[test]
    fn investments_list_declares_currencies_left_out_of_the_average() {
        let conn = list_db("CZK");
        conn.execute_batch(
            r#"
            INSERT INTO exchange_rate_history VALUES (1699920000, 'CZK', 25.0), (1699920000, 'USD', 1.25);
            INSERT INTO stock_investments VALUES ('a', 'AAA', 'A', '2', 'USD'), ('b', 'BBB', 'B', '1', 'USD');
            INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date) VALUES
                ('t1', 'a', 'buy', 'AAA', 'A', '1', '100', 'USD', 1700000000),
                ('t2', 'a', 'buy', 'AAA', 'A', '1', '50', 'KWD', 1700000000),
                ('t3', 'b', 'buy', 'BBB', 'B', '1', '10', 'USD', 1700000000);
            "#,
        )
        .expect("seed");
        crate::services::fx::set_current_for_test(&[("CZK", 25.0), ("USD", 1.25)]);

        let result = investments_list(&conn).expect("list");

        let rows = result["investments"].as_array().expect("array");
        let a = rows.iter().find(|r| r["id"] == "a").expect("a");
        let b = rows.iter().find(|r| r["id"] == "b").expect("b");
        assert_eq!(a["missingCurrencies"], serde_json::json!(["KWD"]));
        assert_eq!(a["averagePrice"], "100");
        assert!(a["costBasis"].is_null());
        assert_eq!(b["missingCurrencies"], serde_json::json!([]));
        assert_eq!(b["costBasis"], "200.00");
        assert_eq!(result["missingCurrencies"], serde_json::json!(["KWD"]));
    }

    #[test]
    fn investments_list_values_positions_in_the_main_currency() {
        // C1: bought 10 x 100 USD when USDCZK = EURCZK = 25; today USDCZK 25, EURCZK 22.
        let conn = list_db("EUR");
        conn.execute_batch(
            r#"
            INSERT INTO exchange_rate_history VALUES (1699920000, 'CZK', 25.0), (1699920000, 'USD', 1.0);
            INSERT INTO stock_investments VALUES ('a', 'AAPL', 'Apple', '10', 'USD');
            INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date) VALUES
                ('t1', 'a', 'buy', 'AAPL', 'Apple', '10', '100', 'USD', 1700000000);
            INSERT INTO stock_data (ticker, original_price, currency, fetched_at) VALUES ('AAPL', '100', 'USD', 100);
            INSERT INTO dividend_data VALUES ('AAPL', '1', 'USD');
            "#,
        )
        .expect("seed");
        crate::services::fx::set_current_for_test(&[("CZK", 22.0), ("USD", 22.0 / 25.0)]);

        let result = investments_list(&conn).expect("list");

        assert_eq!(result["mainCurrency"], "EUR");
        let aapl = &result["investments"][0];
        assert_eq!(aapl["currentPrice"], "100");
        assert_eq!(aapl["priceCurrency"], "USD");
        assert_eq!(aapl["costBasis"], "1000.00");
        assert_eq!(aapl["value"], "1136.36");
        assert_eq!(aapl["unrealizedGain"], "136.36");
        assert_eq!(aapl["unrealizedGainPct"], "13.64");
        assert_eq!(aapl["annualDividend"], "11.36");
        assert_eq!(result["summary"]["totalValue"], "1136.36");
        assert_eq!(result["summary"]["annualDividends"], "11.36");
        assert_eq!(result["missingCurrencies"], serde_json::json!([]));
        assert_eq!(result["fxEstimated"], false);
        assert_eq!(aapl["fxEstimated"], false);
    }

    #[test]
    fn investments_list_reports_the_quantity_its_value_describes() {
        let conn = list_db("EUR");
        conn.execute_batch(
            r#"
            -- a stale stored quantity (metrics not recalculated): the transactions replay to 2
            INSERT INTO stock_investments VALUES ('a', 'SAP', 'SAP', '5', 'EUR'), ('b', 'NOTX', 'N', '3.50', 'EUR');
            INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date) VALUES
                ('t1', 'a', 'buy', 'SAP', 'SAP', '2', '100', 'EUR', 1700000000);
            INSERT INTO stock_data (ticker, original_price, currency, fetched_at) VALUES ('SAP', '120', 'EUR', 100);
            "#,
        )
        .expect("seed");

        let result = investments_list(&conn).expect("list");

        let rows = result["investments"].as_array().expect("array");
        let a = rows.iter().find(|r| r["id"] == "a").expect("a");
        assert_eq!(a["quantity"], "2");
        assert_eq!(a["value"], "240.00");
        // without transactions the stored text stays as it is
        let b = rows.iter().find(|r| r["id"] == "b").expect("b");
        assert_eq!(b["quantity"], "3.50");
        let detail = investment_detail(&conn, "a").expect("detail");
        assert_eq!(detail["quantity"], "2");
    }

    #[test]
    fn investments_list_flags_cost_estimated_at_todays_rate() {
        // No stored rate for the buy day: the cost falls back to today's quotes.
        let conn = list_db("EUR");
        conn.execute_batch(
            r#"
            INSERT INTO stock_investments VALUES ('a', 'AAPL', 'Apple', '10', 'USD');
            INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date) VALUES
                ('t1', 'a', 'buy', 'AAPL', 'Apple', '10', '100', 'USD', 1700000000);
            INSERT INTO stock_data (ticker, original_price, currency, fetched_at) VALUES ('AAPL', '100', 'USD', 100);
            "#,
        )
        .expect("seed");
        crate::services::fx::set_current_for_test(&[("CZK", 20.0), ("USD", 20.0 / 25.0)]);

        let result = investments_list(&conn).expect("list");

        assert_eq!(result["investments"][0]["costBasis"], "1250.00");
        assert_eq!(result["investments"][0]["fxEstimated"], true);
        assert_eq!(result["fxEstimated"], true);
        assert_eq!(result["missingCurrencies"], serde_json::json!([]));
    }
}
