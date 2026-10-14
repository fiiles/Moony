//! MCP tools: bonds. Query body moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use crate::error::{AppError, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use super::money::currency_or_main;
use super::sql_to_json;

pub fn bonds_list(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT id, name, isin, coupon_value, quantity, currency,
                interest_rate, maturity_date, created_at, updated_at, purchase_date
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
                "purchaseDate": sql_to_json(row.get::<_, rusqlite::types::Value>(10).unwrap_or(rusqlite::types::Value::Null)),
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
    // The service defaults to CZK for the UI path (ADR 0007); an MCP client
    // that leaves the currency out means the user's own currency.
    let mut data = data.clone();
    data.currency = Some(currency_or_main(conn, data.currency.as_deref())?);
    let bond = crate::services::bonds::create_bond(conn, &data)?;
    Ok(serde_json::to_value(bond)?)
}

/// The day the history changes from for a new bond (the MCP follow-up's earliest day): its
/// purchase day, `None` for an undated bond, which is carried on every day, so the history is
/// rebuilt from the first fact (a follow-up without a day).
pub fn earliest_date(data: &crate::models::InsertBond) -> Option<i64> {
    data.purchase_date
        .and_then(|day| crate::models::validation::utc_day(day, "validation.invalidDate").ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InsertBond;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
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

    fn valid_bond(name: &str) -> InsertBond {
        InsertBond {
            name: name.to_string(),
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

    #[test]
    fn create_defaults_a_missing_currency_to_the_main_currency() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO user_profile (currency) VALUES ('EUR')", [])
            .unwrap();

        let value = bond_create(&conn, &valid_bond("Euro bond")).unwrap();

        assert_eq!(value["currency"], "EUR");
    }

    #[test]
    fn create_keeps_an_explicit_currency() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO user_profile (currency) VALUES ('EUR')", [])
            .unwrap();
        let mut bond = valid_bond("Dollar bond");
        bond.currency = Some("USD".into());

        let value = bond_create(&conn, &bond).unwrap();

        assert_eq!(value["currency"], "USD");
    }

    #[test]
    fn create_stores_the_purchase_day_and_list_exposes_it() {
        let conn = setup_test_db();
        let day = 1_767_225_600; // 2026-01-01
        let mut dated = valid_bond("Dated bond");
        // the tool takes a Unix timestamp; the bond keeps the UTC day
        dated.purchase_date = Some(day + 3_600);
        let created = bond_create(&conn, &dated).unwrap();
        assert_eq!(created["purchaseDate"], day);
        bond_create(&conn, &valid_bond("Undated bond")).unwrap();

        let listed = bonds_list(&conn).unwrap();
        let by_name = |name: &str| {
            listed
                .as_array()
                .unwrap()
                .iter()
                .find(|bond| bond["name"] == name)
                .unwrap()
                .clone()
        };
        assert_eq!(by_name("Dated bond")["purchaseDate"], day);
        assert!(by_name("Undated bond")["purchaseDate"].is_null());
    }

    #[test]
    fn create_refuses_a_purchase_day_in_the_future_and_writes_nothing() {
        let conn = setup_test_db();
        let mut bond = valid_bond("Future bond");
        bond.purchase_date = Some(chrono::Utc::now().timestamp() + 3 * 86_400);
        let err = bond_create(&conn, &bond).unwrap_err();
        assert!(
            matches!(&err, AppError::Validation(key) if key == "validation.purchaseDateInFuture"),
            "{err:?}"
        );
        assert_eq!(bonds_list(&conn).unwrap().as_array().unwrap().len(), 0);
    }

    #[test]
    fn the_history_follow_up_starts_at_the_purchase_day_or_the_first_fact() {
        let mut bond = valid_bond("Bond");
        assert_eq!(earliest_date(&bond), None, "undated: from the first fact");
        bond.purchase_date = Some(1_767_225_600 + 3_600);
        assert_eq!(earliest_date(&bond), Some(1_767_225_600));
    }

    #[test]
    fn the_tool_advertises_an_optional_purchase_date_documented_for_the_client() {
        use rmcp::handler::server::tool::schema_for_input;
        let schema = schema_for_input::<InsertBond>().unwrap();
        let property = &schema["properties"]["purchaseDate"];
        assert!(property.is_object(), "purchaseDate is part of the input");
        assert!(
            property["description"]
                .as_str()
                .is_some_and(|text| text.contains("bought")),
            "the client reads what the day means: {property}"
        );
        let required = schema
            .get("required")
            .and_then(|required| required.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(!required.iter().any(|name| name == "purchaseDate"));

        // and the tool takes it by that name, the day as Unix seconds
        let args: InsertBond = serde_json::from_value(serde_json::json!({
            "name": "B", "couponValue": "1", "purchaseDate": 1_767_225_600
        }))
        .unwrap();
        assert_eq!(args.purchase_date, Some(1_767_225_600));
        let args: InsertBond =
            serde_json::from_value(serde_json::json!({"name": "B", "couponValue": "1"})).unwrap();
        assert_eq!(args.purchase_date, None);
    }
}
