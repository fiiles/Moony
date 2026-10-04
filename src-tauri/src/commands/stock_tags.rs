//! Stock tags commands for investment categorization and analysis

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    InsertStockTag, InsertStockTagGroup, StockInvestmentWithTags, StockTag, StockTagGroup,
    TagMetrics,
};
use crate::services::currency::convert_to_czk;
use tauri::State;
use uuid::Uuid;

/// Type alias for investment row data (id, ticker, company_name, quantity, current_price, price_currency, dividend, dividend_currency)
type InvestmentRow = (
    String,
    String,
    String,
    String,
    String,
    String,
    String,
    String,
);

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

/// Get all investments with their tags and computed metrics for analysis
#[tauri::command]
pub async fn get_stocks_analysis(db: State<'_, Database>) -> Result<Vec<StockInvestmentWithTags>> {
    db.with_conn(|conn| {
        // Historical cost basis derived from transactions (ADR 0001: never
        // convert old values at today's rate).
        let cost_map = crate::services::cost_basis::cost_basis_for_all(
            conn,
            crate::services::cost_basis::TxTable::Stocks,
        )?;

        // Get all investments with current prices
        let mut stmt = conn.prepare(
            "SELECT
                si.id,
                si.ticker,
                si.company_name,
                si.quantity,
                COALESCE(CASE WHEN spo.price IS NOT NULL AND (sd.fetched_at IS NULL OR sd.fetched_at <= spo.updated_at) THEN spo.price ELSE sd.original_price END, '0') as current_price,
                COALESCE(CASE WHEN spo.price IS NOT NULL AND (sd.fetched_at IS NULL OR sd.fetched_at <= spo.updated_at) THEN spo.currency ELSE sd.currency END, 'USD') as price_currency,
                COALESCE(do.yearly_dividend_sum, dd.yearly_dividend_sum, '0') as dividend_amount,
                COALESCE(do.currency, dd.currency, 'USD') as dividend_currency
             FROM stock_investments si
             LEFT JOIN stock_data sd ON si.ticker = sd.ticker
             LEFT JOIN stock_price_overrides spo ON si.ticker = spo.ticker
             LEFT JOIN dividend_data dd ON si.ticker = dd.ticker
             LEFT JOIN dividend_overrides do ON si.ticker = do.ticker
             ORDER BY si.company_name",
        )?;

        let investments: Vec<InvestmentRow> = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })?
            .filter_map(|r| r.ok())
            .collect();

        let mut result = Vec::new();

        for (
            id,
            ticker,
            company_name,
            quantity_str,
            current_price_str,
            price_currency,
            dividend_str,
            dividend_currency,
        ) in investments
        {
            let quantity: f64 = quantity_str.parse().unwrap_or(0.0);
            let current_price: f64 = current_price_str.parse().unwrap_or(0.0);
            let yearly_dividend: f64 = dividend_str.parse().unwrap_or(0.0);

            // Current value converts at today's rate; cost basis comes from
            // the per-transaction historical derivation.
            let current_price_czk = convert_to_czk(current_price, &price_currency);
            let yearly_dividend_czk = convert_to_czk(yearly_dividend, &dividend_currency);

            let position = cost_map.get(&id);
            let current_value = quantity * current_price_czk;
            let cost_basis = position.map(|p| p.cost_basis_czk).unwrap_or(0.0);
            let avg_price_str = position
                .map(|p| p.average_price_native().to_string())
                .unwrap_or_else(|| "0".to_string());
            let gain_loss = current_value - cost_basis;
            let gain_loss_percent = if cost_basis > 0.0 {
                (gain_loss / cost_basis) * 100.0
            } else {
                0.0
            };
            let dividend_yield = quantity * yearly_dividend_czk;

            // Get tags for this investment (with group_id)
            let mut tag_stmt = conn.prepare(
                "SELECT t.id, t.name, t.color, t.group_id, t.created_at
                 FROM stock_tags t
                 INNER JOIN stock_investment_tags sit ON t.id = sit.tag_id
                 WHERE sit.investment_id = ?1
                 ORDER BY t.name",
            )?;

            let tags: Vec<StockTag> = tag_stmt
                .query_map([&id], |row| {
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

            result.push(StockInvestmentWithTags {
                id,
                ticker,
                company_name,
                quantity: quantity_str,
                average_price: avg_price_str,
                current_price: format!("{:.2}", current_price_czk),
                current_value,
                gain_loss,
                gain_loss_percent,
                dividend_yield,
                tags,
            });
        }

        Ok(result)
    })
}

/// Get aggregated metrics per tag
#[tauri::command]
pub async fn get_tag_metrics(
    db: State<'_, Database>,
    tag_ids: Vec<String>,
) -> Result<Vec<TagMetrics>> {
    // If no tags specified, return metrics for all tags
    let stocks = get_stocks_analysis(db.clone()).await?;

    db.with_conn(|conn| {
        // First, get the tags to process
        let tags_to_process: Vec<StockTag> = if tag_ids.is_empty() {
            // Get all tags
            let mut stmt = conn.prepare(
                "SELECT id, name, color, group_id, created_at FROM stock_tags ORDER BY name",
            )?;
            let tags: Vec<StockTag> = stmt
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
            tags
        } else {
            // Get specified tags one by one to avoid lifetime issues
            let mut tags = Vec::new();
            for tag_id in &tag_ids {
                if let Ok(tag) = conn.query_row(
                    "SELECT id, name, color, group_id, created_at FROM stock_tags WHERE id = ?1",
                    [tag_id],
                    |row| {
                        Ok(StockTag {
                            id: row.get(0)?,
                            name: row.get(1)?,
                            color: row.get(2)?,
                            group_id: row.get(3)?,
                            created_at: row.get(4)?,
                        })
                    },
                ) {
                    tags.push(tag);
                }
            }
            tags
        };

        // Calculate total portfolio value for percentage calculations
        let total_portfolio_value: f64 = stocks.iter().map(|s| s.current_value).sum();

        let mut metrics = Vec::new();

        for tag in tags_to_process {
            // Find stocks with this tag
            let tagged_stocks: Vec<&StockInvestmentWithTags> = stocks
                .iter()
                .filter(|s| s.tags.iter().any(|t| t.id == tag.id))
                .collect();

            let total_value: f64 = tagged_stocks.iter().map(|s| s.current_value).sum();
            // Use pre-calculated gain_loss from stocks (already converted to CZK)
            let gain_loss: f64 = tagged_stocks.iter().map(|s| s.gain_loss).sum();
            // Derive total_cost from value and gain/loss for consistency
            let total_cost = total_value - gain_loss;
            let gain_loss_percent = if total_cost > 0.0 {
                (gain_loss / total_cost) * 100.0
            } else {
                0.0
            };
            let estimated_yearly_dividend: f64 =
                tagged_stocks.iter().map(|s| s.dividend_yield).sum();
            let portfolio_percent = if total_portfolio_value > 0.0 {
                (total_value / total_portfolio_value) * 100.0
            } else {
                0.0
            };

            metrics.push(TagMetrics {
                tag,
                total_value,
                total_cost,
                gain_loss,
                gain_loss_percent,
                estimated_yearly_dividend,
                portfolio_percent,
                holdings_count: tagged_stocks.len() as i32,
            });
        }

        Ok(metrics)
    })
}
