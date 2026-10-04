//! Single writer for `portfolio_metrics_history`
//!
//! Invariants enforced here and nowhere else:
//! - I1: native per-currency breakdowns are the canonical daily record.
//! - I3: CZK totals are a derived cache — computed from the breakdowns at the
//!   row's rates by this module only. No other code writes history columns.
//! - I4: provenance — `source = 'live'` rows are authentic records; a
//!   `'backfill'` write never overwrites a live row, while a live write
//!   overwrites anything for its day.
//!
//! Rate semantics: live rows convert at the current in-memory rates (identical
//! to what `save_rates_to_history_db` stores for today, so the timeseries and
//! the cache agree); backfill rows convert at their day's rates from the
//! timeseries (`get_rates_for_date`, ≤10-day walk-back, current-rates fallback
//! when the timeseries predates the day).

use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;
use uuid::Uuid;

use crate::error::Result;
use crate::models::PortfolioMetricsHistory;
use crate::services::currency::{convert_to_czk_with_rates, get_all_rates, get_rates_for_date};

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
/// Real estate is split so both stored totals can be derived; the stored
/// `real_estate_by_currency` column is the merge of the two maps.
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

fn czk_total(breakdown: &HashMap<String, f64>, rates: &HashMap<String, f64>) -> f64 {
    breakdown
        .iter()
        .map(|(currency, amount)| convert_to_czk_with_rates(*amount, currency, rates))
        .sum()
}

