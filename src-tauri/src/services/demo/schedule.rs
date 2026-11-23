//! The shared event calendar of the demo life: when the big things happen, counted in calendar
//! months before today (`Mn`). Data plus the few date rules more than one part needs; the bank,
//! investment and asset parts all read it so that a purchase paid from savings shows up on the
//! same day in every part.

use super::clock::{self, DAY};
use super::prices;
use super::rng::Rng;
use chrono::{Datelike, NaiveDate};

/// Months of bank history (the simulation starts on the first day of `M48`).
pub const HISTORY_MONTHS: i32 = 48;

/// The flat is bought (and the mortgage paid out) at `M60` on this day: the first day of the
/// net-worth history.
pub const FLAT_BOUGHT_M: i32 = 60;
pub const FLAT_DAY: u32 = 15;

/// The one extra mortgage repayment (5 000, funded from savings the same day).
pub const EXTRA_MORTGAGE_M: i32 = 18;
/// Day of the month of the extra repayment.
pub const EXTRA_MORTGAGE_DAY: u32 = 20;
/// Amount of the extra repayment in EUR.
pub const EXTRA_MORTGAGE_EUR: f64 = 5_000.0;

/// The kitchen loan starts; the 213 monthly payment runs from `M19` on.
pub const KITCHEN_M: i32 = 20;
/// Day of the month the kitchen is bought and the loan paid out.
pub const KITCHEN_DAY: u32 = 12;
/// Monthly mortgage instalment in EUR.
pub const MORTGAGE_PAYMENT: f64 = 791.07;
/// Monthly kitchen loan instalment in EUR.
pub const KITCHEN_PAYMENT: f64 = 213.0;

/// The parking space is bought (21 500 from savings); rent of 95 runs from `M32` on.
pub const PARKING_BOUGHT_M: i32 = 33;
/// Day of the month of the parking space purchase.
pub const PARKING_DAY: u32 = 8;
/// Purchase price of the parking space in EUR.
pub const PARKING_PRICE_EUR: f64 = 21_500.0;
/// Monthly rent of the parking space in EUR.
pub const PARKING_RENT_EUR: f64 = 95.0;

/// Purchase price of the car in EUR.
pub const CAR_PRICE_EUR: f64 = 17_900.0;

/// Two one-ounce gold coins are bought (2 x 2 520 from savings).
pub const GOLD_BOUGHT_M: i32 = 25;
/// Day of the month of the gold purchase.
pub const GOLD_DAY: u32 = 15;
/// Price of one coin in EUR.
pub const GOLD_COIN_EUR: f64 = 2_520.0;

/// Months of the three Bitcoin purchases (day [`BTC_DAY`]) and the EUR sent each time.
pub const BTC_BUYS_M: [i32; 3] = [43, 30, 12];
/// EUR paid for each Bitcoin purchase, same order as [`BTC_BUYS_M`].
pub const BTC_BUYS_EUR: [f64; 3] = [1_300.0, 2_400.0, 2_600.0];
/// Day of the month of a Bitcoin purchase.
pub const BTC_DAY: u32 = 14;
/// Months of the two Ethereum purchases (day [`ETH_DAY`]).
pub const ETH_BUYS_M: [i32; 2] = [40, 16];
/// EUR paid for each Ethereum purchase, same order as [`ETH_BUYS_M`].
pub const ETH_BUYS_EUR: [f64; 2] = [2_100.0, 1_450.0];
/// Day of the month of an Ethereum purchase or sale.
pub const ETH_DAY: u32 = 20;
/// Month of the partial Ethereum sale (credited to the savings account, which pays for the
/// crypto purchases).
pub const ETH_SELL_M: i32 = 11;
/// EUR the Ethereum sale aims at (the credited amount is [`eth_sale_eur`]).
pub const ETH_SELL_EUR: f64 = 1_500.0;

/// The government bond is bought (6 x 1 000 from savings).
pub const BOND_BOUGHT_M: i32 = 27;
/// Day of the month of the bond purchase.
pub const BOND_DAY: u32 = 3;
/// Price paid for the bonds in EUR.
pub const BOND_PRICE_EUR: f64 = 6_000.0;

/// The monthly ETF plan starts here (its broker deposit goes out on the 3rd, the purchase on
/// the 28th).
pub const BROKER_FROM_M: i32 = 46;

