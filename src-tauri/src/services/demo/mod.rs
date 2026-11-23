//! Demo profile generator: a believable four-year financial life for the fictional Alex Meyer,
//! written into an already migrated database.
//!
//! Everything is relative to [`DemoOptions::now`]; nothing reads the wall clock, so the same
//! options always produce the same data. Spec: `docs/specs/2026-11-23-demo-profile-design.md`,
//! plan: `docs/plans/2026-11-23-demo-profile-data.md`.

pub mod clock;
pub mod prices;
pub mod rng;
pub mod text;

use crate::error::Result;
use rusqlite::Connection;
use std::collections::BTreeMap;

/// First name of the persona.
pub const PERSONA_NAME: &str = "Alex";
/// Surname of the persona.
pub const PERSONA_SURNAME: &str = "Meyer";
/// Email address of the persona (reserved example domain).
pub const PERSONA_EMAIL: &str = "alex.meyer@example.com";
/// Home currency of the persona.
pub const PERSONA_CURRENCY: &str = "EUR";

/// What to generate.
#[derive(Debug, Clone)]
pub struct DemoOptions {
    /// UI language code for all free text (en, cs, de, es, it, nb, pl, sv, ja, zh).
    pub language: String,
    /// Unix timestamp of "today"; every date is derived from it.
    pub now: i64,
    /// Attach documents (purchase contract, policy) to the encrypted attachments store.
    pub attachments: bool,
}

/// What a run created.
#[derive(Debug, Clone, Default)]
pub struct DemoSummary {
    /// Rows created per table or kind (filled in by the parts).
    pub counts: BTreeMap<&'static str, usize>,
    /// Net worth today in EUR.
    pub net_worth_eur: f64,
}

/// Fill a migrated database with the demo data set, in one SQL transaction.
pub fn seed(conn: &Connection, opts: &DemoOptions) -> Result<DemoSummary> {
    let _ = opts;
    let tx = conn.unchecked_transaction()?;
    let summary = DemoSummary::default();

    // Task 2: bank::seed(&tx, opts, &mut summary)?;      accounts, payments, balance records
    // Task 2: planning::seed(&tx, opts, &mut summary)?;  budgets, spending plans, cashflow items
    // Task 3: invest::seed(&tx, opts, &mut summary)?;    stocks, crypto, bond, watchlist
    // Task 3: assets::seed(&tx, opts, &mut summary)?;    property, loans, insurance, other assets
    // Task 4: history::seed(&tx, opts, &mut summary)?;   rates, daily values, markers

    tx.commit()?;
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_on_an_empty_plan_runs() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        let opts = DemoOptions {
            language: "en".into(),
            now: 1_795_000_000,
            attachments: false,
        };
        let summary = seed(&conn, &opts).expect("seed");
        assert!(summary.counts.is_empty());
    }
}
