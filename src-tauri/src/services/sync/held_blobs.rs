//! Blobs a replace holds (ADR 0030, audit H3). A join, repair or re-sync makes the attachment rows
//! the vault's, but a row this device pushed after the vault's newest snapshot (the re-sync pushes
//! just before it replaces) is not in that snapshot: the replace drops it and the next pull brings
//! it back. Its content must still be here then, and it may be the only copy (an upload that had
//! not finished, a server that lost it).
//!
//! So the replace holds every blob this device's rows named before it (`hold_named`, inside the
//! replace's savepoint), and the blob sweep (`attachment_store::delete_unreferenced`, through
//! `attachment_store::HOLDS`) keeps held blobs whoever runs it: the repair after each merged
//! changeset, the sweep after the next unlock, the maintenance command. The table is in the
//! profile, so a hold survives an app restart. The first pull after the replace that reaches the
//! sync server's end with nothing stopped or held releases every hold and sweeps (`release`):
//! only then do the blobs no row names go.

use rusqlite::{params, Connection};

use super::in_savepoint;
use crate::db::files_schema;
use crate::error::Result;
use crate::services::attachment_store::{self, REFERENCES};

/// Holds every blob this device's attachment rows name now (an earlier hold stays); returns how
/// many rows `sync_held_blobs` gained.
pub fn hold_named(conn: &Connection, now: i64) -> Result<usize> {
    let named = REFERENCES
        .iter()
        .map(|(table, column)| {
            format!("SELECT {column} AS b FROM main.{table} WHERE {column} IS NOT NULL")
        })
        .collect::<Vec<_>>()
        .join(" UNION ");
    Ok(conn.execute(
        &format!(
            "INSERT INTO sync_held_blobs (blob_id, held_at) SELECT b, ?1 FROM ({named}) \
             WHERE 1 ON CONFLICT(blob_id) DO NOTHING"
        ),
        params![now],
    )?)
}

/// Releases every hold, after a pull that caught up, and sweeps the blobs no row names (when the
/// attachment store is open; otherwise the next sweep takes them). Returns the holds released.
pub fn release(conn: &Connection) -> Result<usize> {
    in_savepoint(conn, "sync_release_held_blobs", || {
        let released = conn.execute("DELETE FROM sync_held_blobs", [])?;
        if released > 0 && files_schema::is_attached(conn)? {
            let swept = attachment_store::delete_unreferenced(conn)?;
            log::info!(
                "[SYNC] The pull caught up after a replace: {released} held blobs released, {swept} no longer named"
            );
        }
        Ok(released)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::test_support::migrated;

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).expect(sql)
    }

    #[test]
    fn a_hold_names_every_blob_of_every_attachment_row_once() {
        let conn = migrated();
        conn.execute_batch(
            "INSERT INTO insurance_policies \
               (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
             VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly'); \
             INSERT INTO real_estate (id, name, address, type) VALUES ('r1', 'Flat', 'Street', 'flat'); \
             INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
               VALUES ('pb1', 'r1', 1767225600); \
             INSERT INTO insurance_documents (id, insurance_id, name, blob_id) \
               VALUES ('d1', 'p1', 'Contract', 'b-doc'); \
             INSERT INTO insurance_documents (id, insurance_id, name, file_path) \
               VALUES ('old', 'p1', 'Legacy', 'insurance/p1/old.pdf'); \
             INSERT INTO real_estate_documents (id, real_estate_id, name, blob_id) \
               VALUES ('rd1', 'r1', 'Deed', 'b-deed'); \
             INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id) \
               VALUES ('ph1', 'pb1', 'b-full', 'b-thumb');",
        )
        .expect("rows");

        assert_eq!(hold_named(&conn, 7).expect("hold"), 4);
        assert_eq!(hold_named(&conn, 8).expect("hold again"), 0);

        let held: Vec<(String, i64)> = conn
            .prepare("SELECT blob_id, held_at FROM sync_held_blobs ORDER BY blob_id")
            .expect("prepare")
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .expect("query")
            .map(|r| r.expect("row"))
            .collect();
        assert_eq!(
            held,
            vec![
                ("b-deed".to_string(), 7),
                ("b-doc".to_string(), 7),
                ("b-full".to_string(), 7),
                ("b-thumb".to_string(), 7),
            ]
        );
        assert_eq!(release(&conn).expect("release"), 4);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_held_blobs"), 0);
    }
}
