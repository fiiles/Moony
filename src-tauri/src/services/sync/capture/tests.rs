use super::*;
use crate::error::AppError;
use crate::services::sync::hlc::{now_ms, pack, Clock, DeviceId};
use crate::services::sync::test_support::migrated;
use rusqlite::Connection;

const DEVICE: DeviceId = DeviceId([7; 16]);
const OTHER: DeviceId = DeviceId([9; 16]);

/// Every Synced table of the registry (spec §5.1 with the 16a attachment change).
const SYNCED: [&str; 44] = [
    "app_config",
    "bank_account_zones",
    "bank_accounts",
    "bank_balance_records",
    "bank_transactions",
    "bonds",
    "budget_goal_versions",
    "cashflow_items",
    "cashflow_plan_exclusions",
    "categorization_rules",
    "crypto_investments",
    "crypto_price_overrides",
    "crypto_transactions",
    "csv_import_batches",
    "dividend_overrides",
    "institutions",
    "insurance_policies",
    "investment_dividends",
    "investment_transactions",
    "learned_payees",
    "loan_events",
    "loans",
    "milestone_states",
    "other_asset_transactions",
    "other_asset_valuations",
    "other_assets",
    "projection_settings",
    "real_estate",
    "real_estate_insurances",
    "real_estate_loans",
    "real_estate_one_time_costs",
    "real_estate_valuations",
    "spending_plans",
    "stock_import_batches",
    "stock_investment_tags",
    "stock_investments",
    "stock_price_overrides",
    "stock_splits",
    "stock_tag_groups",
    "stock_tags",
    "sync_devices",
    "transaction_categories",
    "user_profile",
    "watched_stocks",
];

fn installed() -> Connection {
    let conn = migrated();
    install(&conn, DEVICE).expect("install");
    conn
}

fn exec(conn: &Connection, sql: &str) {
    conn.execute_batch(sql).expect(sql);
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

/// `(pk, col, hlc, device)` of one table's clocks, ordered by key and column.
fn clocks(conn: &Connection, table: &str) -> Vec<(String, String, i64, Vec<u8>)> {
    let mut stmt = conn
        .prepare("SELECT pk, col, hlc, device FROM sync_clock WHERE tbl = ?1 ORDER BY pk, col")
        .expect("prepare");
    stmt.query_map([table], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
    })
    .expect("query")
    .map(|r| r.expect("row"))
    .collect()
}

fn clock_cols(conn: &Connection, table: &str, pk: &str) -> Vec<String> {
    clocks(conn, table)
        .into_iter()
        .filter(|c| c.0 == pk)
        .map(|c| c.1)
        .collect()
}

fn clock_of(conn: &Connection, table: &str, pk: &str, col: &str) -> Option<i64> {
    clocks(conn, table)
        .into_iter()
        .find(|c| c.0 == pk && c.1 == col)
        .map(|c| c.2)
}

fn outbox(conn: &Connection) -> Vec<(String, String)> {
    let mut stmt = conn
        .prepare("SELECT tbl, pk FROM sync_outbox ORDER BY tbl, pk")
        .expect("prepare");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
}

fn row(tbl: &str, pk: &str) -> (String, String) {
    (tbl.to_string(), pk.to_string())
}

#[test]
fn a_profile_without_sync_has_no_triggers_and_no_journal() {
    let conn = migrated();
    crate::services::sync::on_open(&conn);
    let account = crate::models::bank_accounts::InsertBankAccount {
        name: "Current".into(),
        account_type: None,
        iban: None,
        bban: None,
        currency: Some("CZK".into()),
        balance: Some("100".into()),
        balance_as_of: Some(Some(1_700_000_000 / 86_400 * 86_400)),
        institution_id: None,
        interest_rate: None,
        has_zone_designation: None,
        termination_date: None,
        interest_rate_valid_until: None,
        exclude_from_balance: None,
    };
    crate::services::bank_accounts::create_account(&conn, &account).expect("create account");

    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM sqlite_temp_master"),
        0,
        "no TEMP triggers and no TEMP table"
    );
    assert!(!is_installed(&conn).unwrap());
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_clock"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_outbox"), 0);
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_state"), 0);
    assert_eq!(mode(&conn).unwrap(), Mode::Local);
}

