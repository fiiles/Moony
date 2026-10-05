//! Stock Monitor (watchlist) commands — thin wrappers per rust-backend rules.
//!
//! Spec D10: these commands never call update_todays_snapshot — the watchlist
//! has zero effect on portfolio value. The one thing they share with the portfolio refresh is
//! the history rebuild after a quote unit change (a followed ticker can be a held one, and the
//! refresh reports a change only once), see `rebuild_history_for_unit_changes`.

use crate::commands::price_api::rebuild_history_for_unit_changes;
use crate::db::Database;
use crate::error::Result;
use crate::models::stock_monitor::{
    InsertWatchedStock, StockMonitorDetail, StockPricePoint, WatchedStock, WatchedStockRow,
};
use crate::services::history_recalc::HistoryRecalc;
use crate::services::price_api::{self, StockPriceRefreshResult};
use crate::services::stock_monitor;
use tauri::{AppHandle, State};

/// Watchlist price staleness TTL (spec D3); portfolio keeps its 4 h TTL
const WATCHLIST_PRICE_TTL_SECONDS: i64 = 15 * 60;

#[tauri::command]
pub async fn get_watched_stocks(db: State<'_, Database>) -> Result<Vec<WatchedStockRow>> {
    db.with_conn(stock_monitor::list_watched_stocks)
}

#[tauri::command]
pub async fn follow_stock(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    ticker: String,
) -> Result<WatchedStock> {
    let data = InsertWatchedStock { ticker };
    data.validate()?;
    let watched = db.with_conn(|conn| stock_monitor::follow_stock(conn, &data))?;
    // Populate the cache right away so the new row has price + metadata.
    // Fetch errors are ignored: following offline is fine (spec §8).
    let tickers = vec![watched.ticker.clone()];
    if let Ok(result) = price_api::refresh_stock_prices_yahoo_with_ttl(
        &db,
        tickers.clone(),
        false,
        WATCHLIST_PRICE_TTL_SECONDS,
    )
    .await
    {
        rebuild_history_for_unit_changes(&app, &db, &recalc, &result);
    }
    let _ = price_api::refresh_stock_metadata_yahoo(&db, tickers, false).await;
    Ok(watched)
}

/// Tickers the "follow my portfolio" action would add — drives both the
/// button's visibility and its count.
#[tauri::command]
pub async fn get_portfolio_follow_candidates(db: State<'_, Database>) -> Result<Vec<String>> {
    db.with_conn(stock_monitor::portfolio_follow_candidates)
}

/// Follow every portfolio stock that is not followed yet, then populate the
/// price/metadata cache for the newly added tickers in one batch (fetch
/// failures are ignored: following offline is fine, the poll fills them in).
#[tauri::command]
pub async fn follow_portfolio_stocks(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
) -> Result<Vec<String>> {
    let added = db.with_conn(stock_monitor::follow_portfolio_stocks)?;
    if !added.is_empty() {
        if let Ok(result) = price_api::refresh_stock_prices_yahoo_with_ttl(
            &db,
            added.clone(),
            false,
            WATCHLIST_PRICE_TTL_SECONDS,
        )
        .await
        {
            rebuild_history_for_unit_changes(&app, &db, &recalc, &result);
        }
        let _ = price_api::refresh_stock_metadata_yahoo(&db, added.clone(), false).await;
    }
    Ok(added)
}

#[tauri::command]
pub async fn unfollow_stock(db: State<'_, Database>, ticker: String) -> Result<()> {
    db.with_conn(|conn| stock_monitor::unfollow_stock(conn, &ticker))
}

#[tauri::command]
pub async fn set_watched_target_price(
    db: State<'_, Database>,
    ticker: String,
    target_price: Option<String>,
    target_direction: Option<String>,
) -> Result<WatchedStock> {
    db.with_conn(|conn| {
        stock_monitor::set_target_price(
            conn,
            &ticker,
            target_price.clone(),
            target_direction.clone(),
        )
    })
}

#[tauri::command]
pub async fn update_watched_notes(
    db: State<'_, Database>,
    ticker: String,
    notes: String,
) -> Result<WatchedStock> {
    db.with_conn(|conn| stock_monitor::update_notes(conn, &ticker, notes.clone()))
}

/// Refresh prices (15-min TTL) + opportunistic metadata (24 h TTL) for all
/// watched tickers. force_refresh bypasses the price TTL (manual button).
#[tauri::command]
pub async fn refresh_watched_stock_prices(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    force_refresh: Option<bool>,
) -> Result<StockPriceRefreshResult> {
    let tickers: Vec<String> = db.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT ticker FROM watched_stocks")?;
        let tickers = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(tickers)
    })?;
    if tickers.is_empty() {
        return Ok(StockPriceRefreshResult {
            updated: vec![],
            remaining_tickers: vec![],
            rate_limit_hit: false,
            unit_changed: vec![],
        });
    }
    let force = force_refresh.unwrap_or(false);
    let result = price_api::refresh_stock_prices_yahoo_with_ttl(
        &db,
        tickers.clone(),
        force,
        WATCHLIST_PRICE_TTL_SECONDS,
    )
    .await?;
    rebuild_history_for_unit_changes(&app, &db, &recalc, &result);
    // Manual refresh forces metadata too, so corrected fields (e.g. a fixed
    // dividend yield) heal immediately instead of after the 24 h TTL.
    let _ = price_api::refresh_stock_metadata_yahoo(&db, tickers, force).await;
    Ok(result)
}

/// Detail for any ticker (followed or not). Ensures the cache is fresh first
/// (both fetches TTL-guarded and failure-tolerant), then reads from the DB.
#[tauri::command]
pub async fn get_stock_monitor_detail(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    ticker: String,
    force_refresh: Option<bool>,
) -> Result<StockMonitorDetail> {
    let data = InsertWatchedStock {
        ticker: ticker.clone(),
    };
    data.validate()?;
    let force = force_refresh.unwrap_or(false);
    let t = ticker.trim().to_uppercase();
    if let Ok(result) = price_api::refresh_stock_prices_yahoo_with_ttl(
        &db,
        vec![t.clone()],
        force,
        WATCHLIST_PRICE_TTL_SECONDS,
    )
    .await
    {
        rebuild_history_for_unit_changes(&app, &db, &recalc, &result);
    }
    let _ = price_api::refresh_stock_metadata_yahoo(&db, vec![t.clone()], force).await;
    db.with_conn(|conn| stock_monitor::get_stock_monitor_detail(conn, &t))
}

#[tauri::command]
pub async fn get_stock_price_range(ticker: String, period: String) -> Result<Vec<StockPricePoint>> {
    let data = InsertWatchedStock {
        ticker: ticker.clone(),
    };
    data.validate()?;
    price_api::get_stock_price_range(&ticker, &period).await
}
