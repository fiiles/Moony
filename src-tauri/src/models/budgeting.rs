//! Budgeting models for visualizing income and spending by category

use serde::{Deserialize, Serialize};
use specta::Type;

/// Budget goal entity - defines spending limits per category per timeframe
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BudgetGoal {
    pub id: String,
    pub category_id: String,
    pub timeframe: String, // "monthly", "quarterly", "yearly"
    pub amount: String,
    pub currency: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Data for creating/updating a budget goal
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct InsertBudgetGoal {
    pub category_id: String,
    pub timeframe: String,
    pub amount: String,
    pub currency: Option<String>,
}

impl InsertBudgetGoal {
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::AppError;

        // Validate timeframe
        let valid_timeframes = ["monthly", "quarterly", "yearly"];
        if !valid_timeframes.contains(&self.timeframe.as_str()) {
            return Err(AppError::Validation(format!(
                "Invalid timeframe '{}'. Must be one of: monthly, quarterly, yearly",
                self.timeframe
            )));
        }

        // Validate amount is a positive number
        let amount: f64 = self
            .amount
            .parse()
            .map_err(|_| AppError::Validation("Invalid amount format".into()))?;
        if amount <= 0.0 {
            return Err(AppError::Validation(
                "Budget amount must be positive".into(),
            ));
        }

        // Validate category_id is not empty
        if self.category_id.is_empty() {
            return Err(AppError::Validation("Category ID is required".into()));
        }

        Ok(())
    }
}

/// Category spending summary for a period
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CategorySpendingSummary {
    pub category_id: String,
    pub category_name: String,
    pub category_icon: Option<String>,
    pub category_color: Option<String>,
    pub total_amount: String,
    pub transaction_count: i32,
    pub budget_goal: Option<BudgetGoal>,
    pub budget_percentage: Option<f64>, // How much of budget used (0-100+)
}

/// Transaction for display in category detail within budgeting view
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BudgetingTransaction {
    pub id: String,
    pub booking_date: i64,
    pub amount: String,
    pub currency: String,
    pub description: Option<String>,
    pub counterparty_name: Option<String>,
    pub counterparty_iban: Option<String>,
    pub category_id: Option<String>,
    pub bank_account_id: String,
    pub bank_account_name: String,
    pub tx_type: String,
}

/// Full budgeting report for a period
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct BudgetingReport {
    pub period_start: i64,
    pub period_end: i64,
    pub timeframe: String,
    pub total_income: String,
    pub total_expenses: String,
    pub net_balance: String,
    pub income_categories: Vec<CategorySpendingSummary>,
    pub expense_categories: Vec<CategorySpendingSummary>,
    pub uncategorized_income: String,
    pub uncategorized_expenses: String,
    pub uncategorized_transaction_count: i32,
}
