//! Rebuilds of a stock ticker's `stock_value_history` rows.
//!
//! A historical transaction (add, edit, delete, import) changes the position
//! on every day after it, so the ticker's daily value rows are recomputed from
//! that day to today. The rebuild used to cost 4-5 s for ~600 days: one `SUM`
//! query per day for the position, one rate lookup (up to eleven statements)
//! per day, and one autocommit write — one fsync — per row, all while holding
//! the database lock.
//!
//! It is now planned in a single pass and written in a single transaction:
//!
//! 1. [`plan_stock_ticker_history`] reads the ticker's transactions once and
//!    walks the days with a running position, looks the day's FX rates up in a
//!    map preloaded for the whole range, and prices each day through the
//!    caller's `price_at` closure. It only reads; the result is a list of
//!    [`DayPlan`]s.
//! 2. [`apply_stock_ticker_plan`] writes a slice of the plan through prepared
//!    statements inside one transaction, so a long rebuild can be applied in
//!    short chunks that each hold the lock briefly (see
//!    `services/history_recalc.rs`).
//!
//! The per-day semantics are those of the previous implementation: the
//! position on a day is the signed sum of the transactions dated at or before
//! that day's midnight (clamped at zero); a day without a position drops its
//! row, a held but unpriced day is left untouched.

use std::collections::HashMap;

use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::Result;
use crate::services::currency::{
    convert_to_czk_with_rates, get_all_rates, get_rates_for_date_range,
};
use crate::services::portfolio_history::SECONDS_PER_DAY;

/// A position smaller than this is float noise from summing fractional
/// quantities (0.1 + 0.2 - 0.3), not a holding: the day counts as empty.
const POSITION_EPSILON: f64 = 1e-9;

/// How far back a day's FX rates may be borrowed from (weekends, holidays) —
/// the same window as `currency::get_rates_for_date`.
const FX_LOOKBACK_DAYS: i64 = 10;

/// What a rebuild does to one day of a ticker's history.
#[derive(Debug, Clone, PartialEq)]
pub enum DayAction {
    /// A position is held and priced: write the row.
    Upsert {
        quantity: f64,
        price: f64,
        currency: String,
        value_czk: f64,
    },
    /// Nothing is held: drop whatever row is stored for the day.
    Delete,
}

/// One planned change, keyed by the day's midnight timestamp.
#[derive(Debug, Clone, PartialEq)]
pub struct DayPlan {
    pub day: i64,
    pub action: DayAction,
}

/// What a rebuild did, for logging and tests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RebuildStats {
    pub upserted: usize,
    pub deleted: usize,
}

fn floor_day(ts: i64) -> i64 {
    (ts / SECONDS_PER_DAY) * SECONDS_PER_DAY
}

/// Signed quantity changes of a ticker's transactions, oldest first.
/// `CAST(.. AS REAL)` keeps SQLite's leniency about malformed quantity text.
fn load_position_changes(conn: &Connection, ticker: &str) -> Result<Vec<(i64, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT transaction_date,
                CASE WHEN type = 'buy' THEN CAST(quantity AS REAL)
                     ELSE -CAST(quantity AS REAL) END
         FROM investment_transactions
         WHERE ticker = ?1
         ORDER BY transaction_date",
    )?;
    let rows = stmt.query_map([ticker], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let mut changes = Vec::new();
    for row in rows {
        changes.push(row?);
    }
    Ok(changes)
}

/// Plan the rows of `ticker`'s history for every day in `from_day..=until_day`.
///
/// `price_at(day)` returns the price (and its currency) to value the day at,
/// or `None` when no price is known; it is the caller's choice how a day maps
/// to a quote. Read-only: nothing is written until the plan is applied.
pub fn plan_stock_ticker_history<F>(
    conn: &Connection,
    ticker: &str,
    from_day: i64,
    until_day: i64,
    price_at: F,
) -> Result<Vec<DayPlan>>
where
    F: Fn(i64) -> Option<(f64, String)>,
{
    let from_day = floor_day(from_day);
    let until_day = floor_day(until_day);
    if from_day > until_day {
        return Ok(Vec::new());
    }

    let changes = load_position_changes(conn, ticker)?;
    let rates_by_day = get_rates_for_date_range(
        conn,
        from_day - FX_LOOKBACK_DAYS * SECONDS_PER_DAY,
        until_day,
    );
    // Today's in-memory rates, read at most once and only if a day has no
    // timeseries snapshot at all (what `get_rates_for_date` falls back to).
    let mut current_rates: Option<HashMap<String, f64>> = None;

    let mut plan = Vec::new();
    let mut position = 0.0_f64;
    let mut next_change = 0usize;
    let mut day = from_day;
    while day <= until_day {
        while next_change < changes.len() && changes[next_change].0 <= day {
            position += changes[next_change].1;
            next_change += 1;
        }
        let quantity = position.max(0.0);

        if quantity <= POSITION_EPSILON {
            plan.push(DayPlan {
                day,
                action: DayAction::Delete,
            });
        } else if let Some((price, currency)) = price_at(day) {
            let snapshot = (0..=FX_LOOKBACK_DAYS)
                .find_map(|offset| rates_by_day.get(&(day - offset * SECONDS_PER_DAY)));
            let value_czk = match snapshot {
                Some(rates) => convert_to_czk_with_rates(quantity * price, &currency, rates),
                None => {
                    let rates = current_rates.get_or_insert_with(get_all_rates);
                    convert_to_czk_with_rates(quantity * price, &currency, rates)
                }
            };
            plan.push(DayPlan {
                day,
                action: DayAction::Upsert {
                    quantity,
                    price,
                    currency,
                    value_czk,
                },
            });
        }
        day += SECONDS_PER_DAY;
    }
    Ok(plan)
}

