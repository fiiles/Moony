//! Snapshots (spec §8, D16): a logical dump of every synced row with its clocks, every
//! tombstone, the chain heads and the Seed tables, as a stream of length-prefixed JSON records
//! (`stream`). Phase 2 deflates and encrypts the same bytes.
//!
//! Rows are read through `journal::Reader`, so a column a repair shows differently travels with
//! the value typed (a renamed name, a cleared link) and the receiver derives the display itself.
//! Local columns, non-allowlisted `app_config` keys and Local tables (`sync_state`, `sync_typed`,
//! the attachment tables) never enter a snapshot.
//!
//! Two restores, both through the merge engine (D12–D15) and both followed by `repair::full`:
//!
//! - **replace** (a joining device): the synced and Seed tables are emptied first, so the
//!   joiner's own rows and migration seeds cannot come back (a seed deleted in the vault left a
//!   tombstone, not a row);
//! - **merge** (after `410 snapshot_required`, or a re-join): the device's rows came from the
//!   vault, so they merge by their clocks; the outbox stays and is pushed afterwards.
//!
//! A stream is read whole and checked before anything is written; a snapshot from below the
//! barrier this device has crossed is refused (§14).

mod stream;

pub use stream::{read_record, Head, Record};

use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Write};

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use super::capture::{self, triggers::pk_expr, Mode, CLOCK_UPSERT};
use super::chain;
use super::changeset::{DevTable, RowChange, RowKey, SqlValue};
use super::hlc::{Clock, DeviceId, Observation};
use super::in_savepoint;
use super::journal::{category_depth, Reader};
use super::merge::{self, MergeCtx, Outcome};
use super::registry::keys::{fk_depth, schema_number};
use super::registry::{
    self, column_class, schema, ColumnClass, KeyShape, TableClass, TableSpec,
    APP_CONFIG_SYNCED_KEYS,
};
use super::repair;
use super::state::{self, SyncState};
use crate::error::{AppError, Result};
use stream::{encode, invalid, read_all, write_record, Snapshot};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DumpStats {
    /// Row records: live rows and tombstone-only changes.
    pub rows: u64,
    pub seeds: u64,
    /// The newest clock any row record carries (0 when none has one).
    pub max_hlc: u64,
}

/// Writes the snapshot of this profile at relay sequence `seq` (the cursor applied, §8):
/// `devs`, `header` (with the barrier marker's relay sequence when this device has seen it, and its
/// pending below-barrier miss, if any), the
/// synced rows (live rows parents first, then tombstone-only keys children first), the Seed rows,
/// `end`. Call with exclusive use of the connection (the engine holds the session mutex for the
/// whole dump).
///
/// Only a profile with nothing waiting to go out may be dumped: an empty `sync_unsent` (§8) and,
/// by ruling T18e, an empty `sync_outbox`. A snapshot carrying rows its source has not pushed
/// lets a device that restored it push children of those rows ahead of the source's own push,
/// and every other device then stops on the missing parent. The engine pushes and pulls before
/// dumping; otherwise `validation.syncSnapshotPending`, and nothing is written.
pub fn dump(conn: &Connection, seq: u64, out: &mut impl Write) -> Result<DumpStats> {
    let sync = loaded(conn)?;
    let pending: i64 = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM sync_outbox) + (SELECT COUNT(*) FROM sync_unsent)",
        [],
        |r| r.get(0),
    )?;
    if pending > 0 {
        return Err(AppError::Validation(
            "validation.syncSnapshotPending".into(),
        ));
    }
    let rows = synced_rows(conn)?;
    let seeds = seed_rows(conn)?;
    let stats = DumpStats {
        rows: rows.len() as u64,
        seeds: seeds.len() as u64,
        max_hlc: rows.iter().map(newest_clock).max().unwrap_or(0),
    };
    let header = Record::Header {
        schema: schema_number(),
        barrier: sync.barrier,
        seq,
        heads: heads_of(conn, &sync)?,
        barrier_seq: sync.barrier_seq,
        missed_below_barrier: sync.missed_below_barrier,
    };

    // The `devs` record goes first but is known only once every row is encoded.
    let mut devs = DevTable::new();
    let encoded: Vec<_> = rows
        .into_iter()
        .map(|row| encode(&Record::Row(row), &mut devs))
        .collect();
    let mut no_devs = DevTable::new();
    write_record(
        out,
        &encode(&Record::Devices(devs.into_vec()), &mut no_devs),
    )?;
    write_record(out, &encode(&header, &mut no_devs))?;
    for row in &encoded {
        write_record(out, row)?;
    }
    for (table, row) in seeds {
        write_record(out, &encode(&Record::Seed { table, row }, &mut no_devs))?;
    }
    write_record(
        out,
        &encode(
            &Record::End {
                rows: stats.rows,
                seeds: stats.seeds,
            },
            &mut no_devs,
        ),
    )?;
    Ok(stats)
}

