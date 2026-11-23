//! Alex's bank history: three accounts, about 2 200 categorized payments over 48 months and a
//! month-end balance record for every account, all consistent with the payments.
//!
//! The history is simulated forward day by day from the opening balances (see
//! [`OPEN_CURRENT_EUR`] and [`OPEN_SAVINGS_EUR`]). Scheduled payments are generated first with
//! the one deterministic [`Rng`] in a fixed call order, then replayed in date order against the
//! balances, which adds the dynamic parts: savings interest, the month-end sweep of surplus
//! cash to the savings account and the rescue transfer when the current account would fall
//! below [`MIN_CURRENT_EUR`].

use super::clock::{self, DAY};
use super::prices;
use super::rng::Rng;
use super::schedule::*;
use super::text::{t, T};
use super::{DemoOptions, DemoSummary};
use crate::error::Result;
use crate::models::bank_accounts::{InsertBalanceRecord, InsertBankAccount};
use crate::models::bank_transactions::{FlowKind, InsertBankTransaction};
use crate::services::bank_accounts;
use chrono::Datelike;

/// Opening balance of the current account on the first simulated day.
pub const OPEN_CURRENT_EUR: f64 = 2_900.0;
/// Opening balance of the savings account on the first simulated day.
pub const OPEN_SAVINGS_EUR: f64 = 36_000.0;
/// Yearly interest of the savings account in percent.
pub const SAVINGS_RATE: f64 = 2.6;
/// The savings account never drops below this on any day.
pub const MIN_SAVINGS_EUR: f64 = 500.0;
/// The current account is topped up from savings when a payment would leave less than this.
pub const MIN_CURRENT_EUR: f64 = 300.0;
/// A month end above this balance sweeps the excess above [`SWEEP_KEEP_EUR`] to savings.
pub const SWEEP_ABOVE_EUR: f64 = 7_000.0;
/// What the current account keeps after a sweep.
pub const SWEEP_KEEP_EUR: f64 = 5_000.0;

const CUR: usize = 0;
const SAV: usize = 1;
const TRV: usize = 2;

/// Euros to cents.
fn c(eur: f64) -> i64 {
    (eur * 100.0).round() as i64
}

/// Cents as a decimal text ("-12.30").
fn money(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let a = cents.abs();
    format!("{sign}{}.{:02}", a / 100, a % 100)
}

/// One payment of the simulated history.
#[derive(Clone)]
struct Pay {
    day: i64,
    acct: usize,
    credit: bool,
    cents: i64,
    desc: T,
    cp: String,
    cat: &'static str,
    transfer: bool,
}

/// Builds the scheduled payments.
struct Gen {
    rng: Rng,
    out: Vec<Pay>,
    start: i64,
    today: i64,
    names: [String; 3],
}

impl Gen {
    fn amt(&mut self, lo: f64, hi: f64) -> i64 {
        c(self.rng.range(lo, hi))
    }

