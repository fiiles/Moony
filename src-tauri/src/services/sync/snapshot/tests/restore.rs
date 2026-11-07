use super::*;

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

    assert_eq!(restored, stats);
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
