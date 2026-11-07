//! Repairs after a merge (spec §7.1 steps 1–3, §7.2, §7.4, §14): the recalculations of every
//! touched parent (`mirrors`), the invariants the database does not enforce (`invariants`), the
//! rename rule (`renames`), the unlink rule (`links`), the history work per domain, and the
//! migration seeds that must not revive deleted rows.
//!
//! Rename and unlink are derived (ruling U3, U4): the value a person typed is what syncs, kept in
//! the Local table `sync_typed` while a repair shows another value, and the stored column is a
//! pure function of the typed values of its scope.
//!
//! Everything here runs in `repair` mode, so nothing is captured: every device computes the same
//! result from the same merged rows. A repair is a function of the merged rows only — every
//! selection is ordered by primary key, never by arrival order, device or time.

pub mod invariants;
pub mod links;
pub mod mirrors;
pub mod renames;

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::{params, Connection, OptionalExtension};

use super::capture::{self, Mode, CLOCK_UPSERT};
use super::hlc::{Clock, DeviceId};
use super::registry::{self, schema, KeyShape};
use crate::error::{AppError, Result};
use crate::services::mcp_refresh;

/// What a merge changed, as the repairs need it. Task 13's merge fills it exactly as each field
/// documents.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Touched {
    /// table → pk text of every row the merge inserted, changed or deleted
    pub rows: BTreeMap<String, BTreeSet<String>>,
    /// parent table → ids referenced, old and new, by every touched child row
    pub parents: BTreeMap<String, BTreeSet<String>>,
    /// tables that lost a row (tombstone, cascade rule, orphan rule, unique loser)
    pub deleted: BTreeSet<String>,
    /// refresh domain → earliest affected UTC day; None = from the first fact
    pub days: BTreeMap<&'static str, Option<i64>>,
    /// old and new `ticker` of touched rows of tables that have one
    pub tickers: BTreeSet<String>,
    /// old and new (category_id, timeframe) of touched budget_goal_versions
    pub budget_keys: BTreeSet<(String, String)>,
    /// (table, scope value or "") of touched rows of the four rename tables
    pub rename_scopes: BTreeSet<(String, String)>,
}

impl Touched {
    pub fn row(&mut self, table: &str, pk: &str) {
        self.rows
            .entry(table.to_string())
            .or_default()
            .insert(pk.to_string());
    }

    pub fn parent(&mut self, table: &str, id: &str) {
        self.parents
            .entry(table.to_string())
            .or_default()
            .insert(id.to_string());
    }

    pub fn mark_deleted(&mut self, table: &str) {
        self.deleted.insert(table.to_string());
    }

    /// Keeps the earliest day per domain; `None` (from the first fact) absorbs any day.
    pub fn day(&mut self, domain: &'static str, day: Option<i64>) {
        match self.days.get_mut(domain) {
            None => {
                self.days.insert(domain, day);
            }
            Some(stored) => {
                *stored = match (*stored, day) {
                    (Some(a), Some(b)) => Some(a.min(b)),
                    _ => None,
                };
            }
        }
    }

    /// The ids of `table` that are touched as a row or referenced as a parent, ascending.
    pub(crate) fn owners(&self, table: &str) -> BTreeSet<String> {
        let mut ids = self.rows.get(table).cloned().unwrap_or_default();
        if let Some(parents) = self.parents.get(table) {
            ids.extend(parents.iter().cloned());
        }
        ids
    }

    /// Whether any row of one of `tables` was touched.
    pub(crate) fn touches_any(&self, tables: &[&str]) -> bool {
        tables.iter().any(|t| self.rows.contains_key(*t))
    }
}

/// What the repairs could not do exactly (each one a warning, the merge still commits) and the
/// history domains they queued.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RepairReport {
    pub warnings: Vec<String>,
    pub domains: BTreeMap<&'static str, Option<i64>>,
}

/// Runs every repair a merge needs, from `touched` only, in `repair` mode: the recalculations,
/// then the invariants, then the history queue. A recalculation that fails never fails the
/// merge (it becomes a warning, see `mirrors`); SQL errors propagate.
pub fn after_merge(conn: &Connection, touched: &Touched, now: i64) -> Result<RepairReport> {
    capture::with_mode(conn, Mode::Repair, || {
        let mut report = RepairReport::default();
        mirrors::run(conn, touched, now, &mut report)?;
        invariants::run(conn, touched, now)?;
        for (domain, day) in &touched.days {
            mcp_refresh::enqueue(conn, domain, *day)?;
        }
        report.domains = touched.days.clone();
        Ok(report)
    })
}

