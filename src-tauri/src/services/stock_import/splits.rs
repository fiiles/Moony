//! The split check of the import wizard (spec D12, D22): which splits Yahoo reports after the
//! first trade of each instrument the file would import, and whether the file's prices are
//! already adjusted for them (D7, against the fetched closes, never `stock_value_history`).
//! Reads only; the network part runs with the database lock released.

use std::collections::HashMap;
use std::future::Future;

use rusqlite::{params, Connection};

use crate::db::Database;
use crate::error::Result;
use crate::services::price_api::{self, HistoricalSeries};
use crate::services::stock_splits::classify::{classify_cumulative, verdict_of};
use crate::services::stock_splits::types::ratio_is_valid;
use crate::services::stock_splits::{DetectedSplit, StockSplitStatus, StockSplitVerdict};

use super::simulate::simulate;
use super::types::{
    ParsedFile, StockImportConfig, StockSplitCheck, StockSplitCheckSplit, StockSplitCheckTrade,
};

const DAY_SECONDS: i64 = 86_400;

/// The will-import trades of one instrument and what Moony stores for its ticker.
#[derive(Debug, Clone, Default)]
pub(crate) struct TickerGroup {
    /// `StockImportInstrument.key`.
    pub key: String,
    pub ticker: String,
    /// `(line, day, price, currency)` in file order; the price as it would be stored.
    pub trades: Vec<(usize, i64, f64, String)>,
    /// Status of the ticker's `stock_splits` rows by effective day.
    pub stored_status: HashMap<i64, StockSplitStatus>,
    /// Days of the ticker's stored transactions, ascending (the pre-split count of any day).
    pub stored_days: Vec<i64>,
}

/// Per instrument: the splits after its first will-import trade with the D7 verdicts.
///
/// The file is replayed with the config as given but without its split choices (the check
/// is what fills them), the groups and the stored state are read under one short lock, and
/// the Yahoo history is fetched after it is released. A ticker whose history cannot be
/// fetched is reported `checked: false`. Writes nothing.
pub async fn check_splits(
    db: &Database,
    parsed: &ParsedFile,
    config: &StockImportConfig,
) -> Result<Vec<StockSplitCheck>> {
    let groups = db.with_conn(|conn| groups(conn, parsed, config))?;
    let today = floor_day(chrono::Utc::now().timestamp());
    Ok(check_with(fetch_series, groups, today).await)
}

/// The ticker's daily closes and splits over `[from, to]` from Yahoo; an error when the
/// fetch failed (the non-strict fetch then leaves the ticker out).
async fn fetch_series(ticker: String, from: i64, to: i64) -> Result<HistoricalSeries> {
    let mut fetched = price_api::get_historical_stock_series_yahoo(
        std::slice::from_ref(&ticker),
        from,
        to,
        false,
    )
    .await?;
    Ok(fetched.remove(&ticker).unwrap_or_default())
}

/// The will-import trades of `parsed` by instrument (instruments without a ticker are left
/// out), in order of first appearance, with their ticker's stored state.
pub(crate) fn groups(
    conn: &Connection,
    parsed: &ParsedFile,
    config: &StockImportConfig,
) -> Result<Vec<TickerGroup>> {
    let mut config = config.clone();
    for over in &mut config.instrument_overrides {
        over.splits.clear();
    }
    let simulation = simulate(conn, parsed, &config)?;

    let mut groups: Vec<TickerGroup> = Vec::new();
    let mut index_of: HashMap<String, usize> = HashMap::new();
    for trade in simulation.trades.iter().filter(|t| t.will_import()) {
        let Some(ticker) = &trade.ticker else {
            continue;
        };
        let index = *index_of
            .entry(trade.instrument_key.clone())
            .or_insert_with(|| {
                groups.push(TickerGroup {
                    key: trade.instrument_key.clone(),
                    ticker: ticker.clone(),
                    ..TickerGroup::default()
                });
                groups.len() - 1
            });
        groups[index].trades.push((
            trade.line,
            trade.day,
            trade.price,
            trade.currency.clone().unwrap_or_default(),
        ));
    }

    let mut stored: HashMap<String, (HashMap<i64, StockSplitStatus>, Vec<i64>)> = HashMap::new();
    for group in &mut groups {
        if !stored.contains_key(&group.ticker) {
            let state = (
                stored_statuses(conn, &group.ticker)?,
                stored_days(conn, &group.ticker)?,
            );
            stored.insert(group.ticker.clone(), state);
        }
        if let Some((statuses, days)) = stored.get(&group.ticker) {
            group.stored_status = statuses.clone();
            group.stored_days = days.clone();
        }
    }
    Ok(groups)
}