    fn push(&mut self, p: Pay) {
        if p.day >= self.start && p.day <= self.today {
            self.out.push(p);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pay(
        &mut self,
        day: i64,
        acct: usize,
        credit: bool,
        cents: i64,
        desc: T,
        cp: &str,
        cat: &'static str,
    ) {
        self.push(Pay {
            day,
            acct,
            credit,
            cents,
            desc,
            cp: cp.to_string(),
            cat,
            transfer: false,
        });
    }

    /// A debit on the current account.
    fn debit(&mut self, day: i64, cents: i64, desc: T, cp: &str, cat: &'static str) {
        self.pay(day, CUR, false, cents, desc, cp, cat);
    }

    /// A credit on the current account.
    fn credit(&mut self, day: i64, cents: i64, desc: T, cp: &str, cat: &'static str) {
        self.pay(day, CUR, true, cents, desc, cp, cat);
    }

    /// Both sides of a move between two own accounts (`cents` is in the source currency and
    /// `cents_in` in the target currency).
    #[allow(clippy::too_many_arguments)]
    fn transfer(
        &mut self,
        day: i64,
        from: usize,
        to: usize,
        cents: i64,
        cents_in: i64,
        desc_out: T,
        desc_in: T,
        cat: &'static str,
    ) {
        let (a, b) = (self.names[from].clone(), self.names[to].clone());
        for (acct, credit, amount, desc, cp) in [
            (from, false, cents, desc_out, b),
            (to, true, cents_in, desc_in, a),
        ] {
            self.push(Pay {
                day,
                acct,
                credit,
                cents: amount,
                desc,
                cp,
                cat,
                transfer: true,
            });
        }
    }
}

fn heating_base(month: u32) -> f64 {
    match month {
        12 | 1 | 2 => 115.0,
        3 | 11 => 90.0,
        4 | 10 => 66.0,
        5 | 9 => 48.0,
        _ => 38.0,
    }
}

/// Generate every scheduled payment (everything except interest, sweeps and rescues).
fn generate(opts: &DemoOptions, names: [String; 3]) -> Vec<Pay> {
    let now = opts.now;
    let lang = opts.language.as_str();
    let car_from = car_bought_day(now);
    let today = clock::today(now);
    let start = clock::months_ago(now, HISTORY_MONTHS, 1);
    let mut g = Gen {
        rng: Rng::new(),
        out: Vec::new(),
        start,
        today,
        names,
    };

    let etf = etf_buys(now);
    let mut delivery_days = std::collections::HashSet::new();
    for m in (0..=HISTORY_MONTHS).rev() {
        let date = clock::date_of(clock::months_ago(now, m, 1));
        let month = date.month();
        let d = |dom: u32| clock::months_ago(now, m, dom);

        // Income
        let salary = match m {
            37..=48 => 3_350.0,
            25..=36 => 3_550.0,
            13..=24 => 3_850.0,
            _ => 4_100.0,
        };
        g.credit(d(1), c(salary), T::Salary, "Northwind Labs", "cat_income");
        if month == 3 {
            g.credit(
                d(1),
                c(salary * 0.9),
                T::Bonus,
                "Northwind Labs",
                "cat_income",
            );
        }
        if month == 5 {
            let a = g.amt(380.0, 620.0);
            g.credit(
                d(14),
                a,
                T::TaxRefund,
                t(lang, T::TaxOffice),
                "cat_other_income",
            );
        }
        if m < PARKING_BOUGHT_M {
            g.credit(
                d(5),
                c(PARKING_RENT_EUR),
                T::ParkingRent,
                "M. Keller",
                "cat_investment_income",
            );
        }

        // Fixed payments
        g.debit(
            d(15),
            c(MORTGAGE_PAYMENT),
            T::MortgagePayment,
            "Nordlicht Bank",
            "cat_loan_payments",
        );
        if m < KITCHEN_M {
            g.debit(
                d(12),
                c(KITCHEN_PAYMENT),
                T::KitchenLoanPayment,
                "Nordlicht Bank",
                "cat_loan_payments",
            );
        }
        g.debit(
            d(10),
            c(185.0),
            T::BuildingFee,
            t(lang, T::OwnersAssociation),
            "cat_housing",
        );
        let a = g.amt(58.0, 64.0);
        g.debit(d(8), a, T::Electricity, "CityEnergy", "cat_utilities");
        let a = c(heating_base(month) + g.rng.range(-3.0, 3.0));
        g.debit(d(8), a, T::Heating, "CityEnergy", "cat_utilities");
        g.debit(d(6), c(34.90), T::Internet, "Vodafone", "cat_utilities");
        g.debit(d(7), c(19.99), T::MobilePlan, "Vodafone", "cat_utilities");
        g.debit(
            d(18),
            c(13.99),
            T::Streaming,
            "Netflix",
            "cat_entertainment",
        );
        g.debit(d(19), c(10.99), T::MusicSub, "Spotify", "cat_entertainment");
        g.debit(
            d(20),
            c(2.99),
            T::CloudStorage,
            "Apple",
            "cat_entertainment",
        );
        g.debit(d(3), c(39.0), T::Gym, "FitLife", "cat_health");
        g.debit(
            d(1),
            c(49.0),
            T::TransitPass,
            t(lang, T::PublicTransport),
            "cat_transport",
        );
        g.debit(
            d(LIFE_DAY),
            c(31.50),
            T::PremLife,
            "Aurora Insurance",
            "cat_insurance",
        );
        match month {
            m if m == HOME_PREMIUM.0 => g.debit(
                d(HOME_PREMIUM.1),
                c(168.0),
                T::PremHome,
                "Aurora Insurance",
                "cat_insurance",
            ),
            m if m == LIABILITY_PREMIUM.0 => g.debit(
                d(LIABILITY_PREMIUM.1),
                c(62.0),
                T::PremLiability,
                "Aurora Insurance",
                "cat_insurance",
            ),
            m if m == TRAVEL_PREMIUM.0 => g.debit(
                d(TRAVEL_PREMIUM.1),
                c(54.0),
                T::PremTravel,
                "Aurora Insurance",
                "cat_insurance",
            ),
            5 => g.debit(
                d(20),
                c(96.0),
                T::PropertyTax,
                t(lang, T::Municipality),
                "cat_taxes",
            ),
            _ => {}
        }

        // Variable payments
        for _ in 0..g.rng.int_range(6, 9) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(18.0, 95.0);
            let shop = *g.rng.pick(&["Lidl", "Aldi", "SPAR"]).unwrap_or(&"Lidl");
            g.debit(d(dom), a, T::Groceries, shop, "cat_groceries");
        }
        for _ in 0..g.rng.int_range(1, 2) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(9.0, 38.0);
            g.debit(d(dom), a, T::Drugstore, "dm", "cat_shopping");
        }
        for _ in 0..g.rng.int_range(4, 7) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(9.0, 64.0);
            let (desc, place) = *g
                .rng
                .pick(&[
                    (T::Restaurant, "Trattoria Sole"),
                    (T::Cafe, "Café Luna"),
                    (T::Restaurant, "Sushi Koi"),
                    (T::Restaurant, "Burger Brothers"),
                    (T::FoodDelivery, "Wolt"),
                    (T::FoodDelivery, "Wolt"),
                ])
                .unwrap_or(&(T::Restaurant, "Trattoria Sole"));
            // at most one food delivery a day
            let (desc, place) = if desc == T::FoodDelivery && !delivery_days.insert(d(dom)) {
                (T::Restaurant, "Burger Brothers")
            } else {
                (desc, place)
            };
            g.debit(d(dom), a, desc, place, "cat_dining");
        }
        if d(1) >= clock::month_start(car_from) {
            for _ in 0..2 {
                let dom = g.rng.int_range(1, 28) as u32;
                let a = g.amt(48.0, 78.0);
                if d(dom) <= car_from {
                    continue;
                }
                g.debit(d(dom), a, T::Fuel, "Shell", "cat_transport");
            }
            if month == 10 && d(1) > car_from {
                let a = g.amt(240.0, 460.0);
                g.debit(d(20), a, T::CarService, "Autohaus Lindner", "cat_transport");
            }
        }
        for _ in 0..g.rng.int_range(0, 3) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(7.0, 24.0);
            g.debit(d(dom), a, T::Taxi, "Bolt", "cat_transport");
        }
        for _ in 0..g.rng.int_range(1, 3) {
            let dom = g.rng.int_range(1, 28) as u32;
            let r = g.rng.next();
            if r < 0.4 {
                let a = g.amt(14.0, 140.0);
                g.debit(d(dom), a, T::OnlineOrder, "Amazon", "cat_shopping");
            } else if r < 0.7 {
                let a = g.amt(28.0, 120.0);
                let shop = *g.rng.pick(&["H&M", "Zara"]).unwrap_or(&"H&M");
                g.debit(d(dom), a, T::Clothes, shop, "cat_shopping");
            } else {
                let a = g.amt(20.0, 110.0);
                g.debit(d(dom), a, T::SportsGear, "Decathlon", "cat_shopping");
            }
        }
        if g.rng.chance(0.25) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(35.0, 320.0);
            g.debit(d(dom), a, T::Furniture, "IKEA", "cat_shopping");
        }
        if g.rng.chance(0.1) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(150.0, 800.0);
            g.debit(d(dom), a, T::Electronics, "MediaMarkt", "cat_shopping");
        }
        if g.rng.chance(0.75) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(6.0, 34.0);
            g.debit(d(dom), a, T::Pharmacy, "Linden Apotheke", "cat_health");
        }
        if month == 2 || month == 8 {
            let a = g.amt(60.0, 130.0);
            g.debit(d(16), a, T::Dentist, "Smile Dental", "cat_health");
        }
        for _ in 0..g.rng.int_range(0, 2) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(11.0, 26.0);
            g.debit(d(dom), a, T::Cinema, "CineStar", "cat_entertainment");
        }
        if month % 3 == 1 {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(42.0, 120.0);
            g.debit(
                d(dom),
                a,
                T::ConcertTickets,
                "Ticketmaster",
                "cat_entertainment",
            );
        }
        if g.rng.chance(0.25) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(8.0, 40.0);
            g.debit(d(dom), a, T::Games, "Steam", "cat_entertainment");
        }
        if g.rng.chance(0.3) {
            let dom = g.rng.int_range(1, 28) as u32;
            let a = g.amt(12.0, 35.0);
            g.debit(d(dom), a, T::Books, "Thalia", "cat_entertainment");
        }
        if month == 12 {
            for _ in 0..g.rng.int_range(3, 5) {
                let dom = g.rng.int_range(1, 22) as u32;
                let a = g.amt(30.0, 120.0);
                let shop = *g
                    .rng
                    .pick(&["Amazon", "Zalando", "Douglas", "Thalia", "LEGO Store"])
                    .unwrap_or(&"Amazon");
                g.debit(d(dom), a, T::Gifts, shop, "cat_shopping");
            }
        }

        // Transfers
        g.transfer(
            d(2),
            CUR,
            SAV,
            c(300.0),
            c(300.0),
            T::TransferSavings,
            T::TransferSavings,
            "cat_savings",
        );
        // The month's ETF purchase, funded on the 3rd
        if let Some(buy) = etf.iter().find(|b| b.m == m) {
            let cost = buy.quantity * expected_price(VWCE.0, buy.day) + TRADE_FEE;
            g.debit(
                d(3),
                c(next_50(cost)),
                T::BrokerDeposit,
                BROKER,
                "cat_investments",
            );
        }

        // Dated events of the life
        if m == KITCHEN_M {
            g.debit(
                d(KITCHEN_DAY),
                c(1_380.0),
                T::KitchenAppliances,
                "MediaMarkt",
                "cat_shopping",
            );
        }
        if m == EXTRA_MORTGAGE_M {
            g.transfer(
                d(EXTRA_MORTGAGE_DAY - 1),
                SAV,
                CUR,
                c(EXTRA_MORTGAGE_EUR),
                c(EXTRA_MORTGAGE_EUR),
                T::TransferFromSavings,
                T::TransferFromSavings,
                "cat_savings",
            );
            g.debit(
                d(EXTRA_MORTGAGE_DAY),
                c(EXTRA_MORTGAGE_EUR),
                T::ExtraMortgage,
                "Nordlicht Bank",
                "cat_loan_payments",
            );
        }
        if m == PARKING_BOUGHT_M {
            g.pay(
                d(PARKING_DAY),
                SAV,
                false,
                c(PARKING_PRICE_EUR),
                T::ParkingPurchase,
                "Notariat Hartmann",
                "cat_other",
            );
        }
        if m == BOND_BOUGHT_M {
            g.pay(
                d(BOND_DAY),
                SAV,
                false,
                c(BOND_PRICE_EUR),
                T::BondName,
                BROKER,
                "cat_investments",
            );
        }
        if m == GOLD_BOUGHT_M {
            g.pay(
                d(GOLD_DAY),
                SAV,
                false,
                c(2.0 * GOLD_COIN_EUR),
                T::GoldPurchase,
                "Degussa",
                "cat_investments",
            );
        }
        for (i, month_n) in BTC_BUYS_M.iter().enumerate() {
            if m == *month_n {
                g.pay(
                    d(BTC_DAY),
                    SAV,
                    false,
                    c(BTC_BUYS_EUR[i]),
                    T::CryptoPurchase,
                    "Kraken",
                    "cat_investments",
                );
            }
        }
        for (i, month_n) in ETH_BUYS_M.iter().enumerate() {
            if m == *month_n {
                g.pay(
                    d(ETH_DAY),
                    SAV,
                    false,
                    c(ETH_BUYS_EUR[i]),
                    T::CryptoPurchase,
                    "Kraken",
                    "cat_investments",
                );
            }
        }
        if m == ETH_SELL_M {
            g.pay(
                d(ETH_DAY),
                SAV,
                true,
                c(eth_sale_eur(now)),
                T::CryptoSale,
                "Kraken",
                "cat_investments",
            );
        }
    }

    g.pay(
        car_from,
        SAV,
        false,
        c(CAR_PRICE_EUR),
        T::CarPurchase,
        "Autohaus Lindner",
        "cat_other",
    );
    car_insurance(&mut g, now);
    broker_funding(&mut g, now);
    trips(&mut g, opts);
    g.out
}

