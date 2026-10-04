//! User profile and app configuration models

use serde::{Deserialize, Serialize};
use specta::Type;

fn default_true() -> bool {
    true
}

/// Menu preferences for sidebar visibility
#[derive(Debug, Clone, Serialize, Deserialize, Default, Type)]
pub struct MenuPreferences {
    pub loans: bool,
    pub insurance: bool,
    pub investments: bool,
    pub bonds: bool,
    #[serde(rename = "realEstate")]
    pub real_estate: bool,
    #[serde(default = "default_true")]
    pub crypto: bool,
    #[serde(rename = "otherAssets", default = "default_true")]
    pub other_assets: bool,
}

impl MenuPreferences {
    pub fn all_enabled() -> Self {
        Self {
            loans: true,
            insurance: true,
            investments: true,
            bonds: true,
            real_estate: true,
            crypto: true,
            other_assets: true,
        }
    }
}

/// User profile stored in database
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct UserProfile {
    pub id: i64,
    pub name: String,
    pub surname: String,
    pub email: String,
    #[serde(rename = "menuPreferences")]
    pub menu_preferences: MenuPreferences,
    pub currency: String,
    pub language: String,
    #[serde(rename = "excludePersonalRealEstate")]
    pub exclude_personal_real_estate: bool,
    #[serde(rename = "coingeckoModalDismissed", default)]
    pub coingecko_modal_dismissed: bool,
    #[serde(rename = "mcpServerEnabled", default)]
    pub mcp_server_enabled: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating/updating user profile
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertUserProfile {
    pub name: String,
    pub surname: String,
    pub email: String,
    #[serde(rename = "menuPreferences")]
    pub menu_preferences: Option<MenuPreferences>,
    pub currency: Option<String>,
    pub language: Option<String>,
    #[serde(rename = "excludePersonalRealEstate")]
    pub exclude_personal_real_estate: Option<bool>,
}

/// Data for updating user profile (partial updates)
#[derive(Debug, Clone, Deserialize, Type)]
pub struct UpdateUserProfile {
    pub name: Option<String>,
    pub surname: Option<String>,
    pub email: Option<String>,
    #[serde(rename = "menuPreferences")]
    pub menu_preferences: Option<MenuPreferences>,
    pub currency: Option<String>,
    pub language: Option<String>,
    #[serde(rename = "excludePersonalRealEstate")]
    pub exclude_personal_real_estate: Option<bool>,
    #[serde(rename = "coingeckoModalDismissed")]
    pub coingecko_modal_dismissed: Option<bool>,
    #[serde(rename = "mcpServerEnabled")]
    pub mcp_server_enabled: Option<bool>,
}

/// Portfolio metrics history entry
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PortfolioMetricsHistory {
    pub id: String,
    #[serde(rename = "totalSavings")]
    pub total_savings: String,
    #[serde(rename = "totalLoansPrincipal")]
    pub total_loans_principal: String,
    #[serde(rename = "totalInvestments")]
    pub total_investments: String,
    #[serde(rename = "totalCrypto")]
    pub total_crypto: String,
    #[serde(rename = "totalBonds")]
    pub total_bonds: String,
    #[serde(rename = "totalRealEstatePersonal")]
    pub total_real_estate_personal: String,
    #[serde(rename = "totalRealEstateInvestment")]
    pub total_real_estate_investment: String,
    #[serde(rename = "totalOtherAssets")]
    pub total_other_assets: String,
    #[serde(rename = "recordedAt")]
    pub recorded_at: i64,
    #[serde(rename = "investmentsByCurrency")]
    pub investments_by_currency: String,
    #[serde(rename = "cryptoByCurrency")]
    pub crypto_by_currency: String,
    #[serde(rename = "savingsByCurrency")]
    pub savings_by_currency: String,
    #[serde(rename = "bondsByCurrency")]
    pub bonds_by_currency: String,
    #[serde(rename = "realEstateByCurrency")]
    pub real_estate_by_currency: String,
    #[serde(rename = "loansByCurrency")]
    pub loans_by_currency: String,
    #[serde(rename = "otherAssetsByCurrency")]
    pub other_assets_by_currency: String,
    /// `"live"` (recorded by the running app) or `"backfill"` (reconstructed;
    /// the static classes of such a day are carried from the nearest live row).
    #[serde(rename = "source")]
    pub source: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_preferences_ignore_the_retired_savings_flag() {
        let stored = r#"{"savings":true,"loans":true,"insurance":false,"investments":true,
            "bonds":true,"realEstate":true,"crypto":false,"otherAssets":true}"#;
        let prefs: MenuPreferences = serde_json::from_str(stored).expect("legacy JSON parses");
        assert!(prefs.loans);
        assert!(!prefs.insurance);
        assert!(!prefs.crypto);

        let written = serde_json::to_string(&prefs).expect("serializes");
        assert!(!written.contains("savings"));
    }

    #[test]
    fn menu_preferences_default_new_classes_to_visible() {
        let stored =
            r#"{"loans":true,"insurance":true,"investments":true,"bonds":true,"realEstate":true}"#;
        let prefs: MenuPreferences = serde_json::from_str(stored).expect("parses");
        assert!(prefs.crypto);
        assert!(prefs.other_assets);
    }
}