#[test]
fn installed_triggers_cover_every_synced_table_and_nothing_else() {
    let conn = installed();
    let mut stmt = conn
        .prepare(
            "SELECT name, tbl_name FROM sqlite_temp_master WHERE type = 'trigger' ORDER BY name",
        )
        .unwrap();
    let actual: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(|r| r.unwrap())
        .collect();

    let mut expected: Vec<(String, String)> = Vec::new();
    for t in SYNCED {
        // The singleton has no key to change; the two link tables have no column besides the key.
        let kinds: &[&str] = match t {
            "user_profile" => &["bdel", "del", "ins", "upd"],
            "real_estate_insurances" | "real_estate_loans" => &["bdel", "del", "ins", "pk"],
            _ => &["bdel", "del", "ins", "pk", "upd"],
        };
        for k in kinds {
            expected.push((format!("sync_{t}_{k}"), t.to_string()));
        }
    }
    // The shadow triggers of the recomputed-or-typed columns (ruling T18b).
    for (t, columns) in [
        ("bank_accounts", &["balance", "balance_as_of"][..]),
        ("stock_investments", &["quantity"][..]),
        ("crypto_investments", &["quantity", "currency"][..]),
        (
            "real_estate",
            &["market_price", "market_price_currency"][..],
        ),
        (
            "other_assets",
            &["market_price", "quantity", "average_purchase_price"][..],
        ),
    ] {
        for column in columns.iter().chain(&["del"]) {
            expected.push((format!("sync_{t}_typed_{column}"), t.to_string()));
        }
    }
    expected.sort();
    assert_eq!(actual, expected);
    assert!(is_installed(&conn).unwrap());
    assert_eq!(mode(&conn).unwrap(), Mode::Local);
}

#[test]
fn an_insert_records_a_row_clock_and_queues_the_row() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );

    let c = clocks(&conn, "institutions");
    assert_eq!(c.len(), 1);
    assert_eq!((c[0].0.as_str(), c[0].1.as_str()), ("i1", "*"));
    assert!(c[0].2 > 0);
    assert_eq!(c[0].3, DEVICE.0.to_vec());
    assert_eq!(outbox(&conn), vec![row("institutions", "i1")]);
    assert_eq!(
        count(&conn, "SELECT queued_at FROM sync_outbox"),
        c[0].2,
        "queued_at is the insert clock"
    );
}

#[test]
fn an_update_records_clocks_only_for_changed_synced_columns() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO spending_plans (id, name, year, amount, currency, icon, color) \
           VALUES ('p1', 'Trip', 2026, '100', 'CZK', 'i', 'c'); \
         INSERT INTO loans (id, name, principal) VALUES ('l1', 'Car', '1000'); \
         DELETE FROM sync_outbox;",
    );
    let star = clock_of(&conn, "spending_plans", "p1", "*").unwrap();

    exec(
        &conn,
        "UPDATE spending_plans SET note = 'flights' WHERE id = 'p1'",
    );
    assert_eq!(clock_cols(&conn, "spending_plans", "p1"), vec!["*", "note"]);
    assert!(clock_of(&conn, "spending_plans", "p1", "note").unwrap() > star);
    assert_eq!(outbox(&conn), vec![row("spending_plans", "p1")]);

    exec(&conn, "DELETE FROM sync_outbox");
    exec(
        &conn,
        "UPDATE spending_plans SET note = 'flights' WHERE id = 'p1'",
    );
    exec(
        &conn,
        "UPDATE loans SET updated_at = updated_at + 5 WHERE id = 'l1'",
    );
    exec(
        &conn,
        "UPDATE loans SET balance_anchor_amount = '900', balance_anchor_date = 0 WHERE id = 'l1'",
    );
    assert_eq!(clock_cols(&conn, "loans", "l1"), vec!["*"]);
    assert_eq!(outbox(&conn), Vec::<(String, String)>::new());
}

