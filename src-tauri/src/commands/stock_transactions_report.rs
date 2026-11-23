//! Tauri command of the stock transactions report: validates the request and delegates to the
//! service.

use crate::db::Database;
use crate::error::Result;
use crate::models::{StockTransactionsReport, StockTransactionsReportRequest};
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
