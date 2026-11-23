//! Valuation of the realized sales of the stock transactions report: converts every trade at its
//! own day, prices each match by the cost method, applies the holding-period test and sums the
//! counted sales into the summary and the per-ticker results (spec D5 to D7, D15, D16).
//!
//! Money is integer minor units of the report currency (`money`): each trade's volume and fee is
//! rounded once, then split over the pieces it pays for, so every sum adds up exactly.

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Days, Months, Utc};

use super::lots::{Replay, Trade, TradeKind};
use super::money::{Minor, Split};
use crate::models::stock_dividends::amount_text;
use crate::models::{
    StockReportAmounts, StockReportCostMethod, StockReportMatch, StockReportSale,
    StockReportSummary, StockReportTickerResult, StockTransactionsReportRequest,
};
use crate::services::fx::{History, Quality, Quotes};
use crate::services::reporting::money_in_currency;

/// Quantities below this are rounding dust.
const EPSILON: f64 = 1e-9;

/// Converts at a trade's day, falling back to the current quotes (marking the response
/// estimated, the way `cost_basis` does), and records what could not be converted at all.
/// Mirrors `cost_basis::DayConverter`; keep the two fallbacks alike.
pub struct Converter<'a> {
    history: &'a History,
    current: &'a Quotes,
    target: &'a str,
    pub quality: Quality,
}

impl<'a> Converter<'a> {
    pub fn new(history: &'a History, current: &'a Quotes, target: &'a str) -> Self {
        Self {
            history,
            current,
            target,
            quality: Quality::default(),
        }
    }

    /// The day's quotes (ten-day walk-back), else the current quotes, else `None`. Records
    /// nothing when it fails; see `tracked`.
    pub fn value(&mut self, amount: f64, from: &str, day: i64) -> Option<f64> {
        if let Some(value) = self.history.convert_on(amount, from, self.target, day) {
            return Some(value);
        }
        let value = self.current.convert(amount, from, self.target)?;
        self.quality.mark_estimated();
        Some(value)
    }

    /// Like `value`, counting one missing conversion when it fails.
    pub fn tracked(&mut self, amount: f64, from: &str, day: i64) -> Option<f64> {
        let value = self.value(amount, from, day);
        if value.is_none() {
            self.quality
                .track_conversion(self.current, from, self.target, None);
        }
        value
    }

    /// Report-currency value of one unit of `from` on `day` ("1" for the same currency).
    pub fn rate(&mut self, from: &str, day: i64) -> Option<f64> {
        self.value(1.0, from, day)
    }
}

/// Per trade index (replay order), in minor units of the report currency at the trade's day,
/// each rounded once.
pub struct Valued {
    /// Quantity times price, for every trade.
    pub volume: Vec<Option<i64>>,
    /// The same as `volume` for sales, `None` for buys.
    pub proceeds: Vec<Option<i64>>,
    /// The fee of the whole trade, `Some(0)` when it has none.
    pub fee: Vec<Option<i64>>,
}

/// The average method's pool of one position after the whole replay, in minor units.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolState {
    pub quantity: f64,
    /// `None` once an unconvertible buy entered the pool (until it empties).
    pub cost: Option<i64>,
    pub fee: Option<i64>,
}

impl PoolState {
    fn empty() -> Self {
        Self {
            quantity: 0.0,
            cost: Some(0),
            fee: Some(0),
        }
    }
}

/// What `value_sales` produces for the report and for the stats, ledger and open lots.
pub struct SalesOutcome {
    pub valued: Valued,
    pub pools: HashMap<String, PoolState>,
    /// Per open buy index (FIFO): the units of the lot's cost and fee no sale consumed.
    pub lot_rest: HashMap<usize, (Option<i64>, Option<i64>)>,
    /// Sales in the period, newest first.
    pub sales: Vec<StockReportSale>,
    pub summary: StockReportSummary,
    pub tickers: Vec<StockReportTickerResult>,
    pub uncovered_tickers: Vec<String>,
    /// `(sale day, result after fees in minor units)` of every summed sale in the period.
    pub results: Vec<(i64, i64)>,
}

/// The first day a sale is exempt: the day after `buy_day` plus `months` calendar months (a day
/// missing in the target month becomes its last day).
pub fn pass_day(buy_day: i64, months: u32) -> i64 {
    let shifted = DateTime::<Utc>::from_timestamp(buy_day, 0)
        .and_then(|d| d.date_naive().checked_add_months(Months::new(months)))
        .and_then(|d| d.checked_add_days(Days::new(1)))
        .and_then(|d| d.and_hms_opt(0, 0, 0));
    match shifted {
        Some(date) => date.and_utc().timestamp(),
        // Out of the calendar's range: never exempt.
        None => i64::MAX,
    }
}

fn value_trades(trades: &[Trade], conv: &mut Converter, minor: Minor) -> Valued {
    let mut valued = Valued {
        volume: Vec::with_capacity(trades.len()),
        proceeds: Vec::with_capacity(trades.len()),
        fee: Vec::with_capacity(trades.len()),
    };
    for trade in trades {
        let volume = conv
            .tracked(trade.quantity * trade.price, &trade.currency, trade.day)
            .map(|v| minor.units(v));
        valued.proceeds.push(match trade.kind {
            TradeKind::Sell => volume,
            TradeKind::Buy => None,
        });
        valued.volume.push(volume);
        valued.fee.push(if trade.fee == 0.0 {
            Some(0)
        } else {
            conv.tracked(trade.fee, &trade.currency, trade.day)
                .map(|f| minor.units(f))
        });
    }
    valued
}

