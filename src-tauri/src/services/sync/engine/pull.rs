//! Pull (spec §9.3, §9.4): pages of envelopes from the relay, each one opened, judged against its
//! author's chain and applied or acted on, the cursor moving exactly as far as the verdicts
//! allow. The relay is untrusted: a page outside its contract, an envelope that does not open
//! and a broken chain link all stop the pull without moving the cursor past them; nothing this
//! device has not taken is skipped. An envelope that comes before an earlier one of its author, or
//! that references a parent not here yet, is held like a quarantined author's (audit M3): the
//! cursor stays before it, the rest of the pull goes on, and this device keeps pushing, so a
//! withheld or reordered envelope never stalls it. A newer-schema envelope stops the pull too,
//! but this device keeps pushing past it (spec §14), so the rest of the pull only looks for this
//! device's own envelopes and acknowledges them (their echo, audit M4), applying nothing. A
//! changeset dated more than ten years past this device's time is held the same way, with the
//! later envelopes of its author as gaps behind it, until the person skips it (audit M6,
//! `chain::too_far_ahead`).

use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;

use super::{ConnAccess, PullReport, PullStop};
use crate::error::{AppError, Result};
use crate::services::sync::blobs;
use crate::services::sync::chain::gaps::{self, GapKind};
use crate::services::sync::chain::{self, EnvelopeMeta, RejoinReport, Verdict};
use crate::services::sync::changeset::{self, Changeset, RowChange, RowKey, SqlValue};
use crate::services::sync::devices;
use crate::services::sync::envelope::{envelope_epoch, envelope_hash, open_op, OpenError};
use crate::services::sync::error::SyncResult;
use crate::services::sync::hlc::{now_ms, DeviceId};
use crate::services::sync::identity::{self, trust};
use crate::services::sync::in_savepoint;
use crate::services::sync::keys::{self, Credentials};
use crate::services::sync::merge::{apply_changeset, Outcome, StopReason};
use crate::services::sync::rekey::head;
use crate::services::sync::relay::{PullPage, PulledOp, Relay};
use crate::services::sync::state;

/// Envelopes per pull page.
pub const PULL_PAGE: u32 = 500;

/// Pages one pull fetches at most (100,000 envelopes); the rest is left to the next round.
pub const MAX_PULL_PAGES: usize = 200;

/// The name a re-joined device gets when the old ID had no `sync_devices` row.
pub(crate) const DEFAULT_DEVICE_NAME: &str = "Moony";

/// What one envelope did to the pull.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpStep {
    /// Dealt with: the cursor may move past it.
    Advance,
    /// Kept for a quarantined author, waiting for an earlier envelope of its author or for a
    /// parent it references, or dated too far ahead (audit M6): the cursor stays before it, later
    /// envelopes are still handled.
    Hold,
    /// The pull ends here; the cursor stays before it.
    Stop(PullStop),
    /// This device re-joined under a new ID, then dealt with the envelope: the cursor may move
    /// past it.
    Rejoined,
}

/// Opens, judges and applies (or acts on) one pulled envelope, under the connection lock.
/// `report.applied`, `report.held`, `report.gaps`, `report.ahead` and `report.rejoined` are kept
/// here; the caller
/// keeps the cursor and `stop`. On `Clone` the device re-joins and the same envelope is judged
/// again (the old ID is then an author like any other). An error is a local failure (the
/// database, or a re-join that could not finish) and ends the round, so it never pushes.
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
            // A later epoch than this device's is a hint to look for a key change (phase 5 §7.1);
            // the pickup decides, the op itself proves nothing.
            if let OpenError::UnknownEpoch(epoch) = error {
                if epoch > creds.keys.current_epoch() {
                    report.key_hint = Some(epoch);
                }
            }
            return Ok(OpStep::Stop(PullStop::Undecryptable));
        }
    };
    // The epoch is authenticated now. Only a misbehaving server puts an op on the wrong side of a
    // key change (phase 5 §6.1).
    let epoch = envelope_epoch(&op.ct);
    if !epoch_fits_position(conn, creds, epoch, op.seq)? {
        log::warn!(
            "[SYNC] Relay op {} is sealed under an epoch that does not fit its place",
            op.seq
        );
        return Ok(OpStep::Stop(PullStop::Invalid));
    }
    // An older op past where this device stood at a key change waits for the remover's signed
    // head: the server's alone could have been moved to let a removed device's ops in.
    if let Some(epoch) = epoch {
        if head::awaits_head(conn, epoch, op.seq)? {
            log::info!(
                "[SYNC] Relay op {} waits for the signed start of a key change",
                op.seq
            );
            return Ok(OpStep::Stop(PullStop::KeyChangeUnconfirmed));
        }
    }
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

