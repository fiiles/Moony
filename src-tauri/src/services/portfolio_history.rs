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

    fn from_stored(text: &str) -> Self {
        if text == "live" {
            SnapshotSource::Live
        } else {
            SnapshotSource::Backfill
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

/// A set of classes of a stored row, for [`update_class_set`]. Real estate is one member: it
/// stands for both columns (combined and personal subset).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClassSet {
    pub savings: bool,
    pub investments: bool,
    pub crypto: bool,
    pub bonds: bool,
    pub real_estate: bool,
    pub loans: bool,
    pub other_assets: bool,
}

impl ClassSet {
    /// Every class.
    pub const ALL: ClassSet = ClassSet {
        savings: true,
        investments: true,
        crypto: true,
        bonds: true,
        real_estate: true,
        loans: true,
        other_assets: true,
    };
}

/// Rewrite the classes in `classes` on the existing row of `day_timestamp`'s day from
/// `breakdowns`; every other column (the other classes, `source`, the timestamp) is untouched.
/// One statement, so a day is never half-written. Returns the rows changed (0 when the day has
/// no row: a missing day is created by [`upsert_snapshot`], never here).
pub fn update_class_set(
    conn: &Connection,
    day_timestamp: i64,
    breakdowns: &ClassBreakdowns,
    classes: ClassSet,
) -> Result<usize> {
    use rusqlite::types::Value;

    let day_start = (day_timestamp / SECONDS_PER_DAY) * SECONDS_PER_DAY;
    let mut assignments: Vec<(&str, String)> = Vec::new();
    if classes.savings {
        assignments.push(("savings_by_currency", breakdown_json(&breakdowns.savings)));
    }
    if classes.investments {
        assignments.push((
            "investments_by_currency",
            breakdown_json(&breakdowns.investments),
        ));
    }
    if classes.crypto {
        assignments.push(("crypto_by_currency", breakdown_json(&breakdowns.crypto)));
    }
    if classes.bonds {
        assignments.push(("bonds_by_currency", breakdown_json(&breakdowns.bonds)));
    }
    if classes.real_estate {
        let combined = merged(
            &breakdowns.real_estate_personal,
            &breakdowns.real_estate_investment,
        );
        assignments.push(("real_estate_by_currency", breakdown_json(&combined)));
        assignments.push((
            "real_estate_personal_by_currency",
            breakdown_json(&breakdowns.real_estate_personal),
        ));
    }
    if classes.loans {
        assignments.push(("loans_by_currency", breakdown_json(&breakdowns.loans)));
    }
    if classes.other_assets {
        assignments.push((
            "other_assets_by_currency",
            breakdown_json(&breakdowns.other_assets),
        ));
    }
    if assignments.is_empty() {
        return Ok(0);
    }

    let set_clause = assignments
        .iter()
        .enumerate()
        .map(|(index, (column, _))| format!("{column} = ?{}", index + 1))
        .collect::<Vec<_>>()
        .join(", ");
    let from = assignments.len() + 1;
    let sql = format!(
        "UPDATE portfolio_metrics_history SET {set_clause}
         WHERE recorded_at >= ?{from} AND recorded_at < ?{}",
        from + 1
    );
    let mut values: Vec<Value> = assignments
        .into_iter()
        .map(|(_, json)| Value::Text(json))
        .collect();
    values.push(Value::Integer(day_start));
    values.push(Value::Integer(day_start + SECONDS_PER_DAY));
    Ok(conn.execute(&sql, rusqlite::params_from_iter(values))?)
}

/// Provenance of the stored row of every day in `[from_day, until_day_exclusive)` that has
/// one, keyed by the day's midnight. A day with several rows counts as `Live` when any of
/// them is.
pub fn day_sources(
    conn: &Connection,
    from_day: i64,
    until_day_exclusive: i64,
) -> Result<BTreeMap<i64, SnapshotSource>> {
    let mut stmt = conn.prepare(
        "SELECT (recorded_at / 86400) * 86400, source FROM portfolio_metrics_history
         WHERE recorded_at >= ?1 AND recorded_at < ?2",
    )?;
    let rows = stmt.query_map([from_day, until_day_exclusive], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
    })?;
    let mut sources: BTreeMap<i64, SnapshotSource> = BTreeMap::new();
    for row in rows {
        let (day, source) = row?;
        let source = SnapshotSource::from_stored(&source);
        let entry = sources.entry(day).or_insert(source);
        if source == SnapshotSource::Live {
            *entry = SnapshotSource::Live;
        }
    }
    Ok(sources)
}

