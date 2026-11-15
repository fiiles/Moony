//! Blob streams between the attachment store and the wire (attachments design §5.2), one chunk at
//! a time so a 100 MiB document never sits in memory.
//!
//! `UploadSource` reads a complete blob from the store a chunk at a time and seals it: it yields
//! the stream header, then one sealed chunk per item, as the body of a `put_blob`. `StoreSink`
//! takes a `get_blob` body piece by piece, opens it and writes the plaintext into an incoming blob
//! (`attachment_store::begin_incoming`), which no reader sees until the whole stream has
//! authenticated and its hash is recorded. Decrypted bytes go nowhere but `attachments.db`. One
//! download per blob ID runs at a time (`IncomingBlobs`): the store's incoming-blob calls find the
//! row by ID, so a second transfer of the same ID would write into the first one's row.
//!
//! Each read and each write is one short `ConnAccess::with_conn` hold; none is open while the
//! network is read or written. The holds are taken from synchronous callbacks the HTTP client
//! drives on an async worker (the body iterator, the sink), so each one runs through
//! [`blocking`]: on a multi-threaded runtime the worker hands its other tasks to another thread
//! while this one waits for the connection, and back-pressure stays the plain one of a callback
//! that returns when its chunk is stored. Two holds are long for a large file and accepted:
//! starting a download reserves the whole blob (`zeroblob`, which writes every page of it), and
//! abandoning one frees every page again; for a 100 MiB file each holds the database for as long
//! as allocating or freeing 100 MiB takes.

use std::collections::BTreeSet;
use std::io;
use std::sync::{Mutex, PoisonError};

use sha2::{Digest, Sha256};
use tokio::runtime::{Handle, RuntimeFlavor};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::services::attachment_store;
use crate::services::sync::engine::ConnAccess;
use crate::services::sync::envelope::blob::{sealed_len, BlobOpener, BlobSealer};
use crate::services::sync::envelope::stream::CHUNK;
use crate::services::sync::keys::VaultKeys;
use crate::services::sync::relay::{BlobBody, BlobSink, Delivery, RelayResult, SinkFlow};

