//! Built-in prices, FX rates and dividends for the demo instruments.
//!
//! Quarter-end anchor prices per instrument by absolute date, approximately the real closes,
//! from 2020-12-31 through 2025-12-31 (confident through 2024; 2025 values are rougher).
//! Between anchors the series is interpolated geometrically with a smooth deterministic
//! bump and a small day-keyed wiggle (a hash of ticker and day, never the shared `Rng`, so
//! a day's value never depends on call order). After the last anchor a fixed gentle annual
//! drift continues the series, so any future "today" works.

use super::clock::{day_of, DAY};
use chrono::{Datelike, NaiveDate};
use std::sync::OnceLock;

const N: usize = 21;
/// Anchor dates: every quarter end from 2020-12-31 to 2025-12-31.
const ANCHORS: [(i32, u32, u32); N] = [
    (2020, 12, 31),
    (2021, 3, 31),
    (2021, 6, 30),
    (2021, 9, 30),
    (2021, 12, 31),
    (2022, 3, 31),
    (2022, 6, 30),
    (2022, 9, 30),
    (2022, 12, 31),
    (2023, 3, 31),
    (2023, 6, 30),
    (2023, 9, 30),
    (2023, 12, 31),
    (2024, 3, 31),
    (2024, 6, 30),
    (2024, 9, 30),
    (2024, 12, 31),
    (2025, 3, 31),
    (2025, 6, 30),
    (2025, 9, 30),
    (2025, 12, 31),
];

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Equity,
    Crypto,
    Fx,
}

struct Instrument {
    ticker: &'static str,
    currency: &'static str,
    kind: Kind,
    /// Annual drift after the last anchor.
    drift: f64,
    anchors: [f64; N],
}

const fn eq(ticker: &'static str, currency: &'static str, anchors: [f64; N]) -> Instrument {
    Instrument {
        ticker,
        currency,
        kind: Kind::Equity,
        drift: 0.06,
        anchors,
    }
}

const fn crypto(ticker: &'static str, anchors: [f64; N]) -> Instrument {
    Instrument {
        ticker,
        currency: "USD",
        kind: Kind::Crypto,
        drift: 0.10,
        anchors,
    }
}

