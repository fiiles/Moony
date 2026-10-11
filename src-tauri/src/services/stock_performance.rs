//! Performance of the held stock positions over a period, for the stocks analysis page.
//!
//! A daily engine chain-links the existing TWR method (`investments::compute_twr_for_tickers`)
//! over every held position at once. For each position `p` and day `s` of the range, in the
//! report currency (values and flows converted at their own day):
//!
//! - `base_p(s) = V_p(s-1) + F_p(s)` and `gain_p(s) = V_p(s) - V_p(s-1) - F_p(s)`, where `V` is
//!   the value and `F` the flow (quantity change times the day's native price);
//! - a set `S` of positions chains `R_S(s) = sum_S gain / sum_S base`, so its time-weighted
//!   return is `prod (1 + R_S) - 1`; the whole portfolio is the set of every held position;
//! - the contribution of `S` to the portfolio return is
//!   `C_S(t) = sum_{s <= t} G(s-1) * sum_S gain(s) / sum_all base(s)`, with `G` the portfolio
//!   growth. The contributions of a partition of the held positions add up exactly to the
//!   portfolio TWR, because the portfolio's own step is `G(s-1) * sum_all gain / sum_all base`.
//!
//! A holding any of whose conversions fails drops out of that day only (its `base` and `gain`
//! are unknown) and is counted in `fxQuality`; the other holdings keep chaining. The
//! money-weighted return is the cumulative modified Dietz return of the portfolio value.

use std::collections::HashMap;

use rusqlite::Connection;

use crate::error::Result;
use crate::models::fx::ConversionQuality;
use crate::models::{
    PerformanceSeries, PerformanceSeriesSpec, PositionPerformance, StockPerformance,
    StockPerformanceRequest,
};
use crate::services::fx::{Quality, DAY};
use crate::services::investments::{
    load_day_holdings, load_twr_history, lookup_ticker_at, ts_to_date_str, DayHolding, ReportFx,
};
use crate::services::reporting::{money_in_currency, normalize_currency};

/// Most dates a report emits before the last day is added.
const MAX_POINTS: usize = 400;
/// A base or quantity at or below this is zero.
const EPSILON: f64 = 1e-9;

/// A held stock position.
struct Held {
    id: String,
    ticker: String,
}

/// Every position still held (quantity > 0), ordered by ticker.
fn held_positions(conn: &Connection) -> Result<Vec<Held>> {
    let mut stmt = conn.prepare(
        "SELECT id, ticker FROM stock_investments WHERE CAST(quantity AS REAL) > 0 ORDER BY ticker",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok(Held {
            id: row.get(0)?,
            ticker: row.get(1)?,
        })
    })?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Daily moves of positions in one currency. Index 0 is the base day.
struct Daily {
    days: Vec<i64>,
    /// `[k][p]`: `V_p(k-1) + F_p(k)` for `k >= 1`; `None` when a conversion failed that day.
    base: Vec<Vec<Option<f64>>>,
    /// `[k][p]`: `V_p(k) - V_p(k-1) - F_p(k)`.
    gain: Vec<Vec<Option<f64>>>,
    /// `[k][p]`: `V_p(k)`, `k >= 0`.
    value: Vec<Vec<Option<f64>>>,
    /// `[k][p]`: `F_p(k)`.
    flow: Vec<Vec<Option<f64>>>,
}

/// The value of `holding` converted at `at`; an empty holding is worth nothing and needs no rate.
fn value_at(
    fx: &ReportFx<'_>,
    holding: Option<&DayHolding>,
    at: i64,
    quality: &mut Quality,
) -> Option<f64> {
    let Some(h) = holding.filter(|h| h.quantity > EPSILON) else {
        return Some(0.0);
    };
    fx.history
        .convert_on_tracked(quality, h.quantity * h.price, &h.currency, fx.currency, at)
}