/// Runs blocking store work from a synchronous callback of an async transfer. On a
/// multi-threaded runtime it goes through `block_in_place`, so the worker's other tasks move to
/// another thread while this one waits for the connection or the disk; on a current-thread
/// runtime (where that is not allowed) and outside a runtime it runs in place.
fn blocking<T>(f: impl FnOnce() -> T) -> T {
    match Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

/// One short hold of the connection, through [`blocking`].
pub(crate) fn hold<D: ConnAccess, T>(
    db: &D,
    f: impl FnOnce(&rusqlite::Connection) -> Result<T>,
) -> Result<T> {
    blocking(|| db.with_conn(f))
}

/// The blob IDs a download is writing into one profile's store right now
/// (`ConnAccess::incoming_blobs`). A `StoreSink` claims its ID before it reserves the incoming
/// blob and releases it once it settled, so a second download of the same ID is refused instead
/// of replacing the first one's row under it. Uploads only read and need no claim.
#[derive(Debug, Default)]
pub struct IncomingBlobs {
    ids: Mutex<BTreeSet<String>>,
}

impl IncomingBlobs {
    pub const fn new() -> Self {
        Self {
            ids: Mutex::new(BTreeSet::new()),
        }
    }

    /// Claims `id`; false when a download of it is in progress.
    pub(crate) fn claim(&self, id: &str) -> bool {
        self.ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(id.to_string())
    }

    /// Frees `id` for the next download.
    pub(crate) fn release(&self, id: &str) {
        self.ids
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(id);
    }
}

/// A complete blob of the store on its way up, sealed as it is read: the header, then one sealed
/// chunk per item. The first item checks, in its own hold, that the blob is complete here at the
/// declared length; each chunk is read under its own hold into one reused buffer, and the
/// connection is never held between items.
///
/// An error ends it: the item is the error and nothing follows. A blob that is not complete here
/// (never was, or deleted under the upload) is `ErrorKind::NotFound`; a declared length other
/// than the stored size is `ErrorKind::InvalidData`, before anything is sent. Every error carries
/// the `AppError` it came from (a locked profile is `AppError::Auth`), for `get_ref()` and
/// `downcast_ref` to find.
pub struct UploadSource<D: ConnAccess> {
    db: D,
    blob_id: String,
    /// The server's name for the blob, for error messages.
    blob: Uuid,
    plain_len: u64,
    sealer: BlobSealer,
    /// Plaintext bytes read so far.
    offset: u64,
    /// The one plaintext chunk buffer, reused for every chunk.
    buffer: Zeroizing<Vec<u8>>,
    header_sent: bool,
    done: bool,
}

impl<D: ConnAccess> UploadSource<D> {
    /// The blob `blob_id` of the store, declared `plain_len` bytes long (checked against its
    /// recorded size before anything is sent), sealed by `sealer` (made for `blob`, the ID the
    /// server knows it by).
    pub fn new(db: D, blob_id: String, blob: Uuid, plain_len: u64, sealer: BlobSealer) -> Self {
        Self {
            db,
            blob_id,
            blob,
            plain_len,
            sealer,
            offset: 0,
            buffer: Zeroizing::new(vec![0u8; CHUNK]),
            header_sent: false,
            done: false,
        }
    }

    /// The length of everything the source yields, the body's `Content-Length`.
    pub fn sealed_len(&self) -> u64 {
        sealed_len(self.plain_len)
    }

    /// The source as a `put_blob` body.
    pub fn into_body(self) -> BlobBody {
        Box::new(self)
    }

    /// The header, once the blob is complete here at the declared length.
    fn header(&self) -> io::Result<Vec<u8>> {
        let (id, declared) = (self.blob_id.as_str(), self.plain_len);
        let stored = hold(&self.db, |conn| attachment_store::size_of(conn, id))
            .map_err(|e| self.io_error(e, io::ErrorKind::Other))?;
        match stored {
            Some(size) if size == declared => Ok(self.sealer.header().to_vec()),
            Some(size) => Err(self.io_error(
                AppError::Internal(format!(
                    "sync blob: declared {declared} bytes, the store holds {size}"
                )),
                io::ErrorKind::InvalidData,
            )),
            None => Err(self.io_error(
                AppError::NotFound("sync blob: the content is not complete on this device".into()),
                io::ErrorKind::NotFound,
            )),
        }
    }

    /// Reads and seals the next chunk: a full `CHUNK` while at least that much is left, else the
    /// rest (`0..CHUNK` bytes) as the last chunk, after which the source is done.
    fn next_chunk(&mut self) -> io::Result<Vec<u8>> {
        let left = self.plain_len - self.offset;
        let last = left < CHUNK as u64;
        // `left < CHUNK` when it is the last chunk, so the cast cannot truncate.
        let want = if last { left as usize } else { CHUNK };
        let (id, offset) = (self.blob_id.as_str(), self.offset);
        let buffer = &mut self.buffer[..want];
        hold(&self.db, |conn| read_exact_at(conn, id, offset, buffer))
            .map_err(|e| self.io_error(e, io::ErrorKind::Other))?;
        self.offset += want as u64;
        let sealed = self
            .sealer
            .seal_chunk(&self.buffer[..want], last)
            .map_err(|e| self.io_error(e, io::ErrorKind::Other))?;
        self.done = last;
        Ok(sealed)
    }

    /// `error` as an `io::Error` of `kind`, or `NotFound` for a blob that is gone.
    fn io_error(&self, error: AppError, kind: io::ErrorKind) -> io::Error {
        let kind = match error {
            AppError::NotFound(_) => io::ErrorKind::NotFound,
            _ => kind,
        };
        log::debug!("[SYNC] Blob {} upload stopped: {error}", self.blob);
        io::Error::new(kind, error)
    }
}

/// Fills `buffer` from the complete blob `id` at `offset`; a blob that ends sooner is an error.
fn read_exact_at(
    conn: &rusqlite::Connection,
    id: &str,
    offset: u64,
    buffer: &mut [u8],
) -> Result<()> {
    let mut filled = 0;
    // Runs at least once, so a blob that is gone fails even when nothing is left to read.
    loop {
        let n = attachment_store::read_at(conn, id, offset + filled as u64, &mut buffer[filled..])?;
        filled += n;
        if filled == buffer.len() {
            return Ok(());
        }
        if n == 0 {
            return Err(AppError::Internal(
                "sync blob: the stored content is shorter than its size".into(),
            ));
        }
    }
}

impl<D: ConnAccess> Iterator for UploadSource<D> {
    type Item = io::Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }
        let item = if self.header_sent {
            self.next_chunk()
        } else {
            self.header_sent = true;
            self.header()
        };
        if item.is_err() {
            self.done = true;
        }
        Some(item)
    }
}

