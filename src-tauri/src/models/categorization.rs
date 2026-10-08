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