/// What a restore did: the stream's counts, and what it could not do exactly or found suspicious
/// (a clock far ahead of this device's time, a merge or repair warning). The restore still
/// committed; the engine shows the warnings in Sync diagnostics.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RestoreReport {
    pub stats: DumpStats,
    pub warnings: Vec<String>,
}

/// A device joins the vault from a snapshot, in the order of ruling R4: its `sync_state` row,
/// the restore by replace (nothing is clocked or queued), then capture, whose clock starts above
/// every restored clock. The rule packs are not rewritten: they come from the vault. The joiner
/// starts at its barrier, so it needs no barrier marker (`state::note_starting_barrier`). One
/// savepoint: a refused or broken stream leaves the profile as it was.
pub fn join(
    conn: &Connection,
    device: DeviceId,
    input: &mut impl Read,
    now: i64,
) -> Result<RestoreReport> {
    let snapshot = read_all(input)?;
    in_savepoint(conn, "sync_snapshot_join", || {
        state::ensure_row(conn, device)?;
        let report = replace(conn, snapshot, now)?;
        capture::install(conn, device)?;
        state::note_starting_barrier(conn)?;
        Ok(report)
    })
}

/// Restores by replacing (a joining device, §8): the synced and Seed tables are emptied (the
/// `user_profile` row stays, with its synced columns overwritten; only the allowlisted
/// `app_config` keys go) with `sync_clock`, `sync_outbox`, `sync_heads` and `sync_typed`, then the
/// snapshot's rows go in through the merge engine, its seeds and heads are written, the cursor
/// and snapshot bookkeeping set, and every repair runs.
///
/// A joining device replaces everything: emptying the synced tables cascades away the Local rows
/// that hang off them, so the joiner's own attachment rows (`real_estate_photo_batches` and their
/// photos, `real_estate_documents`, `insurance_documents`) go with its own real estate and
/// policies.
pub fn restore_replace(
    conn: &Connection,
    input: &mut impl Read,
    now: i64,
) -> Result<RestoreReport> {
    let snapshot = read_all(input)?;
    in_savepoint(conn, "sync_snapshot_replace", || {
        replace(conn, snapshot, now)
    })
}

/// Restores by merging (after `410 snapshot_required`, or a re-join, §8): the rows go through
/// `merge::merge_rows` (D12–D15, so newer local edits win), seeds go in only where their key is
/// absent, every head takes the higher sequence, the cursor and snapshot bookkeeping never move
/// back, the outbox stays; then every repair runs. Needs capture installed, as on any
/// sync-enabled connection.
pub fn restore_merge(conn: &Connection, input: &mut impl Read, now: i64) -> Result<RestoreReport> {
    let snapshot = read_all(input)?;
    in_savepoint(conn, "sync_snapshot_merge", || {
        let sync = admissible(conn, &snapshot)?;
        let mut report = RestoreReport {
            stats: stats_of(&snapshot),
            warnings: Vec::new(),
        };
        observe(conn, report.stats.max_hlc, &mut report.warnings)?;
        match merge::merge_rows(conn, &snapshot.rows, now)? {
            Outcome::Applied(applied) => report.warnings.extend(applied.warnings),
            Outcome::Stopped(reason) => {
                log::warn!("[SYNC] A snapshot did not merge: {reason:?}");
                return Err(invalid());
            }
        }
        finish(conn, &snapshot, &sync, report.stats.max_hlc, false)?;
        report.warnings.extend(repair::full(conn, now)?.warnings);
        Ok(report)
    })
}

