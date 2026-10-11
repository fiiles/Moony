//! Stock tags commands for investment categorization and analysis

use crate::db::Database;
use crate::error::Result;
use crate::models::{
    InsertStockTag, InsertStockTagGroup, StockTag, StockTagGroup, StocksAnalysis, TagMetricsReport,
};
use crate::services::{position_reports, stock_tags};
use tauri::State;

// ============================================================================
// Tag Group Commands
// ============================================================================

/// Get all stock tag groups
#[tauri::command]
pub async fn get_all_stock_tag_groups(db: State<'_, Database>) -> Result<Vec<StockTagGroup>> {
    db.with_conn(stock_tags::list_groups)
}

/// Create a new stock tag group
#[tauri::command]
pub async fn create_stock_tag_group(
    db: State<'_, Database>,
    data: InsertStockTagGroup,
) -> Result<StockTagGroup> {
    data.validate()?;
    db.with_conn(|conn| stock_tags::create_group(conn, data))
}

/// Update an existing stock tag group
#[tauri::command]
pub async fn update_stock_tag_group(
    db: State<'_, Database>,
    id: String,
    data: InsertStockTagGroup,
) -> Result<StockTagGroup> {
    data.validate()?;
    db.with_conn(|conn| stock_tags::update_group(conn, &id, data))
}

/// Delete a stock tag group
#[tauri::command]
pub async fn delete_stock_tag_group(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| stock_tags::delete_group(conn, &id))
}

// ============================================================================
// Tag Commands
// ============================================================================

/// Get all stock tags (includes group_id)
#[tauri::command]
pub async fn get_all_stock_tags(db: State<'_, Database>) -> Result<Vec<StockTag>> {
    db.with_conn(stock_tags::list_tags)
}

/// Create a new stock tag
#[tauri::command]
pub async fn create_stock_tag(db: State<'_, Database>, data: InsertStockTag) -> Result<StockTag> {
    data.validate()?;
    db.with_conn(|conn| stock_tags::create_tag(conn, data))
}

/// Update an existing stock tag
#[tauri::command]
pub async fn update_stock_tag(
    db: State<'_, Database>,
    id: String,
    data: InsertStockTag,
) -> Result<StockTag> {
    data.validate()?;
    db.with_conn(|conn| stock_tags::update_tag(conn, &id, data))
}

/// Delete a stock tag
#[tauri::command]
pub async fn delete_stock_tag(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| stock_tags::delete_tag(conn, &id))
}

/// Get tags for a specific investment
#[tauri::command]
pub async fn get_investment_tags(
    db: State<'_, Database>,
    investment_id: String,
) -> Result<Vec<StockTag>> {
    db.with_conn(|conn| stock_tags::investment_tags(conn, &investment_id))
}

/// Set tags for an investment (replaces its tags; at most one tag per group)
#[tauri::command]
pub async fn set_investment_tags(
    db: State<'_, Database>,
    investment_id: String,
    tag_ids: Vec<String>,
) -> Result<()> {
    db.with_conn(|conn| stock_tags::set_investment_tags(conn, &investment_id, &tag_ids))
}

/// Every stock position with its tags in `report_currency` (positions service).
#[tauri::command]
pub async fn get_stocks_analysis(
    db: State<'_, Database>,
    report_currency: String,
) -> Result<StocksAnalysis> {
    db.with_conn(|conn| position_reports::stocks_analysis(conn, &report_currency))
}

/// Metrics per tag in `report_currency`; every tag when `tag_ids` is empty.
#[tauri::command]
pub async fn get_tag_metrics(
    db: State<'_, Database>,
    tag_ids: Vec<String>,
    report_currency: String,
) -> Result<TagMetricsReport> {
    db.with_conn(|conn| position_reports::tag_metrics(conn, &report_currency, &tag_ids))
}
