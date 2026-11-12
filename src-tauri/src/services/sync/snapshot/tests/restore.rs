use super::*;
use crate::services::sync::chain::quarantine;
use crate::services::sync::hlc::{now_ms, pack};

#[test]
fn a_dump_restored_by_replace_reproduces_every_synced_table_and_clock() {
    let a = rich_dumper();
    assert_eq!(
        typed_dump(&a),
        vec!["stock_tags tag-c name Growth -> Growth (2)"],
        "the fixture holds a renamed tag"
    );
    let (bytes, stats) = snapshot(&a, 3);

    let b = joined(&bytes);

    assert_eq!(table_dump(&b), table_dump(&a));
    assert_eq!(clock_dump_live(&b), clock_dump_live(&a));
    assert_eq!(typed_dump(&b), typed_dump(&a));
    assert_eq!(
        stats.max_hlc,
        clock_dump(&a)
            .iter()
            .map(|c| c.3 as u64)
            .max()
            .expect("clocks")
    );
    let order: Vec<String> = rows(&records(&bytes))
        .iter()
        .filter(|r| r.table == "transaction_categories" && r.insert)
        .map(|r| r.key.to_pk_text().unwrap())
        .filter(|pk| pk == "z-parent" || pk == "a-child")
        .collect();
    assert_eq!(
        order,
        vec!["z-parent", "a-child"],
        "categories parent-first"
    );
}

#[test]
fn replace_discards_the_joining_profiles_own_rows_and_seeds() {
    let a = device(A);
    let seeded: String = value(
        &a,
        "SELECT id FROM transaction_categories c \
         WHERE NOT EXISTS (SELECT 1 FROM transaction_categories k WHERE k.parent_id = c.id) \
           AND NOT EXISTS (SELECT 1 FROM categorization_rules r WHERE r.category_id = c.id) \
         ORDER BY id LIMIT 1",
    )
    .expect("a seeded leaf category");
    exec(
        &a,
        &format!(
            "DELETE FROM transaction_categories WHERE id = '{seeded}'; \
             INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('AAA', 1.5, 10);"
        ),
    );
    let (bytes, _) = snapshot(&a, 1);

    let b = migrated();
    exec(
        &b,
        "INSERT INTO institutions (id, name) VALUES ('b-own', 'Mine'); \
         INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('ZZZ', 9.5, 10); \
         INSERT INTO app_config (key, value) VALUES ('milestones.mutedKinds', '[\"b\"]');",
    );
    assert_eq!(
        count(
            &b,
            &format!("SELECT COUNT(*) FROM transaction_categories WHERE id = '{seeded}'")
        ),
        1
    );
    join(&b, B, &mut &bytes[..], NOW).expect("join");

    assert_eq!(
        count(
            &b,
            &format!("SELECT COUNT(*) FROM transaction_categories WHERE id = '{seeded}'")
        ),
        0,
        "the seed category deleted in the vault stays deleted"
    );
    assert_eq!(
        count(&b, "SELECT COUNT(*) FROM institutions WHERE id = 'b-own'"),
        0
    );
    assert_eq!(
        strings(
            &b,
            "SELECT currency FROM exchange_rates WHERE currency IN ('AAA', 'ZZZ')"
        ),
        vec!["AAA"]
    );
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM app_config WHERE key = 'milestones.mutedKinds'"
        ),
        0
    );
    assert_eq!(table_dump(&b), table_dump(&a));
    assert_eq!(seed_dump(&b), seed_dump(&a));
}

