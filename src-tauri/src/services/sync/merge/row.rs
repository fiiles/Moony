//! One row change (spec §7.1, correction 10, the R2 amendment). With `L` the local row clock
//! (`'*'`, ZERO for a row without one), `X` the local tombstone (`'-'`) and `g` the change's
//! generation:
//!
//! 1. an insert with `g` older than the newest delete of its key (`X`, or the change's own `del`)
//!    belongs to a delete epoch that has ended: it is dropped, without asking its parents;
//! 2. otherwise its foreign keys are checked (`parents::check`): a reference into a deleted parent
//!    epoch follows the key's `ON DELETE` rule (a CASCADE child becomes a tombstone at the
//!    parent's clock, D14, T18d); a parent never seen stops the merge;
//! 3. a carried tombstone newer than `L` deletes the live row (cascading at that clock), and `X`
//!    becomes the newest tombstone;
//! 4. a row that is not live is inserted with `'*' = g` and explicit clocks for the columns whose
//!    clock differs; a live row merges per column (`columns::merge`), `'*'` becomes `max(L, g)`.
//!    Before the INSERT, and before an UPDATE that writes a column of a unique key,
//!    `unique::resolve` decides what the row may write (§7.3), so no write trips a UNIQUE
//!    constraint.
//!
//! For a live row the column merge runs before the parent check, which then sees only the FK
//! values that won (C1).
//!
//! The local row is read before every write, so `Touched` gets the old and the new values (R12).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::rc::Rc;

use rusqlite::types::Value;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};

use super::columns;
use super::parents::{self, Graph, Parents};
use super::unique::{self, Resolution};
use super::{ApplyReport, StopReason};
use crate::error::{AppError, Result};
use crate::services::sync::capture::CLOCK_UPSERT;
use crate::services::sync::changeset::{RowChange, RowKey, SqlValue};
use crate::services::sync::hlc::{Clock, DeviceId};
use crate::services::sync::identity;
use crate::services::sync::registry::keys::OnDelete;
use crate::services::sync::registry::{
    self, column_class, schema, ColumnClass, KeyShape, TableClass, TableSpec,
    APP_CONFIG_SYNCED_KEYS,
};
use crate::services::sync::repair::{
    domain_of, renames::RENAME_SCOPES, set_typed, shadowed_columns, Touched,
};

/// The row clock (the generation).
pub(super) const ROW: &str = "*";
/// The tombstone.
pub(super) const TOMBSTONE: &str = "-";

/// A row's values by column name.
pub(super) type RowImage = BTreeMap<String, SqlValue>;

/// Carried columns with their values and clocks.
pub(super) type Columns = BTreeMap<String, (SqlValue, Clock)>;

