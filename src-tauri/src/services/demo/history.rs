//! Exchange rates and the derived history: the USD and DKK rate history the conversions need,
//! the daily value rows of every ticker, the portfolio days rebuilt from the dated facts,
//! today's live row and the two markers that keep the first unlock from rebuilding everything
//! from the network.

use super::clock::{self, DAY};
use super::prices;
use super::{DemoOptions, DemoSummary};
use crate::error::Result;
use crate::services::fx::Quotes;
use crate::services::portfolio_history::{self, SnapshotSource};
use crate::services::pricing::AssetKind;
use crate::services::{currency, history_days, history_plan, net_worth, ticker_history};
use rusqlite::{params, Connection};

/// A quoted currency and its built-in series of units per 1 EUR.
type RateSeries = (&'static str, fn(i64) -> f64);

/// Quoted currencies the demo uses besides the EUR reference.
const RATES: [RateSeries; 2] = [("USD", prices::usd_per_eur), ("DKK", prices::dkk_per_eur)];

/// Days of rate history before the first day of `M60`, more than the app's 10-day look-back.
const RATE_LEAD_DAYS: i64 = 12;

/// A rate as the ECB publishes it (four decimals).
fn rate4(rate: f64) -> f64 {
    (rate * 10_000.0).round() / 10_000.0
}

/// The USD and DKK rate history (units per 1 EUR), one row per day from 12 days before the
/// first day of `M60` through today, the current rates fetched "now", and the in-process quote
/// snapshot loaded from them. Runs first: other-asset and report conversions read these rates.
pub fn seed_rates(conn: &Connection, opts: &DemoOptions) -> Result<usize> {
    let today = clock::today(opts.now);
    let first = clock::months_ago(opts.now, 60, 1) - RATE_LEAD_DAYS * DAY;
    let mut rows = 0;
    let tx = conn.unchecked_transaction()?;
    {
        let mut insert = tx.prepare(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)
             ON CONFLICT(date, currency) DO UPDATE SET rate = excluded.rate",
        )?;
        let mut day = first;
        while day <= today {
            for (code, series) in RATES {
                insert.execute(params![day, code, rate4(series(day))])?;
                rows += 1;
            }
            day += DAY;
        }
        for (code, series) in RATES {
            tx.execute(
                "INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(currency) DO UPDATE SET rate = excluded.rate,
                                                     fetched_at = excluded.fetched_at",
                params![code, rate4(series(today)), opts.now],
            )?;
        }
    }
    tx.commit()?;
    currency::load_rates_from_db(conn)?;
    Ok(rows)
}

/// Rates step: [`seed_rates`] plus the counts.
pub fn seed_fx(conn: &Connection, opts: &DemoOptions, summary: &mut DemoSummary) -> Result<()> {
    let rows = seed_rates(conn, opts)?;
    summary.counts.insert("exchange_rate_history", rows);
    summary.counts.insert("exchange_rates", RATES.len());
    Ok(())
}

/// The derived history, once every dated fact exists: daily value rows for every stock and
/// crypto ticker from its first trade to yesterday at the built-in prices, every portfolio day
/// from the first fact to yesterday, today's live row and ticker rows, and the markers that
/// tell the first unlock the history is current (rebuild version, settled through yesterday).
/// Fills the net worth in EUR.
pub fn seed(conn: &Connection, opts: &DemoOptions, summary: &mut DemoSummary) -> Result<()> {
    let today = clock::today(opts.now);
    let yesterday = today - DAY;

    for kind in [AssetKind::Stock, AssetKind::Crypto] {
        let table = match kind {
            AssetKind::Stock => "investment_transactions",
            AssetKind::Crypto => "crypto_transactions",
        };
        let mut stmt = conn.prepare(&format!(
            "SELECT ticker, MIN(transaction_date) FROM {table} GROUP BY ticker ORDER BY ticker"
        ))?;
        let firsts: Vec<(String, i64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        for (ticker, first) in firsts {
            let price_at = |day: i64| {
                prices::price_on(&ticker, day)
                    .map(|(price, currency)| (round2(price), currency.to_string()))
            };
            match kind {
                AssetKind::Stock => {
                    ticker_history::rebuild_stock_ticker_history(
                        conn, &ticker, first, yesterday, price_at,
                    )?;
                }
                AssetKind::Crypto => {
                    let plan = ticker_history::plan_ticker_history(
                        conn, kind, &ticker, first, yesterday, price_at,
                    )?;
                    ticker_history::apply_ticker_plan(conn, kind, &ticker, &plan)?;
                }
            }
        }
    }

    history_days::rebuild_days(conn, 0, today)?;

    // Today: the live row and the ticker rows, as the running app records them
    let breakdowns = net_worth::compute_breakdowns_at(conn, today)?;
    portfolio_history::upsert_snapshot(conn, opts.now, SnapshotSource::Live, &breakdowns)?;
    ticker_history::record_current_values(conn, today)?;

    history_plan::store_rebuild_version(conn)?;
    history_plan::store_settled_through(conn, yesterday)?;

    // Net worth in EUR at the stored rates (not the process-wide snapshot)
    let rates: Vec<(String, f64)> = conn
        .prepare("SELECT currency, rate FROM exchange_rates")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    let quotes = Quotes::from_stored(rates.iter().map(|(code, rate)| (code.as_str(), *rate)));
    let worth = net_worth::net_worth_of(breakdowns, &quotes, "EUR", false);
    summary.net_worth_eur = worth.net_worth.unwrap_or(0.0);
    let c = worth.classes;
    for (name, value) in [
        ("savings", c.savings),
        ("investments", c.investments),
        ("crypto", c.crypto),
        ("bonds", c.bonds),
        ("real_estate_personal", c.real_estate_personal),
        ("real_estate_investment", c.real_estate_investment),
        ("other_assets", c.other_assets),
        ("loans", c.loans),
    ] {
        summary.classes_eur.insert(name, value.unwrap_or(0.0));
    }

    for table in [
        "stock_value_history",
        "crypto_value_history",
        "portfolio_metrics_history",
    ] {
        let rows: i64 =
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        summary
            .counts
            .insert(table, usize::try_from(rows).unwrap_or(0));
    }
    Ok(())
}

fn round2(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
