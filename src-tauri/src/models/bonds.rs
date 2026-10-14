//! Bond models

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Bond holding
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Bond {
    pub id: String,
    pub name: String,
    pub isin: Option<String>,
    #[serde(rename = "couponValue")]
    pub coupon_value: String,
    pub quantity: String,
    pub currency: String,
    #[serde(rename = "interestRate")]
    pub interest_rate: String,
    #[serde(rename = "maturityDate")]
    pub maturity_date: Option<i64>,
    /// The UTC day the bond was bought (midnight, unix seconds); `None` while unknown. History
    /// counts a dated bond from this day on, an undated one on every day.
    #[serde(rename = "purchaseDate")]
    pub purchase_date: Option<i64>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// Data for creating/updating bond
#[derive(Debug, Clone, Deserialize, Type, rmcp::schemars::JsonSchema)]
pub struct InsertBond {
    pub name: String,
    pub isin: Option<String>,
    #[serde(rename = "couponValue")]
    pub coupon_value: String,
    pub quantity: Option<String>,
    pub currency: Option<String>,
    #[serde(rename = "interestRate")]
    pub interest_rate: Option<String>,
    /// Calendar date as Unix seconds, years 0001-9999; stored at UTC midnight.
    #[serde(rename = "maturityDate")]
    pub maturity_date: Option<i64>,
    /// The day the bond was bought, as Unix seconds (UTC day, stored at UTC midnight; years
    /// 0001-9999, not after today). Optional: without it the bond is counted across the whole
    /// history; with it, from that day on.
    #[serde(rename = "purchaseDate")]
    pub purchase_date: Option<i64>,
}

// Input validation at trust boundary
use crate::error::{AppError, Result};

impl InsertBond {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        self.validate_at(chrono::Utc::now().timestamp())
    }

    /// `validate` with the current time passed in (unix seconds), so the "not in the future"
    /// rule can be tested without a clock.
    pub fn validate_at(&self, now: i64) -> Result<()> {
        // Name validation
        if self.name.is_empty() {
            return Err(AppError::Validation("validation.bondNameRequired".into()));
        }
        if self.name.len() > 100 {
            return Err(AppError::Validation("validation.bondNameRequired".into()));
        }

        // ISIN validation (12 characters: 2 letter country code + 9 alphanumeric + 1 check digit)
        // Only validate if ISIN is provided
        if let Some(ref isin) = self.isin {
            if !isin.is_empty() && isin.len() != 12 {
                return Err(AppError::Validation("validation.isinLength".into()));
            }
        }

        // Coupon value validation
        if self.coupon_value.is_empty() {
            return Err(AppError::Validation(
                "validation.couponValueRequired".into(),
            ));
        }
        let coupon =
            super::validation::finite_number(&self.coupon_value, "validation.invalidAmount")?;
        if coupon < 0.0 {
            return Err(AppError::Validation("validation.amountRequired".into()));
        }

        // Quantity validation (if provided)
        if let Some(ref qty) = self.quantity {
            let qty_val = super::validation::finite_number(qty, "validation.invalidQuantity")?;
            if qty_val <= 0.0 {
                return Err(AppError::Validation("validation.quantityPositive".into()));
            }
        }

        // Currency validation (if provided)
        if let Some(ref currency) = self.currency {
            if currency.len() != 3 {
                return Err(AppError::Validation("validation.currencyInvalid".into()));
            }
        }

        // Interest rate validation (if provided)
        if let Some(ref rate) = self.interest_rate {
            let rate_val =
                super::validation::finite_number(rate, "validation.interestRatePositive")?;
            if !(0.0..=100.0).contains(&rate_val) {
                return Err(AppError::Validation(
                    "validation.interestRatePositive".into(),
                ));
            }
        }

        let quantity = self.quantity.as_deref().unwrap_or("1");
        let quantity = super::validation::finite_number(quantity, "validation.invalidQuantity")?;
        if !(coupon * quantity).is_finite() {
            return Err(AppError::Validation("validation.invalidAmount".into()));
        }
        if let Some(date) = self.maturity_date {
            super::validation::utc_day(date, "validation.maturityDateInvalid")?;
        }
        // Purchase date: a day up to and including today, compared as UTC calendar days (ADR
        // 0008), the rule of a property's purchase date. A later hour of today is still today.
        if let Some(date) = self.purchase_date {
            super::validation::utc_day(date, "validation.invalidDate")?;
            if date.div_euclid(86_400) > now.div_euclid(86_400) {
                return Err(AppError::Validation(
                    "validation.purchaseDateInFuture".into(),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400;
    /// 2026-10-04 14:30 UTC.
    const NOW: i64 = 1_791_124_200;

    fn bond() -> InsertBond {
        InsertBond {
            name: "Bond".into(),
            isin: None,
            coupon_value: "1000".into(),
            quantity: None,
            currency: None,
            interest_rate: None,
            maturity_date: None,
            purchase_date: None,
        }
    }

    fn bought(date: Option<i64>) -> InsertBond {
        InsertBond {
            purchase_date: date,
            ..bond()
        }
    }

    #[test]
    fn the_purchase_date_is_optional() {
        assert!(bought(None).validate_at(NOW).is_ok());
    }

    #[test]
    fn a_purchase_date_up_to_today_is_valid() {
        let today = NOW.div_euclid(DAY) * DAY;
        for date in [today - 3_650 * DAY, today - DAY, today] {
            assert!(bought(Some(date)).validate_at(NOW).is_ok(), "{date}");
        }
        // A later hour of today is still today; the service stores the day anyway.
        assert!(bought(Some(NOW + 3_600)).validate_at(NOW).is_ok());
        assert!(bought(Some(today + 12_345)).validate_at(NOW).is_ok());
    }

    #[test]
    fn a_purchase_date_after_today_is_rejected() {
        let tomorrow = (NOW.div_euclid(DAY) + 1) * DAY;
        for date in [tomorrow, tomorrow + 3_600, NOW + 400 * DAY] {
            let err = bought(Some(date))
                .validate_at(NOW)
                .expect_err("a future date must be rejected");
            assert!(
                matches!(&err, AppError::Validation(key) if key == "validation.purchaseDateInFuture"),
                "{err:?}"
            );
        }
    }

    #[test]
    fn an_unrepresentable_purchase_date_is_rejected() {
        for date in [i64::MAX, i64::MIN, -800_000 * DAY, 300_000_000_000] {
            let err = bought(Some(date))
                .validate_at(NOW)
                .expect_err("not a calendar day");
            assert!(
                matches!(&err, AppError::Validation(key) if key == "validation.invalidDate"),
                "{date}: {err:?}"
            );
        }
    }

    #[test]
    fn validate_uses_the_clock() {
        let now = chrono::Utc::now().timestamp();
        assert!(bought(Some(now - DAY)).validate().is_ok());
        assert!(bought(Some(now + 2 * DAY)).validate().is_err());
    }
}
