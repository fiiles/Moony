//! Push (spec §9.4): this device's changes leave in two steps that never share a lock with the
//! network. First the outbox becomes sealed envelopes in `sync_unsent`, each recorded in the own
//! log as the chain moves on in the same transaction that removes the outbox rows it consumed
//! (ruling R11); one envelope per lock hold, so a large first drain never freezes the profile.
//! Then the waiting envelopes go to the relay in sequence order, in batches within its limits, and
//! each batch the relay answered is deleted. A crash anywhere pushes the same
//! envelopes again: the op ID makes that a no-op on the relay, and this device's chain never
//! forks. A push whose answer was lost is acknowledged by the next pull (`OwnAcknowledged`).

use std::collections::BTreeSet;
use std::fmt;

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

/// Envelopes sealed per `seal_batch`.
pub const SEAL_BATCH: usize = 64;
/// Envelopes sealed per lock hold (one transaction each). Every envelope reads the whole outbox
/// (about 0.12 s for 50,000 queued rows in release), so a first drain of a large one would
/// otherwise hold the profile for seconds.
pub const SEAL_PER_HOLD: usize = 1;
/// Ops per push (the relay accepts 500).
pub const PUSH_MAX_OPS: usize = 100;
/// Base64 bytes of the envelopes in one push (the relay accepts a 4 MiB body).
pub const PUSH_MAX_B64: usize = 3 * 1024 * 1024;

/// What one `seal_batch` did. Its `Debug` shows a refused row's table and size, never its key.
#[derive(Default)]
pub struct SealReport {
    /// Envelopes with rows sealed into `sync_unsent`.
    pub sealed: usize,
    /// A barrier marker (an envelope with no rows) was sealed instead.
    pub marker: bool,
    /// Rows too large to send, once each; they stay in the outbox.
    pub refused: Vec<Refused>,
}

impl SealReport {
    /// Adds the refused rows not already listed.
    fn add_refused(&mut self, refused: Vec<Refused>) {
        for row in refused {
            if !self
                .refused
                .iter()
                .any(|r| r.table == row.table && r.pk == row.pk)
            {
                self.refused.push(row);
            }
        }
    }
}

impl fmt::Debug for SealReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let refused: Vec<(&str, usize)> = self
            .refused
            .iter()
            .map(|r| (r.table.as_str(), r.bytes))
            .collect();
        f.debug_struct("SealReport")
            .field("sealed", &self.sealed)
            .field("marker", &self.marker)
            .field("refused", &refused)
            .finish()
    }
}

/// Seals up to `SEAL_BATCH` envelopes from the outbox into `sync_unsent`, only when nothing sealed
/// earlier is still waiting (the chain stays in the order it is pushed). The lock is taken for at
/// most `SEAL_PER_HOLD` envelopes at a time, each hold one transaction, and released in between;
/// the batch ends when the outbox has nothing more to send or the target is met.
///
/// Any own envelope carries this device's barrier, so sealing one marks the barrier
/// (`note_sealed_at_barrier`). With nothing to send, a device past a barrier whose marker it has
/// not met (`barrier_seq` unknown) seals one marker for it, at most one per barrier (ruling P5).
pub fn seal_batch<D: ConnAccess>(db: &D, creds: &Credentials) -> Result<SealReport> {
    let mut report = SealReport::default();
    let mut first = true;
    while report.sealed < SEAL_BATCH {
        let max = SEAL_PER_HOLD.min(SEAL_BATCH - report.sealed);
        let hold = db.with_conn(|conn| seal_hold(conn, creds, max, first))?;
        first = false;
        report.sealed += hold.sealed;
        report.marker |= hold.marker;
        report.add_refused(hold.refused);
        if hold.sealed < max {
            break;
        }
    }
    Ok(report)
}

/// One lock hold of `seal_batch`, in one transaction: up to `max` envelopes. The `first` hold
/// seals nothing while an envelope waits, and is the one that may seal a marker.
fn seal_hold(
    conn: &Connection,
    creds: &Credentials,
    max: usize,
    first: bool,
) -> Result<SealReport> {
    let mut report = SealReport::default();
    if first {
        let waiting: bool =
            conn.query_row("SELECT EXISTS (SELECT 1 FROM sync_unsent)", [], |r| {
                r.get(0)
            })?;
        if waiting {
            return Ok(report);
        }
    }
    let tx = conn.unchecked_transaction()?;
    let sync = state::load(&tx)?.ok_or_else(not_set_up)?;
    for _ in 0..max {
        let (built, refused) = build_next(&tx)?;
        report.add_refused(refused);
        let Some(built) = built else {
            break;
        };
        seal_into_unsent(&tx, creds, &built)?;
        report.sealed += 1;
    }
    if report.sealed > 0 {
        state::note_sealed_at_barrier(&tx, sync.barrier)?;
    } else if first && sync.barrier > 0 && sync.barrier_seq.is_none() {
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

/// Seals and pushes until nothing is left to send: seal (locked one envelope at a time) → next
/// batch (locked) → push
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
        let sealed = seal_batch(db, creds)?;
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
