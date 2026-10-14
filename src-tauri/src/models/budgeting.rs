//! Budgeting models for visualizing income and spending by category

use serde::{Deserialize, Serialize};
use specta::Type;

/// Budget goal entity - defines spending limits per category per timeframe
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BudgetGoal {
    pub id: String,
    #[serde(rename = "categoryId")]
    pub category_id: String,
    pub timeframe: String, // "monthly", "quarterly", "yearly"
    pub amount: String,
    pub currency: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    #[serde(rename = "updatedAt")]
    pub updated_at: i64,
    /// First month of the version whose `amount` is shown: the version in
    /// effect now, or the first scheduled one for a goal that starts later.
    /// `None` only for a legacy goal without versions.
    #[serde(rename = "effectiveMonth")]
    pub effective_month: Option<i64>,
    /// Every limit version, oldest first (`amount: null` = no limit from
    /// that month). Lets the editor prefill the version in effect for the
    /// month being edited and show scheduled changes.
    pub versions: Vec<BudgetGoalVersion>,
}

/// One limit version of a budget goal, effective from a calendar month.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
pub struct BudgetGoalVersion {
    #[serde(rename = "effectiveMonth")]
    pub effective_month: i64,
    pub amount: Option<String>,
    pub currency: String,
}

/// Exact stored interval selected for cancellation (across any number of goals).
#[derive(Debug, Clone, Deserialize, Type)]
pub struct BudgetGoalInterval {
    #[serde(rename = "categoryId")]
    pub category_id: String,
    pub timeframe: String,
    #[serde(rename = "effectiveMonth")]
    pub effective_month: i64,
}

impl BudgetGoalInterval {
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::AppError;
        if self.category_id.trim().is_empty() {
            return Err(AppError::Validation("validation.categoryIdRequired".into()));
        }
        if !["monthly", "quarterly", "yearly"].contains(&self.timeframe.as_str()) {
            return Err(AppError::Validation("validation.timeframeInvalid".into()));
        }
        if crate::services::reporting::month_start(self.effective_month)
            != Some(self.effective_month)
        {
            return Err(AppError::Validation(
                "validation.effectiveMonthInvalid".into(),
            ));
        }
        Ok(())
    }
}

/// Data for creating/updating a budget goal
#[derive(Debug, Clone, Deserialize, Type)]
pub struct InsertBudgetGoal {
    #[serde(rename = "categoryId")]
    pub category_id: String,
    pub timeframe: String,
    pub amount: String,
    pub currency: Option<String>,
    #[serde(rename = "effectiveMonth")]
    pub effective_month: Option<i64>,
    /// Last month of the limit (inclusive); afterwards the limit that applied
    /// before the save returns. None keeps the limit open-ended.
    #[serde(rename = "effectiveUntil", default)]
    pub effective_until: Option<i64>,
}

impl InsertBudgetGoal {
    pub fn validate(&self) -> crate::error::Result<()> {
        use crate::error::AppError;

        if let Some(month) = self.effective_month {
            if crate::services::reporting::month_start(month) != Some(month) {
                return Err(AppError::Validation(
                    "validation.effectiveMonthInvalid".into(),
                ));
            }
        }
        if let Some(until) = self.effective_until {
            if crate::services::reporting::month_start(until) != Some(until) {
                return Err(AppError::Validation(
                    "validation.effectiveMonthInvalid".into(),
                ));
            }
            if self.effective_month.is_some_and(|month| until < month) {
                return Err(AppError::Validation(
                    "validation.effectiveUntilBeforeStart".into(),
                ));
            }
        }
        // Validate timeframe
        let valid_timeframes = ["monthly", "quarterly", "yearly"];
        if !valid_timeframes.contains(&self.timeframe.as_str()) {
            return Err(AppError::Validation("validation.timeframeInvalid".into()));
        }

        // Validate amount is a positive number
        let amount: f64 = self
            .amount
            .parse()
            .map_err(|_| AppError::Validation("validation.invalidAmount".into()))?;
        if !amount.is_finite() {
            return Err(AppError::Validation("validation.invalidAmount".into()));
        }
        if amount <= 0.0 {
            return Err(AppError::Validation("validation.amountPositive".into()));
        }

        // Validate category_id is not empty
        if self.category_id.is_empty() {
            return Err(AppError::Validation("validation.categoryIdRequired".into()));
        }

        Ok(())
    }
}