/// How a download into the store ended.
#[derive(Debug)]
pub enum SinkOutcome {
    /// The whole stream authenticated and the blob is complete in the store under its ID.
    Stored,
    /// The blob is complete in the store already: nothing was downloaded or touched.
    AlreadyHere,
    /// Another download of the same blob ID is in progress in this profile: this one touched
    /// nothing.
    InProgress,
    /// The stream does not open: a length no stream has, a chunk that fails authentication (it
    /// was tampered with, or sealed for another blob or vault), or an epoch this device has no
    /// key for. Nothing is stored.
    Unreadable,
    /// The download failed or ended before the stream's last chunk: nothing is stored.
    Incomplete,
    /// The store failed; nothing is readable. `AppError::NotFound` means the incoming blob was
    /// removed under the transfer (no row names it any more, so a sweep took it): the content is
    /// gone, which is no failure to retry. `AppError::Auth` is a locked profile; the incoming row
    /// may then stay (invisible, replaced by the next download of the ID, swept at start).
    Local(AppError),
}

/// A downloaded blob on its way into the store: `begin` claims the ID and reserves an incoming
/// blob of the plaintext length, each piece is opened and every chunk it completes is written at
/// the running offset and hashed, and `finish` makes the blob visible once the download completed
/// and the opener's `finish` accepted the whole stream. Any failure says `Stop` and is remembered
/// for `finish`.
///
/// Plaintext reaches the store before the last chunk authenticated, but only into the incoming
/// blob, which every reader treats as absent; `finish` deletes it unless the stream checked out.
/// Dropping an unsettled sink (a cancelled transfer) abandons it only if the connection is free
/// that instant, never waiting; otherwise the invisible row stays until the next download of the
/// ID replaces it or the start-up sweep removes it.
///
/// Memory: one sealed chunk in the opener plus the plaintext of the piece being written.
pub struct StoreSink<D: ConnAccess> {
    db: D,
    blob_id: String,
    blob: Uuid,
    vault_id: [u8; 16],
    /// Until `begin` hands them to the opener.
    keys: Option<VaultKeys>,
    opener: Option<BlobOpener>,
    hasher: Sha256,
    /// Plaintext bytes written so far.
    written: u64,
    /// This sink holds its ID in `incoming_blobs` and must release it.
    claimed: bool,
    /// An incoming blob of this sink's is in the store: it must be finished or abandoned.
    reserved: bool,
    /// Why the sink stopped, for `finish`.
    failure: Option<SinkOutcome>,
}

impl<D: ConnAccess> StoreSink<D> {
    /// A sink for the blob the server knows as `blob`, stored under `blob_id`, opened with the
    /// vault keys of `vault_id`.
    pub fn new(db: D, blob_id: String, blob: Uuid, keys: VaultKeys, vault_id: [u8; 16]) -> Self {
        Self {
            db,
            blob_id,
            blob,
            vault_id,
            keys: Some(keys),
            opener: None,
            hasher: Sha256::new(),
            written: 0,
            claimed: false,
            reserved: false,
            failure: None,
        }
    }

    /// Settles the download with what `get_blob` answered: completes the blob only after a
    /// complete delivery whose whole stream authenticated; otherwise abandons it and says why
    /// (an error from `get_blob` is `Incomplete` unless the sink had stopped for its own reason).
    pub fn finish(mut self, delivery: &RelayResult<Delivery>) -> SinkOutcome {
        let outcome = match delivery {
            Ok(Delivery::Complete) => self.settle(),
            Ok(Delivery::Stopped) | Err(_) => {
                self.failure.take().unwrap_or(SinkOutcome::Incomplete)
            }
        };
        if !matches!(outcome, SinkOutcome::Stored) {
            self.abandon_reserved();
        }
        self.release();
        outcome
    }

    /// The outcome of a complete delivery.
    fn settle(&mut self) -> SinkOutcome {
        if let Some(failure) = self.failure.take() {
            return failure;
        }
        let Some(opener) = self.opener.take() else {
            // `begin` never came: nothing arrived.
            return SinkOutcome::Incomplete;
        };
        let plain_len = opener.plain_len();
        if opener.finish().is_err() || self.written != plain_len {
            return SinkOutcome::Incomplete;
        }
        let sha256 = hex::encode(std::mem::take(&mut self.hasher).finalize());
        let id = self.blob_id.as_str();
        match hold(&self.db, |conn| {
            attachment_store::finish_incoming(conn, id, &sha256)
        }) {
            Ok(()) => {
                self.reserved = false;
                SinkOutcome::Stored
            }
            Err(e) => SinkOutcome::Local(e),
        }
    }

