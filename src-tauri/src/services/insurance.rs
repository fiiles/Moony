//! Insurance business logic. Extracted from commands/insurance.rs so the
//! Tauri command and the MCP write tool share one validated path (ADR 0007).
//!
//! This also unifies a known divergence: the command previously passed
//! `policy_number: None` straight through to a NOT NULL column (a latent
//! failure), while the MCP tool defaulted it to an empty string. The
//! empty-string default wins here since it already matched the UI form's
//! de-facto behavior.

use crate::error::{AppError, Result};
use crate::models::{InsertInsurancePolicy, InsurancePolicy};
use uuid::Uuid;

/// Create a new insurance policy. SINGLE SOURCE OF TRUTH for insurance
/// policy creation.
pub fn create_policy(
    conn: &rusqlite::Connection,
    data: &InsertInsurancePolicy,
) -> Result<InsurancePolicy> {
    data.validate()?;
    ensure_policy_identity_available(conn, data, None)?;

    let id = Uuid::new_v4().to_string();
    let now = chrono::Utc::now().timestamp();
    let policy_number = data.policy_number.as_deref().unwrap_or_default().trim();
    // The UI treats an omitted or empty optional premium as zero. Persist a
    // readable numeric fact because the cashflow report parses this field.
    let regular_payment = data
        .regular_payment
        .as_deref()
        .filter(|payment| !payment.is_empty())
        .unwrap_or("0");
    let regular_payment_currency = data
        .regular_payment_currency
        .clone()
        .unwrap_or_else(|| crate::constants::STORAGE_DEFAULT_CURRENCY.to_string());
    let status = data.status.as_deref().unwrap_or("active").to_lowercase();
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
            data.policy_type.to_lowercase(),
            data.provider.trim(),
            data.policy_name,
            policy_number,
            crate::models::validation::utc_day(data.start_date, "validation.invalidDate")?,
            data.end_date
                .map(|day| crate::models::validation::utc_day(day, "validation.invalidDate"))
                .transpose()?,
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

/// A stable nonblank provider + policy number identifies a policy across retries.
fn ensure_policy_identity_available(
    conn: &rusqlite::Connection,
    data: &InsertInsurancePolicy,
    excluded_id: Option<&str>,
) -> Result<()> {
    let number = data.policy_number.as_deref().unwrap_or_default().trim();
    if number.is_empty() {
        return Ok(());
    }
    let duplicate: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM insurance_policies WHERE LOWER(TRIM(provider)) = LOWER(?1) AND LOWER(TRIM(policy_number)) = LOWER(?2) AND (?3 IS NULL OR id != ?3))",
        rusqlite::params![data.provider.trim(), number, excluded_id], |r| r.get(0),
    )?;
    if duplicate {
        return Err(AppError::Validation(
            "validation.insurancePolicyAlreadyExists".into(),
        ));
    }
    Ok(())
}

