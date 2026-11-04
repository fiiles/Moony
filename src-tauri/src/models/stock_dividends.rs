//! Received stock dividends (spec 2026-10-30-stock-dividends-design, ADR 0022). Yahoo Finance is
//! the source; a payout entered by hand counts only for a ticker Yahoo has no dividends for. Every
//! amount is gross (before withholding tax), the payout's total in `currency`, on the ex-date.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, Result};
use crate::models::fx::ConversionQuality;
use crate::models::validation::{finite_number, utc_day};
use crate::services::fx;
use crate::services::quote_unit::{is_pence, pence_to_pounds};

const MAX_NOTE_CHARS: usize = 500;

/// Where a dividend comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum StockDividendSource {
    /// Yahoo Finance's payout per share × the shares held the day before the ex-date.
    #[serde(rename = "yahoo")]
    Yahoo,
    /// Entered by the user.
    #[serde(rename = "manual")]
    Manual,
}

/// One dividend of a position.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockDividend {
    /// The record's id for a manual dividend; `yahoo:<ticker>:<exDate>` for a Yahoo one.
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "investmentId")]
    pub investment_id: String,
    #[serde(rename = "source")]
    pub source: StockDividendSource,
    /// UTC midnight of the ex-date.
    #[serde(rename = "exDate")]
    pub ex_date: i64,
    /// Yahoo only: the gross payout per share.
    #[serde(rename = "amountPerShare")]
    pub amount_per_share: Option<String>,
    /// Yahoo only: the shares held at the end of the day before the ex-date.
    #[serde(rename = "shares")]
    pub shares: Option<String>,
    /// The payout's gross total in `currency`.
    #[serde(rename = "grossAmount")]
    pub gross_amount: String,
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "note")]
    pub note: Option<String>,
    /// A manual dividend of a ticker Yahoo has dividends for: shown, but left out of every sum
    /// (Yahoo's payouts count instead).
    #[serde(rename = "superseded")]
    pub superseded: bool,
}

/// A dividend to enter by hand, or the new values of one (the position comes from the command).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct InsertStockDividend {
    /// Any moment of the ex-date; stored as its UTC midnight.
    #[serde(rename = "exDate")]
    pub ex_date: i64,
    /// The payout's gross total (before withholding tax).
    #[serde(rename = "grossAmount")]
    pub gross_amount: String,
    /// ISO code; `GBX`/`GBp` amounts are pence and are stored in GBP.
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "note", default)]
    pub note: Option<String>,
}

/// A validated manual dividend: the amount as decimal text, the day at UTC midnight.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidDividend {
    pub ex_date: i64,
    pub gross_amount: String,
    pub currency: String,
    pub note: Option<String>,
}

/// A position's dividends, newest first, with their sum in the report currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockDividendList {
    /// The report currency of the total.
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "items")]
    pub items: Vec<StockDividend>,
    /// Every counted payout converted at its ex-date's rate; `null` when payouts exist but none
    /// could be converted, "0" without payouts.
    #[serde(rename = "totalGross")]
    pub total_gross: Option<String>,
    /// Yahoo Finance has dividends for the ticker: they count, and none can be entered by hand.
    #[serde(rename = "fromYahoo")]
    pub from_yahoo: bool,
}

fn invalid(key: &str) -> AppError {
    AppError::Validation(format!("validation.{key}"))
}

/// Decimal text without float noise: up to ten decimals, trailing zeros dropped.
pub fn amount_text(value: f64) -> String {
    let text = format!("{:.10}", (value * 1e10).round() / 1e10);
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() {
        "0".to_string()
    } else {
        text.to_string()
    }
}

impl InsertStockDividend {
    /// Check every field at the trust boundary and return the normalized values.
    pub fn validate(&self) -> Result<ValidDividend> {
        let ex_date = utc_day(self.ex_date, "validation.dateInvalid")?;
        let mut gross = finite_number(self.gross_amount.trim(), "validation.invalidAmount")?;
        if gross <= 0.0 {
            return Err(invalid("dividendGrossPositive"));
        }
        let raw_currency = self.currency.trim();
        let currency = if is_pence(raw_currency) {
            gross = pence_to_pounds(gross);
            "GBP".to_string()
        } else {
            fx::normalize(raw_currency).ok_or_else(|| invalid("currencyInvalid"))?
        };
        if !fx::is_supported(&currency) {
            return Err(invalid("currencyUnknown"));
        }
        let note = self
            .note
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
            .map(str::to_string);
        if note
            .as_deref()
            .is_some_and(|n| n.chars().count() > MAX_NOTE_CHARS)
        {
            return Err(invalid("dividendNoteTooLong"));
        }
        Ok(ValidDividend {
            ex_date,
            gross_amount: amount_text(gross),
            currency,
            note,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(gross: &str, currency: &str) -> InsertStockDividend {
        InsertStockDividend {
            ex_date: 1_767_225_600 + 3_600,
            gross_amount: gross.into(),
            currency: currency.into(),
            note: None,
        }
    }

    fn key(result: Result<ValidDividend>) -> String {
        match result {
            Err(AppError::Validation(key)) => key,
            other => format!("{other:?}"),
        }
    }

    #[test]
    fn a_valid_dividend_is_normalized() {
        let valid = input(" 12.50 ", "usd").validate().expect("valid");
        assert_eq!(valid.ex_date, 1_767_225_600, "UTC midnight of the ex-date");
        assert_eq!(valid.gross_amount, "12.5");
        assert_eq!(valid.currency, "USD");
        assert_eq!(valid.note, None);
    }

    #[test]
    fn the_gross_must_be_positive_and_finite() {
        assert_eq!(
            key(input("0", "USD").validate()),
            "validation.dividendGrossPositive"
        );
        assert_eq!(
            key(input("-1", "USD").validate()),
            "validation.dividendGrossPositive"
        );
        for bad in ["NaN", "inf", "1e999", "abc"] {
            assert_eq!(
                key(input(bad, "USD").validate()),
                "validation.invalidAmount"
            );
        }
    }

    #[test]
    fn the_currency_must_be_one_moony_converts() {
        assert_eq!(
            key(input("1", "dollar").validate()),
            "validation.currencyInvalid"
        );
        assert_eq!(
            key(input("1", "XYZ").validate()),
            "validation.currencyUnknown"
        );
    }

    #[test]
    fn pence_are_stored_as_pounds() {
        let valid = input("1250", "GBX").validate().expect("valid");
        assert_eq!(
            (valid.gross_amount.as_str(), valid.currency.as_str()),
            ("12.5", "GBP")
        );
        assert_eq!(input("100", "GBp").validate().unwrap().currency, "GBP");
    }

    #[test]
    fn the_note_is_trimmed_and_bounded() {
        let mut row = input("1", "USD");
        row.note = Some("  special  ".into());
        assert_eq!(row.validate().unwrap().note.as_deref(), Some("special"));
        row.note = Some("x".repeat(501));
        assert_eq!(key(row.validate()), "validation.dividendNoteTooLong");
    }

    #[test]
    fn the_ex_date_must_be_a_calendar_day() {
        let mut row = input("1", "USD");
        row.ex_date = i64::MAX;
        assert_eq!(key(row.validate()), "validation.dateInvalid");
    }

    #[test]
    fn amounts_are_written_without_float_noise() {
        assert_eq!(amount_text(0.1 + 0.2), "0.3");
        assert_eq!(amount_text(100.0), "100");
        assert_eq!(amount_text(0.0), "0");
        assert_eq!(amount_text(-0.0), "0");
    }
}
