//! Reading every row of a file with a configuration. Implemented by work
//! package B1.

use super::types::{ParsedFile, StockImportConfig};
use crate::error::{AppError, Result};

/// Read every data row: each ends up as a trade, a skipped row or an error.
/// The config must have passed `validate()`.
pub fn parse_file(bytes: &[u8], config: &StockImportConfig) -> Result<ParsedFile> {
    let _ = (bytes, config);
    Err(AppError::Internal(
        "stock import: parse_file is not implemented yet".into(),
    ))
}