#[test]
fn joining_queues_nothing_and_installs_capture_after_the_restore() {
    let a = migrated();
    exec(
        &a,
        "INSERT INTO user_profile (id, name, surname, email, language) VALUES (1, 'A', 'A', 'a@a', 'cs'); \
         DELETE FROM app_config WHERE key LIKE 'rule_packs%';",
    );
    state::enable(&a, A).expect("enable");
    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
         UPDATE user_profile SET surname = 'Z';",
    );
    let (bytes, stats) = snapshot(&a, 1);

    let b = migrated();
    exec(
        &b,
        "INSERT INTO user_profile (id, name, surname, email, language) VALUES (1, 'B', 'B', 'b@b', 'en')",
    );
    join(&b, B, &mut &bytes[..], NOW).expect("join");

    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_outbox"), 0);
    assert!(capture::is_installed(&b).expect("installed"));
    assert!(capture::tick(&b).expect("tick") > stats.max_hlc);
    let packs =
        "SELECT key || '=' || value FROM app_config WHERE key LIKE 'rule_packs%' ORDER BY key";
    assert_eq!(strings(&b, packs), strings(&a, packs));
    assert_eq!(
        strings(&b, packs),
        vec![
            "rule_packs_disabled_rules=[]",
            "rule_packs_enabled=[\"global\",\"cz\"]"
        ]
    );
    assert_eq!(
        strings(
            &b,
            "SELECT name || ' ' || surname || ' ' || email || ' ' || language FROM user_profile"
        ),
        vec!["A Z a@a en"],
        "the profile's synced columns come from the snapshot, its Local columns stay"
    );
    let profile_clocks = |conn: &Connection| -> Vec<ClockRow> {
        clock_dump(conn)
            .into_iter()
            .filter(|c| c.0 == "user_profile")
            .collect()
    };
    assert_eq!(profile_clocks(&b), profile_clocks(&a));
    assert_eq!(profile_clocks(&b).len(), 1, "the surname's column clock");
    let state = state::load(&b).expect("load").expect("row");
    assert!(state.enabled);
    assert_eq!(state.device, B);
    exec(
        &b,
        "INSERT INTO institutions (id, name) VALUES ('i2', 'Later')",
    );
    assert_eq!(
        strings(&b, "SELECT tbl || ' ' || pk FROM sync_outbox"),
        vec!["institutions i2"]
    );
}

#[test]
fn a_device_joined_from_a_snapshot_applies_the_dumpers_next_envelope() {
    let a = device(A);
    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    let first = send(&a);
    assert_eq!(first.len(), 1);
    let (bytes, _) = snapshot(&a, 1);
    let b = joined(&bytes);

    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('i2', 'Bank 2')",
    );
    let second = send(&a);
    assert_eq!(second[0].0.seq, 2);
    // The snapshot was taken at relay sequence 1; this envelope is the relay's second op.
    let meta = EnvelopeMeta::of(&second[0].0, second[0].1, 2);
    assert_eq!(judge(&b, &meta).expect("judge"), Verdict::Apply);
    apply(&b, &second);
    assert_eq!(table_dump(&b), table_dump(&a));
}

#[test]
fn restore_by_merge_keeps_newer_local_edits_and_the_outbox() {
    let a = device(A);
    let b = device(B);
    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    apply(&b, &send(&a));
    exec(
        &a,
        "INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('AAA', 1.5, 10), ('BBB', 3.5, 10)",
    );
    exec(
        &b,
        "INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('AAA', 2.5, 20)",
    );
    // B goes offline. A renames the bank and adds one; B renames it later and adds its own.
    exec(
        &a,
        "UPDATE institutions SET name = 'Older name' WHERE id = 'i1'; \
         INSERT INTO institutions (id, name) VALUES ('i2', 'From A');",
    );
    let from_a = send(&a);
    capture::observe(&b, capture::tick(&a).expect("tick")).expect("observe");
    exec(
        &b,
        "UPDATE institutions SET name = 'Newer name' WHERE id = 'i1'; \
         INSERT INTO institutions (id, name) VALUES ('i3', 'From B');",
    );
    let (bytes, stats) = snapshot(&a, 9);

    let restored = restore_merge(&b, &mut &bytes[..], NOW).expect("restore");

    assert_eq!(
        restored,
        RestoreReport {
            stats,
            warnings: Vec::new(),
            attachments: AttachmentCarry::default()
        }
    );
    assert_eq!(
        strings(
            &b,
            "SELECT id || ' ' || name FROM institutions WHERE id IN ('i1', 'i2', 'i3') ORDER BY id"
        ),
        vec!["i1 Newer name", "i2 From A", "i3 From B"]
    );
    assert_eq!(
        strings(&b, "SELECT tbl || ' ' || pk FROM sync_outbox ORDER BY 1"),
        vec!["institutions i1", "institutions i3"]
    );
    assert_eq!(
        strings(
            &b,
            "SELECT currency || ' ' || rate FROM exchange_rates WHERE currency IN ('AAA', 'BBB') ORDER BY 1"
        ),
        vec!["AAA 2.5", "BBB 3.5"],
        "a seed goes in only where its key is absent"
    );
    let state = state::load(&b).expect("load").expect("row");
    assert_eq!(state.cursor, 9);
    assert_eq!(state.snapshot_hlc, stats.max_hlc);
    let a_head = (A.0.to_vec(), from_a[0].0.seq as i64, from_a[0].1.to_vec());
    assert_eq!(heads(&b), vec![a_head]);
}

