//! External API services for price fetching
//!
//! - Yahoo Finance API for stock prices and dividends (via yahoo_finance_api crate)
//! - CoinGecko API for cryptocurrency prices and search

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::stock_monitor::StockPricePoint;
use crate::services::quote_unit::{quote_unit, unit_changed as quote_unit_changed, QuoteUnit};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use yahoo_finance_api::{YResponse, YahooError};

/// Get current Unix timestamp. Uses expect() because system clock
/// being before Unix epoch indicates a fundamentally broken system.
fn unix_timestamp_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("System clock is before Unix epoch")
        .as_secs() as i64
}

/// Bounded Yahoo connector: the crate's default reqwest client has NO timeout,
/// which can hang commands indefinitely on dead networks. 15 s bounds every call.
fn yahoo_connector(
) -> std::result::Result<yahoo_finance_api::YahooConnector, yahoo_finance_api::YahooError> {
    yahoo_finance_api::YahooConnector::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
}

// ============================================================================
// Reading a Yahoo chart response
// ============================================================================
//
// Every price in a response is in the unit its metadata names (`quote_unit`), not the one the
// ticker's suffix suggests; the readers below turn them into the stored unit in one place.

/// The unit of a chart response: the currency its metadata reports, else the suffix guess.
fn unit_of(response: &YResponse, ticker: &str) -> QuoteUnit {
    let reported = response.metadata().ok().and_then(|meta| meta.currency);
    quote_unit(reported.as_deref(), ticker)
}

/// The currency a chart response reports for its prices, as it came (`GBp`, `USD`), trimmed;
/// `None` when it names none.
fn reported_currency(response: &YResponse) -> Option<String> {
    response
        .metadata()
        .ok()
        .and_then(|meta| meta.currency)
        .map(|code| code.trim().to_string())
        .filter(|code| !code.is_empty())
}

/// What a 1d chart response says about the latest price, in the quote's own unit.
#[derive(Debug, PartialEq)]
struct ChartQuote {
    price: f64,
    previous_close: Option<f64>,
    /// `meta.currency` as Yahoo reported it (`GBp`, `USD`); `None` when the response names none.
    currency: Option<String>,
}

/// The latest price (the metadata's regular market price, else the last quote's close), the
/// previous session's close and the reported currency; `None` without a positive price.
fn read_chart_quote(response: &YResponse) -> Option<ChartQuote> {
    // Prefer regular_market_price from metadata (reflects current price), fall back to
    // last_quote().close if metadata price is unavailable.
    let meta = response.metadata().ok();
    let price = meta
        .as_ref()
        .and_then(|m| m.regular_market_price)
        .filter(|&p| p > 0.0)
        .or_else(|| {
            response
                .last_quote()
                .ok()
                .map(|q| q.close)
                .filter(|&p| p > 0.0)
        })?;
    // Yesterday's close for the day-change calc (spec D4).
    let previous_close = meta
        .as_ref()
        .and_then(|m| m.previous_close.or(m.chart_previous_close))
        .filter(|&p| p > 0.0);
    Some(ChartQuote {
        price,
        previous_close,
        currency: reported_currency(response),
    })
}

/// The closes of a chart response in the stored unit.
fn history_points(
    response: &YResponse,
    ticker: &str,
) -> std::result::Result<Vec<HistoricalPrice>, YahooError> {
    let quotes = response.quotes()?;
    let unit = unit_of(response, ticker);
    Ok(quotes
        .iter()
        .map(|q| HistoricalPrice {
            timestamp: q.timestamp,
            price: unit.apply(q.close),
            currency: unit.currency.clone(),
        })
        .collect())
}

/// The positive closes of a chart response in the stored unit, for the Stock Monitor chart.
fn range_points(
    response: &YResponse,
    ticker: &str,
) -> std::result::Result<Vec<StockPricePoint>, YahooError> {
    let quotes = response.quotes()?;
    let unit = unit_of(response, ticker);
    Ok(quotes
        .iter()
        .filter(|q| q.close > 0.0)
        .map(|q| StockPricePoint {
            timestamp: q.timestamp,
            price: unit.apply(q.close),
            currency: unit.currency.clone(),
        })
        .collect())
}

/// The dividends of a chart response summed in the stored unit; `None` when the response holds no
/// result at all. Yahoo reports them in the quote unit, like the prices (BARC.L: 5.6 and 5.9
/// pence). A year without dividends sums to a positive zero: `Iterator::sum` of nothing is -0.0,
/// which would be stored as "-0.00".
fn dividend_sum(response: &YResponse, unit: &QuoteUnit) -> Option<f64> {
    let dividends = response.dividends().ok()?;
    let sum = dividends.iter().fold(0.0, |sum, d| sum + d.amount);
    Some(unit.apply(sum))
}

/// A price figure of a Yahoo response as stored text; `None` when absent or not finite.
fn fmt_price(unit: &QuoteUnit, quoted: Option<f64>) -> Option<String> {
    quoted
        .filter(|price| price.is_finite())
        .map(|price| unit.price_text(unit.apply(price)))
}

// ============================================================================
// Storing a quote
// ============================================================================

/// What storing a quote did.
#[derive(Debug, PartialEq)]
struct StoredQuote {
    /// The price as stored, in `currency`.
    price: f64,
    currency: String,
    /// The unit differs from the one the ticker's history was written in
    /// (`quote_unit::unit_changed`): that history has to be rebuilt.
    unit_changed: bool,
}

/// Store a ticker's latest quote: the price and previous close in the unit the response
/// reported, the raw code beside them, and whether the unit differs from the stored history's.
///
/// A response without a currency keeps the unit recorded at the last refresh instead of falling
/// back to the suffix guess, so one thin answer cannot turn a pence quote into pounds. When the
/// unit did change, what was stored in the old one is dropped with it: the previous close, the
/// 52-week range (the company data is fetched again) and the cached dividends (fetched again at
/// the next dividend refresh).
fn store_stock_quote(
    conn: &rusqlite::Connection,
    ticker: &str,
    quote: &ChartQuote,
    now: i64,
) -> Result<StoredQuote> {
    use rusqlite::OptionalExtension;

    let tx = conn.unchecked_transaction()?;
    let recorded: Option<String> = tx
        .query_row(
            "SELECT quote_currency FROM stock_data WHERE ticker = ?1",
            [ticker],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    let reported = quote.currency.as_deref().or(recorded.as_deref());
    let unit = quote_unit(reported, ticker);
    let unit_changed = quote_unit_changed(recorded.as_deref(), &unit, ticker);
    let price = unit.apply(quote.price);
    let previous_close = quote.previous_close.map(|p| unit.price_text(unit.apply(p)));

    tx.execute(
        "INSERT INTO stock_data (id, ticker, original_price, currency, price_date, fetched_at, previous_close, quote_currency)
         VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7)
         ON CONFLICT(ticker) DO UPDATE SET
           original_price = ?3, currency = ?4, price_date = ?5, fetched_at = ?5,
           previous_close = CASE WHEN ?8 THEN ?6 ELSE COALESCE(?6, previous_close) END,
           quote_currency = ?7",
        rusqlite::params![
            uuid::Uuid::new_v4().to_string(),
            ticker,
            unit.price_text(price),
            &unit.currency,
            now,
            previous_close,
            reported,
            unit_changed,
        ],
    )?;
    if unit_changed {
        tx.execute(
            "UPDATE stock_data SET fifty_two_week_high = NULL, fifty_two_week_low = NULL,
                                   metadata_fetched_at = NULL
             WHERE ticker = ?1",
            [ticker],
        )?;
        tx.execute(
            "UPDATE dividend_data SET last_fetched_at = 0 WHERE ticker = ?1",
            [ticker],
        )?;
    }
    tx.commit()?;

    Ok(StoredQuote {
        price,
        currency: unit.currency,
        unit_changed,
    })
}

