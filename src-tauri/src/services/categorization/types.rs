//! Core types for the categorization engine

use serde::{Deserialize, Serialize};
use specta::Type;

/// Source of categorization (for audit trail and analytics)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
#[serde(tag = "type", content = "data")]
pub enum CategorizationSource {
    /// Matched by a user-defined or system rule
    Rule { rule_id: String, rule_name: String },
    /// Matched by exact payee lookup (learned from user)
    ExactMatch { payee: String },
    /// Manually set by user
    Manual,
}

/// Result of a categorization attempt
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(tag = "type", content = "data")]
pub enum CategorizationResult {
    /// Definitive match from rules or exact payee match
    Match {
        #[serde(rename = "categoryId")]
        category_id: String,
        source: CategorizationSource,
    },
    /// No categorization could be determined
    None,
}

impl CategorizationResult {
    /// Check if this result carries a category
    pub fn has_category(&self) -> bool {
        matches!(self, CategorizationResult::Match { .. })
    }

    /// Get the category ID if present
    pub fn category_id(&self) -> Option<&str> {
        match self {
            CategorizationResult::Match { category_id, .. } => Some(category_id),
            CategorizationResult::None => None,
        }
    }
}

/// Transaction data structure for categorization (subset of full BankTransaction)
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TransactionInput {
    pub id: String,
    pub description: Option<String>,
    pub counterparty: Option<String>,
    #[serde(rename = "counterpartyIban")]
    pub counterparty_iban: Option<String>,
    pub amount: f64,
    #[serde(rename = "isCredit")]
    pub is_credit: bool,
    /// Source bank account ID (for learning context)
    #[serde(rename = "bankAccountId")]
    pub bank_account_id: Option<String>,
}

impl TransactionInput {
    /// Create from raw transaction data
    pub fn new(
        id: String,
        description: Option<String>,
        counterparty: Option<String>,
        amount: f64,
    ) -> Self {
        Self {
            id,
            description,
            counterparty,
            counterparty_iban: None,
            is_credit: amount >= 0.0,
            amount,
            bank_account_id: None,
        }
    }

    /// Get combined text for matching (description + counterparty + counterparty_iban)
    pub fn combined_text(&self) -> String {
        let mut parts = Vec::new();
        if let Some(ref desc) = self.description {
            parts.push(desc.as_str());
        }
        if let Some(ref cp) = self.counterparty {
            parts.push(cp.as_str());
        }
        if let Some(ref iban) = self.counterparty_iban {
            parts.push(iban.as_str());
        }
        parts.join(" ")
    }
}

/// Rule types for pattern matching
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Type)]
pub enum RuleType {
    /// Full regex pattern (most flexible)
    Regex,
    /// Simple case-insensitive substring match
    Contains,
    /// Whole-word match (pattern wrapped in word boundaries) — the safe
    /// choice for short patterns that would collide as substrings
    Word,
    /// Text starts with pattern
    StartsWith,
    /// Text ends with pattern
    EndsWith,
    /// Match if transaction is a credit (incoming money)
    IsCredit,
    /// Match if transaction is a debit (outgoing money)
    IsDebit,
}

impl std::fmt::Display for RuleType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RuleType::Regex => write!(f, "regex"),
            RuleType::Contains => write!(f, "contains"),
            RuleType::Word => write!(f, "word"),
            RuleType::StartsWith => write!(f, "starts_with"),
            RuleType::EndsWith => write!(f, "ends_with"),
            RuleType::IsCredit => write!(f, "is_credit"),
            RuleType::IsDebit => write!(f, "is_debit"),
        }
    }
}

impl std::str::FromStr for RuleType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "regex" => Ok(RuleType::Regex),
            "contains" => Ok(RuleType::Contains),
            "word" => Ok(RuleType::Word),
            "starts_with" | "startswith" => Ok(RuleType::StartsWith),
            "ends_with" | "endswith" => Ok(RuleType::EndsWith),
            "is_credit" | "credit" => Ok(RuleType::IsCredit),
            "is_debit" | "debit" => Ok(RuleType::IsDebit),
            _ => Err(format!("Unknown rule type: {}", s)),
        }
    }
}

/// Enhanced categorization rule with stop_processing flag and compound matching
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CategorizationRule {
    pub id: String,
    pub name: String,
    #[serde(rename = "ruleType")]
    pub rule_type: RuleType,
    pub pattern: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
    pub priority: i32,
    #[serde(rename = "isActive")]
    pub is_active: bool,
    /// If true, stop the waterfall when this rule matches
    #[serde(rename = "stopProcessing")]
    pub stop_processing: bool,
    /// Optional IBAN/BBAN pattern to match against counterparty IBAN
    #[serde(rename = "ibanPattern")]
    pub iban_pattern: Option<String>,
}

impl CategorizationRule {
    /// Create a new rule with default values
    pub fn new(
        id: String,
        name: String,
        rule_type: RuleType,
        pattern: String,
        category_id: String,
    ) -> Self {
        Self {
            id,
            name,
            rule_type,
            pattern,
            category_id,
            priority: 50,
            is_active: true,
            stop_processing: false,
            iban_pattern: None,
        }
    }
}

/// Batch categorization request
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BatchCategorizationRequest {
    pub transactions: Vec<TransactionInput>,
}

/// Batch categorization response
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BatchCategorizationResponse {
    pub results: Vec<CategorizationResult>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_input_combined_text() {
        let tx = TransactionInput {
            id: "1".into(),
            description: Some("Payment for goods".into()),
            counterparty: Some("Albert CZ".into()),
            counterparty_iban: None,
            amount: -500.0,
            is_credit: false,
            bank_account_id: None,
        };

        assert_eq!(tx.combined_text(), "Payment for goods Albert CZ");
    }

    #[test]
    fn test_categorization_result_category_id() {
        let match_result = CategorizationResult::Match {
            category_id: "cat_groceries".into(),
            source: CategorizationSource::Rule {
                rule_id: "r1".into(),
                rule_name: "Test".into(),
            },
        };
        assert_eq!(match_result.category_id(), Some("cat_groceries"));
        assert!(match_result.has_category());

        let none = CategorizationResult::None;
        assert_eq!(none.category_id(), None);
    }

    #[test]
    fn test_rule_type_from_str() {
        assert_eq!("regex".parse::<RuleType>().unwrap(), RuleType::Regex);
        assert_eq!("contains".parse::<RuleType>().unwrap(), RuleType::Contains);
        assert_eq!("word".parse::<RuleType>().unwrap(), RuleType::Word);
        assert!("invalid".parse::<RuleType>().is_err());
    }
}
