//! The journal (spec §6.4, §9.1): turns the outbox into plaintext changesets.
//!
//! The outbox holds keys, not values (D11), so every change is the full current row (R1): five
//! edits of a note send one value, and a row changed and then deleted sends only its tombstone.
//! Live rows go out parents first (FK depth, then first `queued_at`); tombstone-only rows follow,
//! children first, so a child's change never travels after its parent's tombstone.
//!
//! One envelope per `build_next` call, inside the caller's transaction (R11): the caller builds,
//! hashes, calls `record_sealed` and commits, so the outbox rows the envelope consumed disappear
//! exactly when the envelope joins the chain. A write after the commit re-queues its row through
//! the trigger.

mod cut;
mod read;

pub use read::read_row;

use std::cmp::Reverse;

use rusqlite::{params, Connection, Transaction};

use crate::error::{AppError, Result};
use crate::services::sync::changeset::{max_hlc, Changeset};
use crate::services::sync::registry::keys::{fk_depth, schema_number};
use crate::services::sync::state;
use read::Reader;

/// Largest padded deflated envelope (60 KiB; the relay accepts 64 KiB per op).
pub const ENVELOPE_BUDGET: usize = 61_440;
/// Largest padded deflated single row (180 KiB); a larger row is refused.
pub const SINGLE_ROW_LIMIT: usize = 184_320;

#[derive(Debug, Clone, PartialEq)]
pub struct Built {
    pub changeset: Changeset,
    pub max_hlc: u64,
}

/// A row too large to send even alone. It stays in the outbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    pub table: String,
    pub pk: String,
    /// Its padded deflated size alone in an envelope.
    pub bytes: usize,
}

/// One outbox entry.
struct Queued {
    table: String,
    pk: String,
    queued_at: i64,
}

/// The next envelope, built inside `tx`, which also loses the outbox rows the envelope consumed.
/// `None` when nothing but refused rows is left.
pub fn build_next(tx: &Transaction<'_>) -> Result<(Option<Built>, Vec<Refused>)> {
    let conn: &Connection = tx;
    let sync = state::load(conn)?
        .ok_or_else(|| AppError::Internal("sync journal: sync is not set up".into()))?;
    let reader = Reader::new(conn)?;
    let queue = ordered_queue(conn, &reader)?;
    let envelope = Changeset {
        schema: schema_number(),
        barrier: sync.barrier,
        device: sync.device,
        seq: sync.next_seq,
        prev: sync.last_hash,
        rows: Vec::new(),
    };
    let cut = cut::cut(envelope, &queue, |q| reader.read(&q.table, &q.pk))?;

    let mut delete = conn.prepare_cached("DELETE FROM sync_outbox WHERE tbl = ?1 AND pk = ?2")?;
    for (table, pk) in &cut.consumed {
        delete.execute([table, pk])?;
    }
    let built = (!cut.changeset.rows.is_empty()).then(|| Built {
        max_hlc: max_hlc(&cut.changeset),
        changeset: cut.changeset,
    });
    Ok((built, cut.refused))
}

/// Records the sealed envelope in the same transaction: `sync_own_log` gains `(seq, hash,
/// max_hlc)`, and the chain moves on (`next_seq = seq + 1`, `last_hash = hash`).
pub fn record_sealed(tx: &Transaction<'_>, built: &Built, hash: [u8; 32]) -> Result<()> {
    let seq = i64::try_from(built.changeset.seq)
        .map_err(|_| AppError::Internal("sync journal: sequence out of range".into()))?;
    let max_hlc = i64::try_from(built.max_hlc)
        .map_err(|_| AppError::Internal("sync journal: clock out of range".into()))?;
    tx.execute(
        "INSERT INTO sync_own_log (seq, hash, max_hlc) VALUES (?1, ?2, ?3)",
        params![seq, hash.to_vec(), max_hlc],
    )?;
    let advanced = tx.execute(
        "UPDATE sync_state SET next_seq = ?1 + 1, last_hash = ?2 WHERE id = 1 AND next_seq = ?1",
        params![seq, hash.to_vec()],
    )?;
    if advanced != 1 {
        return Err(AppError::Internal(format!(
            "sync journal: envelope {seq} is not the next in this device's chain"
        )));
    }
    Ok(())
}

/// The outbox in sending order: live rows by `(fk_depth, category depth, queued_at, tbl, pk)`,
/// then tombstone-only keys by `(fk_depth DESC, queued_at, tbl, pk)`.
fn ordered_queue(conn: &Connection, reader: &Reader<'_>) -> Result<Vec<Queued>> {
    let depth = fk_depth(conn)?;
    let depth_of = |table: &str| depth.get(table).copied().unwrap_or(0);
    let queued: Vec<Queued> = conn
        .prepare("SELECT tbl, pk, queued_at FROM sync_outbox")?
        .query_map([], |r| {
            Ok(Queued {
                table: r.get(0)?,
                pk: r.get(1)?,
                queued_at: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<_, _>>()?;

    let mut live = Vec::new();
    let mut gone = Vec::new();
    for q in queued {
        if reader.exists(&q.table, &q.pk)? {
            // Categories reference categories: within their FK depth, a parent goes first.
            let within = if q.table == "transaction_categories" {
                category_depth(conn, &q.pk)?
            } else {
                0
            };
            live.push(((depth_of(&q.table), within), q));
        } else {
            gone.push((Reverse(depth_of(&q.table)), q));
        }
    }
    live.sort_by(|(a, qa), (b, qb)| {
        (a, qa.queued_at, &qa.table, &qa.pk).cmp(&(b, qb.queued_at, &qb.table, &qb.pk))
    });
    gone.sort_by(|(a, qa), (b, qb)| {
        (a, qa.queued_at, &qa.table, &qa.pk).cmp(&(b, qb.queued_at, &qb.table, &qb.pk))
    });
    Ok(live
        .into_iter()
        .map(|(_, q)| q)
        .chain(gone.into_iter().map(|(_, q)| q))
        .collect())
}

/// Number of ancestors of a live category (a cycle stops at 32).
fn category_depth(conn: &Connection, id: &str) -> Result<u32> {
    Ok(conn
        .prepare_cached(
            "WITH RECURSIVE up(id, parent_id, n) AS ( \
           SELECT id, parent_id, 0 FROM transaction_categories WHERE id = ?1 \
           UNION ALL \
           SELECT c.id, c.parent_id, up.n + 1 FROM transaction_categories c \
             JOIN up ON c.id = up.parent_id WHERE up.n < 32) \
         SELECT COALESCE(MAX(n), 0) FROM up",
        )?
        .query_row([id], |r| r.get(0))?)
}

#[cfg(test)]
mod tests;