/// Days before a USD trip its account is topped up.
const TOP_UP_LEAD_DAYS: i64 = 7;

/// The broker the stock purchases go through.
const BROKER: &str = "Trade Republic";
/// Fee of a stock trade (in the trade currency).
const TRADE_FEE: f64 = 1.0;
/// Margin over the expected price, the most a trade price deviates from the close.
const PRICE_MARGIN: f64 = 1.006;

/// The next multiple of 50 above `eur`.
fn next_50(eur: f64) -> f64 {
    ((eur / 50.0).floor() + 1.0) * 50.0
}

/// Upper estimate of a trade price in EUR (today's units) on `day`, from the built-in closes.
fn expected_price(ticker: &str, day: i64) -> f64 {
    match prices::price_on(ticker, day) {
        Some((price, "USD")) => price * PRICE_MARGIN / prices::usd_per_eur(day),
        Some((price, _)) => price * PRICE_MARGIN,
        None => 0.0,
    }
}

/// Every single-stock purchase is funded from savings 2-4 days before it, with its expected
/// cost in EUR rounded up to the next 50. Sale proceeds stay at the broker.
fn broker_funding(g: &mut Gen, now: i64) {
    for (i, trade) in stock_trades(now).iter().filter(|t| !t.sell).enumerate() {
        let usd = matches!(prices::price_on(trade.ticker, trade.day), Some((_, "USD")));
        let fee = if usd {
            TRADE_FEE / prices::usd_per_eur(trade.day)
        } else {
            TRADE_FEE
        };
        let cost =
            trade.quantity * trade.units_factor * expected_price(trade.ticker, trade.day) + fee;
        g.pay(
            trade.day - (2 + i as i64 % 3) * DAY,
            SAV,
            false,
            c(next_50(cost)),
            T::BrokerDeposit,
            BROKER,
            "cat_investments",
        );
    }
}