// ============================================================================
// CoinGecko API Types
// ============================================================================

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct CoinGeckoSearchResult {
    pub id: String,
    pub symbol: String,
    pub name: String,
    pub market_cap_rank: Option<i32>,
    pub thumb: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CoinGeckoSearchResponse {
    coins: Option<Vec<CoinGeckoSearchResult>>,
}

// ============================================================================
// Dividend Cache Configuration
// ============================================================================

const DIVIDEND_CACHE_DAYS: i64 = 30; // Only refetch dividends if older than 30 days
const PRICE_CACHE_HOURS: i64 = 4; // Skip price fetch if updated within last 4 hours

#[derive(Debug, Serialize)]
pub struct DividendResult {
    pub ticker: String,
    pub yearly_sum: f64,
    pub currency: String,
}

#[derive(Debug, Serialize)]
pub struct StockPriceResult {
    pub ticker: String,
    pub price: f64,
    pub currency: String,
}

/// Result from stock price refresh
#[derive(Debug, Serialize)]
pub struct StockPriceRefreshResult {
    pub updated: Vec<StockPriceResult>,
    pub remaining_tickers: Vec<String>,
    pub rate_limit_hit: bool,
    /// Tickers whose quote unit differs from the one their stored history was written in (earlier
    /// versions guessed it from the ticker suffix; see `quote_unit::unit_changed`). The command
    /// that ran the refresh queues a history rebuild for them. Not part of the wire contract.
    #[serde(skip)]
    pub unit_changed: Vec<String>,
}

/// Refresh stock prices from Yahoo Finance using yahoo_finance_api crate
/// This crate handles the cookie/crumb authentication that Yahoo requires
/// Works for US, European (.DE, .PA, .MI, etc.) and HK (.HK) stocks
pub async fn refresh_stock_prices_yahoo(
    db: &Database,
    tickers: Vec<String>,
    force_refresh: bool,
) -> Result<StockPriceRefreshResult> {
    refresh_stock_prices_yahoo_with_ttl(db, tickers, force_refresh, PRICE_CACHE_HOURS * 3600).await
}

/// Same as refresh_stock_prices_yahoo but with a caller-chosen staleness TTL.
/// The Stock Monitor uses 15 minutes (spec D3); the portfolio keeps 4 hours.
pub async fn refresh_stock_prices_yahoo_with_ttl(
    db: &Database,
    tickers: Vec<String>,
    force_refresh: bool,
    max_age_seconds: i64,
) -> Result<StockPriceRefreshResult> {
    if tickers.is_empty() {
        return Ok(StockPriceRefreshResult {
            updated: vec![],
            remaining_tickers: vec![],
            rate_limit_hit: false,
            unit_changed: vec![],
        });
    }

    let mut updated_prices = Vec::new();
    let mut unit_changed: Vec<String> = Vec::new();
    let mut failed_tickers: Vec<(String, String)> = Vec::new();
    let now = unix_timestamp_now();

    log::info!(
        "[YAHOO FINANCE] Fetching prices for {} tickers",
        tickers.len()
    );
    log::debug!("[YAHOO FINANCE] Tickers: {:?}", tickers);

    // Create Yahoo connector (handles cookies and authentication)
    let provider = yahoo_connector()
        .map_err(|e| AppError::ExternalApi(format!("Yahoo connector failed: {}", e)))?;

    // Process each ticker (with caching check)
    for ticker in &tickers {
        let ticker_upper = ticker.to_uppercase();

        // Check if we have a recent price (within PRICE_CACHE_HOURS)
        // Skip cache check if force_refresh is true
        let should_fetch = if force_refresh {
            true
        } else {
            db.with_conn(|conn| {
                let last_fetched: Option<i64> = conn
                    .query_row(
                        "SELECT fetched_at FROM stock_data WHERE ticker = ?1",
                        [&ticker_upper],
                        |row| row.get(0),
                    )
                    .ok();

                if let Some(fetched_at) = last_fetched {
                    Ok(now - fetched_at >= max_age_seconds)
                } else {
                    Ok(true) // No cached price, should fetch
                }
            })?
        };

        if !should_fetch {
            log::debug!(
                "[YAHOO FINANCE] Skipping {} (cached within {}s)",
                ticker,
                max_age_seconds
            );
            continue;
        }

        // Fetch an intraday 1d window and read regular_market_price from the
        // response metadata — always populated with the most recent market
        // price regardless of market hours; last_quote().close is only a
        // fallback since today's open bar can have close=null.
        // The 1d window also matters for previous_close: its metadata carries
        // the true previous session close, while get_latest_quotes fetches a
        // 1-MONTH window whose chart_previous_close is the month-ago close
        // (verified live: AAPL meta gave 333.74 vs the real previous close
        // 305.93 → a -8.43 % "day" change that was actually the monthly move).
        match provider.get_quote_range(ticker, "5m", "1d").await {
            Ok(response) => match read_chart_quote(&response) {
                Some(quote) => {
                    if quote.previous_close.is_none() {
                        log::debug!(
                            "[YAHOO FINANCE] {} - no previous_close in response; keeping stored value",
                            ticker_upper
                        );
                    }
                    // Stored in the unit the response reports (pence become pounds), never in
                    // the one the ticker's suffix suggests.
                    match db.with_conn(|conn| store_stock_quote(conn, &ticker_upper, &quote, now)) {
                        Ok(stored) => {
                            if stored.unit_changed {
                                unit_changed.push(ticker_upper.clone());
                            }
                            updated_prices.push(StockPriceResult {
                                ticker: ticker_upper,
                                price: stored.price,
                                currency: stored.currency,
                            });
                        }
                        Err(e) => {
                            failed_tickers.push((ticker.clone(), format!("DB error: {}", e)));
                            continue;
                        }
                    }
                }
                None => {
                    failed_tickers.push((ticker.clone(), "price unavailable".to_string()));
                }
            },
            Err(e) => {
                failed_tickers.push((ticker.clone(), format!("{}", e)));
            }
        }
    }

    // Summary logging
    log::info!("[YAHOO FINANCE] ========== SUMMARY ==========");
    log::info!("[YAHOO FINANCE] Updated: {} tickers", updated_prices.len());
    if !updated_prices.is_empty() {
        let updated_list: Vec<_> = updated_prices.iter().map(|p| p.ticker.as_str()).collect();
        log::debug!("[YAHOO FINANCE] Successfully updated: {:?}", updated_list);
    }
    if !unit_changed.is_empty() {
        log::info!(
            "[YAHOO FINANCE] Quote unit changed for {} tickers; their history is rebuilt",
            unit_changed.len()
        );
        log::debug!("[YAHOO FINANCE] Unit changed: {:?}", unit_changed);
    }
    if !failed_tickers.is_empty() {
        log::warn!("[YAHOO FINANCE] Failed: {} tickers", failed_tickers.len());
        for (ticker, reason) in &failed_tickers {
            log::debug!("[YAHOO FINANCE]   - {}: {}", ticker, reason);
        }
    }
    log::info!("[YAHOO FINANCE] ==============================");

    Ok(StockPriceRefreshResult {
        updated: updated_prices,
        remaining_tickers: vec![],
        rate_limit_hit: false,
        unit_changed,
    })
}

/// Get currency from ticker suffix
pub(crate) fn get_currency_from_ticker(ticker: &str) -> &'static str {
    if let Some(suffix) = ticker.split('.').nth(1) {
        match suffix.to_uppercase().as_str() {
            "L" | "LON" => "GBP",          // London
            "PA" => "EUR",                 // Paris
            "AS" => "EUR",                 // Amsterdam
            "BR" => "EUR",                 // Brussels
            "DE" | "F" | "XETRA" => "EUR", // Germany
            "SW" => "CHF",                 // Swiss
            "ST" => "SEK",                 // Stockholm
            "HE" => "EUR",                 // Helsinki
            "MI" => "EUR",                 // Milan
            "VI" => "EUR",                 // Vienna
            "PR" => "CZK",                 // Prague
            "T" | "TYO" => "JPY",          // Tokyo
            "HK" => "HKD",                 // Hong Kong
            "SS" | "SZ" => "CNY",          // Shanghai/Shenzhen
            "AM" => "EUR",                 // Amsterdam
            _ => "USD",
        }
    } else {
        "USD" // Default for US tickers without suffix
    }
}

// ============================================================================
// Crypto Price Refresh (CoinGecko)
// ============================================================================

#[derive(Debug, Serialize)]
pub struct CryptoPriceResult {
    pub ticker: String,
    pub price: f64,
    pub currency: String,
}

/// Refresh crypto prices from CoinGecko API
/// Takes coingecko_id -> ticker mapping
pub async fn refresh_crypto_prices(
    db: &Database,
    api_key: Option<&str>,
    id_to_ticker: HashMap<String, String>,
) -> Result<Vec<CryptoPriceResult>> {
    if id_to_ticker.is_empty() {
        return Ok(vec![]);
    }

    let ids: Vec<&str> = id_to_ticker.keys().map(|s| s.as_str()).collect();
    let ids_param = ids.join(",");
    let url = format!(
        "https://api.coingecko.com/api/v3/simple/price?ids={}&vs_currencies=usd",
        ids_param
    );

    log::debug!("[PRICE API] Fetching crypto prices for: {}", ids_param);

    let client = crate::services::http::client();
    let mut request = client.get(&url);

    // Add API key header if provided
    if let Some(key) = api_key {
        if !key.is_empty() {
            request = request.header("x-cg-demo-api-key", key);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| AppError::ExternalApi(format!("CoinGecko request failed: {}", e)))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| AppError::ExternalApi(format!("CoinGecko read error: {}", e)))?;

    if !status.is_success() {
        return Err(AppError::ExternalApi(coingecko_error_message(
            status, &body,
        )));
    }

    let price_data = parse_simple_price_usd(&body)?;

    let mut updated_prices = Vec::new();
    let now = unix_timestamp_now();

    for (coingecko_id, usd_price) in price_data {
        if let Some(ticker) = id_to_ticker.get(&coingecko_id) {
            let ticker_upper = ticker.to_uppercase();

            // Upsert into crypto_prices table
            db.with_conn(|conn| {
                conn.execute(
                    "INSERT INTO crypto_prices (id, symbol, coingecko_id, price, currency, fetched_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(symbol) DO UPDATE SET
                       price = ?4, currency = ?5, coingecko_id = ?3, fetched_at = ?6",
                    rusqlite::params![
                        uuid::Uuid::new_v4().to_string(),
                        &ticker_upper,
                        &coingecko_id,
                        format!("{:.2}", usd_price),
                        "USD",
                        now,
                    ],
                )?;
                Ok(())
            })?;

            log::debug!("[PRICE API] Updated {}: {} USD", ticker_upper, usd_price);

            updated_prices.push(CryptoPriceResult {
                ticker: ticker_upper,
                price: usd_price,
                currency: "USD".to_string(),
            });
        }
    }

    Ok(updated_prices)
}

