//! Built-in prices, FX rates and dividends for the demo instruments.
//!
//! Real data from the app's own providers, through [`data_through`]: daily closes
//! (split-adjusted, in the quote currency; crypto in USD), per-share dividends and ECB rates,
//! generated into `prices_data.rs` by `cargo test --lib refresh_demo_prices -- --ignored`.
//! A day is valued as the app's history rebuild values it: the last real bar on or before it
//! (`backfill::price_at_or_before`), so weekends and holidays carry the previous close, and a
//! day before a series starts takes its first close.
//!
//! After the data ends the series continue from the last real close with a fixed gentle annual
//! drift, a bounded slow wave and a small day-keyed wiggle (a hash of ticker and day, never the
//! shared `Rng`, so a value never depends on call order); dividends repeat the last real year
//! with a yearly growth. So any future "today" works, and a refresh of the data replaces the
//! invented tail.

use super::clock::{day_of, DAY};
use super::prices_data::{DATA_THROUGH, DIVIDENDS, RATES, SERIES};
use chrono::{Datelike, NaiveDate};
use std::collections::HashMap;
use std::sync::OnceLock;

/// How a series' closes are stored: `f32` for Yahoo's single-precision closes, `f64` for the
/// four-decimal ECB rates.
pub enum Values {
    F32(&'static [f32]),
    F64(&'static [f64]),
}

impl Values {
    fn to_vec(&self) -> Vec<f64> {
        match self {
            Values::F32(v) => v.iter().map(|x| f64::from(*x)).collect(),
            Values::F64(v) => v.to_vec(),
        }
    }
}

/// One real daily series of `prices_data.rs`: the closes of its bars, and the bar days as a
/// calendar from `first_day` (every day when `daily`, else weekdays) without the `missing` and
/// with the `extra` day offsets.
pub struct Series {
    pub name: &'static str,
    pub currency: &'static str,
    pub first_day: i64,
    pub daily: bool,
    pub missing: &'static [u16],
    pub extra: &'static [u16],
    pub values: Values,
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Equity,
    Crypto,
    Fx,
}

impl Kind {
    /// Annual drift, wiggle and wave of the invented tail.
    fn tail(self) -> (f64, f64, f64) {
        match self {
            Kind::Equity => (0.06, 0.005, 0.03),
            Kind::Crypto => (0.10, 0.015, 0.08),
            Kind::Fx => (0.0, 0.0015, 0.02),
        }
    }
}

/// A decoded series.
struct Decoded {
    currency: &'static str,
    kind: Kind,
    key: u64,
    days: Vec<i64>,
    values: Vec<f64>,
}

fn is_weekday(day: i64) -> bool {
    (day.div_euclid(DAY) + 3).rem_euclid(7) < 5
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

/// The bar days of an encoded series (see [`Series`]), `count` of them.
pub fn bar_days_of(
    first_day: i64,
    daily: bool,
    missing: &[u16],
    extra: &[u16],
    count: usize,
) -> Vec<i64> {
    let mut out = Vec::with_capacity(count);
    let (mut m, mut e) = (0, 0);
    let mut offset: u16 = 0;
    while out.len() < count {
        let day = first_day + i64::from(offset) * DAY;
        let is_missing = missing.get(m) == Some(&offset);
        if is_missing {
            m += 1;
        }
        let is_extra = extra.get(e) == Some(&offset);
        if is_extra {
            e += 1;
        }
        if ((daily || is_weekday(day)) && !is_missing) || is_extra {
            out.push(day);
        }
        offset = match offset.checked_add(1) {
            Some(next) => next,
            None => break,
        };
    }
    out
}

fn decoded() -> &'static HashMap<&'static str, Decoded> {
    static ALL: OnceLock<HashMap<&'static str, Decoded>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut all = HashMap::new();
        for (list, fx) in [(SERIES, false), (RATES, true)] {
            for s in list {
                let values = s.values.to_vec();
                let kind = if fx {
                    Kind::Fx
                } else if s.name == "BTC" || s.name == "ETH" {
                    Kind::Crypto
                } else {
                    Kind::Equity
                };
                all.insert(
                    s.name,
                    Decoded {
                        currency: s.currency,
                        kind,
                        key: fnv(s.name) ^ u64::from(fx),
                        days: bar_days_of(s.first_day, s.daily, s.missing, s.extra, values.len()),
                        values,
                    },
                );
            }
        }
        all
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

impl Decoded {
    /// Index of the last real bar on or before `day`.
    fn bar_index(&self, day: i64) -> Option<usize> {
        self.days.partition_point(|d| *d <= day).checked_sub(1)
    }

    /// The real close of `day` (the first close before the series starts).
    fn real(&self, day: i64) -> f64 {
        let i = self.bar_index(day).unwrap_or(0);
        self.values.get(i).copied().unwrap_or(1.0)
    }

    fn value(&self, day: i64) -> f64 {
        if day <= DATA_THROUGH {
            return self.real(day);
        }
        let last = self.real(DATA_THROUGH);
        let d = if self.kind == Kind::Crypto {
            day
        } else {
            weekday_on_or_before(day)
        };
        if d <= DATA_THROUGH {
            return last;
        }
        let (drift, wiggle, wave) = self.kind.tail();
        let years = (d - DATA_THROUGH) as f64 / (365.25 * DAY as f64);
        let trend = (1.0 + drift).ln() * years
            + wave * (2.0 * std::f64::consts::PI * years / 0.9).sin() * unit(self.key, 7).signum();
        last * (trend + unit(self.key, d as u64) * wiggle).exp()
    }
}

/// UTC midnight of the last day the built-in real data covers.
pub fn data_through() -> i64 {
    DATA_THROUGH
}

/// Close (or crypto spot) of `ticker` on `day` (UTC midnight) and its native currency: the last
/// real bar on or before the day. `None` for tickers outside the demo set.
pub fn price_on(ticker: &str, day: i64) -> Option<(f64, &'static str)> {
    let series = decoded().get(ticker).filter(|s| s.kind != Kind::Fx)?;
    Some((series.value(day), series.currency))
}

/// The day of the bar that values a trade on `day`: the last real trading day on or before it
/// (a weekend or a market holiday moves to the session before); after the data ends, Friday
/// for a weekend (crypto trades every day). `day` itself for unknown tickers.
pub fn trading_day(ticker: &str, day: i64) -> i64 {
    let Some(series) = decoded().get(ticker) else {
        return day;
    };
    if day > DATA_THROUGH {
        return if series.kind == Kind::Crypto {
            day
        } else {
            weekday_on_or_before(day)
        };
    }
    series
        .bar_index(day)
        .and_then(|i| series.days.get(i).copied())
        .unwrap_or(day)
}

/// Every ticker `price_on` knows.
pub fn tickers() -> impl Iterator<Item = &'static str> {
    SERIES.iter().map(|s| s.name)
}

fn rate(code: &str, day: i64) -> f64 {
    decoded().get(code).map_or(1.0, |s| s.value(day))
}

/// USD per 1 EUR on `day`.
pub fn usd_per_eur(day: i64) -> f64 {
    rate("USD", day)
}

/// DKK per 1 EUR on `day`.
pub fn dkk_per_eur(day: i64) -> f64 {
    rate("DKK", day)
}

/// Whether the ECB published a rate of `code` on `day` (after the data ends: every weekday).
pub fn rate_published(code: &str, day: i64) -> bool {
    if day > DATA_THROUGH {
        return is_weekday(day);
    }
    decoded()
        .get(code)
        .is_some_and(|s| s.days.binary_search(&day).is_ok())
}

/// Yearly growth of the dividends after the real data.
const DIVIDEND_GROWTH: [(&str, f64); 5] = [
    ("MSFT", 0.10),
    ("AAPL", 0.04),
    ("NVDA", 0.0),
    ("ASML.AS", 0.05),
    ("ALV.DE", 0.05),
];

/// Per-share dividend events `(ex-day at UTC midnight, amount in the stock's currency,
/// today's units)` for the demo payers, oldest first: the real ones through the data end, then
/// the last real year repeated every year (ex-days moved off weekends, amounts grown) through
/// the year of `until`. Empty for other tickers.
pub fn dividends(ticker: &str, until: i64) -> Vec<(i64, f64)> {
    let Some((_, real)) = DIVIDENDS.iter().find(|(t, _)| *t == ticker) else {
        return Vec::new();
    };
    let growth = DIVIDEND_GROWTH
        .iter()
        .find(|(t, _)| *t == ticker)
        .map_or(0.0, |(_, g)| *g);
    let mut out: Vec<(i64, f64)> = real.to_vec();
    let last_year: Vec<(i64, f64)> = real
        .iter()
        .filter(|(d, _)| *d > DATA_THROUGH - 365 * DAY)
        .copied()
        .collect();
    let until_year = super::clock::date_of(until).year();
    for k in 1.. {
        let mut any = false;
        for (day, amount) in &last_year {
            let date = super::clock::date_of(*day);
            let year = date.year() + k;
            if year > until_year {
                continue;
            }
            any = true;
            let dom = date
                .day()
                .min(super::clock::days_in_month(year, date.month()));
            if let Some(next) = NaiveDate::from_ymd_opt(year, date.month(), dom) {
                out.push((
                    weekday_on_or_before(day_of(next)),
                    amount * (1.0 + growth).powi(k),
                ));
            }
        }
        if !any {
            break;
        }
    }
    out.sort_by_key(|e| e.0);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i32, m: u32, d: u32) -> i64 {
        day_of(NaiveDate::from_ymd_opt(y, m, d).expect("date"))
    }

    fn year_of(day: i64) -> i32 {
        super::super::clock::date_of(day).year()
    }

    /// Positive every day from 2019 to 2032; no day-to-day jump above `max_jump` from the
    /// last real day on (the real days move as the market did).
    fn check_series(label: &str, max_jump: f64, f: impl Fn(i64) -> f64) {
        let (start, end) = (ts(2019, 1, 1), ts(2032, 12, 31));
        let tail = data_through();
        let mut prev = f(start);
        let mut day = start;
        while day <= end {
            let v = f(day);
            assert!(v.is_finite() && v > 0.0, "{label} not positive on {day}");
            if day > tail {
                let jump = (v / prev - 1.0).abs();
                assert!(jump < max_jump, "{label} jumps {jump:.3} on day {day}");
            }
            prev = v;
            day += DAY;
        }
    }

    #[test]
    fn every_instrument_is_positive_and_continuous_past_the_data() {
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
    fn the_tail_starts_at_the_last_real_close() {
        let end = data_through();
        for t in tickers() {
            let (last, _) = price_on(t, end).expect("known");
            let (next, _) = price_on(t, end + DAY).expect("known");
            let limit = if t == "BTC" || t == "ETH" {
                0.02
            } else {
                0.006
            };
            assert!((next / last - 1.0).abs() < limit, "{t}: {last} -> {next}");
        }
    }

    #[test]
    fn currencies_and_unknown_tickers() {
        let d = ts(2024, 3, 12);
        assert_eq!(price_on("VWCE.DE", d).map(|p| p.1), Some("EUR"));
        assert_eq!(price_on("MSFT", d).map(|p| p.1), Some("USD"));
        assert_eq!(price_on("NOVO-B.CO", d).map(|p| p.1), Some("DKK"));
        assert_eq!(price_on("BTC", d).map(|p| p.1), Some("USD"));
        assert!(price_on("NOPE", d).is_none());
        assert!(price_on("USD", d).is_none(), "rates are not tickers");
    }

    #[test]
    fn deterministic_and_order_independent() {
        let d = ts(2023, 5, 17);
        let a = price_on("AAPL", d);
        let _ = price_on("MSFT", d);
        let _ = price_on("AAPL", d + DAY);
        assert_eq!(a, price_on("AAPL", d));
        let later = data_through() + 400 * DAY;
        assert_eq!(price_on("AAPL", later), price_on("AAPL", later));
        assert_eq!(usd_per_eur(d).to_bits(), usd_per_eur(d).to_bits());
    }

    #[test]
    fn weekends_and_holidays_carry_the_last_close_except_crypto() {
        let fri = ts(2024, 5, 17);
        assert_eq!(price_on("MSFT", fri + DAY), price_on("MSFT", fri));
        assert_eq!(price_on("MSFT", fri + 2 * DAY), price_on("MSFT", fri));
        assert_ne!(price_on("BTC", fri + DAY), price_on("BTC", fri));
        assert_eq!(usd_per_eur(fri + DAY), usd_per_eur(fri));
        // Christmas 2024 (a Wednesday): no session, a trade moves to the 24th
        let xmas = ts(2024, 12, 25);
        assert_eq!(price_on("MSFT", xmas), price_on("MSFT", xmas - DAY));
        assert_eq!(trading_day("MSFT", xmas), xmas - DAY);
        assert_eq!(trading_day("BTC", xmas), xmas);
        assert_eq!(trading_day("MSFT", fri + 2 * DAY), fri);
        assert!(!rate_published("USD", xmas) && rate_published("USD", fri));
    }

    #[test]
    fn real_closes_match_the_record() {
        // Well-known closes, split-adjusted: MSFT 2024-06-28 446.95, NVDA on its split day
        // 2024-06-10 121.79, BTC on 2024-03-13 about 73 100
        let (msft, _) = price_on("MSFT", ts(2024, 6, 28)).expect("known");
        assert!((msft / 446.95 - 1.0).abs() < 0.01, "{msft}");
        let (nvda, _) = price_on("NVDA", ts(2024, 6, 10)).expect("known");
        assert!((nvda / 121.79 - 1.0).abs() < 0.01, "{nvda}");
        let (btc, _) = price_on("BTC", ts(2024, 3, 13)).expect("known");
        assert!((btc / 73_100.0 - 1.0).abs() < 0.03, "{btc}");
        // a value at a fixed date equals the table entry
        let first = &SERIES[0];
        let (v, _) = price_on(first.name, first.first_day).expect("known");
        let table = match &first.values {
            Values::F32(v) => f64::from(v[0]),
            Values::F64(v) => v[0],
        };
        assert_eq!(v.to_bits(), table.to_bits());
    }

    #[test]
    fn the_series_keeps_drifting_past_the_data() {
        let a = price_on("VWCE.DE", data_through()).expect("known").0;
        let b = price_on("VWCE.DE", data_through() + 4 * 365 * DAY)
            .expect("known")
            .0;
        assert!(b > a * 0.7 && b < a * 2.0);
    }

    #[test]
    fn every_series_has_a_value_back_to_2019() {
        // VWCE.DE was listed in July 2019: before that it holds its first close
        let vwce = price_on("VWCE.DE", ts(2019, 1, 1)).expect("known").0;
        assert_eq!(
            Some(vwce),
            price_on("VWCE.DE", ts(2019, 7, 29)).map(|p| p.0)
        );
        for day in [ts(2019, 1, 1), ts(2020, 6, 15), ts(2020, 12, 30)] {
            for t in tickers() {
                assert!(price_on(t, day).is_some_and(|(p, _)| p > 0.0), "{t}");
            }
            assert!(usd_per_eur(day) > 0.5 && dkk_per_eur(day) > 5.0);
        }
    }

    #[test]
    fn dividends_are_real_then_continue_every_year() {
        let end_2032 = ts(2032, 12, 31);
        for t in ["MSFT", "AAPL", "NVDA", "ASML.AS", "ALV.DE"] {
            let ev = dividends(t, end_2032);
            assert!(ev.len() >= 7, "{t}");
            assert!(ev.windows(2).all(|w| w[0].0 < w[1].0), "{t} ordered");
            assert!(ev.iter().all(|(d, a)| *a > 0.0 && is_weekday(*d)), "{t}");
        }
        let per_year = |t: &str, y: i32| {
            dividends(t, end_2032)
                .iter()
                .filter(|(d, _)| year_of(*d) == y)
                .count()
        };
        for y in 2027..=2032 {
            assert_eq!(per_year("MSFT", y), 4, "MSFT {y}");
            assert_eq!(per_year("ALV.DE", y), 1, "ALV.DE {y}");
        }
        assert!(dividends("PYPL", end_2032).is_empty());
        // no fixed horizon: any later year has its events
        let in_2040 = dividends("MSFT", ts(2040, 6, 30));
        assert_eq!(
            in_2040.iter().filter(|(d, _)| year_of(*d) == 2040).count(),
            4
        );
    }
}
