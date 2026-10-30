//! Received stock dividends: one record per payout of a position (spec
//! 2026-10-30-stock-dividends-design, ADR 0022). Amounts are the payout's totals in `currency`,
//! gross before withholding tax; the net amount is derived, never stored.

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, Result};
use crate::models::fx::ConversionQuality;
use crate::models::validation::{finite_number, utc_day};
use crate::services::fx;
use crate::services::quote_unit::{is_pence, pence_to_pounds};

const MAX_NOTE_CHARS: usize = 500;

/// A received dividend as stored, with its net amount.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockDividend {
    pub id: String,
    #[serde(rename = "investmentId")]
    pub investment_id: String,
    /// UTC midnight of the pay date.
    #[serde(rename = "paidDate")]
    pub paid_date: i64,
    /// The payout's total before withholding tax, in `currency`.
    #[serde(rename = "grossAmount")]
    pub gross_amount: String,
    /// Tax withheld from this payout, in `currency` ("0" when none).
    #[serde(rename = "withholdingTax")]
    pub withholding_tax: String,
    /// `grossAmount − withholdingTax`, in `currency`.
    #[serde(rename = "netAmount")]
    pub net_amount: String,
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "note")]
    pub note: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// A dividend to record or the new values of one (the position comes from the command).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct InsertStockDividend {
    /// Any moment of the pay day; stored as its UTC midnight.
    #[serde(rename = "paidDate")]
    pub paid_date: i64,
    #[serde(rename = "grossAmount")]
    pub gross_amount: String,
    /// Absent or empty: no tax.
    #[serde(rename = "withholdingTax", default)]
    pub withholding_tax: Option<String>,
    /// ISO code; `GBX`/`GBp` amounts are pence and are stored in GBP.
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "note", default)]
    pub note: Option<String>,
}

/// A validated dividend: amounts as decimal text, the day at UTC midnight, a supported currency.
#[derive(Debug, Clone, PartialEq)]
pub struct ValidDividend {
    pub paid_date: i64,
    pub gross: f64,
    pub tax: f64,
    pub gross_amount: String,
    pub withholding_tax: String,
    pub currency: String,
    pub note: Option<String>,
}

/// A position's dividends, newest first, with their sums in the report currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct StockDividendList {
    /// The report currency of the totals.
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "items")]
    pub items: Vec<StockDividend>,
    /// Every payout's gross converted at its pay day's rate; `null` when payouts exist but none
    /// could be converted, "0" without payouts.
    #[serde(rename = "totalGross")]
    pub total_gross: Option<String>,
    /// The same for the net amounts.
    #[serde(rename = "totalNet")]
    pub total_net: Option<String>,
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
        let paid_date = utc_day(self.paid_date, "validation.dateInvalid")?;
        let mut gross = finite_number(self.gross_amount.trim(), "validation.invalidAmount")?;
        if gross <= 0.0 {
            return Err(invalid("dividendGrossPositive"));
        }
        let mut tax = match self.withholding_tax.as_deref().map(str::trim) {
            None | Some("") => 0.0,
            Some(raw) => finite_number(raw, "validation.invalidAmount")?,
        };
        let raw_currency = self.currency.trim();
        let currency = if is_pence(raw_currency) {
            gross = pence_to_pounds(gross);
            tax = pence_to_pounds(tax);
            "GBP".to_string()
        } else {
            fx::normalize(raw_currency).ok_or_else(|| invalid("currencyInvalid"))?
        };
        if !fx::is_supported(&currency) {
            return Err(invalid("currencyUnknown"));
        }
        if !(0.0..=gross).contains(&tax) {
            return Err(invalid("dividendTaxInvalid"));
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
            paid_date,
            gross,
            tax,
            gross_amount: amount_text(gross),
            withholding_tax: amount_text(tax),
            currency,
            note,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(gross: &str, tax: Option<&str>, currency: &str) -> InsertStockDividend {
        InsertStockDividend {
            paid_date: 1_767_225_600 + 3_600,
            gross_amount: gross.into(),
            withholding_tax: tax.map(str::to_string),
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
        let valid = input(" 12.50 ", Some("1.875"), "usd")
            .validate()
            .expect("valid");
        assert_eq!(
            valid.paid_date, 1_767_225_600,
            "UTC midnight of the pay day"
        );
        assert_eq!(valid.gross_amount, "12.5");
        assert_eq!(valid.withholding_tax, "1.875");
        assert_eq!(valid.currency, "USD");
        assert_eq!(valid.note, None);
    }

    #[test]
    fn a_missing_or_empty_tax_is_zero() {
        assert_eq!(
            input("3", None, "EUR").validate().unwrap().withholding_tax,
            "0"
        );
        assert_eq!(
            input("3", Some(" "), "EUR")
                .validate()
                .unwrap()
                .withholding_tax,
            "0"
        );
    }

    #[test]
    fn the_gross_must_be_positive_and_finite() {
        assert_eq!(
            key(input("0", None, "USD").validate()),
            "validation.dividendGrossPositive"
        );
        assert_eq!(
            key(input("-1", None, "USD").validate()),
            "validation.dividendGrossPositive"
        );
        for bad in ["NaN", "inf", "1e999", "abc"] {
            assert_eq!(
                key(input(bad, None, "USD").validate()),
                "validation.invalidAmount"
            );
        }
    }

    #[test]
    fn the_tax_lies_between_zero_and_the_gross() {
        assert!(input("10", Some("10"), "USD").validate().is_ok());
        assert_eq!(
            key(input("10", Some("10.01"), "USD").validate()),
            "validation.dividendTaxInvalid"
        );
        assert_eq!(
            key(input("10", Some("-1"), "USD").validate()),
            "validation.dividendTaxInvalid"
        );
        assert_eq!(
            key(input("10", Some("NaN"), "USD").validate()),
            "validation.invalidAmount"
        );
    }

    #[test]
    fn the_currency_must_be_one_moony_converts() {
        assert_eq!(
            key(input("1", None, "dollar").validate()),
            "validation.currencyInvalid"
        );
        assert_eq!(
            key(input("1", None, "XYZ").validate()),
            "validation.currencyUnknown"
        );
    }

    #[test]
    fn pence_are_stored_as_pounds() {
        let valid = input("1250", Some("125"), "GBX").validate().expect("valid");
        assert_eq!(
            (valid.gross_amount.as_str(), valid.withholding_tax.as_str()),
            ("12.5", "1.25")
        );
        assert_eq!(valid.currency, "GBP");
        assert_eq!(
            input("100", None, "GBp").validate().unwrap().currency,
            "GBP"
        );
    }

    #[test]
    fn the_note_is_trimmed_and_bounded() {
        let mut row = input("1", None, "USD");
        row.note = Some("  special  ".into());
        assert_eq!(row.validate().unwrap().note.as_deref(), Some("special"));
        row.note = Some("x".repeat(501));
        assert_eq!(key(row.validate()), "validation.dividendNoteTooLong");
    }

    #[test]
    fn the_pay_date_must_be_a_calendar_day() {
        let mut row = input("1", None, "USD");
        row.paid_date = i64::MAX;
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
