//! Op envelopes (spec §9.2, plan corrections 1–3): what the relay stores for one changeset.
//!
//! ```text
//! envelope  = "MNY1" | epoch (u32 BE) | nonce (24 random bytes) | ciphertext+tag
//! op key    = HKDF-SHA256(vault_key[epoch], salt = vault_id (16 raw bytes), "moony sync ops v1")
//! AAD       = "MNY1" | epoch (u32 BE) | vault_id (16 raw bytes)
//! plaintext = raw deflate (RFC 1951) of the changeset JSON, zero-padded to a multiple of 1 KiB
//! op ID     = the first 32 characters of base64url-no-pad(SHA-256(envelope))
//! ```
//!
//! The cipher is XChaCha20-Poly1305: random 24-byte nonces are safe at any volume Moony sees.
//! The fixed vectors in the tests are the contract with every other device.

use std::io::Write;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::XChaCha20Poly1305;
use flate2::{Decompress, FlushDecompress, Status};
use hkdf::Hkdf;
use rand::Rng;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::error::{AppError, Result};

pub const MAGIC: [u8; 4] = *b"MNY1";
pub const OPS_INFO: &[u8] = b"moony sync ops v1";
pub const TOKEN_INFO: &[u8] = b"moony sync token v1";
pub const SNAPSHOT_INFO: &[u8] = b"moony sync snapshot v1";
/// The padded plaintext is a multiple of this (and at least this long).
pub const PAD: usize = 1024;
/// Magic (4) + epoch (4) + nonce (24) + Poly1305 tag (16).
pub const ENVELOPE_OVERHEAD: usize = 48;
/// The relay's per-op limit (192 KiB).
pub const MAX_OP_BYTES: usize = 196_608;
/// The largest changeset JSON an envelope may inflate to.
pub const MAX_CHANGESET_JSON: usize = 4 * 1024 * 1024;

const HEADER: usize = 32;
const NONCE: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OpenError {
    #[error("malformed")]
    Malformed,
    #[error("unknown epoch {0}")]
    UnknownEpoch(u32),
    #[error("authentication failed")]
    Authentication,
    #[error("bad padding")]
    Padding,
    #[error("too large")]
    TooLarge,
}

/// HKDF-SHA256 (RFC 5869) with a 32-byte output.
pub fn hkdf32(ikm: &[u8; 32], salt: &[u8], info: &[u8]) -> Zeroizing<[u8; 32]> {
    let mut okm = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(salt), ikm)
        .expand(info, &mut okm[..])
        .expect("32 bytes is within HKDF-SHA256's output limit");
    okm
}

/// The bearer token the relay knows the vault by: base64url-no-pad of
/// HKDF(vault_key[epoch], vault_id, "moony sync token v1"), 43 characters.
pub fn vault_token(vault_key: &[u8; 32], vault_id: &[u8; 16]) -> String {
    URL_SAFE_NO_PAD.encode(*hkdf32(vault_key, vault_id, TOKEN_INFO))
}

/// Raw deflate (flate2's default level) zero-padded to a multiple of `PAD`, at least `PAD`; the
/// same length `changeset::padded_deflated_len` estimates.
pub fn compress_padded(json: &[u8]) -> Result<Vec<u8>> {
    let deflate = |e: std::io::Error| AppError::Internal(format!("sync envelope: deflate: {e}"));
    let mut encoder =
        flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(json).map_err(deflate)?;
    let mut plain = encoder.finish().map_err(deflate)?;
    plain.resize(plain.len().div_ceil(PAD).max(1) * PAD, 0);
    Ok(plain)
}

/// Seals a changeset JSON under the vault key of `epoch` with a fresh random nonce.
pub fn seal_op(
    epoch: u32,
    vault_key: &[u8; 32],
    vault_id: &[u8; 16],
    json: &[u8],
) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE];
    rand::rng().fill_bytes(&mut nonce);
    seal_op_with_nonce(epoch, vault_key, vault_id, json, nonce)
}

fn seal_op_with_nonce(
    epoch: u32,
    vault_key: &[u8; 32],
    vault_id: &[u8; 16],
    json: &[u8],
    nonce: [u8; NONCE],
) -> Result<Vec<u8>> {
    seal_padded(epoch, vault_key, vault_id, &compress_padded(json)?, nonce)
}

fn seal_padded(
    epoch: u32,
    vault_key: &[u8; 32],
    vault_id: &[u8; 16],
    plain: &[u8],
    nonce: [u8; NONCE],
) -> Result<Vec<u8>> {
    let op_key = hkdf32(vault_key, vault_id, OPS_INFO);
    let sealed = aead_seal(&op_key, &nonce, &op_aad(epoch, vault_id), plain)
        .map_err(|_| AppError::Internal("sync envelope: encryption failed".into()))?;
    let mut envelope = Vec::with_capacity(HEADER + sealed.len());
    envelope.extend_from_slice(&MAGIC);
    envelope.extend_from_slice(&epoch.to_be_bytes());
    envelope.extend_from_slice(&nonce);
    envelope.extend_from_slice(&sealed);
    Ok(envelope)
}