const INSTRUMENTS: &[Instrument] = &[
    eq(
        "VWCE.DE",
        "EUR",
        [
            79.0, 85.7, 92.3, 92.4, 100.4, 97.6, 84.7, 82.2, 88.6, 91.5, 98.8, 96.0, 103.6, 117.0,
            122.5, 124.7, 129.8, 125.6, 128.0, 138.4, 144.0,
        ],
    ),
    eq(
        "MSFT",
        "USD",
        [
            222.4, 235.8, 270.9, 281.9, 336.3, 308.3, 256.8, 232.9, 239.8, 288.3, 340.5, 315.8,
            376.0, 420.7, 446.9, 430.3, 421.5, 375.4, 497.4, 517.9, 484.0,
        ],
    ),
    eq(
        "AAPL",
        "USD",
        [
            132.7, 122.2, 137.0, 141.5, 177.6, 174.6, 136.7, 138.2, 129.9, 164.9, 194.0, 171.2,
            192.5, 171.5, 210.6, 233.0, 250.4, 222.1, 205.2, 254.6, 272.0,
        ],
    ),
    eq(
        "ASML.AS",
        "EUR",
        [
            398.5, 518.6, 580.0, 593.0, 619.4, 612.0, 446.5, 487.0, 505.2, 626.0, 618.0, 553.0,
            701.0, 896.0, 960.0, 744.0, 658.0, 620.0, 700.0, 830.0, 950.0,
        ],
    ),
    eq(
        "ALV.DE",
        "EUR",
        [
            200.0, 205.0, 211.0, 197.0, 207.0, 218.0, 203.0, 168.0, 200.0, 213.0, 206.0, 220.0,
            242.0, 272.0, 285.0, 283.0, 285.0, 335.0, 342.0, 350.0, 355.0,
        ],
    ),
    // Fully split-adjusted: the 4:1 of 2021 and the 10:1 of 2024-06-10 are already folded in.
    eq(
        "NVDA",
        "USD",
        [
            13.1, 13.3, 18.8, 20.7, 29.4, 27.3, 15.2, 12.1, 14.6, 27.8, 42.3, 43.5, 49.5, 90.4,
            123.5, 121.4, 134.3, 108.4, 158.2, 186.6, 187.0,
        ],
    ),
    eq(
        "PYPL",
        "USD",
        [
            234.2, 242.8, 291.5, 260.2, 188.6, 115.6, 69.9, 85.5, 75.9, 76.0, 65.8, 58.4, 61.4,
            59.8, 67.0, 77.9, 85.4, 65.2, 71.3, 67.1, 58.0,
        ],
    ),
    eq(
        "GOOGL",
        "USD",
        [
            87.6, 103.7, 134.0, 135.0, 144.9, 139.7, 109.4, 95.7, 88.2, 103.7, 120.2, 131.1, 140.9,
            150.9, 183.4, 165.0, 189.3, 165.5, 177.9, 243.1, 310.0,
        ],
    ),
    eq(
        "AMZN",
        "USD",
        [
            162.8, 154.7, 172.0, 164.3, 166.7, 162.0, 106.2, 113.0, 84.0, 103.3, 130.4, 127.1,
            151.9, 180.4, 193.3, 186.3, 219.4, 190.3, 219.4, 219.6, 230.0,
        ],
    ),
    eq(
        "SIE.DE",
        "EUR",
        [
            117.0, 138.0, 134.0, 140.0, 152.0, 125.0, 97.0, 100.0, 127.0, 150.0, 151.0, 133.0,
            170.0, 175.0, 174.0, 173.0, 188.0, 207.0, 213.0, 224.0, 235.0,
        ],
    ),
    // Split-adjusted for the 2:1 of 2023; least certain series, rough levels.
    eq(
        "NOVO-B.CO",
        "DKK",
        [
            215.0, 218.0, 255.0, 305.0, 365.0, 360.0, 410.0, 400.0, 420.0, 480.0, 540.0, 590.0,
            700.0, 780.0, 950.0, 800.0, 500.0, 460.0, 440.0, 330.0, 320.0,
        ],
    ),
    crypto(
        "BTC",
        [
            29000.0, 58800.0, 35000.0, 43800.0, 46300.0, 45500.0, 19800.0, 19400.0, 16500.0,
            28500.0, 30500.0, 27000.0, 42300.0, 71300.0, 62800.0, 63300.0, 93400.0, 82500.0,
            107100.0, 114000.0, 88000.0,
        ],
    ),
    crypto(
        "ETH",
        [
            737.0, 1920.0, 2280.0, 3000.0, 3680.0, 3380.0, 1070.0, 1330.0, 1200.0, 1810.0, 1930.0,
            1670.0, 2280.0, 3500.0, 3400.0, 2650.0, 3330.0, 1820.0, 2520.0, 4140.0, 2950.0,
        ],
    ),
];

/// USD per 1 EUR.
const USD_PER_EUR: Instrument = Instrument {
    ticker: "FX:USD",
    currency: "USD",
    kind: Kind::Fx,
    drift: 0.0,
    anchors: [
        1.222, 1.173, 1.186, 1.158, 1.137, 1.107, 1.048, 0.980, 1.070, 1.084, 1.091, 1.057, 1.104,
        1.079, 1.071, 1.113, 1.035, 1.082, 1.172, 1.173, 1.170,
    ],
};

