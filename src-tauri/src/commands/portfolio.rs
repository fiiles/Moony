//! Portfolio metrics commands

use crate::db::Database;
use crate::error::Result;
use crate::models::PortfolioMetricsHistory;
use crate::services::currency::convert_to_czk;
use crate::services::loan_amortization::today_utc_day;
use crate::services::parsing::parse_money;
use crate::services::portfolio_history::{
    self, AssetClassKind, ClassBreakdowns, SnapshotSource, StaticBreakdowns,
};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

/// Current portfolio metrics
#[derive(Debug, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct PortfolioMetrics {
    pub total_savings: f64,
    pub total_investments: f64,
    pub total_crypto: f64,
    pub total_bonds: f64,
    pub total_real_estate_personal: f64,
    pub total_real_estate_investment: f64,
    pub total_real_estate: f64,
    pub total_other_assets: f64,
    pub total_liabilities: f64,
    pub total_assets: f64,
    pub net_worth: f64,
    // Native currency breakdowns
    pub savings_by_currency: std::collections::HashMap<String, f64>,
    pub investments_by_currency: std::collections::HashMap<String, f64>,
    pub crypto_by_currency: std::collections::HashMap<String, f64>,
    pub bonds_by_currency: std::collections::HashMap<String, f64>,
    pub real_estate_by_currency: std::collections::HashMap<String, f64>,
    pub loans_by_currency: std::collections::HashMap<String, f64>,
    pub other_assets_by_currency: std::collections::HashMap<String, f64>,
}

/// Get current portfolio metrics
#[tauri::command]
pub async fn get_portfolio_metrics(
    db: State<'_, Database>,
    exclude_personal_real_estate: bool,
) -> Result<PortfolioMetrics> {
    calculate_portfolio_metrics(&db, exclude_personal_real_estate)
}

fn calculate_portfolio_metrics(
    db: &Database,
    exclude_personal_real_estate: bool,
) -> Result<PortfolioMetrics> {
    Ok(calculate_portfolio_metrics_full(db, exclude_personal_real_estate)?.0)
}

/// Live metrics plus the full native breakdowns (real estate split included,
/// regardless of the exclude flag) for the single snapshot writer.
fn calculate_portfolio_metrics_full(
    db: &Database,
    exclude_personal_real_estate: bool,
) -> Result<(PortfolioMetrics, ClassBreakdowns)> {
    db.with_conn(|conn| {
        // --- Savings ---
        let mut savings_by_currency: HashMap<String, f64> = HashMap::new();
        let mut bank_stmt = conn.prepare(
            "SELECT balance, currency FROM bank_accounts WHERE exclude_from_balance = 0",
        )?;
        let total_savings: f64 = bank_stmt
            .query_map([], |row| {
                let balance: f64 =
                    parse_money(&row.get::<_, String>(0)?, 0.0, "bank_accounts.balance");
                let currency: String = row.get(1)?;
                Ok((balance, currency))
            })?
            .filter_map(|r| r.ok())
            .map(|(balance, currency)| {
                *savings_by_currency.entry(currency.clone()).or_insert(0.0) += balance;
                convert_to_czk(balance, &currency)
            })
            .sum();

        // --- Bonds ---
        let mut bonds_by_currency: HashMap<String, f64> = HashMap::new();
        let mut bonds_stmt = conn.prepare("SELECT coupon_value, quantity, currency FROM bonds")?;
        let total_bonds: f64 = bonds_stmt
            .query_map([], |row| {
                let value: f64 = parse_money(&row.get::<_, String>(0)?, 0.0, "bonds.coupon_value");
                let quantity: f64 = parse_money(&row.get::<_, String>(1)?, 1.0, "bonds.quantity");
                let currency: String = row.get(2)?;
                Ok((value, quantity, currency))
            })?
            .filter_map(|r| r.ok())
            .map(|(value, quantity, currency)| {
                *bonds_by_currency.entry(currency.clone()).or_insert(0.0) += value * quantity;
                convert_to_czk(value * quantity, &currency)
            })
            .sum();

        // --- Liabilities (loans), valued at the amortized balance as of today ---
        let loans_by_currency: HashMap<String, f64> =
            crate::services::loans::outstanding_by_currency(conn, today_utc_day())?;
        let total_liabilities: f64 = loans_by_currency
            .iter()
            .map(|(currency, balance)| convert_to_czk(*balance, currency))
            .sum();

        // --- Real estate ---
        let mut real_estate_by_currency: HashMap<String, f64> = HashMap::new();
        let mut re_personal_by_currency: HashMap<String, f64> = HashMap::new();
        let mut re_investment_by_currency: HashMap<String, f64> = HashMap::new();
        let mut stmt =
            conn.prepare("SELECT type, market_price, market_price_currency FROM real_estate")?;
        let mut total_re_personal = 0.0;
        let mut total_re_investment = 0.0;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;

        for row in rows.filter_map(|r| r.ok()) {
            let price: f64 = parse_money(&row.1, 0.0, "real_estate.market_price");
            let currency = row.2.clone();
            let price_czk = convert_to_czk(price, &currency);
            let is_personal = row.0 == "personal";
            // Breakdown must sum (in CZK terms) to total_real_estate, which excludes
            // personal properties when exclude_personal_real_estate is set. Personal
            // properties still count into total_re_personal below (always reported).
            if !(exclude_personal_real_estate && is_personal) {
                *real_estate_by_currency
                    .entry(currency.clone())
                    .or_insert(0.0) += price;
            }
            if is_personal {
                total_re_personal += price_czk;
                *re_personal_by_currency.entry(currency).or_insert(0.0) += price;
            } else {
                total_re_investment += price_czk;
                *re_investment_by_currency.entry(currency).or_insert(0.0) += price;
            }
        }

        // --- Investments (stocks) ---
        let mut investments_by_currency: HashMap<String, f64> = HashMap::new();
        let mut total_investments = 0.0;
        let mut inv_stmt = conn.prepare("SELECT ticker, quantity FROM stock_investments")?;
        let investments: Vec<(String, String)> = inv_stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        for inv in investments {
            let qty: f64 = parse_money(&inv.1, 0.0, "stock_investments.quantity");
            if let Some(resolved) = crate::services::pricing::resolve_stock_price(conn, &inv.0) {
                total_investments += resolved.price_czk * qty;
                let native_price: f64 = parse_money(
                    &resolved.original_price,
                    0.0,
                    "resolved stock price (original_price)",
                );
                *investments_by_currency
                    .entry(resolved.currency.clone())
                    .or_insert(0.0) += native_price * qty;
            }
        }

        // --- Crypto ---
        let mut crypto_by_currency: HashMap<String, f64> = HashMap::new();
        let mut total_crypto = 0.0;
        let mut crypto_stmt = conn.prepare("SELECT ticker, quantity FROM crypto_investments")?;
        let cryptos: Vec<(String, String)> = crypto_stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        for crypto in cryptos {
            let qty: f64 = parse_money(&crypto.1, 0.0, "crypto_investments.quantity");
            if let Some(resolved) = crate::services::pricing::resolve_crypto_price(conn, &crypto.0)
            {
                total_crypto += resolved.price_czk * qty;
                let native_price: f64 = parse_money(
                    &resolved.original_price,
                    0.0,
                    "resolved crypto price (original_price)",
                );
                *crypto_by_currency
                    .entry(resolved.currency.clone())
                    .or_insert(0.0) += native_price * qty;
            }
        }

        // --- Other assets ---
        let mut other_assets_by_currency: HashMap<String, f64> = HashMap::new();
        let mut total_other_assets = 0.0;
        let mut other_stmt =
            conn.prepare("SELECT quantity, market_price, currency FROM other_assets")?;
        let other_assets: Vec<(String, String, String)> = other_stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .filter_map(|r| r.ok())
            .collect();

        for asset in other_assets {
            let qty: f64 = parse_money(&asset.0, 0.0, "other_assets.quantity");
            let price: f64 = parse_money(&asset.1, 0.0, "other_assets.market_price");
            let currency = asset.2;
            *other_assets_by_currency
                .entry(currency.clone())
                .or_insert(0.0) += qty * price;
            total_other_assets += convert_to_czk(qty * price, &currency);
        }

        // --- Totals ---
        let total_real_estate = if exclude_personal_real_estate {
            total_re_investment
        } else {
            total_re_personal + total_re_investment
        };

        let total_assets = total_savings
            + total_investments
            + total_crypto
            + total_bonds
            + total_real_estate
            + total_other_assets;
        let net_worth = total_assets - total_liabilities;

        let breakdowns = ClassBreakdowns {
            savings: savings_by_currency.clone(),
            investments: investments_by_currency.clone(),
            crypto: crypto_by_currency.clone(),
            bonds: bonds_by_currency.clone(),
            real_estate_personal: re_personal_by_currency,
            real_estate_investment: re_investment_by_currency,
            loans: loans_by_currency.clone(),
            other_assets: other_assets_by_currency.clone(),
        };

        Ok((
            PortfolioMetrics {
                total_savings,
                total_investments,
                total_crypto,
                total_bonds,
                total_real_estate_personal: total_re_personal,
                total_real_estate_investment: total_re_investment,
                total_real_estate,
                total_other_assets,
                total_liabilities,
                total_assets,
                net_worth,
                savings_by_currency,
                investments_by_currency,
                crypto_by_currency,
                bonds_by_currency,
                real_estate_by_currency,
                loans_by_currency,
                other_assets_by_currency,
            },
            breakdowns,
        ))
    })
}

