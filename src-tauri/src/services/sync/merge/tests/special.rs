//! Weak `pending` splits, write-once public keys, clocks, schema, idempotency, heads, `Touched`.

use super::*;
use crate::services::sync::changeset::max_hlc;
use crate::services::sync::hlc::{now_ms, pack};

const SPLIT_DAY: i64 = 1_767_225_600;

fn split_id() -> String {
    natural_id("stock_splits", &["AAPL", &SPLIT_DAY.to_string()])
}

/// A detection: a new split row in `pending`.
fn detect(conn: &Connection) {
    conn.execute(
        "INSERT INTO stock_splits (id, ticker, effective_day, numerator, denominator, source, status, detected_at) \
         VALUES (?1, 'AAPL', ?2, '4', '1', 'yahoo', 'pending', 100)",
        rusqlite::params![split_id(), SPLIT_DAY],
    )
    .expect("insert split");
}

fn status(conn: &Connection) -> Option<String> {
    value(conn, "SELECT status FROM stock_splits")
}

#[test]
fn a_detection_insert_never_undoes_an_applied_split() {
    let a = device(A);
    let b = device(B);
    detect(&a);
    exec(&a, "UPDATE stock_splits SET status = 'applied'");
    let applied = clocks(&a, "stock_splits", &split_id())["status"];
    later(&b, &a);
    detect(&b);
    let from_a = send(&a);
    let from_b = send(&b);
    let generation_b = from_b[0].0.rows[0].generation;
    assert!(generation_b > applied);
    let c = device(C);
    let d = device(D);

    apply(&a, &from_b);
    apply(&b, &from_a);
    apply(&c, &from_a);
    apply(&c, &from_b);
    apply(&d, &from_b);
    apply(&d, &from_a);

    for conn in [&a, &b, &c, &d] {
        assert_eq!(status(conn).as_deref(), Some("applied"));
        let clocks = clocks(conn, "stock_splits", &split_id());
        assert_eq!(clocks["*"], generation_b);
        assert_eq!(clocks["status"], applied);
        assert_eq!(dump(conn), dump(&a));
    }
}

#[test]
fn a_weak_pending_never_gets_a_column_clock() {
    let a = device(A);
    let b = device(B);
    detect(&a);
    later(&b, &a);
    detect(&b);
    let from_a = send(&a);
    let from_b = send(&b);
    let generation_a = from_a[0].0.rows[0].generation;
    let generation_b = from_b[0].0.rows[0].generation;
    let c = device(C);

    apply(&a, &from_b);
    apply(&b, &from_a);
    apply(&c, &from_a);

    for (conn, generation) in [(&a, generation_b), (&b, generation_b), (&c, generation_a)] {
        assert_eq!(status(conn).as_deref(), Some("pending"));
        let expected: BTreeMap<String, Clock> = [("*".to_string(), generation)].into();
        assert_eq!(clocks(conn, "stock_splits", &split_id()), expected);
    }
}

#[test]
fn a_revert_reaches_the_other_devices() {
    let a = device(A);
    let b = device(B);
    detect(&a);
    exec(&a, "UPDATE stock_splits SET status = 'applied'");
    apply(&b, &send(&a));
    assert_eq!(status(&b).as_deref(), Some("applied"));
    exec(&a, "UPDATE stock_splits SET status = 'pending'");
    let reverted = clocks(&a, "stock_splits", &split_id())["status"];

    apply(&b, &send(&a));

    assert_eq!(status(&b).as_deref(), Some("pending"));
    assert_eq!(clocks(&b, "stock_splits", &split_id())["status"], reverted);
    assert_eq!(dump(&a), dump(&b));
}

fn device_row(conn: &Connection, key: &str, name: &str) {
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at) \
         VALUES ('dev-x', ?1, ?2, 'ios', '1.0', 100)",
        rusqlite::params![key, name],
    )
    .expect("insert device");
}

#[test]
fn the_older_public_key_of_a_device_wins_in_both_orders() {
    let a = device(A);
    let b = device(B);
    device_row(&a, "KA", "Phone A");
    later(&b, &a);
    device_row(&b, "KB", "Phone B");
    let from_a = send(&a);
    let from_b = send(&b);
    let c = device(C);
    let d = device(D);

    let on_a = apply(&a, &from_b);
    let on_b = apply(&b, &from_a);
    apply(&c, &from_a);
    let on_c = apply(&c, &from_b);
    apply(&d, &from_b);
    let on_d = apply(&d, &from_a);

    for conn in [&a, &b, &c, &d] {
        assert_eq!(
            value(conn, "SELECT public_key || ' ' || name FROM sync_devices").as_deref(),
            Some("KA Phone B")
        );
        assert_eq!(dump(conn), dump(&a));
        assert_eq!(
            clocks(conn, "sync_devices", "dev-x"),
            clocks(&a, "sync_devices", "dev-x")
        );
    }
    for report in [&on_a[0], &on_b[0], &on_c[0], &on_d[0]] {
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(
            report.warnings[0].contains("dev-x"),
            "{:?}",
            report.warnings
        );
    }
}

#[test]
fn a_clock_more_than_24_hours_ahead_is_applied_with_a_warning() {
    let b = device(B);
    let ahead = clock(pack(now_ms() + 25 * 3_600_000, 0), A);

    let reports = apply(&b, &vec![changeset(A, 1, vec![plan_row("p1", ahead)])]);

    assert_eq!(
        count(&b, "SELECT COUNT(*) FROM spending_plans WHERE id = 'p1'"),
        1
    );
    assert_eq!(reports[0].warnings.len(), 1, "{:?}", reports[0].warnings);
    assert!(
        reports[0].warnings[0].contains(&A.to_b64()),
        "{:?}",
        reports[0].warnings
    );
}

