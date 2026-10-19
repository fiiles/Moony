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

// ==================== The categorize queue (phone, spec 2026-10-17 §8.5) ====================

/// The most payments one snooze call may name.
const SNOOZE_MAX_TRANSACTIONS: usize = 1000;
/// The longest a payment may be held back from the queue.
const SNOOZE_MAX_DAYS: i64 = 365;

/// Where a suggested category comes from (the pill's sub-label on the phone).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub enum SuggestionSource {
    /// An AI client suggested it through the local API and waits for the user.
    #[serde(rename = "mcp")]
    Mcp,
    /// A payee the user categorized before.
    #[serde(rename = "learned")]
    Learned,
    /// A custom rule, or a transfer between the user's own accounts.
    #[serde(rename = "rule")]
    Rule,
    /// A rule of an enabled locale pack.
    #[serde(rename = "pack")]
    Pack,
}

/// A category the app would give a payment, and where the idea comes from.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type, PartialEq, Eq)]
pub struct CategorySuggestion {
    #[serde(rename = "categoryId")]
    pub category_id: String,
    #[serde(rename = "source")]
    pub source: SuggestionSource,
}

/// One payment of the queue and the category suggested for it, if any.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CategorizeQueueRow {
    #[serde(rename = "transaction")]
    pub transaction: crate::models::bank_transactions::BankTransaction,
    #[serde(rename = "suggestion")]
    pub suggestion: Option<CategorySuggestion>,
}

/// Booked payments without a category, newest first.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
pub struct CategorizeQueue {
    /// The newest payments, capped; `total` counts all of them.
    #[serde(rename = "rows")]
    pub rows: Vec<CategorizeQueueRow>,
    /// Every payment waiting in the queue (not snoozed), listed or not.
    #[serde(rename = "total")]
    pub total: i64,
    /// Uncategorized payments held back from the queue by a snooze that is still running.
    #[serde(rename = "snoozed")]
    pub snoozed: i64,
}

/// Hold uncategorized payments back from the queue for a number of days.
#[derive(Debug, Clone, Deserialize, specta::Type)]
pub struct SnoozeCategorization {
    #[serde(rename = "transactionIds")]
    pub transaction_ids: Vec<String>,
    #[serde(rename = "days")]
    pub days: i64,
}

impl SnoozeCategorization {
    pub fn validate(&self) -> Result<()> {
        if self.transaction_ids.is_empty()
            || self.transaction_ids.len() > SNOOZE_MAX_TRANSACTIONS
            || self.transaction_ids.iter().any(|id| id.trim().is_empty())
        {
            return Err(AppError::Validation(
                "validation.snoozeSelectionInvalid".into(),
            ));
        }
        if !(1..=SNOOZE_MAX_DAYS).contains(&self.days) {
            return Err(AppError::Validation("validation.snoozeDaysInvalid".into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snooze(ids: &[&str], days: i64) -> SnoozeCategorization {
        SnoozeCategorization {
            transaction_ids: ids.iter().map(|id| id.to_string()).collect(),
            days,
        }
    }

    fn rejected_with(input: &SnoozeCategorization) -> String {
        match input.validate() {
            Err(AppError::Validation(key)) => key,
            other => panic!("expected a validation error, got {other:?}"),
        }
    }

    #[test]
    fn snooze_accepts_one_to_a_thousand_ids_and_one_to_365_days() {
        assert!(snooze(&["a"], 1).validate().is_ok());
        assert!(snooze(&["a", "b"], 30).validate().is_ok());
        assert!(snooze(&["a"], 365).validate().is_ok());
        let thousand: Vec<String> = (0..1000).map(|n| format!("t{n}")).collect();
        let full = SnoozeCategorization {
            transaction_ids: thousand,
            days: 30,
        };
        assert!(full.validate().is_ok());
    }

    #[test]
    fn snooze_rejects_no_ids_too_many_ids_and_blank_ids() {
        assert_eq!(
            rejected_with(&snooze(&[], 30)),
            "validation.snoozeSelectionInvalid"
        );
        let too_many = SnoozeCategorization {
            transaction_ids: (0..1001).map(|n| format!("t{n}")).collect(),
            days: 30,
        };
        assert_eq!(
            rejected_with(&too_many),
            "validation.snoozeSelectionInvalid"
        );
        assert_eq!(
            rejected_with(&snooze(&["a", ""], 30)),
            "validation.snoozeSelectionInvalid"
        );
        assert_eq!(
            rejected_with(&snooze(&["  "], 30)),
            "validation.snoozeSelectionInvalid"
        );
    }

    #[test]
    fn snooze_rejects_days_outside_one_to_365() {
        for days in [0, -1, 366, i64::MAX] {
            assert_eq!(
                rejected_with(&snooze(&["a"], days)),
                "validation.snoozeDaysInvalid",
                "{days}"
            );
        }
    }

    #[test]
    fn suggestion_sources_serialize_as_lowercase_words() {
        let json = serde_json::to_value(CategorySuggestion {
            category_id: "cat_food".into(),
            source: SuggestionSource::Mcp,
        })
        .expect("serialize");
        assert_eq!(
            json,
            serde_json::json!({ "categoryId": "cat_food", "source": "mcp" })
        );
        for (source, word) in [
            (SuggestionSource::Mcp, "mcp"),
            (SuggestionSource::Learned, "learned"),
            (SuggestionSource::Rule, "rule"),
            (SuggestionSource::Pack, "pack"),
        ] {
            assert_eq!(
                serde_json::to_value(source).expect("serialize"),
                serde_json::json!(word)
            );
        }
    }
}
