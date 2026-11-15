//! Moving document and photo content between this device's store and the sync server, one blob
//! at a time (attachments design §5.3, §5.4; ADR 0030).
//!
//! `upload_one` and `download_one` move one queued blob and record how it went in the queue
//! (`sync_blobs`); `run_uploads` and `run_downloads` take the queue one blob at a time under a lane
//! lock of the shared [`BlobPolicy`], so one upload and one download run at once, and stop between
//! two blobs when nothing is due, the caller's deadline passed, or the next blob would meet the
//! same trouble.
//!
//! Blob errors are classified here and never reach the sync round's error path
//! (`services/sync/error.rs`): a blob that does not move never fails, pauses or slows a round.
//! A transfer ends as one of [`Outcome`]: done on the server's word (an accepted body, or a
//! `HEAD` that finds the blob at its size); gone (its row or its content no longer exists: the
//! entry is dropped); found (content that turned up here waits for the server's word); failed,
//! recorded on the entry with a backoff of that blob's own (passing trouble: the network, a busy
//! or failing server, `429`; no room on the server, which also pauses uploads through the policy;
//! an unreadable download, content the server does not have yet, a local store error) or for good
//! (an invalid ID, a refusal of the blob, a conflict on the server); blocked by what the round
//! already shows (payment, token, vault: nothing is recorded); skipped while another download of
//! it runs; or a locked profile.
//!
//! Every database hold goes through `stream::hold`, off the async worker on a multi-threaded
//! runtime, and none is open across a network call.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};
use tokio::time::Instant;

use super::canonical_id;
use super::policy::{BlobPolicy, ServerBlobs};
use super::queue::{self, BlobState, Reconciled, Transfer, TransferError};
use super::stream::{hold, SinkOutcome, StoreSink, UploadSource};
use crate::error::{AppError, Result};
use crate::models::sync::SyncAttachmentsProblem;
use crate::services::attachment_store::{self, complete_sql};
use crate::services::sync::engine::ConnAccess;
use crate::services::sync::envelope::blob::{sealed_len, BlobSealer};
use crate::services::sync::keys::Credentials;
use crate::services::sync::relay::{Relay, RelayError};

/// The most a sync round spends on uploads before it pushes its changes, waiting for the upload
/// lane included: a blob never holds the round's changes back for longer.
pub const PRE_PUSH_BUDGET: Duration = Duration::from_secs(20);
/// The largest blob (plaintext bytes) a round uploads before its push; larger content waits for
/// the upload lane.
pub const PRE_PUSH_MAX_PLAIN: u64 = 8 * 1024 * 1024;

/// How one blob's transfer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The server accepted the blob's body (an upload), or the blob was downloaded and stored.
    Done,
    /// The server already had the blob at its size (a `HEAD`): done without a body.
    Confirmed,
    /// No syncing row names the blob any more, or its content left the store under an upload: the
    /// entry is dropped, nothing is retried and no blob of it is left in the store.
    Gone,
    /// The content of a blob being fetched turned up complete here: not taken as on the server,
    /// the entry waits for upload again (its `HEAD` decides).
    Found,
    /// Recorded on the entry (`last_error`) with that error's backoff (`i64::MAX`: only "try
    /// again" moves it).
    Failed(TransferError),
    /// The server refuses this vault (payment, token, vault gone): the round shows why; nothing is
    /// recorded and the run stops.
    Blocked,
    /// The profile is locked: nothing more can be read or recorded.
    Locked,
    /// Another download of the same blob is running here: nothing recorded; the run goes on with
    /// the next blob and leaves this one for its next run.
    Busy,
}

/// What a run did: blobs uploaded (the server took the body or already had it) and stored here,
/// transfers that failed, and whether anything the person sees changed (the queue's reconcile
/// included).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TransferReport {
    pub uploaded: usize,
    pub stored: usize,
    pub failed: usize,
    pub changed: bool,
}