/// The cash flow of a day: the quantity change priced with the day's own native price and
/// converted at that day. `None` when the conversion failed.
fn flow_at(
    fx: &ReportFx<'_>,
    prev: Option<&DayHolding>,
    curr: Option<&DayHolding>,
    day: i64,
    quality: &mut Quality,
) -> Option<f64> {
    let delta = curr.map_or(0.0, |h| h.quantity) - prev.map_or(0.0, |h| h.quantity);
    if delta.abs() <= EPSILON {
        return Some(0.0);
    }
    // A sold-out position is priced with the day before it
    let priced = if curr.is_some_and(|h| h.quantity > EPSILON) {
        curr
    } else {
        prev
    };
    let Some(h) = priced else {
        return Some(0.0);
    };
    fx.history
        .convert_on_tracked(quality, delta * h.price, &h.currency, fx.currency, day)
}

/// The daily moves of `tickers` on `days` (consecutive days, the first is the base), converted
/// into `fx.currency`. Every value is converted once, as it was when it was "today".
fn build_daily(
    history: &HashMap<String, Vec<DayHolding>>,
    tickers: &[&str],
    days: &[i64],
    fx: &ReportFx<'_>,
    quality: &mut Quality,
) -> Daily {
    let grid = || vec![vec![None; tickers.len()]; days.len()];
    let mut daily = Daily {
        days: days.to_vec(),
        base: grid(),
        gain: grid(),
        value: grid(),
        flow: grid(),
    };
    for (p, ticker) in tickers.iter().enumerate() {
        let mut prev: Option<&DayHolding> = None;
        let mut prev_value: Option<f64> = None;
        for (k, &day) in days.iter().enumerate() {
            let curr = lookup_ticker_at(history, ticker, day);
            let value = value_at(fx, curr, day, quality);
            daily.value[k][p] = value;
            if k == 0 {
                daily.flow[k][p] = Some(0.0);
            } else if let (Some(v), Some(v_prev)) = (value, prev_value) {
                // A holding that cannot be converted drops out of this day only; the miss is
                // already recorded.
                if let Some(flow) = flow_at(fx, prev, curr, day, quality) {
                    daily.flow[k][p] = Some(flow);
                    daily.base[k][p] = Some(v_prev + flow);
                    daily.gain[k][p] = Some(v - v_prev - flow);
                }
            }
            prev = curr;
            prev_value = value;
        }
    }
    daily
}

/// The cumulative time-weighted return in percent of `members` over the days (first 0): their
/// own daily returns `sum gain / sum base`, chain-linked.
fn chain(daily: &Daily, members: &[usize]) -> Vec<f64> {
    let mut factor = 1.0;
    let mut out = vec![0.0];
    for k in 1..daily.days.len() {
        let (mut b, mut g) = (0.0, 0.0);
        for &p in members {
            if let (Some(bp), Some(gp)) = (daily.base[k][p], daily.gain[k][p]) {
                b += bp;
                g += gp;
            }
        }
        if b > EPSILON {
            factor *= 1.0 + g / b;
        }
        out.push((factor - 1.0) * 100.0);
    }
    out
}

/// What turns a set's gains into portfolio return, for every day.
struct Weights {
    /// `[k]`: `G(k-1) / sum_all base(k)`, 0 when the base is not positive.
    per_gain: Vec<f64>,
}

impl Weights {
    fn of_portfolio(daily: &Daily, all: &[usize]) -> Self {
        let mut growth = 1.0;
        let mut per_gain = vec![0.0];
        for k in 1..daily.days.len() {
            let (mut b, mut g) = (0.0, 0.0);
            for &p in all {
                if let (Some(bp), Some(gp)) = (daily.base[k][p], daily.gain[k][p]) {
                    b += bp;
                    g += gp;
                }
            }
            if b > EPSILON {
                per_gain.push(growth / b);
                growth *= 1.0 + g / b;
            } else {
                per_gain.push(0.0);
            }
        }
        Self { per_gain }
    }
}

/// The cumulative contribution in percentage points of `members` to the portfolio return:
/// `C_S(t) = sum_{s <= t} G(s-1) * sum_S gain(s) / sum_all base(s)`.
fn contribution(daily: &Daily, weights: &Weights, members: &[usize]) -> Vec<f64> {
    let mut c = 0.0;
    let mut out = vec![0.0];
    for k in 1..daily.days.len() {
        let gain: f64 = members.iter().filter_map(|&p| daily.gain[k][p]).sum();
        c += weights.per_gain[k] * gain;
        out.push(c * 100.0);
    }
    out
}

