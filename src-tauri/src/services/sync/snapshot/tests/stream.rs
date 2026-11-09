use super::*;

#[test]
fn a_profile_with_rows_from_before_sync_dumps_every_row() {
    let a = migrated();
    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('i-old', 'Before sync')",
    );
    state::enable(&a, A).expect("enable");
    let (bytes, _) = snapshot(&a, 0);
    let records = records(&bytes);
    let rows = rows(&records);

    let old = rows
        .iter()
        .find(|r| r.table == "institutions" && r.key == RowKey::Single("i-old".into()))
        .expect("the pre-sync row travels");
    assert!(old.insert);
    assert_eq!(old.generation, Clock::ZERO);
    assert!(old.columns.values().all(|(_, clock)| *clock == Clock::ZERO));
    assert_eq!(
        old.columns.get("name"),
        Some(&(SqlValue::Text("Before sync".into()), Clock::ZERO))
    );
    for spec in synced_tables() {
        let live = rows
            .iter()
            .filter(|r| r.table == spec.name && r.insert)
            .count() as i64;
        let expected = match spec.name {
            "app_config" => count(
                &a,
                &format!(
                    "SELECT COUNT(*) FROM app_config WHERE key IN ({})",
                    APP_CONFIG_SYNCED_KEYS
                        .iter()
                        .map(|k| format!("'{k}'"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            ),
            table => count(&a, &format!("SELECT COUNT(*) FROM {table}")),
        };
        assert_eq!(live, expected, "{}", spec.name);
    }
    assert!(count(&a, "SELECT COUNT(*) FROM transaction_categories") > 0);
}

#[test]
fn local_data_never_enters_a_snapshot() {
    let a = device(A);
    exec(
        &a,
        "INSERT INTO user_profile (id, name, surname, email, language, mcp_server_token, theme) \
           VALUES (1, 'Alex', 'Novak', 'a@b.cz', 'xx-language-marker', 'token-marker', 'theme-marker'); \
         INSERT INTO app_config (key, value) VALUES ('fx_reference', 'fx-marker') \
           ON CONFLICT(key) DO UPDATE SET value = excluded.value; \
         INSERT INTO app_config (key, value) VALUES ('backup.lastCreatedAt', 'backup-marker') \
           ON CONFLICT(key) DO UPDATE SET value = excluded.value; \
         UPDATE sync_state SET relay_url = 'relay-marker', vault_keys = 'vault-marker'; \
         INSERT INTO real_estate (id, name, address, type) VALUES ('re-1', 'Flat', 'Street', 'flat'); \
         INSERT INTO real_estate_documents (id, real_estate_id, name, blob_id) \
           VALUES ('doc-1', 're-1', 'doc-marker', 'blob-marker');",
    );
    let (bytes, _) = snapshot(&a, 1);
    let text = String::from_utf8_lossy(&bytes);
    for marker in [
        "xx-language-marker",
        "token-marker",
        "theme-marker",
        "fx-marker",
        "backup-marker",
        "relay-marker",
        "vault-marker",
        "doc-marker",
        "blob-marker",
        "fx_reference",
        "backup.lastCreatedAt",
        "mcp_server_token",
        "sync_state",
        "real_estate_documents",
    ] {
        assert!(!text.contains(marker), "{marker} leaked into the snapshot");
    }
    for record in records(&bytes) {
        match record {
            Record::Row(row) => {
                let spec = crate::services::sync::registry::spec(&row.table).expect("spec");
                assert_eq!(spec.class, TableClass::Synced, "{}", row.table);
                for column in row.columns.keys() {
                    assert_ne!(
                        column_class(&row.table, column),
                        ColumnClass::Local,
                        "{} {column}",
                        row.table
                    );
                }
            }
            Record::Seed { table, .. } => {
                let spec = crate::services::sync::registry::spec(&table).expect("spec");
                assert_eq!(spec.class, TableClass::Seed, "{table}");
            }
            _ => {}
        }
    }
}

#[test]
fn the_seed_tables_travel_as_plain_rows() {
    let a = device(A);
    exec(
        &a,
        "DELETE FROM exchange_rates; DELETE FROM exchange_rate_history; \
         INSERT INTO portfolio_metrics_history (id, recorded_at) VALUES ('pmh-1', 100); \
         INSERT INTO stock_value_history (id, ticker, recorded_at, quantity, price, currency) \
           VALUES ('svh-1', 'AAPL', 100, '3', '150.25', 'USD'); \
         INSERT INTO crypto_value_history (id, ticker, recorded_at, quantity, price, currency) \
           VALUES ('cvh-1', 'BTC', 100, '0.5', '60000', 'USD'); \
         INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('USD', 0.85125, 100); \
         INSERT INTO exchange_rate_history (date, currency, rate) VALUES (100, 'USD', 0.8525); \
         INSERT INTO stock_data (id, ticker, original_price, price_date) VALUES ('sd-1', 'AAPL', '150.25', 100); \
         INSERT INTO crypto_prices (id, symbol, price) VALUES ('cp-1', 'BTC', '60000'); \
         INSERT INTO dividend_data (id, ticker) VALUES ('dd-1', 'AAPL'); \
         INSERT INTO stock_dividend_events (ticker, ex_date, amount, currency, fetched_at) \
           VALUES ('AAPL', 100, '0.25', 'USD', 100); \
         INSERT INTO stock_dividend_coverage (ticker, covered_from, fetched_at) VALUES ('AAPL', 50, 100);",
    );
    let (bytes, stats) = snapshot(&a, 1);
    let seeds: Vec<(String, BTreeMap<String, SqlValue>)> = records(&bytes)
        .into_iter()
        .filter_map(|r| match r {
            Record::Seed { table, row } => Some((table, row)),
            _ => None,
        })
        .collect();
    assert_eq!(stats.seeds, seeds.len() as u64);
    let mut seed_tables: Vec<&str> = seeds.iter().map(|(t, _)| t.as_str()).collect();
    seed_tables.dedup();
    assert_eq!(
        seed_tables,
        vec![
            "portfolio_metrics_history",
            "stock_value_history",
            "crypto_value_history",
            "exchange_rates",
            "exchange_rate_history",
            "stock_data",
            "crypto_prices",
            "dividend_data",
            "stock_dividend_events",
            "stock_dividend_coverage",
        ]
    );
    let rate = seeds
        .iter()
        .find(|(t, _)| t == "exchange_rates")
        .map(|(_, row)| row.clone())
        .expect("rate");
    assert_eq!(
        rate,
        BTreeMap::from([
            ("currency".to_string(), SqlValue::Text("USD".into())),
            ("fetched_at".to_string(), SqlValue::Integer(100)),
            ("rate".to_string(), SqlValue::Real(0.85125)),
        ])
    );

    let b = joined(&bytes);
    assert_eq!(seed_dump(&b), seed_dump(&a));
}

#[test]
fn heads_and_cursor_come_from_the_header() {
    let a = device(A);
    let c = device(C);
    exec(
        &c,
        "INSERT INTO institutions (id, name) VALUES ('ic', 'From C')",
    );
    let from_c = send(&c);
    apply(&a, &from_c);
    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('ia', 'From A')",
    );
    let from_a = send(&a);
    let (bytes, stats) = snapshot(&a, 7);

    let header = records(&bytes)
        .into_iter()
        .find(|r| matches!(r, Record::Header { .. }))
        .expect("header");
    let Record::Header {
        schema: header_schema,
        barrier,
        seq,
        heads: header_heads,
        barrier_seq,
        missed_below_barrier,
    } = header
    else {
        unreachable!();
    };
    assert_eq!(
        header_schema,
        crate::services::sync::registry::keys::schema_number()
    );
    assert_eq!(
        barrier,
        state::load(&a).expect("load").expect("row").barrier
    );
    assert_eq!(seq, 7);
    assert_eq!(barrier_seq, None, "A never saw a barrier marker");
    assert_eq!(missed_below_barrier, None, "A skipped nothing");
    assert_eq!(
        header_heads,
        vec![
            Head {
                device: C,
                seq: 1,
                hash: from_c[0].1,
                quarantined: false
            },
            Head {
                device: A,
                seq: from_a[0].0.seq,
                hash: from_a[0].1,
                quarantined: false
            },
        ]
    );

    let b = joined(&bytes);
    assert_eq!(
        heads(&b),
        vec![
            (A.0.to_vec(), 1, from_a[0].1.to_vec()),
            (C.0.to_vec(), 1, from_c[0].1.to_vec()),
        ]
    );
    let state = state::load(&b).expect("load").expect("row");
    assert_eq!(state.cursor, 7);
    assert_eq!(state.snapshot_hlc, stats.max_hlc);
    assert_eq!(count(&b, "SELECT snapshot_seq FROM sync_state"), 7);
}

#[test]
fn a_truncated_stream_is_rejected_without_writing() {
    let a = rich_dumper();
    let (bytes, _) = snapshot(&a, 1);
    let all = records(&bytes);
    let without_end = stream_of(&all[..all.len() - 1]);
    let mut miscounted = all.clone();
    let Some(Record::End { rows, .. }) = miscounted.last_mut() else {
        panic!("end last");
    };
    *rows += 1;
    let miscounted = stream_of(&miscounted);
    let mut trailing = bytes.clone();
    trailing.extend_from_slice(&stream_of(&all[..1]));

    for broken in [
        bytes[..bytes.len() - 1].to_vec(),
        bytes[..40].to_vec(),
        without_end,
        miscounted,
        trailing,
    ] {
        let b = migrated();
        exec(
            &b,
            "INSERT INTO institutions (id, name) VALUES ('b-own', 'Mine')",
        );
        let before = table_dump(&b);
        let error = join(&b, B, &mut &broken[..], NOW).expect_err("rejected");
        assert!(
            matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotInvalid"),
            "{error:?}"
        );
        assert_eq!(table_dump(&b), before);
        assert!(state::load(&b).expect("load").is_none(), "join rolled back");
        assert!(!capture::is_installed(&b).expect("installed"));

        let b = device(B);
        let before = (table_dump(&b), clock_dump(&b));
        for restore in restorers() {
            let error = restore(&b, &mut &broken[..], NOW).expect_err("rejected");
            assert!(matches!(error, AppError::Validation(_)), "{error:?}");
            assert_eq!((table_dump(&b), clock_dump(&b)), before);
        }
    }
}

#[test]
fn a_snapshot_from_below_a_crossed_barrier_is_refused() {
    let a = rich_dumper();
    let (bytes, _) = snapshot(&a, 1);

    let b = device(B);
    exec(&b, "UPDATE sync_state SET barrier = barrier + 1");
    let before = (table_dump(&b), clock_dump(&b));
    for restore in restorers() {
        let error = restore(&b, &mut &bytes[..], NOW).expect_err("refused");
        assert!(
            matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotBelowBarrier"),
            "{error:?}"
        );
        assert_eq!((table_dump(&b), clock_dump(&b)), before);
    }
    let error = join(&b, B, &mut &bytes[..], NOW).expect_err("refused");
    assert!(
        matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotBelowBarrier"),
        "{error:?}"
    );
    assert_eq!((table_dump(&b), clock_dump(&b)), before);

    let mut newer = records(&bytes);
    let Some(Record::Header { schema, .. }) = newer.get_mut(1) else {
        panic!("header second");
    };
    *schema += 1;
    let newer = stream_of(&newer);
    let fresh = migrated();
    let error = join(&fresh, B, &mut &newer[..], NOW).expect_err("refused");
    assert!(
        matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotNewerSchema"),
        "{error:?}"
    );
    assert!(state::load(&fresh).expect("load").is_none());
}

/// An account saved by an older build leaves a disabled row with that build's barrier; joining
/// on a build with a higher barrier still refuses a snapshot from below it.
#[test]
fn joining_from_a_stale_disabled_row_refuses_a_snapshot_below_the_barrier() {
    use crate::services::sync::keys;
    use crate::services::sync::registry::keys::barrier_number;

    let a = rich_dumper();
    let (bytes, _) = snapshot(&a, 1);
    let mut stale = records(&bytes);
    let Some(Record::Header { barrier, .. }) = stale.get_mut(1) else {
        panic!("header second");
    };
    *barrier = barrier_number() - 1;
    let stale = stream_of(&stale);
    let old_build_row = || {
        let conn = migrated();
        keys::save_account(&conn, "https://relay.example.org", "4417938205561273")
            .expect("account");
        exec(&conn, "UPDATE sync_state SET barrier = barrier - 1");
        conn
    };

    let b = old_build_row();
    let error = join(&b, B, &mut &stale[..], NOW).expect_err("refused");
    assert!(
        matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotBelowBarrier"),
        "{error:?}"
    );
    let row = state::load(&b).expect("load").expect("row");
    assert!(!row.enabled, "the refused join changed nothing");
    assert_eq!(row.barrier, barrier_number() - 1);

    let c = old_build_row();
    join(&c, C, &mut &bytes[..], NOW).expect("a snapshot at this build's barrier joins");
    assert_eq!(
        state::load(&c).expect("load").expect("row").barrier,
        barrier_number()
    );
}

#[test]
fn malformed_streams_are_errors_not_panics() {
    let invalid = |result: Result<Option<Record>>| {
        assert!(
            matches!(&result, Err(AppError::Validation(key)) if key == "validation.syncSnapshotInvalid"),
            "{result:?}"
        );
    };
    invalid(read_record(&mut &[0xFF_u8, 0xFF, 0xFF, 0xFF][..], &[]));
    for cut in 1..=3 {
        invalid(read_record(&mut &[0_u8, 0, 0, 2][..cut], &[]));
    }
    let mut unknown = Vec::new();
    write_record(&mut unknown, &serde_json::json!({ "r": "nope" })).expect("write");
    invalid(read_record(&mut &unknown[..], &[]));

    let a = device(A);
    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
         INSERT INTO exchange_rates (currency, rate, fetched_at) VALUES ('AAA', 1.5, 10);",
    );
    let (bytes, _) = snapshot(&a, 1);
    let all = records(&bytes);
    let first_seed = all
        .iter()
        .position(|r| matches!(r, Record::Seed { .. }))
        .expect("a seed");
    let mut duplicate_header = all.clone();
    duplicate_header.insert(2, all[1].clone());
    let mut duplicate_devs = all.clone();
    duplicate_devs.insert(1, all[0].clone());
    let mut row_after_seed = all.clone();
    let row = row_after_seed.remove(first_seed - 1);
    row_after_seed.insert(first_seed, row);
    assert!(matches!(
        row_after_seed[first_seed - 1],
        Record::Seed { .. }
    ));
    assert!(matches!(row_after_seed[first_seed], Record::Row(_)));
    for broken in [duplicate_header, duplicate_devs, row_after_seed] {
        let broken = stream_of(&broken);
        let error = super::super::stream::read_all(&mut &broken[..]).expect_err("rejected");
        assert!(
            matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotInvalid"),
            "{error:?}"
        );
        let b = migrated();
        assert!(join(&b, B, &mut &broken[..], NOW).is_err());
        assert!(state::load(&b).expect("load").is_none());
    }
}

