//! Company data for the stock position detail — thin wrapper per rust-backend rules.

use crate::db::Database;
use crate::error::Result;
use crate::models::company_info::StockCompanyInfo;
use crate::models::stock_monitor::InsertWatchedStock;
use crate::services::company_info;
use crate::services::price_api;
use tauri::State;

/// Company data of a stock (sector, valuation figures, 52-week range, dividend) from the
/// `stock_data` cache. With `refresh`, metadata that was never fetched or is older than 24 h is
/// fetched from Yahoo first. The network call runs outside the DB lock and is failure-tolerant
/// (as in the stock monitor): whatever is stored is returned either way, so the card can say
/// "nothing stored" instead of the whole query failing.
#[tauri::command]
pub async fn get_stock_company_info(
    db: State<'_, Database>,
    ticker: String,
    refresh: bool,
) -> Result<StockCompanyInfo> {
    // Same ticker grammar as the stock monitor commands
    InsertWatchedStock {
        ticker: ticker.clone(),
    }
    .validate()?;
    let ticker = ticker.trim().to_uppercase();

    let mut info = db.with_conn(|conn| company_info::read_company_info(conn, &ticker))?;
    let now = chrono::Utc::now().timestamp();
    if refresh && company_info::metadata_is_stale(info.metadata_fetched_at, now) {
        if let Err(e) =
            price_api::refresh_stock_metadata_yahoo(&db, vec![ticker.clone()], false).await
        {
            // The ticker stays out of the message: only debug may name portfolio data
            log::warn!("[COMPANY INFO] Metadata refresh failed: {}", e);
        }
        info = db.with_conn(|conn| company_info::read_company_info(conn, &ticker))?;
    }
    Ok(info)
}
