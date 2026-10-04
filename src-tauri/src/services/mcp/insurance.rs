//! MCP tools: insurance. Query bodies moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use rmcp::schemars;
use rusqlite::Connection;
use serde::Deserialize;
use serde_json::Value;

use crate::error::Result;

use super::sql_to_json;

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct InsuranceDetailArgs {
    #[schemars(description = "The insurance policy ID")]
    pub id: String,
}

pub fn insurance_list(conn: &Connection) -> Result<Value> {
    let mut stmt = conn.prepare(
        "SELECT id, type, provider, policy_name, policy_number,
                start_date, end_date, payment_frequency,
                one_time_payment, one_time_payment_currency,
                regular_payment, regular_payment_currency,
                limits, notes, status, created_at, updated_at
         FROM insurance_policies ORDER BY policy_name",
    )?;
    let rows: Vec<Value> = stmt.query_map([], |row| {
        // Stored as a JSON TEXT column; parse it so the wire shape matches
        // InsurancePolicy.limits (a real array), not a double-encoded string.
        let limits: Vec<crate::models::InsuranceLimit> = row
            .get::<_, Option<String>>(12)?
            .as_deref()
            .map(|s| serde_json::from_str(s).unwrap_or_default())
            .unwrap_or_default();
        Ok(serde_json::json!({
        "id": row.get::<_, String>(0)?,
        "type": row.get::<_, String>(1)?,
        "provider": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
        "policyName": row.get::<_, String>(3)?,
        "policyNumber": sql_to_json(row.get::<_, rusqlite::types::Value>(4).unwrap_or(rusqlite::types::Value::Null)),
        "startDate": sql_to_json(row.get::<_, rusqlite::types::Value>(5).unwrap_or(rusqlite::types::Value::Null)),
        "endDate": sql_to_json(row.get::<_, rusqlite::types::Value>(6).unwrap_or(rusqlite::types::Value::Null)),
        "paymentFrequency": sql_to_json(row.get::<_, rusqlite::types::Value>(7).unwrap_or(rusqlite::types::Value::Null)),
        "oneTimePayment": sql_to_json(row.get::<_, rusqlite::types::Value>(8).unwrap_or(rusqlite::types::Value::Null)),
        "oneTimePaymentCurrency": sql_to_json(row.get::<_, rusqlite::types::Value>(9).unwrap_or(rusqlite::types::Value::Null)),
        "regularPayment": sql_to_json(row.get::<_, rusqlite::types::Value>(10).unwrap_or(rusqlite::types::Value::Null)),
        "regularPaymentCurrency": sql_to_json(row.get::<_, rusqlite::types::Value>(11).unwrap_or(rusqlite::types::Value::Null)),
        "limits": limits,
        "notes": sql_to_json(row.get::<_, rusqlite::types::Value>(13).unwrap_or(rusqlite::types::Value::Null)),
        "status": row.get::<_, String>(14)?,
        "createdAt": row.get::<_, i64>(15)?,
        "updatedAt": row.get::<_, i64>(16)?,
    }))})?.filter_map(|r| r.ok()).collect();
    Ok(Value::Array(rows))
}

