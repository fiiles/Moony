//! Tauri command of the stock transactions report: validates the request and delegates to the
//! service.

use crate::db::Database;
use crate::error::Result;
use crate::models::{
    StockTransactionsReport, StockTransactionsReportRequest, StockTransactionsReportSettings,
};
use crate::services::stock_transactions_report;
use tauri::State;

/// Realized stock sales of a period by cost method and holding-period test, with the period's
/// trades, dividends and the open lots.
#[tauri::command]
pub async fn get_stock_transactions_report(
    db: State<'_, Database>,
    request: StockTransactionsReportRequest,
) -> Result<StockTransactionsReport> {
    request.validate()?;
    let today = chrono::Utc::now().timestamp();
    db.with_conn(|conn| stock_transactions_report::report(conn, &request, today))
}

/// The remembered cost method and holding test of the report page.
#[tauri::command]
pub async fn get_stock_transactions_report_settings(
    db: State<'_, Database>,
) -> Result<StockTransactionsReportSettings> {
    db.with_conn(stock_transactions_report::settings::load)
}

/// Remembers the cost method and holding test; refuses a test length outside 1..=600 months.
#[tauri::command]
pub async fn save_stock_transactions_report_settings(
    db: State<'_, Database>,
    settings: StockTransactionsReportSettings,
) -> Result<()> {
    db.with_conn(|conn| stock_transactions_report::settings::save(conn, &settings))
}
