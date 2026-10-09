//! Stock tags commands for investment categorization and analysis

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    InsertStockTag, InsertStockTagGroup, StockTag, StockTagGroup, StocksAnalysis, TagMetricsReport,
};
use crate::services::position_reports;
use tauri::State;
use uuid::Uuid;

// ============================================================================
// Tag Group Commands
// ============================================================================

/// Get all stock tag groups
#[tauri::command]
pub async fn get_all_stock_tag_groups(db: State<'_, Database>) -> Result<Vec<StockTagGroup>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, description, created_at FROM stock_tag_groups ORDER BY name",
        )?;

        let groups = stmt
            .query_map([], |row| {
                Ok(StockTagGroup {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(groups)
    })
}

/// Create a new stock tag group
#[tauri::command]
pub async fn create_stock_tag_group(
    db: State<'_, Database>,
    data: InsertStockTagGroup,
) -> Result<StockTagGroup> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO stock_tag_groups (id, name, description, created_at) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, data.name, data.description, now],
        )?;

        Ok(StockTagGroup {
            id,
            name: data.name,
            description: data.description,
            created_at: now,
        })
    })
}

/// Update an existing stock tag group
#[tauri::command]
pub async fn update_stock_tag_group(
    db: State<'_, Database>,
    id: String,
    data: InsertStockTagGroup,
) -> Result<StockTagGroup> {
    db.with_conn(|conn| {
        let changes = conn.execute(
            "UPDATE stock_tag_groups SET name = ?1, description = ?2 WHERE id = ?3",
            rusqlite::params![data.name, data.description, id],
        )?;

        if changes == 0 {
            return Err(AppError::NotFound("Stock tag group not found".into()));
        }

        conn.query_row(
            "SELECT id, name, description, created_at FROM stock_tag_groups WHERE id = ?1",
            [&id],
            |row| {
                Ok(StockTagGroup {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    created_at: row.get(3)?,
                })
            },
        )
        .map_err(|_| AppError::NotFound("Stock tag group not found".into()))
    })
}

/// Delete a stock tag group
#[tauri::command]
pub async fn delete_stock_tag_group(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| {
        let changes = conn.execute("DELETE FROM stock_tag_groups WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Stock tag group not found".into()));
        }
        Ok(())
    })
}

// ============================================================================
// Tag Commands
// ============================================================================

/// Get all stock tags (includes group_id)
#[tauri::command]
pub async fn get_all_stock_tags(db: State<'_, Database>) -> Result<Vec<StockTag>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, color, group_id, created_at FROM stock_tags ORDER BY name",
        )?;

        let tags = stmt
            .query_map([], |row| {
                Ok(StockTag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    group_id: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tags)
    })
}

/// Create a new stock tag
#[tauri::command]
pub async fn create_stock_tag(db: State<'_, Database>, data: InsertStockTag) -> Result<StockTag> {
    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    db.with_conn(|conn| {
        conn.execute(
            "INSERT INTO stock_tags (id, name, color, group_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![id, data.name, data.color, data.group_id, now],
        )?;

        Ok(StockTag {
            id,
            name: data.name,
            color: data.color,
            group_id: data.group_id,
            created_at: now,
        })
    })
}

/// Update an existing stock tag
#[tauri::command]
pub async fn update_stock_tag(
    db: State<'_, Database>,
    id: String,
    data: InsertStockTag,
) -> Result<StockTag> {
    db.with_conn(|conn| {
        let changes = conn.execute(
            "UPDATE stock_tags SET name = ?1, color = ?2, group_id = ?3 WHERE id = ?4",
            rusqlite::params![data.name, data.color, data.group_id, id],
        )?;

        if changes == 0 {
            return Err(AppError::NotFound("Stock tag not found".into()));
        }

        conn.query_row(
            "SELECT id, name, color, group_id, created_at FROM stock_tags WHERE id = ?1",
            [&id],
            |row| {
                Ok(StockTag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    group_id: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
        .map_err(|_| AppError::NotFound("Stock tag not found".into()))
    })
}

/// Delete a stock tag
#[tauri::command]
pub async fn delete_stock_tag(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| {
        let changes = conn.execute("DELETE FROM stock_tags WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Stock tag not found".into()));
        }
        Ok(())
    })
}

/// Get tags for a specific investment
#[tauri::command]
pub async fn get_investment_tags(
    db: State<'_, Database>,
    investment_id: String,
) -> Result<Vec<StockTag>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT t.id, t.name, t.color, t.group_id, t.created_at
             FROM stock_tags t
             INNER JOIN stock_investment_tags sit ON t.id = sit.tag_id
             WHERE sit.investment_id = ?1
             ORDER BY t.name",
        )?;

        let tags = stmt
            .query_map([&investment_id], |row| {
                Ok(StockTag {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    color: row.get(2)?,
                    group_id: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(tags)
    })
}

/// Set tags for an investment (replaces all existing tags)
#[tauri::command]
pub async fn set_investment_tags(
    db: State<'_, Database>,
    investment_id: String,
    tag_ids: Vec<String>,
) -> Result<()> {
    let now = chrono::Utc::now().timestamp();

    db.with_conn(|conn| {
        // First, remove all existing tags for this investment
        conn.execute(
            "DELETE FROM stock_investment_tags WHERE investment_id = ?1",
            [&investment_id],
        )?;

        // Then, add the new tags
        for tag_id in tag_ids {
            conn.execute(
                "INSERT INTO stock_investment_tags (investment_id, tag_id, created_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![investment_id, tag_id, now],
            )?;
        }

        Ok(())
    })
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