#[test]
fn a_group_member_change_stamps_every_member_with_one_clock() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO bank_accounts (id, name) VALUES ('a1', 'Acc'); \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date) \
           VALUES ('t1', 'a1', 'debit', '10', 'CZK', 0);",
    );
    exec(
        &conn,
        "UPDATE bank_transactions SET amount = '12' WHERE id = 't1'",
    );

    assert_eq!(
        clock_cols(&conn, "bank_transactions", "t1"),
        vec!["*", "amount", "currency"]
    );
    let amount = clock_of(&conn, "bank_transactions", "t1", "amount").unwrap();
    assert_eq!(
        clock_of(&conn, "bank_transactions", "t1", "currency"),
        Some(amount)
    );
}

#[test]
fn a_recomputed_or_typed_column_is_captured_only_without_source_rows() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO stock_investments (id, ticker, company_name, quantity) \
           VALUES ('inv-1', 'AAPL', 'Apple', '10');",
    );
    exec(
        &conn,
        "UPDATE stock_investments SET quantity = '12' WHERE id = 'inv-1'",
    );
    let typed = clock_of(&conn, "stock_investments", "inv-1", "quantity").expect("typed quantity");

    exec(
        &conn,
        "INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, quantity, \
           price_per_unit, currency, transaction_date) \
           VALUES ('tx-1', 'inv-1', 'buy', 'AAPL', 'Apple', '15', '100', 'USD', 0); \
         DELETE FROM sync_outbox;",
    );
    exec(
        &conn,
        "UPDATE stock_investments SET quantity = '15' WHERE id = 'inv-1'",
    );

    assert_eq!(
        clock_of(&conn, "stock_investments", "inv-1", "quantity"),
        Some(typed),
        "a recomputed quantity is not captured"
    );
    assert_eq!(outbox(&conn), Vec::<(String, String)>::new());
}

#[test]
fn a_delete_records_a_tombstone_and_keeps_the_generation() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    let generation = clock_of(&conn, "institutions", "i1", "*").unwrap();
    exec(
        &conn,
        "UPDATE institutions SET name = 'Better bank' WHERE id = 'i1'",
    );
    exec(&conn, "DELETE FROM institutions WHERE id = 'i1'");

    let c = clocks(&conn, "institutions");
    let cols: Vec<&str> = c.iter().map(|c| c.1.as_str()).collect();
    assert_eq!(
        cols,
        vec!["*", "-"],
        "column clocks go, the generation stays"
    );
    assert_eq!(c[0].2, generation);
    assert!(c[1].2 > generation);
    assert_eq!(c[1].3, DEVICE.0.to_vec());
    assert_eq!(outbox(&conn), vec![row("institutions", "i1")]);
}

#[test]
fn a_primary_key_change_is_a_tombstone_and_a_new_row() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    exec(
        &conn,
        "UPDATE institutions SET name = 'Renamed' WHERE id = 'i1'",
    );
    let generation = clock_of(&conn, "institutions", "i1", "*").unwrap();
    exec(&conn, "UPDATE institutions SET id = 'i2' WHERE id = 'i1'");

    assert_eq!(clock_cols(&conn, "institutions", "i1"), vec!["*", "-"]);
    assert_eq!(clock_of(&conn, "institutions", "i1", "*"), Some(generation));
    let tomb = clock_of(&conn, "institutions", "i1", "-").unwrap();
    assert_eq!(clock_cols(&conn, "institutions", "i2"), vec!["*"]);
    assert_eq!(clock_of(&conn, "institutions", "i2", "*"), Some(tomb));
    assert_eq!(
        outbox(&conn),
        vec![row("institutions", "i1"), row("institutions", "i2")]
    );
}