/// DKK per 1 EUR (pegged, practically flat).
const DKK_PER_EUR: Instrument = Instrument {
    ticker: "FX:DKK",
    currency: "DKK",
    kind: Kind::Fx,
    drift: 0.0,
    anchors: [
        7.444, 7.436, 7.436, 7.437, 7.437, 7.439, 7.437, 7.437, 7.437, 7.451, 7.446, 7.457, 7.454,
        7.457, 7.458, 7.459, 7.458, 7.460, 7.460, 7.462, 7.460,
    ],
};

fn anchor_days() -> &'static [i64; N] {
    static DAYS: OnceLock<[i64; N]> = OnceLock::new();
    DAYS.get_or_init(|| {
        let mut out = [0i64; N];
        for (slot, (y, m, d)) in out.iter_mut().zip(ANCHORS) {
            *slot = NaiveDate::from_ymd_opt(y, m, d).map(day_of).unwrap_or(0);
        }
        out
    })
}

fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01B3)
    })
}

/// Deterministic value in [-1, 1) from a key and a salt.
fn unit(key: u64, salt: u64) -> f64 {
    let h = mix(key ^ mix(salt));
    ((h >> 11) as f64) / ((1u64 << 53) as f64) * 2.0 - 1.0
}

/// Friday for Saturday and Sunday.
fn weekday_on_or_before(day: i64) -> i64 {
    let dow = (day.div_euclid(DAY) + 3).rem_euclid(7); // Monday = 0; 1970-01-01 was Thursday
    if dow >= 5 {
        day - (dow - 4) * DAY
    } else {
        day
    }
}

fn series(inst: &Instrument, day: i64) -> f64 {
    let key = fnv(inst.ticker);
    let d = if inst.kind == Kind::Crypto {
        day
    } else {
        weekday_on_or_before(day)
    };
    let (wiggle, bump, wave) = match inst.kind {
        Kind::Equity => (0.005, 0.04, 0.03),
        Kind::Crypto => (0.015, 0.10, 0.08),
        Kind::Fx => (0.0015, 0.015, 0.02),
    };
    let days = anchor_days();
    let last = N - 1;
    let base_ln = if d <= days[0] {
        inst.anchors[0].ln()
    } else if d >= days[last] {
        let years = (d - days[last]) as f64 / (365.25 * DAY as f64);
        let trend = inst.anchors[last].ln() + (1.0 + inst.drift).ln() * years;
        trend + wave * (2.0 * std::f64::consts::PI * years / 0.9).sin() * unit(key, 7).signum()
    } else {
        let i = days.partition_point(|a| *a <= d) - 1;
        let f = (d - days[i]) as f64 / (days[i + 1] - days[i]) as f64;
        let (a, b) = (inst.anchors[i].ln(), inst.anchors[i + 1].ln());
        a + (b - a) * f + (std::f64::consts::PI * f).sin() * bump * unit(key, i as u64 + 1)
    };
    base_ln.exp() * (unit(key, d as u64) * wiggle).exp()
}

/// Close (or crypto spot) of `ticker` on `day` (UTC midnight) and its native currency.
/// `None` for tickers outside the demo set. Weekends return Friday's value, except crypto.
pub fn price_on(ticker: &str, day: i64) -> Option<(f64, &'static str)> {
    let inst = INSTRUMENTS.iter().find(|i| i.ticker == ticker)?;
    Some((series(inst, day), inst.currency))
}

/// Every ticker `price_on` knows.
pub fn tickers() -> impl Iterator<Item = &'static str> {
    INSTRUMENTS.iter().map(|i| i.ticker)
}

/// USD per 1 EUR on `day`.
pub fn usd_per_eur(day: i64) -> f64 {
    series(&USD_PER_EUR, day)
}

/// DKK per 1 EUR on `day`.
pub fn dkk_per_eur(day: i64) -> f64 {
    series(&DKK_PER_EUR, day)
}

/// Pattern of a dividend payer: ex-dates (month, day) each year and per-share amounts by
/// year in today's units; years after the table grow by `growth`.
struct Payer {
    ticker: &'static str,
    dates: &'static [(u32, u32)],
    first_year: i32,
    per_payment: &'static [f64],
    growth: f64,
}

