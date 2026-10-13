//! Other assets commands

use crate::commands::portfolio::{schedule_history_rebuild, update_todays_snapshot};
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    InsertOtherAsset, InsertOtherAssetTransaction, OtherAsset, OtherAssetTransaction,
};
use crate::services::history_plan;
use crate::services::history_recalc::{HistoryRecalc, Job};
use crate::services::other_assets as service;
use rusqlite::OptionalExtension;
use tauri::{AppHandle, State};

/// Get all other assets
#[tauri::command]
pub async fn get_all_other_assets(db: State<'_, Database>) -> Result<Vec<OtherAsset>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, name, quantity, market_price, currency, average_purchase_price,
                    yield_type, yield_value, created_at, updated_at
             FROM other_assets ORDER BY name",
        )?;

        let assets = stmt
            .query_map([], |row| {
                Ok(OtherAsset {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    quantity: row.get(2)?,
                    market_price: row.get(3)?,
                    currency: row.get(4)?,
                    average_purchase_price: row.get(5)?,
                    yield_type: row.get(6)?,
                    yield_value: row.get(7)?,
                    created_at: row.get(8)?,
                    updated_at: row.get(9)?,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        Ok(assets)
    })
}

/// Create other asset
#[tauri::command]
pub async fn create_other_asset(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    data: InsertOtherAsset,
    initial_transaction: Option<InsertOtherAssetTransaction>,
) -> Result<OtherAsset> {
    let created =
        db.with_conn(|conn| service::create_asset(conn, &data, initial_transaction.as_ref()))?;
    // An initial transaction dates the first quantity of the asset: history from its day
    if let Some(tx) = &initial_transaction {
        schedule_history_rebuild(&app, &db, &recalc, vec![Job::Days(tx.transaction_date)]);
    }
    Ok(created)
}

/// Update other asset
#[tauri::command]
pub async fn update_other_asset(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    id: String,
    data: InsertOtherAsset,
) -> Result<OtherAsset> {
    // Validate inputs at the trust boundary
    data.validate()?;

    let now = chrono::Utc::now().timestamp();

    let (updated, old_currency) = db.with_conn(|conn| {
        let old_currency: Option<String> = conn
            .query_row(
                "SELECT currency FROM other_assets WHERE id = ?1",
                [&id],
                |row| row.get(0),
            )
            .optional()?;
        conn.execute(
            "UPDATE other_assets SET name = ?1,
             quantity = COALESCE(?2, quantity), market_price = COALESCE(?3, market_price),
             currency = COALESCE(?4, currency), average_purchase_price = COALESCE(?5, average_purchase_price),
             yield_type = COALESCE(?6, yield_type), yield_value = ?7, updated_at = ?8
             WHERE id = ?9",
            rusqlite::params![
                data.name, data.quantity, data.market_price, data.currency,
                data.average_purchase_price, data.yield_type, data.yield_value, now, id
            ],
        )?;
        // A changed market price is an estimate too: keep the valuation log complete
        if data.market_price.is_some() {
            let (price, currency): (String, String) = conn.query_row(
                "SELECT market_price, currency FROM other_assets WHERE id = ?1",
                [&id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            crate::services::valuations::record_price_change(
                conn,
                crate::services::valuations::ValuationKind::OtherAsset,
                &id,
                &price,
                &currency,
                crate::services::loan_amortization::today_utc_day(),
            )?;
        }

        let updated = conn.query_row(
            "SELECT id, name, quantity, market_price, currency, average_purchase_price,
                    yield_type, yield_value, created_at, updated_at
             FROM other_assets WHERE id = ?1",
            [&id],
            |row| Ok(OtherAsset {
                id: row.get(0)?,
                name: row.get(1)?,
                quantity: row.get(2)?,
                market_price: row.get(3)?,
                currency: row.get(4)?,
                average_purchase_price: row.get(5)?,
                yield_type: row.get(6)?,
                yield_value: row.get(7)?,
                created_at: row.get(8)?,
                updated_at: row.get(9)?,
            }),
        ).map_err(|_| AppError::NotFound("Other asset not found".into()))?;
        Ok((updated, old_currency))
    })?;

    // A new currency relabels the asset on every day; a price edit is dated today
    if let Some(job) = old_currency
        .and_then(|old| history_plan::other_asset_updated(&old, data.currency.as_deref()))
    {
        schedule_history_rebuild(&app, &db, &recalc, vec![job]);
    }
    Ok(updated)
}

/// Delete other asset
#[tauri::command]
pub async fn delete_other_asset(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    id: String,
) -> Result<()> {
    // Get earliest transaction date before deleting (for historical recalc)
    let earliest_tx_date: Option<i64> = db.with_conn(|conn| {
        Ok(conn
            .query_row(
                "SELECT MIN(transaction_date) FROM other_asset_transactions WHERE asset_id = ?1",
                [&id],
                |row| row.get(0),
            )
            .ok()
            .flatten())
    })?;

    db.with_conn(|conn| {
        // Delete transactions first (due to foreign key)
        conn.execute(
            "DELETE FROM other_asset_transactions WHERE asset_id = ?1",
            [&id],
        )?;

        let changes = conn.execute("DELETE FROM other_assets WHERE id = ?1", [&id])?;
        if changes == 0 {
            return Err(AppError::NotFound("Other asset not found".into()));
        }
        Ok(())
    })?;

    // Update portfolio snapshot
    update_todays_snapshot(&db).await.ok();

    // From its first transaction day, else from the first fact
    schedule_history_rebuild(
        &app,
        &db,
        &recalc,
        vec![history_plan::other_asset_deleted(earliest_tx_date)],
    );

    Ok(())
}

/// Get transactions for other asset
#[tauri::command]
pub async fn get_other_asset_transactions(
    db: State<'_, Database>,
    asset_id: String,
) -> Result<Vec<OtherAssetTransaction>> {
    db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT id, asset_id, type, quantity, price_per_unit, currency, transaction_date, created_at
             FROM other_asset_transactions WHERE asset_id = ?1 ORDER BY transaction_date DESC"
        )?;

        let txs = stmt.query_map([&asset_id], |row| {
            Ok(OtherAssetTransaction {
                id: row.get(0)?,
                asset_id: row.get(1)?,
                tx_type: row.get(2)?,
                quantity: row.get(3)?,
                price_per_unit: row.get(4)?,
                currency: row.get(5)?,
                transaction_date: row.get(6)?,
                created_at: row.get(7)?,
            })
        })?.filter_map(|r| r.ok()).collect();

        Ok(txs)
    })
}