/// Days a dated fact moves money between the savings account and another class (a purchase,
/// a crypto sale, a stock trade, the extra repayment): each gets a savings balance record of
/// its own, so the history moves the cash on the same day as the asset.
fn event_days(now: i64) -> std::collections::BTreeSet<i64> {
    let m = |n: i32, dom: u32| clock::months_ago(now, n, dom);
    let mut days: std::collections::BTreeSet<i64> = [
        car_bought_day(now),
        m(PARKING_BOUGHT_M, PARKING_DAY),
        m(GOLD_BOUGHT_M, GOLD_DAY),
        m(BOND_BOUGHT_M, BOND_DAY),
        m(EXTRA_MORTGAGE_M, EXTRA_MORTGAGE_DAY),
        m(ETH_SELL_M, ETH_DAY),
    ]
    .into_iter()
    .collect();
    days.extend(BTC_BUYS_M.iter().map(|n| m(*n, BTC_DAY)));
    days.extend(ETH_BUYS_M.iter().map(|n| m(*n, ETH_DAY)));
    days.extend(stock_trades(now).iter().map(|t| t.day));
    days
}

/// Yearly car premium on the policy anniversaries ([`car_premium_days`]); the next one falls
/// 30-40 days after today.
fn car_insurance(g: &mut Gen, now: i64) {
    for day in car_premium_days(now) {
        g.debit(
            day,
            c(612.0),
            T::PremCar,
            "Aurora Insurance",
            "cat_insurance",
        );
    }
}

