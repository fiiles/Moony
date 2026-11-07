//! Unique keys under interleavings (rulings U1–U4, the Task 14 review's C1, C2 and I1): a key
//! collision that only exists halfway through a changeset waits for the rest of it (U2), keyed
//! rows share their natural id (U1), a cleared link or a renamed name stays derived from the typed
//! value (U3), and the typed value is what travels (U4).

use super::unique::{
    balance, cashflow_item, item_links, legacy_balance, listed, shared_parents, tag, typed,
    warnings, DAY,
};
use super::*;
use crate::models::InsertStockTag;
use crate::services::sync::repair::links;
use crate::services::{bank_accounts, cashflow_planning, stock_tags};

const DAY_2: i64 = DAY + 86_400;
const DAY_3: i64 = DAY + 2 * 86_400;

/// Everything `sent` carries as ONE changeset of `device`, its rows reversed: the order a
/// receiver must not depend on.
fn reversed(sent: &Sent, device: DeviceId) -> Sent {
    let mut rows = rows_of(sent);
    rows.reverse();
    vec![changeset(device, 1, rows)]
}

/// Everything `sent` carries as ONE changeset of `device`, in its order.
fn joined(sent: &Sent, device: DeviceId) -> Sent {
    vec![changeset(device, 1, rows_of(sent))]
}

fn balances(conn: &Connection) -> Vec<String> {
    listed(
        conn,
        "SELECT id || ' ' || as_of || ' ' || amount FROM bank_balance_records ORDER BY as_of",
    )
}

/// C1, keep-newest shape: on B, rec-x moves away from DAY and rec-y moves onto DAY (legacy ids,
/// so they are two ids on one key on the receiver for a moment). In either row order every
/// device ends with both rows moved, nothing deleted and no fallback warning.
#[test]
fn c1_a_move_away_and_a_move_onto_the_key_in_one_changeset() {
    let a = device(A);
    let b = device(B);
    let c = device(C);
    shared_parents(&a, &[&b, &c]);
    legacy_balance(&a, "rec-x", DAY, "100.00");
    legacy_balance(&a, "rec-y", DAY_2, "200.00");
    let base = send(&a);
    apply(&b, &base);
    apply(&c, &base);
    exec(
        &b,
        "UPDATE bank_balance_records SET as_of = 1767398400 WHERE id = 'rec-x'; \
         UPDATE bank_balance_records SET as_of = 1767225600 WHERE id = 'rec-y';",
    );
    bank_accounts::refresh_balance_columns(&b, "acc").expect("refresh");
    let from_b = send(&b);

    let on_a = apply(&a, &joined(&from_b, B));
    let on_c = apply(&c, &reversed(&from_b, B));

    for conn in [&a, &b, &c] {
        assert_eq!(
            balances(conn),
            vec![
                format!("rec-y {DAY} 200.00"),
                format!("rec-x {DAY_3} 100.00")
            ]
        );
        assert!(tombstones(conn).is_empty());
    }
    assert!(warnings(&on_a).is_empty() && warnings(&on_c).is_empty());
    assert_converged(&[&a, &b, &c]);
}

/// C1, unlink shape: on B, item-a (holding the link) is deleted and item-b takes its source in
/// one changeset. In either row order item-b ends with the link.
#[test]
fn c1_unlink_a_delete_and_a_relink_in_one_changeset() {
    let a = device(A);
    let b = device(B);
    let c = device(C);
    cashflow_item(&a, "item-a", Some("loan-1"));
    cashflow_item(&a, "item-b", None);
    let base = send(&a);
    apply(&b, &base);
    apply(&c, &base);
    exec(
        &b,
        "DELETE FROM cashflow_items WHERE id = 'item-a'; \
         UPDATE cashflow_items SET replacement_source_id = 'loan-1' WHERE id = 'item-b';",
    );
    let from_b = send(&b);

    apply(&a, &joined(&from_b, B));
    apply(&c, &reversed(&from_b, B));

    for conn in [&a, &b, &c] {
        assert_eq!(item_links(conn), vec!["item-b loan-1"]);
        assert!(typed(conn).is_empty());
    }
    assert_converged(&[&a, &b, &c]);
}