/// Opens an envelope and returns the changeset JSON. `key_for` maps an epoch to its vault key.
pub fn open_op(
    key_for: impl Fn(u32) -> Option<Zeroizing<[u8; 32]>>,
    vault_id: &[u8; 16],
    envelope: &[u8],
) -> std::result::Result<Vec<u8>, OpenError> {
    if envelope.len() > MAX_OP_BYTES {
        return Err(OpenError::TooLarge);
    }
    if envelope.len() < ENVELOPE_OVERHEAD + PAD || envelope[0..4] != MAGIC {
        return Err(OpenError::Malformed);
    }
    let epoch = u32::from_be_bytes([envelope[4], envelope[5], envelope[6], envelope[7]]);
    let vault_key = key_for(epoch).ok_or(OpenError::UnknownEpoch(epoch))?;
    let mut nonce = [0u8; NONCE];
    nonce.copy_from_slice(&envelope[8..HEADER]);
    let op_key = hkdf32(&vault_key, vault_id, OPS_INFO);
    let plain = Zeroizing::new(
        aead_open(
            &op_key,
            &nonce,
            &op_aad(epoch, vault_id),
            &envelope[HEADER..],
        )
        .map_err(|_| OpenError::Authentication)?,
    );
    inflate_padded(&plain)
}

/// SHA-256 of the whole envelope (the chain hash).
pub fn envelope_hash(envelope: &[u8]) -> [u8; 32] {
    Sha256::digest(envelope).into()
}

/// The ID the relay stores the op under: the first 32 characters of
/// base64url-no-pad(SHA-256(envelope)), so a retry is idempotent and names no author.
pub fn op_id(envelope: &[u8]) -> String {
    let mut id = URL_SAFE_NO_PAD.encode(envelope_hash(envelope));
    id.truncate(32);
    id
}

fn op_aad(epoch: u32, vault_id: &[u8; 16]) -> [u8; 24] {
    let mut aad = [0u8; 24];
    aad[0..4].copy_from_slice(&MAGIC);
    aad[4..8].copy_from_slice(&epoch.to_be_bytes());
    aad[8..24].copy_from_slice(vault_id);
    aad
}

fn aead_seal(
    key: &[u8; 32],
    nonce: &[u8; NONCE],
    aad: &[u8],
    msg: &[u8],
) -> std::result::Result<Vec<u8>, chacha20poly1305::Error> {
    XChaCha20Poly1305::new(key.into()).encrypt(nonce.into(), Payload { msg, aad })
}

fn aead_open(
    key: &[u8; 32],
    nonce: &[u8; NONCE],
    aad: &[u8],
    msg: &[u8],
) -> std::result::Result<Vec<u8>, chacha20poly1305::Error> {
    XChaCha20Poly1305::new(key.into()).decrypt(nonce.into(), Payload { msg, aad })
}

/// Inflates the raw deflate stream at the start of `plain` (at most `MAX_CHANGESET_JSON` bytes)
/// and requires every byte after the stream's end to be zero.
fn inflate_padded(plain: &[u8]) -> std::result::Result<Vec<u8>, OpenError> {
    // One byte over the limit tells "exactly at the limit" from "over it".
    const CAP: usize = MAX_CHANGESET_JSON + 1;
    let mut inflater = Decompress::new(false);
    let mut json = Vec::with_capacity((plain.len() * 4).min(CAP));
    loop {
        if json.len() == json.capacity() {
            if json.len() >= CAP {
                return Err(OpenError::TooLarge);
            }
            json.reserve_exact(json.len().max(PAD).min(CAP - json.len()));
        }
        let (read, written) = (inflater.total_in(), inflater.total_out());
        let consumed = usize::try_from(read).map_err(|_| OpenError::Malformed)?;
        let status = inflater
            .decompress_vec(&plain[consumed..], &mut json, FlushDecompress::None)
            .map_err(|_| OpenError::Malformed)?;
        if status == Status::StreamEnd {
            break;
        }
        let stalled = inflater.total_in() == read && inflater.total_out() == written;
        if stalled && json.len() < json.capacity() {
            // Input exhausted with room to write: the stream ends without its final block.
            return Err(OpenError::Malformed);
        }
    }
    if json.len() > MAX_CHANGESET_JSON {
        return Err(OpenError::TooLarge);
    }
    let end = usize::try_from(inflater.total_in()).map_err(|_| OpenError::Malformed)?;
    if plain[end..].iter().any(|&b| b != 0) {
        return Err(OpenError::Padding);
    }
    Ok(json)
}

#[cfg(test)]
mod tests;