/// One short trip each April and one long trip each September, by calendar month.
fn trips(g: &mut Gen, opts: &DemoOptions) {
    let now = opts.now;
    let today = clock::today(now);
    let (first_year, last_year) = (clock::date_of(g.start).year(), clock::date_of(today).year());
    // The two most recent September trips are paid from the USD account. A trip counts once
    // its top-up day (a week before it starts) has passed; while it is in progress only its
    // payments up to today exist (later ones are not pushed), so the travel balance is higher.
    let mut usd_trips: Vec<i64> = (first_year..=last_year)
        .map(|y| ymd(y, LONG_TRIP.0, LONG_TRIP.1))
        .filter(|s| *s - TOP_UP_LEAD_DAYS * DAY <= today)
        .collect();
    usd_trips.drain(..usd_trips.len().saturating_sub(2));
    let mut usd_bal: i64 = 0;
    // Trip payments have holiday descriptions of their own (their category is travel)
    let eur_places = [
        (T::HolidayMeal, "Ristorante Da Marco"),
        (T::HolidayMeal, "Bar Sol"),
        (T::HolidayTransport, "FreeNow"),
        (T::HolidaySpending, "Bottega Lucia"),
        (T::MuseumTickets, "Museo Civico"),
    ];
    let usd_places = [
        (T::HolidayMeal, "Blue Plate Diner"),
        (T::HolidayTransport, "Uber"),
        (T::HolidayShopping, "Whole Foods"),
        (T::MuseumTickets, "Whitmore Museum"),
        (T::HolidayMeal, "Corner Brew Cafe"),
        (T::HolidaySpending, "Mariner's Cove"),
    ];

    for year in first_year..=last_year {
        for long in [false, true] {
            let (mo, dom) = if long { LONG_TRIP } else { SHORT_TRIP };
            let days = if long {
                LONG_TRIP_DAYS
            } else {
                SHORT_TRIP_DAYS
            };
            let start = ymd(year, mo, dom);
            let (flight, lodging) = if long {
                (g.amt(420.0, 780.0), g.amt(520.0, 900.0))
            } else {
                (g.amt(90.0, 240.0), g.amt(220.0, 420.0))
            };
            let (airline, host) = if long {
                ("Lufthansa", "Airbnb")
            } else {
                ("Ryanair", "Booking.com")
            };
            g.debit(start - 30 * DAY, flight, T::Flights, airline, "cat_travel");
            g.debit(start, lodging, T::Accommodation, host, "cat_travel");

            let n = g.rng.int_range(4, 8) as usize;
            if long && usd_trips.contains(&start) {
                let eur = c((g.rng.range(1_350.0, 1_450.0) / 10.0).round() * 10.0);
                let usd = (eur as f64 * prices::usd_per_eur(start - TOP_UP_LEAD_DAYS * DAY)).round()
                    as i64;
                g.transfer(
                    start - TOP_UP_LEAD_DAYS * DAY,
                    CUR,
                    TRV,
                    eur,
                    usd,
                    T::TravelTopUp,
                    T::TravelTopUp,
                    "cat_internal_transfers",
                );
                usd_bal += usd;
                let is_last = usd_trips.last() == Some(&start);
                let leave = if is_last {
                    g.amt(1_100.0, 1_300.0)
                } else {
                    g.amt(400.0, 700.0)
                };
                let spend = (usd_bal - leave).max(c(300.0));
                let weights: Vec<f64> = (0..n).map(|_| g.rng.range(0.5, 1.5)).collect();
                let total_w: f64 = weights.iter().sum();
                let mut left = spend;
                for (i, w) in weights.iter().enumerate() {
                    let a = if i + 1 == n {
                        left
                    } else {
                        ((spend as f64) * w / total_w).round() as i64
                    };
                    left -= a;
                    let offset = g.rng.int_range(0, days - 1);
                    let (desc, place) = usd_places[i % usd_places.len()];
                    g.pay(
                        start + offset * DAY,
                        TRV,
                        false,
                        a,
                        desc,
                        place,
                        "cat_travel",
                    );
                }
                usd_bal -= spend;
            } else {
                for _ in 0..n {
                    let offset = g.rng.int_range(0, days - 1);
                    let a = if long {
                        g.amt(25.0, 110.0)
                    } else {
                        g.amt(12.0, 75.0)
                    };
                    let (desc, place) = *g.rng.pick(&eur_places).unwrap_or(&eur_places[0]);
                    g.debit(start + offset * DAY, a, desc, place, "cat_travel");
                }
            }
        }
    }
}

