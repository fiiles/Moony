//! Moving document and photo content between this device's store and the sync server, one blob
//! at a time (attachments design §5.3, §5.4; ADR 0030).
//!
//! `upload_one` and `download_one` move one queued blob and record how it went in the queue
//! (`sync_blobs`); `run_uploads` and `run_downloads` take the queue one blob at a time under a lane
//! lock of the shared [`BlobPolicy`], so one upload and one download run at once, and stop between
//! two blobs when nothing is due, the caller's deadline passed, or the server refuses this vault.
//!
//! Blob errors are classified here and never reach the sync round's error path
//! (`services/sync/error.rs`): a blob that does not move never fails, pauses or slows a round.
//! A transfer ends as one of [`Outcome`]: done; gone (its row or its content no longer exists: the
//! entry is dropped); failed, recorded on the entry with a backoff of that blob's own (passing
//! trouble: the network, a busy or failing server, `429`; no room on the server, which also
//! pauses uploads through the policy; an unreadable download, content the server does not have
//! yet, an invalid ID, a local store error); blocked by what the round already shows (payment,
//! token, vault: nothing is recorded); or a locked profile.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::{Connection, OptionalExtension};
use tokio::time::Instant;

use super::canonical_id;
use super::policy::BlobPolicy;
use super::queue::{self, Reconciled, Transfer, TransferError};
use super::stream::{SinkOutcome, StoreSink, UploadSource};
use crate::error::{AppError, Result};
use crate::models::sync::SyncAttachmentsProblem;
use crate::services::attachment_store::{self, complete_sql};
use crate::services::sync::engine::ConnAccess;
use crate::services::sync::envelope::blob::BlobSealer;
use crate::services::sync::keys::Credentials;
use crate::services::sync::relay::{Relay, RelayError};

/// How one blob's transfer ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The server has the blob (an upload), or it is complete in the store (a download).
    Done,
    /// No syncing row names the blob any more, or its content left the store under an upload: the
    /// entry is dropped, nothing is retried and nothing is left in the store.
    Gone,
    /// Recorded on the entry (`last_error`) with that error's backoff.
    Failed(TransferError),
    /// The server refuses this vault (payment, token, vault gone): the round shows why; nothing is
    /// recorded and the run stops.
    Blocked,
    /// The profile is locked: nothing more can be read or recorded.
    Locked,
    /// Another download of the same blob is running here: skipped for now, nothing recorded.
    Busy,
}

/// What a run did: blobs uploaded (confirmed by the server) and stored here, transfers that
/// failed, and whether anything the person sees changed (the queue's reconcile included).
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
            Outcome::Done if upload => self.uploaded += 1,
            Outcome::Done => self.stored += 1,
            Outcome::Failed(_) => self.failed += 1,
            Outcome::Gone => {}
            Outcome::Blocked | Outcome::Locked | Outcome::Busy => return,
        }
        self.changed = true;
    }
}

fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

fn locked() -> AppError {
    AppError::Auth("Database is locked".into())
}

/// The transfer class of a sync server answer to a blob call. Passing trouble (`Network`, a
/// server error, `429`, an answer outside the contract, a refused body) is retried later per
/// blob; the round's own classes (`SyncError`) are never consulted.
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
        RelayError::PaymentRequired
        | RelayError::Unauthorized
        | RelayError::TokenReadOnly
        | RelayError::VaultNotFound => Outcome::Blocked,
        _ => Outcome::Failed(TransferError::Server),
    }
}

