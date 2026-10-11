//! Performance of the held stock positions over a period, for the stocks analysis page.
//!
//! A daily engine chain-links the time-weighted return over every held position at once. For
//! each position `p` and day `s` of the range, in the
//! report currency (values and flows converted at their own day):
//!
//! - `base_p(s) = V_p(s-1) + F_p(s)` and `gain_p(s) = V_p(s) - V_p(s-1) - F_p(s)`, where `V` is
//!   the value and `F` the flow (quantity change times the day's native price);
//! - the quantity of a day comes from the transactions (the running buys minus sells dated at or
//!   before the day, as `ticker_history` plans the rows), the native price and its currency from
//!   the last `stock_value_history` row at or before the day. A day without a position has no
//!   row (a rebuild deletes it), so the quantity must not be read from the last row: it would
//!   carry the old quantity through the gap and book a re-buy as a sale. A sell-out is priced
//!   with the last row before it. Prices before the first recorded day are filled backwards
//!   from it: a position whose transactions start before its first row is valued flat at that
//!   first price until real prices exist, so its buy is a deposit and its first row does not
//!   bring a whole value in without a flow. Only a position with no row at all has an unknown
//!   value, and then the holding drops out of that day like a failed conversion (without a
//!   counted miss: no conversion was attempted);
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
//!
//! The optional comparison line is a market index fetched on demand (`benchmark_series`): its
//! daily closes are carried over non-trading days, converted at each day's rate and rebased to
//! the first day, so it moves with the exchange rate the same way a holding does.

use std::collections::HashMap;

use chrono::DateTime;
use rusqlite::types::Value;
use rusqlite::{params_from_iter, Connection};

use crate::error::{AppError, Result};
use crate::models::fx::ConversionQuality;
use crate::models::{
    PerformanceSeries, PerformanceSeriesSpec, PositionPerformance, StockBenchmark,
    StockPerformance, StockPerformanceRequest,
};
use crate::services::fx::{History, Quality, DAY};
use crate::services::price_api::HistoricalPrice;
use crate::services::reporting::{money_in_currency, normalize_currency};

/// Most dates a report emits, the first and the last day included.
const MAX_POINTS: usize = 400;
/// A base or quantity at or below this is zero.
const EPSILON: f64 = 1e-9;

/// Convert a Unix timestamp to a "YYYY-MM-DD" date string (UTC).
fn ts_to_date_str(ts: i64) -> String {
    DateTime::from_timestamp(ts, 0)
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// One recorded day of a ticker's value history: the native price and its currency. The
/// quantity of a day comes from the transactions (`Quantities`), not from the row.
#[derive(Debug, Clone)]
struct DayHolding {
    ts: i64,
    price: f64,
    currency: String,
}

/// The most recent recorded holding of a ticker at or before `target_ts`.
///
/// Uses binary search — O(log N) — so entries must be sorted ascending by timestamp
/// (as guaranteed by the SQL `ORDER BY ticker, recorded_at ASC` in `load_day_holdings`).
fn lookup_ticker_at<'a>(
    history: &'a HashMap<String, Vec<DayHolding>>,
    ticker: &str,
    target_ts: i64,
) -> Option<&'a DayHolding> {
    let entries = history.get(ticker)?;
    // partition_point returns the index of the first element where ts > target_ts,
    // so the last element at or before target_ts is at idx-1 (if idx > 0).
    let idx = entries.partition_point(|entry| entry.ts <= target_ts);
    idx.checked_sub(1).map(|i| &entries[i])
}

/// The native price known for `ticker` on `target_ts`: the last row at or before it, else the
/// position's first row, which fills the days before it backwards. When no row is at or before
/// a day of the range, the ticker has none before the range either (the load keeps the last row
/// before it), so its first loaded row is the first it has ever recorded, or the one
/// `load_first_prices` added from beyond the range. `None` only for a ticker without any row.
fn price_on<'a>(
    history: &'a HashMap<String, Vec<DayHolding>>,
    ticker: &str,
    target_ts: i64,
) -> Option<&'a DayHolding> {
    lookup_ticker_at(history, ticker, target_ts).or_else(|| history.get(ticker)?.first())
}

/// The report currency of a performance computation and the day quotes it converts with.
struct ReportFx<'a> {
    currency: &'a str,
    history: &'a History,
}

/// The recorded holdings of `tickers` in `from_ts..=to_ts` plus each ticker's last row before
/// `from_ts` (the base of the first day), ascending by day per ticker.
fn load_day_holdings(
    conn: &rusqlite::Connection,
    tickers: &[String],
    from_ts: i64,
    to_ts: i64,
) -> Result<HashMap<String, Vec<DayHolding>>> {
    let mut history: HashMap<String, Vec<DayHolding>> = HashMap::new();
    if tickers.is_empty() {
        return Ok(history);
    }

    // Build SQL with two IN clause copies (tickers appear twice: once for main range, once for pre-range lookup)
    // params: ?1=from_ts, ?2=to_ts, ?3..?(n+2)=tickers (main range), ?(n+3)..?(2n+2)=tickers (pre-range)
    let n = tickers.len();
    let main_placeholders: Vec<String> = (3..=n + 2).map(|i| format!("?{i}")).collect();
    let pre_placeholders: Vec<String> = (n + 3..=2 * n + 2).map(|i| format!("?{i}")).collect();
    let sql = format!(
        "SELECT ticker, recorded_at, CAST(price AS REAL), currency \
         FROM stock_value_history \
         WHERE ticker IN ({main}) AND recorded_at >= ?1 AND recorded_at <= ?2 \
         UNION ALL \
         SELECT s.ticker, s.recorded_at, CAST(s.price AS REAL), s.currency \
         FROM stock_value_history s \
         WHERE s.ticker IN ({pre}) \
           AND s.recorded_at = ( \
               SELECT MAX(recorded_at) FROM stock_value_history \
               WHERE ticker = s.ticker AND recorded_at < ?1 \
           ) \
         ORDER BY ticker, recorded_at ASC",
        main = main_placeholders.join(", "),
        pre = pre_placeholders.join(", "),
    );

    let params: Vec<Value> = std::iter::once(Value::Integer(from_ts))
        .chain(std::iter::once(Value::Integer(to_ts)))
        .chain(tickers.iter().map(|t| Value::Text(t.clone()))) // main range
        .chain(tickers.iter().map(|t| Value::Text(t.clone()))) // pre-range
        .collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params_from_iter(params), |row| {
        Ok((
            row.get::<_, String>(0)?,
            DayHolding {
                ts: row.get(1)?,
                price: row.get(2)?,
                currency: row.get(3)?,
            },
        ))
    })?;
    for row in rows {
        let (ticker, holding) = row?;
        history.entry(ticker).or_default().push(holding);
    }
    Ok(history)
}

