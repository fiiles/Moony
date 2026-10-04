//! Inspection of a file: shape, source, suggested configuration.
//! Implemented by work package B1.

use super::types::{SavedStockImportFormat, StockCsvInspectOptions, StockCsvInspection};
use crate::error::{AppError, Result};

/// Inspect the bytes of a file. `saved` are the user's saved formats, matched
/// by `header_signature`; a forced `options.source` wins over detection.
pub fn inspect(
    bytes: &[u8],
    file_name: &str,
    options: &StockCsvInspectOptions,
    saved: &[SavedStockImportFormat],
) -> Result<StockCsvInspection> {
    let _ = (bytes, file_name, options, saved);
    Err(AppError::Internal(
        "stock import: inspect is not implemented yet".into(),
    ))
}
