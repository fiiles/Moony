//! Runtime orchestration for durable MCP bookkeeping. Financial queue logic is in services.
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::Emitter;

use super::portfolio;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::services::{fx_backfill, history_recalc, mcp_refresh, price_api};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static REQUESTED: AtomicBool = AtomicBool::new(false);

/// A renderer is never needed. Retries happen on the next mutation or unlock; failed jobs
/// remain on disk. One worker drains all domains and concurrent writes request another pass.
pub(crate) fn resume(app: tauri::AppHandle, db: Database) {
    REQUESTED.store(true, Ordering::SeqCst);
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        while REQUESTED.swap(false, Ordering::SeqCst) {
            let session = match db.current_session() {
                Ok(session) => session,
                Err(e) => {
                    log::warn!("[MCP refresh] session unavailable: {e}");
                    break;
                }
            };
            let jobs = match session.with_conn(mcp_refresh::pending) {
                Ok(jobs) => jobs,
                Err(e) => {
                    log::warn!("[MCP refresh] pending work unavailable: {e}");
                    break;
                }
            };
            for job in jobs {
                match run(&session, &job).await {
                    Ok(()) => {
                        match session.with_conn(|conn| mcp_refresh::acknowledge(conn, &job)) {
                            Ok(()) => log::info!("[MCP refresh] {} completed", job.domain),
                            Err(e) => {
                                log::warn!("[MCP refresh] completion will be retried: {e}")
                            }
                        }
                    }
                    Err(e) => log::warn!("[MCP refresh] {} remains pending: {e}", job.domain),
                }
                // Even a partial refresh can have produced useful new quotes/snapshots.
                app.emit("recalculation-complete", ()).ok();
            }
        }
        ACTIVE.store(false, Ordering::SeqCst);
        // Close the race between the last pending check and releasing the active flag.
        if REQUESTED.load(Ordering::SeqCst) {
            resume(app, db);
        }
    });
}

async fn run(db: &Database, job: &mcp_refresh::PendingRefresh) -> Result<()> {
    // Immediately record committed native holdings even when offline. A partial quote refresh
    // can write useful new prices and still fail; snapshot those facts before retaining the job.
    portfolio::update_todays_snapshot(db).await?;
    let refreshed = refresh_derived_data(db, job).await;
    let snapshot = portfolio::update_todays_snapshot(db).await;
    refreshed?;
    snapshot
}

/// Domains whose refresh is the local re-derivation of the portfolio days: it writes native
/// amounts and needs no provider, so missing FX coverage must not keep it pending.
fn derives_days_locally(domain: &str) -> bool {
    matches!(
        domain,
        "other-assets" | "bank-accounts" | "real-estate" | "loans" | "bonds"
    )
}