#[test]
fn only_allowlisted_app_config_keys_are_captured() {
    let conn = installed();
    for key in ["rule_packs_enabled", "fx_reference", "backup.lastCreatedAt"] {
        conn.execute(
            "INSERT INTO app_config (key, value) VALUES (?1, 'v1') \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            [key],
        )
        .unwrap();
        conn.execute("UPDATE app_config SET value = 'v2' WHERE key = ?1", [key])
            .unwrap();
    }
    exec(
        &conn,
        "DELETE FROM app_config WHERE key = 'backup.lastCreatedAt'",
    );

    assert_eq!(outbox(&conn), vec![row("app_config", "rule_packs_enabled")]);
    let keys: Vec<String> = clocks(&conn, "app_config")
        .into_iter()
        .map(|c| c.0)
        .collect();
    assert_eq!(keys, vec!["rule_packs_enabled", "rule_packs_enabled"]);
    assert_eq!(
        clock_cols(&conn, "app_config", "rule_packs_enabled"),
        vec!["*", "value"]
    );
}

#[test]
fn user_profile_is_captured_as_the_profile_singleton_and_local_columns_are_not() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO user_profile (name, surname, email) VALUES ('Alex', 'Novak', 'a@b.cz')",
    );
    assert_eq!(clock_cols(&conn, "user_profile", "profile"), vec!["*"]);
    assert_eq!(outbox(&conn), vec![row("user_profile", "profile")]);

    exec(&conn, "DELETE FROM sync_outbox");
    exec(
        &conn,
        "UPDATE user_profile SET language = 'cs', theme = 'dark', mcp_server_enabled = 1, \
           mcp_server_token = 'secret', menu_preferences = '{}'",
    );
    assert_eq!(clock_cols(&conn, "user_profile", "profile"), vec!["*"]);
    assert_eq!(outbox(&conn), Vec::<(String, String)>::new());

    exec(&conn, "UPDATE user_profile SET name = 'Alexandra'");
    assert_eq!(
        clock_cols(&conn, "user_profile", "profile"),
        vec!["*", "name"]
    );
    assert_eq!(outbox(&conn), vec![row("user_profile", "profile")]);
}

#[test]
fn the_composite_key_text_is_the_json_array_sqlite_writes() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO stock_investments (id, ticker, company_name, quantity) \
           VALUES ('inv-1', 'AAPL', 'Apple', '1'); \
         INSERT INTO stock_tags (id, name) VALUES ('tag-1', 'Tech'); \
         INSERT INTO stock_investment_tags (investment_id, tag_id) VALUES ('inv-1', 'tag-1');",
    );
    let c = clocks(&conn, "stock_investment_tags");
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, r#"["inv-1","tag-1"]"#);
    assert_eq!(c[0].1, "*");
    assert!(outbox(&conn).contains(&row("stock_investment_tags", r#"["inv-1","tag-1"]"#)));
}

#[test]
fn pk_expr_spells_the_three_key_shapes() {
    let tags = crate::services::sync::registry::spec("stock_investment_tags").unwrap();
    let institutions = crate::services::sync::registry::spec("institutions").unwrap();
    let profile = crate::services::sync::registry::spec("user_profile").unwrap();
    assert_eq!(
        triggers::pk_expr(institutions, &["id".to_string()], "NEW"),
        "NEW.id"
    );
    assert_eq!(
        triggers::pk_expr(tags, &["investment_id".into(), "tag_id".into()], "OLD"),
        "json_array(OLD.investment_id, OLD.tag_id)"
    );
    assert_eq!(
        triggers::pk_expr(profile, &["id".to_string()], "NEW"),
        "'profile'"
    );
}

#[test]
fn merge_mode_records_cascaded_tombstones_with_the_cascade_clock_and_queues_nothing() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO bank_accounts (id, name) VALUES ('a1', 'Acc'); \
         INSERT INTO csv_import_batches (id, bank_account_id, file_name) VALUES ('c1', 'a1', 'f.csv'); \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, import_batch_id) \
           VALUES ('t1', 'a1', 'debit', '1', 'CZK', 0, 'c1'); \
         UPDATE bank_transactions SET description = 'coffee' WHERE id = 't1'; \
         DELETE FROM sync_outbox;",
    );
    let generation = clock_of(&conn, "bank_transactions", "t1", "*").unwrap();
    let cascade = Clock {
        hlc: pack(1, 2),
        device: OTHER,
    };

    with_mode(&conn, Mode::Merge, || {
        set_cascade_clock(&conn, cascade)?;
        conn.execute_batch(
            "INSERT INTO institutions (id, name) VALUES ('i9', 'Remote'); \
             UPDATE bank_accounts SET name = 'Remote name' WHERE id = 'a1'; \
             DELETE FROM csv_import_batches WHERE id = 'c1';",
        )?;
        Ok(())
    })
    .unwrap();

    let tx = clocks(&conn, "bank_transactions");
    let tx: Vec<(&str, i64, &[u8])> = tx
        .iter()
        .map(|c| (c.1.as_str(), c.2, c.3.as_slice()))
        .collect();
    assert_eq!(
        tx,
        vec![
            ("*", generation, &DEVICE.0[..]),
            ("-", pack(1, 2) as i64, &OTHER.0[..]),
        ]
    );
    assert_eq!(
        clock_of(&conn, "csv_import_batches", "c1", "-"),
        Some(pack(1, 2) as i64)
    );
    assert!(clocks(&conn, "institutions").is_empty());
    assert_eq!(clock_cols(&conn, "bank_accounts", "a1"), vec!["*"]);
    assert_eq!(outbox(&conn), Vec::<(String, String)>::new());
    assert_eq!(mode(&conn).unwrap(), Mode::Local);
}