/// Get portfolio history
#[tauri::command]
pub async fn get_portfolio_history(
    db: State<'_, Database>,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<PortfolioMetricsHistory>> {
    db.with_conn(|conn| portfolio_history::read_history(conn, start_date, end_date))
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
        // Record stock values
        let mut inv_stmt = conn.prepare(
            "SELECT si.ticker, si.quantity FROM stock_investments si"
        )?;
        let investments: Vec<(String, String)> = inv_stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        for (ticker, qty_str) in investments {
            let qty: f64 = qty_str.parse().unwrap_or(0.0);
            if qty <= 0.0 {
                continue;
            }

            if let Some(resolved) = crate::services::pricing::resolve_stock_price(conn, &ticker) {
                let price: f64 = resolved.original_price.parse().unwrap_or(0.0);
                if price <= 0.0 {
                    continue;
                }
                let value_czk = resolved.price_czk * qty;
                let id = Uuid::new_v4().to_string();

                conn.execute(
                    "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(ticker, recorded_at) DO UPDATE SET
                         value_czk = excluded.value_czk,
                         quantity = excluded.quantity,
                         price = excluded.price,
                         currency = excluded.currency",
                    rusqlite::params![
                        id,
                        ticker,
                        today_start,
                        value_czk.to_string(),
                        qty.to_string(),
                        price.to_string(),
                        resolved.currency,
                    ],
                )?;
            }
        }

        // Record crypto values
        let mut crypto_stmt = conn.prepare(
            "SELECT ticker, quantity FROM crypto_investments"
        )?;
        let cryptos: Vec<(String, String)> = crypto_stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .filter_map(|r| r.ok())
            .collect();

        for (ticker, qty_str) in cryptos {
            let qty: f64 = qty_str.parse().unwrap_or(0.0);
            if qty <= 0.0 {
                continue;
            }

            if let Some(resolved) = crate::services::pricing::resolve_crypto_price(conn, &ticker) {
                let price: f64 = resolved.original_price.parse().unwrap_or(0.0);
                if price <= 0.0 {
                    continue;
                }
                let value_czk = resolved.price_czk * qty;
                let id = Uuid::new_v4().to_string();

                conn.execute(
                    "INSERT INTO crypto_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(ticker, recorded_at) DO UPDATE SET
                         value_czk = excluded.value_czk,
                         quantity = excluded.quantity,
                         price = excluded.price,
                         currency = excluded.currency",
                    rusqlite::params![
                        id,
                        ticker,
                        today_start,
                        value_czk.to_string(),
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
    let (_, breakdowns) = calculate_portfolio_metrics_full(db, false)?;
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

/// Update exchange rates from ECB and return them
#[tauri::command]
pub async fn refresh_exchange_rates(
    app: AppHandle,
    db: State<'_, Database>,
) -> Result<std::collections::HashMap<String, f64>> {
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
    // Best-effort — a failure only means uncovered days keep falling back to
    // current rates until the next refresh.
    let db_for_backfill = db.inner().clone();
    tauri::async_runtime::spawn(async move {
        match crate::services::fx_backfill::ensure_fx_coverage(&db_for_backfill).await {
            // New rate rows change historical valuations — poke the same
            // event the charts and dated converters already listen to.
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

    Ok(rates)
}

/// Get current exchange rates (without fetching)
#[tauri::command]
pub fn get_exchange_rates() -> std::collections::HashMap<String, f64> {
    crate::services::currency::get_all_rates()
}

/// Get exchange rates (relative to CZK) for a specific date.
/// Falls back to the nearest earlier date with a snapshot, then to today's rates.
#[tauri::command]
pub async fn get_exchange_rates_for_date(
    db: State<'_, Database>,
    date: i64,
) -> Result<std::collections::HashMap<String, f64>> {
    db.with_conn(|conn| Ok(crate::services::currency::get_rates_for_date(conn, date)))
}

/// Get exchange rates for all dates in a range that have history snapshots.
/// Returns a map of date (midnight UTC unix timestamp) → rates.
#[tauri::command]
pub async fn get_exchange_rates_for_date_range(
    db: State<'_, Database>,
    start_date: i64,
    end_date: i64,
) -> Result<std::collections::HashMap<i64, std::collections::HashMap<String, f64>>> {
    db.with_conn(|conn| {
        Ok(crate::services::currency::get_rates_for_date_range(
            conn, start_date, end_date,
        ))
    })
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
    /// Currency codes encountered this session with no known exchange rate
    /// (converted 1:1 to CZK as a last resort — values are wrong; audit M2)
    pub missing_currencies: Vec<String>,
}

/// Staleness thresholds in hours
const STOCKS_STALE_THRESHOLD_HOURS: i64 = 24;
const CRYPTO_STALE_THRESHOLD_HOURS: i64 = 12;
const EXCHANGE_RATES_STALE_THRESHOLD_HOURS: i64 = 48;

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
            .map(|h| h > EXCHANGE_RATES_STALE_THRESHOLD_HOURS)
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
            missing_currencies: crate::services::currency::get_missing_currencies(),
        })
    })
}

// ============================================================================
// Historical Snapshot Backfill
// ============================================================================

use crate::services::price_api::{
    get_api_keys, get_historical_crypto_prices_coingecko, get_historical_stock_prices_yahoo,
    HistoricalPrice,
};
use std::collections::HashMap;

/// Result returned by the backfill command
#[derive(Debug, Clone, serde::Serialize)]
pub struct BackfillResult {
    pub days_processed: i32,
    pub total_days: i32,
    pub completed: bool,
    pub message: String,
}

/// Get snapshot date range (oldest and newest) and all existing day timestamps
fn get_snapshot_date_info(db: &Database) -> Result<(Option<i64>, Option<i64>, Vec<i64>)> {
    db.with_conn(|conn| {
        let min_date: Option<i64> = conn
            .query_row(
                "SELECT MIN(recorded_at) FROM portfolio_metrics_history",
                [],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        let max_date: Option<i64> = conn
            .query_row(
                "SELECT MAX(recorded_at) FROM portfolio_metrics_history",
                [],
                |row| row.get(0),
            )
            .ok()
            .flatten();

        // Get all existing day timestamps (normalized to start of day)
        let mut stmt = conn.prepare(
            "SELECT DISTINCT (recorded_at / 86400) * 86400 FROM portfolio_metrics_history",
        )?;
        let existing_days: Vec<i64> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok((min_date, max_date, existing_days))
    })
}

/// Get all tickers for stocks
fn get_stock_tickers(db: &Database) -> Result<Vec<String>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare("SELECT DISTINCT ticker FROM stock_investments")?;
        let tickers: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();
        Ok(tickers)
    })
}

/// Get crypto investments with their coingecko IDs
fn get_crypto_id_map(db: &Database) -> Result<HashMap<String, String>> {
    db.with_conn(|conn| {
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
    })
}

/// Static classes from the current tables (native amounts) — used only when
/// no live snapshot precedes a reconstructed day (e.g. a fresh install).
fn current_static_breakdowns(conn: &rusqlite::Connection) -> Result<StaticBreakdowns> {
    let mut statics = StaticBreakdowns::default();

    let mut bank_stmt =
        conn.prepare("SELECT balance, currency FROM bank_accounts WHERE exclude_from_balance = 0")?;
    for row in bank_stmt
        .query_map([], |row| {
            let balance: f64 = parse_money(&row.get::<_, String>(0)?, 0.0, "bank_accounts.balance");
            let currency: String = row.get(1)?;
            Ok((balance, currency))
        })?
        .filter_map(|r| r.ok())
    {
        *statics.savings.entry(row.1).or_insert(0.0) += row.0;
    }

    let mut bonds_stmt = conn.prepare("SELECT coupon_value, quantity, currency FROM bonds")?;
    for row in bonds_stmt
        .query_map([], |row| {
            let value: f64 = parse_money(&row.get::<_, String>(0)?, 0.0, "bonds.coupon_value");
            let quantity: f64 = parse_money(&row.get::<_, String>(1)?, 1.0, "bonds.quantity");
            let currency: String = row.get(2)?;
            Ok((value * quantity, currency))
        })?
        .filter_map(|r| r.ok())
    {
        *statics.bonds.entry(row.1).or_insert(0.0) += row.0;
    }

    // Loans at the amortized balance as of today
    for (currency, balance) in
        crate::services::loans::outstanding_by_currency(conn, today_utc_day())?
    {
        *statics.loans.entry(currency).or_insert(0.0) += balance;
    }

    let mut re_stmt =
        conn.prepare("SELECT type, market_price, market_price_currency FROM real_estate")?;
    for row in re_stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .filter_map(|r| r.ok())
    {
        let price: f64 = parse_money(&row.1, 0.0, "real_estate.market_price");
        let target = if row.0 == "personal" {
            &mut statics.real_estate_personal
        } else {
            &mut statics.real_estate_investment
        };
        *target.entry(row.2).or_insert(0.0) += price;
    }

    let mut other_stmt =
        conn.prepare("SELECT quantity, market_price, currency FROM other_assets")?;
    for row in other_stmt
        .query_map([], |row| {
            let qty: f64 = parse_money(&row.get::<_, String>(0)?, 0.0, "other_assets.quantity");
            let price: f64 =
                parse_money(&row.get::<_, String>(1)?, 0.0, "other_assets.market_price");
            let currency = row.get::<_, String>(2)?;
            Ok((qty * price, currency))
        })?
        .filter_map(|r| r.ok())
    {
        *statics.other_assets.entry(row.1).or_insert(0.0) += row.0;
    }

    Ok(statics)
}

/// `(ticker, quantity)` of every row of a holdings table.
fn held_positions(
    conn: &rusqlite::Connection,
    sql: &str,
    context: &str,
) -> Result<Vec<(String, f64)>> {
    let mut stmt = conn.prepare(sql)?;
    let positions = stmt
        .query_map([], |row| {
            let ticker: String = row.get(0)?;
            let qty: f64 = parse_money(&row.get::<_, String>(1)?, 0.0, context);
            Ok((ticker, qty))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(positions)
}

/// Native breakdowns for a reconstructed (gap-backfilled) day: stocks/crypto
/// from historical prices (look-back only, the last known price carried
/// forward), static classes carried forward from the nearest earlier live
/// snapshot (current tables when none). CZK totals are derived later by the
/// single writer.
///
/// Returns `None` when a held position has no known price on that day
///: the day is left unvalued, to be retried by a later run, instead
/// of being written with an invented price (today's quote) or with the
/// position silently left out of the total.
fn breakdowns_for_backfill_day(
    db: &Database,
    day_timestamp: i64,
    stock_prices: &HashMap<String, Vec<HistoricalPrice>>,
    crypto_prices: &HashMap<String, Vec<HistoricalPrice>>,
) -> Result<Option<ClassBreakdowns>> {
    db.with_conn(|conn| {
        let statics =
            match portfolio_history::carried_statics_from_nearest_live(conn, day_timestamp)? {
                Some(statics) => statics,
                None => current_static_breakdowns(conn)?,
            };

        let stock_positions = held_positions(
            conn,
            "SELECT ticker, quantity FROM stock_investments",
            "stock_investments.quantity",
        )?;
        let Some(investments_by_currency) =
            backfill::value_positions_on_day(&stock_positions, stock_prices, day_timestamp)
        else {
            return Ok(None);
        };

        let crypto_positions = held_positions(
            conn,
            "SELECT ticker, quantity FROM crypto_investments",
            "crypto_investments.quantity",
        )?;
        let Some(crypto_by_currency) =
            backfill::value_positions_on_day(&crypto_positions, crypto_prices, day_timestamp)
        else {
            return Ok(None);
        };

        Ok(Some(ClassBreakdowns {
            savings: statics.savings,
            investments: investments_by_currency,
            crypto: crypto_by_currency,
            bonds: statics.bonds,
            real_estate_personal: statics.real_estate_personal,
            real_estate_investment: statics.real_estate_investment,
            loans: statics.loans,
            other_assets: statics.other_assets,
        }))
    })
}

use crate::services::backfill;
use crate::services::portfolio_history::breakdown_json;

/// The price known as of `target`'s day: the latest valid price on or before
/// it, `None` before the first one. Look-back only; the name is kept
/// so the history callers stay untouched, the rule lives in
/// `services::backfill::price_at_or_before`.
fn find_closest_price(
    prices: &[HistoricalPrice],
    target_timestamp: i64,
) -> Option<&HistoricalPrice> {
    backfill::price_at_or_before(prices, target_timestamp)
}

/// Backfill missing portfolio snapshots
/// This is the main function that orchestrates the backfill process
/// It finds gaps between the oldest and newest snapshot and fills them
pub async fn backfill_missing_snapshots(db: &Database) -> Result<BackfillResult> {
    log::info!("[BACKFILL] Starting snapshot backfill...");

    // Get snapshot date range and existing days
    let (min_date, _max_date, existing_days) = get_snapshot_date_info(db)?;

    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400; // Start of today (UTC)

    // Need at least one snapshot to backfill from
    let oldest_day = match min_date {
        Some(date) => (date / 86400) * 86400,
        None => {
            log::info!("[BACKFILL] No existing snapshots found, skipping backfill");
            return Ok(BackfillResult {
                days_processed: 0,
                total_days: 0,
                completed: true,
                message: "No previous snapshots to backfill from".to_string(),
            });
        }
    };

    // Convert existing days to a HashSet for O(1) lookup
    // IMPORTANT: Remove today from the set - we want to fill gaps up to yesterday
    // because today's snapshot may have been created by price refresh before we got here
    let mut existing_days_set: std::collections::HashSet<i64> = existing_days.into_iter().collect();
    existing_days_set.remove(&today_start);

    log::info!(
        "[BACKFILL] Oldest snapshot: {}, existing days (excluding today): {}",
        oldest_day,
        existing_days_set.len()
    );

    // Find the missing days between the oldest snapshot and yesterday (not
    // today): the newest BACKFILL_MAX_DAYS_PER_RUN of them now, older gaps on
    // the next run (they stay missing in the database, nothing else to track).
    let yesterday_start = today_start - 86400;
    let plan = backfill::plan_backfill(
        oldest_day,
        yesterday_start,
        &existing_days_set,
        backfill::BACKFILL_MAX_DAYS_PER_RUN,
    );

    if plan.days.is_empty() {
        log::info!("[BACKFILL] No missing days to backfill");
        return Ok(BackfillResult {
            days_processed: 0,
            total_days: 0,
            completed: true,
            message: "No missing days".to_string(),
        });
    }

    let missing_days = plan.days;
    let deferred_days = plan.deferred;
    let total_missing = missing_days.len() as i32;

    log::info!(
        "[BACKFILL] Found {} missing days to backfill ({} older days deferred to the next run)",
        total_missing,
        deferred_days
    );

    // Get the date range for fetching historical prices; the lead-in gives the
    // first day a price to carry forward (weekend / holiday before it).
    let fetch_start =
        missing_days.first().copied().unwrap_or(today_start) - backfill::PRICE_LEAD_IN_DAYS * 86400;
    let fetch_end = today_start;

    // Get tickers for fetching
    let stock_tickers = get_stock_tickers(db)?;
    let crypto_id_map = get_crypto_id_map(db)?;

    log::info!(
        "[BACKFILL] Found {} stock tickers and {} crypto tickers",
        stock_tickers.len(),
        crypto_id_map.len()
    );

    // Fetch historical prices
    let stock_prices = if !stock_tickers.is_empty() {
        get_historical_stock_prices_yahoo(&stock_tickers, fetch_start, fetch_end).await?
    } else {
        HashMap::new()
    };

    let api_keys = get_api_keys(db)?;
    let crypto_prices = if !crypto_id_map.is_empty() {
        get_historical_crypto_prices_coingecko(
            api_keys.coingecko.as_deref(),
            &crypto_id_map,
            fetch_start,
            fetch_end,
        )
        .await?
    } else {
        HashMap::new()
    };

    // Create snapshots for each missing day. Prices are looked up in the
    // fetched history only (look-back, carry-forward): there is no seed from
    // today's quote, so a day nothing can price is left empty.
    let mut days_processed = 0;
    let mut days_unpriced = 0;

    for day_timestamp in missing_days.clone() {
        // Reconstruct native breakdowns for this day; the single writer
        // derives the CZK totals at the day's rates and stamps provenance.
        let Some(breakdowns) =
            breakdowns_for_backfill_day(db, day_timestamp, &stock_prices, &crypto_prices)?
        else {
            days_unpriced += 1;
            log::debug!(
                "[BACKFILL] No known price for a held position on day {}, leaving it empty",
                day_timestamp
            );
            continue;
        };
        db.with_conn(|conn| {
            portfolio_history::upsert_snapshot(
                conn,
                day_timestamp,
                SnapshotSource::Backfill,
                &breakdowns,
            )
        })?;

        days_processed += 1;

        log::debug!(
            "[BACKFILL] Created snapshot for day {}/{}",
            days_processed,
            total_missing
        );
    }

    // Also populate per-ticker history tables
    log::info!("[BACKFILL] Populating per-ticker history tables...");

    // Each day's value_czk converts at that day's rates (FX timeseries), not
    // today's — one prefetch for the whole missing span, with the 10-day
    // walk-back margin. Days older than the timeseries fall back to current
    // rates, matching get_rates_for_date.
    let rates_by_day = {
        let first = missing_days.first().copied().unwrap_or(0);
        let last = missing_days.last().copied().unwrap_or(0);
        db.with_conn(|conn| {
            Ok(crate::services::currency::get_rates_for_date_range(
                conn,
                first - 10 * 86400,
                last,
            ))
        })?
    };
    let czk_value_at = |amount: f64, currency: &str, day: i64| -> f64 {
        match crate::services::currency::resolve_rates_for_day_from_range(&rates_by_day, day) {
            Some(rates) => {
                crate::services::currency::convert_to_czk_with_rates(amount, currency, rates)
            }
            None => convert_to_czk(amount, currency),
        }
    };

    // Populate stock_value_history
    for ticker in &stock_tickers {
        db.with_conn(|conn| {
            for day_timestamp in &missing_days {
                let quantity = get_stock_quantity_at_date(conn, ticker, *day_timestamp);

                if quantity <= 0.0 {
                    continue;
                }

                let (price, currency) = if let Some(prices) = stock_prices.get(ticker) {
                    if let Some(hp) = find_closest_price(prices, *day_timestamp) {
                        (hp.price, hp.currency.clone())
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                let value_czk = czk_value_at(quantity * price, &currency, *day_timestamp);
                let id = Uuid::new_v4().to_string();

                conn.execute(
                    "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(ticker, recorded_at) DO UPDATE SET
                         value_czk = excluded.value_czk,
                         quantity = excluded.quantity,
                         price = excluded.price,
                         currency = excluded.currency",
                    rusqlite::params![
                        id,
                        ticker,
                        day_timestamp,
                        value_czk.to_string(),
                        quantity.to_string(),
                        price.to_string(),
                        currency,
                    ],
                )?;
            }
            Ok(())
        })?;
    }

    // Populate crypto_value_history
    for ticker in crypto_id_map.keys() {
        db.with_conn(|conn| {
            for day_timestamp in &missing_days {
                let quantity = get_crypto_quantity_at_date(conn, ticker, *day_timestamp);

                if quantity <= 0.0 {
                    continue;
                }

                let (price, currency) = if let Some(prices) = crypto_prices.get(ticker) {
                    if let Some(hp) = find_closest_price(prices, *day_timestamp) {
                        (hp.price, hp.currency.clone())
                    } else {
                        continue;
                    }
                } else {
                    continue;
                };

                let value_czk = czk_value_at(quantity * price, &currency, *day_timestamp);
                let id = Uuid::new_v4().to_string();

                conn.execute(
                    "INSERT INTO crypto_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                     ON CONFLICT(ticker, recorded_at) DO UPDATE SET
                         value_czk = excluded.value_czk,
                         quantity = excluded.quantity,
                         price = excluded.price,
                         currency = excluded.currency",
                    rusqlite::params![
                        id,
                        ticker,
                        day_timestamp,
                        value_czk.to_string(),
                        quantity.to_string(),
                        price.to_string(),
                        currency,
                    ],
                )?;
            }
            Ok(())
        })?;
    }

    log::info!(
        "[BACKFILL] Backfill complete! Created {} snapshots, {} left empty (no price), {} older days deferred",
        days_processed,
        days_unpriced,
        deferred_days
    );

    let mut message = format!("Created {} historical snapshots", days_processed);
    if days_unpriced > 0 {
        message.push_str(&format!(
            "; {} days left empty (a held position had no known price)",
            days_unpriced
        ));
    }
    if deferred_days > 0 {
        message.push_str(&format!(
            "; {} older days remain for the next run",
            deferred_days
        ));
    }
    if api_keys.coingecko.is_none() || api_keys.coingecko.as_deref() == Some("") {
        message.push_str("; WARNING: CoinGecko API key missing, crypto history may be incomplete");
    }

    Ok(BackfillResult {
        days_processed,
        total_days: total_missing,
        // Nothing left for a later run: every planned day was filled
        completed: deferred_days == 0 && days_unpriced == 0,
        message,
    })
}

/// Tauri command to start snapshot backfill
#[tauri::command]
pub async fn start_snapshot_backfill(db: State<'_, Database>) -> Result<BackfillResult> {
    backfill_missing_snapshots(&db).await
}

/// Tauri command to recalculate all portfolio history from scratch
/// This is useful when historical data has been corrupted or needs to be regenerated
#[tauri::command]
pub async fn recalculate_all_portfolio_history(db: State<'_, Database>) -> Result<BackfillResult> {
    log::info!("[RECALC] Starting full portfolio recalculation from oldest snapshot...");

    // Get the oldest snapshot date
    let (min_date, _, _) = get_snapshot_date_info(&db)?;

    let from_timestamp = match min_date {
        Some(date) => date,
        None => {
            // No snapshots exist, nothing to recalculate
            return Ok(BackfillResult {
                days_processed: 0,
                total_days: 0,
                completed: true,
                message: "No existing snapshots to recalculate".to_string(),
            });
        }
    };

    // Trigger full recalculation
    recalculate_history_from_date(&db, from_timestamp).await?;

    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;
    let from_day = (from_timestamp / 86400) * 86400;
    let days_recalculated = ((today_start - from_day) / 86400) as i32;

    Ok(BackfillResult {
        days_processed: days_recalculated,
        total_days: days_recalculated,
        completed: true,
        message: format!(
            "Recalculated {} days of portfolio history",
            days_recalculated
        ),
    })
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

    // Call the existing recalculation function
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

    let coingecko_id = match coingecko_id {
        Some(id) if !id.is_empty() => id,
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
    };

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

    recalculate_crypto_ticker_history(&db, &ticker, &coingecko_id, effective_from).await?;

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

// ============================================================================
// Currency Breakdown Backfill
// ============================================================================

/// Derive an asset class's native breakdown for one day from its per-ticker
/// value-history table, along with the CZK sum of the same rows (used to check
/// that the day's coverage matches the stored total).
fn derive_breakdown_from_value_history(
    conn: &rusqlite::Connection,
    table: &'static str,
    day_start: i64,
    day_end: i64,
) -> Result<(HashMap<String, f64>, f64)> {
    let mut czk_sum = 0.0;
    let breakdown: HashMap<String, f64> = conn
        .prepare(&format!(
            "SELECT currency, SUM(CAST(quantity AS REAL) * CAST(price AS REAL)),
                    SUM(CAST(value_czk AS REAL))
             FROM {table}
             WHERE recorded_at >= ?1 AND recorded_at < ?2
             GROUP BY currency"
        ))?
        .query_map(rusqlite::params![day_start, day_end], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, f64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?
        .filter_map(|r| r.ok())
        .map(|(currency, native, czk)| {
            czk_sum += czk;
            (currency, native)
        })
        .collect();
    Ok((breakdown, czk_sum))
}

/// Backfill and repair native currency breakdowns on portfolio_metrics_history.
///
/// Runs on every startup (SyncProvider). For each row, every `*_by_currency`
/// column is checked against its stored CZK total at that day's rates:
/// - Stocks/crypto: an empty or inconsistent breakdown is rebuilt from
///   stock_value_history / crypto_value_history, and written only when that
///   history covers the stored total (a partial fragment stays '{}' so the
///   frontend renders the row via its CZK total).
/// - Other asset classes: an empty or inconsistent breakdown becomes
///   {"CZK": <czk_total>} (no per-day native history is available).
/// - Breakdowns that cannot be valued at the day's rates are kept as-is.
///
/// Returns the number of rows updated. Idempotent — consistent rows are untouched.
#[tauri::command]
pub async fn backfill_currency_breakdowns(db: State<'_, Database>) -> Result<i32> {
    use crate::services::currency::{
        breakdown_covers_total, get_rates_for_date_range, needs_breakdown_repair,
        resolve_rates_for_day_from_range,
    };

    db.with_conn(|conn| {
        struct SnapshotRow {
            id: String,
            recorded_at: i64,
            total_savings: f64,
            total_bonds: f64,
            total_re: f64,
            total_loans: f64,
            total_other: f64,
            total_investments: f64,
            total_crypto: f64,
            savings_json: String,
            bonds_json: String,
            re_json: String,
            loans_json: String,
            other_json: String,
            inv_json: String,
            crypto_json: String,
        }

        let rows: Vec<SnapshotRow> = conn
            .prepare(
                "SELECT id, recorded_at,
                        CAST(total_savings AS REAL),
                        CAST(total_bonds AS REAL),
                        CAST(total_real_estate_personal AS REAL) + CAST(total_real_estate_investment AS REAL),
                        CAST(total_loans_principal AS REAL),
                        CAST(total_other_assets AS REAL),
                        CAST(total_investments AS REAL),
                        CAST(total_crypto AS REAL),
                        savings_by_currency, bonds_by_currency, real_estate_by_currency,
                        loans_by_currency, other_assets_by_currency,
                        investments_by_currency, crypto_by_currency
                 FROM portfolio_metrics_history",
            )?
            .query_map([], |row| {
                Ok(SnapshotRow {
                    id: row.get(0)?,
                    recorded_at: row.get(1)?,
                    total_savings: row.get(2)?,
                    total_bonds: row.get(3)?,
                    total_re: row.get(4)?,
                    total_loans: row.get(5)?,
                    total_other: row.get(6)?,
                    total_investments: row.get(7)?,
                    total_crypto: row.get(8)?,
                    savings_json: row.get(9)?,
                    bonds_json: row.get(10)?,
                    re_json: row.get(11)?,
                    loans_json: row.get(12)?,
                    other_json: row.get(13)?,
                    inv_json: row.get(14)?,
                    crypto_json: row.get(15)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        if rows.is_empty() {
            return Ok(0);
        }

        // One prefetch of the whole FX timeseries for the rows' span (with the
        // 10-day walk-back margin) instead of per-row queries.
        let min_day = rows.iter().map(|r| r.recorded_at).min().unwrap_or(0);
        let max_day = rows.iter().map(|r| r.recorded_at).max().unwrap_or(0);
        let rates_by_day = get_rates_for_date_range(conn, min_day - 10 * 86400, max_day);

        let mut repaired = 0i32;

        for row in rows {
            let day_start = (row.recorded_at / 86400) * 86400;
            let day_end = day_start + 86400;
            let rates = resolve_rates_for_day_from_range(&rates_by_day, day_start);

            // Static classes: repair to {"CZK": total} (their CZK total is the
            // only per-day record available).
            let fix_static = |json: &str, total: f64| -> Option<String> {
                if needs_breakdown_repair(json, total, rates) {
                    let target = format!("{{\"CZK\":{total}}}");
                    if target != json {
                        return Some(target);
                    }
                }
                None
            };
            let new_savings = fix_static(&row.savings_json, row.total_savings);
            let new_bonds = fix_static(&row.bonds_json, row.total_bonds);
            let new_re = fix_static(&row.re_json, row.total_re);
            let new_loans = fix_static(&row.loans_json, row.total_loans);
            let new_other = fix_static(&row.other_json, row.total_other);

            // Stocks/crypto: rebuild from per-ticker history, gated on coverage.
            let fix_valued = |json: &str,
                                  total: f64,
                                  table: &'static str|
             -> Result<Option<String>> {
                if !needs_breakdown_repair(json, total, rates) {
                    return Ok(None);
                }
                let (derived, czk_sum) =
                    derive_breakdown_from_value_history(conn, table, day_start, day_end)?;
                let target = if breakdown_covers_total(czk_sum, total) {
                    breakdown_json(&derived)
                } else {
                    "{}".to_string()
                };
                Ok((target != json).then_some(target))
            };
            let new_inv = fix_valued(&row.inv_json, row.total_investments, "stock_value_history")?;
            let new_crypto = fix_valued(&row.crypto_json, row.total_crypto, "crypto_value_history")?;

            let changed = [
                &new_savings,
                &new_bonds,
                &new_re,
                &new_loans,
                &new_other,
                &new_inv,
                &new_crypto,
            ]
            .iter()
            .any(|c| c.is_some());
            if !changed {
                continue;
            }

            conn.execute(
                "UPDATE portfolio_metrics_history
                 SET investments_by_currency = ?2,
                     crypto_by_currency = ?3,
                     savings_by_currency = ?4,
                     bonds_by_currency = ?5,
                     real_estate_by_currency = ?6,
                     loans_by_currency = ?7,
                     other_assets_by_currency = ?8
                 WHERE id = ?1",
                rusqlite::params![
                    row.id,
                    new_inv.unwrap_or(row.inv_json),
                    new_crypto.unwrap_or(row.crypto_json),
                    new_savings.unwrap_or(row.savings_json),
                    new_bonds.unwrap_or(row.bonds_json),
                    new_re.unwrap_or(row.re_json),
                    new_loans.unwrap_or(row.loans_json),
                    new_other.unwrap_or(row.other_json),
                ],
            )?;

            repaired += 1;
        }

        log::info!("[BACKFILL] Currency breakdowns: repaired {} rows", repaired);
        Ok(repaired)
    })
}

// ============================================================================
// Historical Recalculation for Retrospective Transactions
// ============================================================================

/// Calculate stock quantity at a specific point in time by summing transactions
fn get_stock_quantity_at_date(
    conn: &rusqlite::Connection,
    ticker: &str,
    date_timestamp: i64,
) -> f64 {
    let result: rusqlite::Result<f64> = conn.query_row(
        "SELECT COALESCE(
            SUM(CASE WHEN type = 'buy' THEN CAST(quantity AS REAL) ELSE -CAST(quantity AS REAL) END),
            0.0
        ) FROM investment_transactions 
        WHERE ticker = ?1 AND transaction_date <= ?2",
        rusqlite::params![ticker, date_timestamp],
        |row| row.get(0),
    );
    result.unwrap_or(0.0).max(0.0)
}

/// Calculate crypto quantity at a specific point in time by summing transactions
fn get_crypto_quantity_at_date(
    conn: &rusqlite::Connection,
    ticker: &str,
    date_timestamp: i64,
) -> f64 {
    let result: rusqlite::Result<f64> = conn.query_row(
        "SELECT COALESCE(
            SUM(CASE WHEN type = 'buy' THEN CAST(quantity AS REAL) ELSE -CAST(quantity AS REAL) END),
            0.0
        ) FROM crypto_transactions 
        WHERE ticker = ?1 AND transaction_date <= ?2",
        rusqlite::params![ticker, date_timestamp],
        |row| row.get(0),
    );
    result.unwrap_or(0.0).max(0.0)
}

/// Calculate other asset quantity at a specific point in time by summing transactions
fn get_other_asset_quantity_at_date(
    conn: &rusqlite::Connection,
    asset_id: &str,
    date_timestamp: i64,
) -> f64 {
    let result: rusqlite::Result<f64> = conn.query_row(
        "SELECT COALESCE(
            SUM(CASE WHEN type = 'buy' THEN CAST(quantity AS REAL) ELSE -CAST(quantity AS REAL) END),
            0.0
        ) FROM other_asset_transactions 
        WHERE asset_id = ?1 AND transaction_date <= ?2",
        rusqlite::params![asset_id, date_timestamp],
        |row| row.get(0),
    );
    result.unwrap_or(0.0).max(0.0)
}

/// Per-currency native amounts of one asset class (currency -> amount).
type CurrencyBreakdown = HashMap<String, f64>;

/// Native breakdowns of the transaction-driven classes for a specific day:
/// quantities from the transaction ledgers at that date, prices from the
/// prefetched historical price maps. CZK totals are derived by the writer.
fn historical_transaction_breakdowns(
    db: &Database,
    day_timestamp: i64,
    stock_prices: &HashMap<String, Vec<HistoricalPrice>>,
    crypto_prices: &HashMap<String, Vec<HistoricalPrice>>,
) -> Result<(CurrencyBreakdown, CurrencyBreakdown, CurrencyBreakdown)> {
    db.with_conn(|conn| {
        // Investments using HISTORICAL quantities and prices
        let mut investments_by_currency: HashMap<String, f64> = HashMap::new();
        let mut inv_stmt = conn.prepare("SELECT DISTINCT ticker FROM stock_investments")?;
        let tickers: Vec<String> = inv_stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        for ticker in tickers {
            let qty = get_stock_quantity_at_date(conn, &ticker, day_timestamp);
            if qty > 0.0 {
                if let Some(prices) = stock_prices.get(&ticker) {
                    if let Some(price) = find_closest_price(prices, day_timestamp) {
                        *investments_by_currency
                            .entry(price.currency.clone())
                            .or_insert(0.0) += price.price * qty;
                    }
                }
            }
        }

        // Crypto using HISTORICAL quantities and prices
        let mut crypto_by_currency: HashMap<String, f64> = HashMap::new();
        let mut crypto_stmt = conn.prepare("SELECT DISTINCT ticker FROM crypto_investments")?;
        let crypto_tickers: Vec<String> = crypto_stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        for ticker in crypto_tickers {
            let qty = get_crypto_quantity_at_date(conn, &ticker, day_timestamp);
            if qty > 0.0 {
                if let Some(prices) = crypto_prices.get(&ticker) {
                    if let Some(price) = find_closest_price(prices, day_timestamp) {
                        *crypto_by_currency
                            .entry(price.currency.clone())
                            .or_insert(0.0) += price.price * qty;
                    }
                }
            }
        }

        // Other assets using HISTORICAL quantities (current prices — no history)
        let mut other_assets_by_currency: HashMap<String, f64> = HashMap::new();
        let mut other_stmt = conn.prepare("SELECT id, market_price, currency FROM other_assets")?;
        let other_rows: Vec<(String, f64, String)> = other_stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    parse_money(&row.get::<_, String>(1)?, 0.0, "other_assets.market_price"),
                    row.get::<_, String>(2)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        for (asset_id, price, currency) in other_rows {
            let qty = get_other_asset_quantity_at_date(conn, &asset_id, day_timestamp);
            if qty > 0.0 {
                *other_assets_by_currency
                    .entry(currency.clone())
                    .or_insert(0.0) += qty * price;
            }
        }

        Ok((
            investments_by_currency,
            crypto_by_currency,
            other_assets_by_currency,
        ))
    })
}

/// Rewrite one recalculated day. Live rows keep their recorded static classes
/// and only the transaction-driven classes change (a backdated transaction
/// cannot rewrite an authentic savings balance); reconstructed or missing days
/// are rebuilt in full through the single writer.
fn write_recalculated_day(
    db: &Database,
    day_timestamp: i64,
    investments: HashMap<String, f64>,
    crypto: HashMap<String, f64>,
    other_assets: HashMap<String, f64>,
) -> Result<()> {
    db.with_conn(|conn| {
        let day_start = (day_timestamp / 86400) * 86400;
        let existing_source: Option<String> = conn
            .query_row(
                "SELECT source FROM portfolio_metrics_history
                 WHERE recorded_at >= ?1 AND recorded_at < ?2 LIMIT 1",
                [day_start, day_start + 86400],
                |row| row.get(0),
            )
            .ok();

        if existing_source.as_deref() == Some("live") {
            return portfolio_history::update_classes(
                conn,
                day_start,
                &[
                    (AssetClassKind::Investments, investments),
                    (AssetClassKind::Crypto, crypto),
                    (AssetClassKind::OtherAssets, other_assets),
                ],
            );
        }

        let statics = match portfolio_history::carried_statics_from_nearest_live(conn, day_start)? {
            Some(statics) => statics,
            None => current_static_breakdowns(conn)?,
        };
        let breakdowns = ClassBreakdowns {
            savings: statics.savings,
            investments,
            crypto,
            bonds: statics.bonds,
            real_estate_personal: statics.real_estate_personal,
            real_estate_investment: statics.real_estate_investment,
            loans: statics.loans,
            other_assets,
        };
        portfolio_history::upsert_snapshot(conn, day_start, SnapshotSource::Backfill, &breakdowns)
    })
}

/// Recalculate portfolio history from a given date
/// This is called when a retrospective transaction is created/deleted
/// Will create new snapshots for dates before the oldest existing snapshot
pub async fn recalculate_history_from_date(db: &Database, from_timestamp: i64) -> Result<()> {
    log::info!(
        "[RECALC] Starting historical recalculation from timestamp {}",
        from_timestamp
    );

    // Get the oldest snapshot date (if any)
    let (min_date, _, _) = get_snapshot_date_info(db)?;
    let oldest_snapshot = min_date.map(|d| (d / 86400) * 86400);

    // Start recalculation from the transaction date
    // This allows creating new snapshots for dates before any existing snapshots
    let from_day = (from_timestamp / 86400) * 86400;
    let recalc_start = from_day;

    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;

    // Log whether we're extending history backwards
    if let Some(oldest) = oldest_snapshot {
        if from_day < oldest {
            log::info!(
                "[RECALC] Extending history backwards from {} to {} (creating {} new days)",
                oldest,
                from_day,
                (oldest - from_day) / 86400
            );
        }
    } else {
        log::info!(
            "[RECALC] No existing snapshots, will create new history from {}",
            from_day
        );
    }

    // Build list of days to recalculate
    let mut days_to_recalc: Vec<i64> = Vec::new();
    let mut check_day = recalc_start;
    while check_day <= today_start {
        days_to_recalc.push(check_day);
        check_day += 86400;
    }

    if days_to_recalc.is_empty() {
        log::info!("[RECALC] No days to recalculate");
        return Ok(());
    }

    log::info!(
        "[RECALC] Recalculating {} days from {} to {}",
        days_to_recalc.len(),
        recalc_start,
        today_start
    );

    // Get tickers for fetching historical prices
    let stock_tickers = get_stock_tickers(db)?;
    let crypto_id_map = get_crypto_id_map(db)?;

    // Fetch historical prices for the date range
    let fetch_start = recalc_start;
    let fetch_end = today_start;

    let stock_prices = if !stock_tickers.is_empty() {
        get_historical_stock_prices_yahoo(&stock_tickers, fetch_start, fetch_end).await?
    } else {
        HashMap::new()
    };

    let api_keys = get_api_keys(db)?;
    let crypto_prices = if !crypto_id_map.is_empty() {
        get_historical_crypto_prices_coingecko(
            api_keys.coingecko.as_deref(),
            &crypto_id_map,
            fetch_start,
            fetch_end,
        )
        .await?
    } else {
        HashMap::new()
    };

    // Recalculate each day
    let mut days_processed = 0;
    for day_timestamp in days_to_recalc {
        let (investments, crypto, other_assets) =
            historical_transaction_breakdowns(db, day_timestamp, &stock_prices, &crypto_prices)?;
        write_recalculated_day(db, day_timestamp, investments, crypto, other_assets)?;
        days_processed += 1;
    }

    log::info!(
        "[RECALC] Historical recalculation complete! Recalculated {} snapshots",
        days_processed
    );

    Ok(())
}

/// Asset type for targeted recalculation
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AssetType {
    Stocks,
    Crypto,
    OtherAssets,
    Savings,
}

/// Calculate historical value for a specific asset type on a given day
/// Calculate historical value for a specific asset type on a given day
fn calculate_asset_breakdown_for_day(
    db: &Database,
    day_timestamp: i64,
    asset_type: AssetType,
    stock_prices: Option<&HashMap<String, Vec<HistoricalPrice>>>,
    crypto_prices: Option<&HashMap<String, Vec<HistoricalPrice>>>,
) -> Result<HashMap<String, f64>> {
    match asset_type {
        AssetType::Stocks => {
            let empty_map = HashMap::new();
            let prices_map = stock_prices.unwrap_or(&empty_map);
            let stock_tickers = get_stock_tickers(db)?;

            db.with_conn(move |conn| {
                let mut by_currency: HashMap<String, f64> = HashMap::new();
                for ticker in &stock_tickers {
                    let qty = get_stock_quantity_at_date(conn, ticker, day_timestamp);
                    if qty > 0.0 {
                        if let Some(prices) = prices_map.get(ticker) {
                            if let Some(price) = find_closest_price(prices, day_timestamp) {
                                *by_currency.entry(price.currency.clone()).or_insert(0.0) +=
                                    price.price * qty;
                            }
                        }
                    }
                }
                Ok(by_currency)
            })
        }
        AssetType::Crypto => {
            let empty_map = HashMap::new();
            let prices_map = crypto_prices.unwrap_or(&empty_map);

            db.with_conn(move |conn| {
                let mut by_currency: HashMap<String, f64> = HashMap::new();
                let mut stmt = conn.prepare("SELECT DISTINCT ticker FROM crypto_investments")?;
                let crypto_tickers: Vec<String> = stmt
                    .query_map([], |row| row.get(0))?
                    .filter_map(|r| r.ok())
                    .collect();

                for ticker in crypto_tickers {
                    let qty = get_crypto_quantity_at_date(conn, &ticker, day_timestamp);
                    if qty > 0.0 {
                        if let Some(prices) = prices_map.get(&ticker) {
                            if let Some(price) = find_closest_price(prices, day_timestamp) {
                                *by_currency.entry(price.currency.clone()).or_insert(0.0) +=
                                    price.price * qty;
                            }
                        }
                    }
                }
                Ok(by_currency)
            })
        }
        AssetType::OtherAssets => db.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT id, market_price, currency FROM other_assets")?;
            let mut by_currency: HashMap<String, f64> = HashMap::new();
            let rows: Vec<(String, f64, String)> = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?.parse().unwrap_or(0.0),
                        row.get::<_, String>(2)?,
                    ))
                })?
                .filter_map(|r| r.ok())
                .collect();

            for (asset_id, price, currency) in rows {
                let qty = get_other_asset_quantity_at_date(conn, &asset_id, day_timestamp);
                if qty > 0.0 {
                    // The estimate that applied on that day (valuation log, migration 014),
                    // falling back to today's price for assets without one
                    let price_at_day = crate::services::valuations::value_at_day(
                        conn,
                        crate::services::valuations::ValuationKind::OtherAsset,
                        &asset_id,
                        day_timestamp,
                    )?
                    .unwrap_or(price);
                    *by_currency.entry(currency.clone()).or_insert(0.0) += qty * price_at_day;
                }
            }
            Ok(by_currency)
        }),
        AssetType::Savings => db.with_conn(|conn| {
            // Current balances — bank accounts have no per-day history.
            let mut by_currency: HashMap<String, f64> = HashMap::new();
            let mut bank_stmt = conn.prepare(
                "SELECT balance, currency FROM bank_accounts WHERE exclude_from_balance = 0",
            )?;
            for row in bank_stmt
                .query_map([], |row| {
                    let balance: f64 = row.get::<_, String>(0)?.parse().unwrap_or(0.0);
                    let currency: String = row.get(1)?;
                    Ok((balance, currency))
                })?
                .filter_map(|r| r.ok())
            {
                *by_currency.entry(row.1).or_insert(0.0) += row.0;
            }
            Ok(by_currency)
        }),
    }
}

impl AssetType {
    fn class_kind(self) -> AssetClassKind {
        match self {
            AssetType::Stocks => AssetClassKind::Investments,
            AssetType::Crypto => AssetClassKind::Crypto,
            AssetType::OtherAssets => AssetClassKind::OtherAssets,
            AssetType::Savings => AssetClassKind::Savings,
        }
    }
}

/// Recalculate only a specific asset type's history from a given date
/// If the date is before existing snapshots, creates full snapshots for those days
pub async fn recalculate_asset_history_from_date(
    db: &Database,
    from_timestamp: i64,
    asset_type: AssetType,
) -> Result<()> {
    log::info!(
        "[RECALC] Starting {:?} recalculation from timestamp {}",
        asset_type,
        from_timestamp
    );

    // Get the oldest snapshot date (if any)
    let (min_date, _, _) = get_snapshot_date_info(db)?;
    let oldest_snapshot = min_date.map(|d| (d / 86400) * 86400);

    let from_day = (from_timestamp / 86400) * 86400;
    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;

    // If from_day is before oldest_snapshot (or no snapshots exist),
    // we need to create full snapshots using recalculate_history_from_date
    let needs_full_recalc = match oldest_snapshot {
        Some(oldest) => from_day < oldest,
        None => true,
    };

    if needs_full_recalc {
        log::info!(
            "[RECALC] Transaction date precedes existing snapshots, using full recalculation"
        );
        // Fall back to full recalculation which creates complete snapshots
        return recalculate_history_from_date(db, from_timestamp).await;
    }

    // If we're only updating existing snapshots, proceed with asset-specific update
    let recalc_start = from_day;

    // Build list of days to recalculate
    let mut days_to_recalc: Vec<i64> = Vec::new();
    let mut check_day = recalc_start;
    while check_day <= today_start {
        days_to_recalc.push(check_day);
        check_day += 86400;
    }

    if days_to_recalc.is_empty() {
        log::info!("[RECALC] No days to recalculate");
        return Ok(());
    }

    log::info!(
        "[RECALC] Recalculating {:?} for {} days",
        asset_type,
        days_to_recalc.len()
    );

    // Fetch necessary prices upfront
    let stock_prices = if asset_type == AssetType::Stocks {
        // Get tickers for fetching historical prices
        let stock_tickers = get_stock_tickers(db)?;
        if !stock_tickers.is_empty() {
            Some(
                get_historical_stock_prices_yahoo(&stock_tickers, recalc_start, today_start)
                    .await?,
            )
        } else {
            None
        }
    } else {
        None
    };

    let crypto_prices = if asset_type == AssetType::Crypto {
        let crypto_id_map = get_crypto_id_map(db)?;
        if !crypto_id_map.is_empty() {
            let api_keys = get_api_keys(db)?;
            Some(
                get_historical_crypto_prices_coingecko(
                    api_keys.coingecko.as_deref(),
                    &crypto_id_map,
                    recalc_start,
                    today_start,
                )
                .await?,
            )
        } else {
            None
        }
    } else {
        None
    };

    // Recalculate each day: the class total and its breakdown change together,
    // derived at the day's rates by the single writer. Days without a snapshot
    // are left for the full backfill to create.
    for day_timestamp in days_to_recalc {
        let by_currency = calculate_asset_breakdown_for_day(
            db,
            day_timestamp,
            asset_type,
            stock_prices.as_ref(),
            crypto_prices.as_ref(), // Passed as Optional references
        )?;
        db.with_conn(|conn| {
            portfolio_history::update_classes(
                conn,
                day_timestamp,
                &[(asset_type.class_kind(), by_currency.clone())],
            )
        })?;
    }

    log::info!("[RECALC] {:?} recalculation complete!", asset_type);

    Ok(())
}

/// Earliest date the retrospective recalculation walks back to. A typo'd
/// transaction date (year 202 instead of 2024) must not trigger a
/// hundreds-of-thousands-of-days rebuild holding the global DB mutex.
const RECALC_FLOOR_TS: i64 = 946_684_800; // 2000-01-01

/// Trigger historical recalculation for a specific asset type
/// Only triggers if transaction_date is before today
pub async fn trigger_historical_recalculation_for_asset(
    db: &Database,
    transaction_date: i64,
    asset_type: AssetType,
) -> Result<()> {
    let transaction_date = transaction_date.max(RECALC_FLOOR_TS);
    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;
    let tx_day = (transaction_date / 86400) * 86400;

    // Only recalculate if transaction is historical (before today)
    if tx_day < today_start {
        log::info!(
            "[RECALC] Transaction date {} is historical, triggering {:?} recalculation",
            tx_day,
            asset_type
        );
        recalculate_asset_history_from_date(db, transaction_date, asset_type).await?;
    }

    Ok(())
}

/// Legacy function - recalculates all asset types (kept for backward compatibility)
pub async fn trigger_historical_recalculation(db: &Database, transaction_date: i64) -> Result<()> {
    let transaction_date = transaction_date.max(RECALC_FLOOR_TS);
    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;
    let tx_day = (transaction_date / 86400) * 86400;

    // Only recalculate if transaction is historical (before today)
    if tx_day < today_start {
        log::info!(
            "[RECALC] Transaction date {} is historical, triggering recalculation",
            tx_day
        );
        recalculate_history_from_date(db, transaction_date).await?;
    }

    Ok(())
}

// ============================================================================
// Per-Ticker Historical Recalculation
// ============================================================================

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

    crate::services::history_recalc::rebuild_stock_ticker(
        db,
        ticker,
        from_timestamp,
        today_start,
        find_closest_price,
    )
    .await?;

    log::debug!("[RECALC] Ticker {} history recalculation complete!", ticker);

    Ok(())
}

/// Recalculate crypto value history for a single ticker from a given date
pub async fn recalculate_crypto_ticker_history(
    db: &Database,
    ticker: &str,
    coingecko_id: &str,
    from_timestamp: i64,
) -> Result<()> {
    log::debug!(
        "[RECALC] Starting crypto ticker-specific recalculation for {} from {}",
        ticker,
        from_timestamp
    );

    let from_day = (from_timestamp / 86400) * 86400;
    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;

    // Build list of days to recalculate
    let mut days_to_recalc: Vec<i64> = Vec::new();
    let mut check_day = from_day;
    while check_day <= today_start {
        days_to_recalc.push(check_day);
        check_day += 86400;
    }

    if days_to_recalc.is_empty() {
        log::debug!("[RECALC] No days to recalculate for crypto {}", ticker);
        return Ok(());
    }

    // Fetch historical prices for just this crypto
    // Note: id_to_ticker map uses coingecko_id as key, ticker as value
    let mut crypto_map = HashMap::new();
    crypto_map.insert(coingecko_id.to_string(), ticker.to_string());

    let api_keys = crate::services::price_api::get_api_keys(db)?;
    let crypto_prices = crate::services::price_api::get_historical_crypto_prices_coingecko(
        api_keys.coingecko.as_deref(),
        &crypto_map,
        from_day,
        today_start,
    )
    .await?;

    // Get the last known price before the recalculation start date to seed the fallback
    // This ensures continuity if the API fails for the first few days
    let mut last_known_price: Option<(f64, String)> = db.with_conn(|conn| {
        let res = conn
            .query_row(
                "SELECT price, currency FROM crypto_value_history 
             WHERE ticker = ?1 AND recorded_at < ?2 
             ORDER BY recorded_at DESC LIMIT 1",
                rusqlite::params![ticker, from_day],
                |row| {
                    let price: f64 = row.get::<_, String>(0)?.parse().unwrap_or(0.0);
                    let currency: String = row.get(1)?;
                    Ok((price, currency))
                },
            )
            .ok();
        Ok(res)
    })?;

    // If no history exists, try to use the current price as a fallback seed
    // This is useful if we're backfilling for the first time and API fails immediately
    if last_known_price.is_none() {
        last_known_price = db.with_conn(|conn| {
            let res = conn
                .query_row(
                    "SELECT price, currency FROM crypto_prices WHERE symbol = ?1",
                    rusqlite::params![ticker],
                    |row| {
                        let price: f64 = row.get::<_, String>(0)?.parse().unwrap_or(0.0);
                        let currency: String = row.get(1)?;
                        Ok((price, currency))
                    },
                )
                .ok();
            Ok(res)
        })?;
    }

    // Calculate and store value for each day
    let ticker_clone = ticker.to_string();
    db.with_conn(move |conn| {
        for day_timestamp in days_to_recalc {
            let quantity = get_crypto_quantity_at_date(conn, &ticker_clone, day_timestamp);

            if quantity <= 0.0 {
                conn.execute(
                    "DELETE FROM crypto_value_history WHERE ticker = ?1 AND recorded_at = ?2",
                    rusqlite::params![ticker_clone, day_timestamp],
                )?;
                continue;
            }

            // Determine price to use
            let (price, currency) = if let Some(prices) = crypto_prices.get(&ticker_clone) {
                if let Some(hp) = find_closest_price(prices, day_timestamp) {
                    // Found price in API data, update last known
                    last_known_price = Some((hp.price, hp.currency.clone()));
                    (hp.price, hp.currency.clone())
                } else if let Some((last_price, last_currency)) = &last_known_price {
                    // Gap in API data, use last known
                    (*last_price, last_currency.clone())
                } else {
                    // No API data and no last known, skip
                    continue;
                }
            } else if let Some((last_price, last_currency)) = &last_known_price {
                 // No API data keys at all for this ticker, use last known
                 (*last_price, last_currency.clone())
            } else {
                continue;
            };

            // Value at that day's rates (falls back to current rates when the
            // FX timeseries has no snapshot near that day).
            let value_czk =
                crate::services::currency::convert_to_czk_at(conn, quantity * price, &currency, day_timestamp);

            let id = Uuid::new_v4().to_string();
            conn.execute(
                "INSERT INTO crypto_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(ticker, recorded_at) DO UPDATE SET
                     value_czk = excluded.value_czk,
                     quantity = excluded.quantity,
                     price = excluded.price,
                     currency = excluded.currency",
                rusqlite::params![
                    id,
                    ticker_clone,
                    day_timestamp,
                    value_czk.to_string(),
                    quantity.to_string(),
                    price.to_string(),
                    currency,
                ],
            )?;
        }
        Ok(())
    })?;

    log::debug!(
        "[RECALC] Crypto ticker {} history recalculation complete!",
        ticker
    );

    Ok(())
}

/// Update only stock investments in portfolio_metrics_history from stock_value_history
/// Ensures all stock tickers have their history populated before aggregating
pub async fn update_portfolio_stocks_from_ticker_table(
    db: &Database,
    from_timestamp: i64,
) -> Result<()> {
    let from_day = (from_timestamp / 86400) * 86400;
    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;

    // Get all stock tickers that have holdings
    let stock_tickers = get_stock_tickers(db)?;

    if stock_tickers.is_empty() {
        return Ok(());
    }

    // For each ticker, check if it has history data for the date range
    // If not, backfill it from Yahoo API
    for ticker in &stock_tickers {
        let has_data = db.with_conn(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM stock_value_history 
                     WHERE ticker = ?1 AND recorded_at >= ?2 AND recorded_at <= ?3",
                    rusqlite::params![ticker, from_day, today_start],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            Ok(count > 0)
        })?;

        if !has_data {
            log::debug!(
                "[BACKFILL] Ticker {} missing history data for date range, fetching...",
                ticker
            );
            // Backfill this ticker's history
            recalculate_stock_ticker_history(db, ticker, from_timestamp).await?;
        }
    }

    // Aggregate the populated stock_value_history table into the snapshot rows
    // up to (not including) today: today's row is owned by the live snapshot
    //, and sold tickers stop contributing on their sale day. Chunked
    // so no single lock hold covers the whole range.
    crate::services::history_recalc::aggregate_stocks(db, from_day, today_start).await
}

