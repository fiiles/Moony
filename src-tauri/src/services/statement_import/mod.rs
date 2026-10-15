//! Bank statement import (CAMT.053, MT940, OFX/QFX, QIF): every format parses to
//! the neutral [`Statement`] model, which [`to_rows::to_rows`] turns into the
//! [`ParsedImport`](crate::services::csv_import::ParsedImport) the shared CSV
//! import core already consumes (preview, duplicate check, write path).

pub mod account_match;
pub mod decimal;
pub(crate) mod decode;
pub(crate) mod detect;
pub mod merge;
pub mod model;
#[cfg(test)]
pub(crate) mod test_support;
pub mod to_rows;

pub use crate::services::csv_import::{DeclaredBalance, SourceCategoryMapping};
pub use account_match::{match_account, AccountMatch};
pub use decode::decode;
pub use detect::{check_size, detect, reject_doctype, DETECT_BYTES, MAX_FILE_BYTES};
pub use merge::{merge, BalanceCheck, MergedStatement};
pub use model::{join_description, utc_day, AccountRef, Entry, Statement, StatementFormat};
