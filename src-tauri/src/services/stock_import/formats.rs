//! Custom mappings remembered per header layout. Implemented by work package B2.

use super::types::{SavedStockImportFormat, StockImportConfig};
use crate::error::{AppError, Result};

pub fn list_formats(conn: &rusqlite::Connection) -> Result<Vec<SavedStockImportFormat>> {
    let _ = conn;
    Err(AppError::Internal(
        "stock import: list_formats is not implemented yet".into(),
    ))
}

pub fn save_format(
    conn: &rusqlite::Connection,
    name: &str,
    headers: &[String],
    config: &StockImportConfig,
) -> Result<SavedStockImportFormat> {
    let _ = (conn, name, headers, config);
    Err(AppError::Internal(
        "stock import: save_format is not implemented yet".into(),
    ))
}

pub fn delete_format(conn: &rusqlite::Connection, id: &str) -> Result<()> {
    let _ = (conn, id);
    Err(AppError::Internal(
        "stock import: delete_format is not implemented yet".into(),
    ))
}
