//! Push (spec §9.4): this device's changes leave in two steps that never share a lock with the
//! network. First the outbox becomes sealed envelopes in `sync_unsent`, each recorded in the own
//! log as the chain moves on in the same transaction that removes the outbox rows it consumed
//! (ruling R11); one envelope per lock hold, so a large first drain never freezes the profile.
//! Then the waiting envelopes go to the relay in sequence order, in batches within its limits.
//! The relay's answer deletes nothing (audit M4): an envelope leaves `sync_unsent` only when this
//! device pulls it back from the relay (its echo, `OwnAcknowledged` → `chain::acknowledge_own`),
//! so a relay that acknowledges a push and then loses it gets the envelope again in the next
//! round. That does not cover a relay restored from an older backup: its head goes below this
//! device's cursor, the pull stops (`PullStop::Invalid`), and envelopes echoed after that backup
//! are no longer here to send (audit M4 stays open for that case; the re-sync keeps its backup).
//! A crash anywhere pushes the same envelopes again: the op ID makes that a no-op on a relay that
//! holds them, and this device's chain never forks. A batch of nothing but re-sends that the relay
//! answers with no new op (it holds them all, yet never serves them back) ends the re-sends of
//! that call, and the round reports a relay error. At most `MAX_UNECHOED` envelopes wait for their
//! echo; past that nothing more is sealed and the round reports a relay error.

use std::collections::BTreeSet;
use std::fmt;

use rusqlite::Connection;

use super::ConnAccess;
use crate::error::{AppError, Result};
use crate::services::sync::changeset::to_json;
use crate::services::sync::envelope::{envelope_epoch, envelope_hash, op_id, seal_op};
use crate::services::sync::error::{SyncError, SyncResult};
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
/// Envelopes that may wait in `sync_unsent` for their echo (60 MiB at the envelope budget). An
/// honest relay serves them back in the round's next pull; a relay that acknowledges and never
/// serves back stops this device sealing more (`SealReport::unechoed_cap`) instead of growing the
/// table and every round's re-send without bound.
pub const MAX_UNECHOED: usize = 1024;

/// What one `seal_batch` did. Its `Debug` shows a refused row's table and size, never its key.
#[derive(Default)]
pub struct SealReport {
    /// Envelopes with rows sealed into `sync_unsent`.
    pub sealed: usize,
    /// A barrier marker (an envelope with no rows) was sealed instead.
    pub marker: bool,
    /// Rows too large to send, once each; they stay in the outbox.
    pub refused: Vec<Refused>,
    /// Sealing stopped because `sync_unsent` already holds `MAX_UNECHOED` envelopes.
    pub unechoed_cap: bool,
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
            .field("unechoed_cap", &self.unechoed_cap)
            .finish()
    }
}

/// Seals up to `SEAL_BATCH` envelopes from the outbox into `sync_unsent`, only when nothing sealed
/// earlier is still waiting to be sent: no row above `after`, the last sequence this `push_all`
/// sent (any row when `None`), so the chain stays in the order it is pushed. Rows at or below
/// `after` were sent and wait for their echo. Nothing is sealed while the table already holds
/// `MAX_UNECHOED` rows (`unechoed_cap`). The lock is taken for at most `SEAL_PER_HOLD` envelopes at
/// a time, each hold one transaction, and released in between; the batch ends when the outbox has
/// nothing more to send, the target is met or the cap is reached.
///
/// Any own envelope carries this device's barrier, so sealing one marks the barrier
/// (`note_sealed_at_barrier`). With nothing to send, a device past a barrier whose marker it has
/// not met (`barrier_seq` unknown) seals one marker for it, at most one per barrier (ruling P5).
pub fn seal_batch<D: ConnAccess>(
    db: &D,
    creds: &Credentials,
    after: Option<u64>,
) -> Result<SealReport> {
    let mut report = SealReport::default();
    let mut first = true;
    while report.sealed < SEAL_BATCH {
        let max = SEAL_PER_HOLD.min(SEAL_BATCH - report.sealed);
        let hold = db.with_conn(|conn| seal_hold(conn, creds, max, first, after))?;
        first = false;
        report.sealed += hold.sealed;
        report.marker |= hold.marker;
        report.unechoed_cap |= hold.unechoed_cap;
        report.add_refused(hold.refused);
        if hold.sealed < max {
            break;
        }
    }
    Ok(report)
}