#[test]
fn derived_columns_are_recomputed_after_restore() {
    let a = device(A);
    exec(
        &a,
        "INSERT INTO transaction_categories (id, name) VALUES ('food', 'Food'); \
         INSERT INTO bank_accounts (id, name, currency, balance) VALUES ('acc', 'Main', 'CZK', '125.00'); \
         INSERT INTO bank_balance_records (id, bank_account_id, amount, currency, as_of) \
           VALUES ('rec-1', 'acc', '250.00', 'CZK', 1767225600);",
    );
    budgeting::upsert_goal_at(
        &a,
        InsertBudgetGoal {
            category_id: "food".into(),
            timeframe: "monthly".into(),
            amount: "500.00".into(),
            currency: Some("CZK".into()),
            effective_month: Some(DAY),
            effective_until: None,
        },
        NOW,
    )
    .expect("budget");
    assert_eq!(
        value(&a, "SELECT balance FROM bank_accounts WHERE id = 'acc'"),
        Some("125.00".into()),
        "the dumper's mirror is stale"
    );
    let (bytes, _) = snapshot(&a, 1);

    let b = joined(&bytes);

    let goals = "SELECT category_id || ' ' || timeframe || ' ' || amount || ' ' || currency FROM budget_goals ORDER BY 1";
    assert_eq!(strings(&b, goals), vec!["food monthly 500.00 CZK"]);
    assert_eq!(strings(&b, goals), strings(&a, goals));
    assert_eq!(
        strings(
            &b,
            "SELECT balance || ' ' || balance_as_of FROM bank_accounts WHERE id = 'acc'"
        ),
        vec!["250.00 1767225600"]
    );
}

#[test]
fn a_snapshot_with_clocks_far_ahead_restores_and_warns() {
    let a = device(A);
    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('i-future', 'Ahead')",
    );
    let ahead = pack(now_ms() + 2 * 86_400_000, 0);
    a.execute(
        "UPDATE sync_clock SET hlc = ?1 WHERE tbl = 'institutions' AND pk = 'i-future'",
        [ahead as i64],
    )
    .expect("clock ahead");
    let (bytes, stats) = snapshot(&a, 1);
    assert_eq!(stats.max_hlc, ahead);
    let far = |warnings: &[String]| {
        warnings
            .iter()
            .filter(|w| {
                w.starts_with("the snapshot carries a clock")
                    && w.ends_with("minutes ahead of this device's time")
            })
            .count()
    };

    let b = migrated();
    let joined = join(&b, B, &mut &bytes[..], NOW).expect("join");
    assert_eq!(far(&joined.warnings), 1, "{:?}", joined.warnings);
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM institutions WHERE id = 'i-future'"
        ),
        1
    );
    assert!(capture::tick(&b).expect("tick") > ahead);

    let c = device(C);
    let replaced = restore_replace(&c, &mut &bytes[..], NOW).expect("replace");
    assert_eq!(far(&replaced.warnings), 1, "{:?}", replaced.warnings);

    let d = device(D);
    let merged = restore_merge(&d, &mut &bytes[..], NOW).expect("merge");
    assert_eq!(far(&merged.warnings), 1, "{:?}", merged.warnings);
    assert_eq!(
        count(
            &d,
            "SELECT COUNT(*) FROM institutions WHERE id = 'i-future'"
        ),
        1
    );
}