/// Seed of the year before the payment history (its own stream).
const PRE_SEED: u64 = 0x5052_4548_4953;
/// How much lower the savings are at the flat purchase than at the opening of the payments.
const PRE_SAVINGS_GAP_EUR: f64 = 12_000.0;

/// Balance records of the current and savings accounts for the twelve months before the
/// payment history (no payments): one on the flat purchase day, so the first day of the
/// net-worth history holds cash, then every month end up to the day before the payments start,
/// which equals the opening balances. Savings rise unevenly from about 12 000 below the
/// opening (never above it); the current account wanders between about 2 300 and 3 400.
fn pre_history(now: i64, open_savings: i64) -> Vec<Snapshot> {
    let mut rng = Rng::with_seed(PRE_SEED);
    let mut days = vec![clock::months_ago(now, FLAT_BOUGHT_M, FLAT_DAY)];
    days.extend(
        ((HISTORY_MONTHS + 1)..=FLAT_BOUGHT_M)
            .rev()
            .map(|n| clock::month_end(clock::months_ago(now, n, 1))),
    );
    let last = days.len() - 1;
    days.into_iter()
        .enumerate()
        .map(|(k, day)| {
            let bal = if k == last {
                [c(OPEN_CURRENT_EUR), open_savings, 0]
            } else {
                let rest = 1.0 - k as f64 / last as f64;
                let savings =
                    open_savings - c(PRE_SAVINGS_GAP_EUR * rest) + c(rng.range(-350.0, 250.0));
                [
                    c(rng.range(2_300.0, 3_400.0)),
                    savings.min(open_savings - c(40.0)),
                    0,
                ]
            };
            Snapshot {
                day,
                bal,
                savings_only: false,
            }
        })
        .collect()
}