/// Update only crypto in portfolio_metrics_history from crypto_value_history
/// Ensures all crypto tickers have their history populated before aggregating
pub async fn update_portfolio_crypto_from_ticker_table(
    db: &Database,
    from_timestamp: i64,
) -> Result<()> {
    let from_day = (from_timestamp / 86400) * 86400;
    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;

    // Get all crypto tickers that have holdings
    let crypto_id_map = get_crypto_id_map(db)?;

    if crypto_id_map.is_empty() {
        return Ok(());
    }

    // For each ticker, check if it has history data for the date range
    // If not, backfill it from CoinGecko API
    for (coingecko_id, ticker) in &crypto_id_map {
        let has_data = db.with_conn(|conn| {
            let count: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM crypto_value_history 
                     WHERE ticker = ?1 AND recorded_at >= ?2 AND recorded_at <= ?3",
                    rusqlite::params![ticker, from_day, today_start],
                    |row| row.get(0),
                )
                .unwrap_or(0);
            Ok(count > 0)
        })?;

        if !has_data {
            log::debug!(
                "[BACKFILL] Crypto {} missing history data for date range, fetching...",
                ticker
            );
            // Backfill this ticker's history
            recalculate_crypto_ticker_history(db, ticker, coingecko_id, from_timestamp).await?;
        }
    }

    // Aggregate crypto_value_history into the snapshot rows up to (not
    // including) today — see the stocks variant above.
    db.with_conn(|conn| {
        crate::services::portfolio_history::aggregate_ticker_history(
            conn,
            crate::services::portfolio_history::AssetClassKind::Crypto,
            from_day,
            today_start,
        )
    })
}