const PAYERS: &[Payer] = &[
    Payer {
        ticker: "MSFT",
        dates: &[(2, 19), (5, 21), (8, 20), (11, 19)],
        first_year: 2021,
        per_payment: &[0.56, 0.62, 0.68, 0.75, 0.83],
        growth: 0.10,
    },
    Payer {
        ticker: "AAPL",
        dates: &[(2, 7), (5, 9), (8, 8), (11, 7)],
        first_year: 2021,
        per_payment: &[0.205, 0.22, 0.23, 0.24, 0.25],
        growth: 0.04,
    },
    // Fully split-adjusted (4:1 and 10:1).
    Payer {
        ticker: "NVDA",
        dates: &[(3, 6), (6, 11), (9, 11), (12, 4)],
        first_year: 2021,
        per_payment: &[0.004, 0.004, 0.004, 0.0075, 0.01],
        growth: 0.0,
    },
    Payer {
        ticker: "ASML.AS",
        dates: &[(2, 3), (5, 2), (7, 29), (10, 28)],
        first_year: 2021,
        per_payment: &[1.35, 1.45, 1.52, 1.55, 1.60],
        growth: 0.05,
    },
    Payer {
        ticker: "ALV.DE",
        dates: &[(5, 7)],
        first_year: 2021,
        per_payment: &[9.60, 10.80, 11.40, 13.80, 15.40],
        growth: 0.05,
    },
];