/// The balances at the end of a simulated day.
struct Snapshot {
    day: i64,
    bal: [i64; 3],
    /// A record of the savings account alone (the day of a fact it funds), not a month end.
    savings_only: bool,
}

fn apply(bal: &mut [i64; 3], p: &Pay) {
    if p.credit {
        bal[p.acct] += p.cents;
    } else {
        bal[p.acct] -= p.cents;
    }
}

/// Replay the scheduled payments day by day, adding interest, sweeps and rescues.
/// Returns the final payment list in booking order and the month-end snapshots.
fn simulate(
    opts: &DemoOptions,
    names: &[String; 3],
    scheduled: &[Pay],
    open_savings: i64,
    events: &std::collections::BTreeSet<i64>,
) -> (Vec<Pay>, Vec<Snapshot>, i64) {
    let today = clock::today(opts.now);
    let start = clock::months_ago(opts.now, HISTORY_MONTHS, 1);
    let mut sched = scheduled.to_vec();
    sched.sort_by_key(|p| p.day);
    let mut bal = [c(OPEN_CURRENT_EUR), open_savings, 0];
    let mut min_savings = open_savings;
    let mut out: Vec<Pay> = Vec::with_capacity(sched.len() + 200);
    let mut snaps = Vec::new();
    let mut idx = 0;
    let mut day = start;

    let pair = |out: &mut Vec<Pay>,
                bal: &mut [i64; 3],
                day: i64,
                from: usize,
                to: usize,
                cents: i64,
                d_out: T,
                d_in: T| {
        for (acct, credit, desc, cp) in [
            (from, false, d_out, names[to].clone()),
            (to, true, d_in, names[from].clone()),
        ] {
            let p = Pay {
                day,
                acct,
                credit,
                cents,
                desc,
                cp,
                cat: "cat_savings",
                transfer: true,
            };
            apply(bal, &p);
            out.push(p);
        }
    };

    while day <= today {
        while idx < sched.len() && sched[idx].day == day {
            let p = sched[idx].clone();
            idx += 1;
            if p.acct == CUR && !p.credit {
                while bal[CUR] - p.cents < c(MIN_CURRENT_EUR) {
                    let pull = c(1_000.0).min(bal[SAV]);
                    if pull <= 0 {
                        break;
                    }
                    pair(
                        &mut out,
                        &mut bal,
                        day,
                        SAV,
                        CUR,
                        pull,
                        T::TransferFromSavings,
                        T::TransferFromSavings,
                    );
                }
            }
            apply(&mut bal, &p);
            out.push(p);
            min_savings = min_savings.min(bal[SAV]);
        }
        if day == clock::month_end(day) {
            let interest = (bal[SAV] as f64 * SAVINGS_RATE / 1200.0).round() as i64;
            if interest > 0 {
                let p = Pay {
                    day,
                    acct: SAV,
                    credit: true,
                    cents: interest,
                    desc: T::SavingsInterest,
                    cp: "Nordlicht Bank".to_string(),
                    cat: "cat_investment_income",
                    transfer: false,
                };
                apply(&mut bal, &p);
                out.push(p);
            }
            if bal[CUR] > c(SWEEP_ABOVE_EUR) {
                let excess = (bal[CUR] - c(SWEEP_KEEP_EUR)) / 5_000 * 5_000;
                if excess > 0 {
                    pair(
                        &mut out,
                        &mut bal,
                        day,
                        CUR,
                        SAV,
                        excess,
                        T::TransferSavings,
                        T::TransferSavings,
                    );
                }
            }
        }
        let regular = day == clock::month_end(day) || day == today;
        if regular || events.contains(&day) {
            snaps.push(Snapshot {
                day,
                bal,
                savings_only: !regular,
            });
        }
        day += DAY;
    }
    (out, snaps, min_savings)
}

