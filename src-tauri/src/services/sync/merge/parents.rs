//! Foreign keys during a merge (D14): a child whose parent's epoch ended follows the key's own
//! `ON DELETE` rule, a parent never seen stops the merge, and a delete first applies the NO ACTION
//! orphan rules so the deferred FK check at commit cannot fail.

use std::collections::{BTreeSet, HashMap};

use rusqlite::Connection;

use super::row::{key_text, note, quoted, read_clocks, Columns, MergeCtx, RowImage, Table};
use super::row::{ROW, TOMBSTONE};
use super::StopReason;
use crate::error::Result;
use crate::services::sync::capture::{self, Mode};
use crate::services::sync::changeset::{RowChange, RowKey, SqlValue};
use crate::services::sync::hlc::Clock;
use crate::services::sync::registry::keys::{foreign_keys, ForeignKey, OnDelete, OrphanRule};

/// The foreign keys between Synced tables, by child and by parent.
pub(super) struct Graph {
    by_child: HashMap<String, Vec<ForeignKey>>,
    by_parent: HashMap<String, Vec<ForeignKey>>,
}

impl Graph {
    pub fn load(conn: &Connection) -> Result<Self> {
        let mut by_child: HashMap<String, Vec<ForeignKey>> = HashMap::new();
        let mut by_parent: HashMap<String, Vec<ForeignKey>> = HashMap::new();
        for fk in foreign_keys(conn)? {
            by_child
                .entry(fk.child.clone())
                .or_default()
                .push(fk.clone());
            by_parent.entry(fk.parent.clone()).or_default().push(fk);
        }
        Ok(Self {
            by_child,
            by_parent,
        })
    }

    /// The keys `table` holds as a child.
    pub fn of_child(&self, table: &str) -> &[ForeignKey] {
        self.by_child.get(table).map_or(&[], Vec::as_slice)
    }

    /// The keys that point at `table`.
    pub fn of_parent(&self, table: &str) -> &[ForeignKey] {
        self.by_parent.get(table).map_or(&[], Vec::as_slice)
    }
}

/// The verdict on an insert's parents.
pub(super) enum Parents {
    Ok,
    /// A CASCADE or drop-child key points into a deleted parent epoch: the change becomes a
    /// tombstone at the parent's tombstone clock.
    Tombstone(Clock),
    Stop(StopReason),
}

/// Checks every non-NULL foreign key value in `columns` (the values that will be in the row: all
/// carried ones for a fresh row, the winning ones for a merged row) and every key part
/// against the parent's row clock `PL` and tombstone `PD`, given `pg`, the parent generation the
/// author saw (`change.parents`, ZERO when absent):
/// `PD` present and `pg < PD` → the key's rule (SET NULL rules clear the column in `columns`,
/// keeping its clock); else a live parent with `pg ≤ PL` is fine; anything else stops the merge.
pub(super) fn check(
    conn: &Connection,
    ctx: &mut MergeCtx,
    table: &Table,
    change: &RowChange,
    pk: &str,
    columns: &mut Columns,
) -> Result<Parents> {
    let graph = ctx.shapes.graph(conn)?;
    let key_parts = table.key_params(&change.key)?;
    let mut cascade: Option<Clock> = None;
    for fk in graph.of_child(table.spec.name) {
        let key_part = table.key.iter().position(|k| *k == fk.column);
        let value = match (columns.get(&fk.column), key_part) {
            (Some((value, _)), _) => key_text(value),
            (None, Some(i)) => key_parts.get(i).cloned(),
            (None, None) => None,
        };
        let Some(value) = value else {
            continue;
        };
        let seen = change
            .parents
            .get(&fk.column)
            .copied()
            .unwrap_or(Clock::ZERO);
        let clocks = read_clocks(conn, &fk.parent, &value)?;
        if let Some(gone) = clocks.get(TOMBSTONE).copied().filter(|d| seen < *d) {
            match fk.on_delete {
                OnDelete::SetNull | OnDelete::NoAction(OrphanRule::SetNull)
                    if key_part.is_none() =>
                {
                    if let Some(entry) = columns.get_mut(&fk.column) {
                        entry.0 = SqlValue::Null;
                    }
                }
                _ => cascade = cascade.max(Some(gone)),
            }
            continue;
        }
        let generation = clocks.get(ROW).copied().unwrap_or(Clock::ZERO);
        if parent_exists(conn, fk, &value)? && seen <= generation {
            continue;
        }
        return Ok(Parents::Stop(StopReason::MissingParent {
            table: table.spec.name.to_string(),
            key: pk.to_string(),
            parent: format!("{} {value}", fk.parent),
        }));
    }
    Ok(cascade.map_or(Parents::Ok, Parents::Tombstone))
}

