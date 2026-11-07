//! Shared helpers for the sync engine tests: a migrated connection, and the one convergence
//! comparison every multi-device test uses (`assert_converged`, `divergence`).

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::types::Value;
use rusqlite::Connection;

use super::journal::Reader;
use super::registry::keys::{foreign_keys, OnDelete, OrphanRule};
use super::registry::{
    column_class, schema, synced_tables, tables, ColumnClass, TableClass, APP_CONFIG_SYNCED_KEYS,
};
use super::repair::shadowed_columns;

/// In-memory connection with foreign keys on and the real migrations applied.
pub(crate) fn migrated() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("foreign keys");
    crate::db::run_migrations(&conn).expect("migrations");
    conn
}

/// One `sync_clock` row: `(tbl, pk, col, hlc, device)`.
pub(crate) type ClockRow = (String, String, String, i64, Vec<u8>);

/// Every synced row of every synced table (Local columns left out, `app_config` limited to the
/// keys that sync), then every Derived table's rows without their device-local key and stamps
/// (`id`, `created_at`, `updated_at`: a rebuild mints them), in a stable order: what must be
/// identical on every device.
pub(crate) fn dump(conn: &Connection) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    let derived = tables().iter().filter(|t| t.class == TableClass::Derived);
    for spec in synced_tables().chain(derived) {
        let columns: Vec<String> = schema::columns(conn, spec.name)
            .expect("columns")
            .into_iter()
            .map(|c| c.name)
            .filter(|c| column_class(spec.name, c) != ColumnClass::Local)
            .filter(|c| {
                spec.class != TableClass::Derived
                    || !["id", "created_at", "updated_at"].contains(&c.as_str())
            })
            .collect();
        let list = columns
            .iter()
            .map(|c| format!("\"{c}\""))
            .collect::<Vec<_>>()
            .join(", ");
        let filter = if spec.name == "app_config" {
            format!(
                "WHERE key IN ({})",
                APP_CONFIG_SYNCED_KEYS
                    .iter()
                    .map(|k| format!("'{k}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        } else {
            String::new()
        };
        let order = (1..=columns.len())
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!("SELECT {list} FROM {} {filter} ORDER BY {order}", spec.name);
        let mut stmt = conn.prepare(&sql).expect("prepare dump");
        let rows: Vec<String> = stmt
            .query_map([], |r| {
                Ok((0..columns.len())
                    .map(|i| format!("{:?}", r.get::<_, Value>(i).expect("value")))
                    .collect::<Vec<_>>()
                    .join(" | "))
            })
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        out.push((spec.name.to_string(), rows));
    }
    out
}

pub(crate) fn clock_dump(conn: &Connection) -> Vec<ClockRow> {
    let mut stmt = conn
        .prepare("SELECT tbl, pk, col, hlc, device FROM sync_clock ORDER BY tbl, pk, col")
        .expect("prepare");
    stmt.query_map([], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
    })
    .expect("query")
    .map(|r| r.expect("row"))
    .collect()
}

/// Every clock of every live row (its `'*'` and column clocks), which must be identical on every
/// device. A dead row's `'*'` and column clocks may differ by device, and nothing reads them.
/// Tombstones are compared by `tombstone_disagreement`.
pub(crate) fn clock_dump_live(conn: &Connection) -> Vec<ClockRow> {
    let reader = Reader::new(conn).expect("reader");
    clock_dump(conn)
        .into_iter()
        .filter(|(tbl, pk, col, _, _)| {
            col != "-"
                && reader
                    .read(tbl, pk)
                    .expect("read row")
                    .is_some_and(|row| row.insert)
        })
        .collect()
}

/// Every tombstone `'-'`, by key.
pub(crate) fn tombstones(conn: &Connection) -> BTreeMap<(String, String), (i64, Vec<u8>)> {
    clock_dump(conn)
        .into_iter()
        .filter(|(_, _, col, _, _)| col == "-")
        .map(|(tbl, pk, _, hlc, device)| ((tbl, pk), (hlc, device)))
        .collect()
}

/// What every live row of a table with a column a repair may show differently (a renamed name,
/// a cleared link) carries on the wire for that column: the value typed (`sync_typed`), which
/// must be the same everywhere. `(table, pk, column, value)`, in key order.
pub(crate) fn typed_view(conn: &Connection) -> Vec<(String, String, String, String)> {
    let reader = Reader::new(conn).expect("reader");
    let mut out = Vec::new();
    for spec in synced_tables() {
        let columns = shadowed_columns(spec.name);
        if columns.is_empty() {
            continue;
        }
        let mut stmt = conn
            .prepare(&format!("SELECT id FROM {} ORDER BY id", spec.name))
            .expect("prepare typed view");
        let keys: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .expect("query")
            .map(|r| r.expect("key"))
            .collect();
        for pk in keys {
            let row = reader
                .read(spec.name, &pk)
                .expect("read row")
                .expect("a live row reads");
            for column in &columns {
                let value = row
                    .columns
                    .get(*column)
                    .map_or("<absent>".to_string(), |(value, _)| format!("{value:?}"));
                out.push((spec.name.to_string(), pk.clone(), column.to_string(), value));
            }
        }
    }
    out
}

/// The tombstone invariant of `merge/mod.rs`: the same keys are tombstoned on both devices with
/// the same clock, except where a cascade raised a key above the author's own earlier tombstone.
/// There the higher value must equal, on the device holding it, the tombstone of some row of a
/// table the key's table cascades from (CASCADE, or a NO ACTION drop-child rule, which the merge
/// stamps the same way). Without the dead child's row the exact parent key is unknown, so this
/// is the documented tolerance: any row of a cascade-parent table with exactly that tombstone.
/// `None` when the invariant holds, else what breaks it.
pub(crate) fn tombstone_disagreement(x: &Connection, y: &Connection) -> Option<String> {
    let fks = foreign_keys(x).expect("foreign keys");
    let (tx, ty) = (tombstones(x), tombstones(y));
    if tx.keys().ne(ty.keys()) {
        let (kx, ky): (BTreeSet<_>, BTreeSet<_>) = (tx.keys().collect(), ty.keys().collect());
        let only_x: Vec<_> = kx.difference(&ky).take(3).collect();
        let only_y: Vec<_> = ky.difference(&kx).take(3).collect();
        return Some(format!(
            "the tombstoned keys differ: only on the first {only_x:?}, only on the second {only_y:?}"
        ));
    }
    for (key, ours) in &tx {
        let theirs = &ty[key];
        if ours == theirs {
            continue;
        }
        let (high, holder) = if ours > theirs {
            (ours, &tx)
        } else {
            (theirs, &ty)
        };
        let parents: Vec<&str> = fks
            .iter()
            .filter(|f| {
                f.child == key.0
                    && matches!(
                        f.on_delete,
                        OnDelete::Cascade | OnDelete::NoAction(OrphanRule::DropChild)
                    )
            })
            .map(|f| f.parent.as_str())
            .collect();
        if !holder
            .iter()
            .any(|((table, _), clock)| parents.contains(&table.as_str()) && clock == high)
        {
            return Some(format!(
                "the tombstone of {key:?} differs ({ours:?} vs {theirs:?}) and the higher one is \
                 no cascade parent's tombstone"
            ));
        }
    }
    None
}

/// The first difference between device 0 and any other device, or `None` when they converged:
/// the synced rows, the live rows' clocks, the typed values of renamed or unlinked columns, and
/// the tombstones (`tombstone_disagreement`). Names the table and the rows that differ.
pub(crate) fn divergence(devices: &[&Connection]) -> Option<String> {
    let (first, rest) = devices.split_first().expect("a device");
    let (rows, clocks, typed) = (dump(first), clock_dump_live(first), typed_view(first));
    for (i, conn) in rest.iter().enumerate() {
        let other = i + 1;
        if let Some(diff) = first_row_difference(&rows, &dump(conn)) {
            return Some(format!(
                "synced rows of device 0 and device {other} differ: {diff}"
            ));
        }
        let key = |c: &ClockRow| (c.0.clone(), c.1.clone());
        if let Some(diff) = first_key_difference(&clocks, &clock_dump_live(conn), key) {
            return Some(format!(
                "live clocks of device 0 and device {other} differ: {diff}"
            ));
        }
        let key = |t: &(String, String, String, String)| (t.0.clone(), t.1.clone());
        if let Some(diff) = first_key_difference(&typed, &typed_view(conn), key) {
            return Some(format!(
                "typed values of device 0 and device {other} differ: {diff}"
            ));
        }
        if let Some(diff) = tombstone_disagreement(first, conn) {
            return Some(format!("tombstones of device 0 and device {other}: {diff}"));
        }
    }
    None
}

/// The synced rows, the live rows' clocks, the typed values and the tombstones agree on every
/// device.
pub(crate) fn assert_converged(devices: &[&Connection]) {
    if let Some(message) = divergence(devices) {
        panic!("not converged: {message}");
    }
}

/// The first table whose rows differ, with the rows only one side holds (three at most).
fn first_row_difference(
    ours: &[(String, Vec<String>)],
    theirs: &[(String, Vec<String>)],
) -> Option<String> {
    for ((table, a), (other_table, b)) in ours.iter().zip(theirs) {
        assert_eq!(table, other_table, "dumps list the same tables");
        if a == b {
            continue;
        }
        let (sa, sb): (BTreeSet<&String>, BTreeSet<&String>) =
            (a.iter().collect(), b.iter().collect());
        let only_a: Vec<_> = sa.difference(&sb).take(3).collect();
        let only_b: Vec<_> = sb.difference(&sa).take(3).collect();
        return Some(format!(
            "table {table}: only on device 0 {only_a:#?}, only on the other {only_b:#?}"
        ));
    }
    None
}

/// The first `(table, pk)` whose entries differ, with both sides' entries for it.
fn first_key_difference<T: Ord + Clone + std::fmt::Debug>(
    ours: &[T],
    theirs: &[T],
    key_of: impl Fn(&T) -> (String, String),
) -> Option<String> {
    if ours == theirs {
        return None;
    }
    let (sa, sb): (BTreeSet<&T>, BTreeSet<&T>) = (ours.iter().collect(), theirs.iter().collect());
    let Some(first) = sa.symmetric_difference(&sb).map(|t| key_of(t)).min() else {
        // The same entries, in another order or count.
        return Some(format!(
            "the same entries differ in order or count: device 0 has {} entries, the other {}",
            ours.len(),
            theirs.len()
        ));
    };
    let of = |list: &[T]| -> Vec<T> {
        list.iter()
            .filter(|t| key_of(t) == first)
            .cloned()
            .collect()
    };
    Some(format!(
        "key {first:?}: device 0 has {:?}, the other has {:?}",
        of(ours),
        of(theirs)
    ))
}
