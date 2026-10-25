//! Crypto commands

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    CryptoInvestment, CryptoPositions, CryptoTransaction, InsertCryptoInvestment,
    InsertCryptoTransaction,
};
use crate::services::crypto_investments as crypto_service;
use crate::services::fx;
use crate::services::history_recalc::{crypto_rebuild_jobs, HistoryRecalc, Job};
use crate::services::position_reports;
use crate::services::pricing::AssetKind;
use crate::services::ticker_history;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// Every crypto position in `report_currency` (positions service, one quote snapshot).
#[tauri::command]
pub async fn get_all_crypto(
    db: State<'_, Database>,
    report_currency: String,
) -> Result<CryptoPositions> {
    db.with_conn(|conn| position_reports::crypto_positions(conn, &report_currency))
}

/// Create crypto investment
#[tauri::command]
pub async fn create_crypto(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    mut data: InsertCryptoInvestment,
    initial_transaction: Option<InsertCryptoTransaction>,
) -> Result<CryptoInvestment> {
    // 1. Validate inputs at the trust boundary (the symbol is trimmed and uppercased first)
    data.yahoo_symbol = data
        .yahoo_symbol
        .map(|symbol| symbol.trim().to_ascii_uppercase())
        .filter(|symbol| !symbol.is_empty());
    data.validate()?;
    if let Some(ref tx) = initial_transaction {
        tx.validate()?;
    }

    let ticker = data.ticker.to_uppercase();
    let initial_tx_date = initial_transaction.as_ref().map(|t| t.transaction_date);

    // 2. Delegate to service layer
    let inv = db.with_conn(|conn| {
        crypto_service::create_crypto_with_transaction(
            conn,
            &ticker,
            data.yahoo_symbol.as_deref(),
            &data.name,
            initial_transaction.as_ref(),
        )
    })?;

    // 3. Handle side effects (portfolio updates)
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // 4. Queue the ticker's historical rebuild (runs in the background) if there was an
    //    initial transaction
    if let Some(date) = initial_tx_date {
        crate::commands::portfolio::schedule_history_rebuild(
            &app,
            &db,
            &recalc,
            vec![Job::CryptoTicker(ticker, date)],
        );

        app.emit("recalculation-complete", ()).ok();
    }

    Ok(inv)
}

/// Store the Yahoo Finance symbol picked for a holding (the symbol picker, spec D9), then queue
/// the rebuild of its history from its first transaction so it follows the right coin.
#[tauri::command]
pub async fn set_crypto_symbol(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    id: String,
    symbol: String,
) -> Result<()> {
    // The service trims, uppercases and validates the symbol (`validation.yahooSymbolInvalid`).
    let ticker = db.with_conn(|conn| {
        crypto_service::set_symbol(conn, &id, &symbol)?;
        Ok(crypto_service::get_crypto_by_id(conn, &id)?.ticker)
    })?;

    let jobs = db.with_conn(|conn| crypto_rebuild_jobs(conn, std::slice::from_ref(&ticker)))?;
    if !jobs.is_empty() {
        crate::commands::portfolio::schedule_history_rebuild(
            &app,
            &db,
            &recalc,
            jobs.into_iter()
                .map(|(ticker, first)| Job::CryptoTicker(ticker, first))
                .collect(),
        );
    }

    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();
    app.emit("recalculation-complete", ()).ok();

    Ok(())
}

/// Delete crypto investment
#[tauri::command]
pub async fn delete_crypto(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    id: String,
) -> Result<()> {
    // Get earliest transaction date and ticker before deleting (for historical recalc)
    let crypto_info: Option<(i64, String)> = db.with_conn(|conn| {
        let info: Option<String> = conn
            .query_row(
                "SELECT ticker FROM crypto_investments WHERE id = ?1",
                [&id],
                |row| row.get(0),
            )
            .ok();

        if let Some(ticker) = info {
            // Get earliest transaction date
            let earliest: Option<i64> = conn
                .query_row(
                    "SELECT MIN(transaction_date) FROM crypto_transactions WHERE investment_id = ?1",
                    [&id],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            if let Some(date) = earliest {
                return Ok(Some((date, ticker)));
            }
        }
        Ok(None)
    })?;

    db.with_conn(|conn| {
        // Delete transactions first to avoid FK constraint violation
        conn.execute(
            "DELETE FROM crypto_transactions WHERE investment_id = ?1",
            [&id],
        )?;

        let changes = conn.execute("DELETE FROM crypto_investments WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Crypto investment not found".into()));
        }
        Ok(())
    })?;

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // Queue the ticker's rebuild if there were transactions: with the holding gone the worker
    // only removes its history rows and re-derives the portfolio days without it
    if let Some((tx_date, ticker)) = crypto_info {
        crate::commands::portfolio::schedule_history_rebuild(
            &app,
            &db,
            &recalc,
            vec![Job::CryptoTicker(ticker, tx_date)],
        );

        app.emit("recalculation-complete", ()).ok();
    }

    Ok(())
}

