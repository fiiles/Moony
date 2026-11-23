//! Alex's investments: seven stock positions (a monthly ETF plan, five single stocks, one
//! closed position), the NVIDIA split, dividends, tags, the watchlist and cached quotes; two
//! crypto holdings; one euro government bond.
//!
//! Trades are priced from [`prices::price_on`] with a small deterministic wiggle, so the
//! history the app derives later (`history.rs`) matches the trade prices.

use super::clock::{self, DAY};
use super::prices;
use super::rng::Rng;
use super::schedule::*;
use super::text::{t, T};
use super::{DemoOptions, DemoSummary};
use crate::error::{AppError, Result};
use crate::models::{
    InsertBond, InsertCryptoTransaction, InsertInvestmentTransaction, InsertStockTag,
    InsertStockTagGroup, InsertWatchedStock,
};
use crate::services::stock_dividends::{self, DividendEvent};
use crate::services::stock_splits::InsertStockSplit;
use crate::services::{
    bonds, crypto_investments, investments, stock_monitor, stock_splits, stock_tags,
};
use rusqlite::{params, Connection};
use std::collections::HashMap;

/// Seed of the trade-price wiggle (its own stream, independent of the bank's).
const WIGGLE_SEED: u64 = 0x5354_4F43_4B53;
/// Largest relative deviation of a trade price from the day's close.
const WIGGLE: f64 = 0.006;
/// Fee of every stock trade, in the trade currency.
const FEE: &str = "1";

/// The tickers that pay dividends (Yahoo-style per-share events).
const PAYERS: [&str; 5] = ["MSFT", "AAPL", "NVDA", "ASML.AS", "ALV.DE"];

/// Quote metadata in Yahoo's wording: (ticker, short name, long name, sector, industry,
/// exchange, quote type). ETFs have no sector.
type Meta = (
    &'static str,
    &'static str,
    &'static str,
    Option<&'static str>,
    Option<&'static str>,
    &'static str,
    &'static str,
);
const QUOTES: [Meta; 11] = [
    (
        "VWCE.DE",
        "VANGUARD FTSE ALL-WORLD U.ETF",
        "Vanguard FTSE All-World UCITS ETF USD Accumulation",
        None,
        None,
        "GER",
        "ETF",
    ),
    (
        "MSFT",
        "Microsoft Corporation",
        "Microsoft Corporation",
        Some("Technology"),
        Some("Software - Infrastructure"),
        "NMS",
        "EQUITY",
    ),
    (
        "AAPL",
        "Apple Inc.",
        "Apple Inc.",
        Some("Technology"),
        Some("Consumer Electronics"),
        "NMS",
        "EQUITY",
    ),
    (
        "ASML.AS",
        "ASML HOLDING",
        "ASML Holding N.V.",
        Some("Technology"),
        Some("Semiconductor Equipment & Materials"),
        "AMS",
        "EQUITY",
    ),
    (
        "ALV.DE",
        "ALLIANZ SE NA O.N.",
        "Allianz SE",
        Some("Financial Services"),
        Some("Insurance - Diversified"),
        "GER",
        "EQUITY",
    ),
    (
        "NVDA",
        "NVIDIA Corporation",
        "NVIDIA Corporation",
        Some("Technology"),
        Some("Semiconductors"),
        "NMS",
        "EQUITY",
    ),
    (
        "PYPL",
        "PayPal Holdings, Inc.",
        "PayPal Holdings, Inc.",
        Some("Financial Services"),
        Some("Credit Services"),
        "NMS",
        "EQUITY",
    ),
    (
        "GOOGL",
        "Alphabet Inc.",
        "Alphabet Inc.",
        Some("Communication Services"),
        Some("Internet Content & Information"),
        "NMS",
        "EQUITY",
    ),
    (
        "AMZN",
        "Amazon.com, Inc.",
        "Amazon.com, Inc.",
        Some("Consumer Cyclical"),
        Some("Internet Retail"),
        "NMS",
        "EQUITY",
    ),
    (
        "SIE.DE",
        "SIEMENS AG NA O.N.",
        "Siemens Aktiengesellschaft",
        Some("Industrials"),
        Some("Specialty Industrial Machinery"),
        "GER",
        "EQUITY",
    ),
    (
        "NOVO-B.CO",
        "Novo Nordisk A/S",
        "Novo Nordisk A/S",
        Some("Healthcare"),
        Some("Drug Manufacturers - General"),
        "CPH",
        "EQUITY",
    ),
];