/// Calendar month (1..=12) and day the short trip starts every year.
pub const SHORT_TRIP: (u32, u32) = (4, 10);
/// Calendar month and day the long trip starts every year.
pub const LONG_TRIP: (u32, u32) = (9, 6);
/// Length of the short trip in days.
pub const SHORT_TRIP_DAYS: i64 = 4;
/// Length of the long trip in days.
pub const LONG_TRIP_DAYS: i64 = 10;

/// Calendar months ahead the savings account's promotional rate ends (on the 15th): inside the
/// 90 days the milestones look ahead, so the deadline shows.
pub const SAVINGS_RATE_END_AHEAD_M: i32 = 2;

/// The next anniversary of the car policy: 30 to 40 days after today (picked from the day
/// itself, no random stream), so the insurance deadline shows. The yearly premium is paid on
/// the same month and day.
pub fn car_policy_anniversary(now: i64) -> i64 {
    let today = clock::today(now);
    today + (30 + (today / DAY).rem_euclid(11)) * DAY
}

/// Years between the car purchase and the next policy anniversary (the car is about `M35`).
const CAR_YEARS: i32 = 3;

/// `date`'s month and day in `year` (Feb 29 becomes Feb 28).
fn in_year(date: NaiveDate, year: i32) -> i64 {
    let dom = date.day().min(clock::days_in_month(year, date.month()));
    NaiveDate::from_ymd_opt(year, date.month(), dom).map_or(0, clock::day_of)
}

/// UTC midnight of `(year, month, day)`, the day clamped to the month.
pub fn ymd(year: i32, month: u32, day: u32) -> i64 {
    let dom = day.min(clock::days_in_month(year, month));
    NaiveDate::from_ymd_opt(year, month, dom).map_or(0, clock::day_of)
}

/// The first `(month, day)` on or after `day`.
pub fn first_on_or_after(day: i64, (month, dom): (u32, u32)) -> i64 {
    let year = clock::date_of(day).year();
    let this = ymd(year, month, dom);
    if this >= day {
        this
    } else {
        ymd(year + 1, month, dom)
    }
}

/// The car purchase: the policy anniversary's month and day three years before the next
/// anniversary. The car policy starts that day, so every anniversary falls on it.
pub fn car_bought_day(now: i64) -> i64 {
    let next = clock::date_of(car_policy_anniversary(now));
    in_year(next, next.year() - CAR_YEARS)
}

/// The car premium days, oldest first: the purchase day (the policy start) and every
/// anniversary since, up to the year before the next one.
pub fn car_premium_days(now: i64) -> Vec<i64> {
    let next = clock::date_of(car_policy_anniversary(now));
    (1..=CAR_YEARS)
        .rev()
        .map(|k| in_year(next, next.year() - k))
        .collect()
}

/// Start of the car policy: the car purchase.
pub fn car_policy_start(now: i64) -> i64 {
    car_bought_day(now)
}

/// Month and day the yearly home, liability and travel premiums are paid (policy anniversaries).
pub const HOME_PREMIUM: (u32, u32) = (2, 12);
pub const LIABILITY_PREMIUM: (u32, u32) = (3, 14);
pub const TRAVEL_PREMIUM: (u32, u32) = (6, 3);
/// The life policy starts here; its monthly premium is paid on this day of the month.
pub const LIFE_START_M: i32 = 52;
pub const LIFE_DAY: u32 = 21;

/// First day of the bank history.
pub fn history_start(now: i64) -> i64 {
    clock::months_ago(now, HISTORY_MONTHS, 1)
}

/// Home policy: from the first premium day after the flat purchase.
pub fn home_policy_start(now: i64) -> i64 {
    first_on_or_after(
        clock::months_ago(now, FLAT_BOUGHT_M, FLAT_DAY),
        HOME_PREMIUM,
    )
}

/// Liability policy: March four years ago, or the first premium in the history if that is
/// earlier.
pub fn liability_policy_start(now: i64) -> i64 {
    let year = clock::date_of(clock::today(now)).year();
    let (m, d) = LIABILITY_PREMIUM;
    ymd(year - 4, m, d).min(first_on_or_after(history_start(now), LIABILITY_PREMIUM))
}