#[test]
fn a_dump_refuses_while_rows_wait_to_be_pushed() {
    let pending = |conn: &Connection| {
        let mut out = Vec::new();
        let error = dump(conn, 1, &mut out).expect_err("refused");
        assert!(
            matches!(&error, AppError::Validation(key) if key == "validation.syncSnapshotPending"),
            "{error:?}"
        );
        assert!(out.is_empty(), "nothing is written");
    };
    let a = device(A);
    exec(
        &a,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Unpushed')",
    );
    pending(&a);

    send(&a);
    exec(
        &a,
        "INSERT INTO sync_unsent (seq, envelope) VALUES (1, x'00')",
    );
    pending(&a);

    exec(&a, "DELETE FROM sync_unsent");
    let mut out = Vec::new();
    let stats = dump(&a, 1, &mut out).expect("dump");
    assert!(stats.rows > 0);
    assert!(!out.is_empty());
}

#[test]
fn snapshot_header_carries_the_marker_to_a_joiner() {
    let barrier_seq =
        |conn: &Connection| state::load(conn).expect("load").expect("row").barrier_seq;
    let header_seq = |bytes: &[u8]| {
        records(bytes)
            .into_iter()
            .find_map(|r| match r {
                Record::Header { barrier_seq, .. } => Some(barrier_seq),
                _ => None,
            })
            .expect("header")
    };
    let a = device(A);
    let (without, _) = snapshot(&a, 4);
    assert_eq!(header_seq(&without), None);
    assert!(!String::from_utf8_lossy(&without).contains("barrierSeq"));

    state::set_barrier_seq(&a, Some(5)).expect("marker");
    let (bytes, _) = snapshot(&a, 9);
    assert_eq!(header_seq(&bytes), Some(5));
    assert!(String::from_utf8_lossy(&bytes).contains(r#""barrierSeq":5"#));

    // Replace adopts it.
    assert_eq!(barrier_seq(&joined(&bytes)), Some(5));
    // Merge keeps a local one, and takes the header's when it has none.
    let kept = device(C);
    state::set_barrier_seq(&kept, Some(3)).expect("local marker");
    restore_merge(&kept, &mut &bytes[..], NOW).expect("merge");
    assert_eq!(barrier_seq(&kept), Some(3));
    let taken = device(D);
    restore_merge(&taken, &mut &bytes[..], NOW).expect("merge");
    assert_eq!(barrier_seq(&taken), Some(5));

    // A marker of another barrier than this device's is not adopted, and replace drops the old.
    let higher = device(A);
    exec(
        &higher,
        "UPDATE sync_state SET barrier = barrier + 1, barrier_seq = 5",
    );
    let (other_barrier, _) = snapshot(&higher, 9);
    assert_eq!(header_seq(&other_barrier), Some(5));
    let replaced = device(B);
    state::set_barrier_seq(&replaced, Some(3)).expect("local marker");
    restore_replace(&replaced, &mut &other_barrier[..], NOW).expect("replace");
    assert_eq!(barrier_seq(&replaced), None);
    let merged = device(C);
    restore_merge(&merged, &mut &other_barrier[..], NOW).expect("merge");
    assert_eq!(barrier_seq(&merged), None);
}

#[test]
fn the_header_carries_the_dumpers_pending_miss() {
    let header_missed = |bytes: &[u8]| {
        records(bytes)
            .into_iter()
            .find_map(|r| match r {
                Record::Header {
                    missed_below_barrier,
                    ..
                } => Some(missed_below_barrier),
                _ => None,
            })
            .expect("header")
    };
    let a = device(A);
    let (clean, _) = snapshot(&a, 4);
    assert_eq!(header_missed(&clean), None);
    assert!(!String::from_utf8_lossy(&clean).contains("missedBelowBarrier"));

    state::note_missed_below_barrier(&a, 7).expect("miss");
    let (pending, _) = snapshot(&a, 9);
    assert_eq!(header_missed(&pending), Some(7));
    assert!(String::from_utf8_lossy(&pending).contains(r#""missedBelowBarrier":7"#));
}
