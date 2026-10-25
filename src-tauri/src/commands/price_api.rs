//! Price API commands for stock and crypto price fetching

use crate::commands::portfolio;
use crate::db::Database;
use crate::error::Result;
use crate::services::crypto_symbols::CryptoSearchResult;
use crate::services::history_recalc::{self, HistoryRecalc, Job};
use crate::services::price_api::{
    self, CryptoPriceResult, DividendResult, StockPriceRefreshResult, StockSearchResult,
};
use tauri::{AppHandle, State};

/// Queue the history rebuild a stock price refresh asks for. The tickers in
/// `result.unit_changed` have a stored history that was valued in another unit than the quotes
/// now are (earlier versions guessed the currency from the ticker suffix; see
/// `quote_unit::unit_changed`); the tickers in `result.new_splits` have a new pending split whose
/// pre-split closes are still the unadjusted ones (spec D18). Each is rebuilt from its earliest
/// transaction ([`rebuild_full_history`]). Every command that runs the refresh calls this,
/// because the refresh records the new unit (and the split) and would not report it again.
pub fn rebuild_history_after_quote_refresh(
    app: &AppHandle,
    db: &Database,
    recalc: &HistoryRecalc,
    result: &StockPriceRefreshResult,
) {
    let tickers: Vec<String> = result
        .unit_changed
        .iter()
        .chain(&result.new_splits)
        .cloned()
        .collect();
    rebuild_full_history(app, db, recalc, &tickers);
}

/// Queue a rebuild of each ticker from its earliest transaction; the rebuild also re-derives the
/// portfolio history of those days. A ticker without transactions (a watchlist ticker) has no
/// history. Never fails: the refresh that asked for it went through, so a problem is only
/// logged.
fn rebuild_full_history(
    app: &AppHandle,
    db: &Database,
    recalc: &HistoryRecalc,
    tickers: &[String],
) {
    if tickers.is_empty() {
        return;
    }
    match db.with_conn(|conn| history_recalc::rebuild_jobs(conn, tickers)) {
        Ok(jobs) => portfolio::schedule_history_rebuild(app, db, recalc, Job::stocks(jobs)),
        Err(e) => log::warn!("[RECALC] Could not queue the rebuild after a stock refresh: {e}"),
    }
}

/// Queue a rebuild of each crypto ticker from its first transaction (a holding whose Yahoo symbol
/// was just resolved or picked: its history moves to that coin's Yahoo series, spec D7 / D9). A
/// ticker without transactions has no history. Never fails: a problem is only logged.
pub fn rebuild_crypto_history(
    app: &AppHandle,
    db: &Database,
    recalc: &HistoryRecalc,
    tickers: &[String],
) {
    if tickers.is_empty() {
        return;
    }
    match db.with_conn(|conn| history_recalc::crypto_rebuild_jobs(conn, tickers)) {
        Ok(jobs) if !jobs.is_empty() => portfolio::schedule_history_rebuild(
            app,
            db,
            recalc,
            jobs.into_iter()
                .map(|(ticker, first)| Job::CryptoTicker(ticker, first))
                .collect(),
        ),
        Ok(_) => {}
        Err(e) => log::warn!("[RECALC] Could not queue the rebuild of crypto history: {e}"),
    }
}

/// Refresh stock prices from Yahoo Finance
/// Batches all tickers into a single request (no rate limiting issues)
/// No API key required
#[tauri::command]
pub async fn refresh_stock_prices(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    force_refresh: Option<bool>,
) -> Result<StockPriceRefreshResult> {
    // Get all tickers from investments
    let tickers: Vec<String> = db.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT DISTINCT si.ticker FROM stock_investments si")?;
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
            new_splits: vec![],
        });
    }

    // Fetch prices using Yahoo Finance (batched request, no API key needed)
    let result =
        price_api::refresh_stock_prices_yahoo(&db, tickers, force_refresh.unwrap_or(false)).await?;

    // Before the snapshot: it can fail, and neither a unit change nor a split is reported a
    // second time.
    rebuild_history_after_quote_refresh(&app, &db, &recalc, &result);

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db).await?;

    Ok(result)
}

/// Refresh crypto prices from Yahoo Finance (spec D7). The holdings whose symbol this refresh
/// resolved get their history rebuilt from their first transaction. `force_refresh` skips the
/// 4 h cache (the explicit refresh buttons pass it).
#[tauri::command]
pub async fn refresh_crypto_prices(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    force_refresh: Option<bool>,
) -> Result<Vec<CryptoPriceResult>> {
    let result = price_api::refresh_crypto_prices(&db, force_refresh.unwrap_or(false)).await?;

    // Before the snapshot: it can fail, and a resolution is not reported a second time.
    rebuild_crypto_history(&app, &db, &recalc, &result.resolved);

    crate::commands::portfolio::update_todays_snapshot(&db).await?;

    Ok(result.updated)
}

/// Search Yahoo Finance for coins (spec D6).
#[tauri::command]
pub async fn search_crypto(query: String) -> Result<Vec<CryptoSearchResult>> {
    price_api::search_crypto(&query).await
}

/// Refresh dividend data from Yahoo Finance
/// No API key required
/// A ticker whose fetched year holds a new split is rebuilt from its first transaction (D18).
#[tauri::command]
pub async fn refresh_dividends(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
) -> Result<Vec<DividendResult>> {
    // Get all tickers from investments
    let tickers: Vec<String> = db.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT DISTINCT ticker FROM stock_investments")?;
        let tickers = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(tickers)
    })?;

    if tickers.is_empty() {
        return Ok(vec![]);
    }

    // Fetch dividends using Yahoo Finance (no API key needed)
    let refresh = price_api::refresh_dividends(&db, tickers).await?;
    rebuild_full_history(&app, &db, &recalc, &refresh.new_splits);
    Ok(refresh.results)
}

/// Search for stock tickers using Yahoo Finance
#[tauri::command]
pub async fn search_stock_tickers(query: String) -> Result<Vec<StockSearchResult>> {
    price_api::search_stock_tickers(&query).await
}
