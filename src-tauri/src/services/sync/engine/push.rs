//! Push (spec §9.4): this device's changes leave in two steps that never share a lock with the
//! network. First, in one transaction, the outbox becomes sealed envelopes in `sync_unsent`, each
//! recorded in the own log as the chain moves on, and the outbox rows they consumed disappear
//! (ruling R11). Then the waiting envelopes go to the relay in sequence order, in batches within
//! its limits, and each batch the relay answered is deleted. A crash anywhere pushes the same
//! envelopes again: the op ID makes that a no-op on the relay, and this device's chain never
//! forks. A push whose answer was lost is acknowledged by the next pull (`OwnAcknowledged`).

use std::collections::BTreeSet;

use rusqlite::{Connection, Transaction};

use super::ConnAccess;
use crate::error::{AppError, Result};
use crate::services::sync::changeset::to_json;
use crate::services::sync::envelope::{envelope_hash, op_id, seal_op};
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::in_savepoint;
use crate::services::sync::journal::{build_marker, build_next, record_sealed, Built, Refused};
use crate::services::sync::keys::Credentials;
use crate::services::sync::relay::{OutgoingOp, Relay, RelayError};
use crate::services::sync::state;

/// Envelopes sealed per `seal_batch` transaction.
pub const SEAL_BATCH: usize = 64;
/// Ops per push (the relay accepts 500).
pub const PUSH_MAX_OPS: usize = 100;
/// Base64 bytes of the envelopes in one push (the relay accepts a 4 MiB body).
pub const PUSH_MAX_B64: usize = 3 * 1024 * 1024;

/// What one `seal_batch` did.
#[derive(Debug, Default)]
pub struct SealReport {
    /// Envelopes with rows sealed into `sync_unsent`.
    pub sealed: usize,
    /// A barrier marker (an envelope with no rows) was sealed instead.
    pub marker: bool,
    /// Rows too large to send, once each; they stay in the outbox.
    pub refused: Vec<Refused>,
}

/// Seals up to `SEAL_BATCH` envelopes from the outbox into `sync_unsent`, in one transaction, and
/// only when nothing sealed earlier is still waiting (the chain stays in the order it is pushed).
///
/// Any own envelope carries this device's barrier, so sealing one marks the barrier
/// (`note_sealed_at_barrier`). With nothing to send, a device past a barrier whose marker it has
/// not met (`barrier_seq` unknown) seals one marker for it, at most one per barrier (ruling P5).
pub fn seal_batch(conn: &Connection, creds: &Credentials) -> Result<SealReport> {
    let mut report = SealReport::default();
    let waiting: bool = conn.query_row("SELECT EXISTS (SELECT 1 FROM sync_unsent)", [], |r| {
        r.get(0)
    })?;
    if waiting {
        return Ok(report);
    }
    let tx = conn.unchecked_transaction()?;
    let sync = state::load(&tx)?.ok_or_else(not_set_up)?;
    for _ in 0..SEAL_BATCH {
        let (built, refused) = build_next(&tx)?;
        for row in refused {
            if !report
                .refused
                .iter()
                .any(|r| r.table == row.table && r.pk == row.pk)
            {
                report.refused.push(row);
            }
        }
        let Some(built) = built else {
            break;
        };
        seal_into_unsent(&tx, creds, &built)?;
        report.sealed += 1;
    }
    if report.sealed > 0 {
        state::note_sealed_at_barrier(&tx, sync.barrier)?;
    } else if sync.barrier > 0 && sync.barrier_seq.is_none() {
        let marks = state::marks(&tx)?.ok_or_else(not_set_up)?;
        if marks.marker_barrier < sync.barrier {
            seal_into_unsent(&tx, creds, &build_marker(&tx)?)?;
            state::note_sealed_at_barrier(&tx, sync.barrier)?;
            report.marker = true;
        }
    }
    tx.commit()?;
    Ok(report)
}

/// Seals `built` under the vault's current key, keeps it in `sync_unsent` under its sequence and
/// records it in the own log, moving the chain on.
fn seal_into_unsent(tx: &Transaction<'_>, creds: &Credentials, built: &Built) -> Result<()> {
    let json = to_json(&built.changeset)?;
    let envelope = seal_op(
        creds.keys.current_epoch(),
        creds.keys.current(),
        &creds.vault_id.0,
        &json,
    )?;
    let seq = i64::try_from(built.changeset.seq)
        .map_err(|_| AppError::Internal("sync push: sequence out of range".into()))?;
    tx.execute(
        "INSERT INTO sync_unsent (seq, envelope) VALUES (?1, ?2)",
        rusqlite::params![seq, envelope],
    )?;
    record_sealed(tx, built, envelope_hash(&envelope))
}

