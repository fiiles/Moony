//! Cashflow item models for user-defined income/expense entries

use serde::{Deserialize, Serialize};
use specta::Type;

/// User-defined cashflow item (income or expense)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CashflowItem {
    pub id: String,
    pub name: String,
    pub amount: String,
    pub currency: String,
    pub frequency: String, // "monthly", "quarterly" or "yearly"
    #[serde(rename = "itemType")]
    pub item_type: String, // "income" or "expense"
    #[serde(rename = "replacementSourceId")]
    pub replacement_source_id: Option<String>,
    pub category: String, // Category key e.g. "savingsInterest", "rentalIncome", etc.
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// Data for creating/updating cashflow item
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertCashflowItem {
    pub name: String,
    pub amount: String,
    pub currency: Option<String>,
    pub frequency: String, // "monthly", "quarterly" or "yearly"
    #[serde(rename = "itemType")]
    pub item_type: String, // "income" or "expense"
    #[serde(rename = "replacementSourceId")]
    pub replacement_source_id: Option<String>,
    pub category: String, // Category key
}

/// Category keys of the planning report (personal first, then investments).
pub const CASHFLOW_CATEGORIES: [&str; 16] = [
    "personalIncome",
    "personalLoans",
    "personalInsurance",
    "personalRealEstateCosts",
    "subscriptions",
    "consumptionCosts",
    "otherPersonalCosts",
    "interestIncome",
    "stockDividends",
    "bondsInterest",
    "rentalIncome",
    "otherInvestmentIncome",
    "investmentLoans",
    "investmentInsurance",
    "investmentRealEstateCosts",
    "otherInvestmentCosts",
];

/// Type prefixes of automatic source ids (`<prefix>:<native id>`), shared by
/// replacements (`cashflow_items.replacement_source_id`) and plan exclusions.
pub const PLAN_SOURCE_PREFIXES: [&str; 7] = [
    "loan",
    "insurance",
    "propertyCosts",
    "account",
    "stock",
    "bond",
    "rental",
];

/// Count a planning row in or leave it out (`cashflow_plan_exclusions`).
#[derive(Debug, Clone, Deserialize, Type)]
pub struct SetCashflowPlanInclusion {
    /// `category:<categoryKey>`, an automatic source id, or `item:<manual item id>`.
    pub key: String,
    pub included: bool,
}

impl SetCashflowPlanInclusion {
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::AppError;
        let invalid = || AppError::Validation("validation.cashflowPlanKeyInvalid".into());
        let (prefix, rest) = self.key.split_once(':').ok_or_else(invalid)?;
        if rest.is_empty() || rest.chars().any(char::is_whitespace) {
            return Err(invalid());
        }
        let known = match prefix {
            "category" => CASHFLOW_CATEGORIES.contains(&rest),
            "item" => true,
            other => PLAN_SOURCE_PREFIXES.contains(&other),
        };
        if known {
            Ok(())
        } else {
            Err(invalid())
        }
    }
}

