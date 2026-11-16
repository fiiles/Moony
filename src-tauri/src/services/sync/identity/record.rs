//! Signed device records (phase 5 spec §5.4): a device says it `left`, or a trusted device says
//! it `removed` another. Stored base64url in the synced write-once columns
//! `sync_devices.left_record` and `removal_record`.
//!
//! ```text
//! record = "moony device record v1" ‖ type (u8: 1 left, 2 removed) ‖ vault_id (16)
//!          ‖ subject_device (16) ‖ signer_device (16) ‖ at (i64) ‖ epoch (u32) ‖ sig (64)
//! ```
//!
//! `sig` is Ed25519 by the signer over every byte before it. A `left` record names its subject as
//! signer; a `removed` record never does (removing itself is turning sync off). Whether the
//! signer is trusted is the trust store's question.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;

use super::{verify_sig, SignSeed};
use crate::error::{AppError, Result};
use crate::services::sync::hlc::DeviceId;

const RECORD_DOMAIN: &[u8; 22] = b"moony device record v1";
const SIGNED_LEN: usize = 22 + 1 + 16 + 16 + 16 + 8 + 4;
const RECORD_LEN: usize = SIGNED_LEN + 64;
const MAX_B64_LEN: usize = RECORD_LEN.div_ceil(3) * 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecordType {
    Left = 1,
    Removed = 2,
}

impl RecordType {
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(RecordType::Left),
            2 => Some(RecordType::Removed),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub kind: RecordType,
    pub vault_id: [u8; 16],
    pub subject: DeviceId,
    pub signer: DeviceId,
    pub at: i64,
    /// The key epoch current when the record was signed.
    pub epoch: u32,
    pub sig: [u8; 64],
}

fn invalid() -> AppError {
    AppError::Internal("sync: a device record is not valid".into())
}

fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(bytes);
    out
}

/// A `left` record is signed by its subject; a `removed` one by another device.
fn check_shape(kind: RecordType, subject: DeviceId, signer: DeviceId) -> Result<()> {
    let ok = match kind {
        RecordType::Left => subject == signer,
        RecordType::Removed => subject != signer,
    };
    ok.then_some(()).ok_or_else(invalid)
}

impl Record {
    /// Signs a record with the signer's key.
    pub fn sign(
        kind: RecordType,
        vault_id: [u8; 16],
        subject: DeviceId,
        signer: DeviceId,
        at: i64,
        epoch: u32,
        key: &SignSeed,
    ) -> Result<Self> {
        check_shape(kind, subject, signer)?;
        let mut record = Record {
            kind,
            vault_id,
            subject,
            signer,
            at,
            epoch,
            sig: [0; 64],
        };
        record.sig = key.sign(&record.signed_bytes());
        Ok(record)
    }

    fn signed_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(RECORD_LEN);
        out.extend_from_slice(RECORD_DOMAIN);
        out.push(self.kind as u8);
        out.extend_from_slice(&self.vault_id);
        out.extend_from_slice(&self.subject.0);
        out.extend_from_slice(&self.signer.0);
        out.extend_from_slice(&self.at.to_be_bytes());
        out.extend_from_slice(&self.epoch.to_be_bytes());
        out
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.signed_bytes();
        out.extend_from_slice(&self.sig);
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != RECORD_LEN || &bytes[..RECORD_DOMAIN.len()] != RECORD_DOMAIN {
            return Err(invalid());
        }
        let kind = RecordType::from_u8(bytes[22]).ok_or_else(invalid)?;
        let record = Record {
            kind,
            vault_id: array(&bytes[23..39]),
            subject: DeviceId(array(&bytes[39..55])),
            signer: DeviceId(array(&bytes[55..71])),
            at: i64::from_be_bytes(array(&bytes[71..79])),
            epoch: u32::from_be_bytes(array(&bytes[79..83])),
            sig: array(&bytes[83..]),
        };
        check_shape(record.kind, record.subject, record.signer)?;
        Ok(record)
    }

    pub fn to_b64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.encode())
    }

    pub fn from_b64(s: &str) -> Result<Self> {
        if s.len() > MAX_B64_LEN {
            return Err(invalid());
        }
        let bytes = URL_SAFE_NO_PAD.decode(s).map_err(|_| invalid())?;
        Self::decode(&bytes)
    }

    /// The signature is by `signer_pub` (the caller looks the signer's key up) and the shape
    /// holds.
    pub fn verify(&self, signer_pub: &[u8; 32]) -> Result<()> {
        check_shape(self.kind, self.subject, self.signer)?;
        verify_sig(signer_pub, &self.signed_bytes(), &self.sig).map_err(|_| invalid())
    }
}
