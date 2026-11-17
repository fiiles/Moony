//! Weak `pending` splits, write-once public keys, clocks, schema, idempotency, heads, `Touched`.

use super::*;
use crate::services::sync::changeset::max_hlc;
use crate::services::sync::hlc::{now_ms, pack};
use crate::services::sync::identity::cert::{self, CertFields, CertKind, Certificate};
use crate::services::sync::identity::{Record, RecordType, SignSeed};

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
        assert_converged(&[conn, &a]);
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
    assert_converged(&[&a, &b]);
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
        assert_converged(&[conn, &a]);
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
    assert_converged(&[&a, &b]);
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

#[test]
fn merge_rows_refuses_to_run_outside_a_transaction() {
    let a = device(A);
    let b = device(B);
    plan(&a, "p1", 2026);
    let rows = rows_of(&send(&a));

    let err = merge_rows(&b, &rows, NOW).expect_err("autocommit is refused");

    assert!(err.to_string().contains("transaction"), "{err}");
    assert_eq!(count(&b, "SELECT COUNT(*) FROM spending_plans"), 0);
}

/// The tombstone tolerance of `assert_converged` is narrow: a `'-'` that differs without a
/// cascade parent holding the higher value still fails.
#[test]
#[should_panic(expected = "no cascade parent's tombstone")]
fn tombstones_that_differ_without_a_cascade_parent_do_not_converge() {
    let a = device(A);
    let b = device(B);
    plan(&a, "p1", 2026);
    apply(&b, &send(&a));
    exec(&a, "DELETE FROM spending_plans WHERE id = 'p1'");
    apply(&b, &send(&a));
    exec(
        &b,
        "UPDATE sync_clock SET hlc = hlc + 1 WHERE tbl = 'spending_plans' AND pk = 'p1' AND col = '-'",
    );

    assert_converged(&[&a, &b]);
}

/// `id`'s `sync_devices` row on `conn`, written there (so under `conn`'s clock).
fn own_device_row(conn: &Connection, id: DeviceId, key: Option<&str>) {
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at) \
         VALUES (?1, ?2, 'Phone', 'ios', '1.0', 100)",
        rusqlite::params![id.to_hex(), key],
    )
    .expect("insert device");
}

/// A full `sync_devices` row for `id` with `key`, every column at `at`.
fn device_change(id: DeviceId, key: &str, at: Clock) -> RowChange {
    row(
        "sync_devices",
        &id.to_hex(),
        at,
        &[
            ("public_key", text(key)),
            ("name", text("Phone")),
            ("platform", text("ios")),
            ("app_version", text("1.0")),
            ("joined_at", SqlValue::Integer(100)),
        ],
    )
}

fn key_of(conn: &Connection, id: DeviceId) -> Option<String> {
    conn.query_row(
        "SELECT public_key FROM sync_devices WHERE id = ?1",
        [id.to_hex()],
        |r| r.get(0),
    )
    .expect("public key")
}

/// The clock of `id`'s public key on `conn`: its column clock, else the row clock.
fn key_clock(conn: &Connection, id: DeviceId) -> Clock {
    let clocks = clocks(conn, "sync_devices", &id.to_hex());
    clocks.get("public_key").copied().unwrap_or(clocks["*"])
}

fn applied(conn: &Connection, cs: &(Changeset, [u8; 32])) -> ApplyReport {
    match apply_changeset(conn, &cs.0, cs.1, NOW).expect("apply") {
        Outcome::Applied(report) => report,
        Outcome::Stopped(reason) => panic!("stopped: {reason:?}"),
    }
}