/// Adds the first recorded row of every ticker `history` has no row for, which is the backward
/// fill of a ticker whose recorded prices all start after the range (`price_on`). A ticker that
/// has rows in or before the range already has its first row in `history`; one without any row
/// stays unpriced.
fn load_first_prices(
    conn: &Connection,
    tickers: &[String],
    history: &mut HashMap<String, Vec<DayHolding>>,
) -> Result<()> {
    let unpriced: Vec<&String> = tickers
        .iter()
        .filter(|ticker| !history.contains_key(*ticker))
        .collect();
    if unpriced.is_empty() {
        return Ok(());
    }
    let placeholders = vec!["?"; unpriced.len()].join(", ");
    let mut stmt = conn.prepare(&format!(
        "SELECT s.ticker, s.recorded_at, CAST(s.price AS REAL), s.currency \
         FROM stock_value_history s \
         WHERE s.ticker IN ({placeholders}) \
           AND s.recorded_at = ( \
               SELECT MIN(recorded_at) FROM stock_value_history WHERE ticker = s.ticker \
           )"
    ))?;
    let rows = stmt.query_map(params_from_iter(unpriced), |row| {
        Ok((
            row.get::<_, String>(0)?,
            DayHolding {
                ts: row.get(1)?,
                price: row.get(2)?,
                currency: row.get(3)?,
            },
        ))
    })?;
    for row in rows {
        let (ticker, holding) = row?;
        history.entry(ticker).or_default().push(holding);
    }
    Ok(())
}

