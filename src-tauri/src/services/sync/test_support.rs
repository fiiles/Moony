//! Shared helpers for the sync engine tests.

use rusqlite::Connection;

/// In-memory connection with foreign keys on and the real migrations applied.
pub(crate) fn migrated() -> Connection {
    let conn = Connection::open_in_memory().expect("in-memory db");
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .expect("foreign keys");
    crate::db::run_migrations(&conn).expect("migrations");
    conn
}
