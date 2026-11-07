//! Unique keys (§7.3, D15, corrections 15 and 17, rulings U1–U4): same ID (with the keep-newest
//! fallback by the key clock), rename, unlink. The merge writes the outcome directly, so no
//! changeset trips a UNIQUE constraint. Interleavings and transient collisions:
//! `unique_interleavings`.

use super::*;
use crate::models::bank_accounts::InsertBalanceRecord;
use crate::models::budgeting::InsertBudgetGoal;
use crate::services::{bank_accounts, budgeting, investments};

pub(super) const DAY: i64 = 1_767_225_600; // 2026-01-01
const MONTH_1: i64 = 1_767_225_600; // 2026-01
const MONTH_2: i64 = 1_769_904_000; // 2026-02

/// Device A creates the parents the conflicting rows need (account `acc`, category `cat`) and
/// every other device receives them.
pub(super) fn shared_parents(a: &Connection, others: &[&Connection]) {
    account(a, "acc");
    category(a, "cat");
    let base = send(a);
    for conn in others {
        apply(conn, &base);
    }
}

/// `acc`'s balance on `day` through the service (natural id, U1).
pub(super) fn balance(conn: &Connection, day: i64, amount: &str) {
    bank_accounts::record_balance(
        conn,
        "acc",
        &InsertBalanceRecord {
            amount: amount.into(),
            currency: "CZK".into(),
            as_of: day,
            confirm_unknown_as_of: true,
            source_batch_id: None,
        },
    )
    .expect("record balance");
}

/// A balance record with a random id, as created before U1, the mirror refreshed.
pub(super) fn legacy_balance(conn: &Connection, id: &str, day: i64, amount: &str) {
    conn.execute(
        "INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of, recorded_at) \
         VALUES (?1, 'acc', ?2, 'CZK', ?3, 100)",
        rusqlite::params![id, amount, day],
    )
    .expect("insert balance record");
    bank_accounts::refresh_balance_columns(conn, "acc").expect("refresh balance");
}

/// `cat`'s monthly limit from `MONTH_1` through the service (natural id, U1).
pub(super) fn version(conn: &Connection, amount: &str) {
    budgeting::upsert_goal_at(
        conn,
        InsertBudgetGoal {
            category_id: "cat".into(),
            timeframe: "monthly".into(),
            amount: amount.into(),
            currency: Some("CZK".into()),
            effective_month: Some(MONTH_1),
            effective_until: None,
        },
        NOW,
    )
    .expect("upsert goal");
}

pub(super) fn tag(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO stock_tags (id, name, created_at) VALUES (?1, ?2, 100)",
        [id, name],
    )
    .expect("insert tag");
}

pub(super) fn tag_group(conn: &Connection, id: &str, name: &str) {
    conn.execute(
        "INSERT INTO stock_tag_groups (id, name, created_at) VALUES (?1, ?2, 100)",
        [id, name],
    )
    .expect("insert tag group");
}

pub(super) fn cashflow_item(conn: &Connection, id: &str, replaces: Option<&str>) {
    conn.execute(
        "INSERT INTO cashflow_items (id, name, amount, currency, frequency, item_type, created_at, \
           updated_at, replacement_source_id) \
         VALUES (?1, 'Item ' || ?1, '10.00', 'CZK', 'monthly', 'expense', 100, 100, ?2)",
        rusqlite::params![id, replaces],
    )
    .expect("insert cashflow item");
}

fn learned(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO learned_payees (id, normalized_payee, original_payee, category_id, \
           bank_account_id, is_credit, created_at, updated_at) \
         VALUES (?1, 'albert', 'Albert', 'cat', 'acc', 0, 100, 100)",
        [id],
    )
    .expect("insert learned payee");
}

pub(super) fn listed(conn: &Connection, sql: &str) -> Vec<String> {
    let mut stmt = conn.prepare(sql).expect(sql);
    stmt.query_map([], |r| r.get::<_, String>(0))
        .expect(sql)
        .map(|r| r.expect("row"))
        .collect()
}

