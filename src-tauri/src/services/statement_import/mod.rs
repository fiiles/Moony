//! Bank statement import (CAMT.053, MT940, OFX/QFX, QIF): every format parses to
//! the neutral [`Statement`] model, which [`to_rows::to_rows`] turns into the
//! [`ParsedImport`](crate::services::csv_import::ParsedImport) the shared CSV
//! import core already consumes (preview, duplicate check, write path).

pub mod decimal;
pub mod model;
pub mod to_rows;

pub use crate::services::csv_import::{DeclaredBalance, SourceCategoryMapping};
pub use model::{join_description, utc_day, AccountRef, Entry, Statement, StatementFormat};