/// Update and normalize a policy through the shared validated path.
pub fn update_policy(
    conn: &rusqlite::Connection,
    id: &str,
    data: &InsertInsurancePolicy,
) -> Result<InsurancePolicy> {
    // Validate inputs at the trust boundary
    data.validate()?;

    let now = chrono::Utc::now().timestamp();
    ensure_policy_identity_available(conn, data, Some(id))?;
    let limits_json = data
        .limits
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    let changed = conn.execute(
        "UPDATE insurance_policies SET
             type = ?1, provider = ?2, policy_name = ?3, policy_number = ?4,
             start_date = ?5, end_date = ?6, payment_frequency = ?7,
             one_time_payment = ?8, one_time_payment_currency = ?9,
             regular_payment = COALESCE(?10, regular_payment),
             regular_payment_currency = COALESCE(?11, regular_payment_currency),
             limits = COALESCE(?12, limits), notes = ?13, status = COALESCE(?14, status),
             updated_at = ?15 WHERE id = ?16",
        rusqlite::params![
            data.policy_type.to_lowercase(),
            data.provider.trim(),
            data.policy_name,
            data.policy_number.as_deref().unwrap_or_default().trim(),
            crate::models::validation::utc_day(data.start_date, "validation.invalidDate")?,
            data.end_date
                .map(|day| crate::models::validation::utc_day(day, "validation.invalidDate"))
                .transpose()?,
            data.payment_frequency,
            data.one_time_payment,
            data.one_time_payment_currency,
            data.regular_payment
                .as_ref()
                .map(|payment| if payment.is_empty() {
                    "0"
                } else {
                    payment.as_str()
                }),
            data.regular_payment_currency,
            limits_json,
            data.notes,
            data.status.as_ref().map(|status| status.to_lowercase()),
            now,
            id
        ],
    )?;

    if changed == 0 {
        return Err(AppError::NotFound("Insurance policy not found".into()));
    }
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
    .map_err(Into::into)
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
    #[test]
    fn audit_assets_insurance_status_is_canonical() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.status = Some("ACTIVE".into());
        let policy = create_policy(&conn, &data).unwrap();
        assert_eq!(policy.status, "active");
    }

    #[test]
    fn audit_assets_insurance_retry_does_not_duplicate_a_numbered_policy() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.policy_number = Some("ABC-123".into());
        create_policy(&conn, &data).unwrap();
        data.provider = " acme insurance ".into();
        data.policy_number = Some(" abc-123 ".into());
        assert!(create_policy(&conn, &data).is_err());
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM insurance_policies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn audit_assets_unnumbered_insurance_policies_can_be_distinct() {
        let conn = setup_test_db();
        create_policy(&conn, &valid_insert()).unwrap();
        create_policy(&conn, &valid_insert()).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM insurance_policies", [], |r| r.get(0))
            .unwrap();
        assert_eq!(count, 2);
    }

    #[test]
    fn audit_assets_policy_dates_are_bounded_and_canonical() {
        let conn = setup_test_db();
        let policy = create_policy(&conn, &valid_insert()).unwrap();
        assert_eq!(policy.start_date, 1_699_920_000);
        let mut invalid = valid_insert();
        invalid.start_date = i64::MAX;
        assert!(create_policy(&conn, &invalid).is_err());
    }

    #[test]
    fn audit_assets_policy_update_normalizes_and_protects_identity() {
        let conn = setup_test_db();
        let mut first = valid_insert();
        first.policy_number = Some("A123".into());
        let policy = create_policy(&conn, &first).unwrap();
        first.status = Some("INACTIVE".into());
        first.policy_type = "LIFE".into();
        first.policy_number = Some(" a123 ".into());
        let updated = update_policy(&conn, &policy.id, &first).unwrap();
        assert_eq!(updated.status, "inactive");
        assert_eq!(updated.policy_type, "life");
        assert_eq!(updated.policy_number.as_deref(), Some("a123"));
        let second = create_policy(&conn, &valid_insert()).unwrap();
        assert!(update_policy(&conn, &second.id, &first).is_err());
        let unchanged: String = conn
            .query_row(
                "SELECT policy_number FROM insurance_policies WHERE id=?1",
                [&second.id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(unchanged, "");
    }
    #[test]
    fn audit_assets_blank_optional_premium_stays_readable_on_create_and_update() {
        let conn = setup_test_db();
        let mut data = valid_insert();
        data.regular_payment = Some(String::new());
        let policy = create_policy(&conn, &data).unwrap();
        assert_eq!(policy.regular_payment, "0");
        assert_eq!(
            crate::services::reporting::parse_money(&policy.regular_payment).unwrap(),
            0.0
        );
        data.regular_payment = Some("50".into());
        update_policy(&conn, &policy.id, &data).unwrap();
        data.regular_payment = Some(String::new());
        let updated = update_policy(&conn, &policy.id, &data).unwrap();
        assert_eq!(updated.regular_payment, "0");
        data.regular_payment = Some("  ".into());
        assert!(update_policy(&conn, &policy.id, &data).is_err());
    }
}