/// C2 (a): A records DAY, B records DAY too (same id, U1), and A deletes its record after B's
/// write but before it sees it. The delete is the newer event: the day is gone everywhere.
#[test]
fn c2_a_the_shared_row_is_deleted_before_the_other_author_sees_it() {
    let a = device(A);
    let b = device(B);
    shared_parents(&a, &[&b]);
    balance(&a, DAY, "100.00");
    later(&b, &a);
    balance(&b, DAY, "200.00");
    later(&a, &b);
    let id = bank_accounts::balance_record_id("acc", DAY);
    bank_accounts::delete_balance_record(&a, &id).expect("delete");
    let deleted = clocks(&a, "bank_balance_records", &id)["-"];
    let from_a = send(&a);
    let from_b = send(&b);

    apply(&a, &from_b);
    apply(&b, &from_a);

    for conn in [&a, &b] {
        assert!(balances(conn).is_empty());
        assert_eq!(clocks(conn, "bank_balance_records", &id)["-"], deleted);
    }
    assert_converged(&[&a, &b]);
}

/// C2 (b): after both devices met on one day's record, A moves it to another day (U1: a delete
/// and an insert under the new day's id, the shown value carried over).
#[test]
fn c2_b_a_merged_row_moved_to_another_key_later() {
    let a = device(A);
    let b = device(B);
    shared_parents(&a, &[&b]);
    balance(&a, DAY, "100.00");
    later(&b, &a);
    balance(&b, DAY, "200.00");
    let (from_a, from_b) = (send(&a), send(&b));
    apply(&a, &from_b);
    apply(&b, &from_a);
    let id = bank_accounts::balance_record_id("acc", DAY);
    bank_accounts::delete_balance_record(&a, &id).expect("delete");
    balance(&a, DAY_2, "200.00");

    apply(&b, &send(&a));

    let moved = bank_accounts::balance_record_id("acc", DAY_2);
    for conn in [&a, &b] {
        assert_eq!(balances(conn), vec![format!("{moved} {DAY_2} 200.00")]);
    }
    assert_converged(&[&a, &b]);
}

/// C2 (c), unlink: A's item-a holds the link; B's item-b, linked offline, shows none once B sees
/// item-a. A deletes item-a before seeing item-b: item-b gets its typed link back on B, and A
/// never cleared it. Then B moves item-b's link to another source.
#[test]
fn c2_c_unlink_the_holder_deleted_and_the_loser_relinked_later() {
    let a = device(A);
    let b = device(B);
    let c = device(C);
    cashflow_item(&a, "item-a", Some("loan-1"));
    let first = send(&a);
    later(&b, &a);
    cashflow_item(&b, "item-b", Some("loan-1"));
    let from_b = send(&b);
    apply(&b, &first);
    assert_eq!(item_links(&b), vec!["item-a loan-1", "item-b -"]);
    cashflow_planning::delete_cashflow_item(&a, "item-a".into()).expect("delete");
    let second = send(&a);

    apply(&a, &from_b);
    apply(&b, &second);
    apply(&c, &from_b);
    apply(&c, &first);
    apply(&c, &second);

    for conn in [&a, &b, &c] {
        assert_eq!(item_links(conn), vec!["item-b loan-1"]);
        assert!(typed(conn).is_empty());
    }
    assert_converged(&[&a, &b, &c]);

    exec(
        &b,
        "UPDATE cashflow_items SET replacement_source_id = 'loan-2' WHERE id = 'item-b'",
    );
    links::after_local_write(&b).expect("relink");
    let third = send(&b);
    apply(&a, &third);
    apply(&c, &third);
    for conn in [&a, &b, &c] {
        assert_eq!(item_links(conn), vec!["item-b loan-2"]);
    }
    assert_converged(&[&a, &b, &c]);
}

/// I1: A deletes a legacy-id record and records the same day again (now under its natural id) in
/// one changeset. The legacy row's tombstone is A's own on every device, in either row order.
#[test]
fn i1_a_legacy_row_deleted_and_its_key_recorded_again_in_one_changeset() {
    let a = device(A);
    let b = device(B);
    let c = device(C);
    shared_parents(&a, &[&b, &c]);
    legacy_balance(&a, "legacy", DAY, "100.00");
    let base = send(&a);
    apply(&b, &base);
    apply(&c, &base);
    bank_accounts::delete_balance_record(&a, "legacy").expect("delete");
    let deleted = clocks(&a, "bank_balance_records", "legacy")["-"];
    balance(&a, DAY, "300.00");
    let from_a = send(&a);

    let on_b = apply(&b, &joined(&from_a, A));
    let on_c = apply(&c, &reversed(&from_a, A));

    let id = bank_accounts::balance_record_id("acc", DAY);
    for conn in [&a, &b, &c] {
        assert_eq!(balances(conn), vec![format!("{id} {DAY} 300.00")]);
        assert_eq!(clocks(conn, "bank_balance_records", "legacy")["-"], deleted);
    }
    assert!(warnings(&on_b).is_empty() && warnings(&on_c).is_empty());
    assert_converged(&[&a, &b, &c]);
}