impl TransferReport {
    fn count(&mut self, outcome: Outcome, upload: bool) {
        match outcome {
            Outcome::Done | Outcome::Confirmed if upload => self.uploaded += 1,
            Outcome::Done | Outcome::Confirmed => self.stored += 1,
            Outcome::Failed(_) => self.failed += 1,
            Outcome::Gone | Outcome::Found => {}
            Outcome::Blocked | Outcome::Locked | Outcome::Busy => return,
        }
        self.changed = true;
    }
}

fn locked() -> AppError {
    AppError::Auth("Database is locked".into())
}

/// The server's own refusals of a blob that no retry changes (`400` / `411` / `413` of the blob
/// routes), and an old server's plain `404 not_found` for a route it does not have.
const REFUSALS: &[&str] = &[
    "blob_too_large",
    "empty_body",
    "length_required",
    "not_found",
];

/// The transfer class of a sync server answer to a blob call. Passing trouble (`Network`, a
/// server error, `429`, an answer outside the contract, a body cut short) is retried later per
/// blob; a refusal of the blob itself (one the client decides, "body too large", or the server's
/// `REFUSALS`) fails for good; the round's own classes (`SyncError`) are never consulted.
pub(super) fn classify(error: &RelayError) -> Outcome {
    match error {
        RelayError::Network(_) => Outcome::Failed(TransferError::Network),
        RelayError::BlobNotFound => Outcome::Failed(TransferError::NotOnServer),
        RelayError::QuotaExceeded | RelayError::InsufficientStorage => {
            Outcome::Failed(TransferError::NoRoom)
        }
        RelayError::BadRequest(code) if code == "invalid_blob_id" => {
            Outcome::Failed(TransferError::InvalidId)
        }
        RelayError::BadRequest(code) if REFUSALS.contains(&code.as_str()) => {
            Outcome::Failed(TransferError::Refused)
        }
        RelayError::Protocol(reason) if reason == "body too large" => {
            Outcome::Failed(TransferError::Refused)
        }
        RelayError::PaymentRequired
        | RelayError::Unauthorized
        | RelayError::TokenReadOnly
        | RelayError::VaultNotFound => Outcome::Blocked,
        _ => Outcome::Failed(TransferError::Server),
    }
}

/// Records `outcome` of `blob_id`'s transfer in the queue (one hold) and returns it: done, gone
/// (the entry goes), found (the entry waits for upload), failed (one more attempt and its
/// backoff), else the entry only stops being active. A locked profile is `Locked`; another local
/// error is logged and `Failed(Local)`. `Done` and `Confirmed` of an upload go through
/// [`confirmed`] instead.
fn record<D: ConnAccess>(db: &D, blob_id: &str, outcome: Outcome, now: i64) -> Outcome {
    let recorded = hold(db, |conn| match outcome {
        Outcome::Done | Outcome::Confirmed => queue::note_done(conn, blob_id),
        Outcome::Gone => queue::note_gone(conn, blob_id),
        Outcome::Found => queue::note_found(conn, blob_id, now).map(|_| ()),
        Outcome::Failed(error) => queue::note_failed(conn, blob_id, error, now),
        Outcome::Blocked | Outcome::Locked | Outcome::Busy => {
            queue::set_active(conn, blob_id, false)
        }
    });
    match recorded {
        Ok(()) => outcome,
        Err(e) => local_unrecorded(e),
    }
}

/// A local error that could not even be recorded: `Locked`, or logged as `Failed(Local)`.
fn local_unrecorded(error: AppError) -> Outcome {
    if matches!(error, AppError::Auth(_)) {
        return Outcome::Locked;
    }
    log::warn!("[SYNC] A document or photo transfer could not be recorded: {error}");
    Outcome::Failed(TransferError::Local)
}

