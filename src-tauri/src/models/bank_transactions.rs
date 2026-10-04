//! Bank transaction models for tracking account transactions

use serde::{Deserialize, Serialize};
use specta::Type;

/// Transaction type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum TransactionType {
    Credit,
    Debit,
}

impl std::fmt::Display for TransactionType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransactionType::Credit => write!(f, "credit"),
            TransactionType::Debit => write!(f, "debit"),
        }
    }
}

impl std::str::FromStr for TransactionType {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "credit" => Ok(TransactionType::Credit),
            "debit" => Ok(TransactionType::Debit),
            _ => Err(format!("Unknown transaction type: {}", s)),
        }
    }
}

/// Transaction status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Type)]
#[serde(rename_all = "lowercase")]
pub enum TransactionStatus {
    #[default]
    Booked,
    Pending,
}

impl std::fmt::Display for TransactionStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransactionStatus::Booked => write!(f, "booked"),
            TransactionStatus::Pending => write!(f, "pending"),
        }
    }
}

/// Bank transaction entity
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BankTransaction {
    pub id: String,
    #[serde(rename = "bankAccountId")]
    pub bank_account_id: String,
    #[serde(rename = "transactionId")]
    pub transaction_id: Option<String>,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub amount: String,
    pub currency: String,
    pub description: Option<String>,
    #[serde(rename = "counterpartyName")]
    pub counterparty_name: Option<String>,
    #[serde(rename = "counterpartyIban")]
    pub counterparty_iban: Option<String>,
    #[serde(rename = "bookingDate")]
    pub booking_date: i64,
    #[serde(rename = "valueDate")]
    pub value_date: Option<i64>,
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
    #[serde(rename = "merchantCategoryCode")]
    pub merchant_category_code: Option<String>,
    #[serde(rename = "remittanceInfo")]
    pub remittance_info: Option<String>,
    /// Category suggested by an MCP client, pending user confirmation
    #[serde(rename = "suggestedCategoryId")]
    pub suggested_category_id: Option<String>,
    pub status: String,
    #[serde(rename = "dataSource")]
    pub data_source: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating/updating a transaction
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertBankTransaction {
    #[serde(rename = "bankAccountId")]
    pub bank_account_id: String,
    #[serde(rename = "transactionId")]
    pub transaction_id: Option<String>,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub amount: String,
    pub currency: Option<String>,
    pub description: Option<String>,
    #[serde(rename = "counterpartyName")]
    pub counterparty_name: Option<String>,
    #[serde(rename = "counterpartyIban")]
    pub counterparty_iban: Option<String>,
    #[serde(rename = "bookingDate")]
    pub booking_date: i64,
    #[serde(rename = "valueDate")]
    pub value_date: Option<i64>,
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
    pub status: Option<String>,
}

// Input validation at trust boundary
use crate::error::{AppError, Result};

impl InsertBankTransaction {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        let tx_type = self.tx_type.to_lowercase();
        if tx_type != "credit" && tx_type != "debit" {
            return Err(AppError::Validation(
                "validation.transactionTypeInvalid".into(),
            ));
        }

        let amount: f64 = self
            .amount
            .parse()
            .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
        if amount <= 0.0 {
            return Err(AppError::Validation("validation.amountRequired".into()));
        }

        if let Some(ref currency) = self.currency {
            if currency.len() != 3 || !currency.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(AppError::Validation("validation.currencyInvalid".into()));
            }
        }

        if self.booking_date <= 0 {
            return Err(AppError::Validation("validation.bookingDateInvalid".into()));
        }

        if let Some(ref status) = self.status {
            let s = status.to_lowercase();
            if s != "booked" && s != "pending" {
                return Err(AppError::Validation("validation.statusInvalid".into()));
            }
        }

        Ok(())
    }
}

/// Transaction category
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TransactionCategory {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    #[serde(rename = "sortOrder")]
    pub sort_order: i32,
    #[serde(rename = "isSystem")]
    pub is_system: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Longest category name accepted (characters, after trimming)
pub const CATEGORY_NAME_MAX_CHARS: usize = 50;

/// Data for creating a category
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertTransactionCategory {
    pub name: String,
    pub icon: Option<String>,
    pub color: Option<String>,
    #[serde(rename = "parentId")]
    pub parent_id: Option<String>,
    #[serde(rename = "sortOrder")]
    pub sort_order: Option<i32>,
}

impl InsertTransactionCategory {
    /// Validate input data at the trust boundary. Uniqueness of the name is a
    /// database question and is checked by the service.
    pub fn validate(&self) -> Result<()> {
        validate_category_name(&self.name)?;
        if let Some(ref icon) = self.icon {
            validate_category_icon(icon)?;
        }
        if let Some(ref color) = self.color {
            validate_category_color(color)?;
        }
        Ok(())
    }
}

/// Data for editing a category's definition. A field that is absent (`None`)
/// is left unchanged, so an edit dialog can send only what the user touched.
#[derive(Debug, Clone, Default, Deserialize, Type)]
pub struct UpdateTransactionCategory {
    pub name: Option<String>,
    pub icon: Option<String>,
    pub color: Option<String>,
}

impl UpdateTransactionCategory {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        if let Some(ref name) = self.name {
            validate_category_name(name)?;
        }
        if let Some(ref icon) = self.icon {
            validate_category_icon(icon)?;
        }
        if let Some(ref color) = self.color {
            validate_category_color(color)?;
        }
        Ok(())
    }
}

