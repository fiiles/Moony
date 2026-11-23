//! Refresh of the built-in demo prices (`prices_data.rs`) from the app's own providers.
//!
//! Run by a developer, with network: `cargo test --lib refresh_demo_prices -- --ignored`.
//! It is an ignored test, like `generate_bindings`, because the providers are async and a test
//! gets a runtime and the crate's private helpers for free; it never runs in CI.
//!
//! It reads exactly what the app's history rebuild reads:
//! - daily closes: `price_api::get_historical_stock_series_yahoo` (strict), the call
//!   `history_recalc` makes for stocks and, with the coin's Yahoo symbol, for crypto (USD only);
//!   a day is valued with `backfill::price_at_or_before`, the rule the rebuild passes;
//! - dividends: `price_api::get_dividend_history_yahoo` (the reading of
//!   `refresh_dividend_events`); splits come with the closes;
//! - rates: frankfurter.dev, the URL and `fx_backfill::parse_frankfurter_rates` of
//!   `fx_backfill::ensure_fx_coverage`.
//!
//! Any failed or implausible fetch fails the run before the file is written.

#![cfg(test)]

use std::collections::BTreeMap;
use std::fmt::Write as _;

use super::clock::{self, DAY};
use crate::services::backfill::price_at_or_before;
use crate::services::fx_backfill::parse_frankfurter_rates;
use crate::services::price_api::{
    get_dividend_history_yahoo, get_historical_stock_series_yahoo, HistoricalPrice,
};

/// (name in the demo, Yahoo symbol, expected currency).
const INSTRUMENTS: [(&str, &str, &str); 13] = [
    ("VWCE.DE", "VWCE.DE", "EUR"),
    ("MSFT", "MSFT", "USD"),
    ("AAPL", "AAPL", "USD"),
    ("ASML.AS", "ASML.AS", "EUR"),
    ("ALV.DE", "ALV.DE", "EUR"),
    ("NVDA", "NVDA", "USD"),
    ("PYPL", "PYPL", "USD"),
    ("GOOGL", "GOOGL", "USD"),
    ("AMZN", "AMZN", "USD"),
    ("SIE.DE", "SIE.DE", "EUR"),
    ("NOVO-B.CO", "NOVO-B.CO", "DKK"),
    ("BTC", "BTC-USD", "USD"),
    ("ETH", "ETH-USD", "USD"),
];
const PAYERS: [&str; 5] = ["MSFT", "AAPL", "NVDA", "ASML.AS", "ALV.DE"];
const RATE_CODES: [&str; 2] = ["USD", "DKK"];

/// One series as written: the day of each real bar (or quote) and its value.
struct Bars {
    name: String,
    currency: String,
    days: Vec<i64>,
    values: Vec<f64>,
}

fn from_2019() -> i64 {
    clock::day_of(chrono::NaiveDate::from_ymd_opt(2019, 1, 1).expect("date"))
}

/// One bar per UTC day that had one (the latest of a day, as `price_at_or_before` picks),
/// valid prices only, through `last_day`.
fn daily_bars(name: &str, currency: &str, prices: &[HistoricalPrice], last_day: i64) -> Bars {
    let mut days: Vec<i64> = prices
        .iter()
        .map(|p| clock::today(p.timestamp))
        .filter(|d| *d <= last_day)
        .collect();
    days.sort_unstable();
    days.dedup();
    let values = days
        .iter()
        .map(|d| price_at_or_before(prices, *d).map_or(f64::NAN, |p| p.price))
        .collect();
    Bars {
        name: name.to_string(),
        currency: currency.to_string(),
        days,
        values,
    }
}

/// Fails unless the series is long, positive, continuous enough and ends near `last_day`.
fn check_bars(bars: &Bars, last_day: i64) {
    let n = bars.values.len();
    assert!(n > 250, "{}: only {n} bars", bars.name);
    assert!(
        bars.values.iter().all(|v| v.is_finite() && *v > 0.0),
        "{}: invalid value",
        bars.name
    );
    let last = *bars.days.last().expect("bars");
    assert!(
        last_day - last <= 7 * DAY,
        "{}: ends {}",
        bars.name,
        clock::date_of(last)
    );
    for w in bars.days.windows(2) {
        let gap = (w[1] - w[0]) / DAY;
        assert!(
            (1..=10).contains(&gap),
            "{}: gap {gap} at {}",
            bars.name,
            clock::date_of(w[0])
        );
    }
}