// Audit M6 (forged older clock on sync_devices), flipped: a key stamped with another device's ID
// no longer replaces a device's public key, whatever its clock; one stamped with the device's own
// ID does (and that stamp can itself be forged, see the next test).
#[test]
fn a_key_stamped_with_the_devices_own_id_beats_any_other() {
    let a = device(A);
    own_device_row(&a, A, Some("KA"));
    let c = device(C);
    let from_a = send(&a);
    apply(&c, &from_a);
    let forged = changeset(B, 1, vec![device_change(A, "EVIL", clock(1, B))]);

    for conn in [&a, &c] {
        let report = applied(conn, &forged);
        assert_eq!(key_of(conn, A).as_deref(), Some("KA"));
        assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
        assert!(
            report.warnings[0].contains("sync_devices"),
            "{:?}",
            report.warnings
        );
        assert!(
            !report.warnings[0].contains("KA") && !report.warnings[0].contains("EVIL"),
            "the warning never names a key: {:?}",
            report.warnings
        );
    }
    // A device that meets the forgery first takes it (it has no key for A yet) and the key stamped
    // with A's ID replaces it when it arrives: every order ends with A's key.
    let d = device(D);
    applied(&d, &forged);
    assert_eq!(key_of(&d, A).as_deref(), Some("EVIL"));
    apply(&d, &from_a);
    assert_eq!(key_of(&d, A).as_deref(), Some("KA"));
    assert_converged(&[&a, &c, &d]);

    // Between two keys stamped with A's ID the older stays (write-once, like the COALESCE in
    // `devices.rs`), in both orders.
    let renewed = changeset(
        A,
        2,
        vec![device_change(A, "KA2", clock(pack(now_ms(), 0), A))],
    );
    let report = applied(&c, &renewed);
    assert_eq!(key_of(&c, A).as_deref(), Some("KA"));
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
    let e = device(DeviceId([5; 16]));
    applied(&e, &renewed);
    assert_eq!(key_of(&e, A).as_deref(), Some("KA2"));
    apply(&e, &from_a);
    assert_eq!(key_of(&e, A).as_deref(), Some("KA"));
    applied(&e, &forged);
    assert_eq!(key_of(&e, A).as_deref(), Some("KA"));
}

/// `id`'s full `sync_devices` row with key `KA` and `column` = `value`, every column at `at`.
fn device_row_with(id: DeviceId, column: &str, value: &str, at: Clock) -> RowChange {
    let mut change = device_change(id, "KA", at);
    set(&mut change, column, text(value), at);
    change
}

fn column_of(conn: &Connection, id: DeviceId, column: &str) -> Option<String> {
    conn.query_row(
        &format!("SELECT {column} FROM sync_devices WHERE id = ?1"),
        [id.to_hex()],
        |r| r.get(0),
    )
    .expect("column")
}

/// Phase 5: a device's certificate and its `left` record follow the public key's rule.
#[test]
fn a_certificate_or_left_record_stamped_with_the_devices_own_id_beats_any_other() {
    for column in ["cert", "left_record"] {
        let a = device(A);
        own_device_row(&a, A, Some("KA"));
        exec(&a, &format!("UPDATE sync_devices SET {column} = 'OWN'"));
        let from_a = send(&a);
        let c = device(C);
        apply(&c, &from_a);
        let forged = changeset(B, 1, vec![device_row_with(A, column, "EVIL", clock(1, B))]);

        let report = applied(&c, &forged);

        assert_eq!(column_of(&c, A, column).as_deref(), Some("OWN"), "{column}");
        assert_eq!(report.warnings.len(), 1, "{column}: {:?}", report.warnings);
        assert!(
            !report.warnings[0].contains("OWN") && !report.warnings[0].contains("EVIL"),
            "the warning never names a value: {:?}",
            report.warnings
        );
        let d = device(D);
        applied(&d, &forged);
        assert_eq!(column_of(&d, A, column).as_deref(), Some("EVIL"));
        apply(&d, &from_a);
        assert_eq!(column_of(&d, A, column).as_deref(), Some("OWN"), "{column}");
        assert_converged(&[&a, &c, &d]);
    }
}

#[test]
fn a_later_certificate_for_a_device_that_has_one_is_ignored() {
    let a = device(A);
    own_device_row(&a, A, Some("KA"));
    exec(&a, "UPDATE sync_devices SET cert = 'CERT-1'");
    let c = device(C);
    apply(&c, &send(&a));
    let renewed = changeset(
        A,
        2,
        vec![device_row_with(
            A,
            "cert",
            "CERT-2",
            clock(pack(now_ms() + 60_000, 0), A),
        )],
    );

    let report = applied(&c, &renewed);

    assert_eq!(column_of(&c, A, "cert").as_deref(), Some("CERT-1"));
    assert_eq!(report.warnings.len(), 1, "{:?}", report.warnings);
}

/// A removal record is signed by another device, so no stamp is preferred: the older stays, in
/// both orders.
#[test]
fn the_older_removal_record_stays_whoever_stamped_it() {
    let older = changeset(
        B,
        1,
        vec![device_row_with(
            A,
            "removal_record",
            "BY-B",
            clock(pack(now_ms() + 60_000, 0), B),
        )],
    );
    let newer_own = changeset(
        A,
        1,
        vec![device_row_with(
            A,
            "removal_record",
            "BY-A",
            clock(pack(now_ms() + 120_000, 0), A),
        )],
    );
    let c = device(C);
    applied(&c, &older);
    applied(&c, &newer_own);
    let d = device(D);
    applied(&d, &newer_own);
    applied(&d, &older);

    for conn in [&c, &d] {
        assert_eq!(
            column_of(conn, A, "removal_record").as_deref(),
            Some("BY-B")
        );
    }
}

