//! MCP tools: crypto. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::{Connection, OptionalExtension};
use serde::Deserialize;
use serde_json::Value;

use std::collections::HashMap;

use crate::error::Result;
use crate::services::positions::{AssetKind, PositionValue};
use crate::services::{crypto_investments as crypto_service, dedup};

use super::money::{add_position_fields, main_currency_positions, positions_response};

use super::{
    sql_to_json, ticker_value_history, BulkWriteReport, RowError, SkippedRow, MAX_BULK_ROWS,
};

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

/// Every crypto position with its native price and average cost plus the value, cost basis
/// and gains in the user's main currency (the app's positions service and current snapshot).
pub fn crypto_list(conn: &Connection) -> Result<Value> {
    let positions = main_currency_positions(conn, AssetKind::Crypto)?;
    let by_id: HashMap<&str, &PositionValue> = positions
        .items
        .iter()
        .map(|item| (item.investment_id.as_str(), item))
        .collect();
    let mut stmt = conn.prepare(
        "SELECT ci.id, ci.ticker, ci.yahoo_symbol, ci.name, ci.quantity,
                ci.currency, cp.fetched_at
         FROM crypto_investments ci
         LEFT JOIN crypto_prices cp ON ci.ticker = cp.symbol
         ORDER BY ci.name",
    )?;
    let rows: Vec<Value> = stmt.query_map([], |row| Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "ticker": row.get::<_, String>(1)?,
        "yahooSymbol": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
        "name": row.get::<_, String>(3)?,
        "quantity": row.get::<_, String>(4)?,
        "currency": row.get::<_, String>(5)?,
        "priceFetchedAt": sql_to_json(row.get::<_, rusqlite::types::Value>(6).unwrap_or(rusqlite::types::Value::Null)),
    })))?.filter_map(|r| r.ok()).collect();
    let rows: Vec<Value> = rows
        .into_iter()
        .map(|mut v| {
            let item = v["id"].as_str().and_then(|id| by_id.get(id).copied());
            add_position_fields(&mut v, item, &positions.currency);
            v
        })
        .collect();
    Ok(positions_response(&positions, "crypto", rows))
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
    ticker_value_history(
        conn,
        crate::services::pricing::AssetKind::Crypto,
        ticker,
        start_date,
        end_date,
    )
}

// ============================================================================
// Write tools (Task 9: mirrors the canonical bulk engine in services/mcp/bank_accounts.rs
// and services/mcp/investments.rs::stock_transactions_create)
// ============================================================================

#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CryptoTransactionRow {
    #[schemars(description = "Ticker symbol, e.g. BTC")]
    pub ticker: String,
    #[schemars(
        description = "Coin name; used when the holding is created. Defaults to the ticker"
    )]
    pub name: Option<String>,
    #[serde(rename = "yahooSymbol")]
    #[schemars(
        description = "Yahoo Finance symbol of the coin's USD pair, e.g. BTC-USD or SUI20947-USD; when omitted Moony looks it up from the ticker and name"
    )]
    pub yahoo_symbol: Option<String>,
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
    #[serde(rename = "importAnyway", default)]
    #[schemars(
        description = "Import a genuine identical fill even when its values match an existing transaction"
    )]
    pub import_anyway: bool,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CryptoTransactionsCreateArgs {
    #[schemars(length(max = MAX_BULK_ROWS))]
    pub transactions: Vec<CryptoTransactionRow>,
}

/// The row's Yahoo symbol, normalized like every entry point does
/// (`crypto_symbols::optional_symbol`); `None` when absent, blank or invalid. An invalid one is
/// rejected in phase 1 (`validate_crypto_row`) before any caller reads it.
fn row_symbol(row: &CryptoTransactionRow) -> Option<String> {
    crate::services::crypto_symbols::optional_symbol(row.yahoo_symbol.as_deref())
        .ok()
        .flatten()
}

