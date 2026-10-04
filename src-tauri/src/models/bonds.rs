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
    #[serde(rename = "maturityDate")]
    pub maturity_date: Option<i64>,
}

// Input validation at trust boundary
use crate::error::{AppError, Result};

impl InsertBond {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
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
        let coupon: f64 = self
            .coupon_value
            .parse()
            .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
        if coupon < 0.0 {
            return Err(AppError::Validation("validation.amountRequired".into()));
        }

        // Quantity validation (if provided)
        if let Some(ref qty) = self.quantity {
            if !qty.is_empty() {
                let qty_val: f64 = qty
                    .parse()
                    .map_err(|_| AppError::Validation("validation.invalidQuantity".into()))?;
                if qty_val <= 0.0 {
                    return Err(AppError::Validation("validation.quantityPositive".into()));
                }
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
            if !rate.is_empty() {
                let rate_val: f64 = rate
                    .parse()
                    .map_err(|_| AppError::Validation("validation.interestRatePositive".into()))?;
                if !(0.0..=100.0).contains(&rate_val) {
                    return Err(AppError::Validation(
                        "validation.interestRatePositive".into(),
                    ));
                }
            }
        }

        Ok(())
    }
}
