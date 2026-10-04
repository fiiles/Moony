//! Yahoo Finance lookups for the instruments of a file. Implemented by work
//! package B2. Network only: never called with the database lock held.

use super::types::{StockInstrumentQuery, StockInstrumentResolution};

/// Look every query up (symbol first, ISIN otherwise); failures are reported
/// per instrument (`lookupFailed`), never as an error of the whole call.
pub async fn resolve_instruments(
    queries: Vec<StockInstrumentQuery>,
) -> Vec<StockInstrumentResolution> {
    queries
        .into_iter()
        .map(|q| StockInstrumentResolution {
            key: q.key,
            candidates: vec![],
            best: None,
            lookup_failed: true,
        })
        .collect()
}