/// What one merge has done so far, and the table shapes it looked up.
#[derive(Default)]
pub struct MergeCtx {
    pub touched: Touched,
    pub report: ApplyReport,
    pub(super) shapes: Shapes,
    /// Rename and unlink scopes a unique-key outcome wrote a stand-in value in,
    /// `(table, scope value)`; recomputed right after the row is written (`unique::finish`).
    pub(super) rescopes: Vec<(&'static str, String)>,
    /// While set, a row that collides on a unique key is deferred instead of resolved (U2).
    pub(super) defer: bool,
}

impl MergeCtx {
    /// Queues one rename or unlink scope for `unique::finish`.
    pub(super) fn recompute(&mut self, table: &'static str, scope_value: String) {
        if !self.rescopes.contains(&(table, scope_value.clone())) {
            self.rescopes.push((table, scope_value));
        }
    }
}

#[derive(Debug)]
pub enum RowResult {
    Applied,
    /// The insert did not land (its delete epoch ended, or its parent's did, or it lost its
    /// unique key).
    Dropped,
    /// The row collides on a unique key while `MergeCtx::defer` is set; nothing of it was written
    /// except a carried tombstone, so applying it again later is safe (U2).
    Deferred,
    Stop(StopReason),
}

/// Applies one row change. The caller has set `merge` mode and deferred the FK checks.
pub fn apply_row(conn: &Connection, change: &RowChange, ctx: &mut MergeCtx) -> Result<RowResult> {
    let table = ctx.shapes.table(conn, &change.table)?;
    let pk = change.key.to_pk_text()?;
    if table.spec.name == "app_config" && !APP_CONFIG_SYNCED_KEYS.contains(&pk.as_str()) {
        return Ok(RowResult::Dropped);
    }
    let clocks = read_clocks(conn, table.spec.name, &pk)?;
    let generation = clocks.get(ROW).copied().unwrap_or(Clock::ZERO);
    let mut live = table.read(conn, &change.key)?;
    let mut columns = table.known(&change.columns);
    let mut deleted = change.deleted;

    let epoch_end = clocks.get(TOMBSTONE).copied().max(change.deleted);
    let mut insert = change.insert && epoch_end.is_none_or(|x| change.generation >= x);
    let mut dropped = change.insert && !insert;
    // The row the insert lands in: a fresh one when the row is not live or the change's own
    // tombstone deletes it first, else the live row, merged per column.
    let fresh = live.is_none() || change.deleted.is_some_and(|at| at > generation);
    let mut merged = None;
    if insert {
        // Parent rules apply only to the FK values that end up in the row (C1): every carried
        // value of a fresh row, only the winning values of a merged one (key parts always). A
        // stale reference that loses the column merge must not delete or clear anything.
        let verdict = match live.as_ref().filter(|_| !fresh) {
            None => parents::check(conn, ctx, &table, change, &pk, &mut columns)?,
            Some(old) => {
                let vault = (table.spec.name == "sync_devices")
                    .then(|| identity::bound_vault(conn))
                    .flatten();
                let mut m = columns::merge(
                    &table,
                    change,
                    &columns,
                    old,
                    &clocks,
                    generation,
                    &pk,
                    vault.as_ref(),
                );
                let verdict = parents::check(conn, ctx, &table, change, &pk, &mut m.writes)?;
                merged = Some(m);
                verdict
            }
        };
        match verdict {
            Parents::Ok => {}
            Parents::Tombstone(at) => {
                insert = false;
                dropped = true;
                deleted = deleted.max(Some(at));
            }
            Parents::Stop(reason) => return Ok(RowResult::Stop(reason)),
        }
    }

    if let Some(at) = deleted {
        if let Some(old) = live.as_ref().filter(|_| at > generation) {
            parents::delete_row(conn, ctx, &table, &pk, old, at)?;
            live = None;
        }
        raise_clock(conn, table.spec.name, &pk, TOMBSTONE, at)?;
    }
    if !insert {
        return Ok(if dropped {
            RowResult::Dropped
        } else {
            RowResult::Applied
        });
    }
    let typed;
    match (live, merged) {
        (None, _) => {
            typed = shadowed(&table, &columns);
            if unique::has_rules(table.spec.name) {
                let resolution =
                    unique::resolve(conn, table.spec.name, &pk, &columns, change.generation, ctx)?;
                columns = match resolution {
                    Resolution::Write(columns) => columns,
                    Resolution::Drop => return Ok(RowResult::Dropped),
                    Resolution::Defer => return Ok(RowResult::Deferred),
                };
            }
            insert_row(conn, ctx, &table, change, &pk, &columns)?
        }
        (Some(old), Some(mut merged)) => {
            typed = shadowed(&table, &merged.writes);
            if merged
                .writes
                .keys()
                .any(|column| unique::guards(table.spec.name, column))
            {
                let star = generation.max(change.generation);
                let before = resulting(&table, &old, &clocks, generation, &merged.writes);
                match unique::resolve(conn, table.spec.name, &pk, &before, star, ctx)? {
                    Resolution::Write(after) => {
                        for (column, entry) in after {
                            if before.get(&column) != Some(&entry) {
                                merged.writes.insert(column, entry);
                            }
                        }
                    }
                    Resolution::Drop => return Ok(RowResult::Dropped),
                    Resolution::Defer => return Ok(RowResult::Deferred),
                }
            }
            update_row(conn, ctx, &table, change, &pk, merged, &clocks, old)?
        }
        (Some(_), None) => {
            return Err(AppError::Internal(format!(
                "sync merge: {} {pk} is live but was not merged",
                table.spec.name
            )))
        }
    }
    record_typed(conn, &table, &change.key, &pk, &typed)?;
    unique::finish(conn, ctx)?;
    Ok(RowResult::Applied)
}

/// Whether the insert `change` references, through a foreign key (a carried value or a key
/// part), a row in `held` (`(table, pk text)`).
pub(super) fn references(
    conn: &Connection,
    ctx: &mut MergeCtx,
    change: &RowChange,
    held: &BTreeSet<(String, String)>,
) -> Result<bool> {
    if held.is_empty() {
        return Ok(false);
    }
    let table = ctx.shapes.table(conn, &change.table)?;
    let graph = ctx.shapes.graph(conn)?;
    let key_parts = table.key_params(&change.key)?;
    for fk in graph.of_child(table.spec.name) {
        let value = match change.columns.get(&fk.column) {
            Some((value, _)) => key_text(value),
            None => table
                .key
                .iter()
                .position(|k| *k == fk.column)
                .and_then(|i| key_parts.get(i).cloned()),
        };
        if let Some(value) = value {
            if held.contains(&(fk.parent.clone(), value)) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The written columns a repair may show differently (`repair::shadowed_columns`), with the value
/// the change carries: the typed value (U4).
fn shadowed(table: &Table, written: &Columns) -> Vec<(&'static str, SqlValue)> {
    shadowed_columns(table.spec.name)
        .into_iter()
        .filter_map(|column| Some((column, written.get(column)?.0.clone())))
        .collect()
}

/// Records, after the write, what each shadowed column was typed as against what it now holds
/// (`sync_typed`), so the scope's recompute and the journal see the typed value. A write that
/// landed as typed clears an older entry.
fn record_typed(
    conn: &Connection,
    table: &Table,
    key: &RowKey,
    pk: &str,
    typed: &[(&'static str, SqlValue)],
) -> Result<()> {
    if typed.is_empty() {
        return Ok(());
    }
    let Some(stored) = table.read(conn, key)? else {
        return Ok(());
    };
    for (column, value) in typed {
        set_typed(
            conn,
            table.spec.name,
            pk,
            column,
            text_of(value),
            stored.get(*column).and_then(text_of),
        )?;
    }
    Ok(())
}

fn text_of(value: &SqlValue) -> Option<&str> {
    match value {
        SqlValue::Text(text) => Some(text),
        _ => None,
    }
}

/// Every clocked column of a live row as it will be once `writes` land: the written value and
/// clock, else the local value with its column clock (the row clock `generation` without one).
fn resulting(
    table: &Table,
    old: &RowImage,
    clocks: &HashMap<String, Clock>,
    generation: Clock,
    writes: &Columns,
) -> Columns {
    table
        .clocked()
        .filter_map(|name| {
            let entry = writes.get(name).cloned().or_else(|| {
                old.get(name).map(|value| {
                    let clock = clocks.get(name).copied().unwrap_or(generation);
                    (value.clone(), clock)
                })
            })?;
            Some((name.to_string(), entry))
        })
        .collect()
}

/// The row is not live: every carried column goes in with `'*' = g`. The `'*'` and column clocks
/// a dead incarnation left behind are meaningless (I1) and are replaced.
fn insert_row(
    conn: &Connection,
    ctx: &mut MergeCtx,
    table: &Table,
    change: &RowChange,
    pk: &str,
    columns: &Columns,
) -> Result<()> {
    conn.prepare_cached(
        "DELETE FROM sync_clock WHERE tbl = ?1 AND pk = ?2 AND col NOT IN ('*', '-')",
    )?
    .execute(params![table.spec.name, pk])?;
    let mut names: Vec<&str> = table.key.iter().map(String::as_str).collect();
    let mut values: Vec<Value> = table
        .key_params(&change.key)?
        .into_iter()
        .map(Value::Text)
        .collect();
    for (name, (value, _)) in columns {
        names.push(name);
        values.push(value.clone().into());
    }
    let sql = if names.is_empty() {
        format!("INSERT INTO {} DEFAULT VALUES", quoted(table.spec.name))
    } else {
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            quoted(table.spec.name),
            names
                .iter()
                .map(|n| quoted(n))
                .collect::<Vec<_>>()
                .join(", "),
            (1..=names.len())
                .map(|i| format!("?{i}"))
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    conn.execute(&sql, params_from_iter(values))?;

    let star = change.generation;
    set_clock(conn, table.spec.name, pk, ROW, nonzero(star))?;
    for (name, (value, clock)) in columns {
        let weak = columns::is_weak_incoming(table.spec.name, name, value, *clock, change);
        let explicit = columns::explicit_clock(table.spec.name, name, value, *clock, star, weak);
        set_clock(conn, table.spec.name, pk, name, explicit)?;
    }
    let new = table.read(conn, &change.key)?;
    let graph = ctx.shapes.graph(conn)?;
    note(&mut ctx.touched, &graph, table, pk, None, new.as_ref());
    Ok(())
}

/// The row is live and `merged` holds what the incoming columns won against their local clocks
/// (a column clock, else `L`): those are written, `'*'` becomes `max(L, g)`, and then every
/// column's explicit clock follows from its resulting clock against the new `'*'`.
#[allow(clippy::too_many_arguments)]
fn update_row(
    conn: &Connection,
    ctx: &mut MergeCtx,
    table: &Table,
    change: &RowChange,
    pk: &str,
    merged: columns::Merged,
    clocks: &HashMap<String, Clock>,
    old: RowImage,
) -> Result<()> {
    let generation = clocks.get(ROW).copied().unwrap_or(Clock::ZERO);
    ctx.report.warnings.extend(merged.warnings);
    if !merged.writes.is_empty() {
        let (filter, mut values) = table.filter(&change.key)?;
        let offset = values.len();
        let sets = merged
            .writes
            .keys()
            .enumerate()
            .map(|(i, name)| format!("{} = ?{}", quoted(name), offset + i + 1))
            .collect::<Vec<_>>()
            .join(", ");
        values.extend(merged.writes.values().map(|(v, _)| v.clone().into()));
        conn.execute(
            &format!("UPDATE {} SET {sets} {filter}", quoted(table.spec.name)),
            params_from_iter(values),
        )?;
    }

    let star = generation.max(change.generation);
    if star != generation {
        set_clock(conn, table.spec.name, pk, ROW, Some(star))?;
    }
    for name in table.clocked() {
        let (value, clock, weak) = match merged.writes.get(name) {
            Some((value, clock)) => (value, *clock, false),
            None => {
                let Some(value) = old.get(name) else {
                    continue;
                };
                let clock = clocks.get(name).copied().unwrap_or(generation);
                let weak = columns::is_weak_local(table.spec.name, name, value, clocks);
                (value, clock, weak)
            }
        };
        let explicit = columns::explicit_clock(table.spec.name, name, value, clock, star, weak);
        if clocks.get(name).copied() != explicit {
            set_clock(conn, table.spec.name, pk, name, explicit)?;
        }
    }

    let new = table.read(conn, &change.key)?;
    if new.as_ref() != Some(&old) {
        let graph = ctx.shapes.graph(conn)?;
        note(
            &mut ctx.touched,
            &graph,
            table,
            pk,
            Some(&old),
            new.as_ref(),
        );
    }
    Ok(())
}

/// Records one written (`new` is `Some`) or deleted row in `touched`, as `Touched` documents it:
/// the row, the old and new parents, tickers, budget keys and rename scopes, and the history day
/// of its domain (a written row with a date column: the earlier of its old and new date; a
/// deletion or a row without a date column: from the first fact, as for the domains of the
/// deleted table's CASCADE children).
pub(super) fn note(
    touched: &mut Touched,
    graph: &Graph,
    table: &Table,
    pk: &str,
    old: Option<&RowImage>,
    new: Option<&RowImage>,
) {
    let name = table.spec.name;
    touched.row(name, pk);
    let images: Vec<&RowImage> = old.into_iter().chain(new).collect();
    let scope = RENAME_SCOPES.iter().find(|s| s.table == name);
    for image in &images {
        for fk in graph.of_child(name) {
            if let Some(id) = image.get(&fk.column).and_then(key_text) {
                touched.parent(&fk.parent, &id);
            }
        }
        if let Some(ticker) = image.get("ticker").and_then(key_text) {
            touched.tickers.insert(ticker);
        }
        if name == "budget_goal_versions" {
            if let (Some(category), Some(timeframe)) = (
                image.get("category_id").and_then(key_text),
                image.get("timeframe").and_then(key_text),
            ) {
                touched.budget_keys.insert((category, timeframe));
            }
        }
        if let Some(scope) = scope {
            let value = match scope.scope_column {
                None => Some(String::new()),
                Some(column) => image.get(column).and_then(key_text),
            };
            if let Some(value) = value {
                touched.rename_scopes.insert((name.to_string(), value));
            }
        }
    }
    let domain = domain_of(name);
    if new.is_none() {
        touched.mark_deleted(name);
        if let Some((domain, _)) = domain {
            touched.day(domain, None);
        }
        for fk in graph.of_parent(name) {
            if fk.on_delete == OnDelete::Cascade {
                if let Some((domain, _)) = domain_of(&fk.child) {
                    touched.day(domain, None);
                }
            }
        }
    } else if let Some((domain, date)) = domain {
        let day = date.and_then(|column| {
            images
                .iter()
                .filter_map(|image| match image.get(column) {
                    Some(SqlValue::Integer(day)) => Some(*day),
                    _ => None,
                })
                .min()
        });
        touched.day(domain, day);
    }
}

/// A key or FK value as the text `sync_clock.pk` and the journal use; `None` for NULL.
pub(super) fn key_text(value: &SqlValue) -> Option<String> {
    match value {
        SqlValue::Text(text) => Some(text.clone()),
        SqlValue::Integer(i) => Some(i.to_string()),
        SqlValue::Null | SqlValue::Real(_) | SqlValue::Blob(_) => None,
    }
}

/// The shape of one Synced table as the merge needs it.
pub(super) struct Table {
    pub spec: &'static TableSpec,
    /// The columns of the logical key (none for a singleton).
    pub key: Vec<String>,
    /// The primary key columns (the singleton's local `id` included).
    primary: Vec<String>,
    columns: Vec<schema::ColumnInfo>,
}

impl Table {
    fn load(conn: &Connection, name: &str) -> Result<Self> {
        let spec = registry::spec(name)
            .filter(|s| s.class == TableClass::Synced)
            .ok_or_else(|| AppError::Internal(format!("sync merge: {name} is not synced")))?;
        let primary = schema::primary_key(conn, spec.name)?;
        let key = match spec.key {
            KeyShape::Singleton(_) => Vec::new(),
            KeyShape::Primary => primary.clone(),
        };
        Ok(Self {
            spec,
            key,
            primary,
            columns: schema::columns(conn, spec.name)?,
        })
    }

    pub fn has(&self, column: &str) -> bool {
        self.columns.iter().any(|c| c.name == column)
    }

    /// The columns that travel and carry clocks: not part of the primary key, not Local.
    pub fn clocked(&self) -> impl Iterator<Item = &str> {
        self.columns
            .iter()
            .map(|c| c.name.as_str())
            .filter(|c| !self.primary.iter().any(|p| p == c))
            .filter(|c| column_class(self.spec.name, c) != ColumnClass::Local)
    }

    /// The carried columns this schema has (`PRAGMA table_info`). A newer schema may send columns
    /// this build does not know: they are ignored, never an error. Key and Local columns in `c`
    /// are ignored too.
    pub fn known(&self, carried: &Columns) -> Columns {
        let clocked: Vec<&str> = self.clocked().collect();
        carried
            .iter()
            .filter(|(name, _)| clocked.contains(&name.as_str()))
            .map(|(name, entry)| (name.clone(), entry.clone()))
            .collect()
    }

    pub fn key_params(&self, key: &RowKey) -> Result<Vec<String>> {
        let params = match key {
            RowKey::Singleton => Vec::new(),
            RowKey::Single(text) => vec![text.clone()],
            RowKey::Composite(parts) => parts.clone(),
        };
        if params.len() != self.key.len() {
            return Err(AppError::Internal(format!(
                "sync merge: a {} key has the wrong shape",
                self.spec.name
            )));
        }
        Ok(params)
    }

    /// `WHERE …` selecting the key's row, and its parameters.
    pub fn filter(&self, key: &RowKey) -> Result<(String, Vec<Value>)> {
        let params = self.key_params(key)?;
        let filter = match self.spec.key {
            KeyShape::Singleton(_) => format!(
                "WHERE rowid = (SELECT MIN(rowid) FROM {})",
                quoted(self.spec.name)
            ),
            KeyShape::Primary => format!(
                "WHERE {}",
                self.key
                    .iter()
                    .enumerate()
                    .map(|(i, c)| format!("{} = ?{}", quoted(c), i + 1))
                    .collect::<Vec<_>>()
                    .join(" AND ")
            ),
        };
        Ok((filter, params.into_iter().map(Value::Text).collect()))
    }

    /// Every column of the key's row, or `None` when it is not live.
    pub fn read(&self, conn: &Connection, key: &RowKey) -> Result<Option<RowImage>> {
        let (filter, params) = self.filter(key)?;
        Ok(conn
            .prepare_cached(&format!("{} {filter}", self.select()))?
            .query_row(params_from_iter(params), |r| self.image(r))
            .optional()?)
    }

    /// The rows whose `column` holds `value`, as `(pk text, image)` in key order.
    pub fn select_where(
        &self,
        conn: &Connection,
        column: &str,
        value: &str,
    ) -> Result<Vec<(String, RowImage)>> {
        self.select_matching(
            conn,
            &format!("{} = ?1", quoted(column)),
            vec![Value::Text(value.to_string())],
        )
    }

    /// The rows matching the SQL `condition` (its parameters `params`), as `(pk text, image)` in
    /// key order.
    pub fn select_matching(
        &self,
        conn: &Connection,
        condition: &str,
        params: Vec<Value>,
    ) -> Result<Vec<(String, RowImage)>> {
        let images = conn
            .prepare_cached(&format!("{} WHERE {condition}", self.select()))?
            .query_map(params_from_iter(params), |r| self.image(r))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let mut rows = images
            .into_iter()
            .map(|image| Ok((self.pk_text(&image)?, image)))
            .collect::<Result<Vec<_>>>()?;
        rows.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(rows)
    }

    fn select(&self) -> String {
        format!(
            "SELECT {} FROM {}",
            self.columns
                .iter()
                .map(|c| quoted(&c.name))
                .collect::<Vec<_>>()
                .join(", "),
            quoted(self.spec.name)
        )
    }

    fn image(&self, r: &rusqlite::Row<'_>) -> rusqlite::Result<RowImage> {
        self.columns
            .iter()
            .enumerate()
            .map(|(i, c)| Ok((c.name.clone(), SqlValue::from(r.get::<_, Value>(i)?))))
            .collect()
    }

    /// The logical key text of a row (what `sync_clock.pk` holds).
    pub fn pk_text(&self, image: &RowImage) -> Result<String> {
        let part = |column: &String| {
            image.get(column).and_then(key_text).ok_or_else(|| {
                AppError::Internal(format!(
                    "sync merge: {}.{column} holds no key value",
                    self.spec.name
                ))
            })
        };
        Ok(match self.spec.key {
            KeyShape::Singleton(constant) => constant.to_string(),
            KeyShape::Primary if self.key.len() == 1 => part(&self.key[0])?,
            KeyShape::Primary => {
                RowKey::Composite(self.key.iter().map(part).collect::<Result<_>>()?).to_pk_text()?
            }
        })
    }
}

/// Table shapes and the FK graph, looked up once per merge.
#[derive(Default)]
pub(super) struct Shapes {
    tables: HashMap<&'static str, Rc<Table>>,
    graph: Option<Rc<Graph>>,
}

impl Shapes {
    pub fn table(&mut self, conn: &Connection, name: &str) -> Result<Rc<Table>> {
        if let Some(table) = self.tables.get(name) {
            return Ok(Rc::clone(table));
        }
        let table = Rc::new(Table::load(conn, name)?);
        self.tables.insert(table.spec.name, Rc::clone(&table));
        Ok(table)
    }

    pub fn graph(&mut self, conn: &Connection) -> Result<Rc<Graph>> {
        if let Some(graph) = &self.graph {
            return Ok(Rc::clone(graph));
        }
        let graph = Rc::new(Graph::load(conn)?);
        self.graph = Some(Rc::clone(&graph));
        Ok(graph)
    }
}

/// Every clock of one key, by column.
pub(super) fn read_clocks(
    conn: &Connection,
    table: &str,
    pk: &str,
) -> Result<HashMap<String, Clock>> {
    let rows = conn
        .prepare_cached("SELECT col, hlc, device FROM sync_clock WHERE tbl = ?1 AND pk = ?2")?
        .query_map([table, pk], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Vec<u8>>(2)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    rows.into_iter()
        .map(|(col, hlc, device)| {
            let hlc = u64::try_from(hlc).map_err(|_| {
                AppError::Internal(format!("sync_clock holds a negative clock {hlc}"))
            })?;
            let device: [u8; 16] = device
                .try_into()
                .map_err(|_| AppError::Internal("sync_clock.device is not 16 bytes".into()))?;
            Ok((
                col,
                Clock {
                    hlc,
                    device: DeviceId(device),
                },
            ))
        })
        .collect()
}

/// `None` for the zero clock: a row clock that was never issued is stored as no entry.
fn nonzero(clock: Clock) -> Option<Clock> {
    (clock != Clock::ZERO).then_some(clock)
}

/// Sets one clock to exactly `clock`, or removes it (`None`).
fn set_clock(
    conn: &Connection,
    table: &str,
    pk: &str,
    col: &str,
    clock: Option<Clock>,
) -> Result<()> {
    match clock {
        Some(clock) => {
            conn.prepare_cached(
                "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES (?1, ?2, ?3, ?4, ?5) \
                 ON CONFLICT(tbl, pk, col) DO UPDATE SET hlc = excluded.hlc, device = excluded.device",
            )?
            .execute(params![
                table,
                pk,
                col,
                sql_clock(clock.hlc)?,
                clock.device.0.to_vec()
            ])?;
        }
        None => {
            conn.prepare_cached("DELETE FROM sync_clock WHERE tbl = ?1 AND pk = ?2 AND col = ?3")?
                .execute(params![table, pk, col])?;
        }
    }
    Ok(())
}

/// Raises one clock to `clock` when that is newer (a tombstone only ever moves forward).
pub(super) fn raise_clock(
    conn: &Connection,
    table: &str,
    pk: &str,
    col: &str,
    clock: Clock,
) -> Result<()> {
    conn.prepare_cached(&format!(
        "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES (?1, ?2, ?3, ?4, ?5) {CLOCK_UPSERT}"
    ))?
    .execute(params![
        table,
        pk,
        col,
        sql_clock(clock.hlc)?,
        clock.device.0.to_vec()
    ])?;
    Ok(())
}

fn sql_clock(hlc: u64) -> Result<i64> {
    i64::try_from(hlc).map_err(|_| AppError::Internal(format!("clock {hlc} is out of range")))
}

pub(super) fn quoted(identifier: &str) -> String {
    format!("\"{}\"", identifier.replace('"', "\"\""))
}
