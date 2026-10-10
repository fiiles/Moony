//! Portfolio projection models

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppError, Result};
use crate::models::fx::ConversionQuality;

/// Projection settings for a specific asset type
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ProjectionSettings {
    pub id: String,
    #[serde(rename = "assetType")]
    pub asset_type: String,
    /// Yearly growth in percent as text. An explicit `"0"` is honoured as 0 %.
    /// An empty string means "not set": savings and bonds then use the weighted
    /// rate calculated from the user's own accounts / bonds, the other classes
    /// their built-in default.
    #[serde(rename = "yearlyGrowthRate")]
    pub yearly_growth_rate: String,
    /// Monthly contribution as text, in `contribution_currency` (the currency the user typed;
    /// converted into the report currency when a projection is calculated).
    #[serde(rename = "monthlyContribution")]
    pub monthly_contribution: String,
    #[serde(rename = "contributionCurrency")]
    pub contribution_currency: String,
    pub enabled: bool,
    #[serde(rename = "createdAt", default)]
    pub created_at: i64,
    #[serde(rename = "updatedAt", default)]
    pub updated_at: i64,
}

impl ProjectionSettings {
    /// Validate input data at the trust boundary: the contribution currency must be a
    /// three-letter ISO code and the contribution a finite, non-negative amount.
    pub fn validate(&self) -> Result<()> {
        if crate::services::fx::normalize(&self.contribution_currency).is_none() {
            return Err(AppError::Validation("validation.currencyInvalid".into()));
        }
        let amount = self
            .monthly_contribution
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|v| v.is_finite())
            .ok_or_else(|| AppError::Validation("validation.invalidAmount".into()))?;
        if amount < 0.0 {
            return Err(AppError::Validation("validation.amountNonNegative".into()));
        }
        Ok(())
    }
}

/// Input for calculating a projection (the report currency is a separate command argument).
#[derive(Debug, Deserialize)]
pub struct ProjectionInput {
    #[serde(rename = "horizonYears")]
    pub horizon_years: i32,
    #[serde(rename = "viewType")]
    pub view_type: String, // "monthly" or "yearly"
    #[serde(rename = "excludePersonalRealEstate", default)]
    pub exclude_personal_real_estate: bool,
    /// Scenario shift in percentage points added to every active class's
    /// annual rate (design system §7 Outlook: the ± 2 p.p. band). Default 0.
    #[serde(rename = "rateShift", default)]
    pub rate_shift: f64,
}

/// A single point in the projection timeline; amounts are decimal text in the report
/// currency, rounded at its minor unit.
#[derive(Debug, Clone, Serialize, Type)]
pub struct ProjectionTimelinePoint {
    #[serde(rename = "date")]
    pub date: i64,
    #[serde(rename = "totalAssets")]
    pub total_assets: String,
    #[serde(rename = "totalLiabilities")]
    pub total_liabilities: String,
    #[serde(rename = "netWorth")]
    pub net_worth: String,
    // Category breakdown
    #[serde(rename = "savings")]
    pub savings: String,
    #[serde(rename = "investments")]
    pub investments: String,
    #[serde(rename = "crypto")]
    pub crypto: String,
    #[serde(rename = "bonds")]
    pub bonds: String,
    #[serde(rename = "realEstate")]
    pub real_estate: String,
    #[serde(rename = "otherAssets")]
    pub other_assets: String,
    #[serde(rename = "loans")]
    pub loans: String,
}

/// Calculated default rates from actual portfolio data (percent)
#[derive(Debug, Clone, Serialize, Type)]
pub struct CalculatedDefaults {
    #[serde(rename = "savingsRate")]
    pub savings_rate: f64,
    #[serde(rename = "bondsRate")]
    pub bonds_rate: f64,
}

/// Full portfolio projection response, valued in `currency`. What could not be converted
/// (a holding or a contribution in a currency without a quote) is left out and named in
/// `fxQuality`.
#[derive(Debug, Clone, Serialize, Type)]
pub struct PortfolioProjection {
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "horizonYears")]
    pub horizon_years: i32,
    #[serde(rename = "viewType")]
    pub view_type: String, // "monthly" or "yearly"
    #[serde(rename = "timeline")]
    pub timeline: Vec<ProjectionTimelinePoint>,
    #[serde(rename = "projectedNetWorth")]
    pub projected_net_worth: String,
    #[serde(rename = "totalContributions")]
    pub total_contributions: String,
    #[serde(rename = "totalGrowth")]
    pub total_growth: String,
    #[serde(rename = "calculatedDefaults")]
    pub calculated_defaults: CalculatedDefaults,
}
