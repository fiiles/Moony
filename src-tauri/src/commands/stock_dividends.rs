//! Tauri commands for stock dividends
//!
//! Thin command handlers that delegate to the stock dividends service. Yahoo Finance's dividends
//! are fetched with the yearly dividend refresh (`refresh_dividends`); these commands list them
//! and keep the manual ones. A dividend changes no quantity or value, so no history rebuild is
//! queued; `recalculation-complete` tells open pages to refetch.

use crate::db::Database;
use crate::error::Result;
use crate::models::{InsertStockDividend, StockDividend, StockDividendList};
use crate::services::{fx, stock_dividends};
use tauri::{AppHandle, Emitter, State};

/// A position's dividends with the sum of the counted ones in `report_currency`
#[tauri::command]
pub async fn get_stock_dividends(
    db: State<'_, Database>,
    investment_id: String,
    report_currency: String,
) -> Result<StockDividendList> {
    db.with_conn(|conn| {
        stock_dividends::list(
            conn,
            &investment_id,
            &report_currency,
            &fx::current(),
            chrono::Utc::now().timestamp(),
        )
    })
}

/// Enter a dividend by hand; refused for a ticker Yahoo Finance has dividends for
#[tauri::command]
pub async fn create_stock_dividend(
    db: State<'_, Database>,
    app: AppHandle,
    investment_id: String,
    data: InsertStockDividend,
) -> Result<StockDividend> {
    data.validate()?;
    let created = db.with_conn(|conn| stock_dividends::create(conn, &investment_id, &data))?;
    app.emit("recalculation-complete", ()).ok();
    Ok(created)
}

#[tauri::command]
pub async fn update_stock_dividend(
    db: State<'_, Database>,
    app: AppHandle,
    id: String,
    data: InsertStockDividend,
) -> Result<StockDividend> {
    data.validate()?;
    let updated = db.with_conn(|conn| stock_dividends::update(conn, &id, &data))?;
    app.emit("recalculation-complete", ()).ok();
    Ok(updated)
}

#[tauri::command]
pub async fn delete_stock_dividend(
    db: State<'_, Database>,
    app: AppHandle,
    id: String,
) -> Result<()> {
    db.with_conn(|conn| stock_dividends::delete(conn, &id))?;
    app.emit("recalculation-complete", ()).ok();
    Ok(())
}
