//! Recorded imports and their undo. Implemented by work package B2.

use super::types::{StockImportBatch, StockImportUndoResult};
use crate::error::{AppError, Result};

/// Recorded imports, newest first.
pub fn list_batches(conn: &rusqlite::Connection) -> Result<Vec<StockImportBatch>> {
    let _ = conn;
    Err(AppError::Internal(
        "stock import: list_batches is not implemented yet".into(),
    ))
}

/// Delete a batch's transactions (and positions left without any) atomically.
pub fn undo_batch(
    conn: &mut rusqlite::Connection,
    batch_id: &str,
) -> Result<StockImportUndoResult> {
    let _ = (conn, batch_id);
    Err(AppError::Internal(
        "stock import: undo_batch is not implemented yet".into(),
    ))
}