/// Category spending summary for a period
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CategorySpendingSummary {
    #[serde(rename = "budgetCurrency")]
    pub budget_currency: Option<String>,
    #[serde(rename = "budgetSpent")]
    pub budget_spent: Option<String>,
    #[serde(rename = "budgetLimitNative")]
    pub budget_limit_native: Option<String>,
    #[serde(rename = "budgetLimitFxQuality")]
    pub budget_limit_fx_quality: Option<crate::models::FxQuality>,
    #[serde(rename = "budgetLimit")]
    pub budget_limit: Option<String>,
    #[serde(rename = "categoryId")]
    pub category_id: String,
    #[serde(rename = "categoryName")]
    pub category_name: String,
    #[serde(rename = "categoryIcon")]
    pub category_icon: Option<String>,
    #[serde(rename = "categoryColor")]
    pub category_color: Option<String>,
    #[serde(rename = "totalAmount")]
    pub total_amount: String,
    #[serde(rename = "transactionCount")]
    pub transaction_count: i32,
    #[serde(rename = "budgetGoal")]
    pub budget_goal: Option<BudgetGoal>,
    #[serde(rename = "budgetPercentage")]
    pub budget_percentage: Option<f64>, // How much of budget used (0-100+)
}

pub type BudgetingTransaction = crate::models::ReportTransaction;

/// Full budgeting report for a period
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BudgetingReport {
    #[serde(rename = "bankCashMovement")]
    pub bank_cash_movement: String,
    pub quality: crate::models::ReportQuality,
    #[serde(rename = "nonconsumptionFlows")]
    pub nonconsumption_flows: Vec<crate::models::CashflowGroup>,
    #[serde(rename = "periodStart")]
    pub period_start: i64,
    #[serde(rename = "periodEnd")]
    pub period_end: i64,
    pub timeframe: String,
    #[serde(rename = "totalIncome")]
    pub total_income: String,
    #[serde(rename = "totalExpenses")]
    pub total_expenses: String,
    #[serde(rename = "netBalance")]
    pub net_balance: String,
    #[serde(rename = "incomeCategories")]
    pub income_categories: Vec<CategorySpendingSummary>,
    #[serde(rename = "expenseCategories")]
    pub expense_categories: Vec<CategorySpendingSummary>,
    #[serde(rename = "uncategorizedIncome")]
    pub uncategorized_income: String,
    #[serde(rename = "uncategorizedExpenses")]
    pub uncategorized_expenses: String,
    #[serde(rename = "uncategorizedTransactionCount")]
    pub uncategorized_transaction_count: i32,
    /// Included booked expense rows without a category, including unavailable FX.
    #[serde(rename = "uncategorizedExpenseTransactionCount")]
    pub uncategorized_expense_transaction_count: i32,
    #[serde(rename = "reportCurrency")]
    pub report_currency: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validate_rejects_nonfinite_goal_amounts() {
        for amount in ["NaN", "inf", "-inf", "1e999"] {
            let goal = InsertBudgetGoal {
                category_id: "cat_other".into(),
                timeframe: "monthly".into(),
                amount: amount.into(),
                currency: None,
                effective_month: None,
                effective_until: None,
            };
            assert!(goal.validate().is_err(), "accepted {amount}");
        }
    }

    #[test]
    fn goal_validation_reports_keys_not_sentences() {
        use crate::error::AppError;
        let goal = |category: &str, timeframe: &str, amount: &str| InsertBudgetGoal {
            category_id: category.into(),
            timeframe: timeframe.into(),
            amount: amount.into(),
            currency: None,
            effective_month: None,
            effective_until: None,
        };
        let key = |goal: InsertBudgetGoal| match goal.validate() {
            Err(AppError::Validation(key)) => key,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            key(goal("cat_other", "weekly", "10")),
            "validation.timeframeInvalid"
        );
        assert_eq!(
            key(goal("cat_other", "monthly", "ten")),
            "validation.invalidAmount"
        );
        assert_eq!(
            key(goal("cat_other", "monthly", "0")),
            "validation.amountPositive"
        );
        assert_eq!(
            key(goal("", "monthly", "10")),
            "validation.categoryIdRequired"
        );
    }

    #[test]
    fn goal_validation_checks_the_range_end() {
        use crate::error::AppError;
        const JUN: i64 = 1780272000;
        const MAR: i64 = 1772323200;
        let goal = |month: Option<i64>, until: i64| InsertBudgetGoal {
            category_id: "cat_other".into(),
            timeframe: "monthly".into(),
            amount: "10".into(),
            currency: None,
            effective_month: month,
            effective_until: Some(until),
        };
        let key = |goal: InsertBudgetGoal| match goal.validate() {
            Err(AppError::Validation(key)) => key,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            key(goal(Some(JUN), JUN + 86_400)),
            "validation.effectiveMonthInvalid"
        );
        assert_eq!(
            key(goal(Some(JUN), MAR)),
            "validation.effectiveUntilBeforeStart"
        );
        assert!(goal(Some(JUN), JUN).validate().is_ok());
        assert!(goal(None, MAR).validate().is_ok());
    }
}