/// The body of `restore_replace`, inside the caller's savepoint.
fn replace(conn: &Connection, mut snapshot: Snapshot, now: i64) -> Result<RestoreReport> {
    let sync = admissible(conn, &snapshot)?;
    let mut report = RestoreReport {
        stats: stats_of(&snapshot),
        warnings: Vec::new(),
    };
    conn.execute_batch("PRAGMA defer_foreign_keys = ON")?;
    // The rows go in in merge mode, so nothing is clocked or queued. Without capture (a joining
    // device, R4) it is installed for the restore only.
    let installed = capture::is_installed(conn)?;
    if !installed {
        capture::install(conn, sync.device)?;
    }
    observe(conn, report.stats.max_hlc, &mut report.warnings)?;
    capture::with_mode(conn, Mode::Repair, || clear(conn))?;
    if let Some(i) = snapshot
        .rows
        .iter()
        .position(|r| r.key == RowKey::Singleton)
    {
        let singleton = &snapshot.rows[i];
        if capture::with_mode(conn, Mode::Repair, || overwrite_singleton(conn, singleton))? {
            snapshot.rows.remove(i);
        }
    }
    let mut ctx = MergeCtx::default();
    let stopped = capture::with_mode(conn, Mode::Merge, || {
        merge::apply_all(conn, &snapshot.rows, &mut ctx)
    })?;
    if let Some(reason) = stopped {
        log::warn!("[SYNC] A snapshot did not restore: {reason:?}");
        return Err(invalid());
    }
    report.warnings.append(&mut ctx.report.warnings);
    finish(conn, &snapshot, &sync, report.stats.max_hlc, true)?;
    report.warnings.extend(repair::full(conn, now)?.warnings);
    if !installed {
        capture::uninstall(conn)?;
    }
    Ok(report)
}

/// Moves this connection's clock past the snapshot's newest clock (capture is installed on every
/// restore path by now) and warns when that clock is more than 24 hours ahead of local time: it is
/// restored anyway, as a changeset's would be (§6.3).
fn observe(conn: &Connection, max_hlc: u64, warnings: &mut Vec<String>) -> Result<()> {
    if let Observation::FarFuture { ahead_ms } = capture::observe(conn, max_hlc)? {
        let warning = format!(
            "the snapshot carries a clock {} minutes ahead of this device's time",
            ahead_ms / 60_000
        );
        log::warn!("[SYNC] {warning}");
        warnings.push(warning);
    }
    Ok(())
}

/// The sync row, once the snapshot is known to be one this device may restore.
fn admissible(conn: &Connection, snapshot: &Snapshot) -> Result<SyncState> {
    let sync = loaded(conn)?;
    if snapshot.schema > schema_number() {
        return Err(AppError::Validation(
            "validation.syncSnapshotNewerSchema".into(),
        ));
    }
    if snapshot.barrier < sync.barrier {
        return Err(AppError::Validation(
            "validation.syncSnapshotBelowBarrier".into(),
        ));
    }
    Ok(sync)
}

/// Seeds, heads (with their quarantine), the cursor and snapshot bookkeeping, the barrier
/// marker and a healed below-barrier miss, shared by both restores. Replacing sets them; merging never moves them back.
fn finish(
    conn: &Connection,
    snapshot: &Snapshot,
    sync: &SyncState,
    max_hlc: u64,
    replacing: bool,
) -> Result<()> {
    insert_seeds(conn, &snapshot.seeds, replacing)?;
    // A quarantine is never lifted by a snapshot: the fork stays a fork.
    let mut head = conn.prepare_cached(
        "INSERT INTO sync_heads (device, seq, hash, quarantined) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(device) DO UPDATE SET \
           hash = CASE WHEN excluded.seq > seq THEN excluded.hash ELSE hash END, \
           seq = MAX(seq, excluded.seq), \
           quarantined = MAX(quarantined, excluded.quarantined)",
    )?;
    // This device's own envelopes are judged from its own log, never from a head.
    for h in snapshot.heads.iter().filter(|h| h.device != sync.device) {
        head.execute(params![
            h.device.0.to_vec(),
            sql_int(h.seq)?,
            h.hash.to_vec(),
            h.quarantined
        ])?;
    }
    let sql = if replacing {
        "UPDATE sync_state SET cursor = ?1, snapshot_seq = ?1, snapshot_hlc = ?2 WHERE id = 1"
    } else {
        "UPDATE sync_state SET cursor = MAX(cursor, ?1), \
           snapshot_seq = MAX(COALESCE(snapshot_seq, 0), ?1), \
           snapshot_hlc = MAX(snapshot_hlc, ?2) WHERE id = 1"
    };
    conn.execute(sql, params![sql_int(snapshot.seq)?, sql_int(max_hlc)?])?;
    // The marker is a place in relay order, the same on every device, but only for the barrier
    // it marks. Replacing adopts it (or none); merging keeps a marker already found here.
    let marker = snapshot
        .barrier_seq
        .filter(|_| snapshot.barrier == sync.barrier);
    if replacing || sync.barrier_seq.is_none() {
        state::set_barrier_seq(conn, marker)?;
    }
    // A snapshot dumped past every below-barrier envelope this device skipped carries their rows
    // (ruling T8-2).
    if chain::snapshot_heals_missed(
        sync,
        snapshot.barrier,
        snapshot.seq,
        snapshot.missed_below_barrier,
    ) {
        state::clear_missed_below_barrier(conn)?;
    }
    Ok(())
}

