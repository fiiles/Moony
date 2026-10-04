//! Tauri commands for Budgeting feature
//!
//! Thin command handlers that delegate to the budgeting service

use crate::db::Database;
use crate::error::Result;
use crate::models::budgeting::{
    BudgetGoal, BudgetingReport, BudgetingTransaction, InsertBudgetGoal,
};
use crate::services;
use tauri::State;

/// Get the budgeting report for a time period
///
/// Aggregates all transactions across all bank accounts by category,
/// including budget goal progress calculations.
#[tauri::command]
pub async fn get_budgeting_report(
    db: State<'_, Database>,
    start_date: i64,
    end_date: i64,
    timeframe: String,
) -> Result<BudgetingReport> {
    services::budgeting::get_budgeting_report(&db, start_date, end_date, timeframe).await
}

/// Get transactions for a specific category in a time period
///
/// Used for drilling down into a category to see individual transactions.
/// Use "uncategorized" as category_id to get transactions without a category.
#[tauri::command]
pub async fn get_category_transactions(
    db: State<'_, Database>,
    category_id: String,
    start_date: i64,
    end_date: i64,
) -> Result<Vec<BudgetingTransaction>> {
    services::budgeting::get_transactions_for_category(&db, category_id, start_date, end_date).await
}

/// Get all budget goals
#[tauri::command]
pub async fn get_budget_goals(db: State<'_, Database>) -> Result<Vec<BudgetGoal>> {
    services::budgeting::get_budget_goals(&db).await
}

/// Create or update a budget goal
///
/// If a goal already exists for the same category + timeframe, it will be updated.
#[tauri::command]
pub async fn upsert_budget_goal(
    db: State<'_, Database>,
    data: InsertBudgetGoal,
) -> Result<BudgetGoal> {
    data.validate()?;
    services::budgeting::upsert_budget_goal(&db, data).await
}

/// Delete a budget goal
#[tauri::command]
pub async fn delete_budget_goal(db: State<'_, Database>, id: String) -> Result<()> {
    services::budgeting::delete_budget_goal(&db, id).await
}
