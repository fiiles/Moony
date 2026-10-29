//! Spending plans: a yearly pot of money for one purpose (a holiday, a renovation) that payments
//! are tagged to (spec 2026-10-29-spending-plans-design).

use crate::error::{AppError, Result};
use crate::models::fx::ConversionQuality;
use crate::models::validation::finite_number;
use crate::models::BankTransaction;
use crate::services::fx;
use serde::{Deserialize, Serialize};
use specta::Type;

const MAX_NAME_CHARS: usize = 100;
const MAX_NOTE_CHARS: usize = 500;
const MAX_ICON_CHARS: usize = 40;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlan {
    pub id: String,
    pub name: String,
    pub year: i32,
    /// Planned amount in `currency` (TEXT money).
    pub amount: String,
    pub currency: String,
    pub icon: String,
    pub color: String,
    pub note: Option<String>,
    /// Unix epoch the plan was closed; `None` while open.
    #[serde(rename = "closedAt")]
    pub closed_at: Option<i64>,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct InsertSpendingPlan {
    pub name: String,
    pub year: i32,
    pub amount: String,
    pub currency: String,
    pub icon: String,
    pub color: String,
    pub note: Option<String>,
}

fn invalid(key: &str) -> AppError {
    AppError::Validation(format!("validation.{key}"))
}

impl InsertSpendingPlan {
    /// Check every field and return the normalized copy (trimmed name, uppercase currency).
    pub fn validate(&self) -> Result<InsertSpendingPlan> {
        let name = self.name.trim();
        if name.is_empty() {
            return Err(invalid("spendingPlanNameRequired"));
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Err(invalid("spendingPlanNameTooLong"));
        }
        if !(2000..=2100).contains(&self.year) {
            return Err(invalid("spendingPlanYearInvalid"));
        }
        let amount = finite_number(&self.amount, "validation.spendingPlanAmountInvalid")?;
        if amount <= 0.0 {
            return Err(invalid("spendingPlanAmountInvalid"));
        }
        let currency = fx::normalize(&self.currency).ok_or_else(|| invalid("currencyInvalid"))?;
        let icon_ok = (1..=MAX_ICON_CHARS).contains(&self.icon.len())
            && self
                .icon
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if !icon_ok {
            return Err(invalid("spendingPlanIconInvalid"));
        }
        let color_ok = self.color.len() == 7
            && self.color.starts_with('#')
            && self.color[1..].bytes().all(|b| b.is_ascii_hexdigit());
        if !color_ok {
            return Err(invalid("spendingPlanColorInvalid"));
        }
        if let Some(note) = &self.note {
            if note.chars().count() > MAX_NOTE_CHARS {
                return Err(invalid("spendingPlanNoteTooLong"));
            }
        }
        Ok(InsertSpendingPlan {
            name: name.to_string(),
            currency,
            ..self.clone()
        })
    }
}

/// Tag a payment with a plan, or clear its tag (`plan_id: None`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlanAssignment {
    #[serde(rename = "transactionId")]
    pub transaction_id: String,
    #[serde(rename = "planId")]
    pub plan_id: Option<String>,
}

/// A plan with what has been spent on it, in the plan's currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlanSummary {
    pub plan: SpendingPlan,
    /// Booked debits minus credits in the plan currency; `None` when there are booked payments
    /// and none could be converted, `"0.00"`-style when there are none.
    pub spent: Option<String>,
    /// Tagged payments, pending ones included.
    #[serde(rename = "paymentCount")]
    pub payment_count: i64,
    #[serde(rename = "lastPaymentDate")]
    pub last_payment_date: Option<i64>,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
}

/// Sums over the plans of one year, in the report currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlanTotals {
    pub planned: Option<String>,
    pub spent: Option<String>,
    #[serde(rename = "remainingActive")]
    pub remaining_active: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlansOverview {
    pub year: i32,
    /// Currency of `totals`.
    pub currency: String,
    #[serde(rename = "fxQuality")]
    pub fx_quality: ConversionQuality,
    pub plans: Vec<SpendingPlanSummary>,
    pub totals: SpendingPlanTotals,
    /// Each plan's spent in `currency`, largest first (`None` last). A plan that cannot be
    /// converted has `None`; the miss is counted in `fx_quality`.
    #[serde(rename = "planSpent")]
    pub plan_spent: Vec<SpendingPlanSpent>,
    /// Years that have at least one plan, ascending.
    #[serde(rename = "yearsWithPlans")]
    pub years_with_plans: Vec<i32>,
}

/// One plan's spent converted into the overview's report currency.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlanSpent {
    #[serde(rename = "planId")]
    pub plan_id: String,
    /// Booked debits minus credits in the report currency; `None` when it cannot be converted.
    pub spent: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlanMonth {
    /// `YYYY-MM` of the booking day (UTC).
    pub month: String,
    pub spent: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlanCategory {
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
    pub spent: String,
    pub count: i64,
}

/// One month x category bucket; the frontend sums cells for filtered views.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlanCell {
    pub month: String,
    #[serde(rename = "categoryId")]
    pub category_id: Option<String>,
    pub spent: String,
    pub count: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct SpendingPlanDetail {
    pub summary: SpendingPlanSummary,
    pub months: Vec<SpendingPlanMonth>,
    pub categories: Vec<SpendingPlanCategory>,
    pub cells: Vec<SpendingPlanCell>,
    #[serde(rename = "accountCount")]
    pub account_count: i64,
}

/// Filters for payments that could be tagged to a plan (any account).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct PlanCandidateFilters {
    #[serde(rename = "planId")]
    pub plan_id: String,
    #[serde(rename = "dateFrom")]
    pub date_from: Option<i64>,
    #[serde(rename = "dateTo")]
    pub date_to: Option<i64>,
    #[serde(rename = "accountId")]
    pub account_id: Option<String>,
    #[serde(rename = "onlyUnassigned")]
    pub only_unassigned: bool,
    /// Matched against counterparty, description and category name, ignoring case and diacritics.
    pub query: Option<String>,
    /// Categories whose shown (translated) name matches `query`: their payments match it too.
    #[serde(rename = "categoryIds")]
    pub category_ids: Option<Vec<String>>,
    pub limit: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct PlanCandidatesResult {
    pub transactions: Vec<BankTransaction>,
    pub truncated: bool,
}
