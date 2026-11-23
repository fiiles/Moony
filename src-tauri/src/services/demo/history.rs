//! Exchange rates and the derived history: the USD and DKK rate history the conversions need,
//! the daily value rows of every ticker, the portfolio days rebuilt from the dated facts,
//! today's live row and the two markers that keep the first unlock from rebuilding everything
//! from the network.

use super::clock::{self, DAY};
use super::prices;
use super::{DemoOptions, DemoSummary};
use crate::error::Result;
use crate::services::currency;
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