#[test]
fn a_newer_schema_stops_before_touching_anything() {
    let b = device(B);
    let before = (dump(&b), clock_dump(&b));
    let ahead = clock(pack(now_ms() + 3_600_000, 0), A);
    let (mut cs, _) = changeset(A, 1, vec![plan_row("p1", ahead)]);
    cs.schema = schema_number() + 1;
    let hash = hash_of(&cs);

    let outcome = apply_changeset(&b, &cs, hash, NOW).expect("apply");

    assert!(
        matches!(outcome, Outcome::Stopped(StopReason::NewerSchema(n)) if n == schema_number() + 1),
        "{outcome:?}"
    );
    assert_eq!((dump(&b), clock_dump(&b)), before);
    assert!(heads(&b).is_empty());
    assert!(capture::tick(&b).expect("tick") < ahead.hlc);
}

#[test]
fn applying_a_changeset_twice_changes_nothing() {
    let a = device(A);
    let b = device(B);
    account(&a, "acc");
    plan(&a, "p1", 2026);
    plan(&a, "p2", 2026);
    payment(&a, "t1", "acc", 1_767_225_600);
    exec(
        &a,
        "UPDATE spending_plans SET note = 'edited' WHERE id = 'p1'; \
         UPDATE bank_transactions SET spending_plan_id = 'p1' WHERE id = 't1'; \
         DELETE FROM spending_plans WHERE id = 'p2';",
    );
    detect(&a);
    let sent = send(&a);
    apply(&b, &sent);
    let first = (dump(&b), clock_dump(&b), heads(&b));

    let again = apply(&b, &sent);

    assert_eq!((dump(&b), clock_dump(&b), heads(&b)), first);
    assert!(again
        .iter()
        .all(|r| r.domains.is_empty() && r.warnings.is_empty()));
    assert_eq!(dump(&a), dump(&b));
}

#[test]
fn merge_moves_the_local_clock_past_every_clock_it_saw() {
    let a = device(A);
    let b = device(B);
    capture::observe(&a, pack(now_ms() + 3_600_000, 0)).expect("observe");
    plan(&a, "p1", 2026);
    let sent = send(&a);
    let seen = max_hlc(&sent[0].0);

    apply(&b, &sent);

    assert!(capture::tick(&b).expect("tick") > seen);
}

#[test]
fn the_head_of_the_author_is_recorded_in_the_same_transaction() {
    let a = device(A);
    let b = device(B);
    plan(&a, "p1", 2026);
    exec(&a, "UPDATE spending_plans SET note = 'two' WHERE id = 'p1'");
    let sent = send(&a);

    apply(&b, &sent);

    let (cs, hash) = &sent[0];
    assert_eq!(
        heads(&b),
        vec![(A.0.to_vec(), cs.seq as i64, hash.to_vec())]
    );

    // A changeset that stops leaves no head behind.
    let g = clock(1_000 << 16, C);
    let mut orphan = row(
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
    orphan.parents.insert("bank_account_id".into(), g);
    let (cs, h) = changeset(C, 1, vec![orphan]);
    assert!(matches!(
        apply_changeset(&b, &cs, h, NOW).expect("apply"),
        Outcome::Stopped(StopReason::MissingParent { .. })
    ));
    assert_eq!(heads(&b).len(), 1);
}

#[test]
fn the_merge_fills_touched_as_documented() {
    let a = device(A);
    let b = device(B);
    account(&a, "acc-1");
    payment(&a, "pay-2", "acc-1", 300);
    plan(&a, "sp", 2026);
    apply(&b, &send(&a));
    payment(&a, "pay-1", "acc-1", 100);
    exec(
        &a,
        "UPDATE bank_transactions SET booking_date = 200 WHERE id = 'pay-2'; \
         DELETE FROM spending_plans WHERE id = 'sp';",
    );

    let ctx = merge_ctx(&b, &rows_of(&send(&a)));

    let t = &ctx.touched;
    let rows: BTreeMap<String, Vec<String>> = t
        .rows
        .iter()
        .map(|(table, pks)| (table.clone(), pks.iter().cloned().collect()))
        .collect();
    assert_eq!(
        rows,
        BTreeMap::from([
            (
                "bank_transactions".to_string(),
                vec!["pay-1".to_string(), "pay-2".to_string()]
            ),
            ("spending_plans".to_string(), vec!["sp".to_string()]),
        ])
    );
    assert!(t.parents["bank_accounts"].contains("acc-1"));
    assert_eq!(t.days.get("bank-accounts"), Some(&Some(100)));
    assert!(t.deleted.contains("spending_plans"));
    assert!(t
        .rename_scopes
        .contains(&("spending_plans".to_string(), "2026".to_string())));
}

#[test]
fn rename_scopes_name_deleted_rows_and_both_years() {
    let a = device(A);
    let b = device(B);
    plan(&a, "p1", 2026);
    plan(&a, "p2", 2025);
    apply(&b, &send(&a));
    exec(
        &a,
        "UPDATE spending_plans SET year = 2027 WHERE id = 'p1'; \
         DELETE FROM spending_plans WHERE id = 'p2';",
    );

    let ctx = merge_ctx(&b, &rows_of(&send(&a)));

    let scopes: Vec<(String, String)> = ctx.touched.rename_scopes.into_iter().collect();
    assert_eq!(
        scopes,
        [
            ("spending_plans".to_string(), "2025".to_string()),
            ("spending_plans".to_string(), "2026".to_string()),
            ("spending_plans".to_string(), "2027".to_string()),
        ]
    );
    assert!(ctx.touched.deleted.contains("spending_plans"));
}