/// Every `sync_typed` entry as `"tbl pk col typed -> assigned"` (`-` for a cleared link).
pub(super) fn typed(conn: &Connection) -> Vec<String> {
    listed(
        conn,
        "SELECT tbl || ' ' || pk || ' ' || col || ' ' || typed || ' -> ' \
           || COALESCE(assigned, '-') FROM sync_typed ORDER BY tbl, pk, col",
    )
}

/// `"id link"` of every cash-flow item (`-` for none).
pub(super) fn item_links(conn: &Connection) -> Vec<String> {
    listed(
        conn,
        "SELECT id || ' ' || COALESCE(replacement_source_id, '-') FROM cashflow_items ORDER BY id",
    )
}

pub(super) fn warnings(reports: &[ApplyReport]) -> Vec<String> {
    reports.iter().flat_map(|r| r.warnings.clone()).collect()
}

#[test]
fn same_id_two_devices_buying_aapl_offline_share_one_position() {
    let a = device(A);
    let b = device(B);
    investments::import_single_transaction(&a, "AAPL", "Apple Inc.", "buy", "1", "100", "USD", DAY)
        .expect("buy on A");
    later(&b, &a);
    investments::import_single_transaction(
        &b,
        "aapl",
        "Apple Inc.",
        "buy",
        "2",
        "110",
        "USD",
        DAY + 86_400,
    )
    .expect("buy on B");
    let from_a = send(&a);
    let from_b = send(&b);

    let on_a = apply(&a, &from_b);
    let on_b = apply(&b, &from_a);

    let position = natural_id("stock_investments", &["AAPL"]);
    for conn in [&a, &b] {
        assert_eq!(
            listed(conn, "SELECT id || ' ' || quantity FROM stock_investments"),
            vec![format!("{position} 3")]
        );
        assert_eq!(
            count(
                conn,
                &format!(
                    "SELECT COUNT(*) FROM investment_transactions WHERE investment_id = '{position}'"
                )
            ),
            2
        );
    }
    assert!(warnings(&on_a).is_empty() && warnings(&on_b).is_empty());
    assert_converged(&[&a, &b]);
}

#[test]
fn balance_records_of_the_same_day_share_one_id() {
    let a = device(A);
    let b = device(B);
    shared_parents(&a, &[&b]);
    balance(&a, DAY, "100.00");
    later(&b, &a);
    balance(&b, DAY, "200.00");
    let from_a = send(&a);
    let from_b = send(&b);

    let on_a = apply(&a, &from_b);
    let on_b = apply(&b, &from_a);

    let id = bank_accounts::balance_record_id("acc", DAY);
    for conn in [&a, &b] {
        assert_eq!(
            listed(conn, "SELECT id || ' ' || amount FROM bank_balance_records"),
            vec![format!("{id} 200.00")]
        );
        assert_eq!(
            value(conn, "SELECT balance FROM bank_accounts WHERE id = 'acc'").as_deref(),
            Some("200.00")
        );
        assert!(tombstones(conn).is_empty());
    }
    assert!(warnings(&on_a).is_empty() && warnings(&on_b).is_empty());
    assert_converged(&[&a, &b]);
}

