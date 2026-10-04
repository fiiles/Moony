//! Investment commands

use crate::commands::portfolio;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    DividendOverride, EnrichedStockInvestment, InsertInvestmentTransaction, InsertStockInvestment,
    InvestmentTransaction, StockInvestment, StockPriceOverride, StockTag, TwrSeries,
};
use crate::services::currency::convert_to_czk;
use crate::services::history_recalc::HistoryRecalc;
use crate::services::investments as investment_service;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// Investment with all related data (compound response to reduce IPC calls)
#[derive(Debug, Clone, Serialize)]
pub struct InvestmentWithDetails {
    pub investment: EnrichedStockInvestment,
    pub transactions: Vec<InvestmentTransaction>,
    pub tags: Vec<StockTag>,
}

/// Get all investments with enriched price data
/// Derived native average price per investment id (weighted average in the
/// position's primary currency, computed from transactions — never stored).
fn derived_average_prices(
    conn: &rusqlite::Connection,
) -> Result<std::collections::HashMap<String, String>> {
    Ok(crate::services::cost_basis::cost_basis_for_all(
        conn,
        crate::services::cost_basis::TxTable::Stocks,
    )?
    .into_iter()
    .map(|(id, p)| (id, p.average_price_native().to_string()))
    .collect())
}

#[tauri::command]
pub async fn get_all_investments(db: State<'_, Database>) -> Result<Vec<EnrichedStockInvestment>> {
    db.with_conn(|conn| {
        let avg_map = derived_average_prices(conn)?;
        let mut stmt = conn.prepare(
            "SELECT id, ticker, company_name, quantity, currency FROM stock_investments",
        )?;

        let investments: Vec<StockInvestment> = stmt
            .query_map([], |row| {
                Ok(StockInvestment {
                    id: row.get(0)?,
                    ticker: row.get(1)?,
                    company_name: row.get(2)?,
                    quantity: row.get(3)?,
                    average_price: String::new(),
                    currency: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .map(|mut inv| {
                inv.average_price = avg_map.get(&inv.id).cloned().unwrap_or_else(|| "0".into());
                inv
            })
            .collect();

        // Enrich with price data
        let mut enriched = Vec::new();
        for inv in investments {
            enriched.push(enrich_investment(conn, inv)?);
        }

        Ok(enriched)
    })
}

/// Get a single investment by ID with enriched price data
#[tauri::command]
pub async fn get_investment(
    db: State<'_, Database>,
    id: String,
) -> Result<EnrichedStockInvestment> {
    db.with_conn(|conn| {
        let inv = investment_service::get_investment_by_id(conn, &id)?;

        enrich_investment(conn, inv)
    })
}

/// Get investment with all details in a single call (compound command to reduce IPC overhead)
#[tauri::command]
pub async fn get_investment_with_details(
    db: State<'_, Database>,
    id: String,
) -> Result<InvestmentWithDetails> {
    db.with_conn(|conn| {
        // Get investment
        let inv = investment_service::get_investment_by_id(conn, &id)?;

        let enriched = enrich_investment(conn, inv)?;

        // Get transactions
        let mut stmt = conn.prepare(
            "SELECT id, investment_id, type, ticker, company_name, quantity, price_per_unit,
                    currency, transaction_date, created_at
             FROM investment_transactions WHERE investment_id = ?1
             ORDER BY transaction_date DESC, created_at DESC, rowid DESC",
        )?;

        let transactions: Vec<InvestmentTransaction> = stmt
            .query_map([&id], |row| {
                Ok(InvestmentTransaction {
                    id: row.get(0)?,
                    investment_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    ticker: row.get(3)?,
                    company_name: row.get(4)?,
                    quantity: row.get(5)?,
                    price_per_unit: row.get(6)?,
                    currency: row.get(7)?,
                    transaction_date: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Get tags
        let mut tag_stmt = conn.prepare(
            "SELECT t.id, t.name, t.color, t.group_id, t.created_at
             FROM stock_tags t
             JOIN investment_tags it ON t.id = it.tag_id
             WHERE it.investment_id = ?1",
        )?;

        let tags: Vec<StockTag> = tag_stmt
            .query_map([&id], |row| {
                Ok(StockTag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    group_id: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(InvestmentWithDetails {
            investment: enriched,
            transactions,
            tags,
        })
    })
}

/// Helper function to enrich a stock investment with price and dividend data
fn enrich_investment(
    conn: &rusqlite::Connection,
    inv: StockInvestment,
) -> Result<EnrichedStockInvestment> {
    use crate::services::pricing;

    // Determine active price using unified resolver
    let resolved = pricing::resolve_stock_price(conn, &inv.ticker);
    let (original_price, currency, current_price, fetched_at, is_manual_price) = match resolved {
        Some(rp) => (
            rp.original_price,
            rp.currency,
            rp.price_czk,
            rp.fetched_at,
            rp.is_manual,
        ),
        None => ("0".to_string(), "CZK".to_string(), 0.0, None, false),
    };

    // Get dividend data
    let user_dividend: Option<(String, String)> = conn
        .query_row(
            "SELECT yearly_dividend_sum, currency FROM dividend_overrides WHERE ticker = ?1",
            [&inv.ticker],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let global_dividend: Option<(String, String)> = conn
        .query_row(
            "SELECT yearly_dividend_sum, currency FROM dividend_data WHERE ticker = ?1",
            [&inv.ticker],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    // Get original dividend and currency
    let (original_dividend_yield, dividend_currency, dividend_yield, is_manual_dividend) =
        match (&user_dividend, &global_dividend) {
            (Some((sum, curr)), _) => {
                let orig = sum.parse().unwrap_or(0.0);
                (orig, curr.clone(), convert_to_czk(orig, curr), true)
            }
            (None, Some((sum, curr))) => {
                let orig = sum.parse().unwrap_or(0.0);
                (orig, curr.clone(), convert_to_czk(orig, curr), false)
            }
            _ => (0.0, "CZK".to_string(), 0.0, false),
        };

    Ok(EnrichedStockInvestment {
        id: inv.id,
        ticker: inv.ticker,
        company_name: inv.company_name,
        quantity: inv.quantity,
        average_price: inv.average_price,
        average_price_currency: inv.currency,
        current_price,
        original_price,
        currency,
        fetched_at,
        is_manual_price,
        dividend_yield,
        original_dividend_yield,
        dividend_currency,
        is_manual_dividend,
    })
}

/// Create investment
#[tauri::command]
pub async fn create_investment(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    data: InsertStockInvestment,
    initial_transaction: Option<InsertInvestmentTransaction>,
) -> Result<StockInvestment> {
    // 1. Validate inputs at the trust boundary
    data.validate()?;
    if let Some(ref tx) = initial_transaction {
        tx.validate()?;
    }

    let ticker = data.ticker.to_uppercase();
    let ticker_for_recalc = ticker.clone();
    let initial_tx_date = initial_transaction.as_ref().map(|t| t.transaction_date);

    // 2. Delegate to service layer
    let inv = db.with_conn(|conn| {
        investment_service::create_investment_with_transaction(
            conn,
            &ticker,
            &data.company_name,
            initial_transaction.as_ref(),
        )
    })?;

    // 3. Handle side effects (portfolio updates)
    portfolio::update_todays_snapshot(&db).await.ok();

    // 4. Queue the ticker's historical rebuild (runs in the background) if there
    //    was an initial transaction
    if let Some(tx_date) = initial_tx_date {
        portfolio::schedule_stock_history_rebuild(
            &app,
            &db,
            &recalc,
            vec![(ticker_for_recalc, tx_date)],
        );

        app.emit("recalculation-complete", ()).ok();
    }

    Ok(inv)
}

/// Update investment name
#[tauri::command]
pub async fn update_investment_name(
    db: State<'_, Database>,
    id: String,
    company_name: String,
) -> Result<EnrichedStockInvestment> {
    // Validate name
    if company_name.trim().is_empty() {
        return Err(AppError::Validation("Company name cannot be empty".into()));
    }

    db.with_conn(|conn| {
        let changes = conn.execute(
            "UPDATE stock_investments SET company_name = ?1 WHERE id = ?2",
            rusqlite::params![company_name.trim(), id],
        )?;

        if changes == 0 {
            return Err(AppError::NotFound("Investment not found".into()));
        }

        // Also update company_name in all related transactions
        conn.execute(
            "UPDATE investment_transactions SET company_name = ?1 WHERE investment_id = ?2",
            rusqlite::params![company_name.trim(), id],
        )?;

        // Return updated investment
        let inv = investment_service::get_investment_by_id(conn, &id)?;

        enrich_investment(conn, inv)
    })
}

/// Delete investment
#[tauri::command]
pub async fn delete_investment(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    id: String,
) -> Result<()> {
    // Get earliest transaction date and ticker before deleting (for historical recalc)
    let tx_info: Option<(i64, String)> = db.with_conn(|conn| {
        Ok(conn
            .query_row(
                "SELECT MIN(transaction_date), ticker FROM investment_transactions WHERE investment_id = ?1",
                [&id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok())
    })?;

    // Also get ticker from the investment itself (in case there are no transactions)
    let ticker: Option<String> = db.with_conn(|conn| {
        Ok(conn
            .query_row(
                "SELECT ticker FROM stock_investments WHERE id = ?1",
                [&id],
                |row| row.get(0),
            )
            .ok())
    })?;

    db.with_conn(|conn| {
        // Delete transactions first (due to foreign key)
        conn.execute(
            "DELETE FROM investment_transactions WHERE investment_id = ?1",
            [&id],
        )?;

        let changes = conn.execute("DELETE FROM stock_investments WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Investment not found".into()));
        }
        Ok(())
    })?;

    // Delete this ticker's history from stock_value_history
    if let Some(ref t) = ticker {
        db.with_conn(|conn| {
            conn.execute("DELETE FROM stock_value_history WHERE ticker = ?1", [t])?;
            Ok(())
        })?;
    }

    // Update portfolio snapshot
    portfolio::update_todays_snapshot(&db).await.ok();

    // Re-derive the aggregate portfolio history without the deleted ticker (in
    // the background; the ticker has no transactions left, so nothing is fetched)
    if let (Some((tx_date, _)), Some(ticker)) = (tx_info, ticker) {
        portfolio::schedule_stock_history_rebuild(&app, &db, &recalc, vec![(ticker, tx_date)]);
    }

    app.emit("recalculation-complete", ()).ok();

    Ok(())
}

/// Get transactions for investment
#[tauri::command]
pub async fn get_investment_transactions(
    db: State<'_, Database>,
    investment_id: String,
) -> Result<Vec<InvestmentTransaction>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, investment_id, type, ticker, company_name, quantity, price_per_unit,
                    currency, transaction_date, created_at
             FROM investment_transactions WHERE investment_id = ?1
             ORDER BY transaction_date DESC, created_at DESC, rowid DESC",
        )?;

        let txs = stmt
            .query_map([&investment_id], |row| {
                Ok(InvestmentTransaction {
                    id: row.get(0)?,
                    investment_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    ticker: row.get(3)?,
                    company_name: row.get(4)?,
                    quantity: row.get(5)?,
                    price_per_unit: row.get(6)?,
                    currency: row.get(7)?,
                    transaction_date: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(txs)
    })
}

/// Get all stock transactions across all investments
#[tauri::command]
pub async fn get_all_stock_transactions(
    db: State<'_, Database>,
) -> Result<Vec<InvestmentTransaction>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, investment_id, type, ticker, company_name, quantity, price_per_unit,
                    currency, transaction_date, created_at
             FROM investment_transactions
             ORDER BY transaction_date DESC, created_at DESC, rowid DESC",
        )?;

        let txs = stmt
            .query_map([], |row| {
                Ok(InvestmentTransaction {
                    id: row.get(0)?,
                    investment_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    ticker: row.get(3)?,
                    company_name: row.get(4)?,
                    quantity: row.get(5)?,
                    price_per_unit: row.get(6)?,
                    currency: row.get(7)?,
                    transaction_date: row.get(8)?,
                    created_at: row.get(9)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(txs)
    })
}

/// Create transaction
#[tauri::command]
pub async fn create_investment_transaction(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    investment_id: String,
    data: InsertInvestmentTransaction,
) -> Result<InvestmentTransaction> {
    // 1. Validate inputs at the trust boundary
    data.validate()?;

    // 2. Delegate to service layer
    let result = db.with_conn(|conn| {
        investment_service::add_transaction_to_investment(conn, &investment_id, &data)
    })?;

    // 3. Handle side effects (portfolio updates)
    portfolio::update_todays_snapshot(&db).await.ok();

    // 4. Queue the ticker's historical rebuild (runs in the background)
    portfolio::schedule_stock_history_rebuild(
        &app,
        &db,
        &recalc,
        vec![(result.ticker.clone(), result.transaction_date)],
    );

    app.emit("recalculation-complete", ()).ok();

    Ok(result)
}

/// Delete transaction
#[tauri::command]
pub async fn delete_investment_transaction(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    tx_id: String,
) -> Result<()> {
    delete_transactions_and_rebuild(&db, &recalc, &app, vec![tx_id]).await
}

/// Delete several transactions together: one database transaction, then one
/// history rebuild per affected ticker from its earliest deleted date.
#[tauri::command]
pub async fn delete_investment_transactions(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    tx_ids: Vec<String>,
) -> Result<()> {
    delete_transactions_and_rebuild(&db, &recalc, &app, tx_ids).await
}

async fn delete_transactions_and_rebuild(
    db: &Database,
    recalc: &HistoryRecalc,
    app: &AppHandle,
    tx_ids: Vec<String>,
) -> Result<()> {
    // Earliest deleted date per ticker (for the historical rebuild)
    let jobs = db.with_conn(|conn| investment_service::delete_transactions(conn, &tx_ids))?;

    // Update portfolio snapshot
    portfolio::update_todays_snapshot(db).await.ok();

    // Queue the tickers' historical rebuilds (run in the background)
    if !jobs.is_empty() {
        portfolio::schedule_stock_history_rebuild(app, db, recalc, jobs);

        app.emit("recalculation-complete", ()).ok();
    }

    Ok(())
}

/// Update transaction
#[tauri::command]
pub async fn update_investment_transaction(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    tx_id: String,
    data: InsertInvestmentTransaction,
) -> Result<InvestmentTransaction> {
    // Validate inputs at the trust boundary
    data.validate()?;

    let result = db.with_conn(|conn| {
        // Get investment_id and the date before the edit (history must
        // be rebuilt from the earlier of the old and the new date).
        let (investment_id, old_date): (String, i64) = conn.query_row(
            "SELECT investment_id, transaction_date FROM investment_transactions WHERE id = ?1",
            [&tx_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;

        conn.execute(
            "UPDATE investment_transactions
             SET type = ?2, quantity = ?3, price_per_unit = ?4, currency = ?5, transaction_date = ?6
             WHERE id = ?1",
            rusqlite::params![
                tx_id,
                data.tx_type,
                data.quantity,
                data.price_per_unit,
                data.currency,
                data.transaction_date,
            ],
        )?;

        investment_service::recalculate_investment_metrics(conn, &investment_id)?;

        // Fetch updated transaction
        let tx = conn.query_row(
            "SELECT id, investment_id, type, ticker, company_name, quantity, price_per_unit,
                    currency, transaction_date, created_at
             FROM investment_transactions WHERE id = ?1",
            [&tx_id],
            |row| {
                Ok(InvestmentTransaction {
                    id: row.get(0)?,
                    investment_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    ticker: row.get(3)?,
                    company_name: row.get(4)?,
                    quantity: row.get(5)?,
                    price_per_unit: row.get(6)?,
                    currency: row.get(7)?,
                    transaction_date: row.get(8)?,
                    created_at: row.get(9)?,
                })
            },
        )?;

        Ok((tx, old_date))
    })?;
    let (result, old_date) = result;

    // Update portfolio snapshot
    portfolio::update_todays_snapshot(&db).await.ok();

    // Queue the ticker's historical rebuild (runs in the background).
    // Rebuild history from the earlier of the two dates: moving a buy later must
    // also clear the days it no longer covers.
    portfolio::schedule_stock_history_rebuild(
        &app,
        &db,
        &recalc,
        vec![(result.ticker.clone(), old_date.min(result.transaction_date))],
    );

    app.emit("recalculation-complete", ()).ok();

    Ok(result)
}

/// Import investment transactions from CSV data
#[tauri::command]
pub async fn import_investment_transactions(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    transactions: Vec<serde_json::Value>,
    default_currency: String,
) -> Result<serde_json::Value> {
    use crate::services::price_api;
    use std::collections::HashMap;
    use tauri::Emitter;

    // Progress event payload
    #[derive(Clone, serde::Serialize)]
    struct ImportProgress {
        phase: String,
        current: usize,
        total: usize,
    }

    let mut success_count = 0;
    let mut errors: Vec<String> = Vec::new();
    let mut imported: Vec<String> = Vec::new();
    // Track earliest date per ticker for per-ticker recalculation
    let mut ticker_earliest_dates: HashMap<String, i64> = HashMap::new();

    // First pass: collect tickers that need name lookup
    let mut name_cache: HashMap<String, String> = HashMap::new();
    let mut tickers_needing_lookup: Vec<String> = Vec::new();

    for tx in &transactions {
        if let Some(ticker) = tx
            .get("Ticker")
            .or_else(|| tx.get("ticker"))
            .and_then(|v| v.as_str())
        {
            let ticker_upper = ticker.to_uppercase();

            // Check if name is provided and not empty/same as ticker
            let name = tx
                .get("Name")
                .or_else(|| tx.get("name"))
                .or_else(|| tx.get("Company_name"))
                .or_else(|| tx.get("company_name"))
                .and_then(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty() && s.to_uppercase() != ticker_upper);

            if name.is_none() && !name_cache.contains_key(&ticker_upper) {
                // Check if investment already exists with a name
                let existing_name: Option<String> = db
                    .with_conn(|conn| {
                        conn.query_row(
                            "SELECT company_name FROM stock_investments WHERE ticker = ?1",
                            [&ticker_upper],
                            |row| row.get(0),
                        )
                        .ok()
                        .map(Ok)
                        .transpose()
                    })
                    .ok()
                    .flatten();

                if let Some(existing) = existing_name {
                    // Use existing company name from database
                    name_cache.insert(ticker_upper.clone(), existing);
                } else if !tickers_needing_lookup.contains(&ticker_upper) {
                    tickers_needing_lookup.push(ticker_upper.clone());
                }
            } else if let Some(n) = name {
                name_cache.insert(ticker_upper, n);
            }
        }
    }

    // Look up company names from Yahoo Finance for tickers without names
    if !tickers_needing_lookup.is_empty() {
        log::info!(
            "[IMPORT] Looking up company names for {} tickers from Yahoo Finance",
            tickers_needing_lookup.len()
        );

        for (index, ticker) in tickers_needing_lookup.iter().enumerate() {
            // Emit progress for name lookup phase
            app.emit(
                "stock-import-progress",
                ImportProgress {
                    phase: "lookup".to_string(),
                    current: index + 1,
                    total: tickers_needing_lookup.len(),
                },
            )
            .ok();

            // Add small delay between lookups to avoid rate limiting
            tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

            match price_api::search_stock_tickers(ticker).await {
                Ok(results) => {
                    // Find exact match or first result
                    if let Some(result) = results
                        .iter()
                        .find(|r| r.symbol.to_uppercase() == ticker.to_uppercase())
                        .or_else(|| results.first())
                    {
                        log::debug!("[IMPORT] Found name for {}: {}", ticker, result.shortname);
                        name_cache.insert(ticker.clone(), result.shortname.clone());
                    } else {
                        log::debug!("[IMPORT] No name found for {}, using ticker", ticker);
                        name_cache.insert(ticker.clone(), ticker.clone());
                    }
                }
                Err(e) => {
                    log::debug!("[IMPORT] Failed to lookup {}: {}, using ticker", ticker, e);
                    name_cache.insert(ticker.clone(), ticker.clone());
                }
            }
        }
    }

    // Second pass: process transactions with enriched names
    // Process each transaction individually so we can emit progress events
    let total_transactions = transactions.len();
    for (index, tx) in transactions.iter().enumerate() {
        // Emit progress for import phase
        app.emit(
            "stock-import-progress",
            ImportProgress {
                phase: "import".to_string(),
                current: index + 1,
                total: total_transactions,
            },
        )
        .ok();

        let result = db.with_conn(|conn| {
            // Parse JSON fields
            let ticker = tx
                .get("Ticker")
                .or_else(|| tx.get("ticker"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| crate::error::AppError::Validation("Missing ticker".into()))?
                .to_uppercase();

            let tx_type = tx
                .get("Type")
                .or_else(|| tx.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("buy")
                .to_lowercase();

            let raw_quantity = tx
                .get("Quantity")
                .or_else(|| tx.get("quantity"))
                .and_then(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| v.as_f64().map(|n| n.to_string()))
                })
                .ok_or_else(|| {
                    crate::error::AppError::Validation(format!("{}: Missing quantity", ticker))
                })?;
            let quantity = raw_quantity
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .replace(',', ".");

            let raw_price = tx
                .get("Price")
                .or_else(|| tx.get("price"))
                .and_then(|v| {
                    v.as_str()
                        .map(|s| s.to_string())
                        .or_else(|| v.as_f64().map(|n| n.to_string()))
                })
                .ok_or_else(|| {
                    crate::error::AppError::Validation(format!("{}: Missing price", ticker))
                })?;
            let price = raw_price
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .replace(',', ".");

            let raw_currency = tx
                .get("Currency")
                .or_else(|| tx.get("currency"))
                .and_then(|v| v.as_str())
                .unwrap_or(&default_currency);
            let currency = if raw_currency.chars().all(char::is_numeric) {
                default_currency.as_str()
            } else {
                raw_currency
                    .split_whitespace()
                    .next()
                    .unwrap_or(&default_currency)
            };

            let date_str = tx
                .get("Date")
                .or_else(|| tx.get("date"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let transaction_date = if !date_str.is_empty() {
                crate::services::date_parser::parse_date_to_timestamp(date_str, None).map_err(
                    |_| {
                        crate::error::AppError::Validation(format!(
                            "{}: Invalid date format '{}'",
                            ticker, date_str
                        ))
                    },
                )?
            } else {
                chrono::Utc::now().timestamp()
            };

            let company_name = name_cache
                .get(&ticker)
                .cloned()
                .or_else(|| {
                    tx.get("Name")
                        .or_else(|| tx.get("name"))
                        .or_else(|| tx.get("Company_name"))
                        .or_else(|| tx.get("company_name"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                })
                .unwrap_or_else(|| ticker.clone());

            // Delegate to service layer
            investment_service::import_single_transaction(
                conn,
                &ticker,
                &company_name,
                &tx_type,
                &quantity,
                &price,
                currency,
                transaction_date,
            )
        });

        match result {
            Ok((description, tx_date, ticker)) => {
                success_count += 1;
                imported.push(format!("Row {}: {}", index + 1, description));
                // Track the earliest transaction date per ticker
                ticker_earliest_dates
                    .entry(ticker)
                    .and_modify(|e| *e = (*e).min(tx_date))
                    .or_insert(tx_date);
            }
            Err(e) => errors.push(format!("Row {}: {}", index + 1, e)),
        }
    }

    // Update portfolio snapshot after all imports
    portfolio::update_todays_snapshot(&db).await.ok();

    // Queue one historical rebuild per imported ticker, from its earliest
    // imported date (runs in the background and coalesces with other pending work)
    log::debug!(
        "[IMPORT] Queueing history rebuild for {} imported ticker(s)",
        ticker_earliest_dates.len()
    );
    portfolio::schedule_stock_history_rebuild(
        &app,
        &db,
        &recalc,
        ticker_earliest_dates.into_iter().collect(),
    );

    app.emit("recalculation-complete", ()).ok();

    // Return result structure
    Ok(serde_json::json!({
        "success": success_count,
        "errors": errors,
        "imported": imported
    }))
}

/// Set manual price override
#[tauri::command]
pub async fn set_manual_price(
    db: State<'_, Database>,
    ticker: String,
    price: String,
    currency: String,
) -> Result<StockPriceOverride> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let ticker = ticker.to_uppercase();

    let id_clone = id.clone();
    let ticker_clone = ticker.clone();
    let price_clone = price.clone();
    let currency_clone = currency.clone();

    db.with_conn(move |conn| {
        conn.execute(
            "INSERT INTO stock_price_overrides (id, ticker, price, currency, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(ticker) DO UPDATE SET price = ?3, currency = ?4, updated_at = ?5",
            rusqlite::params![id, ticker, price, currency, now],
        )?;

        Ok(StockPriceOverride {
            id,
            ticker,
            price,
            currency,
            updated_at: now,
        })
    })?;

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db).await?;

    Ok(StockPriceOverride {
        id: id_clone,
        ticker: ticker_clone,
        price: price_clone,
        currency: currency_clone,
        updated_at: now,
    })
}

/// Delete manual price override
#[tauri::command]
pub async fn delete_manual_price(db: State<'_, Database>, ticker: String) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM stock_price_overrides WHERE ticker = ?1",
            [&ticker.to_uppercase()],
        )?;
        Ok(())
    })
}

/// Set manual dividend override
#[tauri::command]
pub async fn set_manual_dividend(
    db: State<'_, Database>,
    ticker: String,
    amount: String,
    currency: String,
) -> Result<DividendOverride> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let ticker = ticker.to_uppercase();

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO dividend_overrides (id, ticker, yearly_dividend_sum, currency, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(ticker) DO UPDATE SET yearly_dividend_sum = ?3, currency = ?4, updated_at = ?5",
            rusqlite::params![id, ticker, amount, currency, now],
        )?;

        Ok(DividendOverride {
            id,
            ticker,
            yearly_dividend_sum: amount,
            currency,
            updated_at: now,
        })
    })
}

/// Delete manual dividend override
#[tauri::command]
pub async fn delete_manual_dividend(db: State<'_, Database>, ticker: String) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM dividend_overrides WHERE ticker = ?1",
            [&ticker.to_uppercase()],
        )?;
        Ok(())
    })
}

/// Get value history for a specific stock ticker
#[tauri::command]
pub async fn get_stock_value_history(
    db: State<'_, Database>,
    ticker: String,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<crate::models::TickerValueHistory>> {
    db.with_conn(|conn| investment_service::get_value_history(conn, &ticker, start_date, end_date))
}

/// Get time-weighted return series for a set of stock tags and/or the whole portfolio.
///
/// `tag_ids`: IDs of tags to compute separate series for.
/// `include_portfolio`: if true, also include a whole-portfolio series.
/// `include_untagged`: if true, include a series for stocks with no tags assigned.
/// `investment_ids`: when given, every tag (and untagged) series covers only these positions;
/// the whole-portfolio series is never narrowed.
/// `from_ts` / `to_ts`: Unix timestamps (seconds, midnight UTC) for the date range.
///
/// When `tag_ids` is empty and `include_untagged` is false, only the portfolio series is returned.
#[tauri::command]
pub async fn get_stock_twr(
    db: State<'_, Database>,
    tag_ids: Vec<String>,
    include_portfolio: bool,
    include_untagged: bool,
    investment_ids: Option<Vec<String>>,
    from_ts: i64,
    to_ts: i64,
) -> Result<Vec<TwrSeries>> {
    db.with_conn(|conn| {
        investment_service::twr_series(
            conn,
            &tag_ids,
            include_portfolio,
            include_untagged,
            investment_ids.as_deref(),
            from_ts,
            to_ts,
        )
    })
}