fn validate_crypto_row(row: &CryptoTransactionRow) -> Result<()> {
    crate::services::crypto_symbols::optional_symbol(row.yahoo_symbol.as_deref())?;
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
/// business-rule failures mid-batch (sell without position, conflicting
/// yahooSymbol) roll the WHOLE batch back and are reported as that row's error.
/// A new coin without a yahooSymbol is created unresolved; the price refresh
/// looks its symbol up (spec D4).
pub fn crypto_transactions_create(
    conn: &mut rusqlite::Connection,
    args: &CryptoTransactionsCreateArgs,
) -> Result<BulkWriteReport> {
    Ok(crypto_transactions_create_reporting(conn, args)?.0)
}

/// [`crypto_transactions_create`] that also returns the tickers whose supplied `yahooSymbol`
/// filled an existing holding that had none, in this committed batch (empty when the batch was
/// rejected). Such a holding's history sits on another series and its cached price belongs to
/// no symbol: the caller rebuilds it from its first transaction and drops the old price, like
/// the symbol picker does (spec D7 / D9). The wire report is unchanged.
pub fn crypto_transactions_create_reporting(
    conn: &mut rusqlite::Connection,
    args: &CryptoTransactionsCreateArgs,
) -> Result<(BulkWriteReport, Vec<String>)> {
    let mut symbols_set = Vec::new();
    let report = import_rows(conn, args, &mut symbols_set)?;
    if !report.errors.is_empty() {
        // Every rejection rolled the whole batch back.
        symbols_set.clear();
    }
    Ok((report, symbols_set))
}

fn import_rows(
    conn: &mut rusqlite::Connection,
    args: &CryptoTransactionsCreateArgs,
    symbols_set: &mut Vec<String>,
) -> Result<BulkWriteReport> {
    super::check_bulk_size(args.transactions.len())?;

    let mut errors: Vec<RowError> = Vec::new();
    for (i, row) in args.transactions.iter().enumerate() {
        if let Err(e) = validate_crypto_row(row) {
            errors.push(RowError {
                index: i,
                message: super::error_text::render(&e),
            });
        }
    }
    if !errors.is_empty() {
        return Ok(BulkWriteReport {
            import_batch_id: None,
            promoted: 0,
            created: 0,
            skipped_duplicates: vec![],
            errors,
        });
    }

    // Reject conflicting symbols across the entire request, including rows that
    // would otherwise be skipped as duplicates.
    let mut identities = std::collections::HashMap::<String, String>::new();
    for (index, row) in args.transactions.iter().enumerate() {
        if let Some(symbol) = row_symbol(row) {
            let ticker = row.ticker.to_uppercase();
            if identities
                .insert(ticker.clone(), symbol.clone())
                .is_some_and(|previous| previous != symbol)
            {
                return Ok(BulkWriteReport {
                    import_batch_id: None,
                    promoted: 0,
                    created: 0,
                    skipped_duplicates: vec![],
                    errors: vec![RowError {
                        index,
                        message: format!("{ticker}: conflicting yahooSymbol values in batch"),
                    }],
                });
            }
        }
    }

    let mut order: Vec<usize> = (0..args.transactions.len()).collect();
    order.sort_by_key(|&i| {
        let r = &args.transactions[i];
        (
            r.transaction_date.div_euclid(86_400),
            if r.tx_type.to_lowercase() == "buy" {
                0
            } else {
                1
            },
        )
    });

    let tx = conn.transaction()?;
    let mut written: Vec<(usize, String)> = Vec::new();
    let mut created = 0usize;
    let mut skipped: Vec<SkippedRow> = Vec::new();
    for &i in &order {
        let row = &args.transactions[i];
        let ticker_upper = row.ticker.to_uppercase();
        if let Some(provided) = row_symbol(row) {
            let stored: Option<String> = tx
                .query_row(
                    "SELECT yahoo_symbol FROM crypto_investments WHERE ticker = ?1",
                    [&ticker_upper],
                    |r| r.get::<_, Option<String>>(0),
                )
                .optional()?
                .flatten()
                // A blank stored symbol is unresolved, like `NULL`.
                .filter(|symbol| !symbol.trim().is_empty());
            if stored.is_some_and(|symbol| symbol != provided) {
                drop(tx);
                return Ok(BulkWriteReport {
                    import_batch_id: None,
                    promoted: 0,
                    created: 0,
                    skipped_duplicates: vec![],
                    errors: vec![RowError {
                        index: i,
                        message: format!(
                            "{ticker_upper}: yahooSymbol conflicts with stored holding"
                        ),
                    }],
                });
            }
        }
        if !row.import_anyway {
            if let Some(reason) = dedup::find_duplicate_crypto_transaction(
                &tx,
                &ticker_upper,
                crate::models::validation::utc_day(row.transaction_date, "validation.dateInvalid")?,
                &row.tx_type.to_lowercase(),
                &row.quantity,
                &row.price_per_unit,
                &row.currency,
            )? {
                skipped.push(SkippedRow::possible_duplicate(i, reason));
                continue;
            }
        }
        let name_or_ticker = row.name.as_deref().unwrap_or(&row.ticker);
        match crypto_service::import_crypto_transaction_deferred(
            &tx,
            &row.ticker,
            name_or_ticker,
            identities.get(&ticker_upper).map(String::as_str),
            &row.tx_type,
            &row.quantity,
            &row.price_per_unit,
            &row.currency,
            row.transaction_date,
        ) {
            Ok(holding) => {
                if holding.symbol_set_now && !symbols_set.contains(&ticker_upper) {
                    symbols_set.push(ticker_upper.clone());
                }
                let id = holding.id;
                if !written.iter().any(|(_, existing)| existing == &id) {
                    written.push((i, id));
                }
            }
            Err(e) => {
                // roll back everything; report the failing row
                drop(tx);
                return Ok(BulkWriteReport {
                    import_batch_id: None,
                    promoted: 0,
                    created: 0,
                    skipped_duplicates: vec![],
                    errors: vec![RowError {
                        index: i,
                        message: super::error_text::render(&e),
                    }],
                });
            }
        }
        created += 1;
    }
    for (first_index, id) in written {
        if let Err(error) = crate::services::investments::ensure_valid_holdings_history(
            &tx,
            "crypto_transactions",
            &id,
        ) {
            let ticker = &args.transactions[first_index].ticker;
            let index =
                crate::services::investments::oversold_day(&tx, "crypto_transactions", &id)?
                    .and_then(|day| {
                        crate::services::investments::blamed_sell_row(
                            args.transactions
                                .iter()
                                .enumerate()
                                .filter(|(_, row)| row.ticker.eq_ignore_ascii_case(ticker))
                                .map(|(i, row)| {
                                    (
                                        i,
                                        row.transaction_date.div_euclid(86_400) * 86_400,
                                        row.tx_type.eq_ignore_ascii_case("sell"),
                                    )
                                }),
                            day,
                        )
                    })
                    .unwrap_or(first_index);
            drop(tx);
            return Ok(BulkWriteReport {
                import_batch_id: None,
                promoted: 0,
                created: 0,
                skipped_duplicates: vec![],
                errors: vec![RowError {
                    index,
                    message: super::error_text::render(&error),
                }],
            });
        }
        crypto_service::recalculate_crypto_metrics(&tx, &id)?;
    }
    tx.commit()?;

    Ok(BulkWriteReport {
        import_batch_id: None,
        promoted: 0,
        created,
        skipped_duplicates: skipped,
        errors: vec![],
    })
}

/// Legacy input helper for touched tickers with the yahooSymbol a row supplied. Callers that
/// schedule pricing work must load the symbols from the committed holdings instead of this input.
pub fn distinct_tickers(args: &CryptoTransactionsCreateArgs) -> Vec<(String, Option<String>)> {
    let mut map: std::collections::BTreeMap<String, Option<String>> =
        std::collections::BTreeMap::new();
    for row in &args.transactions {
        let ticker_upper = row.ticker.to_uppercase();
        let entry = map.entry(ticker_upper).or_insert(None);
        if entry.is_none() {
            *entry = row_symbol(row);
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
        .expect("schema");
        conn
    }

    fn row(
        ticker: &str,
        yahoo_symbol: Option<&str>,
        tx_type: &str,
        quantity: &str,
        price: &str,
        currency: &str,
        date: i64,
    ) -> CryptoTransactionRow {
        CryptoTransactionRow {
            ticker: ticker.to_string(),
            name: None,
            yahoo_symbol: yahoo_symbol.map(String::from),
            tx_type: tx_type.to_string(),
            quantity: quantity.to_string(),
            price_per_unit: price.to_string(),
            currency: currency.to_string(),
            transaction_date: date,
            import_anyway: false,
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
            "CZK",
            &crate::services::fx::current(),
        )
        .ok()?
        .0
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
                    Some("BTC-USD"),
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
                    Some("BTC-USD"),
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
                Some("BTC-USD"),
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
    fn new_coin_without_symbol_is_created_unresolved() {
        let mut conn = setup_test_db();
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row(
                    "btc",
                    Some(" btc-usd "),
                    "buy",
                    "1",
                    "900000",
                    "CZK",
                    1_700_000_000,
                ),
                row("xyz", None, "buy", "10", "5", "CZK", 1_700_100_000), // new coin, no symbol
            ],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert!(report.errors.is_empty(), "{report:?}");
        assert_eq!(report.created, 2);
        let symbol = |ticker: &str| -> Option<String> {
            conn.query_row(
                "SELECT yahoo_symbol FROM crypto_investments WHERE ticker = ?1",
                [ticker],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(symbol("BTC").as_deref(), Some("BTC-USD"));
        assert_eq!(symbol("XYZ"), None);
    }

    #[test]
    fn an_invalid_symbol_is_a_phase1_row_error() {
        let mut conn = setup_test_db();
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![row(
                "btc",
                Some("bitcoin"),
                "buy",
                "1",
                "900000",
                "CZK",
                1_700_000_000,
            )],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].index, 0);
        assert_eq!(
            report.errors[0].message,
            "Validation error: The Yahoo Finance symbol must be the coin's USD pair, for example BTC-USD"
        );
        assert_eq!(holding_count(&conn), 0);
    }

    #[test]
    fn a_batch_reports_the_tickers_whose_supplied_symbol_filled_an_unresolved_holding() {
        let mut conn = setup_test_db();
        let seed = CryptoTransactionsCreateArgs {
            transactions: vec![
                row("sui", None, "buy", "1", "2", "USD", 1_700_000_000),
                row(
                    "btc",
                    Some("BTC-USD"),
                    "buy",
                    "1",
                    "2",
                    "USD",
                    1_700_000_000,
                ),
            ],
        };
        let (_, set) = crypto_transactions_create_reporting(&mut conn, &seed).unwrap();
        assert!(
            set.is_empty(),
            "new coins and a coin created with its symbol"
        );

        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row(
                    "sui",
                    Some("SUI20947-USD"),
                    "buy",
                    "1",
                    "2",
                    "USD",
                    1_700_086_400,
                ),
                row(
                    "sui",
                    Some("sui20947-usd"),
                    "buy",
                    "1",
                    "3",
                    "USD",
                    1_700_172_800,
                ),
                row(
                    "btc",
                    Some("BTC-USD"),
                    "buy",
                    "1",
                    "2",
                    "USD",
                    1_700_086_400,
                ),
            ],
        };
        let (report, set) = crypto_transactions_create_reporting(&mut conn, &args).unwrap();
        assert_eq!(report.created, 3, "{report:?}");
        assert_eq!(
            set,
            vec!["SUI".to_string()],
            "once, and not the coin that had its symbol"
        );

        // The same batch again fills nothing: duplicates are skipped and the symbol is set.
        let (_, set) = crypto_transactions_create_reporting(&mut conn, &args).unwrap();
        assert!(set.is_empty());
    }

    #[test]
    fn a_rejected_batch_reports_no_symbol_set() {
        let mut conn = setup_test_db();
        crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![row("sui", None, "buy", "1", "2", "USD", 1_700_000_000)],
            },
        )
        .unwrap();
        // The symbol is set by the first row, then the second row (a sell above the holdings)
        // rolls the batch back.
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row(
                    "sui",
                    Some("SUI20947-USD"),
                    "buy",
                    "1",
                    "2",
                    "USD",
                    1_700_086_400,
                ),
                row(
                    "sui",
                    Some("SUI20947-USD"),
                    "sell",
                    "99",
                    "2",
                    "USD",
                    1_700_172_800,
                ),
            ],
        };
        let (report, set) = crypto_transactions_create_reporting(&mut conn, &args).unwrap();
        assert_eq!(report.errors.len(), 1, "{report:?}");
        assert!(set.is_empty());
        assert_eq!(holding_symbol(&conn, "SUI"), None, "rolled back");
    }

    fn holding_symbol(conn: &Connection, ticker: &str) -> Option<String> {
        conn.query_row(
            "SELECT yahoo_symbol FROM crypto_investments WHERE ticker = ?1",
            [ticker],
            |r| r.get(0),
        )
        .unwrap()
    }

    #[test]
    fn duplicate_row_is_skipped_not_inserted() {
        let mut conn = setup_test_db();
        let seed = CryptoTransactionsCreateArgs {
            transactions: vec![row(
                "btc",
                Some("BTC-USD"),
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
                if i == 0 { Some("BTC-USD") } else { None },
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
            Some("BTC-USD"),
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
                    Some("ETH-USD"),
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
    fn distinct_tickers_prefers_some_symbol_when_deduping() {
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row("btc", None, "buy", "1", "900000", "CZK", 1),
                row("btc", Some("BTC-USD"), "sell", "1", "900000", "CZK", 2),
                row("eth", Some("ETH-USD"), "buy", "1", "60000", "CZK", 3),
            ],
        };
        let tickers = distinct_tickers(&args);
        assert_eq!(
            tickers,
            vec![
                ("BTC".to_string(), Some("BTC-USD".to_string())),
                ("ETH".to_string(), Some("ETH-USD".to_string())),
            ]
        );
    }

    #[test]
    fn integrity_crypto_oversell_and_backdated_sell_are_atomic() {
        let mut conn = setup_test_db();
        let report = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![
                    row(
                        "BTC",
                        Some("BTC-USD"),
                        "buy",
                        "1",
                        "100",
                        "USD",
                        1_700_000_000,
                    ),
                    row("BTC", None, "sell", "2", "100", "USD", 1_700_100_000),
                    row("BTC", None, "buy", "3", "100", "USD", 1_700_200_000),
                ],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(report.errors.len(), 1);
        // The error names the oversell, not the ticker's first (valid) row.
        assert_eq!(report.errors[0].index, 1);
        assert_eq!(tx_count(&conn), 0);
        crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![
                    row(
                        "BTC",
                        Some("BTC-USD"),
                        "buy",
                        "10",
                        "100",
                        "USD",
                        1_700_000_000,
                    ),
                    row("BTC", None, "sell", "8", "100", "USD", 1_700_200_000),
                ],
            },
        )
        .unwrap();
        let report = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![row("BTC", None, "sell", "5", "100", "USD", 1_700_100_000)],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert!(!report.errors.is_empty());
        assert_eq!(holding_quantity(&conn, "BTC"), Some("2".into()));
    }

    #[test]
    fn integrity_crypto_identity_conflicts_reject_the_whole_batch() {
        let mut conn = setup_test_db();
        let report = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![
                    row(
                        "BTC",
                        Some("ETH-USD"),
                        "buy",
                        "1",
                        "100",
                        "USD",
                        1_700_100_000,
                    ),
                    row(
                        "BTC",
                        Some("BTC-USD"),
                        "buy",
                        "1",
                        "100",
                        "USD",
                        1_700_000_000,
                    ),
                ],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert!(!report.errors.is_empty());
        assert_eq!(holding_count(&conn), 0);
        crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![row(
                    "BTC",
                    Some("BTC-USD"),
                    "buy",
                    "1",
                    "100",
                    "USD",
                    1_700_000_000,
                )],
            },
        )
        .unwrap();
        let report = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![row(
                    "BTC",
                    Some("ETH-USD"),
                    "buy",
                    "1",
                    "100",
                    "USD",
                    1_700_000_000,
                )],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert!(!report.errors.is_empty());
        assert_eq!(tx_count(&conn), 1);
    }

    #[test]
    fn integrity_crypto_numeric_dedup_and_multiple_currencies() {
        let mut conn = setup_test_db();
        crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![row(
                    "BTC",
                    Some("BTC-USD"),
                    "buy",
                    "10",
                    "100",
                    "USD",
                    1_700_000_000,
                )],
            },
        )
        .unwrap();
        let report = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![
                    row("BTC", None, "buy", "10.0", "100.00", "USD", 1_700_000_000),
                    row("BTC", None, "buy", "10", "100", "EUR", 1_700_000_000),
                ],
            },
        )
        .unwrap();
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert_eq!(report.created, 1);
        assert_eq!(report.skipped_duplicates.len(), 1);
        assert_eq!(holding_quantity(&conn, "BTC"), Some("20".into()));
    }

    #[test]
    fn integrity_crypto_identical_fill_requires_explicit_override() {
        let mut conn = setup_test_db();
        let first = row(
            "BTC",
            Some("BTC-USD"),
            "buy",
            "1",
            "100",
            "USD",
            1_700_000_000,
        );
        crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![first.clone()],
            },
        )
        .unwrap();
        let skipped = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![first.clone()],
            },
        )
        .unwrap();
        assert_eq!(skipped.created, 0);
        let mut genuine_fill = first;
        genuine_fill.import_anyway = true;
        let imported = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![genuine_fill],
            },
        )
        .unwrap();
        assert_eq!(imported.created, 1);
        assert_eq!(holding_quantity(&conn, "BTC"), Some("2".into()));
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
    #[test]
    fn merged_crypto_batch_history_is_validated_after_all_rows() {
        let mut conn = setup_test_db();
        const D: i64 = 1_699_920_000;
        let seed = CryptoTransactionsCreateArgs {
            transactions: vec![
                row("BTC", Some("BTC-USD"), "buy", "10", "100", "USD", D),
                row("BTC", None, "sell", "8", "100", "USD", D + 3 * 86400),
            ],
        };
        assert_eq!(
            crypto_transactions_create(&mut conn, &seed)
                .unwrap()
                .created,
            2
        );
        let args = CryptoTransactionsCreateArgs {
            transactions: vec![
                row("BTC", None, "sell", "5", "100", "USD", D + 86400),
                row("BTC", None, "buy", "5", "100", "USD", D + 2 * 86400),
            ],
        };
        let report = crypto_transactions_create(&mut conn, &args).unwrap();
        assert_eq!(report.created, 2, "{report:?}");
        assert_eq!(holding_quantity(&conn, "BTC").as_deref(), Some("2"));
        let bad = CryptoTransactionsCreateArgs {
            transactions: vec![row("BTC", None, "sell", "3", "100", "USD", D + 86400)],
        };
        let report = crypto_transactions_create(&mut conn, &bad).unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(tx_count(&conn), 4);
    }

    #[test]
    fn crypto_duplicate_is_possible_and_override_preserves_identical_fill() {
        let mut conn = setup_test_db();
        let a = row(
            "BTC",
            Some("BTC-USD"),
            "buy",
            "1",
            "100",
            "USD",
            1_700_000_000,
        );
        crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![a.clone()],
            },
        )
        .unwrap();
        let mut same = a.clone();
        same.quantity = "1.00".into();
        same.price_per_unit = "1e2".into();
        same.transaction_date += 1;
        let retry = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![same],
            },
        )
        .unwrap();
        assert_eq!(retry.created, 0);
        assert!(retry.skipped_duplicates[0].possible_duplicate);
        let mut forced = a;
        forced.import_anyway = true;
        assert_eq!(
            crypto_transactions_create(
                &mut conn,
                &CryptoTransactionsCreateArgs {
                    transactions: vec![forced]
                }
            )
            .unwrap()
            .created,
            1
        );
    }

    #[test]
    fn crypto_keeps_distinct_transaction_currencies_and_rejects_conflicting_symbols() {
        let mut conn = setup_test_db();
        let a = row(
            "BTC",
            Some("BTC-USD"),
            "buy",
            "1",
            "100",
            "USD",
            1_700_000_000,
        );
        let mut b = a.clone();
        b.currency = "EUR".into();
        let report = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![a.clone(), b.clone()],
            },
        )
        .unwrap();
        assert_eq!(report.created, 2, "{report:?}");
        let mut conflicting = a.clone();
        conflicting.yahoo_symbol = Some("ETH-USD".into());
        let report = crypto_transactions_create(
            &mut conn,
            &CryptoTransactionsCreateArgs {
                transactions: vec![conflicting.clone()],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert!(
            report.errors[0]
                .message
                .contains("yahooSymbol conflicts with stored holding"),
            "{report:?}"
        );
        let mut fresh = setup_test_db();
        let report = crypto_transactions_create(
            &mut fresh,
            &CryptoTransactionsCreateArgs {
                transactions: vec![a, conflicting],
            },
        )
        .unwrap();
        assert_eq!(report.created, 0);
        assert_eq!(holding_count(&fresh), 0);
        assert_eq!(tx_count(&conn), 2);
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
            CREATE TABLE crypto_transactions (
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
            CREATE TABLE crypto_value_history (
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
            INSERT INTO crypto_value_history (id, ticker, recorded_at, quantity, price, currency)
                VALUES ('v1', 'BTC', 100 * 86400, '10', '12', 'USD'),
                       ('v2', 'BTC', 101 * 86400, '10', '12', 'USD'),
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

        let result = crypto_value_history(&conn, "BTC", None, None).expect("history");

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

        let result = crypto_value_history(&conn, "BTC", None, None).expect("history");

        let first = &result["history"][0];
        assert_eq!(first["price"], "12");
        assert_eq!(first["currency"], "USD");
        assert_eq!(first["quantity"], "10");
        assert_eq!(first["isStale"], false);
    }

    #[test]
    fn value_history_stays_in_czk_for_a_czk_main_currency() {
        let conn = setup_db("CZK");

        let result = crypto_value_history(&conn, "BTC", None, None).expect("history");

        assert_eq!(result["mainCurrency"], "CZK");
        assert_eq!(result["history"][0]["value"], "2400.00");
        assert_eq!(result["history"][1]["value"], "2400.00");
        assert_eq!(result["missingCurrencies"], serde_json::json!([]));
    }

    #[test]
    fn value_history_applies_the_date_window() {
        let conn = setup_db("EUR");

        let result = crypto_value_history(&conn, "BTC", Some(101 * DAY), None).expect("history");

        let rows = result["history"].as_array().expect("history array");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["recordedAt"], 101 * DAY);
    }

    #[test]
    fn value_history_of_an_unknown_ticker_is_empty_but_names_the_currency() {
        let conn = setup_db("EUR");

        let result = crypto_value_history(&conn, "NOPE", None, None).expect("history");

        assert_eq!(result["mainCurrency"], "EUR");
        assert_eq!(result["history"], serde_json::json!([]));
    }

    #[test]
    fn value_history_names_the_currency_it_cannot_convert() {
        let conn = setup_db("EUR");
        conn.execute(
            "INSERT INTO crypto_value_history (id, ticker, recorded_at, quantity, price, currency)
             VALUES ('v4', 'BTC', 102 * 86400, '1', '5', 'XAU')",
            [],
        )
        .expect("row");

        let result = crypto_value_history(&conn, "BTC", None, None).expect("history");

        assert_eq!(result["history"][2]["value"], serde_json::Value::Null);
        assert_eq!(result["history"][2]["price"], "5");
        assert_eq!(result["missingCurrencies"], serde_json::json!(["XAU"]));
    }
}

#[cfg(test)]
mod list_tests {
    use super::*;

    #[test]
    fn crypto_list_values_positions_in_the_main_currency() {
        let conn = Connection::open_in_memory().expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE crypto_investments (id TEXT PRIMARY KEY, ticker TEXT, yahoo_symbol TEXT, name TEXT, quantity TEXT, currency TEXT NOT NULL);
            CREATE TABLE crypto_prices (symbol TEXT PRIMARY KEY, price TEXT, currency TEXT, fetched_at INTEGER);
            CREATE TABLE crypto_price_overrides (symbol TEXT PRIMARY KEY, price TEXT, currency TEXT, updated_at INTEGER);
            CREATE TABLE crypto_transactions (id TEXT PRIMARY KEY, investment_id TEXT, type TEXT, ticker TEXT, name TEXT, quantity TEXT, price_per_unit TEXT, currency TEXT, transaction_date INTEGER, created_at INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE exchange_rate_history (date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL, PRIMARY KEY (date, currency));
            CREATE TABLE user_profile (id INTEGER PRIMARY KEY AUTOINCREMENT, currency TEXT NOT NULL);
            INSERT INTO user_profile (currency) VALUES ('EUR');
            INSERT INTO exchange_rate_history VALUES (1699920000, 'CZK', 25.0), (1699920000, 'USD', 1.0);
            INSERT INTO crypto_investments VALUES
                ('c1', 'BTC', 'BTC-USD', 'Bitcoin', '1', 'USD'),
                ('c2', 'KWC', NULL, 'Kuwait Coin', '1', 'KWD');
            INSERT INTO crypto_transactions (id, investment_id, type, ticker, name, quantity, price_per_unit, currency, transaction_date) VALUES
                ('t1', 'c1', 'buy', 'BTC', 'Bitcoin', '1', '30000', 'USD', 1700000000),
                ('t2', 'c2', 'buy', 'KWC', 'Kuwait Coin', '1', '10', 'KWD', 1700000000);
            INSERT INTO crypto_prices VALUES ('BTC', '44000', 'USD', 100), ('KWC', '12', 'KWD', 100);
            -- an override older than the API price no longer applies (the resolver's rule)
            INSERT INTO crypto_price_overrides VALUES ('BTC', '1', 'EUR', 50);
            "#,
        )
        .expect("schema");
        crate::services::fx::set_current_for_test(&[("CZK", 22.0), ("USD", 22.0 / 25.0)]);

        let result = crypto_list(&conn).expect("list");

        assert_eq!(result["mainCurrency"], "EUR");
        let rows = result["crypto"].as_array().expect("rows");
        let btc = rows.iter().find(|r| r["ticker"] == "BTC").expect("btc");
        assert_eq!(btc["costBasis"], "30000.00");
        assert_eq!(btc["value"], "50000.00");
        assert_eq!(btc["unrealizedGain"], "20000.00");
        assert!(btc.get("annualDividend").is_none());
        // The unconvertible coin stays listed with its native price, its amounts null.
        assert_eq!(btc["currentPrice"], "44000");
        assert_eq!(btc["priceCurrency"], "USD");
        let kwc = rows.iter().find(|r| r["ticker"] == "KWC").expect("kwc");
        assert_eq!(kwc["currentPrice"], "12");
        assert_eq!(kwc["priceCurrency"], "KWD");
        assert!(kwc["value"].is_null());
        assert!(kwc["costBasis"].is_null());
        assert_eq!(kwc["missingCurrencies"], serde_json::json!(["KWD"]));
        assert_eq!(result["summary"]["totalValue"], "50000.00");
        assert!(result["summary"].get("annualDividends").is_none());
        assert_eq!(result["missingCurrencies"], serde_json::json!(["KWD"]));
    }
}
