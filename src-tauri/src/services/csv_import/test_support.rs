//! Shared helpers for the CSV import tests (and the preset validity test in
//! `csv_presets`): in-memory schema, fixture access, and the zero-click path
//! "inspect a file, then import with exactly what the inspection suggested".

use rusqlite::Connection;

use super::inspect::{inspect_csv, InspectOptions};
use super::types::{CsvImportConfig, CsvImportResult, CsvPreviewResult};
use super::{decode_for_import, import_transactions};
use crate::services::csv_presets::ResolvedPreset;

/// `src-tauri/tests/fixtures/csv/`.
pub const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/csv");

pub fn fixture_bytes(name: &str) -> Vec<u8> {
    let path = format!("{FIXTURE_DIR}/{name}");
    std::fs::read(&path).unwrap_or_else(|e| panic!("cannot read fixture {path}: {e}"))
}

pub fn setup_import_db() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    conn.execute_batch(
        r#"
        CREATE TABLE bank_accounts (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            currency TEXT NOT NULL DEFAULT 'CZK',
            created_at INTEGER NOT NULL DEFAULT 0,
            updated_at INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE transaction_categories (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL
        );

        CREATE TABLE app_config (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE csv_import_batches (
            id TEXT PRIMARY KEY,
            bank_account_id TEXT NOT NULL REFERENCES bank_accounts(id) ON DELETE CASCADE,
            file_name TEXT NOT NULL,
            imported_count INTEGER NOT NULL DEFAULT 0,
            duplicate_count INTEGER NOT NULL DEFAULT 0,
            error_count INTEGER NOT NULL DEFAULT 0,
            imported_at INTEGER NOT NULL DEFAULT (unixepoch())
        );

        CREATE TABLE bank_transactions (
            id TEXT PRIMARY KEY,
            bank_account_id TEXT NOT NULL REFERENCES bank_accounts(id) ON DELETE CASCADE,
            transaction_id TEXT,
            tx_type TEXT NOT NULL,
            amount TEXT NOT NULL,
            currency TEXT NOT NULL,
            description TEXT,
            counterparty_name TEXT,
            counterparty_iban TEXT,
            booking_date INTEGER NOT NULL,
            value_date INTEGER,
            category_id TEXT REFERENCES transaction_categories(id),
            merchant_category_code TEXT,
            remittance_info TEXT,
            suggested_category_id TEXT REFERENCES transaction_categories(id),
            status TEXT NOT NULL DEFAULT 'booked',
            data_source TEXT NOT NULL DEFAULT 'manual',
            created_at INTEGER NOT NULL DEFAULT 0,
            import_batch_id TEXT REFERENCES csv_import_batches(id) ON DELETE CASCADE,
            categorization_source TEXT,
            UNIQUE(bank_account_id, transaction_id)
        );

        INSERT INTO bank_accounts (id, name, currency) VALUES ('acc-1', 'Main', 'CZK');
        INSERT INTO bank_accounts (id, name, currency) VALUES ('acc-eur', 'Euro', 'EUR');
        INSERT INTO transaction_categories (id, name) VALUES ('cat-1', 'Groceries');
        "#,
    )
    .expect("schema");
    conn
}

pub fn tx_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT count(*) FROM bank_transactions", [], |r| r.get(0))
        .expect("count")
}

/// What the import config would be if the user accepted every suggestion of
/// the inspection unchanged — the "≤ 3 clicks" path of the dialog.
pub fn config_from_inspection(r: &CsvPreviewResult) -> CsvImportConfig {
    let mapped = |role: &str| r.suggested_mappings.get(role).map(|(h, _)| h.clone());
    CsvImportConfig {
        delimiter: r.delimiter.clone(),
        header_row: Some(r.header_row),
        date_column: mapped("date").unwrap_or_default(),
        date_format: r.date_format.clone(),
        amount_column: mapped("amount"),
        debit_column: mapped("debit"),
        credit_column: mapped("credit"),
        balance_column: mapped("balance"),
        transaction_id_column: mapped("transactionId"),
        description_columns: Some(r.suggested_description_columns.clone()),
        counterparty_column: mapped("counterparty"),
        counterparty_iban_column: mapped("counterparty_iban"),
        currency_column: mapped("currency"),
        decimal_separator: Some(r.decimal_separator.clone()),
        row_filter: r.suggested_row_filter.clone(),
        ..Default::default()
    }
}

/// Inspect a fixture with every setting auto-detected.
pub fn inspect_fixture(name: &str) -> CsvPreviewResult {
    inspect_csv(&fixture_bytes(name), &InspectOptions::default())
        .unwrap_or_else(|e| panic!("inspect {name}: {e}"))
}

/// Import a fixture into `acc-1` with the inspection's own suggestions.
pub fn import_fixture_auto(conn: &mut Connection, name: &str) -> CsvImportResult {
    let bytes = fixture_bytes(name);
    let inspection = inspect_fixture(name);
    let config = config_from_inspection(&inspection);
    let decoded = decode_for_import(&bytes, None, None);
    import_transactions(conn, "acc-1", name, &decoded.text, &config, None)
        .unwrap_or_else(|e| panic!("import {name}: {e}"))
}

impl ResolvedPreset {
    /// Import config for a preset resolved against a file's headers.
    pub fn to_config(&self, delimiter: &str, date_format: &str) -> CsvImportConfig {
        CsvImportConfig {
            delimiter: delimiter.to_string(),
            date_column: self.date.clone(),
            date_format: date_format.to_string(),
            amount_column: self.amount.clone(),
            debit_column: self.debit.clone(),
            credit_column: self.credit.clone(),
            balance_column: self.balance.clone(),
            transaction_id_column: self.transaction_id.clone(),
            description_columns: Some(self.description.clone()),
            counterparty_column: self.counterparty.clone(),
            counterparty_iban_column: self.counterparty_iban.clone(),
            currency_column: self.currency.clone(),
            row_filter: self.row_filter.clone(),
            ..Default::default()
        }
    }
}