/// The next push: the waiting envelopes in sequence order, within the relay limits.
pub fn next_push_batch(conn: &Connection) -> Result<Vec<(u64, OutgoingOp)>> {
    next_push_batch_with(conn, PUSH_MAX_OPS, PUSH_MAX_B64)
}

/// The waiting envelopes from the lowest sequence on, as many as fit `max_ops` and `max_b64`
/// base64 bytes. The first one always goes, so an envelope over `max_b64` alone (never one the
/// journal builds) cannot hold the rest back forever.
pub fn next_push_batch_with(
    conn: &Connection,
    max_ops: usize,
    max_b64: usize,
) -> Result<Vec<(u64, OutgoingOp)>> {
    let mut stmt = conn.prepare_cached("SELECT seq, envelope FROM sync_unsent ORDER BY seq")?;
    let mut rows = stmt.query([])?;
    let mut batch = Vec::new();
    let mut b64 = 0usize;
    while batch.len() < max_ops {
        let Some(row) = rows.next()? else {
            break;
        };
        let seq: i64 = row.get(0)?;
        let ct: Vec<u8> = row.get(1)?;
        let size = ct.len().div_ceil(3) * 4;
        if !batch.is_empty() && b64.saturating_add(size) > max_b64 {
            break;
        }
        b64 += size;
        let seq = u64::try_from(seq)
            .map_err(|_| AppError::Internal("sync push: a negative unsent sequence".into()))?;
        batch.push((seq, OutgoingOp { id: op_id(&ct), ct }));
    }
    Ok(batch)
}

/// The relay holds these envelopes: they stop waiting.
pub fn acknowledge_pushed(conn: &Connection, seqs: &[u64]) -> Result<()> {
    in_savepoint(conn, "sync_acknowledge_pushed", || {
        let mut delete = conn.prepare_cached("DELETE FROM sync_unsent WHERE seq = ?1")?;
        for seq in seqs {
            let seq = i64::try_from(*seq)
                .map_err(|_| AppError::Internal("sync push: sequence out of range".into()))?;
            delete.execute([seq])?;
        }
        Ok(())
    })
}

/// What one `push_all` did.
#[derive(Debug, Default)]
pub struct PushReport {
    /// Envelopes the relay acknowledged (markers included).
    pub pushed: usize,
    /// Rows too large to send, each counted once; they stay in the outbox.
    pub refused: usize,
}

/// Seals and pushes until nothing is left to send: seal (locked) → next batch (locked) → push
/// (unlocked) → acknowledge (locked). A refused row stays in the outbox and never keeps the loop
/// going. The first error ends it, returned as its `SyncError` class (`402` → `Lapsed`, `413` →
/// `TooLarge(Quota)`, `403` → `ReadOnlyToken`, …); sealed envelopes keep waiting in `sync_unsent`
/// for the next round. A receipt that accepts more ops than were sent is outside the relay
/// contract: nothing is acknowledged on its word.
pub async fn push_all<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
) -> SyncResult<PushReport> {
    let auth = creds.auth();
    let mut report = PushReport::default();
    // (table, pk) of each refused row, logged once.
    let mut refused: BTreeSet<(String, String)> = BTreeSet::new();
    loop {
        let sealed = db.with_conn(|conn| seal_batch(conn, creds))?;
        for row in sealed.refused {
            if refused.insert((row.table.clone(), row.pk)) {
                log::warn!(
                    "[SYNC] A {} row of {} bytes is too large to send; it stays queued",
                    row.table,
                    row.bytes
                );
            }
        }
        let batch = db.with_conn(next_push_batch)?;
        if batch.is_empty() {
            break;
        }
        let (seqs, ops): (Vec<u64>, Vec<OutgoingOp>) = batch.into_iter().unzip();
        let receipt = relay.push(&auth, &ops).await?;
        if receipt.accepted as usize > ops.len() {
            log::warn!(
                "[SYNC] The relay accepted {} ops of a push of {}",
                receipt.accepted,
                ops.len()
            );
            return Err(SyncError::from(RelayError::Protocol(
                "push accepted more ops than sent".into(),
            )));
        }
        db.with_conn(|conn| acknowledge_pushed(conn, &seqs))?;
        report.pushed += ops.len();
    }
    report.refused = refused.len();
    Ok(report)
}

fn not_set_up() -> AppError {
    AppError::Internal("sync push: sync is not set up".into())
}