/// A local error before or after a transfer: `Locked` for a locked profile, else recorded as a
/// local failure.
fn local<D: ConnAccess>(db: &D, blob_id: &str, error: AppError, now: i64) -> Outcome {
    if matches!(error, AppError::Auth(_)) {
        return Outcome::Locked;
    }
    log::warn!("[SYNC] A document or photo transfer failed on this device: {error}");
    record(db, blob_id, Outcome::Failed(TransferError::Local), now)
}

/// The server has the blob (`outcome` is `Done` or `Confirmed`): `done` while a syncing row still
/// names it; else its row went during the upload and the entry goes (`Gone`; the server's copy is
/// left to the daily pass).
fn confirmed<D: ConnAccess>(db: &D, blob_id: &str, outcome: Outcome) -> Outcome {
    let settled = hold(db, |conn| {
        if queue::is_named(conn, blob_id)? {
            queue::note_done(conn, blob_id)?;
            return Ok(outcome);
        }
        queue::note_gone(conn, blob_id)?;
        Ok(Outcome::Gone)
    });
    settled.unwrap_or_else(local_unrecorded)
}

/// The recorded size of `blob_id` when a syncing row names it and it is complete here
/// (`attachment_store::complete_sql`), else `None`.
fn named_and_held(conn: &Connection, blob_id: &str) -> Result<Option<u64>> {
    if !queue::is_named(conn, blob_id)? {
        return Ok(None);
    }
    let size: Option<i64> = conn
        .query_row(
            &format!(
                "SELECT f.size FROM files.blobs f WHERE f.id = ?1 AND {}",
                complete_sql("f.id")
            ),
            [blob_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(size.map(|size| u64::try_from(size).unwrap_or(0)))
}

/// Uploads one blob held here: checks that a syncing row still names it and that it is complete
/// (else `Gone`), asks the server whether it has it (`HEAD`: at the blob's sealed size it is
/// `Confirmed` without a body; at another size the server holds something else under this ID,
/// which it never overwrites: `Failed(Conflict)`), and otherwise streams it up sealed under the
/// current key epoch. A blob ID that is not canonical fails as `InvalidId` before any request or
/// store read. A local error of the upload's source (the content left the store, a locked
/// profile, a read error) is recovered beside the call, so it is never mistaken for the network
/// trouble the HTTP client reports it as. A row that went during the upload ends it as `Gone`. The
/// outcome is recorded in the queue.
pub async fn upload_one<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    transfer: &Transfer,
    now: i64,
) -> Outcome {
    let id = transfer.blob_id.as_str();
    let Some(blob) = canonical_id(id) else {
        log::debug!("[SYNC] Not uploading {id}: not a valid blob ID");
        return record(db, id, Outcome::Failed(TransferError::InvalidId), now);
    };
    let size = match hold(db, |conn| named_and_held(conn, id)) {
        Ok(Some(size)) => size,
        Ok(None) => return record(db, id, Outcome::Gone, now),
        Err(e) => return local(db, id, e, now),
    };
    let declared = transfer.size.unwrap_or(size);
    let auth = creds.auth();
    match relay.head_blob(&auth, blob).await {
        Ok(Some(stored)) if stored == sealed_len(declared) => {
            return confirmed(db, id, Outcome::Confirmed)
        }
        Ok(Some(stored)) => {
            log::warn!(
                "[SYNC] The sync server holds {stored} bytes under blob {blob}, not this content"
            );
            return record(db, id, Outcome::Failed(TransferError::Conflict), now);
        }
        Ok(None) => {}
        Err(e) => return record(db, id, classify(&e), now),
    }
    let sealer = BlobSealer::new(
        creds.keys.current_epoch(),
        creds.keys.current(),
        &creds.vault_id.0,
        &blob,
    );
    let source = UploadSource::new(db.clone(), id.to_string(), blob, declared, sealer);
    let sealed = source.sealed_len();
    let cause = Arc::new(Mutex::new(None));
    let body = Box::new(Watched {
        body: source,
        cause: Arc::clone(&cause),
    });
    let sent = relay.put_blob(&auth, blob, sealed, body).await;
    let source_failure = cause.lock().unwrap_or_else(PoisonError::into_inner).take();
    match (sent, source_failure) {
        (Ok(()), _) => confirmed(db, id, Outcome::Done),
        (Err(_), Some(failure)) => record(db, id, failure, now),
        (Err(e), None) => record(db, id, classify(&e), now),
    }
}

/// An upload body that keeps what its first error meant locally: the HTTP client turns any body
/// error into a network error, and the cause is read from here instead.
struct Watched<I> {
    body: I,
    cause: Arc<Mutex<Option<Outcome>>>,
}

impl<I: Iterator<Item = io::Result<Vec<u8>>>> Iterator for Watched<I> {
    type Item = io::Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.body.next();
        if let Some(Err(error)) = &item {
            self.cause
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get_or_insert(source_failure(error));
        }
        item
    }
}