/// Update portfolio_metrics_history by summing values from per-ticker history tables
/// This updates BOTH stocks and crypto - use only when both asset types need updating
pub async fn update_portfolio_history_from_ticker_tables(
    db: &Database,
    from_timestamp: i64,
) -> Result<()> {
    update_portfolio_stocks_from_ticker_table(db, from_timestamp).await?;
    update_portfolio_crypto_from_ticker_table(db, from_timestamp).await?;
    Ok(())
}

/// Trigger historical recalculation for a specific stock ticker
/// Only triggers if transaction_date is before today
pub async fn trigger_historical_recalculation_for_stock_ticker(
    db: &Database,
    transaction_date: i64,
    ticker: &str,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;
    let tx_day = (transaction_date / 86400) * 86400;

    // Only recalculate if transaction is historical (before today)
    if tx_day < today_start {
        log::debug!(
            "[RECALC] Stock transaction for {} on {} is historical, triggering ticker recalculation",
            ticker, tx_day
        );

        // Recalculate just this ticker's history
        recalculate_stock_ticker_history(db, ticker, transaction_date).await?;

        // Update only stock values in aggregate portfolio history
        update_portfolio_stocks_from_ticker_table(db, transaction_date).await?;
    }

    Ok(())
}

/// Queue background rebuilds of stock tickers' history and return at
/// once: the write command that called this has already stored its rows, the
/// ticker rebuild and portfolio aggregate follow on a worker that holds the
/// database lock in short chunks. Progress reaches the UI as
/// `history-recalculation` events (`{ ticker, status }`), and
/// `recalculation-complete` fires when a batch is done. Jobs dated today or
/// later are ignored (nothing historical to repair).
pub fn schedule_stock_history_rebuild(
    app: &AppHandle,
    db: &Database,
    recalc: &crate::services::history_recalc::HistoryRecalc,
    jobs: Vec<(String, i64)>,
) {
    use crate::services::history_recalc::{self, Notice};

    let handle = app.clone();
    let notify: history_recalc::Notifier = std::sync::Arc::new(move |notice| match notice {
        Notice::Status(event) => {
            handle.emit(history_recalc::EVENT_STATUS, event).ok();
        }
        Notice::BatchComplete => {
            handle.emit(history_recalc::EVENT_COMPLETE, ()).ok();
        }
    });
    history_recalc::schedule(recalc, db, notify, find_closest_price, jobs);
}

