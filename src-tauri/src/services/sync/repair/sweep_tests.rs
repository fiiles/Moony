//! The unlock sweep: only rows a newly applied migration seeded can be revived seeds.

use std::collections::BTreeSet;

use rusqlite::Connection;

use super::*;
use crate::services::sync::capture;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::state;
use crate::services::sync::test_support::migrated;

const DEVICE: DeviceId = DeviceId([7; 16]);

fn exec(conn: &Connection, sql: &str) {
    conn.execute_batch(sql).expect(sql);
}

fn count(conn: &Connection, sql: &str) -> i64 {
    conn.query_row(sql, [], |r| r.get(0)).expect(sql)
}

fn enabled() -> Connection {
    let conn = migrated();
    state::enable(&conn, DEVICE).expect("enable");
    exec(&conn, "DELETE FROM sync_outbox");
    conn
}

/// `cat_entertainment` deleted with capture on, then put back while capture was off.
fn revive_a_seed(conn: &Connection) {
    exec(
        conn,
        "DELETE FROM transaction_categories WHERE id = 'cat_entertainment'",
    );
    capture::uninstall(conn).unwrap();
    exec(
        conn,
        "INSERT INTO transaction_categories (id, name, sort_order, is_system, kind) \
         VALUES ('cat_entertainment', 'Zábava', 1, 1, 'expense')",
    );
}

const SEED: &str = "SELECT COUNT(*) FROM transaction_categories WHERE id = 'cat_entertainment'";

#[test]
fn a_revived_seed_of_a_new_migration_still_goes() {
    let conn = enabled();
    let seed = "SELECT * FROM transaction_categories WHERE id = 'cat_entertainment'";
    let row: (String, Option<String>, Option<String>, i64, i64, String) = conn
        .query_row(
            "SELECT name, icon, color, sort_order, is_system, kind FROM transaction_categories \
             WHERE id = 'cat_entertainment'",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .expect(seed);
    exec(
        &conn,
        "DELETE FROM transaction_categories WHERE id = 'cat_entertainment'",
    );
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM sync_clock WHERE tbl = 'transaction_categories' \
             AND pk = 'cat_entertainment' AND col = '-'"
        ),
        1
    );
    capture::uninstall(&conn).unwrap();
    conn.execute(
        "INSERT INTO transaction_categories (id, name, icon, color, sort_order, is_system, kind) \
         VALUES ('cat_entertainment', ?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![row.0, row.1, row.2, row.3, row.4, row.5],
    )
    .unwrap();

    let tables = BTreeSet::from(["transaction_categories"]);
    assert_eq!(delete_revived_seeds(&conn, &tables).unwrap(), 1);
    assert_eq!(
        count(
            &conn,
            "SELECT COUNT(*) FROM transaction_categories WHERE id = 'cat_entertainment'"
        ),
        0
    );
    assert_eq!(delete_revived_seeds(&conn, &tables).unwrap(), 0);
}

#[test]
fn an_ordinary_row_revived_while_capture_was_off_survives_the_unlock() {
    let mut conn = enabled();
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
         DELETE FROM institutions WHERE id = 'i1';",
    );
    capture::uninstall(&conn).unwrap();
    exec(
        &conn,
        "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank')",
    );
    exec(&conn, "DELETE FROM sync_outbox");

    crate::services::sync::on_open(&mut conn);
    assert!(capture::is_installed(&conn).unwrap());
    assert_eq!(
        count(&conn, "SELECT COUNT(*) FROM institutions WHERE id = 'i1'"),
        1
    );
}

#[test]
fn the_sweep_waits_while_capture_is_marked_broken() {
    let mut conn = enabled();
    revive_a_seed(&conn);
    exec(&conn, "UPDATE sync_state SET schema_seen = 1");
    state::mark_capture_broken(&conn, 1_767_225_600).unwrap();

    crate::services::sync::on_open(&mut conn);
    assert_eq!(count(&conn, SEED), 1);
    assert_eq!(state::load(&conn).unwrap().expect("row").schema_seen, 1);
}

#[test]
fn a_later_clean_unlock_sweeps_what_was_skipped() {
    let mut conn = enabled();
    revive_a_seed(&conn);
    exec(&conn, "UPDATE sync_state SET schema_seen = 1");
    state::mark_capture_broken(&conn, 1_767_225_600).unwrap();
    crate::services::sync::on_open(&mut conn);
    assert_eq!(count(&conn, SEED), 1);

    exec(&conn, "UPDATE sync_state SET capture_broken_at = NULL");
    crate::services::sync::on_open(&mut conn);
    assert_eq!(count(&conn, SEED), 0);
    assert_eq!(
        state::load(&conn).unwrap().expect("row").schema_seen,
        crate::services::sync::registry::keys::schema_number()
    );
}
