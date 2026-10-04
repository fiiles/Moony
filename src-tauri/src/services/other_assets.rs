//! Other assets business logic. Extracted from commands/other_assets.rs so
//! the Tauri command and the MCP write tool share one validated path
//! (ADR 0007).

use crate::error::{AppError, Result};
use crate::models::{
    InsertOtherAsset, InsertOtherAssetTransaction, OtherAsset, OtherAssetTransaction,
};
use crate::services::valuations::{self, ValuationKind};
use rusqlite::OptionalExtension;
use uuid::Uuid;

/// Recalculate and persist an asset's aggregate quantity and average
/// purchase price from its transaction history. MOVED verbatim from
/// commands/other_assets.rs.
pub fn recalculate_asset_totals(conn: &rusqlite::Connection, asset_id: &str) -> Result<()> {
    let mut stmt = conn.prepare(
        "SELECT type, quantity, price_per_unit FROM other_asset_transactions WHERE asset_id = ?1",
    )?;

    let rows = stmt.query_map([asset_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    let mut total_quantity = 0.0;
    let mut total_cost = 0.0;
    let mut total_buy_quantity = 0.0;

    for row in rows {
        let (tx_type, qty_str, price_str) = row?;
        let qty = qty_str.parse::<f64>().unwrap_or(0.0);
        let price = price_str.parse::<f64>().unwrap_or(0.0);

        if tx_type == "buy" {
            total_quantity += qty;
            total_buy_quantity += qty;
            total_cost += qty * price;
        } else if tx_type == "sell" {
            total_quantity -= qty;
        }
    }

    let average_purchase_price = if total_buy_quantity > 0.0 {
        total_cost / total_buy_quantity
    } else {
        0.0
    };

    conn.execute(
        "UPDATE other_assets SET quantity = ?1, average_purchase_price = ?2, updated_at = ?3 WHERE id = ?4",
        rusqlite::params![
            total_quantity.to_string(),
            average_purchase_price.to_string(),
            chrono::Utc::now().timestamp(),
            asset_id
        ],
    )?;

    Ok(())
}

/// Create a new other asset, optionally with an initial buy/sell
/// transaction. SINGLE SOURCE OF TRUTH for other-asset creation. A priced
/// asset also gets the first row of its valuation log (price per unit, dated
/// at the creation day), so the first revaluation does not overwrite the only
/// record of the original estimate.
///
/// Quirk preserved from the original command: totals are NOT recalculated
/// after the initial transaction, so `quantity`/`averagePurchasePrice` on the
/// returned/stored asset stay at their inserted defaults even when an
/// `initial_transaction` is supplied. This is byte-compatible with today's
/// UI behavior — noted, not fixed.
pub fn create_asset(
    conn: &rusqlite::Connection,
    data: &InsertOtherAsset,
    initial_transaction: Option<&InsertOtherAssetTransaction>,
) -> Result<OtherAsset> {
    data.validate()?;
    if let Some(tx) = initial_transaction {
        tx.validate()?;
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let market_price = data.market_price.clone().unwrap_or_else(|| "0".to_string());
    let currency = data.currency.clone().unwrap_or_else(|| "CZK".to_string());

    // The asset, its first estimate and its initial transaction are saved together or not at all
    let db_tx = conn.unchecked_transaction()?;
    db_tx.execute(
        "INSERT INTO other_assets
         (id, name, quantity, market_price, currency, average_purchase_price, yield_type, yield_value, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
        rusqlite::params![
            id,
            data.name,
            data.quantity.clone().unwrap_or_else(|| "0".to_string()),
            market_price,
            currency,
            data.average_purchase_price
                .clone()
                .unwrap_or_else(|| "0".to_string()),
            data.yield_type.clone().unwrap_or_else(|| "none".to_string()),
            data.yield_value,
            now,
        ],
    )?;

    valuations::record_initial_valuation(
        &db_tx,
        ValuationKind::OtherAsset,
        &id,
        &market_price,
        &currency,
        now,
    )?;

    if let Some(tx) = initial_transaction {
        let tx_id = Uuid::new_v4().to_string();
        // Normalize enum-like tx_type to lowercase so the stored value matches
        // both recalculate_asset_totals's exact "buy"/"sell" comparisons and
        // the dedup queries (mirrors services/bank_accounts.rs::create_transaction).
        let tx_type = tx.tx_type.to_lowercase();
        db_tx.execute(
            "INSERT INTO other_asset_transactions
             (id, asset_id, type, quantity, price_per_unit, currency, transaction_date, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                tx_id,
                id,
                tx_type,
                tx.quantity,
                tx.price_per_unit,
                tx.currency,
                tx.transaction_date,
                now
            ],
        )?;
    }
    db_tx.commit()?;

    conn.query_row(
        "SELECT id, name, quantity, market_price, currency, average_purchase_price,
                yield_type, yield_value, created_at, updated_at
         FROM other_assets WHERE id = ?1",
        [&id],
        |row| {
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
        },
    )
    .map_err(|e| e.into())
}

/// Create a transaction against an existing other asset and recalculate its
/// totals. SINGLE SOURCE OF TRUTH for other-asset-transaction creation.
///
/// Hardening beyond the original command (spec D2/D10): verifies the asset
/// exists before inserting. The pre-extraction command inserted an orphan
/// transaction row for a bad `asset_id` with no error; the MCP path needs
/// the check and the UI benefits too.
pub fn create_asset_transaction(
    conn: &rusqlite::Connection,
    asset_id: &str,
    data: &InsertOtherAssetTransaction,
) -> Result<OtherAssetTransaction> {
    data.validate()?;

    let exists: Option<String> = conn
        .query_row(
            "SELECT id FROM other_assets WHERE id = ?1",
            [asset_id],
            |row| row.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Err(AppError::NotFound(format!(
            "Other asset not found: {}",
            asset_id
        )));
    }

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();

    // Normalize enum-like tx_type to lowercase so the stored value matches
    // both recalculate_asset_totals's exact "buy"/"sell" comparisons and the
    // dedup queries, which compare a lowercased tx_type against the stored
    // value (mirrors services/bank_accounts.rs::create_transaction).
    let tx_type = data.tx_type.to_lowercase();

    // 1. Insert Transaction
    conn.execute(
        "INSERT INTO other_asset_transactions
         (id, asset_id, type, quantity, price_per_unit, currency, transaction_date, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![
            id,
            asset_id,
            tx_type,
            data.quantity,
            data.price_per_unit,
            data.currency,
            data.transaction_date,
            now
        ],
    )?;

    // 2. Recalculate Asset Totals
    recalculate_asset_totals(conn, asset_id)?;

    Ok(OtherAssetTransaction {
        id,
        asset_id: asset_id.to_string(),
        tx_type,
        quantity: data.quantity.clone(),
        price_per_unit: data.price_per_unit.clone(),
        currency: data.currency.clone(),
        transaction_date: data.transaction_date,
        created_at: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE other_assets (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                quantity TEXT NOT NULL DEFAULT '0',
                market_price TEXT NOT NULL DEFAULT '0',
                currency TEXT NOT NULL DEFAULT 'CZK',
                average_purchase_price TEXT NOT NULL DEFAULT '0',
                yield_type TEXT NOT NULL DEFAULT 'none',
                yield_value TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE other_asset_transactions (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL REFERENCES other_assets(id) ON DELETE CASCADE,
                type TEXT NOT NULL,
                quantity TEXT NOT NULL,
                price_per_unit TEXT NOT NULL,
                currency TEXT NOT NULL,
                transaction_date INTEGER NOT NULL,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );

            CREATE TABLE other_asset_valuations (
                id TEXT PRIMARY KEY,
                asset_id TEXT NOT NULL REFERENCES other_assets(id) ON DELETE CASCADE,
                value TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                valued_at INTEGER NOT NULL,
                note TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn valid_asset() -> InsertOtherAsset {
        InsertOtherAsset {
            name: "Gold coins".into(),
            quantity: None,
            market_price: None,
            currency: None,
            average_purchase_price: None,
            yield_type: None,
            yield_value: None,
        }
    }

    fn valid_tx() -> InsertOtherAssetTransaction {
        InsertOtherAssetTransaction {
            asset_id: None,
            tx_type: "buy".into(),
            quantity: "2".into(),
            price_per_unit: "5000".into(),
            currency: "CZK".into(),
            transaction_date: 1_700_000_000,
        }
    }

    #[test]
    fn create_asset_applies_defaults() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        assert_eq!(asset.quantity, "0");
        assert_eq!(asset.currency, "CZK");
        assert_eq!(asset.yield_type, "none");
        assert_eq!(asset.average_purchase_price, "0");
    }

    /// `(value, currency, valued_at)` of every valuation of an asset, oldest first.
    fn valuation_rows(conn: &Connection, asset_id: &str) -> Vec<(String, String, i64)> {
        let mut stmt = conn
            .prepare(
                "SELECT value, currency, valued_at FROM other_asset_valuations
                 WHERE asset_id = ?1 ORDER BY valued_at, created_at",
            )
            .unwrap();
        let rows = stmt
            .query_map([asset_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    #[test]
    fn create_asset_writes_the_first_estimate() {
        let conn = setup_test_db();
        let mut data = valid_asset();
        data.market_price = Some("62000".into());
        data.currency = Some("EUR".into());

        let before = crate::services::loan_amortization::today_utc_day();
        let asset = create_asset(&conn, &data, None).unwrap();
        let after = crate::services::loan_amortization::today_utc_day();

        let rows = valuation_rows(&conn, &asset.id);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "62000");
        assert_eq!(rows[0].1, "EUR");
        assert!(
            rows[0].2 == before || rows[0].2 == after,
            "dated at the UTC day of the creation"
        );
    }

    #[test]
    fn create_asset_without_a_price_has_no_estimate_yet() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        assert!(valuation_rows(&conn, &asset.id).is_empty());

        let mut blank = valid_asset();
        blank.name = "Stamps".into();
        blank.market_price = Some("".into());
        let asset = create_asset(&conn, &blank, None).unwrap();
        assert!(valuation_rows(&conn, &asset.id).is_empty());
    }

    /// The asset, its first estimate and its initial transaction are saved
    /// together or not at all: a failed write must not leave an asset behind
    /// that a retry duplicates.
    #[test]
    fn create_asset_saves_nothing_when_a_write_fails() {
        let conn = setup_test_db();
        conn.execute_batch(
            "CREATE TRIGGER fail_valuation_insert BEFORE INSERT ON other_asset_valuations
             BEGIN SELECT RAISE(ABORT, 'valuation write failed'); END;",
        )
        .expect("trigger");
        let mut data = valid_asset();
        data.market_price = Some("62000".into());

        assert!(create_asset(&conn, &data, None).is_err());
        let assets: i64 = conn
            .query_row("SELECT COUNT(*) FROM other_assets", [], |r| r.get(0))
            .unwrap();
        assert_eq!(assets, 0);
    }

    #[test]
    fn create_asset_rejects_invalid() {
        let conn = setup_test_db();
        let mut data = valid_asset();
        data.name = "".into();
        assert!(create_asset(&conn, &data, None).is_err());
    }

    #[test]
    fn create_asset_with_initial_transaction_does_not_recalculate() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), Some(&valid_tx())).unwrap();

        // Transaction row exists...
        let tx_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM other_asset_transactions WHERE asset_id = ?1",
                [&asset.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(tx_count, 1);

        // ...but totals are untouched (preserved quirk).
        assert_eq!(asset.quantity, "0");
        assert_eq!(asset.average_purchase_price, "0");
    }

    #[test]
    fn create_asset_transaction_recalculates_totals() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();

        let tx = create_asset_transaction(&conn, &asset.id, &valid_tx()).unwrap();
        assert_eq!(tx.asset_id, asset.id);

        let (quantity, average_purchase_price): (String, String) = conn
            .query_row(
                "SELECT quantity, average_purchase_price FROM other_assets WHERE id = ?1",
                [&asset.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(quantity, "2");
        assert_eq!(average_purchase_price, "5000");
    }

    #[test]
    fn create_asset_transaction_rejects_unknown_asset() {
        let conn = setup_test_db();
        let result = create_asset_transaction(&conn, "ghost-asset", &valid_tx());
        assert!(matches!(result.unwrap_err(), AppError::NotFound(_)));
    }

    #[test]
    fn create_asset_transaction_rejects_invalid_data() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        let mut data = valid_tx();
        data.quantity = "0".into();
        assert!(create_asset_transaction(&conn, &asset.id, &data).is_err());
    }

    #[test]
    fn create_asset_transaction_normalizes_type_to_lowercase() {
        let conn = setup_test_db();
        let asset = create_asset(&conn, &valid_asset(), None).unwrap();
        let mut data = valid_tx();
        data.tx_type = "BUY".into();

        let tx = create_asset_transaction(&conn, &asset.id, &data).unwrap();
        assert_eq!(
            tx.tx_type, "buy",
            "returned struct must carry the normalized value"
        );

        let stored_type: String = conn
            .query_row(
                "SELECT type FROM other_asset_transactions WHERE id = ?1",
                [&tx.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_type, "buy");

        // Mixed-case must still be visible to recalculate_asset_totals's
        // exact "buy" comparison.
        let quantity: String = conn
            .query_row(
                "SELECT quantity FROM other_assets WHERE id = ?1",
                [&asset.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(quantity, "2");
    }

    #[test]
    fn create_asset_normalizes_initial_transaction_type_to_lowercase() {
        let conn = setup_test_db();
        let mut tx = valid_tx();
        tx.tx_type = "Buy".into();

        let asset = create_asset(&conn, &valid_asset(), Some(&tx)).unwrap();

        let stored_type: String = conn
            .query_row(
                "SELECT type FROM other_asset_transactions WHERE asset_id = ?1",
                [&asset.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored_type, "buy");
    }
}