/// Every repair over every parent and scope, as after a snapshot restore. Queues no history
/// work: a restored snapshot brings its own.
pub fn full(conn: &Connection, now: i64) -> Result<RepairReport> {
    after_merge(conn, &everything(conn)?, now)
}

/// A `Touched` naming every parent, ticker, budget and rename scope there is.
fn everything(conn: &Connection) -> Result<Touched> {
    let mut touched = Touched::default();
    for table in [
        "bank_accounts",
        "loans",
        "real_estate",
        "other_assets",
        "stock_investments",
        "crypto_investments",
    ] {
        for id in strings(conn, &format!("SELECT id FROM {table} ORDER BY id"))? {
            touched.parent(table, &id);
        }
    }
    touched.tickers.extend(strings(
        conn,
        "SELECT ticker FROM investment_transactions UNION SELECT ticker FROM stock_splits \
         UNION SELECT ticker FROM stock_price_overrides UNION SELECT ticker FROM watched_stocks \
         UNION SELECT ticker FROM dividend_overrides",
    )?);
    let mut stmt = conn.prepare(
        "SELECT category_id, timeframe FROM budget_goal_versions \
         UNION SELECT category_id, timeframe FROM budget_goals",
    )?;
    let keys = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    touched.budget_keys.extend(keys);
    for scope in &renames::RENAME_SCOPES {
        match scope.scope_column {
            None => {
                touched
                    .rename_scopes
                    .insert((scope.table.to_string(), String::new()));
            }
            Some(column) => {
                for value in strings(
                    conn,
                    &format!(
                        "SELECT DISTINCT CAST({column} AS TEXT) FROM {}",
                        scope.table
                    ),
                )? {
                    touched
                        .rename_scopes
                        .insert((scope.table.to_string(), value));
                }
            }
        }
    }
    for table in invariants::TABLES {
        touched.rows.entry(table.to_string()).or_default();
    }
    for table in invariants::ATTACHMENT_OWNERS {
        touched.mark_deleted(table);
    }
    Ok(touched)
}

fn strings(conn: &Connection, sql: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(sql)?;
    let values = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(values)
}

/// Deletes, in every Synced table, the rows whose key has a tombstone (`'-'`) and no row clock
/// (`'*'`) at least as new: a migration seed brought back a row a device deleted (§14). Driven
/// from the tombstones (the `sync_clock` key range of the table), so the cost follows the number
/// of tombstones, not the size of the table. Runs before capture is installed, so no trigger
/// records the deletes. Returns how many went.
pub fn delete_revived_seeds(conn: &Connection) -> Result<usize> {
    let mut deleted = 0;
    for spec in registry::synced_tables() {
        if spec.key != KeyShape::Primary {
            continue;
        }
        let pk = schema::primary_key(conn, spec.name)?;
        let (columns, keys) = match pk.as_slice() {
            [] => continue,
            [column] => (column.clone(), "d.pk".to_string()),
            columns => (
                format!("({})", columns.join(", ")),
                (0..columns.len())
                    .map(|i| format!("json_extract(d.pk, '$[{i}]')"))
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
        };
        deleted += conn.execute(
            &format!(
                "DELETE FROM main.{table} WHERE {columns} IN (
                   SELECT {keys} FROM sync_clock d
                   WHERE d.tbl = ?1 AND d.col = '-'
                     AND NOT EXISTS (
                       SELECT 1 FROM sync_clock s
                       WHERE s.tbl = d.tbl AND s.pk = d.pk AND s.col = '*'
                         AND (s.hlc, s.device) >= (d.hlc, d.device)))",
                table = spec.name
            ),
            [spec.name],
        )?;
    }
    Ok(deleted)
}

/// The columns of `table` a repair may show differently from what was typed (U3, U4): the name of
/// a rename scope, `cashflow_items.replacement_source_id`.
pub fn shadowed_columns(table: &str) -> Vec<&'static str> {
    renames::RENAME_SCOPES
        .iter()
        .filter(|scope| scope.table == table)
        .map(|scope| scope.name_column)
        .chain(
            links::LINKS
                .iter()
                .filter(|(t, _)| *t == table)
                .map(|(_, column)| *column),
        )
        .collect()
}

/// Records that `col` of row `pk` shows `shown` while `typed` was typed (`sync_typed`); no entry
/// when they are the same or nothing (NULL) was typed.
pub(crate) fn set_typed(
    conn: &Connection,
    table: &str,
    pk: &str,
    col: &str,
    typed: Option<&str>,
    shown: Option<&str>,
) -> Result<()> {
    match typed {
        Some(typed) if Some(typed) != shown => {
            conn.prepare_cached(
                "INSERT INTO sync_typed (tbl, pk, col, typed, assigned) VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(tbl, pk, col) DO UPDATE SET typed = excluded.typed,
                   assigned = excluded.assigned",
            )?
            .execute(params![table, pk, col, typed, shown])?;
        }
        _ => {
            conn.prepare_cached("DELETE FROM sync_typed WHERE tbl = ?1 AND pk = ?2 AND col = ?3")?
                .execute(params![table, pk, col])?;
        }
    }
    Ok(())
}

