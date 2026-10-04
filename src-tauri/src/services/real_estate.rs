//! Real estate business logic. Extracted from commands/real_estate.rs so the
//! Tauri command and the MCP write tool share one validated path (ADR 0007).

use crate::error::{AppError, Result};
use crate::models::{InsertRealEstate, RealEstate};
use crate::services::loan_amortization::day_floor;
use crate::services::valuations::{self, ValuationKind};
use rusqlite::{Connection, OptionalExtension, Row};
use uuid::Uuid;

/// Columns of a property, in the order `property_from_row` reads them.
const PROPERTY_COLUMNS: &str = "id, name, address, type, purchase_price, purchase_price_currency,
     purchase_date, market_price, market_price_currency, monthly_rent, monthly_rent_currency,
     recurring_costs, photos, notes, created_at, updated_at";

fn property_from_row(row: &Row) -> rusqlite::Result<RealEstate> {
    let recurring_costs: String = row.get(11)?;
    let photos: String = row.get(12)?;
    Ok(RealEstate {
        id: row.get(0)?,
        name: row.get(1)?,
        address: row.get(2)?,
        property_type: row.get(3)?,
        purchase_price: row.get(4)?,
        purchase_price_currency: row.get(5)?,
        purchase_date: row.get(6)?,
        market_price: row.get(7)?,
        market_price_currency: row.get(8)?,
        monthly_rent: row.get(9)?,
        monthly_rent_currency: row.get(10)?,
        recurring_costs: serde_json::from_str(&recurring_costs).unwrap_or_default(),
        photos: serde_json::from_str(&photos).unwrap_or_default(),
        notes: row.get(13)?,
        created_at: row.get(14)?,
        updated_at: row.get(15)?,
    })
}

/// Every property, ordered by name.
pub fn list_properties(conn: &Connection) -> Result<Vec<RealEstate>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {PROPERTY_COLUMNS} FROM real_estate ORDER BY name"
    ))?;
    let rows = stmt.query_map([], property_from_row)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// One property, or `None` when the id is unknown.
pub fn get_property(conn: &Connection, id: &str) -> Result<Option<RealEstate>> {
    Ok(conn
        .query_row(
            &format!("SELECT {PROPERTY_COLUMNS} FROM real_estate WHERE id = ?1"),
            [id],
            property_from_row,
        )
        .optional()?)
}

/// Create a new real estate property. SINGLE SOURCE OF TRUTH for property
/// creation. Returns the new row's id; callers re-read the full record
/// (e.g. via `get_property`).
///
/// A priced property also gets the first row of its valuation log, dated at
/// the creation day, so the first revaluation does not overwrite the only
/// record of the original estimate.
pub fn create_property(conn: &Connection, data: &InsertRealEstate) -> Result<String> {
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
         recurring_costs, photos, notes, created_at, updated_at, purchase_date)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?14, ?15)",
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
            data.purchase_date.map(day_floor),
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