/// What a sale releases from its position's average pool: cost and fee (units, each rounded
/// once), the covered quantity they belong to and the average cost per share before the sale.
#[derive(Clone, Copy)]
struct Release {
    cost: Option<i64>,
    fee: Option<i64>,
    covered: f64,
    per_share: Option<f64>,
}

/// Replays the pools of the average method over the whole history: per sale what it releases,
/// per position the pool left at the end.
fn average_pools(
    replay: &Replay,
    valued: &Valued,
    uncovered: &HashMap<usize, f64>,
    minor: Minor,
) -> (HashMap<usize, Release>, HashMap<String, PoolState>) {
    let mut pools: HashMap<String, PoolState> = HashMap::new();
    let mut per_sale = HashMap::new();
    for (index, trade) in replay.trades.iter().enumerate() {
        let pool = pools
            .entry(trade.investment_id.clone())
            .or_insert_with(PoolState::empty);
        match trade.kind {
            TradeKind::Buy => {
                pool.quantity += trade.quantity;
                pool.cost = pool.cost.zip(valued.volume[index]).map(|(a, b)| a + b);
                pool.fee = pool.fee.zip(valued.fee[index]).map(|(a, b)| a + b);
            }
            TradeKind::Sell => {
                let covered = trade.quantity - uncovered.get(&index).copied().unwrap_or(0.0);
                if pool.quantity < EPSILON || covered < EPSILON {
                    per_sale.insert(
                        index,
                        Release {
                            cost: None,
                            fee: None,
                            covered,
                            per_share: None,
                        },
                    );
                    continue;
                }
                let release =
                    |total: Option<i64>| total.map(|t| Split::new(t, pool.quantity).take(covered));
                let (cost, fee) = (release(pool.cost), release(pool.fee));
                per_sale.insert(
                    index,
                    Release {
                        cost,
                        fee,
                        covered,
                        per_share: pool.cost.map(|c| minor.amount(c) / pool.quantity),
                    },
                );
                pool.quantity -= covered;
                pool.cost = pool.cost.zip(cost).map(|(a, b)| a - b);
                pool.fee = pool.fee.zip(fee).map(|(a, b)| a - b);
                if pool.quantity < EPSILON {
                    *pool = PoolState::empty();
                }
            }
        }
    }
    (per_sale, pools)
}

/// Per match (index into `Replay::matches`) the units of its lot's cost and buy fee, each lot
/// split over the sales that consume it in replay order; per open lot what is left (FIFO).
type LotPieces = (
    Vec<(Option<i64>, Option<i64>)>,
    HashMap<usize, (Option<i64>, Option<i64>)>,
);

fn fifo_pieces(replay: &Replay, valued: &Valued) -> LotPieces {
    let mut splits: HashMap<usize, (Option<Split>, Option<Split>)> = HashMap::new();
    let lot_split = |buy: usize| {
        let quantity = replay.trades[buy].quantity;
        (
            valued.volume[buy].map(|v| Split::new(v, quantity)),
            valued.fee[buy].map(|f| Split::new(f, quantity)),
        )
    };
    let mut pieces = Vec::with_capacity(replay.matches.len());
    for m in &replay.matches {
        let (cost, fee) = splits.entry(m.buy).or_insert_with(|| lot_split(m.buy));
        pieces.push((
            cost.as_mut().map(|s| s.take(m.quantity)),
            fee.as_mut().map(|s| s.take(m.quantity)),
        ));
    }
    let rest = replay
        .open
        .iter()
        .map(|(buy, _)| {
            let (cost, fee) = splits.get(buy).copied().unwrap_or_else(|| lot_split(*buy));
            (*buy, (cost.map(|s| s.rest()), fee.map(|s| s.rest())))
        })
        .collect();
    (pieces, rest)
}

/// Proceeds, cost and fees of matched shares, in minor units.
#[derive(Default, Clone, Copy)]
struct Sums {
    proceeds: i64,
    cost: i64,
    fees: i64,
}

impl Sums {
    fn add(&mut self, other: Sums) {
        self.proceeds += other.proceeds;
        self.cost += other.cost;
        self.fees += other.fees;
    }

    fn result(&self) -> i64 {
        self.proceeds - self.cost
    }

    fn after_fees(&self) -> i64 {
        self.result() - self.fees
    }

    fn wire(&self, minor: Minor) -> StockReportAmounts {
        StockReportAmounts {
            proceeds: minor.text(self.proceeds),
            cost: minor.text(self.cost),
            result: minor.text(self.result()),
            fees: minor.text(self.fees),
            result_after_fees: minor.text(self.after_fees()),
        }
    }
}

#[derive(Default)]
struct TickerSums {
    ticker: String,
    company_name: String,
    quantity: f64,
    total: Sums,
    taxable: Sums,
    exempt: Sums,
}

