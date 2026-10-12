//! Other assets models

use rmcp::schemars;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Other asset (commodities, collectibles, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OtherAsset {
    pub id: String,
    pub name: String,
    pub quantity: String,
    #[serde(rename = "marketPrice")]
    pub market_price: String,
    pub currency: String,
    #[serde(rename = "averagePurchasePrice")]
    pub average_purchase_price: String,
    #[serde(rename = "yieldType")]
    pub yield_type: String,
    #[serde(rename = "yieldValue")]
    pub yield_value: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// Data for creating/updating other asset
#[derive(Debug, Clone, Deserialize, Type, rmcp::schemars::JsonSchema)]
pub struct InsertOtherAsset {
    pub name: String,
    #[schemars(
        description = "Starting quantity when no transactions are provided. When initialTransaction is provided, quantity is derived from it atomically. Later transaction imports recalculate quantity from all buy/sell transactions."
    )]
    pub quantity: Option<String>,
    #[serde(rename = "marketPrice")]
    pub market_price: Option<String>,
    pub currency: Option<String>,
    #[serde(rename = "averagePurchasePrice")]
    #[schemars(
        description = "Starting average purchase price in the asset currency when no transactions are provided. When initialTransaction is provided, the average is derived atomically using transaction-day FX. Later imports recalculate the average from all buys."
    )]
    pub average_purchase_price: Option<String>,
    #[serde(rename = "yieldType")]
    pub yield_type: Option<String>,
    #[serde(rename = "yieldValue")]
    pub yield_value: Option<String>,
}

/// Transaction for other asset
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct OtherAssetTransaction {
    pub id: String,
    #[serde(rename = "assetId")]
    pub asset_id: String,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub quantity: String,
    #[serde(rename = "pricePerUnit")]
    pub price_per_unit: String,
    pub currency: String,
    #[serde(rename = "transactionDate")]
    pub transaction_date: i64,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating transaction
#[derive(Debug, Clone, Deserialize, Type, rmcp::schemars::JsonSchema)]
pub struct InsertOtherAssetTransaction {
    #[serde(rename = "assetId")]
    pub asset_id: Option<String>,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub quantity: String,
    #[serde(rename = "pricePerUnit")]
    pub price_per_unit: String,
    pub currency: String,
    #[serde(rename = "transactionDate")]
    pub transaction_date: i64,
}

// Input validation at trust boundary
use crate::error::{AppError, Result};

impl InsertOtherAsset {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        // Name validation
        if self.name.is_empty() {
            return Err(AppError::Validation("validation.assetNameRequired".into()));
        }
        if self.name.len() > 100 {
            return Err(AppError::Validation("validation.nameTooLong".into()));
        }

        // Quantity validation (if provided)
        if let Some(ref qty) = self.quantity {
            if !qty.is_empty() {
                let qty_val = super::validation::finite_number(qty, "validation.invalidQuantity")?;
                if qty_val < 0.0 {
                    return Err(AppError::Validation(
                        "validation.quantityNonNegative".into(),
                    ));
                }
            }
        }

        // Market price validation (if provided)
        if let Some(ref price) = self.market_price {
            if !price.is_empty() {
                let price_val = super::validation::finite_number(price, "validation.invalidPrice")?;
                if price_val < 0.0 {
                    return Err(AppError::Validation("validation.priceNonNegative".into()));
                }
            }
        }

        // Average purchase price validation (if provided)
        if let Some(ref price) = self.average_purchase_price {
            if !price.is_empty() {
                let price_val = super::validation::finite_number(price, "validation.invalidPrice")?;
                if price_val < 0.0 {
                    return Err(AppError::Validation(
                        "validation.averagePriceNonNegative".into(),
                    ));
                }
            }
        }

        let quantity = self
            .quantity
            .as_deref()
            .filter(|v| !v.is_empty())
            .unwrap_or("0");
        let quantity = super::validation::finite_number(quantity, "validation.invalidQuantity")?;
        for price in [&self.market_price, &self.average_purchase_price]
            .into_iter()
            .flatten()
        {
            if !price.is_empty()
                && !(quantity * super::validation::finite_number(price, "validation.invalidPrice")?)
                    .is_finite()
            {
                return Err(AppError::Validation("validation.invalidAmount".into()));
            }
        }

        // Currency validation (if provided)
        if let Some(ref currency) = self.currency {
            if currency.len() != 3 {
                return Err(AppError::Validation("validation.currencyInvalid".into()));
            }
        }

        // Yield type validation (if provided)
        if let Some(ref yield_type) = self.yield_type {
            let valid_types = ["none", "fixed", "percent_purchase", "percent_market"];
            if !valid_types.contains(&yield_type.to_lowercase().as_str()) {
                return Err(AppError::Validation("validation.yieldTypeInvalid".into()));
            }
        }

        // Yield value validation (if provided)
        if let Some(ref yield_val) = self.yield_value {
            if !yield_val.is_empty() {
                let yv = super::validation::finite_number(yield_val, "validation.invalidAmount")?;
                if yv < 0.0 {
                    return Err(AppError::Validation("validation.yieldNonNegative".into()));
                }
            }
        }

        Ok(())
    }
}