/// The residual the rule leaves (audit M6): the clock's device is written by whoever seals the
/// changeset, so a holder of the vault key can stamp a key with another device's own ID and an
/// older clock, and it is taken. Nothing may trust the column for rekeying until changes are
/// signed per device.
#[test]
fn a_forged_own_id_stamp_still_replaces_the_key() {
    let a = device(A);
    own_device_row(&a, A, Some("KA"));
    let c = device(C);
    apply(&c, &send(&a));
    let forged = changeset(B, 1, vec![device_change(A, "EVIL", clock(1, A))]);

    applied(&c, &forged);

    assert_eq!(key_of(&c, A).as_deref(), Some("EVIL"));
}

#[test]
fn a_first_key_for_a_row_is_taken_from_any_clock() {
    let c = device(C);
    own_device_row(&c, A, None);
    // A re-joined device sends the key under its new ID's clock.
    let new = DeviceId([0x90; 16]);
    let first = changeset(
        new,
        1,
        vec![device_change(A, "KA", clock(pack(now_ms(), 0), new))],
    );

    applied(&c, &first);

    assert_eq!(key_of(&c, A).as_deref(), Some("KA"));
}

#[test]
fn a_rejoined_devices_restamped_key_clock_keeps_the_stored_key() {
    let a = device(A);
    own_device_row(&a, A, Some("KA"));
    let c = device(C);
    apply(&c, &send(&a));
    let stored = key_clock(&c, A);
    let h = stored.hlc;
    // After a re-join A's column clocks keep their hlc under an ID above A's (ruling T8-1).
    let new = DeviceId([0x90; 16]);
    let restamped = changeset(new, 1, vec![device_change(A, "KA", clock(h, new))]);

    let report = applied(&c, &restamped);

    assert_eq!(key_of(&c, A).as_deref(), Some("KA"));
    assert_eq!(key_clock(&c, A), stored);
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

// Fix round (phase 5): a write-once `cert`, `left_record` or `removal_record` that does not
// verify on its own never locks out one that does, whatever their clocks or stamps.

const VAULT: [u8; 16] = [0x11; 16];

fn bind_vault(conn: &Connection) {
    conn.execute(
        "UPDATE sync_state SET vault_id = ?1",
        [crate::services::sync::keys::VaultId(VAULT).to_b64()],
    )
    .expect("vault");
}

fn seed(byte: u8) -> SignSeed {
    SignSeed::from_bytes(zeroize::Zeroizing::new([byte; 32]))
}

/// `device`'s genesis certificate in `vault`, signed with `seed(byte)`.
fn cert_of(device: DeviceId, vault: [u8; 16], byte: u8) -> Certificate {
    cert::issue_self(
        CertKind::Genesis,
        &CertFields {
            vault_id: vault,
            device,
            box_pub: [byte ^ 0xb0; 32],
            issued_at: NOW,
        },
        &seed(byte),
    )
    .expect("cert")
}

/// A's row with `column` = `value` (and, for a `left` record, A's real certificate) at `at`.
fn poisonable_row(column: &str, value: &str, at: Clock, with_cert: bool) -> RowChange {
    // Every column of the row is carried, as the journal sends it.
    let mut change = device_change(A, "KA", at);
    for other in [
        "cert",
        "left_record",
        "removal_record",
        "confirm_record",
        "kit_storage",
    ] {
        set(&mut change, other, SqlValue::Null, at);
    }
    set(&mut change, "removed", SqlValue::Integer(0), at);
    set(&mut change, column, text(value), at);
    if with_cert {
        set(
            &mut change,
            "cert",
            text(&cert_of(A, VAULT, 0x0a).to_b64()),
            at,
        );
    }
    change
}

#[test]
fn an_invalid_write_once_value_never_beats_a_valid_one_in_either_order() {
    let left = Record::sign(RecordType::Left, VAULT, A, A, NOW, 0, &seed(0x0a)).expect("left");
    let removal =
        Record::sign(RecordType::Removed, VAULT, A, B, NOW, 0, &seed(0x0b)).expect("removal");
    let other_vault = cert_of(A, [0x22; 16], 0x0a).to_b64();
    // A `left` record signed with a key other than the one in the row's certificate.
    let forged_left = Record::sign(RecordType::Left, VAULT, A, A, NOW, 0, &seed(0x0c))
        .expect("left")
        .to_b64();
    let confirmation =
        Record::sign(RecordType::Confirmed, VAULT, A, B, NOW, 0, &seed(0x0b)).expect("confirmed");
    let cases: Vec<(&str, String, String)> = vec![
        ("confirm_record", confirmation.to_b64(), "EVIL".into()),
        // A removal record in the confirmation column has the wrong shape for it.
        ("confirm_record", confirmation.to_b64(), removal.to_b64()),
        ("cert", cert_of(A, VAULT, 0x0a).to_b64(), "EVIL".into()),
        ("cert", cert_of(A, VAULT, 0x0a).to_b64(), other_vault),
        ("left_record", left.to_b64(), "EVIL".into()),
        ("left_record", left.to_b64(), forged_left),
        ("removal_record", removal.to_b64(), "EVIL".into()),
        (
            "removal_record",
            removal.to_b64(),
            Record::sign(RecordType::Removed, [0x22; 16], A, B, NOW, 0, &seed(0x0b))
                .expect("removal")
                .to_b64(),
        ),
    ];
    for (column, real, poison) in cases {
        let with_cert = column == "left_record";
        // The poison is older and stamped with A's own ID: under the plain write-once rule it
        // would win for good.
        let poisoned = changeset(
            B,
            1,
            vec![poisonable_row(column, &poison, clock(1, A), with_cert)],
        );
        let genuine = changeset(
            A,
            1,
            vec![poisonable_row(
                column,
                &real,
                clock(pack(now_ms(), 0), A),
                with_cert,
            )],
        );
        let c = device(C);
        bind_vault(&c);
        applied(&c, &poisoned);
        applied(&c, &genuine);
        let d = device(D);
        bind_vault(&d);
        applied(&d, &genuine);
        applied(&d, &poisoned);

        for conn in [&c, &d] {
            assert_eq!(
                column_of(conn, A, column).as_deref(),
                Some(real.as_str()),
                "{column} vs {poison}"
            );
        }
        assert_converged(&[&c, &d]);
    }
}

#[test]
fn between_two_valid_values_the_write_once_rule_stays() {
    // Two certificates that both verify: the older one stamped with A's own ID stays, as before.
    let first = cert_of(A, VAULT, 0x0a).to_b64();
    let second = cert_of(A, VAULT, 0x0d).to_b64();
    let older = changeset(
        A,
        1,
        vec![poisonable_row("cert", &first, clock(1, A), false)],
    );
    let newer = changeset(
        A,
        2,
        vec![poisonable_row(
            "cert",
            &second,
            clock(pack(now_ms(), 0), A),
            false,
        )],
    );
    let c = device(C);
    bind_vault(&c);
    applied(&c, &newer);
    applied(&c, &older);
    assert_eq!(column_of(&c, A, "cert").as_deref(), Some(first.as_str()));
}

/// Review of Task 17, I1: where the Kit is kept only ever grows when it arrives through sync
/// (`paper` < `copied` < `file` < `cloud`; NULL or an unknown value never lowers it), whatever
/// the clocks, so a device about to be removed cannot switch the Kit's replacement off by
/// writing a weaker value with a newer clock.
#[test]
fn the_kit_storage_only_grows_through_sync_in_either_order() {
    let kit = DeviceId([0x6b; 16]);
    let later = |n: u64, by: DeviceId| clock(pack(now_ms(), 0) + n, by);
    let honest = changeset(
        A,
        1,
        vec![device_row_with(kit, "kit_storage", "copied", clock(1, A))],
    );
    for weaker in [Some("paper"), None, Some("bogus")] {
        let mut change = device_change(kit, "KA", later(5, B));
        set(
            &mut change,
            "kit_storage",
            weaker.map_or(SqlValue::Null, text),
            later(5, B),
        );
        let attack = changeset(B, 1, vec![change]);
        // Honest first, then the weaker value with a newer clock.
        let c = device(C);
        applied(&c, &honest);
        applied(&c, &attack);
        assert_eq!(
            column_of(&c, kit, "kit_storage").as_deref(),
            Some("copied"),
            "{weaker:?} after"
        );
        // The weaker value first, then the honest one with an older clock.
        let d = device(D);
        applied(&d, &attack);
        applied(&d, &honest);
        assert_eq!(
            column_of(&d, kit, "kit_storage").as_deref(),
            Some("copied"),
            "{weaker:?} before"
        );
    }
    // A stronger value wins whatever its clock.
    let stronger = changeset(
        B,
        2,
        vec![device_row_with(kit, "kit_storage", "cloud", clock(1, B))],
    );
    let e = device(DeviceId([5; 16]));
    applied(&e, &honest);
    applied(&e, &stronger);
    assert_eq!(column_of(&e, kit, "kit_storage").as_deref(), Some("cloud"));
}