/// What an `UploadSource` error means: a locked profile, content no longer complete here (gone),
/// or a local failure (a read error, a length other than the one declared).
fn source_failure(error: &io::Error) -> Outcome {
    let cause = error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<AppError>());
    match cause {
        Some(AppError::Auth(_)) => Outcome::Locked,
        Some(AppError::NotFound(_)) => Outcome::Gone,
        _ if error.kind() == io::ErrorKind::NotFound => Outcome::Gone,
        _ => Outcome::Failed(TransferError::Local),
    }
}

/// Downloads one blob into the store: a blob no syncing row names any more is `Gone` without a
/// request, and content already complete here is `Found` (it waits for upload again: whether the
/// server has it is the server's word, never assumed). Otherwise its stream is opened piece by
/// piece into an incoming blob that becomes visible only once the whole stream authenticated. A
/// stream that does not open or ends early is `Failed(Unreadable)` and nothing of it stays; one
/// sealed with a key epoch this device has not received yet is `Failed(Local)` (on its way). A row
/// that went during the download leaves no blob behind: an incoming blob a sweep removed ends the
/// transfer as gone, and a blob that completed after its row went is deleted again. The outcome
/// is recorded in the queue.
pub async fn download_one<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    transfer: &Transfer,
    now: i64,
) -> Outcome {
    let id = transfer.blob_id.as_str();
    let Some(blob) = canonical_id(id) else {
        log::debug!("[SYNC] Not downloading {id}: not a valid blob ID");
        return record(db, id, Outcome::Failed(TransferError::InvalidId), now);
    };
    let before = hold(db, |conn| {
        let named = queue::is_named(conn, id)?;
        let here: bool = conn.query_row(&format!("SELECT {}", complete_sql("?1")), [id], |r| {
            r.get(0)
        })?;
        Ok((named, here))
    });
    match before {
        Ok((false, _)) => return record(db, id, Outcome::Gone, now),
        Ok((true, true)) => return record(db, id, Outcome::Found, now),
        Ok((true, false)) => {}
        Err(e) => return local(db, id, e, now),
    }
    let mut sink = StoreSink::new(
        db.clone(),
        id.to_string(),
        blob,
        creds.keys.clone(),
        creds.vault_id.0,
    );
    let delivery = relay.get_blob(&creds.auth(), blob, &mut sink).await;
    let outcome = match sink.finish(&delivery) {
        SinkOutcome::Stored => return stored(db, id, now),
        SinkOutcome::AlreadyHere => Outcome::Found,
        SinkOutcome::InProgress => Outcome::Busy,
        SinkOutcome::Unreadable => Outcome::Failed(TransferError::Unreadable),
        // The pull brings the key soon: on its way, retried like a local failure, never shown as
        // "Could not download" (a new `last_error` code would need a migration).
        SinkOutcome::UnknownEpoch => Outcome::Failed(TransferError::Local),
        SinkOutcome::Incomplete => match &delivery {
            Err(e) => classify(e),
            Ok(_) => Outcome::Failed(TransferError::Unreadable),
        },
        SinkOutcome::Local(AppError::NotFound(_)) => {
            match hold(db, |conn| queue::is_named(conn, id)) {
                Ok(false) => Outcome::Gone,
                Ok(true) => Outcome::Failed(TransferError::Local),
                Err(e) => return local(db, id, e, now),
            }
        }
        SinkOutcome::Local(e) => return local(db, id, e, now),
    };
    if let Outcome::Failed(error) = outcome {
        log::debug!("[SYNC] Blob {blob} download failed: {}", error.as_str());
    }
    record(db, id, outcome, now)
}

