//! Blob streams (attachments design §5.2): the content of one document, photo or thumbnail on
//! the sync server, sealed and opened one chunk at a time so a 100 MiB file never sits in memory.
//!
//! ```text
//! blob      = the snapshot STREAM (stream.rs), byte for byte, with
//! key       = HKDF-SHA256(vault_key[epoch], salt, "moony sync blob v1")   (one per stream)
//! object ID = the blob ID, 16 raw UUID bytes
//! ```
//!
//! A blob stays sealed under the epoch it was uploaded with and names it in its header. The
//! fixed vector in the tests is the contract with every other device.

use rand::Rng;
use uuid::Uuid;
use zeroize::Zeroizing;

use super::stream::{
    chunk_nonce, stream_aad, stream_header, CHUNK, HEADER_LEN, PREFIX, SALT, SEALED_CHUNK, TAG,
};
use super::{aead_open, aead_seal, hkdf32, OpenError, BLOB_INFO, MAGIC};
use crate::error::{AppError, Result};
use crate::services::sync::keys::VaultKeys;

/// The largest plaintext a blob carries: the document limit, 100 MiB.
pub const MAX_BLOB_PLAIN: u64 = 104_857_600;
/// The largest sealed blob, the sync server's per-blob limit.
pub const MAX_BLOB_SEALED: u64 = sealed_len(MAX_BLOB_PLAIN);

const CHUNK_64: u64 = CHUNK as u64;
const SEALED_CHUNK_64: u64 = SEALED_CHUNK as u64;
const TAG_64: u64 = TAG as u64;
const HEADER_64: u64 = HEADER_LEN as u64;

/// The sealed length of `plain_len` bytes: the header, every full chunk with its tag, then the
/// last chunk (`0..CHUNK` bytes) with its tag. Saturates instead of overflowing.
pub const fn sealed_len(plain_len: u64) -> u64 {
    HEADER_64
        .saturating_add((plain_len / CHUNK_64).saturating_mul(SEALED_CHUNK_64))
        .saturating_add(plain_len % CHUNK_64)
        .saturating_add(TAG_64)
}

/// The plaintext length of a sealed stream of `sealed_len` bytes; `None` when the length leaves
/// no room for a header and a last chunk with its tag.
pub const fn plain_len(sealed_len: u64) -> Option<u64> {
    if sealed_len < HEADER_64 + TAG_64 {
        return None;
    }
    let body = sealed_len - HEADER_64;
    let last = body % SEALED_CHUNK_64;
    if last < TAG_64 {
        return None;
    }
    Some((body / SEALED_CHUNK_64) * CHUNK_64 + last - TAG_64)
}

/// The object ID a blob is sealed for: the 16 raw bytes of its UUID.
pub fn blob_object_id(blob: &Uuid) -> [u8; 16] {
    *blob.as_bytes()
}

/// Seals one blob chunk by chunk: the caller writes `header()`, then every full chunk, then the
/// last one.
pub struct BlobSealer {
    key: Zeroizing<[u8; 32]>,
    header: [u8; HEADER_LEN],
    prefix: [u8; PREFIX],
    aad: Vec<u8>,
    /// The counter of the next chunk; `None` once the last chunk is sealed or no counter is left.
    counter: Option<u32>,
}

impl BlobSealer {
    /// A stream under the vault key of `epoch` with a fresh random salt (so its own key) and a
    /// fresh random nonce prefix.
    pub fn new(epoch: u32, vault_key: &[u8; 32], vault_id: &[u8; 16], blob: &Uuid) -> Self {
        let mut salt = [0u8; SALT];
        let mut prefix = [0u8; PREFIX];
        let mut rng = rand::rng();
        rng.fill_bytes(&mut salt);
        rng.fill_bytes(&mut prefix);
        Self::new_with(epoch, vault_key, vault_id, blob, salt, prefix)
    }

    pub(super) fn new_with(
        epoch: u32,
        vault_key: &[u8; 32],
        vault_id: &[u8; 16],
        blob: &Uuid,
        salt: [u8; SALT],
        prefix: [u8; PREFIX],
    ) -> Self {
        Self {
            key: hkdf32(vault_key, &salt, BLOB_INFO),
            header: stream_header(epoch, &salt, &prefix),
            prefix,
            aad: stream_aad(epoch, vault_id, &blob_object_id(blob)),
            counter: Some(0),
        }
    }

    /// The stream header, which comes before the first chunk.
    pub fn header(&self) -> [u8; HEADER_LEN] {
        self.header
    }

    /// Seals the next chunk: exactly `CHUNK` bytes, or `0..CHUNK` bytes when it is the `last`.
    /// Nothing may follow the last chunk.
    pub fn seal_chunk(&mut self, plain: &[u8], last: bool) -> Result<Vec<u8>> {
        let counter = self.counter.ok_or_else(|| {
            AppError::Internal("sync blob: a chunk after the last one or the counter's end".into())
        })?;
        let fits = if last {
            plain.len() < CHUNK
        } else {
            plain.len() == CHUNK
        };
        if !fits {
            return Err(AppError::Internal(format!(
                "sync blob: a {} chunk of {} bytes",
                if last { "last" } else { "full" },
                plain.len()
            )));
        }
        let sealed = aead_seal(
            &self.key,
            &chunk_nonce(&self.prefix, counter, last),
            &self.aad,
            plain,
        )
        .map_err(|_| AppError::Internal("sync blob: encryption failed".into()))?;
        self.counter = if last { None } else { counter.checked_add(1) };
        Ok(sealed)
    }
}

/// The per-stream key, read from the header.
struct StreamKey {
    key: Zeroizing<[u8; 32]>,
    prefix: [u8; PREFIX],
    aad: Vec<u8>,
}