/// One lock hold of `seal_batch`, in one transaction: up to `max` envelopes, and never past
/// `MAX_UNECHOED` rows in `sync_unsent`. The `first` hold seals nothing while an envelope above
/// `after` waits to be sent, and is the one that may seal a marker.
fn seal_hold(
    conn: &Connection,
    creds: &Credentials,
    max: usize,
    first: bool,
    after: Option<u64>,
) -> Result<SealReport> {
    let mut report = SealReport::default();
    if first {
        let waiting: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sync_unsent WHERE seq > ?1)",
            [seq_after(after)?],
            |r| r.get(0),
        )?;
        if waiting {
            return Ok(report);
        }
    }
    let held: i64 = conn.query_row("SELECT COUNT(*) FROM sync_unsent", [], |r| r.get(0))?;
    let room = MAX_UNECHOED.saturating_sub(usize::try_from(held).unwrap_or(usize::MAX));
    if room == 0 {
        report.unechoed_cap = true;
        return Ok(report);
    }
    let max = max.min(room);
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
pub(crate) fn seal_into_unsent(tx: &Connection, creds: &Credentials, built: &Built) -> Result<()> {
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

/// The next push: the waiting envelopes above `after` in sequence order, within the relay limits.
pub fn next_push_batch(conn: &Connection, after: Option<u64>) -> Result<Vec<(u64, OutgoingOp)>> {
    next_push_batch_with(conn, after, PUSH_MAX_OPS, PUSH_MAX_B64)
}

/// The waiting envelopes above `after` (all when `None`) from the lowest sequence on, as many as
/// fit `max_ops` and `max_b64` base64 bytes. The first one always goes, so an envelope over
/// `max_b64` alone (never one the journal builds) cannot hold the rest back forever.
pub fn next_push_batch_with(
    conn: &Connection,
    after: Option<u64>,
    max_ops: usize,
    max_b64: usize,
) -> Result<Vec<(u64, OutgoingOp)>> {
    let mut stmt =
        conn.prepare_cached("SELECT seq, envelope FROM sync_unsent WHERE seq > ?1 ORDER BY seq")?;
    let mut rows = stmt.query([seq_after(after)?])?;
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

/// `after` as the lower bound of a `seq > ?` query: every sequence is at least 1.
fn seq_after(after: Option<u64>) -> Result<i64> {
    i64::try_from(after.unwrap_or(0))
        .map_err(|_| AppError::Internal("sync push: sequence out of range".into()))
}

/// What one `push_all` did.
#[derive(Debug, Default)]
pub struct PushReport {
    /// Envelopes the relay acknowledged (markers included, and envelopes sent again because
    /// their echo has not come back).
    pub pushed: usize,
    /// Rows too large to send, each counted once; they stay in the outbox.
    pub refused: usize,
    /// Sequences the relay acknowledged in this call (they wait for their echo).
    pub acknowledged: Vec<u64>,
    /// Sealing stopped at `MAX_UNECHOED`: the relay acknowledges and never serves back.
    pub unechoed_cap: bool,
    /// The highest sequence already waiting in `sync_unsent` when the call began: envelopes up to
    /// it were sealed (and as a rule sent) in an earlier call and are re-sends here.
    pub resent_through: Option<u64>,
    /// A batch of re-sends only was answered with no new op: the rest of the re-sends were skipped.
    pub resends_skipped: bool,
}

/// Seals and pushes until nothing is left to send: seal (locked one envelope at a time) → next
/// batch (locked) → push (unlocked), each envelope at most once per call (`after` moves to the
/// last sequence sent). Every envelope still waiting for its echo is sent again first, from the
/// lowest sequence, and only then is anything new sealed: a relay that lost an envelope it
/// acknowledged gets it back in order. The answer removes nothing from `sync_unsent`
/// (`PushReport::acknowledged` lists what it took); the echo does. A refused row stays in the
/// outbox and never keeps the loop going. The first error ends it, returned as its `SyncError`
/// class (`402` → `Lapsed`, `413` → `TooLarge(Quota)`, `403` → `ReadOnlyToken`, …); sealed
/// envelopes keep waiting in `sync_unsent` for the next round. A receipt that accepts more ops
/// than were sent is outside the relay contract: nothing is taken on its word.
pub async fn push_all<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
) -> SyncResult<PushReport> {
    let auth = creds.auth();
    let mut report = PushReport::default();
    // (table, pk) of each refused row, logged once.
    let mut refused: BTreeSet<(String, String)> = BTreeSet::new();
    let mut after = None;
    let mut older_key_held = false;
    report.resent_through = db.with_conn(|conn| {
        let max: Option<i64> =
            conn.query_row("SELECT MAX(seq) FROM sync_unsent", [], |r| r.get(0))?;
        Ok(max.and_then(|seq| u64::try_from(seq).ok()))
    })?;
    loop {
        let sealed = seal_batch(db, creds, after)?;
        report.unechoed_cap |= sealed.unechoed_cap;
        for row in sealed.refused {
            if refused.insert((row.table.clone(), row.pk)) {
                log::warn!(
                    "[SYNC] A {} row of {} bytes is too large to send; it stays queued",
                    row.table,
                    row.bytes
                );
            }
        }
        let mut batch = db.with_conn(|conn| next_push_batch(conn, after))?;
        // R20: nothing sealed under an epoch below the current key's goes out, whoever calls
        // (the round, the leave, the re-sync's pre-push): above a key change's head it would end
        // every other device's pull. It waits in `sync_unsent` for the re-seal, and so does
        // everything behind it, so the sender's chain stays in order. (It also holds back an
        // older-key envelope the server already took whose echo this device has not pulled yet:
        // a harmless resend that waits, and a leave then stays unannounced until a pull.)
        let current = creds.keys.current_epoch();
        if let Some(cut) = batch
            .iter()
            .position(|(_, op)| envelope_epoch(&op.ct).is_some_and(|epoch| epoch < current))
        {
            batch.truncate(cut);
            if !older_key_held {
                older_key_held = true;
                log::info!("[SYNC] Envelopes sealed under an older key wait for the re-seal");
            }
        }
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
        let last = seqs.last().copied();
        let resends_only = last.is_some_and(|seq| report.resent_through >= Some(seq));
        after = last;
        report.pushed += ops.len();
        report.acknowledged.extend(seqs);
        if resends_only && receipt.accepted == 0 && after < report.resent_through {
            // The relay holds every one of them and serves none back: sending the rest of the
            // re-sends changes nothing. The new envelopes still go.
            log::warn!("[SYNC] The relay already holds envelopes it does not serve back");
            report.resends_skipped = true;
            after = report.resent_through;
        }
    }
    report.refused = refused.len();
    if report.unechoed_cap {
        log::warn!(
            "[SYNC] {MAX_UNECHOED} envelopes wait for the relay to serve them back: \
             nothing more is sealed"
        );
    }
    Ok(report)
}

fn not_set_up() -> AppError {
    AppError::Internal("sync push: sync is not set up".into())
}