/// Values every trade, prices the matches of the sales inside `[from_day, to_day]` and sums
/// the counted ones.
pub fn value_sales(
    replay: &Replay,
    request: &StockTransactionsReportRequest,
    currency: &str,
    conv: &mut Converter,
) -> SalesOutcome {
    let minor = Minor::of(currency);
    let valued = value_trades(&replay.trades, conv, minor);
    let uncovered: HashMap<usize, f64> = replay.uncovered.iter().copied().collect();
    let (average, pools) = average_pools(replay, &valued, &uncovered, minor);
    let (lot_pieces, lot_rest) = fifo_pieces(replay, &valued);
    let months = request
        .holding_test_months
        .and_then(|m| u32::try_from(m).ok());

    // Match indices per sale, oldest lot first.
    let mut by_sale: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (position, m) in replay.matches.iter().enumerate() {
        by_sale.entry(m.sale).or_default().push(position);
    }

    let mut total = Sums::default();
    let mut taxable = Sums::default();
    let mut exempt = Sums::default();
    let (mut count, mut in_profit, mut in_loss) = (0u32, 0u32, 0u32);
    let mut tickers: BTreeMap<String, TickerSums> = BTreeMap::new();
    let mut sales = Vec::new();
    let mut uncovered_tickers: Vec<String> = Vec::new();
    let mut results: Vec<(i64, i64)> = Vec::new();

    for (index, sale) in replay.trades.iter().enumerate() {
        if sale.kind != TradeKind::Sell {
            continue;
        }
        if sale.day < request.from_day || sale.day > request.to_day {
            continue;
        }
        let uncovered_quantity = uncovered.get(&index).copied().unwrap_or(0.0);
        if uncovered_quantity > 0.0 && !uncovered_tickers.contains(&sale.ticker) {
            uncovered_tickers.push(sale.ticker.clone());
        }
        let match_ids: Vec<usize> = by_sale.get(&index).cloned().unwrap_or_default();

        // The sale's proceeds and fee over its whole quantity (the uncovered rest is last and
        // never taken); the average's released cost and fee over the covered quantity.
        let mut proceeds = valued.proceeds[index].map(|p| Split::new(p, sale.quantity));
        let mut sale_fee = valued.fee[index].map(|f| Split::new(f, sale.quantity));
        let release = average.get(&index).copied();
        let mut released_cost = release.and_then(|r| r.cost.map(|c| Split::new(c, r.covered)));
        let mut released_fee = release.and_then(|r| r.fee.map(|f| Split::new(f, r.covered)));

        let mut figures: Vec<Option<Sums>> = Vec::with_capacity(match_ids.len());
        for &id in &match_ids {
            let quantity = replay.matches[id].quantity;
            let proceeds = proceeds.as_mut().map(|s| s.take(quantity));
            let sale_fee = sale_fee.as_mut().map(|s| s.take(quantity));
            let (cost, buy_fee) = match request.cost_method {
                StockReportCostMethod::Fifo => lot_pieces[id],
                StockReportCostMethod::Average => (
                    released_cost.as_mut().map(|s| s.take(quantity)),
                    released_fee.as_mut().map(|s| s.take(quantity)),
                ),
            };
            figures.push(match (proceeds, cost, buy_fee, sale_fee) {
                (Some(proceeds), Some(cost), Some(buy_fee), Some(sale_fee)) => Some(Sums {
                    proceeds,
                    cost,
                    fees: buy_fee + sale_fee,
                }),
                _ => None,
            });
        }
        let counted = valued.proceeds[index].is_some() && valued.fee[index].is_some();
        let known: Option<Vec<Sums>> = figures
            .into_iter()
            .collect::<Option<Vec<_>>>()
            .filter(|_| counted);
        let exempt_flags: Vec<Option<bool>> = match_ids
            .iter()
            .map(|&id| {
                let buy_day = replay.trades[replay.matches[id].buy].day;
                months.map(|n| sale.day >= pass_day(buy_day, n))
            })
            .collect();
        let average_cost = release
            .and_then(|r| r.per_share)
            .filter(|_| request.cost_method == StockReportCostMethod::Average);

        let mut wire_matches = Vec::new();
        for (position, &id) in match_ids.iter().enumerate() {
            let m = &replay.matches[id];
            let buy = &replay.trades[m.buy];
            let figure = known.as_ref().map(|k| k[position]);
            wire_matches.push(StockReportMatch {
                buy_day: buy.day,
                quantity: amount_text(m.quantity),
                buy_price: amount_text(buy.price),
                buy_currency: buy.currency.clone(),
                buy_rate: conv.rate(&buy.currency, buy.day).map(amount_text),
                average_cost: figure
                    .and(average_cost)
                    .map(|c| money_in_currency(c, currency)),
                proceeds: figure.map(|f| minor.text(f.proceeds)),
                cost: figure.map(|f| minor.text(f.cost)),
                fees: figure.map(|f| minor.text(f.fees)),
                result_after_fees: figure.map(|f| minor.text(f.after_fees())),
                exempt: exempt_flags[position],
            });
        }

        // Only a sale with at least one counted match is in the sums and their counts.
        if let Some(figures) = known.as_ref().filter(|f| !f.is_empty()) {
            count += 1;
            let mut sale_sums = Sums::default();
            let entry = tickers
                .entry(sale.investment_id.clone())
                .or_insert_with(|| TickerSums {
                    ticker: sale.ticker.clone(),
                    company_name: sale.company_name.clone(),
                    ..Default::default()
                });
            for (position, figure) in figures.iter().enumerate() {
                sale_sums.add(*figure);
                total.add(*figure);
                entry.total.add(*figure);
                entry.quantity += replay.matches[match_ids[position]].quantity;
                match exempt_flags[position] {
                    Some(true) => {
                        exempt.add(*figure);
                        entry.exempt.add(*figure);
                    }
                    Some(false) => {
                        taxable.add(*figure);
                        entry.taxable.add(*figure);
                    }
                    None => {}
                }
            }
            let result = sale_sums.after_fees();
            results.push((sale.day, result));
            if result > 0 {
                in_profit += 1;
            } else if result < 0 {
                in_loss += 1;
            }
        }

        sales.push(StockReportSale {
            id: sale.id.clone(),
            investment_id: sale.investment_id.clone(),
            ticker: sale.ticker.clone(),
            company_name: sale.company_name.clone(),
            day: sale.day,
            quantity: amount_text(sale.quantity),
            price: amount_text(sale.price),
            currency: sale.currency.clone(),
            rate: conv.rate(&sale.currency, sale.day).map(amount_text),
            uncovered_quantity: amount_text(uncovered_quantity),
            counted: known.is_some(),
            matches: wire_matches,
        });
    }
    // Pushed in replay order: newest day first and, within a day, the later trade first.
    sales.reverse();

    let test_on = months.is_some();
    let mut ticker_rows: Vec<(i64, StockReportTickerResult)> = tickers
        .into_iter()
        .map(|(investment_id, t)| {
            (
                t.total.after_fees(),
                StockReportTickerResult {
                    investment_id,
                    ticker: t.ticker,
                    company_name: t.company_name,
                    quantity: amount_text(t.quantity),
                    result_after_fees: minor.text(t.total.after_fees()),
                    taxable_result: test_on.then(|| minor.text(t.taxable.after_fees())),
                    exempt_result: test_on.then(|| minor.text(t.exempt.after_fees())),
                },
            )
        })
        .collect();
    ticker_rows.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.ticker.cmp(&b.1.ticker)));

    let summary = StockReportSummary {
        total: total.wire(minor),
        taxable: test_on.then(|| taxable.wire(minor)),
        exempt: test_on.then(|| exempt.wire(minor)),
        sales_count: count,
        sales_in_profit: in_profit,
        sales_in_loss: in_loss,
    };
    SalesOutcome {
        valued,
        pools,
        lot_rest,
        sales,
        summary,
        tickers: ticker_rows.into_iter().map(|(_, row)| row).collect(),
        uncovered_tickers,
        results,
    }
}