/// Watchlist: (ticker, target relative to today's price, direction, note).
/// A watched ticker, its optional target (factor of today's price, direction) and note.
type Watch = (&'static str, Option<(f64, &'static str)>, Option<T>);
const WATCHLIST: [Watch; 4] = [
    ("GOOGL", Some((0.92, "below")), Some(T::WatchNote1)),
    ("SIE.DE", Some((1.10, "above")), Some(T::WatchNote2)),
    ("AMZN", None, None),
    ("NOVO-B.CO", None, None),
];

fn price(ticker: &str, day: i64) -> Result<(f64, &'static str)> {
    prices::price_on(ticker, day)
        .ok_or_else(|| AppError::Internal(format!("demo: no built-in price for {ticker}")))
}

fn round(value: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (value * f).round() / f
}

/// Decimal text without trailing zeros ("4", "0.25").
fn num(value: f64) -> String {
    let text = format!("{value:.6}");
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Creates positions on their first trade and adds the later ones.
struct Book<'a> {
    conn: &'a Connection,
    ids: HashMap<&'static str, String>,
    names: HashMap<&'static str, &'static str>,
    first_day: HashMap<&'static str, i64>,
    rng: Rng,
    trades: usize,
}

impl Book<'_> {
    /// One trade at `units_factor` × the day's close (in today's units) with the wiggle.
    fn trade(
        &mut self,
        ticker: &'static str,
        company: &'static str,
        day: i64,
        quantity: f64,
        sell: bool,
        units_factor: f64,
    ) -> Result<()> {
        let (close, currency) = price(ticker, day)?;
        let wiggle = self.rng.range(-WIGGLE, WIGGLE);
        let data = InsertInvestmentTransaction {
            investment_id: None,
            tx_type: if sell { "sell" } else { "buy" }.to_string(),
            ticker: ticker.to_string(),
            company_name: company.to_string(),
            quantity: num(quantity),
            price_per_unit: format!("{:.2}", round(close * units_factor * (1.0 + wiggle), 2)),
            currency: currency.to_string(),
            transaction_date: day,
            fee: Some(FEE.to_string()),
        };
        match self.ids.get(ticker) {
            Some(id) => {
                investments::add_transaction_to_investment(self.conn, id, &data)?;
            }
            None => {
                let created = investments::create_investment_with_transaction(
                    self.conn,
                    ticker,
                    company,
                    Some(&data),
                )?;
                self.ids.insert(ticker, created.id);
                self.names.insert(ticker, company);
            }
        }
        let first = self.first_day.entry(ticker).or_insert(day);
        *first = (*first).min(day);
        self.trades += 1;
        Ok(())
    }
}

/// Stocks, crypto, the bond, dividends, tags, the watchlist and the cached quotes.
pub fn seed(conn: &Connection, opts: &DemoOptions, summary: &mut DemoSummary) -> Result<()> {
    let now = opts.now;
    let lang = opts.language.as_str();
    let mut book = Book {
        conn,
        ids: HashMap::new(),
        names: HashMap::new(),
        first_day: HashMap::new(),
        rng: Rng::with_seed(WIGGLE_SEED),
        trades: 0,
    };

    // The trade calendar the bank funds: the monthly ETF plan, then the single stocks
    for buy in etf_buys(now) {
        book.trade(VWCE.0, VWCE.1, buy.day, buy.quantity, false, 1.0)?;
    }
    for trade in stock_trades(now) {
        book.trade(
            trade.ticker,
            trade.company,
            trade.day,
            trade.quantity,
            trade.sell,
            trade.units_factor,
        )?;
        // A buy entered in pre-split units is followed by the split, which rewrites it into
        // today's units (a later Yahoo detection of the same day keeps this row:
        // `record_detected` inserts with `ON CONFLICT DO NOTHING`).
        if trade.units_factor > 1.0 {
            stock_splits::record_manual(
                book.conn,
                &InsertStockSplit {
                    ticker: trade.ticker.to_string(),
                    effective_day: nvda_split_day(),
                    numerator: format!("{}", trade.units_factor),
                    denominator: "1".to_string(),
                },
            )?;
        }
    }

    // Dividends: per-share events from the first buy on, and the yearly sum
    let today = clock::today(now);
    let mut events_total = 0;
    for ticker in PAYERS {
        let Some(first) = book.first_day.get(ticker).copied() else {
            continue;
        };
        let (_, currency) = price(ticker, today)?;
        let events: Vec<DividendEvent> = prices::dividends(ticker)
            .into_iter()
            .filter(|(ex, _)| *ex >= first && *ex <= today)
            .map(|(ex_date, amount)| DividendEvent {
                ex_date,
                amount,
                currency: currency.to_string(),
            })
            .collect();
        let yearly: f64 = events
            .iter()
            .filter(|e| e.ex_date > today - 365 * DAY)
            .map(|e| e.amount)
            .sum();
        events_total += events.len();
        stock_dividends::record_events(conn, ticker, &events, first, now)?;
        conn.execute(
            "INSERT INTO dividend_data (id, ticker, yearly_dividend_sum, currency, last_fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                uuid::Uuid::new_v4().to_string(),
                ticker,
                num(round(yearly, 4)),
                currency,
                now
            ],
        )?;
    }

    seed_quotes(conn, now)?;
    seed_tags(conn, lang, &book.ids)?;
    seed_watchlist(conn, lang, now)?;
    let crypto_trades = seed_crypto(conn, now)?;
    seed_bond(conn, lang, now)?;

    summary.counts.insert("stock_investments", book.ids.len());
    summary
        .counts
        .insert("investment_transactions", book.trades);
    summary.counts.insert("stock_dividend_events", events_total);
    summary.counts.insert("stock_data", QUOTES.len());
    summary.counts.insert("watched_stocks", WATCHLIST.len());
    summary.counts.insert("crypto_investments", 2);
    summary.counts.insert("crypto_transactions", crypto_trades);
    summary.counts.insert("bonds", 1);
    Ok(())
}

