//! Unique keys during a merge (spec §7.3, D15, corrections 15 and 17): an incoming row that would
//! collide with a local row on a UNIQUE index gets its outcome written directly, never inserted
//! first and fixed later.
//!
//! A collision is found by evaluating the index's expressions over the values the row will hold
//! and looking for another local row with the same key (the partial clause honoured, NULLs never
//! colliding, as in the index). Each index has one rule (`registry::keys::unique_rules`):
//!
//! - **SameId**: the creating services derive the id from the key, so two devices create the same
//!   row; another id on the same key is impossible after D22 and is handled as KeepNewest, with a
//!   warning.
//! - **KeepNewest** (correction 15, R10): each side's key clock is `max('*', clocks of the key
//!   columns)` — the clock that put the row on its key, not an edit of another column. The newest
//!   key clock wins, ties by the lowest primary key. A losing local row is deleted at the winner's
//!   key clock (its delete epoch ends there); a losing incoming row is dropped and its key
//!   tombstoned the same way (a live one deleted).
//! - **Rename**: the incoming row is written under the placeholder name `"\u{1f}" + pk`, its typed
//!   name kept in `sync_renamed`, and once it is written `renames::recompute_scope` assigns every
//!   name of the scope (the one rename function).
//! - **Unlink**: among the rows linked to the same source, every one but the lowest id loses the
//!   link: the local rows (in `repair` mode), or the incoming value.
//!
//! Every row an outcome changes goes into `ctx.touched`. The deletes go through
//! `parents::delete_row`, so a loser's cascade is tombstoned at the same clock.

use rusqlite::types::Value;
use rusqlite::{params, Connection};

use super::parents;
use super::row::{key_text, note, quoted, read_clocks, Columns, MergeCtx, RowImage, Table, ROW};
use crate::error::{AppError, Result};
use crate::services::sync::capture::{self, Mode};
use crate::services::sync::changeset::{RowKey, SqlValue};
use crate::services::sync::hlc::Clock;
use crate::services::sync::registry::keys::{unique_rules, UniqueRule, UniqueSpec};
use crate::services::sync::repair::{renames, tombstone_local};

/// What an incoming row writes once its unique keys are resolved.
#[derive(Debug)]
pub enum Resolution {
    /// Write these columns (possibly unlinked, or with a placeholder name).
    Write(Columns),
    /// The row lost its key: it is not written, its key is tombstoned (a live one deleted).
    Drop,
}

/// Whether `table` has a unique key with a rule.
pub fn has_rules(table: &str) -> bool {
    rules_of(table).next().is_some()
}

/// Whether `column` of `table` is read by one of the table's unique keys.
pub fn guards(table: &str, column: &str) -> bool {
    rules_of(table).any(|rule| rule.key_columns.contains(&column))
}