/// Notes where this device first saw each device's certificate and `left` record (phase 5
/// revision §3.1, `trust::note_sighting`; the first one wins): every `cert` or `left_record` value
/// of a `sync_devices` row of `rows` that the row now holds and that verifies, at relay position
/// `position`. The pull calls it for an applied changeset (with its op's position), a snapshot
/// restore for its rows (with the snapshot's).
pub(crate) fn note_trust_positions(
    conn: &Connection,
    rows: &[RowChange],
    position: u64,
) -> Result<()> {
    for row in rows.iter().filter(|row| row.table == "sync_devices") {
        let RowKey::Single(id) = &row.key else {
            continue;
        };
        let Ok(device) = DeviceId::from_hex(id) else {
            continue;
        };
        for (column, what) in [
            ("cert", trust::SeenWhat::Cert),
            ("left_record", trust::SeenWhat::Left),
        ] {
            if let Some((SqlValue::Text(value), _)) = row.columns.get(column) {
                trust::note_sighting(conn, device, what, value, position)?;
            }
        }
    }
    Ok(())
}

/// The epoch rule (phase 5 §6.1): for every key change this device knows the relay position of
/// (`sync_epochs`, epoch k at head h), an op above h is sealed under epoch k or later and an op at
/// or below h under an earlier one; a Kit joiner's bound (`state::KitEpochFloor`) holds as a head
/// would for the ops past it. A key epoch this device holds a key for but has no head for is not
/// checked (a warning is logged once per process, not per op or round).
pub(super) fn epoch_fits_position(
    conn: &Connection,
    creds: &Credentials,
    epoch: Option<u32>,
    position: u64,
) -> Result<bool> {
    let Some(epoch) = epoch else {
        return Ok(false);
    };
    let heads = state::epochs(conn)?;
    for (known, head) in &heads {
        if (position > *head) != (epoch >= *known) {
            return Ok(false);
        }
    }
    // A Kit joiner's newest epoch began at or before the snapshot it restored (revision §2.6
    // step 5): nothing past that snapshot is sealed under an older epoch.
    if let Some(floor) = state::kit_epoch_floor(conn)? {
        if position > floor.after && epoch < floor.epoch {
            return Ok(false);
        }
    }
    // One warning per process: every op of every round would meet the same gap.
    static LOGGED: AtomicBool = AtomicBool::new(false);
    if creds
        .keys
        .epochs()
        .any(|(known, _)| known > 0 && !heads.contains_key(&known))
        && !LOGGED.swap(true, Ordering::Relaxed)
    {
        log::warn!("[SYNC] A key epoch has no known relay position: it is not checked");
    }
    Ok(true)
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
        // A change dated more than ten years ahead is held (audit M6): applying it would move
        // this device's clock that far and let it win every edit made before then. Nothing of it
        // is written; the next pulls meet it again, and the person may skip it after 14 days.
        Verdict::Apply if chain::too_far_ahead(changeset::max_hlc(cs), now_ms()) => {
            log::info!(
                "[SYNC] Relay op {} is dated more than ten years ahead: it is held",
                meta.position
            );
            gaps::note_held_ahead(conn, meta, changeset::max_hlc(cs), now)?;
            report.note_ahead(meta.device);
            OpStep::Hold
        }
        Verdict::Apply => match apply_changeset(conn, cs, meta.hash, now)? {
            Outcome::Applied(_) => {
                note_trust_positions(conn, &cs.rows, meta.position)?;
                for ticket in &cs.tickets {
                    trust::note_ticket(conn, ticket, cs.device, meta.position)?;
                }
                report.applied += 1;
                OpStep::Advance
            }
            // The merge rolled back: the parent may come in a later envelope (a resent one, or
            // one the relay placed after this), so the change waits like a gap.
            Outcome::Stopped(StopReason::MissingParent { table, key, parent }) => {
                log::debug!("[SYNC] {table} {key} references the missing parent {parent}");
                log::debug!(
                    "[SYNC] Relay op {} references a parent this device has not seen: it waits",
                    meta.position
                );
                gaps::note_held_gap(conn, meta, GapKind::Parent, now)?;
                report.note_gap(meta.device);
                OpStep::Hold
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
            report.held += 1;
            OpStep::Hold
        }
        Verdict::Held => {
            report.held += 1;
            OpStep::Hold
        }
        Verdict::BelowBarrier => {
            chain::skip_below_barrier(conn, meta)?;
            OpStep::Advance
        }
        Verdict::Gap => {
            log::debug!(
                "[SYNC] Relay op {} comes before an earlier envelope of its author: it waits",
                meta.position
            );
            // Behind its author's change dated too far ahead it is an ordinary gap too: the
            // author's record stays the lower `Ahead` one, and skipping that releases it.
            gaps::note_held_gap(conn, meta, GapKind::Missing, now)?;
            report.note_gap(meta.device);
            OpStep::Hold
        }
        Verdict::NewerSchema => OpStep::Stop(PullStop::NewerSchema),
        Verdict::Invalid => OpStep::Stop(PullStop::Invalid),
    };
    Ok(Some(step))
}

