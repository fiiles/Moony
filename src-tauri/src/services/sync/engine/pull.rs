//! Pull (spec §9.3, §9.4): pages of envelopes from the relay, each one opened, judged against its
//! author's chain and applied or acted on, the cursor moving exactly as far as the verdicts
//! allow. The relay is untrusted: a page outside its contract, an envelope that does not open
//! and a broken chain all stop the pull without moving the cursor past them; nothing this device
//! has not taken is skipped.

use rusqlite::Connection;

use super::{ConnAccess, PullReport, PullStop};
use crate::error::{AppError, Result};
use crate::services::sync::chain::{self, EnvelopeMeta, RejoinReport, Verdict};
use crate::services::sync::changeset::{self, Changeset};
use crate::services::sync::devices;
use crate::services::sync::envelope::{envelope_hash, open_op};
use crate::services::sync::error::SyncResult;
use crate::services::sync::in_savepoint;
use crate::services::sync::keys::{self, Credentials};
use crate::services::sync::merge::{apply_changeset, Outcome, StopReason};
use crate::services::sync::relay::{PullPage, PulledOp, Relay};
use crate::services::sync::state;

/// Envelopes per pull page.
pub const PULL_PAGE: u32 = 500;

/// The name a re-joined device gets when the old ID had no `sync_devices` row.
const DEFAULT_DEVICE_NAME: &str = "Moony";

/// What one envelope did to the pull.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpStep {
    /// Dealt with: the cursor may move past it.
    Advance,
    /// Kept for a quarantined author: the cursor stays before it, later envelopes are still
    /// handled.
    Hold,
    /// The pull ends here; the cursor stays before it.
    Stop(PullStop),
    /// This device re-joined under a new ID, then dealt with the envelope: the cursor may move
    /// past it.
    Rejoined,
}

/// Opens, judges and applies (or acts on) one pulled envelope, under the connection lock.
/// `report.applied` and `report.rejoined` are kept here; the caller keeps the cursor, `held` and
/// `stop`. On `Clone` the device re-joins and the same envelope is judged again (the old ID is
/// then an author like any other). An error is a local failure (the database, or a re-join that
/// could not finish) and ends the round, so it never pushes.
pub fn handle_op(
    conn: &Connection,
    creds: &Credentials,
    op: &PulledOp,
    now: i64,
    report: &mut PullReport,
) -> Result<OpStep> {
    // The epoch is read from the unauthenticated header: an unknown one is no sign of a rekey,
    // only of an envelope this device cannot open. Nothing changes because of it.
    let json = match open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &op.ct) {
        Ok(json) => json,
        Err(error) => {
            log::warn!("[SYNC] Relay op {} does not open: {error}", op.seq);
            return Ok(OpStep::Stop(PullStop::Undecryptable));
        }
    };
    let Ok(cs) = changeset::from_json(&json) else {
        log::warn!("[SYNC] Relay op {} holds no valid changeset", op.seq);
        return Ok(OpStep::Stop(PullStop::Invalid));
    };
    let meta = EnvelopeMeta::of(&cs, envelope_hash(&op.ct), op.seq);
    chain::note_barrier_marker(conn, &meta)?;
    if let Some(step) = act(conn, &cs, &meta, chain::judge(conn, &meta)?, now, report)? {
        return Ok(step);
    }
    rejoin_as_new_device(conn, Some(&meta), now)?;
    report.rejoined = true;
    log::info!("[SYNC] This profile was copied or rolled back: it re-joined under a new ID");
    match act(conn, &cs, &meta, chain::judge(conn, &meta)?, now, report)? {
        None => Err(AppError::Internal(
            "sync pull: an envelope judged a clone again after the re-join".into(),
        )),
        Some(OpStep::Advance) => Ok(OpStep::Rejoined),
        Some(step) => Ok(step),
    }
}

/// Acts on `verdict` for `cs`; `None` for `Clone`, which the caller answers with a re-join.
fn act(
    conn: &Connection,
    cs: &Changeset,
    meta: &EnvelopeMeta,
    verdict: Verdict,
    now: i64,
    report: &mut PullReport,
) -> Result<Option<OpStep>> {
    let step = match verdict {
        Verdict::Apply => match apply_changeset(conn, cs, meta.hash, now)? {
            Outcome::Applied(_) => {
                report.applied += 1;
                OpStep::Advance
            }
            Outcome::Stopped(StopReason::MissingParent { table, key, parent }) => {
                log::debug!("[SYNC] {table} {key} references the missing parent {parent}");
                log::warn!(
                    "[SYNC] Relay op {} references a parent this device has not seen",
                    meta.position
                );
                OpStep::Stop(PullStop::MissingParent)
            }
            Outcome::Stopped(StopReason::NewerSchema(_)) => OpStep::Stop(PullStop::NewerSchema),
        },
        Verdict::SkipReplay => OpStep::Advance,
        Verdict::OwnAcknowledged => {
            chain::acknowledge_own(conn, meta.seq)?;
            OpStep::Advance
        }
        Verdict::Clone => return Ok(None),
        Verdict::Quarantine => {
            log::warn!(
                "[SYNC] Device {} forked its chain: its envelopes are held",
                meta.device.to_hex()
            );
            chain::quarantine(conn, meta.device)?;
            OpStep::Hold
        }
        Verdict::Held => OpStep::Hold,
        Verdict::BelowBarrier => {
            chain::skip_below_barrier(conn, meta)?;
            OpStep::Advance
        }
        Verdict::Gap => OpStep::Stop(PullStop::Gap),
        Verdict::NewerSchema => OpStep::Stop(PullStop::NewerSchema),
        Verdict::Invalid => OpStep::Stop(PullStop::Invalid),
    };
    Ok(Some(step))
}