/// Builds a readable error for a non-2xx CoinGecko response. Error bodies look like
/// `{"status":{"error_code":429,"error_message":"..."}}` or `{"error":"..."}`.
fn coingecko_error_message(status: reqwest::StatusCode, body: &str) -> String {
    let detail = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.pointer("/status/error_message")
                .or_else(|| v.get("error"))
                .and_then(|m| m.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| body.chars().take(200).collect());
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        format!(
            "CoinGecko rate limit exceeded, try again in a minute ({})",
            detail
        )
    } else {
        format!("CoinGecko returned HTTP {}: {}", status.as_u16(), detail)
    }
}

/// Parses a `/simple/price?vs_currencies=usd` body (`{"bitcoin":{"usd":12345.67}}`).
/// Coins with a missing or null `usd` price are skipped instead of failing the whole batch.
fn parse_simple_price_usd(body: &str) -> Result<HashMap<String, f64>> {
    let raw: HashMap<String, HashMap<String, Option<f64>>> =
        serde_json::from_str(body).map_err(|e| {
            let snippet: String = body.chars().take(200).collect();
            AppError::ExternalApi(format!("CoinGecko parse error: {} (body: {})", e, snippet))
        })?;
    Ok(raw
        .into_iter()
        .filter_map(|(id, prices)| prices.get("usd").copied().flatten().map(|p| (id, p)))
        .collect())
}

// ============================================================================
// Stock Ticker Search (Yahoo Finance)
// ============================================================================

#[derive(Debug, Serialize)]
pub struct StockSearchResult {
    pub symbol: String,
    pub shortname: String,
    pub exchange: String,
}

/// Search for stock tickers using Yahoo Finance
/// Filters for EQUITY and ETF types only
pub async fn search_stock_tickers(query: &str) -> Result<Vec<StockSearchResult>> {
    log::debug!("[TICKER SEARCH] Searching for: {}", query);

    let provider = yahoo_connector()
        .map_err(|e| AppError::ExternalApi(format!("Yahoo connector failed: {}", e)))?;

    let search_result = provider
        .search_ticker(query)
        .await
        .map_err(|e| AppError::ExternalApi(format!("Yahoo Finance search failed: {}", e)))?;

    let results: Vec<StockSearchResult> = search_result
        .quotes
        .into_iter()
        .filter(|quote| quote.quote_type == "EQUITY" || quote.quote_type == "ETF")
        .map(|quote| {
            let shortname = if !quote.short_name.is_empty() {
                quote.short_name
            } else if !quote.long_name.is_empty() {
                quote.long_name
            } else {
                quote.symbol.clone()
            };

            StockSearchResult {
                symbol: quote.symbol,
                shortname,
                exchange: quote.exchange,
            }
        })
        .collect();

    log::debug!(
        "[TICKER SEARCH] Found {} results for '{}'",
        results.len(),
        query
    );

    Ok(results)
}

// ============================================================================
// Crypto Search (CoinGecko)
// ============================================================================

/// Search for cryptocurrencies by name/symbol
pub async fn search_crypto(
    api_key: Option<&str>,
    query: &str,
) -> Result<Vec<CoinGeckoSearchResult>> {
    let url = format!(
        "https://api.coingecko.com/api/v3/search?query={}",
        urlencoding::encode(query)
    );

    log::debug!("[PRICE API] Searching crypto: {}", query);

    let client = crate::services::http::client();
    let mut request = client.get(&url);

    if let Some(key) = api_key {
        if !key.is_empty() {
            request = request.header("x-cg-demo-api-key", key);
        }
    }

    let response = request
        .send()
        .await
        .map_err(|e| AppError::ExternalApi(format!("CoinGecko search failed: {}", e)))?;

    let data: CoinGeckoSearchResponse = response
        .json::<CoinGeckoSearchResponse>()
        .await
        .map_err(|e| AppError::ExternalApi(format!("CoinGecko search parse error: {}", e)))?;

    let mut results: Vec<CoinGeckoSearchResult> = data
        .coins
        .unwrap_or_default()
        .into_iter()
        .map(|c| CoinGeckoSearchResult {
            id: c.id,
            symbol: c.symbol.to_uppercase(),
            name: c.name,
            market_cap_rank: c.market_cap_rank,
            thumb: c.thumb,
        })
        .collect();

    // Sort by market cap rank (lower = better)
    results.sort_by_key(|r| r.market_cap_rank.unwrap_or(99999));

    Ok(results)
}

// ============================================================================
// Dividend Refresh (Yahoo Finance)
// ============================================================================

/// Refresh dividends from Yahoo Finance
/// Only fetches if data is older than 30 days
/// No API key required - uses yahoo_finance_api crate
pub async fn refresh_dividends(db: &Database, tickers: Vec<String>) -> Result<Vec<DividendResult>> {
    if tickers.is_empty() {
        return Ok(vec![]);
    }

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;

    let mut results = Vec::new();
    let mut failed_tickers: Vec<(String, String)> = Vec::new();

    log::info!(
        "[YAHOO DIVIDENDS] Fetching dividends for {} tickers",
        tickers.len()
    );

    // Create Yahoo connector
    let provider = yahoo_connector()
        .map_err(|e| AppError::ExternalApi(format!("Yahoo connector failed: {}", e)))?;

    for ticker in tickers {
        // Check if we need to refresh (data older than 30 days)
        let should_fetch = db.with_conn(|conn| {
            let last_fetched: Option<i64> = conn
                .query_row(
                    "SELECT last_fetched_at FROM dividend_data WHERE ticker = ?1",
                    [&ticker],
                    |row| row.get(0),
                )
                .ok();

            if let Some(fetched_at) = last_fetched {
                let days_since = (now - fetched_at) / (60 * 60 * 24);
                Ok(days_since >= DIVIDEND_CACHE_DAYS)
            } else {
                Ok(true)
            }
        })?;

        if !should_fetch {
            // Return cached data
            let cached: Option<(String, String)> = db.with_conn(|conn| {
                conn.query_row(
                    "SELECT yearly_dividend_sum, currency FROM dividend_data WHERE ticker = ?1",
                    [&ticker],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok()
                .map(Ok)
                .transpose()
            })?;

            if let Some((sum, currency)) = cached {
                log::debug!("[YAHOO DIVIDENDS] Using cached dividend for {}", ticker);
                results.push(DividendResult {
                    ticker,
                    yearly_sum: sum.parse().unwrap_or(0.0),
                    currency,
                });
            }
            continue;
        }

        // Fetch dividend history from Yahoo Finance (last 1 year + 1 day to ensure we get all)
        // yahoo_finance_api uses time::OffsetDateTime, not chrono
        let now_ts = unix_timestamp_now();
        let start_ts = now_ts - (366 * 24 * 60 * 60); // 366 days ago

        let start = time::OffsetDateTime::from_unix_timestamp(start_ts)
            .unwrap_or(time::OffsetDateTime::now_utc());
        let end = time::OffsetDateTime::now_utc();

        match provider.get_quote_history(&ticker, start, end).await {
            Ok(response) => {
                // Dividends come in the quote unit of the response (pence for GBp), so they are
                // converted exactly like its prices.
                let unit = unit_of(&response, &ticker);
                match dividend_sum(&response, &unit) {
                    Some(yearly_sum) => {
                        let currency = unit.currency.clone();

                        // Store in database
                        if let Err(e) = db.with_conn(|conn| {
                            conn.execute(
                                "INSERT INTO dividend_data (id, ticker, yearly_dividend_sum, currency, last_fetched_at)
                                 VALUES (?1, ?2, ?3, ?4, ?5)
                                 ON CONFLICT(ticker) DO UPDATE SET
                                   yearly_dividend_sum = ?3, currency = ?4, last_fetched_at = ?5",
                                rusqlite::params![
                                    uuid::Uuid::new_v4().to_string(),
                                    &ticker,
                                    unit.price_text(yearly_sum),
                                    &currency,
                                    now,
                                ],
                            )?;
                            Ok(())
                        }) {
                            failed_tickers.push((ticker.clone(), format!("DB error: {}", e)));
                            continue;
                        }

                        results.push(DividendResult {
                            ticker: ticker.clone(),
                            yearly_sum,
                            currency,
                        });
                    }
                    None => {
                        // No dividends found - store as 0
                        let currency = unit.currency.clone();

                        db.with_conn(|conn| {
                            conn.execute(
                                "INSERT INTO dividend_data (id, ticker, yearly_dividend_sum, currency, last_fetched_at)
                                 VALUES (?1, ?2, ?3, ?4, ?5)
                                 ON CONFLICT(ticker) DO UPDATE SET
                                   yearly_dividend_sum = ?3, currency = ?4, last_fetched_at = ?5",
                                rusqlite::params![
                                    uuid::Uuid::new_v4().to_string(),
                                    &ticker,
                                    "0.00",
                                    &currency,
                                    now,
                                ],
                            )?;
                            Ok(())
                        })?;

                        results.push(DividendResult {
                            ticker: ticker.clone(),
                            yearly_sum: 0.0,
                            currency,
                        });
                    }
                }
            }
            Err(e) => {
                failed_tickers.push((ticker.clone(), format!("{}", e)));
            }
        }
    }

    // Summary logging
    log::info!("[YAHOO DIVIDENDS] ========== SUMMARY ==========");
    log::info!("[YAHOO DIVIDENDS] Updated: {} tickers", results.len());
    if !failed_tickers.is_empty() {
        log::warn!("[YAHOO DIVIDENDS] Failed: {} tickers", failed_tickers.len());
        for (ticker, reason) in &failed_tickers {
            log::debug!("[YAHOO DIVIDENDS]   - {}: {}", ticker, reason);
        }
    }
    log::info!("[YAHOO DIVIDENDS] ==============================");

    Ok(results)
}

// ============================================================================
// Historical Price Fetching (for snapshot backfill)
// ============================================================================

/// Historical price data for a single ticker
#[derive(Debug, Clone, Serialize)]
pub struct HistoricalPrice {
    pub timestamp: i64,
    pub price: f64,
    pub currency: String,
}

/// Result of historical price backfill
#[derive(Debug, Serialize)]
pub struct BackfillProgress {
    pub days_processed: i32,
    pub total_days: i32,
    pub completed: bool,
}

/// Get historical stock prices from Yahoo Finance for a date range
/// Returns a map of ticker -> (date_timestamp -> price)
/// Uses batched requests with throttling to avoid rate limits
pub async fn get_historical_stock_prices_yahoo(
    tickers: &[String],
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<HashMap<String, Vec<HistoricalPrice>>> {
    if tickers.is_empty() {
        return Ok(HashMap::new());
    }

    let mut results: HashMap<String, Vec<HistoricalPrice>> = HashMap::new();

    log::info!(
        "[YAHOO HISTORICAL] Fetching historical prices for {} tickers from {} to {}",
        tickers.len(),
        start_timestamp,
        end_timestamp
    );

    // Create Yahoo connector - return empty if offline/failed
    let provider = match yahoo_connector() {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "[YAHOO HISTORICAL] Connector failed (offline?): {} - returning empty",
                e
            );
            return Ok(HashMap::new());
        }
    };

    // Convert timestamps to time::OffsetDateTime
    let start = time::OffsetDateTime::from_unix_timestamp(start_timestamp)
        .unwrap_or(time::OffsetDateTime::now_utc());
    let end = time::OffsetDateTime::from_unix_timestamp(end_timestamp)
        .unwrap_or(time::OffsetDateTime::now_utc());

    // Process in batches of 5 with 500ms delay between batches
    for (batch_idx, chunk) in tickers.chunks(5).enumerate() {
        if batch_idx > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }

        for ticker in chunk {
            match provider.get_quote_history(ticker, start, end).await {
                Ok(response) => {
                    // The closes are in the unit the response reports (pence become pounds)
                    match history_points(&response, ticker) {
                        Ok(prices) => {
                            if !prices.is_empty() {
                                log::debug!(
                                    "[YAHOO HISTORICAL] {} - got {} historical prices",
                                    ticker,
                                    prices.len()
                                );
                                results.insert(ticker.clone(), prices);
                            }
                        }
                        Err(e) => {
                            log::debug!("[YAHOO HISTORICAL] {} - no quotes: {}", ticker, e);
                        }
                    }
                }
                Err(e) => {
                    log::debug!("[YAHOO HISTORICAL] {} - error: {}", ticker, e);
                }
            }
        }
    }

    log::info!(
        "[YAHOO HISTORICAL] Fetched historical data for {} tickers",
        results.len()
    );

    Ok(results)
}