/// Write `plan` for `ticker` in one transaction through prepared statements.
pub fn apply_stock_ticker_plan(
    conn: &Connection,
    ticker: &str,
    plan: &[DayPlan],
) -> Result<RebuildStats> {
    let mut stats = RebuildStats::default();
    if plan.is_empty() {
        return Ok(stats);
    }

    let tx = conn.unchecked_transaction()?;
    {
        let mut upsert = tx.prepare_cached(
            "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(ticker, recorded_at) DO UPDATE SET
                 value_czk = excluded.value_czk,
                 quantity = excluded.quantity,
                 price = excluded.price,
                 currency = excluded.currency",
        )?;
        let mut delete = tx.prepare_cached(
            "DELETE FROM stock_value_history WHERE ticker = ?1 AND recorded_at = ?2",
        )?;
        for day_plan in plan {
            match &day_plan.action {
                DayAction::Upsert {
                    quantity,
                    price,
                    currency,
                    value_czk,
                } => {
                    upsert.execute(params![
                        Uuid::new_v4().to_string(),
                        ticker,
                        day_plan.day,
                        value_czk.to_string(),
                        quantity.to_string(),
                        price.to_string(),
                        currency,
                    ])?;
                    stats.upserted += 1;
                }
                DayAction::Delete => {
                    stats.deleted += delete.execute(params![ticker, day_plan.day])?;
                }
            }
        }
    }
    tx.commit()?;
    Ok(stats)
}