/// Pulls from the persisted cursor until the relay has no more, an envelope stops the pull, or the
/// pull reaches its bounds. Each page is fetched without the connection lock and checked against
/// the relay contract before any of its envelopes is handled, each envelope under its own lock.
/// The persisted cursor moves over consecutive envelopes that were dealt with (`Advance`,
/// `Rejoined`); after the first `Hold` (a quarantined author, a gap in an author's chain, a missing
/// parent, a change dated too far ahead) it stays before that envelope while later ones are still handled (other authors keep
/// syncing, this device's own envelopes are still acknowledged), so the next pull meets the held
/// envelope again. A stop ends the pull with the cursor before the stopping envelope, and so does
/// a page outside the contract (`Invalid`); one whose head is below where the pull stands is a
/// sync server that went back (restored from an older backup), noted until a replace or turning
/// sync off (`state::note_vault_went_back`). A `NewerSchema` stop alone goes on to the same bounds
/// in scan mode (`acknowledge_if_own`) while anything of this device waits in `sync_unsent`
/// (checked at the stop and after each page, so with nothing waiting it stops at once): this
/// device's own envelopes are acknowledged, nothing is applied, no other author's head and not
/// the cursor move, so the pushes a newer-schema stop allows still get their echo. A page outside
/// the contract met while scanning ends the scan and the pull stays reported as `NewerSchema`.
///
/// Relay positions are not authenticated, so one pull is bounded: it ends once it has handled
/// everything up to the head the first page reported (later envelopes are left to the next
/// round, as if they had arrived after the pull), and in any case after `MAX_PULL_PAGES` pages
/// (`unfinished`: a relay that keeps reporting a higher head, or a backlog that large).
///
/// An envelope's size counts towards `pulled_bytes` once, when the persisted cursor moves past
/// it: an envelope fetched again (behind a held one, or after a stop) is not counted again.
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
    pull_bounded(relay, db, creds, now, MAX_PULL_PAGES, None).await
}

/// The round's echo pull after its push: `pull_all`, except that a scan after a `NewerSchema`
/// stop starts past `scan_from`, the reach (`PullReport::reach`) of the round's first pull, which
/// already looked at everything up to it.
pub async fn pull_echo<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    now: i64,
    scan_from: Option<u64>,
) -> SyncResult<PullReport> {
    pull_bounded(relay, db, creds, now, MAX_PULL_PAGES, scan_from).await
}