/// Travel policy: from the first travel premium inside the history.
pub fn travel_policy_start(now: i64) -> i64 {
    first_on_or_after(history_start(now), TRAVEL_PREMIUM)
}

/// One crypto trade, priced at the day's close (USD, two decimals).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CryptoTrade {
    pub ticker: &'static str,
    pub name: &'static str,
    pub symbol: &'static str,
    pub day: i64,
    /// Units, five decimals.
    pub quantity: f64,
    /// USD per unit, two decimals.
    pub price: f64,
    pub sell: bool,
}

fn round_to(value: f64, decimals: i32) -> f64 {
    let f = 10f64.powi(decimals);
    (value * f).round() / f
}

/// Every crypto trade, oldest first per coin: the Bitcoin buys and the Ethereum buys for the
/// scheduled euro amounts, then the partial Ethereum sale (never more than half the holding).
pub fn crypto_trades(now: i64) -> Vec<CryptoTrade> {
    let trade = |(ticker, name, symbol): (&'static str, &'static str, &'static str),
                 day: i64,
                 eur: f64,
                 cap: f64,
                 sell: bool| {
        let price = round_to(prices::price_on(ticker, day).map_or(1.0, |(p, _)| p), 2);
        CryptoTrade {
            ticker,
            name,
            symbol,
            day,
            quantity: round_to(eur * prices::usd_per_eur(day) / price, 5).min(cap),
            price,
            sell,
        }
    };
    const BTC: (&str, &str, &str) = ("BTC", "Bitcoin", "BTC-USD");
    const ETH: (&str, &str, &str) = ("ETH", "Ethereum", "ETH-USD");
    let mut out = Vec::new();
    for (m, eur) in BTC_BUYS_M.iter().zip(BTC_BUYS_EUR) {
        out.push(trade(
            BTC,
            clock::months_ago(now, *m, BTC_DAY),
            eur,
            f64::MAX,
            false,
        ));
    }
    let mut held = 0.0;
    for (m, eur) in ETH_BUYS_M.iter().zip(ETH_BUYS_EUR) {
        let buy = trade(
            ETH,
            clock::months_ago(now, *m, ETH_DAY),
            eur,
            f64::MAX,
            false,
        );
        held += buy.quantity;
        out.push(buy);
    }
    let half = (held / 2.0 * 100_000.0).floor() / 100_000.0;
    out.push(trade(
        ETH,
        clock::months_ago(now, ETH_SELL_M, ETH_DAY),
        ETH_SELL_EUR,
        half,
        true,
    ));
    out
}

/// EUR the Ethereum sale credits: its proceeds at the day's rate, in cents.
pub fn eth_sale_eur(now: i64) -> f64 {
    crypto_trades(now).iter().find(|t| t.sell).map_or(0.0, |t| {
        round_to(t.quantity * t.price / prices::usd_per_eur(t.day), 2)
    })
}

// --- the stock trade calendar (invest.rs books it, bank.rs funds it) ----------------------

/// The ETF of the monthly plan.
pub const VWCE: (&str, &str) = ("VWCE.DE", "Vanguard FTSE All-World UCITS ETF");
/// Day of the month of the monthly ETF purchase (the trading day before on a weekend).
const VWCE_DAY: u32 = 28;
/// Seed of the monthly unit count (its own stream).
const ETF_SEED: u64 = 0x4554_4650_4C41;

/// Single-stock trades: (ticker, company, [(Mn, day of month, quantity, sell)]).
type Trades = &'static [(i32, u32, f64, bool)];
const STOCKS: [(&str, &str, Trades); 5] = [
    (
        "MSFT",
        "Microsoft Corporation",
        &[
            (40, 9, 12.0, false),
            (28, 9, 6.0, false),
            (10, 9, 4.0, false),
        ],
    ),
    (
        "AAPL",
        "Apple Inc.",
        &[(44, 6, 20.0, false), (18, 6, 10.0, false)],
    ),
    (
        "ASML.AS",
        "ASML Holding N.V.",
        &[(35, 16, 6.0, false), (15, 16, 3.0, false)],
    ),
    ("ALV.DE", "Allianz SE", &[(30, 21, 12.0, false)]),
    (
        "PYPL",
        "PayPal Holdings, Inc.",
        &[(45, 7, 30.0, false), (31, 7, 30.0, true)],
    ),
];
const NVDA: (&str, &str) = ("NVDA", "NVIDIA Corporation");
/// The NVIDIA 10:1 split.
const NVDA_SPLIT: (i32, u32, u32) = (2024, 6, 10);
const NVDA_SPLIT_RATIO: f64 = 10.0;
/// Shares bought (in today's units) and sold.
const NVDA_BOUGHT: f64 = 40.0;
const NVDA_SOLD: f64 = 15.0;
/// Day of the month of the NVIDIA trades.
const NVDA_DAY: u32 = 12;
/// NVIDIA buy before the split (`M43`), the fallback buy after it (`M24`) and the sale (`M8`).
const NVDA_BUY_M: i32 = 43;
const NVDA_LATE_BUY_M: i32 = 24;
const NVDA_SELL_M: i32 = 8;

/// One monthly ETF purchase.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EtfBuy {
    /// Calendar month before today (`Mn`) it belongs to.
    pub m: i32,
    pub day: i64,
    pub quantity: f64,
}

