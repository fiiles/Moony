//! Merge tests: two to four `migrated()` devices exchanging changesets from the journal, and
//! hand-made changesets for the stop cases and the clock edge cases.

mod basics;
mod epochs;
mod parents;
mod special;

use std::collections::BTreeMap;

use rusqlite::types::Value;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::*;
use crate::services::sync::capture;
use crate::services::sync::changeset::{to_json, Changeset, RowChange, RowKey, SqlValue};
use crate::services::sync::hlc::{Clock, DeviceId};
use crate::services::sync::journal::{build_next, record_sealed, Reader};
use crate::services::sync::natural_id::natural_id;
use crate::services::sync::registry::keys::schema_number;
use crate::services::sync::registry::{
    column_class, schema, synced_tables, ColumnClass, APP_CONFIG_SYNCED_KEYS,
};
use crate::services::sync::state;
use crate::services::sync::test_support::migrated;

const A: DeviceId = DeviceId([1; 16]);
const B: DeviceId = DeviceId([2; 16]);
const C: DeviceId = DeviceId([3; 16]);
const D: DeviceId = DeviceId([4; 16]);
const NOW: i64 = 1_772_323_200; // 2026-03-01

type Sent = Vec<(Changeset, [u8; 32])>;

/// A sync-enabled profile of device `id` with an empty outbox and no clocks. The migration seeds' timestamps
/// are pinned first: profiles migrated in different seconds would differ before any sync (a real
/// second device starts from the vault's snapshot instead).
fn device(id: DeviceId) -> Connection {
    let conn = migrated();
    for spec in synced_tables() {
        for column in schema::columns(&conn, spec.name).expect("columns") {
            if column.has_default && column.name.ends_with("_at") {
                exec(
                    &conn,
                    &format!("UPDATE {} SET \"{}\" = 0", spec.name, column.name),
                );
            }
        }
    }
    state::enable(&conn, id).expect("enable");
    // Enabling clocks and queues the two rule-pack keys; the tests never send them, so their
    // device-specific clocks are dropped too.
    exec(&conn, "DELETE FROM sync_outbox; DELETE FROM sync_clock;");
    conn
}

fn exec(conn: &Connection, sql: &str) {
    conn.execute_batch(sql).expect(sql);
}

fn hash_of(cs: &Changeset) -> [u8; 32] {
    let digest = Sha256::digest(to_json(cs));
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Everything queued on `conn`, sealed as the journal seals it.
fn send(conn: &Connection) -> Sent {
    let mut out = Vec::new();
    loop {
        let tx = conn.unchecked_transaction().expect("transaction");
        let (built, refused) = build_next(&tx).expect("build_next");
        assert!(refused.is_empty(), "{refused:?}");
        let Some(built) = built else {
            return out;
        };
        let hash = hash_of(&built.changeset);
        record_sealed(&tx, &built, hash).expect("record_sealed");
        tx.commit().expect("commit");
        out.push((built.changeset, hash));
    }
}

/// Applies every changeset; each must apply.
fn apply(conn: &Connection, sent: &Sent) -> Vec<ApplyReport> {
    sent.iter()
        .map(
            |(cs, hash)| match apply_changeset(conn, cs, *hash, NOW).expect("apply") {
                Outcome::Applied(report) => report,
                Outcome::Stopped(reason) => panic!("stopped: {reason:?}"),
            },
        )
        .collect()
}

/// A hand-made changeset of `device`.
fn changeset(device: DeviceId, seq: u64, rows: Vec<RowChange>) -> (Changeset, [u8; 32]) {
    let cs = Changeset {
        schema: schema_number(),
        barrier: 0,
        device,
        seq,
        prev: None,
        rows,
    };
    let hash = hash_of(&cs);
    (cs, hash)
}

fn clock(hlc: u64, device: DeviceId) -> Clock {
    Clock { hlc, device }
}

fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_string())
}

/// A full-row insert of `table` with every column at `generation`.
fn row(table: &str, key: &str, generation: Clock, columns: &[(&str, SqlValue)]) -> RowChange {
    RowChange {
        table: table.to_string(),
        key: RowKey::Single(key.to_string()),
        generation,
        insert: true,
        deleted: None,
        columns: columns
            .iter()
            .map(|(name, value)| (name.to_string(), (value.clone(), generation)))
            .collect(),
        parents: BTreeMap::new(),
    }
}

/// A spending plan row as the journal would send it, every column at `generation`.
fn plan_row(id: &str, generation: Clock) -> RowChange {
    row(
        "spending_plans",
        id,
        generation,
        &[
            ("amount", text("1000.00")),
            ("closed_at", SqlValue::Null),
            ("color", text("#336699")),
            ("created_at", SqlValue::Integer(NOW)),
            ("currency", text("CZK")),
            ("icon", text("plane")),
            ("name", text(&format!("Plan {id}"))),
            ("note", text("first")),
            ("updated_at", SqlValue::Integer(NOW)),
            ("year", SqlValue::Integer(2026)),
        ],
    )
}

/// The value and clock of one column of a hand-made row.
fn set(row: &mut RowChange, column: &str, value: SqlValue, at: Clock) {
    row.columns.insert(column.to_string(), (value, at));
}

