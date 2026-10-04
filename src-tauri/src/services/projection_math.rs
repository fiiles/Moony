//! Pure math for the portfolio projection (no database, no Tauri state).
//!
//! The projection command (`commands/projection.rs`) only loads inputs and
//! delegates here, so every formula is unit-testable with plain numbers.

use chrono::{DateTime, Months, Utc};

/// Future value of a stream of equal monthly contributions.
///
/// Monthly compounding, contribution paid at the end of each month:
/// `PMT * ((1 + r/12)^m - 1) / (r/12)` with `r` the nominal annual rate as a
/// ratio (0.07 for 7 %) and `m` the number of months. A rate of exactly zero
/// degenerates to the plain sum `PMT * m`.
///
/// the earlier `((1 + r)^years - 1) / (r/12)` form is an annuity of
/// *annual* payments at year end, so the first year's contributions earned
/// nothing (1 000/month at 7 % gave 12 000 instead of 12 392.59 after a year).
pub fn contribution_future_value(monthly_contribution: f64, annual_rate: f64, months: i32) -> f64 {
    if months <= 0 {
        return 0.0;
    }
    if annual_rate == 0.0 {
        return monthly_contribution * months as f64;
    }
    let monthly_rate = annual_rate / 12.0;
    let growth = (1.0 + monthly_rate).max(0.0).powi(months);
    monthly_contribution * (growth - 1.0) / monthly_rate
}

/// One growth-rate setting as stored (TEXT) or sent by the frontend.
///
/// `Some(rate)` (in percent) is an explicit choice and is honoured, including
/// an explicit `0`; `None` means "not set" (empty or unusable text) and the
/// caller falls back to the class default (the weighted rate of the user's own
/// accounts or bonds, or the documented constant). Historically the old
/// `if rate > 0.0` check made an explicit 0 % for savings or bonds impossible.
pub fn parse_rate_setting(raw: &str) -> Option<f64> {
    raw.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// Date of projection point number `period` (1-based), counted from `now`.
///
/// Calendar arithmetic in UTC: a monthly point is `period` calendar months
/// ahead, a yearly point `period` calendar years ahead, and a day that does
/// not exist in the target month (31 Jan + 1 month) is clamped to the last
/// valid day. Adding `30 * period` days drifted (12 monthly points
/// ended 5 days before the one-year mark; yearly points could repeat a year).
pub fn projection_date(now: DateTime<Utc>, period: i32, monthly: bool) -> DateTime<Utc> {
    let months = if monthly {
        period.max(0) as u32
    } else {
        period.max(0) as u32 * 12
    };
    now.checked_add_months(Months::new(months)).unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn one_year_of_1000_a_month_at_7_percent_compounds_monthly() {
        let fv = contribution_future_value(1000.0, 0.07, 12);
        assert!((fv - 12_392.59).abs() < 0.01, "got {fv}");
    }

    #[test]
    fn ten_years_of_1000_a_month_at_7_percent() {
        let fv = contribution_future_value(1000.0, 0.07, 120);
        assert!((fv - 173_085.0).abs() < 1.0, "got {fv}");
    }

    #[test]
    fn zero_rate_is_the_plain_sum() {
        assert_eq!(contribution_future_value(1000.0, 0.0, 12), 12_000.0);
    }

    #[test]
    fn zero_months_is_zero() {
        assert_eq!(contribution_future_value(1000.0, 0.07, 0), 0.0);
    }

    #[test]
    fn the_first_year_earns_interest() {
        // The annual-annuity form returned exactly 12 000 here.
        let fv = contribution_future_value(1000.0, 0.07, 12);
        assert!(
            fv > 12_300.0,
            "first-year contributions must earn interest, got {fv}"
        );
    }

    // ---- calendar months, explicit 0 % ----------------------------

    fn utc(y: i32, m: u32, d: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(y, m, d, 12, 0, 0)
            .single()
            .expect("valid date")
    }

    #[test]
    fn twelve_monthly_points_land_on_the_same_day_next_year() {
        // 12 x 30 days = 360 days drifted five days short of a year.
        let now = utc(2026, 1, 15);
        assert_eq!(projection_date(now, 12, true), utc(2027, 1, 15));
    }

    #[test]
    fn monthly_points_follow_the_calendar() {
        let now = utc(2026, 1, 15);
        assert_eq!(projection_date(now, 1, true), utc(2026, 2, 15));
        assert_eq!(projection_date(now, 2, true), utc(2026, 3, 15));
        assert_eq!(projection_date(now, 6, true), utc(2026, 7, 15));
    }

    #[test]
    fn month_end_is_clamped_to_the_last_valid_day() {
        let now = utc(2026, 1, 31);
        assert_eq!(projection_date(now, 1, true), utc(2026, 2, 28));
        assert_eq!(projection_date(now, 2, true), utc(2026, 3, 31));
        // leap year
        assert_eq!(
            projection_date(utc(2027, 12, 31), 2, true),
            utc(2028, 2, 29)
        );
    }

    #[test]
    fn yearly_points_are_calendar_years_across_leap_years() {
        // 365 * 2 days from 1 Jan 2027 lands on 31 Dec 2028 (2028 is leap):
        // the yearly chart would label 2028 twice.
        let now = utc(2027, 1, 1);
        assert_eq!(projection_date(now, 1, false), utc(2028, 1, 1));
        assert_eq!(projection_date(now, 2, false), utc(2029, 1, 1));
        assert_eq!(
            projection_date(utc(2028, 2, 29), 1, false),
            utc(2029, 2, 28)
        );
    }

    #[test]
    fn an_explicit_zero_rate_is_some_zero() {
        assert_eq!(parse_rate_setting("0"), Some(0.0));
        assert_eq!(parse_rate_setting("0.00"), Some(0.0));
        assert_eq!(parse_rate_setting(" 3.5 "), Some(3.5));
        assert_eq!(parse_rate_setting("-2"), Some(-2.0));
    }

    #[test]
    fn a_missing_or_unusable_rate_is_none() {
        assert_eq!(parse_rate_setting(""), None);
        assert_eq!(parse_rate_setting("   "), None);
        assert_eq!(parse_rate_setting("abc"), None);
        assert_eq!(parse_rate_setting("NaN"), None);
        assert_eq!(parse_rate_setting("inf"), None);
    }

    #[test]
    fn negative_rate_shrinks_the_stream_below_the_plain_sum() {
        let fv = contribution_future_value(1000.0, -0.05, 12);
        assert!(fv < 12_000.0 && fv > 11_000.0, "got {fv}");
    }
}