fn merged(a: &HashMap<String, f64>, b: &HashMap<String, f64>) -> HashMap<String, f64> {
    let mut out = a.clone();
    for (currency, amount) in b {
        *out.entry(currency.clone()).or_insert(0.0) += amount;
    }
    out
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

    let rates = match source {
        SnapshotSource::Live => get_all_rates(),
        SnapshotSource::Backfill => get_rates_for_date(conn, day_start),
    };

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

    match existing {
        Some((_, existing_source))
            if source == SnapshotSource::Backfill && existing_source == "live" =>
        {
            // I4: reconstructions never replace authentic records.
            Ok(())
        }
        Some((id, _)) => {
            conn.execute(
                "UPDATE portfolio_metrics_history
                 SET total_savings = ?2, total_loans_principal = ?3, total_investments = ?4,
                     total_crypto = ?5, total_bonds = ?6, total_real_estate_personal = ?7,
                     total_real_estate_investment = ?8, total_other_assets = ?9, recorded_at = ?10,
                     investments_by_currency = ?11, crypto_by_currency = ?12,
                     savings_by_currency = ?13, bonds_by_currency = ?14,
                     real_estate_by_currency = ?15, loans_by_currency = ?16,
                     other_assets_by_currency = ?17, source = ?18
                 WHERE id = ?1",
                rusqlite::params![
                    id,
                    czk_total(&breakdowns.savings, &rates).to_string(),
                    czk_total(&breakdowns.loans, &rates).to_string(),
                    czk_total(&breakdowns.investments, &rates).to_string(),
                    czk_total(&breakdowns.crypto, &rates).to_string(),
                    czk_total(&breakdowns.bonds, &rates).to_string(),
                    czk_total(&breakdowns.real_estate_personal, &rates).to_string(),
                    czk_total(&breakdowns.real_estate_investment, &rates).to_string(),
                    czk_total(&breakdowns.other_assets, &rates).to_string(),
                    row_ts,
                    breakdown_json(&breakdowns.investments),
                    breakdown_json(&breakdowns.crypto),
                    breakdown_json(&breakdowns.savings),
                    breakdown_json(&breakdowns.bonds),
                    breakdown_json(&real_estate_merged),
                    breakdown_json(&breakdowns.loans),
                    breakdown_json(&breakdowns.other_assets),
                    source.as_str(),
                ],
            )?;
            Ok(())
        }
        None => {
            conn.execute(
                "INSERT INTO portfolio_metrics_history
                 (id, total_savings, total_loans_principal, total_investments, total_crypto,
                  total_bonds, total_real_estate_personal, total_real_estate_investment,
                  total_other_assets, recorded_at,
                  investments_by_currency, crypto_by_currency, savings_by_currency,
                  bonds_by_currency, real_estate_by_currency, loans_by_currency,
                  other_assets_by_currency, source)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                         ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
                rusqlite::params![
                    Uuid::new_v4().to_string(),
                    czk_total(&breakdowns.savings, &rates).to_string(),
                    czk_total(&breakdowns.loans, &rates).to_string(),
                    czk_total(&breakdowns.investments, &rates).to_string(),
                    czk_total(&breakdowns.crypto, &rates).to_string(),
                    czk_total(&breakdowns.bonds, &rates).to_string(),
                    czk_total(&breakdowns.real_estate_personal, &rates).to_string(),
                    czk_total(&breakdowns.real_estate_investment, &rates).to_string(),
                    czk_total(&breakdowns.other_assets, &rates).to_string(),
                    row_ts,
                    breakdown_json(&breakdowns.investments),
                    breakdown_json(&breakdowns.crypto),
                    breakdown_json(&breakdowns.savings),
                    breakdown_json(&breakdowns.bonds),
                    breakdown_json(&real_estate_merged),
                    breakdown_json(&breakdowns.loans),
                    breakdown_json(&breakdowns.other_assets),
                    source.as_str(),
                ],
            )?;
            Ok(())
        }
    }
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
/// cliff the net-worth chart at the live boundary). Real estate is carried
/// from the live row's split CZK totals — the stored breakdown is combined,
/// so the split survives only in CZK terms. Returns None only when no live
/// row exists at all (caller falls back to current table values).
pub fn carried_statics_from_nearest_live(
    conn: &Connection,
    before_ts: i64,
) -> Result<Option<StaticBreakdowns>> {
    use crate::services::currency::breakdown_from_json;

    const COLUMNS: &str = "savings_by_currency, bonds_by_currency, loans_by_currency,
                    other_assets_by_currency,
                    CAST(total_real_estate_personal AS REAL),
                    CAST(total_real_estate_investment AS REAL)";

    type StaticsRow = (String, String, String, String, f64, f64);
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

    Ok(row.map(
        |(savings, bonds, loans, other, re_personal, re_investment)| {
            let czk_map = |v: f64| -> HashMap<String, f64> {
                if v != 0.0 {
                    HashMap::from([("CZK".to_string(), v)])
                } else {
                    HashMap::new()
                }
            };
            StaticBreakdowns {
                savings: breakdown_from_json(&savings),
                bonds: breakdown_from_json(&bonds),
                loans: breakdown_from_json(&loans),
                other_assets: breakdown_from_json(&other),
                real_estate_personal: czk_map(re_personal),
                real_estate_investment: czk_map(re_investment),
            }
        },
    ))
}

/// Rewrite selected transaction-driven classes of an existing snapshot row:
/// the class total and its breakdown change together, both derived at the
/// day's rates. Rows the day does not have are left for the full backfill to
/// create; every other column (including `source`) is untouched.
pub fn update_classes(
    conn: &Connection,
    day_timestamp: i64,
    classes: &[(AssetClassKind, HashMap<String, f64>)],
) -> Result<()> {
    let day_start = (day_timestamp / SECONDS_PER_DAY) * SECONDS_PER_DAY;
    let day_end = day_start + SECONDS_PER_DAY;

    let exists: bool = conn
        .query_row(
            "SELECT 1 FROM portfolio_metrics_history
             WHERE recorded_at >= ?1 AND recorded_at < ?2 LIMIT 1",
            [day_start, day_end],
            |_| Ok(true),
        )
        .unwrap_or(false);
    if !exists {
        return Ok(());
    }

    let rates = get_rates_for_date(conn, day_start);

    for (kind, breakdown) in classes {
        let (total_column, breakdown_column) = match kind {
            AssetClassKind::Savings => ("total_savings", "savings_by_currency"),
            AssetClassKind::Investments => ("total_investments", "investments_by_currency"),
            AssetClassKind::Crypto => ("total_crypto", "crypto_by_currency"),
            AssetClassKind::OtherAssets => ("total_other_assets", "other_assets_by_currency"),
        };
        let sql = format!(
            "UPDATE portfolio_metrics_history SET {total_column} = ?1, {breakdown_column} = ?2
             WHERE recorded_at >= ?3 AND recorded_at < ?4"
        );
        conn.execute(
            &sql,
            rusqlite::params![
                czk_total(breakdown, &rates).to_string(),
                breakdown_json(breakdown),
                day_start,
                day_end
            ],
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
/// ever. The class total and its breakdown are written together
/// through `update_classes`, keeping invariant I3.
///
/// `until_day_exclusive` is typically today's day start: today's row belongs
/// to the live snapshot and must not be replaced by a reconstruction.
pub fn aggregate_ticker_history(
    conn: &Connection,
    kind: AssetClassKind,
    from_day: i64,
    until_day_exclusive: i64,
) -> Result<()> {
    let (history_table, tx_table) = match kind {
        AssetClassKind::Investments => ("stock_value_history", "investment_transactions"),
        AssetClassKind::Crypto => ("crypto_value_history", "crypto_transactions"),
        AssetClassKind::Savings | AssetClassKind::OtherAssets => {
            return Err(crate::error::AppError::Internal(
                "aggregate_ticker_history supports stocks and crypto only".into(),
            ))
        }
    };
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
    if days.is_empty() {
        return Ok(());
    }

    // One pass instead of one aggregate query per day: every ticker's
    // history rows and position changes are read once, then each day moves the
    // two per-ticker cursors forward. The old per-day query re-scanned every
    // history row before the day, 600 times over.
    let last_day = days.last().copied().unwrap_or(from_day);
    let mut series = load_ticker_series(conn, history_table, tx_table, last_day + SECONDS_PER_DAY)?;

    for day in days {
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

/// Snapshot rows for the history charts, newest first, optionally limited to
/// `recorded_at` in `[start_date, end_date]`. Every row carries its provenance
/// (`source`: `live` = recorded by the running app, `backfill` = reconstructed;
/// see [`SnapshotSource`]) so charts can tell an authentic record from an
/// estimate — on backfilled days the static classes (bank accounts, bonds,
/// real estate, loans) are carried from the nearest live row.
pub fn read_history(
    conn: &Connection,
    start_date: Option<i64>,
    end_date: Option<i64>,
) -> Result<Vec<PortfolioMetricsHistory>> {
    let mut query = String::from(
        "SELECT id, total_savings, total_loans_principal, total_investments, total_crypto,
                total_bonds, total_real_estate_personal, total_real_estate_investment,
                total_other_assets, recorded_at,
                investments_by_currency, crypto_by_currency, savings_by_currency,
                bonds_by_currency, real_estate_by_currency, loans_by_currency,
                other_assets_by_currency, source
         FROM portfolio_metrics_history",
    );

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut conditions = Vec::new();

    if let Some(start) = start_date {
        conditions.push(" recorded_at >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_date {
        conditions.push(" recorded_at <= ?");
        params.push(Box::new(end));
    }

    if !conditions.is_empty() {
        query.push_str(" WHERE");
        query.push_str(&conditions.join(" AND"));
    }
    query.push_str(" ORDER BY recorded_at DESC");

    let mut stmt = conn.prepare(&query)?;
    let history = stmt
        .query_map(rusqlite::params_from_iter(params), |row| {
            Ok(PortfolioMetricsHistory {
                id: row.get(0)?,
                total_savings: row.get(1)?,
                total_loans_principal: row.get(2)?,
                total_investments: row.get(3)?,
                total_crypto: row.get(4)?,
                total_bonds: row.get(5)?,
                total_real_estate_personal: row.get(6)?,
                total_real_estate_investment: row.get(7)?,
                total_other_assets: row.get(8).unwrap_or("0".to_string()),
                recorded_at: row.get(9)?,
                investments_by_currency: row.get::<_, String>(10).unwrap_or("{}".to_string()),
                crypto_by_currency: row.get::<_, String>(11).unwrap_or("{}".to_string()),
                savings_by_currency: row.get::<_, String>(12).unwrap_or("{}".to_string()),
                bonds_by_currency: row.get::<_, String>(13).unwrap_or("{}".to_string()),
                real_estate_by_currency: row.get::<_, String>(14).unwrap_or("{}".to_string()),
                loans_by_currency: row.get::<_, String>(15).unwrap_or("{}".to_string()),
                other_assets_by_currency: row.get::<_, String>(16).unwrap_or("{}".to_string()),
                source: row
                    .get::<_, String>(17)
                    .unwrap_or_else(|_| SnapshotSource::Live.as_str().to_string()),
            })
        })?
        .filter_map(|r| r.ok())
        .collect();

    Ok(history)
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
    use rusqlite::Connection;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            r#"
            CREATE TABLE portfolio_metrics_history (
                id TEXT PRIMARY KEY,
                total_savings TEXT NOT NULL,
                total_loans_principal TEXT NOT NULL,
                total_investments TEXT NOT NULL,
                total_crypto TEXT NOT NULL,
                total_bonds TEXT NOT NULL,
                total_real_estate_personal TEXT NOT NULL,
                total_real_estate_investment TEXT NOT NULL,
                total_other_assets TEXT NOT NULL DEFAULT '0',
                recorded_at INTEGER NOT NULL,
                investments_by_currency TEXT NOT NULL DEFAULT '{}',
                crypto_by_currency TEXT NOT NULL DEFAULT '{}',
                savings_by_currency TEXT NOT NULL DEFAULT '{}',
                bonds_by_currency TEXT NOT NULL DEFAULT '{}',
                real_estate_by_currency TEXT NOT NULL DEFAULT '{}',
                loans_by_currency TEXT NOT NULL DEFAULT '{}',
                other_assets_by_currency TEXT NOT NULL DEFAULT '{}',
                source TEXT NOT NULL DEFAULT 'live'
            );
            CREATE TABLE exchange_rate_history (
                date INTEGER NOT NULL,
                currency TEXT NOT NULL,
                rate REAL NOT NULL,
                PRIMARY KEY (date, currency)
            );
        "#,
        )
        .expect("schema");
        conn
    }

    const DAY: i64 = SECONDS_PER_DAY;
    const TEST_DAY: i64 = DAY * 20_000;

    fn insert_day_rate(conn: &Connection, day: i64, currency: &str, rate: f64) {
        conn.execute(
            "INSERT INTO exchange_rate_history (date, currency, rate) VALUES (?1, ?2, ?3)",
            rusqlite::params![day, currency, rate],
        )
        .expect("insert rate");
    }

    fn map(entries: &[(&str, f64)]) -> HashMap<String, f64> {
        entries.iter().map(|(c, v)| (c.to_string(), *v)).collect()
    }

    fn row(conn: &Connection) -> (String, String, String, String, i64, String) {
        conn.query_row(
            "SELECT total_investments, total_savings, investments_by_currency,
                    real_estate_by_currency, recorded_at, source
             FROM portfolio_metrics_history LIMIT 1",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .expect("row")
    }

    #[test]
    fn backfill_insert_derives_totals_at_day_rates() {
        let conn = setup_test_db();
        insert_day_rate(&conn, TEST_DAY, "USD", 20.0);

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

        let (inv, savings, inv_json, _, recorded_at, source) = row(&conn);
        assert_eq!(inv, "2000");
        assert_eq!(savings, "500");
        assert!(inv_json.contains("\"USD\":100"));
        assert_eq!(recorded_at, TEST_DAY, "backfill rows sit at midnight");
        assert_eq!(source, "backfill");
    }

    #[test]
    fn backfill_never_overwrites_live_row() {
        let conn = setup_test_db();
        insert_day_rate(&conn, TEST_DAY, "USD", 20.0);

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

        let (inv, _, _, _, recorded_at, source) = row(&conn);
        assert_eq!(inv, "1111", "live totals must survive a backfill write");
        assert_eq!(recorded_at, TEST_DAY + 4000);
        assert_eq!(source, "live");
    }

    #[test]
    fn live_overwrites_backfill_row_and_claims_provenance() {
        let conn = setup_test_db();
        insert_day_rate(&conn, TEST_DAY, "USD", 20.0);

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

        let (inv, _, _, _, recorded_at, source) = row(&conn);
        assert_eq!(inv, "3333");
        assert_eq!(recorded_at, TEST_DAY + 5555);
        assert_eq!(source, "live");
    }

    #[test]
    fn backfill_updates_existing_backfill_row() {
        let conn = setup_test_db();
        insert_day_rate(&conn, TEST_DAY, "USD", 20.0);

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

        let (inv, _, inv_json, _, _, source) = row(&conn);
        assert_eq!(inv, "3000");
        assert!(inv_json.contains("\"USD\":150"));
        assert_eq!(source, "backfill");
    }

    #[test]
    fn real_estate_split_totals_with_merged_breakdown() {
        let conn = setup_test_db();
        let breakdowns = ClassBreakdowns {
            real_estate_personal: map(&[("CZK", 100.0)]),
            real_estate_investment: map(&[("CZK", 50.0)]),
            ..Default::default()
        };
        upsert_snapshot(&conn, TEST_DAY, SnapshotSource::Backfill, &breakdowns).expect("upsert");

        let (personal, investment, re_json): (String, String, String) = conn
            .query_row(
                "SELECT total_real_estate_personal, total_real_estate_investment,
                        real_estate_by_currency
                 FROM portfolio_metrics_history",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("row");
        assert_eq!(personal, "100");
        assert_eq!(investment, "50");
        assert!(re_json.contains("\"CZK\":150"));
    }

    #[test]
    fn update_classes_rewrites_total_and_breakdown_together() {
        let conn = setup_test_db();
        insert_day_rate(&conn, TEST_DAY, "USD", 20.0);

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

        let (inv, savings, inv_json, _, _, _) = row(&conn);
        assert_eq!(inv, "4000", "total derived from the new breakdown");
        assert!(inv_json.contains("\"USD\":200"));
        assert_eq!(savings, "500", "other classes untouched");
    }

    #[test]
    fn carried_statics_come_from_nearest_earlier_live_row() {
        let conn = setup_test_db();
        insert_day_rate(&conn, TEST_DAY, "EUR", 25.0);

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
        assert!((statics.savings["EUR"] - 40.0).abs() < 1e-9);
        assert!((statics.bonds["CZK"] - 100.0).abs() < 1e-9);
        assert!((statics.real_estate_personal["CZK"] - 900.0).abs() < 1e-9);
        assert!(statics.loans.is_empty());
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
        assert!((statics.savings["CZK"] - 2500.0).abs() < 1e-9);
        assert!((statics.loans["CZK"] - 12000.0).abs() < 1e-9);
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
                value_czk TEXT NOT NULL, quantity TEXT NOT NULL, price TEXT NOT NULL,
                currency TEXT NOT NULL, UNIQUE(ticker, recorded_at)
            );
            CREATE TABLE crypto_value_history (
                id TEXT PRIMARY KEY, ticker TEXT NOT NULL, recorded_at INTEGER NOT NULL,
                value_czk TEXT NOT NULL, quantity TEXT NOT NULL, price TEXT NOT NULL,
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

    fn insert_history_day(conn: &Connection, day: i64) {
        conn.execute(
            "INSERT INTO portfolio_metrics_history (id, total_savings, total_loans_principal,
                total_investments, total_crypto, total_bonds, total_real_estate_personal,
                total_real_estate_investment, recorded_at, source)
             VALUES (?1, '0', '0', '999', '999', '0', '0', '0', ?2, 'live')",
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
                "INSERT INTO {table} (id, ticker, recorded_at, value_czk, quantity, price, currency)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)"
            ),
            rusqlite::params![
                format!("{ticker}-{day}"),
                ticker,
                day,
                (qty * price).to_string(),
                qty.to_string(),
                price.to_string(),
                ccy
            ],
        )
        .expect("ticker row");
    }

    fn total_investments(conn: &Connection, day: i64) -> f64 {
        conn.query_row(
            "SELECT CAST(total_investments AS REAL) FROM portfolio_metrics_history WHERE recorded_at = ?1",
            [day],
            |r| r.get(0),
        )
        .expect("total")
    }

    /// a ticker sold on day 4 must not be carried forward from its
    /// last row forever. AAPL (1 000 CZK) is held days 1–3 and sold on day 4;
    /// MSFT (500 CZK) is held throughout. Day 6 must total 500, not 1 500.
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

        assert!((total_investments(&conn, day(3)) - 1_500.0).abs() < 1e-9);
        assert!((total_investments(&conn, day(4)) - 500.0).abs() < 1e-9);
        assert!((total_investments(&conn, day(6)) - 500.0).abs() < 1e-9);
        // I3: the breakdown is rewritten together with the total
        let breakdown: String = conn
            .query_row(
                "SELECT investments_by_currency FROM portfolio_metrics_history WHERE recorded_at = ?1",
                [day(6)],
                |r| r.get(0),
            )
            .expect("breakdown");
        assert_eq!(
            breakdown_from_json_sorted(&breakdown),
            vec![("CZK".to_string(), 500.0)]
        );
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

        let total = |day: i64| -> f64 {
            conn.query_row(
                "SELECT CAST(total_crypto AS REAL) FROM portfolio_metrics_history WHERE recorded_at = ?1",
                [day],
                |r| r.get(0),
            )
            .expect("total")
        };
        assert!((total(D1) - 200.0).abs() < 1e-9);
        assert!(
            (total(d2) - 999.0).abs() < 1e-9,
            "excluded day keeps its live value"
        );
    }

    fn breakdown_from_json_sorted(json: &str) -> Vec<(String, f64)> {
        let mut v: Vec<(String, f64)> = crate::services::currency::breakdown_from_json(json)
            .into_iter()
            .collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v
    }

    fn insert_history_row(conn: &Connection, id: &str, recorded_at: i64, source: &str) {
        conn.execute(
            "INSERT INTO portfolio_metrics_history
             (id, total_savings, total_loans_principal, total_investments, total_crypto,
              total_bonds, total_real_estate_personal, total_real_estate_investment,
              recorded_at, source)
             VALUES (?1, '500', '0', '0', '0', '0', '0', '0', ?2, ?3)",
            rusqlite::params![id, recorded_at, source],
        )
        .unwrap();
    }

    #[test]
    fn read_history_returns_the_row_source_newest_first() {
        // the frontend must be able to tell reconstructed days from
        // authentic records.
        let conn = setup_test_db();
        insert_history_row(&conn, "old", TEST_DAY, "backfill");
        insert_history_row(&conn, "new", TEST_DAY + DAY + 3600, "live");

        let rows = read_history(&conn, None, None).unwrap();
        let sources: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r.id.as_str(), r.source.as_str()))
            .collect();
        assert_eq!(sources, vec![("new", "live"), ("old", "backfill")]);
        assert_eq!(rows[0].total_savings, "500");

        let json = serde_json::to_value(&rows[1]).unwrap();
        assert_eq!(json["source"], "backfill");
    }

    #[test]
    fn read_history_filters_the_range_inclusively() {
        let conn = setup_test_db();
        insert_history_row(&conn, "a", TEST_DAY, "backfill");
        insert_history_row(&conn, "b", TEST_DAY + DAY, "backfill");
        insert_history_row(&conn, "c", TEST_DAY + 2 * DAY, "backfill");

        let ids = |start, end| -> Vec<String> {
            read_history(&conn, start, end)
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect()
        };
        assert_eq!(ids(Some(TEST_DAY + DAY), None), vec!["c", "b"]);
        assert_eq!(ids(None, Some(TEST_DAY + DAY)), vec!["b", "a"]);
        assert_eq!(ids(Some(TEST_DAY + DAY), Some(TEST_DAY + DAY)), vec!["b"]);
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