#[test]
fn a_restore_that_stops_in_the_merge_leaves_nothing_behind() {
    let a = rich_dumper();
    exec(
        &a,
        "INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('AAA', 1.5, 10)",
    );
    let (bytes, _) = snapshot(&a, 4);
    let broken = with_orphan(&bytes);
    let refused = |error: AppError| {
        assert!(
            matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotInvalid"),
            "{error:?}"
        );
    };

    let b = migrated();
    exec(
        &b,
        "INSERT INTO institutions (id, name) VALUES ('b-own', 'Mine')",
    );
    let before = (table_dump(&b), clock_dump(&b), seed_dump(&b));
    refused(join(&b, B, &mut &broken[..], NOW).expect_err("stopped"));
    assert_eq!((table_dump(&b), clock_dump(&b), seed_dump(&b)), before);
    assert!(state::load(&b).expect("load").is_none());
    assert!(!capture::is_installed(&b).expect("installed"));
    assert!(b.is_autocommit());

    let c = device(C);
    exec(
        &c,
        "INSERT INTO institutions (id, name) VALUES ('c-own', 'Mine'); \
         UPDATE institutions SET name = 'Bank' WHERE id = 'c-own';",
    );
    let outbox = "SELECT tbl || ' ' || pk FROM sync_outbox ORDER BY 1";
    let snapshot_state = "SELECT cursor || ' ' || COALESCE(snapshot_seq, '-') || ' ' || snapshot_hlc FROM sync_state";
    let before = (
        table_dump(&c),
        clock_dump(&c),
        seed_dump(&c),
        heads(&c),
        strings(&c, outbox),
        strings(&c, snapshot_state),
    );
    for restore in restorers() {
        refused(restore(&c, &mut &broken[..], NOW).expect_err("stopped"));
        let after = (
            table_dump(&c),
            clock_dump(&c),
            seed_dump(&c),
            heads(&c),
            strings(&c, outbox),
            strings(&c, snapshot_state),
        );
        assert_eq!(after, before);
        assert!(
            capture::is_installed(&c).expect("installed"),
            "capture stays as it was"
        );
        assert_eq!(capture::mode(&c).expect("mode"), capture::Mode::Local);
        assert!(c.is_autocommit());
    }
}

#[test]
fn a_quarantined_author_stays_held_after_a_restore() {
    let a = device(A);
    let c = device(C);
    exec(
        &c,
        "INSERT INTO institutions (id, name) VALUES ('ic', 'From C')",
    );
    let first = send(&c);
    apply(&a, &first);
    quarantine(&a, C).expect("quarantine");
    exec(
        &c,
        "INSERT INTO institutions (id, name) VALUES ('ic2', 'Forked')",
    );
    let second = send(&c);
    let (bytes, _) = snapshot(&a, 2);
    let header_heads = records(&bytes)
        .into_iter()
        .find_map(|r| match r {
            Record::Header { heads, .. } => Some(heads),
            _ => None,
        })
        .expect("header");
    assert!(header_heads.iter().any(|h| h.device == C && h.quarantined));
    // The snapshot was taken at relay sequence 2; the fork's envelope comes after it.
    let meta = EnvelopeMeta::of(&second[0].0, second[0].1, 3);
    let quarantined =
        "SELECT quarantined FROM sync_heads WHERE device = x'03030303030303030303030303030303'";

    let b = joined(&bytes);
    assert_eq!(count(&b, quarantined), 1);
    assert_eq!(judge(&b, &meta).expect("judge"), Verdict::Held);

    // A device that applied C's first envelope before the fork learns of the quarantine.
    let d = device(D);
    apply(&d, &first);
    assert_eq!(count(&d, quarantined), 0);
    restore_merge(&d, &mut &bytes[..], NOW).expect("merge");
    assert_eq!(count(&d, quarantined), 1);
    assert_eq!(judge(&d, &meta).expect("judge"), Verdict::Held);
}