/// Empties what a replace restores: every Synced table deepest first (the singleton row stays,
/// only the allowlisted `app_config` keys go), every Seed table, and the bookkeeping of the rows.
fn clear(conn: &Connection) -> Result<()> {
    let depth = fk_depth(conn)?;
    let mut synced: Vec<&TableSpec> = registry::synced_tables().collect();
    synced.sort_by_key(|s| (std::cmp::Reverse(depth.get(s.name).copied()), s.name));
    for spec in synced {
        match spec.key {
            KeyShape::Singleton(_) => {}
            KeyShape::Primary if spec.name == "app_config" => {
                let keys = vec!["?"; APP_CONFIG_SYNCED_KEYS.len()].join(", ");
                conn.execute(
                    &format!("DELETE FROM main.app_config WHERE key IN ({keys})"),
                    params_from_iter(APP_CONFIG_SYNCED_KEYS),
                )?;
            }
            KeyShape::Primary => {
                conn.execute(&format!("DELETE FROM main.{}", quoted(spec.name)), [])?;
            }
        }
    }
    for spec in seed_tables() {
        conn.execute(&format!("DELETE FROM main.{}", quoted(spec.name)), [])?;
    }
    conn.execute_batch(
        "DELETE FROM sync_clock; DELETE FROM sync_outbox; DELETE FROM sync_heads; \
         DELETE FROM sync_typed;",
    )?;
    Ok(())
}

/// Overwrites the synced columns of the existing singleton row with the snapshot's values and
/// clocks (its Local columns stay). `false` when there is no row: the merge inserts it.
fn overwrite_singleton(conn: &Connection, row: &RowChange) -> Result<bool> {
    let spec = registry::spec(&row.table).ok_or_else(invalid)?;
    let rowid: Option<i64> = conn
        .query_row(
            &format!(
                "SELECT rowid FROM main.{} ORDER BY rowid LIMIT 1",
                quoted(spec.name)
            ),
            [],
            |r| r.get(0),
        )
        .optional()?;
    let Some(rowid) = rowid else {
        return Ok(false);
    };
    let known = column_names(conn, spec.name)?;
    let columns: Vec<(&String, &(SqlValue, Clock))> = row
        .columns
        .iter()
        .filter(|(name, _)| {
            known.contains(*name) && column_class(spec.name, name) != ColumnClass::Local
        })
        .collect();
    if !columns.is_empty() {
        let sets = columns
            .iter()
            .enumerate()
            .map(|(i, (name, _))| format!("{} = ?{}", quoted(name), i + 2))
            .collect::<Vec<_>>()
            .join(", ");
        let values = std::iter::once(Value::Integer(rowid))
            .chain(columns.iter().map(|(_, (value, _))| value.clone().into()));
        conn.execute(
            &format!(
                "UPDATE main.{} SET {sets} WHERE rowid = ?1",
                quoted(spec.name)
            ),
            params_from_iter(values),
        )?;
    }
    let pk = row.key.to_pk_text()?;
    let clocks = std::iter::once(("*", Some(row.generation)))
        .chain(std::iter::once(("-", row.deleted)))
        .chain(
            columns
                .iter()
                .map(|(name, (_, clock))| (name.as_str(), Some(*clock))),
        );
    for (col, clock) in clocks {
        let explicit = match col {
            "*" => clock.filter(|c| *c != Clock::ZERO),
            "-" => clock,
            _ => clock.filter(|c| *c != row.generation),
        };
        if let Some(clock) = explicit {
            conn.execute(
                &format!(
                    "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES (?1, ?2, ?3, ?4, ?5) \
                     {CLOCK_UPSERT}"
                ),
                params![
                    spec.name,
                    pk,
                    col,
                    sql_int(clock.hlc)?,
                    clock.device.0.to_vec()
                ],
            )?;
        }
    }
    Ok(true)
}

