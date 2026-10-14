//! Investment commands

use crate::commands::portfolio;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    DividendOverride, InsertInvestmentTransaction, InsertStockInvestment, InvestmentTransaction,
    InvestmentWithDetails, StockBenchmark, StockInvestment, StockPerformance,
    StockPerformanceRequest, StockPosition, StockPositions, StockPriceOverride, StockTag,
};
use crate::services::fx;
use crate::services::history_recalc::{HistoryRecalc, Job};
use crate::services::investments as investment_service;
use crate::services::position_reports;
use crate::services::price_api;
use crate::services::pricing::AssetKind;
use crate::services::stock_performance;
use crate::services::ticker_history;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// Days of closes fetched before a benchmark's first day: its base is the last close on or
/// before it, which on a Monday is Friday's (a long weekend plus a holiday is a handful of days).
const BENCHMARK_LEAD_SECONDS: i64 = 10 * 86_400;
/// The last day's own close is stamped within that day; Yahoo's end is exclusive.
const BENCHMARK_TAIL_SECONDS: i64 = 86_400;

/// Every stock position in `report_currency` (positions service, one quote snapshot).
#[tauri::command]
pub async fn get_all_investments(
    db: State<'_, Database>,
    report_currency: String,
) -> Result<StockPositions> {
    db.with_conn(|conn| position_reports::stock_positions(conn, &report_currency))
}

/// One stock position in `report_currency`.
#[tauri::command]
pub async fn get_investment(
    db: State<'_, Database>,
    id: String,
    report_currency: String,
) -> Result<StockPosition> {
    db.with_conn(|conn| position_reports::stock_position(conn, &id, &report_currency))
}

/// Get investment with all details in a single call (compound command to reduce IPC overhead)
#[tauri::command]
pub async fn get_investment_with_details(
    db: State<'_, Database>,
    id: String,
    report_currency: String,
) -> Result<InvestmentWithDetails> {
    db.with_conn(|conn| {
        let position = position_reports::stock_position(conn, &id, &report_currency)?;

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
             JOIN stock_investment_tags it ON t.id = it.tag_id
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
            currency: position.currency,
            fx_quality: position.fx_quality,
            investment: position.investment,
            transactions,
            tags,
        })
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
        portfolio::schedule_history_rebuild(
            &app,
            &db,
            &recalc,
            vec![Job::StockTicker(ticker_for_recalc, tx_date)],
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
    report_currency: String,
) -> Result<StockPosition> {
    // Validate name
    if company_name.trim().is_empty() {
        return Err(AppError::Validation(
            "validation.companyNameRequired".into(),
        ));
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

        // Return the updated position
        position_reports::stock_position(conn, &id, &report_currency)
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
        portfolio::schedule_history_rebuild(
            &app,
            &db,
            &recalc,
            vec![Job::StockTicker(ticker, tx_date)],
        );
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
    portfolio::schedule_history_rebuild(
        &app,
        &db,
        &recalc,
        vec![Job::StockTicker(
            result.ticker.clone(),
            result.transaction_date,
        )],
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
        portfolio::schedule_history_rebuild(app, db, recalc, Job::stocks(jobs));

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

    let (result, old_date) =
        db.with_conn(|conn| investment_service::update_transaction(conn, &tx_id, &data))?;

    // Update portfolio snapshot
    portfolio::update_todays_snapshot(&db).await.ok();

    // Queue the ticker's historical rebuild (runs in the background).
    // Rebuild history from the earlier of the two dates: moving a buy later must
    // also clear the days it no longer covers.
    portfolio::schedule_history_rebuild(
        &app,
        &db,
        &recalc,
        vec![Job::StockTicker(
            result.ticker.clone(),
            old_date.min(result.transaction_date),
        )],
    );

    app.emit("recalculation-complete", ()).ok();

    Ok(result)
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

/// Value history of a stock ticker in `report_currency` (native quantity, price and currency
/// per day plus the value converted at that day's rate).
#[tauri::command]
pub async fn get_stock_value_history(
    db: State<'_, Database>,
    ticker: String,
    start_date: Option<i64>,
    end_date: Option<i64>,
    report_currency: String,
) -> Result<crate::models::TickerValueHistoryReport> {
    db.with_conn(|conn| {
        ticker_history::value_history(
            conn,
            AssetKind::Stock,
            &ticker,
            (start_date, end_date),
            &report_currency,
            &fx::current(),
        )
    })
}

/// Performance of the held positions over a period in `request.report_currency`: time-weighted
/// and money-weighted return, value and deposits, the requested series and every position's
/// return and contribution (see `services::stock_performance`).
#[tauri::command]
pub async fn get_stock_performance(
    db: State<'_, Database>,
    request: StockPerformanceRequest,
) -> Result<StockPerformance> {
    request.validate()?;
    db.with_conn(|conn| stock_performance::stock_performance(conn, &request))
}

/// Daily returns of a market index (any Yahoo ticker, e.g. `CSPX.L`, `^GSPC`) over a period in
/// `report_currency`, for the comparison line of the performance chart. The closes are fetched
/// from Yahoo first (a few days before `from_ts` for the base), so the DB lock is held only for
/// the conversion; offline or an unknown ticker returns empty `dates`.
#[tauri::command]
pub async fn get_stock_benchmark(
    db: State<'_, Database>,
    ticker: String,
    from_ts: i64,
    to_ts: i64,
    report_currency: String,
) -> Result<StockBenchmark> {
    let ticker = stock_performance::validate_benchmark_ticker(&ticker)?;
    let mut fetched = price_api::get_historical_stock_prices_yahoo(
        std::slice::from_ref(&ticker),
        from_ts.saturating_sub(BENCHMARK_LEAD_SECONDS),
        to_ts.saturating_add(BENCHMARK_TAIL_SECONDS),
    )
    .await?;
    let prices = fetched.remove(&ticker).unwrap_or_default();
    db.with_conn(|conn| {
        stock_performance::benchmark_series(
            conn,
            &ticker,
            &prices,
            from_ts,
            to_ts,
            &report_currency,
        )
    })
}
