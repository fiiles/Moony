//! Activity of the stock transactions report: the stats (D9), the buy/sell buckets (D10) and the
//! ledger of trades and dividends (D14), all for the period of the request.

use std::collections::HashSet;

use chrono::{DateTime, Datelike, Days, Months, NaiveDate, Utc};
use rusqlite::Connection;

use super::lots::{Replay, TradeKind};
use super::sales::{Converter, SalesOutcome};
use crate::error::Result;
use crate::models::stock_dividends::amount_text;
use crate::models::{
    StockDividendSource, StockReportBucket, StockReportLedgerKind, StockReportLedgerRow,
    StockReportStats, StockTransactionsReportRequest,
};
use crate::services::reporting::money_in_currency;
use crate::services::stock_dividends;

/// A period touching more calendar months than this is bucketed by year.
const MAX_MONTHLY_BUCKETS: i64 = 24;

/// A counted dividend payout with an ex-date in the period.
pub struct Dividend {
    pub id: String,
    pub investment_id: String,
    pub ticker: String,
    pub company_name: String,
    pub ex_date: i64,
    /// Gross total in `currency`.
    pub gross: f64,
    pub currency: String,
    /// Shares held and amount per share; only for a Yahoo payout.
    pub shares: Option<String>,
    pub per_share: Option<String>,
}

pub struct Activity {
    pub stats: StockReportStats,
    pub buckets: Vec<StockReportBucket>,
    pub ledger: Vec<StockReportLedgerRow>,
}

/// The counted payouts (never a superseded manual one, none after `today`) of every stock
/// position with the ex-date in the period, oldest first.
pub fn dividends_in_period(
    conn: &Connection,
    request: &StockTransactionsReportRequest,
    today: i64,
) -> Result<Vec<Dividend>> {
    let mut stmt = conn.prepare("SELECT id, ticker, company_name FROM stock_investments")?;
    let positions = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut found = Vec::new();
    for (investment_id, ticker, company_name) in positions {
        let (items, _) = stock_dividends::for_investment(conn, &investment_id, today)?;
        for item in items {
            if item.superseded
                || item.ex_date > today
                || item.ex_date < request.from_day
                || item.ex_date > request.to_day
            {
                continue;
            }
            let Ok(gross) = item.gross_amount.parse::<f64>() else {
                continue;
            };
            let yahoo = item.source == StockDividendSource::Yahoo;
            found.push(Dividend {
                id: item.id,
                investment_id: investment_id.clone(),
                ticker: ticker.clone(),
                company_name: company_name.clone(),
                ex_date: item.ex_date,
                gross,
                currency: item.currency,
                shares: item.shares.filter(|_| yahoo),
                per_share: item.amount_per_share.filter(|_| yahoo),
            });
        }
    }
    found.sort_by(|a, b| a.ex_date.cmp(&b.ex_date).then_with(|| a.id.cmp(&b.id)));
    Ok(found)
}

