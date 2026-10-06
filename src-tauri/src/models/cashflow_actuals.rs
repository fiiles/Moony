//! Actual cashflow from bank transactions (design system §11): what the
//! "Cashflow" report page shows, as opposed to the planned items of
//! "Plánování cashflow". All amounts are CZK TEXT with two decimals (ADR 0001).

use serde::{Deserialize, Serialize};
use specta::Type;

/// Income and expenses of one calendar month (UTC).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CashflowMonth {
    #[serde(rename = "nonconsumptionFlows")]
    pub nonconsumption_flows: Vec<CashflowGroup>,
    #[serde(rename = "bankCashMovement")]
    pub bank_cash_movement: String,
    // Unix seconds of the first day of the month, UTC midnight.
    pub month: i64,
    pub income: String,
    pub expenses: String,
}

/// One row of "expenses by category" or "income by source", with the same sum
/// for the preceding range so the UI can show "vs. minulé".
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CashflowGroup {
    // Category id (`uncategorized` for rows without one) or a normalized source key.
    pub key: String,
    // Category name as stored, or the counterparty as first seen; empty for an unknown source.
    pub name: String,
    pub amount: String,
    #[serde(rename = "previousAmount")]
    pub previous_amount: String,
    // Transactions behind `amount` in the current range.
    pub count: i32,
}

/// Actual monthly cashflow for a range plus the preceding range of equal length.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct CashflowActuals {
    #[serde(rename = "netBalance")]
    pub net_balance: String,
    #[serde(rename = "bankCashMovement")]
    pub bank_cash_movement: String,
    #[serde(rename = "averageCompletedMonths")]
    pub average_completed_months: Vec<i64>,
    pub quality: crate::models::ReportQuality,
    #[serde(rename = "previousQuality")]
    pub previous_quality: crate::models::ReportQuality,
    #[serde(rename = "nonconsumptionFlows")]
    pub nonconsumption_flows: Vec<CashflowGroup>,
    #[serde(rename = "averageIncome")]
    pub average_income: Option<String>,
    #[serde(rename = "averageExpenses")]
    pub average_expenses: Option<String>,
    // `6m`, `12m` or `ytd`.
    pub range: String,
    #[serde(rename = "rangeStart")]
    pub range_start: i64,
    #[serde(rename = "rangeEnd")]
    pub range_end: i64,
    #[serde(rename = "previousStart")]
    pub previous_start: i64,
    #[serde(rename = "previousEnd")]
    pub previous_end: i64,
    // Every month of the range, oldest first, zero-filled.
    pub months: Vec<CashflowMonth>,
    // Net expenses by category, largest first (refunds reduce a category).
    #[serde(rename = "expenseCategories")]
    pub expense_categories: Vec<CashflowGroup>,
    // Income by counterparty, largest first.
    #[serde(rename = "incomeSources")]
    pub income_sources: Vec<CashflowGroup>,
    #[serde(rename = "totalIncome")]
    pub total_income: String,
    #[serde(rename = "totalExpenses")]
    pub total_expenses: String,
    #[serde(rename = "previousIncome")]
    pub previous_income: String,
    #[serde(rename = "previousExpenses")]
    pub previous_expenses: String,
    // Bank accounts that had a transaction in the range.
    #[serde(rename = "accountCount")]
    pub account_count: i32,
    // Booked rows in scope, including separated flows and explicit FX gaps.
    #[serde(rename = "transactionCount")]
    pub transaction_count: i32,
}