fn parent_exists(conn: &Connection, fk: &ForeignKey, value: &str) -> Result<bool> {
    Ok(conn
        .prepare_cached(&format!(
            "SELECT EXISTS (SELECT 1 FROM {} WHERE {} = ?1)",
            quoted(&fk.parent),
            quoted(&fk.parent_column)
        ))?
        .query_row([value], |r| r.get(0))?)
}

/// One row a delete removes or changes.
struct Affected {
    table: std::rc::Rc<Table>,
    pk: String,
    image: RowImage,
}

/// Deletes the live row `pk` of `table` (its values `image`) at the tombstone clock `at`, as the
/// deleting device's delete did:
///
/// 1. the rows SQLite will cascade to and the rows it will set NULL are read first (`Touched`
///    needs their old values);
/// 2. the NO ACTION orphan rules run on the live children of the row and of every cascaded row,
///    in `repair` mode: a nullable reference is cleared, a required one deletes the child the
///    same way (recursively, at `at`);
/// 3. the DELETE runs in `merge` mode with the cascade clock `at`, so the row and every cascaded
///    row get the tombstone `at` from the triggers. Every merge delete goes through here, so the
///    cascade clock is always set first (the triggers refuse a merge delete without one).
pub(super) fn delete_row(
    conn: &Connection,
    ctx: &mut MergeCtx,
    table: &Table,
    pk: &str,
    image: &RowImage,
    at: Clock,
) -> Result<()> {
    let graph = ctx.shapes.graph(conn)?;
    let mut doomed = vec![Affected {
        table: ctx.shapes.table(conn, table.spec.name)?,
        pk: pk.to_string(),
        image: image.clone(),
    }];
    let mut seen: BTreeSet<(String, String)> =
        [(table.spec.name.to_string(), pk.to_string())].into();
    let mut nulled: Vec<Affected> = Vec::new();
    let mut orphans: Vec<(Affected, OrphanRule, String)> = Vec::new();
    let mut next = 0;
    while next < doomed.len() {
        let parent_name = doomed[next].table.spec.name;
        let parent_image = doomed[next].image.clone();
        next += 1;
        for fk in graph.of_parent(parent_name) {
            let Some(value) = parent_image.get(&fk.parent_column).and_then(key_text) else {
                continue;
            };
            let child = ctx.shapes.table(conn, &fk.child)?;
            for (child_pk, child_image) in child.select_where(conn, &fk.column, &value)? {
                let affected = Affected {
                    table: std::rc::Rc::clone(&child),
                    pk: child_pk,
                    image: child_image,
                };
                match fk.on_delete {
                    OnDelete::Cascade => {
                        if seen.insert((fk.child.clone(), affected.pk.clone())) {
                            doomed.push(affected);
                        }
                    }
                    OnDelete::SetNull => nulled.push(affected),
                    OnDelete::NoAction(rule) => orphans.push((affected, rule, fk.column.clone())),
                }
            }
        }
    }

    for (orphan, rule, column) in orphans {
        if seen.contains(&(orphan.table.spec.name.to_string(), orphan.pk.clone())) {
            continue;
        }
        let key = RowKey::from_pk_text(orphan.table.spec, &orphan.pk)?;
        match rule {
            OrphanRule::SetNull => {
                let (filter, params) = orphan.table.filter(&key)?;
                capture::with_mode(conn, Mode::Repair, || {
                    conn.execute(
                        &format!(
                            "UPDATE {} SET {} = NULL {filter}",
                            quoted(orphan.table.spec.name),
                            quoted(&column)
                        ),
                        rusqlite::params_from_iter(params),
                    )?;
                    Ok(())
                })?;
                nulled.push(orphan);
            }
            OrphanRule::DropChild => {
                // A second parent's rules may already have removed it.
                if let Some(current) = orphan.table.read(conn, &key)? {
                    delete_row(conn, ctx, &orphan.table, &orphan.pk, &current, at)?;
                }
            }
        }
    }

    let (filter, params) = table.filter(&RowKey::from_pk_text(table.spec, pk)?)?;
    capture::set_cascade_clock(conn, at)?;
    conn.execute(
        &format!("DELETE FROM {} {filter}", quoted(table.spec.name)),
        rusqlite::params_from_iter(params),
    )?;

    for row in &doomed {
        note(
            &mut ctx.touched,
            &graph,
            &row.table,
            &row.pk,
            Some(&row.image),
            None,
        );
    }
    let mut noted: BTreeSet<(String, String)> = BTreeSet::new();
    for row in nulled {
        let name = row.table.spec.name;
        if seen.contains(&(name.to_string(), row.pk.clone()))
            || !noted.insert((name.to_string(), row.pk.clone()))
        {
            continue;
        }
        let current = row
            .table
            .read(conn, &RowKey::from_pk_text(row.table.spec, &row.pk)?)?;
        if current.as_ref() != Some(&row.image) {
            note(
                &mut ctx.touched,
                &graph,
                &row.table,
                &row.pk,
                Some(&row.image),
                current.as_ref(),
            );
        }
    }
    Ok(())
}