/// Records `outcome` of `blob_id`'s transfer in the queue (one hold) and returns it: done, gone
/// (the entry goes), failed (one more attempt and its backoff), else the entry only stops being
/// active. A locked profile is `Locked`; another local error is logged and `Failed(Local)`.
fn record<D: ConnAccess>(db: &D, blob_id: &str, outcome: Outcome, now: i64) -> Outcome {
    let recorded = db.with_conn(|conn| match outcome {
        Outcome::Done => queue::note_done(conn, blob_id),
        Outcome::Gone => queue::note_gone(conn, blob_id),
        Outcome::Failed(error) => queue::note_failed(conn, blob_id, error, now),
        Outcome::Blocked | Outcome::Locked | Outcome::Busy => {
            queue::set_active(conn, blob_id, false)
        }
    });
    match recorded {
        Ok(()) => outcome,
        Err(AppError::Auth(_)) => Outcome::Locked,
        Err(e) => {
            log::warn!("[SYNC] A document or photo transfer could not be recorded: {e}");
            Outcome::Failed(TransferError::Local)
        }
    }
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
/// (else `Gone`), asks the server whether it has it (`HEAD`: then `Done` without a body), and
/// otherwise streams it up sealed under the current key epoch. A blob ID that is not canonical
/// fails as `InvalidId` before any request or store read. A local error of the upload's source
/// (the content left the store, a locked profile, a read error) is recovered beside the call, so
/// it is never mistaken for the network trouble the HTTP client reports it as. The outcome is
/// recorded in the queue.
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
    let size = match db.with_conn(|conn| named_and_held(conn, id)) {
        Ok(Some(size)) => size,
        Ok(None) => return record(db, id, Outcome::Gone, now),
        Err(e) => return local(db, id, e, now),
    };
    let auth = creds.auth();
    match relay.head_blob(&auth, blob).await {
        Ok(Some(_)) => return record(db, id, Outcome::Done, now),
        Ok(None) => {}
        Err(e) => return record(db, id, classify(&e), now),
    }
    let sealer = BlobSealer::new(
        creds.keys.current_epoch(),
        creds.keys.current(),
        &creds.vault_id.0,
        &blob,
    );
    let declared = transfer.size.unwrap_or(size);
    let source = UploadSource::new(db.clone(), id.to_string(), blob, declared, sealer);
    let sealed_len = source.sealed_len();
    let cause = Arc::new(Mutex::new(None));
    let body = Box::new(Watched {
        body: source,
        cause: Arc::clone(&cause),
    });
    let sent = relay.put_blob(&auth, blob, sealed_len, body).await;
    let source_failure = cause.lock().unwrap_or_else(PoisonError::into_inner).take();
    let outcome = match (sent, source_failure) {
        (Ok(()), _) => Outcome::Done,
        (Err(_), Some(failure)) => failure,
        (Err(e), None) => classify(&e),
    };
    record(db, id, outcome, now)
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

/// Downloads one blob into the store: a blob no syncing row names any more is `Gone`, one already
/// complete here `Done`, without a request; otherwise its stream is opened piece by piece into an
/// incoming blob that becomes visible only once the whole stream authenticated. A stream that
/// does not open or ends early is `Failed(Unreadable)` and nothing of it stays. A row that went
/// during the download leaves no blob behind: an incoming blob a sweep removed ends the transfer
/// as gone, and a blob that completed after its row went is deleted again. The outcome is recorded
/// in the queue.
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
    let before = db.with_conn(|conn| {
        let named = queue::is_named(conn, id)?;
        let here: bool = conn.query_row(&format!("SELECT {}", complete_sql("?1")), [id], |r| {
            r.get(0)
        })?;
        Ok((named, here))
    });
    match before {
        Ok((false, _)) => return record(db, id, Outcome::Gone, now),
        Ok((true, true)) => return record(db, id, Outcome::Done, now),
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
        SinkOutcome::AlreadyHere => Outcome::Done,
        SinkOutcome::InProgress => Outcome::Busy,
        SinkOutcome::Unreadable => Outcome::Failed(TransferError::Unreadable),
        SinkOutcome::Incomplete => match &delivery {
            Err(e) => classify(e),
            Ok(_) => Outcome::Failed(TransferError::Unreadable),
        },
        SinkOutcome::Local(AppError::NotFound(_)) => {
            match db.with_conn(|conn| queue::is_named(conn, id)) {
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
    let settled = db.with_conn(|conn| {
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

/// The queue brought in line with the rows and the store, and whether content waits to move at
/// all and to upload.
fn reconciled<D: ConnAccess>(db: &D, now: i64) -> Result<(Reconciled, bool, bool)> {
    db.with_conn(|conn| {
        Ok((
            queue::reconcile(conn, now)?,
            queue::waiting(conn, false)?,
            queue::waiting(conn, true)?,
        ))
    })
}

/// An entry marked active for its transfer; a transfer dropped before it settled (a cancelled
/// run) clears the mark if the connection is free, so the entry is not skipped until the lanes
/// restart.
struct Active<'a, D: ConnAccess> {
    db: &'a D,
    blob_id: String,
    armed: bool,
}

impl<D: ConnAccess> Drop for Active<'_, D> {
    fn drop(&mut self) {
        if self.armed {
            let id = self.blob_id.as_str();
            let _ = self
                .db
                .try_with_conn(|conn| queue::set_active(conn, id, false));
        }
    }
}

/// The next transfer `next` finds, marked active in the same hold.
fn take<'a, D: ConnAccess>(
    db: &'a D,
    next: impl FnOnce(&Connection) -> Result<Option<Transfer>>,
) -> Result<Option<(Transfer, Active<'a, D>)>> {
    let transfer = db.with_conn(|conn| {
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
            armed: true,
        };
        (transfer, active)
    }))
}

/// Uploads the blobs that are due, one at a time, oldest first (`queue::next_upload`), each of at
/// most `max_plain` bytes when given, until none is due, `until` passed (checked before each
/// blob and while waiting for the lane), or the server refused: no room (uploads pause, the
/// policy shows it), passing trouble (the rest waits for the next run) or this vault (`Blocked`).
/// The queue is reconciled first. Nothing runs while the server lacks the blob routes (the policy
/// then shows that, as long as content waits) or while uploads are paused. A transfer error is
/// never an `Err`: only a local error of the queue is, and a locked profile (`AppError::Auth`).
pub async fn run_uploads<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    until: Option<Instant>,
    max_plain: Option<u64>,
) -> Result<TransferReport> {
    let Some(_lane) = enter(policy.up_lane(), until).await else {
        return Ok(TransferReport::default());
    };
    let now = unix_now();
    let (reconciled, any, up) = reconciled(db, now)?;
    let mut report = TransferReport {
        changed: reconciled != Reconciled::default(),
        ..TransferReport::default()
    };
    if !any {
        policy.clear(SyncAttachmentsProblem::ServerNeedsUpdate);
    }
    if !up {
        policy.clear(SyncAttachmentsProblem::NoRoom);
        return Ok(report);
    }
    if !policy.server_has_blobs(relay, now).await {
        policy.note_server(false, now);
        return Ok(report);
    }
    if policy.uploads_paused(now) {
        return Ok(report);
    }
    loop {
        if until.is_some_and(|at| Instant::now() >= at) {
            break;
        }
        let now = unix_now();
        let Some((transfer, mut active)) =
            take(db, |conn| queue::next_upload(conn, now, max_plain))?
        else {
            break;
        };
        let outcome = upload_one(relay, db, creds, &transfer, now).await;
        active.armed = false;
        report.count(outcome, true);
        match outcome {
            Outcome::Done => policy.note_uploaded(),
            Outcome::Failed(TransferError::NoRoom) => {
                policy.note_no_room(now);
                break;
            }
            Outcome::Locked => return Err(locked()),
            _ if stops(outcome) => break,
            _ => {}
        }
    }
    Ok(report)
}

