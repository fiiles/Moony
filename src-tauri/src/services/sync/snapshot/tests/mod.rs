//! Snapshot tests: a dumping device `A` (sync on), a joining device `B` (fresh profile), and a
//! third device `C` whose changesets `A` merges. `stream` holds what the stream carries and
//! refuses, `restore` what each restore leaves behind.

mod restore;
mod stream;

use std::collections::BTreeMap;

use rusqlite::types::Value;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::stream::{encode, write_record};
use super::*;
use crate::models::budgeting::InsertBudgetGoal;
use crate::services::budgeting;
use crate::services::sync::capture;
use crate::services::sync::chain::{judge, EnvelopeMeta, Verdict};
use crate::services::sync::changeset::{to_json, Changeset, DevTable, RowChange, RowKey, SqlValue};
use crate::services::sync::hlc::{Clock, DeviceId};
use crate::services::sync::journal::{build_next, record_sealed, Reader};
use crate::services::sync::merge::{apply_changeset, Outcome};
use crate::services::sync::registry::{
    column_class, schema, synced_tables, tables, ColumnClass, TableClass, APP_CONFIG_SYNCED_KEYS,
};
use crate::services::sync::state;
use crate::services::sync::test_support::migrated;

const A: DeviceId = DeviceId([1; 16]);
const B: DeviceId = DeviceId([2; 16]);
const C: DeviceId = DeviceId([3; 16]);
const D: DeviceId = DeviceId([4; 16]);
const NOW: i64 = 1_772_323_200; // 2026-03-01
const DAY: i64 = 1_767_225_600; // 2026-01-01

type Sent = Vec<(Changeset, [u8; 32])>;

