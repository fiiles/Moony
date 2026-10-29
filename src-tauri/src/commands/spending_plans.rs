//! Tauri commands for spending plans
//!
//! Thin command handlers that delegate to the spending plans service

use crate::db::Database;
use crate::error::Result;
use crate::models::{
    BankTransaction, InsertSpendingPlan, PlanCandidateFilters, PlanCandidatesResult, SpendingPlan,
    SpendingPlanAssignment, SpendingPlanDetail, SpendingPlanSummary, SpendingPlansOverview,
};
use crate::services::{self, fx};
use tauri::State;

/// Every plan of every year with what has been spent on it
#[tauri::command]
pub async fn get_spending_plan_index(db: State<'_, Database>) -> Result<Vec<SpendingPlanSummary>> {
    db.with_conn(services::spending_plans::plan_index)
}

/// The plans of one year with totals in the report currency
#[tauri::command]
pub async fn get_spending_plans_overview(
    db: State<'_, Database>,
    year: i32,
    report_currency: String,
) -> Result<SpendingPlansOverview> {
    db.with_conn(|conn| {
        services::spending_plans::overview(conn, year, &report_currency, &fx::current())
    })
}

#[tauri::command]
pub async fn get_spending_plan_detail(
    db: State<'_, Database>,
    id: String,
) -> Result<SpendingPlanDetail> {
    db.with_conn(|conn| services::spending_plans::detail(conn, &id))
}

#[tauri::command]
pub async fn get_spending_plan_transactions(
    db: State<'_, Database>,
    id: String,
) -> Result<Vec<BankTransaction>> {
    db.with_conn(|conn| services::spending_plans::plan_transactions(conn, &id))
}

/// Payments (any account) that could be tagged to a plan
#[tauri::command]
pub async fn search_plan_candidate_transactions(
    db: State<'_, Database>,
    filters: PlanCandidateFilters,
) -> Result<PlanCandidatesResult> {
    db.with_conn(|conn| services::spending_plans::candidate_transactions(conn, &filters))
}

#[tauri::command]
pub async fn create_spending_plan(
    db: State<'_, Database>,
    data: InsertSpendingPlan,
) -> Result<SpendingPlan> {
    data.validate()?;
    db.with_conn(|conn| services::spending_plans::create_plan(conn, &data))
}

/// Create several plans in one transaction
#[tauri::command]
pub async fn create_spending_plans(
    db: State<'_, Database>,
    items: Vec<InsertSpendingPlan>,
) -> Result<Vec<SpendingPlan>> {
    for item in &items {
        item.validate()?;
    }
    db.with_conn(|conn| services::spending_plans::create_plans(conn, &items))
}

#[tauri::command]
pub async fn update_spending_plan(
    db: State<'_, Database>,
    id: String,
    data: InsertSpendingPlan,
) -> Result<SpendingPlan> {
    data.validate()?;
    db.with_conn(|conn| services::spending_plans::update_plan(conn, &id, &data))
}

#[tauri::command]
pub async fn set_spending_plan_closed(
    db: State<'_, Database>,
    id: String,
    closed: bool,
) -> Result<SpendingPlan> {
    db.with_conn(|conn| services::spending_plans::set_plan_closed(conn, &id, closed))
}

/// Delete a plan; its payments stay, untagged
#[tauri::command]
pub async fn delete_spending_plan(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| services::spending_plans::delete_plan(conn, &id))
}

/// Tag payments with a plan or clear their tag; `restore` allows tagging a closed plan (undo)
#[tauri::command]
pub async fn set_transaction_spending_plans(
    db: State<'_, Database>,
    assignments: Vec<SpendingPlanAssignment>,
    restore: bool,
) -> Result<()> {
    db.with_conn(|conn| {
        services::spending_plans::set_transaction_plans(conn, &assignments, restore)
    })
}