/// Opens one blob of a known sealed length as its bytes arrive, returning each chunk once it is
/// complete and authenticated. It buffers at most one sealed chunk.
pub struct BlobOpener {
    /// Until the header is in; dropped (and wiped) once the stream key is derived.
    keys: Option<VaultKeys>,
    vault_id: [u8; 16],
    blob: Uuid,
    sealed_len: u64,
    plain_len: u64,
    /// Full chunks before the last one.
    full_chunks: u64,
    /// The sealed length of the last chunk, tag included.
    last_len: usize,
    received: u64,
    /// The header until it is complete, then the sealed chunk being received.
    buffer: Vec<u8>,
    stream: Option<StreamKey>,
    /// Chunks opened so far.
    opened: u64,
    done: bool,
    failed: bool,
}

impl BlobOpener {
    /// `Malformed` when `sealed_len` is no stream's length or its plaintext is over
    /// `MAX_BLOB_PLAIN`.
    pub fn new(
        keys: VaultKeys,
        vault_id: [u8; 16],
        blob: Uuid,
        sealed_len: u64,
    ) -> std::result::Result<Self, OpenError> {
        let plain_len = plain_len(sealed_len).ok_or(OpenError::Malformed)?;
        if plain_len > MAX_BLOB_PLAIN {
            return Err(OpenError::Malformed);
        }
        let body = sealed_len - HEADER_64;
        let last_len = usize::try_from(body % SEALED_CHUNK_64).map_err(|_| OpenError::Malformed)?;
        Ok(Self {
            keys: Some(keys),
            vault_id,
            blob,
            sealed_len,
            plain_len,
            full_chunks: body / SEALED_CHUNK_64,
            last_len,
            received: 0,
            buffer: Vec::with_capacity(SEALED_CHUNK),
            stream: None,
            opened: 0,
            done: false,
            failed: false,
        })
    }

    /// The plaintext length the announced sealed length carries.
    pub fn plain_len(&self) -> u64 {
        self.plain_len
    }

    /// Takes the next sealed bytes and returns the plaintext of every chunk they complete, in
    /// order. After an error the opener stays failed.
    pub fn push(
        &mut self,
        sealed: &[u8],
    ) -> std::result::Result<Vec<Zeroizing<Vec<u8>>>, OpenError> {
        if self.failed {
            return Err(OpenError::Malformed);
        }
        let pushed = self.push_inner(sealed);
        if pushed.is_err() {
            self.failed = true;
            self.buffer.clear();
        }
        pushed
    }

    fn push_inner(
        &mut self,
        mut sealed: &[u8],
    ) -> std::result::Result<Vec<Zeroizing<Vec<u8>>>, OpenError> {
        let pushed = u64::try_from(sealed.len()).map_err(|_| OpenError::Malformed)?;
        let received = self
            .received
            .checked_add(pushed)
            .filter(|&total| total <= self.sealed_len)
            .ok_or(OpenError::Malformed)?;
        self.received = received;

        let mut chunks = Vec::new();
        while !sealed.is_empty() {
            let wanted = self.next_len();
            let take = (wanted - self.buffer.len()).min(sealed.len());
            self.buffer.extend_from_slice(&sealed[..take]);
            sealed = &sealed[take..];
            if self.buffer.len() == wanted {
                if self.stream.is_none() {
                    self.read_header()?;
                } else {
                    chunks.push(self.open_chunk()?);
                }
                self.buffer.clear();
            }
        }
        Ok(chunks)
    }

    /// The length of what the buffer is filling: the header, a full chunk or the last chunk.
    fn next_len(&self) -> usize {
        if self.stream.is_none() {
            HEADER_LEN
        } else if self.opened < self.full_chunks {
            SEALED_CHUNK
        } else {
            self.last_len
        }
    }

    fn read_header(&mut self) -> std::result::Result<(), OpenError> {
        let header = &self.buffer;
        if header[0..4] != MAGIC {
            return Err(OpenError::Malformed);
        }
        let epoch = u32::from_be_bytes([header[4], header[5], header[6], header[7]]);
        let keys = self.keys.take().ok_or(OpenError::Malformed)?;
        let vault_key = keys.get(epoch).ok_or(OpenError::UnknownEpoch(epoch))?;
        let mut prefix = [0u8; PREFIX];
        prefix.copy_from_slice(&header[8 + SALT..HEADER_LEN]);
        self.stream = Some(StreamKey {
            key: hkdf32(&vault_key, &header[8..8 + SALT], BLOB_INFO),
            prefix,
            aad: stream_aad(epoch, &self.vault_id, &blob_object_id(&self.blob)),
        });
        Ok(())
    }

    fn open_chunk(&mut self) -> std::result::Result<Zeroizing<Vec<u8>>, OpenError> {
        let stream = self.stream.as_ref().ok_or(OpenError::Malformed)?;
        let counter = u32::try_from(self.opened).map_err(|_| OpenError::Malformed)?;
        let last = self.opened == self.full_chunks;
        let nonce = chunk_nonce(&stream.prefix, counter, last);
        let plain = Zeroizing::new(
            aead_open(&stream.key, &nonce, &stream.aad, &self.buffer)
                .map_err(|_| OpenError::Authentication)?,
        );
        self.opened += 1;
        self.done = last;
        Ok(plain)
    }

    /// `Malformed` unless exactly the announced bytes came and the flagged last chunk opened.
    pub fn finish(self) -> std::result::Result<(), OpenError> {
        if self.failed || !self.done || self.received != self.sealed_len {
            return Err(OpenError::Malformed);
        }
        Ok(())
    }
}