/// `pull_all` with at most `max_pages` pages (tests).
#[cfg(test)]
pub(crate) async fn pull_pages<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    now: i64,
    max_pages: usize,
) -> SyncResult<PullReport> {
    pull_bounded(relay, db, creds, now, max_pages, None).await
}

/// `pull_all` with at most `max_pages` pages, scanning only past `scan_from`; then the held gaps
/// are brought up to date (`settle_gaps`).
async fn pull_bounded<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    now: i64,
    max_pages: usize,
    scan_from: Option<u64>,
) -> SyncResult<PullReport> {
    let report = pull_pages_from_cursor(relay, db, creds, now, max_pages, scan_from).await?;
    // Each merged changeset brought the transfer queue in line with its rows
    // (`queue::reconcile_rows`); the entries are checked against the store only at lane start,
    // after a replace and in a full repair, never per pull (ADR 0030). Another device's changes
    // arriving are the best sign its uploads are landing: content the server did not have at the
    // last try is due again at once (one UPDATE, no store read).
    db.with_conn(|conn| {
        settle_gaps(conn, &report)?;
        if report.applied > 0 {
            blobs::queue::content_may_have_landed(conn)?;
        }
        Ok(())
    })?;
    Ok(report)
}

/// After a pull: a pull that reached the relay's end with nothing waiting for a missing envelope
/// or parent and nothing held for its date holds no gap any more, so every record goes (`gaps::forget_all`); otherwise the gaps
/// whose author's head reached them go (`gaps::forget_healed_gaps`). A stopped or unfinished pull
/// may not have met a held envelope again, so it forgets only the healed ones.
fn settle_gaps(conn: &Connection, report: &PullReport) -> Result<()> {
    if report.gaps == 0 && report.ahead == 0 && report.stop.is_none() && !report.unfinished {
        gaps::forget_all(conn)
    } else {
        gaps::forget_healed_gaps(conn).map(|_| ())
    }
}