/// Trigger historical recalculation for a specific crypto ticker
pub async fn trigger_historical_recalculation_for_crypto_ticker(
    db: &Database,
    transaction_date: i64,
    ticker: &str,
    coingecko_id: &str,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let today_start = (now / 86400) * 86400;
    let tx_day = (transaction_date / 86400) * 86400;

    if tx_day < today_start {
        log::debug!(
            "[RECALC] Crypto transaction for {} on {} is historical, triggering ticker recalculation",
            ticker, tx_day
        );

        // A historical transaction may predate the FX timeseries — extend it
        // first so the recalc (and cost basis) values days at real rates.
        crate::services::fx_backfill::ensure_fx_coverage_best_effort(db).await;

        recalculate_crypto_ticker_history(db, ticker, coingecko_id, transaction_date).await?;
        // Update only crypto values in aggregate portfolio history
        update_portfolio_crypto_from_ticker_table(db, transaction_date).await?;
    }

    Ok(())
}

/// Resolve a coingecko id for a ticker that arrived without one — the MCP
/// layer only knows the id for coins it created in this same call; existing
/// holdings need a DB lookup.
async fn resolve_coingecko_id(db: &Database, ticker: &str) -> Option<String> {
    let ticker = ticker.to_string();
    db.with_conn(move |conn| {
        use rusqlite::OptionalExtension;
        conn.query_row(
            "SELECT coingecko_id FROM crypto_investments WHERE ticker = ?1",
            rusqlite::params![ticker],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(Into::into)
    })
    .ok()
    .flatten()
}

/// Trigger historical recalculation after an MCP bulk import that contained
/// retroactive transactions. Thin wrapper over the same internal helpers the
/// UI create/delete commands call. Recalculation errors are best-effort (the
/// import itself already succeeded and must not be failed by this) but are
/// logged so a Yahoo/CoinGecko failure doesn't vanish without a trace.
/// Emits `recalculation-complete` when done, mirroring the UI import flows
/// (commands/investments.rs, commands/crypto.rs) so PortfolioValueTrendChart
/// and TickerValueTrendChart refresh without waiting for a remount.
#[tauri::command]
pub async fn trigger_import_recalculation(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, crate::services::history_recalc::HistoryRecalc>,
    asset_type: String,
    tickers: Vec<(String, Option<String>)>,
    earliest_date: i64,
) -> Result<()> {
    match asset_type.as_str() {
        "stocks" => {
            // Queued, not awaited: the rebuild runs in the background
            let jobs = tickers
                .iter()
                .map(|(ticker, _)| (ticker.clone(), earliest_date))
                .collect();
            schedule_stock_history_rebuild(&app, &db, &recalc, jobs);
        }
        "crypto" => {
            for (ticker, coingecko_id) in &tickers {
                let resolved = match coingecko_id {
                    Some(cg) => Some(cg.clone()),
                    None => resolve_coingecko_id(&db, ticker).await,
                };
                match resolved {
                    Some(cg) => {
                        if let Err(e) = trigger_historical_recalculation_for_crypto_ticker(
                            &db,
                            earliest_date,
                            ticker,
                            &cg,
                        )
                        .await
                        {
                            log::debug!("[RECALC] {} failed: {}", ticker, e);
                        }
                    }
                    None => {
                        log::debug!(
                            "[RECALC] {} skipped: no coingeckoId provided and none found in crypto_investments",
                            ticker
                        );
                    }
                }
            }
        }
        _ => {
            if let Err(e) = trigger_historical_recalculation_for_asset(
                &db,
                earliest_date,
                AssetType::OtherAssets,
            )
            .await
            {
                log::error!("[RECALC] {} failed: {}", asset_type, e);
            }
        }
    }
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

        crate::services::currency::update_exchange_rates(
            [("USD".to_string(), 23.0), ("JPY".to_string(), 0.15)]
                .into_iter()
                .collect(),
        );

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