/// The day quotes a report of `from_ts..=to_ts` converts with: the requested range limited to
/// the span of `stock_value_history` (a transient absurd range loads nothing extra), plus the
/// day before it, which is the base of the first day's return.
fn load_fx_history(conn: &rusqlite::Connection, from_ts: i64, to_ts: i64) -> Result<History> {
    let (first, last): (Option<i64>, Option<i64>) = conn.query_row(
        "SELECT MIN(recorded_at), MAX(recorded_at) FROM stock_value_history",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let (Some(first), Some(last)) = (first, last) else {
        return Ok(History::default());
    };
    let start = from_ts.max(first) - 86400;
    let end = to_ts.min(last);
    History::load(conn, start, end.max(start))
}

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

/// The shares of each ticker held on a day, from the transactions: the running sum of buys minus
/// sells dated at or before the day, clamped at zero. This is the rule `ticker_history` rebuilds
/// the value rows with (`load_position_changes`), so a day on which nothing is held reads 0
/// even though its row was deleted.
struct Quantities {
    /// Per ticker, ascending by date: the running position after each transaction.
    steps: HashMap<String, Vec<(i64, f64)>>,
}

impl Quantities {
    /// Every transaction of `tickers`, in one query. `CAST(.. AS REAL)` keeps SQLite's leniency
    /// about malformed quantity text, as `ticker_history` does.
    fn load(conn: &Connection, tickers: &[String]) -> Result<Self> {
        let mut steps: HashMap<String, Vec<(i64, f64)>> = HashMap::new();
        if tickers.is_empty() {
            return Ok(Self { steps });
        }
        let placeholders = vec!["?"; tickers.len()].join(", ");
        let mut stmt = conn.prepare(&format!(
            "SELECT ticker, transaction_date,
                    CASE WHEN type = 'buy' THEN CAST(quantity AS REAL)
                         ELSE -CAST(quantity AS REAL) END
             FROM investment_transactions
             WHERE ticker IN ({placeholders})
             ORDER BY ticker, transaction_date"
        ))?;
        let rows = stmt.query_map(params_from_iter(tickers), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?;
        for row in rows {
            let (ticker, date, change) = row?;
            let running = steps.entry(ticker).or_default();
            let position = running.last().map_or(0.0, |&(_, position)| position);
            running.push((date, position + change));
        }
        Ok(Self { steps })
    }

    /// The shares of `ticker` held on `day` (midnight UTC); 0 when nothing is held.
    fn at(&self, ticker: &str, day: i64) -> f64 {
        let Some(steps) = self.steps.get(ticker) else {
            return 0.0;
        };
        let applied = steps.partition_point(|&(date, _)| date <= day);
        let position = applied.checked_sub(1).map_or(0.0, |i| steps[i].1);
        if position > EPSILON {
            position
        } else {
            0.0
        }
    }
}

/// The value of `quantity` shares priced with `price` (the last row at or before `at`),
/// converted at `at`. No shares are worth nothing and need neither a price nor a rate; shares
/// without any price row have an unknown value (`None`, no conversion was attempted).
fn value_at(
    fx: &ReportFx<'_>,
    quantity: f64,
    price: Option<&DayHolding>,
    at: i64,
    quality: &mut Quality,
) -> Option<f64> {
    if quantity <= EPSILON {
        return Some(0.0);
    }
    let h = price?;
    fx.history
        .convert_on_tracked(quality, quantity * h.price, &h.currency, fx.currency, at)
}

/// The cash flow of a day: the change of the quantity priced with the day's own native price
/// (a sold-out day has no row, so it is the last row before it) and converted at that day.
/// `None` when the price is unknown or the conversion failed.
fn flow_at(
    fx: &ReportFx<'_>,
    delta: f64,
    price: Option<&DayHolding>,
    day: i64,
    quality: &mut Quality,
) -> Option<f64> {
    if delta.abs() <= EPSILON {
        return Some(0.0);
    }
    let h = price?;
    fx.history
        .convert_on_tracked(quality, delta * h.price, &h.currency, fx.currency, day)
}

/// The daily moves of `tickers` on `days` (consecutive days, the first is the base), converted
/// into `fx.currency`. Every value is converted once, as it was when it was "today".
fn build_daily(
    history: &HashMap<String, Vec<DayHolding>>,
    quantities: &Quantities,
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
        let mut prev_quantity = 0.0;
        let mut prev_value: Option<f64> = None;
        for (k, &day) in days.iter().enumerate() {
            let quantity = quantities.at(ticker, day);
            let price = price_on(history, ticker, day);
            let value = value_at(fx, quantity, price, day, quality);
            daily.value[k][p] = value;
            if k == 0 {
                daily.flow[k][p] = Some(0.0);
            } else if let (Some(v), Some(v_prev)) = (value, prev_value) {
                // A holding that cannot be converted drops out of this day only; the miss is
                // already recorded.
                if let Some(flow) = flow_at(fx, quantity - prev_quantity, price, day, quality) {
                    daily.flow[k][p] = Some(flow);
                    daily.base[k][p] = Some(v_prev + flow);
                    daily.gain[k][p] = Some(v - v_prev - flow);
                }
            }
            prev_quantity = quantity;
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

/// The days to emit out of `count`: the first, every n-th and the last, at most `MAX_POINTS` in
/// all. Chaining ran every day; only the output is sampled.
fn emitted_indices(count: usize) -> Vec<usize> {
    let every = count.saturating_sub(1).div_ceil(MAX_POINTS - 1).max(1);
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
    let mut history = load_day_holdings(conn, &tickers, request.from_ts, request.to_ts)?;
    let (from_ts, to_ts) = match history_span(&history) {
        Some((earliest, latest)) => {
            let from_ts = request.from_ts.max(earliest);
            (from_ts, request.to_ts.min(latest).max(from_ts))
        }
        None => (request.from_ts, request.from_ts),
    };
    // after the span: a first row beyond the range only prices, it does not widen the range
    load_first_prices(conn, &tickers, &mut history)?;
    let days = day_range(from_ts, to_ts);
    let fx_history = load_fx_history(conn, from_ts, to_ts)?;
    let fx = ReportFx {
        currency: &currency,
        history: &fx_history,
    };
    let mut quality = Quality::default();

    let quantities = Quantities::load(conn, &tickers)?;
    let ticker_refs: Vec<&str> = tickers.iter().map(String::as_str).collect();
    let daily = build_daily(
        &history,
        &quantities,
        &ticker_refs,
        &days,
        &fx,
        &mut quality,
    );
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
                let native_daily = build_daily(
                    &history,
                    &quantities,
                    &member_tickers,
                    &days,
                    &native_fx,
                    &mut quality,
                );
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

/// Longest ticker a benchmark may have.
const MAX_TICKER_LEN: usize = 24;

/// The ticker of a benchmark, trimmed and uppercased: 1 to 24 characters of `A-Z 0-9 . - ^ =`
/// (`CSPX.L`, `^GSPC`, `EURUSD=X`). The ticker goes into a request to a third party, so only
/// what a Yahoo symbol can contain passes.
pub fn validate_benchmark_ticker(ticker: &str) -> Result<String> {
    let ticker = ticker.trim();
    let valid = !ticker.is_empty()
        && ticker.len() <= MAX_TICKER_LEN
        && ticker
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '^' | '='));
    if valid {
        Ok(ticker.to_ascii_uppercase())
    } else {
        Err(AppError::Validation(
            "validation.benchmarkTickerInvalid".into(),
        ))
    }
}

/// The start of the UTC day `ts` falls on.
fn day_start(ts: i64) -> i64 {
    ts.div_euclid(DAY) * DAY
}

/// The closes that can be used (finite and positive), oldest first.
fn usable_closes(prices: &[HistoricalPrice]) -> Vec<&HistoricalPrice> {
    let mut closes: Vec<&HistoricalPrice> = prices
        .iter()
        .filter(|close| close.price.is_finite() && close.price > 0.0)
        .collect();
    closes.sort_by_key(|close| close.timestamp);
    closes
}

/// The close known at the end of `day`: the latest one on or before it, so a Saturday carries
/// Friday's close forward and a day before the first close has none.
fn close_on<'a>(closes: &[&'a HistoricalPrice], day: i64) -> Option<&'a HistoricalPrice> {
    let known = closes.partition_point(|close| close.timestamp < day + DAY);
    known.checked_sub(1).map(|i| closes[i])
}

/// The days a benchmark covers: `from_ts..=to_ts` limited to the days between the first and the
/// last close, because nothing exists to carry before the first one and a transient absurd range
/// (typing a year in a date input) must not run for millions of iterations while holding the DB
/// lock. Empty without closes.
fn benchmark_days(closes: &[&HistoricalPrice], from_ts: i64, to_ts: i64) -> Vec<i64> {
    let (Some(first), Some(last)) = (closes.first(), closes.last()) else {
        return Vec::new();
    };
    let start = day_start(from_ts).max(day_start(first.timestamp));
    let end = day_start(to_ts).min(day_start(last.timestamp)).max(start);
    (start..=end).step_by(DAY as usize).collect()
}

