use std::collections::BTreeMap;

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use rusqlite::Connection;
use sha2::{Digest, Sha256};

use super::*;
use crate::models::bank_transactions::InsertBankTransaction;
use crate::models::categorization::InsertScopedLearning;
use crate::services::categorization::engine::CategorizationEngine;
use crate::services::categorization::learn::learn_scoped;
use crate::services::sync::changeset::{
    max_hlc, padded_deflated_len, to_json, Changeset, RowChange, RowKey, SqlValue,
};
use crate::services::sync::hlc::{Clock, DeviceId};
use crate::services::sync::registry::keys::schema_number;
use crate::services::sync::state;
use crate::services::sync::test_support::migrated;
use crate::services::{bank_accounts, csv_import, investments, stock_monitor};

const DEVICE: DeviceId = DeviceId([7; 16]);
const DAY: i64 = 1_767_225_600; // 2026-01-01 00:00 UTC

fn exec(conn: &Connection, sql: &str) {
    conn.execute_batch(sql).expect(sql);
}

/// Turns sync on and empties the outbox (enabling queues the two rule-pack keys).
fn enable(conn: &Connection) {
    state::enable(conn, DEVICE).expect("enable");
    exec(conn, "DELETE FROM sync_outbox");
}

fn enabled() -> Connection {
    let conn = migrated();
    enable(&conn);
    conn
}

fn clock(conn: &Connection, table: &str, pk: &str, col: &str) -> Option<Clock> {
    let mut stmt = conn
        .prepare("SELECT hlc, device FROM sync_clock WHERE tbl = ?1 AND pk = ?2 AND col = ?3")
        .expect("prepare");
    let mut rows = stmt
        .query_map([table, pk, col], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
        })
        .expect("query");
    rows.next().map(|r| {
        let (hlc, device) = r.expect("row");
        Clock {
            hlc: hlc as u64,
            device: DeviceId(device.try_into().expect("16 bytes")),
        }
    })
}

fn outbox(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT tbl, pk FROM sync_outbox ORDER BY tbl, pk")
        .expect("prepare");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
}

