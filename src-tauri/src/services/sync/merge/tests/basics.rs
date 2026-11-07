//! Inserts, per-column clocks, ties, groups and generations within one delete epoch.

use super::*;

#[test]
fn an_insert_is_created_with_its_clocks_and_triggers_queue_nothing() {
    let a = device(A);
    let b = device(B);
    plan(&a, "p1", 2026);
    exec(
        &a,
        "UPDATE spending_plans SET note = 'second' WHERE id = 'p1'",
    );

    let reports = apply(&b, &send(&a));

    assert_eq!(reports.len(), 1);
    assert_eq!((reports[0].applied, reports[0].dropped), (1, 0));
    assert_converged(&[&b, &a]);
    let on_a = clocks(&a, "spending_plans", "p1");
    let on_b = clocks(&b, "spending_plans", "p1");
    assert_eq!(on_b, on_a);
    assert_eq!(
        on_b.keys().map(String::as_str).collect::<Vec<_>>(),
        ["*", "note"]
    );
    assert!(on_b["note"] > on_b["*"]);
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_outbox"), 0);
}

#[test]
fn a_newer_column_clock_wins_and_an_older_one_is_dropped() {
    let a = device(A);
    let b = device(B);
    plan(&a, "p1", 2026);
    apply(&b, &send(&a));
    exec(
        &a,
        "UPDATE spending_plans SET note = 'from A' WHERE id = 'p1'",
    );
    exec(&a, "UPDATE spending_plans SET icon = 'car' WHERE id = 'p1'");
    later(&b, &a);
    exec(
        &b,
        "UPDATE spending_plans SET note = 'from B' WHERE id = 'p1'",
    );
    let from_a = send(&a);
    let from_b = send(&b);

    apply(&a, &from_b);
    apply(&b, &from_a);

    for conn in [&a, &b] {
        assert_eq!(
            value(conn, "SELECT note FROM spending_plans WHERE id = 'p1'").as_deref(),
            Some("from B")
        );
        assert_eq!(
            value(conn, "SELECT icon FROM spending_plans WHERE id = 'p1'").as_deref(),
            Some("car")
        );
        let c = clocks(conn, "spending_plans", "p1");
        assert_eq!(c["note"].device, B);
        assert_eq!(c["icon"].device, A);
    }
    assert_converged(&[&a, &b]);
    assert_eq!(
        clocks(&a, "spending_plans", "p1"),
        clocks(&b, "spending_plans", "p1")
    );
}

#[test]
fn equal_hlcs_tie_break_by_device() {
    let g = clock(1_000 << 16, A);
    let mut from_a = plan_row("p1", g);
    set(&mut from_a, "note", text("from A"), clock(2_000 << 16, A));
    let mut from_b = plan_row("p1", g);
    set(&mut from_b, "note", text("from B"), clock(2_000 << 16, B));
    let base = changeset(A, 1, vec![plan_row("p1", g)]);
    let a_edit = changeset(A, 2, vec![from_a]);
    let b_edit = changeset(B, 1, vec![from_b]);

    let c = device(C);
    apply(&c, &vec![base.clone(), a_edit.clone(), b_edit.clone()]);
    let d = device(D);
    apply(&d, &vec![base, b_edit, a_edit]);

    for conn in [&c, &d] {
        assert_eq!(
            value(conn, "SELECT note FROM spending_plans WHERE id = 'p1'").as_deref(),
            Some("from B")
        );
        assert_eq!(
            clocks(conn, "spending_plans", "p1")["note"],
            clock(2_000 << 16, B)
        );
    }
    assert_converged(&[&c, &d]);
}

