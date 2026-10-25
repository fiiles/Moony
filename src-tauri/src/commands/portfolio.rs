//! Portfolio metrics commands

use crate::db::Database;
use crate::error::Result;
use crate::models::{DatedFact, PortfolioHistoryResponse, PortfolioMetrics};
use crate::services::dated_facts;
use crate::services::loan_amortization::today_utc_day;
use crate::services::net_worth;
use crate::services::portfolio_history::{self, SnapshotSource};
use crate::services::positions::{self, AssetKind};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// Current net worth in `report_currency` (native breakdowns at the current quote snapshot).
#[tauri::command]
pub async fn get_portfolio_metrics(
    db: State<'_, Database>,
    exclude_personal_real_estate: bool,
    report_currency: String,
) -> Result<PortfolioMetrics> {
    db.with_conn(|conn| {
        net_worth::metrics_report(conn, &report_currency, exclude_personal_real_estate)
    })
}

/// Portfolio history in `report_currency`, oldest day first, every day at its own rates.
#[tauri::command]
pub async fn get_portfolio_history(
    db: State<'_, Database>,
    start_date: Option<i64>,
    end_date: Option<i64>,
    report_currency: String,
) -> Result<PortfolioHistoryResponse> {
    db.with_conn(|conn| net_worth::history_report(conn, &report_currency, start_date, end_date))
}

/// Dated facts (purchases, estimates, new loans, manual balances, days of trades) in the UTC days
/// `[start_date, end_date]`: what the dashboard names behind a big step of net worth. Amounts are
/// native, as entered.
#[tauri::command]
pub async fn get_dated_facts(
    db: State<'_, Database>,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<DatedFact>> {
    db.with_conn(|conn| dated_facts::dated_facts(conn, start_date, end_date))
}

/// Record today's per-ticker values to stock_value_history and crypto_value_history tables
/// This is called alongside update_todays_snapshot to ensure per-ticker chart data is available
fn record_todays_ticker_values(db: &Database) -> Result<()> {
    let now = chrono::Utc::now();
    let today_start = now
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .expect("Valid date should have valid midnight time")
        .and_utc()
        .timestamp();

    db.with_conn(|conn| {
        // Record stock values, at the quantity the positions value (one quantity rule)
        for (ticker, qty) in positions::held_quantities(conn, AssetKind::Stock)? {
            if qty <= 0.0 {
                continue;
            }

            if let Some(resolved) = crate::services::pricing::resolve_stock_price(conn, &ticker) {
                let price: f64 = resolved.original_price.parse().unwrap_or(0.0);
                if price <= 0.0 {
                    continue;
                }
                let id = Uuid::new_v4().to_string();

                conn.execute(
                    "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(ticker, recorded_at) DO UPDATE SET
                         quantity = excluded.quantity,
                         price = excluded.price,
                         currency = excluded.currency",
                    rusqlite::params![
                        id,
                        ticker,
                        today_start,
                        qty.to_string(),
                        price.to_string(),
                        resolved.currency,
                    ],
                )?;
            }
        }

        // Record crypto values
        for (ticker, qty) in positions::held_quantities(conn, AssetKind::Crypto)? {
            if qty <= 0.0 {
                continue;
            }

            if let Some(resolved) = crate::services::pricing::resolve_crypto_price(conn, &ticker) {
                let price: f64 = resolved.original_price.parse().unwrap_or(0.0);
                if price <= 0.0 {
                    continue;
                }
                let id = Uuid::new_v4().to_string();

                conn.execute(
                    "INSERT INTO crypto_value_history (id, ticker, recorded_at, quantity, price, currency)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                     ON CONFLICT(ticker, recorded_at) DO UPDATE SET
                         quantity = excluded.quantity,
                         price = excluded.price,
                         currency = excluded.currency",
                    rusqlite::params![
                        id,
                        ticker,
                        today_start,
                        qty.to_string(),
                        price.to_string(),
                        resolved.currency,
                    ],
                )?;
            }
        }

        Ok(())
    })
}