/// Days of `bars` as the table encodes them: a calendar (every day, or weekdays) from the first
/// bar, minus the `missing` offsets, plus the `extra` ones. Verified against the real days with
/// the decoder `prices.rs` uses.
fn encode_days(bars: &Bars) -> (bool, Vec<u16>, Vec<u16>) {
    let first = bars.days[0];
    let last = *bars.days.last().expect("bars");
    let daily = bars.days.len() as i64 * 100 > (last - first) / DAY * 90;
    let actual: std::collections::BTreeSet<i64> = bars.days.iter().copied().collect();
    let offset = |d: i64| u16::try_from((d - first) / DAY).expect("offset fits u16");
    let mut missing = Vec::new();
    let mut day = first;
    while day <= last {
        if (daily || is_weekday(day)) && !actual.contains(&day) {
            missing.push(offset(day));
        }
        day += DAY;
    }
    let extra = bars
        .days
        .iter()
        .filter(|d| !daily && !is_weekday(**d))
        .map(|d| offset(*d))
        .collect();
    (daily, missing, extra)
}

fn is_weekday(day: i64) -> bool {
    (day.div_euclid(DAY) + 3).rem_euclid(7) < 5
}

/// Stock and crypto closes are single-precision at Yahoo: the shortest `f32` literal keeps
/// them (within an ulp of the parsed `f64`) in a third of the text. Rates are published with
/// four decimals and are kept as `f64` exactly.
fn render_series(out: &mut String, bars: &Bars, as_f32: bool) {
    let (daily, missing, extra) = encode_days(bars);
    let decoded =
        super::prices::bar_days_of(bars.days[0], daily, &missing, &extra, bars.days.len());
    assert_eq!(
        decoded, bars.days,
        "{}: day encoding does not round-trip",
        bars.name
    );
    let (kind, values): (&str, Vec<String>) = if as_f32 {
        (
            "F32",
            bars.values
                .iter()
                .map(|v| format!("{:?}", *v as f32))
                .collect(),
        )
    } else {
        (
            "F64",
            bars.values.iter().map(|v| format!("{v:?}")).collect(),
        )
    };
    let list = |v: &[u16]| v.iter().map(u16::to_string).collect::<Vec<_>>();
    let _ = writeln!(
        out,
        "    Series {{\n        name: {:?},\n        currency: {:?},\n        first_day: {}, // {}\n        daily: {daily},",
        bars.name,
        bars.currency,
        bars.days[0],
        clock::date_of(bars.days[0])
    );
    for (field, items) in [("missing", list(&missing)), ("extra", list(&extra))] {
        if items.is_empty() {
            let _ = writeln!(out, "        {field}: &[],");
        } else {
            let _ = writeln!(out, "        {field}: &[");
            wrap(out, &items, "            ");
            let _ = writeln!(out, "        ],");
        }
    }
    let _ = writeln!(out, "        values: Values::{kind}(&[");
    wrap(out, &values, "            ");
    let _ = writeln!(out, "        ]),\n    }},");
}

/// Comma-separated items on lines of at most 100 characters.
fn wrap(out: &mut String, items: &[String], indent: &str) {
    let mut line = String::from(indent);
    for item in items {
        if line.len() + item.len() + 2 > 100 && line.len() > indent.len() {
            out.push_str(line.trim_end());
            out.push('\n');
            line = String::from(indent);
        }
        line.push_str(item);
        line.push_str(", ");
    }
    if line.len() > indent.len() {
        out.push_str(line.trim_end());
        out.push('\n');
    }
}

fn render(
    through: i64,
    series: &[Bars],
    rates: &[Bars],
    dividends: &BTreeMap<String, Vec<(i64, f64)>>,
) -> String {
    let date = clock::date_of(through);
    let mut out = format!(
        "//! Built-in real market data for the demo profile: daily closes (split-adjusted, in the\n\
         //! quote currency; crypto in USD), per-share dividends and ECB rates (units per 1 EUR).\n\
         //!\n\
         //! GENERATED by `cargo test --lib refresh_demo_prices -- --ignored`\n\
         //! (`src/services/demo/refresh.rs`). Do not edit. Data through {date}.\n\
         //!\n\
         //! Each series lists the closes of its real bars; the bar days are a calendar from\n\
         //! `first_day` (every day when `daily`, else weekdays) without the `missing` and with the\n\
         //! `extra` day offsets. `mod.rs` keeps rustfmt off this file.\n\n\
         use super::prices::{{Series, Values}};\n\n\
         /// UTC midnight of the last day the data covers ({date}).\n\
         pub const DATA_THROUGH: i64 = {through};\n\n\
         /// Daily closes per instrument.\n\
         pub static SERIES: &[Series] = &[\n"
    );
    for bars in series {
        render_series(&mut out, bars, true);
    }
    out.push_str("];\n\n/// ECB reference rates, units per 1 EUR, on the days they were published.\npub static RATES: &[Series] = &[\n");
    for bars in rates {
        render_series(&mut out, bars, false);
    }
    out.push_str(
        "];\n\n/// Per-share dividends `(ex-day, amount)` in the stock's currency, today's units.\n\
         pub static DIVIDENDS: &[(&str, &[(i64, f64)])] = &[\n",
    );
    for (ticker, events) in dividends {
        let items: Vec<String> = events
            .iter()
            .map(|(d, a)| format!("({d}, {a:?})"))
            .collect();
        let _ = writeln!(out, "    ({ticker:?}, &[");
        wrap(&mut out, &items, "        ");
        out.push_str("    ]),\n");
    }
    out.push_str("];\n");
    out
}

