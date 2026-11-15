//! This device's documents and photos across a replace (ADR 0030). The attachment tables are
//! Synced: the clear empties them with the rest, and the refill brings back the vault's rows.
//! Content this device holds under a blob ID a vault row names stays (the repair's sweep keeps
//! every referenced blob), so nothing is downloaded again; this device's rows the vault lacks,
//! legacy rows without a blob included, are gone with their blobs and remain only in the backup.
//!
//! Nothing is set aside or put back here: `mine` notes this device's rows before the clear and
//! `carry` counts which of them are there again after the refill.

use rusqlite::{Connection, OptionalExtension};

use super::{quoted, AttachmentCarry};
use crate::error::{AppError, Result};

/// The tables whose rows `AttachmentCarry` counts (photo batches are not).
const COUNTED: [&str; 3] = [
    "real_estate_photos",
    "real_estate_documents",
    "insurance_documents",
];

/// This device's document and photo rows before a replace, `(table, id)`, legacy rows included.
pub(super) struct Mine {
    rows: Vec<(&'static str, String)>,
}

/// Notes every document and photo row this device has, before the clear.
pub(super) fn mine(conn: &Connection) -> Result<Mine> {
    let mut rows = Vec::new();
    for table in COUNTED {
        let mut stmt = conn.prepare(&format!(
            "SELECT id FROM main.{} ORDER BY id",
            quoted(table)
        ))?;
        let ids = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        rows.extend(ids.into_iter().map(|id| (table, id)));
    }
    Ok(Mine { rows })
}

/// After the refill: the rows of `mine` that are there again (`kept`, the vault has them) and
/// the rest (`orphaned`).
pub(super) fn carry(conn: &Connection, mine: Mine) -> Result<AttachmentCarry> {
    let mut carry = AttachmentCarry::default();
    for (table, id) in &mine.rows {
        let present = conn
            .prepare_cached(&format!(
                "SELECT 1 FROM main.{} WHERE id = ?1",
                quoted(table)
            ))?
            .query_row([id], |_| Ok(()))
            .optional()?
            .is_some();
        let count = if present {
            &mut carry.kept
        } else {
            &mut carry.orphaned
        };
        *count = count.checked_add(1).ok_or_else(too_many)?;
    }
    if carry.orphaned > 0 {
        log::info!(
            "[SYNC] A replace kept {} documents and photos; {} were not in the vault and stay only in the backup",
            carry.kept,
            carry.orphaned
        );
    }
    Ok(carry)
}

fn too_many() -> AppError {
    AppError::Internal("sync snapshot: too many documents and photos to count".into())
}
