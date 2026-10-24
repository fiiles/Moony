//! Wire types of stock splits (spec section 6; TS mirror in `shared/schema.ts`, section
//! "Stock splits"). Per-field `#[serde(rename = "camelCase")]`, as everywhere in this
//! codebase; mirror every change in `shared/schema.ts`.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, Result};
use crate::models::validation::utc_day;

pub use super::classify::Verdict as StockSplitVerdict;

/// Largest numerator or denominator a split may carry (D11).
pub const MAX_RATIO_PART: u64 = 1_000_000;

/// Where a split record came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum StockSplitSource {
    Yahoo,
    Manual,
    Import,
}

impl StockSplitSource {
    /// The `stock_splits.source` value.
    pub fn as_str(self) -> &'static str {
        match self {
            StockSplitSource::Yahoo => "yahoo",
            StockSplitSource::Manual => "manual",
            StockSplitSource::Import => "import",
        }
    }

    /// Parses a stored `stock_splits.source` value.
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "yahoo" => Ok(StockSplitSource::Yahoo),
            "manual" => Ok(StockSplitSource::Manual),
            "import" => Ok(StockSplitSource::Import),
            other => Err(AppError::Database(format!("unknown split source {other}"))),
        }
    }
}

/// Where a split stands: detected and waiting, applied to the stored rows, or dismissed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum StockSplitStatus {
    Pending,
    Applied,
    Dismissed,
}

impl StockSplitStatus {
    /// The `stock_splits.status` value.
    pub fn as_str(self) -> &'static str {
        match self {
            StockSplitStatus::Pending => "pending",
            StockSplitStatus::Applied => "applied",
            StockSplitStatus::Dismissed => "dismissed",
        }
    }

    /// Parses a stored `stock_splits.status` value.
    pub fn parse(text: &str) -> Result<Self> {
        match text {
            "pending" => Ok(StockSplitStatus::Pending),
            "applied" => Ok(StockSplitStatus::Applied),
            "dismissed" => Ok(StockSplitStatus::Dismissed),
            other => Err(AppError::Database(format!("unknown split status {other}"))),
        }
    }
}

/// One `stock_splits` row plus what is derived on read: the number of transactions dated
/// before the split (D6) and, for a pending split, the D7 verdict.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct StockSplit {
    pub id: String,
    pub ticker: String,
    /// UTC midnight of the ex-date.
    #[serde(rename = "effectiveDay")]
    pub effective_day: i64,
    /// Integer as text, e.g. `"4"`.
    pub numerator: String,
    /// Integer as text, e.g. `"1"`.
    pub denominator: String,
    pub source: StockSplitSource,
    pub status: StockSplitStatus,
    #[serde(rename = "detectedAt")]
    pub detected_at: i64,
    #[serde(rename = "resolvedAt")]
    pub resolved_at: Option<i64>,
    /// Transactions rewritten by the last apply.
    #[serde(rename = "appliedCount")]
    pub applied_count: Option<i64>,
    /// Transactions of the ticker dated before `effective_day`, now.
    #[serde(rename = "preSplitCount")]
    pub pre_split_count: i64,
    /// D7 from `stock_value_history`; `unknown` unless pending.
    pub verdict: StockSplitVerdict,
}

/// A split the user records by hand (D11). The caller trims and uppercases `ticker`.
#[derive(Clone, Debug, Serialize, Deserialize, Type)]
pub struct InsertStockSplit {
    pub ticker: String,
    #[serde(rename = "effectiveDay")]
    pub effective_day: i64,
    pub numerator: String,
    pub denominator: String,
}

impl InsertStockSplit {
    /// Validate input data at the trust boundary: a ticker, the ratio of [`ratio_parts`], and
    /// an ex-date that is a UTC midnight not after today.
    pub fn validate(&self) -> Result<()> {
        if self.ticker.trim().is_empty() {
            return Err(AppError::Validation("validation.tickerRequired".into()));
        }
        ratio_parts(&self.numerator, &self.denominator)?;
        let day_invalid = || AppError::Validation("validation.stockSplitDayInvalid".into());
        let day = utc_day(self.effective_day, "validation.stockSplitDayInvalid")?;
        let today = chrono::Utc::now().timestamp().div_euclid(86_400) * 86_400;
        if day != self.effective_day || day > today {
            return Err(day_invalid());
        }
        Ok(())
    }
}

/// What an apply or revert did: the split as it is now, how many transactions were
/// rewritten, and the ticker's first transaction day (where the history rebuild starts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Type)]
pub struct StockSplitApplyResult {
    pub split: StockSplit,
    pub rewritten: i64,
    #[serde(rename = "earliestDay")]
    pub earliest_day: Option<i64>,
}

/// One split as a Yahoo response reports it, already reduced to integers and the UTC day.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DetectedSplit {
    pub effective_day: i64,
    pub numerator: u64,
    pub denominator: u64,
}

/// The ratio of a split as integers: both positive whole numbers up to [`MAX_RATIO_PART`],
/// written as plain digits, and different from each other.
///
/// Errors: `validation.stockSplitRatioInvalid`.
pub fn ratio_parts(numerator: &str, denominator: &str) -> Result<(u64, u64)> {
    let invalid = || AppError::Validation("validation.stockSplitRatioInvalid".into());
    let part = |text: &str| -> Result<u64> {
        let text = text.trim();
        if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid());
        }
        let value: u64 = text.parse().map_err(|_| invalid())?;
        if value == 0 || value > MAX_RATIO_PART {
            return Err(invalid());
        }
        Ok(value)
    };
    let (n, d) = (part(numerator)?, part(denominator)?);
    if n == d {
        return Err(invalid());
    }
    Ok((n, d))
}

/// Whether a detected ratio is one Moony stores (the same bounds as [`ratio_parts`]).
pub(crate) fn ratio_is_valid(numerator: u64, denominator: u64) -> bool {
    let in_range = |v: u64| (1..=MAX_RATIO_PART).contains(&v);
    in_range(numerator) && in_range(denominator) && numerator != denominator
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratio_parts_accepts_plain_positive_integers() {
        assert_eq!(ratio_parts("4", "1").unwrap(), (4, 1));
        assert_eq!(ratio_parts(" 1 ", "10").unwrap(), (1, 10));
        assert_eq!(ratio_parts("1000000", "1").unwrap(), (1_000_000, 1));
    }

    #[test]
    fn ratio_parts_rejects_everything_else() {
        for (n, d) in [
            ("4", "4"),
            ("0", "1"),
            ("1", "0"),
            ("1000001", "1"),
            ("1.5", "1"),
            ("-2", "1"),
            ("", "1"),
            ("abc", "1"),
            ("1e3", "1"),
        ] {
            let err = ratio_parts(n, d).expect_err(&format!("{n}:{d} accepted"));
            assert!(
                matches!(&err, AppError::Validation(k) if k == "validation.stockSplitRatioInvalid"),
                "{n}:{d} gave {err:?}"
            );
        }
    }
}
