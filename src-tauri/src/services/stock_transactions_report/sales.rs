//! Valuation of the realized sales of the stock transactions report: converts every trade at its
//! own day, prices each match by the cost method, applies the holding-period test and sums the
//! counted sales into the summary and the per-ticker results (spec D5 to D7, D15, D16).

use std::collections::{BTreeMap, HashMap};

use chrono::{DateTime, Days, Months, Utc};

use super::lots::{Match, Replay, Trade, TradeKind};
use crate::models::stock_dividends::amount_text;
use crate::models::{
    StockReportAmounts, StockReportCostMethod, StockReportMatch, StockReportSale,
    StockReportSummary, StockReportTickerResult, StockTransactionsReportRequest,
};
use crate::services::fx::{History, Quality, Quotes};
use crate::services::reporting::money_in_currency;

/// Quantities below this are rounding dust.
const EPSILON: f64 = 1e-9;
/// Results this close to zero are neither a profit nor a loss.
const RESULT_EPSILON: f64 = 0.005;

/// Converts at a trade's day, falling back to the current quotes (marking the response
/// estimated, the way `cost_basis` does), and records what could not be converted at all.
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

/// Per trade index (replay order), in the report currency at the trade's day.
pub struct Valued {
    /// Quantity times price, for every trade.
    pub volume: Vec<Option<f64>>,
    /// The same as `volume` for sales, `None` for buys.
    pub proceeds: Vec<Option<f64>>,
    /// The fee of the whole trade, `Some(0.0)` when it has none.
    pub fee: Vec<Option<f64>>,
}

/// The average method's pool of one position after the whole replay.
#[derive(Debug, Clone, PartialEq)]
pub struct PoolState {
    pub quantity: f64,
    /// `None` once an unconvertible buy entered the pool (until it empties).
    pub cost: Option<f64>,
    pub fee: Option<f64>,
}

