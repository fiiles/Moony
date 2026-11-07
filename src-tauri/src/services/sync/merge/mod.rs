//! Applying another device's changes (spec §7.1, D12–D14): one changeset at a time, in one SQL
//! transaction with `PRAGMA defer_foreign_keys = ON` and capture in `merge` mode, then the repairs
//! of everything the merge touched.
//!
//! Every device must end with identical synced tables whatever order the changesets arrive in.
//!
//! Invariant: a dead row's `'*'` (and any column clock it left behind) is meaningless. Dead rows
//! keep different `'*'` values on different devices (cascades, NO ACTION drops, a row created
//! and deleted before it was sent), so no merge, journal or repair decision may read it: only a
//! live row's clocks and every key's tombstone `'-'` must agree across devices. A re-insert sets
//! `'*' = g`, never `max` with a dead incarnation.
//! The row algorithm lives in `row`, the per-column rules in `columns`, the foreign-key rules in
//! `parents`.

mod columns;
mod parents;
mod row;

pub use row::{apply_row, MergeCtx, RowResult};

use std::collections::BTreeMap;

use rusqlite::{params, Connection};

use super::capture::{self, Mode};
use super::changeset::{Changeset, RowChange};
use super::hlc::{DeviceId, Observation};
use super::registry::keys::schema_number;
use super::repair;
use crate::error::{AppError, Result};

/// The event the engine (phase 2) emits after a merge, with `ApplyReport::domains`; the frontend
/// runs the same ritual as for `mcp-data-changed`.
pub const DATA_CHANGED_EVENT: &str = "sync-data-changed";

/// Why a changeset was not applied. The caller rolls back and stops (§9.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    /// A child references a parent this device has never seen and holds no tombstone of (an
    /// envelope gap or an ordering bug). `parent` is `"<parent table> <parent key>"`.
    MissingParent {
        table: String,
        key: String,
        parent: String,
    },
    /// The changeset was written by a newer schema than this build knows.
    NewerSchema(u32),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ApplyReport {
    /// Row changes that landed (tombstone-only changes included).
    pub applied: usize,
    /// Inserts dropped: from an ended delete epoch, or lost to a deleted parent.
    pub dropped: usize,
    /// History domains queued, with their earliest affected day (`None`: from the first fact).
    pub domains: BTreeMap<&'static str, Option<i64>>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Applied(ApplyReport),
    Stopped(StopReason),
}

/// Applies one changeset of `cs.device` in its own transaction and records it as that device's
/// head (`sync_heads`) in the same transaction. A changeset from a newer schema stops before
/// anything is read or written; a stop rolls everything back.
pub fn apply_changeset(
    conn: &Connection,
    cs: &Changeset,
    hash: [u8; 32],
    now: i64,
) -> Result<Outcome> {
    if cs.schema > schema_number() {
        return Ok(Outcome::Stopped(StopReason::NewerSchema(cs.schema)));
    }
    let tx = conn.unchecked_transaction()?;
    match merge_rows(&tx, &cs.rows, now)? {
        Outcome::Applied(report) => {
            record_head(&tx, cs.device, cs.seq, hash)?;
            tx.commit()?;
            Ok(Outcome::Applied(report))
        }
        stopped => {
            tx.rollback()?;
            Ok(stopped)
        }
    }
}

/// Merges `rows` in the given order inside the CALLER's transaction (also used by the snapshot
/// restore): every clock is observed first, then each row goes through `apply_row`; the first
/// stop returns `Stopped` (the caller rolls back). Then the repairs of everything touched run, and
/// their warnings and history domains join the report.
pub fn merge_rows(conn: &Connection, rows: &[RowChange], now: i64) -> Result<Outcome> {
    if conn.is_autocommit() {
        return Err(AppError::Internal(
            "sync merge: merge_rows must run inside a transaction".into(),
        ));
    }
    if !capture::is_installed(conn)? {
        return Err(AppError::Internal(
            "sync merge: capture is not installed on this connection".into(),
        ));
    }
    conn.execute_batch("PRAGMA defer_foreign_keys = ON")?;
    let mut ctx = MergeCtx::default();
    observe_clocks(conn, rows, &mut ctx.report)?;
    let stopped = capture::with_mode(conn, Mode::Merge, || {
        for change in rows {
            match apply_row(conn, change, &mut ctx)? {
                RowResult::Applied => ctx.report.applied += 1,
                RowResult::Dropped => ctx.report.dropped += 1,
                RowResult::Stop(reason) => return Ok(Some(reason)),
            }
        }
        Ok(None)
    })?;
    if let Some(reason) = stopped {
        return Ok(Outcome::Stopped(reason));
    }
    let repairs = repair::after_merge(conn, &ctx.touched, now)?;
    ctx.report.warnings.extend(repairs.warnings);
    ctx.report.domains = repairs.domains;
    Ok(Outcome::Applied(ctx.report))
}

/// Moves the connection's clock past every clock the rows carry. The newest clock of each author
/// is observed (the same effect as observing each clock); one far ahead of local time is applied
/// anyway, with one warning naming the author.
fn observe_clocks(conn: &Connection, rows: &[RowChange], report: &mut ApplyReport) -> Result<()> {
    let mut newest: BTreeMap<DeviceId, u64> = BTreeMap::new();
    for row in rows {
        let clocks = std::iter::once(row.generation)
            .chain(row.deleted)
            .chain(row.columns.values().map(|(_, c)| *c))
            .chain(row.parents.values().copied());
        for clock in clocks {
            let entry = newest.entry(clock.device).or_insert(0);
            *entry = (*entry).max(clock.hlc);
        }
    }
    for (device, hlc) in newest {
        if let Observation::FarFuture { ahead_ms } = capture::observe(conn, hlc)? {
            let warning = format!(
                "device {} sent a clock {} minutes ahead of this device's time",
                device.to_b64(),
                ahead_ms / 60_000
            );
            log::warn!("[SYNC] {warning}");
            report.warnings.push(warning);
        }
    }
    Ok(())
}

fn record_head(conn: &Connection, device: DeviceId, seq: u64, hash: [u8; 32]) -> Result<()> {
    let seq = i64::try_from(seq)
        .map_err(|_| AppError::Internal(format!("sync merge: sequence {seq} is out of range")))?;
    conn.execute(
        "INSERT INTO sync_heads (device, seq, hash) VALUES (?1, ?2, ?3) \
         ON CONFLICT(device) DO UPDATE SET seq = excluded.seq, hash = excluded.hash",
        params![device.0.to_vec(), seq, hash.to_vec()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests;
