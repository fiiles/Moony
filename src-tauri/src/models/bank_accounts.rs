//! Bank account models for managing bank accounts and their transactions

use serde::{Deserialize, Serialize};
use specta::Type;

/// Bank account types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum AccountType {
    #[default]
    Checking,
    Savings,
    CreditCard,
    Investment,
}

impl std::fmt::Display for AccountType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountType::Checking => write!(f, "checking"),
            AccountType::Savings => write!(f, "savings"),
            AccountType::CreditCard => write!(f, "credit_card"),
            AccountType::Investment => write!(f, "investment"),
        }
    }
}

impl std::str::FromStr for AccountType {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "checking" => Ok(AccountType::Checking),
            "savings" => Ok(AccountType::Savings),
            "credit_card" => Ok(AccountType::CreditCard),
            "investment" => Ok(AccountType::Investment),
            _ => Err(format!("Unknown account type: {}", s)),
        }
    }
}

/// Data source for accounts and transactions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Type)]
#[serde(rename_all = "snake_case")]
pub enum DataSource {
    #[default]
    Manual,
    CsvImport,
    ApiSync,
}

impl std::fmt::Display for DataSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataSource::Manual => write!(f, "manual"),
            DataSource::CsvImport => write!(f, "csv_import"),
            DataSource::ApiSync => write!(f, "api_sync"),
        }
    }
}

impl std::str::FromStr for DataSource {
    type Err = String;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "manual" => Ok(DataSource::Manual),
            "csv_import" => Ok(DataSource::CsvImport),
            "api_sync" => Ok(DataSource::ApiSync),
            _ => Err(format!("Unknown data source: {}", s)),
        }
    }
}

/// Bank account entity
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BankAccount {
    pub id: String,
    pub name: String,
    #[serde(rename = "accountType")]
    pub account_type: String,
    pub iban: Option<String>,
    pub bban: Option<String>,
    pub currency: String,
    pub balance: String,
    #[serde(rename = "balanceAsOf")]
    pub balance_as_of: Option<i64>,
    #[serde(rename = "institutionId")]
    pub institution_id: Option<String>,
    #[serde(rename = "externalAccountId")]
    pub external_account_id: Option<String>,
    #[serde(rename = "dataSource")]
    pub data_source: String,
    #[serde(rename = "lastSyncedAt")]
    pub last_synced_at: Option<i64>,
    // Savings-specific fields
    #[serde(rename = "interestRate")]
    pub interest_rate: Option<String>,
    #[serde(rename = "hasZoneDesignation")]
    pub has_zone_designation: bool,
    #[serde(rename = "terminationDate")]
    pub termination_date: Option<i64>,
    /// UTC day the promotional interest rate ends; None when unknown or not promotional.
    #[serde(rename = "interestRateValidUntil")]
    pub interest_rate_valid_until: Option<i64>,
    /// Left out of net worth, totals and charts when true (still listed and editable).
    #[serde(rename = "excludeFromBalance")]
    pub exclude_from_balance: bool,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
}

/// Data for creating/updating a bank account
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertBankAccount {
    pub name: String,
    #[serde(rename = "accountType")]
    pub account_type: Option<String>,
    pub iban: Option<String>,
    pub bban: Option<String>,
    pub currency: Option<String>,
    pub balance: Option<String>,
    /// UTC day of the opening balance (create only; with it the create writes the first balance record). The update command ignores it.
    #[serde(
        rename = "balanceAsOf",
        default,
        deserialize_with = "deserialize_balance_day"
    )]
    pub balance_as_of: Option<Option<i64>>,
    #[serde(rename = "institutionId")]
    pub institution_id: Option<String>,
    #[serde(rename = "interestRate")]
    pub interest_rate: Option<String>,
    #[serde(rename = "hasZoneDesignation")]
    pub has_zone_designation: Option<bool>,
    #[serde(rename = "terminationDate")]
    pub termination_date: Option<i64>,
    /// Overwritten on update like `terminationDate`: the form always sends the stored value.
    #[serde(rename = "interestRateValidUntil", default)]
    pub interest_rate_valid_until: Option<i64>,
    /// `None` keeps the stored value on update and means "included" on create.
    #[serde(rename = "excludeFromBalance", default)]
    pub exclude_from_balance: Option<bool>,
}

/// Financial institution (bank)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct Institution {
    pub id: String,
    pub name: String,
    pub bic: Option<String>,
    pub country: Option<String>,
    #[serde(rename = "logoUrl")]
    pub logo_url: Option<String>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating an institution
