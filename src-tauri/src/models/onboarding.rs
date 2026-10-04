//! First-run onboarding progress

use serde::{Deserialize, Serialize};
use specta::Type;

/// Done-state of the dashboard "Getting started" checklist plus the
/// onboarding flags stored in `app_config`.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct OnboardingProgress {
    #[serde(rename = "hasAccount")]
    pub has_account: bool,
    #[serde(rename = "hasTransactions")]
    pub has_transactions: bool,
    #[serde(rename = "hasInvestment")]
    pub has_investment: bool,
    #[serde(rename = "hasBudget")]
    pub has_budget: bool,
    #[serde(rename = "lastBackupAt")]
    pub last_backup_at: Option<i64>,
    #[serde(rename = "checklistDismissed")]
    pub checklist_dismissed: bool,
    #[serde(rename = "completedAt")]
    pub completed_at: Option<i64>,
}