/// Get historical crypto prices from CoinGecko for a specific date
/// CoinGecko's /history endpoint returns price for a specific date
pub async fn get_historical_crypto_price_coingecko(
    api_key: Option<&str>,
    coingecko_id: &str,
    date: &str, // Format: "dd-mm-yyyy"
) -> Result<Option<f64>> {
    let url = format!(
        "https://api.coingecko.com/api/v3/coins/{}/history?date={}&localization=false",
        coingecko_id, date
    );

    let client = crate::services::http::client();
    let mut request = client.get(&url);

    if let Some(key) = api_key {
        if !key.is_empty() {
            request = request.header("x-cg-demo-api-key", key);
        }
    }

    let response = request.send().await.map_err(|e| {
        AppError::ExternalApi(format!("CoinGecko historical request failed: {}", e))
    })?;

    if !response.status().is_success() {
        return Ok(None);
    }

    #[derive(Deserialize)]
    struct MarketData {
        current_price: Option<HashMap<String, f64>>,
    }

    #[derive(Deserialize)]
    struct HistoryResponse {
        market_data: Option<MarketData>,
    }

    let data: HistoryResponse = response
        .json()
        .await
        .map_err(|e| AppError::ExternalApi(format!("CoinGecko historical parse error: {}", e)))?;

    if let Some(market_data) = data.market_data {
        if let Some(prices) = market_data.current_price {
            return Ok(prices.get("usd").copied());
        }
    }

    Ok(None)
}

/// Get historical crypto prices for multiple coins over a date range
/// Uses CoinGecko's market_chart/range endpoint which is more efficient
pub async fn get_historical_crypto_prices_coingecko(
    api_key: Option<&str>,
    id_to_ticker: &HashMap<String, String>,
    start_timestamp: i64,
    end_timestamp: i64,
) -> Result<HashMap<String, Vec<HistoricalPrice>>> {
    if id_to_ticker.is_empty() {
        return Ok(HashMap::new());
    }

    let mut results: HashMap<String, Vec<HistoricalPrice>> = HashMap::new();

    log::info!(
        "[COINGECKO HISTORICAL] Fetching historical prices for {} cryptos",
        id_to_ticker.len()
    );
    log::info!(
        "[COINGECKO HISTORICAL] API key present: {}",
        api_key.map(|k| !k.is_empty()).unwrap_or(false)
    );

    let client = crate::services::http::client();

    // Process each crypto with a delay to stay within CoinGecko free tier (~40 req/min)
    for (idx, (coingecko_id, ticker)) in id_to_ticker.iter().enumerate() {
        if idx > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;
        }

        let url = format!(
            "https://api.coingecko.com/api/v3/coins/{}/market_chart/range?vs_currency=usd&from={}&to={}",
            coingecko_id, start_timestamp, end_timestamp
        );

        let mut request = client.get(&url);

        if let Some(key) = api_key {
            if !key.is_empty() {
                request = request.header("x-cg-demo-api-key", key);
            }
        }

        match request.send().await {
            Ok(response) => {
                if response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS {
                    log::debug!(
                        "[COINGECKO HISTORICAL] Rate limit hit at {} ({}), stopping early with {} results",
                        ticker,
                        coingecko_id,
                        results.len()
                    );
                    break;
                }
                if response.status().is_success() {
                    #[derive(Deserialize)]
                    struct RangeResponse {
                        prices: Option<Vec<(f64, f64)>>, // [[timestamp_ms, price], ...]
                    }

                    if let Ok(data) = response.json::<RangeResponse>().await {
                        if let Some(prices) = data.prices {
                            // Convert to daily prices (CoinGecko returns hourly for short ranges)
                            // Group by day and take the last price for each day
                            let mut daily_prices: HashMap<i64, f64> = HashMap::new();
                            for (ts_ms, price) in prices {
                                let ts = (ts_ms / 1000.0) as i64;
                                // Round to start of day (UTC)
                                let day_start = (ts / 86400) * 86400;
                                daily_prices.insert(day_start, price);
                            }

                            let historical: Vec<HistoricalPrice> = daily_prices
                                .into_iter()
                                .map(|(ts, price)| HistoricalPrice {
                                    timestamp: ts,
                                    price,
                                    currency: "USD".to_string(),
                                })
                                .collect();

                            if !historical.is_empty() {
                                log::debug!(
                                    "[COINGECKO HISTORICAL] {} - got {} daily prices",
                                    ticker,
                                    historical.len()
                                );
                                results.insert(ticker.clone(), historical);
                            }
                        }
                    }
                } else {
                    // Log the actual error response body
                    let status = response.status();
                    let error_body = response.text().await.unwrap_or_default();
                    log::debug!(
                        "[COINGECKO HISTORICAL] {} - HTTP error: {} - Body: {}",
                        ticker,
                        status,
                        error_body
                    );
                }
            }
            Err(e) => {
                log::debug!("[COINGECKO HISTORICAL] {} - error: {}", ticker, e);
            }
        }
    }

    log::info!(
        "[COINGECKO HISTORICAL] Fetched historical data for {} cryptos",
        results.len()
    );

    Ok(results)
}

// ============================================================================
// API Key Storage
// ============================================================================

/// Stored API keys. Only CoinGecko takes one (optional, demo tier); Yahoo Finance
/// needs none. The Finnhub and Marketstack keys were removed in migration 011.
#[derive(Debug, Serialize, Deserialize, Default)]
pub struct ApiKeys {
    pub coingecko: Option<String>,
}

/// Get API keys from app_config table
pub fn get_api_keys(db: &Database) -> Result<ApiKeys> {
    db.with_conn(|conn| {
        let coingecko: Option<String> = conn
            .query_row(
                "SELECT value FROM app_config WHERE key = 'api_key_coingecko'",
                [],
                |row| row.get(0),
            )
            .ok();

        Ok(ApiKeys { coingecko })
    })
}

