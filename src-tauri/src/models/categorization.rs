use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};

/// Explicit opt-in: one own account, movement direction and reliable account/payee identity.
#[derive(Debug, Clone, Deserialize, specta::Type)]
pub struct InsertScopedLearning {
    #[serde(rename = "bankAccountId")]
    pub bank_account_id: String,
    #[serde(rename = "isCredit")]
    pub is_credit: bool,
    #[serde(rename = "payee")]
    pub payee: Option<String>,
    #[serde(rename = "counterpartyIban")]
    pub counterparty_iban: Option<String>,
    #[serde(rename = "counterpartyBankCode")]
    pub counterparty_bank_code: Option<String>,
    #[serde(rename = "counterpartyCountry")]
    pub counterparty_country: Option<String>,
    #[serde(rename = "categoryId")]
    pub category_id: String,
}
impl InsertScopedLearning {
    pub fn validate(&self) -> Result<()> {
        if self.bank_account_id.trim().is_empty() || self.category_id.trim().is_empty() {
            return Err(AppError::Validation("validation.learningIdentity".into()));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, specta::Type)]
pub struct LearnedPayeeEntry {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "ruleType")]
    pub rule_type: String,
    #[serde(rename = "normalizedPayee")]
    pub normalized_payee: Option<String>,
    #[serde(rename = "originalPayee")]
    pub original_payee: Option<String>,
    #[serde(rename = "counterpartyIban")]
    pub counterparty_iban: Option<String>,
    #[serde(rename = "categoryId")]
    pub category_id: String,
    #[serde(rename = "bankAccountId")]
    pub bank_account_id: Option<String>,
    #[serde(rename = "isCredit")]
    pub is_credit: Option<bool>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// One current-category bucket of the payments a rewritten rule covers.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct CategoryCount {
    /// `None` = the payments without a category.
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
    #[serde(rename = "count")]
    pub count: usize,
}

/// A payment a bulk recategorization changed, with what it held before (for undo).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct ChangedRow {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "previousCategoryId")]
    pub previous_category_id: Option<String>,
    #[serde(rename = "previousSource")]
    pub previous_source: Option<String>,
}

/// The custom rule that decides a transaction's category today.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct GoverningRule {
    #[serde(rename = "id")]
    pub id: String,
    #[serde(rename = "name")]
    pub name: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
}

/// What rewriting the rule behind a transaction would cover (spec 2026-10-10).
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct RecategorizationPreview {
    /// `ownAccount` | `custom` | `learned` | `pack` | `none` — the layer that decides today.
    #[serde(rename = "governing")]
    pub governing: String,
    /// Set when `governing` is `custom`.
    #[serde(rename = "rule")]
    pub rule: Option<GoverningRule>,
    /// False when neither a payee key nor an account identity exists (nothing to remember).
    #[serde(rename = "learnable")]
    pub learnable: bool,
    /// Other payments the rewritten rule matches, the edited row excluded.
    #[serde(rename = "matches")]
    pub matches: usize,
    #[serde(rename = "byCategory")]
    pub by_category: Vec<CategoryCount>,
}