#[derive(Debug, Clone, Deserialize)]
pub struct InsertInstitution {
    pub name: String,
    pub bic: Option<String>,
    pub country: Option<String>,
    #[serde(rename = "logoUrl")]
    pub logo_url: Option<String>,
}

/// Bank account with institution data (enriched)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BankAccountWithInstitution {
    #[serde(flatten)]
    pub account: BankAccount,
    pub institution: Option<Institution>,
    // Computed fields for savings accounts
    #[serde(rename = "effectiveInterestRate")]
    pub effective_interest_rate: Option<f64>,
    #[serde(rename = "projectedEarnings")]
    pub projected_earnings: Option<f64>,
}

// Input validation at trust boundary
use crate::error::{AppError, Result};

impl InsertBankAccount {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        if let Some(day) = self.balance_as_of.flatten() {
            validate_balance_day(day)?;
        }
        // Name validation
        if self.name.is_empty() {
            return Err(AppError::Validation(
                "validation.accountNameRequired".into(),
            ));
        }
        if self.name.len() > 100 {
            return Err(AppError::Validation("validation.accountNameTooLong".into()));
        }

        // Account type validation (if provided)
        if let Some(ref account_type) = self.account_type {
            let valid_types = ["checking", "savings", "credit_card", "investment"];
            if !valid_types.contains(&account_type.to_lowercase().as_str()) {
                return Err(AppError::Validation("validation.accountTypeInvalid".into()));
            }
        }

        // Currency validation (if provided)
        if let Some(ref currency) = self.currency {
            if currency.len() != 3 {
                return Err(AppError::Validation("validation.currencyInvalid".into()));
            }
            if !currency.chars().all(|c| c.is_ascii_alphabetic()) {
                return Err(AppError::Validation("validation.currencyInvalid".into()));
            }
        }

        // Balance validation (if provided)
        if let Some(ref balance) = self.balance {
            if !balance.is_empty() {
                let amount = balance
                    .parse::<f64>()
                    .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
                if !amount.is_finite() {
                    return Err(AppError::Validation("validation.invalidAmount".into()));
                }
            }
        }

        // Interest rate validation (if provided)
        if let Some(ref rate) = self.interest_rate {
            if !rate.is_empty() {
                let rate_val: f64 = rate
                    .parse()
                    .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
                if !(0.0..=100.0).contains(&rate_val) {
                    return Err(AppError::Validation("validation.interestRateRange".into()));
                }
            }
        }

        Ok(())
    }
}

impl InsertInstitution {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        // Name validation
        if self.name.is_empty() {
            return Err(AppError::Validation(
                "validation.institutionNameRequired".into(),
            ));
        }
        if self.name.len() > 100 {
            return Err(AppError::Validation("validation.nameTooLong".into()));
        }

        // BIC validation (if provided) - should be 8 or 11 characters
        if let Some(ref bic) = self.bic {
            if !bic.is_empty() && bic.len() != 8 && bic.len() != 11 {
                return Err(AppError::Validation("validation.bicInvalid".into()));
            }
        }

        // Country validation (if provided)
        if let Some(ref country) = self.country {
            if country.len() > 2 && country.len() != 2 {
                // Allow full country names or 2-letter codes
            }
        }

        Ok(())
    }
}