/// What `value_sales` produces for the report and for Task 3's stats, ledger and open lots.
pub struct SalesOutcome {
    pub valued: Valued,
    pub pools: HashMap<String, PoolState>,
    /// Sales in the period, newest first.
    pub sales: Vec<StockReportSale>,
    pub summary: StockReportSummary,
    pub tickers: Vec<StockReportTickerResult>,
    pub uncovered_tickers: Vec<String>,
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

fn value_trades(trades: &[Trade], conv: &mut Converter) -> Valued {
    let mut valued = Valued {
        volume: Vec::with_capacity(trades.len()),
        proceeds: Vec::with_capacity(trades.len()),
        fee: Vec::with_capacity(trades.len()),
    };
    for trade in trades {
        let volume = conv.tracked(trade.quantity * trade.price, &trade.currency, trade.day);
        valued.proceeds.push(match trade.kind {
            TradeKind::Sell => volume,
            TradeKind::Buy => None,
        });
        valued.volume.push(volume);
        valued.fee.push(if trade.fee == 0.0 {
            Some(0.0)
        } else {
            conv.tracked(trade.fee, &trade.currency, trade.day)
        });
    }
    valued
}

/// Per sale index: average cost and buy fee per share before the sale.
type AverageBeforeSale = HashMap<usize, (Option<f64>, Option<f64>)>;

/// Replays the pools of the average method over the whole history: per sale the average cost
/// and buy fee per share before it, per position the pool left at the end.
fn average_pools(
    replay: &Replay,
    valued: &Valued,
    uncovered: &HashMap<usize, f64>,
) -> (AverageBeforeSale, HashMap<String, PoolState>) {
    let mut pools: HashMap<String, PoolState> = HashMap::new();
    let mut per_sale = HashMap::new();
    for (index, trade) in replay.trades.iter().enumerate() {
        let pool = pools
            .entry(trade.investment_id.clone())
            .or_insert(PoolState {
                quantity: 0.0,
                cost: Some(0.0),
                fee: Some(0.0),
            });
        match trade.kind {
            TradeKind::Buy => {
                pool.quantity += trade.quantity;
                pool.cost = pool.cost.zip(valued.volume[index]).map(|(a, b)| a + b);
                pool.fee = pool.fee.zip(valued.fee[index]).map(|(a, b)| a + b);
            }
            TradeKind::Sell => {
                let covered = trade.quantity - uncovered.get(&index).copied().unwrap_or(0.0);
                if pool.quantity < EPSILON || covered < EPSILON {
                    per_sale.insert(index, (None, None));
                    continue;
                }
                let per_share = |total: Option<f64>| total.map(|t| t / pool.quantity);
                per_sale.insert(index, (per_share(pool.cost), per_share(pool.fee)));
                let kept = ((pool.quantity - covered) / pool.quantity).max(0.0);
                pool.quantity -= covered;
                pool.cost = pool.cost.map(|c| c * kept);
                pool.fee = pool.fee.map(|f| f * kept);
                if pool.quantity < EPSILON {
                    *pool = PoolState {
                        quantity: 0.0,
                        cost: Some(0.0),
                        fee: Some(0.0),
                    };
                }
            }
        }
    }
    (per_sale, pools)
}

/// One match in the report currency; `None` fields when anything it needs is unconvertible.
struct MatchAmounts {
    proceeds: Option<f64>,
    cost: Option<f64>,
    fees: Option<f64>,
    average_cost: Option<f64>,
}

impl MatchAmounts {
    fn known(&self) -> Option<(f64, f64, f64)> {
        Some((self.proceeds?, self.cost?, self.fees?))
    }
}

fn amounts_of(
    m: &Match,
    replay: &Replay,
    valued: &Valued,
    method: StockReportCostMethod,
    average: Option<&(Option<f64>, Option<f64>)>,
) -> MatchAmounts {
    let sale = &replay.trades[m.sale];
    let buy = &replay.trades[m.buy];
    let share_of_sale = m.quantity / sale.quantity;
    let proceeds = valued.proceeds[m.sale].map(|p| p * share_of_sale);
    let sale_fee = valued.fee[m.sale].map(|f| f * share_of_sale);
    let (cost, buy_fee, average_cost) = match method {
        StockReportCostMethod::Fifo => {
            let share_of_lot = m.quantity / buy.quantity;
            (
                valued.volume[m.buy].map(|v| v * share_of_lot),
                valued.fee[m.buy].map(|f| f * share_of_lot),
                None,
            )
        }
        StockReportCostMethod::Average => {
            let (cost, fee) = average.copied().unwrap_or((None, None));
            (
                cost.map(|c| c * m.quantity),
                fee.map(|f| f * m.quantity),
                cost,
            )
        }
    };
    MatchAmounts {
        proceeds,
        cost,
        fees: buy_fee.zip(sale_fee).map(|(a, b)| a + b),
        average_cost,
    }
}

#[derive(Default, Clone, Copy)]
struct Sums {
    proceeds: f64,
    cost: f64,
    fees: f64,
}

impl Sums {
    fn add(&mut self, (proceeds, cost, fees): (f64, f64, f64)) {
        self.proceeds += proceeds;
        self.cost += cost;
        self.fees += fees;
    }

    fn after_fees(&self) -> f64 {
        self.proceeds - self.cost - self.fees
    }