fn plan(conn: &Connection, id: &str, year: i64) {
    conn.execute(
        "INSERT INTO spending_plans (id, name, year, amount, currency, icon, color, note) \
         VALUES (?1, 'Plan ' || ?1, ?2, '1000.00', 'CZK', 'plane', '#336699', 'first')",
        rusqlite::params![id, year],
    )
    .expect("insert plan");
}

fn account(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO bank_accounts (id, name, currency, balance) VALUES (?1, 'Main', 'CZK', '125.00')",
        [id],
    )
    .expect("insert account");
}

fn payment(conn: &Connection, id: &str, account: &str, day: i64) {
    conn.execute(
        "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date) \
         VALUES (?1, ?2, 'debit', '-10.00', 'CZK', ?3)",
        rusqlite::params![id, account, day],
    )
    .expect("insert payment");
}

fn category(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO transaction_categories (id, name) VALUES (?1, 'Category ' || ?1)",
        [id],
    )
    .expect("insert category");
}

fn watched_id() -> String {
    natural_id("watched_stocks", &["AAPL"])
}

fn watch(conn: &Connection, notes: &str, target: Option<&str>) {
    conn.execute(
        "INSERT INTO watched_stocks (id, ticker, target_price, notes, created_at, updated_at) \
         VALUES (?1, 'AAPL', ?2, ?3, 100, 100)",
        rusqlite::params![watched_id(), target, notes],
    )
    .expect("insert watched stock");
}

/// `conn`'s clock moves past every clock `than` issued so far.
fn later(conn: &Connection, than: &Connection) {
    let theirs = capture::tick(than).expect("tick");
    capture::observe(conn, theirs).expect("observe");
}

fn value(conn: &Connection, sql: &str) -> Option<String> {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

/// Every clock of one key, by column.
fn clocks(conn: &Connection, table: &str, pk: &str) -> BTreeMap<String, Clock> {
    let mut stmt = conn
        .prepare("SELECT col, hlc, device FROM sync_clock WHERE tbl = ?1 AND pk = ?2")
        .expect("prepare");
    stmt.query_map([table, pk], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Vec<u8>>(2)?,
        ))
    })
    .expect("query")
    .map(|r| {
        let (col, hlc, device) = r.expect("row");
        (
            col,
            clock(hlc as u64, DeviceId(device.try_into().expect("16 bytes"))),
        )
    })
    .collect()
}

fn clock_dump(conn: &Connection) -> Vec<(String, String, String, i64, Vec<u8>)> {
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

/// `sync_clock` limited to what must agree across devices: every entry of a live row, and every
/// tombstone. A dead row's `'*'` and column clocks may differ by device and nothing reads them.
fn clock_dump_live(conn: &Connection) -> Vec<(String, String, String, i64, Vec<u8>)> {
    let reader = Reader::new(conn).expect("reader");
    clock_dump(conn)
        .into_iter()
        .filter(|(tbl, pk, col, _, _)| {
            col == "-"
                || reader
                    .read(tbl, pk)
                    .expect("read row")
                    .is_some_and(|row| row.insert)
        })
        .collect()
}

/// Both the synced rows and the clocks that matter are identical on every device.
fn assert_converged(devices: &[&Connection]) {
    let (first, rest) = devices.split_first().expect("a device");
    for conn in rest {
        assert_eq!(dump(conn), dump(first));
        assert_eq!(clock_dump_live(conn), clock_dump_live(first));
    }
}

fn heads(conn: &Connection) -> Vec<(Vec<u8>, i64, Vec<u8>)> {
    let mut stmt = conn
        .prepare("SELECT device, seq, hash FROM sync_heads ORDER BY device")
        .expect("prepare");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
}

/// Every synced row of every synced table (Local columns left out, `app_config` limited to the
/// keys that sync), in a stable order: what must be identical on every device.
fn dump(conn: &Connection) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for spec in synced_tables() {
        let columns: Vec<String> = schema::columns(conn, spec.name)
            .expect("columns")
            .into_iter()
            .map(|c| c.name)
            .filter(|c| column_class(spec.name, c) != ColumnClass::Local)
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

/// Applies `rows` one by one with `apply_row` in merge mode inside a transaction, and returns
/// the context, as `merge_rows` does before the repairs.
fn merge_ctx(conn: &Connection, rows: &[RowChange]) -> MergeCtx {
    let tx = conn.unchecked_transaction().expect("transaction");
    tx.execute_batch("PRAGMA defer_foreign_keys = ON")
        .expect("defer");
    let mut ctx = MergeCtx::default();
    capture::with_mode(&tx, capture::Mode::Merge, || {
        for change in rows {
            match apply_row(&tx, change, &mut ctx)? {
                RowResult::Stop(reason) => panic!("stopped: {reason:?}"),
                RowResult::Applied | RowResult::Dropped => {}
            }
        }
        Ok(())
    })
    .expect("apply rows");
    tx.commit().expect("commit");
    ctx
}

fn rows_of(sent: &Sent) -> Vec<RowChange> {
    sent.iter().flat_map(|(cs, _)| cs.rows.clone()).collect()
}