/// Get transactions for crypto
#[tauri::command]
pub async fn get_crypto_transactions(
    db: State<'_, Database>,
    investment_id: String,
) -> Result<Vec<CryptoTransaction>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, investment_id, type, ticker, name, quantity, price_per_unit,
                    currency, transaction_date, created_at
             FROM crypto_transactions WHERE investment_id = ?1
             ORDER BY transaction_date DESC, created_at DESC, rowid DESC",
        )?;

        let txs = stmt
            .query_map([&investment_id], |row| {
                Ok(CryptoTransaction {
                    id: row.get(0)?,
                    investment_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    ticker: row.get(3)?,
                    name: row.get(4)?,
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

/// Get all crypto transactions across all investments
#[tauri::command]
pub async fn get_all_crypto_transactions(
    db: State<'_, Database>,
) -> Result<Vec<CryptoTransaction>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, investment_id, type, ticker, name, quantity, price_per_unit,
                    currency, transaction_date, created_at
             FROM crypto_transactions
             ORDER BY transaction_date DESC, created_at DESC, rowid DESC",
        )?;

        let txs = stmt
            .query_map([], |row| {
                Ok(CryptoTransaction {
                    id: row.get(0)?,
                    investment_id: row.get(1)?,
                    tx_type: row.get(2)?,
                    ticker: row.get(3)?,
                    name: row.get(4)?,
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

/// Create crypto transaction
#[tauri::command]
pub async fn create_crypto_transaction(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    investment_id: String,
    data: InsertCryptoTransaction,
) -> Result<CryptoTransaction> {
    // 1. Validate inputs at the trust boundary
    data.validate()?;

    // 2. Delegate to service layer
    let result = db
        .with_conn(|conn| crypto_service::add_transaction_to_crypto(conn, &investment_id, &data))?;

    // 3. Handle side effects (portfolio updates)
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // 4. Queue the ticker's historical rebuild (runs in the background)
    crate::commands::portfolio::schedule_history_rebuild(
        &app,
        &db,
        &recalc,
        vec![Job::CryptoTicker(
            result.ticker.clone(),
            result.transaction_date,
        )],
    );

    app.emit("recalculation-complete", ()).ok();

    Ok(result)
}

/// Delete crypto transaction
#[tauri::command]
pub async fn delete_crypto_transaction(
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    app: AppHandle,
    tx_id: String,
) -> Result<()> {
    let (date, ticker) = db.with_conn(|conn| crypto_service::delete_transaction(conn, &tx_id))?;

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // Queue the ticker's rebuild (the worker reads the Yahoo symbol from the holding)
    crate::commands::portfolio::schedule_history_rebuild(
        &app,
        &db,
        &recalc,
        vec![Job::CryptoTicker(ticker, date)],
    );
    app.emit("recalculation-complete", ()).ok();

    Ok(())
}

/// Update crypto price manually (creates or updates override)
#[tauri::command]
pub async fn update_crypto_price(
    db: State<'_, Database>,
    symbol: String,
    price: String,
    currency: String,
) -> Result<()> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let symbol_upper = symbol.to_uppercase();

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO crypto_price_overrides (id, symbol, price, currency, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(symbol) DO UPDATE SET price = ?3, currency = ?4, updated_at = ?5",
            rusqlite::params![id, symbol_upper, price, currency, now],
        )?;
        Ok(())
    })?;

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db).await?;

    Ok(())
}

/// Delete manual crypto price override (revert to API price)
#[tauri::command]
pub async fn delete_crypto_manual_price(db: State<'_, Database>, symbol: String) -> Result<()> {
    db.with_conn(|conn| {
        conn.execute(
            "DELETE FROM crypto_price_overrides WHERE symbol = ?1",
            [symbol.to_uppercase()],
        )?;
        Ok(())
    })?;

    Ok(())
}

/// Value history of a crypto ticker in `report_currency` (native quantity, price and currency
/// per day plus the value converted at that day's rate).
#[tauri::command]
pub async fn get_crypto_value_history(
    db: State<'_, Database>,
    ticker: String,
    start_date: Option<i64>,
    end_date: Option<i64>,
    report_currency: String,
) -> Result<crate::models::TickerValueHistoryReport> {
    db.with_conn(|conn| {
        ticker_history::value_history(
            conn,
            AssetKind::Crypto,
            &ticker,
            (start_date, end_date),
            &report_currency,
            &fx::current(),
        )
    })
}