/// A download completed: `Done` while a syncing row names the blob; else its row went during the
/// download, so the blob is deleted again (unless a replace holds it) and the entry dropped.
fn stored<D: ConnAccess>(db: &D, blob_id: &str, now: i64) -> Outcome {
    let settled = hold(db, |conn| {
        if queue::is_named(conn, blob_id)? {
            queue::note_done(conn, blob_id)?;
            return Ok(Outcome::Done);
        }
        attachment_store::delete_if_unreferenced(conn, blob_id)?;
        queue::note_gone(conn, blob_id)?;
        Ok(Outcome::Gone)
    });
    match settled {
        Ok(outcome) => outcome,
        Err(e) => local(db, blob_id, e, now),
    }
}

/// The lane lock, waiting at most until `until`; `None` when the deadline passed first.
async fn enter(
    lane: &tokio::sync::Mutex<()>,
    until: Option<Instant>,
) -> Option<tokio::sync::MutexGuard<'_, ()>> {
    match until {
        Some(at) => tokio::time::timeout_at(at, lane.lock()).await.ok(),
        None => Some(lane.lock().await),
    }
}

/// The queue brought in line with the rows (`queue::reconcile_rows`: only newly named blobs are
/// looked up in the store; the full reconcile is the lane start's and a merging pull's).
fn reconciled_rows<D: ConnAccess>(db: &D, now: i64) -> Result<Reconciled> {
    hold(db, |conn| queue::reconcile_rows(conn, now))
}

/// Run by the holder of `state`'s lane before it picks work: entries left active by a dropped
/// transfer count a failed attempt (`queue::fail_abandoned`); then whether content waits to move
/// at all and to upload. Whether anything was abandoned comes first.
fn lane_start<D: ConnAccess>(db: &D, state: BlobState, now: i64) -> Result<(bool, bool, bool)> {
    hold(db, |conn| {
        Ok((
            queue::fail_abandoned(conn, state, now)? > 0,
            queue::waiting(conn, false)?,
            queue::waiting(conn, true)?,
        ))
    })
}

/// Whether the run may go on, by the server's blob routes: an answer "no" shows that the server
/// needs an update (callers ask only while content waits); a server that could not be asked
/// stops this run and shows nothing.
async fn server_ready<R: Relay>(relay: &R, policy: &BlobPolicy, now: i64) -> bool {
    match policy.server_has_blobs(relay, now).await {
        ServerBlobs::Yes => true,
        ServerBlobs::No => {
            policy.note_server(false, now);
            false
        }
        ServerBlobs::Unknown => false,
    }
}

/// An entry marked active for its transfer. A transfer dropped before it settled (cut by the
/// round's budget, or its engine stopped) records a failed attempt of the network's kind if the
/// connection is free, so its backoff applies and the next run does not start it again at once;
/// if the connection is busy, the next run of its lane finds the entry still active and records
/// it then (`lane_start`).
struct Active<'a, D: ConnAccess> {
    db: &'a D,
    blob_id: String,
    /// The run's time (unix seconds) when the transfer was taken, and the same moment on the
    /// runtime's clock: a drop dates its failure from the drop, so the backoff runs from there.
    now: i64,
    taken: Instant,
    armed: bool,
}