/// Plan and apply a rebuild in one go: the synchronous form for callers that
/// already hold the connection (tests, benchmarks, one-off repairs).
pub fn rebuild_stock_ticker_history<F>(
    conn: &Connection,
    ticker: &str,
    from_day: i64,
    until_day: i64,
    price_at: F,
) -> Result<RebuildStats>
where
    F: Fn(i64) -> Option<(f64, String)>,
{
    let plan = plan_stock_ticker_history(conn, ticker, from_day, until_day, price_at)?;
    apply_stock_ticker_plan(conn, ticker, &plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = SECONDS_PER_DAY;
    /// A fixed "today" so fixtures do not depend on the clock.
    const TODAY: i64 = DAY * 20_500;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                investment_id TEXT NOT NULL,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                company_name TEXT NOT NULL DEFAULT '',
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'USD',
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE stock_value_history (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL,
                recorded_at INTEGER NOT NULL,
                value_czk TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price TEXT NOT NULL,
                currency TEXT NOT NULL,
                is_stale INTEGER NOT NULL DEFAULT 0,
                UNIQUE(ticker, recorded_at)
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn add_tx(conn: &Connection, id: &str, ticker: &str, kind: &str, qty: &str, date: i64) {
        conn.execute(
            "INSERT INTO investment_transactions (id, investment_id, type, ticker, quantity, transaction_date)
             VALUES (?1, 'inv', ?2, ?3, ?4, ?5)",
            params![id, kind, ticker, qty, date],
        )
        .expect("insert tx");
    }

    fn add_rate(conn: &Connection, day: i64, currency: &str, rate: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            params![day, currency, rate],
        )
        .expect("insert rate");
    }

    /// `(recorded_at, value_czk, quantity, price, currency)` of every stored row.
    type StoredRow = (i64, f64, f64, f64, String);

    fn stored_rows(conn: &Connection, ticker: &str) -> Vec<StoredRow> {
        let mut stmt = conn
            .prepare(
                "SELECT recorded_at, CAST(value_czk AS REAL), CAST(quantity AS REAL),
                        CAST(price AS REAL), currency
                 FROM stock_value_history WHERE ticker = ?1 ORDER BY recorded_at",
            )
            .expect("prepare");
        stmt.query_map([ticker], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
    }

    fn flat_price(price: f64) -> impl Fn(i64) -> Option<(f64, String)> {
        move |_| Some((price, "USD".to_string()))
    }

    #[test]
    fn position_is_a_running_sum_of_transactions_up_to_each_day() {
        let conn = setup_test_db();
        let d0 = TODAY - 10 * DAY;
        add_tx(&conn, "t1", "AAPL", "buy", "10", d0);
        add_tx(&conn, "t2", "AAPL", "buy", "5", d0 + 3 * DAY);
        add_tx(&conn, "t3", "AAPL", "sell", "15", d0 + 6 * DAY);
        add_rate(&conn, d0 - DAY, "USD", 20.0);

        let stats = rebuild_stock_ticker_history(
            &conn,
            "AAPL",
            d0 - 2 * DAY,
            d0 + 8 * DAY,
            flat_price(2.0),
        )
        .expect("rebuild");

        let rows = stored_rows(&conn, "AAPL");
        let by_day: Vec<(i64, f64)> = rows.iter().map(|r| ((r.0 - d0) / DAY, r.2)).collect();
        // 0..=2: 10, 3..=5: 15, from the sale day on: nothing, so no rows
        assert_eq!(
            by_day,
            vec![
                (0, 10.0),
                (1, 10.0),
                (2, 10.0),
                (3, 15.0),
                (4, 15.0),
                (5, 15.0)
            ]
        );
        assert_eq!(stats.upserted, 6);
        // value = qty * price * USD rate of the (carried-back) day
        assert!((rows[0].1 - 10.0 * 2.0 * 20.0).abs() < 1e-9);
    }

    #[test]
    fn a_transaction_after_midnight_counts_from_the_next_day() {
        // `transaction_date <= day midnight`: a buy at 15:00 is not held at 00:00
        let conn = setup_test_db();
        let d0 = TODAY - 5 * DAY;
        add_tx(&conn, "t1", "AAPL", "buy", "4", d0 + 15 * 3600);
        add_rate(&conn, d0 - DAY, "USD", 20.0);

        rebuild_stock_ticker_history(&conn, "AAPL", d0, d0 + 2 * DAY, flat_price(1.0))
            .expect("rebuild");

        let days: Vec<i64> = stored_rows(&conn, "AAPL")
            .iter()
            .map(|r| (r.0 - d0) / DAY)
            .collect();
        assert_eq!(days, vec![1, 2]);
    }

    #[test]
    fn days_without_a_position_drop_stale_rows_and_unpriced_days_are_left_alone() {
        let conn = setup_test_db();
        let d0 = TODAY - 6 * DAY;
        add_tx(&conn, "t1", "AAPL", "buy", "1", d0 + 2 * DAY);
        add_rate(&conn, d0 - DAY, "USD", 20.0);
        // Stale rows from an earlier state: one before the buy, one on a day
        // that will have a position but no price.
        for (id, day) in [("stale0", d0), ("stale3", d0 + 3 * DAY)] {
            conn.execute(
                "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                 VALUES (?1, 'AAPL', ?2, '999', '9', '9', 'USD')",
                params![id, day],
            )
            .expect("stale row");
        }

        let unpriced_day = d0 + 3 * DAY;
        let price_at = move |day: i64| {
            if day == unpriced_day {
                None
            } else {
                Some((3.0, "USD".to_string()))
            }
        };
        let stats = rebuild_stock_ticker_history(&conn, "AAPL", d0, d0 + 4 * DAY, price_at)
            .expect("rebuild");

        let rows = stored_rows(&conn, "AAPL");
        let days: Vec<i64> = rows.iter().map(|r| (r.0 - d0) / DAY).collect();
        // d0 and d0+1 (no position) deleted; d0+2 and d0+4 written; d0+3 untouched
        assert_eq!(days, vec![2, 3, 4]);
        assert_eq!(rows[1].1, 999.0, "an unpriced held day keeps its old row");
        assert_eq!(stats.deleted, 1);
        assert_eq!(stats.upserted, 2);
    }

    #[test]
    fn rebuilding_twice_is_idempotent_and_replaces_rows_in_place() {
        let conn = setup_test_db();
        let d0 = TODAY - 4 * DAY;
        add_tx(&conn, "t1", "AAPL", "buy", "2", d0);
        add_rate(&conn, d0 - DAY, "USD", 20.0);

        rebuild_stock_ticker_history(&conn, "AAPL", d0, d0 + 3 * DAY, flat_price(1.0))
            .expect("first");
        let first = stored_rows(&conn, "AAPL");
        rebuild_stock_ticker_history(&conn, "AAPL", d0, d0 + 3 * DAY, flat_price(5.0))
            .expect("second");
        let second = stored_rows(&conn, "AAPL");

        assert_eq!(first.len(), 4);
        assert_eq!(
            second.len(),
            4,
            "the (ticker, day) key is upserted, not duplicated"
        );
        assert!((second[0].3 - 5.0).abs() < 1e-12);
    }

    #[test]
    fn rates_come_from_the_nearest_earlier_snapshot_within_ten_days() {
        let conn = setup_test_db();
        let d0 = TODAY - 30 * DAY;
        add_tx(&conn, "t1", "AAPL", "buy", "1", d0);
        add_rate(&conn, d0, "USD", 20.0);
        add_rate(&conn, d0 + 12 * DAY, "USD", 25.0);

        rebuild_stock_ticker_history(&conn, "AAPL", d0, d0 + 13 * DAY, flat_price(1.0))
            .expect("rebuild");

        let rows = stored_rows(&conn, "AAPL");
        let value_on = |offset: i64| {
            rows.iter()
                .find(|r| r.0 == d0 + offset * DAY)
                .expect("row")
                .1
        };
        assert_eq!(value_on(0), 20.0);
        assert_eq!(value_on(10), 20.0, "still inside the 10-day walk-back");
        assert_eq!(value_on(12), 25.0, "a newer snapshot takes over on its day");
        assert_eq!(value_on(13), 25.0);
    }

    #[test]
    fn float_residue_from_fractional_quantities_is_an_empty_position() {
        // 0.1 + 0.2 - 0.3 is 5.5e-17 in floating point: sold out, not held
        let conn = setup_test_db();
        let d0 = TODAY - 5 * DAY;
        add_tx(&conn, "t1", "AAPL", "buy", "0.1", d0);
        add_tx(&conn, "t2", "AAPL", "buy", "0.2", d0);
        add_tx(&conn, "t3", "AAPL", "sell", "0.3", d0 + 2 * DAY);
        add_rate(&conn, d0 - DAY, "USD", 20.0);

        rebuild_stock_ticker_history(&conn, "AAPL", d0, d0 + 4 * DAY, flat_price(1.0))
            .expect("rebuild");

        let days: Vec<i64> = stored_rows(&conn, "AAPL")
            .iter()
            .map(|r| (r.0 - d0) / DAY)
            .collect();
        assert_eq!(days, vec![0, 1]);
    }

    #[test]
    fn an_empty_range_plans_nothing() {
        let conn = setup_test_db();
        let plan = plan_stock_ticker_history(&conn, "AAPL", TODAY, TODAY - DAY, flat_price(1.0))
            .expect("plan");
        assert!(plan.is_empty());
    }
}

/// Golden comparison against the previous implementation, plus an ignored
/// benchmark on a real (SQLCipher, on-disk, fully migrated) database.
#[cfg(test)]
mod golden {
    use super::*;
    use crate::db::Database;
    use crate::services::currency::convert_to_czk_at;
    use crate::services::portfolio_history::{
        aggregate_ticker_history, update_classes, AssetClassKind,
    };
    use std::time::Instant;

    const DAY: i64 = SECONDS_PER_DAY;
    const TODAY: i64 = DAY * 20_500;

    type Quote = (i64, f64, String);
    type HistoryRow = (i64, f64, f64, f64, String);

    /// Verbatim logic of the previous `recalculate_stock_ticker_history` DB
    /// phase (commands/portfolio.rs): one `SUM` per day, one rate lookup per
    /// day, one autocommit write per row. `(timestamp, price, currency)`
    /// quotes stand in for the Yahoo response.
    fn legacy_recalculate(
        conn: &Connection,
        ticker: &str,
        from_timestamp: i64,
        today_start: i64,
        quotes: &[Quote],
    ) {
        let from_day = (from_timestamp / 86400) * 86400;
        let mut days = Vec::new();
        let mut check_day = from_day;
        while check_day <= today_start {
            days.push(check_day);
            check_day += 86400;
        }
        for day_timestamp in days {
            let quantity: f64 = conn
                .query_row(
                    "SELECT COALESCE(
                        SUM(CASE WHEN type = 'buy' THEN CAST(quantity AS REAL) ELSE -CAST(quantity AS REAL) END),
                        0.0
                    ) FROM investment_transactions
                    WHERE ticker = ?1 AND transaction_date <= ?2",
                    params![ticker, day_timestamp],
                    |row| row.get(0),
                )
                .unwrap_or(0.0_f64)
                .max(0.0);
            if quantity <= 0.0 {
                conn.execute(
                    "DELETE FROM stock_value_history WHERE ticker = ?1 AND recorded_at = ?2",
                    params![ticker, day_timestamp],
                )
                .expect("legacy delete");
                continue;
            }
            let Some((_, price, currency)) = quotes
                .iter()
                .min_by_key(|(ts, _, _)| (ts - day_timestamp).abs())
            else {
                continue;
            };
            let value_czk = convert_to_czk_at(conn, quantity * price, currency, day_timestamp);
            conn.execute(
                "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(ticker, recorded_at) DO UPDATE SET
                     value_czk = excluded.value_czk,
                     quantity = excluded.quantity,
                     price = excluded.price,
                     currency = excluded.currency",
                params![
                    Uuid::new_v4().to_string(),
                    ticker,
                    day_timestamp,
                    value_czk.to_string(),
                    quantity.to_string(),
                    price.to_string(),
                    currency,
                ],
            )
            .expect("legacy upsert");
        }
    }

    /// Verbatim logic of the previous `aggregate_ticker_history` for stocks
    /// (services/portfolio_history.rs): one aggregate query and one
    /// autocommit `update_classes` per day.
    fn legacy_aggregate(conn: &Connection, from_day: i64, until_day_exclusive: i64) {
        let from_day = (from_day / DAY) * DAY;
        let days: Vec<i64> = {
            let mut stmt = conn
                .prepare(
                    "SELECT DISTINCT (recorded_at / 86400) * 86400 AS day FROM portfolio_metrics_history
                     WHERE (recorded_at / 86400) * 86400 >= ?1 AND (recorded_at / 86400) * 86400 < ?2
                     ORDER BY day",
                )
                .expect("prepare days");
            let rows = stmt
                .query_map([from_day, until_day_exclusive], |row| row.get(0))
                .expect("days");
            rows.filter_map(|r| r.ok()).collect()
        };
        let mut stmt = conn
            .prepare(
                "SELECT s.currency, SUM(CAST(s.quantity AS REAL) * CAST(s.price AS REAL))
                 FROM stock_value_history s
                 JOIN (
                     SELECT ticker, MAX(recorded_at) AS max_ts
                     FROM stock_value_history
                     WHERE recorded_at < ?1
                     GROUP BY ticker
                 ) latest ON s.ticker = latest.ticker AND s.recorded_at = latest.max_ts
                 WHERE s.ticker IN (
                     SELECT ticker FROM investment_transactions
                     WHERE transaction_date < ?1
                     GROUP BY ticker
                     HAVING SUM(CASE WHEN type = 'buy' THEN CAST(quantity AS REAL)
                                     ELSE -CAST(quantity AS REAL) END) > 0.000001
                 )
                 GROUP BY s.currency",
            )
            .expect("prepare aggregate");
        for day in days {
            let breakdown: HashMap<String, f64> = stmt
                .query_map([day + DAY], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, f64>(1)?))
                })
                .expect("aggregate")
                .filter_map(|r| r.ok())
                .filter(|(_, v)| *v != 0.0)
                .collect();
            update_classes(conn, day, &[(AssetClassKind::Investments, breakdown)])
                .expect("legacy update_classes");
        }
    }

    /// The previous call shape's closest-quote rule, for the new code's closure.
    fn closest_quote(quotes: &[Quote], day: i64) -> Option<(f64, String)> {
        quotes
            .iter()
            .min_by_key(|(ts, _, _)| (ts - day).abs())
            .map(|(_, price, currency)| (*price, currency.clone()))
    }

    fn weekday_of(day: i64) -> i64 {
        (day / DAY + 3) % 7 // 0 = Monday (1970-01-01 was a Thursday)
    }

    /// Deterministic weekday quotes (weekends missing, like a real feed).
    fn weekday_quotes(first_day: i64, last_day: i64, currency: &str, seed: i64) -> Vec<Quote> {
        let mut quotes = Vec::new();
        let mut day = first_day;
        while day <= last_day {
            if weekday_of(day) < 5 {
                let wobble = ((day / DAY + seed) % 23) as f64;
                quotes.push((
                    day + 14 * 3600,
                    100.0 + wobble * 1.37 + seed as f64,
                    currency.to_string(),
                ));
            }
            day += DAY;
        }
        quotes
    }

    fn minimal_schema(conn: &Connection) {
        conn.execute_batch(
            r#"
            CREATE TABLE investment_transactions (id TEXT PRIMARY KEY, investment_id TEXT NOT NULL, type TEXT NOT NULL, ticker TEXT NOT NULL, company_name TEXT NOT NULL, quantity TEXT NOT NULL, price_per_unit TEXT NOT NULL, currency TEXT NOT NULL, transaction_date INTEGER NOT NULL, created_at INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE stock_value_history (id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL, value_czk TEXT NOT NULL, quantity TEXT NOT NULL, price TEXT NOT NULL, currency TEXT NOT NULL, is_stale INTEGER NOT NULL DEFAULT 0, UNIQUE(ticker, recorded_at));
            CREATE TABLE exchange_rate_history (date INTEGER NOT NULL, currency TEXT NOT NULL, rate REAL NOT NULL, PRIMARY KEY (date, currency));
            CREATE TABLE portfolio_metrics_history (
                id TEXT PRIMARY KEY, total_savings TEXT NOT NULL, total_loans_principal TEXT NOT NULL,
                total_investments TEXT NOT NULL, total_crypto TEXT NOT NULL, total_bonds TEXT NOT NULL,
                total_real_estate_personal TEXT NOT NULL, total_real_estate_investment TEXT NOT NULL,
                total_other_assets TEXT NOT NULL DEFAULT '0', recorded_at INTEGER NOT NULL,
                investments_by_currency TEXT NOT NULL DEFAULT '{}', crypto_by_currency TEXT NOT NULL DEFAULT '{}',
                savings_by_currency TEXT NOT NULL DEFAULT '{}', bonds_by_currency TEXT NOT NULL DEFAULT '{}',
                real_estate_by_currency TEXT NOT NULL DEFAULT '{}', loans_by_currency TEXT NOT NULL DEFAULT '{}',
                other_assets_by_currency TEXT NOT NULL DEFAULT '{}', source TEXT NOT NULL DEFAULT 'live'
            );
            "#,
        )
        .expect("schema");
    }

    /// FX snapshots on weekdays only, so weekend days exercise the walk-back.
    fn seed_rates(conn: &Connection, first_day: i64) {
        let mut day = first_day - 15 * DAY;
        while day <= TODAY {
            if weekday_of(day) < 5 {
                for (currency, rate) in
                    [("USD", 21.0 + (day / DAY % 11) as f64 * 0.1), ("EUR", 24.5)]
                {
                    conn.execute(
                        "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
                        params![day, currency, rate],
                    )
                    .expect("rate");
                }
            }
            day += DAY;
        }
    }

    fn seed_snapshot_days(conn: &Connection, first_day: i64, skip_every: i64) {
        let mut day = first_day;
        let mut n = 0;
        while day <= TODAY {
            n += 1;
            if skip_every == 0 || n % skip_every != 0 {
                conn.execute(
                    "INSERT INTO portfolio_metrics_history
                     (id, total_savings, total_loans_principal, total_investments, total_crypto, total_bonds,
                      total_real_estate_personal, total_real_estate_investment, recorded_at, source)
                     VALUES (?1, '1', '0', '1', '0', '0', '0', '0', ?2, 'backfill')",
                    params![Uuid::new_v4().to_string(), day],
                )
                .expect("snapshot");
            }
            day += DAY;
        }
    }

    fn add_txs(conn: &Connection, ticker: &str, currency: &str, txs: &[(&str, &str, i64)]) {
        for (i, (kind, qty, date)) in txs.iter().enumerate() {
            conn.execute(
                "INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, quantity, price_per_unit, currency, transaction_date, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?4, ?5, '100', ?6, ?7, 0)",
                params![format!("{ticker}-{i}"), format!("inv-{ticker}"), kind, ticker, qty, currency, date],
            )
            .expect("tx");
        }
    }

    /// Buy / partial sell / sell-out / re-buy with fractional quantities and
    /// one transaction at a non-midnight time. The quantities are exact binary
    /// fractions so the sell-out lands on exactly zero in any summation order.
    fn seed_main_ticker(conn: &Connection, ticker: &str, first_day: i64) {
        add_txs(
            conn,
            ticker,
            "USD",
            &[
                ("buy", "12.5", first_day + 40 * DAY),
                ("buy", "0.375", first_day + 41 * DAY + 13 * 3600),
                ("sell", "4.125", first_day + 120 * DAY),
                ("buy", "3", first_day + 200 * DAY),
                ("sell", "11.75", first_day + 330 * DAY),
                ("buy", "8.25", first_day + 460 * DAY),
            ],
        );
    }

    /// Inexact fractions that never sell out: the running sum and SQLite's
    /// `SUM` may differ in the last bits, which the comparisons tolerate.
    fn seed_fractional_ticker(conn: &Connection, ticker: &str, currency: &str, first_day: i64) {
        add_txs(
            conn,
            ticker,
            currency,
            &[
                ("buy", "0.37", first_day + 10 * DAY),
                ("buy", "1.13", first_day + 90 * DAY),
                ("sell", "0.41", first_day + 250 * DAY),
                ("buy", "2.0000007", first_day + 400 * DAY),
            ],
        );
    }

    fn rows_of(conn: &Connection, ticker: &str) -> Vec<HistoryRow> {
        let mut stmt = conn
            .prepare(
                "SELECT recorded_at, CAST(value_czk AS REAL), CAST(quantity AS REAL),
                        CAST(price AS REAL), currency
                 FROM stock_value_history WHERE ticker = ?1 ORDER BY recorded_at",
            )
            .expect("prepare");
        stmt.query_map([ticker], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
    }

    fn close(x: f64, y: f64) -> bool {
        (x - y).abs() <= 1e-9 * x.abs().max(1.0)
    }

    fn assert_rows_match(legacy: &[HistoryRow], new: &[HistoryRow]) {
        assert_eq!(legacy.len(), new.len(), "same number of history rows");
        for (a, b) in legacy.iter().zip(new) {
            assert_eq!(a.0, b.0, "same day");
            assert_eq!(a.4, b.4, "same currency on day {}", a.0);
            for (x, y, what) in [
                (a.1, b.1, "value_czk"),
                (a.2, b.2, "quantity"),
                (a.3, b.3, "price"),
            ] {
                assert!(close(x, y), "{what} differs on day {}: {x} vs {y}", a.0);
            }
        }
    }

    #[test]
    fn rebuild_matches_the_previous_implementation_on_a_600_day_fixture() {
        let first_day = TODAY - 600 * DAY;
        let quotes = weekday_quotes(first_day - 3 * DAY, TODAY, "USD", 5);

        let legacy_db = Connection::open_in_memory().expect("db");
        let new_db = Connection::open_in_memory().expect("db");
        for conn in [&legacy_db, &new_db] {
            minimal_schema(conn);
            seed_rates(conn, first_day);
            seed_main_ticker(conn, "AAPL", first_day);
            seed_fractional_ticker(conn, "MSFT", "USD", first_day);
        }

        // A first full build, then an edit rebuilt from the middle: both
        // implementations must also agree on the replace-in-place path.
        for ticker in ["AAPL", "MSFT"] {
            legacy_recalculate(&legacy_db, ticker, first_day, TODAY, &quotes);
            rebuild_stock_ticker_history(&new_db, ticker, first_day, TODAY, |d| {
                closest_quote(&quotes, d)
            })
            .expect("rebuild");
            assert_rows_match(&rows_of(&legacy_db, ticker), &rows_of(&new_db, ticker));
        }

        for conn in [&legacy_db, &new_db] {
            conn.execute(
                "UPDATE investment_transactions SET quantity = '20.5' WHERE id = 'AAPL-3'",
                [],
            )
            .expect("edit");
        }
        let from = first_day + 150 * DAY;
        legacy_recalculate(&legacy_db, "AAPL", from, TODAY, &quotes);
        rebuild_stock_ticker_history(&new_db, "AAPL", from, TODAY, |d| closest_quote(&quotes, d))
            .expect("rebuild");
        let legacy_rows = rows_of(&legacy_db, "AAPL");
        assert!(
            legacy_rows.len() > 300,
            "the fixture holds a position for most of the range"
        );
        assert_rows_match(&legacy_rows, &rows_of(&new_db, "AAPL"));
    }

    /// `(day, investments_by_currency, total_investments)` per snapshot row.
    fn investments_of(conn: &Connection) -> Vec<(i64, HashMap<String, f64>, f64)> {
        let mut stmt = conn
            .prepare(
                "SELECT recorded_at, investments_by_currency, CAST(total_investments AS REAL)
                 FROM portfolio_metrics_history ORDER BY recorded_at",
            )
            .expect("prepare");
        stmt.query_map([], |r| {
            let json: String = r.get(1)?;
            Ok((r.get(0)?, json, r.get(2)?))
        })
        .expect("query")
        .map(|r| {
            let (day, json, total) = r.expect("row");
            (
                day,
                crate::services::currency::breakdown_from_json(&json),
                total,
            )
        })
        .collect()
    }

    #[test]
    fn aggregate_matches_the_previous_per_day_query() {
        let first_day = TODAY - 400 * DAY;
        let usd = weekday_quotes(first_day - 3 * DAY, TODAY, "USD", 3);
        let eur = weekday_quotes(first_day - 3 * DAY, TODAY, "EUR", 8);

        let legacy_db = Connection::open_in_memory().expect("db");
        let new_db = Connection::open_in_memory().expect("db");
        for conn in [&legacy_db, &new_db] {
            minimal_schema(conn);
            seed_rates(conn, first_day);
            // Holiday-like gaps in the snapshot days
            seed_snapshot_days(conn, first_day, 9);
            seed_main_ticker(conn, "AAPL", first_day - 100 * DAY);
            seed_fractional_ticker(conn, "MSFT", "USD", first_day - 100 * DAY);
            seed_fractional_ticker(conn, "SAP", "EUR", first_day - 50 * DAY);
            // Transactions but no history rows, and history rows but no transactions
            add_txs(conn, "NOHIST", "USD", &[("buy", "5", first_day)]);
            conn.execute(
                "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                 VALUES ('ghost', 'GHOST', ?1, '1', '1', '1', 'USD')",
                [first_day + 5 * DAY],
            )
            .expect("ghost row");
            for (ticker, quotes) in [("AAPL", &usd), ("MSFT", &usd), ("SAP", &eur)] {
                rebuild_stock_ticker_history(conn, ticker, first_day - 100 * DAY, TODAY, |d| {
                    closest_quote(quotes, d)
                })
                .expect("history");
            }
        }

        legacy_aggregate(&legacy_db, first_day, TODAY);
        aggregate_ticker_history(&new_db, AssetClassKind::Investments, first_day, TODAY)
            .expect("aggregate");

        let legacy = investments_of(&legacy_db);
        let new = investments_of(&new_db);
        assert_eq!(legacy.len(), new.len());
        assert!(
            legacy
                .iter()
                .any(|(_, by_currency, _)| by_currency.len() == 2),
            "the fixture mixes two currencies on some days"
        );
        assert!(
            legacy
                .iter()
                .any(|(_, by_currency, _)| by_currency.is_empty()),
            "and has days where nothing is held"
        );
        for (a, b) in legacy.iter().zip(&new) {
            assert_eq!(a.0, b.0);
            assert_eq!(a.1.len(), b.1.len(), "same currencies on day {}", a.0);
            for (currency, x) in &a.1 {
                let y = b.1.get(currency).expect("currency present");
                assert!(
                    close(*x, *y),
                    "{currency} differs on day {}: {x} vs {y}",
                    a.0
                );
            }
            assert!(close(a.2, b.2), "total differs on day {}", a.0);
        }
    }

    // ---- benchmark -------------------------------------------------------

    fn open_disk_db(dir: &std::path::Path) -> Database {
        let db = Database::new();
        let key = crate::services::crypto::master_key_to_hex(&[7u8; 32]);
        db.create_with_key(dir.join("bench.db"), &key)
            .expect("create db");
        db
    }

    /// A realistic book: `other` tickers held across the whole range with a
    /// full history, daily portfolio snapshots, USD/EUR rates, plus the
    /// ticker under test.
    fn seed_bench_db(conn: &Connection, first_day: i64, other: usize) {
        seed_rates(conn, first_day);
        conn.execute_batch("BEGIN").expect("begin");
        seed_snapshot_days(conn, first_day, 0);
        for t in 0..other {
            let ticker = format!("T{t:02}");
            conn.execute(
                "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
                 VALUES (?1, ?2, ?2, '10', 'USD')",
                params![format!("inv-{ticker}"), ticker],
            )
            .expect("investment");
            add_txs(
                conn,
                &ticker,
                "USD",
                &[("buy", "10", first_day + (t as i64) * DAY)],
            );
            let quotes = weekday_quotes(first_day - 3 * DAY, TODAY, "USD", t as i64);
            rebuild_stock_ticker_history_in_open_txn(conn, &ticker, first_day, &quotes);
        }
        conn.execute_batch("COMMIT").expect("commit");
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency)
             VALUES ('inv-AAPL', 'AAPL', 'AAPL', '1', 'USD')",
            [],
        )
        .expect("main investment");
        seed_main_ticker(conn, "AAPL", first_day);
    }

    /// Seeding happens inside one outer transaction, so write the plan with
    /// plain statements instead of `apply_stock_ticker_plan` (which opens its own).
    fn rebuild_stock_ticker_history_in_open_txn(
        conn: &Connection,
        ticker: &str,
        first_day: i64,
        quotes: &[Quote],
    ) {
        let plan =
            plan_stock_ticker_history(conn, ticker, first_day, TODAY, |d| closest_quote(quotes, d))
                .expect("plan");
        for day_plan in plan {
            if let DayAction::Upsert {
                quantity,
                price,
                currency,
                value_czk,
            } = day_plan.action
            {
                conn.execute(
                    "INSERT INTO stock_value_history (id, ticker, recorded_at, value_czk, quantity, price, currency)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![
                        Uuid::new_v4().to_string(),
                        ticker,
                        day_plan.day,
                        value_czk.to_string(),
                        quantity.to_string(),
                        price.to_string(),
                        currency
                    ],
                )
                .expect("seed row");
            }
        }
    }

    /// One recalculation of the ticker under test, end to end on a real
    /// database: previous pipeline (per-day SUM + rate lookup + autocommit
    /// writes, per-day aggregate query + autocommit writes, no
    /// `portfolio_metrics_history.recorded_at` index) against the new one.
    ///
    /// `cargo test --lib ticker_history::golden::benchmark -- --ignored --nocapture`
    #[test]
    #[ignore = "benchmark: prints timings, asserts nothing about wall-clock"]
    fn benchmark_full_recalculation() {
        let first_day = TODAY - 630 * DAY;
        let quotes = weekday_quotes(first_day - 3 * DAY, TODAY, "USD", 5);

        let dir_old = tempfile::tempdir().expect("tempdir");
        let dir_new = tempfile::tempdir().expect("tempdir");
        let db_old = open_disk_db(dir_old.path());
        let db_new = open_disk_db(dir_new.path());
        for db in [&db_old, &db_new] {
            db.with_conn(|conn| {
                seed_bench_db(conn, first_day, 20);
                Ok(())
            })
            .expect("seed");
        }
        // The previous schema did not have the migration-013 index.
        db_old
            .with_conn(|conn| {
                conn.execute_batch(
                    "DROP INDEX IF EXISTS idx_portfolio_metrics_history_recorded_at",
                )?;
                Ok(())
            })
            .expect("drop index");

        let t = Instant::now();
        db_old
            .with_conn(|conn| {
                legacy_recalculate(conn, "AAPL", first_day, TODAY, &quotes);
                Ok(())
            })
            .expect("legacy history");
        let legacy_history = t.elapsed();
        let t = Instant::now();
        db_old
            .with_conn(|conn| {
                legacy_aggregate(conn, first_day, TODAY);
                Ok(())
            })
            .expect("legacy aggregate");
        let legacy_aggregate_time = t.elapsed();

        let t = Instant::now();
        let mut plan_time = std::time::Duration::ZERO;
        db_new
            .with_conn(|conn| {
                let plan = plan_stock_ticker_history(conn, "AAPL", first_day, TODAY, |d| {
                    closest_quote(&quotes, d)
                })?;
                plan_time = t.elapsed();
                apply_stock_ticker_plan(conn, "AAPL", &plan)?;
                Ok(())
            })
            .expect("new history");
        let new_history = t.elapsed();
        let t = Instant::now();
        db_new
            .with_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                aggregate_ticker_history(&tx, AssetClassKind::Investments, first_day, TODAY)?;
                tx.commit()?;
                Ok(())
            })
            .expect("new aggregate");
        let new_aggregate = t.elapsed();

        println!("--- 630 days x 1 ticker (+20 other tickers), SQLCipher on disk, debug build ---");
        println!("ticker history   previous: {legacy_history:?}   new: {new_history:?} (plan {plan_time:?})");
        println!("aggregate step   previous: {legacy_aggregate_time:?}   new: {new_aggregate:?}");
        println!(
            "total            previous: {:?}   new: {:?}",
            legacy_history + legacy_aggregate_time,
            new_history + new_aggregate
        );

        let a = db_old.with_conn(|c| Ok(rows_of(c, "AAPL"))).expect("rows");
        let b = db_new.with_conn(|c| Ok(rows_of(c, "AAPL"))).expect("rows");
        assert_rows_match(&a, &b);
    }
}