/// Interest rate zone (tier) of a bank account
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SavingsAccountZone {
    pub id: String,
    #[serde(rename = "savingsAccountId")]
    pub savings_account_id: String,
    #[serde(rename = "fromAmount")]
    pub from_amount: String,
    #[serde(rename = "toAmount")]
    pub to_amount: Option<String>,
    #[serde(rename = "interestRate")]
    pub interest_rate: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

/// Data for creating a zone
#[derive(Debug, Clone, Deserialize)]
pub struct InsertSavingsAccountZone {
    #[serde(rename = "savingsAccountId")]
    pub savings_account_id: String,
    #[serde(rename = "fromAmount")]
    pub from_amount: String,
    #[serde(rename = "toAmount")]
    pub to_amount: Option<String>,
    #[serde(rename = "interestRate")]
    pub interest_rate: String,
}

impl InsertSavingsAccountZone {
    /// Validate input data at the trust boundary
    pub fn validate(&self) -> Result<()> {
        // Bank account ID validation
        if self.savings_account_id.is_empty() {
            return Err(AppError::Validation(
                "Bank account ID cannot be empty".into(),
            ));
        }

        // From amount validation
        let from: f64 = self.from_amount.parse().map_err(|_| {
            AppError::Validation(format!("Invalid from amount '{}'", self.from_amount))
        })?;
        if !from.is_finite() {
            return Err(AppError::Validation("validation.invalidAmount".into()));
        }
        if from < 0.0 {
            return Err(AppError::Validation(
                "From amount cannot be negative".into(),
            ));
        }

        // To amount validation (if provided)
        if let Some(ref to_amount) = self.to_amount {
            if !to_amount.is_empty() {
                let to: f64 = to_amount.parse().map_err(|_| {
                    AppError::Validation(format!("Invalid to amount '{}'", to_amount))
                })?;
                if !to.is_finite() {
                    return Err(AppError::Validation("validation.invalidAmount".into()));
                }
                if to <= from {
                    return Err(AppError::Validation(
                        "To amount must be greater than from amount".into(),
                    ));
                }
            }
        }

        // Interest rate validation
        let rate: f64 = self.interest_rate.parse().map_err(|_| {
            AppError::Validation(format!("Invalid interest rate '{}'", self.interest_rate))
        })?;
        if !(0.0..=100.0).contains(&rate) {
            return Err(AppError::Validation(
                "Interest rate must be between 0 and 100".into(),
            ));
        }

        Ok(())
    }
}

/// A dated balance of a bank account (migration 013). The newest record is mirrored
/// into `bank_accounts.balance` / `balance_as_of`.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BankBalanceRecord {
    pub id: String,
    #[serde(rename = "bankAccountId")]
    pub bank_account_id: String,
    /// Balance in the account currency (canonical decimal string).
    pub amount: String,
    pub currency: String,
    /// UTC day start (unix seconds).
    #[serde(rename = "asOf")]
    pub as_of: i64,
    /// `manual` (entered by hand) or `statement` (confirmed from a CSV import).
    pub source: String,
    #[serde(rename = "sourceBatchId")]
    pub source_batch_id: Option<String>,
    #[serde(rename = "recordedAt")]
    pub recorded_at: i64,
}

/// Input of `record_bank_account_balance`: one dated balance. `source` is derived:
/// `statement` when a batch id is given, else `manual`.
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertBalanceRecord {
    pub amount: String,
    pub currency: String,
    #[serde(rename = "asOf")]
    pub as_of: i64,
    /// Required (true) only while the account has no dated record and its undated
    /// balance is not 0: the first dated record replaces that number.
    #[serde(rename = "confirmUnknownAsOf", default)]
    pub confirm_unknown_as_of: bool,
    #[serde(rename = "sourceBatchId", default)]
    pub source_batch_id: Option<String>,
}

impl InsertBalanceRecord {
    pub fn validate(&self) -> Result<()> {
        let amount = self
            .amount
            .parse::<f64>()
            .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
        if !amount.is_finite() {
            return Err(AppError::Validation("validation.invalidAmount".into()));
        }
        if self.currency.len() != 3 || !self.currency.bytes().all(|b| b.is_ascii_alphabetic()) {
            return Err(AppError::Validation("validation.currencyInvalid".into()));
        }
        validate_balance_day(self.as_of)
    }
}

fn validate_balance_day(day: i64) -> Result<()> {
    if day <= 0
        || day % 86400 != 0
        || chrono::DateTime::from_timestamp(day, 0).is_none()
        || day > chrono::Utc::now().timestamp().div_euclid(86400) * 86400
    {
        return Err(AppError::Validation("validation.balanceAsOfInvalid".into()));
    }
    Ok(())
}

fn deserialize_balance_day<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<Option<i64>>, D::Error> {
    Option::<i64>::deserialize(deserializer).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_balance_record_validates_amount_currency_and_day() {
        let ok = InsertBalanceRecord {
            amount: "1200.50".into(),
            currency: "EUR".into(),
            as_of: 1767312000,
            confirm_unknown_as_of: false,
            source_batch_id: None,
        };
        assert!(ok.validate().is_ok());
        for (amount, currency, as_of) in [
            ("NaN", "EUR", 1767312000),
            ("12", "EU", 1767312000),
            ("12", "EUR", 1767312001),
            ("12", "EUR", i64::MAX),
        ] {
            let bad = InsertBalanceRecord {
                amount: amount.into(),
                currency: currency.into(),
                as_of,
                confirm_unknown_as_of: false,
                source_batch_id: None,
            };
            assert!(
                bad.validate().is_err(),
                "accepted {amount} {currency} {as_of}"
            );
        }
    }
}
