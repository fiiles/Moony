//! The user's own accounts as the categorization engine sees them.
//!
//! The own-account layer turns "counterparty IBAN = one of my accounts" into an
//! Internal Transfer. The engine holds the IBANs in memory, so they must be
//! re-read whenever the accounts table changes — loading them only at unlock
//! left every account created afterwards invisible: "Salary September"
//! paid to a fresh account's own IBAN stayed uncategorized. Normalization
//! (whitespace, case) is the engine's job, via
//! [`tokenizer::normalize_iban`](super::tokenizer::normalize_iban), on both sides.

use rusqlite::Connection;

use super::CategorizationEngine;
use crate::error::Result;

/// The raw IBANs stored on the user's bank accounts.
pub fn load_own_ibans(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT iban FROM bank_accounts WHERE iban IS NOT NULL AND TRIM(iban) != ''")?;
    let ibans = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(ibans)
}

/// Re-read the own-account IBANs into the engine. Call after unlock and after
/// every account create / update / delete. Returns how many accounts carry an IBAN.
pub fn refresh_own_ibans(conn: &Connection, engine: &CategorizationEngine) -> Result<usize> {
    let ibans = load_own_ibans(conn)?;
    let count = ibans.len();
    engine.set_own_ibans(ibans);
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::categorization::types::TransactionInput;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE bank_accounts (id TEXT PRIMARY KEY, name TEXT NOT NULL, iban TEXT);
            "#,
        )
        .expect("schema");
        conn
    }

    fn salary_to(iban: &str) -> TransactionInput {
        TransactionInput {
            id: "t1".into(),
            description: Some("Salary September".into()),
            counterparty: None,
            counterparty_iban: Some(iban.into()),
            amount: 3200.0,
            is_credit: true,
            bank_account_id: None,
        }
    }

    #[test]
    fn an_account_created_after_unlock_is_recognised_once_refreshed() {
        let conn = setup_test_db();
        let engine = CategorizationEngine::new_empty();

        // Unlock: no accounts yet.
        assert_eq!(refresh_own_ibans(&conn, &engine).unwrap(), 0);
        let tx = salary_to("de89370400440532013000");
        assert!(!engine.categorize(&tx).has_category());

        // The user adds an account, typing the IBAN with spaces.
        conn.execute(
            "INSERT INTO bank_accounts (id, name, iban) VALUES ('a1', 'Main', 'DE89 3704 0044 0532 0130 00')",
            [],
        )
        .unwrap();
        // Without a refresh the engine is stale...
        assert!(!engine.categorize(&tx).has_category());
        // ...with one, the own-account layer sees it.
        assert_eq!(refresh_own_ibans(&conn, &engine).unwrap(), 1);
        assert_eq!(
            engine.categorize(&tx).category_id(),
            Some("cat_internal_transfers")
        );
    }

    #[test]
    fn editing_or_deleting_an_account_drops_its_old_iban() {
        let conn = setup_test_db();
        let engine = CategorizationEngine::new_empty();
        conn.execute(
            "INSERT INTO bank_accounts (id, name, iban) VALUES ('a1', 'Main', 'DE89370400440532013000')",
            [],
        )
        .unwrap();
        refresh_own_ibans(&conn, &engine).unwrap();
        let tx = salary_to("DE89 3704 0044 0532 0130 00");
        assert!(engine.categorize(&tx).has_category());

        conn.execute(
            "UPDATE bank_accounts SET iban = 'CZ6508000000192000145399'",
            [],
        )
        .unwrap();
        refresh_own_ibans(&conn, &engine).unwrap();
        assert!(!engine.categorize(&tx).has_category());
        assert!(engine
            .categorize(&salary_to("CZ65 0800 0000 1920 0014 5399"))
            .has_category());

        conn.execute("DELETE FROM bank_accounts", []).unwrap();
        assert_eq!(refresh_own_ibans(&conn, &engine).unwrap(), 0);
        assert!(!engine
            .categorize(&salary_to("CZ6508000000192000145399"))
            .has_category());
    }

    #[test]
    fn accounts_without_an_iban_are_skipped() {
        let conn = setup_test_db();
        conn.execute_batch(
            "INSERT INTO bank_accounts (id, name, iban) VALUES ('a1', 'Cash', NULL), ('a2', 'Wallet', '  ');",
        )
        .unwrap();
        assert!(load_own_ibans(&conn).unwrap().is_empty());
    }
}