/// Inserts the Seed rows: into emptied tables when replacing, else only where no row holds the
/// key already. Columns this build does not have are left out.
fn insert_seeds(
    conn: &Connection,
    seeds: &[(String, BTreeMap<String, SqlValue>)],
    replacing: bool,
) -> Result<()> {
    let mut known: HashMap<&str, Vec<String>> = HashMap::new();
    for (table, row) in seeds {
        if !known.contains_key(table.as_str()) {
            known.insert(table, column_names(conn, table)?);
        }
        let columns: Vec<(&String, &SqlValue)> = row
            .iter()
            .filter(|(name, _)| known[table.as_str()].contains(*name))
            .collect();
        if columns.is_empty() {
            continue;
        }
        let sql = format!(
            "INSERT INTO main.{} ({}) VALUES ({}){}",
            quoted(table),
            columns
                .iter()
                .map(|(name, _)| quoted(name))
                .collect::<Vec<_>>()
                .join(", "),
            vec!["?"; columns.len()].join(", "),
            if replacing {
                ""
            } else {
                " ON CONFLICT DO NOTHING"
            }
        );
        conn.prepare_cached(&sql)?.execute(params_from_iter(
            columns
                .iter()
                .map(|(_, value)| Value::from((*value).clone())),
        ))?;
    }
    Ok(())
}