#[test]
fn a_merge_delete_without_a_cascade_clock_is_refused() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    let err = with_mode(&conn, Mode::Merge, || {
        conn.execute_batch("DELETE FROM institutions WHERE id = 'i1'")?;
        Ok(())
    })
    .expect_err("refused");
    assert!(err.to_string().contains("cascade clock"), "{err}");
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM institutions WHERE id = 'i1'"),
        1,
        "the delete was rolled back"
    );
}

#[test]
fn repair_mode_records_nothing() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO bank_accounts (id, name) VALUES ('a1', 'Acc'); DELETE FROM sync_outbox;",
    );
    let before = clocks(&conn, "bank_accounts");

    with_mode(&conn, Mode::Repair, || {
        conn.execute_batch(
            "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
             UPDATE institutions SET name = 'Other' WHERE id = 'i1'; \
             DELETE FROM institutions WHERE id = 'i1'; \
             UPDATE bank_accounts SET name = 'Renamed' WHERE id = 'a1'; \
             DELETE FROM bank_accounts WHERE id = 'a1';",
        )?;
        Ok(())
    })
    .unwrap();

    assert!(clocks(&conn, "institutions").is_empty());
    assert_eq!(clocks(&conn, "bank_accounts"), before);
    assert_eq!(outbox(&conn), Vec::<(String, String)>::new());
}

#[test]
fn with_mode_restores_the_previous_mode_after_an_error() {
    let conn = installed();
    let err = with_mode(&conn, Mode::Merge, || -> Result<()> {
        assert_eq!(mode(&conn)?, Mode::Merge);
        Err(AppError::Internal("boom".into()))
    })
    .expect_err("the closure's error");
    assert!(err.to_string().contains("boom"));
    assert_eq!(mode(&conn).unwrap(), Mode::Local);

    with_mode(&conn, Mode::Repair, || {
        with_mode(&conn, Mode::Merge, || {
            assert_eq!(mode(&conn)?, Mode::Merge);
            Ok(())
        })?;
        assert_eq!(mode(&conn)?, Mode::Repair);
        Ok(())
    })
    .unwrap();
    assert_eq!(mode(&conn).unwrap(), Mode::Local);

    // Not installed: a plain call.
    let plain = migrated();
    assert_eq!(with_mode(&plain, Mode::Merge, || Ok(5)).unwrap(), 5);
    assert_eq!(mode(&plain).unwrap(), Mode::Local);
}

