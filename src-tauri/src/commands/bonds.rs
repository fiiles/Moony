//! Bond commands

use crate::commands::portfolio::schedule_history_rebuild;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{Bond, InsertBond};
use crate::services::history_plan;
use crate::services::history_recalc::HistoryRecalc;
use tauri::{AppHandle, State};

/// Get all bonds
#[tauri::command]
pub async fn get_all_bonds(db: State<'_, Database>) -> Result<Vec<Bond>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, isin, coupon_value, quantity, currency, interest_rate, maturity_date, created_at, updated_at
             FROM bonds ORDER BY name"
        )?;

        let bonds = stmt.query_map([], |row| {
            Ok(Bond {
                id: row.get(0)?,
                name: row.get(1)?,
                isin: row.get(2)?,
                coupon_value: row.get(3)?,
                quantity: row.get(4)?,
                currency: row.get(5)?,
                interest_rate: row.get(6)?,
                maturity_date: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            })
        })?.filter_map(|r| r.ok()).collect();

        Ok(bonds)
    })
}

/// Create bond
#[tauri::command]
pub async fn create_bond(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    data: InsertBond,
) -> Result<Bond> {
    let created = db.with_conn(|conn| crate::services::bonds::create_bond(conn, &data))?;
    // Bonds have no purchase date: they carry their current value on every day
    schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::bond_changed()]);
    Ok(created)
}

/// Update bond
#[tauri::command]
pub async fn update_bond(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    id: String,
    data: InsertBond,
) -> Result<Bond> {
    let updated = db.with_conn(|conn| crate::services::bonds::update_bond(conn, &id, &data))?;
    schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::bond_changed()]);
    Ok(updated)
}

/// Delete bond
#[tauri::command]
pub async fn delete_bond(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    id: String,
) -> Result<()> {
    db.with_conn(|conn| {
        let changes = conn.execute("DELETE FROM bonds WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Bond not found".into()));
        }
        Ok(())
    })?;
    schedule_history_rebuild(&app, &db, &recalc, vec![history_plan::bond_changed()]);
    Ok(())
}
