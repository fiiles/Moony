//! MCP tools: loans. Query body moved verbatim from the retired REST API
//! (services/local_api.rs) (ADR 0006).

use crate::error::{AppError, Result};
use rusqlite::{Connection, OptionalExtension};
use serde_json::Value;

use super::money::currency_or_main;

pub fn loans_list(conn: &Connection) -> Result<Value> {
    // Through the loans service so every loan carries the balance of today's
    // ledger walk and its mode; the shape is the one the tool always returned
    // plus `balanceMode`.
    let loans = crate::services::loans::list_loans(
        conn,
        crate::services::loan_amortization::today_utc_day(),
    )?;
    Ok(Value::Array(
        loans
            .into_iter()
            .map(|loan| {
                serde_json::json!({
                    "id": loan.id,
                    "name": loan.name,
                    "principal": loan.principal,
                    "outstandingBalance": loan.outstanding_balance,
                    "balanceMode": loan.balance_mode,
                    "balanceAnchorAmount": loan.balance_anchor_amount,
                    "balanceAnchorDate": loan.balance_anchor_date,
                    "currency": loan.currency,
                    "interestRate": loan.interest_rate,
                    "interestRateValidityDate": loan.interest_rate_validity_date,
                    "monthlyPayment": loan.monthly_payment,
                    "startDate": loan.start_date,
                    "endDate": loan.end_date,
                    "createdAt": loan.created_at,
                    "updatedAt": loan.updated_at,
                })
            })
            .collect(),
    ))
}

// ============================================================================
// Write tools (Task 10: single-create entity tool, dup check per spec D4)
// ============================================================================

