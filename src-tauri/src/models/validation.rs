//! Shared validation for financial inputs and calendar dates.

use crate::error::{AppError, Result};
use chrono::Datelike;

/// Parse a financial number without allowing NaN, infinity, or overflow.
pub fn finite_number(value: &str, key: &str) -> Result<f64> {
    let number = value
        .parse::<f64>()
        .map_err(|_| AppError::Validation(key.to_string()))?;
    if !number.is_finite() {
        return Err(AppError::Validation(key.to_string()));
    }
    Ok(number)
}

/// Validate a representable calendar date and return its UTC midnight epoch.
pub fn utc_day(timestamp: i64, key: &str) -> Result<i64> {
    let date = chrono::DateTime::from_timestamp(timestamp, 0)
        .ok_or_else(|| AppError::Validation(key.to_string()))?;
    if !(1..=9999).contains(&date.year()) {
        return Err(AppError::Validation(key.to_string()));
    }
    Ok(timestamp.div_euclid(86_400) * 86_400)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn financial_numbers_must_be_finite() {
        assert_eq!(finite_number("1.25", "bad").unwrap(), 1.25);
        for value in ["NaN", "inf", "-inf", "1e999", "abc", " 1.25 "] {
            assert!(finite_number(value, "bad").is_err(), "accepted {value}");
        }
    }

    #[test]
    fn calendar_dates_are_bounded_and_normalized() {
        assert_eq!(utc_day(86_401, "bad").unwrap(), 86_400);
        assert_eq!(utc_day(-1, "bad").unwrap(), -86_400);
        assert!(utc_day(i64::MAX, "bad").is_err());
    }
}