/// Resolves every unique key of `table` for the incoming row `incoming_pk`, whose columns will
/// hold `values` (value and clock of each column) under the row clock `generation`. Runs in
/// `merge` mode before the row is written; the outcomes for local rows are written here.
pub fn resolve(
    conn: &Connection,
    table: &str,
    incoming_pk: &str,
    values: &Columns,
    generation: Clock,
    ctx: &mut MergeCtx,
) -> Result<Resolution> {
    if !has_rules(table) {
        return Ok(Resolution::Write(values.clone()));
    }
    let shape = ctx.shapes.table(conn, table)?;
    let mut values = values.clone();
    for rule in rules_of(table) {
        let rivals = rivals(conn, &shape, rule, incoming_pk, &values)?;
        if rivals.is_empty() {
            continue;
        }
        let kept = match rule.rule {
            UniqueRule::SameId | UniqueRule::KeepNewest => {
                if rule.rule == UniqueRule::SameId {
                    let warning = format!(
                        "{table}: {incoming_pk} and {} share a natural key under different ids; \
                         kept the row whose key is newest",
                        rivals
                            .iter()
                            .map(|(pk, _)| pk.as_str())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                    log::warn!("[SYNC] {warning}");
                    ctx.report.warnings.push(warning);
                }
                let incoming = (incoming_pk, &values, generation);
                keep_newest(conn, ctx, &shape, rule, incoming, &rivals)?
            }
            UniqueRule::Rename => {
                rename(conn, ctx, table, incoming_pk, &mut values)?;
                true
            }
            UniqueRule::Unlink => {
                unlink(conn, ctx, &shape, rule, incoming_pk, &mut values, &rivals)?;
                true
            }
        };
        if !kept {
            return Ok(Resolution::Drop);
        }
    }
    Ok(Resolution::Write(values))
}

/// Runs after the incoming row is written: recomputes the names of every scope a rename left a
/// placeholder in, and records the rows whose names changed.
pub(super) fn finish(conn: &Connection, ctx: &mut MergeCtx) -> Result<()> {
    for (table, scope_value) in std::mem::take(&mut ctx.renames) {
        let scope = renames::scope_of(table)?;
        let before = names(conn, scope, &scope_value)?;
        capture::with_mode(conn, Mode::Repair, || {
            renames::recompute_scope(conn, scope, &scope_value)
        })?;
        for (pk, name) in names(conn, scope, &scope_value)? {
            if !before.contains(&(pk.clone(), name)) {
                ctx.touched.row(table, &pk);
            }
        }
    }
    Ok(())
}

fn rules_of(table: &str) -> impl Iterator<Item = &'static UniqueSpec> + '_ {
    unique_rules()
        .iter()
        .filter(move |rule| rule.table == table)
}

/// The local rows other than `incoming_pk` holding the key `values` evaluate to under `rule`;
/// none when the partial clause excludes the incoming row or a key part is NULL.
fn rivals(
    conn: &Connection,
    table: &Table,
    rule: &UniqueSpec,
    incoming_pk: &str,
    values: &Columns,
) -> Result<Vec<(String, RowImage)>> {
    let carried: Vec<Value> = rule
        .key_columns
        .iter()
        .map(|column| {
            values
                .get(*column)
                .map_or(Value::Null, |(value, _)| value.clone().into())
        })
        .collect();
    let source = rule
        .key_columns
        .iter()
        .enumerate()
        .map(|(i, column)| format!("?{} AS {}", i + 1, quoted(column)))
        .collect::<Vec<_>>()
        .join(", ");
    let width = rule.exprs.len();
    let evaluated: Vec<Value> = conn
        .prepare_cached(&format!(
            "SELECT {}, ({}) FROM (SELECT {source})",
            rule.exprs.join(", "),
            rule.partial.unwrap_or("1")
        ))?
        .query_row(rusqlite::params_from_iter(carried), |r| {
            (0..=width).map(|i| r.get::<_, Value>(i)).collect()
        })?;
    let (key, applies) = evaluated.split_at(width);
    let indexed = matches!(applies, [Value::Integer(n)] if *n != 0);
    if !indexed || key.contains(&Value::Null) {
        return Ok(Vec::new());
    }

    let mut conditions: Vec<String> = rule
        .exprs
        .iter()
        .enumerate()
        .map(|(i, expr)| format!("({expr}) = ?{}", i + 1))
        .collect();
    if let Some(partial) = rule.partial {
        conditions.push(format!("({partial})"));
    }
    let mut params = key.to_vec();
    let own = table.key_params(&RowKey::from_pk_text(table.spec, incoming_pk)?)?;
    if !own.is_empty() {
        conditions.push(format!(
            "NOT ({})",
            table
                .key
                .iter()
                .enumerate()
                .map(|(i, column)| format!("{} = ?{}", quoted(column), width + i + 1))
                .collect::<Vec<_>>()
                .join(" AND ")
        ));
        params.extend(own.into_iter().map(Value::Text));
    }
    table.select_matching(conn, &conditions.join(" AND "), params)
}

/// The clock that put a local row on its key: `max('*', clocks of the key columns)`.
fn local_key_clock(conn: &Connection, table: &str, rule: &UniqueSpec, pk: &str) -> Result<Clock> {
    let clocks = read_clocks(conn, table, pk)?;
    let star = clocks.get(ROW).copied().unwrap_or(Clock::ZERO);
    Ok(rule
        .key_columns
        .iter()
        .filter_map(|column| clocks.get(*column).copied())
        .fold(star, Clock::max))
}

/// The incoming row's key clock: `max(generation, carried clocks of the key columns)`.
fn incoming_key_clock(rule: &UniqueSpec, values: &Columns, generation: Clock) -> Clock {
    rule.key_columns
        .iter()
        .filter_map(|column| values.get(*column).map(|(_, clock)| *clock))
        .fold(generation, Clock::max)
}

/// Keeps the row with the newest key clock (ties: the lowest pk) and deletes the others at that
/// clock. Returns whether the incoming row is the one kept.
/// `incoming` is the incoming row's pk, values and row clock.
fn keep_newest(
    conn: &Connection,
    ctx: &mut MergeCtx,
    table: &Table,
    rule: &UniqueSpec,
    (incoming_pk, values, generation): (&str, &Columns, Clock),
    rivals: &[(String, RowImage)],
) -> Result<bool> {
    let name = table.spec.name;
    let mut contenders = vec![(incoming_key_clock(rule, values, generation), incoming_pk)];
    for (pk, _) in rivals {
        contenders.push((local_key_clock(conn, name, rule, pk)?, pk.as_str()));
    }
    let (winner_clock, winner_pk) = contenders
        .iter()
        .copied()
        .max_by(|a, b| a.0.cmp(&b.0).then_with(|| b.1.cmp(a.1)))
        .ok_or_else(|| AppError::Internal(format!("sync merge: {name} has no contender")))?;
    for (pk, image) in rivals {
        if pk != winner_pk {
            parents::delete_row(conn, ctx, table, pk, image, winner_clock)?;
        }
    }
    if winner_pk == incoming_pk {
        return Ok(true);
    }
    match table.read(conn, &RowKey::from_pk_text(table.spec, incoming_pk)?)? {
        Some(live) => parents::delete_row(conn, ctx, table, incoming_pk, &live, winner_clock)?,
        None => tombstone_local(conn, name, incoming_pk, winner_clock)?,
    }
    Ok(false)
}

/// Gives the incoming row its placeholder name and records the typed one; `finish` assigns the
/// scope's names once the row is written.
fn rename(
    conn: &Connection,
    ctx: &mut MergeCtx,
    table: &str,
    incoming_pk: &str,
    values: &mut Columns,
) -> Result<()> {
    let scope = renames::scope_of(table)?;
    let Some((SqlValue::Text(typed), clock)) = values.get(scope.name_column).cloned() else {
        return Err(AppError::Internal(format!(
            "sync merge: {table} {incoming_pk} collides on a name it does not carry"
        )));
    };
    let placeholder = format!("\u{1f}{incoming_pk}");
    values.insert(
        scope.name_column.to_string(),
        (SqlValue::Text(placeholder.clone()), clock),
    );
    conn.execute(
        "INSERT INTO sync_renamed (tbl, pk, typed, assigned) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(tbl, pk) DO UPDATE SET typed = excluded.typed, assigned = excluded.assigned",
        params![table, incoming_pk, typed, placeholder],
    )?;
    let scope_value = match scope.scope_column {
        None => String::new(),
        Some(column) => values
            .get(column)
            .and_then(|(value, _)| key_text(value))
            .unwrap_or_default(),
    };
    if !ctx.renames.contains(&(scope.table, scope_value.clone())) {
        ctx.renames.push((scope.table, scope_value));
    }
    Ok(())
}

/// Among the incoming row and its rivals, every one but the lowest pk loses the link: the
/// incoming value is cleared (keeping its clock), or the local rows are cleared in `repair` mode.
fn unlink(
    conn: &Connection,
    ctx: &mut MergeCtx,
    table: &Table,
    rule: &UniqueSpec,
    incoming_pk: &str,
    values: &mut Columns,
    rivals: &[(String, RowImage)],
) -> Result<()> {
    let lowest = rivals
        .iter()
        .map(|(pk, _)| pk.as_str())
        .chain([incoming_pk])
        .min();
    if lowest != Some(incoming_pk) {
        for column in rule.key_columns {
            if let Some(entry) = values.get_mut(*column) {
                entry.0 = SqlValue::Null;
            }
        }
        return Ok(());
    }
    let sets = rule
        .key_columns
        .iter()
        .map(|column| format!("{} = NULL", quoted(column)))
        .collect::<Vec<_>>()
        .join(", ");
    let graph = ctx.shapes.graph(conn)?;
    for (pk, old) in rivals {
        let key = RowKey::from_pk_text(table.spec, pk)?;
        let (filter, params) = table.filter(&key)?;
        capture::with_mode(conn, Mode::Repair, || {
            conn.execute(
                &format!("UPDATE {} SET {sets} {filter}", quoted(table.spec.name)),
                rusqlite::params_from_iter(params),
            )?;
            Ok(())
        })?;
        let new = table.read(conn, &key)?;
        note(&mut ctx.touched, &graph, table, pk, Some(old), new.as_ref());
    }
    Ok(())
}

/// `(pk, name)` of every row of one rename scope.
fn names(
    conn: &Connection,
    scope: &renames::RenameScope,
    scope_value: &str,
) -> Result<Vec<(String, String)>> {
    let (table, name) = (scope.table, scope.name_column);
    let map = |r: &rusqlite::Row| Ok((r.get(0)?, r.get(1)?));
    let rows = match scope.scope_column {
        Some(column) => conn
            .prepare(&format!(
                "SELECT id, {name} FROM {table} WHERE {column} = ?1"
            ))?
            .query_map([scope_value], map)?
            .collect::<std::result::Result<_, _>>()?,
        None => conn
            .prepare(&format!("SELECT id, {name} FROM {table}"))?
            .query_map([], map)?
            .collect::<std::result::Result<_, _>>()?,
    };
    Ok(rows)
}
