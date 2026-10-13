//! Single writer for `portfolio_metrics_history`
//!
//! Invariants enforced here and nowhere else:
//! - I1: native per-currency breakdowns are the only stored record of a day (the CZK totals
//!   were retired by migration 012). Nothing is converted on write; readers convert per
//!   response (`services/net_worth.rs`).
//! - I2: real estate is stored as the combined breakdown (`real_estate_by_currency`) plus its
//!   personal subset (`real_estate_personal_by_currency`); investment real estate is the
//!   difference per currency ([`investment_real_estate`]).
//! - I4: provenance — `source = 'live'` rows are authentic records; a
//!   `'backfill'` write never overwrites a live row, while a live write
//!   overwrites anything for its day.

use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;
use uuid::Uuid;

use crate::error::Result;

pub const SECONDS_PER_DAY: i64 = 86_400;

/// Provenance of a snapshot row (see migration 002).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotSource {
    /// Recorded by the running app from live data — an authentic record.
    Live,
    /// Reconstructed after the fact (gap backfill, recalculation).
    Backfill,
}

impl SnapshotSource {
    fn as_str(self) -> &'static str {
        match self {
            SnapshotSource::Live => "live",
            SnapshotSource::Backfill => "backfill",
        }
    }
}

/// Native per-currency amounts for every asset class of one snapshot day.
/// Real estate is split; the stored `real_estate_by_currency` column is the
/// merge of the two maps and `real_estate_personal_by_currency` the personal one.
#[derive(Debug, Clone, Default)]
pub struct ClassBreakdowns {
    pub savings: HashMap<String, f64>,
    pub investments: HashMap<String, f64>,
    pub crypto: HashMap<String, f64>,
    pub bonds: HashMap<String, f64>,
    pub real_estate_personal: HashMap<String, f64>,
    pub real_estate_investment: HashMap<String, f64>,
    pub loans: HashMap<String, f64>,
    pub other_assets: HashMap<String, f64>,
}

/// Transaction-driven classes that a targeted recalculation may rewrite on an
/// existing row (loans/bonds/real estate have no transaction history).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetClassKind {
    Savings,
    Investments,
    Crypto,
    OtherAssets,
}

/// Serialize a per-currency breakdown map for a `*_by_currency` column.
pub fn breakdown_json(map: &HashMap<String, f64>) -> String {
    serde_json::to_string(map).unwrap_or_else(|_| "{}".to_string())
}

fn merged(a: &HashMap<String, f64>, b: &HashMap<String, f64>) -> HashMap<String, f64> {
    let mut out = a.clone();
    for (currency, amount) in b {
        *out.entry(currency.clone()).or_insert(0.0) += amount;
    }
    out
}

/// Below this a difference of two stored amounts is float residue, not money (far under any
/// currency's minor unit).
const RESIDUE_EPSILON: f64 = 1e-6;

/// Investment real estate of a stored row: the combined breakdown minus its personal subset,
/// per currency (currencies that net to zero, residue included, are left out).
pub fn investment_real_estate(
    combined: &HashMap<String, f64>,
    personal: &HashMap<String, f64>,
) -> HashMap<String, f64> {
    combined
        .iter()
        .map(|(currency, amount)| {
            let personal = personal.get(currency).copied().unwrap_or(0.0);
            (currency.clone(), amount - personal)
        })
        .filter(|(_, amount)| amount.abs() > RESIDUE_EPSILON)
        .collect()
}