pub fn loan_create(conn: &Connection, data: &crate::models::InsertLoan) -> Result<Value> {
    let dup: Option<String> = conn
        .query_row(
            "SELECT id FROM loans WHERE LOWER(name) = LOWER(?1)",
            [&data.name],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(id) = dup {
        return Err(AppError::Validation(format!(
            "A loan with this name already exists (id: {})",
            id
        )));
    }
    // The service defaults to CZK for the UI path (ADR 0007); an MCP client
    // that leaves the currency out means the user's own currency.
    let mut data = data.clone();
    data.currency = Some(currency_or_main(conn, data.currency.as_deref())?);
    let loan = crate::services::loans::create_loan(conn, &data)?;
    Ok(serde_json::to_value(loan)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::InsertLoan;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE user_profile (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                currency TEXT NOT NULL DEFAULT 'CZK'
            );
            CREATE TABLE loans (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                principal TEXT NOT NULL,
                currency TEXT NOT NULL DEFAULT 'CZK',
                interest_rate TEXT NOT NULL DEFAULT '0',
                interest_rate_validity_date INTEGER,
                monthly_payment TEXT NOT NULL DEFAULT '0',
                start_date INTEGER NOT NULL,
                end_date INTEGER,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
                balance_anchor_amount TEXT,
                balance_anchor_date INTEGER
            );
            CREATE TABLE loan_events (
                id TEXT PRIMARY KEY,
                loan_id TEXT NOT NULL REFERENCES loans(id) ON DELETE CASCADE,
                kind TEXT NOT NULL CHECK (kind IN ('extra_payment', 'rate_change', 'balance_check')),
                event_day INTEGER NOT NULL,
                amount TEXT,
                rate TEXT,
                monthly_payment TEXT,
                note TEXT,
                created_at INTEGER NOT NULL DEFAULT (unixepoch()),
                previous_rate TEXT,
                previous_monthly_payment TEXT
            );
            "#,
        )
        .expect("schema");
        conn
    }

    fn valid_loan(name: &str) -> InsertLoan {
        InsertLoan {
            name: name.to_string(),
            principal: "1000000".into(),
            currency: None,
            interest_rate: None,
            interest_rate_validity_date: None,
            monthly_payment: None,
            start_date: None,
            end_date: None,
            balance_anchor_amount: None,
            balance_anchor_date: None,
        }
    }

    #[test]
    fn create_happy_path() {
        let conn = setup_test_db();
        let value = loan_create(&conn, &valid_loan("Mortgage - main flat")).unwrap();
        assert_eq!(value["name"], "Mortgage - main flat");
        assert_eq!(value["currency"], "CZK");
    }

    #[test]
    fn create_defaults_a_missing_currency_to_the_main_currency() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO user_profile (currency) VALUES ('EUR')", [])
            .unwrap();

        let value = loan_create(&conn, &valid_loan("Euro loan")).unwrap();

        assert_eq!(value["currency"], "EUR");
    }

    #[test]
    fn create_keeps_an_explicit_currency() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO user_profile (currency) VALUES ('EUR')", [])
            .unwrap();
        let mut loan = valid_loan("Dollar loan");
        loan.currency = Some("USD".into());

        let value = loan_create(&conn, &loan).unwrap();

        assert_eq!(value["currency"], "USD");
    }

    #[test]
    fn list_exposes_the_interest_rate_validity_date() {
        let conn = setup_test_db();
        let mut loan = valid_loan("Mortgage - main flat");
        loan.interest_rate_validity_date = Some(1_800_000_000);
        loan_create(&conn, &loan).unwrap();

        let listed = loans_list(&conn).unwrap();
        let row = &listed[0];
        assert_eq!(
            row["interestRateValidityDate"],
            1_800_000_000i64.div_euclid(86_400) * 86_400
        );
    }

    #[test]
    fn list_exposes_the_amortized_balance_next_to_the_original_principal() {
        let conn = setup_test_db();
        let mut loan = valid_loan("Mortgage - main flat");
        loan.start_date = Some(1_577_836_800); // 2020-01-01
        loan.monthly_payment = Some("1000".into());
        loan_create(&conn, &loan).unwrap();

        let listed = loans_list(&conn).unwrap();
        let row = &listed[0];
        assert_eq!(row["principal"], "1000000");
        let outstanding: f64 = row["outstandingBalance"].as_str().unwrap().parse().unwrap();
        assert!(outstanding < 1_000_000.0, "payments since 2020 reduce it");
        assert!(row["balanceAnchorAmount"].is_null());
        assert!(row["balanceAnchorDate"].is_null());
        assert_eq!(row["balanceMode"], "estimated");
    }

    #[test]
    fn list_exposes_a_manual_balance_that_holds_and_its_mode() {
        let conn = setup_test_db();
        let mut loan = valid_loan("Mortgage - main flat");
        loan.start_date = Some(1_577_836_800); // 2020-01-01
        loan.monthly_payment = Some("1000".into());
        loan.balance_anchor_amount = Some("400000".into());
        loan.balance_anchor_date = Some(1_735_689_600); // 2025-01-01
        loan_create(&conn, &loan).unwrap();

        let listed = loans_list(&conn).unwrap();
        // A manual balance holds: the payments since it do not amortize it.
        assert_eq!(listed[0]["outstandingBalance"], "400000.00");
        assert_eq!(listed[0]["balanceMode"], "manual");
        // The anchor columns are the mirror of the newest record.
        assert_eq!(listed[0]["balanceAnchorAmount"], "400000");
        assert_eq!(listed[0]["balanceAnchorDate"], 1_735_689_600i64);
    }

    #[test]
    fn create_with_an_anchor_creates_a_manual_record() {
        let conn = setup_test_db();
        let mut loan = valid_loan("Mortgage - main flat");
        loan.start_date = Some(1_577_836_800); // 2020-01-01
        loan.balance_anchor_amount = Some("400000".into());
        loan.balance_anchor_date = Some(1_735_689_600); // 2025-01-01
        let created = loan_create(&conn, &loan).unwrap();

        let records: Vec<(String, i64, Option<String>)> = conn
            .prepare("SELECT kind, event_day, amount FROM loan_events WHERE loan_id = ?1")
            .unwrap()
            .query_map([created["id"].as_str().unwrap()], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert_eq!(
            records,
            vec![(
                "balance_check".to_string(),
                1_735_689_600i64,
                Some("400000".to_string())
            )]
        );
        assert_eq!(created["balanceMode"], "manual");
        assert_eq!(created["events"][0]["kind"], "balance_check");
    }

    #[test]
    fn list_reports_a_null_validity_date_as_null() {
        let conn = setup_test_db();
        loan_create(&conn, &valid_loan("Consumer loan")).unwrap();
        let listed = loans_list(&conn).unwrap();
        // Indexing returns Null for an absent key too, so assert on the key
        // itself — otherwise this test passes against the missing field.
        let field = listed[0]
            .get("interestRateValidityDate")
            .expect("key must be present even when unset");
        assert!(field.is_null(), "an unset validity date must be null");
    }

    #[test]
    fn create_rejects_duplicate_name_case_insensitive_and_reports_existing_id() {
        let conn = setup_test_db();
        let first = loan_create(&conn, &valid_loan("Mortgage - main flat")).unwrap();
        let existing_id = first["id"].as_str().unwrap().to_string();

        let err = loan_create(&conn, &valid_loan("mortgage - main flat")).unwrap_err();
        match err {
            AppError::Validation(msg) => assert!(
                msg.contains(&existing_id),
                "error should carry the existing loan id: {msg}"
            ),
            other => panic!("expected Validation error, got {other:?}"),
        }
    }
}