#[cfg(test)]
mod tests {
    use super::super::report;
    use super::super::tests::*;
    use super::*;
    use crate::models::StockTransactionsReport;
    use crate::services::fx;

    const TODAY: i64 = 1_790_000_000;
    const PERIOD: (i64, i64) = (1_767_225_600, 1_798_675_200); // 2026-01-01 .. 2026-12-31

    fn request(
        method: StockReportCostMethod,
        months: Option<i64>,
    ) -> StockTransactionsReportRequest {
        StockTransactionsReportRequest {
            from_day: PERIOD.0,
            to_day: PERIOD.1,
            report_currency: "CZK".into(),
            cost_method: method,
            holding_test_months: months,
        }
    }

    fn run(
        conn: &rusqlite::Connection,
        req: &StockTransactionsReportRequest,
    ) -> StockTransactionsReport {
        report(conn, req, TODAY).expect("report")
    }

    fn czk(amount: f64) -> String {
        money_in_currency(amount, "CZK")
    }

    fn some(text: &Option<String>) -> String {
        text.clone().expect("amount")
    }

    fn assert_match(m: &StockReportMatch, row: [f64; 5]) {
        assert_eq!(some(&m.proceeds), czk(row[0]));
        assert_eq!(some(&m.cost), czk(row[1]));
        assert_eq!(some(&m.fees), czk(row[3]));
        assert_eq!(some(&m.result_after_fees), czk(row[4]));
    }

    fn assert_amounts(a: &StockReportAmounts, row: [f64; 5]) {
        assert_eq!(a.proceeds, czk(row[0]));
        assert_eq!(a.cost, czk(row[1]));
        assert_eq!(a.result, czk(row[2]));
        assert_eq!(a.fees, czk(row[3]));
        assert_eq!(a.result_after_fees, czk(row[4]));
    }

    const FIFO_1: [f64; 5] = [75_000.0, 20_000.0, 55_000.0, 90.0, 54_910.0];
    const FIFO_2: [f64; 5] = [37_500.0, 25_000.0, 12_500.0, 75.0, 12_425.0];
    const FIFO_T: [f64; 5] = [112_500.0, 45_000.0, 67_500.0, 165.0, 67_335.0];
    const AVG_1: [f64; 5] = [75_000.0, 35_000.0, 40_000.0, 120.0, 39_880.0];
    const AVG_2: [f64; 5] = [37_500.0, 17_500.0, 20_000.0, 60.0, 19_940.0];
    const AVG_T: [f64; 5] = [112_500.0, 52_500.0, 60_000.0, 180.0, 59_820.0];

    #[test]
    fn fifo_amounts_follow_each_lot() {
        let r = run(&fixture(), &request(StockReportCostMethod::Fifo, None));
        let sale = &r.sales[0];
        assert!(sale.counted);
        assert_match(&sale.matches[0], FIFO_1);
        assert_match(&sale.matches[1], FIFO_2);
        assert_amounts(&r.summary.total, FIFO_T);
        assert!(r.summary.taxable.is_none() && r.summary.exempt.is_none());
        assert_eq!(r.summary.sales_count, 1);
        assert_eq!(r.summary.sales_in_profit, 1);
        assert_eq!(r.summary.sales_in_loss, 0);
        assert_eq!(sale.matches[0].exempt, None);
        assert_eq!(r.currency, "CZK");
    }

    #[test]
    fn average_amounts_use_the_running_average() {
        let r = run(&fixture(), &request(StockReportCostMethod::Average, None));
        let sale = &r.sales[0];
        assert_match(&sale.matches[0], AVG_1);
        assert_match(&sale.matches[1], AVG_2);
        assert_amounts(&r.summary.total, AVG_T);
        for m in &sale.matches {
            assert_eq!(some(&m.average_cost), czk(3_500.0));
        }
    }