/// Delete the reconstructed (`source = 'backfill'`) rows dated before `before`; authentic
/// live rows are never deleted. Returns the rows removed.
pub fn delete_backfill_before(conn: &Connection, before: i64) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM portfolio_metrics_history WHERE source = 'backfill' AND recorded_at < ?1",
        [before],
    )?)
}

/// One ticker's history rows and position changes, with cursors for a day-by-day walk
/// (`history_days`).
pub(crate) struct TickerSeries {
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
    pub(crate) fn value_before(&mut self, cutoff: i64) -> Option<(f64, &str)> {
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
pub(crate) fn load_ticker_series(
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

    /// A ticker sold on day 4 must not be carried forward from its last row for ever.
    /// AAPL (1 000 CZK) is held days 1-3 and sold on day 4; MSFT (500 CZK) is held
    /// throughout. Day 6 values 500, not 1 500 (`history_days` walks the series by day).
    #[test]
    fn a_ticker_series_drops_sold_positions() {
        let conn = setup_ticker_history_db();
        const D1: i64 = 1_700_006_400;
        let day = |n: i64| D1 + (n - 1) * SECONDS_PER_DAY;
        for n in 1..=6 {
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

        let mut series = load_ticker_series(
            &conn,
            "stock_value_history",
            "investment_transactions",
            day(7),
        )
        .expect("series");
        let mut value_on = |n: i64| -> f64 {
            series
                .values_mut()
                .filter_map(|ticker| ticker.value_before(day(n) + SECONDS_PER_DAY))
                .map(|(value, _)| value)
                .sum()
        };
        assert_eq!(value_on(3), 1_500.0);
        assert_eq!(value_on(4), 500.0);
        assert_eq!(value_on(6), 500.0);
    }

    fn full_breakdowns() -> ClassBreakdowns {
        ClassBreakdowns {
            savings: map(&[("CZK", 1.0)]),
            investments: map(&[("CZK", 2.0)]),
            crypto: map(&[("CZK", 3.0)]),
            bonds: map(&[("CZK", 4.0)]),
            real_estate_personal: map(&[("EUR", 5.0)]),
            real_estate_investment: map(&[("EUR", 6.0)]),
            loans: map(&[("CZK", 7.0)]),
            other_assets: map(&[("CZK", 8.0)]),
        }
    }

    #[test]
    fn update_class_set_rewrites_exactly_the_selected_classes() {
        let conn = setup_test_db();
        let recorded = ClassBreakdowns {
            savings: map(&[("CZK", 100.0)]),
            loans: map(&[("CZK", 700.0)]),
            bonds: map(&[("CZK", 400.0)]),
            real_estate_personal: map(&[("EUR", 50.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY + 4000, SnapshotSource::Live, &recorded).expect("live");

        let changed = update_class_set(
            &conn,
            TEST_DAY,
            &full_breakdowns(),
            ClassSet {
                savings: true,
                real_estate: true,
                other_assets: true,
                ..Default::default()
            },
        )
        .expect("update");
        assert_eq!(changed, 1);

        assert_eq!(
            column(&conn, TEST_DAY, "savings_by_currency"),
            map(&[("CZK", 1.0)])
        );
        assert_eq!(
            column(&conn, TEST_DAY, "other_assets_by_currency"),
            map(&[("CZK", 8.0)])
        );
        assert_eq!(
            column(&conn, TEST_DAY, "real_estate_by_currency"),
            map(&[("EUR", 11.0)]),
            "combined: personal plus investment"
        );
        assert_eq!(
            column(&conn, TEST_DAY, "real_estate_personal_by_currency"),
            map(&[("EUR", 5.0)])
        );
        // the unselected classes and the provenance stay
        assert_eq!(
            column(&conn, TEST_DAY, "loans_by_currency"),
            map(&[("CZK", 700.0)])
        );
        assert_eq!(
            column(&conn, TEST_DAY, "bonds_by_currency"),
            map(&[("CZK", 400.0)])
        );
        assert_eq!(provenance(&conn), (TEST_DAY + 4000, "live".to_string()));
    }

    #[test]
    fn update_class_set_without_a_row_or_a_class_changes_nothing() {
        let conn = setup_test_db();
        assert_eq!(
            update_class_set(&conn, TEST_DAY, &full_breakdowns(), ClassSet::ALL).expect("none"),
            0
        );
        upsert_snapshot(
            &conn,
            TEST_DAY,
            SnapshotSource::Backfill,
            &full_breakdowns(),
        )
        .expect("row");
        assert_eq!(
            update_class_set(
                &conn,
                TEST_DAY,
                &ClassBreakdowns::default(),
                ClassSet::default()
            )
            .expect("empty set"),
            0
        );
        assert_eq!(
            column(&conn, TEST_DAY, "loans_by_currency"),
            map(&[("CZK", 7.0)])
        );
    }

    #[test]
    fn day_sources_report_the_provenance_per_day_and_live_wins() {
        let conn = setup_test_db();
        let breakdowns = ClassBreakdowns::default();
        upsert_snapshot(&conn, TEST_DAY, SnapshotSource::Backfill, &breakdowns).expect("a");
        upsert_snapshot(
            &conn,
            TEST_DAY + DAY + 500,
            SnapshotSource::Live,
            &breakdowns,
        )
        .expect("b");
        conn.execute(
            "INSERT INTO portfolio_metrics_history (id, recorded_at, source)
             VALUES ('dup', ?1, 'backfill')",
            [TEST_DAY + DAY + 900],
        )
        .expect("a second row of the live day");
        upsert_snapshot(
            &conn,
            TEST_DAY + 5 * DAY,
            SnapshotSource::Backfill,
            &breakdowns,
        )
        .expect("outside");

        let sources = day_sources(&conn, TEST_DAY, TEST_DAY + 2 * DAY).expect("sources");
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[&TEST_DAY], SnapshotSource::Backfill);
        assert_eq!(sources[&(TEST_DAY + DAY)], SnapshotSource::Live);
    }

    #[test]
    fn delete_backfill_before_keeps_live_rows_and_later_days() {
        let conn = setup_test_db();
        let breakdowns = ClassBreakdowns::default();
        upsert_snapshot(&conn, TEST_DAY, SnapshotSource::Backfill, &breakdowns).expect("old");
        upsert_snapshot(&conn, TEST_DAY + DAY + 7, SnapshotSource::Live, &breakdowns)
            .expect("old live");
        upsert_snapshot(
            &conn,
            TEST_DAY + 3 * DAY,
            SnapshotSource::Backfill,
            &breakdowns,
        )
        .expect("kept");

        assert_eq!(
            delete_backfill_before(&conn, TEST_DAY + 3 * DAY).expect("delete"),
            1
        );

        let days: Vec<i64> = conn
            .prepare("SELECT recorded_at FROM portfolio_metrics_history ORDER BY recorded_at")
            .expect("prepare")
            .query_map([], |row| row.get(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("rows");
        assert_eq!(days, vec![TEST_DAY + DAY + 7, TEST_DAY + 3 * DAY]);
    }
}