/// Save API keys to app_config table
pub fn set_api_keys(db: &Database, keys: &ApiKeys) -> Result<()> {
    db.with_conn(|conn| {
        if let Some(ref key) = keys.coingecko {
            conn.execute(
                "INSERT INTO app_config (key, value) VALUES ('api_key_coingecko', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = ?1",
                [key],
            )?;
        }

        Ok(())
    })
}

// ============================================================================
// Stock Metadata Refresh (Yahoo Finance quoteSummary)
// ============================================================================

const METADATA_CACHE_HOURS: i64 = 24; // Company metadata changes slowly

/// Yahoo's trailing dividend fields mix currencies for ADRs (e.g. Toyota:
/// the ¥95 Tokyo dividend divided by the USD ADR price → a nonsense "49.7 %"
/// yield). Prefer the forward dividend_yield field; fall back to the trailing
/// one only when it is plausible; drop anything above 25 % as garbage.
fn sane_dividend_yield(dividend_yield: Option<f64>, trailing_yield: Option<f64>) -> Option<f64> {
    const MAX_PLAUSIBLE_YIELD_FRACTION: f64 = 0.25;
    let plausible = |y: &f64| y.is_finite() && *y > 0.0 && *y <= MAX_PLAUSIBLE_YIELD_FRACTION;
    dividend_yield
        .filter(plausible)
        .or_else(|| trailing_yield.filter(plausible))
}

/// Format an Option<f64> to a decimal string, dropping non-finite values
fn fmt_f64(v: Option<f64>, decimals: usize) -> Option<String> {
    v.filter(|x| x.is_finite())
        .map(|x| format!("{:.*}", decimals, x))
}

/// Refresh company metadata (name, market cap, P/E, 52-week range, dividend
/// yield, sector, exchange) into stock_data. UPDATE-only: rows are created by
/// the price refresh; a missing row is skipped and retried next time.
/// Returns the number of tickers updated. Never fails on per-ticker errors;
/// those are counted and logged at warn (tickers and Yahoo's error text only
/// at debug, rust-backend rule 8).
pub async fn refresh_stock_metadata_yahoo(
    db: &Database,
    tickers: Vec<String>,
    force_refresh: bool,
) -> Result<usize> {
    if tickers.is_empty() {
        return Ok(0);
    }
    let now = unix_timestamp_now();
    let mut provider = yahoo_connector()
        .map_err(|e| AppError::ExternalApi(format!("Yahoo connector failed: {}", e)))?;

    let mut updated = 0usize;
    let mut attempted = 0usize;
    let mut failed = 0usize;
    for (batch_idx, chunk) in tickers.chunks(5).enumerate() {
        if batch_idx > 0 {
            tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        }
        for ticker in chunk {
            let ticker_upper = ticker.to_uppercase();

            let should_fetch = if force_refresh {
                true
            } else {
                db.with_conn(|conn| {
                    let last: Option<Option<i64>> = conn
                        .query_row(
                            "SELECT metadata_fetched_at FROM stock_data WHERE ticker = ?1",
                            [&ticker_upper],
                            |row| row.get(0),
                        )
                        .ok();
                    Ok(match last {
                        // Row exists, never fetched metadata → fetch
                        Some(None) => true,
                        // Row exists with timestamp → TTL check
                        Some(Some(at)) => now - at >= METADATA_CACHE_HOURS * 3600,
                        // No stock_data row yet → price refresh must succeed first;
                        // fetching metadata now would UPDATE 0 rows (wasted call)
                        None => false,
                    })
                })?
            };
            if !should_fetch {
                continue;
            }
            attempted += 1;

            let summary = match provider.get_ticker_info(&ticker_upper).await {
                Ok(s) => s,
                Err(e) => {
                    failed += 1;
                    log::debug!("[YAHOO METADATA] {} - error: {}", ticker_upper, e);
                    continue;
                }
            };
            let Some(data) = summary
                .quote_summary
                .and_then(|qs| qs.result)
                .and_then(|mut v| {
                    if v.is_empty() {
                        None
                    } else {
                        Some(v.remove(0))
                    }
                })
            else {
                failed += 1;
                log::debug!("[YAHOO METADATA] {} - empty quoteSummary", ticker_upper);
                // Yahoo answered and has nothing for this ticker: count the attempt as a fetch so
                // the ticker is not sent again within the TTL. A connection error (above) is not
                // counted, so going back online fetches at once.
                db.with_conn(|conn| {
                    crate::services::company_info::mark_metadata_checked(conn, &ticker_upper, now)
                })?;
                continue;
            };

            let sd = data.summary_detail;
            let qt = data.quote_type;
            let ap = data.asset_profile;
            let ks = data.default_key_statistics;

            let short_name = qt.as_ref().and_then(|q| q.short_name.clone());
            let long_name = qt.as_ref().and_then(|q| q.long_name.clone());
            let exchange = qt.as_ref().and_then(|q| q.exchange.clone());
            let sector = ap.as_ref().and_then(|a| a.sector.clone());
            let industry = ap.as_ref().and_then(|a| a.industry.clone());
            let description = ap.as_ref().and_then(|a| a.long_business_summary.clone());
            let pe_ratio = fmt_f64(sd.as_ref().and_then(|s| s.trailing_pe), 2);
            let forward_pe = fmt_f64(
                sd.as_ref()
                    .and_then(|s| s.forward_pe)
                    .or_else(|| ks.as_ref().and_then(|k| k.forward_pe)),
                2,
            );
            let market_cap = sd
                .as_ref()
                .and_then(|s| s.market_cap)
                .map(|v| v.to_string());
            let beta = fmt_f64(sd.as_ref().and_then(|s| s.beta), 3);
            // The 52-week range is quoted in the unit of the price (pence for BARC.L), so it is
            // stored in the same unit as the price. The dividend rate and the market cap below
            // are in the currency itself (verified live: BARC.L 0.12 pounds a year, 52 weeks
            // 353-538 pence).
            let unit = quote_unit(
                sd.as_ref().and_then(|s| s.currency.as_deref()),
                &ticker_upper,
            );
            let high_52w = fmt_price(&unit, sd.as_ref().and_then(|s| s.fifty_two_week_high));
            let low_52w = fmt_price(&unit, sd.as_ref().and_then(|s| s.fifty_two_week_low));
            // Forward dividend fields are currency-consistent; trailing ones are
            // not for ADRs (see sane_dividend_yield). Rate follows the same rule.
            let div_rate = fmt_f64(
                sd.as_ref()
                    .and_then(|s| s.dividend_rate.or(s.trailing_annual_dividend_rate)),
                2,
            );
            // Stored as a raw fraction (e.g. "0.033100" = 3.31 %); UI multiplies by 100
            let div_yield = fmt_f64(
                sane_dividend_yield(
                    sd.as_ref().and_then(|s| s.dividend_yield),
                    sd.as_ref().and_then(|s| s.trailing_annual_dividend_yield),
                ),
                6,
            );
            let ex_div = sd.as_ref().and_then(|s| s.ex_dividend_date);
            let quote_type = qt.as_ref().and_then(|q| q.quote_type.clone());

            // COALESCE preserves last-known metadata when a partial Yahoo response omits a module (values only clear on a future non-NULL fetch).
            let changed = db.with_conn(|conn| {
                let n = conn.execute(
                    "UPDATE stock_data SET
                       short_name = COALESCE(?2, short_name), long_name = COALESCE(?3, long_name),
                       sector = COALESCE(?4, sector), industry = COALESCE(?5, industry),
                       pe_ratio = COALESCE(?6, pe_ratio), forward_pe = COALESCE(?7, forward_pe),
                       market_cap = COALESCE(?8, market_cap), beta = COALESCE(?9, beta),
                       fifty_two_week_high = COALESCE(?10, fifty_two_week_high),
                       fifty_two_week_low = COALESCE(?11, fifty_two_week_low),
                       trailing_dividend_rate = COALESCE(?12, trailing_dividend_rate),
                       trailing_dividend_yield = COALESCE(?13, trailing_dividend_yield),
                       ex_dividend_date = COALESCE(?14, ex_dividend_date),
                       description = COALESCE(?15, description), exchange = COALESCE(?16, exchange),
                       quote_type = COALESCE(?17, quote_type), metadata_fetched_at = ?18
                     WHERE ticker = ?1",
                    rusqlite::params![
                        &ticker_upper,
                        short_name,
                        long_name,
                        sector,
                        industry,
                        pe_ratio,
                        forward_pe,
                        market_cap,
                        beta,
                        high_52w,
                        low_52w,
                        div_rate,
                        div_yield,
                        ex_div,
                        description,
                        exchange,
                        quote_type,
                        now,
                    ],
                )?;
                Ok(n)
            })?;
            if changed > 0 {
                updated += 1;
                log::debug!("[YAHOO METADATA] Updated metadata for {}", ticker_upper);
            } else {
                // Reachable only via force_refresh on a ticker with no stock_data row yet
                // (the normal staleness check above already skips that case).
                log::debug!(
                    "[YAHOO METADATA] {} has no stock_data row yet; skipped",
                    ticker_upper
                );
            }
        }
    }
    if failed > 0 {
        log::warn!(
            "[YAHOO METADATA] Could not fetch company data for {} of {} tickers",
            failed,
            attempted
        );
    }
    Ok(updated)
}