/// How many rows reference a category — what a delete would have to move.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CategoryUsage {
    #[serde(rename = "categoryId")]
    pub category_id: String,
    pub transactions: i64,
    pub rules: i64,
    #[serde(rename = "learnedPayees")]
    pub learned_payees: i64,
    #[serde(rename = "budgetGoals")]
    pub budget_goals: i64,
}

impl CategoryUsage {
    /// Total number of referencing rows
    pub fn total(&self) -> i64 {
        self.transactions + self.rules + self.learned_payees + self.budget_goals
    }
}

/// Trimmed, non-empty and at most [`CATEGORY_NAME_MAX_CHARS`] characters.
fn validate_category_name(name: &str) -> Result<()> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(AppError::Validation(
            "validation.categoryNameRequired".into(),
        ));
    }
    if trimmed.chars().count() > CATEGORY_NAME_MAX_CHARS {
        return Err(AppError::Validation(
            "validation.categoryNameTooLong".into(),
        ));
    }
    Ok(())
}

/// Icons are lucide-style names (`piggy-bank`): lowercase ASCII, digits, hyphens.
fn validate_category_icon(icon: &str) -> Result<()> {
    let ok = !icon.is_empty()
        && icon.len() <= 40
        && icon
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok {
        Ok(())
    } else {
        Err(AppError::Validation(
            "validation.categoryIconInvalid".into(),
        ))
    }
}

/// Colors are `#RRGGBB`.
fn validate_category_color(color: &str) -> Result<()> {
    let ok = color.len() == 7
        && color.starts_with('#')
        && color[1..].chars().all(|c| c.is_ascii_hexdigit());
    if ok {
        Ok(())
    } else {
        Err(AppError::Validation(
            "validation.categoryColorInvalid".into(),
        ))
    }
}

/// Transaction with category info (enriched)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BankTransactionWithCategory {
    #[serde(flatten)]
    pub transaction: BankTransaction,
    pub category: Option<TransactionCategory>,
}

/// Filters for querying transactions
#[derive(Debug, Clone, Default, Deserialize, Type)]
pub struct TransactionFilters {
    #[serde(rename = "dateFrom")]
    pub date_from: Option<i64>,
    #[serde(rename = "dateTo")]
    pub date_to: Option<i64>,
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
    #[serde(rename = "txType")]
    pub tx_type: Option<String>,
    pub search: Option<String>,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// Result of transaction query with pagination
#[derive(Debug, Clone, Serialize, Type)]
pub struct TransactionQueryResult {
    pub transactions: Vec<BankTransaction>,
    pub total: i64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    fn valid_tx() -> InsertBankTransaction {
        InsertBankTransaction {
            bank_account_id: "acc-1".into(),
            transaction_id: None,
            tx_type: "debit".into(),
            amount: "250.50".into(),
            currency: Some("CZK".into()),
            description: Some("groceries".into()),
            counterparty_name: None,
            counterparty_iban: None,
            booking_date: 1700000000,
            value_date: None,
            category_id: None,
            status: None,
        }
    }

    #[test]
    fn validate_accepts_valid_transaction() {
        assert!(valid_tx().validate().is_ok());
    }

    #[test]
    fn validate_rejects_bad_tx_type() {
        let mut tx = valid_tx();
        tx.tx_type = "transfer".into();
        assert!(matches!(
            tx.validate().unwrap_err(),
            AppError::Validation(_)
        ));
    }

    #[test]
    fn validate_rejects_non_numeric_or_non_positive_amount() {
        let mut tx = valid_tx();
        tx.amount = "abc".into();
        assert!(tx.validate().is_err());
        tx.amount = "-5".into();
        assert!(tx.validate().is_err());
        tx.amount = "0".into();
        assert!(tx.validate().is_err());
    }

    #[test]
    fn validate_rejects_bad_currency() {
        let mut tx = valid_tx();
        tx.currency = Some("CZKX".into());
        assert!(tx.validate().is_err());
        tx.currency = Some("C1K".into());
        assert!(tx.validate().is_err());
        tx.currency = None;
        assert!(
            tx.validate().is_ok(),
            "currency is optional (account fallback)"
        );
    }

    #[test]
    fn validate_rejects_non_positive_booking_date() {
        let mut tx = valid_tx();
        tx.booking_date = 0;
        assert!(tx.validate().is_err());
    }

    #[test]
    fn validate_rejects_bad_status() {
        let mut tx = valid_tx();
        tx.status = Some("imaginary".into());
        assert!(tx.validate().is_err());
        tx.status = Some("pending".into());
        assert!(tx.validate().is_ok());
    }
}