/// U4, names: C first sees tag-b through a later changeset (a colour edit) that carries the typed
/// name, and shows and remembers it exactly like A and B. Freeing the name gives it back
/// everywhere.
#[test]
fn u4_a_renamed_row_first_seen_later_has_the_same_typed_name_and_display() {
    let a = device(A);
    let b = device(B);
    let c = device(C);
    tag(&a, "tag-b", "Growth");
    later(&b, &a);
    tag(&b, "tag-a", "Growth");
    let (first_a, first_b) = (send(&a), send(&b));
    apply(&a, &first_b);
    apply(&b, &first_a);
    let edit = |name: &str, color: Option<&str>| InsertStockTag {
        name: name.into(),
        color: color.map(str::to_string),
        group_id: None,
    };
    stock_tags::update_tag(&a, "tag-b", edit("Growth (2)", Some("#00ff00"))).expect("recolour");
    let second_a = send(&a);
    assert_eq!(
        rows_of(&second_a)[0].columns["name"].0,
        text("Growth"),
        "the typed name travels"
    );

    apply(&b, &second_a);
    apply(&c, &first_b);
    apply(&c, &second_a);

    for conn in [&a, &b, &c] {
        assert_eq!(
            listed(conn, "SELECT id || ' ' || name FROM stock_tags ORDER BY id"),
            vec!["tag-a Growth", "tag-b Growth (2)"]
        );
        assert_eq!(
            typed(conn),
            vec!["stock_tags tag-b name Growth -> Growth (2)"]
        );
    }
    assert_converged(&[&a, &b, &c]);

    stock_tags::update_tag(&b, "tag-a", edit("Alpha", None)).expect("rename");
    let freed = send(&b);
    apply(&a, &freed);
    apply(&c, &freed);
    for conn in [&a, &b, &c] {
        assert_eq!(
            listed(conn, "SELECT id || ' ' || name FROM stock_tags ORDER BY id"),
            vec!["tag-a Alpha", "tag-b Growth"]
        );
        assert!(typed(conn).is_empty());
    }
    assert_converged(&[&a, &b, &c]);
}

/// U4, links: C first sees item-b through a later changeset (an amount edit that sends the shown
/// NULL back through the service) carrying the typed link, and shows and remembers it like A and
/// B. Deleting the holder gives the link back everywhere.
#[test]
fn u4_an_unlinked_row_first_seen_later_has_the_same_typed_link_and_display() {
    let a = device(A);
    let b = device(B);
    let c = device(C);
    cashflow_item(&a, "item-b", Some("loan-1"));
    later(&b, &a);
    cashflow_item(&b, "item-a", Some("loan-1"));
    let (first_a, first_b) = (send(&a), send(&b));
    apply(&a, &first_b);
    apply(&b, &first_a);
    cashflow_planning::update_cashflow_item(
        &a,
        "item-b".into(),
        crate::models::InsertCashflowItem {
            name: "Item item-b".into(),
            amount: "20.00".into(),
            currency: Some("CZK".into()),
            frequency: "monthly".into(),
            item_type: "expense".into(),
            replacement_source_id: None,
            category: "subscriptions".into(),
        },
    )
    .expect("edit amount");
    let second_a = send(&a);
    assert_eq!(
        rows_of(&second_a)[0].columns["replacement_source_id"].0,
        text("loan-1"),
        "the typed link travels"
    );

    apply(&b, &second_a);
    apply(&c, &first_b);
    apply(&c, &second_a);

    for conn in [&a, &b, &c] {
        assert_eq!(item_links(conn), vec!["item-a loan-1", "item-b -"]);
        assert_eq!(
            typed(conn),
            vec!["cashflow_items item-b replacement_source_id loan-1 -> -"]
        );
    }
    assert_converged(&[&a, &b, &c]);

    cashflow_planning::delete_cashflow_item(&b, "item-a".into()).expect("delete holder");
    let freed = send(&b);
    apply(&a, &freed);
    apply(&c, &freed);
    for conn in [&a, &b, &c] {
        assert_eq!(item_links(conn), vec!["item-b loan-1"]);
        assert!(typed(conn).is_empty());
    }
    assert_converged(&[&a, &b, &c]);
}