// ============================================================================
// Quote Currency Lookup (stock import)
// ============================================================================

/// The currency Yahoo reports for a ticker's quote, as it came (`GBp` for pence, `USD` for a
/// dollar ETF listed in London); `None` when the response names none. One chart request; the
/// stock import asks it for the listing it chose, to show that listing in its real currency.
/// A failed request is an error here and the caller's to interpret (the suffix guess stays).
pub async fn get_quote_currency(ticker: &str) -> Result<Option<String>> {
    log::debug!("[QUOTE CURRENCY] Looking up {}", ticker);
    let provider = yahoo_connector()
        .map_err(|e| AppError::ExternalApi(format!("Yahoo connector failed: {}", e)))?;
    let response = provider
        .get_quote_range(ticker.trim(), "1d", "1d")
        .await
        .map_err(|e| AppError::ExternalApi(format!("Yahoo Finance quote failed: {}", e)))?;
    Ok(reported_currency(&response))
}

// ============================================================================
// Chart Range Fetching (Stock Monitor detail chart, spec D9)
// ============================================================================

/// Map a Stock Monitor chart period to Yahoo (range, interval)
fn chart_period_params(period: &str) -> Result<(&'static str, &'static str)> {
    Ok(match period {
        "1D" => ("1d", "5m"),
        "5D" => ("5d", "30m"),
        "1M" => ("1mo", "1d"),
        "6M" => ("6mo", "1d"),
        "YTD" => ("ytd", "1d"),
        "1Y" => ("1y", "1d"),
        "5Y" => ("5y", "1wk"),
        "MAX" => ("max", "1mo"),
        _ => {
            return Err(AppError::Validation(
                "validation.invalidChartPeriod".to_string(),
            ))
        }
    })
}

