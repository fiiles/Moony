//! Blob streams between the attachment store and the wire (attachments design §5.2), one chunk at
//! a time so a 100 MiB document never sits in memory.
//!
//! `UploadSource` reads a complete blob from the store a chunk at a time and seals it: it yields
//! the stream header, then one sealed chunk per item, as the body of a `put_blob`. `StoreSink`
//! takes a `get_blob` body piece by piece, opens it and writes the plaintext into an incoming blob
//! (`attachment_store::begin_incoming`), which no reader sees until the whole stream has
//! authenticated and its hash is recorded. Decrypted bytes go nowhere but `attachments.db`.
//!
//! Each read and each write is one short `ConnAccess::with_conn` hold; none is open while the
//! network is read or written.

use std::io;

use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::error::AppError;
use crate::services::attachment_store;
use crate::services::sync::engine::ConnAccess;
use crate::services::sync::envelope::blob::{sealed_len, BlobOpener, BlobSealer};
use crate::services::sync::envelope::stream::CHUNK;
use crate::services::sync::keys::VaultKeys;
use crate::services::sync::relay::{BlobBody, BlobSink, SinkFlow};

/// A complete blob of the store on its way up, sealed as it is read: the header, then one sealed
/// chunk per item. Each chunk is read under its own `with_conn` into one reused buffer, and the
/// connection is never held between items.
///
/// An error ends it: the item is the error and nothing follows. A blob deleted under the upload
/// is `ErrorKind::NotFound`; every error carries the `AppError` it came from (a locked profile is
/// `AppError::Auth`), for `get_ref()` and `downcast_ref` to find.
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
    /// The blob `blob_id` of the store, `plain_len` bytes long (its recorded size), sealed by
    /// `sealer` (made for `blob`, the ID the server knows it by).
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

    /// Reads and seals the next chunk: a full `CHUNK` while at least that much is left, else the
    /// rest (`0..CHUNK` bytes) as the last chunk, after which the source is done.
    fn next_chunk(&mut self) -> io::Result<Vec<u8>> {
        let left = self.plain_len - self.offset;
        let last = left < CHUNK as u64;
        // `left < CHUNK` when it is the last chunk, so the cast cannot truncate.
        let want = if last { left as usize } else { CHUNK };
        let (db, id, offset) = (&self.db, self.blob_id.as_str(), self.offset);
        let buffer = &mut self.buffer[..want];
        db.with_conn(|conn| read_exact_at(conn, id, offset, buffer))
            .map_err(|e| self.io_error(e))?;
        self.offset += want as u64;
        let sealed = self
            .sealer
            .seal_chunk(&self.buffer[..want], last)
            .map_err(|e| self.io_error(e))?;
        self.done = last;
        Ok(sealed)
    }

    fn io_error(&self, error: AppError) -> io::Error {
        let kind = match error {
            AppError::NotFound(_) => io::ErrorKind::NotFound,
            _ => io::ErrorKind::Other,
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
) -> crate::error::Result<()> {
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
        if !self.header_sent {
            self.header_sent = true;
            return Some(Ok(self.sealer.header().to_vec()));
        }
        let chunk = self.next_chunk();
        if chunk.is_err() {
            self.done = true;
        }
        Some(chunk)
    }
}

/// How a download into the store ended.
#[derive(Debug)]
pub enum SinkOutcome {
    /// The whole stream authenticated and the blob is complete in the store under its ID.
    Stored,
    /// The stream does not open: a length no stream has, a chunk that fails authentication (it
    /// was tampered with, or sealed for another blob or vault), or an epoch this device has no
    /// key for. Nothing is stored.
    Unreadable,
    /// The stream ended before its last chunk: nothing is stored.
    Incomplete,
    /// The store failed; nothing is stored. `AppError::NotFound` means the incoming blob was
    /// removed under the transfer (no row names it any more, so a sweep took it): the content is
    /// gone, which is no failure to retry. `AppError::Auth` is a locked profile.
    Local(AppError),
}

/// A downloaded blob on its way into the store: `begin` reserves an incoming blob of the plaintext
/// length, each piece is opened and every chunk it completes is written at the running offset and
/// hashed, and `finish` makes the blob visible once the opener's `finish` accepted the whole
/// stream. Any failure says `Stop` and is remembered for `finish`.
///
/// Plaintext reaches the store before the last chunk authenticated, but only into the incoming
/// blob, which every reader treats as absent; `finish` deletes it unless the stream checked out.
/// When the download itself fails (`get_blob` answers an error), call `abandon`, even if every
/// byte arrived. Dropping an unsettled sink abandons too (a cancelled transfer), with one short
/// hold of the connection, so never drop it inside `with_conn`.
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
    /// An incoming blob was reserved and is not settled yet: it must be finished or abandoned.
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
            reserved: false,
            failure: None,
        }
    }

    /// Completes the blob when the whole stream came and authenticated, else abandons it and says
    /// why. Call it when `get_blob` answered `Delivery::Complete` or `Delivery::Stopped`.
    pub fn finish(mut self) -> SinkOutcome {
        let outcome = self.settle();
        if !matches!(outcome, SinkOutcome::Stored) {
            self.abandon_reserved();
        }
        outcome
    }

    /// Deletes what the sink stored: the download failed, so nothing of it may stay.
    pub fn abandon(mut self) {
        self.abandon_reserved();
    }

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
        match self
            .db
            .with_conn(|conn| attachment_store::finish_incoming(conn, id, &sha256))
        {
            Ok(()) => {
                self.reserved = false;
                SinkOutcome::Stored
            }
            Err(e) => SinkOutcome::Local(e),
        }
    }

    /// Deletes the incoming blob when one is reserved. A failure leaves it incomplete, which no
    /// reader sees and the start-up sweep removes.
    fn abandon_reserved(&mut self) {
        if !self.reserved {
            return;
        }
        self.reserved = false;
        let id = self.blob_id.as_str();
        if let Err(e) = self
            .db
            .with_conn(|conn| attachment_store::abandon_incoming(conn, id))
        {
            log::warn!("[SYNC] An unfinished download could not be removed: {e}");
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
        let opener = match BlobOpener::new(keys, self.vault_id, self.blob, sealed_len) {
            Ok(opener) => opener,
            Err(_) => return self.stop(SinkOutcome::Unreadable),
        };
        let (id, plain_len) = (self.blob_id.as_str(), opener.plain_len());
        if let Err(e) = self
            .db
            .with_conn(|conn| attachment_store::begin_incoming(conn, id, plain_len))
        {
            return self.stop(SinkOutcome::Local(e));
        }
        self.reserved = true;
        self.opener = Some(opener);
        SinkFlow::Continue
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
            if let Err(e) = self
                .db
                .with_conn(|conn| attachment_store::write_incoming(conn, id, offset, &chunk))
            {
                return self.stop(SinkOutcome::Local(e));
            }
            self.hasher.update(&chunk[..]);
            self.written += chunk.len() as u64;
        }
        SinkFlow::Continue
    }
}

impl<D: ConnAccess> Drop for StoreSink<D> {
    fn drop(&mut self) {
        self.abandon_reserved();
    }
}
