//! UTC-midnight day helpers, all relative to the `now` the generator is given.
//!
//! A "day" is the unix timestamp (seconds) of a UTC midnight, the form every day-granular
//! column in the database uses.

use chrono::{DateTime, Datelike, NaiveDate};

/// Seconds in a day.
pub const DAY: i64 = 86_400;

/// Calendar date of a unix timestamp (UTC). Out-of-range input maps to the epoch.
pub fn date_of(ts: i64) -> NaiveDate {
    DateTime::from_timestamp(ts, 0)
        .map(|d| d.date_naive())
        .unwrap_or_default()
}

/// UTC midnight of a calendar date.
pub fn day_of(date: NaiveDate) -> i64 {
    date.and_hms_opt(0, 0, 0)
        .map(|d| d.and_utc().timestamp())
        .unwrap_or(0)
}

/// Number of days in a month (`month` 1..=12).
pub fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    };
    match (
        NaiveDate::from_ymd_opt(year, month, 1),
        NaiveDate::from_ymd_opt(ny, nm, 1),
    ) {
        (Some(a), Some(b)) => (b - a).num_days() as u32,
        _ => 30,
    }
}

/// Today, as UTC midnight.
pub fn today(now: i64) -> i64 {
    now.div_euclid(DAY) * DAY
}

/// `n` days before today.
pub fn days_ago(now: i64, n: i64) -> i64 {
    today(now) - n * DAY
}

/// First day of the month `n` calendar months before the month of `now`.
fn month_start_back(now: i64, n: i32) -> NaiveDate {
    let d = date_of(today(now));
    let total = d.year() * 12 + d.month0() as i32 - n;
    let (y, m0) = (total.div_euclid(12), total.rem_euclid(12) as u32);
    NaiveDate::from_ymd_opt(y, m0 + 1, 1).unwrap_or(d)
}

/// The given day of the month, `n` calendar months before the month of `now`; the day is
/// clamped to the length of that month (31 in February becomes the 28th or 29th).
pub fn months_ago(now: i64, n: i32, day_of_month: u32) -> i64 {
    let first = month_start_back(now, n);
    let dom = day_of_month.clamp(1, days_in_month(first.year(), first.month()));
    day_of(first.with_day(dom).unwrap_or(first))
}

/// First day of each of the last `count` calendar months, oldest first, ending with the
/// month of `now`.
pub fn last_months(now: i64, count: u32) -> impl Iterator<Item = i64> {
    (0..count as i32)
        .rev()
        .map(move |n| day_of(month_start_back(now, n)))
}

/// Last day of the month that contains `day`.
pub fn month_end(day: i64) -> i64 {
    let d = date_of(day);
    let last = days_in_month(d.year(), d.month());
    day_of(d.with_day(last).unwrap_or(d))
}

/// First day of the month that contains `day`.
pub fn month_start(day: i64) -> i64 {
    let d = date_of(day);
    day_of(d.with_day(1).unwrap_or(d))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(y: i32, m: u32, d: u32) -> i64 {
        day_of(NaiveDate::from_ymd_opt(y, m, d).expect("date"))
    }

    #[test]
    fn today_floors_to_utc_midnight() {
        let now = ts(2026, 11, 23) + 15 * 3600 + 42;
        assert_eq!(today(now), ts(2026, 11, 23));
        assert_eq!(days_ago(now, 10), ts(2026, 11, 13));
    }

    #[test]
    fn months_ago_clamps_to_month_length() {
        let now = ts(2026, 3, 31) + 100;
        assert_eq!(months_ago(now, 1, 31), ts(2026, 2, 28));
        assert_eq!(months_ago(now, 0, 31), ts(2026, 3, 31));
        assert_eq!(months_ago(ts(2024, 3, 15), 1, 30), ts(2024, 2, 29));
        assert_eq!(months_ago(now, 2, 31), ts(2026, 1, 31));
        assert_eq!(months_ago(now, 4, 31), ts(2025, 11, 30));
    }

    #[test]
    fn months_ago_crosses_year_boundaries() {
        let now = ts(2026, 1, 10);
        assert_eq!(months_ago(now, 1, 25), ts(2025, 12, 25));
        assert_eq!(months_ago(now, 13, 5), ts(2024, 12, 5));
        assert_eq!(months_ago(now, 60, 1), ts(2021, 1, 1));
    }

    #[test]
    fn last_months_are_first_days_oldest_first() {
        let v: Vec<i64> = last_months(ts(2026, 2, 17), 4).collect();
        assert_eq!(
            v,
            vec![
                ts(2025, 11, 1),
                ts(2025, 12, 1),
                ts(2026, 1, 1),
                ts(2026, 2, 1)
            ]
        );
        assert_eq!(last_months(ts(2026, 2, 17), 0).count(), 0);
    }

    #[test]
    fn month_end_handles_lengths_and_leap_years() {
        assert_eq!(month_end(ts(2025, 2, 3)), ts(2025, 2, 28));
        assert_eq!(month_end(ts(2024, 2, 3)), ts(2024, 2, 29));
        assert_eq!(month_end(ts(2025, 12, 31)), ts(2025, 12, 31));
        assert_eq!(month_end(ts(2025, 4, 1)), ts(2025, 4, 30));
        assert_eq!(month_start(ts(2025, 4, 17)), ts(2025, 4, 1));
    }
}