/// The cumulative return in percent of a market index (or any ticker) over a period, in the
/// report currency, for the comparison line of the performance chart.
///
/// `prices` are the fetched daily closes; they need not be sorted and carry their own currency.
/// Every day of the period (limited to the days that have closes) is valued with the latest
/// close known on it, converted at that day's rate, so a foreign index carries the exchange rate
/// like a holding does. The base is the value on the first day: the last close on or before
/// `from_ts`, or the index's first close when it started trading later. A day that cannot be
/// converted is left out and counted in `fxQuality`; the first convertible day is the base (0).
/// Without closes the series is empty.
pub fn benchmark_series(
    conn: &Connection,
    ticker: &str,
    prices: &[HistoricalPrice],
    from_ts: i64,
    to_ts: i64,
    report_currency: &str,
) -> Result<StockBenchmark> {
    let currency = normalize_currency(report_currency)?;
    let closes = usable_closes(prices);
    let days = benchmark_days(&closes, from_ts, to_ts);
    let mut quality = Quality::default();
    let (mut dates, mut twr) = (Vec::new(), Vec::new());

    if let (Some(&first), Some(&last)) = (days.first(), days.last()) {
        let fx_history = History::load(conn, first, last)?;
        let mut base: Option<f64> = None;
        for day in days {
            let Some(close) = close_on(&closes, day) else {
                continue;
            };
            let Some(value) = fx_history.convert_on_tracked(
                &mut quality,
                close.price,
                &close.currency,
                &currency,
                day,
            ) else {
                continue;
            };
            if base.is_none() && value > EPSILON {
                base = Some(value);
            }
            if let Some(base) = base {
                dates.push(ts_to_date_str(day));
                twr.push((value / base - 1.0) * 100.0);
            }
        }
    }

    Ok(StockBenchmark {
        ticker: ticker.to_string(),
        currency,
        fx_quality: ConversionQuality::from(&quality),
        dates,
        twr,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PerformanceSeriesSpec;

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
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                ticker TEXT NOT NULL,
                quantity TEXT NOT NULL,
                transaction_date INTEGER NOT NULL
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

    fn insert_investment(conn: &Connection, id: &str, ticker: &str, held: &str) {
        conn.execute(
            "INSERT INTO stock_investments (id, ticker, quantity) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, ticker, held],
        )
        .expect("investment");
    }

    /// One recorded day of the value history on `day(at)`.
    fn insert_row(
        conn: &Connection,
        ticker: &str,
        at: i64,
        quantity: f64,
        price: f64,
        currency: &str,
    ) {
        conn.execute(
            "INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![
                format!("{ticker}-{at}"),
                ticker,
                day(at),
                quantity.to_string(),
                price.to_string(),
                currency
            ],
        )
        .expect("history row");
    }

    /// A buy (`quantity > 0`) or a sell (`quantity < 0`) of `quantity` shares on `day(at)`.
    fn insert_tx(conn: &Connection, ticker: &str, at: i64, quantity: f64) {
        conn.execute(
            "INSERT INTO investment_transactions (id, type, ticker, quantity, transaction_date)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![
                uuid::Uuid::new_v4().to_string(),
                if quantity > 0.0 { "buy" } else { "sell" },
                ticker,
                quantity.abs().to_string(),
                day(at)
            ],
        )
        .expect("transaction");
    }

    /// A position `id` of `ticker` whose recorded days are `rows[i] = (quantity, price)` on
    /// `day(i)`, with the transactions those quantities imply (a buy or sell on the day the
    /// quantity changes). `held` is the stored quantity (0 closes the position).
    fn seed(
        conn: &Connection,
        id: &str,
        ticker: &str,
        held: &str,
        currency: &str,
        rows: &[(f64, f64)],
    ) {
        insert_investment(conn, id, ticker, held);
        let mut previous = 0.0;
        for (i, (quantity, price)) in rows.iter().enumerate() {
            let at = i as i64;
            insert_row(conn, ticker, at, *quantity, *price, currency);
            if (quantity - previous).abs() > 1e-9 {
                insert_tx(conn, ticker, at, quantity - previous);
            }
            previous = *quantity;
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
    fn a_flow_in_another_currency_is_converted_at_its_own_day() {
        // 10 units on day 0 (20000 CZK), 10 more bought on day 1 at an unchanged price while
        // USD/CZK goes 20 -> 25. The purchase is a flow of 1000 USD = 25000 CZK at its own
        // day (not 20000), so day 1 returns (50000 - 20000 - 25000) / (20000 + 25000).
        let conn = setup_db();
        seed(
            &conn,
            "usd",
            "USDCO",
            "20",
            "USD",
            &[(10.0, 100.0), (20.0, 100.0), (20.0, 100.0)],
        );
        for (at, rate) in [(0, 20.0), (1, 25.0), (2, 25.0)] {
            add_usd_czk(&conn, day(at), rate);
        }

        let report = perf(&conn, day(0), day(2), vec![]);

        let expected = 100.0 * 5000.0 / 45000.0;
        close(report.portfolio[1], expected, 1e-9);
        close(report.portfolio[2], expected, 1e-9);
        assert_eq!(report.net_flows, ["0.00", "25000.00", "25000.00"]);
        assert_eq!(last(&report.values), &Some("50000.00".to_string()));
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

    /// 1000 days whose price moves every day and whose position grows from 10 to 15 shares on
    /// day 500 (a deposit in the middle of the period).
    fn seed_long_history(conn: &Connection) -> Vec<f64> {
        let prices: Vec<f64> = (0..1000).map(|i| 100.0 + f64::from(i % 17)).collect();
        let rows: Vec<(f64, f64)> = prices
            .iter()
            .enumerate()
            .map(|(i, price)| (if i < 500 { 10.0 } else { 15.0 }, *price))
            .collect();
        seed(conn, "aapl", "AAPL", "15", "CZK", &rows);
        prices
    }

    #[test]
    fn emits_at_most_400_points_and_always_the_last_day() {
        let conn = setup_db();
        let prices = seed_long_history(&conn);

        let report = perf(&conn, day(0), day(999), vec![series("all", &["aapl"])]);

        // 1000 days emit every third one (0, 3, ..., 999), the last day included
        assert!(report.dates.len() <= 400, "{} points", report.dates.len());
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
        // (the deposit on day 500 is not return)
        let hand = hand_twr(&[&(0..1000)
            .map(|i| (if i < 500 { 10.0 } else { 15.0 }, prices[i]))
            .collect::<Vec<_>>()]);
        close(*last(&report.portfolio), *last(&hand), 1e-9);

        // a short history emits every day
        let short = perf(&conn, day(0), day(9), vec![]);
        assert_eq!(short.dates.len(), 10);
    }

    #[test]
    fn an_emitted_index_is_the_same_day_in_every_column() {
        let conn = setup_db();
        seed_long_history(&conn);
        let ids = series("all", &["aapl"]);

        let report = perf(&conn, day(0), day(999), vec![ids.clone()]);

        // every third day is emitted; each emitted index must equal the figures of a report
        // that ends on exactly that day (its last point is that day, whatever the sampling)
        let step = 3;
        assert_eq!(report.dates.len(), 334);
        let contribution = report.series[0]
            .contribution
            .as_ref()
            .expect("contribution");
        for i in [1, 50, 166, 167, 200, 300, 333] {
            let at = i as i64 * step;
            assert_eq!(report.dates[i], ts_to_date_str(day(at)), "date {i}");

            let until = perf(&conn, day(0), day(at), vec![ids.clone()]);
            assert_eq!(last(&until.dates), &report.dates[i], "date {i}");
            close(*last(&until.portfolio), report.portfolio[i], 1e-9);
            assert_eq!(last(&until.values), &report.values[i], "values {i}");
            assert_eq!(last(&until.net_flows), &report.net_flows[i], "netFlows {i}");
            close(*last(&until.mwr), report.mwr[i], 1e-9);
            close(*last(&until.series[0].twr), report.series[0].twr[i], 1e-9);
            close(
                *last(until.series[0].contribution.as_ref().expect("contribution")),
                contribution[i],
                1e-9,
            );
        }
        // the deposit on day 500 sits between emitted days 166 (day 498) and 167 (day 501)
        assert_eq!(report.net_flows[166], "0.00");
        assert_ne!(report.net_flows[167], "0.00");
    }

    #[test]
    fn emitted_indices_never_exceed_the_cap_and_keep_both_ends() {
        for count in [
            0, 1, 2, 3, 399, 400, 401, 402, 798, 799, 800, 801, 1000, 1001, 5000, 123_457,
        ] {
            let indices = emitted_indices(count);
            assert!(indices.len() <= MAX_POINTS, "{count}: {}", indices.len());
            if count == 0 {
                assert!(indices.is_empty());
                continue;
            }
            assert_eq!(indices[0], 0, "{count}");
            assert_eq!(*last(&indices), count - 1, "{count}");
            assert!(indices.windows(2).all(|w| w[0] < w[1]), "{count}");
        }
        // a range that fits is emitted whole
        assert_eq!(emitted_indices(400), (0..400).collect::<Vec<_>>());
    }

    #[test]
    fn a_sold_out_gap_is_not_a_return_and_a_rebuy_is_not_a_sale() {
        let conn = setup_db();
        insert_investment(&conn, "aapl", "AAPL", "5");
        // buy 10 on day 1, sell them on day 10, buy 5 on day 20; the history has no row for
        // the days the position is not held (10..=19)
        insert_tx(&conn, "AAPL", 1, 10.0);
        insert_tx(&conn, "AAPL", 10, -10.0);
        insert_tx(&conn, "AAPL", 20, 5.0);
        for at in 1..=9 {
            insert_row(&conn, "AAPL", at, 10.0, 100.0, "CZK");
        }
        for at in 20..=25 {
            insert_row(&conn, "AAPL", at, 5.0, 130.0, "CZK");
        }

        let report = perf(&conn, day(1), day(25), vec![series("a", &["aapl"])]);

        assert_eq!(report.dates.len(), 25);
        // no price moved while the position was held: no return anywhere
        for (i, twr) in report.portfolio.iter().enumerate() {
            close(*twr, 0.0, 1e-9);
            close(report.series[0].twr[i], 0.0, 1e-9);
        }
        // the sale at day 9's price (-1000 on day 10) and the rebuy at 130 (+650 on day 20)
        assert_eq!(last(&report.net_flows), "-350.00");
        assert_eq!(report.net_flows[8], "0.00");
        assert_eq!(report.net_flows[9], "-1000.00");
        assert_eq!(report.net_flows[18], "-1000.00");
        assert_eq!(report.net_flows[19], "-350.00");
        // worth nothing while nothing was held, whatever the stale row says
        assert_eq!(report.values[8], Some("1000.00".to_string()));
        for i in 9..19 {
            assert_eq!(report.values[i], Some("0.00".to_string()), "day {}", i + 1);
        }
        assert_eq!(last(&report.values), &Some("650.00".to_string()));
        // the position's own figures agree
        assert_eq!(report.positions.len(), 1);
        close(report.positions[0].twr, 0.0, 1e-9);
        close(report.positions[0].contribution, 0.0, 1e-9);
        assert_eq!(report.positions[0].start_value.as_deref(), Some("1000.00"));
        assert_eq!(report.positions[0].end_value.as_deref(), Some("650.00"));
        assert!(report.fx_quality.missing_currencies.is_empty());
    }

    #[test]
    fn the_quantity_comes_from_the_transactions_not_from_the_last_row() {
        let conn = setup_db();
        insert_investment(&conn, "aapl", "AAPL", "3");
        // two buys on the same day and a partial sell: 4 + 2 - 3 shares from day 2 on
        insert_tx(&conn, "AAPL", 0, 4.0);
        insert_tx(&conn, "AAPL", 0, 2.0);
        insert_tx(&conn, "AAPL", 2, -3.0);
        // the rows keep a stale quantity of 6; only their price counts
        for (at, price) in [(0, 100.0), (1, 110.0), (2, 110.0), (3, 110.0)] {
            insert_row(&conn, "AAPL", at, 6.0, price, "CZK");
        }

        let report = perf(&conn, day(0), day(3), vec![]);

        assert_eq!(
            report.values,
            [
                Some("600.00".into()),
                Some("660.00".into()),
                Some("330.00".into()),
                Some("330.00".into())
            ]
        );
        // the partial sell at 110 is a flow of -330, not a loss: +10 % then +0 %
        assert_eq!(report.net_flows, ["0.00", "0.00", "-330.00", "-330.00"]);
        close(*last(&report.portfolio), 10.0, 1e-9);
    }

    /// A decimal TEXT figure of the report as a number.
    fn number(text: &str) -> f64 {
        text.parse().expect("a decimal figure")
    }

    /// A portfolio value of the report as a number.
    fn value_of(value: &Option<String>) -> f64 {
        number(value.as_deref().expect("a known value"))
    }

    #[test]
    fn a_holding_without_early_prices_is_valued_at_its_first_price() {
        let conn = setup_db();
        // a flat anchor with prices from day 0 opens the range on day 0 (a range starts at the
        // first recorded day of any held position)
        seed_czk(&conn, "anchor", "ANCH", 5.0, &[20.0; 11]);
        // 10 shares bought on day 1, but their prices are recorded only from day 5 to day 10
        insert_investment(&conn, "late", "LATE", "10");
        insert_tx(&conn, "LATE", 1, 10.0);
        for at in 5..=10 {
            insert_row(&conn, "LATE", at, 10.0, 100.0, "CZK");
        }

        let report = perf(&conn, day(0), day(10), vec![series("late", &["late"])]);

        assert_eq!(report.dates.len(), 11);
        // valued at its first known price from the buy on, not unknown until day 5
        assert_eq!(report.values[0], Some("100.00".to_string()));
        for k in 1..=10 {
            assert_eq!(report.values[k], Some("1100.00".to_string()), "day {k}");
        }
        // the buy is a deposit on its own day
        assert_eq!(report.net_flows[0], "0.00");
        for k in 1..=10 {
            assert_eq!(report.net_flows[k], "1000.00", "day {k}");
        }
        // no price moved: no return of any kind, and the change of value is the deposit
        for k in 0..=10 {
            close(report.portfolio[k], 0.0, 1e-9);
            close(report.mwr[k], 0.0, 1e-9);
            close(report.series[0].twr[k], 0.0, 1e-9);
        }
        let change = value_of(last(&report.values)) - value_of(&report.values[0]);
        close(change, number(last(&report.net_flows)), 1e-9);

        // a range that starts after the buy starts with the holding already worth 1000
        let inner = perf(&conn, day(2), day(10), vec![]);
        assert_eq!(inner.dates.len(), 9);
        assert_eq!(inner.values[0], Some("1100.00".to_string()));
        assert_eq!(last(&inner.values), &Some("1100.00".to_string()));
        assert!(inner.net_flows.iter().all(|flow| flow == "0.00"));
        for k in 0..inner.dates.len() {
            close(inner.portfolio[k], 0.0, 1e-9);
            close(inner.mwr[k], 0.0, 1e-9);
        }
        let late = inner
            .positions
            .iter()
            .find(|p| p.investment_id == "late")
            .expect("late");
        assert_eq!(late.start_value.as_deref(), Some("1000.00"));
        assert_eq!(late.end_value.as_deref(), Some("1000.00"));
        close(late.twr, 0.0, 1e-9);
    }

    #[test]
    fn a_buy_before_the_first_price_row_is_a_deposit() {
        let conn = setup_db();
        let a_prices = [
            100.0, 102.0, 101.0, 105.0, 108.0, 107.0, 110.0, 112.0, 111.0, 115.0, 120.0,
        ];
        seed_czk(&conn, "a", "AAA", 10.0, &a_prices);
        // B: 4 shares bought on day 3, priced only from day 7 on (constantly 50)
        insert_investment(&conn, "b", "BBB", "4");
        insert_tx(&conn, "BBB", 3, 4.0);
        for at in 7..=10 {
            insert_row(&conn, "BBB", at, 4.0, 50.0, "CZK");
        }

        let report = perf(
            &conn,
            day(0),
            day(10),
            vec![series("a", &["a"]), series("b", &["b"])],
        );

        // B's buy is a deposit of 4 * 50 on its own day, and nothing else is
        assert_eq!(report.dates.len(), 11);
        for k in 0..3 {
            assert_eq!(report.net_flows[k], "0.00", "day {k}");
        }
        for k in 3..=10 {
            assert_eq!(report.net_flows[k], "200.00", "day {k}");
        }
        // B is worth its first price from the buy on, so its own return is flat ...
        for twr in &report.series[1].twr {
            close(*twr, 0.0, 1e-9);
        }
        // ... and the portfolio chains A's moves over A's and B's money, as if B had been
        // priced all along
        let a_rows: Vec<(f64, f64)> = a_prices.iter().map(|price| (10.0, *price)).collect();
        let b_rows: Vec<(f64, f64)> = (0..=10)
            .map(|i| (if i < 3 { 0.0 } else { 4.0 }, 50.0))
            .collect();
        let expected = hand_twr(&[&a_rows, &b_rows]);
        for (actual, expected) in report.portfolio.iter().zip(&expected) {
            close(*actual, *expected, 1e-9);
        }
        // before the buy the portfolio is A alone
        let own = hand_twr(&[&a_rows]);
        for k in 0..3 {
            close(report.portfolio[k], own[k], 1e-9);
        }
        // the whole return is A's contribution
        let a = report.series[0].contribution.as_ref().expect("a");
        let b = report.series[1].contribution.as_ref().expect("b");
        close(*last(a), *last(&report.portfolio), 1e-9);
        close(*last(b), 0.0, 1e-9);
        // the change of value beyond the deposit is A's market gain: 10 shares, 100 -> 120
        let market = value_of(last(&report.values))
            - value_of(&report.values[0])
            - number(last(&report.net_flows));
        close(market, 10.0 * (120.0 - 100.0), 1e-9);
    }

    #[test]
    fn a_position_priced_only_after_the_range_is_valued_at_its_first_price() {
        let conn = setup_db();
        seed_czk(&conn, "a", "AAA", 10.0, &[100.0; 11]);
        // B is bought on day 3 and first priced on day 10, after the range ends
        insert_investment(&conn, "b", "BBB", "4");
        insert_tx(&conn, "BBB", 3, 4.0);
        insert_row(&conn, "BBB", 10, 4.0, 50.0, "CZK");

        let report = perf(&conn, day(0), day(8), vec![]);

        assert_eq!(report.dates.len(), 9);
        for k in 0..3 {
            assert_eq!(report.values[k], Some("1000.00".to_string()), "day {k}");
            assert_eq!(report.net_flows[k], "0.00", "day {k}");
        }
        for k in 3..=8 {
            assert_eq!(report.values[k], Some("1200.00".to_string()), "day {k}");
            assert_eq!(report.net_flows[k], "200.00", "day {k}");
        }
        for k in 0..=8 {
            close(report.mwr[k], 0.0, 1e-9);
        }
    }

    #[test]
    fn a_holding_without_a_rate_on_one_day_keeps_the_partition_exact() {
        // CZ (a CZK stock) and US (a USD stock) over 16 days. USD is quoted on day 0 and from
        // day 12 on, so day 11 is the only day outside the ten-day window of the last quote:
        // US drops out of day 11 (and, with no value to chain from, of day 12), CZ keeps going.
        let conn = setup_db();
        let cz: Vec<f64> = (0..16).map(|i| 100.0 + f64::from(i * i % 11)).collect();
        let us: Vec<f64> = (0..16).map(|i| 50.0 + f64::from(i * 3 % 7)).collect();
        seed_czk(&conn, "cz", "CZ", 10.0, &cz);
        seed(
            &conn,
            "us",
            "US",
            "20",
            "USD",
            &us.iter().map(|price| (20.0, *price)).collect::<Vec<_>>(),
        );
        add_usd_czk(&conn, day(0), 20.0);
        for at in 12..16 {
            add_usd_czk(&conn, day(at), 20.0 + at as f64 / 10.0);
        }

        let report = perf(
            &conn,
            day(0),
            day(15),
            vec![series("cz", &["cz"]), series("us", &["us"])],
        );

        // the miss happened and is counted ...
        assert!(report
            .fx_quality
            .missing_currencies
            .contains(&"USD".to_string()));
        assert!(report.fx_quality.missing_count > 0);
        let us_position = report
            .positions
            .iter()
            .find(|p| p.investment_id == "us")
            .expect("us");
        assert!(us_position.end_value.is_some());
        // ... the value of day 11 counts the holding that converts ...
        assert_eq!(
            report.values[11],
            Some(money_in_currency(10.0 * cz[11], "CZK"))
        );
        // ... and at every date the contributions of the partition add up to the portfolio
        let a = report.series[0].contribution.as_ref().expect("cz");
        let b = report.series[1].contribution.as_ref().expect("us");
        assert_eq!(report.dates.len(), 16);
        for k in 0..report.dates.len() {
            close(a[k] + b[k], report.portfolio[k], 1e-9);
        }
        let sum: f64 = report.positions.iter().map(|p| p.contribution).sum();
        close(sum, *last(&report.portfolio), 1e-9);
        // the portfolio does move, so the identity is not trivially 0 = 0
        assert!(report.portfolio.iter().any(|twr| twr.abs() > 0.1));
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

    // ----- benchmark -----

    fn price(at: i64, price: f64, currency: &str) -> HistoricalPrice {
        HistoricalPrice {
            timestamp: at,
            price,
            currency: currency.to_string(),
        }
    }

    /// The benchmark of `prices` from `from` to `to` in `currency`.
    fn bench(
        conn: &Connection,
        prices: &[HistoricalPrice],
        from: i64,
        to: i64,
        currency: &str,
    ) -> StockBenchmark {
        benchmark_series(conn, "CSPX.L", prices, from, to, currency).expect("benchmark")
    }

    #[test]
    fn benchmark_carries_the_last_close_over_non_trading_days() {
        let conn = setup_db();
        // closes on days 0, 3 and 4 only: days 1 and 2 are a weekend
        let prices = [
            price(day(0), 100.0, "CZK"),
            price(day(3), 110.0, "CZK"),
            price(day(4), 121.0, "CZK"),
        ];

        let report = bench(&conn, &prices, day(0), day(4), "CZK");

        assert_eq!(report.ticker, "CSPX.L");
        assert_eq!(report.currency, "CZK");
        assert_eq!(
            report.dates,
            (0..5).map(|i| ts_to_date_str(day(i))).collect::<Vec<_>>()
        );
        assert_eq!(report.twr.len(), 5);
        assert_eq!(report.twr[0], 0.0);
        close(report.twr[1], 0.0, 1e-9);
        close(report.twr[2], 0.0, 1e-9);
        close(report.twr[3], 10.0, 1e-9);
        close(report.twr[4], 21.0, 1e-9);
        assert!(report.fx_quality.missing_currencies.is_empty());
    }

    #[test]
    fn benchmark_converts_each_day_at_its_own_rate() {
        let conn = setup_db();
        add_usd_czk(&conn, day(0), 20.0);
        add_usd_czk(&conn, day(1), 21.0);
        // flat USD prices while the dollar gains 5 % against the koruna
        let prices = [price(day(0), 100.0, "USD"), price(day(1), 100.0, "USD")];

        let in_czk = bench(&conn, &prices, day(0), day(1), "CZK");
        let in_usd = bench(&conn, &prices, day(0), day(1), "USD");

        assert_eq!(in_czk.currency, "CZK");
        close(*last(&in_czk.twr), 5.0, 1e-9);
        assert_eq!(in_usd.currency, "USD");
        close(*last(&in_usd.twr), 0.0, 1e-9);
        assert!(in_czk.fx_quality.missing_currencies.is_empty());
    }

    #[test]
    fn benchmark_without_prices_is_empty() {
        let conn = setup_db();

        let report = bench(&conn, &[], day(0), day(4), "CZK");

        assert_eq!(report.ticker, "CSPX.L");
        assert!(report.dates.is_empty());
        assert!(report.twr.is_empty());
    }

    #[test]
    fn benchmark_base_is_the_last_close_on_or_before_the_start() {
        let conn = setup_db();
        // the close three days before the period is the base; the first day shows 0
        let prices = [price(day(-3), 100.0, "CZK"), price(day(1), 110.0, "CZK")];

        let report = bench(&conn, &prices, day(0), day(1), "CZK");

        assert_eq!(report.dates, [day(0), day(1)].map(ts_to_date_str));
        assert_eq!(report.twr[0], 0.0);
        close(report.twr[1], 10.0, 1e-9);
    }

    #[test]
    fn benchmark_starts_on_its_first_close_when_none_precedes_the_period() {
        let conn = setup_db();
        let prices = [price(day(2), 50.0, "CZK"), price(day(3), 55.0, "CZK")];

        let report = bench(&conn, &prices, day(0), day(3), "CZK");

        assert_eq!(report.dates, [day(2), day(3)].map(ts_to_date_str));
        assert_eq!(report.twr[0], 0.0);
        close(report.twr[1], 10.0, 1e-9);
    }

    #[test]
    fn benchmark_range_is_bounded_by_the_prices() {
        let conn = setup_db();
        let prices = [
            price(day(0), 100.0, "CZK"),
            price(day(1), 105.0, "CZK"),
            price(day(2), 110.0, "CZK"),
        ];

        // a transient absurd range (typing a year passes through year 0002)
        let report = bench(&conn, &prices, -62_000_000_000, 253_402_300_799, "CZK");

        assert_eq!(report.dates, [day(0), day(1), day(2)].map(ts_to_date_str));
        close(*last(&report.twr), 10.0, 1e-9);
    }

    #[test]
    fn benchmark_skips_days_that_cannot_be_converted_and_counts_them() {
        let conn = setup_db();
        // only CZK is quoted, so a USD close has no rate on any day
        for at in [day(0), day(1)] {
            conn.execute(
                "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, 'CZK', 25.0)",
                [at],
            )
            .expect("rate");
        }
        let prices = [price(day(0), 100.0, "USD"), price(day(1), 110.0, "USD")];

        let report = bench(&conn, &prices, day(0), day(1), "CZK");

        assert!(report.dates.is_empty());
        assert!(report.twr.is_empty());
        assert_eq!(report.fx_quality.missing_currencies, ["USD"]);
        assert!(report.fx_quality.missing_count > 0);
    }

    #[test]
    fn benchmark_ignores_unusable_prices_and_rejects_a_bad_currency() {
        let conn = setup_db();
        let prices = [
            price(day(0), 100.0, "CZK"),
            price(day(1), 0.0, "CZK"),
            price(day(2), f64::NAN, "CZK"),
            price(day(3), 120.0, "CZK"),
        ];

        let report = bench(&conn, &prices, day(0), day(3), "czk");

        // the zero and the NaN never replace the last valid close
        assert_eq!(report.currency, "CZK");
        close(report.twr[1], 0.0, 1e-9);
        close(report.twr[2], 0.0, 1e-9);
        close(report.twr[3], 20.0, 1e-9);
        assert!(benchmark_series(&conn, "X", &prices, day(0), day(3), "??").is_err());
    }

    fn ticker_key(result: Result<String>) -> String {
        match result {
            Err(AppError::Validation(key)) => key,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn validate_benchmark_ticker_accepts_indices_and_etfs() {
        for ticker in ["CSPX.L", "^GSPC", "IWDA.L", "BRK-B", "EURUSD=X", "A"] {
            assert_eq!(
                validate_benchmark_ticker(ticker).expect("valid"),
                ticker.to_string()
            );
        }
        // trimmed and uppercased
        assert_eq!(
            validate_benchmark_ticker("  cspx.l ").expect("valid"),
            "CSPX.L"
        );
        assert_eq!(
            validate_benchmark_ticker(&"A".repeat(24)).expect("24 characters"),
            "A".repeat(24)
        );
    }

    #[test]
    fn validate_benchmark_ticker_rejects_the_rest() {
        for ticker in ["", "   ", "a b", "AB$", "SPX/L", "ČEZ", "A,B"] {
            assert_eq!(
                ticker_key(validate_benchmark_ticker(ticker)),
                "validation.benchmarkTickerInvalid",
                "{ticker:?}"
            );
        }
        assert_eq!(
            ticker_key(validate_benchmark_ticker(&"A".repeat(25))),
            "validation.benchmarkTickerInvalid"
        );
    }
}
