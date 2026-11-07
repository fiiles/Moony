//! Tombstones and delete epochs (D13, the R2 amendment, worked example 2).

use super::*;
use crate::services::investments;

#[test]
fn d13_a_tombstone_beats_a_later_edit_of_the_same_generation() {
    let a = device(A);
    let b = device(B);
    plan(&a, "p1", 2026);
    apply(&b, &send(&a));
    exec(&a, "DELETE FROM spending_plans WHERE id = 'p1'");
    let tombstone = clocks(&a, "spending_plans", "p1")["-"];
    capture::observe(&b, tombstone.hlc).expect("observe");
    exec(
        &b,
        "UPDATE spending_plans SET note = 'late edit' WHERE id = 'p1'",
    );
    assert!(clocks(&b, "spending_plans", "p1")["note"] > tombstone);
    let from_a = send(&a);
    let from_b = send(&b);

    let on_a = apply(&a, &from_b);
    apply(&b, &from_a);

    assert_eq!((on_a[0].applied, on_a[0].dropped), (0, 1));
    for conn in [&a, &b] {
        assert_eq!(count(conn, "SELECT COUNT(*) FROM spending_plans"), 0);
        assert_eq!(clocks(conn, "spending_plans", "p1")["-"], tombstone);
    }
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn d13_an_insert_newer_than_the_tombstone_recreates_the_row() {
    let a = device(A);
    let b = device(B);
    watch(&a, "first", None);
    apply(&b, &send(&a));
    exec(&a, "DELETE FROM watched_stocks");
    watch(&a, "again", Some("150"));
    let id = watched_id();
    let on_a = clocks(&a, "watched_stocks", &id);
    assert!(on_a["*"] > on_a["-"]);
    let from_a = send(&a);
    assert_eq!(from_a[0].0.rows[0].deleted, Some(on_a["-"]));

    apply(&b, &from_a);
    let c = device(C);
    apply(&c, &from_a);

    for conn in [&b, &c] {
        assert_eq!(
            value(
                conn,
                "SELECT notes || ' ' || target_price FROM watched_stocks"
            )
            .as_deref(),
            Some("again 150")
        );
        assert_eq!(clocks(conn, "watched_stocks", &id), on_a);
        assert_eq!(dump(conn), dump(&a));
    }
}

#[test]
fn an_edit_of_an_ended_epoch_is_dropped() {
    let a = device(A);
    let b = device(B);
    watch(&a, "first", None);
    apply(&b, &send(&a));
    exec(&a, "DELETE FROM watched_stocks");
    watch(&a, "new epoch", None);
    later(&b, &a);
    exec(&b, "UPDATE watched_stocks SET notes = 'old epoch'");
    let from_a = send(&a);
    let from_b = send(&b);

    let on_a = apply(&a, &from_b);
    apply(&b, &from_a);

    assert_eq!((on_a[0].applied, on_a[0].dropped), (0, 1));
    for conn in [&a, &b] {
        assert_eq!(
            value(conn, "SELECT notes FROM watched_stocks").as_deref(),
            Some("new epoch")
        );
    }
    assert_eq!(dump(&a), dump(&b));
    let id = watched_id();
    assert_eq!(
        clocks(&a, "watched_stocks", &id),
        clocks(&b, "watched_stocks", &id)
    );
}

fn stock(conn: &Connection, company: &str) {
    conn.execute(
        "INSERT INTO stock_investments (id, ticker, company_name, quantity, currency) \
         VALUES (?1, 'AAPL', ?2, '0', 'USD')",
        rusqlite::params![natural_id("stock_investments", &["AAPL"]), company],
    )
    .expect("insert stock");
}

fn stock_tx(conn: &Connection, id: &str, quantity: &str) {
    conn.execute(
        "INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, \
           quantity, price_per_unit, currency, transaction_date) \
         VALUES (?1, ?2, 'buy', 'AAPL', 'Apple', ?3, '100', 'USD', 1767225600)",
        rusqlite::params![id, natural_id("stock_investments", &["AAPL"]), quantity],
    )
    .expect("insert stock transaction");
    investments::recalculate_investment_metrics(conn, &natural_id("stock_investments", &["AAPL"]))
        .expect("recalculate");
}

#[test]
fn delete_and_recreate_with_a_concurrent_child_converges_in_both_orders() {
    let a = device(A);
    let b = device(B);
    let c = device(C);
    let d = device(D);
    stock(&a, "Apple");
    stock_tx(&a, "old-1", "5");
    let base = send(&a);
    for conn in [&b, &c, &d] {
        apply(conn, &base);
    }
    let stock_id = natural_id("stock_investments", &["AAPL"]);
    let old_generation = clocks(&a, "stock_investments", &stock_id)["*"];

    // A deletes AAPL (cascading old-1) and creates it again; B, offline, adds `c` under the old
    // generation and edits the old row.
    exec(&a, "DELETE FROM stock_investments");
    stock(&a, "Apple Inc");
    stock_tx(&a, "new-1", "2");
    later(&b, &a);
    stock_tx(&b, "c", "7");
    exec(
        &b,
        "UPDATE stock_investments SET company_name = 'Apple (B)'",
    );
    let from_a = send(&a);
    let from_b = send(&b);
    let c_row = rows_of(&from_b)
        .into_iter()
        .find(|r| r.key == RowKey::Single("c".into()))
        .expect("c travels");
    assert_eq!(c_row.parents["investment_id"], old_generation);

    apply(&a, &from_b);
    apply(&b, &from_a);
    apply(&c, &from_a);
    apply(&c, &from_b);
    apply(&d, &from_b);
    apply(&d, &from_a);

    let tombstone = clocks(&a, "stock_investments", &stock_id)["-"];
    for conn in [&a, &b, &c, &d] {
        assert_eq!(dump(conn), dump(&a));
        assert_eq!(
            value(conn, "SELECT company_name FROM stock_investments").as_deref(),
            Some("Apple Inc")
        );
        assert_eq!(
            value(
                conn,
                "SELECT group_concat(id) FROM (SELECT id FROM investment_transactions ORDER BY id)"
            )
            .as_deref(),
            Some("new-1")
        );
        assert_eq!(clocks(conn, "investment_transactions", "c")["-"], tombstone);
    }
}
