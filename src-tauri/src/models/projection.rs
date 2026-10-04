//! Portfolio projection models

use serde::{Deserialize, Serialize};
use specta::Type;

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
    /// Monthly contribution in CZK (the base currency, ADR 0001) as text.
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

/// A single point in the projection timeline
#[derive(Debug, Clone, Serialize)]
pub struct ProjectionTimelinePoint {
    pub date: i64,
    #[serde(rename = "totalAssets")]
    pub total_assets: f64,
    #[serde(rename = "totalLiabilities")]
    pub total_liabilities: f64,
    #[serde(rename = "netWorth")]
    pub net_worth: f64,
    // Category breakdown
    pub savings: f64,
    pub investments: f64,
    pub crypto: f64,
    pub bonds: f64,
    #[serde(rename = "realEstate")]
    pub real_estate: f64,
    #[serde(rename = "otherAssets")]
    pub other_assets: f64,
    pub loans: f64,
}

/// Calculated default rates from actual portfolio data
#[derive(Debug, Clone, Serialize)]
pub struct CalculatedDefaults {
    #[serde(rename = "savingsRate")]
    pub savings_rate: f64,
    #[serde(rename = "bondsRate")]
    pub bonds_rate: f64,
}

/// Full portfolio projection response
#[derive(Debug, Clone, Serialize)]
pub struct PortfolioProjection {
    #[serde(rename = "horizonYears")]
    pub horizon_years: i32,
    #[serde(rename = "viewType")]
    pub view_type: String, // "monthly" or "yearly"
    pub timeline: Vec<ProjectionTimelinePoint>,
    #[serde(rename = "projectedNetWorth")]
    pub projected_net_worth: f64,
    #[serde(rename = "totalContributions")]
    pub total_contributions: f64,
    #[serde(rename = "totalGrowth")]
    pub total_growth: f64,
    #[serde(rename = "calculatedDefaults")]
    pub calculated_defaults: CalculatedDefaults,
}