#[test]
fn the_hlc_starts_above_every_stored_clock() {
    let conn = migrated();
    let future = pack(now_ms() + 10 * 86_400_000, 9);
    conn.execute(
        "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES ('institutions', 'x', '*', ?1, ?2)",
        rusqlite::params![future as i64, OTHER.0.to_vec()],
    )
    .unwrap();
    install(&conn, DEVICE).unwrap();

    let t = tick(&conn).unwrap();
    assert_eq!(t, future + 1);
    assert_eq!(count(&conn, "SELECT sync_hlc_now()"), t as i64);
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    assert_eq!(
        clock_of(&conn, "institutions", "i1", "*"),
        Some((future + 2) as i64)
    );
}

#[test]
fn observing_a_remote_clock_moves_the_hlc_and_flags_a_far_future_one() {
    let conn = installed();
    let near = pack(now_ms() + 60_000, 3);
    assert!(matches!(observe(&conn, near).unwrap(), Observation::Ok));
    assert_eq!(tick(&conn).unwrap(), near + 1);

    let far = pack(now_ms() + 3 * 86_400_000, 0);
    match observe(&conn, far).unwrap() {
        Observation::FarFuture { ahead_ms } => assert!(ahead_ms > 2 * 86_400_000),
        Observation::Ok => panic!("a clock three days ahead must be flagged"),
    }
    assert_eq!(tick(&conn).unwrap(), far + 1);
}

/// A local delete ticks once: the deleted row and every row its FK cascades delete, recursively,
/// get that same tombstone, exactly as merge mode stamps a cascade (F1).
#[test]
fn a_local_cascade_stamps_every_cascaded_row_with_the_parent_tombstone() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO bank_accounts (id, name) VALUES ('a1', 'Acc'), ('a2', 'Other'); \
         INSERT INTO csv_import_batches (id, bank_account_id, file_name) VALUES \
           ('c1', 'a1', 'one.csv'), ('c2', 'a2', 'two.csv'); \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, import_batch_id) VALUES \
           ('t1', 'a1', 'debit', '1', 'CZK', 0, 'c1'), ('t2', 'a1', 'debit', '2', 'CZK', 0, 'c1'), \
           ('t3', 'a2', 'debit', '3', 'CZK', 0, 'c2'); \
         DELETE FROM sync_outbox;",
    );

    exec(&conn, "DELETE FROM csv_import_batches WHERE id = 'c1'");
    exec(&conn, "DELETE FROM bank_accounts WHERE id = 'a2'");

    let batch = clock_of(&conn, "csv_import_batches", "c1", "-").expect("batch tombstone");
    assert_eq!(clock_of(&conn, "bank_transactions", "t1", "-"), Some(batch));
    assert_eq!(clock_of(&conn, "bank_transactions", "t2", "-"), Some(batch));
    let account = clock_of(&conn, "bank_accounts", "a2", "-").expect("account tombstone");
    assert!(account > batch);
    assert_eq!(
        clock_of(&conn, "csv_import_batches", "c2", "-"),
        Some(account)
    );
    assert_eq!(
        clock_of(&conn, "bank_transactions", "t3", "-"),
        Some(account)
    );
    assert_eq!(
        outbox(&conn),
        vec![
            row("bank_accounts", "a2"),
            row("bank_transactions", "t1"),
            row("bank_transactions", "t2"),
            row("bank_transactions", "t3"),
            row("csv_import_batches", "c1"),
            row("csv_import_batches", "c2"),
        ]
    );
}

