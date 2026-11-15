//! STREAM encryption for snapshots (spec §9.2): one sealed object of any length, cut into
//! 64 KiB chunks that cannot be dropped, reordered, extended or moved to another object.
//!
//! ```text
//! stream    = header | chunk 0 | chunk 1 | … | last chunk
//! header    = "MNY1" | epoch (u32 BE) | salt (32 random bytes) | nonce prefix (19 random bytes)
//! key       = HKDF-SHA256(vault_key[epoch], salt, "moony sync snapshot v1")   (one per stream)
//! nonce     = prefix (19) | chunk counter (u32 BE) | 0x01 on the last chunk, else 0x00
//! AAD       = "MNY1" | epoch (u32 BE) | vault_id (16 raw bytes) | object ID
//! chunk     = XChaCha20-Poly1305 of CHUNK plaintext bytes + 16-byte tag; the last chunk carries
//!             0..CHUNK bytes, so an input that is a multiple of CHUNK ends in an empty last chunk
//! object ID = for a snapshot, its relay sequence `S` as u64 BE
//! ```
//!
//! A blob is the same stream with its own info string and object ID (`blob.rs`).
//!
//! Every chunk but the last is full and the last never is, so the chunk boundaries follow from
//! the length alone. The fixed vectors in the tests are the contract with every other device.

use rand::Rng;
use zeroize::{Zeroize, Zeroizing};

use super::{aead_open, aead_seal, hkdf32, op_aad, OpenError, MAGIC, NONCE, SNAPSHOT_INFO};
use crate::error::{AppError, Result};

/// Plaintext bytes in every chunk but the last.
pub const CHUNK: usize = 65_536;
/// Magic (4) + epoch (4) + salt (32) + nonce prefix (19).
pub const HEADER_LEN: usize = 59;

pub(super) const TAG: usize = 16;
pub(super) const SEALED_CHUNK: usize = CHUNK + TAG;
pub(super) const SALT: usize = 32;
pub(super) const PREFIX: usize = 19;

/// Seals a stream as it is written: every full `CHUNK` is sealed at once, `finish` seals the rest
/// as the last chunk and returns the whole stream.
pub struct StreamSealer {
    key: Zeroizing<[u8; 32]>,
    prefix: [u8; PREFIX],
    aad: Vec<u8>,
    /// The counter of the next chunk; `None` once no counter is left for it.
    counter: Option<u32>,
    buffer: Zeroizing<Vec<u8>>,
    out: Vec<u8>,
}

impl StreamSealer {
    /// A stream under the vault key of `epoch` with a fresh random salt (so its own key) and a
    /// fresh random nonce prefix.
    pub fn new(epoch: u32, vault_key: &[u8; 32], vault_id: &[u8; 16], object_id: &[u8]) -> Self {
        let mut salt = [0u8; SALT];
        let mut prefix = [0u8; PREFIX];
        let mut rng = rand::rng();
        rng.fill_bytes(&mut salt);
        rng.fill_bytes(&mut prefix);
        Self::new_with(epoch, vault_key, vault_id, object_id, salt, prefix)
    }

    pub(super) fn new_with(
        epoch: u32,
        vault_key: &[u8; 32],
        vault_id: &[u8; 16],
        object_id: &[u8],
        salt: [u8; SALT],
        prefix: [u8; PREFIX],
    ) -> Self {
        Self::new_with_info(
            SNAPSHOT_INFO,
            epoch,
            vault_key,
            vault_id,
            object_id,
            salt,
            prefix,
        )
    }

    /// `new_with` with the stream key derived under `info` instead of the snapshot's.
    pub(super) fn new_with_info(
        info: &[u8],
        epoch: u32,
        vault_key: &[u8; 32],
        vault_id: &[u8; 16],
        object_id: &[u8],
        salt: [u8; SALT],
        prefix: [u8; PREFIX],
    ) -> Self {
        let mut out = Vec::with_capacity(HEADER_LEN + SEALED_CHUNK);
        out.extend_from_slice(&stream_header(epoch, &salt, &prefix));
        Self {
            key: hkdf32(vault_key, &salt, info),
            prefix,
            aad: stream_aad(epoch, vault_id, object_id),
            counter: Some(0),
            buffer: Zeroizing::new(Vec::with_capacity(CHUNK)),
            out,
        }
    }

    /// Seals what is left as the last chunk and returns the whole stream.
    pub fn finish(mut self) -> Result<Vec<u8>> {
        self.seal_buffer(true)?;
        Ok(self.out)
    }

    /// Seals the buffer as the next chunk and empties it.
    fn seal_buffer(&mut self, last: bool) -> Result<()> {
        let counter = self.counter.ok_or_else(|| {
            AppError::Internal("sync stream: more chunks than a u32 counter allows".into())
        })?;
        // Under the relay's 100 MiB cap a stream has at most 1,601 chunks.
        debug_assert!(
            last || counter < u32::MAX,
            "sync stream: chunk counter overflow"
        );
        let sealed = aead_seal(
            &self.key,
            &chunk_nonce(&self.prefix, counter, last),
            &self.aad,
            &self.buffer,
        )
        .map_err(|_| AppError::Internal("sync stream: encryption failed".into()))?;
        self.out.extend_from_slice(&sealed);
        self.buffer.zeroize();
        self.counter = if last { None } else { counter.checked_add(1) };
        Ok(())
    }
}

