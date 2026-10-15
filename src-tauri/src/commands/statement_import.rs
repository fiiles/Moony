//! Statement import commands (design:
//! `docs/specs/2026-10-15-statement-import-formats-design.md`).
//!
//! Thin by design: read the file, delegate to `services::statement_import`, and run the
//! background FX backfill an import needs, as the CSV import commands do. The frontend
//! holds a file path and the options, as in the CSV wizard.

use tauri::State;

use crate::commands::categorization::CategorizationState;
use crate::commands::portfolio;
use crate::db::Database;
use crate::error::{AppError, Result};
use crate::services::csv_import::{CsvImportPreview, CsvImportResult};
use crate::services::statement_import::{self, StatementImportOptions, StatementInspection};

/// Reads a statement file. Detection needs only the head, so a file that is not a statement
/// is answered (`None`) without reading the rest and the statement size cap never applies to
/// a CSV.
fn read_statement_file(path: &str) -> Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut file = std::fs::File::open(path)
        .map_err(|e| AppError::Internal(format!("Cannot open file: {e}")))?;
    let mut head = vec![0u8; statement_import::DETECT_BYTES];
    let n = file
        .read(&mut head)
        .map_err(|e| AppError::Internal(format!("Cannot read file: {e}")))?;
    head.truncate(n);
    if statement_import::detect(&head)?.is_none() {
        return Ok(None);
    }
    let len = file
        .metadata()
        .map_err(|e| AppError::Internal(format!("Cannot read file: {e}")))?
        .len();
    statement_import::check_size(len)?;
    let mut bytes = head;
    file.read_to_end(&mut bytes)
        .map_err(|e| AppError::Internal(format!("Cannot read file: {e}")))?;
    Ok(Some(bytes))
}

/// A file the statement commands were asked to read but the detection does not recognize.
fn not_recognized() -> AppError {
    AppError::Validation("validation.statementNotRecognized".into())
}

/// Inspect a file: which statement format it is and what its statements hold. `None` = not
/// a statement (use the CSV path). `country` (the profile country) picks the legacy encoding
/// of the text formats.
#[tauri::command]
pub async fn inspect_statement_file(
    db: State<'_, Database>,
    account_id: String,
    file_path: String,
    country: Option<String>,
) -> Result<Option<StatementInspection>> {
    match read_statement_file(&file_path)? {
        None => Ok(None),
        Some(bytes) => db.with_conn(|conn| {
            statement_import::inspect(conn, &account_id, &bytes, country.as_deref())
        }),
    }
}

/// Dry-run importing one statement of the file: rows with their status, nothing is written.
#[tauri::command]
pub async fn preview_statement_import(
    db: State<'_, Database>,
    categorization: State<'_, CategorizationState>,
    account_id: String,
    file_path: String,
    country: Option<String>,
    options: StatementImportOptions,
) -> Result<CsvImportPreview> {
    let bytes = read_statement_file(&file_path)?.ok_or_else(not_recognized)?;
    // The preview shows the category each row would get, so the engine the import uses
    // resolves it here.
    let engine = categorization.0.clone();
    db.with_conn(|conn| {
        statement_import::preview(
            conn,
            &account_id,
            &bytes,
            country.as_deref(),
            &options,
            Some(&engine),
        )
    })
}

#[tauri::command]
pub async fn import_statement_file(
    app: tauri::AppHandle,
    db: State<'_, Database>,
    categorization: State<'_, CategorizationState>,
    account_id: String,
    file_path: String,
    country: Option<String>,
    options: StatementImportOptions,
) -> Result<CsvImportResult> {
    let bytes = read_statement_file(&file_path)?.ok_or_else(not_recognized)?;
    let file_name = std::path::Path::new(&file_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("statement")
        .to_string();
    let engine = categorization.0.clone();
    let result = db.with_conn_mut(|conn| {
        statement_import::import(
            conn,
            &account_id,
            &file_name,
            &bytes,
            country.as_deref(),
            &options,
            Some(&engine),
        )
    })?;
    // Statements may predate the FX timeseries (foreign-currency accounts): extend it so
    // reports convert those rows at historical rates.
    if result.imported_count > 0 {
        portfolio::spawn_fx_backfill(app, db.inner().clone());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn file_with(head: &[u8], len: u64) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("temp file");
        file.write_all(head).expect("write head");
        file.as_file().set_len(len).expect("extend");
        file
    }

    fn read(file: &tempfile::NamedTempFile) -> Result<Option<Vec<u8>>> {
        read_statement_file(file.path().to_str().expect("utf-8 path"))
    }

    #[test]
    fn a_statement_is_read_whole() {
        let xml = std::fs::read(format!(
            "{}/tests/fixtures/statements/camt053/basic.xml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let file = file_with(&xml, xml.len() as u64);
        assert_eq!(read(&file).unwrap(), Some(xml));
    }

    #[test]
    fn a_large_csv_is_answered_without_the_size_cap() {
        let csv = b"Date,Amount\n2026-01-01,5\n";
        let file = file_with(csv, statement_import::MAX_FILE_BYTES + 1);
        assert_eq!(read(&file).unwrap(), None);
    }

    #[test]
    fn an_oversized_statement_is_refused_before_it_is_read() {
        let head = b"<?xml version=\"1.0\"?><Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.053.001.02\"><BkToCstmrStmt>";
        let file = file_with(head, statement_import::MAX_FILE_BYTES + 1);
        let error = read(&file).unwrap_err();
        assert_eq!(error.keyed().unwrap().0, "validation.statementTooLarge");
    }

    #[test]
    fn a_missing_file_is_an_internal_error() {
        assert!(matches!(
            read_statement_file("/definitely/not/here.xml"),
            Err(AppError::Internal(_))
        ));
    }
}