/// Update today's portfolio snapshot (or create one if it doesn't exist).
/// All column writing happens in the single writer (services/portfolio_history).
pub async fn update_todays_snapshot(db: &Database) -> Result<()> {
    let breakdowns = db.with_conn(net_worth::compute_breakdowns)?;
    let now_ts = chrono::Utc::now().timestamp();

    db.with_conn(|conn| {
        portfolio_history::upsert_snapshot(conn, now_ts, SnapshotSource::Live, &breakdowns)
    })?;

    // Also record per-ticker values for stock and crypto history charts
    record_todays_ticker_values(db)?;

    Ok(())
}

/// Record current portfolio snapshot
#[tauri::command]
pub async fn record_portfolio_snapshot(db: State<'_, Database>) -> Result<()> {
    update_todays_snapshot(&db).await
}

/// Update exchange rates from the ECB (current snapshot, stored rates and the daily history).
/// The quotes themselves are read through `get_fx_snapshot`.
#[tauri::command]
pub async fn refresh_exchange_rates(app: AppHandle, db: State<'_, Database>) -> Result<()> {
    let ecb = crate::services::currency::fetch_ecb_rates().await?;
    let rates = ecb.rates;

    // Persist latest rates for offline use
    db.with_conn(|conn| crate::services::currency::save_rates_to_db(conn, &rates))?;

    // Also persist to daily history for accurate historical chart rendering,
    // under the ECB reference day (not the fetch day)
    db.with_conn(|conn| {
        crate::services::currency::save_rates_to_history_db(conn, &rates, ecb.reference_day)
    })?;

    // Self-heal the FX timeseries in the background: fill the prefix before
    // the earliest transaction and any tail gap from days the app was closed.
    // Best-effort — a failure only means uncovered days stay without a historical
    // quote (and are reported as missing) until the next refresh.
    spawn_fx_backfill(app, db.inner().clone());

    Ok(())
}

/// Extend the FX timeseries in the background (best-effort). New rate rows
/// change historical valuations and report conversions, so it pokes the
/// `recalculation-complete` event the charts, dated converters and reports
/// listen to.
pub(crate) fn spawn_fx_backfill(app: AppHandle, db: Database) {
    tauri::async_runtime::spawn(async move {
        match crate::services::fx_backfill::ensure_fx_coverage(&db).await {
            Ok(n) if n > 0 => {
                app.emit("recalculation-complete", ()).ok();
            }
            Ok(_) => {}
            Err(e) => {
                log::warn!(
                    "[FX_BACKFILL] WARNING: backfill failed (will retry on next refresh): {e}"
                )
            }
        }
    });
}

// ============================================================================
// Price Status (for stale indicator)
// ============================================================================

/// Status of prices and exchange rates freshness
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PriceStatus {
    /// Are any stock prices considered stale (older than 24 hours)?
    pub stocks_stale: bool,
    /// Are any crypto prices considered stale (older than 12 hours)?
    pub crypto_stale: bool,
    /// Are exchange rates considered stale (older than 48 hours)?
    pub exchange_rates_stale: bool,
    /// Age of oldest stock price in hours (None if no stocks)
    pub oldest_stock_price_age_hours: Option<i64>,
    /// Age of oldest crypto price in hours (None if no crypto)
    pub oldest_crypto_price_age_hours: Option<i64>,
    /// Age of exchange rates in hours (None if never fetched)
    pub exchange_rates_age_hours: Option<i64>,
    /// Number of stocks without any price data
    pub stocks_missing_price: i32,
    /// Number of cryptos without any price data
    pub crypto_missing_price: i32,
    /// Currencies that enter converted totals (open positions, transactions, entities,
    /// budgets, the profile currency) without a current quote; amounts in them are excluded
    /// from those totals, never valued 1:1. Watch-only and closed-position quotes do not count
    pub missing_currencies: Vec<String>,
    /// Held currencies whose newest stored exchange rate is more than 14 days old;
    /// their values use that old rate
    #[serde(rename = "staleCurrencies")]
    pub stale_currencies: Vec<String>,
}

/// Staleness thresholds in hours
const STOCKS_STALE_THRESHOLD_HOURS: i64 = 24;
const CRYPTO_STALE_THRESHOLD_HOURS: i64 = 12;
const FX_STALE_THRESHOLD_HOURS: i64 = 48;