pub fn insurance_detail(conn: &Connection, id: &str) -> Result<Value> {
    let id = id.to_string();
    let pol = conn.query_row(
        "SELECT id, type, provider, policy_name, policy_number, status, created_at, updated_at
         FROM insurance_policies WHERE id = ?",
        [&id],
        |row| Ok(serde_json::json!({
            "id": row.get::<_, String>(0)?,
            "type": row.get::<_, String>(1)?,
            "provider": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
            "policyName": row.get::<_, String>(3)?,
            "policyNumber": sql_to_json(row.get::<_, rusqlite::types::Value>(4).unwrap_or(rusqlite::types::Value::Null)),
            "status": row.get::<_, String>(5)?,
            "createdAt": row.get::<_, i64>(6)?,
            "updatedAt": row.get::<_, i64>(7)?,
        })),
    );
    match pol {
        Err(rusqlite::Error::QueryReturnedNoRows) => {
            Ok(serde_json::json!({ "error": format!("Insurance {} not found", id) }))
        }
        Err(e) => Err(e.into()),
        Ok(mut obj) => {
            let docs: Vec<Value> = {
                let mut s = conn.prepare(
                    "SELECT id, name, description, file_type, file_size, uploaded_at
                     FROM insurance_documents WHERE insurance_id = ? ORDER BY uploaded_at DESC",
                )?;
                let result = s.query_map([&id], |row| Ok(serde_json::json!({
                    "id": row.get::<_, String>(0)?,
                    "name": row.get::<_, String>(1)?,
                    "description": sql_to_json(row.get::<_, rusqlite::types::Value>(2).unwrap_or(rusqlite::types::Value::Null)),
                    "fileType": sql_to_json(row.get::<_, rusqlite::types::Value>(3).unwrap_or(rusqlite::types::Value::Null)),
                    "fileSize": sql_to_json(row.get::<_, rusqlite::types::Value>(4).unwrap_or(rusqlite::types::Value::Null)),
                    "uploadedAt": row.get::<_, i64>(5)?,
                })))?.filter_map(|r| r.ok()).collect();
                result
            };
            obj["documents"] = Value::Array(docs);
            Ok(obj)
        }
    }
}

pub fn insurance_create(
    conn: &Connection,
    body: crate::models::InsertInsurancePolicy,
) -> Result<Value> {
    let policy = crate::services::insurance::create_policy(conn, &body)?;
    Ok(serde_json::to_value(policy)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{InsertInsurancePolicy, InsuranceLimit};

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE insurance_policies (
                id TEXT PRIMARY KEY,
                type TEXT NOT NULL,
                provider TEXT NOT NULL,
                policy_name TEXT NOT NULL,
                policy_number TEXT NOT NULL,
                start_date INTEGER NOT NULL,
                end_date INTEGER,
                payment_frequency TEXT NOT NULL,
                one_time_payment TEXT,
                one_time_payment_currency TEXT DEFAULT 'CZK',
                regular_payment TEXT NOT NULL DEFAULT '0',
                regular_payment_currency TEXT NOT NULL DEFAULT 'CZK',
                limits TEXT DEFAULT '[]',
                notes TEXT,
                status TEXT NOT NULL DEFAULT 'active',
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn valid_insert() -> InsertInsurancePolicy {
        InsertInsurancePolicy {
            policy_type: "life".to_string(),
            provider: "Acme Insurance".to_string(),
            policy_name: "Family life cover".to_string(),
            policy_number: None,
            start_date: 1_700_000_000,
            end_date: None,
            payment_frequency: "monthly".to_string(),
            one_time_payment: None,
            one_time_payment_currency: None,
            regular_payment: None,
            regular_payment_currency: None,
            limits: Some(vec![InsuranceLimit {
                title: "Liability".to_string(),
                amount: 5_000_000.0,
                currency: "CZK".to_string(),
            }]),
            notes: None,
            status: None,
        }
    }

    #[test]
    fn list_emits_limits_as_real_json_array() {
        let conn = setup_test_db();
        let created = insurance_create(&conn, valid_insert()).unwrap();

        let rows = insurance_list(&conn).unwrap();
        let listed = &rows.as_array().unwrap()[0];

        assert!(
            listed["limits"].is_array(),
            "limits must be a JSON array, got: {}",
            listed["limits"]
        );
        assert_eq!(
            listed["limits"], created["limits"],
            "insurance_list and insurance_create must agree on the limits wire shape"
        );
        assert_eq!(listed["limits"][0]["title"], "Liability");
        assert_eq!(listed["limits"][0]["amount"], 5_000_000.0);
        assert_eq!(listed["limits"][0]["currency"], "CZK");
    }

    #[test]
    fn list_emits_empty_array_when_limits_column_is_null() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO insurance_policies
                (id, type, provider, policy_name, policy_number, start_date, payment_frequency, limits)
             VALUES ('p1', 'life', 'Acme', 'Legacy policy', '', 1700000000, 'monthly', NULL)",
            [],
        )
        .expect("insert legacy row");

        let rows = insurance_list(&conn).unwrap();
        assert_eq!(rows.as_array().unwrap()[0]["limits"], serde_json::json!([]));
    }
}