/// One single-stock trade.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StockTrade {
    pub ticker: &'static str,
    pub company: &'static str,
    pub day: i64,
    /// Shares as entered: today's units, or the units of the day before a split.
    pub quantity: f64,
    pub sell: bool,
    /// Price factor against today's units: the split ratio for a buy entered before the
    /// split (the split is recorded right after it), else 1.
    pub units_factor: f64,
}

/// The session a trade of `ticker` planned for `day` happens in: the last real trading day
/// on or before it (weekends and market holidays move back), so the app's own history has a
/// bar on the trade day.
pub fn trading_day(ticker: &str, day: i64) -> i64 {
    prices::trading_day(ticker, day)
}

/// UTC midnight of the NVIDIA split day.
pub fn nvda_split_day() -> i64 {
    let (y, m, d) = NVDA_SPLIT;
    NaiveDate::from_ymd_opt(y, m, d).map_or(0, clock::day_of)
}

/// Whether NVIDIA is bought before its split (true for every `now` until about 2028); later
/// the buy moves after the split (`M24`) and no split is recorded.
pub fn nvda_bought_before_split(now: i64) -> bool {
    trading_day(NVDA.0, clock::months_ago(now, NVDA_BUY_M, NVDA_DAY)) < nvda_split_day()
}

/// The monthly ETF plan, oldest first: `M46` to last month, 3-5 units on the 28th.
pub fn etf_buys(now: i64) -> Vec<EtfBuy> {
    let mut rng = Rng::with_seed(ETF_SEED);
    (1..=BROKER_FROM_M)
        .rev()
        .map(|m| EtfBuy {
            m,
            day: trading_day(VWCE.0, clock::months_ago(now, m, VWCE_DAY)),
            quantity: rng.int_range(3, 5) as f64,
        })
        .collect()
}

/// Every single-stock trade, in booking order (per ticker oldest first; NVIDIA last, its
/// pre-split buy before the sale).
pub fn stock_trades(now: i64) -> Vec<StockTrade> {
    let mut out = Vec::new();
    for (ticker, company, trades) in STOCKS {
        for (m, dom, quantity, sell) in trades {
            out.push(StockTrade {
                ticker,
                company,
                day: trading_day(ticker, clock::months_ago(now, *m, *dom)),
                quantity: *quantity,
                sell: *sell,
                units_factor: 1.0,
            });
        }
    }
    let (ticker, company) = NVDA;
    out.push(if nvda_bought_before_split(now) {
        StockTrade {
            ticker,
            company,
            day: trading_day(ticker, clock::months_ago(now, NVDA_BUY_M, NVDA_DAY)),
            quantity: NVDA_BOUGHT / NVDA_SPLIT_RATIO,
            sell: false,
            units_factor: NVDA_SPLIT_RATIO,
        }
    } else {
        StockTrade {
            ticker,
            company,
            day: trading_day(ticker, clock::months_ago(now, NVDA_LATE_BUY_M, NVDA_DAY)),
            quantity: NVDA_BOUGHT,
            sell: false,
            units_factor: 1.0,
        }
    });
    out.push(StockTrade {
        ticker,
        company,
        day: trading_day(ticker, clock::months_ago(now, NVDA_SELL_M, NVDA_DAY)),
        quantity: NVDA_SOLD,
        sell: true,
        units_factor: 1.0,
    });
    out
}
