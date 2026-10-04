//! Pure planning and valuation for the snapshot backfill (no database, no network).
//!
//! `commands/portfolio.rs::backfill_missing_snapshots` fetches prices and writes
//! rows; the decisions live here so they are unit-testable with tiny fixtures:
//!
//! * which missing days one run closes,
//! * which price counts as "known on a day",
//! * how the held positions of one asset class are valued on a day.

use crate::services::price_api::HistoricalPrice;
use std::collections::{HashMap, HashSet};

pub const SECONDS_PER_DAY: i64 = 86_400;

/// Most missing days one backfill run closes (newest first); older gaps stay
/// missing in the database and are picked up by the next run.
///
/// The former cap of 30 days existed "to avoid too many API calls", but one
/// price fetch covers the whole span whatever the number of days (the calls
/// scale with the number of tickers, not days). The real limits are CoinGecko's
/// 365-day history on the free tier and the time spent writing rows, hence 365.
/// Nothing new is persisted to track progress: a gap is simply a day without a
/// snapshot row, so every start continues where the previous run stopped
///.
pub const BACKFILL_MAX_DAYS_PER_RUN: usize = 365;

/// Days of price history fetched before the first day to fill, so that the
/// first reconstructed day can carry forward the last close before it (a long
/// weekend plus a holiday is at most a handful of days).
pub const PRICE_LEAD_IN_DAYS: i64 = 14;

/// The days one backfill run will fill, and how many it leaves for later runs.
#[derive(Debug, PartialEq, Eq)]
pub struct BackfillPlan {
    /// UTC day starts to fill now, ascending (the newest missing days, at most `max_days`).
    pub days: Vec<i64>,
    /// Missing days left over for later runs (all older than `days`).
    pub deferred: usize,
}

/// Plan a run: every day after `oldest_day` up to and including `last_day`
/// that has no snapshot in `existing_days`, newest `max_days` of them.
pub fn plan_backfill(
    oldest_day: i64,
    last_day: i64,
    existing_days: &HashSet<i64>,
    max_days: usize,
) -> BackfillPlan {
    let mut missing: Vec<i64> = Vec::new();
    let mut day = oldest_day + SECONDS_PER_DAY;
    while day <= last_day {
        if !existing_days.contains(&day) {
            missing.push(day);
        }
        day += SECONDS_PER_DAY;
    }

    let deferred = missing.len().saturating_sub(max_days);
    BackfillPlan {
        days: missing.split_off(deferred),
        deferred,
    }
}

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

