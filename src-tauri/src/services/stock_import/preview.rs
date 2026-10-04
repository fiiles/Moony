//! Dry run against the database. Implemented by work package B2.

use super::types::{ParsedFile, StockImportConfig, StockImportPreview};
use crate::error::{AppError, Result};

/// Instruments, statuses and counts of an import without writing anything.
pub fn preview(
    conn: &rusqlite::Connection,
    parsed: &ParsedFile,
    config: &StockImportConfig,
) -> Result<StockImportPreview> {
    let _ = (conn, parsed, config);
    Err(AppError::Internal(
        "stock import: preview is not implemented yet".into(),
    ))
}