/// Money-weighted return from the first day to each day (modified Dietz, cumulative, percent):
/// `(V_k - V_0 - F_k) / (V_0 + sum_j F_j * (k - j + 1) / k)` where `F_k` is the flow cumulated
/// to `k` and the flow of day `j` counts for the `k - j + 1` days it was invested. Running sums
/// keep it linear: `sum_j F_j * (k - j + 1) = (k + 1) * sum F - sum F_j * j`. 0 when the
/// denominator is not positive; a day whose value is unknown repeats the previous figure.
fn money_weighted(values: &[Option<f64>], flows: &[f64]) -> Vec<f64> {
    let v0 = values[0].unwrap_or(0.0);
    let (mut sum_f, mut sum_fj) = (0.0, 0.0);
    let mut out = vec![0.0];
    for k in 1..values.len() {
        let steps = k as f64;
        sum_f += flows[k];
        sum_fj += flows[k] * steps;
        let Some(v) = values[k] else {
            out.push(out[k - 1]);
            continue;
        };
        let denominator = v0 + ((steps + 1.0) * sum_f - sum_fj) / steps;
        out.push(if denominator > EPSILON {
            (v - v0 - sum_f) / denominator * 100.0
        } else {
            0.0
        });
    }
    out
}

/// The days to emit out of `count`: the first, every n-th and the last, so that a report never
/// holds more than `MAX_POINTS` plus the last day. Chaining ran every day; only the output is
/// sampled.
fn emitted_indices(count: usize) -> Vec<usize> {
    let every = count.div_ceil(MAX_POINTS).max(1);
    let mut indices: Vec<usize> = (0..count).step_by(every).collect();
    if indices.last().is_some_and(|&last| last + 1 != count) {
        indices.push(count - 1);
    }
    indices
}

fn sample(values: &[f64], indices: &[usize]) -> Vec<f64> {
    indices.iter().map(|&k| values[k]).collect()
}

/// The held positions a series covers (indexes into the held list, ascending, unique); ids of
/// positions that are not held are ignored.
fn members_of(spec: &PerformanceSeriesSpec, index_of: &HashMap<&str, usize>) -> Vec<usize> {
    let mut members: Vec<usize> = spec
        .investment_ids
        .iter()
        .filter_map(|id| index_of.get(id.as_str()).copied())
        .collect();
    members.sort_unstable();
    members.dedup();
    members
}

/// First and last recorded day over every holding history.
fn history_span(history: &HashMap<String, Vec<DayHolding>>) -> Option<(i64, i64)> {
    let earliest = history
        .values()
        .filter_map(|rows| rows.first().map(|entry| entry.ts))
        .min()?;
    let latest = history
        .values()
        .filter_map(|rows| rows.last().map(|entry| entry.ts))
        .max()?;
    Some((earliest, latest))
}

/// The consecutive days from `from_ts` to `to_ts`, at least the first.
fn day_range(from_ts: i64, to_ts: i64) -> Vec<i64> {
    let mut days = vec![from_ts];
    let mut day = from_ts + DAY;
    while day <= to_ts {
        days.push(day);
        day += DAY;
    }
    days
}

