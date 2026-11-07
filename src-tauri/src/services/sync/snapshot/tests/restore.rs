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
        .map(|r| r.key.to_pk_text())
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
    let meta = EnvelopeMeta::of(&second[0].0, second[0].1);
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
            warnings: Vec::new()
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

/// `bytes` with one more row record at the end of the rows: a category whose parent no device
/// has ever seen, so the merge stops after writing every row before it.
fn with_orphan(bytes: &[u8]) -> Vec<u8> {
    let mut all = records(bytes);
    let at = all
        .iter()
        .position(|r| matches!(r, Record::Seed { .. } | Record::End { .. }))
        .expect("seeds or end");
    let clock = Clock { hlc: 5, device: A };
    let text = |v: &str| (SqlValue::Text(v.to_string()), clock);
    all.insert(
        at,
        Record::Row(RowChange {
            table: "transaction_categories".into(),
            key: RowKey::Single("orphan".into()),
            generation: clock,
            insert: true,
            deleted: None,
            columns: BTreeMap::from([
                ("name".to_string(), text("Orphan")),
                ("parent_id".to_string(), text("never-seen")),
                ("kind".to_string(), text("expense")),
            ]),
            parents: BTreeMap::from([("parent_id".to_string(), clock)]),
        }),
    );
    let Some(Record::End { rows, .. }) = all.last_mut() else {
        panic!("end last");
    };
    *rows += 1;
    stream_of(&all)
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
    let meta = EnvelopeMeta::of(&second[0].0, second[0].1);
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