/// The SameId fallback for two ids on one key (legacy ids, unreachable after U1 and D22). V1
/// (generation 10) moved onto (cat, monthly, month 2) by an update at 50; V2 created there at 30
/// and its amount edited at 70, in separate changesets. V1's key clock is 50, V2's is 30 (an edit
/// of another column does not make a row win its key): V1 stays on both devices, with a warning.
#[test]
fn keep_newest_uses_the_clock_that_put_the_row_on_its_key() {
    let at = |hlc: u64, device: DeviceId| clock(hlc << 16, device);
    let cat = row(
        "transaction_categories",
        "cat",
        at(5, C),
        &[("name", text("Food"))],
    );
    let v1 = |month: i64| {
        row(
            "budget_goal_versions",
            "v1",
            at(10, C),
            &[
                ("amount", text("100.00")),
                ("category_id", text("cat")),
                ("created_at", SqlValue::Integer(100)),
                ("currency", text("CZK")),
                ("effective_month", SqlValue::Integer(month)),
                ("timeframe", text("monthly")),
            ],
        )
    };
    let base = changeset(C, 1, vec![cat, v1(MONTH_1)]);
    let mut moved = v1(MONTH_2);
    set(
        &mut moved,
        "effective_month",
        SqlValue::Integer(MONTH_2),
        at(50, A),
    );
    let from_a = changeset(A, 1, vec![moved]);
    let mut v2 = row(
        "budget_goal_versions",
        "v2",
        at(30, B),
        &[
            ("amount", text("200.00")),
            ("category_id", text("cat")),
            ("created_at", SqlValue::Integer(100)),
            ("currency", text("CZK")),
            ("effective_month", SqlValue::Integer(MONTH_2)),
            ("timeframe", text("monthly")),
        ],
    );
    set(&mut v2, "amount", text("250.00"), at(70, B));
    let from_b = changeset(B, 1, vec![v2]);

    let x = device(D);
    let y = device(D);
    let on_x = apply(&x, &vec![base.clone(), from_a.clone(), from_b.clone()]);
    let on_y = apply(&y, &vec![base, from_b, from_a]);

    for conn in [&x, &y] {
        assert_eq!(
            listed(
                conn,
                "SELECT id || ' ' || effective_month || ' ' || amount FROM budget_goal_versions"
            ),
            vec![format!("v1 {MONTH_2} 100.00")]
        );
        assert_eq!(clocks(conn, "budget_goal_versions", "v2")["-"], at(50, A));
    }
    for reports in [&on_x, &on_y] {
        let warnings = warnings(reports);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(warnings[0].contains("budget_goal_versions"), "{warnings:?}");
    }
    assert_converged(&[&x, &y]);
}

#[test]
fn budget_versions_of_the_same_month_share_one_id() {
    let a = device(A);
    let b = device(B);
    shared_parents(&a, &[&b]);
    version(&a, "100.00");
    later(&b, &a);
    version(&b, "300.00");
    let from_a = send(&a);
    let from_b = send(&b);

    apply(&a, &from_b);
    apply(&b, &from_a);

    let id = budgeting::version_id("cat", "monthly", MONTH_1);
    for conn in [&a, &b] {
        assert_eq!(
            listed(conn, "SELECT id || ' ' || amount FROM budget_goal_versions"),
            vec![format!("{id} 300.00")]
        );
        assert!(tombstones(conn).is_empty());
        assert_eq!(
            value(
                conn,
                "SELECT amount FROM budget_goals WHERE category_id = 'cat'"
            )
            .as_deref(),
            Some("300.00")
        );
    }
    assert_converged(&[&a, &b]);
}

#[test]
fn rename_tags_with_the_same_name_by_id() {
    let a = device(A);
    let b = device(B);
    tag(&a, "tag-b", "Growth");
    later(&b, &a);
    tag(&b, "tag-a", "Growth");
    let from_a = send(&a);
    let from_b = send(&b);

    apply(&a, &from_b);
    apply(&b, &from_a);

    for conn in [&a, &b] {
        assert_eq!(
            listed(conn, "SELECT id || ' ' || name FROM stock_tags ORDER BY id"),
            vec!["tag-a Growth", "tag-b Growth (2)"]
        );
        assert_eq!(
            typed(conn),
            vec!["stock_tags tag-b name Growth -> Growth (2)"]
        );
    }
    assert_converged(&[&a, &b]);
}

