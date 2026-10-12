//! Bond commands

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{Bond, InsertBond};
use tauri::State;

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
pub async fn create_bond(db: State<'_, Database>, data: InsertBond) -> Result<Bond> {
    db.with_conn(|conn| crate::services::bonds::create_bond(conn, &data))
}

/// Update bond
#[tauri::command]
pub async fn update_bond(db: State<'_, Database>, id: String, data: InsertBond) -> Result<Bond> {
    db.with_conn(|conn| crate::services::bonds::update_bond(conn, &id, &data))
}

/// Delete bond
#[tauri::command]
pub async fn delete_bond(db: State<'_, Database>, id: String) -> Result<()> {
    db.with_conn(|conn| {
        let changes = conn.execute("DELETE FROM bonds WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Bond not found".into()));
        }
        Ok(())
    })
}
