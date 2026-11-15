//! Blob queue tests: an in-memory profile with its attachment store attached (as the app opens
//! one) and sync on, where documents and photos name blobs that are held here or not.

mod pass;
mod queue;
mod stream;
mod transfer;

use rusqlite::Connection;

use crate::db::files_schema;
use crate::services::attachment_store;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::state;
use crate::services::sync::test_support::migrated;

const DEVICE: DeviceId = DeviceId([7; 16]);
const NOW: i64 = 1_772_323_200; // 2026-03-01

fn exec(conn: &Connection, sql: &str) {
    conn.execute_batch(sql).expect(sql);
}

/// A sync-enabled profile with an attachment store, outbox empty, one policy `p1` and one
/// property `r1`.
fn profile() -> Connection {
    let conn = migrated();
    conn.execute("ATTACH DATABASE ':memory:' AS files", [])
        .expect("attach");
    files_schema::migrate(&conn).expect("files schema");
    state::enable(&conn, DEVICE).expect("enable");
    exec(
        &conn,
        "INSERT INTO insurance_policies \
           (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
         VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly'); \
         INSERT INTO real_estate (id, name, address, type) VALUES ('r1', 'Flat', 'Street', 'flat'); \
         DELETE FROM sync_outbox;",
    );
    conn
}

/// A new blob held here; returns its ID.
fn held(conn: &Connection, bytes: &[u8]) -> String {
    attachment_store::put_bytes(conn, bytes).expect("blob").id
}

/// A blob ID no blob here has (content that lives on another device).
fn elsewhere(n: u8) -> String {
    format!("0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f{n:02x}")
}

/// A policy document `id` naming `blob`, uploaded at `at`.
fn document(conn: &Connection, id: &str, blob: &str, at: i64) {
    conn.execute(
        "INSERT INTO insurance_documents (id, insurance_id, name, blob_id, uploaded_at) \
         VALUES (?1, 'p1', 'Contract', ?2, ?3)",
        rusqlite::params![id, blob, at],
    )
    .expect("document");
}

/// A photo `id` of property `r1` naming `full` and `thumb`, created at `at`.
fn photo(conn: &Connection, id: &str, full: &str, thumb: &str, at: i64) {
    conn.execute(
        "INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
         VALUES ('batch-1', 'r1', 1767225600) ON CONFLICT(id) DO NOTHING",
        [],
    )
    .expect("batch");
    conn.execute(
        "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id, created_at) \
         VALUES (?1, 'batch-1', ?2, ?3, ?4)",
        rusqlite::params![id, full, thumb, at],
    )
    .expect("photo");
}

/// The queue as `blob_id state thumbnail wanted row_at queued_at attempts last_error`, by ID.
fn queue_rows(conn: &Connection) -> Vec<String> {
    let mut stmt = conn
        .prepare(
            "SELECT blob_id || ' ' || state || ' ' || thumbnail || ' ' || wanted || ' ' || row_at \
               || ' ' || queued_at || ' ' || attempts || ' ' || COALESCE(last_error, '-') \
             FROM sync_blobs ORDER BY blob_id",
        )
        .expect("prepare");
    stmt.query_map([], |r| r.get(0))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
}

fn state_of(conn: &Connection, blob: &str) -> Option<String> {
    conn.query_row(
        "SELECT state FROM sync_blobs WHERE blob_id = ?1",
        [blob],
        |r| r.get(0),
    )
    .ok()
}