/// Price series for one ticker over a named period. Intraday intervals for
/// 1D/5D, daily and coarser beyond. Returns empty when offline (graceful
/// degradation, same convention as get_historical_stock_prices_yahoo).
pub async fn get_stock_price_range(ticker: &str, period: &str) -> Result<Vec<StockPricePoint>> {
    let (range, interval) = chart_period_params(period)?;
    let ticker_upper = ticker.trim().to_uppercase();

    let provider = match yahoo_connector() {
        Ok(p) => p,
        Err(e) => {
            log::warn!(
                "[YAHOO RANGE] Connector failed (offline?): {} - returning empty",
                e
            );
            return Ok(vec![]);
        }
    };

    let response = match provider
        .get_quote_range(&ticker_upper, interval, range)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            log::debug!(
                "[YAHOO RANGE] {} {}/{} - error: {}",
                ticker_upper,
                range,
                interval,
                e
            );
            return Ok(vec![]);
        }
    };

    // The chart is in the unit the response reports (pence become pounds), as the stored quote is.
    let points = match range_points(&response, &ticker_upper) {
        Ok(points) => points,
        Err(e) => {
            log::debug!("[YAHOO RANGE] {} - no quotes: {}", ticker_upper, e);
            vec![]
        }
    };
    log::debug!(
        "[YAHOO RANGE] {} {}/{} - {} points",
        ticker_upper,
        range,
        interval,
        points.len()
    );
    Ok(points)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_price_parses_integer_and_float_prices() {
        let prices =
            parse_simple_price_usd(r#"{"bitcoin":{"usd":83523},"ethereum":{"usd":2500.5}}"#)
                .expect("parse");
        assert_eq!(prices.get("bitcoin"), Some(&83523.0));
        assert_eq!(prices.get("ethereum"), Some(&2500.5));
    }

    #[test]
    fn simple_price_skips_null_or_missing_usd() {
        let prices =
            parse_simple_price_usd(r#"{"bitcoin":{"usd":1.0},"dead":{"usd":null},"empty":{}}"#)
                .expect("parse");
        assert_eq!(prices.len(), 1);
        assert_eq!(prices.get("bitcoin"), Some(&1.0));
    }

    #[test]
    fn simple_price_parse_error_includes_body() {
        let err = parse_simple_price_usd("<html>oops</html>").unwrap_err();
        assert!(err.to_string().contains("<html>oops</html>"));
    }

    #[test]
    fn coingecko_error_message_extracts_rate_limit_detail() {
        let body =
            r#"{"status":{"error_code":429,"error_message":"You've exceeded the Rate Limit."}}"#;
        let msg = coingecko_error_message(reqwest::StatusCode::TOO_MANY_REQUESTS, body);
        assert!(msg.contains("rate limit"));
        assert!(msg.contains("You've exceeded the Rate Limit."));
    }

    #[test]
    fn coingecko_error_message_falls_back_to_raw_body() {
        let msg = coingecko_error_message(reqwest::StatusCode::UNAUTHORIZED, "bad key");
        assert_eq!(msg, "CoinGecko returned HTTP 401: bad key");
    }

    #[test]
    fn dividend_yield_prefers_forward_field() {
        // AAPL-style: both sane — forward wins
        assert_eq!(
            sane_dividend_yield(Some(0.0035), Some(0.0034)),
            Some(0.0035)
        );
    }

    #[test]
    fn dividend_yield_discards_adr_garbage() {
        // TM regression: Yahoo computes trailing yield from the ¥95 Tokyo
        // dividend against the USD ADR price (95/191.11 = 0.4971 = "49.7 %").
        // Forward field is sane (3.31 %) and must win; a garbage-only trailing
        // value must be dropped entirely rather than displayed.
        assert_eq!(
            sane_dividend_yield(Some(0.0331), Some(0.4971)),
            Some(0.0331)
        );
        assert_eq!(sane_dividend_yield(None, Some(0.4971)), None);
    }

    #[test]
    fn dividend_yield_falls_back_to_sane_trailing() {
        assert_eq!(sane_dividend_yield(None, Some(0.028)), Some(0.028));
    }

    #[test]
    fn dividend_yield_rejects_non_positive_and_non_finite() {
        assert_eq!(sane_dividend_yield(Some(0.0), None), None);
        assert_eq!(sane_dividend_yield(Some(-0.01), None), None);
        assert_eq!(sane_dividend_yield(Some(f64::NAN), None), None);
        assert_eq!(sane_dividend_yield(None, None), None);
    }

    // ---- Yahoo chart responses ---------------------------------------------------------
    //
    // Shaped like live answers (2026-10-04): BARC.L quotes in "GBp" (pence), CSPX.L in "USD"
    // although it is a London listing, VWRL.L in "GBP".

    use serde_json::json;

    /// A chart response with one day per close, the unit in `meta.currency` (when given),
    /// `extra_meta` merged into the metadata and a dividend event per amount.
    fn chart(
        currency: Option<&str>,
        closes: &[f64],
        dividends: &[f64],
        extra_meta: serde_json::Value,
    ) -> YResponse {
        let period = |start: u32, end: u32| json!({ "timezone": "BST", "start": start, "end": end, "gmtoffset": 3600 });
        let mut meta = json!({
            "symbol": "BARC.L", "instrumentType": "EQUITY", "exchangeName": "LSE",
            "fullExchangeName": "LSE", "gmtoffset": 3600, "timezone": "BST",
            "exchangeTimezoneName": "Europe/London", "hasPrePostMarketData": false,
            "priceHint": 2, "dataGranularity": "1d", "range": "1d", "validRanges": ["1d", "5d"],
            "currentTradingPeriod": {
                "pre": period(1, 2), "regular": period(2, 3), "post": period(3, 4)
            },
        });
        if let Some(currency) = currency {
            meta["currency"] = json!(currency);
        }
        for (key, value) in extra_meta.as_object().into_iter().flatten() {
            meta[key] = value.clone();
        }
        let timestamps: Vec<i64> = (0..closes.len() as i64)
            .map(|i| 1_790_000_000 + i * 86_400)
            .collect();
        let mut block = json!({
            "meta": meta,
            "timestamp": timestamps,
            "indicators": { "quote": [{
                "open": closes, "high": closes, "low": closes, "close": closes,
                "volume": vec![1_000_u64; closes.len()],
            }] },
        });
        if !dividends.is_empty() {
            let events: serde_json::Map<String, serde_json::Value> = dividends
                .iter()
                .enumerate()
                .map(|(i, amount)| {
                    let date = 1_780_000_000 + i as i64 * 7_776_000;
                    (date.to_string(), json!({ "amount": amount, "date": date }))
                })
                .collect();
            block["events"] = json!({ "dividends": events });
        }
        YResponse::from_json(json!({ "chart": { "result": [block], "error": null } }))
            .expect("a valid chart response")
    }

    fn unit(reported: &str, ticker: &str) -> QuoteUnit {
        quote_unit(Some(reported), ticker)
    }

    #[test]
    fn a_response_is_in_the_unit_its_metadata_reports() {
        let pence = chart(Some("GBp"), &[443.65], &[], json!({}));
        assert_eq!(unit_of(&pence, "BARC.L"), unit("GBp", "BARC.L"));
        assert_eq!(unit_of(&pence, "BARC.L").currency, "GBP");

        // A London listing of a dollar ETF: the suffix says GBP, the response says USD.
        let dollars = chart(Some("USD"), &[832.99], &[], json!({}));
        assert_eq!(unit_of(&dollars, "CSPX.L").currency, "USD");
        assert_eq!(unit_of(&dollars, "CSPX.L").scale, 1.0);
    }

    #[test]
    fn a_response_without_a_currency_falls_back_to_the_suffix() {
        let nameless = chart(None, &[100.0], &[], json!({}));
        let fallback = unit_of(&nameless, "VUSA.L");
        assert_eq!((fallback.currency.as_str(), fallback.scale), ("GBP", 1.0));
        assert_eq!(unit_of(&nameless, "AAPL").currency, "USD");
    }

    #[test]
    fn the_latest_quote_is_read_in_the_quotes_own_unit() {
        let response = chart(
            Some("GBp"),
            &[440.0],
            &[],
            json!({ "regularMarketPrice": 443.65, "chartPreviousClose": 438.95 }),
        );
        assert_eq!(
            read_chart_quote(&response),
            Some(ChartQuote {
                price: 443.65,
                previous_close: Some(438.95),
                currency: Some("GBp".to_string()),
            })
        );
    }

    #[test]
    fn the_previous_close_of_the_metadata_wins_over_the_chart_one() {
        let response = chart(
            Some("USD"),
            &[830.0],
            &[],
            json!({
                "regularMarketPrice": 832.99, "previousClose": 822.67, "chartPreviousClose": 800.0
            }),
        );
        let quote = read_chart_quote(&response).expect("a quote");
        assert_eq!(quote.previous_close, Some(822.67));
    }

    #[test]
    fn the_last_close_stands_in_for_a_missing_market_price() {
        let response = chart(Some("USD"), &[100.0, 101.5], &[], json!({}));
        let quote = read_chart_quote(&response).expect("a quote");
        assert_eq!(quote.price, 101.5);
        assert_eq!(quote.previous_close, None);
    }

    #[test]
    fn a_response_without_a_positive_price_is_no_quote() {
        let zero = chart(
            Some("USD"),
            &[0.0],
            &[],
            json!({ "regularMarketPrice": 0.0 }),
        );
        assert_eq!(read_chart_quote(&zero), None);
    }

    #[test]
    fn a_response_that_names_no_currency_reports_none() {
        let nameless = chart(None, &[100.0], &[], json!({}));
        assert_eq!(reported_currency(&nameless), None);
        let pence = chart(Some("GBp"), &[100.0], &[], json!({}));
        assert_eq!(reported_currency(&pence).as_deref(), Some("GBp"));
    }

    #[test]
    fn the_reported_currency_is_trimmed_and_a_blank_one_is_none() {
        let padded = chart(
            Some(" USD "),
            &[1.0],
            &[],
            json!({ "regularMarketPrice": 1.0 }),
        );
        assert_eq!(
            read_chart_quote(&padded)
                .and_then(|q| q.currency)
                .as_deref(),
            Some("USD")
        );
        let blank = chart(
            Some("  "),
            &[1.0],
            &[],
            json!({ "regularMarketPrice": 1.0 }),
        );
        assert_eq!(read_chart_quote(&blank).and_then(|q| q.currency), None);
    }

    #[test]
    fn history_in_pence_is_stored_in_pounds() {
        let response = chart(Some("GBp"), &[443.65, 450.0], &[], json!({}));
        let points = history_points(&response, "BARC.L").expect("points");
        let seen: Vec<(f64, &str)> = points
            .iter()
            .map(|p| (p.price, p.currency.as_str()))
            .collect();
        assert_eq!(seen, vec![(4.4365, "GBP"), (4.5, "GBP")]);
        assert_eq!(points[0].timestamp, 1_790_000_000);
        assert_eq!(points[1].timestamp, 1_790_086_400);
    }

    #[test]
    fn history_of_a_dollar_etf_listed_in_london_is_in_dollars() {
        let response = chart(Some("USD"), &[832.99, 830.5], &[], json!({}));
        let points = history_points(&response, "CSPX.L").expect("points");
        let seen: Vec<(f64, &str)> = points
            .iter()
            .map(|p| (p.price, p.currency.as_str()))
            .collect();
        assert_eq!(seen, vec![(832.99, "USD"), (830.5, "USD")]);
    }

    #[test]
    fn history_in_pounds_is_not_scaled() {
        // `GBP` and `GBp` differ by the case of one letter; pounds stay pounds.
        let response = chart(Some("GBP"), &[74.12], &[], json!({}));
        let points = history_points(&response, "VWRL.L").expect("points");
        assert_eq!(
            (points[0].price, points[0].currency.as_str()),
            (74.12, "GBP")
        );
    }

    #[test]
    fn history_without_a_currency_uses_the_suffix_at_scale_one() {
        let response = chart(None, &[100.0], &[], json!({}));
        let points = history_points(&response, "SXR8.DE").expect("points");
        assert_eq!(
            (points[0].price, points[0].currency.as_str()),
            (100.0, "EUR")
        );
    }

    #[test]
    fn history_without_quotes_is_an_error_not_an_empty_series() {
        let response = chart(Some("GBp"), &[], &[], json!({}));
        assert!(history_points(&response, "BARC.L").is_err());
    }

    #[test]
    fn the_range_chart_drops_empty_closes_and_scales_the_rest() {
        let response = chart(Some("GBp"), &[0.0, 443.65, 104.0], &[], json!({}));
        let points = range_points(&response, "BARC.L").expect("points");
        let seen: Vec<(f64, &str)> = points
            .iter()
            .map(|p| (p.price, p.currency.as_str()))
            .collect();
        assert_eq!(seen, vec![(4.4365, "GBP"), (1.04, "GBP")]);
    }

    #[test]
    fn the_range_chart_of_a_dollar_etf_in_london_is_in_dollars() {
        let response = chart(Some("USD"), &[832.99], &[], json!({}));
        let points = range_points(&response, "CSPX.L").expect("points");
        assert_eq!(
            (points[0].price, points[0].currency.as_str()),
            (832.99, "USD")
        );
    }

    #[test]
    fn dividends_in_pence_are_summed_in_pounds() {
        // BARC.L paid 5.6p and 5.9p: 11.5p, i.e. 0.115 pounds.
        let response = chart(Some("GBp"), &[443.65], &[5.6000004, 5.9], json!({}));
        let sum = dividend_sum(&response, &unit_of(&response, "BARC.L")).expect("dividends");
        assert!((sum - 0.115).abs() < 1e-6, "{sum}");
    }

    #[test]
    fn dividends_in_the_quote_currency_are_summed_as_they_are() {
        let response = chart(
            Some("GBP"),
            &[74.12],
            &[0.407582, 0.342574, 0.683298, 0.406941],
            json!({}),
        );
        let sum = dividend_sum(&response, &unit_of(&response, "VWRL.L")).expect("dividends");
        assert!((sum - 1.840395).abs() < 1e-9, "{sum}");
    }

    #[test]
    fn a_year_without_dividends_sums_to_a_positive_zero() {
        for (currency, ticker) in [("USD", "CSPX.L"), ("GBp", "BARC.L")] {
            let response = chart(Some(currency), &[832.99], &[], json!({}));
            let sum = dividend_sum(&response, &unit_of(&response, ticker)).expect("a sum");
            // Not -0.0: that is stored as "-0.00".
            assert_eq!(sum.to_bits(), 0.0_f64.to_bits(), "{ticker}");
        }
    }

    #[test]
    fn a_response_without_a_result_has_no_dividend_sum() {
        let empty = YResponse::from_json(json!({ "chart": { "result": null, "error": null } }))
            .expect("a response without a result");
        assert_eq!(dividend_sum(&empty, &unit("USD", "CSPX.L")), None);
    }

    // ---- storing a quote ---------------------------------------------------------------

    fn quotes_db() -> rusqlite::Connection {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_data (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL UNIQUE, original_price TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'USD', price_date INTEGER NOT NULL,
                fetched_at INTEGER NOT NULL DEFAULT 0, previous_close TEXT, quote_currency TEXT,
                fifty_two_week_high TEXT, fifty_two_week_low TEXT, metadata_fetched_at INTEGER
            );
            CREATE TABLE dividend_data (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL UNIQUE, yearly_dividend_sum TEXT NOT NULL,
                currency TEXT NOT NULL, last_fetched_at INTEGER NOT NULL
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn quote(price: f64, previous_close: Option<f64>, currency: Option<&str>) -> ChartQuote {
        ChartQuote {
            price,
            previous_close,
            currency: currency.map(str::to_string),
        }
    }

    /// `(price, currency, previous close, quote currency)` of a ticker's stored row.
    type Row = (String, String, Option<String>, Option<String>);

    fn row(conn: &rusqlite::Connection, ticker: &str) -> Row {
        conn.query_row(
            "SELECT original_price, currency, previous_close, quote_currency
             FROM stock_data WHERE ticker = ?1",
            [ticker],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .expect("a stored quote")
    }

    fn store(conn: &rusqlite::Connection, ticker: &str, quote: &ChartQuote) -> StoredQuote {
        store_stock_quote(conn, ticker, quote, 1_790_000_000).expect("stored")
    }

    fn text(value: &str) -> Option<String> {
        Some(value.to_string())
    }

    #[test]
    fn a_pence_quote_is_stored_in_pounds_beside_the_raw_code() {
        let conn = quotes_db();
        let stored = store(&conn, "BARC.L", &quote(443.65, Some(438.95), Some("GBp")));

        assert_eq!(
            stored,
            StoredQuote {
                price: 4.4365,
                currency: "GBP".to_string(),
                unit_changed: true
            }
        );
        assert_eq!(
            row(&conn, "BARC.L"),
            (
                "4.4365".to_string(),
                "GBP".to_string(),
                text("4.3895"),
                text("GBp")
            )
        );
    }

    #[test]
    fn a_dollar_etf_listed_in_london_is_stored_in_dollars() {
        let conn = quotes_db();
        let stored = store(&conn, "CSPX.L", &quote(832.99, Some(822.67), Some("USD")));

        assert_eq!((stored.price, stored.currency.as_str()), (832.99, "USD"));
        assert!(stored.unit_changed, "earlier versions stored it as GBP");
        assert_eq!(
            row(&conn, "CSPX.L"),
            (
                "832.99".to_string(),
                "USD".to_string(),
                text("822.67"),
                text("USD")
            )
        );
    }

    #[test]
    fn a_quote_the_suffix_guessed_right_needs_no_repair() {
        let conn = quotes_db();
        for (ticker, code) in [
            ("VUSA.L", "GBP"),
            ("SXR8.DE", "EUR"),
            ("CEZ.PR", "CZK"),
            ("AAPL", "USD"),
        ] {
            let stored = store(&conn, ticker, &quote(100.0, Some(99.0), Some(code)));
            assert!(!stored.unit_changed, "{ticker}");
            assert_eq!(row(&conn, ticker).3.as_deref(), Some(code), "{ticker}");
        }
    }

    #[test]
    fn an_unchanged_unit_is_reported_once() {
        let conn = quotes_db();
        assert!(store(&conn, "BARC.L", &quote(443.65, None, Some("GBp"))).unit_changed);
        assert!(!store(&conn, "BARC.L", &quote(450.0, None, Some("GBp"))).unit_changed);
        // Yahoo naming the same unit another way is no change either.
        assert!(!store(&conn, "BARC.L", &quote(451.0, None, Some("GBX"))).unit_changed);
        assert_eq!(row(&conn, "BARC.L").3.as_deref(), Some("GBX"));
        assert_eq!(row(&conn, "BARC.L").0, "4.51");
    }

    #[test]
    fn a_ticker_stored_before_the_code_was_recorded_is_compared_with_the_suffix_guess() {
        let conn = quotes_db();
        // A row as the earlier version wrote it: the guess, no code.
        conn.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency, price_date)
             VALUES ('a', 'CSPX.L', '832.99', 'GBP', 1), ('b', 'VUSA.L', '74.12', 'GBP', 1)",
            [],
        )
        .expect("old rows");

        assert!(store(&conn, "CSPX.L", &quote(833.0, None, Some("USD"))).unit_changed);
        assert!(!store(&conn, "VUSA.L", &quote(74.5, None, Some("GBP"))).unit_changed);
    }

    #[test]
    fn a_change_of_unit_after_the_code_was_recorded_is_reported() {
        let conn = quotes_db();
        store(&conn, "CSPX.L", &quote(832.99, None, Some("USD")));
        assert!(store(&conn, "CSPX.L", &quote(650.0, None, Some("GBP"))).unit_changed);
        assert_eq!(row(&conn, "CSPX.L").1, "GBP");
    }

    #[test]
    fn a_response_without_a_currency_keeps_the_recorded_unit() {
        let conn = quotes_db();
        store(&conn, "BARC.L", &quote(443.65, Some(438.95), Some("GBp")));

        // One thin answer must not turn the pence quote into pounds.
        let stored = store(&conn, "BARC.L", &quote(450.0, Some(444.0), None));
        assert_eq!((stored.price, stored.currency.as_str()), (4.5, "GBP"));
        assert!(!stored.unit_changed);
        assert_eq!(
            row(&conn, "BARC.L"),
            (
                "4.50".to_string(),
                "GBP".to_string(),
                text("4.44"),
                text("GBp")
            )
        );
    }

    #[test]
    fn a_response_without_a_currency_for_a_ticker_never_recorded_keeps_the_old_guess() {
        let conn = quotes_db();
        let stored = store(&conn, "CSPX.L", &quote(832.99, None, None));
        assert_eq!((stored.price, stored.currency.as_str()), (832.99, "GBP"));
        assert!(!stored.unit_changed, "nothing learned, nothing to repair");
        assert_eq!(row(&conn, "CSPX.L").3, None, "no code is made up");
    }

    fn seed_old_unit_data(conn: &rusqlite::Connection) {
        conn.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency, price_date,
                                     previous_close, fifty_two_week_high, fifty_two_week_low,
                                     metadata_fetched_at)
             VALUES ('a', 'BARC.L', '443.65', 'GBP', 1, '438.95', '538.30', '353.55', 1790000000)",
            [],
        )
        .expect("a quote of the earlier version");
        conn.execute(
            "INSERT INTO dividend_data (id, ticker, yearly_dividend_sum, currency, last_fetched_at)
             VALUES ('d', 'BARC.L', '11.50', 'GBP', 1790000000)",
            [],
        )
        .expect("its dividends");
    }

    #[test]
    fn a_changed_unit_drops_what_was_stored_in_the_old_one() {
        let conn = quotes_db();
        seed_old_unit_data(&conn);

        // The response has no previous close: the old one (pence) must not survive as pounds.
        let stored = store(&conn, "BARC.L", &quote(443.65, None, Some("GBp")));
        assert!(stored.unit_changed);

        let (previous_close, high, low, metadata_at): (
            Option<String>,
            Option<String>,
            Option<String>,
            Option<i64>,
        ) = conn
            .query_row(
                "SELECT previous_close, fifty_two_week_high, fifty_two_week_low, metadata_fetched_at
                 FROM stock_data WHERE ticker = 'BARC.L'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("row");
        assert_eq!(
            (previous_close, high, low, metadata_at),
            (None, None, None, None),
            "the figures quoted in pence are gone and the company data is due again"
        );
        let dividends_fetched_at: i64 = conn
            .query_row(
                "SELECT last_fetched_at FROM dividend_data WHERE ticker = 'BARC.L'",
                [],
                |r| r.get(0),
            )
            .expect("dividends");
        assert_eq!(
            dividends_fetched_at, 0,
            "the cached dividends are due again"
        );
    }

    #[test]
    fn an_unchanged_unit_keeps_what_is_stored() {
        let conn = quotes_db();
        seed_old_unit_data(&conn);
        store(&conn, "BARC.L", &quote(443.65, Some(438.95), Some("GBp")));
        // Make the stored company data and dividends the ones of the new unit.
        conn.execute(
            "UPDATE stock_data SET fifty_two_week_high = '5.383', fifty_two_week_low = '3.5355',
                                   metadata_fetched_at = 1790000000",
            [],
        )
        .expect("company data in pounds");
        conn.execute("UPDATE dividend_data SET last_fetched_at = 1790000000", [])
            .expect("dividends in pounds");

        // The next refresh finds no previous close, as thin answers do: the stored one stays.
        let stored = store(&conn, "BARC.L", &quote(450.0, None, Some("GBp")));
        assert!(!stored.unit_changed);

        let (previous_close, high, metadata_at): (Option<String>, Option<String>, Option<i64>) =
            conn.query_row(
                "SELECT previous_close, fifty_two_week_high, metadata_fetched_at
                 FROM stock_data WHERE ticker = 'BARC.L'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("row");
        assert_eq!(previous_close.as_deref(), Some("4.3895"));
        assert_eq!(high.as_deref(), Some("5.383"));
        assert_eq!(metadata_at, Some(1_790_000_000));
        let dividends_fetched_at: i64 = conn
            .query_row(
                "SELECT last_fetched_at FROM dividend_data WHERE ticker = 'BARC.L'",
                [],
                |r| r.get(0),
            )
            .expect("dividends");
        assert_eq!(dividends_fetched_at, 1_790_000_000);
    }

    #[test]
    fn the_refresh_result_keeps_its_wire_shape() {
        let result = StockPriceRefreshResult {
            updated: vec![],
            remaining_tickers: vec![],
            rate_limit_hit: false,
            unit_changed: vec!["BARC.L".to_string()],
        };
        let json = serde_json::to_value(&result).expect("serialize");
        let mut keys: Vec<&str> = json
            .as_object()
            .expect("object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, vec!["rate_limit_hit", "remaining_tickers", "updated"]);
    }

    #[test]
    fn price_figures_are_stored_in_the_unit_of_the_price() {
        let dollars = unit("USD", "AAPL");
        assert_eq!(fmt_price(&dollars, Some(538.3)).as_deref(), Some("538.30"));
        assert_eq!(fmt_price(&dollars, None), None);
        assert_eq!(fmt_price(&dollars, Some(f64::NAN)), None);
        assert_eq!(fmt_price(&dollars, Some(f64::INFINITY)), None);

        // The 52-week range of BARC.L is quoted in pence like its price.
        let pence = unit("GBp", "BARC.L");
        assert_eq!(fmt_price(&pence, Some(538.3)).as_deref(), Some("5.383"));
        assert_eq!(fmt_price(&pence, Some(353.55)).as_deref(), Some("3.5355"));
        assert_eq!(
            fmt_price(&unit("ZAc", "NPN.JO"), Some(130635.0)).as_deref(),
            Some("1306.35")
        );
    }
}