/// The performance of the held positions from `request.from_ts` to `request.to_ts` in the
/// report currency: the whole portfolio, every requested series and every position.
///
/// The range is clamped to the recorded history, because nothing exists to chain-link before
/// the first known row or after the last one, and a transient absurd range (typing a year in a
/// date input) must not run for millions of iterations while holding the DB lock. The caller
/// validates the request (`StockPerformanceRequest::validate`).
pub fn stock_performance(
    conn: &Connection,
    request: &StockPerformanceRequest,
) -> Result<StockPerformance> {
    let currency = normalize_currency(&request.report_currency)?;
    let held = held_positions(conn)?;
    let tickers: Vec<String> = held.iter().map(|h| h.ticker.clone()).collect();
    let history = load_day_holdings(conn, &tickers, request.from_ts, request.to_ts)?;
    let (from_ts, to_ts) = match history_span(&history) {
        Some((earliest, latest)) => {
            let from_ts = request.from_ts.max(earliest);
            (from_ts, request.to_ts.min(latest).max(from_ts))
        }
        None => (request.from_ts, request.from_ts),
    };
    let days = day_range(from_ts, to_ts);
    let fx_history = load_twr_history(conn, from_ts, to_ts)?;
    let fx = ReportFx {
        currency: &currency,
        history: &fx_history,
    };
    let mut quality = Quality::default();

    let ticker_refs: Vec<&str> = tickers.iter().map(String::as_str).collect();
    let daily = build_daily(&history, &ticker_refs, &days, &fx, &mut quality);
    let all: Vec<usize> = (0..held.len()).collect();
    let weights = Weights::of_portfolio(&daily, &all);
    let indices = emitted_indices(days.len());

    // Whole-portfolio value, deposits and money-weighted return
    let values: Vec<Option<f64>> = (0..days.len())
        .map(|k| {
            let known: Vec<f64> = daily.value[k].iter().flatten().copied().collect();
            (!known.is_empty()).then(|| known.iter().sum())
        })
        .collect();
    let flows: Vec<f64> = (0..days.len())
        .map(|k| daily.flow[k].iter().flatten().sum())
        .collect();
    let mut net_flows = Vec::with_capacity(days.len());
    let mut cumulative = 0.0;
    for flow in &flows {
        cumulative += flow;
        net_flows.push(cumulative);
    }
    let mwr = money_weighted(&values, &flows);

    let index_of: HashMap<&str, usize> = held
        .iter()
        .enumerate()
        .map(|(p, h)| (h.id.as_str(), p))
        .collect();
    let mut series = Vec::with_capacity(request.series.len());
    for spec in &request.series {
        let members = members_of(spec, &index_of);
        let native = match &spec.currency {
            Some(code) => Some(normalize_currency(code)?),
            None => None,
        };
        let (twr, contribution) = match native {
            // The native return of a currency group: its own positions in their currency
            Some(code) if code != currency => {
                let member_tickers: Vec<&str> =
                    members.iter().map(|&p| held[p].ticker.as_str()).collect();
                let native_fx = ReportFx {
                    currency: &code,
                    history: &fx_history,
                };
                let native_daily =
                    build_daily(&history, &member_tickers, &days, &native_fx, &mut quality);
                let local: Vec<usize> = (0..member_tickers.len()).collect();
                (chain(&native_daily, &local), None)
            }
            Some(_) => (chain(&daily, &members), None),
            None => (
                chain(&daily, &members),
                Some(sample(&contribution(&daily, &weights, &members), &indices)),
            ),
        };
        series.push(PerformanceSeries {
            key: spec.key.clone(),
            twr: sample(&twr, &indices),
            contribution,
        });
    }

    let last = days.len() - 1;
    let positions = held
        .iter()
        .enumerate()
        .map(|(p, h)| PositionPerformance {
            investment_id: h.id.clone(),
            twr: chain(&daily, &[p]).last().copied().unwrap_or(0.0),
            contribution: contribution(&daily, &weights, &[p])
                .last()
                .copied()
                .unwrap_or(0.0),
            start_value: daily.value[0][p].map(|v| money_in_currency(v, &currency)),
            end_value: daily.value[last][p].map(|v| money_in_currency(v, &currency)),
        })
        .collect();

    Ok(StockPerformance {
        dates: indices.iter().map(|&k| ts_to_date_str(days[k])).collect(),
        portfolio: sample(&chain(&daily, &all), &indices),
        values: indices
            .iter()
            .map(|&k| values[k].map(|v| money_in_currency(v, &currency)))
            .collect(),
        net_flows: indices
            .iter()
            .map(|&k| money_in_currency(net_flows[k], &currency))
            .collect(),
        mwr: sample(&mwr, &indices),
        series,
        positions,
        fx_quality: ConversionQuality::from(&quality),
        currency,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PerformanceSeriesSpec;
    use crate::services::investments::ts_to_date_str;

    const DAY: i64 = 86_400;
    const DAY0: i64 = 1_700_000_000 / DAY * DAY;

    fn day(index: i64) -> i64 {
        DAY0 + index * DAY
    }

    /// `stock_investments`, the value history and the EUR-referenced rate history, as the
    /// production schema stores them (only the columns the engine reads).
    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "CREATE TABLE stock_investments (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL UNIQUE,
                quantity TEXT NOT NULL
            );
            CREATE TABLE stock_value_history (
                id TEXT PRIMARY KEY,
                ticker TEXT NOT NULL,
                recorded_at INTEGER NOT NULL,
                quantity TEXT NOT NULL,
                price TEXT NOT NULL,
                currency TEXT NOT NULL,
                UNIQUE(ticker, recorded_at)
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );",
        )
        .expect("schema");
        conn
    }

    /// A position `id` of `ticker` whose recorded days are `rows[i] = (quantity, price)` on
    /// `day(i)`. `held` is the stored quantity (0 closes the position).
    fn seed(
        conn: &Connection,
        id: &str,
        ticker: &str,
        held: &str,
        currency: &str,
        rows: &[(f64, f64)],
    ) {
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, quantity) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, ticker, held],
        )
        .expect("investment");
        for (i, (quantity, price)) in rows.iter().enumerate() {
            conn.execute(
                "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![
                    format!("{ticker}-{i}"),
                    ticker,
                    day(i as i64),
                    quantity.to_string(),
                    price.to_string(),
                    currency
                ],
            )
            .expect("history row");
        }
    }

    /// A CZK position held at `quantity` (constant) with the given daily prices.
    fn seed_czk(conn: &Connection, id: &str, ticker: &str, quantity: f64, prices: &[f64]) {
        let rows: Vec<(f64, f64)> = prices.iter().map(|price| (quantity, *price)).collect();
        seed(conn, id, ticker, &quantity.to_string(), "CZK", &rows);
    }

    /// One day of EUR quotes on which 1 USD costs `czk_per_usd` CZK (CZK at 25 per EUR).
    fn add_usd_czk(conn: &Connection, at: i64, czk_per_usd: f64) {
        for (currency, units_per_eur) in [("CZK", 25.0), ("USD", 25.0 / czk_per_usd)] {
            conn.execute(
                "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
                rusqlite::params![at, currency, units_per_eur],
            )
            .expect("rate");
        }
    }

    fn series(key: &str, ids: &[&str]) -> PerformanceSeriesSpec {
        PerformanceSeriesSpec {
            key: key.to_string(),
            investment_ids: ids.iter().map(|id| id.to_string()).collect(),
            currency: None,
        }
    }

    /// The report from `from` to `to` in CZK.
    fn perf(
        conn: &Connection,
        from: i64,
        to: i64,
        series: Vec<PerformanceSeriesSpec>,
    ) -> StockPerformance {
        stock_performance(
            conn,
            &StockPerformanceRequest {
                from_ts: from,
                to_ts: to,
                report_currency: "CZK".to_string(),
                series,
            },
        )
        .expect("performance")
    }

    fn close(actual: f64, expected: f64, tolerance: f64) {
        assert!(
            (actual - expected).abs() <= tolerance,
            "{actual} != {expected} (±{tolerance})"
        );
    }

    fn last<T>(values: &[T]) -> &T {
        values.last().expect("a last point")
    }

    #[test]
    fn portfolio_twr_matches_value_ratio_without_flows() {
        let conn = setup_db();
        seed_czk(&conn, "aapl", "AAPL", 10.0, &[100.0, 110.0]);

        let report = perf(&conn, day(0), day(1), vec![]);

        assert_eq!(report.currency, "CZK");
        assert_eq!(report.dates[0], ts_to_date_str(day(0)));
        assert_eq!(report.dates.len(), 2);
        assert_eq!(report.portfolio[0], 0.0);
        close(*last(&report.portfolio), 10.0, 1e-9);
        assert_eq!(
            report.values,
            [Some("1000.00".into()), Some("1100.00".into())]
        );
        assert_eq!(report.net_flows, ["0.00", "0.00"]);
        assert!(report.fx_quality.missing_currencies.is_empty());
    }

    #[test]
    fn a_deposit_is_not_return() {
        let conn = setup_db();
        // 10 shares at 100, then 10 more bought at an unchanged price
        seed(
            &conn,
            "aapl",
            "AAPL",
            "20",
            "CZK",
            &[(10.0, 100.0), (20.0, 100.0)],
        );

        let report = perf(&conn, day(0), day(1), vec![]);

        close(*last(&report.portfolio), 0.0, 1e-9);
        assert_eq!(last(&report.net_flows), "1000.00");
        close(*last(&report.mwr), 0.0, 1e-9);
        assert_eq!(last(&report.values), &Some("2000.00".to_string()));
    }

    /// Cumulative chain-linked return in percent of positions given as per-day
    /// `(quantity, price)` rows, computed independently of the engine.
    fn hand_twr(positions: &[&[(f64, f64)]]) -> Vec<f64> {
        let days = positions[0].len();
        let mut factor = 1.0;
        let mut out = vec![0.0];
        for k in 1..days {
            let (mut base, mut gain) = (0.0, 0.0);
            for rows in positions {
                let ((q0, p0), (q1, p1)) = (rows[k - 1], rows[k]);
                let flow = (q1 - q0) * p1;
                base += q0 * p0 + flow;
                gain += q1 * p1 - q0 * p0 - flow;
            }
            factor *= 1.0 + gain / base;
            out.push((factor - 1.0) * 100.0);
        }
        out
    }

    #[test]
    fn contributions_of_a_partition_sum_to_the_portfolio() {
        let conn = setup_db();
        let p1: Vec<(f64, f64)> = [100.0, 105.0, 110.0, 108.0, 120.0]
            .iter()
            .map(|price| (10.0, *price))
            .collect();
        // p2 gets 5 more shares on day 2 (a buy in the middle of the period)
        let p2: Vec<(f64, f64)> = [
            (10.0, 200.0),
            (10.0, 190.0),
            (15.0, 195.0),
            (15.0, 210.0),
            (15.0, 205.0),
        ]
        .to_vec();
        let p3: Vec<(f64, f64)> = [50.0, 52.0, 49.0, 55.0, 60.0]
            .iter()
            .map(|price| (20.0, *price))
            .collect();
        seed(&conn, "p1", "AAA", "10", "CZK", &p1);
        seed(&conn, "p2", "BBB", "15", "CZK", &p2);
        seed(&conn, "p3", "CCC", "20", "CZK", &p3);

        let report = perf(
            &conn,
            day(0),
            day(4),
            vec![series("a", &["p1"]), series("b", &["p2", "p3"])],
        );

        // the engine agrees with an independent hand computation
        let expected = hand_twr(&[&p1, &p2, &p3]);
        assert_eq!(report.portfolio.len(), 5);
        for (actual, expected) in report.portfolio.iter().zip(&expected) {
            close(*actual, *expected, 1e-9);
        }

        // contributions of the partition add up to the portfolio at every date
        let a = report.series[0]
            .contribution
            .as_ref()
            .expect("contribution a");
        let b = report.series[1]
            .contribution
            .as_ref()
            .expect("contribution b");
        for k in 0..5 {
            close(a[k] + b[k], report.portfolio[k], 1e-9);
        }
        close(last(a) + last(b), *last(&report.portfolio), 1e-9);
        assert_eq!(a[0], 0.0);

        // ... and so do the contributions of the single positions
        assert_eq!(report.positions.len(), 3);
        let sum: f64 = report.positions.iter().map(|p| p.contribution).sum();
        close(sum, *last(&report.portfolio), 1e-9);
        // a position's period return is its own chain
        let own = hand_twr(&[&p2]);
        let p2_report = report
            .positions
            .iter()
            .find(|p| p.investment_id == "p2")
            .expect("p2");
        close(p2_report.twr, *last(&own), 1e-9);
        assert_eq!(p2_report.start_value.as_deref(), Some("2000.00"));
        assert_eq!(p2_report.end_value.as_deref(), Some("3075.00"));
    }

    #[test]
    fn series_twr_is_its_own_chain() {
        let conn = setup_db();
        seed_czk(&conn, "p1", "AAA", 10.0, &[100.0, 102.0, 105.0]);
        seed_czk(&conn, "p2", "BBB", 10.0, &[100.0, 110.0, 99.0]);

        let report = perf(
            &conn,
            day(0),
            day(2),
            vec![series("p2", &["p2"]), series("both", &["p1", "p2"])],
        );

        // +10 % then -10 %: 1.1 * 0.9 - 1
        let p2_series = &report.series[0];
        assert_eq!(p2_series.key, "p2");
        close(p2_series.twr[1], 10.0, 1e-9);
        close(p2_series.twr[2], -1.0, 1e-9);
        // the whole portfolio is not p2: 200 -> 212 -> 204
        close(
            *last(&report.portfolio),
            (204.0 / 200.0 - 1.0) * 100.0,
            1e-9,
        );
        // a series with both positions chains the same as the portfolio
        for (both, portfolio) in report.series[1].twr.iter().zip(&report.portfolio) {
            close(*both, *portfolio, 1e-12);
        }
    }

    #[test]
    fn mwr_falls_below_twr_when_money_arrives_before_a_drop() {
        let conn = setup_db();
        // day 0: 10 @ 100; day 1: 10 @ 120 (+20 %); day 2: 90 bought at 120 (100 shares, no
        // price move); day 3: 100 @ 96 (-20 %)
        seed(
            &conn,
            "aapl",
            "AAPL",
            "100",
            "CZK",
            &[(10.0, 100.0), (10.0, 120.0), (100.0, 120.0), (100.0, 96.0)],
        );

        let report = perf(&conn, day(0), day(3), vec![]);

        // TWR = 1.2 * 1.0 * 0.8 - 1
        close(*last(&report.portfolio), -4.0, 1e-9);
        // V0 = 1000, F = 90 * 120 = 10800 on day 2 (held for 2 of the 3 days), V3 = 9600
        let expected = (9600.0 - 1000.0 - 10_800.0) / (1000.0 + 10_800.0 * 2.0 / 3.0) * 100.0;
        close(*last(&report.mwr), expected, 1e-9);
        close(*last(&report.mwr), -26.83, 0.01);
        assert!(last(&report.mwr) < last(&report.portfolio));
        assert_eq!(report.mwr[0], 0.0);
        assert_eq!(last(&report.net_flows), "10800.00");
    }

    #[test]
    fn native_currency_series_excludes_the_rate_move() {
        let conn = setup_db();
        seed(
            &conn,
            "usd",
            "USDCO",
            "10",
            "USD",
            &[(10.0, 100.0), (10.0, 100.0)],
        );
        add_usd_czk(&conn, day(0), 20.0);
        add_usd_czk(&conn, day(1), 21.0);

        let mut native = series("native", &["usd"]);
        native.currency = Some("usd".to_string());
        let report = perf(&conn, day(0), day(1), vec![series("czk", &["usd"]), native]);

        // 1000 USD at 20 -> 21 CZK is +5 % in the report currency ...
        close(*last(&report.portfolio), 5.0, 1e-9);
        close(*last(&report.series[0].twr), 5.0, 1e-9);
        assert!(report.series[0].contribution.is_some());
        // ... and 0 in the position's own currency, which has no contribution
        assert_eq!(report.series[1].key, "native");
        assert_eq!(report.series[1].twr, [0.0, 0.0]);
        assert_eq!(report.series[1].contribution, None);
        assert!(report.fx_quality.missing_currencies.is_empty());
    }

    #[test]
    fn range_is_clamped_to_history() {
        let conn = setup_db();
        seed_czk(&conn, "aapl", "AAPL", 10.0, &[100.0, 105.0, 110.0]);

        // a transient absurd range (typing a year passes through year 0002)
        let report = perf(&conn, -62_000_000_000, 253_402_300_799, vec![]);

        assert_eq!(report.dates, [day(0), day(1), day(2)].map(ts_to_date_str));
        close(*last(&report.portfolio), 10.0, 1e-9);

        // a range inside the history starts on its own first day
        let inner = perf(&conn, day(1), day(2), vec![]);
        assert_eq!(inner.dates, [day(1), day(2)].map(ts_to_date_str));
        close(*last(&inner.portfolio), (110.0 / 105.0 - 1.0) * 100.0, 1e-9);
    }

    #[test]
    fn emits_at_most_400_points_and_always_the_last_day() {
        let conn = setup_db();
        let prices: Vec<f64> = (0..1000).map(|i| 100.0 + f64::from(i % 17)).collect();
        seed_czk(&conn, "aapl", "AAPL", 10.0, &prices);

        let report = perf(&conn, day(0), day(999), vec![series("all", &["aapl"])]);

        assert!(report.dates.len() <= 401, "{} points", report.dates.len());
        assert!(report.dates.len() > 300);
        assert_eq!(report.dates[0], ts_to_date_str(day(0)));
        assert_eq!(last(&report.dates), &ts_to_date_str(day(999)));
        // every column has one entry per emitted date
        let n = report.dates.len();
        assert_eq!(report.portfolio.len(), n);
        assert_eq!(report.values.len(), n);
        assert_eq!(report.net_flows.len(), n);
        assert_eq!(report.mwr.len(), n);
        assert_eq!(report.series[0].twr.len(), n);
        assert_eq!(
            report.series[0].contribution.as_ref().map(Vec::len),
            Some(n)
        );
        // sampling skips days, never accuracy: the last point is the full chain
        close(
            *last(&report.portfolio),
            (prices[999] / prices[0] - 1.0) * 100.0,
            1e-9,
        );

        // a short history emits every day
        let short = perf(&conn, day(0), day(9), vec![]);
        assert_eq!(short.dates.len(), 10);
    }

    #[test]
    fn closed_positions_and_unknown_ids_are_ignored() {
        let conn = setup_db();
        seed_czk(&conn, "held", "HELD", 10.0, &[100.0, 110.0]);
        // sold out: stored quantity 0, so it never counts however its history moves
        seed(
            &conn,
            "sold",
            "SOLD",
            "0",
            "CZK",
            &[(10.0, 100.0), (10.0, 300.0)],
        );

        let report = perf(
            &conn,
            day(0),
            day(1),
            vec![
                series("mixed", &["held", "sold", "nope"]),
                series("none", &["sold"]),
            ],
        );

        assert_eq!(report.positions.len(), 1);
        assert_eq!(report.positions[0].investment_id, "held");
        close(*last(&report.portfolio), 10.0, 1e-9);
        close(*last(&report.series[0].twr), 10.0, 1e-9);
        // a series without a held position is flat
        assert_eq!(report.series[1].twr, [0.0, 0.0]);
        assert_eq!(report.series[1].contribution, Some(vec![0.0, 0.0]));
    }

    #[test]
    fn unconvertible_holdings_drop_out_of_the_day_and_are_counted() {
        // CZ (a CZK stock) goes 100 -> 110; GOLD is priced in XAU, which has no quote at all
        let conn = setup_db();
        seed_czk(&conn, "cz", "CZ", 10.0, &[100.0, 110.0]);
        seed(
            &conn,
            "gold",
            "GOLD",
            "10",
            "XAU",
            &[(10.0, 2000.0), (10.0, 2100.0)],
        );
        for at in [day(0), day(1)] {
            conn.execute(
                "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'CZK', 25.0)",
                [at],
            )
            .expect("rate");
        }

        let report = perf(&conn, day(0), day(1), vec![]);

        close(*last(&report.portfolio), 10.0, 1e-9);
        assert_eq!(report.fx_quality.missing_currencies, ["XAU"]);
        assert!(report.fx_quality.missing_count > 0);
        // the value counts the holdings that convert
        assert_eq!(last(&report.values), &Some("1100.00".to_string()));
        let gold = report
            .positions
            .iter()
            .find(|p| p.investment_id == "gold")
            .expect("gold");
        assert_eq!(gold.start_value, None);
        assert_eq!(gold.end_value, None);
    }

    #[test]
    fn without_held_positions_the_report_is_one_flat_point() {
        let conn = setup_db();
        seed(
            &conn,
            "sold",
            "SOLD",
            "0",
            "CZK",
            &[(10.0, 100.0), (10.0, 300.0)],
        );

        let report = perf(&conn, day(0), day(1), vec![series("a", &["sold"])]);

        assert_eq!(report.dates, [ts_to_date_str(day(0))]);
        assert_eq!(report.portfolio, [0.0]);
        assert_eq!(report.values, [None]);
        assert_eq!(report.net_flows, ["0.00"]);
        assert_eq!(report.mwr, [0.0]);
        assert!(report.positions.is_empty());
    }

    #[test]
    fn rejects_an_invalid_report_currency() {
        let conn = setup_db();
        let request = StockPerformanceRequest {
            from_ts: day(0),
            to_ts: day(1),
            report_currency: "??".to_string(),
            series: vec![],
        };
        assert!(stock_performance(&conn, &request).is_err());
    }
}