async fn refresh_derived_data(db: &Database, job: &mcp_refresh::PendingRefresh) -> Result<()> {
    match fx_backfill::ensure_fx_coverage(db).await {
        Ok(_) => {}
        Err(e) if derives_days_locally(&job.domain) => {
            log::warn!(
                "[MCP refresh] FX coverage unavailable ({e}); the days are derived from local facts anyway"
            );
        }
        Err(e) => return Err(e),
    }
    let today = chrono::Utc::now().timestamp().div_euclid(86_400) * 86_400;
    match job.domain.as_str() {
        "stocks" => {
            let tickers = db.with_conn(mcp_refresh::stock_tickers)?;
            let refreshed =
                price_api::refresh_stock_prices_yahoo(db, tickers.clone(), false).await?;
            // The price API reports partial failures without returning Err. Keep the durable job
            // until each held ticker has a current quote, including quotes reused from its cache.
            db.with_conn(|conn| {
                mcp_refresh::require_current_stock_prices(
                    conn,
                    &tickers,
                    chrono::Utc::now().timestamp(),
                )
            })?;
            price_api::refresh_dividends(db, tickers.clone()).await?;
            let jobs = db.with_conn(|conn| history_recalc::rebuild_jobs(conn, &tickers))?;
            let mut aggregate_from = job.earliest_date;
            for (ticker, first_date) in jobs {
                let from = if refreshed.unit_changed.contains(&ticker) {
                    first_date
                } else {
                    job.earliest_date.unwrap_or(first_date).max(first_date)
                };
                aggregate_from = Some(aggregate_from.map_or(from, |old| old.min(from)));
                if from < today {
                    history_recalc::rebuild_stock_ticker_strict(
                        db,
                        &ticker,
                        from,
                        today,
                        portfolio::find_closest_price,
                    )
                    .await?;
                }
            }
            if let Some(from) = aggregate_from {
                if from < today {
                    history_recalc::rebuild_days_async(db, from, today).await?;
                }
            }
        }
        "crypto" => {
            // Resolve every identifier from committed data, including on restart.
            let identities = db.with_conn(mcp_refresh::crypto_identities)?;
            let map: HashMap<String, String> = identities.iter().cloned().collect();
            // A provider id shared by aliases needs separate quote writes; never silently drop
            // one of the stored tickers through HashMap collection.
            if map.len() != identities.len() {
                for (id, ticker) in &identities {
                    let keys = price_api::get_api_keys(db)?;
                    let one = HashMap::from([(id.clone(), ticker.clone())]);
                    let quotes =
                        price_api::refresh_crypto_prices(db, keys.coingecko.as_deref(), one)
                            .await?;
                    if quotes.is_empty() {
                        return Err(AppError::Internal(format!(
                            "Crypto price refresh incomplete for {ticker}"
                        )));
                    }
                }
            }
            let keys = price_api::get_api_keys(db)?;
            if map.len() == identities.len() {
                let result =
                    price_api::refresh_crypto_prices(db, keys.coingecko.as_deref(), map).await?;
                if result.len() < identities.len() {
                    return Err(AppError::Internal("Crypto price refresh incomplete".into()));
                }
            }
            // A job without a date is "from the first fact" (it wins over every dated write
            // queued next to it): each coin from its first transaction, the days from the
            // first fact (`rebuild_days_async` clamps day 0).
            let from = job.earliest_date.unwrap_or(0);
            if from < today {
                let tickers: Vec<String> = identities
                    .iter()
                    .map(|(_, ticker)| ticker.clone())
                    .collect();
                let firsts: HashMap<String, i64> = db
                    .with_conn(|conn| history_recalc::crypto_rebuild_jobs(conn, &tickers))?
                    .into_iter()
                    .collect();
                for ticker in &tickers {
                    let from = match job.earliest_date {
                        Some(from) => from,
                        None => match firsts.get(ticker) {
                            Some(first) => *first,
                            None => continue,
                        },
                    };
                    if from >= today {
                        continue;
                    }
                    history_recalc::rebuild_crypto_ticker_strict(
                        db,
                        ticker,
                        from,
                        today,
                        portfolio::find_closest_price,
                    )
                    .await?;
                }
                history_recalc::rebuild_days_async(db, from, today).await?;
            }
        }
        // The portfolio days follow from the dated facts: an other asset's transactions (the
        // import and the create tool give the earliest day), and accounts, real estate, loans and
        // bonds. A job without a date comes from a write whose date is not known up front (an
        // undated account balance, a bond, an other asset with a quantity and no transaction,
        // which is carried on every day) or that wins over every dated write queued next to it:
        // from the first fact (`rebuild_days` clamps day 0).
        "other-assets" | "bank-accounts" | "real-estate" | "loans" | "bonds" => {
            rebuild_days_from(db, job.earliest_date.unwrap_or(0), today).await?;
        }
        _ => {}
    }
    Ok(())
}

/// Re-derive the portfolio days from `from` when it is before today.
async fn rebuild_days_from(db: &Database, from: i64, today: i64) -> Result<()> {
    if from < today {
        history_recalc::rebuild_days_async(db, from, today).await?;
    }
    Ok(())
}