fn exec(conn: &Connection, sql: &str) {
    conn.execute_batch(sql).expect(sql);
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

fn value(conn: &Connection, sql: &str) -> Option<String> {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

fn strings(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).expect(sql);
    stmt.query_map([], |r| r.get(0))
        .expect(sql)
        .map(|r| r.expect("row"))
        .collect()
}

/// A sync-enabled profile of `id` (capture installed, the rule-pack keys clocked), outbox empty.
fn device(id: DeviceId) -> Connection {
    let conn = migrated();
    state::enable(&conn, id).expect("enable");
    exec(&conn, "DELETE FROM sync_outbox");
    conn
}

fn hash_of(cs: &Changeset) -> [u8; 32] {
    let digest = Sha256::digest(to_json(cs).unwrap());
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

fn apply(conn: &Connection, sent: &Sent) {
    for (cs, hash) in sent {
        match apply_changeset(conn, cs, *hash, NOW).expect("apply") {
            Outcome::Applied(_) => {}
            Outcome::Stopped(reason) => panic!("stopped: {reason:?}"),
        }
    }
}

/// Pushes everything `conn` has queued (a dump needs an empty outbox, ruling T18e), then dumps.
fn snapshot(conn: &Connection, seq: u64) -> (Vec<u8>, DumpStats) {
    send(conn);
    let mut out = Vec::new();
    let stats = dump(conn, seq, &mut out).expect("dump");
    (out, stats)
}

fn records(bytes: &[u8]) -> Vec<Record> {
    let mut input = bytes;
    let mut devs = Vec::new();
    let mut out = Vec::new();
    while let Some(record) = read_record(&mut input, &devs).expect("record") {
        if let Record::Devices(list) = &record {
            devs = list.clone();
        }
        out.push(record);
    }
    out
}

fn rows(records: &[Record]) -> Vec<&RowChange> {
    records
        .iter()
        .filter_map(|r| match r {
            Record::Row(row) => Some(row),
            _ => None,
        })
        .collect()
}

/// Re-encodes `records` as a stream.
fn stream_of(records: &[Record]) -> Vec<u8> {
    let mut devs = DevTable::new();
    let Some(Record::Devices(list)) = records.first() else {
        panic!("devs first");
    };
    for device in list {
        devs.index(*device);
    }
    let mut out = Vec::new();
    for record in records {
        write_record(&mut out, &encode(record, &mut devs)).expect("write");
    }
    out
}

/// Every synced row of every synced table (Local columns left out, `app_config` limited to the
/// keys that sync), in a stable order.
fn table_dump(conn: &Connection) -> Vec<(String, Vec<String>)> {
    let mut out = Vec::new();
    for spec in synced_tables() {
        let columns: Vec<String> = schema::columns(conn, spec.name)
            .expect("columns")
            .into_iter()
            .map(|c| c.name)
            .filter(|c| column_class(spec.name, c) != ColumnClass::Local)
            .collect();
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
        out.push((
            spec.name.to_string(),
            all_rows(conn, spec.name, &columns, &filter),
        ));
    }
    out
}

fn all_rows(conn: &Connection, table: &str, columns: &[String], filter: &str) -> Vec<String> {
    let list = columns
        .iter()
        .map(|c| format!("\"{c}\""))
        .collect::<Vec<_>>()
        .join(", ");
    let order = (1..=columns.len())
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!("SELECT {list} FROM {table} {filter} ORDER BY {order}");
    let mut stmt = conn.prepare(&sql).expect("prepare dump");
    stmt.query_map([], |r| {
        Ok((0..columns.len())
            .map(|i| format!("{:?}", r.get::<_, Value>(i).expect("value")))
            .collect::<Vec<_>>()
            .join(" | "))
    })
    .expect("query")
    .map(|r| r.expect("row"))
    .collect()
}

/// Every row of every Seed table.
fn seed_dump(conn: &Connection) -> Vec<(String, Vec<String>)> {
    tables()
        .iter()
        .filter(|t| t.class == TableClass::Seed)
        .map(|spec| {
            let columns: Vec<String> = schema::columns(conn, spec.name)
                .expect("columns")
                .into_iter()
                .map(|c| c.name)
                .collect();
            (
                spec.name.to_string(),
                all_rows(conn, spec.name, &columns, ""),
            )
        })
        .collect()
}

type ClockRow = (String, String, String, i64, Vec<u8>);

fn clock_dump(conn: &Connection) -> Vec<ClockRow> {
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

/// Every clock of every live row, and every tombstone (a dead row's `'*'` means nothing).
fn clock_dump_live(conn: &Connection) -> Vec<ClockRow> {
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

fn typed_dump(conn: &Connection) -> Vec<String> {
    strings(
        conn,
        "SELECT tbl || ' ' || pk || ' ' || col || ' ' || typed || ' -> ' || COALESCE(assigned, 'NULL') \
         FROM sync_typed ORDER BY 1",
    )
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

fn tag(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO stock_tags (id, name, created_at) VALUES (?1, ?2, 100)",
        [id, name],
    )
    .expect("insert tag");
}

/// A profile with a little of everything: a category tree whose child sorts before its parent,
/// an account under an institution, a payment, a composite-key link, an edited column, a
/// deleted row, the profile singleton, and a tag renamed by a merge from `C`.
fn rich_dumper() -> Connection {
    let a = device(A);
    exec(
        &a,
        "INSERT INTO user_profile (id, name, surname, email, language) \
           VALUES (1, 'Alex', 'Novak', 'a@b.cz', 'cs'); \
         INSERT INTO institutions (id, name) VALUES ('inst-1', 'Bank'); \
         INSERT INTO bank_accounts (id, name, currency, balance, institution_id) \
           VALUES ('acc-1', 'Main', 'CZK', '125.00', 'inst-1'); \
         INSERT INTO transaction_categories (id, name) VALUES ('z-parent', 'Parent'); \
         INSERT INTO transaction_categories (id, name, parent_id) VALUES ('a-child', 'Child', 'z-parent'); \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, category_id) \
           VALUES ('tx-1', 'acc-1', 'debit', '-10.00', 'CZK', 1767225600, 'a-child'); \
         INSERT INTO stock_investments (id, ticker, company_name, quantity) VALUES ('si-1', 'AAPL', 'Apple', '3'); \
         INSERT INTO stock_tags (id, name, created_at) VALUES ('tag-x', 'Value', 100); \
         INSERT INTO stock_investment_tags (investment_id, tag_id, created_at) VALUES ('si-1', 'tag-x', 100); \
         INSERT INTO institutions (id, name) VALUES ('inst-gone', 'Old bank'); \
         UPDATE user_profile SET surname = 'Novakova'; \
         UPDATE institutions SET name = 'Bank 2' WHERE id = 'inst-1'; \
         DELETE FROM institutions WHERE id = 'inst-gone'; \
         INSERT INTO app_config (key, value) VALUES ('milestones.mutedKinds', '[\"tax\"]');",
    );
    tag(&a, "tag-c", "Growth");
    let c = device(C);
    tag(&c, "tag-b", "Growth");
    apply(&a, &send(&c));
    a
}

type Restore<'s> = fn(&Connection, &mut &'s [u8], i64) -> Result<RestoreReport>;

fn restorers<'s>() -> [Restore<'s>; 2] {
    [restore_replace, restore_merge]
}

fn joined(bytes: &[u8]) -> Connection {
    let b = migrated();
    join(&b, B, &mut &bytes[..], NOW).expect("join");
    b
}