/// Status of every `stock_splits` row of `ticker` by effective day.
fn stored_statuses(conn: &Connection, ticker: &str) -> Result<HashMap<i64, StockSplitStatus>> {
    let mut stmt =
        conn.prepare("SELECT effective_day, status FROM stock_splits WHERE ticker = ?1")?;
    let rows = stmt.query_map(params![ticker], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut statuses = HashMap::new();
    for row in rows {
        let (day, status) = row?;
        statuses.insert(day, StockSplitStatus::parse(&status)?);
    }
    Ok(statuses)
}

/// Days of the stored transactions of `ticker`, ascending.
fn stored_days(conn: &Connection, ticker: &str) -> Result<Vec<i64>> {
    let mut stmt = conn.prepare(
        "SELECT transaction_date FROM investment_transactions WHERE ticker = ?1
         ORDER BY transaction_date",
    )?;
    let days = stmt
        .query_map(params![ticker], |r| r.get(0))?
        .collect::<std::result::Result<Vec<i64>, _>>()?;
    Ok(days)
}

/// [`check_splits`] once the groups are read, with the history fetch injected: `fetch(ticker,
/// from, to)` returns the ticker's series. Each ticker is fetched once, from the earliest
/// trade of its groups through `today`; the checks keep the order of `groups`.
pub(crate) async fn check_with<F, Fut>(
    fetch: F,
    groups: Vec<TickerGroup>,
    today: i64,
) -> Vec<StockSplitCheck>
where
    F: Fn(String, i64, i64) -> Fut,
    Fut: Future<Output = Result<HistoricalSeries>>,
{
    let mut first_day_of: Vec<(String, i64)> = Vec::new();
    for group in &groups {
        let Some(first) = first_day(group) else {
            continue;
        };
        match first_day_of.iter_mut().find(|(t, _)| *t == group.ticker) {
            Some((_, day)) => *day = (*day).min(first),
            None => first_day_of.push((group.ticker.clone(), first)),
        }
    }

    let mut series_of: HashMap<String, HistoricalSeries> = HashMap::new();
    for (ticker, from) in first_day_of {
        match fetch(ticker.clone(), from, today).await {
            // No closes at all: Yahoo does not know the symbol.
            Ok(series) if !series.prices.is_empty() => {
                series_of.insert(ticker, series);
            }
            Ok(_) => log::warn!("[STOCK-IMPORT] No price history for {ticker}; splits unchecked"),
            Err(e) => log::warn!("[STOCK-IMPORT] Split check failed for {ticker}: {e}"),
        }
    }

    groups
        .into_iter()
        .map(|group| match series_of.get(&group.ticker) {
            Some(series) => StockSplitCheck {
                splits: check_group(&group, series),
                key: group.key,
                ticker: group.ticker,
                checked: true,
            },
            None => StockSplitCheck {
                key: group.key,
                ticker: group.ticker,
                checked: false,
                splits: Vec::new(),
            },
        })
        .collect()
}

/// The group's first trade day; `None` for a group without trades.
fn first_day(group: &TickerGroup) -> Option<i64> {
    group.trades.iter().map(|&(_, day, _, _)| day).min()
}

/// The splits of `series` after the group's first trade, oldest first, each with the D7
/// verdict of the trades dated before it (a trade on the ex-date is post-split). A trade is
/// classified against the cumulative factor of every split dated after it, since the file's
/// rows are the broker's and the closes are adjusted for all splits.
fn check_group(group: &TickerGroup, series: &HistoricalSeries) -> Vec<StockSplitCheckSplit> {
    let Some(first) = first_day(group) else {
        return Vec::new();
    };
    let closes: HashMap<i64, (f64, &str)> = series
        .prices
        .iter()
        .map(|p| (floor_day(p.timestamp), (p.price, p.currency.as_str())))
        .collect();

    let mut splits: Vec<DetectedSplit> = series
        .splits
        .iter()
        .filter(|s| ratio_is_valid(s.numerator, s.denominator))
        .copied()
        .collect();
    splits.sort_by_key(|s| s.effective_day);
    splits.dedup_by_key(|s| s.effective_day);

    // One answer per trade: is it in today's units?
    let verdict_of_trade: HashMap<usize, StockSplitVerdict> = group
        .trades
        .iter()
        .map(|(line, day, price, currency)| {
            let ratios: Vec<(u64, u64)> = splits
                .iter()
                .filter(|s| s.effective_day > *day)
                .map(|s| (s.numerator, s.denominator))
                .collect();
            let verdict = classify_cumulative(*price, currency, closes.get(day).copied(), &ratios);
            (*line, verdict)
        })
        .collect();

    splits
        .into_iter()
        .filter(|s| s.effective_day > first)
        .map(|split| {
            let trades: Vec<StockSplitCheckTrade> = group
                .trades
                .iter()
                .filter(|&&(_, day, _, _)| day < split.effective_day)
                .map(|(line, _, _, _)| StockSplitCheckTrade {
                    line: *line,
                    verdict: verdict_of_trade
                        .get(line)
                        .copied()
                        .unwrap_or(StockSplitVerdict::Unknown),
                })
                .collect();
            let verdicts: Vec<_> = trades.iter().map(|t| t.verdict).collect();
            StockSplitCheckSplit {
                effective_day: split.effective_day,
                numerator: split.numerator.to_string(),
                denominator: split.denominator.to_string(),
                verdict: verdict_of(&verdicts),
                trades,
                stored_status: group.stored_status.get(&split.effective_day).copied(),
                stored_pre_split_count: group
                    .stored_days
                    .partition_point(|&day| day < split.effective_day)
                    as i64,
            }
        })
        .collect()
}

/// UTC midnight of the day `ts` falls on.
fn floor_day(ts: i64) -> i64 {
    ts.div_euclid(DAY_SECONDS) * DAY_SECONDS
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::super::simulate::test_db::{add_stored, buy, config, day, db, over, parsed, trade};
    use super::super::types::{
        StockSplitAction, StockSplitCheckSplit, StockSplitCheckTrade, StockSplitOverride,
        TradeDirection,
    };
    use super::*;
    use crate::error::AppError;
    use crate::services::price_api::HistoricalPrice;
    use crate::services::stock_splits::DetectedSplit;

    /// A close quoted mid-afternoon of `day` (the check floors it to the UTC day).
    fn close(day: i64, price: f64, currency: &str) -> HistoricalPrice {
        HistoricalPrice {
            timestamp: day + 14 * 3_600 + 30 * 60,
            price,
            currency: currency.to_string(),
        }
    }

    fn split(effective_day: i64, numerator: u64, denominator: u64) -> DetectedSplit {
        DetectedSplit {
            effective_day,
            numerator,
            denominator,
        }
    }

    fn group(trades: Vec<(usize, i64, f64, &str)>) -> TickerGroup {
        TickerGroup {
            key: "symbol:NVDA".into(),
            ticker: "NVDA".into(),
            trades: trades
                .into_iter()
                .map(|(line, day, price, currency)| (line, day, price, currency.to_string()))
                .collect(),
            ..TickerGroup::default()
        }
    }

    /// NVDA closing at 96 USD on day 100 (split-adjusted), split 10:1 on day 200.
    fn nvda_series() -> HistoricalSeries {
        HistoricalSeries {
            prices: vec![close(day(100), 96.0, "USD"), close(day(250), 120.0, "USD")],
            splits: vec![split(day(200), 10, 1)],
        }
    }

    async fn run(
        series: Result<HistoricalSeries>,
        groups: Vec<TickerGroup>,
    ) -> Vec<StockSplitCheck> {
        let series = RefCell::new(Some(series));
        check_with(
            |_ticker, _from, _to| {
                let next = series.borrow_mut().take().expect("one fetch");
                async move { next }
            },
            groups,
            day(300),
        )
        .await
    }

    #[tokio::test]
    async fn classifies_trades_against_the_series() {
        let fetched = RefCell::new(Vec::new());
        let checks = check_with(
            |ticker, from, to| {
                fetched.borrow_mut().push((ticker, from, to));
                async { Ok(nvda_series()) }
            },
            vec![group(vec![
                (2, day(100), 950.0, "USD"),
                (3, day(250), 121.0, "USD"),
            ])],
            day(300),
        )
        .await;

        assert_eq!(
            fetched.into_inner(),
            vec![("NVDA".to_string(), day(100), day(300))]
        );
        assert_eq!(
            checks,
            vec![StockSplitCheck {
                key: "symbol:NVDA".into(),
                ticker: "NVDA".into(),
                checked: true,
                splits: vec![StockSplitCheckSplit {
                    effective_day: day(200),
                    numerator: "10".into(),
                    denominator: "1".into(),
                    verdict: StockSplitVerdict::Unadjusted,
                    trades: vec![StockSplitCheckTrade {
                        line: 2,
                        verdict: StockSplitVerdict::Unadjusted,
                    }],
                    stored_status: None,
                    stored_pre_split_count: 0,
                }],
            }]
        );
    }

    #[tokio::test]
    async fn trades_before_several_splits_use_the_cumulative_factor() {
        let series = HistoricalSeries {
            prices: vec![close(day(100), 95.0, "USD"), close(day(250), 95.0, "USD")],
            splits: vec![split(day(200), 4, 1), split(day(300), 10, 1)],
        };
        let checks = check_with(
            |_ticker, _from, _to| {
                let series = series.clone();
                async move { Ok(series) }
            },
            vec![group(vec![
                (2, day(100), 3800.0, "USD"),
                (3, day(250), 950.0, "USD"),
            ])],
            day(400),
        )
        .await;

        let unadjusted = |line| StockSplitCheckTrade {
            line,
            verdict: StockSplitVerdict::Unadjusted,
        };
        let splits = &checks[0].splits;
        assert_eq!(splits.len(), 2);
        assert_eq!(splits[0].effective_day, day(200));
        assert_eq!(splits[0].verdict, StockSplitVerdict::Unadjusted);
        assert_eq!(splits[0].trades, vec![unadjusted(2)]);
        assert_eq!(splits[1].effective_day, day(300));
        assert_eq!(splits[1].verdict, StockSplitVerdict::Unadjusted);
        assert_eq!(splits[1].trades, vec![unadjusted(2), unadjusted(3)]);
    }

    #[tokio::test]
    async fn adjusted_prices_and_missing_closes() {
        let checks = run(
            Ok(nvda_series()),
            vec![group(vec![
                (2, day(100), 97.0, "USD"),
                // No close that day.
                (3, day(101), 950.0, "USD"),
            ])],
        )
        .await;
        let split = &checks[0].splits[0];
        assert_eq!(split.verdict, StockSplitVerdict::Adjusted);
        assert_eq!(
            split.trades,
            vec![
                StockSplitCheckTrade {
                    line: 2,
                    verdict: StockSplitVerdict::Adjusted
                },
                StockSplitCheckTrade {
                    line: 3,
                    verdict: StockSplitVerdict::Unknown
                },
            ]
        );
    }

    #[tokio::test]
    async fn splits_before_the_first_trade_are_not_reported() {
        let series = HistoricalSeries {
            prices: vec![close(day(100), 96.0, "USD")],
            splits: vec![
                split(day(50), 4, 1),
                // A trade on the ex-date is post-split.
                split(day(100), 2, 1),
                split(day(200), 10, 1),
            ],
        };
        let checks = run(Ok(series), vec![group(vec![(2, day(100), 950.0, "USD")])]).await;

        assert!(checks[0].checked);
        let days: Vec<i64> = checks[0].splits.iter().map(|s| s.effective_day).collect();
        assert_eq!(days, vec![day(200)]);
    }

    #[tokio::test]
    async fn fetch_failure_marks_unchecked() {
        let failed = run(
            Err(AppError::ExternalApi("offline".into())),
            vec![group(vec![(2, day(100), 950.0, "USD")])],
        )
        .await;
        assert_eq!(
            failed,
            vec![StockSplitCheck {
                key: "symbol:NVDA".into(),
                ticker: "NVDA".into(),
                checked: false,
                splits: vec![],
            }]
        );

        // No closes at all: the symbol is unknown to Yahoo, not a stock without splits.
        let empty = run(
            Ok(HistoricalSeries::default()),
            vec![group(vec![(2, day(100), 950.0, "USD")])],
        )
        .await;
        assert!(!empty[0].checked);
        assert!(empty[0].splits.is_empty());
    }

    #[tokio::test]
    async fn currency_mismatch_is_unknown() {
        let checks = run(
            Ok(nvda_series()),
            vec![group(vec![(2, day(100), 950.0, "EUR")])],
        )
        .await;
        let split = &checks[0].splits[0];
        assert_eq!(split.verdict, StockSplitVerdict::Unknown);
        assert_eq!(split.trades[0].verdict, StockSplitVerdict::Unknown);
    }

    #[tokio::test]
    async fn stored_status_and_count_are_passed_through() {
        let mut nvda = group(vec![(2, day(100), 950.0, "USD")]);
        nvda.stored_status = HashMap::from([(day(200), StockSplitStatus::Dismissed)]);
        nvda.stored_days = vec![day(10), day(150), day(200), day(260)];
        let checks = run(Ok(nvda_series()), vec![nvda]).await;

        let split = &checks[0].splits[0];
        assert_eq!(split.stored_status, Some(StockSplitStatus::Dismissed));
        // Rows dated before the ex-date; the one on it is post-split.
        assert_eq!(split.stored_pre_split_count, 2);
    }

    #[tokio::test]
    async fn one_fetch_per_ticker_from_its_earliest_trade() {
        let mut other = group(vec![(5, day(90), 940.0, "USD")]);
        other.key = "isin:US67066G1040".into();
        let fetched = RefCell::new(Vec::new());
        let checks = check_with(
            |ticker, from, to| {
                fetched.borrow_mut().push((ticker, from, to));
                async { Ok(nvda_series()) }
            },
            vec![group(vec![(2, day(100), 950.0, "USD")]), other],
            day(300),
        )
        .await;

        assert_eq!(
            fetched.into_inner(),
            vec![("NVDA".to_string(), day(90), day(300))]
        );
        let keys: Vec<&str> = checks.iter().map(|c| c.key.as_str()).collect();
        assert_eq!(keys, vec!["symbol:NVDA", "isin:US67066G1040"]);
        assert!(checks.iter().all(|c| c.checked && c.splits.len() == 1));
    }

    #[test]
    fn groups_hold_the_will_import_trades_and_the_stored_state() {
        let conn = db();
        add_stored(&conn, "NVDA", "buy", "1", "900", day(80), None);
        add_stored(&conn, "NVDA", "buy", "1", "120", day(220), None);
        conn.execute(
            "INSERT INTO stock_splits (id, ticker, effective_day, numerator, denominator, source,
                                       status)
             VALUES ('s1', 'NVDA', ?1, '10', '1', 'yahoo', 'pending')",
            [day(200)],
        )
        .expect("split");
        let file = parsed(vec![
            buy(2, day(100), "NVDA", 1.0, 950.0),
            // Not imported: selling more than is held.
            trade(
                3,
                day(101),
                TradeDirection::Sell,
                "NVDA",
                50.0,
                950.0,
                "USD",
            ),
            buy(4, day(110), "AAPL", 1.0, 180.0),
        ]);

        // The wizard's earlier choice must not hide the split from the check.
        let mut chosen = config("custom");
        let mut nvda = over("symbol:NVDA");
        nvda.splits = vec![StockSplitOverride {
            effective_day: day(200),
            numerator: "10".into(),
            denominator: "1".into(),
            action: StockSplitAction::Convert,
        }];
        chosen.instrument_overrides = vec![nvda];

        let groups = groups(&conn, &file, &chosen).expect("groups");

        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].key, "symbol:NVDA");
        assert_eq!(groups[0].ticker, "NVDA");
        assert_eq!(
            groups[0].trades,
            vec![(2, day(100), 950.0, "USD".to_string())]
        );
        assert_eq!(
            groups[0].stored_status,
            HashMap::from([(day(200), StockSplitStatus::Pending)])
        );
        assert_eq!(groups[0].stored_days, vec![day(80), day(220)]);
        assert_eq!(groups[1].ticker, "AAPL");
        assert!(groups[1].stored_days.is_empty());
    }
}
