//! Classifies a pre-split transaction as split-adjusted or not (spec D7), by comparing its
//! price to the close of the trade day.

use serde::{Deserialize, Serialize};
use specta::Type;

/// Lower bound of the band a price must fall in, relative to its expected value.
pub(crate) const BAND_LOW: f64 = 0.7;
/// Upper bound of the band a price must fall in, relative to its expected value.
pub(crate) const BAND_HIGH: f64 = 1.4;

/// Whether a transaction's price is already expressed in post-split terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Adjusted,
    Unadjusted,
    Unknown,
}

/// D7 for one transaction. `close` is `(price, currency)` of the trade day when known.
pub fn classify(price: f64, currency: &str, close: Option<(f64, &str)>, n: u64, d: u64) -> Verdict {
    let Some((close, close_currency)) = close else {
        return Verdict::Unknown;
    };
    if !close_currency.eq_ignore_ascii_case(currency) || n == 0 || d == 0 {
        return Verdict::Unknown;
    }
    let factor = n as f64 / d as f64;
    // Close to 1 the two bands overlap, so a price cannot tell the cases apart.
    if factor > 0.5 && factor < 2.0 {
        return Verdict::Unknown;
    }
    let ratio = price / close;
    let in_band = |expected: f64| (BAND_LOW * expected..=BAND_HIGH * expected).contains(&ratio);
    match (in_band(factor), in_band(1.0)) {
        (true, false) => Verdict::Unadjusted,
        (false, true) => Verdict::Adjusted,
        _ => Verdict::Unknown,
    }
}

/// D7 for the split: unadjusted / adjusted only when no transaction says the opposite.
pub fn verdict_of(verdicts: &[Verdict]) -> Verdict {
    let has = |v| verdicts.contains(&v);
    match (has(Verdict::Unadjusted), has(Verdict::Adjusted)) {
        (true, false) => Verdict::Unadjusted,
        (false, true) => Verdict::Adjusted,
        _ => Verdict::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pre_split_price_near_factor_times_close_is_unadjusted() {
        assert_eq!(
            classify(950.0, "USD", Some((96.0, "USD")), 10, 1),
            Verdict::Unadjusted
        );
    }

    #[test]
    fn price_near_close_is_adjusted() {
        assert_eq!(
            classify(96.0, "USD", Some((95.0, "USD")), 10, 1),
            Verdict::Adjusted
        );
    }

    #[test]
    fn out_of_both_bands_is_unknown() {
        assert_eq!(
            classify(300.0, "USD", Some((95.0, "USD")), 10, 1),
            Verdict::Unknown
        );
    }

    #[test]
    fn reverse_split_bands() {
        assert_eq!(
            classify(9.5, "USD", Some((95.0, "USD")), 1, 10),
            Verdict::Unadjusted
        );
        assert_eq!(
            classify(96.0, "USD", Some((95.0, "USD")), 1, 10),
            Verdict::Adjusted
        );
    }

    #[test]
    fn overlapping_bands_are_unknown() {
        assert_eq!(
            classify(150.0, "USD", Some((100.0, "USD")), 3, 2),
            Verdict::Unknown
        );
        assert_eq!(
            classify(100.0, "USD", Some((100.0, "USD")), 3, 2),
            Verdict::Unknown
        );
    }

    #[test]
    fn missing_close_or_other_currency_is_unknown() {
        assert_eq!(classify(950.0, "USD", None, 10, 1), Verdict::Unknown);
        assert_eq!(
            classify(950.0, "USD", Some((96.0, "GBP")), 10, 1),
            Verdict::Unknown
        );
    }

    #[test]
    fn split_verdict() {
        use Verdict::*;
        assert_eq!(verdict_of(&[Unadjusted, Unknown]), Unadjusted);
        assert_eq!(verdict_of(&[Adjusted, Unknown]), Adjusted);
        assert_eq!(verdict_of(&[Adjusted, Unadjusted]), Unknown);
        assert_eq!(verdict_of(&[Unknown]), Unknown);
        assert_eq!(verdict_of(&[]), Unknown);
    }
}