#[test]
fn a_group_is_taken_whole_from_the_newer_side() {
    // P stamps the group at t2; Q carries amount at t1 < t2 but currency at t3 > t2, so Q's
    // group is newer as a whole. Per column, amount would come from P and currency from Q.
    let g = clock(1_000 << 16, A);
    let mut p = plan_row("p1", g);
    set(&mut p, "amount", text("2000.00"), clock(2_000 << 16, A));
    set(&mut p, "currency", text("CZK"), clock(2_000 << 16, A));
    let mut q = plan_row("p1", g);
    set(&mut q, "amount", text("1000.00"), clock(1_500 << 16, B));
    set(&mut q, "currency", text("EUR"), clock(3_000 << 16, B));
    let base = changeset(A, 1, vec![plan_row("p1", g)]);
    let p = changeset(A, 2, vec![p]);
    let q = changeset(B, 1, vec![q]);

    let c = device(C);
    apply(&c, &vec![base.clone(), p.clone(), q.clone()]);
    let d = device(D);
    apply(&d, &vec![base, q, p]);

    for conn in [&c, &d] {
        assert_eq!(
            value(
                conn,
                "SELECT amount || ' ' || currency FROM spending_plans WHERE id = 'p1'"
            )
            .as_deref(),
            Some("1000.00 EUR")
        );
        let clocks = clocks(conn, "spending_plans", "p1");
        assert_eq!(clocks["amount"], clock(1_500 << 16, B));
        assert_eq!(clocks["currency"], clock(3_000 << 16, B));
    }
    assert_converged(&[&c, &d]);
}

#[test]
fn concurrent_creation_of_same_natural_id_merges_per_column() {
    let a = device(A);
    let b = device(B);
    watch(&a, "from A", Some("100"));
    later(&b, &a);
    watch(&b, "from B", Some("200"));
    let from_a = send(&a);
    let from_b = send(&b);
    let c = device(C);
    let d = device(D);

    apply(&a, &from_b);
    apply(&b, &from_a);
    apply(&c, &from_a);
    apply(&c, &from_b);
    apply(&d, &from_b);
    apply(&d, &from_a);

    let id = watched_id();
    let generation_b = from_b[0].0.rows[0].generation;
    for conn in [&a, &b, &c, &d] {
        assert_converged(&[conn, &a]);
        assert_eq!(
            value(
                conn,
                "SELECT notes || ' ' || target_price FROM watched_stocks"
            )
            .as_deref(),
            Some("from B 200")
        );
        assert_eq!(clocks(conn, "watched_stocks", &id)["*"], generation_b);
    }
}

#[test]
fn the_stored_generation_is_the_larger_and_unchanged_columns_keep_their_clocks() {
    let a = device(A);
    let b = device(B);
    watch(&a, "from A", Some("100"));
    later(&b, &a);
    watch(&b, "from B", Some("200"));
    later(&a, &b);
    exec(&a, "UPDATE watched_stocks SET notes = 'A later'");
    let id = watched_id();
    let notes = clocks(&a, "watched_stocks", &id)["notes"];
    let from_a = send(&a);
    let from_b = send(&b);
    let generation_a = from_a[0].0.rows[0].generation;
    let generation_b = from_b[0].0.rows[0].generation;
    assert!(generation_a < generation_b && generation_b < notes);

    apply(&a, &from_b);
    apply(&b, &from_a);

    let expected: BTreeMap<String, Clock> = [
        ("*".to_string(), generation_b),
        ("notes".to_string(), notes),
    ]
    .into();
    for conn in [&a, &b] {
        assert_eq!(clocks(conn, "watched_stocks", &id), expected);
        assert_eq!(
            value(
                conn,
                "SELECT notes || ' ' || target_price FROM watched_stocks"
            )
            .as_deref(),
            Some("A later 200")
        );
    }
    assert_converged(&[&a, &b]);
}

#[test]
fn an_unknown_column_from_a_newer_schema_is_ignored() {
    let b = device(B);
    let g = clock(1_000 << 16, A);
    let mut insert = plan_row("p1", g);
    set(&mut insert, "mood", text("sunny"), g);
    let mut update = plan_row("p1", g);
    set(&mut update, "mood", text("rainy"), clock(2_000 << 16, A));
    set(&mut update, "note", text("second"), clock(2_000 << 16, A));

    apply(
        &b,
        &vec![changeset(A, 1, vec![insert]), changeset(A, 2, vec![update])],
    );

    assert_eq!(
        value(&b, "SELECT note FROM spending_plans WHERE id = 'p1'").as_deref(),
        Some("second")
    );
    let clocks = clocks(&b, "spending_plans", "p1");
    assert_eq!(
        clocks.keys().map(String::as_str).collect::<Vec<_>>(),
        ["*", "note"]
    );
}