/// Cached quotes and metadata for every position and watched ticker, fetched "now", with
/// yesterday's close and the 52-week range.
fn seed_quotes(conn: &Connection, now: i64) -> Result<()> {
    let today = clock::today(now);
    let tx = conn.unchecked_transaction()?;
    for (ticker, short, long, sector, industry, exchange, quote_type) in QUOTES {
        let (close, currency) = price(ticker, today)?;
        let (previous, _) = price(ticker, trading_day(today - DAY))?;
        let mut high = f64::MIN;
        let mut low = f64::MAX;
        for n in 0..365 {
            let (p, _) = price(ticker, today - n * DAY)?;
            high = high.max(p);
            low = low.min(p);
        }
        tx.execute(
            "INSERT INTO stock_data (id, ticker, original_price, currency, price_date, fetched_at,
                 short_name, long_name, sector, industry, fifty_two_week_high, fifty_two_week_low,
                 exchange, quote_type, metadata_fetched_at, previous_close, quote_currency,
                 splits_checked_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?5, ?14, ?4, ?5)",
            params![
                uuid::Uuid::new_v4().to_string(),
                ticker,
                format!("{close:.2}"),
                currency,
                now,
                short,
                long,
                sector,
                industry,
                format!("{high:.2}"),
                format!("{low:.2}"),
                exchange,
                quote_type,
                format!("{previous:.2}"),
            ],
        )?;
    }
    tx.commit()?;
    Ok(())
}

/// The "Strategy" group: Core (the ETF), Growth (tech) and Dividend (Allianz).
fn seed_tags(conn: &Connection, lang: &str, ids: &HashMap<&'static str, String>) -> Result<()> {
    let group = stock_tags::create_group(
        conn,
        InsertStockTagGroup {
            name: t(lang, T::TagGroupStrategy).to_string(),
            description: None,
        },
    )?;
    let members: [(T, &[&str]); 3] = [
        (T::TagCore, &["VWCE.DE"]),
        (T::TagGrowth, &["MSFT", "AAPL", "NVDA", "ASML.AS"]),
        (T::TagDividend, &["ALV.DE"]),
    ];
    for (key, tickers) in members {
        let tag = stock_tags::create_tag(
            conn,
            InsertStockTag {
                name: t(lang, key).to_string(),
                color: None,
                group_id: Some(group.id.clone()),
            },
        )?;
        for ticker in tickers {
            if let Some(id) = ids.get(ticker) {
                stock_tags::set_investment_tags(conn, id, std::slice::from_ref(&tag.id))?;
            }
        }
    }
    Ok(())
}

