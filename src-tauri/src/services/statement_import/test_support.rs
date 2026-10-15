//! Shared helpers for the statement import tests: the CSV import schema plus the
//! account identifier columns and the institutions the account match reads.

use rusqlite::Connection;

use crate::services::csv_import::test_support::setup_import_db;

/// [`setup_import_db`] (accounts `acc-1` and `acc-eur` without identifiers, category
/// `cat-1`) with `iban`, `bban` and `institution_id` on `bank_accounts` and an
/// `institutions` table.
pub fn setup_statement_db() -> Connection {
    let conn = setup_import_db();
    conn.execute_batch(
        r#"
        ALTER TABLE bank_accounts ADD COLUMN iban TEXT;
        ALTER TABLE bank_accounts ADD COLUMN bban TEXT;
        ALTER TABLE bank_accounts ADD COLUMN institution_id TEXT;
        CREATE TABLE institutions (id TEXT PRIMARY KEY, country TEXT);
        "#,
    )
    .expect("statement schema");
    conn
}
