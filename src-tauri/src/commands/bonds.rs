//! Bond commands

use crate::commands::portfolio::schedule_history_rebuild;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{Bond, InsertBond};
use crate::services::bonds as bond_service;
use crate::services::history_plan;
use crate::services::history_recalc::HistoryRecalc;
use tauri::{AppHandle, State};

/// Get all bonds
#[tauri::command]
pub async fn get_all_bonds(db: State<'_, Database>) -> Result<Vec<Bond>> {
    db.with_conn(bond_service::list_bonds)
}

/// Create bond
#[tauri::command]
pub async fn create_bond(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    data: InsertBond,
) -> Result<Bond> {
    let created = db.with_conn(|conn| bond_service::create_bond(conn, &data))?;
    // A dated bond counts from its purchase day, an undated one on every day
    schedule_history_rebuild(
        &app,
        &db,
        &recalc,
        vec![history_plan::bond_created(created.purchase_date)],
    );
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
    let (before, updated) = db.with_conn(|conn| {
        let before = bond_service::get_bond(conn, &id)?;
        let updated = bond_service::update_bond(conn, &id, &data)?;
        Ok((before, updated))
    })?;
    // From the earlier of the old and new purchase day; every day for an undated bond whose
    // value or currency changed; nothing for an edit the history does not read (a name)
    if let Some(job) = before.and_then(|before| history_plan::bond_updated(&before, &updated)) {
        schedule_history_rebuild(&app, &db, &recalc, vec![job]);
    }
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
    let purchase_date = db.with_conn(|conn| {
        let bond = bond_service::get_bond(conn, &id)?
            .ok_or_else(|| AppError::NotFound("Bond not found".into()))?;
        bond_service::delete_bond(conn, &id)?;
        Ok(bond.purchase_date)
    })?;
    schedule_history_rebuild(
        &app,
        &db,
        &recalc,
        vec![history_plan::bond_deleted(purchase_date)],
    );
    Ok(())
}