pub fn build(
    replay: &Replay,
    outcome: &SalesOutcome,
    dividends: &[Dividend],
    request: &StockTransactionsReportRequest,
    currency: &str,
    conv: &mut Converter,
) -> Activity {
    let in_period = |day: i64| day >= request.from_day && day <= request.to_day;
    let valued = &outcome.valued;

    let mut bought = 0.0;
    let mut proceeds = 0.0;
    let mut fees = 0.0;
    let (mut buys_count, mut sales_count) = (0u32, 0u32);
    let mut bought_tickers: HashSet<&str> = HashSet::new();
    let mut sold_tickers: HashSet<&str> = HashSet::new();
    let mut ledger = Vec::new();

    for (index, trade) in replay.trades.iter().enumerate() {
        if !in_period(trade.day) {
            continue;
        }
        fees += valued.fee[index].unwrap_or(0.0);
        let kind = match trade.kind {
            TradeKind::Buy => {
                buys_count += 1;
                bought_tickers.insert(&trade.investment_id);
                bought += valued.volume[index].unwrap_or(0.0);
                StockReportLedgerKind::Buy
            }
            TradeKind::Sell => {
                sales_count += 1;
                sold_tickers.insert(&trade.investment_id);
                proceeds += valued.proceeds[index].unwrap_or(0.0);
                StockReportLedgerKind::Sell
            }
        };
        ledger.push(StockReportLedgerRow {
            id: trade.id.clone(),
            kind,
            day: trade.day,
            investment_id: trade.investment_id.clone(),
            ticker: trade.ticker.clone(),
            company_name: trade.company_name.clone(),
            quantity: Some(amount_text(trade.quantity)),
            price: Some(amount_text(trade.price)),
            currency: trade.currency.clone(),
            rate: conv.rate(&trade.currency, trade.day).map(amount_text),
            amount: valued.volume[index].map(|v| money_in_currency(v, currency)),
            fee: (trade.fee != 0.0).then(|| amount_text(trade.fee)),
        });
    }

    let mut dividend_sum = 0.0;
    for dividend in dividends {
        let amount = conv.tracked(dividend.gross, &dividend.currency, dividend.ex_date);
        dividend_sum += amount.unwrap_or(0.0);
        ledger.push(StockReportLedgerRow {
            id: dividend.id.clone(),
            kind: StockReportLedgerKind::Dividend,
            day: dividend.ex_date,
            investment_id: dividend.investment_id.clone(),
            ticker: dividend.ticker.clone(),
            company_name: dividend.company_name.clone(),
            quantity: dividend.shares.clone(),
            price: dividend.per_share.clone(),
            currency: dividend.currency.clone(),
            rate: conv
                .rate(&dividend.currency, dividend.ex_date)
                .map(amount_text),
            amount: amount.map(|v| money_in_currency(v, currency)),
            fee: None,
        });
    }
    ledger.sort_by(|a, b| b.day.cmp(&a.day).then_with(|| b.id.cmp(&a.id)));

    let stats = StockReportStats {
        proceeds: money_in_currency(proceeds, currency),
        sales_count,
        sold_tickers: sold_tickers.len() as u32,
        bought: money_in_currency(bought, currency),
        buys_count,
        bought_tickers: bought_tickers.len() as u32,
        fees_paid: money_in_currency(fees, currency),
        traded_volume: money_in_currency(bought + proceeds, currency),
        dividends: money_in_currency(dividend_sum, currency),
        dividends_count: dividends.len() as u32,
    };

    Activity {
        stats,
        buckets: buckets(replay, outcome, request, currency),
        ledger,
    }
}

fn date_of(day: i64) -> Option<NaiveDate> {
    DateTime::<Utc>::from_timestamp(day, 0).map(|d| d.date_naive())
}

fn midnight(date: NaiveDate) -> i64 {
    date.and_hms_opt(0, 0, 0)
        .map_or(0, |d| d.and_utc().timestamp())
}

