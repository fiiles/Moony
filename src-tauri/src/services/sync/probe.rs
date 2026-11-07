//! Probes of the SQLite behaviours the trigger-capture design rests on. They are facts, not
//! features: a failure here means the design has to change.

use super::test_support::migrated;
use rusqlite::functions::FunctionFlags;
use rusqlite::Connection;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;

fn with_log() -> Connection {
    let conn = migrated();
    conn.execute_batch("CREATE TABLE probe_log (tbl TEXT, op TEXT, pk TEXT)")
        .expect("probe_log");
    conn
}

fn log_rows(conn: &Connection) -> Vec<(String, String, Option<String>)> {
    let mut stmt = conn
        .prepare("SELECT tbl, op, pk FROM main.probe_log ORDER BY rowid")
        .expect("prepare");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect("count")
}

const INSTITUTION: &str = "INSERT INTO main.institutions (id, name) VALUES ('i1', 'Bank')";
const INSTITUTION_TRIGGER: &str = "CREATE TEMP TRIGGER t AFTER INSERT ON main.institutions \
     BEGIN INSERT INTO probe_log VALUES ('institutions','ins',NEW.id); END";

#[test]
fn temp_trigger_on_main_table_writes_an_unqualified_main_table() {
    let conn = with_log();
    conn.execute_batch(INSTITUTION_TRIGGER).unwrap();
    conn.execute_batch(INSTITUTION).unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM main.probe_log"), 1);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sqlite_temp_master WHERE name = 't'"
        ),
        1
    );
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM sqlite_master WHERE name = 't'"),
        0
    );
}

#[test]
fn temp_trigger_fires_inside_a_transaction_that_also_writes_the_attached_files_schema() {
    let conn = with_log();
    let dir = tempfile::tempdir().unwrap();
    crate::db::files_schema::attach(&conn, &dir.path().join("attachments.db")).unwrap();
    conn.execute_batch(INSTITUTION_TRIGGER).unwrap();
    conn.execute_batch(
        "BEGIN; \
         INSERT INTO files.blobs (id, data, size, sha256) VALUES ('b1', x'00', 1, 'x'); \
         INSERT INTO main.institutions (id, name) VALUES ('i1', 'Bank'); \
         COMMIT;",
    )
    .unwrap();
    assert_eq!(log_rows(&conn).len(), 1);
}

#[test]
fn temp_triggers_survive_attaching_files() {
    let conn = with_log();
    conn.execute_batch(INSTITUTION_TRIGGER).unwrap();
    let dir = tempfile::tempdir().unwrap();
    crate::db::files_schema::attach(&conn, &dir.path().join("attachments.db")).unwrap();
    conn.execute_batch(INSTITUTION).unwrap();
    assert_eq!(log_rows(&conn).len(), 1);
}

#[test]
fn cascade_and_set_null_fire_the_child_triggers() {
    let conn = with_log();
    conn.execute_batch(
        "INSERT INTO bank_accounts (id, name) VALUES ('a1', 'Acc'); \
         INSERT INTO csv_import_batches (id, bank_account_id, file_name) VALUES ('c1', 'a1', 'f.csv'); \
         INSERT INTO spending_plans (id, name, year, amount, currency, icon, color) \
           VALUES ('p1', 'Plan', 2026, '100', 'CZK', 'i', 'c'); \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, import_batch_id) \
           VALUES ('t1', 'a1', 'debit', '1', 'CZK', 0, 'c1'); \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date, spending_plan_id) \
           VALUES ('t2', 'a1', 'debit', '1', 'CZK', 0, 'p1'); \
         CREATE TEMP TRIGGER td AFTER DELETE ON main.bank_transactions \
           BEGIN INSERT INTO probe_log VALUES ('bank_transactions','del',OLD.id); END; \
         CREATE TEMP TRIGGER tu AFTER UPDATE ON main.bank_transactions \
           BEGIN INSERT INTO probe_log VALUES ('bank_transactions','upd', \
             CASE WHEN NEW.spending_plan_id IS NULL THEN NEW.id || ':null' ELSE NEW.id END); END;",
    )
    .unwrap();
    conn.execute_batch("DELETE FROM csv_import_batches WHERE id = 'c1'")
        .unwrap();
    assert_eq!(
        log_rows(&conn),
        vec![("bank_transactions".into(), "del".into(), Some("t1".into()))]
    );
    conn.execute_batch("DELETE FROM spending_plans WHERE id = 'p1'")
        .unwrap();
    let rows = log_rows(&conn);
    assert_eq!(
        rows[1],
        (
            "bank_transactions".into(),
            "upd".into(),
            Some("t2:null".into())
        )
    );
}