/// Drops the `sync_typed` entries of `table.col` whose row is gone.
pub(crate) fn forget_deleted(conn: &Connection, table: &str, col: &str) -> Result<()> {
    conn.execute(
        &format!(
            "DELETE FROM sync_typed WHERE tbl = ?1 AND col = ?2 AND pk NOT IN (SELECT id FROM {table})"
        ),
        [table, col],
    )?;
    Ok(())
}

/// Records a tombstone `'-'` at `clock` for a row a repair deleted, the way the deleting device
/// would. Never queued: every device makes the same repair itself.
pub fn tombstone_local(conn: &Connection, table: &str, pk: &str, clock: Clock) -> Result<()> {
    let hlc = i64::try_from(clock.hlc)
        .map_err(|_| AppError::Internal(format!("clock {} is out of range", clock.hlc)))?;
    conn.execute(
        &format!(
            "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES (?1, ?2, '-', ?3, ?4) \
             {CLOCK_UPSERT}"
        ),
        params![table, pk, hlc, clock.device.0.to_vec()],
    )?;
    Ok(())
}

/// The row clock (`'*'`, the generation) of a key, `Clock::ZERO` for a row that never had one
/// (it predates sync on this profile).
pub(crate) fn row_clock(conn: &Connection, table: &str, pk: &str) -> Result<Clock> {
    let raw: Option<(i64, Vec<u8>)> = conn
        .query_row(
            "SELECT hlc, device FROM sync_clock WHERE tbl = ?1 AND pk = ?2 AND col = '*'",
            params![table, pk],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((hlc, device)) = raw else {
        return Ok(Clock::ZERO);
    };
    let hlc = u64::try_from(hlc)
        .map_err(|_| AppError::Internal(format!("sync_clock holds a negative clock {hlc}")))?;
    let device: [u8; 16] = device
        .try_into()
        .map_err(|_| AppError::Internal("sync_clock.device is not 16 bytes".into()))?;
    Ok(Clock {
        hlc,
        device: DeviceId(device),
    })
}

/// The history domain (`mcp_refresh`) a synced table belongs to, and the column holding the day
/// a row affects (`None`: the row affects the domain from its first fact).
pub fn domain_of(table: &str) -> Option<(&'static str, Option<&'static str>)> {
    Some(match table {
        "stock_investments" => ("stocks", None),
        "investment_transactions" => ("stocks", Some("transaction_date")),
        "investment_dividends" => ("stocks", Some("ex_date")),
        "stock_splits" => ("stocks", Some("effective_day")),
        "stock_price_overrides" | "dividend_overrides" => ("stocks", None),
        "crypto_investments" | "crypto_price_overrides" => ("crypto", None),
        "crypto_transactions" => ("crypto", Some("transaction_date")),
        "other_assets" => ("other-assets", None),
        "other_asset_transactions" => ("other-assets", Some("transaction_date")),
        "other_asset_valuations" => ("other-assets", Some("valued_at")),
        "bank_accounts" | "csv_import_batches" | "bank_account_zones" => ("bank-accounts", None),
        "bank_balance_records" => ("bank-accounts", Some("as_of")),
        "bank_transactions" => ("bank-accounts", Some("booking_date")),
        "real_estate" | "real_estate_loans" => ("real-estate", None),
        "real_estate_valuations" => ("real-estate", Some("valued_at")),
        "real_estate_one_time_costs" => ("real-estate", Some("date")),
        "loans" => ("loans", None),
        "loan_events" => ("loans", Some("event_day")),
        "bonds" => ("bonds", None),
        _ => return None,
    })
}

/// Runs one recalculation in its own savepoint, so a failing one leaves nothing half-written.
/// `Ok(Err(e))` is a recalculation that failed on the data (the caller warns and falls back);
/// an SQL error (`AppError::Database`) propagates.
pub(crate) fn attempt(
    conn: &Connection,
    work: impl FnOnce() -> Result<()>,
) -> Result<std::result::Result<(), AppError>> {
    match super::in_savepoint(conn, "sync_repair_step", work) {
        Ok(()) => Ok(Ok(())),
        Err(AppError::Database(message)) => Err(AppError::Database(message)),
        Err(other) => Ok(Err(other)),
    }
}

#[cfg(test)]
mod tests;
