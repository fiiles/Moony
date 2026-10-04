//! The atomic import of one file as a batch. Implemented by work package B2.

use super::types::{ParsedFile, StockImportConfig, StockImportResult};
use crate::error::{AppError, Result};

/// Import the rows the preview would import, in one SQL transaction, as a
/// recorded batch (none when nothing is imported).
pub fn import(
    conn: &mut rusqlite::Connection,
    parsed: &ParsedFile,
    config: &StockImportConfig,
    file_name: &str,
) -> Result<StockImportResult> {
    let _ = (conn, parsed, config, file_name);
    Err(AppError::Internal(
        "stock import: import is not implemented yet".into(),
    ))
}
