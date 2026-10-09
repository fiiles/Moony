//! Portfolio projection commands
//!
//! Calculates projected portfolio values based on growth rates,
//! contributions, and loan amortization.

use crate::db::Database;
use crate::error::Result;
use crate::models::{PortfolioProjection, ProjectionInput, ProjectionSettings};
use crate::services::projection;
use chrono::Utc;
use tauri::State;
use uuid::Uuid;

/// Get all projection settings
#[tauri::command]
pub async fn get_projection_settings(db: State<'_, Database>) -> Result<Vec<ProjectionSettings>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, asset_type, yearly_growth_rate, monthly_contribution,
                    contribution_currency, enabled, created_at, updated_at
             FROM projection_settings",
        )?;

        let settings = stmt
            .query_map([], |row| {
                Ok(ProjectionSettings {
                    id: row.get(0)?,
                    asset_type: row.get(1)?,
                    yearly_growth_rate: row.get(2)?,
                    monthly_contribution: row.get(3)?,
                    contribution_currency: row.get(4)?,
                    enabled: row.get::<_, i32>(5)? != 0,
                    created_at: row.get(6)?,
                    updated_at: row.get(7)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(settings)
    })
}

/// Save projection settings (upsert)
#[tauri::command]
pub async fn save_projection_settings(
    db: State<'_, Database>,
    settings: Vec<ProjectionSettings>,
) -> Result<()> {
    let now = Utc::now().timestamp();

    db.with_conn(|conn| {
        for s in settings {
            let id = if s.id.is_empty() {
                Uuid::new_v4().to_string()
            } else {
                s.id
            };

            conn.execute(
                "INSERT INTO projection_settings
                    (id, asset_type, yearly_growth_rate, monthly_contribution,
                     contribution_currency, enabled, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(asset_type) DO UPDATE SET
                    yearly_growth_rate = excluded.yearly_growth_rate,
                    monthly_contribution = excluded.monthly_contribution,
                    contribution_currency = excluded.contribution_currency,
                    enabled = excluded.enabled,
                    updated_at = excluded.updated_at",
                rusqlite::params![
                    id,
                    s.asset_type,
                    s.yearly_growth_rate,
                    s.monthly_contribution,
                    s.contribution_currency,
                    if s.enabled { 1 } else { 0 },
                    now,
                    now,
                ],
            )?;
        }
        Ok(())
    })
}

/// Calculate the portfolio projection in `report_currency` (contributions are converted from
/// the currency they were stored in).
#[tauri::command]
pub async fn calculate_portfolio_projection(
    db: State<'_, Database>,
    input: ProjectionInput,
    report_currency: String,
) -> Result<PortfolioProjection> {
    db.with_conn(|conn| projection::calculate_projection(conn, &report_currency, &input))
}