    #[test]
    fn the_holding_test_splits_matches() {
        let r = run(&fixture(), &request(StockReportCostMethod::Fifo, Some(36)));
        let taxable = r.summary.taxable.as_ref().expect("taxable");
        let exempt = r.summary.exempt.as_ref().expect("exempt");
        assert_amounts(taxable, FIFO_2);
        assert_amounts(exempt, FIFO_1);
        let (t, e, all) = (columns(taxable), columns(exempt), columns(&r.summary.total));
        for column in 0..5 {
            assert_eq!(t[column] + e[column], all[column], "{column}");
        }
        let flags: Vec<_> = r.sales[0].matches.iter().map(|m| m.exempt).collect();
        assert_eq!(flags, vec![Some(true), Some(false)]);
    }

    #[test]
    fn the_test_boundary_is_the_day_after_the_full_period() {
        for (sale_day, expected) in [(D3, false), (D3 + 86_400, true)] {
            let conn = setup_test_db();
            add_position(&conn, "AAA", "AAA");
            let buy = 1_676_000_000_i64.div_euclid(86_400) * 86_400; // 2023-02-10
            add_trade(&conn, "b", "AAA", "buy", "1", "10", "CZK", buy, None);
            add_trade(&conn, "s", "AAA", "sell", "1", "20", "CZK", sale_day, None);
            let r = run(&conn, &request(StockReportCostMethod::Fifo, Some(36)));
            assert_eq!(r.sales[0].matches[0].exempt, Some(expected), "{sale_day}");
        }
    }

    #[test]
    fn a_month_end_purchase_clamps() {
        let day = |y: i32, m: u32, d: u32| {
            chrono::NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp()
        };
        assert_eq!(pass_day(day(2022, 11, 30), 3), day(2023, 3, 1));
        assert_eq!(pass_day(day(2023, 11, 30), 3), day(2024, 3, 1));
    }

    #[test]
    fn a_custom_length_in_months_works() {
        let r = run(&fixture(), &request(StockReportCostMethod::Fifo, Some(20)));
        let flags: Vec<_> = r.sales[0].matches.iter().map(|m| m.exempt).collect();
        assert_eq!(flags, vec![Some(true), Some(true)]);
    }

    #[test]
    fn the_period_includes_both_ends() {
        let conn = fixture();
        let mut req = request(StockReportCostMethod::Fifo, None);
        req.from_day = D3;
        req.to_day = D3;
        assert_eq!(run(&conn, &req).sales.len(), 1);
        req.from_day = D3 - 86_400;
        req.to_day = D3 - 86_400;
        let r = run(&conn, &req);
        assert!(r.sales.is_empty());
        assert_amounts(&r.summary.total, [0.0; 5]);
        assert_eq!(r.summary.sales_count, 0);
    }

    #[test]
    fn lots_bought_before_the_period_are_used() {
        let r = run(&fixture(), &request(StockReportCostMethod::Fifo, None));
        assert_eq!(r.sales[0].matches[0].buy_day, D1);
        assert_eq!(r.first_transaction_day, Some(D1));
    }

    #[test]
    fn an_oversold_sale_counts_only_its_covered_part() {
        let conn = setup_test_db();
        add_position(&conn, "BBB", "BBB");
        add_trade(&conn, "b", "BBB", "buy", "5", "10", "CZK", D2, None);
        add_trade(&conn, "s", "BBB", "sell", "8", "20", "CZK", D3, Some("8"));
        let r = run(&conn, &request(StockReportCostMethod::Fifo, None));
        let sale = &r.sales[0];
        assert!(sale.counted);
        assert_eq!(sale.matches.len(), 1);
        assert_match(&sale.matches[0], [100.0, 50.0, 50.0, 5.0, 45.0]);
        assert_eq!(sale.uncovered_quantity, "3");
        assert_eq!(r.uncovered_tickers, vec!["BBB".to_string()]);
        assert_eq!(r.summary.total.proceeds, czk(100.0));
    }

    #[test]
    fn an_unconvertible_sale_stays_out_of_the_sums() {
        let conn = setup_test_db();
        add_position(&conn, "XXX", "XXX");
        add_trade(&conn, "b", "XXX", "buy", "1", "10", "XXX", D2, None);
        add_trade(&conn, "s", "XXX", "sell", "1", "20", "XXX", D3, None);
        add_rate(&conn, D3, "CZK", 25.0);
        let r = run(&conn, &request(StockReportCostMethod::Fifo, None));
        let sale = &r.sales[0];
        assert!(!sale.counted);
        assert!(sale.rate.is_none());
        let m = &sale.matches[0];
        assert!(m.proceeds.is_none() && m.cost.is_none() && m.fees.is_none());
        assert!(m.result_after_fees.is_none());
        assert_amounts(&r.summary.total, [0.0; 5]);
        assert_eq!(r.summary.sales_count, 0);
        assert!(r.tickers.is_empty());
        assert!(r.fx_quality.missing_currencies.contains(&"XXX".to_string()));
    }

    #[test]
    fn a_missing_day_rate_falls_back_to_the_current_quote_and_is_marked() {
        fx::set_current_for_test(&[("CZK", 25.0), ("USD", 1.0)]);
        let conn = setup_test_db();
        add_position(&conn, "AAA", "AAA");
        add_trade(&conn, "b", "AAA", "buy", "1", "10", "USD", D2, None);
        add_trade(&conn, "s", "AAA", "sell", "1", "20", "USD", D3, None);
        // Rates exist, but not for USD within ten days of either trade.
        add_rate(&conn, D2, "CZK", 25.0);
        add_rate(&conn, D3, "CZK", 25.0);
        let r = run(&conn, &request(StockReportCostMethod::Fifo, None));
        assert!(r.sales[0].counted);
        assert_eq!(r.summary.total.proceeds, czk(500.0));
        assert!(r.fx_quality.estimated);
        assert!(r.fx_quality.missing_currencies.is_empty());
    }

