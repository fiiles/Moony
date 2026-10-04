//! Stock CSV import (design: `docs/specs/2026-10-04-stock-csv-import-wizard-design.md`).
//!
//! A file is inspected (encoding, delimiter, header row, the source it comes
//! from, a suggested configuration), parsed with a [`StockImportConfig`]
//! (adapters for XTB, Trading 212, Degiro, Interactive Brokers and the Moony
//! table, or a hand mapping), previewed against the database (duplicates,
//! holdings, instruments), imported atomically as one batch through the shared
//! bulk writer, and can be undone. Custom mappings are remembered per header
//! layout.

pub mod adapters;
pub mod batches;
pub mod columns;
pub mod detect;
pub mod formats;
pub mod import;
pub mod inspect;
pub mod instruments;
pub mod parse;
pub mod preview;
pub mod simulate;
pub mod types;

pub use types::*;