impl<D: ConnAccess> Drop for Active<'_, D> {
    fn drop(&mut self) {
        if self.armed {
            let ran = i64::try_from(self.taken.elapsed().as_secs()).unwrap_or(i64::MAX);
            let (id, now) = (self.blob_id.as_str(), self.now.saturating_add(ran));
            log::debug!("[SYNC] A transfer of blob {id} was dropped before it ended");
            let _ = self
                .db
                .try_with_conn(|conn| queue::note_failed(conn, id, TransferError::Network, now));
        }
    }
}

/// The next transfer `next` finds, marked active in the same hold.
fn take<'a, D: ConnAccess>(
    db: &'a D,
    now: i64,
    next: impl FnOnce(&Connection) -> Result<Option<Transfer>>,
) -> Result<Option<(Transfer, Active<'a, D>)>> {
    let transfer = hold(db, |conn| {
        let transfer = next(conn)?;
        if let Some(transfer) = &transfer {
            queue::set_active(conn, &transfer.blob_id, true)?;
        }
        Ok(transfer)
    })?;
    Ok(transfer.map(|transfer| {
        let active = Active {
            db,
            blob_id: transfer.blob_id.clone(),
            now,
            taken: Instant::now(),
            armed: true,
        };
        (transfer, active)
    }))
}

/// Whether an outcome ends the run: the network's or the server's trouble, or its refusal of this
/// vault, would meet the next blob as well. (A refusal or a conflict of one blob does not.)
fn stops(outcome: Outcome) -> bool {
    matches!(
        outcome,
        Outcome::Failed(TransferError::Network | TransferError::Server | TransferError::NoRoom)
            | Outcome::Blocked
            | Outcome::Locked
    )
}

/// Uploads the blobs that are due at `now`, one at a time, oldest first (`queue::next_upload`),
/// each of at most `max_plain` bytes when given, until none is due, `until` passed (checked
/// before each blob and while waiting for the lane), or the next blob would meet the same
/// trouble: no room (uploads pause, the policy shows it), passing trouble (the rest waits for the
/// next run) or a refusal of this vault (`Blocked`). The queue is reconciled by its rows first,
/// and entries a dropped upload left active count a failed attempt. Nothing runs while the server
/// lacks the blob routes (the policy then shows that, as long as content waits), while it cannot
/// be asked, or while uploads are paused. A transfer error is never an `Err`: only a local error
/// of the queue is, and a locked profile (`AppError::Auth`).
pub async fn run_uploads<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    now: i64,
    until: Option<Instant>,
    max_plain: Option<u64>,
) -> Result<TransferReport> {
    let Some(_lane) = enter(policy.up_lane(), until).await else {
        return Ok(TransferReport::default());
    };
    let reconciled = reconciled_rows(db, now)?;
    let pick = Pick {
        max_plain,
        untried_only: false,
    };
    uploads_in_lane(relay, db, creds, policy, now, until, pick, reconciled).await
}

/// Which uploads a run takes: of at most `max_plain` bytes when given, and only those that never
/// failed an attempt with `untried_only`.
#[derive(Clone, Copy)]
struct Pick {
    max_plain: Option<u64>,
    untried_only: bool,
}

/// The sync round's upload step: the queue is reconciled by its rows (so content added here is
/// queued and counted even when the step does nothing else), then `run_uploads` runs only if the
/// upload lane is free right now (`None` when another transfer holds it), and only over content
/// that never failed an attempt (`queue::next_untried_upload`): what failed once, a cut by the
/// round's budget included, is left to the lanes.
pub async fn run_uploads_if_free<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    now: i64,
    until: Option<Instant>,
    max_plain: Option<u64>,
) -> Result<Option<TransferReport>> {
    let reconciled = reconciled_rows(db, now)?;
    let Ok(_lane) = policy.up_lane().try_lock() else {
        return Ok(None);
    };
    let pick = Pick {
        max_plain,
        untried_only: true,
    };
    uploads_in_lane(relay, db, creds, policy, now, until, pick, reconciled)
        .await
        .map(Some)
}

