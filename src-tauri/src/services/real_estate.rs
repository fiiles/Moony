//! Real estate business logic. Extracted from commands/real_estate.rs so the
//! Tauri command and the MCP write tool share one validated path (ADR 0007).

use crate::error::Result;
use crate::models::InsertRealEstate;
use uuid::Uuid;

/// Create a new real estate property. SINGLE SOURCE OF TRUTH for property
/// creation. Returns the new row's id; callers re-read the full record
/// (e.g. via `commands::real_estate::get_real_estate`).
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
            data.market_price.clone().unwrap_or_else(|| "0".to_string()),
            data.market_price_currency
                .clone()
                .unwrap_or_else(|| "CZK".to_string()),
            data.monthly_rent,
            data.monthly_rent_currency,
            rc_json,
            photos_json,
            data.notes,
            now,
        ],
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
            "#,
        )
        .expect("schema");
        conn
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
