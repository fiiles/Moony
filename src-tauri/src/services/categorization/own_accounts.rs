//! The user's own accounts as the categorization engine sees them.
//!
//! The own-account layer turns "counterparty IBAN = one of my accounts" into an
//! Internal Transfer. The engine holds the IBANs in memory, so they must be
//! re-read whenever the accounts table changes — loading them only at unlock
//! left every account created afterwards invisible: "Salary September"
//! paid to a fresh account's own IBAN stayed uncategorized. Normalization uses the shared financial semantics utility on both sides,
//! including Czech domestic/IBAN equivalence. Domestic numbers of 11 or more digits (a UK sort
//! code and account, a US routing number and account) match too. The legacy function names remain
//! compatible with unlock/account refresh callers.

use rusqlite::Connection;

use super::CategorizationEngine;
use crate::error::Result;

/// The raw IBANs and domestic identifiers stored on the user's bank accounts.
pub fn load_own_ibans(conn: &Connection) -> Result<Vec<String>> {
    crate::services::financial_semantics::load_account_identifiers(conn)
}

/// Re-read the own-account IBANs into the engine. Call after unlock and after
/// every account create / update / delete. Returns how many nonempty identifiers were loaded.
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
            CREATE TABLE bank_accounts (id TEXT PRIMARY KEY, name TEXT NOT NULL, iban TEXT, bban TEXT,institution_id TEXT); CREATE TABLE institutions(id TEXT,country TEXT);
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
            counterparty_bank_code: None,
            counterparty_country: None,
            amount: 3200.0,
            is_credit: true,
            bank_account_id: None,
        }
    }

    #[test]
    fn integrity_own_foreign_iban_is_authoritative_over_paired_domestic_shape() {
        let conn = setup_test_db();
        conn.execute("INSERT INTO bank_accounts(id,name,iban,bban) VALUES ('foreign','Foreign','DE89370400440532013000','19-2000145399/0800')",[]).unwrap();
        let own = crate::services::financial_semantics::AccountIdentities::load(&conn).unwrap();
        assert!(!own.contains(Some("CZ6508000000192000145399"), None));
        let engine = CategorizationEngine::new_empty();
        refresh_own_ibans(&conn, &engine).unwrap();
        assert!(!engine
            .categorize(&salary_to("19-2000145399/0800"))
            .has_category());
        assert!(engine
            .categorize(&salary_to("DE89370400440532013000"))
            .has_category());
        conn.execute("UPDATE bank_accounts SET iban=NULL WHERE id='foreign'", [])
            .unwrap();
        refresh_own_ibans(&conn, &engine).unwrap();
        assert!(engine
            .categorize(&salary_to("CZ6508000000192000145399"))
            .has_category());
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

    #[test]
    fn a_uk_account_number_counterparty_is_recognised_as_an_own_account() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO bank_accounts(id, name, bban) VALUES ('uk', 'Current', '12-34-56 12345678')",
            [],
        )
        .unwrap();
        let engine = CategorizationEngine::new_empty();
        assert_eq!(refresh_own_ibans(&conn, &engine).unwrap(), 1);

        let mut tx = salary_to("12345678");
        tx.counterparty_bank_code = Some("123456".into());
        assert_eq!(
            engine.categorize(&tx).category_id(),
            Some("cat_internal_transfers")
        );
        // Another account at the same bank is a stranger.
        tx.counterparty_iban = Some("87654321".into());
        assert!(!engine.categorize(&tx).has_category());
    }
}
