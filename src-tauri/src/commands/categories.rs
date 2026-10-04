//! Tauri commands for managing transaction categories
//!
//! Thin handlers: validate, delegate to `services::categories`, map. Not to be
//! confused with `update_transaction_category` (commands/bank_accounts.rs),
//! which sets the category *of a transaction*; the edit command here is
//! `update_transaction_category_definition`.

use crate::commands::categorization::CategorizationState;
use crate::db::Database;
use crate::error::Result;
use crate::models::{
    CategoryUsage, InsertTransactionCategory, TransactionCategory, UpdateTransactionCategory,
};
use crate::services::categories;
use tauri::State;

/// Get all transaction categories
#[tauri::command]
pub async fn get_transaction_categories(
    db: State<'_, Database>,
) -> Result<Vec<TransactionCategory>> {
    db.with_conn(categories::list_categories)
}

/// Create a custom category
#[tauri::command]
pub async fn create_transaction_category(
    db: State<'_, Database>,
    data: InsertTransactionCategory,
) -> Result<TransactionCategory> {
    data.validate()?;
    db.with_conn(|conn| categories::create_category(conn, &data))
}

/// Rename a category or change its icon or color (system categories included)
#[tauri::command]
pub async fn update_transaction_category_definition(
    db: State<'_, Database>,
    id: String,
    data: UpdateTransactionCategory,
) -> Result<TransactionCategory> {
    data.validate()?;
    db.with_conn(|conn| categories::update_category(conn, &id, &data))
}

/// Delete a custom category. Refused while anything still references it unless
/// `reassign_to` names the category that takes everything over.
#[tauri::command]
pub async fn delete_transaction_category(
    db: State<'_, Database>,
    engine: State<'_, CategorizationState>,
    id: String,
    reassign_to: Option<String>,
) -> Result<()> {
    db.with_conn(|conn| categories::delete_category(conn, &id, reassign_to.as_deref()))?;
    if let Some(target) = reassign_to {
        // The in-memory rule and learned-payee layers must not keep answering
        // with the deleted id.
        engine.0.reassign_category(&id, &target);
    }
    Ok(())
}

/// How many transactions, rules, learned payees and budget goals reference
/// each category (what a delete would have to move)
#[tauri::command]
pub async fn get_category_usage(db: State<'_, Database>) -> Result<Vec<CategoryUsage>> {
    db.with_conn(categories::category_usage)
}