/// Insert or update the snapshot row for `recorded_at`'s day.
///
/// - `Live`: overwrites whatever exists for the day; the row keeps the given
///   (non-midnight) timestamp and becomes `source = 'live'`.
/// - `Backfill`: creates the row at the day's midnight, updates an existing
///   backfill row, and leaves an existing live row untouched.
pub fn upsert_snapshot(
    conn: &Connection,
    recorded_at: i64,
    source: SnapshotSource,
    breakdowns: &ClassBreakdowns,
) -> Result<()> {
    let day_start = (recorded_at / SECONDS_PER_DAY) * SECONDS_PER_DAY;

    let real_estate_merged = merged(
        &breakdowns.real_estate_personal,
        &breakdowns.real_estate_investment,
    );

    let existing: Option<(String, String)> = conn
        .query_row(
            "SELECT id, source FROM portfolio_metrics_history
             WHERE recorded_at >= ?1 AND recorded_at < ?2 LIMIT 1",
            [day_start, day_start + SECONDS_PER_DAY],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .ok();

    let row_ts = match source {
        SnapshotSource::Live => recorded_at,
        SnapshotSource::Backfill => day_start,
    };

    let id = match existing {
        Some((_, existing_source))
            if source == SnapshotSource::Backfill && existing_source == "live" =>
        {
            // I4: reconstructions never replace authentic records.
            return Ok(());
        }
        Some((id, _)) => id,
        None => Uuid::new_v4().to_string(),
    };
    conn.execute(
        "INSERT INTO portfolio_metrics_history
         (id, recorded_at, investments_by_currency, crypto_by_currency, savings_by_currency,
          bonds_by_currency, real_estate_by_currency, real_estate_personal_by_currency,
          loans_by_currency, other_assets_by_currency, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         ON CONFLICT(id) DO UPDATE SET
             recorded_at = excluded.recorded_at,
             investments_by_currency = excluded.investments_by_currency,
             crypto_by_currency = excluded.crypto_by_currency,
             savings_by_currency = excluded.savings_by_currency,
             bonds_by_currency = excluded.bonds_by_currency,
             real_estate_by_currency = excluded.real_estate_by_currency,
             real_estate_personal_by_currency = excluded.real_estate_personal_by_currency,
             loans_by_currency = excluded.loans_by_currency,
             other_assets_by_currency = excluded.other_assets_by_currency,
             source = excluded.source",
        rusqlite::params![
            id,
            row_ts,
            breakdown_json(&breakdowns.investments),
            breakdown_json(&breakdowns.crypto),
            breakdown_json(&breakdowns.savings),
            breakdown_json(&breakdowns.bonds),
            breakdown_json(&real_estate_merged),
            breakdown_json(&breakdowns.real_estate_personal),
            breakdown_json(&breakdowns.loans),
            breakdown_json(&breakdowns.other_assets),
            source.as_str(),
        ],
    )?;
    Ok(())
}

/// Static-class breakdowns used when reconstructing a day (savings, bonds,
/// loans, other assets, real estate split). Investments/crypto are rebuilt
/// from per-ticker history instead and are not part of this.
#[derive(Debug, Clone, Default)]
pub struct StaticBreakdowns {
    pub savings: HashMap<String, f64>,
    pub bonds: HashMap<String, f64>,
    pub loans: HashMap<String, f64>,
    pub other_assets: HashMap<String, f64>,
    pub real_estate_personal: HashMap<String, f64>,
    pub real_estate_investment: HashMap<String, f64>,
}

/// Static classes for a reconstructed day, carried from the nearest live
/// snapshot (spec decision: a live record beats today's balances). The
/// nearest **earlier** live row wins; days that precede every live row carry
/// **backward** from the earliest one instead — the first authentic record is
/// still a better statics estimate than nothing (zeroed savings/loans would
/// cliff the net-worth chart at the live boundary). Every class, real estate
/// split included, is carried in its native currencies. Returns None only when
/// no live row exists at all (caller falls back to current table values).
pub fn carried_statics_from_nearest_live(
    conn: &Connection,
    before_ts: i64,
) -> Result<Option<StaticBreakdowns>> {
    use crate::services::currency::breakdown_from_json;

    const COLUMNS: &str = "savings_by_currency, bonds_by_currency, loans_by_currency,
                    other_assets_by_currency, real_estate_by_currency,
                    real_estate_personal_by_currency";

    type StaticsRow = (String, String, String, String, String, String);
    let fetch = |sql: String| -> Option<StaticsRow> {
        conn.query_row(&sql, [before_ts], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .ok()
    };

    let row = fetch(format!(
        "SELECT {COLUMNS} FROM portfolio_metrics_history
         WHERE recorded_at < ?1 AND source = 'live'
         ORDER BY recorded_at DESC LIMIT 1"
    ))
    .or_else(|| {
        fetch(format!(
            "SELECT {COLUMNS} FROM portfolio_metrics_history
             WHERE recorded_at >= ?1 AND source = 'live'
             ORDER BY recorded_at ASC LIMIT 1"
        ))
    });

    Ok(
        row.map(|(savings, bonds, loans, other, real_estate, personal)| {
            let real_estate = breakdown_from_json(&real_estate);
            let personal = breakdown_from_json(&personal);
            StaticBreakdowns {
                savings: breakdown_from_json(&savings),
                bonds: breakdown_from_json(&bonds),
                loans: breakdown_from_json(&loans),
                other_assets: breakdown_from_json(&other),
                real_estate_investment: investment_real_estate(&real_estate, &personal),
                real_estate_personal: personal,
            }
        }),
    )
}

/// Rewrite the breakdowns of selected transaction-driven classes on an
/// existing snapshot row. Rows the day does not have are left for the full
/// backfill to create; every other column (including `source`) is untouched.
pub fn update_classes(
    conn: &Connection,
    day_timestamp: i64,
    classes: &[(AssetClassKind, HashMap<String, f64>)],
) -> Result<()> {
    let day_start = (day_timestamp / SECONDS_PER_DAY) * SECONDS_PER_DAY;
    let day_end = day_start + SECONDS_PER_DAY;

    for (kind, breakdown) in classes {
        let breakdown_column = match kind {
            AssetClassKind::Savings => "savings_by_currency",
            AssetClassKind::Investments => "investments_by_currency",
            AssetClassKind::Crypto => "crypto_by_currency",
            AssetClassKind::OtherAssets => "other_assets_by_currency",
        };
        let sql = format!(
            "UPDATE portfolio_metrics_history SET {breakdown_column} = ?1
             WHERE recorded_at >= ?2 AND recorded_at < ?3"
        );
        conn.execute(
            &sql,
            rusqlite::params![breakdown_json(breakdown), day_start, day_end],
        )?;
    }

    Ok(())
}

/// Rebuild the stocks or crypto class of every snapshot day in
/// `[from_day, until_day_exclusive)` from the per-ticker value history.
///
/// For each day the latest row of every ticker up to that day is summed per
/// currency ("carry forward" over weekends and holidays) — but only for
/// tickers whose transactions leave a positive quantity on that day. Without
/// that filter a position sold on day D kept contributing its last row for
/// ever. The breakdown is written through `update_classes`.
///
/// `until_day_exclusive` is typically today's day start: today's row belongs
/// to the live snapshot and must not be replaced by a reconstruction.
pub fn aggregate_ticker_history(
    conn: &Connection,
    kind: AssetClassKind,
    from_day: i64,
    until_day_exclusive: i64,
) -> Result<()> {
    let from_day = (from_day / SECONDS_PER_DAY) * SECONDS_PER_DAY;
    let days: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT (recorded_at / 86400) * 86400 AS day FROM portfolio_metrics_history
             WHERE (recorded_at / 86400) * 86400 >= ?1 AND (recorded_at / 86400) * 86400 < ?2
             ORDER BY day",
        )?;
        let rows = stmt.query_map([from_day, until_day_exclusive], |row| row.get(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    aggregate_days(conn, kind, &days)
}

/// [`aggregate_ticker_history`] for the snapshot days in
/// `[from_day, until_day_exclusive)`, after creating the days the range has
/// no row for. Ticker-history writes only ever updated existing days, so on
/// a profile whose history starts today an import of older trades filled
/// `stock_value_history` but left the chart with a single point. A created
/// day carries its statics from the nearest live row (like the full
/// recalculation) and gets the other ticker class derived too, so it never
/// shows a hole where crypto (or stocks) was held.
pub fn rebuild_ticker_class(
    conn: &Connection,
    kind: AssetClassKind,
    from_day: i64,
    until_day_exclusive: i64,
) -> Result<()> {
    let other = match kind {
        AssetClassKind::Investments => AssetClassKind::Crypto,
        AssetClassKind::Crypto => AssetClassKind::Investments,
        AssetClassKind::Savings | AssetClassKind::OtherAssets => {
            return Err(crate::error::AppError::Internal(
                "rebuild_ticker_class supports stocks and crypto only".into(),
            ))
        }
    };
    let created = create_missing_days(conn, from_day, until_day_exclusive)?;
    aggregate_ticker_history(conn, kind, from_day, until_day_exclusive)?;
    aggregate_days(conn, other, &created)
}

/// Insert a backfill row for every day in `[from_day, until_day_exclusive)`
/// that has none; returns the created days, oldest first.
fn create_missing_days(
    conn: &Connection,
    from_day: i64,
    until_day_exclusive: i64,
) -> Result<Vec<i64>> {
    let from_day = (from_day / SECONDS_PER_DAY) * SECONDS_PER_DAY;
    let existing: std::collections::HashSet<i64> = {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT (recorded_at / 86400) * 86400 FROM portfolio_metrics_history
             WHERE recorded_at >= ?1 AND recorded_at < ?2",
        )?;
        let rows = stmt.query_map([from_day, until_day_exclusive], |row| row.get(0))?;
        rows.filter_map(|r| r.ok()).collect()
    };
    let mut created = Vec::new();
    let mut day = from_day;
    while day < until_day_exclusive {
        if !existing.contains(&day) {
            let statics = carried_statics_from_nearest_live(conn, day)?.unwrap_or_default();
            let breakdowns = ClassBreakdowns {
                savings: statics.savings,
                bonds: statics.bonds,
                loans: statics.loans,
                other_assets: statics.other_assets,
                real_estate_personal: statics.real_estate_personal,
                real_estate_investment: statics.real_estate_investment,
                ..ClassBreakdowns::default()
            };
            upsert_snapshot(conn, day, SnapshotSource::Backfill, &breakdowns)?;
            created.push(day);
        }
        day += SECONDS_PER_DAY;
    }
    Ok(created)
}

/// Re-derive the `kind` breakdown of the given snapshot days (ascending).
fn aggregate_days(conn: &Connection, kind: AssetClassKind, days: &[i64]) -> Result<()> {
    let (history_table, tx_table) = match kind {
        AssetClassKind::Investments => ("stock_value_history", "investment_transactions"),
        AssetClassKind::Crypto => ("crypto_value_history", "crypto_transactions"),
        AssetClassKind::Savings | AssetClassKind::OtherAssets => {
            return Err(crate::error::AppError::Internal(
                "aggregate_ticker_history supports stocks and crypto only".into(),
            ))
        }
    };
    let Some(&last_day) = days.last() else {
        return Ok(());
    };

    // One pass instead of one aggregate query per day: every ticker's
    // history rows and position changes are read once, then each day moves the
    // two per-ticker cursors forward. The old per-day query re-scanned every
    // history row before the day, 600 times over.
    let mut series = load_ticker_series(conn, history_table, tx_table, last_day + SECONDS_PER_DAY)?;

    for &day in days {
        let next_day = day + SECONDS_PER_DAY;
        // Native value per currency: latest row per ticker before `next_day`,
        // restricted to tickers still held on that day per the transactions.
        let mut breakdown: HashMap<String, f64> = HashMap::new();
        for ticker in series.values_mut() {
            if let Some((value, currency)) = ticker.value_before(next_day) {
                *breakdown.entry(currency.to_string()).or_insert(0.0) += value;
            }
        }
        breakdown.retain(|_, value| *value != 0.0);
        update_classes(conn, day, &[(kind, breakdown)])?;
    }

    Ok(())
}

/// One ticker's history rows and position changes, with cursors for
/// `aggregate_ticker_history`'s day-by-day walk.
struct TickerSeries {
    /// `(recorded_at, quantity * price, currency)`, oldest first.
    rows: Vec<(i64, f64, String)>,
    /// `(transaction_date, signed quantity)`, oldest first.
    changes: Vec<(i64, f64)>,
    rows_before: usize,
    changes_before: usize,
    position: f64,
}

impl TickerSeries {
    /// Advance both cursors to "strictly before `cutoff`" (cutoffs only grow)
    /// and return the latest row's native value and currency while the
    /// position is positive; `None` for a sold-out or never-bought ticker.
    fn value_before(&mut self, cutoff: i64) -> Option<(f64, &str)> {
        while self.rows_before < self.rows.len() && self.rows[self.rows_before].0 < cutoff {
            self.rows_before += 1;
        }
        while self.changes_before < self.changes.len()
            && self.changes[self.changes_before].0 < cutoff
        {
            self.position += self.changes[self.changes_before].1;
            self.changes_before += 1;
        }
        if self.position <= 0.000001 || self.rows_before == 0 {
            return None;
        }
        let (_, value, currency) = &self.rows[self.rows_before - 1];
        Some((*value, currency.as_str()))
    }
}

/// Every ticker's history rows and position changes dated before `before`
/// (changes: any date; they only matter up to the day being aggregated).
fn load_ticker_series(
    conn: &Connection,
    history_table: &str,
    tx_table: &str,
    before: i64,
) -> Result<BTreeMap<String, TickerSeries>> {
    let mut series: BTreeMap<String, TickerSeries> = BTreeMap::new();
    let new_series = || TickerSeries {
        rows: Vec::new(),
        changes: Vec::new(),
        rows_before: 0,
        changes_before: 0,
        position: 0.0,
    };

    let mut stmt = conn.prepare(&format!(
        "SELECT ticker, recorded_at, CAST(quantity AS REAL) * CAST(price AS REAL), currency
         FROM {history_table} WHERE recorded_at < ?1 ORDER BY ticker, recorded_at"
    ))?;
    let rows = stmt.query_map([before], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    for row in rows {
        let (ticker, recorded_at, value, currency) = row?;
        series
            .entry(ticker)
            .or_insert_with(new_series)
            .rows
            .push((recorded_at, value, currency));
    }

    let mut stmt = conn.prepare(&format!(
        "SELECT ticker, transaction_date,
                CASE WHEN type = 'buy' THEN CAST(quantity AS REAL)
                     ELSE -CAST(quantity AS REAL) END
         FROM {tx_table} WHERE transaction_date < ?1 ORDER BY ticker, transaction_date"
    ))?;
    let changes = stmt.query_map([before], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, f64>(2)?,
        ))
    })?;
    for change in changes {
        let (ticker, date, delta) = change?;
        // A ticker with transactions but no history rows contributes nothing
        // (there is no price to value it at), so it needs no series.
        if let Some(entry) = series.get_mut(&ticker) {
            entry.changes.push((date, delta));
        }
    }

    Ok(series)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::currency::breakdown_from_json;
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE portfolio_metrics_history (
                id TEXT PRIMARY KEY,
                recorded_at INTEGER NOT NULL,
                is_stale INTEGER NOT NULL DEFAULT 0,
                investments_by_currency TEXT NOT NULL DEFAULT '{}',
                crypto_by_currency TEXT NOT NULL DEFAULT '{}',
                savings_by_currency TEXT NOT NULL DEFAULT '{}',
                bonds_by_currency TEXT NOT NULL DEFAULT '{}',
                real_estate_by_currency TEXT NOT NULL DEFAULT '{}',
                loans_by_currency TEXT NOT NULL DEFAULT '{}',
                other_assets_by_currency TEXT NOT NULL DEFAULT '{}',
                source TEXT NOT NULL DEFAULT 'live',
                real_estate_personal_by_currency TEXT NOT NULL DEFAULT '{}'
            );
        "#,
        )
        .expect("schema");
        conn
    }

    const DAY: i64 = SECONDS_PER_DAY;
    const TEST_DAY: i64 = DAY * 20_000;

    fn map(entries: &[(&str, f64)]) -> HashMap<String, f64> {
        entries.iter().map(|(c, v)| (c.to_string(), *v)).collect()
    }

    /// A breakdown column of the row for `day`.
    fn column(conn: &Connection, day: i64, column: &str) -> HashMap<String, f64> {
        let json: String = conn
            .query_row(
                &format!(
                    "SELECT {column} FROM portfolio_metrics_history
                     WHERE recorded_at >= ?1 AND recorded_at < ?2"
                ),
                [day, day + DAY],
                |r| r.get(0),
            )
            .expect("breakdown column");
        breakdown_from_json(&json)
    }

    /// `(recorded_at, source)` of the only row.
    fn provenance(conn: &Connection) -> (i64, String) {
        conn.query_row(
            "SELECT recorded_at, source FROM portfolio_metrics_history",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("row")
    }

    #[test]
    fn backfill_insert_stores_the_native_breakdowns_at_midnight() {
        let conn = setup_test_db();
        let breakdowns = ClassBreakdowns {
            investments: map(&[("USD", 100.0)]),
            savings: map(&[("CZK", 500.0)]),
            ..Default::default()
        };
        upsert_snapshot(
            &conn,
            TEST_DAY + 3600,
            SnapshotSource::Backfill,
            &breakdowns,
        )
        .expect("upsert");

        assert_eq!(
            column(&conn, TEST_DAY, "investments_by_currency"),
            map(&[("USD", 100.0)])
        );
        assert_eq!(
            column(&conn, TEST_DAY, "savings_by_currency"),
            map(&[("CZK", 500.0)])
        );
        assert_eq!(
            provenance(&conn),
            (TEST_DAY, "backfill".to_string()),
            "backfill rows sit at midnight"
        );
    }

    #[test]
    fn backfill_never_overwrites_live_row() {
        let conn = setup_test_db();
        let live = ClassBreakdowns {
            investments: map(&[("CZK", 1111.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY + 4000, SnapshotSource::Live, &live).expect("live");

        let backfill = ClassBreakdowns {
            investments: map(&[("USD", 100.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY + 100, SnapshotSource::Backfill, &backfill)
            .expect("backfill");

        assert_eq!(
            column(&conn, TEST_DAY, "investments_by_currency"),
            map(&[("CZK", 1111.0)]),
            "live breakdowns must survive a backfill write"
        );
        assert_eq!(provenance(&conn), (TEST_DAY + 4000, "live".to_string()));
    }

    #[test]
    fn live_overwrites_backfill_row_and_claims_provenance() {
        let conn = setup_test_db();
        let backfill = ClassBreakdowns {
            investments: map(&[("USD", 100.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY, SnapshotSource::Backfill, &backfill).expect("backfill");

        let live = ClassBreakdowns {
            investments: map(&[("CZK", 3333.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY + 5555, SnapshotSource::Live, &live).expect("live");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM portfolio_metrics_history", [], |r| {
                r.get(0)
            })
            .expect("count");
        assert_eq!(count, 1, "same day must stay one row");
        assert_eq!(
            column(&conn, TEST_DAY, "investments_by_currency"),
            map(&[("CZK", 3333.0)])
        );
        assert_eq!(provenance(&conn), (TEST_DAY + 5555, "live".to_string()));
    }

    #[test]
    fn backfill_updates_existing_backfill_row() {
        let conn = setup_test_db();
        let first = ClassBreakdowns {
            investments: map(&[("USD", 100.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY, SnapshotSource::Backfill, &first).expect("first");

        let second = ClassBreakdowns {
            investments: map(&[("USD", 150.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY, SnapshotSource::Backfill, &second).expect("second");

        assert_eq!(
            column(&conn, TEST_DAY, "investments_by_currency"),
            map(&[("USD", 150.0)])
        );
        assert_eq!(provenance(&conn).1, "backfill");
    }

    #[test]
    fn real_estate_is_stored_combined_with_its_personal_subset() {
        let conn = setup_test_db();
        let breakdowns = ClassBreakdowns {
            real_estate_personal: map(&[("EUR", 100.0)]),
            real_estate_investment: map(&[("EUR", 50.0), ("CZK", 900.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY, SnapshotSource::Live, &breakdowns).expect("upsert");

        assert_eq!(
            column(&conn, TEST_DAY, "real_estate_by_currency"),
            map(&[("EUR", 150.0), ("CZK", 900.0)])
        );
        assert_eq!(
            column(&conn, TEST_DAY, "real_estate_personal_by_currency"),
            map(&[("EUR", 100.0)])
        );
    }

    #[test]
    fn investment_real_estate_is_the_combined_breakdown_minus_the_personal_one() {
        let investment = investment_real_estate(
            &map(&[("EUR", 300.0), ("CZK", 500.0)]),
            &map(&[("EUR", 100.0), ("CZK", 500.0)]),
        );
        assert_eq!(investment, map(&[("EUR", 200.0)]));

        // 0.1 + 0.2 - 0.3 is float residue, not an investment property
        let residue = investment_real_estate(&map(&[("EUR", 0.1 + 0.2)]), &map(&[("EUR", 0.3)]));
        assert!(residue.is_empty(), "{residue:?}");
    }

    #[test]
    fn update_classes_rewrites_only_the_given_breakdowns() {
        let conn = setup_test_db();
        let breakdowns = ClassBreakdowns {
            investments: map(&[("USD", 100.0)]),
            savings: map(&[("CZK", 500.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY, SnapshotSource::Backfill, &breakdowns).expect("upsert");

        update_classes(
            &conn,
            TEST_DAY,
            &[(AssetClassKind::Investments, map(&[("USD", 200.0)]))],
        )
        .expect("update classes");

        assert_eq!(
            column(&conn, TEST_DAY, "investments_by_currency"),
            map(&[("USD", 200.0)])
        );
        assert_eq!(
            column(&conn, TEST_DAY, "savings_by_currency"),
            map(&[("CZK", 500.0)]),
            "other classes untouched"
        );
    }

    #[test]
    fn carried_statics_come_from_nearest_earlier_live_row() {
        let conn = setup_test_db();

        // An older live row, a newer backfill row, and a live row after the
        // target date — only the older live row may be carried.
        let older_live = ClassBreakdowns {
            savings: map(&[("EUR", 40.0)]),
            bonds: map(&[("CZK", 100.0)]),
            real_estate_personal: map(&[("CZK", 900.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY + 500, SnapshotSource::Live, &older_live).expect("live");

        let newer_backfill = ClassBreakdowns {
            savings: map(&[("CZK", 7777.0)]),
            ..Default::default()
        };
        upsert_snapshot(
            &conn,
            TEST_DAY + DAY,
            SnapshotSource::Backfill,
            &newer_backfill,
        )
        .expect("backfill");

        let later_live = ClassBreakdowns {
            savings: map(&[("CZK", 9999.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY + 3 * DAY, SnapshotSource::Live, &later_live)
            .expect("later live");

        let statics = carried_statics_from_nearest_live(&conn, TEST_DAY + 2 * DAY)
            .expect("query")
            .expect("live row exists");
        assert_eq!(statics.savings, map(&[("EUR", 40.0)]));
        assert_eq!(statics.bonds, map(&[("CZK", 100.0)]));
        assert_eq!(statics.real_estate_personal, map(&[("CZK", 900.0)]));
        assert!(statics.real_estate_investment.is_empty());
        assert!(statics.loans.is_empty());
    }

    #[test]
    fn a_carried_day_keeps_a_foreign_property_in_its_own_currency() {
        let conn = setup_test_db();
        let live = ClassBreakdowns {
            real_estate_personal: map(&[("EUR", 300.0)]),
            real_estate_investment: map(&[("EUR", 100.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY + 500, SnapshotSource::Live, &live).expect("live");

        let statics = carried_statics_from_nearest_live(&conn, TEST_DAY + 2 * DAY)
            .expect("query")
            .expect("live row exists");
        assert_eq!(statics.real_estate_personal, map(&[("EUR", 300.0)]));
        assert_eq!(statics.real_estate_investment, map(&[("EUR", 100.0)]));

        // The reconstructed day stores the same native split: no synthetic CZK bucket.
        let carried = ClassBreakdowns {
            real_estate_personal: statics.real_estate_personal,
            real_estate_investment: statics.real_estate_investment,
            ..Default::default()
        };
        upsert_snapshot(
            &conn,
            TEST_DAY + 2 * DAY,
            SnapshotSource::Backfill,
            &carried,
        )
        .expect("backfill");
        let day = TEST_DAY + 2 * DAY;
        assert_eq!(
            column(&conn, day, "real_estate_by_currency"),
            map(&[("EUR", 400.0)])
        );
        assert_eq!(
            column(&conn, day, "real_estate_personal_by_currency"),
            map(&[("EUR", 300.0)])
        );
    }

    #[test]
    fn carried_statics_fall_back_to_the_earliest_later_live_row() {
        let conn = setup_test_db();
        // History starts with reconstructed days; the first live row comes
        // later. Days before it must carry from that first authentic record —
        // not from nothing (which would zero savings/loans and cliff the
        // net-worth chart at the live boundary).
        let first_live = ClassBreakdowns {
            savings: map(&[("CZK", 2500.0)]),
            loans: map(&[("CZK", 12000.0)]),
            ..Default::default()
        };
        upsert_snapshot(
            &conn,
            TEST_DAY + 5 * DAY + 700,
            SnapshotSource::Live,
            &first_live,
        )
        .expect("live");

        let statics = carried_statics_from_nearest_live(&conn, TEST_DAY)
            .expect("query")
            .expect("later live row exists");
        assert_eq!(statics.savings, map(&[("CZK", 2500.0)]));
        assert_eq!(statics.loans, map(&[("CZK", 12000.0)]));
    }

    #[test]
    fn carried_statics_none_without_any_live_row() {
        let conn = setup_test_db();
        let backfill = ClassBreakdowns {
            savings: map(&[("CZK", 1.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY, SnapshotSource::Backfill, &backfill).expect("backfill");
        assert!(carried_statics_from_nearest_live(&conn, TEST_DAY + DAY)
            .expect("query")
            .is_none());
    }

    fn setup_ticker_history_db() -> Connection {
        let conn = setup_test_db();
        conn.execute_batch(
            r#"
            CREATE TABLE stock_value_history (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL,
                quantity TEXT NOT NULL, price TEXT NOT NULL,
                currency TEXT NOT NULL, UNIQUE(ticker, recorded_at)
            );
            CREATE TABLE crypto_value_history (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL,
                quantity TEXT NOT NULL, price TEXT NOT NULL,
                currency TEXT NOT NULL, UNIQUE(ticker, recorded_at)
            );
            CREATE TABLE investment_transactions (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, type TEXT NOT NULL,
                quantity TEXT NOT NULL, transaction_date INTEGER NOT NULL
            );
            CREATE TABLE crypto_transactions (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, type TEXT NOT NULL,
                quantity TEXT NOT NULL, transaction_date INTEGER NOT NULL
            );
        "#,
        )
        .expect("ticker history schema");
        conn
    }

    /// A live day whose stocks and crypto hold a sentinel amount.
    fn insert_history_day(conn: &Connection, day: i64) {
        conn.execute(
            "INSERT INTO portfolio_metrics_history (id, recorded_at, investments_by_currency,
                crypto_by_currency, source)
             VALUES (?1, ?2, '{\"CZK\":999}', '{\"CZK\":999}', 'live')",
            rusqlite::params![format!("row-{day}"), day],
        )
        .expect("history row");
    }

    fn insert_ticker_row(
        conn: &Connection,
        table: &str,
        ticker: &str,
        day: i64,
        qty: f64,
        price: f64,
        ccy: &str,
    ) {
        conn.execute(
            &format!(
                "INSERT INTO {table} (id, ticker, recorded_at, quantity, price, currency)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)"
            ),
            rusqlite::params![
                format!("{ticker}-{day}"),
                ticker,
                day,
                qty.to_string(),
                price.to_string(),
                ccy
            ],
        )
        .expect("ticker row");
    }

    /// a ticker sold on day 4 must not be carried forward from its
    /// last row forever. AAPL (1 000 CZK) is held days 1–3 and sold on day 4;
    /// MSFT (500 CZK) is held throughout. Day 6 must hold 500, not 1 500.
    #[test]
    fn aggregate_ticker_history_drops_sold_positions() {
        let conn = setup_ticker_history_db();
        const D1: i64 = 1_700_006_400;
        let day = |n: i64| D1 + (n - 1) * SECONDS_PER_DAY;
        for n in 1..=6 {
            insert_history_day(&conn, day(n));
            insert_ticker_row(
                &conn,
                "stock_value_history",
                "MSFT",
                day(n),
                1.0,
                500.0,
                "CZK",
            );
        }
        for n in 1..=3 {
            insert_ticker_row(
                &conn,
                "stock_value_history",
                "AAPL",
                day(n),
                1.0,
                1000.0,
                "CZK",
            );
        }
        conn.execute_batch(&format!(
            "INSERT INTO investment_transactions (id, ticker, type, quantity, transaction_date) VALUES
                ('t1', 'AAPL', 'buy', '1', {}), ('t2', 'AAPL', 'sell', '1', {}),
                ('t3', 'MSFT', 'buy', '1', {});",
            day(1),
            day(4),
            day(1)
        ))
        .expect("transactions");

        aggregate_ticker_history(&conn, AssetClassKind::Investments, day(1), day(7))
            .expect("aggregate");

        let investments = |n: i64| column(&conn, day(n), "investments_by_currency");
        assert_eq!(investments(3), map(&[("CZK", 1_500.0)]));
        assert_eq!(investments(4), map(&[("CZK", 500.0)]));
        assert_eq!(investments(6), map(&[("CZK", 500.0)]));
    }

    /// The upper bound is exclusive so today's live snapshot is never
    /// overwritten by a reconstruction (the reconstruction half of the rule).
    #[test]
    fn aggregate_ticker_history_leaves_the_excluded_day_alone() {
        let conn = setup_ticker_history_db();
        const D1: i64 = 1_700_006_400;
        let d2 = D1 + SECONDS_PER_DAY;
        insert_history_day(&conn, D1);
        insert_history_day(&conn, d2);
        insert_ticker_row(&conn, "crypto_value_history", "BTC", D1, 2.0, 100.0, "CZK");
        insert_ticker_row(&conn, "crypto_value_history", "BTC", d2, 2.0, 100.0, "CZK");
        conn.execute(
            "INSERT INTO crypto_transactions (id, ticker, type, quantity, transaction_date) VALUES ('c1', 'BTC', 'buy', '2', ?1)",
            [D1],
        )
        .expect("tx");

        aggregate_ticker_history(&conn, AssetClassKind::Crypto, D1, d2).expect("aggregate");

        assert_eq!(
            column(&conn, D1, "crypto_by_currency"),
            map(&[("CZK", 200.0)])
        );
        assert_eq!(
            column(&conn, d2, "crypto_by_currency"),
            map(&[("CZK", 999.0)]),
            "excluded day keeps its live value"
        );
    }

    /// A fresh profile has only today's live row: importing older trades must
    /// still give the chart a day for every date since the first trade, with
    /// the statics carried from the live row and the other ticker class
    /// derived too (no hole where crypto was held).
    #[test]
    fn rebuild_ticker_class_creates_the_missing_days() {
        let conn = setup_ticker_history_db();
        const D1: i64 = 1_700_006_400;
        let day = |n: i64| D1 + (n - 1) * SECONDS_PER_DAY;
        // today's live row (day 5) holds savings
        conn.execute(
            "INSERT INTO portfolio_metrics_history (id, recorded_at, savings_by_currency,
                investments_by_currency, source)
             VALUES ('live', ?1, '{\"CZK\":70}', '{\"CZK\":999}', 'live')",
            [day(5) + 3600],
        )
        .expect("live row");
        for n in 1..=4 {
            insert_ticker_row(
                &conn,
                "stock_value_history",
                "MSFT",
                day(n),
                1.0,
                500.0,
                "CZK",
            );
        }
        insert_ticker_row(
            &conn,
            "crypto_value_history",
            "BTC",
            day(1),
            1.0,
            10.0,
            "CZK",
        );
        conn.execute_batch(&format!(
            "INSERT INTO investment_transactions (id, ticker, type, quantity, transaction_date)
                VALUES ('t1', 'MSFT', 'buy', '1', {d});
             INSERT INTO crypto_transactions (id, ticker, type, quantity, transaction_date)
                VALUES ('c1', 'BTC', 'buy', '1', {d});",
            d = day(1)
        ))
        .expect("transactions");

        rebuild_ticker_class(&conn, AssetClassKind::Investments, day(1), day(5)).expect("rebuild");

        for n in 1..=4 {
            assert_eq!(
                column(&conn, day(n), "investments_by_currency"),
                map(&[("CZK", 500.0)])
            );
            assert_eq!(
                column(&conn, day(n), "crypto_by_currency"),
                map(&[("CZK", 10.0)])
            );
            assert_eq!(
                column(&conn, day(n), "savings_by_currency"),
                map(&[("CZK", 70.0)])
            );
        }
        assert_eq!(
            column(&conn, day(5), "investments_by_currency"),
            map(&[("CZK", 999.0)]),
            "today's live row is untouched"
        );
    }

    /// Days that already exist keep their other class: only created days get it derived.
    #[test]
    fn rebuild_ticker_class_leaves_the_other_class_of_existing_days() {
        let conn = setup_ticker_history_db();
        const D1: i64 = 1_700_006_400;
        insert_history_day(&conn, D1);
        insert_ticker_row(&conn, "stock_value_history", "MSFT", D1, 1.0, 500.0, "CZK");
        conn.execute(
            "INSERT INTO investment_transactions (id, ticker, type, quantity, transaction_date)
                VALUES ('t1', 'MSFT', 'buy', '1', ?1)",
            [D1],
        )
        .expect("tx");

        rebuild_ticker_class(&conn, AssetClassKind::Investments, D1, D1 + SECONDS_PER_DAY)
            .expect("rebuild");

        assert_eq!(
            column(&conn, D1, "investments_by_currency"),
            map(&[("CZK", 500.0)])
        );
        assert_eq!(
            column(&conn, D1, "crypto_by_currency"),
            map(&[("CZK", 999.0)])
        );
    }

    #[test]
    fn update_classes_without_row_is_a_noop() {
        let conn = setup_test_db();
        update_classes(
            &conn,
            TEST_DAY,
            &[(AssetClassKind::Crypto, map(&[("USD", 1.0)]))],
        )
        .expect("no-op");
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM portfolio_metrics_history", [], |r| {
                r.get(0)
            })
            .expect("count");
        assert_eq!(count, 0);
    }
}