/// Native value of held positions on `day`, per currency, from price histories.
///
/// `positions` are `(ticker, quantity)`; positions with a quantity of zero or
/// less are not held and are ignored. Returns `None` when a held position has
/// no known price on that day: the day cannot be valued, and writing a total
/// that silently leaves the position out would persist a wrong (too low) row
/// that is never revisited. The caller leaves such a day empty so a later run
/// can try again.
pub fn value_positions_on_day(
    positions: &[(String, f64)],
    prices: &HashMap<String, Vec<HistoricalPrice>>,
    day: i64,
) -> Option<HashMap<String, f64>> {
    let mut by_currency: HashMap<String, f64> = HashMap::new();
    for (ticker, quantity) in positions {
        if *quantity <= 0.0 {
            continue;
        }
        let price = prices
            .get(ticker)
            .and_then(|history| price_at_or_before(history, day))?;
        *by_currency.entry(price.currency.clone()).or_insert(0.0) += price.price * quantity;
    }
    Some(by_currency)
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

    // ---- value_positions_on_day ------------------------------------

    fn histories() -> HashMap<String, Vec<HistoricalPrice>> {
        let mut m = HashMap::new();
        m.insert("AAA".to_string(), week());
        m.insert(
            "BBB".to_string(),
            vec![HistoricalPrice {
                timestamp: at(2026, 9, 28, 14),
                price: 5.0,
                currency: "EUR".to_string(),
            }],
        );
        m
    }

    #[test]
    fn positions_are_valued_per_currency_at_the_known_price() {
        let positions = vec![("AAA".to_string(), 2.0), ("BBB".to_string(), 10.0)];
        let v = value_positions_on_day(&positions, &histories(), day(2026, 9, 29)).unwrap();
        assert_eq!(v["USD"], 240.0);
        assert_eq!(v["EUR"], 50.0);
    }

    #[test]
    fn a_held_position_without_a_price_makes_the_day_unvalued() {
        // BBB has no price before Mon 28 Sep: do not write a total without it.
        let positions = vec![("AAA".to_string(), 2.0), ("BBB".to_string(), 10.0)];
        assert!(value_positions_on_day(&positions, &histories(), day(2026, 9, 26)).is_none());
    }

    #[test]
    fn a_ticker_with_no_history_at_all_makes_the_day_unvalued() {
        let positions = vec![("ZZZ".to_string(), 1.0)];
        assert!(value_positions_on_day(&positions, &histories(), day(2026, 9, 29)).is_none());
    }

    #[test]
    fn positions_that_are_not_held_are_ignored() {
        let positions = vec![("AAA".to_string(), 2.0), ("ZZZ".to_string(), 0.0)];
        let v = value_positions_on_day(&positions, &histories(), day(2026, 9, 29)).unwrap();
        assert_eq!(v["USD"], 240.0);
        assert_eq!(v.len(), 1);
    }

    #[test]
    fn no_positions_is_an_empty_valuation() {
        let v = value_positions_on_day(&[], &histories(), day(2026, 9, 29)).unwrap();
        assert!(v.is_empty());
    }

    // ---- plan_backfill ---------------------------------------------

    fn days_from(start: i64, n: i64) -> Vec<i64> {
        (0..n).map(|i| start + i * SECONDS_PER_DAY).collect()
    }

    #[test]
    fn a_60_day_gap_is_closed_in_one_run() {
        // Snapshots exist on day 0 and day 61; days 1..=60 are missing.
        let d0 = day(2026, 1, 1);
        let last = d0 + 61 * SECONDS_PER_DAY;
        let existing: HashSet<i64> = [d0, last].into_iter().collect();

        let plan = plan_backfill(d0, last, &existing, BACKFILL_MAX_DAYS_PER_RUN);

        assert_eq!(plan.days.len(), 60);
        assert_eq!(plan.days, days_from(d0 + SECONDS_PER_DAY, 60));
        assert_eq!(plan.deferred, 0);
    }

    #[test]
    fn the_old_30_day_cap_is_gone() {
        let d0 = day(2026, 1, 1);
        let last = d0 + 61 * SECONDS_PER_DAY;
        let existing: HashSet<i64> = [d0].into_iter().collect();
        let plan = plan_backfill(d0, last, &existing, BACKFILL_MAX_DAYS_PER_RUN);
        assert!(plan.days.len() > 30);
    }

    #[test]
    fn a_gap_beyond_the_cap_closes_over_consecutive_runs() {
        let d0 = day(2025, 1, 1);
        let last = d0 + 401 * SECONDS_PER_DAY;
        let mut existing: HashSet<i64> = [d0].into_iter().collect();

        // Run 1: the newest 365 days; 36 older days are left.
        let run1 = plan_backfill(d0, last, &existing, BACKFILL_MAX_DAYS_PER_RUN);
        assert_eq!(run1.days.len(), 365);
        assert_eq!(*run1.days.last().unwrap(), last);
        assert_eq!(run1.deferred, 36);
        existing.extend(run1.days.iter().copied());

        // Run 2 (next start): the rest, nothing deferred.
        let run2 = plan_backfill(d0, last, &existing, BACKFILL_MAX_DAYS_PER_RUN);
        assert_eq!(run2.days, days_from(d0 + SECONDS_PER_DAY, 36));
        assert_eq!(run2.deferred, 0);
        existing.extend(run2.days.iter().copied());

        // Run 3: nothing left.
        let run3 = plan_backfill(d0, last, &existing, BACKFILL_MAX_DAYS_PER_RUN);
        assert!(run3.days.is_empty());
        assert_eq!(run3.deferred, 0);
    }

    #[test]
    fn existing_days_are_not_planned_again() {
        let d0 = day(2026, 1, 1);
        let last = d0 + 5 * SECONDS_PER_DAY;
        let existing: HashSet<i64> = [d0, d0 + 2 * SECONDS_PER_DAY, last].into_iter().collect();
        let plan = plan_backfill(d0, last, &existing, BACKFILL_MAX_DAYS_PER_RUN);
        assert_eq!(
            plan.days,
            vec![
                d0 + SECONDS_PER_DAY,
                d0 + 3 * SECONDS_PER_DAY,
                d0 + 4 * SECONDS_PER_DAY
            ]
        );
    }

    #[test]
    fn no_gap_means_an_empty_plan() {
        let d0 = day(2026, 1, 1);
        let existing: HashSet<i64> = days_from(d0, 4).into_iter().collect();
        let plan = plan_backfill(d0, d0 + 3 * SECONDS_PER_DAY, &existing, 365);
        assert!(plan.days.is_empty());
        assert_eq!(plan.deferred, 0);
    }
}
