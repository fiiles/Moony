//! Recomputed-or-typed columns (ruling T18b): a column such as `bank_accounts.balance` or
//! `stock_investments.quantity` is computed from its source rows while any exist and typed
//! otherwise. Its typed value is the last-writer-wins one (a captured write made without source
//! rows, or a merge); while source rows exist the capture triggers keep it in `sync_typed`
//! (`capture::triggers::typed_triggers`), the journal sends it, and when the last source row goes
//! — locally or by merge — `restore` puts it back. A profile without sync has no triggers and an
//! empty `sync_typed`, so nothing here changes it (G5).

use rusqlite::{params, Connection};

use super::Touched;
use crate::error::{AppError, Result};
use crate::services::sync::capture;
use crate::services::sync::registry::{self, schema};

/// When row `pk` of `table` has no `source` rows left, puts the typed value back into every
/// column of `table` derived from `source` that holds a computed value over a typed one
/// (`sync_typed`), and forgets a stale entry. A column without an entry already holds its typed
/// value — the value at sync start counts as typed, as in the triggers — and keeps it.
///
/// Returns whether the columns now hold their typed values (capture installed, no `source` rows
/// left): the caller then writes nothing of its own. Without capture it returns false and the
/// caller keeps its pre-sync behaviour, a zero fallback included (G5).
pub fn restore(conn: &Connection, table: &str, pk: &str, source: &str) -> Result<bool> {
    if !capture::is_installed(conn)? {
        return Ok(false);
    }
    let (columns, rows) = registry::spec(table)
        .and_then(|spec| {
            spec.recomputed_or_typed
                .iter()
                .find(|(_, rows)| rows.table == source)
        })
        .ok_or_else(|| {
            AppError::Internal(format!(
                "sync typed: {table} has no columns derived from {source}"
            ))
        })?;
    let has_rows: bool = conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM main.{} WHERE {} = ?1)",
            rows.table, rows.fk
        ),
        [pk],
        |r| r.get(0),
    )?;
    if has_rows {
        return Ok(false);
    }
    let key = match schema::primary_key(conn, table)?.as_slice() {
        [key] => key.clone(),
        _ => {
            return Err(AppError::Internal(format!(
                "sync typed: {table} has recomputed-or-typed columns but no single key"
            )))
        }
    };
    for column in *columns {
        conn.execute(
            &format!(
                "UPDATE main.{table} SET \"{column}\" = (
                   SELECT typed FROM sync_typed WHERE tbl = ?2 AND pk = ?1 AND col = ?3)
                 WHERE \"{key}\" = ?1 AND EXISTS (
                   SELECT 1 FROM sync_typed WHERE tbl = ?2 AND pk = ?1 AND col = ?3
                     AND assigned IS main.{table}.\"{column}\")"
            ),
            params![pk, table, column],
        )?;
        conn.execute(
            "DELETE FROM sync_typed WHERE tbl = ?2 AND pk = ?1 AND col = ?3",
            params![pk, table, column],
        )?;
    }
    Ok(true)
}

/// After a merge: every touched owner row whose source rows are all gone gets its typed values
/// back (`restore`). Owners with source rows are recomputed by `mirrors`.
pub fn restore_sourceless(conn: &Connection, touched: &Touched) -> Result<()> {
    for spec in registry::synced_tables() {
        for (_, rows) in spec.recomputed_or_typed {
            for id in touched.owners(spec.name) {
                restore(conn, spec.name, &id, rows.table)?;
            }
        }
    }
    Ok(())
}
