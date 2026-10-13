//! The price rule of the history rebuilds (no database, no network).
//!
//! The rebuild of a ticker's value history (`services/history_recalc.rs`) values each day at
//! "the price known on that day", which this module defines. The planning of which days a
//! rebuild covers lives in `services/history_plan.rs` and `services/history_days.rs`.

use crate::services::price_api::HistoricalPrice;

pub const SECONDS_PER_DAY: i64 = 86_400;

/// The price known as of the end of `day`'s UTC calendar day: the latest valid
/// price whose timestamp is on or before that day. Look-back only:
/// a Saturday carries Friday's close forward instead of borrowing Monday's, and
/// a day before the first known price has no price (`None`) instead of the
/// first future one.
///
/// Prices need not be sorted. Non-finite and non-positive prices are bad data
/// and are skipped, so the last *valid* earlier price carries forward.
pub fn price_at_or_before(prices: &[HistoricalPrice], day: i64) -> Option<&HistoricalPrice> {
    let day_end = day.div_euclid(SECONDS_PER_DAY) * SECONDS_PER_DAY + SECONDS_PER_DAY;
    prices
        .iter()
        .filter(|p| p.timestamp < day_end && p.price.is_finite() && p.price > 0.0)
        .max_by_key(|p| p.timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    fn at(y: i32, m: u32, d: u32, h: u32) -> i64 {
        Utc.with_ymd_and_hms(y, m, d, h, 30, 0)
            .single()
            .expect("valid time")
            .timestamp()
    }

    fn day(y: i32, m: u32, d: u32) -> i64 {
        at(y, m, d, 0) - 30 * 60
    }

    fn hp(timestamp: i64, price: f64) -> HistoricalPrice {
        HistoricalPrice {
            timestamp,
            price,
            currency: "USD".to_string(),
        }
    }

    /// Fri 25 Sep 100, Mon 28 Sep 110, Tue 29 Sep 120 (14:30 UTC market bars).
    fn week() -> Vec<HistoricalPrice> {
        vec![
            hp(at(2026, 9, 25, 14), 100.0),
            hp(at(2026, 9, 28, 14), 110.0),
            hp(at(2026, 9, 29, 14), 120.0),
        ]
    }

    // ---- price_at_or_before ---------------------------------------

    #[test]
    fn a_weekend_carries_fridays_close_forward_not_mondays() {
        let prices = week();
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 26)).unwrap().price,
            100.0
        );
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 27)).unwrap().price,
            100.0
        );
    }

    #[test]
    fn a_trading_day_uses_its_own_close() {
        let prices = week();
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 25)).unwrap().price,
            100.0
        );
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 28)).unwrap().price,
            110.0
        );
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 29)).unwrap().price,
            120.0
        );
    }

    #[test]
    fn days_after_the_last_price_carry_it_forward() {
        let prices = week();
        assert_eq!(
            price_at_or_before(&prices, day(2026, 10, 5)).unwrap().price,
            120.0
        );
    }

    #[test]
    fn days_before_the_first_price_are_unvalued() {
        let prices = week();
        assert!(price_at_or_before(&prices, day(2026, 9, 24)).is_none());
        assert!(price_at_or_before(&prices, day(2026, 1, 1)).is_none());
    }

    #[test]
    fn an_empty_history_has_no_price() {
        assert!(price_at_or_before(&[], day(2026, 9, 25)).is_none());
    }

    #[test]
    fn the_input_order_does_not_matter() {
        let mut prices = week();
        prices.reverse();
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 26)).unwrap().price,
            100.0
        );
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 28)).unwrap().price,
            110.0
        );
    }

    #[test]
    fn midnight_stamped_daily_prices_belong_to_their_own_day() {
        // CoinGecko prices are stamped with the UTC day start.
        let prices = vec![hp(day(2026, 9, 25), 100.0), hp(day(2026, 9, 26), 105.0)];
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 25)).unwrap().price,
            100.0
        );
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 26)).unwrap().price,
            105.0
        );
    }

    #[test]
    fn a_sunday_between_midnight_stamped_prices_takes_the_friday_not_the_monday_price() {
        // Nearest-in-either-direction picked Monday here (1 day away vs 2).
        let prices = vec![hp(day(2026, 9, 25), 100.0), hp(day(2026, 9, 28), 110.0)];
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 27)).unwrap().price,
            100.0
        );
    }

    #[test]
    fn bad_prices_are_skipped_and_the_last_valid_one_carries_forward() {
        let prices = vec![
            hp(at(2026, 9, 25, 14), 100.0),
            hp(at(2026, 9, 28, 14), 0.0),
            hp(at(2026, 9, 29, 14), f64::NAN),
        ];
        assert_eq!(
            price_at_or_before(&prices, day(2026, 9, 29)).unwrap().price,
            100.0
        );
    }
}