/// Current FX quotes (units per 1 unit of the reference) for frontend conversions.
#[tauri::command]
pub fn get_fx_snapshot() -> Result<crate::models::fx::FxSnapshot> {
    Ok(crate::services::fx::snapshot())
}

/// Stored daily quotes within `[start_day, end_day]` (UTC-midnight unix seconds), oldest first,
/// with the reference they are quoted against.
#[tauri::command]
pub async fn get_fx_history(
    db: State<'_, Database>,
    start_day: i64,
    end_day: i64,
) -> Result<crate::models::fx::FxHistoryQuotes> {
    db.with_conn(|conn| crate::services::fx::history_quotes(conn, start_day, end_day))
}

/// Get the status of prices and exchange rates freshness
#[tauri::command]
pub fn get_price_status(db: State<'_, Database>) -> Result<PriceStatus> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs() as i64;

    db.with_conn(|conn| {
        // Check stock prices
        let (oldest_stock_fetched_at, stocks_missing): (Option<i64>, i32) = {
            // Get oldest fetched_at from stock_data for investments we actually hold
            let oldest: Option<i64> = conn
                .query_row(
                    "SELECT MIN(sd.fetched_at) FROM stock_investments si
                 LEFT JOIN stock_data sd ON si.ticker = sd.ticker
                 WHERE sd.fetched_at IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            // Count stocks without price data
            let missing: i32 = conn
                .query_row(
                    "SELECT COUNT(*) FROM stock_investments si
                 LEFT JOIN stock_data sd ON si.ticker = sd.ticker
                 WHERE sd.ticker IS NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            (oldest, missing)
        };

        // Check crypto prices
        let (oldest_crypto_fetched_at, crypto_missing): (Option<i64>, i32) = {
            let oldest: Option<i64> = conn
                .query_row(
                    "SELECT MIN(cp.fetched_at) FROM crypto_investments ci
                 LEFT JOIN crypto_prices cp ON ci.ticker = cp.symbol
                 WHERE cp.fetched_at IS NOT NULL",
                    [],
                    |row| row.get(0),
                )
                .ok()
                .flatten();

            let missing: i32 = conn
                .query_row(
                    "SELECT COUNT(*) FROM crypto_investments ci
                 LEFT JOIN crypto_prices cp ON ci.ticker = cp.symbol
                 WHERE cp.symbol IS NULL",
                    [],
                    |row| row.get(0),
                )
                .unwrap_or(0);

            (oldest, missing)
        };

        // Get exchange rates fetched_at from memory
        let exchange_rates_fetched_at = crate::services::currency::get_exchange_rates_fetched_at();

        // Calculate ages in hours
        let oldest_stock_price_age_hours = oldest_stock_fetched_at.map(|t| (now - t) / 3600);
        let oldest_crypto_price_age_hours = oldest_crypto_fetched_at.map(|t| (now - t) / 3600);
        let exchange_rates_age_hours = exchange_rates_fetched_at.map(|t| (now - t) / 3600);

        // Determine staleness
        let stocks_stale = oldest_stock_price_age_hours
            .map(|h| h > STOCKS_STALE_THRESHOLD_HOURS)
            .unwrap_or(false)
            || stocks_missing > 0;

        let crypto_stale = oldest_crypto_price_age_hours
            .map(|h| h > CRYPTO_STALE_THRESHOLD_HOURS)
            .unwrap_or(false)
            || crypto_missing > 0;

        let exchange_rates_stale = exchange_rates_age_hours
            .map(|h| h > FX_STALE_THRESHOLD_HOURS)
            .unwrap_or(true); // Stale if never fetched

        Ok(PriceStatus {
            stocks_stale,
            crypto_stale,
            exchange_rates_stale,
            oldest_stock_price_age_hours,
            oldest_crypto_price_age_hours,
            exchange_rates_age_hours,
            stocks_missing_price: stocks_missing,
            crypto_missing_price: crypto_missing,
            missing_currencies: crate::services::fx_backfill::unquoted_currencies(
                conn,
                &crate::services::fx::current(),
            )
            .unwrap_or_else(|e| {
                log::warn!("[PRICE_STATUS] unquoted currency lookup failed: {e}");
                Vec::new()
            }),
            stale_currencies: crate::services::fx_backfill::stale_currencies(
                conn,
                (now / 86_400) * 86_400,
            )
            .unwrap_or_else(|e| {
                log::warn!("[PRICE_STATUS] stale currency lookup failed: {e}");
                Vec::new()
            }),
        })
    })
}

// ============================================================================
// History rebuilds: the queue entry point, startup planning, on-demand backfill
// ============================================================================

use crate::services::backfill;
use crate::services::history_plan;
use crate::services::history_recalc::{self, Completion, HistoryRecalc, Job, Notice};
use crate::services::price_api::HistoricalPrice;

/// Result returned by the backfill command: what was queued (the rebuild itself runs in the
/// background, see `services/history_recalc.rs`).
#[derive(Debug, Clone, serde::Serialize)]
pub struct BackfillResult {
    /// Always 0 for a queued rebuild: nothing is written before the command returns.
    pub days_processed: i32,
    /// The days the queued rebuild covers.
    pub total_days: i32,
    /// True when there was nothing to queue.
    pub completed: bool,
    pub message: String,
}

/// The price known as of `target`'s day: the latest valid price on or before
/// it, `None` before the first one. Look-back only; the rule lives in
/// `services::backfill::price_at_or_before`.
pub(crate) fn find_closest_price(
    prices: &[HistoricalPrice],
    target_timestamp: i64,
) -> Option<&HistoricalPrice> {
    backfill::price_at_or_before(prices, target_timestamp)
}

/// Queue history rebuilds and return at once (the single entry point of every write
/// command): the write has already stored its rows, the ticker rebuilds and the
/// re-derivation of the portfolio days follow on a worker that holds the database lock in
/// short chunks. Progress reaches the UI as `history-recalculation` events
/// (`{ ticker, status }`), and `recalculation-complete` fires when a batch is done. Jobs
/// dated today or later are ignored (nothing historical to repair).
pub fn schedule_history_rebuild(
    app: &AppHandle,
    db: &Database,
    recalc: &HistoryRecalc,
    jobs: Vec<Job>,
) {
    schedule_history_rebuild_then(app, db, recalc, jobs, None);
}

/// [`schedule_history_rebuild`] with a [`Completion`] that learns whether the batch carrying
/// the jobs finished without an error (it runs at once when no job is historical).
pub fn schedule_history_rebuild_then(
    app: &AppHandle,
    db: &Database,
    recalc: &HistoryRecalc,
    jobs: Vec<Job>,
    completion: Option<Completion>,
) {
    let handle = app.clone();
    let notify: history_recalc::Notifier = std::sync::Arc::new(move |notice| match notice {
        Notice::Status(event) => {
            handle.emit(history_recalc::EVENT_STATUS, event).ok();
        }
        Notice::BatchComplete => {
            handle.emit(history_recalc::EVENT_COMPLETE, ()).ok();
        }
    });
    history_recalc::schedule(recalc, db, notify, find_closest_price, jobs, completion);
}

/// What a queued plan reports back through [`BackfillResult`].
fn queued_result(plan: &history_plan::StartupPlan, today: i64, what: &str) -> BackfillResult {
    let tickers = plan
        .jobs
        .iter()
        .filter(|job| !matches!(job, Job::Days(_)))
        .count();
    let total_days = plan
        .from_day
        .map_or(0, |from| ((today - from).max(0) / 86_400) as i32);
    if plan.jobs.is_empty() {
        return BackfillResult {
            days_processed: 0,
            total_days: 0,
            completed: true,
            message: "No history to rebuild".to_string(),
        };
    }
    BackfillResult {
        days_processed: 0,
        total_days,
        completed: false,
        message: format!(
            "Queued {what}: {total_days} days and {tickers} ticker(s) in the background"
        ),
    }
}

/// Stores the rebuild version once the one-time rebuild's days were re-derived, so a failed
/// days rebuild is retried at the next start. Tickers the provider could not serve are
/// logged here (the queue reported them `failed`) and are retried by a later write or the
/// startup gap fill; they do not keep the whole rebuild running at every start.
fn store_version_when_done(db: Database) -> Completion {
    Box::new(move |outcome| {
        if !history_plan::rebuild_version_earned(outcome) {
            log::warn!("[RECALC] The one-time history rebuild did not re-derive the days; it runs again at the next start");
            return;
        }
        if !outcome.failed_tickers.is_empty() {
            log::warn!(
                "[RECALC] The one-time history rebuild is done, but {} ticker(s) could not be rebuilt and are retried by later writes",
                outcome.failed_tickers.len()
            );
        }
        match db.with_conn(history_plan::store_rebuild_version) {
            Ok(()) => log::info!(
                "[RECALC] History rebuilt from dated facts (version {})",
                history_plan::REBUILD_VERSION
            ),
            Err(e) => log::warn!("[RECALC] Could not store the history rebuild version: {e}"),
        }
    })
}

/// Plan and queue what a start needs, then return at once (A7): the first start of a version
/// rebuilds the whole history once, later starts fill the days that are missing. Called by
/// `SyncProvider` after unlock.
#[tauri::command]
pub async fn start_snapshot_backfill(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
) -> Result<BackfillResult> {
    let today = today_utc_day();
    let plan = db.with_conn(|conn| history_plan::plan_startup(conn, today))?;
    let result = queued_result(
        &plan,
        today,
        if plan.one_time {
            "the one-time rebuild of the whole history"
        } else {
            "a repair of the missing days"
        },
    );
    let completion = plan
        .one_time
        .then(|| store_version_when_done(db.inner().clone()));
    log::info!("[BACKFILL] {}", result.message);
    schedule_history_rebuild_then(&app, &db, &recalc, plan.jobs, completion);
    Ok(result)
}

/// Queue a rebuild of the whole history: the days from the first dated fact and every ticker
/// from its first transaction. Useful when stored history has been corrupted.
#[tauri::command]
pub async fn recalculate_all_portfolio_history(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
) -> Result<BackfillResult> {
    let today = today_utc_day();
    let plan = db.with_conn(history_plan::plan_everything)?;
    let result = queued_result(&plan, today, "a full recalculation");
    log::info!("[RECALC] {}", result.message);
    schedule_history_rebuild(&app, &db, &recalc, plan.jobs);
    Ok(result)
}

/// Tauri command to backfill a specific stock ticker's value history on demand
/// This is called when the user views a stock detail page to ensure the chart has data
#[tauri::command]
pub async fn backfill_stock_ticker_history(
    db: State<'_, Database>,
    ticker: String,
) -> Result<BackfillResult> {
    log::debug!(
        "[BACKFILL] On-demand backfill requested for stock ticker: {}",
        ticker
    );

    // Get the oldest transaction date for this ticker to determine backfill range
    let oldest_tx_date: Option<i64> = db.with_conn(|conn| {
        let result: Option<i64> = conn
            .query_row(
                "SELECT MIN(transaction_date) FROM investment_transactions WHERE ticker = ?1",
                [&ticker.to_uppercase()],
                |row| row.get(0),
            )
            .ok()
            .flatten();
        Ok(result)
    })?;

    let from_timestamp = match oldest_tx_date {
        Some(date) => date,
        None => {
            log::debug!(
                "[BACKFILL] No transactions found for ticker {}, nothing to backfill",
                ticker
            );
            return Ok(BackfillResult {
                days_processed: 0,
                total_days: 0,
                completed: true,
                message: "No transactions found for this ticker".to_string(),
            });
        }
    };

    // Limit to last 365 days to avoid too many API calls
    let now = chrono::Utc::now().timestamp();
    let one_year_ago = now - (365 * 86400);
    let effective_from = from_timestamp.max(one_year_ago);

    let from_day = (effective_from / 86400) * 86400;
    let today_start = (now / 86400) * 86400;

    // Count days to process
    let total_days = ((today_start - from_day) / 86400 + 1) as i32;

    // Rebuild this ticker's rows only: the queue owns the portfolio days
    recalculate_stock_ticker_history(&db, &ticker, effective_from).await?;

    log::debug!(
        "[BACKFILL] On-demand backfill completed for ticker {}",
        ticker
    );

    Ok(BackfillResult {
        days_processed: total_days,
        total_days,
        completed: true,
        message: format!("Backfilled {} days for {}", total_days, ticker),
    })
}

/// Tauri command to backfill a specific crypto ticker's value history on demand
/// This is called when the user views a crypto detail page to ensure the chart has data
#[tauri::command]
pub async fn backfill_crypto_ticker_history(
    db: State<'_, Database>,
    ticker: String,
) -> Result<BackfillResult> {
    log::debug!(
        "[BACKFILL] On-demand backfill requested for crypto ticker: {}",
        ticker
    );

    // Get the coingecko_id and oldest transaction date for this ticker
    let (coingecko_id, oldest_tx_date): (Option<String>, Option<i64>) = db.with_conn(|conn| {
        let cg_id: Option<String> = conn
            .query_row(
                "SELECT coingecko_id FROM crypto_investments WHERE ticker = ?1",
                [&ticker.to_uppercase()],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        let oldest: Option<i64> = conn
            .query_row(
                "SELECT MIN(transaction_date) FROM crypto_transactions WHERE ticker = ?1",
                [&ticker.to_uppercase()],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        Ok((cg_id, oldest))
    })?;

    match coingecko_id {
        Some(id) if !id.is_empty() => {}
        _ => {
            log::debug!(
                "[BACKFILL] No coingecko_id found for crypto {}, skipping",
                ticker
            );
            return Ok(BackfillResult {
                days_processed: 0,
                total_days: 0,
                completed: true,
                message: "No CoinGecko ID configured for this crypto".to_string(),
            });
        }
    }

    let from_timestamp = match oldest_tx_date {
        Some(date) => date,
        None => {
            log::debug!(
                "[BACKFILL] No transactions found for crypto {}, nothing to backfill",
                ticker
            );
            return Ok(BackfillResult {
                days_processed: 0,
                total_days: 0,
                completed: true,
                message: "No transactions found for this crypto".to_string(),
            });
        }
    };

    // Limit to last 365 days
    let now = chrono::Utc::now().timestamp();
    let one_year_ago = now - (365 * 86400);
    let effective_from = from_timestamp.max(one_year_ago);

    let from_day = (effective_from / 86400) * 86400;
    let today_start = (now / 86400) * 86400;

    let total_days = ((today_start - from_day) / 86400 + 1) as i32;

    // Rebuild this ticker's rows only: the queue owns the portfolio days
    history_recalc::rebuild_crypto_ticker(
        &db,
        &ticker.to_uppercase(),
        effective_from,
        today_start,
        find_closest_price,
    )
    .await?;

    log::debug!(
        "[BACKFILL] On-demand backfill completed for crypto {}",
        ticker
    );

    Ok(BackfillResult {
        days_processed: total_days,
        total_days,
        completed: true,
        message: format!("Backfilled {} days for {}", total_days, ticker),
    })
}

/// Recalculate stock value history for a single ticker from a given date.
///
/// Quotes are fetched without the database lock and the rows are planned in one
/// pass and written in short chunks: see `services::ticker_history` and
/// `services::history_recalc`.
pub async fn recalculate_stock_ticker_history(
    db: &Database,
    ticker: &str,
    from_timestamp: i64,
) -> Result<()> {
    log::debug!(
        "[RECALC] Starting ticker-specific recalculation for {} from {}",
        ticker,
        from_timestamp
    );

    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;

    let stats = history_recalc::rebuild_stock_ticker(
        db,
        ticker,
        from_timestamp,
        today_start,
        find_closest_price,
    )
    .await?;
    // The fetch found a new split: the closes before its window are unadjusted (spec D18).
    if let Some(first) = stats.full_rebuild_from {
        history_recalc::rebuild_stock_ticker(db, ticker, first, today_start, find_closest_price)
            .await?;
    }

    log::debug!("[RECALC] Ticker {} history recalculation complete!", ticker);

    Ok(())
}

/// Queue the history rebuild an MCP bulk import (or any external caller) asks for: the
/// tickers of `asset_type` from `earliest_date`, or every portfolio day for anything else (the
/// static classes can change every day, `history_plan::all_days`).
/// Caller identifiers (`coingeckoId`) are hints, never the authority for persisted history:
/// the worker reads them from `crypto_investments`. Emits `recalculation-complete` right away,
/// mirroring the UI import flows, so the trend charts do not wait for a remount.
#[tauri::command]
pub async fn trigger_import_recalculation(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    asset_type: String,
    tickers: Vec<(String, Option<String>)>,
    earliest_date: i64,
) -> Result<()> {
    let jobs = match asset_type.as_str() {
        "stocks" => tickers
            .into_iter()
            .map(|(ticker, _)| Job::StockTicker(ticker, earliest_date))
            .collect(),
        "crypto" => tickers
            .into_iter()
            .map(|(ticker, _)| Job::CryptoTicker(ticker, earliest_date))
            .collect(),
        _ => vec![history_plan::all_days()],
    };
    schedule_history_rebuild(&app, &db, &recalc, jobs);
    app.emit("recalculation-complete", ()).ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_calculate_portfolio_metrics_investments_by_currency() {
        let conn = rusqlite::Connection::open_in_memory().expect("db");
        conn.execute_batch(
            r#"
            CREATE TABLE stock_investments (id TEXT PRIMARY KEY, ticker TEXT, quantity TEXT, currency TEXT DEFAULT 'USD');
            CREATE TABLE stock_data (ticker TEXT PRIMARY KEY, original_price TEXT, currency TEXT, fetched_at INTEGER);
            CREATE TABLE stock_price_overrides (ticker TEXT PRIMARY KEY, price TEXT, currency TEXT, updated_at INTEGER);
            CREATE TABLE crypto_investments (id TEXT PRIMARY KEY, ticker TEXT, quantity TEXT, currency TEXT DEFAULT 'USD');
            CREATE TABLE crypto_data (ticker TEXT PRIMARY KEY, original_price TEXT, currency TEXT, fetched_at INTEGER);
            CREATE TABLE crypto_price_overrides (ticker TEXT PRIMARY KEY, price TEXT, currency TEXT, updated_at INTEGER);
            CREATE TABLE bank_accounts (id TEXT PRIMARY KEY, name TEXT, balance TEXT, currency TEXT DEFAULT 'CZK');
            CREATE TABLE bonds (id TEXT PRIMARY KEY, coupon_value TEXT, quantity TEXT, currency TEXT DEFAULT 'CZK');
            CREATE TABLE loans (id TEXT PRIMARY KEY, principal TEXT, currency TEXT DEFAULT 'CZK');
            CREATE TABLE real_estate (id TEXT PRIMARY KEY, type TEXT, market_price TEXT, market_price_currency TEXT DEFAULT 'CZK');
            CREATE TABLE other_assets (id TEXT PRIMARY KEY, quantity TEXT, market_price TEXT, currency TEXT DEFAULT 'CZK');

            INSERT INTO stock_investments VALUES ('1', 'AAPL', '10', 'USD');
            INSERT INTO stock_investments VALUES ('2', 'T7203', '5', 'JPY');
            INSERT INTO stock_data (ticker, original_price, currency, fetched_at) VALUES ('AAPL', '150.0', 'USD', 0);
            INSERT INTO stock_data (ticker, original_price, currency, fetched_at) VALUES ('T7203', '2000.0', 'JPY', 0);
        "#,
        )
        .expect("schema");

        crate::services::fx::set_current_for_test(&[
            ("CZK", 25.0),
            ("USD", 25.0 / 23.0),
            ("JPY", 25.0 / 0.15),
        ]);

        let resolved_aapl =
            crate::services::pricing::resolve_stock_price(&conn, "AAPL").expect("AAPL price");
        assert_eq!(resolved_aapl.currency, "USD");
        assert!((resolved_aapl.original_price.parse::<f64>().unwrap() - 150.0).abs() < 0.01);

        let resolved_t7203 =
            crate::services::pricing::resolve_stock_price(&conn, "T7203").expect("T7203 price");
        assert_eq!(resolved_t7203.currency, "JPY");
        assert!((resolved_t7203.original_price.parse::<f64>().unwrap() - 2000.0).abs() < 0.01);
    }
}
