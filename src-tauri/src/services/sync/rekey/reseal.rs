//! Re-sealing what this device had not yet sent when the key changed (phase 5 spec §7.2 step 3).
//!
//! An envelope still in `sync_unsent` that is sealed under an older epoch must never reach the
//! sync server after the key change: every device's epoch rule (§6.1) stops at an op above the
//! rotation head under an older epoch. So the engine re-seals before it pushes, and only once its
//! pull has looked at every op up to the head of the current epoch (`may_reseal`): every own
//! envelope the server took before the rotation has come back by then (its echo removed it from
//! `sync_unsent`), and the old token can write nothing after it, so what is left was never stored.
//! Each one is opened, its `prev` set to the hash of the envelope before it (the first keeps the
//! hash of the last acknowledged one, which `sync_own_log` holds), sealed under the current epoch
//! and stored back under the same sequence in `sync_unsent` and `sync_own_log`. Same sequence
//! numbers and rows; only the bytes, and so the op ID and the chain hash, change. Nothing is lost
//! or applied twice: every device applies an author's envelopes by sequence, and none of these
//! sequences reached anyone.
//!
//! Nothing older reaches the server in the meantime: `push_all` itself never sends an envelope
//! sealed under an epoch below the current key's, whoever calls it (the round, the leave, the
//! re-sync), and neither does anything queued behind one, so the sender's chain stays in order;
//! such envelopes wait in `sync_unsent` for the re-seal. And while this device's own key-change
//! head is unsigned (`head::own_unverified`) nothing is sealed again at all: the device holds
//! still until it has read its range and signed it (spec
//! 2026-11-20-sync-audit-2-fixes-design §2.3 step 5). A head record queued by that signature is
//! sealed under the current key; one still waiting when a later key change is adopted is sealed
//! again like any envelope, and a record is searched for in envelopes sealed under its epoch or
//! later.

use rusqlite::{params, Connection};

use crate::error::{AppError, Result};
use crate::services::sync::changeset;
use crate::services::sync::envelope::{envelope_epoch, envelope_hash, open_op, seal_op};
use crate::services::sync::in_savepoint;
use crate::services::sync::keys::Credentials;

fn corrupt(what: &str) -> AppError {
    AppError::Internal(format!("sync re-seal: {what}"))
}