    #[test]
    fn tickers_are_sorted_by_result() {
        let conn = fixture();
        add_position(&conn, "LOS", "LOS");
        add_trade(&conn, "lb", "LOS", "buy", "1", "1000", "CZK", D2, None);
        add_trade(&conn, "ls", "LOS", "sell", "1", "400", "CZK", D3, None);
        let r = run(&conn, &request(StockReportCostMethod::Fifo, Some(36)));
        assert_eq!(r.tickers.len(), 2);
        assert_eq!(r.tickers[0].ticker, "AAA");
        assert_eq!(r.tickers[0].result_after_fees, czk(67_335.0));
        assert_eq!(r.tickers[0].quantity, "15");
        assert_eq!(r.tickers[1].ticker, "LOS");
        assert_eq!(r.tickers[1].result_after_fees, czk(-600.0));
        for t in &r.tickers {
            let sum = units(t.taxable_result.as_ref().unwrap())
                + units(t.exempt_result.as_ref().unwrap());
            assert_eq!(sum, units(&t.result_after_fees));
        }
        assert_eq!(r.summary.sales_in_loss, 1);
        let off = run(&conn, &request(StockReportCostMethod::Fifo, None));
        assert!(off.tickers[0].taxable_result.is_none());
    }

    /// Exact minor units of a CZK wire amount ("-12.30" → -1230); panics on any other shape.
    fn units(text: &str) -> i64 {
        let (whole, cents) = text.split_once('.').expect("two decimals");
        assert_eq!(cents.len(), 2, "{text}");
        let negative = whole.starts_with('-');
        let whole: i64 = whole.trim_start_matches('-').parse().expect("whole");
        let cents: i64 = cents.parse().expect("cents");
        let value = whole * 100 + cents;
        if negative {
            -value
        } else {
            value
        }
    }

    fn columns(a: &StockReportAmounts) -> [i64; 5] {
        [
            units(&a.proceeds),
            units(&a.cost),
            units(&a.result),
            units(&a.fees),
            units(&a.result_after_fees),
        ]
    }

    /// Three-decimal prices, fractional quantities, fees and rates like 25.173 / 1.0837, three
    /// lots of one position and a second position in EUR, sales in and out of the test.
    fn awkward() -> rusqlite::Connection {
        let conn = setup_test_db();
        add_position(&conn, "AAA", "AAA");
        add_position(&conn, "BBB", "BBB");
        let day = |y: i32, m: u32, d: u32| {
            chrono::NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap()
                .and_utc()
                .timestamp()
        };
        let trades = [
            (
                "a1",
                "AAA",
                "buy",
                "3.7",
                "101.237",
                "USD",
                day(2023, 1, 10),
                "1.13",
            ),
            (
                "a2",
                "AAA",
                "buy",
                "2.9",
                "133.517",
                "USD",
                day(2024, 1, 10),
                "0.97",
            ),
            (
                "a3",
                "AAA",
                "buy",
                "5.3",
                "97.773",
                "USD",
                day(2024, 2, 9),
                "1.41",
            ),
            (
                "a4",
                "AAA",
                "sell",
                "4.1",
                "141.337",
                "USD",
                day(2026, 2, 10),
                "1.27",
            ),
            (
                "a5",
                "AAA",
                "sell",
                "3.3",
                "152.913",
                "USD",
                day(2026, 3, 2),
                "0.83",
            ),
            (
                "b1",
                "BBB",
                "buy",
                "7",
                "13.457",
                "EUR",
                day(2022, 6, 14),
                "0.37",
            ),
            (
                "b2",
                "BBB",
                "sell",
                "2.6",
                "19.133",
                "EUR",
                day(2026, 4, 7),
                "0.29",
            ),
            (
                "b3",
                "BBB",
                "buy",
                "1.7",
                "14.219",
                "EUR",
                day(2026, 5, 19),
                "0.21",
            ),
            (
                "b4",
                "BBB",
                "sell",
                "1.9",
                "11.111",
                "EUR",
                day(2026, 7, 21),
                "0.4",
            ),
        ];
        let usd = ["1.0837", "1.0912", "1.0779", "1.1043", "1.0951"];
        for (n, (id, inv, kind, qty, price, cur, day, fee)) in trades.into_iter().enumerate() {
            add_trade(&conn, id, inv, kind, qty, price, cur, day, Some(fee));
            add_rate(&conn, day, "CZK", 25.173);
            add_rate(&conn, day, "USD", usd[n % usd.len()].parse().unwrap());
        }
        conn
    }