/// Create the accounts, replay the history into payments and write the balance records.
pub fn seed(
    conn: &rusqlite::Connection,
    opts: &DemoOptions,
    summary: &mut DemoSummary,
) -> Result<()> {
    let lang = opts.language.as_str();
    let names = [
        t(lang, T::AccCurrent).to_string(),
        t(lang, T::AccSavings).to_string(),
        t(lang, T::AccTravel).to_string(),
    ];

    let scheduled = generate(opts, names.clone());
    let events = event_days(opts.now);
    // A different `now` moves the big purchases against the seasonal income, so the opening
    // savings are raised (in whole thousands) until the savings account never runs dry.
    let mut open_savings = c(OPEN_SAVINGS_EUR);
    let (payments, snaps) = loop {
        let (payments, snaps, min_savings) =
            simulate(opts, &names, &scheduled, open_savings, &events);
        if min_savings >= c(MIN_SAVINGS_EUR) || open_savings > c(500_000.0) {
            break (payments, snaps);
        }
        let short = c(MIN_SAVINGS_EUR) - min_savings;
        open_savings += (short / 100_000 + 1) * 100_000;
    };

    let mk = |name: &str,
              kind: &str,
              currency: &str,
              rate: Option<&str>,
              until: Option<i64>,
              inst: Option<&str>| InsertBankAccount {
        name: name.to_string(),
        account_type: Some(kind.to_string()),
        iban: None,
        bban: None,
        currency: Some(currency.to_string()),
        balance: Some("0".to_string()),
        balance_as_of: None,
        institution_id: inst.map(str::to_string),
        interest_rate: rate.map(str::to_string),
        has_zone_designation: None,
        termination_date: None,
        interest_rate_valid_until: until,
        exclude_from_balance: None,
    };
    let rate_until = clock::months_ago(opts.now, -SAVINGS_RATE_END_AHEAD_M, 15);
    let ids = [
        bank_accounts::create_account(conn, &mk(&names[CUR], "checking", "EUR", None, None, None))?
            .id,
        bank_accounts::create_account(
            conn,
            &mk(
                &names[SAV],
                "savings",
                "EUR",
                Some("2.6"),
                Some(rate_until),
                None,
            ),
        )?
        .id,
        bank_accounts::create_account(
            conn,
            &mk(
                &names[TRV],
                "checking",
                "USD",
                None,
                None,
                Some("inst_wise"),
            ),
        )?
        .id,
    ];
    let currencies = ["EUR", "EUR", "USD"];

    // Payments in one transaction (the account and balance services open their own).
    let tx = conn.unchecked_transaction()?;
    for p in &payments {
        bank_accounts::create_transaction(
            &tx,
            &InsertBankTransaction {
                bank_account_id: ids[p.acct].clone(),
                transaction_id: None,
                tx_type: if p.credit { "credit" } else { "debit" }.to_string(),
                flow_kind: if p.transfer {
                    Some(FlowKind::Transfer)
                } else {
                    None
                },
                amount: money(p.cents),
                currency: None,
                description: Some(t(&opts.language, p.desc).to_string()),
                counterparty_name: Some(p.cp.clone()),
                counterparty_iban: None,
                counterparty_bank_code: None,
                counterparty_country: None,
                booking_date: p.day,
                value_date: None,
                category_id: Some(p.cat.to_string()),
                status: None,
            },
        )?;
    }
    tx.commit()?;

    // The year before the payments: month-end records only, from the flat purchase on
    let mut snaps = snaps;
    snaps.splice(0..0, pre_history(opts.now, open_savings));

    // Month-end balance records; the travel account starts with its first use.
    let first_travel = payments.iter().find(|p| p.acct == TRV).map(|p| p.day);
    let mut records = 0;
    for s in &snaps {
        for acct in 0..3 {
            if acct == TRV && first_travel.is_none_or(|d| s.day < d) {
                continue;
            }
            if s.savings_only && acct != SAV {
                continue;
            }
            bank_accounts::record_balance(
                conn,
                &ids[acct],
                &InsertBalanceRecord {
                    amount: money(s.bal[acct]),
                    currency: currencies[acct].to_string(),
                    as_of: s.day,
                    confirm_unknown_as_of: false,
                    source_batch_id: None,
                },
            )?;
            records += 1;
        }
    }

    summary.counts.insert("bank_accounts", 3);
    summary.counts.insert("bank_transactions", payments.len());
    summary.counts.insert("bank_balance_records", records);
    Ok(())
}