#[test]
fn on_conflict_do_update_fires_the_update_trigger_and_not_the_insert_trigger() {
    let conn = with_log();
    conn.execute_batch(
        "CREATE TEMP TRIGGER ti AFTER INSERT ON main.app_config \
           BEGIN INSERT INTO probe_log VALUES ('app_config','ins',NEW.key); END; \
         CREATE TEMP TRIGGER tu AFTER UPDATE ON main.app_config \
           BEGIN INSERT INTO probe_log VALUES ('app_config','upd',NEW.key); END;",
    )
    .unwrap();
    let upsert = "INSERT INTO app_config (key, value) VALUES ('k', 'v') \
                  ON CONFLICT(key) DO UPDATE SET value = excluded.value";
    conn.execute_batch(upsert).unwrap();
    conn.execute_batch(upsert).unwrap();
    let ops: Vec<String> = log_rows(&conn).into_iter().map(|r| r.1).collect();
    assert_eq!(ops, vec!["ins", "upd"]);
}

#[test]
fn insert_or_replace_deletes_without_the_delete_trigger() {
    let conn = with_log();
    conn.execute_batch(
        "INSERT INTO app_config (key, value) VALUES ('k', 'v'); \
         CREATE TEMP TRIGGER td AFTER DELETE ON main.app_config \
           BEGIN INSERT INTO probe_log VALUES ('app_config','del',OLD.key); END;",
    )
    .unwrap();
    assert_eq!(count(&conn, "PRAGMA recursive_triggers"), 0);
    conn.execute_batch("INSERT OR REPLACE INTO app_config (key, value) VALUES ('k', 'w')")
        .unwrap();
    assert!(log_rows(&conn).iter().all(|r| r.1 != "del"));
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM app_config WHERE value = 'w'"),
        1
    );
}

#[test]
fn a_trigger_body_can_select_a_connection_function_and_later_statements_see_its_effect() {
    let conn = with_log();
    let tick = Arc::new(AtomicI64::new(0));
    let writer = Arc::clone(&tick);
    conn.create_scalar_function("probe_tick", 0, FunctionFlags::SQLITE_UTF8, move |_| {
        Ok(writer.fetch_add(1, Ordering::SeqCst) + 1)
    })
    .unwrap();
    let reader = Arc::clone(&tick);
    conn.create_scalar_function("probe_now", 0, FunctionFlags::SQLITE_UTF8, move |_| {
        Ok(reader.load(Ordering::SeqCst))
    })
    .unwrap();
    conn.execute_batch(
        "CREATE TEMP TRIGGER t AFTER INSERT ON main.institutions \
           BEGIN SELECT probe_tick(); INSERT INTO probe_log VALUES ('x','tick', probe_now()); END;",
    )
    .unwrap();
    conn.execute_batch(INSTITUTION).unwrap();
    let rows = log_rows(&conn);
    assert_eq!(rows[0].2.as_deref(), Some("1"));
    assert_eq!(tick.load(Ordering::SeqCst), 1);
}

#[test]
fn a_trigger_when_clause_reads_a_temp_table() {
    let conn = with_log();
    conn.execute_batch(
        "CREATE TEMP TABLE sync_mode(mode TEXT); \
         INSERT INTO sync_mode VALUES ('remote'); \
         CREATE TEMP TRIGGER t AFTER INSERT ON main.institutions \
           WHEN (SELECT mode FROM sync_mode) = 'local' \
           BEGIN INSERT INTO probe_log VALUES ('institutions','ins',NEW.id); END;",
    )
    .unwrap();
    conn.execute_batch("INSERT INTO institutions (id, name) VALUES ('i1', 'a')")
        .unwrap();
    assert_eq!(log_rows(&conn).len(), 0);
    conn.execute_batch("UPDATE sync_mode SET mode = 'local'")
        .unwrap();
    conn.execute_batch("INSERT INTO institutions (id, name) VALUES ('i2', 'b')")
        .unwrap();
    let rows = log_rows(&conn);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].2.as_deref(), Some("i2"));
}