    /// Every figure of the page is the exact sum of the figures under it (review I1).
    #[test]
    fn every_total_reconciles_to_the_minor_unit() {
        let conn = awkward();
        for method in [StockReportCostMethod::Fifo, StockReportCostMethod::Average] {
            let r = run(&conn, &request(method, Some(36)));
            let total = columns(&r.summary.total);
            let taxable = columns(r.summary.taxable.as_ref().expect("taxable"));
            let exempt = columns(r.summary.exempt.as_ref().expect("exempt"));
            for column in 0..5 {
                assert_eq!(
                    taxable[column] + exempt[column],
                    total[column],
                    "{method:?} {column}"
                );
            }
            assert_eq!(total[2], total[0] - total[1], "{method:?}");
            assert_eq!(total[4], total[2] - total[3], "{method:?}");
            assert_ne!(taxable[0], 0, "{method:?}: the data must hit both classes");
            assert_ne!(exempt[0], 0, "{method:?}");

            // Σ match rows (the sales table) = the summary, class by class.
            let mut by_class = [[0i64; 4]; 2];
            let mut sold = [0i64; 4];
            for sale in &r.sales {
                assert!(sale.counted, "{}", sale.id);
                let mut sale_proceeds = 0;
                for m in &sale.matches {
                    let row = [
                        units(&some(&m.proceeds)),
                        units(&some(&m.cost)),
                        units(&some(&m.fees)),
                        units(&some(&m.result_after_fees)),
                    ];
                    assert_eq!(row[3], row[0] - row[1] - row[2], "{method:?} {}", sale.id);
                    let class = usize::from(m.exempt == Some(true));
                    for k in 0..4 {
                        by_class[class][k] += row[k];
                        sold[k] += row[k];
                    }
                    sale_proceeds += row[0];
                }
                // A fully covered sale shows in the results exactly its ledger amount.
                let ledger = r
                    .ledger
                    .iter()
                    .find(|l| l.id == sale.id)
                    .expect("ledger row");
                assert_eq!(
                    sale_proceeds,
                    units(ledger.amount.as_deref().unwrap()),
                    "{}",
                    sale.id
                );
            }
            assert_eq!(sold, [total[0], total[1], total[3], total[4]], "{method:?}");
            assert_eq!(
                by_class[0],
                [taxable[0], taxable[1], taxable[3], taxable[4]]
            );
            assert_eq!(by_class[1], [exempt[0], exempt[1], exempt[3], exempt[4]]);

            // Tickers and buckets add up to the summary.
            let ticker_sum = |pick: fn(&StockReportTickerResult) -> i64| -> i64 {
                r.tickers.iter().map(pick).sum()
            };
            assert_eq!(ticker_sum(|t| units(&t.result_after_fees)), total[4]);
            assert_eq!(
                ticker_sum(|t| units(t.taxable_result.as_deref().unwrap())),
                taxable[4]
            );
            assert_eq!(
                ticker_sum(|t| units(t.exempt_result.as_deref().unwrap())),
                exempt[4]
            );
            let bucket_sum = |pick: fn(&crate::models::StockReportBucket) -> &str| -> i64 {
                r.buckets.iter().map(|b| units(pick(b))).sum()
            };
            assert_eq!(bucket_sum(|b| &b.result), total[4], "{method:?}");

            // Stats and bucket volumes = the ledger amounts (every trade of 2026 converts).
            let ledger_sum = |kind: crate::models::StockReportLedgerKind| -> i64 {
                r.ledger
                    .iter()
                    .filter(|l| l.kind == kind)
                    .map(|l| units(l.amount.as_deref().unwrap()))
                    .sum()
            };
            let sells = ledger_sum(crate::models::StockReportLedgerKind::Sell);
            let buys = ledger_sum(crate::models::StockReportLedgerKind::Buy);
            assert_eq!(units(&r.stats.proceeds), sells);
            assert_eq!(units(&r.stats.bought), buys);
            assert_eq!(units(&r.stats.traded_volume), sells + buys);
            assert_eq!(bucket_sum(|b| &b.sells), sells);
            assert_eq!(bucket_sum(|b| &b.buys), buys);
            assert_eq!(r.stats.sales_count, 4);
            assert_eq!(r.stats.buys_count, 1);
        }
    }

    #[test]
    fn a_wholly_uncovered_sale_is_listed_but_not_counted() {
        let conn = fixture();
        add_position(&conn, "NON", "NON");
        add_trade(&conn, "ns", "NON", "sell", "2", "50", "CZK", D3, None);
        let r = run(&conn, &request(StockReportCostMethod::Fifo, None));
        assert_eq!(r.sales.len(), 2);
        let sale = r.sales.iter().find(|s| s.id == "ns").expect("listed");
        assert!(sale.matches.is_empty());
        assert_eq!(sale.uncovered_quantity, "2");
        // The hero counts what the sums hold, like the table's totals row.
        assert_eq!(r.summary.sales_count, 1);
        assert_eq!((r.summary.sales_in_profit, r.summary.sales_in_loss), (1, 0));
        assert_amounts(&r.summary.total, FIFO_T);
        // The money did come in (D9).
        assert_eq!(r.stats.sales_count, 2);
        assert_eq!(r.uncovered_tickers, vec!["NON".to_string()]);
    }

    #[test]
    fn the_uncovered_alert_names_only_sales_of_the_period() {
        let conn = fixture();
        add_position(&conn, "OLD", "OLD");
        add_trade(&conn, "ob", "OLD", "buy", "1", "10", "CZK", D1, None);
        add_trade(&conn, "os", "OLD", "sell", "3", "10", "CZK", D2, None);
        let r = run(&conn, &request(StockReportCostMethod::Fifo, None));
        assert!(r.uncovered_tickers.is_empty(), "{:?}", r.uncovered_tickers);
        let mut req = request(StockReportCostMethod::Fifo, None);
        req.from_day = 1_704_067_200; // 2024-01-01
        req.to_day = 1_735_603_200; // 2024-12-31
        assert_eq!(run(&conn, &req).uncovered_tickers, vec!["OLD".to_string()]);
    }

    #[test]
    fn the_average_of_a_partly_uncovered_sale_covers_only_its_lots() {
        let conn = setup_test_db();
        add_position(&conn, "PPP", "PPP");
        add_trade(&conn, "p1", "PPP", "buy", "5", "10", "CZK", D1, None);
        add_trade(&conn, "p2", "PPP", "buy", "5", "20", "CZK", D2, None);
        add_trade(
            &conn,
            "ps",
            "PPP",
            "sell",
            "12",
            "30",
            "CZK",
            D3,
            Some("12"),
        );
        for (method, costs) in [
            (StockReportCostMethod::Fifo, [50.0, 100.0]),
            (StockReportCostMethod::Average, [75.0, 75.0]),
        ] {
            let r = run(&conn, &request(method, None));
            let sale = &r.sales[0];
            assert!(sale.counted);
            assert_eq!(sale.uncovered_quantity, "2");
            for (m, cost) in sale.matches.iter().zip(costs) {
                assert_match(m, [150.0, cost, 0.0, 5.0, 145.0 - cost]);
            }
            assert_amounts(&r.summary.total, [300.0, 150.0, 150.0, 10.0, 140.0]);
        }
        let r = run(&conn, &request(StockReportCostMethod::Average, None));
        assert_eq!(some(&r.sales[0].matches[0].average_cost), czk(15.0));
    }