#[test]
fn rename_tag_groups_also_on_a_rename_update() {
    let a = device(A);
    let b = device(B);
    tag_group(&a, "grp-b", "Sector");
    apply(&b, &send(&a));
    exec(
        &a,
        "UPDATE stock_tag_groups SET name = 'Region' WHERE id = 'grp-b'",
    );
    later(&b, &a);
    tag_group(&b, "grp-a", "Region");
    let from_a = send(&a);
    let from_b = send(&b);
    assert_eq!(rows_of(&from_a)[0].columns["name"].0, text("Region"));

    apply(&a, &from_b);
    apply(&b, &from_a);

    for conn in [&a, &b] {
        assert_eq!(
            listed(
                conn,
                "SELECT id || ' ' || name FROM stock_tag_groups ORDER BY id"
            ),
            vec!["grp-a Region", "grp-b Region (2)"]
        );
        assert_eq!(
            typed(conn),
            vec!["stock_tag_groups grp-b name Region -> Region (2)"]
        );
    }
    assert_converged(&[&a, &b]);
}

#[test]
fn unlink_two_items_replacing_the_same_source() {
    let a = device(A);
    let b = device(B);
    cashflow_item(&a, "item-b", Some("loan-1"));
    later(&b, &a);
    cashflow_item(&b, "item-a", Some("loan-1"));
    let from_a = send(&a);
    let from_b = send(&b);

    apply(&a, &from_b);
    apply(&b, &from_a);

    for conn in [&a, &b] {
        assert_eq!(item_links(conn), vec!["item-a loan-1", "item-b -"]);
        assert_eq!(
            typed(conn),
            vec!["cashflow_items item-b replacement_source_id loan-1 -> -"]
        );
    }
    assert_converged(&[&a, &b]);
}

/// A row created before sync keeps its random id, so a second device's natural-id row of the
/// same scope collides (impossible after D22): kept as keep-newest, with a warning.
#[test]
fn learned_payee_scope_rows_with_random_and_natural_ids_keep_the_newer() {
    let a = device(A);
    let b = device(B);
    shared_parents(&a, &[&b]);
    learned(&a, "random-1");
    later(&b, &a);
    let natural = natural_id("learned_payees", &["albert", "", "acc", "0"]);
    learned(&b, &natural);
    let newer = clocks(&b, "learned_payees", &natural)["*"];
    let from_a = send(&a);
    let from_b = send(&b);

    let on_a = apply(&a, &from_b);
    let on_b = apply(&b, &from_a);

    for conn in [&a, &b] {
        assert_eq!(
            listed(conn, "SELECT id FROM learned_payees"),
            vec![natural.clone()]
        );
        assert_eq!(clocks(conn, "learned_payees", "random-1")["-"], newer);
    }
    for reports in [&on_a, &on_b] {
        let warnings = warnings(reports);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        assert!(
            warnings[0].contains("learned_payees") && warnings[0].contains(&natural),
            "{warnings:?}"
        );
    }
    assert_converged(&[&a, &b]);
}

#[test]
fn every_outcome_is_the_same_in_both_orders() {
    let a = device(A);
    let b = device(B);
    let c = device(C);
    let d = device(D);
    shared_parents(&a, &[&b, &c, &d]);
    balance(&a, DAY, "100.00");
    version(&a, "100.00");
    tag(&a, "tag-a", "Value");
    cashflow_item(&a, "item-a", Some("loan-1"));
    later(&b, &a);
    balance(&b, DAY, "200.00");
    version(&b, "200.00");
    tag(&b, "tag-b", "Value");
    cashflow_item(&b, "item-b", Some("loan-1"));
    let from_a = send(&a);
    let from_b = send(&b);

    apply(&a, &from_b);
    apply(&b, &from_a);
    apply(&c, &from_a);
    apply(&c, &from_b);
    apply(&d, &from_b);
    apply(&d, &from_a);

    for conn in [&a, &b, &c, &d] {
        assert_eq!(
            listed(conn, "SELECT amount FROM bank_balance_records"),
            vec!["200.00"]
        );
        assert_eq!(
            listed(conn, "SELECT amount FROM budget_goal_versions"),
            vec!["200.00"]
        );
        assert_eq!(
            listed(conn, "SELECT id || ' ' || name FROM stock_tags ORDER BY id"),
            vec!["tag-a Value", "tag-b Value (2)"]
        );
        assert_eq!(item_links(conn), vec!["item-a loan-1", "item-b -"]);
    }
    assert_converged(&[&a, &b, &c, &d]);
}