impl InsertOtherAssetTransaction {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        // Transaction type validation
        let tx_type = self.tx_type.to_lowercase();
        if tx_type != "buy" && tx_type != "sell" {
            return Err(AppError::Validation(
                "validation.transactionTypeInvalid".into(),
            ));
        }

        // Quantity validation
        let qty = super::validation::finite_number(&self.quantity, "validation.invalidQuantity")?;
        if qty <= 0.0 {
            return Err(AppError::Validation("validation.quantityPositive".into()));
        }

        // Price validation
        let price =
            super::validation::finite_number(&self.price_per_unit, "validation.invalidPrice")?;
        if price <= 0.0 {
            return Err(AppError::Validation("validation.pricePositive".into()));
        }

        super::validation::utc_day(self.transaction_date, "validation.invalidDate")?;
        if !(qty * price).is_finite() {
            return Err(AppError::Validation("validation.invalidAmount".into()));
        }

        // Currency validation
        if self.currency.len() != 3 {
            return Err(AppError::Validation("validation.currencyInvalid".into()));
        }
        if !self.currency.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(AppError::Validation("validation.currencyInvalid".into()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn asset_with_yield(yield_type: &str) -> InsertOtherAsset {
        InsertOtherAsset {
            name: "Gold".into(),
            quantity: None,
            market_price: None,
            currency: None,
            average_purchase_price: None,
            yield_type: Some(yield_type.into()),
            yield_value: Some("5".into()),
        }
    }

    #[test]
    fn accepts_every_yield_type_the_app_offers() {
        for yield_type in ["none", "fixed", "percent_purchase", "percent_market"] {
            assert!(
                asset_with_yield(yield_type).validate().is_ok(),
                "{yield_type} should be valid"
            );
        }
    }

    #[test]
    fn rejects_unknown_yield_type() {
        assert!(asset_with_yield("percentage").validate().is_err());
    }
    #[test]
    fn audit_assets_rejects_nonfinite_amounts() {
        for invalid in ["NaN", "inf", "-inf", "1e999"] {
            let mut asset = asset_with_yield("fixed");
            asset.yield_value = Some(invalid.into());
            assert!(asset.validate().is_err(), "yield {invalid}");
            asset = asset_with_yield("none");
            asset.quantity = Some(invalid.into());
            assert!(asset.validate().is_err(), "quantity {invalid}");
            asset = asset_with_yield("none");
            asset.market_price = Some(invalid.into());
            assert!(asset.validate().is_err(), "market price {invalid}");
        }
    }

    #[test]
    fn audit_assets_rejects_overflowing_initial_holdings() {
        let mut asset = asset_with_yield("none");
        asset.quantity = Some("1e308".into());
        asset.market_price = Some("1e308".into());
        assert!(asset.validate().is_err());
    }
}
