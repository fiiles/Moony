//! The shared event calendar of the demo life: when the big things happen, counted in calendar
//! months before today (`Mn`). Data plus the few date rules more than one part needs; the bank,
//! investment and asset parts all read it so that a purchase paid from savings shows up on the
//! same day in every part.

use super::clock::{self, DAY};
use chrono::{Datelike, NaiveDate};

/// Months of bank history (the simulation starts on the first day of `M48`).
pub const HISTORY_MONTHS: i32 = 48;

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
/// Month of the partial Ethereum sale (credited to the current account).
pub const ETH_SELL_M: i32 = 11;
/// EUR credited by the Ethereum sale.
pub const ETH_SELL_EUR: f64 = 1_500.0;

/// The broker deposit of 450 on the 27th starts here.
pub const BROKER_FROM_M: i32 = 46;
/// Monthly broker deposit in EUR.
pub const BROKER_DEPOSIT_EUR: f64 = 450.0;

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
