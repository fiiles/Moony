//! MCP tools: bonds. Query body moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use crate::error::{AppError, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use super::sql_to_json;

pub fn bonds_list(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT id, name, isin, coupon_value, quantity, currency,
                interest_rate, maturity_date, created_at, updated_at
         FROM bonds ORDER BY name",
    )?;
    let rows: Vec<Value> = stmt
        .query_map([], |row| {
            Ok(serde_json::json!({
                "id": row.get::<_, String>(0)?,
                "name": row.get::<_, String>(1)?,
                "isin": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
                "couponValue": row.get::<_, String>(3)?,
                "quantity": row.get::<_, String>(4)?,
                "currency": row.get::<_, String>(5)?,
                "interestRate": sql_to_json(row.get::<_, rusqlite::types::Value>(6).unwrap_or(rusqlite::types::Value::Null)),
                "maturityDate": sql_to_json(row.get::<_, rusqlite::types::Value>(7).unwrap_or(rusqlite::types::Value::Null)),
                "createdAt": row.get::<_, i64>(8)?,
                "updatedAt": row.get::<_, i64>(9)?,
            }))
        })?
        .filter_map(|r| r.ok())
        .collect();
    Ok(Value::Array(rows))
}

// ============================================================================
// Write tools (Task 10: single-create entity tool, dup check per spec D4)
// ============================================================================

pub fn bond_create(conn: &Connection, data: &crate::models::InsertBond) -> Result<Value> {
    let dup: Option<String> = conn
        .query_row(
            "SELECT id FROM bonds WHERE LOWER(name) = LOWER(?1)",
            [&data.name],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = dup {
        return Err(AppError::Validation(format!(
            "A bond with this name already exists (id: {})",
            id
        )));
    }
    let bond = crate::services::bonds::create_bond(conn, data)?;
    Ok(serde_json::to_value(bond)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InsertBond;

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
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn valid_bond(name: &str) -> InsertBond {
        InsertBond {
            name: name.to_string(),
            isin: None,
            coupon_value: "1000".into(),
            quantity: None,
            currency: None,
            interest_rate: None,
            maturity_date: None,
        }
    }

    #[test]
    fn create_happy_path() {
        let conn = setup_test_db();
        let value = bond_create(&conn, &valid_bond("Czech Gov Bond 2030")).unwrap();
        assert_eq!(value["name"], "Czech Gov Bond 2030");
        assert_eq!(value["currency"], "CZK");
    }

    #[test]
    fn create_rejects_duplicate_name_case_insensitive_and_reports_existing_id() {
        let conn = setup_test_db();
        let first = bond_create(&conn, &valid_bond("Czech Gov Bond 2030")).unwrap();
        let existing_id = first["id"].as_str().unwrap().to_string();

        let err = bond_create(&conn, &valid_bond("czech gov bond 2030")).unwrap_err();
        match err {
            AppError::Validation(msg) => assert!(
                msg.contains(&existing_id),
                "error should carry the existing bond id: {msg}"
            ),
            other => panic!("expected Validation error, got {other:?}"),
        }
    }
}