#[test]
fn restore_by_merge_never_moves_the_bookkeeping_back() {
    let a = device(A);
    let c = device(C);
    exec(
        &c,
        "INSERT INTO institutions (id, name) VALUES ('ic', 'From C')",
    );
    let from_c = send(&c);
    apply(&a, &from_c);
    let (bytes, stats) = snapshot(&a, 9);

    let b = device(B);
    let higher_hlc = stats.max_hlc + 1_000;
    b.execute(
        "UPDATE sync_state SET cursor = 50, snapshot_seq = 40, snapshot_hlc = ?1",
        [higher_hlc as i64],
    )
    .expect("bookkeeping ahead");
    b.execute(
        "INSERT INTO sync_heads (device, seq, hash) VALUES (?1, 3, ?2)",
        rusqlite::params![C.0.to_vec(), vec![0xcc_u8; 32]],
    )
    .expect("newer head");

    restore_merge(&b, &mut &bytes[..], NOW).expect("merge");

    let state = state::load(&b).expect("load").expect("row");
    assert_eq!(state.cursor, 50);
    assert_eq!(state.snapshot_hlc, higher_hlc);
    assert_eq!(count(&b, "SELECT snapshot_seq FROM sync_state"), 40);
    assert_eq!(
        heads(&b),
        vec![(C.0.to_vec(), 3, vec![0xcc; 32])],
        "the header's C head (seq 1) does not move the newer local head back"
    );
}

/// Migration 033: a restored snapshot at this device's barrier is one the vault already holds
/// there, so a crossing it covers forces no upload.
#[test]
fn a_restore_records_the_snapshots_barrier() {
    let a = device(A);
    let (bytes, _) = snapshot(&a, 4);
    let b = device(B);
    exec(&b, "UPDATE sync_state SET snapshot_barrier = 0");
    let forced = |conn: &Connection| {
        let sync = state::load(conn).expect("load").expect("row");
        let marks = state::marks(conn).expect("marks").expect("row");
        state::needs_forced_snapshot(&sync, &marks)
    };
    assert!(forced(&b), "a crossing with no snapshot at the new barrier");

    restore_merge(&b, &mut &bytes[..], NOW).expect("merge");

    let barrier = state::load(&b).expect("load").expect("row").barrier;
    let marks = state::marks(&b).expect("marks").expect("row");
    assert_eq!(marks.snapshot_barrier, barrier);
    assert!(!forced(&b));
}

fn missed(conn: &Connection) -> Option<u64> {
    state::load(conn)
        .expect("load")
        .expect("row")
        .missed_below_barrier
}

/// D has not updated (barrier 19) and writes `institutions` row `id`; relay sequence 3.
fn lagging_envelope() -> (Sent, EnvelopeMeta) {
    let d = device(D);
    exec(&d, "UPDATE sync_state SET barrier = 19");
    exec(
        &d,
        "INSERT INTO institutions (id, name) VALUES ('id', 'From D')",
    );
    let from_d = send(&d);
    assert_eq!(from_d[0].0.barrier, 19);
    let meta = EnvelopeMeta::of(&from_d[0].0, from_d[0].1, 3);
    (from_d, meta)
}

/// Device `id` crossed at unlock with a stale cursor (no marker yet): D's envelope is below its
/// barrier, so it is skipped, never applied raw, and the miss is recorded.
fn crossed_and_skipped(id: DeviceId, meta: &EnvelopeMeta) -> Connection {
    let conn = device(id);
    assert_eq!(judge(&conn, meta).expect("judge"), Verdict::BelowBarrier);
    crate::services::sync::chain::skip_below_barrier(&conn, meta).expect("skip");
    assert_eq!(missed(&conn), Some(3));
    conn
}

fn has_ds_row(conn: &Connection) -> bool {
    count(conn, "SELECT COUNT(*) FROM institutions WHERE id = 'id'") == 1
}

