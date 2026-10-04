//! Insurance business logic. Extracted from commands/insurance.rs so the
//! Tauri command and the MCP write tool share one validated path (ADR 0007).
//!
//! This also unifies a known divergence: the command previously passed
//! `policy_number: None` straight through to a NOT NULL column (a latent
//! failure), while the MCP tool defaulted it to an empty string. The
//! empty-string default wins here since it already matched the UI form's
//! de-facto behavior.

use crate::error::Result;
use crate::models::{InsertInsurancePolicy, InsurancePolicy};
use uuid::Uuid;

/// Create a new insurance policy. SINGLE SOURCE OF TRUTH for insurance
/// policy creation.
pub fn create_policy(
    conn: &rusqlite::Connection,
    data: &InsertInsurancePolicy,
) -> Result<InsurancePolicy> {
    data.validate()?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let policy_number = data.policy_number.clone().unwrap_or_default();
    let regular_payment = data
        .regular_payment
        .clone()
        .unwrap_or_else(|| "0".to_string());
    let regular_payment_currency = data
        .regular_payment_currency
        .clone()
        .unwrap_or_else(|| "CZK".to_string());
    let status = data.status.clone().unwrap_or_else(|| "active".to_string());
    let limits = data.limits.clone().unwrap_or_default();
    let limits_json = serde_json::to_string(&limits)?;

    conn.execute(
        "INSERT INTO insurance_policies
         (id, type, provider, policy_name, policy_number, start_date, end_date,
          payment_frequency, one_time_payment, one_time_payment_currency,
          regular_payment, regular_payment_currency, limits, notes, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?16)",
        rusqlite::params![
            id,
            data.policy_type,
            data.provider,
            data.policy_name,
            policy_number,
            data.start_date,
            data.end_date,
            data.payment_frequency,
            data.one_time_payment,
            data.one_time_payment_currency,
            regular_payment,
            regular_payment_currency,
            limits_json,
            data.notes,
            status,
            now
        ],
    )?;

    conn.query_row(
        "SELECT id, type, provider, policy_name, policy_number, start_date, end_date,
                payment_frequency, one_time_payment, one_time_payment_currency,
                regular_payment, regular_payment_currency, limits, notes, status,
                created_at, updated_at
         FROM insurance_policies WHERE id = ?1",
        [&id],
        |row| {
            let limits_json: String = row.get(12)?;
            Ok(InsurancePolicy {
                id: row.get(0)?,
                policy_type: row.get(1)?,
                provider: row.get(2)?,
                policy_name: row.get(3)?,
                policy_number: row.get(4)?,
                start_date: row.get(5)?,
                end_date: row.get(6)?,
                payment_frequency: row.get(7)?,
                one_time_payment: row.get(8)?,
                one_time_payment_currency: row.get(9)?,
                regular_payment: row.get(10)?,
                regular_payment_currency: row.get(11)?,
                limits: serde_json::from_str(&limits_json).unwrap_or_default(),
                notes: row.get(13)?,
                status: row.get(14)?,
                created_at: row.get(15)?,
                updated_at: row.get(16)?,
            })
        },
    )
    .map_err(|e| e.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

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
            limits: None,
            notes: None,
            status: None,
        }
    }

    #[test]
    fn create_policy_applies_mcp_defaults() {
        let conn = setup_test_db();
        let policy = create_policy(&conn, &valid_insert()).unwrap();

        assert_eq!(policy.regular_payment, "0");
        assert_eq!(policy.regular_payment_currency, "CZK");
        assert_eq!(policy.status, "active");
        assert!(policy.limits.is_empty());
    }

    #[test]
    fn create_policy_stores_missing_policy_number_as_empty_string() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.policy_number = None;

        let policy = create_policy(&conn, &data).unwrap();

        // Round-trip through the NOT NULL column: None becomes "", not NULL.
        assert_eq!(policy.policy_number, Some("".to_string()));

        let stored: String = conn
            .query_row(
                "SELECT policy_number FROM insurance_policies WHERE id = ?1",
                [&policy.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(stored, "");
    }

    #[test]
    fn create_policy_rejects_invalid_input() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.policy_type = "not-a-real-type".to_string();
        assert!(create_policy(&conn, &data).is_err());
    }
}