#[tokio::test]
#[ignore] // Run manually, with network: cargo test --lib refresh_demo_prices -- --ignored
async fn refresh_demo_prices() {
    let today = clock::today(chrono::Utc::now().timestamp());
    let through = today - DAY;
    let start = from_2019();

    // Daily closes and splits, one strict fetch per instrument
    let mut series = Vec::new();
    for (name, symbol, currency) in INSTRUMENTS {
        let fetched = get_historical_stock_series_yahoo(&[symbol.to_string()], start, today, true)
            .await
            .unwrap_or_else(|e| panic!("{symbol}: {e}"));
        let history = fetched
            .get(symbol)
            .unwrap_or_else(|| panic!("{symbol}: no history"));
        let quotes: Vec<HistoricalPrice> = history
            .prices
            .iter()
            .filter(|p| p.currency.eq_ignore_ascii_case(currency))
            .cloned()
            .collect();
        assert_eq!(
            quotes.len(),
            history.prices.len(),
            "{symbol}: quotes not all in {currency}"
        );
        if name == "NVDA" {
            let split = clock::day_of(chrono::NaiveDate::from_ymd_opt(2024, 6, 10).expect("d"));
            assert!(
                history
                    .splits
                    .iter()
                    .any(|s| s.effective_day == split && s.numerator == 10 && s.denominator == 1),
                "NVDA: the 2024-06-10 10:1 split is missing: {:?}",
                history.splits
            );
        }
        let bars = daily_bars(name, currency, &quotes, through);
        check_bars(&bars, through);
        let worst = bars
            .values
            .iter()
            .map(|v| (f64::from(*v as f32) / v - 1.0).abs())
            .fold(0.0, f64::max);
        eprintln!(
            "{name}: {} bars {} .. {}, largest f32 rounding {worst:.1e}",
            bars.days.len(),
            clock::date_of(bars.days[0]),
            clock::date_of(*bars.days.last().expect("bars")),
        );
        series.push(bars);
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }

    // Dividends of the payers
    let mut dividends = BTreeMap::new();
    for ticker in PAYERS {
        let (events, _) = get_dividend_history_yahoo(ticker, start, today)
            .await
            .unwrap_or_else(|e| panic!("{ticker} dividends: {e}"));
        let currency = INSTRUMENTS
            .iter()
            .find(|i| i.0 == ticker)
            .map(|i| i.2)
            .unwrap_or_default();
        let mut rows: Vec<(i64, f64)> = events
            .iter()
            .filter(|e| e.ex_date <= through)
            .inspect(|e| assert_eq!(e.currency, currency, "{ticker} dividend currency"))
            .map(|e| (e.ex_date, e.amount))
            .collect();
        rows.sort_by_key(|r| r.0);
        rows.dedup_by_key(|r| r.0);
        assert!(rows.len() >= 4, "{ticker}: only {} dividends", rows.len());
        eprintln!("{ticker}: {} dividends", rows.len());
        dividends.insert(ticker.to_string(), rows);
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    }

    // ECB rates through frankfurter.dev, as the app's FX backfill fetches them
    let url = format!(
        "https://api.frankfurter.dev/v1/{}..{}?base=EUR&symbols={}",
        clock::date_of(start).format("%Y-%m-%d"),
        clock::date_of(through).format("%Y-%m-%d"),
        RATE_CODES.join(",")
    );
    let body: serde_json::Value = crate::services::http::client()
        .get(&url)
        .send()
        .await
        .expect("frankfurter request")
        .json()
        .await
        .expect("frankfurter body");
    let rows = parse_frankfurter_rates(&body);
    let mut rates = Vec::new();
    for code in RATE_CODES {
        let mut days_values: Vec<(i64, f64)> = rows
            .iter()
            .filter(|(_, c, _)| c == code)
            .map(|(d, _, r)| (*d, *r))
            .collect();
        days_values.sort_by_key(|r| r.0);
        let bars = Bars {
            name: code.to_string(),
            currency: code.to_string(),
            days: days_values.iter().map(|r| r.0).collect(),
            values: days_values.iter().map(|r| r.1).collect(),
        };
        check_bars(&bars, through);
        eprintln!("{code}: {} quotes", bars.days.len());
        rates.push(bars);
    }

    let source = render(through, &series, &rates, &dividends);
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/services/demo/prices_data.rs");
    std::fs::write(&path, source).expect("write prices_data.rs");
    eprintln!(
        "wrote {} (data through {})",
        path.display(),
        clock::date_of(through)
    );
}
