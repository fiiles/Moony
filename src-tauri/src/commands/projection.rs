//! Portfolio projection commands
//!
//! Calculates projected portfolio values based on growth rates,
//! contributions, and loan amortization.

use crate::db::Database;
use crate::error::Result;
use crate::models::{PortfolioProjection, ProjectionInput, ProjectionSettings};
use crate::services::projection;
use tauri::State;

/// Get all projection settings
#[tauri::command]
pub async fn get_projection_settings(db: State<'_, Database>) -> Result<Vec<ProjectionSettings>> {
    db.with_conn(projection::list_settings)
}

/// Save projection settings (upsert); the service validates every entry first
#[tauri::command]
pub async fn save_projection_settings(
    db: State<'_, Database>,
    settings: Vec<ProjectionSettings>,
) -> Result<()> {
    db.with_conn(|conn| projection::save_settings(conn, settings))
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