/// Calendar months of the period (or years when it touches more than 24 months), each clipped
/// to the period, empty ones included.
fn buckets(
    replay: &Replay,
    outcome: &SalesOutcome,
    request: &StockTransactionsReportRequest,
    currency: &str,
) -> Vec<StockReportBucket> {
    let (Some(first), Some(last)) = (date_of(request.from_day), date_of(request.to_day)) else {
        return Vec::new();
    };
    let months = i64::from(last.year() - first.year()) * 12 + i64::from(last.month())
        - i64::from(first.month())
        + 1;
    let yearly = months > MAX_MONTHLY_BUCKETS;
    let step = if yearly { 12 } else { 1 };
    let mut start = if yearly {
        NaiveDate::from_ymd_opt(first.year(), 1, 1)
    } else {
        first.with_day(1)
    };
    let mut result = Vec::new();
    while let Some(bucket_start) = start {
        if bucket_start > last {
            break;
        }
        let Some(next) = bucket_start.checked_add_months(Months::new(step)) else {
            break;
        };
        let from_day = midnight(bucket_start).max(request.from_day);
        let to_day = next
            .checked_sub_days(Days::new(1))
            .map_or(request.to_day, midnight)
            .min(request.to_day);
        let (mut buys, mut sells, mut res) = (0.0, 0.0, 0.0);
        for (index, trade) in replay.trades.iter().enumerate() {
            if trade.day < from_day || trade.day > to_day {
                continue;
            }
            match trade.kind {
                TradeKind::Buy => buys += outcome.valued.volume[index].unwrap_or(0.0),
                TradeKind::Sell => sells += outcome.valued.proceeds[index].unwrap_or(0.0),
            }
        }
        for (day, after_fees) in &outcome.results {
            if *day >= from_day && *day <= to_day {
                res += after_fees;
            }
        }
        result.push(StockReportBucket {
            key: if yearly {
                format!("{:04}", bucket_start.year())
            } else {
                format!("{:04}-{:02}", bucket_start.year(), bucket_start.month())
            },
            from_day,
            to_day,
            buys: money_in_currency(buys, currency),
            sells: money_in_currency(sells, currency),
            result: money_in_currency(res, currency),
        });
        start = Some(next);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::super::report;
    use super::super::tests::*;
    use crate::models::{
        StockReportCostMethod, StockReportLedgerKind, StockTransactionsReport,
        StockTransactionsReportRequest,
    };
    use crate::services::reporting::money_in_currency;

    const TODAY: i64 = 1_790_000_000;
    const Y2026: (i64, i64) = (1_767_225_600, 1_798_675_200);
    const Y2024: (i64, i64) = (1_704_067_200, 1_735_603_200);
    const EX_DATE: i64 = 1_773_100_800; // 2026-03-10

    fn czk(amount: f64) -> String {
        money_in_currency(amount, "CZK")
    }

    fn run(
        conn: &rusqlite::Connection,
        period: (i64, i64),
        method: StockReportCostMethod,
    ) -> StockTransactionsReport {
        let request = StockTransactionsReportRequest {
            from_day: period.0,
            to_day: period.1,
            report_currency: "CZK".into(),
            cost_method: method,
            holding_test_months: None,
        };
        report(conn, &request, TODAY).expect("report")
    }

    fn yahoo_dividend(conn: &rusqlite::Connection, ticker: &str, ex_date: i64, per_share: &str) {
        conn.execute(
            "INSERT INTO stock_dividend_events (ticker, ex_date, amount, currency, fetched_at)
             VALUES (?1, ?2, ?3, 'USD', 0)",
            rusqlite::params![ticker, ex_date, per_share],
        )
        .expect("dividend event");
        add_rate(conn, ex_date, "CZK", 25.0);
        add_rate(conn, ex_date, "USD", 1.0);
    }

    #[test]
    fn stats_cover_every_trade_of_the_period() {
        let conn = fixture();
        let r = run(&conn, Y2026, StockReportCostMethod::Fifo);
        let s = &r.stats;
        assert_eq!(s.proceeds, czk(112_500.0));
        assert_eq!((s.sales_count, s.sold_tickers), (1, 1));
        assert_eq!(s.bought, czk(0.0));
        assert_eq!((s.buys_count, s.bought_tickers), (0, 0));
        assert_eq!(s.fees_paid, czk(75.0));
        assert_eq!(s.traded_volume, czk(112_500.0));
        assert_eq!(s.dividends, czk(0.0));
        assert_eq!(s.dividends_count, 0);

        let r = run(&conn, Y2024, StockReportCostMethod::Fifo);
        let s = &r.stats;
        assert_eq!(s.bought, czk(50_000.0));
        assert_eq!((s.buys_count, s.bought_tickers), (1, 1));
        assert_eq!(s.fees_paid, czk(100.0));
        assert_eq!(s.proceeds, czk(0.0));
        assert_eq!(s.sales_count, 0);
    }

    #[test]
    fn stats_keep_the_whole_oversold_sale() {
        let conn = fixture();
        add_position(&conn, "BBB", "BBB");
        add_trade(&conn, "bb", "BBB", "buy", "5", "20", "CZK", D2, None);
        add_trade(&conn, "bs", "BBB", "sell", "8", "20", "CZK", D3, None);
        let r = run(&conn, Y2026, StockReportCostMethod::Fifo);
        assert_eq!(r.stats.proceeds, czk(112_660.0));
        assert_eq!(r.stats.sales_count, 2);
        assert_eq!(r.stats.sold_tickers, 2);
        assert_eq!(r.summary.total.proceeds, czk(112_600.0));
    }

    #[test]
    fn a_year_has_twelve_monthly_buckets() {
        let conn = fixture();
        let r = run(&conn, Y2026, StockReportCostMethod::Fifo);
        let keys: Vec<&str> = r.buckets.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(
            keys,
            [
                "2026-01", "2026-02", "2026-03", "2026-04", "2026-05", "2026-06", "2026-07",
                "2026-08", "2026-09", "2026-10", "2026-11", "2026-12"
            ]
        );
        for bucket in &r.buckets {
            if bucket.key == "2026-02" {
                assert_eq!(bucket.sells, czk(112_500.0));
                assert_eq!(bucket.result, czk(67_335.0));
                assert_eq!(bucket.buys, czk(0.0));
            } else {
                assert_eq!(bucket.sells, czk(0.0), "{}", bucket.key);
                assert_eq!(bucket.result, czk(0.0), "{}", bucket.key);
            }
        }
        assert_eq!(r.buckets[0].from_day, Y2026.0);
        assert_eq!(r.buckets[11].to_day, Y2026.1);
    }

    #[test]
    fn a_long_period_is_bucketed_by_year() {
        let conn = fixture();
        let r = run(
            &conn,
            (1_672_531_200, Y2026.1), // 2023-01-01 .. 2026-12-31
            StockReportCostMethod::Fifo,
        );
        let keys: Vec<&str> = r.buckets.iter().map(|b| b.key.as_str()).collect();
        assert_eq!(keys, ["2023", "2024", "2025", "2026"]);
        assert_eq!(r.buckets[0].buys, czk(20_000.0));
        assert_eq!(r.buckets[1].buys, czk(50_000.0));
        assert_eq!(r.buckets[3].sells, czk(112_500.0));
        assert_eq!(r.buckets[0].from_day, 1_672_531_200);
        assert_eq!(r.buckets[0].to_day, 1_704_067_199 - 86_399); // 2023-12-31
    }

    #[test]
    fn a_custom_period_clips_its_first_and_last_bucket() {
        let conn = fixture();
        let (from, to) = (1_770_249_600, 1_773_964_800); // 2026-02-05 .. 2026-03-20
        let r = run(&conn, (from, to), StockReportCostMethod::Fifo);
        assert_eq!(r.buckets.len(), 2);
        assert_eq!(r.buckets[0].key, "2026-02");
        assert_eq!(r.buckets[0].from_day, from);
        assert_eq!(r.buckets[1].key, "2026-03");
        assert_eq!(r.buckets[1].to_day, to);
    }

    #[test]
    fn the_ledger_lists_trades_and_dividends_newest_first() {
        let conn = fixture();
        yahoo_dividend(&conn, "AAA", EX_DATE, "1");
        let r = run(&conn, Y2026, StockReportCostMethod::Fifo);
        assert_eq!(r.ledger.len(), 2);
        let dividend = &r.ledger[0];
        assert_eq!(dividend.kind, StockReportLedgerKind::Dividend);
        assert_eq!(dividend.day, EX_DATE);
        assert_eq!(dividend.quantity.as_deref(), Some("5"));
        assert_eq!(dividend.price.as_deref(), Some("1"));
        assert_eq!(dividend.amount.as_deref(), Some(czk(125.0).as_str()));
        assert!(dividend.fee.is_none());
        let sale = &r.ledger[1];
        assert_eq!(sale.kind, StockReportLedgerKind::Sell);
        assert_eq!(sale.amount.as_deref(), Some(czk(112_500.0).as_str()));
        assert_eq!(sale.fee.as_deref(), Some("3"));
        assert_eq!(sale.rate.as_deref(), Some("25"));
        assert_eq!(r.stats.dividends, czk(125.0));
        assert_eq!(r.stats.dividends_count, 1);
    }

    #[test]
    fn a_manual_dividend_has_no_quantity_or_price() {
        let conn = fixture();
        add_position(&conn, "MAN", "MAN");
        add_trade(&conn, "mb", "MAN", "buy", "1", "10", "CZK", D2, None);
        conn.execute(
            "INSERT INTO investment_dividends (id, investment_id, ex_date, gross_amount, currency)
             VALUES ('d1', 'MAN', ?1, '40', 'USD')",
            [EX_DATE],
        )
        .expect("manual dividend");
        add_rate(&conn, EX_DATE, "CZK", 25.0);
        add_rate(&conn, EX_DATE, "USD", 1.0);
        let r = run(&conn, Y2026, StockReportCostMethod::Fifo);
        let row = r
            .ledger
            .iter()
            .find(|l| l.id == "d1")
            .expect("dividend row");
        assert_eq!(row.kind, StockReportLedgerKind::Dividend);
        assert!(row.quantity.is_none() && row.price.is_none());
        assert_eq!(row.amount.as_deref(), Some(czk(1_000.0).as_str()));
        assert_eq!(r.stats.dividends, czk(1_000.0));
    }

    #[test]
    fn a_dividend_outside_the_period_is_left_out() {
        let conn = fixture();
        yahoo_dividend(&conn, "AAA", EX_DATE, "1");
        let r = run(&conn, Y2024, StockReportCostMethod::Fifo);
        assert_eq!(r.stats.dividends_count, 0);
        assert!(r
            .ledger
            .iter()
            .all(|l| l.kind != StockReportLedgerKind::Dividend));
    }
}
