//! Real estate business logic. Extracted from commands/real_estate.rs so the
//! Tauri command and the MCP write tool share one validated path (ADR 0007).

use crate::error::Result;
use crate::models::InsertRealEstate;
use crate::services::valuations::{self, ValuationKind};
use uuid::Uuid;

/// Create a new real estate property. SINGLE SOURCE OF TRUTH for property
/// creation. Returns the new row's id; callers re-read the full record
/// (e.g. via `commands::real_estate::get_real_estate`).
///
/// A priced property also gets the first row of its valuation log, dated at
/// the creation day, so the first revaluation does not overwrite the only
/// record of the original estimate.
pub fn create_property(conn: &rusqlite::Connection, data: &InsertRealEstate) -> Result<String> {
    data.validate()?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    // Validation already accepts the frequency case-insensitively, but the UI's
    // exact-match display logic (`=== 'quarterly'` multipliers, translated
    // labels) needs the stored JSON to always be lowercase.
    let mut recurring_costs = data.recurring_costs.clone().unwrap_or_default();
    for cost in &mut recurring_costs {
        cost.frequency = cost.frequency.to_lowercase();
    }
    let rc_json = serde_json::to_string(&recurring_costs)?;
    let photos_json = serde_json::to_string(&data.photos.clone().unwrap_or_default())?;
    let market_price = data.market_price.clone().unwrap_or_else(|| "0".to_string());
    let market_price_currency = data
        .market_price_currency
        .clone()
        .unwrap_or_else(|| "CZK".to_string());

    conn.execute(
        "INSERT INTO real_estate (id, name, address, type, purchase_price, purchase_price_currency,
         market_price, market_price_currency, monthly_rent, monthly_rent_currency,
         recurring_costs, photos, notes, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14)",
        rusqlite::params![
            id,
            data.name,
            data.address,
            data.property_type,
            data.purchase_price
                .clone()
                .unwrap_or_else(|| "0".to_string()),
            data.purchase_price_currency
                .clone()
                .unwrap_or_else(|| "CZK".to_string()),
            market_price,
            market_price_currency,
            data.monthly_rent,
            data.monthly_rent_currency,
            rc_json,
            photos_json,
            data.notes,
            now,
        ],
    )?;

    valuations::record_initial_valuation(
        conn,
        ValuationKind::RealEstate,
        &id,
        &market_price,
        &market_price_currency,
        now,
    )?;

    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE real_estate (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                address TEXT NOT NULL,
                type TEXT NOT NULL,
                purchase_price TEXT NOT NULL DEFAULT '0',
                purchase_price_currency TEXT NOT NULL DEFAULT 'CZK',
                market_price TEXT NOT NULL DEFAULT '0',
                market_price_currency TEXT NOT NULL DEFAULT 'CZK',
                monthly_rent TEXT,
                monthly_rent_currency TEXT DEFAULT 'CZK',
                recurring_costs TEXT DEFAULT '[]',
                photos TEXT DEFAULT '[]',
                notes TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            CREATE TABLE real_estate_valuations (
                id TEXT PRIMARY KEY,
                real_estate_id TEXT NOT NULL REFERENCES real_estate(id) ON DELETE CASCADE,
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

    /// `(value, currency, valued_at)` of every valuation of a property, oldest first.
    fn valuation_rows(conn: &Connection, property_id: &str) -> Vec<(String, String, i64)> {
        let mut stmt = conn
            .prepare(
                "SELECT value, currency, valued_at FROM real_estate_valuations
                 WHERE real_estate_id = ?1 ORDER BY valued_at, created_at",
            )
            .unwrap();
        let rows = stmt
            .query_map([property_id], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    fn valid_insert() -> InsertRealEstate {
        InsertRealEstate {
            name: "Cottage".into(),
            address: "1 Main St".into(),
            property_type: "house".into(),
            purchase_price: None,
            purchase_price_currency: None,
            market_price: None,
            market_price_currency: None,
            monthly_rent: None,
            monthly_rent_currency: None,
            recurring_costs: None,
            photos: None,
            notes: None,
        }
    }

    #[test]
    fn create_property_inserts_row_with_defaults() {
        let conn = setup_test_db();
        let id = create_property(&conn, &valid_insert()).unwrap();
        assert!(!id.is_empty());

        let (
            name,
            address,
            property_type,
            purchase_price,
            purchase_price_currency,
            recurring_costs_json,
            photos_json,
        ): (String, String, String, String, String, String, String) = conn
            .query_row(
                "SELECT name, address, type, purchase_price, purchase_price_currency, recurring_costs, photos
                 FROM real_estate WHERE id = ?1",
                [&id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(name, "Cottage");
        assert_eq!(address, "1 Main St");
        assert_eq!(property_type, "house");
        assert_eq!(purchase_price, "0");
        assert_eq!(purchase_price_currency, "CZK");

        let recurring_costs: Vec<serde_json::Value> =
            serde_json::from_str(&recurring_costs_json).unwrap();
        assert!(recurring_costs.is_empty());
        let photos: Vec<serde_json::Value> = serde_json::from_str(&photos_json).unwrap();
        assert!(photos.is_empty());
    }

    /// The reported bug: creating a property wrote no valuation row, so the
    /// first revaluation overwrote the only record of the original estimate.
    #[test]
    fn create_property_writes_the_first_estimate() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.market_price = Some("3480000".into());
        data.market_price_currency = Some("EUR".into());

        let before = crate::services::loan_amortization::today_utc_day();
        let id = create_property(&conn, &data).unwrap();
        let after = crate::services::loan_amortization::today_utc_day();

        let rows = valuation_rows(&conn, &id);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "3480000");
        assert_eq!(rows[0].1, "EUR");
        assert!(
            rows[0].2 == before || rows[0].2 == after,
            "dated at the UTC day of the creation"
        );
    }

    #[test]
    fn create_property_without_a_market_price_has_no_estimate_yet() {
        let conn = setup_test_db();
        let id = create_property(&conn, &valid_insert()).unwrap();
        assert!(valuation_rows(&conn, &id).is_empty());

        let mut zero = valid_insert();
        zero.name = "Plot".into();
        zero.market_price = Some("0".into());
        let id = create_property(&conn, &zero).unwrap();
        assert!(valuation_rows(&conn, &id).is_empty());
    }

    #[test]
    fn first_revaluation_of_a_new_property_keeps_the_original_estimate() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.market_price = Some("3480000".into());
        let id = create_property(&conn, &data).unwrap();

        let revalued_at = crate::services::loan_amortization::today_utc_day() + 100 * 86_400;
        crate::services::valuations::add_valuation(
            &conn,
            crate::services::valuations::ValuationKind::RealEstate,
            &crate::models::InsertAssetValuation {
                asset_id: id.clone(),
                value: "3650000".into(),
                currency: Some("CZK".into()),
                valued_at: revalued_at,
                note: None,
            },
        )
        .unwrap();

        let values: Vec<String> = valuation_rows(&conn, &id)
            .into_iter()
            .map(|row| row.0)
            .collect();
        assert_eq!(values, ["3480000", "3650000"]);
    }

    #[test]
    fn create_property_rejects_invalid() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.name = "".into();
        assert!(create_property(&conn, &data).is_err());
    }

    #[test]
    fn create_property_rejects_unknown_recurring_cost_frequency() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.recurring_costs = Some(vec![crate::models::RecurringCost {
            name: "HOA".into(),
            amount: 500.0,
            frequency: "biannual".into(),
            currency: None,
        }]);
        let err = create_property(&conn, &data).unwrap_err();
        match err {
            crate::error::AppError::Validation(msg) => {
                assert_eq!(msg, "validation.recurringCostFrequencyInvalid")
            }
            other => panic!("expected Validation error, got {other:?}"),
        }
    }

    #[test]
    fn create_property_accepts_known_recurring_cost_frequencies() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.recurring_costs = Some(vec![
            crate::models::RecurringCost {
                name: "HOA".into(),
                amount: 500.0,
                frequency: "monthly".into(),
                currency: None,
            },
            crate::models::RecurringCost {
                name: "Insurance".into(),
                amount: 2000.0,
                frequency: "yearly".into(),
                currency: None,
            },
            crate::models::RecurringCost {
                name: "Maintenance".into(),
                amount: 1000.0,
                frequency: "Quarterly".into(), // case-insensitive
                currency: None,
            },
        ]);
        let id = create_property(&conn, &data).unwrap();

        // Stored JSON must be lowercased even though the input used mixed
        // case — the UI's exact-match display logic (=== 'quarterly') and
        // translated labels depend on this.
        let recurring_costs_json: String = conn
            .query_row(
                "SELECT recurring_costs FROM real_estate WHERE id = ?1",
                [&id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(recurring_costs_json.contains("\"quarterly\""));
        assert!(!recurring_costs_json.contains("Quarterly"));
    }
}