fn hash_of(cs: &Changeset) -> [u8; 32] {
    let digest = Sha256::digest(to_json(cs));
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// One phase-1 round: build, hash, record, commit.
fn round(conn: &mut Connection) -> (Option<Built>, Vec<Refused>) {
    let tx = conn.transaction().expect("transaction");
    let (built, refused) = build_next(&tx).expect("build_next");
    if let Some(built) = &built {
        record_sealed(&tx, built, hash_of(&built.changeset)).expect("record_sealed");
    }
    tx.commit().expect("commit");
    (built, refused)
}

/// Rounds until nothing is left; refused rows must not occur.
fn drain(conn: &mut Connection) -> Vec<Built> {
    let mut out = Vec::new();
    loop {
        let (built, refused) = round(conn);
        assert!(refused.is_empty(), "unexpected refusal: {refused:?}");
        match built {
            Some(built) => out.push(built),
            None => return out,
        }
    }
}

fn only_row(conn: &mut Connection) -> RowChange {
    let built = drain(conn);
    assert_eq!(built.len(), 1, "one envelope");
    assert_eq!(built[0].changeset.rows.len(), 1, "one row");
    built[0].changeset.rows[0].clone()
}

fn rows_by_key(built: &[Built]) -> BTreeMap<(String, String), RowChange> {
    built
        .iter()
        .flat_map(|b| b.changeset.rows.iter())
        .map(|r| ((r.table.clone(), r.key.to_pk_text()), r.clone()))
        .collect()
}

fn keys(built: &Built) -> Vec<(String, String)> {
    built
        .changeset
        .rows
        .iter()
        .map(|r| (r.table.clone(), r.key.to_pk_text()))
        .collect()
}

fn key(table: &str, pk: &str) -> (String, String) {
    (table.to_string(), pk.to_string())
}

fn text(value: &str) -> SqlValue {
    SqlValue::Text(value.to_string())
}

fn column_names(row: &RowChange) -> Vec<&str> {
    row.columns.keys().map(String::as_str).collect()
}

fn plan(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO spending_plans (id, name, year, amount, currency, icon, color, note) \
         VALUES (?1, 'Holiday ' || ?1, 2026, '1000.00', 'CZK', 'plane', '#336699', 'first')",
        [id],
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

/// Base64 text of `len` characters from a seeded generator (splitmix64): incompressible beyond
/// base64's own 6 bits per character.
fn noise(len: usize, seed: u64) -> String {
    let mut state = seed;
    let mut bytes = Vec::with_capacity(len);
    while bytes.len() < len {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        bytes.extend_from_slice(&z.to_le_bytes());
    }
    let mut out = STANDARD.encode(&bytes);
    out.truncate(len);
    out
}

const PLAN_COLUMNS: [&str; 10] = [
    "amount",
    "closed_at",
    "color",
    "created_at",
    "currency",
    "icon",
    "name",
    "note",
    "updated_at",
    "year",
];

#[test]
fn five_edits_of_one_row_send_one_value() {
    let mut conn = enabled();
    plan(&conn, "p1");
    for i in 1..=5 {
        exec(
            &conn,
            &format!("UPDATE spending_plans SET note = 'edit {i}' WHERE id = 'p1'"),
        );
    }

    let row = only_row(&mut conn);

    assert_eq!(row.key, RowKey::Single("p1".into()));
    assert_eq!(row.columns["note"].0, text("edit 5"));
    assert_eq!(
        Some(row.columns["note"].1),
        clock(&conn, "spending_plans", "p1", "note")
    );
}

#[test]
fn a_row_changed_then_deleted_sends_only_its_tombstone() {
    let mut conn = enabled();
    plan(&conn, "p1");
    exec(
        &conn,
        "UPDATE spending_plans SET note = 'changed' WHERE id = 'p1'",
    );
    exec(&conn, "DELETE FROM spending_plans WHERE id = 'p1'");

    let row = only_row(&mut conn);

    assert_eq!(
        row,
        RowChange {
            table: "spending_plans".into(),
            key: RowKey::Single("p1".into()),
            generation: clock(&conn, "spending_plans", "p1", "*").unwrap(),
            insert: false,
            deleted: Some(clock(&conn, "spending_plans", "p1", "-").unwrap()),
            columns: BTreeMap::new(),
            parents: BTreeMap::new(),
        }
    );
}

#[test]
fn every_change_is_a_full_row_with_per_column_clocks() {
    let mut conn = enabled();
    plan(&conn, "p1");
    exec(
        &conn,
        "UPDATE spending_plans SET note = 'second' WHERE id = 'p1'",
    );

    let row = only_row(&mut conn);

    let generation = clock(&conn, "spending_plans", "p1", "*").unwrap();
    let note = clock(&conn, "spending_plans", "p1", "note").unwrap();
    assert!(note > generation);
    assert!(row.insert);
    assert_eq!(row.deleted, None);
    assert_eq!(row.generation, generation);
    assert_eq!(column_names(&row), PLAN_COLUMNS);
    assert_eq!(row.columns["note"], (text("second"), note));
    assert_eq!(row.columns["name"], (text("Holiday p1"), generation));
    assert_eq!(row.columns["year"], (SqlValue::Integer(2026), generation));
    assert_eq!(row.columns["closed_at"], (SqlValue::Null, generation));
    for (name, (_, column_clock)) in &row.columns {
        if name != "note" {
            assert_eq!(*column_clock, generation, "{name}");
        }
    }
}

#[test]
fn a_row_that_never_had_a_clock_travels_with_the_zero_clock() {
    let mut conn = migrated();
    plan(&conn, "p1");
    enable(&conn);
    exec(
        &conn,
        "UPDATE spending_plans SET note = 'after' WHERE id = 'p1'",
    );

    let built = drain(&mut conn);

    assert_eq!(built.len(), 1);
    let row = &built[0].changeset.rows[0];
    assert!(row.insert);
    assert_eq!(row.generation, Clock::ZERO);
    assert_eq!(column_names(row), PLAN_COLUMNS);
    let note = clock(&conn, "spending_plans", "p1", "note").unwrap();
    assert_eq!(note.device, DEVICE);
    assert_eq!(row.columns["note"], (text("after"), note));
    for (name, (_, column_clock)) in &row.columns {
        if name != "note" {
            assert_eq!(*column_clock, Clock::ZERO, "{name}");
        }
    }
    let json: serde_json::Value = serde_json::from_slice(&to_json(&built[0].changeset)).unwrap();
    assert_eq!(
        json["devs"],
        serde_json::json!([DEVICE.to_b64(), DeviceId::ZERO.to_b64()])
    );
}

#[test]
fn full_rows_carry_typed_values_and_derived_values_alike() {
    let mut conn = enabled();
    exec(
        &conn,
        "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency) VALUES \
           ('s-typed', 'AAA', 'Typed Co', '7', 'USD'), \
           ('s-derived', 'BBB', 'Derived Co', '0', 'USD'); \
         INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, \
           quantity, price_per_unit, currency, transaction_date) VALUES \
           ('it-1', 's-derived', 'buy', 'BBB', 'Derived Co', '3', '10', 'USD', 1767225600), \
           ('it-2', 's-derived', 'buy', 'BBB', 'Derived Co', '2', '11', 'USD', 1767312000);",
    );
    investments::recalculate_investment_metrics(&conn, "s-derived").unwrap();
    account(&conn, "acc");

    let rows = rows_by_key(&drain(&mut conn));

    let typed = &rows[&key("stock_investments", "s-typed")];
    let derived = &rows[&key("stock_investments", "s-derived")];
    let stock_columns = ["company_name", "currency", "quantity", "ticker"];
    assert_eq!(column_names(typed), stock_columns);
    assert_eq!(column_names(derived), stock_columns);
    assert_eq!(typed.columns["quantity"].0, text("7"));
    assert_eq!(derived.columns["quantity"].0, text("5"));
    // bank_accounts: `updated_at` is Local, the recomputed-or-typed balance travels.
    let bank = &rows[&key("bank_accounts", "acc")];
    assert!(!bank.columns.contains_key("updated_at"));
    assert!(!bank.columns.contains_key("id"));
    assert_eq!(bank.columns["balance"].0, text("125.00"));
    assert_eq!(bank.columns["balance_as_of"].0, SqlValue::Null);
    assert_eq!(bank.columns.len(), 17);
}

#[test]
fn a_deleted_and_recreated_row_carries_del_and_ins() {
    let mut conn = enabled();
    account(&conn, "acc");
    exec(
        &conn,
        "INSERT INTO transaction_categories (id, name) VALUES ('cat', 'Coffee')",
    );
    let engine = CategorizationEngine::new_empty();
    let data = InsertScopedLearning {
        bank_account_id: "acc".into(),
        is_credit: false,
        payee: Some("Coffee Shop".into()),
        counterparty_iban: None,
        counterparty_bank_code: None,
        counterparty_country: None,
        category_id: "cat".into(),
    };
    learn_scoped(&conn, &engine, &data).unwrap();
    learn_scoped(&conn, &engine, &data).unwrap();
    exec(
        &conn,
        "DELETE FROM sync_outbox WHERE tbl <> 'learned_payees'",
    );

    let row = only_row(&mut conn);

    let pk = row.key.to_pk_text();
    let tombstone = clock(&conn, "learned_payees", &pk, "-").unwrap();
    let generation = clock(&conn, "learned_payees", &pk, "*").unwrap();
    assert_eq!(row.table, "learned_payees");
    assert!(row.insert);
    assert_eq!(row.deleted, Some(tombstone));
    assert_eq!(row.generation, generation);
    assert!(generation > tombstone);
    assert_eq!(row.columns["category_id"].0, text("cat"));
}

#[test]
fn every_fk_value_carries_its_parent_generation() {
    let mut conn = migrated();
    exec(
        &conn,
        "INSERT INTO transaction_categories (id, name) VALUES ('cat', 'Food')",
    );
    enable(&conn);
    account(&conn, "acc");
    exec(
        &conn,
        "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, \
           booking_date, category_id) VALUES ('t1', 'acc', 'debit', '-5.00', 'CZK', 1767225600, 'cat')",
    );

    let rows = rows_by_key(&drain(&mut conn));

    let account_generation = clock(&conn, "bank_accounts", "acc", "*").unwrap();
    assert_eq!(
        rows[&key("bank_transactions", "t1")].parents,
        BTreeMap::from([
            ("bank_account_id".to_string(), account_generation),
            ("category_id".to_string(), Clock::ZERO),
        ])
    );
    assert_eq!(rows[&key("bank_accounts", "acc")].parents, BTreeMap::new());
}

#[test]
fn a_repair_renamed_row_travels_with_its_typed_name() {
    let mut conn = enabled();
    exec(
        &conn,
        "INSERT INTO stock_tags (id, name) VALUES ('tag-a', 'Growth (2)'), ('tag-b', 'Value'); \
         INSERT INTO sync_typed (tbl, pk, col, typed, assigned) VALUES \
           ('stock_tags', 'tag-a', 'name', 'Growth', 'Growth (2)'), \
           ('stock_tags', 'tag-b', 'name', 'Dividend', 'Dividend (2)');",
    );

    let rows = rows_by_key(&drain(&mut conn));

    let generation = clock(&conn, "stock_tags", "tag-a", "*").unwrap();
    assert_eq!(
        rows[&key("stock_tags", "tag-a")].columns["name"],
        (text("Growth"), generation)
    );
    // A stale entry (the stored name is no longer the assigned one) is ignored.
    assert_eq!(
        rows[&key("stock_tags", "tag-b")].columns["name"].0,
        text("Value")
    );
}

#[test]
fn a_repair_cleared_link_travels_with_its_typed_link() {
    let mut conn = enabled();
    exec(
        &conn,
        "INSERT INTO cashflow_items (id, name, amount, frequency, item_type, replacement_source_id) \
           VALUES ('src', 'S', '1', 'monthly', 'expense', NULL), \
                  ('item-a', 'A', '1', 'monthly', 'expense', 'src'), \
                  ('item-b', 'B', '1', 'monthly', 'expense', NULL); \
         INSERT INTO sync_typed (tbl, pk, col, typed, assigned) VALUES \
           ('cashflow_items', 'item-b', 'replacement_source_id', 'src', NULL);",
    );

    let rows = rows_by_key(&drain(&mut conn));

    let column = |pk: &str| {
        rows[&key("cashflow_items", pk)].columns["replacement_source_id"]
            .0
            .clone()
    };
    assert_eq!(column("item-a"), text("src"));
    assert_eq!(column("item-b"), text("src"));
    assert_eq!(column("src"), SqlValue::Null);
}

#[test]
fn parents_travel_before_children_and_children_tombstones_before_parents() {
    let mut conn = migrated();
    exec(
        &conn,
        "INSERT INTO transaction_categories (id, name) VALUES ('cat-p', 'Parent'); \
         INSERT INTO transaction_categories (id, name, parent_id) VALUES ('cat-c', 'Child', 'cat-p'); \
         INSERT INTO stock_investments (id, ticker, company_name, quantity, currency) \
           VALUES ('s1', 'AAA', 'A Co', '0', 'USD'); \
         INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, \
           quantity, price_per_unit, currency, transaction_date) VALUES \
           ('it-1', 's1', 'buy', 'AAA', 'A Co', '1', '10', 'USD', 1767225600), \
           ('it-2', 's1', 'buy', 'AAA', 'A Co', '1', '10', 'USD', 1767312000);",
    );
    enable(&conn);
    // The child category is queued before its parent.
    exec(
        &conn,
        "UPDATE transaction_categories SET icon = 'c' WHERE id = 'cat-c'; \
         UPDATE transaction_categories SET icon = 'p' WHERE id = 'cat-p'; \
         DELETE FROM stock_investments WHERE id = 's1';",
    );
    account(&conn, "acc");
    csv_import::insert_batch(&conn, "batch", "acc", "statement.csv", None, None).unwrap();
    exec(
        &conn,
        "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, \
           booking_date, import_batch_id) VALUES ('bt', 'acc', 'debit', '-5.00', 'CZK', 1767225600, 'batch')",
    );

    let built = drain(&mut conn);

    assert_eq!(built.len(), 1);
    assert_eq!(
        keys(&built[0]),
        vec![
            key("transaction_categories", "cat-p"),
            key("transaction_categories", "cat-c"),
            key("bank_accounts", "acc"),
            key("csv_import_batches", "batch"),
            key("bank_transactions", "bt"),
            key("investment_transactions", "it-1"),
            key("investment_transactions", "it-2"),
            key("stock_investments", "s1"),
        ]
    );
}

const BANK_TRANSACTION_COLUMNS: [&str; 26] = [
    "amount",
    "bank_account_id",
    "booking_date",
    "categorization_source",
    "categorize_snoozed_until",
    "category_id",
    "counterparty_bank_code",
    "counterparty_country",
    "counterparty_iban",
    "counterparty_name",
    "created_at",
    "currency",
    "data_source",
    "description",
    "flow_kind",
    "import_batch_id",
    "import_line",
    "merchant_category_code",
    "remittance_info",
    "row_balance",
    "spending_plan_id",
    "status",
    "suggested_category_id",
    "transaction_id",
    "tx_type",
    "value_date",
];

#[test]
fn a_5000_row_import_is_cut_into_envelopes_under_60_kib_with_batch_first() {
    let mut conn = migrated();
    account(&conn, "acc");
    enable(&conn);
    {
        let tx = conn.transaction().unwrap();
        csv_import::insert_batch(&tx, "batch", "acc", "statement.csv", None, None).unwrap();
        for i in 0..5_000 {
            let data = InsertBankTransaction {
                bank_account_id: "acc".into(),
                transaction_id: Some(format!("BANK-{i:05}")),
                tx_type: "debit".into(),
                flow_kind: None,
                amount: format!("{}.{:02}", 10 + i, i % 100),
                currency: Some("CZK".into()),
                description: Some(format!("Card payment {i}")),
                counterparty_name: Some(format!("Merchant {}", i % 37)),
                counterparty_iban: None,
                counterparty_bank_code: None,
                counterparty_country: None,
                booking_date: DAY + (i as i64 % 300) * 86_400,
                value_date: None,
                category_id: None,
                status: None,
            };
            bank_accounts::create_transaction_with_origin(&tx, &data, "csv", Some("batch"), None)
                .unwrap();
        }
        tx.commit().unwrap();
    }
    exec(
        &conn,
        "UPDATE csv_import_batches SET imported_count = 5000 WHERE id = 'batch'",
    );
    let mut expected: Vec<(String, String)> = conn
        .prepare("SELECT 'bank_transactions', id FROM bank_transactions")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    expected.push(key("csv_import_batches", "batch"));
    expected.sort();

    let built = drain(&mut conn);

    assert!(built.len() > 1, "{} envelope(s)", built.len());
    for (i, b) in built.iter().enumerate() {
        let len = padded_deflated_len(&to_json(&b.changeset));
        assert!(len <= ENVELOPE_BUDGET, "envelope {i}: {len}");
        assert_eq!(b.changeset.seq, i as u64 + 1);
    }
    assert_eq!(keys(&built[0])[0], key("csv_import_batches", "batch"));
    let mut sent: Vec<(String, String)> = built.iter().flat_map(keys).collect();
    sent.sort();
    assert_eq!(sent, expected);
    assert_eq!(outbox(&conn), Vec::<(String, String)>::new());
    let batch_generation = clock(&conn, "csv_import_batches", "batch", "*").unwrap();
    for row in built.iter().flat_map(|b| b.changeset.rows.iter()) {
        if row.table == "bank_transactions" {
            assert!(row.insert);
            assert_eq!(column_names(row), BANK_TRANSACTION_COLUMNS);
            assert_eq!(row.parents["import_batch_id"], batch_generation);
        }
    }
}

#[test]
fn the_largest_allowed_row_travels_alone() {
    let mut conn = enabled();
    exec(
        &conn,
        "INSERT INTO watched_stocks (id, ticker) VALUES ('w1', 'AAPL')",
    );
    stock_monitor::update_notes(&conn, "AAPL", noise(100 * 1024, 11)).unwrap();

    let built = drain(&mut conn);

    assert_eq!(built.len(), 1);
    assert_eq!(keys(&built[0]), vec![key("watched_stocks", "w1")]);
    let len = padded_deflated_len(&to_json(&built[0].changeset));
    assert!(
        len > ENVELOPE_BUDGET && len <= SINGLE_ROW_LIMIT,
        "padded length {len}"
    );
}

#[test]
fn a_row_over_180_kib_is_refused_and_stays_queued() {
    let mut conn = enabled();
    exec(
        &conn,
        "INSERT INTO watched_stocks (id, ticker) VALUES ('w1', 'AAPL')",
    );
    conn.execute(
        "UPDATE watched_stocks SET notes = ?1 WHERE id = 'w1'",
        [noise(300 * 1024, 29)],
    )
    .unwrap();
    plan(&conn, "p1");

    let (built, refused) = round(&mut conn);

    assert_eq!(refused.len(), 1);
    assert_eq!(
        (refused[0].table.as_str(), refused[0].pk.as_str()),
        ("watched_stocks", "w1")
    );
    assert!(refused[0].bytes > SINGLE_ROW_LIMIT, "{}", refused[0].bytes);
    assert_eq!(keys(&built.unwrap()), vec![key("spending_plans", "p1")]);
    assert_eq!(outbox(&conn), vec![key("watched_stocks", "w1")]);

    let (built, refused_again) = round(&mut conn);

    assert_eq!(built, None);
    assert_eq!(refused_again, refused);
    assert_eq!(outbox(&conn), vec![key("watched_stocks", "w1")]);
}

#[test]
fn build_next_consumes_the_outbox_in_its_transaction() {
    let mut conn = enabled();
    plan(&conn, "p1");
    plan(&conn, "p2");

    {
        let tx = conn.transaction().unwrap();
        let (built, _) = build_next(&tx).unwrap();
        assert_eq!(
            keys(&built.unwrap()),
            vec![key("spending_plans", "p1"), key("spending_plans", "p2")]
        );
        assert_eq!(outbox(&tx), Vec::<(String, String)>::new());
        tx.rollback().unwrap();
    }

    assert_eq!(
        outbox(&conn),
        vec![key("spending_plans", "p1"), key("spending_plans", "p2")]
    );
}

#[test]
fn record_sealed_advances_the_chain() {
    let mut conn = enabled();
    plan(&conn, "p1");
    let hash = [0xAB; 32];

    let tx = conn.transaction().unwrap();
    let (built, _) = build_next(&tx).unwrap();
    let built = built.unwrap();
    assert_eq!(built.changeset.seq, 1);
    assert_eq!(built.changeset.prev, None);
    assert_eq!(built.changeset.device, DEVICE);
    assert_eq!(built.changeset.schema, schema_number());
    assert_eq!(built.max_hlc, max_hlc(&built.changeset));
    assert_eq!(
        built.max_hlc,
        clock(&tx, "spending_plans", "p1", "*").unwrap().hlc
    );
    record_sealed(&tx, &built, hash).unwrap();
    tx.commit().unwrap();

    let log: Vec<(i64, Vec<u8>, i64)> = conn
        .prepare("SELECT seq, hash, max_hlc FROM sync_own_log ORDER BY seq")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();
    assert_eq!(log, vec![(1, hash.to_vec(), built.max_hlc as i64)]);
    let sync = state::load(&conn).unwrap().unwrap();
    assert_eq!(sync.next_seq, 2);
    assert_eq!(sync.last_hash, Some(hash));

    plan(&conn, "p2");
    let tx = conn.transaction().unwrap();
    let (next, _) = build_next(&tx).unwrap();
    let next = next.unwrap();
    assert_eq!(next.changeset.seq, 2);
    assert_eq!(next.changeset.prev, Some(hash));
}

#[test]
fn an_edit_after_the_commit_requeues_the_row() {
    let mut conn = enabled();
    plan(&conn, "p1");
    assert_eq!(drain(&mut conn).len(), 1);
    assert_eq!(outbox(&conn), Vec::<(String, String)>::new());

    exec(
        &conn,
        "UPDATE spending_plans SET note = 'later' WHERE id = 'p1'",
    );

    assert_eq!(outbox(&conn), vec![key("spending_plans", "p1")]);
    let row = only_row(&mut conn);
    assert_eq!(row.columns["note"].0, text("later"));
}

#[test]
fn a_queued_key_with_neither_row_nor_clock_is_dropped_from_the_outbox() {
    let mut conn = enabled();
    exec(
        &conn,
        "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES ('spending_plans', 'ghost', 1)",
    );

    assert_eq!(read_row(&conn, "spending_plans", "ghost").unwrap(), None);
    let (built, refused) = round(&mut conn);

    assert_eq!((built, refused), (None, Vec::new()));
    assert_eq!(outbox(&conn), Vec::<(String, String)>::new());
}

#[test]
fn queued_keys_of_tables_that_do_not_sync_are_dropped_not_fatal() {
    let mut conn = enabled();
    exec(
        &conn,
        "INSERT INTO sync_outbox (tbl, pk, queued_at) VALUES \
           ('exchange_rates', 'EUR', 1), ('no_such_table', 'x', 2)",
    );
    plan(&conn, "p1");

    let (built, refused) = round(&mut conn);

    assert_eq!(keys(&built.unwrap()), vec![key("spending_plans", "p1")]);
    assert_eq!(refused, Vec::new());
    assert_eq!(outbox(&conn), Vec::<(String, String)>::new());
}

#[test]
fn a_refused_row_holds_back_the_rows_that_reference_it() {
    let mut conn = enabled();
    // No service caps a position's company name, so SQL can make the parent itself too large.
    conn.execute(
        "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency) \
         VALUES ('s1', 'AAA', ?1, '0', 'USD')",
        [noise(300 * 1024, 41)],
    )
    .unwrap();
    exec(
        &conn,
        "INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, \
           quantity, price_per_unit, currency, transaction_date) VALUES \
           ('it-1', 's1', 'buy', 'AAA', 'A Co', '1', '10', 'USD', 1767225600); \
         INSERT INTO stock_tags (id, name) VALUES ('t1', 'Growth'); \
         INSERT INTO stock_investment_tags (investment_id, tag_id) VALUES ('s1', 't1');",
    );
    plan(&conn, "p1");

    let (built, refused) = round(&mut conn);

    assert_eq!(
        refused
            .iter()
            .map(|r| (r.table.as_str(), r.pk.as_str()))
            .collect::<Vec<_>>(),
        vec![("stock_investments", "s1")]
    );
    assert_eq!(
        keys(&built.unwrap()),
        vec![key("spending_plans", "p1"), key("stock_tags", "t1")]
    );
    assert_eq!(
        outbox(&conn),
        vec![
            key("investment_transactions", "it-1"),
            key("stock_investment_tags", r#"["s1","t1"]"#),
            key("stock_investments", "s1"),
        ]
    );
    let (built, refused_again) = round(&mut conn);
    assert_eq!((built, refused_again), (None, refused));
}

#[test]
fn a_composite_link_row_carries_its_key_parents_and_cascaded_tombstone() {
    let mut conn = enabled();
    exec(
        &conn,
        "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency) \
           VALUES ('s1', 'AAA', 'A Co', '0', 'USD'); \
         INSERT INTO stock_tags (id, name) VALUES ('t1', 'Growth'); \
         INSERT INTO stock_investment_tags (investment_id, tag_id) VALUES ('s1', 't1');",
    );
    let link_pk = r#"["s1","t1"]"#;

    let rows = rows_by_key(&drain(&mut conn));

    let link = &rows[&key("stock_investment_tags", link_pk)];
    assert_eq!(link.key, RowKey::Composite(vec!["s1".into(), "t1".into()]));
    assert!(link.insert);
    let generation = clock(&conn, "stock_investment_tags", link_pk, "*").unwrap();
    assert_eq!(
        link.columns.keys().map(String::as_str).collect::<Vec<_>>(),
        vec!["created_at"]
    );
    assert_eq!(link.columns["created_at"].1, generation);
    assert_eq!(
        link.parents,
        BTreeMap::from([
            (
                "investment_id".to_string(),
                clock(&conn, "stock_investments", "s1", "*").unwrap()
            ),
            (
                "tag_id".to_string(),
                clock(&conn, "stock_tags", "t1", "*").unwrap()
            ),
        ])
    );

    exec(&conn, "DELETE FROM stock_tags WHERE id = 't1'");
    let built = drain(&mut conn);

    assert_eq!(built.len(), 1);
    assert_eq!(
        keys(&built[0]),
        vec![
            key("stock_investment_tags", link_pk),
            key("stock_tags", "t1")
        ]
    );
    assert!(built[0]
        .changeset
        .rows
        .iter()
        .all(|r| !r.insert && r.deleted.is_some()));
}