/// Per-share dividend events `(ex-day at UTC midnight, amount in the stock's currency,
/// today's units)` for the demo payers, oldest first, for every year from the payer's first
/// through the year of `until` (the caller filters the days it needs); empty for other
/// tickers. Ex-days that fall on a weekend move to the Friday before.
pub fn dividends(ticker: &str, until: i64) -> Vec<(i64, f64)> {
    let Some(p) = PAYERS.iter().find(|p| p.ticker == ticker) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let last_year = super::clock::date_of(until).year();
    for year in p.first_year..=last_year {
        let idx = (year - p.first_year) as usize;
        let amount = match p.per_payment.get(idx) {
            Some(a) => *a,
            None => {
                let extra = (idx + 1 - p.per_payment.len()) as i32;
                p.per_payment.last().copied().unwrap_or(0.0) * (1.0 + p.growth).powi(extra)
            }
        };
        for (m, d) in p.dates {
            if let Some(date) = NaiveDate::from_ymd_opt(year, *m, *d) {
                debug_assert!(date.year() == year);
                out.push((weekday_on_or_before(day_of(date)), amount));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i32, m: u32, d: u32) -> i64 {
        day_of(NaiveDate::from_ymd_opt(y, m, d).expect("date"))
    }

    fn check_series(label: &str, max_jump: f64, f: impl Fn(i64) -> f64) {
        let (start, end) = (ts(2021, 6, 1), ts(2028, 12, 31));
        let mut prev = f(start);
        let mut day = start;
        while day <= end {
            let v = f(day);
            assert!(v.is_finite() && v > 0.0, "{label} not positive on {day}");
            let jump = (v / prev - 1.0).abs();
            assert!(jump < max_jump, "{label} jumps {jump:.3} on day {day}");
            prev = v;
            day += DAY;
        }
    }

    #[test]
    fn every_instrument_is_positive_and_continuous() {
        let all: Vec<&str> = tickers().collect();
        assert_eq!(all.len(), 13);
        for t in all {
            let limit = if t == "BTC" || t == "ETH" { 0.25 } else { 0.12 };
            check_series(t, limit, |d| price_on(t, d).expect("known").0);
        }
        check_series("usd", 0.12, usd_per_eur);
        check_series("dkk", 0.12, dkk_per_eur);
    }

    #[test]
    fn currencies_and_unknown_tickers() {
        let d = ts(2024, 3, 12);
        assert_eq!(price_on("VWCE.DE", d).map(|p| p.1), Some("EUR"));
        assert_eq!(price_on("MSFT", d).map(|p| p.1), Some("USD"));
        assert_eq!(price_on("NOVO-B.CO", d).map(|p| p.1), Some("DKK"));
        assert_eq!(price_on("BTC", d).map(|p| p.1), Some("USD"));
        assert!(price_on("NOPE", d).is_none());
    }

    #[test]
    fn deterministic_and_order_independent() {
        let d = ts(2023, 5, 17);
        let a = price_on("AAPL", d);
        let _ = price_on("MSFT", d);
        let _ = price_on("AAPL", d + DAY);
        assert_eq!(a, price_on("AAPL", d));
        assert_eq!(usd_per_eur(d).to_bits(), usd_per_eur(d).to_bits());
    }

    #[test]
    fn weekends_carry_fridays_value_except_crypto() {
        let fri = ts(2024, 5, 17);
        assert_eq!(price_on("MSFT", fri + DAY), price_on("MSFT", fri));
        assert_eq!(price_on("MSFT", fri + 2 * DAY), price_on("MSFT", fri));
        assert_ne!(price_on("BTC", fri + DAY), price_on("BTC", fri));
        assert_eq!(usd_per_eur(fri + DAY), usd_per_eur(fri));
    }

    #[test]
    fn anchors_are_close_to_the_table() {
        // Quarter ends can fall on a weekend; compare on a weekday anchor.
        let (p, _) = price_on("MSFT", ts(2024, 6, 28)).expect("known");
        assert!((p / 446.9 - 1.0).abs() < 0.02, "{p}");
        let (n, _) = price_on("NVDA", ts(2024, 6, 3)).expect("known");
        assert!(n > 100.0 && n < 140.0, "NVDA is split-adjusted, got {n}");
    }

    #[test]
    fn the_series_keeps_drifting_past_the_last_anchor() {
        let a = price_on("VWCE.DE", ts(2026, 1, 2)).expect("known").0;
        let b = price_on("VWCE.DE", ts(2030, 1, 2)).expect("known").0;
        assert!(b > a * 0.7 && b < a * 2.0);
    }

    #[test]
    fn every_series_has_a_value_back_to_2019() {
        // the earliest day a seed needs is 12 days before M60 (late 2020 for a `now` in late
        // 2025); before the first anchor the series holds its first value
        for day in [ts(2019, 1, 1), ts(2020, 6, 15), ts(2020, 12, 30)] {
            for t in tickers() {
                assert!(price_on(t, day).is_some_and(|(p, _)| p > 0.0), "{t}");
            }
            assert!(usd_per_eur(day) > 0.5 && dkk_per_eur(day) > 5.0);
        }
    }

    #[test]
    fn dividends_cover_the_payers_in_order() {
        for t in ["MSFT", "AAPL", "NVDA", "ASML.AS", "ALV.DE"] {
            let ev = dividends(t, ts(2032, 12, 31));
            assert!(ev.len() >= 7, "{t}");
            assert!(ev.windows(2).all(|w| w[0].0 < w[1].0), "{t} ordered");
            assert!(ev
                .iter()
                .all(|(d, a)| *a > 0.0 && (d.div_euclid(DAY) + 3).rem_euclid(7) < 5));
        }
        let end_2032 = ts(2032, 12, 31);
        assert_eq!(dividends("MSFT", end_2032).len(), 4 * 12);
        assert_eq!(dividends("ALV.DE", end_2032).len(), 12);
        assert!(dividends("PYPL", end_2032).is_empty());
        // no fixed horizon: any later year has its events
        let in_2040 = dividends("MSFT", ts(2040, 6, 30));
        assert_eq!(in_2040.len(), 4 * 20);
        assert!(in_2040.last().is_some_and(|(d, _)| *d > ts(2040, 11, 1)));
    }
}