impl std::io::Write for StreamSealer {
    /// Takes up to the room left in the current chunk and seals the chunk once it is full.
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        if self.counter.is_none() {
            return Err(std::io::Error::other(
                "sync stream: more chunks than a u32 counter allows",
            ));
        }
        let take = data.len().min(CHUNK - self.buffer.len());
        self.buffer.extend_from_slice(&data[..take]);
        if self.buffer.len() == CHUNK {
            if let Err(e) = self.seal_buffer(false) {
                // Hand the bytes back: an error means none of `data` was taken.
                self.buffer.truncate(CHUNK - take);
                return Err(std::io::Error::other(e.to_string()));
            }
        }
        Ok(take)
    }

    /// A partial chunk is sealed only by `finish`.
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Opens a whole snapshot stream sealed for `object_id`. `key_for` maps an epoch to its vault key.
pub fn open_stream(
    key_for: impl Fn(u32) -> Option<Zeroizing<[u8; 32]>>,
    vault_id: &[u8; 16],
    object_id: &[u8],
    sealed: &[u8],
) -> std::result::Result<Vec<u8>, OpenError> {
    open_stream_with(SNAPSHOT_INFO, key_for, vault_id, object_id, sealed)
}

/// `open_stream` with the stream key derived under `info` instead of the snapshot's.
pub(super) fn open_stream_with(
    info: &[u8],
    key_for: impl Fn(u32) -> Option<Zeroizing<[u8; 32]>>,
    vault_id: &[u8; 16],
    object_id: &[u8],
    sealed: &[u8],
) -> std::result::Result<Vec<u8>, OpenError> {
    if sealed.len() < HEADER_LEN + TAG || sealed[0..4] != MAGIC {
        return Err(OpenError::Malformed);
    }
    let body = &sealed[HEADER_LEN..];
    let full_chunks = body.len() / SEALED_CHUNK;
    let last_len = body.len() % SEALED_CHUNK;
    if last_len < TAG {
        // No room for a last chunk after the full ones: it was dropped or cut.
        return Err(OpenError::Malformed);
    }
    let epoch = u32::from_be_bytes([sealed[4], sealed[5], sealed[6], sealed[7]]);
    let vault_key = key_for(epoch).ok_or(OpenError::UnknownEpoch(epoch))?;
    let mut prefix = [0u8; PREFIX];
    prefix.copy_from_slice(&sealed[8 + SALT..HEADER_LEN]);
    let key = hkdf32(&vault_key, &sealed[8..8 + SALT], info);
    let aad = stream_aad(epoch, vault_id, object_id);

    let mut plain = Zeroizing::new(Vec::with_capacity(full_chunks * CHUNK + last_len - TAG));
    for (index, chunk) in body.chunks(SEALED_CHUNK).enumerate() {
        let counter = u32::try_from(index).map_err(|_| OpenError::Malformed)?;
        let nonce = chunk_nonce(&prefix, counter, index == full_chunks);
        let opened = Zeroizing::new(
            aead_open(&key, &nonce, &aad, chunk).map_err(|_| OpenError::Authentication)?,
        );
        plain.extend_from_slice(&opened);
    }
    Ok(std::mem::take(&mut *plain))
}

/// The object ID a snapshot is sealed for: its relay sequence `S`, u64 BE.
pub fn snapshot_object_id(seq: u64) -> [u8; 8] {
    seq.to_be_bytes()
}

/// `"MNY1"` | epoch (u32 BE) | salt | nonce prefix.
pub(super) fn stream_header(
    epoch: u32,
    salt: &[u8; SALT],
    prefix: &[u8; PREFIX],
) -> [u8; HEADER_LEN] {
    let mut header = [0u8; HEADER_LEN];
    header[0..4].copy_from_slice(&MAGIC);
    header[4..8].copy_from_slice(&epoch.to_be_bytes());
    header[8..8 + SALT].copy_from_slice(salt);
    header[8 + SALT..].copy_from_slice(prefix);
    header
}

/// Prefix | chunk counter (u32 BE) | 0x01 on the last chunk, else 0x00.
pub(super) fn chunk_nonce(prefix: &[u8; PREFIX], counter: u32, last: bool) -> [u8; NONCE] {
    let mut nonce = [0u8; NONCE];
    nonce[..PREFIX].copy_from_slice(prefix);
    nonce[PREFIX..NONCE - 1].copy_from_slice(&counter.to_be_bytes());
    nonce[NONCE - 1] = u8::from(last);
    nonce
}

/// The op AAD followed by the object ID, so one stored object can't stand in for another.
pub(super) fn stream_aad(epoch: u32, vault_id: &[u8; 16], object_id: &[u8]) -> Vec<u8> {
    let mut aad = op_aad(epoch, vault_id).to_vec();
    aad.extend_from_slice(object_id);
    aad
}