/// Pulls from the persisted cursor until the relay has no more or an envelope stops the pull.
/// Each page is fetched without the connection lock and checked against the relay contract
/// before any of its envelopes is handled, each envelope under its own lock. The persisted cursor
/// moves over consecutive envelopes that were dealt with (`Advance`, `Rejoined`); after the first
/// `Hold` it stays before that envelope while later ones are still handled (other authors keep
/// syncing). A stop ends the pull with the cursor before the stopping envelope, and so does a page
/// outside the contract (`Invalid`). Every envelope taken adds its size to `pulled_bytes`; a
/// stopping one is not taken (the next pull fetches it again).
///
/// When the relay's head is past the last envelope it served, the report says `behind`; the
/// cursor never moves to the head.
///
/// A relay refusal is returned as its `SyncError` class (`410` → `SnapshotRequired`, `404` →
/// `VaultNotFound`, `402` → `Lapsed`, …); a local failure as `Local`.
pub async fn pull_all<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    now: i64,
) -> SyncResult<PullReport> {
    let mut after = db.with_conn(|conn| {
        state::load(conn)?
            .map(|sync| sync.cursor)
            .ok_or_else(|| AppError::Internal("sync pull: sync is not set up".into()))
    })?;
    let auth = creds.auth();
    let mut report = PullReport::default();
    let mut holding = false;
    loop {
        let page = relay.pull(&auth, after, PULL_PAGE).await?;
        if let Err(reason) = check_page(&page, after) {
            log::warn!("[SYNC] The relay served a page outside its contract: {reason}");
            report.stop = Some(PullStop::Invalid);
            return Ok(report);
        }
        for op in &page.ops {
            let step = db.with_conn(|conn| {
                let step = handle_op(conn, creds, op, now, &mut report)?;
                if matches!(step, OpStep::Stop(_)) {
                    return Ok(step);
                }
                state::add_pulled_bytes(conn, op.ct.len() as u64)?;
                if !holding && step != OpStep::Hold {
                    state::set_cursor(conn, op.seq)?;
                }
                Ok(step)
            })?;
            match step {
                OpStep::Stop(stop) => {
                    report.stop = Some(stop);
                    return Ok(report);
                }
                OpStep::Hold => {
                    holding = true;
                    report.held += 1;
                }
                OpStep::Advance | OpStep::Rejoined => {}
            }
            after = op.seq;
        }
        if !page.more {
            report.behind = page.head > after;
            if report.behind {
                log::info!(
                    "[SYNC] The relay's head is {} but it served up to {after}",
                    page.head
                );
            }
            return Ok(report);
        }
    }
}

/// The relay contract for a page fetched after `after`: every sequence above `after` and strictly
/// increasing, the head at or above the last one, and no empty page that says there is more
/// (it would be fetched again forever).
fn check_page(page: &PullPage, after: u64) -> std::result::Result<(), &'static str> {
    let mut last = after;
    for op in &page.ops {
        if op.seq <= last {
            return Err("a sequence is not above the one before");
        }
        last = op.seq;
    }
    if page.head < last {
        return Err("the head is below the page");
    }
    if page.more && page.ops.is_empty() {
        return Err("an empty page says there is more");
    }
    Ok(())
}

/// Re-join after a clone or rollback (§11.4), in one savepoint: `chain::rejoin` under an ID drawn
/// above the old one (`chain::rejoin_device_id`), a new device key pair, this device's
/// `sync_devices` row under the new ID with the old row's name (`"Moony"` when there was none;
/// the platform is this build's), and the vault binding marked unwritten (Task 12 writes it
/// again).
pub fn rejoin_as_new_device(
    conn: &Connection,
    trigger: Option<&EnvelopeMeta>,
    now: i64,
) -> Result<RejoinReport> {
    in_savepoint(conn, "sync_rejoin_as_new_device", || {
        let old = state::load(conn)?
            .ok_or_else(|| AppError::Internal("sync pull: sync is not set up".into()))?
            .device;
        let name = devices::device_row(conn, old)?
            .map_or_else(|| DEFAULT_DEVICE_NAME.to_string(), |(name, _)| name);
        let report = chain::rejoin(conn, chain::rejoin_device_id(old)?, trigger)?;
        let public = keys::new_device_keypair(conn)?;
        devices::record_this_device(conn, &name, &public, now)?;
        state::set_binding_written(conn, false)?;
        Ok(report)
    })
}