/// The pages of `pull_bounded`.
async fn pull_pages_from_cursor<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    now: i64,
    max_pages: usize,
    scan_from: Option<u64>,
) -> SyncResult<PullReport> {
    let mut after = db.with_conn(|conn| {
        state::load(conn)?
            .map(|sync| sync.cursor)
            .ok_or_else(|| AppError::Internal("sync pull: sync is not set up".into()))
    })?;
    let auth = creds.auth();
    let mut report = PullReport {
        through: after,
        ..PullReport::default()
    };
    let mut holding = false;
    // After a `NewerSchema` stop: only this device's own envelopes are looked at.
    let mut scanning = false;
    let mut first_head = None;
    for _ in 0..max_pages {
        let page = relay.pull(&auth, after, PULL_PAGE).await?;
        if let Err(reason) = check_page(&page, after, PULL_PAGE) {
            log::warn!("[SYNC] The relay served a page outside its contract: {reason}");
            if page.head < after {
                log::warn!("[SYNC] The sync server's head went back below this device's place");
                db.with_conn(|conn| state::note_vault_went_back(conn, now))?;
            }
            report.stop.get_or_insert(PullStop::Invalid);
            return Ok(report);
        }
        let target = *first_head.get_or_insert(page.head);
        report.reach = Some(target);
        for op in &page.ops {
            if scanning {
                if scan_from.is_none_or(|from| op.seq > from) {
                    db.with_conn(|conn| acknowledge_if_own(conn, creds, op))?;
                }
                after = op.seq;
                report.through = op.seq;
                continue;
            }
            let step = db.with_conn(|conn| {
                let step = handle_op(conn, creds, op, now, &mut report)?;
                if !holding && matches!(step, OpStep::Advance | OpStep::Rejoined) {
                    state::set_cursor(conn, op.seq)?;
                    state::add_pulled_bytes(conn, op.ct.len() as u64)?;
                }
                Ok(step)
            })?;
            match step {
                OpStep::Stop(PullStop::NewerSchema) => {
                    report.stop = Some(PullStop::NewerSchema);
                    if !db.with_conn(waiting_for_echo)? {
                        return Ok(report);
                    }
                    scanning = true;
                }
                OpStep::Stop(stop) => {
                    report.stop = Some(stop);
                    return Ok(report);
                }
                OpStep::Hold => holding = true,
                OpStep::Advance | OpStep::Rejoined => {}
            }
            after = op.seq;
            report.through = op.seq;
        }
        if scanning {
            if !db.with_conn(waiting_for_echo)? {
                return Ok(report);
            }
            if let Some(from) = scan_from {
                after = after.max(from);
            }
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
        if after >= target {
            return Ok(report);
        }
    }
    log::warn!("[SYNC] A pull ended after {max_pages} pages, before the relay's head");
    report.unfinished = true;
    Ok(report)
}

/// Whether anything of this device waits in `sync_unsent` for its echo.
fn waiting_for_echo(conn: &Connection) -> Result<bool> {
    Ok(
        conn.query_row("SELECT EXISTS (SELECT 1 FROM sync_unsent)", [], |r| {
            r.get(0)
        })?,
    )
}

/// Scan mode after a `NewerSchema` stop: acknowledges `op` when it is this device's own envelope as
/// sealed here (`Verdict::OwnAcknowledged`); anything else is left for the pull that reaches it
/// (nothing is applied, no re-join, the cursor does not move).
fn acknowledge_if_own(conn: &Connection, creds: &Credentials, op: &PulledOp) -> Result<()> {
    let Ok(json) = open_op(|epoch| creds.keys.get(epoch), &creds.vault_id.0, &op.ct) else {
        return Ok(());
    };
    let Ok(cs) = changeset::from_json(&json) else {
        return Ok(());
    };
    let meta = EnvelopeMeta::of(&cs, envelope_hash(&op.ct), op.seq);
    if chain::judge(conn, &meta)? == Verdict::OwnAcknowledged {
        chain::acknowledge_own(conn, meta.seq)?;
    }
    Ok(())
}

/// The relay contract for a page fetched after `after` with `limit`: at most `limit` envelopes,
/// every sequence above `after` and strictly increasing, the head at or above the last sequence
/// and never below `after` (an empty page included: the relay's head never goes back), and no
/// empty page that says there is more (it would be fetched again forever).
fn check_page(page: &PullPage, after: u64, limit: u32) -> std::result::Result<(), &'static str> {
    if page.ops.len() > limit as usize {
        return Err("more envelopes than the limit");
    }
    let mut last = after;
    for op in &page.ops {
        if op.seq <= last {
            return Err("a sequence is not above the one before");
        }
        last = op.seq;
    }
    if page.head < last {
        return Err("the head is below the page or the cursor");
    }
    if page.more && page.ops.is_empty() {
        return Err("an empty page says there is more");
    }
    Ok(())
}

/// Re-join after a clone or rollback (§11.4), in one savepoint: `chain::rejoin` under an ID drawn
/// above the old one (`chain::rejoin_device_id`), a new device key pair and signing key with a
/// re-join certificate signed by the old ID's key (phase 5 §5.3; none when the old ID had no
/// pinned certificate, §5.5), this device's `sync_devices` row under the new ID with the old
/// row's name (`"Moony"` when there was none; the platform is this build's) and that certificate,
/// and the vault binding marked unwritten (Task 12 writes it again).
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
        // The old ID's key vouches for the new one (phase 5 §5.3), read before the ID changes.
        let predecessor = identity::predecessor(conn)?;
        let report = chain::rejoin(conn, chain::rejoin_device_id(old)?, trigger)?;
        let public = keys::new_device_keypair(conn)?;
        let cert = identity::successor(conn, predecessor, now)?;
        devices::record_this_device(conn, &name, &public, cert.as_ref(), now)?;
        state::set_binding_written(conn, false)?;
        Ok(report)
    })
}
