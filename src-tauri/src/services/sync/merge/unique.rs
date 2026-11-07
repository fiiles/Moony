//! Unique keys during a merge (spec §7.3, D15, corrections 15 and 17, rulings U1–U4): an
//! incoming row that would collide with a local row on a UNIQUE index gets its outcome written
//! directly, never inserted first and fixed later.
//!
//! A collision is found by evaluating the index's expressions over the values the row will hold
//! and looking for another local row with the same key (the partial clause honoured, NULLs never
//! colliding, as in the index). While `ctx.defer` is set (the first passes over a changeset, U2)
//! a collision only defers the row: it may be transient, a row of the same changeset that moves
//! the rival away is still to come. What still collides afterwards gets its index's rule
//! (`registry::keys::unique_rules`):
//!
//! - **SameId**: the creating services derive the id from the key (U1), so two devices create the
//!   same row and merge it by column. Another id on the same key is impossible after D22; it is
//!   handled defensively as keep-newest, with a warning: each side's key clock is
//!   `max('*', clocks of the key columns)` — the clock that put the row on its key (correction 15)
//!   — the newest wins, ties by the lowest primary key, and the loser is deleted at the winner's
//!   key clock (an incoming loser is dropped and its key tombstoned the same way).
//! - **Rename** and **Unlink** are derived (U3): the incoming row is written with a value that
//!   cannot collide (the placeholder name `"\u{1f}" + pk`, or no link) and, once it is written,
//!   the scope is recomputed from the typed values (`renames::recompute_scope`,
//!   `links::recompute`). The merge records the typed value in `sync_typed` (`row`).
//!
//! Every row an outcome changes goes into `ctx.touched`. The deletes go through
//! `parents::delete_row`, so a loser's cascade is tombstoned at the same clock.

use rusqlite::types::Value;
use rusqlite::Connection;

use super::parents;
use super::row::{key_text, quoted, read_clocks, Columns, MergeCtx, RowImage, Table, ROW};
use crate::error::{AppError, Result};
use crate::services::sync::capture::{self, Mode};
use crate::services::sync::changeset::{RowKey, SqlValue};
use crate::services::sync::hlc::Clock;
use crate::services::sync::registry::keys::{unique_rules, UniqueRule, UniqueSpec};
use crate::services::sync::repair::{links, renames, tombstone_local};

/// What an incoming row writes once its unique keys are resolved.
#[derive(Debug)]
pub enum Resolution {
    /// Write these columns (possibly with a placeholder name or without a link).
    Write(Columns),
    /// The row lost its key: it is not written, its key is tombstoned (a live one deleted).
    Drop,
    /// The row collides and `ctx.defer` is set: nothing is written, the caller retries it later.
    Defer,
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
    let mut values = values.clone();
    if !has_rules(table) {
        return Ok(Resolution::Write(values));
    }
    let shape = ctx.shapes.table(conn, table)?;
    let mut collisions = Vec::new();
    for rule in rules_of(table) {
        let rivals = rivals(conn, &shape, rule, incoming_pk, &values)?;
        if !rivals.is_empty() {
            collisions.push((rule, rivals));
        }
    }
    if collisions.is_empty() {
        return Ok(Resolution::Write(values));
    }
    if ctx.defer {
        return Ok(Resolution::Defer);
    }
    for (rule, rivals) in collisions {
        match rule.rule {
            UniqueRule::SameId => {
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
                let incoming = (incoming_pk, &values, generation);
                if !keep_newest(conn, ctx, &shape, rule, incoming, &rivals)? {
                    return Ok(Resolution::Drop);
                }
            }
            UniqueRule::Rename => {
                let scope = renames::scope_of(table)?;
                let Some((_, clock)) = values.get(scope.name_column).cloned() else {
                    return Err(AppError::Internal(format!(
                        "sync merge: {table} {incoming_pk} collides on a name it does not carry"
                    )));
                };
                let placeholder = SqlValue::Text(format!("\u{1f}{incoming_pk}"));
                values.insert(scope.name_column.to_string(), (placeholder, clock));
                let scope_value = match scope.scope_column {
                    None => String::new(),
                    Some(column) => values
                        .get(column)
                        .and_then(|(value, _)| key_text(value))
                        .unwrap_or_default(),
                };
                ctx.recompute(scope.table, scope_value);
            }
            UniqueRule::Unlink => {
                for column in rule.key_columns {
                    if let Some(entry) = values.get_mut(*column) {
                        entry.0 = SqlValue::Null;
                    }
                }
                ctx.recompute(rule.table, String::new());
            }
        }
    }
    Ok(Resolution::Write(values))
}

/// Runs after the incoming row is written and its typed values recorded: recomputes every scope
/// a rename or unlink wrote a stand-in value in, and records the rows whose shown value changed.
pub(super) fn finish(conn: &Connection, ctx: &mut MergeCtx) -> Result<()> {
    for (table, scope_value) in std::mem::take(&mut ctx.rescopes) {
        let link = links::LINKS.iter().find(|(t, _)| *t == table);
        let (column, scope_column) = match link {
            Some((_, column)) => (*column, None),
            None => {
                let scope = renames::scope_of(table)?;
                (scope.name_column, scope.scope_column)
            }
        };
        let before = shown(conn, table, column, scope_column, &scope_value)?;
        capture::with_mode(conn, Mode::Repair, || match link {
            Some(_) => links::recompute(conn),
            None => renames::recompute_scope(conn, renames::scope_of(table)?, &scope_value),
        })?;
        for row in shown(conn, table, column, scope_column, &scope_value)? {
            if !before.contains(&row) {
                ctx.touched.row(table, &row.0);
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

/// `(pk, shown value)` of every row of one scope.
fn shown(
    conn: &Connection,
    table: &str,
    column: &str,
    scope_column: Option<&str>,
    scope_value: &str,
) -> Result<Vec<(String, Option<String>)>> {
    let map = |r: &rusqlite::Row| Ok((r.get(0)?, r.get(1)?));
    let rows = match scope_column {
        Some(scope) => conn
            .prepare(&format!(
                "SELECT id, {column} FROM {table} WHERE {scope} = ?1"
            ))?
            .query_map([scope_value], map)?
            .collect::<std::result::Result<_, _>>()?,
        None => conn
            .prepare(&format!("SELECT id, {column} FROM {table}"))?
            .query_map([], map)?
            .collect::<std::result::Result<_, _>>()?,
    };
    Ok(rows)
}
