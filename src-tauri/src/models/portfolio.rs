//! Wire types for net worth and its history in a report currency (ADR 0011).

use crate::models::fx::ConversionQuality;
use serde::{Deserialize, Serialize};
use specta::Type;
use std::collections::HashMap;

/// Current net worth in `currency`. Class totals are decimal TEXT, `null` when nothing in the
/// class could be converted; the totals sum what converted and `fxQuality` names the rest.
/// The `*ByCurrency` maps stay native (amounts in their own currencies).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PortfolioMetrics {
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "totalSavings")]
    pub total_savings: Option<String>,
    #[serde(rename = "totalInvestments")]
    pub total_investments: Option<String>,
    #[serde(rename = "totalCrypto")]
    pub total_crypto: Option<String>,
    #[serde(rename = "totalBonds")]
    pub total_bonds: Option<String>,
    /// Always reported, also when personal real estate is excluded from the totals.
    #[serde(rename = "totalRealEstatePersonal")]
    pub total_real_estate_personal: Option<String>,
    #[serde(rename = "totalRealEstateInvestment")]
    pub total_real_estate_investment: Option<String>,
    /// The real estate counted in `totalAssets` (personal left out when excluded).
    #[serde(rename = "totalRealEstate")]
    pub total_real_estate: Option<String>,
    #[serde(rename = "totalOtherAssets")]
    pub total_other_assets: Option<String>,
    #[serde(rename = "totalLiabilities")]
    pub total_liabilities: String,
    #[serde(rename = "totalAssets")]
    pub total_assets: String,
    #[serde(rename = "netWorth")]
    pub net_worth: String,
    #[serde(rename = "savingsByCurrency")]
    pub savings_by_currency: HashMap<String, f64>,
    #[serde(rename = "investmentsByCurrency")]
    pub investments_by_currency: HashMap<String, f64>,
    #[serde(rename = "cryptoByCurrency")]
    pub crypto_by_currency: HashMap<String, f64>,
    #[serde(rename = "bondsByCurrency")]
    pub bonds_by_currency: HashMap<String, f64>,
    /// Personal properties left out when excluded, like `totalRealEstate`.
    #[serde(rename = "realEstateByCurrency")]
    pub real_estate_by_currency: HashMap<String, f64>,
    #[serde(rename = "loansByCurrency")]
    pub loans_by_currency: HashMap<String, f64>,
    #[serde(rename = "otherAssetsByCurrency")]
    pub other_assets_by_currency: HashMap<String, f64>,
}

/// One recorded day of the portfolio history in the response currency (converted at that
/// day's rates). Amounts are decimal TEXT, `null` when nothing in the class converted.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PortfolioHistoryPoint {
    #[serde(rename = "recordedAt")]
    pub recorded_at: i64,
    /// `"live"` (recorded by the running app) or `"backfill"` (reconstructed; the static
    /// classes of such a day are carried from the nearest live row).
    #[serde(rename = "source")]
    pub source: String,
    #[serde(rename = "isStale")]
    pub is_stale: bool,
    #[serde(rename = "totalSavings")]
    pub total_savings: Option<String>,
    #[serde(rename = "totalLoansPrincipal")]
    pub total_loans_principal: Option<String>,
    #[serde(rename = "totalInvestments")]
    pub total_investments: Option<String>,
    #[serde(rename = "totalCrypto")]
    pub total_crypto: Option<String>,
    #[serde(rename = "totalBonds")]
    pub total_bonds: Option<String>,
    #[serde(rename = "totalRealEstatePersonal")]
    pub total_real_estate_personal: Option<String>,
    #[serde(rename = "totalRealEstateInvestment")]
    pub total_real_estate_investment: Option<String>,
    #[serde(rename = "totalOtherAssets")]
    pub total_other_assets: Option<String>,
    /// Every converted asset class (personal real estate included) minus the loans.
    #[serde(rename = "netWorth")]
    pub net_worth: Option<String>,
}

/// The portfolio history in `currency`, oldest day first.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PortfolioHistoryResponse {
    #[serde(rename = "currency")]
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    #[serde(rename = "points")]
    pub points: Vec<PortfolioHistoryPoint>,
}