#[test]
fn unrelated_deletes_after_a_cascade_get_fresh_clocks() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO bank_accounts (id, name) VALUES ('a1', 'Acc'); \
         INSERT INTO csv_import_batches (id, bank_account_id, file_name) VALUES ('c1', 'a1', 'one.csv'); \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, import_batch_id) \
           VALUES ('t1', 'a1', 'debit', '1', 'CZK', 0, 'c1'); \
         INSERT INTO institutions (id, name) VALUES ('i1', 'One'), ('i2', 'Two');",
    );

    exec(&conn, "DELETE FROM csv_import_batches WHERE id = 'c1'");
    exec(&conn, "DELETE FROM institutions WHERE id = 'i1'");
    exec(&conn, "DELETE FROM institutions WHERE id = 'i2'");

    let cascade = clock_of(&conn, "csv_import_batches", "c1", "-").unwrap();
    let first = clock_of(&conn, "institutions", "i1", "-").unwrap();
    let second = clock_of(&conn, "institutions", "i2", "-").unwrap();
    assert!(
        cascade < first && first < second,
        "{cascade} {first} {second}"
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sync_mode WHERE local_hlc IS NOT NULL OR local_owner IS NOT NULL"
        ),
        0
    );
}

/// A delete statement that fails after its BEFORE trigger ran rolls the cascade clock back with
/// it, so the next delete still ticks a fresh clock.
#[test]
fn a_failed_delete_leaves_no_cascade_clock_behind() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO transaction_categories (id, name) VALUES ('k', 'Kept'); \
         INSERT INTO categorization_rules (id, name, rule_type, pattern, category_id) \
           VALUES ('r1', 'Rule', 'contains', 'x', 'k'); \
         INSERT INTO institutions (id, name) VALUES ('i1', 'One');",
    );
    let before = tick(&conn).unwrap() as i64;

    let failed = conn.execute_batch("DELETE FROM transaction_categories WHERE id = 'k'");

    assert!(failed.is_err(), "the NO ACTION key refuses the delete");
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sync_mode WHERE local_hlc IS NOT NULL"
        ),
        0
    );
    exec(&conn, "DELETE FROM institutions WHERE id = 'i1'");
    assert!(clock_of(&conn, "institutions", "i1", "-").unwrap() > before);
    assert_eq!(clock_of(&conn, "transaction_categories", "k", "-"), None);
}

/// A multi-row delete ticks once per outermost row: each batch's cascaded payments carry that
/// batch's own tombstone.
#[test]
fn a_multi_row_delete_stamps_each_cascade_with_its_own_row_clock() {
    let conn = installed();
    exec(
        &conn,
        "INSERT INTO bank_accounts (id, name) VALUES ('a1', 'Acc'); \
         INSERT INTO csv_import_batches (id, bank_account_id, file_name) VALUES \
           ('c1', 'a1', 'one.csv'), ('c2', 'a1', 'two.csv'); \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, import_batch_id) VALUES \
           ('t1', 'a1', 'debit', '1', 'CZK', 0, 'c1'), ('t2', 'a1', 'debit', '2', 'CZK', 0, 'c2');",
    );

    exec(
        &conn,
        "DELETE FROM csv_import_batches WHERE id IN ('c1', 'c2')",
    );

    let first = clock_of(&conn, "csv_import_batches", "c1", "-").unwrap();
    let second = clock_of(&conn, "csv_import_batches", "c2", "-").unwrap();
    assert_ne!(first, second);
    assert_eq!(clock_of(&conn, "bank_transactions", "t1", "-"), Some(first));
    assert_eq!(
        clock_of(&conn, "bank_transactions", "t2", "-"),
        Some(second)
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sync_mode WHERE local_hlc IS NOT NULL"
        ),
        0
    );
}

/// A cascade clock left set outside a delete is a bug; `with_mode` says so loudly in debug.
#[test]
#[should_panic(expected = "local cascade clock")]
fn with_mode_refuses_a_leaked_local_cascade_clock() {
    let conn = installed();
    exec(
        &conn,
        "UPDATE sync_mode SET local_hlc = 1, local_owner = 'x'",
    );
    let _ = with_mode(&conn, Mode::Repair, || Ok(()));
}
