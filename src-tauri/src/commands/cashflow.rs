//! Thin cashflow report and planning commands.
use crate::db::Database;
use crate::error::Result;
use crate::models::{CashflowItem, CashflowReport, InsertCashflowItem};
use crate::services::{cashflow_actuals, cashflow_planning};
use tauri::State;
#[tauri::command]
pub async fn get_cashflow_actuals(
    db: State<'_, Database>,
    start_date: i64,
    end_date: i64,
    account_ids: Option<Vec<String>>,
    report_currency: String,
) -> Result<crate::models::CashflowActuals> {
    db.with_conn(|conn| {
        cashflow_actuals::get_actuals_in_currency(
            conn,
            start_date,
            end_date,
            chrono::Utc::now().timestamp(),
            account_ids.as_deref(),
            &report_currency,
        )
    })
}
#[tauri::command]
pub async fn get_cashflow_report(
    db: State<'_, Database>,
    view_type: String,
    report_currency: String,
) -> Result<CashflowReport> {
    db.with_conn(|conn| {
        cashflow_planning::get_report_in_currency(conn, &view_type, &report_currency)
    })
}
#[tauri::command]
pub async fn get_all_cashflow_items(db: State<'_, Database>) -> Result<Vec<CashflowItem>> {
    db.with_conn(cashflow_planning::get_all_cashflow_items)
}
#[tauri::command]
pub async fn create_cashflow_item(
    db: State<'_, Database>,
    data: InsertCashflowItem,
) -> Result<CashflowItem> {
    data.validate()?;
    db.with_conn(|conn| cashflow_planning::create_cashflow_item(conn, data))
}
#[tauri::command]
pub async fn update_cashflow_item(
    db: State<'_, Database>,
    id: String,
    data: InsertCashflowItem,
) -> Result<CashflowItem> {
    data.validate()?;
    db.with_conn(|conn| cashflow_planning::update_cashflow_item(conn, id, data))
}
#[tauri::command]
pub async fn delete_cashflow_item(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| cashflow_planning::delete_cashflow_item(conn, id))
}
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn get_report_transactions(
    db: State<'_, Database>,
    start_date: i64,
    end_date: i64,
    account_ids: Option<Vec<String>>,
    report_currency: String,
    flow_kind: Option<crate::models::FlowKind>,
    category_id: Option<String>,
) -> Result<Vec<crate::models::ReportTransaction>> {
    db.with_conn(|conn| {
        crate::services::reporting::filtered_transactions_in_currency(
            conn,
            start_date,
            end_date,
            account_ids.as_deref(),
            flow_kind,
            category_id.as_deref(),
            &report_currency,
        )
    })
}
#[tauri::command]
pub async fn get_effective_transaction_flows(
    db: State<'_, Database>,
    transaction_ids: Vec<String>,
) -> Result<Vec<crate::models::EffectiveTransactionFlow>> {
    db.with_conn(|conn| crate::services::reporting::effective_flows(conn, &transaction_ids))
}
