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
/// a CSV. The cap is enforced twice: on the size the file reports, and on what was actually
/// read (through `take`), so a file that grows while it is read, or a pipe, stays bounded.
fn read_statement_file(path: &str) -> Result<Option<Vec<u8>>> {
    use std::io::Read;
    let read_error = |e: std::io::Error| AppError::Internal(format!("Cannot read file: {e}"));
    let mut file = std::fs::File::open(path)
        .map_err(|e| AppError::Internal(format!("Cannot open file: {e}")))?;
    // One `read` may return fewer bytes than asked for (network mounts), so read until the
    // head is full or the file ends.
    let mut head = Vec::with_capacity(statement_import::DETECT_BYTES);
    file.by_ref()
        .take(statement_import::DETECT_BYTES as u64)
        .read_to_end(&mut head)
        .map_err(read_error)?;
    if statement_import::detect(&head)?.is_none() {
        return Ok(None);
    }
    let len = file.metadata().map_err(read_error)?.len();
    statement_import::check_size(len)?;
    let remaining = statement_import::MAX_FILE_BYTES + 1 - head.len() as u64;
    let mut bytes = head;
    file.take(remaining)
        .read_to_end(&mut bytes)
        .map_err(read_error)?;
    statement_import::check_size(bytes.len() as u64)?;
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

    const CAMT_HEAD: &[u8] = b"<?xml version=\"1.0\"?><Document xmlns=\"urn:iso:std:iso:20022:tech:xsd:camt.053.001.02\"><BkToCstmrStmt>";

    #[test]
    fn a_statement_exactly_at_the_cap_is_read_in_full() {
        let file = file_with(CAMT_HEAD, statement_import::MAX_FILE_BYTES);
        let bytes = read(&file).unwrap().expect("a statement");
        assert_eq!(bytes.len() as u64, statement_import::MAX_FILE_BYTES);
        assert!(bytes.starts_with(CAMT_HEAD));
    }

    /// A named pipe: its metadata length is 0 and what it delivers is up to the writer, like a
    /// file on a network mount that hands out short reads or a file that grows while it is read.
    #[cfg(unix)]
    fn fifo() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("statement.xml");
        let status = std::process::Command::new("mkfifo")
            .arg(&path)
            .status()
            .expect("mkfifo");
        assert!(status.success());
        (dir, path)
    }

    #[cfg(unix)]
    #[test]
    fn the_head_is_read_in_full_when_the_source_delivers_it_in_pieces() {
        let (_dir, path) = fifo();
        let (first, rest) = CAMT_HEAD.split_at(21);
        // The first piece alone does not look like a statement.
        assert_eq!(statement_import::detect(first).unwrap(), None);
        let writer_path = path.clone();
        let writer = std::thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new()
                .write(true)
                .open(writer_path)
                .expect("open pipe");
            pipe.write_all(first).expect("first piece");
            pipe.flush().expect("flush");
            std::thread::sleep(std::time::Duration::from_millis(150));
            pipe.write_all(rest).expect("second piece");
        });
        let bytes = read_statement_file(path.to_str().expect("utf-8 path")).unwrap();
        writer.join().expect("writer");
        assert_eq!(bytes.as_deref(), Some(CAMT_HEAD));
    }

    #[cfg(unix)]
    #[test]
    fn a_source_that_outgrows_the_cap_after_the_size_check_is_refused() {
        let (_dir, path) = fifo();
        let writer_path = path.clone();
        let writer = std::thread::spawn(move || {
            let mut pipe = std::fs::OpenOptions::new()
                .write(true)
                .open(writer_path)
                .expect("open pipe");
            let chunk = vec![b' '; 1 << 20];
            let mut sent = 0u64;
            if pipe.write_all(CAMT_HEAD).is_err() {
                return;
            }
            // The reader stops reading once it has refused the file; the write then fails.
            while sent < statement_import::MAX_FILE_BYTES + 2 * chunk.len() as u64 {
                if pipe.write_all(&chunk).is_err() {
                    return;
                }
                sent += chunk.len() as u64;
            }
        });
        // Not `unwrap_err`: a wrongly accepted file would be printed byte by byte.
        let result = read_statement_file(path.to_str().expect("utf-8 path"));
        let Err(error) = result else {
            panic!("the file must be refused");
        };
        assert_eq!(error.keyed().unwrap().0, "validation.statementTooLarge");
        writer.join().expect("writer");
    }

    #[test]
    fn a_missing_file_is_an_internal_error() {
        assert!(matches!(
            read_statement_file("/definitely/not/here.xml"),
            Err(AppError::Internal(_))
        ));
    }
}