    #[test]
    fn an_unconvertible_buy_poisons_the_pool_until_it_empties() {
        let conn = setup_test_db();
        add_position(&conn, "PPP", "PPP");
        add_trade(&conn, "x1", "PPP", "buy", "1", "10", "XXX", D1, None);
        add_trade(&conn, "c1", "PPP", "buy", "1", "10", "CZK", D1, None);
        add_trade(&conn, "s1", "PPP", "sell", "2", "20", "CZK", D2, None);
        add_trade(
            &conn,
            "c2",
            "PPP",
            "buy",
            "1",
            "30",
            "CZK",
            D2 + 86_400,
            None,
        );
        add_trade(&conn, "s2", "PPP", "sell", "1", "50", "CZK", D3, None);
        add_rate(&conn, D1, "CZK", 25.0);
        for method in [StockReportCostMethod::Fifo, StockReportCostMethod::Average] {
            let mut req = request(method, None);
            req.from_day = D1;
            let r = run(&conn, &req);
            let sale = |id: &str| r.sales.iter().find(|s| s.id == id).expect("sale");
            assert!(!sale("s1").counted, "{method:?}");
            assert!(sale("s1").matches.iter().all(|m| m.cost.is_none()));
            // Emptied by s1, the pool starts again clean with c2.
            assert!(sale("s2").counted, "{method:?}");
            assert_match(&sale("s2").matches[0], [50.0, 30.0, 20.0, 0.0, 20.0]);
            assert_eq!(r.summary.sales_count, 1);
            assert_amounts(&r.summary.total, [50.0, 30.0, 20.0, 0.0, 20.0]);
            assert!(r.fx_quality.missing_currencies.contains(&"XXX".to_string()));
        }
    }

    #[test]
    fn paid_fees_are_the_sold_pieces_fees_plus_the_open_lots() {
        let conn = fixture();
        let r = run(&conn, &request(StockReportCostMethod::Fifo, None));
        // D8: the sold pieces' buy fees (40 + 50) and the sale fee (75); D9: paid in 2026, 75.
        assert_eq!(r.summary.total.fees, czk(165.0));
        assert_eq!(r.stats.fees_paid, czk(75.0));
        // Over the whole history every paid fee is either sold (D8) or in the open lot (50).
        let mut req = request(StockReportCostMethod::Fifo, None);
        req.from_day = D1;
        let all = run(&conn, &req);
        assert_eq!(all.stats.fees_paid, czk(215.0));
        assert_eq!(
            units(&all.stats.fees_paid),
            units(&all.summary.total.fees) + 5_000
        );
    }

    #[test]
    fn a_position_without_transactions_adds_nothing() {
        let conn = setup_test_db();
        add_position(&conn, "EMP", "EMP");
        add_price(&conn, "EMP", "10", "CZK");
        let r = run(&conn, &request(StockReportCostMethod::Fifo, Some(36)));
        assert_eq!(r.first_transaction_day, None);
        assert!(r.sales.is_empty() && r.ledger.is_empty() && r.open_lots.is_empty());
        assert!(r.tickers.is_empty());

        let conn = fixture();
        add_position(&conn, "EMP", "EMP");
        add_price(&conn, "EMP", "10", "CZK");
        let r = run(&conn, &request(StockReportCostMethod::Fifo, Some(36)));
        assert_eq!(r.sales.len(), 1);
        assert!(r.ledger.iter().all(|l| l.ticker != "EMP"));
        assert!(r.open_lots.iter().all(|l| l.ticker != "EMP"));
        assert_amounts(&r.summary.total, FIFO_T);
    }

    #[test]
    fn same_day_rows_follow_the_replay_order() {
        let conn = setup_test_db();
        add_position(&conn, "SSS", "SSS");
        add_trade(&conn, "lot", "SSS", "buy", "4", "10", "CZK", D2, None);
        add_trade(&conn, "z-first", "SSS", "sell", "1", "20", "CZK", D3, None);
        add_trade(&conn, "a-second", "SSS", "sell", "1", "30", "CZK", D3, None);
        add_trade(&conn, "m-buy", "SSS", "buy", "1", "10", "CZK", D3, None);
        let r = run(&conn, &request(StockReportCostMethod::Fifo, None));
        let sales: Vec<&str> = r.sales.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(sales, ["a-second", "z-first"]);
        // Newest first: the later trade of the day above, the day's buy (replayed first) last.
        let ledger: Vec<&str> = r.ledger.iter().map(|l| l.id.as_str()).collect();
        assert_eq!(ledger, ["a-second", "z-first", "m-buy"]);
    }

    #[test]
    fn the_cost_method_does_not_change_which_lots_match() {
        let conn = fixture();
        let shape = |method| {
            run(&conn, &request(method, Some(36))).sales[0]
                .matches
                .iter()
                .map(|m| (m.buy_day, m.quantity.clone(), m.exempt))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            shape(StockReportCostMethod::Fifo),
            shape(StockReportCostMethod::Average)
        );
    }
}
