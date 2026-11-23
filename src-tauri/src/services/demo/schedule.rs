//! The shared event calendar of the demo life: when the big things happen, counted in calendar
//! months before today (`Mn`). Data plus the few date rules more than one part needs; the bank,
//! investment and asset parts all read it so that a purchase paid from savings shows up on the
//! same day in every part.

use super::clock::{self, DAY};
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

/// The car is bought (17 900 from savings).
pub const CAR_BOUGHT_M: i32 = 40;
/// Day of the month of the car purchase.
pub const CAR_DAY: u32 = 12;
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
/// EUR credited by the Ethereum sale.
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

/// The car premium days, oldest first: the anniversary's month and day in every year from the
/// first one on or after the car purchase up to the year before the next anniversary. The
/// first one is the start of the car policy.
pub fn car_premium_days(now: i64) -> Vec<i64> {
    let next = clock::date_of(car_policy_anniversary(now));
    let bought = clock::months_ago(now, CAR_BOUGHT_M, CAR_DAY);
    let mut days: Vec<i64> = (1..=CAR_BOUGHT_M / 12 + 2)
        .filter_map(|k| {
            let year = next.year() - k;
            let dom = next.day().min(clock::days_in_month(year, next.month()));
            NaiveDate::from_ymd_opt(year, next.month(), dom).map(clock::day_of)
        })
        .filter(|day| *day >= bought)
        .collect();
    days.sort_unstable();
    days
}

/// Start of the car policy: the first premium day (the car purchase if there is none).
pub fn car_policy_start(now: i64) -> i64 {
    car_premium_days(now)
        .first()
        .copied()
        .unwrap_or_else(|| clock::months_ago(now, CAR_BOUGHT_M, CAR_DAY))
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

/// Friday for Saturday and Sunday (stock exchanges trade on weekdays).
pub fn trading_day(day: i64) -> i64 {
    let dow = (day.div_euclid(DAY) + 3).rem_euclid(7); // Monday = 0
    if dow >= 5 {
        day - (dow - 4) * DAY
    } else {
        day
    }
}

/// UTC midnight of the NVIDIA split day.
pub fn nvda_split_day() -> i64 {
    let (y, m, d) = NVDA_SPLIT;
    NaiveDate::from_ymd_opt(y, m, d).map_or(0, clock::day_of)
}

/// Whether NVIDIA is bought before its split (true for every `now` until about 2028); later
/// the buy moves after the split (`M24`) and no split is recorded.
pub fn nvda_bought_before_split(now: i64) -> bool {
    trading_day(clock::months_ago(now, NVDA_BUY_M, NVDA_DAY)) < nvda_split_day()
}

/// The monthly ETF plan, oldest first: `M46` to last month, 3-5 units on the 28th.
pub fn etf_buys(now: i64) -> Vec<EtfBuy> {
    let mut rng = Rng::with_seed(ETF_SEED);
    (1..=BROKER_FROM_M)
        .rev()
        .map(|m| EtfBuy {
            m,
            day: trading_day(clock::months_ago(now, m, VWCE_DAY)),
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
                day: trading_day(clock::months_ago(now, *m, *dom)),
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
            day: trading_day(clock::months_ago(now, NVDA_BUY_M, NVDA_DAY)),
            quantity: NVDA_BOUGHT / NVDA_SPLIT_RATIO,
            sell: false,
            units_factor: NVDA_SPLIT_RATIO,
        }
    } else {
        StockTrade {
            ticker,
            company,
            day: trading_day(clock::months_ago(now, NVDA_LATE_BUY_M, NVDA_DAY)),
            quantity: NVDA_BOUGHT,
            sell: false,
            units_factor: 1.0,
        }
    });
    out.push(StockTrade {
        ticker,
        company,
        day: trading_day(clock::months_ago(now, NVDA_SELL_M, NVDA_DAY)),
        quantity: NVDA_SOLD,
        sell: true,
        units_factor: 1.0,
    });
    out
}
