//! This device's documents and photos across a replace (audit H3). The attachment tables are
//! Local: no snapshot carries them, and no other device has their blobs. Emptying the synced
//! tables would cascade them away with their real estate and policies, and the repair after the
//! restore (`attachment_store::delete_unreferenced`) would purge their blobs.
//!
//! So a replace keeps them aside first (`keep`: copied into `TEMP` tables, then taken out of
//! `main`) and, once the snapshot's rows are in, puts back every row whose parent the vault has,
//! under its own ID (`reattach`), before the repair runs: only the blobs of rows whose item the
//! vault lacks are purged. The `TEMP` copies live in memory (the bundled SQLCipher is built with
//! `SQLITE_TEMP_STORE=2`) and are dropped in every case.

use rusqlite::Connection;

use super::{quoted, AttachmentCarry};
use crate::error::{AppError, Result};
use crate::services::sync::registry::schema;

/// The attachment tables, parents first, with the column naming each row's parent and the
/// parent's table, and whether its rows are counted in `AttachmentCarry` (batches are not).
const TABLES: [(&str, &str, &str, bool); 4] = [
    (
        "real_estate_photo_batches",
        "real_estate_id",
        "real_estate",
        false,
    ),
    (
        "real_estate_photos",
        "batch_id",
        "real_estate_photo_batches",
        true,
    ),
    (
        "real_estate_documents",
        "real_estate_id",
        "real_estate",
        true,
    ),
    (
        "insurance_documents",
        "insurance_id",
        "insurance_policies",
        true,
    ),
];

/// The Local attachment rows hanging off Synced parents, copied into TEMP tables
/// (`temp.sync_kept_<table>`) for real_estate_photo_batches, real_estate_photos,
/// real_estate_documents and insurance_documents.
pub(super) struct Kept {
    /// Rows kept per table, in the order of `TABLES`.
    rows: [u32; TABLES.len()],
}

/// Copies every attachment row into its `TEMP` table and deletes it from `main` (children
/// first), so the clear that follows finds none to cascade. Leftover copies are dropped first;
/// on a failure the copies made so far are dropped again.
pub(super) fn keep(conn: &Connection) -> Result<Kept> {
    drop_kept(conn)?;
    let kept = (|| {
        let mut rows = [0; TABLES.len()];
        for (i, (table, ..)) in TABLES.iter().enumerate() {
            conn.execute_batch(&format!(
                "CREATE TEMP TABLE {} AS SELECT * FROM main.{}",
                kept_table(table),
                quoted(table)
            ))?;
            rows[i] = row_count(conn, &format!("temp.{}", kept_table(table)))?;
        }
        for (table, ..) in TABLES.iter().rev() {
            conn.execute(&format!("DELETE FROM main.{}", quoted(table)), [])?;
        }
        Ok(Kept { rows })
    })();
    if kept.is_err() {
        discard(conn);
    }
    kept
}

/// Re-inserts, parents first, every kept row whose parent exists after the restore (same ID);
/// drops the TEMP tables in every case.
pub(super) fn reattach(conn: &Connection, kept: Kept) -> Result<AttachmentCarry> {
    let carried = put_back(conn, &kept);
    let dropped = drop_kept(conn);
    let carry = carried?;
    dropped?;
    Ok(carry)
}

/// Drops the `TEMP` copies on a path that already fails: a failure here is only logged.
pub(super) fn discard(conn: &Connection) {
    if let Err(error) = drop_kept(conn) {
        log::warn!("[SYNC] Could not drop the kept attachment rows: {error}");
    }
}

fn put_back(conn: &Connection, kept: &Kept) -> Result<AttachmentCarry> {
    let mut carry = AttachmentCarry::default();
    for (i, (table, parent_column, parent, counted)) in TABLES.iter().enumerate() {
        let columns = schema::columns(conn, table)?
            .into_iter()
            .map(|c| quoted(&c.name))
            .collect::<Vec<_>>()
            .join(", ");
        let back = conn.execute(
            &format!(
                "INSERT INTO main.{t} ({columns}) SELECT {columns} FROM temp.{k} \
                 WHERE {p} IN (SELECT id FROM main.{parent})",
                t = quoted(table),
                k = kept_table(table),
                p = quoted(parent_column),
                parent = quoted(parent),
            ),
            [],
        )?;
        let back = u32::try_from(back).map_err(|_| too_many())?;
        if *counted {
            carry.kept += back;
            carry.orphaned += kept.rows[i].checked_sub(back).ok_or_else(too_many)?;
        }
    }
    if carry.orphaned > 0 {
        log::info!(
            "[SYNC] A replace kept {} documents and photos; {} belonged to items the vault does not have",
            carry.kept,
            carry.orphaned
        );
    }
    Ok(carry)
}

fn drop_kept(conn: &Connection) -> Result<()> {
    for (table, ..) in TABLES {
        conn.execute_batch(&format!("DROP TABLE IF EXISTS temp.{}", kept_table(table)))?;
    }
    Ok(())
}

fn kept_table(table: &str) -> String {
    quoted(&format!("sync_kept_{table}"))
}

fn row_count(conn: &Connection, table: &str) -> Result<u32> {
    let n: i64 = conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
    u32::try_from(n).map_err(|_| too_many())
}

fn too_many() -> AppError {
    AppError::Internal("sync snapshot: the kept attachment rows do not add up".into())
}
