//! Spending plans: a yearly pot of money for one purpose (a holiday, a renovation) that payments
//! are tagged to (spec 2026-10-29-spending-plans-design).

use crate::error::{AppError, Result};
use crate::models::validation::finite_number;
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