/// Whether any envelope waiting to be sent names an epoch below the current one in its header
/// (one whose header does not read is not this module's: it goes as it stands, as before).
pub fn holds_older(conn: &Connection, creds: &Credentials) -> Result<bool> {
    let current = creds.keys.current_epoch();
    let mut stmt = conn.prepare_cached("SELECT envelope FROM sync_unsent")?;
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let envelope: Vec<u8> = row.get(0)?;
        if envelope_epoch(&envelope).is_some_and(|epoch| epoch < current) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Whether the round's pulls let this device re-seal. `through`: every op up to that relay
/// position was looked at in this round; `clean_through`: how far the pull of this pass got when
/// it stopped nowhere (`None` when it stopped). Never while this device's own head is unsigned.
/// Otherwise the current epoch's start must be known:
///
/// - **verified** (`head::verified_epochs`: the remover's own once it signed it, or one its signed
///   head confirmed): the round's pulls got there (`through`). The head and the range below it
///   are what their signer saw: a device that checked the range holds a note for every op the
///   signer found there, and the pull ends on an older-key op at a position of the range that is
///   not the noted one (rule 6) or has no note. So an own envelope the server took before the
///   rotation sits at a noted position at or below the head, the pull that reached the head
///   has met it, and its echo has removed it from `sync_unsent`;
/// - **only the server's** (not refuted; the range is not checked here yet, so no note exists):
///   this pass's pull got there **and stopped nowhere**. Every op up to the head was then dealt
///   with, so every own envelope the server holds up to it came back and left `sync_unsent`. A
///   head lowered below an own envelope the server took cannot pass this: that envelope sits
///   above the lowered head under the old epoch, where the epoch rule stops the pull (`Invalid`);
///   a head raised above new-key ops stops it the same way, and a removed device's old-key ops
///   above where this device stood stop it too (`KeyChangeUnconfirmed`). So nothing is sent
///   twice, and a caught-up device does not wait for the remover's signed record to send what it
///   has.
pub fn may_reseal(
    conn: &Connection,
    creds: &Credentials,
    through: u64,
    clean_through: Option<u64>,
) -> Result<bool> {
    // The remover's own head, until it has read its range and signed it, is no head to re-seal
    // against: nothing is sealed again (spec 2026-11-20-sync-audit-2-fixes-design §2.3 step 5).
    if super::head::own_unverified(conn)? {
        return Ok(false);
    }
    let current = creds.keys.current_epoch();
    if let Some(head) = super::head::verified_epochs(conn)?.get(&current) {
        return Ok(through >= *head);
    }
    let refuted = super::head::unverified_epochs(conn)?
        .iter()
        .any(|e| e.epoch == current && e.refuted);
    let head = crate::services::sync::state::epochs(conn)?
        .get(&current)
        .copied();
    Ok(!refuted
        && head
            .zip(clean_through)
            .is_some_and(|(head, through)| through >= head))
}

/// Re-seals, in sequence order and in one savepoint, every envelope in `sync_unsent` sealed under
/// an epoch below the current one, and every one after the first it re-sealed (its `prev` names
/// the envelope before it, whose hash changed). Envelopes already under the current epoch before
/// the first older one are left alone, so a second call changes nothing. `sync_state.last_hash`
/// follows the newest envelope's new hash. Returns how many it sealed again.
pub fn reseal_unsent(conn: &Connection, creds: &Credentials) -> Result<usize> {
    in_savepoint(conn, "sync_reseal_unsent", || {
        let rows: Vec<(i64, Vec<u8>)> = conn
            .prepare("SELECT seq, envelope FROM sync_unsent ORDER BY seq")?
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        let current = creds.keys.current_epoch();
        let mut resealed = 0usize;
        for (seq, envelope) in rows {
            let older = envelope_epoch(&envelope).is_some_and(|epoch| epoch < current);
            if !older && resealed == 0 {
                continue;
            }
            let json = open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &envelope)
                .map_err(|_| corrupt("an unsent envelope does not open"))?;
            let mut cs = changeset::from_json(&json)?;
            if u64::try_from(seq).ok() != Some(cs.seq) {
                return Err(corrupt(
                    "an unsent envelope is stored under another sequence",
                ));
            }
            // The own log holds the hash of the envelope before it as it now stands: kept when
            // it was acknowledged, new when this call sealed it again.
            cs.prev = if cs.seq == 1 {
                None
            } else {
                Some(own_hash(conn, seq - 1)?)
            };
            let sealed = seal_op(
                current,
                creds.keys.current(),
                &creds.vault_id.0,
                &changeset::to_json(&cs)?,
            )?;
            let hash = envelope_hash(&sealed);
            conn.execute(
                "UPDATE sync_unsent SET envelope = ?2 WHERE seq = ?1",
                params![seq, sealed],
            )?;
            let logged = conn.execute(
                "UPDATE sync_own_log SET hash = ?2 WHERE seq = ?1",
                params![seq, hash.to_vec()],
            )?;
            if logged != 1 {
                return Err(corrupt("an unsent envelope is not in the own log"));
            }
            resealed += 1;
        }
        if resealed > 0 {
            conn.execute(
                "UPDATE sync_state SET last_hash = \
                   (SELECT hash FROM sync_own_log ORDER BY seq DESC LIMIT 1) WHERE id = 1",
                [],
            )?;
        }
        Ok(resealed)
    })
}

/// The chain hash of this device's own envelope `seq`.
fn own_hash(conn: &Connection, seq: i64) -> Result<[u8; 32]> {
    let hash: Vec<u8> = conn
        .query_row("SELECT hash FROM sync_own_log WHERE seq = ?1", [seq], |r| {
            r.get(0)
        })
        .map_err(|_| corrupt("the envelope before the unsent ones is not in the own log"))?;
    hash.try_into()
        .map_err(|_| corrupt("sync_own_log.hash is not 32 bytes"))
}