    /// Deletes this sink's incoming blob, waiting for the connection. A failure leaves it
    /// incomplete, which no reader sees.
    fn abandon_reserved(&mut self) {
        if !self.reserved {
            return;
        }
        self.reserved = false;
        let id = self.blob_id.as_str();
        if let Err(e) = hold(&self.db, |conn| {
            attachment_store::abandon_incoming(conn, id)
        }) {
            log::warn!("[SYNC] An unfinished download could not be removed: {e}");
        }
    }

    /// Frees the ID for the next download, after this sink's row is settled.
    fn release(&mut self) {
        if self.claimed {
            self.claimed = false;
            self.db.incoming_blobs().release(&self.blob_id);
        }
    }

    fn stop(&mut self, why: SinkOutcome) -> SinkFlow {
        if let SinkOutcome::Local(e) = &why {
            log::debug!("[SYNC] Blob {} download stopped: {e}", self.blob);
        }
        self.failure.get_or_insert(why);
        self.opener = None;
        SinkFlow::Stop
    }
}

impl<D: ConnAccess> BlobSink for StoreSink<D> {
    fn begin(&mut self, sealed_len: u64) -> SinkFlow {
        if self.failure.is_some() {
            return SinkFlow::Stop;
        }
        let Some(keys) = self.keys.take() else {
            return self.stop(SinkOutcome::Local(AppError::Internal(
                "sync blob: a download began twice".into(),
            )));
        };
        if !self.db.incoming_blobs().claim(&self.blob_id) {
            return self.stop(SinkOutcome::InProgress);
        }
        self.claimed = true;
        let opener = match BlobOpener::new(keys, self.vault_id, self.blob, sealed_len) {
            Ok(opener) => opener,
            Err(_) => return self.stop(SinkOutcome::Unreadable),
        };
        let (id, plain_len) = (self.blob_id.as_str(), opener.plain_len());
        let reserved = hold(&self.db, |conn| {
            if attachment_store::is_complete(conn, id)? {
                return Ok(false);
            }
            attachment_store::begin_incoming(conn, id, plain_len)?;
            Ok(true)
        });
        match reserved {
            Ok(true) => {
                self.reserved = true;
                self.opener = Some(opener);
                SinkFlow::Continue
            }
            Ok(false) => self.stop(SinkOutcome::AlreadyHere),
            Err(e) => self.stop(SinkOutcome::Local(e)),
        }
    }

    fn piece(&mut self, bytes: &[u8]) -> SinkFlow {
        if self.failure.is_some() {
            return SinkFlow::Stop;
        }
        let Some(opener) = self.opener.as_mut() else {
            return self.stop(SinkOutcome::Local(AppError::Internal(
                "sync blob: a piece before the download began".into(),
            )));
        };
        let chunks = match opener.push(bytes) {
            Ok(chunks) => chunks,
            Err(_) => return self.stop(SinkOutcome::Unreadable),
        };
        for chunk in chunks {
            let (id, offset) = (self.blob_id.as_str(), self.written);
            if let Err(e) = hold(&self.db, |conn| {
                attachment_store::write_incoming(conn, id, offset, &chunk)
            }) {
                return self.stop(SinkOutcome::Local(e));
            }
            self.hasher.update(&chunk[..]);
            self.written += chunk.len() as u64;
        }
        SinkFlow::Continue
    }
}

impl<D: ConnAccess> Drop for StoreSink<D> {
    /// An unsettled sink (a cancelled transfer): abandons its incoming blob if the connection is
    /// free this instant, never waiting for it, so a drop inside a hold of the same connection
    /// cannot hang. The ID is released either way: a row left behind is invisible, and the next
    /// download of the ID replaces it.
    fn drop(&mut self) {
        if self.reserved {
            self.reserved = false;
            let id = self.blob_id.as_str();
            match self
                .db
                .try_with_conn(|conn| attachment_store::abandon_incoming(conn, id))
            {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    log::debug!("[SYNC] A cancelled download of {id} stays incomplete: {e}")
                }
                None => log::debug!(
                    "[SYNC] A cancelled download of {id} stays incomplete: the connection is busy"
                ),
            }
        }
        self.release();
    }
}
