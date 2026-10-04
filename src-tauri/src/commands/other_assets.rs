//! Other assets commands

use crate::db::Database;
use crate::error::{AppError, Result};
use crate::models::{
    InsertOtherAsset, InsertOtherAssetTransaction, OtherAsset, OtherAssetTransaction,
};
use crate::services::other_assets as service;
use tauri::State;

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
    db: State<'_, Database>,
    data: InsertOtherAsset,
    initial_transaction: Option<InsertOtherAssetTransaction>,
) -> Result<OtherAsset> {
    db.with_conn(|conn| service::create_asset(conn, &data, initial_transaction.as_ref()))
}

/// Update other asset
#[tauri::command]
pub async fn update_other_asset(
    db: State<'_, Database>,
    id: String,
    data: InsertOtherAsset,
) -> Result<OtherAsset> {
    // Validate inputs at the trust boundary
    data.validate()?;

    let now = chrono::Utc::now().timestamp();

    db.with_conn(|conn| {
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

        conn.query_row(
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
        ).map_err(|_| AppError::NotFound("Other asset not found".into()))
    })
}

/// Delete other asset
#[tauri::command]
pub async fn delete_other_asset(db: State<'_, Database>, id: String) -> Result<()> {
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
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // Trigger historical recalculation if there were transactions
    if let Some(tx_date) = earliest_tx_date {
        crate::commands::portfolio::trigger_historical_recalculation_for_asset(
            &db,
            tx_date,
            crate::commands::portfolio::AssetType::OtherAssets,
        )
        .await
        .ok();
    }

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
    db: State<'_, Database>,
    asset_id: String,
    data: InsertOtherAssetTransaction,
) -> Result<OtherAssetTransaction> {
    let transaction_date = data.transaction_date;

    let result = db.with_conn(|conn| service::create_asset_transaction(conn, &asset_id, &data))?;

    // Update portfolio snapshot
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // Trigger historical recalculation if transaction is retrospective
    crate::commands::portfolio::trigger_historical_recalculation_for_asset(
        &db,
        transaction_date,
        crate::commands::portfolio::AssetType::OtherAssets,
    )
    .await
    .ok();

    Ok(result)
}

/// Delete other asset transaction
#[tauri::command]
pub async fn delete_other_asset_transaction(db: State<'_, Database>, tx_id: String) -> Result<()> {
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
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();

    // Trigger historical recalculation if transaction was historical
    if let Some(date) = tx_date {
        crate::commands::portfolio::trigger_historical_recalculation_for_asset(
            &db,
            date,
            crate::commands::portfolio::AssetType::OtherAssets,
        )
        .await
        .ok();
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
    db: State<'_, Database>,
    data: crate::models::InsertAssetValuation,
) -> Result<crate::models::AssetValuation> {
    let saved = db.with_conn(|conn| {
        crate::services::valuations::add_valuation(
            conn,
            crate::services::valuations::ValuationKind::OtherAsset,
            &data,
        )
    })?;
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();
    // A backdated estimate changes the history of the days it applied to
    crate::commands::portfolio::trigger_historical_recalculation_for_asset(
        &db,
        saved.valued_at,
        crate::commands::portfolio::AssetType::OtherAssets,
    )
    .await
    .ok();
    Ok(saved)
}

/// Remove an estimate; the market price follows the newest remaining one.
#[tauri::command]
pub async fn delete_other_asset_valuation(
    db: State<'_, Database>,
    valuation_id: String,
) -> Result<()> {
    db.with_conn(|conn| {
        crate::services::valuations::delete_valuation(
            conn,
            crate::services::valuations::ValuationKind::OtherAsset,
            &valuation_id,
        )
    })?;
    crate::commands::portfolio::update_todays_snapshot(&db)
        .await
        .ok();
    Ok(())
}
