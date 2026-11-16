//! Device identities (phase 5 spec §5.2): each device's Ed25519 signing key, the certificates
//! that bind a device ID to its signing and box keys inside one vault and say how it got in
//! (`cert`), the signed `left` and `removed` records (`record`), and the pinned store of the
//! devices this device has verified (`trust`). The signing seed lives in
//! `sync_state.device_sign_key`, never crosses IPC and never appears in a log line; like every
//! sync secret it sits in a `Zeroizing` buffer and prints `<redacted>`.

pub mod cert;
pub mod record;
pub mod trust;

#[cfg(test)]
mod tests;
#[cfg(test)]
mod trust_tests;

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use zeroize::Zeroizing;

use super::keys::os_random;
use crate::error::{AppError, Result};

pub use cert::{CertBody, CertFields, CertKind, Certificate, Ticket, TicketSecret};
pub use record::{Record, RecordType};

/// An Ed25519 signing seed (32 bytes): this device's signing key, or a one-time join ticket's.
pub struct SignSeed(Zeroizing<[u8; 32]>);

impl SignSeed {
    /// Takes the bytes over; they stay in their wiped-on-drop buffer.
    pub fn from_bytes(bytes: Zeroizing<[u8; 32]>) -> Self {
        SignSeed(bytes)
    }

    /// From a stored BLOB: exactly 32 bytes.
    pub fn from_slice(bytes: &[u8]) -> Result<Self> {
        let mut seed = Zeroizing::new([0u8; 32]);
        if bytes.len() != seed.len() {
            return Err(AppError::Internal(
                "sync: a signing key is not 32 bytes".into(),
            ));
        }
        seed.copy_from_slice(bytes);
        Ok(SignSeed(seed))
    }

    /// The seed, for storing it. Never log it.
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }

    /// The Ed25519 signature of `message`.
    pub fn sign(&self, message: &[u8]) -> [u8; 64] {
        // `SigningKey` wipes its copy of the secret on drop (feature `zeroize`).
        SigningKey::from_bytes(&self.0).sign(message).to_bytes()
    }
}

impl fmt::Debug for SignSeed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SignSeed(<redacted>)")
    }
}

/// A fresh signing seed from the operating system's random source.
pub fn new_sign_seed() -> Result<SignSeed> {
    let mut seed = Zeroizing::new([0u8; 32]);
    os_random(&mut seed[..])?;
    Ok(SignSeed(seed))
}

/// The public half of `seed`.
pub fn sign_pub(seed: &SignSeed) -> [u8; 32] {
    SigningKey::from_bytes(&seed.0).verifying_key().to_bytes()
}

/// Checks `sig` over `message` by `public` (strict: no small-order keys, canonical `s`), so a
/// signature has exactly one valid encoding and a flipped bit never verifies.
pub(crate) fn verify_sig(public: &[u8; 32], message: &[u8], sig: &[u8; 64]) -> Result<()> {
    let bad = || AppError::Internal("sync: a signature does not verify".into());
    let key = VerifyingKey::from_bytes(public).map_err(|_| bad())?;
    key.verify_strict(message, &Signature::from_bytes(sig))
        .map_err(|_| bad())
}
