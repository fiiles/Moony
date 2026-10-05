//! Milestones ("Co vás čeká"): upcoming dates and upkeep reminders gathered from every
//! domain (spec 2026-10-05-milestones-design).

use crate::error::{AppError, Result};
use serde::{Deserialize, Serialize};
use specta::Type;

/// One occurrence of something the user should act on or know about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct Milestone {
    /// Stable occurrence key; the done / snoozed state is stored under it.
    pub key: String,
    /// `insurance_anniversary`, `insurance_end`, `insurance_payment`, `loan_fixation_end`,
    /// `loan_fixation_expired`, `loan_payoff`, `loan_balance_check`, `bond_maturity`,
    /// `bond_coupon`, `account_termination`, `savings_rate_end`, `balances_stale`,
    /// `backup_stale`, `valuation_stale`, `watch_target`.
    pub kind: String,
    /// `now` (in the reminder window) or `soon` (within 90 days).
    pub stage: String,
    /// `action` (counts in the top-bar indicator) or `info`.
    pub tone: String,
    /// Policy, loan, bond, account or property id; the ticker for targets.
    #[serde(rename = "sourceId")]
    pub source_id: Option<String>,
    /// Entity name; empty for the backup and balances items.
    pub title: String,
    /// UTC day of the event.
    #[serde(rename = "dueDay")]
    pub due_day: Option<i64>,
    /// Last day to act when it differs from the event (insurance notice deadline).
    #[serde(rename = "actionDay")]
    pub action_day: Option<i64>,
    /// Day of the last backup, balance update, valuation or balance check.
    #[serde(rename = "sinceDay")]
    pub since_day: Option<i64>,
    /// Payment, coupon, returned principal or target price (TEXT money).
    pub amount: Option<String>,
    pub currency: Option<String>,
    /// Current price of a crossed target.
    #[serde(rename = "referenceAmount")]
    pub reference_amount: Option<String>,
    /// `below` / `above` for targets.
    pub direction: Option<String>,
    /// Number of stale accounts.
    pub count: Option<i64>,
}

/// Hidden for good (this occurrence only).
pub const MILESTONE_STATE_DONE: &str = "done";
/// Hidden for a week.
pub const MILESTONE_STATE_SNOOZED: &str = "snoozed";
const MAX_KEY_CHARS: usize = 256;

/// Every kind the service emits; muted kinds must be one of these.
pub const MILESTONE_KINDS: [&str; 15] = [
    "insurance_anniversary",
    "insurance_end",
    "insurance_payment",
    "loan_fixation_end",
    "loan_fixation_expired",
    "loan_payoff",
    "loan_balance_check",
    "bond_maturity",
    "bond_coupon",
    "account_termination",
    "savings_rate_end",
    "balances_stale",
    "backup_stale",
    "valuation_stale",
    "watch_target",
];

/// Muted kinds ("Nepřipomínat …") must be kinds the service knows.
pub fn validate_milestone_kinds(kinds: &[String]) -> Result<()> {
    if kinds.iter().all(|k| MILESTONE_KINDS.contains(&k.trim())) {
        Ok(())
    } else {
        Err(AppError::Validation(
            "validation.milestoneKindUnknown".to_string(),
        ))
    }
}

/// A milestone key is non-empty and at most 256 characters.
pub fn validate_milestone_key(key: &str) -> Result<()> {
    let key = key.trim();
    if key.is_empty() || key.chars().count() > MAX_KEY_CHARS {
        return Err(AppError::Validation(
            "validation.milestoneKeyInvalid".to_string(),
        ));
    }
    Ok(())
}

/// Arguments of `set_milestone_state`: a valid key and `done` or `snoozed`.
pub fn validate_milestone_state(key: &str, state: &str) -> Result<()> {
    validate_milestone_key(key)?;
    if state != MILESTONE_STATE_DONE && state != MILESTONE_STATE_SNOOZED {
        return Err(AppError::Validation(
            "validation.milestoneStateInvalid".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_and_states_are_validated() {
        assert!(validate_milestone_state("backup_stale", "done").is_ok());
        assert!(validate_milestone_state("backup_stale", "snoozed").is_ok());
        assert!(validate_milestone_state("backup_stale", "gone").is_err());
        assert!(validate_milestone_state("  ", "done").is_err());
        assert!(validate_milestone_key(&"k".repeat(257)).is_err());
    }

    #[test]
    fn muted_kinds_must_be_known() {
        assert!(validate_milestone_kinds(&[
            "watch_target".to_string(),
            "backup_stale".to_string()
        ])
        .is_ok());
        assert!(validate_milestone_kinds(&[]).is_ok());
        assert!(validate_milestone_kinds(&["nope".to_string()]).is_err());
    }
}