/// Every key of every Synced table as `read_row` sees it: live rows by FK depth (categories
/// parent first), then tombstone-only keys children first.
fn synced_rows(conn: &Connection) -> Result<Vec<RowChange>> {
    let reader = Reader::new(conn)?;
    let depth = fk_depth(conn)?;
    let mut synced: Vec<&TableSpec> = registry::synced_tables().collect();
    synced.sort_by_key(|s| (depth.get(s.name).copied(), s.name));

    let mut live = Vec::new();
    let mut gone = Vec::new();
    for spec in &synced {
        let mut keys = live_keys(conn, spec)?;
        if spec.name == "transaction_categories" {
            let mut ordered = Vec::with_capacity(keys.len());
            for pk in keys {
                ordered.push((category_depth(conn, &pk)?, pk));
            }
            ordered.sort();
            keys = ordered.into_iter().map(|(_, pk)| pk).collect();
        }
        for pk in keys {
            if let Some(row) = reader.read(spec.name, &pk)? {
                live.push(row);
            }
        }
    }
    for spec in synced.iter().rev() {
        let mut stmt = conn
            .prepare_cached("SELECT pk FROM sync_clock WHERE tbl = ?1 AND col = '-' ORDER BY pk")?;
        let keys: Vec<String> = stmt
            .query_map([spec.name], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        for pk in keys {
            if let Some(row) = reader.read(spec.name, &pk)?.filter(|row| !row.insert) {
                gone.push(row);
            }
        }
    }
    live.extend(gone);
    Ok(live)
}

/// The key text of every live row of one Synced table, in key order (allowlisted keys only for
/// `app_config`).
fn live_keys(conn: &Connection, spec: &TableSpec) -> Result<Vec<String>> {
    let pk = schema::primary_key(conn, spec.name)?;
    let (filter, order) = match spec.key {
        KeyShape::Singleton(_) => (String::new(), "rowid LIMIT 1".to_string()),
        KeyShape::Primary if pk.is_empty() => {
            return Err(AppError::Internal(format!(
                "sync snapshot: {} has no primary key",
                spec.name
            )))
        }
        KeyShape::Primary => {
            let filter = if spec.name == "app_config" {
                format!(
                    "WHERE t.key IN ({})",
                    APP_CONFIG_SYNCED_KEYS
                        .iter()
                        .map(|k| format!("'{k}'"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            } else {
                String::new()
            };
            let order = pk
                .iter()
                .map(|c| format!("t.{}", quoted(c)))
                .collect::<Vec<_>>()
                .join(", ");
            (filter, order)
        }
    };
    let quoted_pk: Vec<String> = pk.iter().map(|c| quoted(c)).collect();
    let sql = format!(
        "SELECT CAST({} AS TEXT) FROM main.{} AS t {filter} ORDER BY {order}",
        pk_expr(spec, &quoted_pk, "t"),
        quoted(spec.name)
    );
    let mut stmt = conn.prepare(&sql)?;
    let keys = stmt
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(keys)
}

/// Every row of every Seed table, by table in registry order, then by key.
fn seed_rows(conn: &Connection) -> Result<Vec<(String, BTreeMap<String, SqlValue>)>> {
    let mut out = Vec::new();
    for spec in seed_tables() {
        let columns = column_names(conn, spec.name)?;
        let pk = schema::primary_key(conn, spec.name)?;
        let order = if pk.is_empty() {
            "rowid".to_string()
        } else {
            pk.iter().map(|c| quoted(c)).collect::<Vec<_>>().join(", ")
        };
        let sql = format!(
            "SELECT {} FROM main.{} ORDER BY {order}",
            columns
                .iter()
                .map(|c| quoted(c))
                .collect::<Vec<_>>()
                .join(", "),
            quoted(spec.name)
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |r| {
            columns
                .iter()
                .enumerate()
                .map(|(i, name)| Ok((name.clone(), SqlValue::from(r.get::<_, Value>(i)?))))
                .collect::<rusqlite::Result<BTreeMap<_, _>>>()
        })?;
        for row in rows {
            out.push((spec.name.to_string(), row?));
        }
    }
    Ok(out)
}

fn seed_tables() -> impl Iterator<Item = &'static TableSpec> {
    registry::tables()
        .iter()
        .filter(|t| t.class == TableClass::Seed)
}

/// Every `sync_heads` row with its quarantine, then this device's own head (R3) once it has sealed an envelope, so a
/// device restoring the snapshot applies the dumper's next envelope instead of seeing a gap.
fn heads_of(conn: &Connection, sync: &SyncState) -> Result<Vec<Head>> {
    let raw: Vec<(Vec<u8>, i64, Vec<u8>, bool)> = conn
        .prepare("SELECT device, seq, hash, quarantined FROM sync_heads ORDER BY device")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<std::result::Result<_, _>>()?;
    let mut heads = raw
        .into_iter()
        .map(|(device, seq, hash, quarantined)| {
            let device: [u8; 16] = device
                .try_into()
                .map_err(|_| AppError::Internal("sync_heads.device is not 16 bytes".into()))?;
            let hash: [u8; 32] = hash
                .try_into()
                .map_err(|_| AppError::Internal("sync_heads.hash is not 32 bytes".into()))?;
            let seq = u64::try_from(seq)
                .map_err(|_| AppError::Internal(format!("sync_heads.seq {seq} is negative")))?;
            Ok(Head {
                device: DeviceId(device),
                seq,
                hash,
                quarantined,
            })
        })
        .collect::<Result<Vec<Head>>>()?;
    if let (true, Some(hash)) = (sync.next_seq > 1, sync.last_hash) {
        if !heads.iter().any(|h| h.device == sync.device) {
            heads.push(Head {
                device: sync.device,
                seq: sync.next_seq - 1,
                hash,
                quarantined: false,
            });
        }
    }
    Ok(heads)
}

fn newest_clock(row: &RowChange) -> u64 {
    std::iter::once(row.generation)
        .chain(row.deleted)
        .chain(row.columns.values().map(|(_, clock)| *clock))
        .chain(row.parents.values().copied())
        .map(|clock| clock.hlc)
        .max()
        .unwrap_or(0)
}

fn stats_of(snapshot: &Snapshot) -> DumpStats {
    DumpStats {
        rows: snapshot.rows.len() as u64,
        seeds: snapshot.seeds.len() as u64,
        max_hlc: snapshot.rows.iter().map(newest_clock).max().unwrap_or(0),
    }
}

fn column_names(conn: &Connection, table: &str) -> Result<Vec<String>> {
    Ok(schema::columns(conn, table)?
        .into_iter()
        .map(|c| c.name)
        .collect())
}

fn loaded(conn: &Connection) -> Result<SyncState> {
    state::load(conn)?.ok_or_else(|| AppError::Internal("sync snapshot: sync is not set up".into()))
}

fn sql_int(value: u64) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| AppError::Internal(format!("sync snapshot: {value} is out of range")))
}

fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}

#[cfg(test)]
mod tests;