/// Update a property. SINGLE SOURCE OF TRUTH for property updates: the update
/// replaces the whole record, so rent, notes and the purchase date are set
/// directly (an absent one clears it), while prices and the recurring costs
/// and photos keep their stored value when the update leaves them out. `now`
/// is the current time in unix seconds.
///
/// A changed market price is an estimate too: it is written to the valuation
/// log (dated today), so the log is complete whichever way the price moves.
pub fn update_property(
    conn: &Connection,
    id: &str,
    data: &InsertRealEstate,
    now: i64,
) -> Result<()> {
    data.validate_at(now)?;

    // Lowercase each recurring cost's frequency, same as create_property —
    // validate() accepts mixed case, but the UI's exact-match display
    // logic (=== 'quarterly') needs the stored JSON to always be lowercase.
    let rc_json = data
        .recurring_costs
        .clone()
        .map(|mut costs| {
            for cost in &mut costs {
                cost.frequency = cost.frequency.to_lowercase();
            }
            serde_json::to_string(&costs)
        })
        .transpose()?;
    let photos_json = data
        .photos
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    let changed = conn.execute(
        "UPDATE real_estate SET name = ?1, address = ?2, type = ?3,
         purchase_price = COALESCE(?4, purchase_price),
         purchase_price_currency = COALESCE(?5, purchase_price_currency),
         purchase_date = ?6,
         market_price = COALESCE(?7, market_price),
         market_price_currency = COALESCE(?8, market_price_currency),
         monthly_rent = ?9, monthly_rent_currency = ?10,
         recurring_costs = COALESCE(?11, recurring_costs),
         photos = COALESCE(?12, photos), notes = ?13, updated_at = ?14
         WHERE id = ?15",
        rusqlite::params![
            data.name,
            data.address,
            data.property_type,
            data.purchase_price,
            data.purchase_price_currency,
            data.purchase_date.map(day_floor),
            data.market_price,
            data.market_price_currency,
            data.monthly_rent,
            data.monthly_rent_currency,
            rc_json,
            photos_json,
            data.notes,
            now,
            id,
        ],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound("Real estate not found".into()));
    }

    if data.market_price.is_some() {
        let (price, currency): (String, String) = conn.query_row(
            "SELECT market_price, market_price_currency FROM real_estate WHERE id = ?1",
            [id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        valuations::record_price_change(
            conn,
            ValuationKind::RealEstate,
            id,
            &price,
            &currency,
            day_floor(now),
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                purchase_date INTEGER
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
            purchase_date: None,
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

    // ---- purchase date, reads and the update ------------------------------

    const DAY: i64 = 86_400;
    /// 2026-10-04 14:30 UTC.
    const NOW: i64 = 1_791_124_200;

    fn day_of(ts: i64) -> i64 {
        ts.div_euclid(DAY) * DAY
    }

    fn bought_on(date: Option<i64>) -> InsertRealEstate {
        InsertRealEstate {
            purchase_date: date,
            ..valid_insert()
        }
    }

    #[test]
    fn create_property_stores_the_purchase_day() {
        let conn = setup_test_db();
        let bought = day_of(NOW) - 400 * DAY;

        // An MCP client may send any second of the day; the stored value is the UTC day.
        let id = create_property(&conn, &bought_on(Some(bought + 3_600))).unwrap();

        let stored = get_property(&conn, &id).unwrap().expect("property");
        assert_eq!(stored.purchase_date, Some(bought));
    }

    #[test]
    fn create_property_without_a_purchase_date_leaves_it_unknown() {
        let conn = setup_test_db();
        let id = create_property(&conn, &bought_on(None)).unwrap();
        assert_eq!(
            get_property(&conn, &id).unwrap().unwrap().purchase_date,
            None
        );
    }

    #[test]
    fn create_property_rejects_a_purchase_date_in_the_future() {
        let conn = setup_test_db();
        let tomorrow = chrono::Utc::now().timestamp() + 2 * DAY;
        let err = create_property(&conn, &bought_on(Some(tomorrow))).unwrap_err();
        assert!(
            matches!(&err, crate::error::AppError::Validation(key) if key == "validation.purchaseDateInFuture"),
            "{err:?}"
        );
        assert!(
            list_properties(&conn).unwrap().is_empty(),
            "nothing is written"
        );
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM real_estate_valuations", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(rows, 0);
    }

    #[test]
    fn get_property_reads_every_column() {
        let conn = setup_test_db();
        let bought = day_of(NOW) - 1_000 * DAY;
        let mut data = bought_on(Some(bought));
        data.purchase_price = Some("2900000".into());
        data.purchase_price_currency = Some("EUR".into());
        data.market_price = Some("3480000".into());
        data.market_price_currency = Some("USD".into());
        data.monthly_rent = Some("16500".into());
        data.monthly_rent_currency = Some("CZK".into());
        data.recurring_costs = Some(vec![crate::models::RecurringCost {
            name: "HOA".into(),
            amount: 500.0,
            frequency: "Monthly".into(),
            currency: Some("CZK".into()),
        }]);
        data.photos = Some(vec!["a.jpg".into()]);
        data.notes = Some("note".into());
        let id = create_property(&conn, &data).unwrap();

        let stored = get_property(&conn, &id).unwrap().expect("property");
        assert_eq!(stored.id, id);
        assert_eq!(stored.name, "Cottage");
        assert_eq!(stored.address, "1 Main St");
        assert_eq!(stored.property_type, "house");
        assert_eq!(stored.purchase_price, "2900000");
        assert_eq!(stored.purchase_price_currency, "EUR");
        assert_eq!(stored.purchase_date, Some(bought));
        assert_eq!(stored.market_price, "3480000");
        assert_eq!(stored.market_price_currency, "USD");
        assert_eq!(stored.monthly_rent.as_deref(), Some("16500"));
        assert_eq!(stored.monthly_rent_currency.as_deref(), Some("CZK"));
        assert_eq!(stored.recurring_costs.len(), 1);
        assert_eq!(stored.recurring_costs[0].frequency, "monthly");
        assert_eq!(stored.photos, ["a.jpg"]);
        assert_eq!(stored.notes.as_deref(), Some("note"));
        assert!(stored.created_at > 0 && stored.updated_at >= stored.created_at);
    }

    #[test]
    fn get_property_of_an_unknown_id_is_none() {
        let conn = setup_test_db();
        assert!(get_property(&conn, "missing").unwrap().is_none());
    }

    #[test]
    fn list_properties_is_ordered_by_name() {
        let conn = setup_test_db();
        for (name, date) in [("Villa", Some(day_of(NOW))), ("Atelier", None)] {
            let mut data = bought_on(date);
            data.name = name.into();
            create_property(&conn, &data).unwrap();
        }
        let listed = list_properties(&conn).unwrap();
        let names: Vec<&str> = listed.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["Atelier", "Villa"]);
        assert_eq!(listed[0].purchase_date, None);
        assert_eq!(listed[1].purchase_date, Some(day_of(NOW)));
    }

    #[test]
    fn update_property_sets_the_purchase_date_directly() {
        let conn = setup_test_db();
        let id = create_property(&conn, &bought_on(Some(day_of(NOW) - 400 * DAY))).unwrap();

        // The update replaces the whole record: a new date is stored as its day...
        let moved = day_of(NOW) - 100 * DAY;
        update_property(&conn, &id, &bought_on(Some(moved + 7_200)), NOW).unwrap();
        assert_eq!(
            get_property(&conn, &id).unwrap().unwrap().purchase_date,
            Some(moved)
        );

        // ...and an absent one clears it.
        update_property(&conn, &id, &bought_on(None), NOW).unwrap();
        assert_eq!(
            get_property(&conn, &id).unwrap().unwrap().purchase_date,
            None
        );
    }

    #[test]
    fn update_property_rejects_a_future_purchase_date_and_changes_nothing() {
        let conn = setup_test_db();
        let bought = day_of(NOW) - 400 * DAY;
        let id = create_property(&conn, &bought_on(Some(bought))).unwrap();

        let mut data = bought_on(Some(day_of(NOW) + 2 * DAY));
        data.name = "Renamed".into();
        let err = update_property(&conn, &id, &data, NOW).unwrap_err();
        assert!(
            matches!(&err, crate::error::AppError::Validation(key) if key == "validation.purchaseDateInFuture"),
            "{err:?}"
        );

        let stored = get_property(&conn, &id).unwrap().unwrap();
        assert_eq!(stored.name, "Cottage");
        assert_eq!(stored.purchase_date, Some(bought));
    }

    #[test]
    fn update_property_keeps_stored_prices_the_update_leaves_out() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.purchase_price = Some("2900000".into());
        data.market_price = Some("3480000".into());
        data.recurring_costs = Some(vec![crate::models::RecurringCost {
            name: "HOA".into(),
            amount: 500.0,
            frequency: "monthly".into(),
            currency: None,
        }]);
        let id = create_property(&conn, &data).unwrap();

        let mut update = valid_insert();
        update.name = "Cottage 2".into();
        update.notes = Some("new note".into());
        update_property(&conn, &id, &update, NOW).unwrap();

        let stored = get_property(&conn, &id).unwrap().unwrap();
        assert_eq!(stored.name, "Cottage 2");
        assert_eq!(stored.notes.as_deref(), Some("new note"));
        assert_eq!(stored.purchase_price, "2900000");
        assert_eq!(stored.market_price, "3480000");
        assert_eq!(stored.recurring_costs.len(), 1);
    }

    #[test]
    fn update_property_records_a_changed_market_price_as_an_estimate() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.market_price = Some("3480000".into());
        let id = create_property(&conn, &data).unwrap();
        // The creation day is "today" for the real clock; pretend the edit is 30 days later.
        let later = chrono::Utc::now().timestamp() + 30 * DAY;

        let mut same = valid_insert();
        same.market_price = Some("3480000".into());
        update_property(&conn, &id, &same, later).unwrap();
        assert_eq!(
            valuation_rows(&conn, &id).len(),
            1,
            "an unchanged price is not an estimate"
        );

        let mut revalued = valid_insert();
        revalued.market_price = Some("3650000".into());
        update_property(&conn, &id, &revalued, later).unwrap();

        let rows = valuation_rows(&conn, &id);
        let values: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
        assert_eq!(
            values,
            ["3480000", "3650000"],
            "the original estimate survives"
        );
        assert_eq!(rows[1].2, day_of(later));
    }

    #[test]
    fn update_property_of_an_unknown_id_is_not_found() {
        let conn = setup_test_db();
        let err = update_property(&conn, "missing", &valid_insert(), NOW).unwrap_err();
        assert!(
            matches!(err, crate::error::AppError::NotFound(_)),
            "{err:?}"
        );
    }

    #[test]
    fn update_property_still_validates_the_rest_of_the_record() {
        let conn = setup_test_db();
        let id = create_property(&conn, &valid_insert()).unwrap();
        let mut data = valid_insert();
        data.name = "".into();
        assert!(update_property(&conn, &id, &data, NOW).is_err());
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
