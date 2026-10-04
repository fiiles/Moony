//! Stock tag models for investment categorization

use serde::{Deserialize, Serialize};
use specta::Type;

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

/// Investment with its associated tags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockInvestmentWithTags {
    pub id: String,
    pub ticker: String,
    #[serde(rename = "companyName")]
    pub company_name: String,
    pub quantity: String,
    #[serde(rename = "averagePrice")]
    pub average_price: String,
    #[serde(rename = "currentPrice")]
    pub current_price: String,
    #[serde(rename = "currentValue")]
    pub current_value: f64,
    #[serde(rename = "gainLoss")]
    pub gain_loss: f64,
    #[serde(rename = "gainLossPercent")]
    pub gain_loss_percent: f64,
    #[serde(rename = "dividendYield")]
    pub dividend_yield: f64,
    pub tags: Vec<StockTag>,
}

/// Aggregated metrics for a specific tag
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TagMetrics {
    pub tag: StockTag,
    #[serde(rename = "totalValue")]
    pub total_value: f64,
    #[serde(rename = "totalCost")]
    pub total_cost: f64,
    #[serde(rename = "gainLoss")]
    pub gain_loss: f64,
    #[serde(rename = "gainLossPercent")]
    pub gain_loss_percent: f64,
    #[serde(rename = "estimatedYearlyDividend")]
    pub estimated_yearly_dividend: f64,
    #[serde(rename = "portfolioPercent")]
    pub portfolio_percent: f64,
    #[serde(rename = "holdingsCount")]
    pub holdings_count: i32,
}

/// Tag group with its tags
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StockTagGroupWithTags {
    pub group: StockTagGroup,
    pub tags: Vec<StockTag>,
}
