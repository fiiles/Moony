//! Bonds business logic. Extracted from commands/bonds.rs so the Tauri
//! command and the MCP write tool share one validated path (ADR 0007).

use crate::error::{AppError, Result};
use crate::models::{Bond, InsertBond};
use uuid::Uuid;

/// The columns of a bond row, in the order [`bond_from_row`] reads them.
const BOND_COLUMNS: &str = "id, name, isin, coupon_value, quantity, currency, interest_rate, \
     maturity_date, purchase_date, created_at, updated_at";

fn bond_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Bond> {
    Ok(Bond {
        id: row.get(0)?,
        name: row.get(1)?,
        isin: row.get(2)?,
        coupon_value: row.get(3)?,
        quantity: row.get(4)?,
        currency: row.get(5)?,
        interest_rate: row.get(6)?,
        maturity_date: row.get(7)?,
        purchase_date: row.get(8)?,
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

/// Every bond, ordered by name.
pub fn list_bonds(conn: &rusqlite::Connection) -> Result<Vec<Bond>> {
    let mut stmt = conn.prepare(&format!("SELECT {BOND_COLUMNS} FROM bonds ORDER BY name"))?;
    let bonds = stmt
        .query_map([], bond_from_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(bonds)
}

/// One bond; `None` when there is no such id.
pub fn get_bond(conn: &rusqlite::Connection, id: &str) -> Result<Option<Bond>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            &format!("SELECT {BOND_COLUMNS} FROM bonds WHERE id = ?1"),
            [id],
            bond_from_row,
        )
        .optional()?)
}

/// The purchase day stored for a validated input: its UTC midnight.
fn purchase_day(data: &InsertBond) -> Result<Option<i64>> {
    data.purchase_date
        .map(|date| crate::models::validation::utc_day(date, "validation.invalidDate"))
        .transpose()
}

/// Create a new bond. SINGLE SOURCE OF TRUTH for bond creation.
pub fn create_bond(conn: &rusqlite::Connection, data: &InsertBond) -> Result<Bond> {
    data.validate()?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let interest_rate = data
        .interest_rate
        .clone()
        .unwrap_or_else(|| "0".to_string());
    let currency = data
        .currency
        .clone()
        .unwrap_or_else(|| crate::constants::STORAGE_DEFAULT_CURRENCY.to_string());
    let quantity = data.quantity.clone().unwrap_or_else(|| "1".to_string());

    let maturity_date = data
        .maturity_date
        .map(|date| crate::models::validation::utc_day(date, "validation.maturityDateInvalid"))
        .transpose()?;
    let purchase_date = purchase_day(data)?;
    conn.execute(
        "INSERT INTO bonds (id, name, isin, coupon_value, quantity, currency, interest_rate, maturity_date, purchase_date, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)",
        rusqlite::params![id, data.name, data.isin.as_deref(), data.coupon_value, quantity, currency, interest_rate, maturity_date, purchase_date, now],
    )?;

    Ok(Bond {
        id,
        name: data.name.clone(),
        isin: data.isin.clone(),
        coupon_value: data.coupon_value.clone(),
        quantity,
        currency,
        interest_rate,
        maturity_date,
        purchase_date,
        created_at: now,
        updated_at: now,
    })
}

/// Update a bond through the same validation and date rules as creation. The dates replace the
/// stored ones, so an edit that leaves one out clears it (the form sends the whole record).
pub fn update_bond(conn: &rusqlite::Connection, id: &str, data: &InsertBond) -> Result<Bond> {
    data.validate()?;
    let existing_quantity: String = conn
        .query_row("SELECT quantity FROM bonds WHERE id = ?1", [id], |row| {
            row.get(0)
        })
        .map_err(|_| AppError::NotFound("Bond not found".into()))?;
    let quantity = data.quantity.as_deref().unwrap_or(&existing_quantity);
    let coupon =
        crate::models::validation::finite_number(&data.coupon_value, "validation.invalidAmount")?;
    let qty = crate::models::validation::finite_number(quantity, "validation.invalidQuantity")?;
    if !(coupon * qty).is_finite() {
        return Err(AppError::Validation("validation.invalidAmount".into()));
    }
    let maturity_date = data
        .maturity_date
        .map(|date| crate::models::validation::utc_day(date, "validation.maturityDateInvalid"))
        .transpose()?;
    let purchase_date = purchase_day(data)?;
    let now = chrono::Utc::now().timestamp();
    conn.execute(
        "UPDATE bonds SET name = ?1, isin = ?2, coupon_value = ?3,
         quantity = COALESCE(?4, quantity), currency = COALESCE(?5, currency),
         interest_rate = COALESCE(?6, interest_rate), maturity_date = ?7, purchase_date = ?8,
         updated_at = ?9
         WHERE id = ?10",
        rusqlite::params![
            data.name,
            data.isin.as_deref(),
            data.coupon_value,
            data.quantity,
            data.currency,
            data.interest_rate,
            maturity_date,
            purchase_date,
            now,
            id
        ],
    )?;
    get_bond(conn, id)?.ok_or_else(|| AppError::NotFound("Bond not found".into()))
}

/// Delete a bond; `NotFound` when there is none.
pub fn delete_bond(conn: &rusqlite::Connection, id: &str) -> Result<()> {
    let changes = conn.execute("DELETE FROM bonds WHERE id = ?1", [id])?;
    if changes == 0 {
        return Err(AppError::NotFound("Bond not found".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE bonds (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                isin TEXT,
                coupon_value TEXT NOT NULL,
                interest_rate TEXT NOT NULL DEFAULT '0',
                maturity_date INTEGER,
                currency TEXT NOT NULL DEFAULT 'CZK',
                quantity TEXT NOT NULL DEFAULT '1',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                purchase_date INTEGER
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn valid_insert() -> InsertBond {
        InsertBond {
            name: "Test bond".into(),
            isin: None,
            coupon_value: "1000".into(),
            quantity: None,
            currency: None,
            interest_rate: None,
            maturity_date: None,
            purchase_date: None,
        }
    }

    #[test]
    fn create_bond_applies_defaults() {
        let conn = setup_test_db();
        let bond = create_bond(&conn, &valid_insert()).unwrap();
        assert_eq!(bond.quantity, "1");
        assert_eq!(bond.currency, "CZK");
        assert_eq!(bond.interest_rate, "0");
    }

    #[test]
    fn create_bond_rejects_invalid() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.name = "".into();
        assert!(create_bond(&conn, &data).is_err());
    }

    #[test]
    fn bond_rejects_non_finite_values_and_product_overflow_before_writing() {
        let conn = setup_test_db();
        for invalid in ["NaN", "inf", "-inf", "1e999"] {
            for field in ["coupon", "quantity", "interest"] {
                let mut data = valid_insert();
                match field {
                    "coupon" => data.coupon_value = invalid.into(),
                    "quantity" => data.quantity = Some(invalid.into()),
                    _ => data.interest_rate = Some(invalid.into()),
                }
                assert!(
                    create_bond(&conn, &data).is_err(),
                    "accepted {field}={invalid}"
                );
            }
        }
        let mut data = valid_insert();
        data.coupon_value = "1e308".into();
        data.quantity = Some("1e308".into());
        assert!(create_bond(&conn, &data).is_err());
        assert_eq!(
            conn.query_row("SELECT count(*) FROM bonds", [], |r| r.get::<_, i64>(0))
                .expect("count"),
            0
        );
    }

    #[test]
    fn blank_bond_numbers_are_rejected_without_mutating_existing_facts() {
        let conn = setup_test_db();
        let existing = create_bond(&conn, &valid_insert()).expect("existing bond");
        for blank in ["", "  ", " 1.25 "] {
            for field in ["quantity", "interest"] {
                let mut data = valid_insert();
                if field == "quantity" {
                    data.quantity = Some(blank.into());
                } else {
                    data.interest_rate = Some(blank.into());
                }
                assert!(
                    create_bond(&conn, &data).is_err(),
                    "create accepted {field}={blank:?}"
                );
                assert!(
                    update_bond(&conn, &existing.id, &data).is_err(),
                    "update accepted {field}={blank:?}"
                );
            }
        }
        let facts: (i64, String, String) = conn
            .query_row(
                "SELECT count(*), quantity, interest_rate FROM bonds",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .expect("facts");
        assert_eq!(facts, (1, "1".into(), "0".into()));
    }

    #[test]
    fn bond_maturity_is_a_bounded_utc_day() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.maturity_date = Some(1767225600 + 12345);
        assert_eq!(
            create_bond(&conn, &data).expect("create").maturity_date,
            Some(1767225600)
        );
        data.maturity_date = Some(i64::MAX);
        assert!(create_bond(&conn, &data).is_err());
    }

    #[test]
    fn a_bond_without_a_purchase_date_stays_undated() {
        let conn = setup_test_db();
        let bond = create_bond(&conn, &valid_insert()).expect("create");
        assert_eq!(bond.purchase_date, None);
        assert_eq!(
            get_bond(&conn, &bond.id)
                .expect("get")
                .expect("row")
                .purchase_date,
            None
        );
    }

    #[test]
    fn the_purchase_date_is_stored_as_its_utc_day() {
        let conn = setup_test_db();
        let day = 1_767_225_600; // 2026-01-01
        let mut data = valid_insert();
        data.purchase_date = Some(day + 12_345);
        let bond = create_bond(&conn, &data).expect("create");
        assert_eq!(bond.purchase_date, Some(day));
        let stored = get_bond(&conn, &bond.id).expect("get").expect("row");
        assert_eq!(stored.purchase_date, Some(day));
        assert_eq!(list_bonds(&conn).expect("list")[0].purchase_date, Some(day));
    }

    #[test]
    fn a_purchase_date_in_the_future_is_refused_and_nothing_is_written() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.purchase_date = Some(chrono::Utc::now().timestamp() + 3 * 86_400);
        assert!(create_bond(&conn, &data).is_err());
        assert!(list_bonds(&conn).expect("list").is_empty());

        let existing = create_bond(&conn, &valid_insert()).expect("existing");
        assert!(update_bond(&conn, &existing.id, &data).is_err());
        assert_eq!(
            get_bond(&conn, &existing.id)
                .expect("get")
                .expect("row")
                .purchase_date,
            None
        );
    }

    #[test]
    fn update_sets_and_clears_the_purchase_date() {
        let conn = setup_test_db();
        let bond = create_bond(&conn, &valid_insert()).expect("create");
        let day = 1_767_225_600;
        let mut data = valid_insert();
        data.purchase_date = Some(day);
        let updated = update_bond(&conn, &bond.id, &data).expect("set");
        assert_eq!(updated.purchase_date, Some(day));

        data.purchase_date = None;
        let cleared = update_bond(&conn, &bond.id, &data).expect("clear");
        assert_eq!(cleared.purchase_date, None);
    }

    #[test]
    fn update_and_delete_report_an_unknown_bond() {
        let conn = setup_test_db();
        assert!(matches!(
            update_bond(&conn, "missing", &valid_insert()),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            delete_bond(&conn, "missing"),
            Err(AppError::NotFound(_))
        ));
        let bond = create_bond(&conn, &valid_insert()).expect("create");
        delete_bond(&conn, &bond.id).expect("delete");
        assert!(get_bond(&conn, &bond.id).expect("get").is_none());
    }
}