/// Whether an outcome ends the run: the network's or the server's trouble, or its refusal of this
/// vault, would meet the next blob as well, and a blob another download is fetching would be
/// taken again at once.
fn stops(outcome: Outcome) -> bool {
    matches!(
        outcome,
        Outcome::Failed(TransferError::Network | TransferError::Server | TransferError::NoRoom)
            | Outcome::Blocked
            | Outcome::Busy
            | Outcome::Locked
    )
}

/// Downloads the blobs that are due, one at a time (`queue::next_download`: wanted first, then
/// thumbnails, then the oldest row; full content only when wanted unless the policy fetches
/// eagerly), until none is due, `until` passed, passing trouble (the rest waits for the next run),
/// the server refused this vault (`Blocked`), or another download of the next blob runs here
/// (`Busy`). The queue is reconciled first; nothing runs while the server lacks the blob routes.
/// A transfer error is never an `Err`: only a local error of the queue is, and a locked profile
/// (`AppError::Auth`).
pub async fn run_downloads<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    until: Option<Instant>,
) -> Result<TransferReport> {
    let Some(_lane) = enter(policy.down_lane(), until).await else {
        return Ok(TransferReport::default());
    };
    let now = unix_now();
    let (reconciled, any, _) = reconciled(db, now)?;
    let mut report = TransferReport {
        changed: reconciled != Reconciled::default(),
        ..TransferReport::default()
    };
    if !any {
        policy.clear(SyncAttachmentsProblem::ServerNeedsUpdate);
        return Ok(report);
    }
    if !policy.server_has_blobs(relay, now).await {
        policy.note_server(false, now);
        return Ok(report);
    }
    let eager_full = policy.eager_full();
    loop {
        if until.is_some_and(|at| Instant::now() >= at) {
            break;
        }
        let now = unix_now();
        let Some((transfer, mut active)) =
            take(db, |conn| queue::next_download(conn, now, eager_full))?
        else {
            break;
        };
        let outcome = download_one(relay, db, creds, &transfer, now).await;
        active.armed = false;
        report.count(outcome, false);
        match outcome {
            Outcome::Locked => return Err(locked()),
            _ if stops(outcome) => break,
            _ => {}
        }
    }
    Ok(report)
}