impl InsertCashflowItem {
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::AppError;
        let amount = crate::services::reporting::parse_money(&self.amount)?;
        if amount <= 0.0
            || self.name.trim().is_empty()
            || !["monthly", "quarterly", "yearly"].contains(&self.frequency.as_str())
            || !["income", "expense"].contains(&self.item_type.as_str())
            || self
                .currency
                .as_ref()
                .is_some_and(|c| c.len() != 3 || !c.bytes().all(|b| b.is_ascii_uppercase()))
            || self
                .replacement_source_id
                .as_ref()
                .is_some_and(|id| id.trim().is_empty())
        {
            return Err(AppError::Validation(
                "validation.cashflowItemInvalid".into(),
            ));
        }
        if !CASHFLOW_CATEGORIES.contains(&self.category.as_str()) {
            return Err(AppError::Validation(
                "validation.cashflowItemInvalid".into(),
            ));
        }
        let income_categories = [
            "personalIncome",
            "interestIncome",
            "stockDividends",
            "bondsInterest",
            "rentalIncome",
            "otherInvestmentIncome",
        ];
        if income_categories.contains(&self.category.as_str()) != (self.item_type == "income") {
            return Err(AppError::Validation(
                "validation.cashflowItemInvalid".into(),
            ));
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn plan_inclusion_keys_are_validated() {
        let ok = |key: &str| {
            SetCashflowPlanInclusion {
                key: key.into(),
                included: false,
            }
            .validate()
        };
        assert!(ok("category:stockDividends").is_ok());
        assert!(ok("item:0f3c").is_ok());
        for prefix in PLAN_SOURCE_PREFIXES {
            assert!(ok(&format!("{prefix}:abc")).is_ok(), "{prefix}");
        }
        for bad in [
            "",
            "stockDividends",
            "category:",
            "category:unknown",
            "item:",
            "item:has space",
            "crypto:btc",
            " loan:abc",
        ] {
            assert!(ok(bad).is_err(), "accepted {bad:?}");
        }
    }
    #[test]
    fn planned_items_reject_nonfinite_and_invalid_enum_values() {
        let mut item = InsertCashflowItem {
            name: "Rent".into(),
            amount: "NaN".into(),
            currency: Some("CZK".into()),
            frequency: "monthly".into(),
            item_type: "expense".into(),
            category: "personalLoans".into(),
            replacement_source_id: None,
        };
        for amount in ["NaN", "inf", "-inf", "0", "-1"] {
            item.amount = amount.into();
            assert!(item.validate().is_err(), "accepted {amount}");
        }
        item.amount = "100".into();
        item.frequency = "quarterly".into();
        assert!(item.validate().is_ok());
        item.frequency = "weekly".into();
        assert!(item.validate().is_err());
    }
}

/// Individual item in a cashflow category
#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
pub struct CashflowReportItem {
    #[serde(rename = "originalComponents")]
    pub original_components: Vec<CashflowOriginalComponent>,
    #[serde(rename = "convertedComponentCount")]
    pub converted_component_count: usize,
    #[serde(rename = "fxQuality")]
    pub fx_quality: crate::models::FxQuality,
    pub category: String,
    #[serde(rename = "itemType")]
    pub item_type: String,
    #[serde(rename = "sourceId")]
    pub source_id: Option<String>,
    #[serde(rename = "replacementSourceId")]
    pub replacement_source_id: Option<String>,
    pub id: String,
    pub name: String,
    pub amount: String, // Amount in user's selected period (monthly, quarterly or yearly)
    #[serde(rename = "originalAmount")]
    pub original_amount: String, // Original amount before period conversion
    #[serde(rename = "originalCurrency")]
    pub original_currency: String,
    #[serde(rename = "originalFrequency")]
    pub original_frequency: String, // "monthly", "quarterly" or "yearly"
    #[serde(rename = "isUserDefined")]
    pub is_user_defined: bool,
    /// False when this row or its whole category is out of the plan; `amount` is still filled.
    pub included: bool,
    /// False when the row's own key is stored, whatever its category says.
    #[serde(rename = "selfIncluded")]
    pub self_included: bool,
}

/// Native components of a composite automatic property-cost source.
#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
pub struct CashflowOriginalComponent {
    pub amount: String,
    pub currency: String,
    pub frequency: String,
}

/// A category with its items
#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
pub struct CashflowCategory {
    pub key: String,
    pub name: String,
    /// Sum of the included items only.
    pub total: String,
    pub items: Vec<CashflowReportItem>,
    #[serde(rename = "isUserEditable")]
    pub is_user_editable: bool,
    /// False when the whole category is out of the plan (`category:<key>`).
    pub included: bool,
}

/// A cashflow section (Personal or Investments)
#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
pub struct CashflowSection {
    pub income: Vec<CashflowCategory>,
    pub expenses: Vec<CashflowCategory>,
    #[serde(rename = "totalIncome")]
    pub total_income: String,
    #[serde(rename = "totalExpenses")]
    pub total_expenses: String,
    #[serde(rename = "netCashflow")]
    pub net_cashflow: String,
}

/// Complete cashflow report with Personal and Investments sections
#[derive(Debug, Clone, Serialize, serde::Deserialize, specta::Type)]
pub struct CashflowReport {
    #[serde(rename = "reportCurrency")]
    pub report_currency: String,
    pub quality: crate::models::ReportQuality,
    #[serde(rename = "ratesAsOf")]
    pub rates_as_of: Option<i64>,
    #[serde(rename = "automaticSources")]
    pub automatic_sources: Vec<CashflowReportItem>,
    #[serde(rename = "asOf")]
    pub as_of: i64,
    pub estimate: bool,
    #[serde(rename = "viewType")]
    pub view_type: String, // "monthly", "quarterly" or "yearly"
    pub personal: CashflowSection,
    pub investments: CashflowSection,
    #[serde(rename = "totalIncome")]
    pub total_income: String,
    #[serde(rename = "totalExpenses")]
    pub total_expenses: String,
    #[serde(rename = "netCashflow")]
    pub net_cashflow: String,
}
