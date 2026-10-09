//! Stock tag models for investment categorization

use serde::{Deserialize, Serialize};
use specta::Type;

use super::fx::ConversionQuality;

/// Stock tag group for organizing related tags
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockTagGroup {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating a stock tag group
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertStockTagGroup {
    pub name: String,
    pub description: Option<String>,
}

/// Stock tag for categorizing investments (e.g., Growth, Value, Dividend)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockTag {
    pub id: String,
    pub name: String,
    pub color: Option<String>,
    #[serde(rename = "groupId")]
    pub group_id: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating a stock tag
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertStockTag {
    pub name: String,
    pub color: Option<String>,
    #[serde(rename = "groupId")]
    pub group_id: Option<String>,
}

/// A stock position with its tags, in the response's report currency (converted amounts are
/// decimal TEXT, `null` when they cannot be converted).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StockInvestmentWithTags {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    pub quantity: String,
    /// Native weighted average cost (currency `averagePriceCurrency`).
    #[serde(rename = "averagePrice")]
    pub average_price: String,
    #[serde(rename = "averagePriceCurrency")]
    pub average_price_currency: String,
    /// Current price in its own currency; `null` when unpriced.
    #[serde(rename = "price")]
    pub price: Option<String>,
    #[serde(rename = "priceCurrency")]
    pub price_currency: Option<String>,
    #[serde(rename = "value")]
    pub value: Option<String>,
    #[serde(rename = "costBasis")]
    pub cost_basis: Option<String>,
    #[serde(rename = "unrealizedGain")]
    pub unrealized_gain: Option<String>,
    #[serde(rename = "unrealizedGainPercent")]
    pub unrealized_gain_percent: Option<f64>,
    #[serde(rename = "yearlyDividend")]
    pub yearly_dividend: Option<String>,
    pub tags: Vec<StockTag>,
}

/// Every stock position with its tags in `currency`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StocksAnalysis {
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "items")]
    pub items: Vec<StockInvestmentWithTags>,
}

/// Sums over a tag's positions in the response's report currency, every known amount counted
/// (the rule of `PositionTotals`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TagMetrics {
    pub tag: StockTag,
    #[serde(rename = "value")]
    pub value: String,
    #[serde(rename = "costBasis")]
    pub cost_basis: String,
    #[serde(rename = "unrealizedGain")]
    pub unrealized_gain: String,
    /// Percent of the cost of the positions in `unrealizedGain`; 0 without a cost basis.
    #[serde(rename = "unrealizedGainPercent")]
    pub unrealized_gain_percent: f64,
    #[serde(rename = "yearlyDividend")]
    pub yearly_dividend: String,
    /// The tag's value in percent of the known value of every position.
    #[serde(rename = "portfolioPercent")]
    pub portfolio_percent: f64,
    /// Positions carrying the tag, summed or not.
    #[serde(rename = "holdingsCount")]
    pub holdings_count: i32,
    /// What the tag's positions could not convert.
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
}

/// Metrics per tag in `currency`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TagMetricsReport {
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "tags")]
    pub tags: Vec<TagMetrics>,
}

/// Tag group with its tags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockTagGroupWithTags {
    pub group: StockTagGroup,
    pub tags: Vec<StockTag>,
}
