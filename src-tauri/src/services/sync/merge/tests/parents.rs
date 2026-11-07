//! Children whose parent is gone (D14): CASCADE, SET NULL, the NO ACTION orphan rules, and the
//! stop when a parent is missing without a tombstone.

use super::*;

fn fk_violations(conn: &Connection) -> i64 {
    count(conn, "SELECT COUNT(*) FROM pragma_foreign_key_check")
}

#[test]
fn cascade_child_added_offline_follows_the_parent_tombstone() {
    let a = device(A);
    let b = device(B);
    account(&a, "acc");
    apply(&b, &send(&a));
    exec(&a, "DELETE FROM bank_accounts WHERE id = 'acc'");
    let tombstone = clocks(&a, "bank_accounts", "acc")["-"];
    later(&b, &a);
    payment(&b, "t1", "acc", 1_767_225_600);
    let from_a = send(&a);
    let from_b = send(&b);

    let on_a = apply(&a, &from_b);
    apply(&b, &from_a);

    assert_eq!((on_a[0].applied, on_a[0].dropped), (0, 1));
    for conn in [&a, &b] {
        assert_eq!(count(conn, "SELECT COUNT(*) FROM bank_accounts"), 0);
        assert_eq!(count(conn, "SELECT COUNT(*) FROM bank_transactions"), 0);
        assert_eq!(clocks(conn, "bank_transactions", "t1")["-"], tombstone);
    }
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn set_null_child_added_offline_loses_the_reference() {
    let a = device(A);
    let b = device(B);
    account(&a, "acc");
    plan(&a, "sp", 2026);
    apply(&b, &send(&a));
    exec(&a, "DELETE FROM spending_plans WHERE id = 'sp'");
    later(&b, &a);
    payment(&b, "t1", "acc", 1_767_225_600);
    exec(
        &b,
        "UPDATE bank_transactions SET spending_plan_id = 'sp' WHERE id = 't1'",
    );
    let from_a = send(&a);
    let from_b = send(&b);

    apply(&a, &from_b);
    apply(&b, &from_a);

    for conn in [&a, &b] {
        assert_eq!(count(conn, "SELECT COUNT(*) FROM spending_plans"), 0);
        assert_eq!(
            count(
                conn,
                "SELECT COUNT(*) FROM bank_transactions WHERE id = 't1' AND spending_plan_id IS NULL"
            ),
            1
        );
    }
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn no_action_orphan_nullable_category_is_cleared() {
    let a = device(A);
    let b = device(B);
    account(&a, "acc");
    category(&a, "k");
    payment(&a, "t1", "acc", 1_767_225_600);
    apply(&b, &send(&a));
    exec(&a, "DELETE FROM transaction_categories WHERE id = 'k'");
    later(&b, &a);
    exec(
        &b,
        "UPDATE bank_transactions SET category_id = 'k' WHERE id = 't1'",
    );
    let from_a = send(&a);
    let from_b = send(&b);

    apply(&a, &from_b);
    apply(&b, &from_a);

    for conn in [&a, &b] {
        assert_eq!(
            count(
                conn,
                "SELECT COUNT(*) FROM transaction_categories WHERE id = 'k'"
            ),
            0
        );
        assert_eq!(
            value(
                conn,
                "SELECT category_id FROM bank_transactions WHERE id = 't1'"
            ),
            None
        );
        assert_eq!(fk_violations(conn), 0);
    }
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn no_action_orphan_required_rule_is_dropped() {
    let a = device(A);
    let b = device(B);
    category(&a, "k");
    apply(&b, &send(&a));
    exec(&a, "DELETE FROM transaction_categories WHERE id = 'k'");
    let tombstone = clocks(&a, "transaction_categories", "k")["-"];
    later(&b, &a);
    exec(
        &b,
        "INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id) \
         VALUES ('r1', 'Coffee', 'contains', 'coffee', 'k')",
    );
    let from_a = send(&a);
    let from_b = send(&b);

    let on_a = apply(&a, &from_b);
    apply(&b, &from_a);

    assert_eq!((on_a[0].applied, on_a[0].dropped), (0, 1));
    for conn in [&a, &b] {
        assert_eq!(
            count(
                conn,
                "SELECT COUNT(*) FROM categorization_rules WHERE id = 'r1'"
            ),
            0
        );
        assert_eq!(clocks(conn, "categorization_rules", "r1")["-"], tombstone);
        assert_eq!(fk_violations(conn), 0);
    }
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn a_tombstone_with_local_no_action_children_still_commits() {
    let a = device(A);
    let b = device(B);
    account(&a, "acc");
    category(&a, "k");
    apply(&b, &send(&a));
    exec(&a, "DELETE FROM transaction_categories WHERE id = 'k'");
    let tombstone = clocks(&a, "transaction_categories", "k")["-"];
    // B's own children of `k`, not sent yet: a payment, a rule and a subcategory.
    payment(&b, "t1", "acc", 1_767_225_600);
    exec(
        &b,
        "UPDATE bank_transactions SET category_id = 'k', suggested_category_id = 'k' WHERE id = 't1'; \
         INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id) \
           VALUES ('r1', 'Coffee', 'contains', 'coffee', 'k'); \
         INSERT INTO transaction_categories (id, name, parent_id) VALUES ('k2', 'Sub', 'k');",
    );

    let reports = apply(&b, &send(&a));

    assert_eq!(reports[0].applied, 1);
    assert_eq!(fk_violations(&b), 0);
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM transaction_categories WHERE id = 'k'"
        ),
        0
    );
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM bank_transactions \
             WHERE id = 't1' AND category_id IS NULL AND suggested_category_id IS NULL"
        ),
        1
    );
    assert_eq!(
        value(
            &b,
            "SELECT parent_id FROM transaction_categories WHERE id = 'k2'"
        ),
        None
    );
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM categorization_rules WHERE id = 'r1'"
        ),
        0
    );
    assert_eq!(clocks(&b, "categorization_rules", "r1")["-"], tombstone);
}

#[test]
fn missing_parent_without_tombstone_stops_and_rolls_back() {
    let b = device(B);
    let before = (dump(&b), clock_dump(&b));
    let g = clock(1_000 << 16, A);
    let mut payment = row(
        "bank_transactions",
        "t1",
        g,
        &[
            ("bank_account_id", text("acc-x")),
            ("tx_type", text("debit")),
            ("amount", text("-10.00")),
            ("currency", text("CZK")),
            ("booking_date", SqlValue::Integer(1_767_225_600)),
        ],
    );
    payment.parents.insert("bank_account_id".into(), g);
    let (cs, h) = changeset(A, 1, vec![plan_row("p1", g), payment]);

    let outcome = apply_changeset(&b, &cs, h, NOW).expect("apply");

    assert!(
        matches!(
            &outcome,
            Outcome::Stopped(StopReason::MissingParent { table, key, parent })
                if table == "bank_transactions" && key == "t1" && parent.contains("acc-x")
        ),
        "{outcome:?}"
    );
    assert_eq!((dump(&b), clock_dump(&b)), before);
    assert!(heads(&b).is_empty());
}