/// Create other asset transaction
#[tauri::command]
pub async fn create_other_asset_transaction(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    asset_id: String,
    data: InsertOtherAssetTransaction,
) -> Result<OtherAssetTransaction> {
    let transaction_date = data.transaction_date;

    let result = db.with_conn(|conn| service::create_asset_transaction(conn, &asset_id, &data))?;

    // Update portfolio snapshot
    update_todays_snapshot(&db).await.ok();

    // The quantity changes from the transaction's day on
    schedule_history_rebuild(&app, &db, &recalc, vec![Job::Days(transaction_date)]);

    Ok(result)
}

/// Delete other asset transaction
#[tauri::command]
pub async fn delete_other_asset_transaction(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    tx_id: String,
) -> Result<()> {
    // Get transaction date before deleting (for historical recalc)
    let tx_date: Option<i64> = db.with_conn(|conn| {
        Ok(conn
            .query_row(
                "SELECT transaction_date FROM other_asset_transactions WHERE id = ?1",
                [&tx_id],
                |row| row.get(0),
            )
            .ok())
    })?;

    db.with_conn(|conn| {
        // 1. Get asset_id before deleting
        let asset_id: String = conn.query_row(
            "SELECT asset_id FROM other_asset_transactions WHERE id = ?1",
            [&tx_id],
            |row| row.get(0),
        )?;

        // 2. Delete Transaction
        conn.execute(
            "DELETE FROM other_asset_transactions WHERE id = ?1",
            [&tx_id],
        )?;

        // 3. Recalculate Asset Totals
        service::recalculate_asset_totals(conn, &asset_id)?;

        Ok(())
    })?;

    // Update portfolio snapshot
    update_todays_snapshot(&db).await.ok();

    if let Some(date) = tx_date {
        schedule_history_rebuild(&app, &db, &recalc, vec![Job::Days(date)]);
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Valuation log (redesign phase 2)
// ---------------------------------------------------------------------------

/// Dated valuations (price per unit) of an asset, oldest first.
#[tauri::command]
pub async fn get_other_asset_valuations(
    db: State<'_, Database>,
    asset_id: String,
) -> Result<Vec<crate::models::AssetValuation>> {
    db.with_conn(|conn| {
        crate::services::valuations::list_valuations(
            conn,
            crate::services::valuations::ValuationKind::OtherAsset,
            &asset_id,
        )
    })
}

/// Record an estimate ("Přecenit"); the newest one becomes the market price.
#[tauri::command]
pub async fn add_other_asset_valuation(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    data: crate::models::InsertAssetValuation,
) -> Result<crate::models::AssetValuation> {
    let saved = db.with_conn(|conn| {
        crate::services::valuations::add_valuation(
            conn,
            crate::services::valuations::ValuationKind::OtherAsset,
            &data,
        )
    })?;
    update_todays_snapshot(&db).await.ok();
    // A backdated estimate changes the history of the days it applies to
    schedule_history_rebuild(&app, &db, &recalc, vec![Job::Days(saved.valued_at)]);
    Ok(saved)
}

/// Remove an estimate; the market price follows the newest remaining one.
#[tauri::command]
pub async fn delete_other_asset_valuation(
    app: AppHandle,
    db: State<'_, Database>,
    recalc: State<'_, HistoryRecalc>,
    valuation_id: String,
) -> Result<()> {
    let valued_at = db.with_conn(|conn| {
        crate::services::valuations::delete_valuation(
            conn,
            crate::services::valuations::ValuationKind::OtherAsset,
            &valuation_id,
        )
    })?;
    update_todays_snapshot(&db).await.ok();
    schedule_history_rebuild(&app, &db, &recalc, vec![Job::Days(valued_at)]);
    Ok(())
}