/// Four watched stocks, two with a target that today's price has not reached.
fn seed_watchlist(conn: &Connection, lang: &str, now: i64) -> Result<()> {
    let today = clock::today(now);
    for (ticker, target, note) in WATCHLIST {
        stock_monitor::follow_stock(
            conn,
            &InsertWatchedStock {
                ticker: ticker.to_string(),
            },
        )?;
        if let Some((factor, direction)) = target {
            let (close, _) = price(ticker, today)?;
            stock_monitor::set_target_price(
                conn,
                ticker,
                Some(format!("{:.2}", round(close * factor, 0))),
                Some(direction.to_string()),
            )?;
        }
        if let Some(note) = note {
            stock_monitor::update_notes(conn, ticker, t(lang, note).to_string())?;
        }
    }
    Ok(())
}

/// Bitcoin and Ethereum bought (and partly sold) for the scheduled euro amounts, in USD, with
/// today's cached prices. Returns the number of trades.
fn seed_crypto(conn: &Connection, now: i64) -> Result<usize> {
    let today = clock::today(now);
    let mut trades = 0;
    let mut add = |ticker: &str,
                   name: &str,
                   symbol: &str,
                   day: i64,
                   quantity: f64,
                   sell: bool,
                   id: &mut Option<String>|
     -> Result<()> {
        let (close, currency) = price(ticker, day)?;
        let data = InsertCryptoTransaction {
            investment_id: None,
            tx_type: if sell { "sell" } else { "buy" }.to_string(),
            ticker: ticker.to_string(),
            name: name.to_string(),
            quantity: format!("{quantity:.5}"),
            price_per_unit: format!("{close:.2}"),
            currency: currency.to_string(),
            transaction_date: day,
        };
        match id {
            Some(id) => {
                crypto_investments::add_transaction_to_crypto(conn, id, &data)?;
            }
            None => {
                let created = crypto_investments::create_crypto_with_transaction(
                    conn,
                    ticker,
                    Some(symbol),
                    name,
                    Some(&data),
                )?;
                *id = Some(created.id);
            }
        }
        trades += 1;
        Ok(())
    };
    let units = |ticker: &str, eur: f64, day: i64| -> Result<f64> {
        let (close, _) = price(ticker, day)?;
        Ok(round(eur * prices::usd_per_eur(day) / close, 5))
    };

    let mut btc = None;
    for (m, eur) in BTC_BUYS_M.iter().zip(BTC_BUYS_EUR) {
        let day = clock::months_ago(now, *m, BTC_DAY);
        add(
            "BTC",
            "Bitcoin",
            "BTC-USD",
            day,
            units("BTC", eur, day)?,
            false,
            &mut btc,
        )?;
    }
    let mut eth = None;
    let mut held = 0.0;
    for (m, eur) in ETH_BUYS_M.iter().zip(ETH_BUYS_EUR) {
        let day = clock::months_ago(now, *m, ETH_DAY);
        let quantity = units("ETH", eur, day)?;
        held += quantity;
        add("ETH", "Ethereum", "ETH-USD", day, quantity, false, &mut eth)?;
    }
    let day = clock::months_ago(now, ETH_SELL_M, ETH_DAY);
    let half = (held / 2.0 * 100_000.0).floor() / 100_000.0;
    let quantity = units("ETH", ETH_SELL_EUR, day)?.min(half);
    add("ETH", "Ethereum", "ETH-USD", day, quantity, true, &mut eth)?;

    for ticker in ["BTC", "ETH"] {
        let (close, currency) = price(ticker, today)?;
        conn.execute(
            "INSERT INTO crypto_prices (id, symbol, price, currency, fetched_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                uuid::Uuid::new_v4().to_string(),
                ticker,
                format!("{close:.2}"),
                currency,
                now
            ],
        )?;
    }
    Ok(trades)
}

/// Months before today the bond was bought, and months ahead it matures.
const BOND_MATURES_IN_M: i32 = 54;

/// Six 1 000 EUR government bonds at 2.9 %, bought at `M27`, maturing in 54 months.
fn seed_bond(conn: &Connection, lang: &str, now: i64) -> Result<()> {
    let today = clock::date_of(clock::today(now));
    bonds::create_bond(
        conn,
        &InsertBond {
            name: t(lang, T::BondName).to_string(),
            isin: None,
            coupon_value: "1000".to_string(),
            quantity: Some("6".to_string()),
            currency: Some("EUR".to_string()),
            interest_rate: Some("2.9".to_string()),
            maturity_date: Some(clock::months_ago(
                now,
                -BOND_MATURES_IN_M,
                chrono::Datelike::day(&today),
            )),
            purchase_date: Some(clock::months_ago(now, BOND_BOUGHT_M, BOND_DAY)),
        },
    )?;
    Ok(())
}