#[test]
fn defer_foreign_keys_allows_a_child_before_its_parent_inside_one_transaction() {
    let conn = with_log();
    conn.execute_batch(
        "BEGIN; PRAGMA defer_foreign_keys = ON; \
         INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date) \
           VALUES ('t1', 'a1', 'debit', '1', 'CZK', 0); \
         INSERT INTO bank_accounts (id, name) VALUES ('a1', 'Acc'); \
         COMMIT;",
    )
    .unwrap();
    assert_eq!(count(&conn, "SELECT COUNT(*) FROM bank_transactions"), 1);
    // Control: without the deferral the same order fails.
    let err = conn.execute_batch(
        "INSERT INTO bank_transactions (id, bank_account_id, tx_type, amount, currency, booking_date) \
           VALUES ('t2', 'nope', 'debit', '1', 'CZK', 0)",
    );
    assert!(err.is_err());
}

fn rust_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// The file without its test code: every `#[cfg(test)]` followed by `mod name { ... }` (brace
/// matched) or `mod name;` is removed, and everything else is kept, so production code that sits
/// after a test module (the migration SQL constants) is still scanned.
fn production_text(text: &str) -> String {
    let marker = regex::Regex::new(r"#\[cfg\(test\)\]\s*mod\s+\w+\s*([;{])").unwrap();
    let mut out = String::new();
    let mut rest = text;
    while let Some(caps) = marker.captures(rest) {
        let whole = caps.get(0).unwrap();
        out.push_str(&rest[..whole.start()]);
        rest = &rest[whole.end()..];
        if &caps[1] == "{" {
            let mut depth = 1usize;
            let mut end = rest.len();
            for (i, c) in rest.char_indices() {
                match c {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = i + 1;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            rest = &rest[end..];
        }
    }
    out.push_str(rest);
    out
}

#[test]
fn the_lint_strips_test_modules_but_keeps_later_production_code() {
    let text = "const A: &str = \"x\";\n#[cfg(test)]\nmod dry_run;\n#[cfg(test)]\nmod t { fn f() { \"REPLACE INTO a\"; } }\nconst B: &str = \"INSERT OR REPLACE INTO b\";";
    let out = production_text(text);
    assert!(out.contains("const A"));
    assert!(out.contains("INSERT OR REPLACE INTO b"));
    assert!(!out.contains("REPLACE INTO a"));
}

#[test]
fn no_source_file_uses_insert_or_replace() {
    let re = regex::Regex::new(r"(?is)\b(INSERT\s+OR\s+REPLACE|REPLACE\s+INTO)\b").unwrap();
    let mut files = Vec::new();
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    rust_files(&src, &mut files);
    let mut hits = Vec::new();
    for file in files {
        // This file spells the forbidden statement out on purpose; `src/bin` holds standalone dev
        // tools (the demo seeder) that never run inside the app's sync-captured connection.
        let rel = file.strip_prefix(&src).unwrap();
        if rel.starts_with("bin") || rel.ends_with("services/sync/probe.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        let mut production = production_text(&text);
        if rel.ends_with("db/migrations.rs") {
            // Migration 011 (applied, append-only) holds the one legacy REPLACE. Migrations run
            // before the capture triggers exist, so it is harmless; any other use still fails.
            production = production.replacen(
                "INSERT OR REPLACE INTO app_config (key, value) VALUES ('fx_reference', 'EUR');",
                "",
                1,
            );
        }
        if re.is_match(&production) {
            hits.push(file.display().to_string());
        }
    }
    assert!(hits.is_empty(), "INSERT OR REPLACE found in: {hits:?}");
}