    fn wire(&self, currency: &str) -> StockReportAmounts {
        StockReportAmounts {
            proceeds: money_in_currency(self.proceeds, currency),
            cost: money_in_currency(self.cost, currency),
            result: money_in_currency(self.proceeds - self.cost, currency),
            fees: money_in_currency(self.fees, currency),
            result_after_fees: money_in_currency(self.after_fees(), currency),
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
    let valued = value_trades(&replay.trades, conv);
    let uncovered: HashMap<usize, f64> = replay.uncovered.iter().copied().collect();
    let (average, pools) = average_pools(replay, &valued, &uncovered);
    let months = request
        .holding_test_months
        .and_then(|m| u32::try_from(m).ok());

    let mut by_sale: BTreeMap<usize, Vec<&Match>> = BTreeMap::new();
    for m in &replay.matches {
        by_sale.entry(m.sale).or_default().push(m);
    }

    let mut total = Sums::default();
    let mut taxable = Sums::default();
    let mut exempt = Sums::default();
    let (mut count, mut in_profit, mut in_loss) = (0u32, 0u32, 0u32);
    let mut tickers: BTreeMap<String, TickerSums> = BTreeMap::new();
    let mut sales = Vec::new();
    let mut uncovered_tickers: Vec<String> = Vec::new();

    for (index, sale) in replay.trades.iter().enumerate() {
        if sale.kind != TradeKind::Sell {
            continue;
        }
        let uncovered_quantity = uncovered.get(&index).copied().unwrap_or(0.0);
        if uncovered_quantity > 0.0 && !uncovered_tickers.contains(&sale.ticker) {
            uncovered_tickers.push(sale.ticker.clone());
        }
        if sale.day < request.from_day || sale.day > request.to_day {
            continue;
        }
        let matches: Vec<&Match> = by_sale.get(&index).cloned().unwrap_or_default();
        let amounts: Vec<MatchAmounts> = matches
            .iter()
            .map(|m| amounts_of(m, replay, &valued, request.cost_method, average.get(&index)))
            .collect();
        let known: Option<Vec<(f64, f64, f64)>> = amounts.iter().map(|a| a.known()).collect();
        let counted = valued.proceeds[index].is_some() && valued.fee[index].is_some();
        let known = known.filter(|_| counted);
        let exempt_flags: Vec<Option<bool>> = matches
            .iter()
            .map(|m| months.map(|n| sale.day >= pass_day(replay.trades[m.buy].day, n)))
            .collect();

        let mut wire_matches = Vec::new();
        for (position, m) in matches.iter().enumerate() {
            let buy = &replay.trades[m.buy];
            let figures = known.as_ref().map(|k| k[position]);
            wire_matches.push(StockReportMatch {
                buy_day: buy.day,
                quantity: amount_text(m.quantity),
                buy_price: amount_text(buy.price),
                buy_currency: buy.currency.clone(),
                buy_rate: conv.rate(&buy.currency, buy.day).map(amount_text),
                average_cost: if figures.is_some() {
                    amounts[position]
                        .average_cost
                        .map(|c| money_in_currency(c, currency))
                } else {
                    None
                },
                proceeds: figures.map(|f| money_in_currency(f.0, currency)),
                cost: figures.map(|f| money_in_currency(f.1, currency)),
                fees: figures.map(|f| money_in_currency(f.2, currency)),
                result_after_fees: figures.map(|f| money_in_currency(f.0 - f.1 - f.2, currency)),
                exempt: exempt_flags[position],
            });
        }

        if let Some(figures) = &known {
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
                entry.quantity += matches[position].quantity;
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
            if result > RESULT_EPSILON {
                in_profit += 1;
            } else if result < -RESULT_EPSILON {
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
    sales.sort_by(|a, b| b.day.cmp(&a.day).then_with(|| b.id.cmp(&a.id)));

    let test_on = months.is_some();
    let mut ticker_rows: Vec<(f64, StockReportTickerResult)> = tickers
        .into_iter()
        .map(|(investment_id, t)| {
            (
                t.total.after_fees(),
                StockReportTickerResult {
                    investment_id,
                    ticker: t.ticker,
                    company_name: t.company_name,
                    quantity: amount_text(t.quantity),
                    result_after_fees: money_in_currency(t.total.after_fees(), currency),
                    taxable_result: test_on
                        .then(|| money_in_currency(t.taxable.after_fees(), currency)),
                    exempt_result: test_on
                        .then(|| money_in_currency(t.exempt.after_fees(), currency)),
                },
            )
        })
        .collect();
    ticker_rows.sort_by(|a, b| {
        b.0.total_cmp(&a.0)
            .then_with(|| a.1.ticker.cmp(&b.1.ticker))
    });

    let summary = StockReportSummary {
        total: total.wire(currency),
        taxable: test_on.then(|| taxable.wire(currency)),
        exempt: test_on.then(|| exempt.wire(currency)),
        sales_count: count,
        sales_in_profit: in_profit,
        sales_in_loss: in_loss,
    };
    SalesOutcome {
        valued,
        pools,
        sales,
        summary,
        tickers: ticker_rows.into_iter().map(|(_, row)| row).collect(),
        uncovered_tickers,
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

    fn num(text: &str) -> f64 {
        text.parse().expect("number")
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
        let total = &r.summary.total;
        for (t, e, all) in [
            (&taxable.proceeds, &exempt.proceeds, &total.proceeds),
            (&taxable.cost, &exempt.cost, &total.cost),
            (&taxable.result, &exempt.result, &total.result),
            (&taxable.fees, &exempt.fees, &total.fees),
            (
                &taxable.result_after_fees,
                &exempt.result_after_fees,
                &total.result_after_fees,
            ),
        ] {
            assert!((num(t) + num(e) - num(all)).abs() < 1e-6);
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
            let sum =
                num(t.taxable_result.as_ref().unwrap()) + num(t.exempt_result.as_ref().unwrap());
            assert!((sum - num(&t.result_after_fees)).abs() < 1e-6);
        }
        assert_eq!(r.summary.sales_in_loss, 1);
        let off = run(&conn, &request(StockReportCostMethod::Fifo, None));
        assert!(off.tickers[0].taxable_result.is_none());
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