/// `run_uploads` once the upload lane is held and the queue reconciled.
#[allow(clippy::too_many_arguments)]
async fn uploads_in_lane<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    now: i64,
    until: Option<Instant>,
    pick: Pick,
    reconciled: Reconciled,
) -> Result<TransferReport> {
    let (abandoned, any, up) = lane_start(db, BlobState::Up, now)?;
    let mut report = TransferReport {
        changed: abandoned || reconciled != Reconciled::default(),
        ..TransferReport::default()
    };
    if !any {
        policy.clear(SyncAttachmentsProblem::ServerNeedsUpdate);
    }
    if !up {
        policy.clear(SyncAttachmentsProblem::NoRoom);
        return Ok(report);
    }
    if !server_ready(relay, policy, now).await || policy.uploads_paused(now) {
        return Ok(report);
    }
    loop {
        if until.is_some_and(|at| Instant::now() >= at) {
            break;
        }
        let Some((transfer, mut active)) = take(db, now, |conn| {
            if pick.untried_only {
                queue::next_untried_upload(conn, now, pick.max_plain)
            } else {
                queue::next_upload(conn, now, pick.max_plain)
            }
        })?
        else {
            break;
        };
        let outcome = upload_one(relay, db, creds, &transfer, now).await;
        active.armed = false;
        report.count(outcome, true);
        match outcome {
            // Only an accepted body says there is room; a blob the server had stored nothing.
            Outcome::Done => policy.note_uploaded(),
            Outcome::Failed(TransferError::NoRoom) => {
                policy.note_no_room(now);
                break;
            }
            Outcome::Locked => return Err(locked()),
            // An upload is never busy; were it, the same entry would come back at once.
            Outcome::Busy => break,
            _ if stops(outcome) => break,
            _ => {}
        }
    }
    Ok(report)
}

/// Downloads the blobs that are due at `now`, one at a time (`queue::next_download`: wanted
/// first, then thumbnails, then the oldest row; full content only when wanted unless the policy
/// fetches eagerly), until none is due, `until` passed, passing trouble (the rest waits for the
/// next run) or the server refused this vault (`Blocked`). A blob another download of this
/// profile holds is passed over until the next run. The queue is reconciled by its rows first,
/// and entries a dropped download left active count a failed attempt; nothing runs while the
/// server lacks the blob routes or cannot be asked. A transfer error is never an `Err`:
/// only a local error of the queue is, and a locked profile (`AppError::Auth`).
pub async fn run_downloads<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    now: i64,
    until: Option<Instant>,
) -> Result<TransferReport> {
    let Some(_lane) = enter(policy.down_lane(), until).await else {
        return Ok(TransferReport::default());
    };
    let reconciled = reconciled_rows(db, now)?;
    let (abandoned, any, _) = lane_start(db, BlobState::Down, now)?;
    let mut report = TransferReport {
        changed: abandoned || reconciled != Reconciled::default(),
        ..TransferReport::default()
    };
    if !any {
        policy.clear(SyncAttachmentsProblem::ServerNeedsUpdate);
        return Ok(report);
    }
    if !server_ready(relay, policy, now).await {
        return Ok(report);
    }
    let eager_full = policy.eager_full();
    let mut busy = Vec::new();
    loop {
        if until.is_some_and(|at| Instant::now() >= at) {
            break;
        }
        let Some((transfer, mut active)) = take(db, now, |conn| {
            queue::next_download_skipping(conn, now, eager_full, &busy)
        })?
        else {
            break;
        };
        let outcome = download_one(relay, db, creds, &transfer, now).await;
        active.armed = false;
        report.count(outcome, false);
        match outcome {
            Outcome::Locked => return Err(locked()),
            Outcome::Busy => busy.push(transfer.blob_id),
            _ if stops(outcome) => break,
            _ => {}
        }
    }
    Ok(report)
}
