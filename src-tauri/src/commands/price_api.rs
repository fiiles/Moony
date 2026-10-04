//! Price API commands for stock and crypto price fetching

use crate::commands::portfolio;
use crate::db::Database;
use crate::error::Result;
use crate::services::history_recalc::{self, HistoryRecalc};
use crate::services::price_api::{
    self, ApiKeys, CoinGeckoSearchResult, CryptoPriceResult, DividendResult,
    StockPriceRefreshResult, StockSearchResult,
};
use std::collections::HashMap;
use tauri::{AppHandle, State};

/// Queue the history rebuild a stock price refresh asks for. The tickers in
/// `result.unit_changed` have a stored history that was valued in another unit than the quotes
/// now are (earlier versions guessed the currency from the ticker suffix; see
/// `quote_unit::unit_changed`), so each is rebuilt from its earliest transaction; the rebuild also
/// re-derives the portfolio history of those days. A ticker without transactions (a watchlist
/// ticker) has no history. Every command that runs the refresh calls this, because the refresh
/// records the new unit and would not report the change again. Never fails: the refresh itself
/// went through, so a problem is only logged.
pub fn rebuild_history_for_unit_changes(
    app: &AppHandle,
    db: &Database,
    recalc: &HistoryRecalc,
    result: &StockPriceRefreshResult,
) {
    if result.unit_changed.is_empty() {
        return;
    }
    match db.with_conn(|conn| history_recalc::rebuild_jobs(conn, &result.unit_changed)) {
        Ok(jobs) => portfolio::schedule_stock_history_rebuild(app, db, recalc, jobs),
        Err(e) => log::warn!("[RECALC] Could not queue the rebuild after a quote unit change: {e}"),
    }
}

/// Get all API keys
#[tauri::command]
pub async fn get_api_keys(db: State<'_, Database>) -> Result<ApiKeys> {
    price_api::get_api_keys(&db)
}

/// Set API keys
#[tauri::command]
pub async fn set_api_keys(db: State<'_, Database>, keys: ApiKeys) -> Result<()> {
    price_api::set_api_keys(&db, &keys)
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
        });
    }

    // Fetch prices using Yahoo Finance (batched request, no API key needed)
    let result =
        price_api::refresh_stock_prices_yahoo(&db, tickers, force_refresh.unwrap_or(false)).await?;

    // Before the snapshot: it can fail, and the unit change is not reported a second time.
    rebuild_history_for_unit_changes(&app, &db, &recalc, &result);

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db).await?;

    Ok(result)
}

/// Refresh crypto prices from CoinGecko
#[tauri::command]
pub async fn refresh_crypto_prices(db: State<'_, Database>) -> Result<Vec<CryptoPriceResult>> {
    // Get API key (optional for CoinGecko)
    let keys = price_api::get_api_keys(&db)?;
    let api_key = keys.coingecko.as_deref();

    // Get all crypto investments with their coingecko IDs
    let id_to_ticker: HashMap<String, String> = db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT ticker, coingecko_id FROM crypto_investments WHERE coingecko_id IS NOT NULL",
        )?;
        let map: HashMap<String, String> = stmt
            .query_map([], |row| {
                let ticker: String = row.get(0)?;
                let coingecko_id: String = row.get(1)?;
                Ok((coingecko_id, ticker))
            })?
            .filter_map(|r| r.ok())
            .collect();
        Ok(map)
    })?;

    if id_to_ticker.is_empty() {
        return Ok(vec![]);
    }

    // Fetch prices
    let result = price_api::refresh_crypto_prices(&db, api_key, id_to_ticker).await?;

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db).await?;

    Ok(result)
}

/// Search for cryptocurrencies using CoinGecko
#[tauri::command]
pub async fn search_crypto(
    db: State<'_, Database>,
    query: String,
) -> Result<Vec<CoinGeckoSearchResult>> {
    // Get API key (optional)
    let keys = price_api::get_api_keys(&db)?;
    let api_key = keys.coingecko.as_deref();

    price_api::search_crypto(api_key, &query).await
}

/// Refresh dividend data from Yahoo Finance
/// No API key required
#[tauri::command]
pub async fn refresh_dividends(db: State<'_, Database>) -> Result<Vec<DividendResult>> {
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
    price_api::refresh_dividends(&db, tickers).await
}

/// Search for stock tickers using Yahoo Finance
#[tauri::command]
pub async fn search_stock_tickers(query: String) -> Result<Vec<StockSearchResult>> {
    price_api::search_stock_tickers(&query).await
}
