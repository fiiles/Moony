//! Actual cashflow from bank transactions (design system §11): what the
//! "Cashflow" report page shows, as opposed to the planned items of
//! "Plánování cashflow". Amounts are TEXT in the declared report currency, at its minor-unit precision.

use crate::models::FlowKind;
use serde::{Deserialize, Serialize};
use specta::Type;

/// Income and expenses of one calendar month (UTC).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CashflowMonth {
    // Unix seconds of the first day of the month, UTC midnight.
    pub month: i64,
    pub income: String,
    pub expenses: String,
}

/// One row of a breakdown: the period's rows of one flow role in one category.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CashflowGroup {
    // Category id, or `uncategorized` for rows without one.
    pub key: String,
    // Category name as stored; empty for uncategorized rows.
    pub name: String,
    #[serde(rename = "flowKind")]
    pub flow_kind: FlowKind,
    // Expense rows debit-positive (refunds lower them), every other role credit-positive.
    pub amount: String,
    // Booked rows behind the group, including rows without a report conversion.
    pub count: i32,
}

/// Actual cashflow for one calendar period, with a twelve-month chart window.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CashflowActuals {
    #[serde(rename = "reportCurrency")]
    pub report_currency: String,
    #[serde(rename = "periodStart")]
    pub period_start: i64,
    #[serde(rename = "periodEnd")]
    pub period_end: i64,
    pub quality: crate::models::ReportQuality,
    // Chart window, oldest first, zero-filled: twelve months ending with the period's last month.
    pub months: Vec<CashflowMonth>,
    #[serde(rename = "totalIncome")]
    pub total_income: String,
    #[serde(rename = "totalExpenses")]
    pub total_expenses: String,
    // Income less expenses.
    #[serde(rename = "netBalance")]
    pub net_balance: String,
    // Income less expenses plus the signed money transfers.
    #[serde(rename = "bankCashMovement")]
    pub bank_cash_movement: String,
    #[serde(rename = "averageIncome")]
    pub average_income: Option<String>,
    #[serde(rename = "averageExpenses")]
    pub average_expenses: Option<String>,
    #[serde(rename = "averageCompletedMonths")]
    pub average_completed_months: Vec<i64>,
    // Net expenses by category, largest first.
    #[serde(rename = "expenseCategories")]
    pub expense_categories: Vec<CashflowGroup>,
    // Income by category, largest first; uncategorized credits form their own group.
    #[serde(rename = "incomeCategories")]
    pub income_categories: Vec<CashflowGroup>,
    // Money transfers (investing, saving, between accounts) by category, largest movement first.
    pub transfers: Vec<CashflowGroup>,
    // Bank accounts that had a transaction in the period.
    #[serde(rename = "accountCount")]
    pub account_count: i32,
    // Booked rows of the period in scope, including transfers and explicit FX gaps.
    #[serde(rename = "transactionCount")]
    pub transaction_count: i32,
}
