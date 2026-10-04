//! Dated valuations of manually priced assets (real estate, other assets).
//!
//! One row per estimate ("Přecenit"); the newest row is what the parent's
//! `market_price` shows.

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

/// One dated estimate of an asset's value.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AssetValuation {
    pub id: String,
    /// `real_estate.id` or `other_assets.id`, depending on the table.
    #[serde(rename = "assetId")]
    pub asset_id: String,
    /// Market price of the whole property, or price per unit of an other asset (money TEXT).
    pub value: String,
    pub currency: String,
    /// UTC day the estimate applies to (ADR 0008).
    #[serde(rename = "valuedAt")]
    pub valued_at: i64,
    pub note: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for recording a valuation.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertAssetValuation {
    #[serde(rename = "assetId")]
    pub asset_id: String,
    pub value: String,
    pub currency: Option<String>,
    #[serde(rename = "valuedAt")]
    pub valued_at: i64,
    pub note: Option<String>,
}

impl InsertAssetValuation {
    /// Validate input data at the trust boundary.
    pub fn validate(&self) -> Result<()> {
        if self.asset_id.trim().is_empty() {
            return Err(AppError::Validation("validation.assetRequired".into()));
        }
        let value: f64 = self
            .value
            .trim()
            .parse()
            .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
        if !value.is_finite() || value < 0.0 {
            return Err(AppError::Validation("validation.invalidAmount".into()));
        }
        if let Some(currency) = &self.currency {
            if currency.len() != 3 {
                return Err(AppError::Validation("validation.currencyInvalid".into()));
            }
        }
        if self.valued_at <= 0 {
            return Err(AppError::Validation("validation.invalidDate".into()));
        }
        Ok(())
    }
}