#[test]
fn a_crossed_device_skips_a_lagging_envelope_until_a_healing_snapshot() {
    use crate::services::sync::chain::{may_upload_snapshot, snapshot_heals_missed};
    let (from_d, meta) = lagging_envelope();
    // A dumps once before it has D's envelope (relay 2), then applies it while still below the
    // barrier, then crosses.
    let a = device(A);
    let (early, _) = snapshot(&a, 2);
    exec(&a, "UPDATE sync_state SET barrier = 19");
    apply(&a, &from_d);
    exec(&a, "UPDATE sync_state SET barrier = 25");
    assert!(may_upload_snapshot(
        &state::load(&a).expect("load").expect("row")
    ));
    let b = crossed_and_skipped(B, &meta);
    assert!(!has_ds_row(&b));

    // A snapshot dumped before the skipped position does not heal it.
    let sync = state::load(&b).expect("load").expect("row");
    assert!(!snapshot_heals_missed(&sync, 25, 2, None));
    restore_merge(&b, &mut &early[..], NOW).expect("merge");
    assert_eq!(missed(&b), Some(3));
    assert!(!has_ds_row(&b));

    // A clean one at the barrier and past it does, and the restore clears the miss.
    let (healing, _) = snapshot(&a, 5);
    assert!(snapshot_heals_missed(&sync, 25, 5, None));
    restore_merge(&b, &mut &healing[..], NOW).expect("merge");
    assert_eq!(missed(&b), None);
    assert_eq!(
        value(&b, "SELECT name FROM institutions WHERE id = 'id'"),
        Some("From D".to_string())
    );
}

#[test]
fn a_snapshot_from_a_dumper_with_a_pending_miss_does_not_heal() {
    use crate::services::sync::chain::may_upload_snapshot;
    let (_, meta) = lagging_envelope();
    // C crossed early and skipped D's envelope too; its snapshot lacks D's row.
    let c = crossed_and_skipped(C, &meta);
    assert!(!may_upload_snapshot(
        &state::load(&c).expect("load").expect("row")
    ));
    let (from_c, _) = snapshot(&c, 6);
    let b = crossed_and_skipped(B, &meta);

    restore_merge(&b, &mut &from_c[..], NOW).expect("merge");

    assert_eq!(
        missed(&b),
        Some(3),
        "the mark stays until a clean dumper heals it"
    );
    assert!(!has_ds_row(&b));
}

/// A dumps at relay sequence 9 with a pending miss at 7 (ruling T8-7: the restorer cannot get
/// the skipped rows from this snapshot either).
fn dumped_with_a_miss() -> Vec<u8> {
    let a = device(A);
    state::note_missed_below_barrier(&a, 7).expect("miss");
    snapshot(&a, 9).0
}

#[test]
fn a_replace_takes_the_dumpers_miss_as_its_own() {
    let pending = dumped_with_a_miss();
    assert_eq!(
        missed(&joined(&pending)),
        Some(7),
        "a joiner adopts the miss"
    );

    // A replace also drops a miss of the device's own: the cursor restarts at the snapshot's.
    let b = device(B);
    state::note_missed_below_barrier(&b, 12).expect("own miss");
    restore_replace(&b, &mut &pending[..], NOW).expect("replace");
    assert_eq!(missed(&b), Some(7));
    let (clean, _) = snapshot(&device(C), 9);
    restore_replace(&b, &mut &clean[..], NOW).expect("replace");
    assert_eq!(missed(&b), None);
}

#[test]
fn a_merge_records_the_dumpers_miss_past_the_local_cursor() {
    let pending = dumped_with_a_miss();
    let at = |cursor: i64, own: Option<u64>| {
        let conn = device(B);
        conn.execute("UPDATE sync_state SET cursor = ?1", [cursor])
            .expect("cursor");
        if let Some(own) = own {
            state::note_missed_below_barrier(&conn, own).expect("own miss");
        }
        restore_merge(&conn, &mut &pending[..], NOW).expect("merge");
        missed(&conn)
    };
    // Below the miss: the cursor jumps to 9 past an envelope the snapshot lacks.
    assert_eq!(at(3, None), Some(7));
    assert_eq!(at(6, None), Some(7));
    // At or past it: this device dealt with that envelope itself.
    assert_eq!(at(7, None), None);
    assert_eq!(at(8, None), None);
    // The newest miss is kept.
    assert_eq!(at(3, Some(10)), Some(10));
    assert_eq!(at(3, Some(5)), Some(7));
}
