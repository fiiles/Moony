//! The Windows Hello wrap, kept apart from the WinRT calls in [`super::windows`] so it is tested on
//! every platform.
//!
//! Windows Hello has no "unlock this secret" call; what it offers is a per-app key pair in the TPM
//! (KeyCredentialManager) that signs a challenge after the Hello prompt. Its RSA signatures are
//! deterministic (PKCS#1 v1.5), so signing the same challenge again yields the same bytes, and those
//! bytes are a secret only the Hello key can produce. They are the input key material here:
//!
//! - key = HKDF-SHA256(ikm = signature, salt = challenge, info = `"moony-hello-v1"`)
//! - blob v1 = `0x01 | challenge (32) | nonce (12) | AES-256-GCM(key, nonce, secret) (32 + 16 tag)`
//!
//! The challenge is random per wrap and stored in the clear (it is what Hello signs at unlock);
//! the signature and the derived key never leave memory and are wiped on drop.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use hkdf::Hkdf;
use rand::Rng;
use sha2::Sha256;
use zeroize::Zeroizing;

/// The first byte of every blob this module writes.
const VERSION: u8 = 0x01;
/// HKDF's `info`, so a key derived here is never the key of another use of the same signature.
const INFO: &[u8] = b"moony-hello-v1";
const CHALLENGE_LEN: usize = 32;
const NONCE_LEN: usize = 12;
const SECRET_LEN: usize = 32;
const TAG_LEN: usize = 16;
const NONCE_AT: usize = 1 + CHALLENGE_LEN;
const CIPHERTEXT_AT: usize = NONCE_AT + NONCE_LEN;
const BLOB_LEN: usize = CIPHERTEXT_AT + SECRET_LEN + TAG_LEN;

/// Why a blob could not be opened. Both mean the blob can never be opened with this credential,
/// which the Windows native reports as `Invalidated`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenError {
    /// Not a v1 blob of the right length.
    Malformed,
    /// The signature derives another key (the Hello credential was replaced) or the blob was
    /// altered: the tag does not verify.
    WrongKey,
}

/// A fresh random challenge for Hello to sign.
pub fn new_challenge() -> [u8; CHALLENGE_LEN] {
    let mut challenge = [0u8; CHALLENGE_LEN];
    rand::rng().fill_bytes(&mut challenge);
    challenge
}

/// Wraps `secret` with the key `signature` (Hello's signature over `challenge`) derives.
pub fn seal(challenge: &[u8; 32], signature: &[u8], secret: &[u8; 32]) -> Vec<u8> {
    let cipher = cipher_for(challenge, signature);
    let mut nonce = [0u8; NONCE_LEN];
    rand::rng().fill_bytes(&mut nonce);
    // AES-GCM refuses only plaintexts longer than 64 GiB; 32 bytes always encrypt.
    let ciphertext = cipher
        .encrypt(&Nonce::from(nonce), secret.as_slice())
        .expect("AES-GCM encrypts 32 bytes");

    let mut blob = Vec::with_capacity(BLOB_LEN);
    blob.push(VERSION);
    blob.extend_from_slice(challenge);
    blob.extend_from_slice(&nonce);
    blob.extend_from_slice(&ciphertext);
    blob
}

/// The challenge a v1 blob was sealed for, which Hello has to sign again to open it.
pub fn challenge_of(blob: &[u8]) -> Option<[u8; 32]> {
    if blob.len() != BLOB_LEN || blob[0] != VERSION {
        return None;
    }
    blob[1..NONCE_AT].try_into().ok()
}

/// Unwraps a blob [`seal`] made, with Hello's signature over [`challenge_of`] the blob.
pub fn open(blob: &[u8], signature: &[u8]) -> Result<Zeroizing<[u8; 32]>, OpenError> {
    let challenge = challenge_of(blob).ok_or(OpenError::Malformed)?;
    let nonce: [u8; NONCE_LEN] = blob[NONCE_AT..CIPHERTEXT_AT]
        .try_into()
        .map_err(|_| OpenError::Malformed)?;
    let plaintext = Zeroizing::new(
        cipher_for(&challenge, signature)
            .decrypt(&Nonce::from(nonce), &blob[CIPHERTEXT_AT..])
            .map_err(|_| OpenError::WrongKey)?,
    );
    let mut secret = Zeroizing::new([0u8; SECRET_LEN]);
    // The tag verified, so this is the 32 bytes `seal` encrypted; a length mismatch would mean a
    // blob from another writer, which is as unusable as a malformed one.
    if plaintext.len() != SECRET_LEN {
        return Err(OpenError::Malformed);
    }
    secret.copy_from_slice(&plaintext);
    Ok(secret)
}

/// The AES-256-GCM cipher keyed with HKDF-SHA256(ikm = signature, salt = challenge).
fn cipher_for(challenge: &[u8; 32], signature: &[u8]) -> Aes256Gcm {
    let mut key = Zeroizing::new([0u8; 32]);
    // HKDF-SHA256 refuses only outputs longer than 255 * 32 bytes; 32 always expand.
    Hkdf::<Sha256>::new(Some(challenge), signature)
        .expand(INFO, key.as_mut_slice())
        .expect("HKDF-SHA256 expands 32 bytes");
    Aes256Gcm::new(&(*key).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: [u8; 32] = [0x42; 32];
    /// Stands in for a Hello signature: RSA-2048 signatures are 256 bytes.
    fn signature(fill: u8) -> Vec<u8> {
        (0..=255u8).map(|byte| byte ^ fill).collect()
    }

    #[test]
    fn seal_open_round_trips() {
        let challenge = new_challenge();
        let blob = seal(&challenge, &signature(1), &SECRET);
        assert_eq!(blob.len(), 1 + 32 + 12 + 48);
        assert_eq!(open(&blob, &signature(1)).map(|secret| *secret), Ok(SECRET));
    }

    #[test]
    fn open_with_other_signature_is_wrong_key() {
        let blob = seal(&new_challenge(), &signature(1), &SECRET);
        assert_eq!(
            open(&blob, &signature(2)).map(|_| ()),
            Err(OpenError::WrongKey)
        );
    }

    #[test]
    fn a_tampered_ciphertext_is_wrong_key() {
        let mut blob = seal(&new_challenge(), &signature(1), &SECRET);
        let last = blob.len() - 1;
        blob[last] ^= 1;
        assert_eq!(
            open(&blob, &signature(1)).map(|_| ()),
            Err(OpenError::WrongKey)
        );
    }

    #[test]
    fn open_truncated_is_malformed() {
        let blob = seal(&new_challenge(), &signature(1), &SECRET);
        for len in [0, 1, 33, 45, blob.len() - 1] {
            assert_eq!(
                open(&blob[..len], &signature(1)).map(|_| ()),
                Err(OpenError::Malformed),
                "{len} bytes"
            );
        }
        let mut longer = blob.clone();
        longer.push(0);
        assert_eq!(
            open(&longer, &signature(1)).map(|_| ()),
            Err(OpenError::Malformed)
        );
    }

    #[test]
    fn open_unknown_version_is_malformed() {
        let mut blob = seal(&new_challenge(), &signature(1), &SECRET);
        for version in [0x00, 0x02, 0xff] {
            blob[0] = version;
            assert_eq!(
                open(&blob, &signature(1)).map(|_| ()),
                Err(OpenError::Malformed)
            );
            assert_eq!(challenge_of(&blob), None);
        }
    }

    #[test]
    fn challenge_of_reads_back_the_challenge() {
        let challenge = new_challenge();
        let blob = seal(&challenge, &signature(1), &SECRET);
        assert_eq!(challenge_of(&blob), Some(challenge));
        assert_eq!(challenge_of(&blob[..20]), None);
        assert_eq!(challenge_of(&[]), None);
    }

    #[test]
    fn two_seals_use_different_nonces() {
        let challenge = new_challenge();
        let first = seal(&challenge, &signature(1), &SECRET);
        let second = seal(&challenge, &signature(1), &SECRET);
        assert_ne!(first[33..45], second[33..45], "nonces");
        assert_ne!(first[45..], second[45..], "ciphertexts");
        assert_ne!(new_challenge(), new_challenge());
    }

    #[test]
    fn blob_never_contains_signature_or_secret() {
        let signature = signature(1);
        let blob = seal(&new_challenge(), &signature, &SECRET);
        let contains = |needle: &[u8]| blob.windows(needle.len()).any(|window| window == needle);
        assert!(!contains(&SECRET), "the secret");
        // The whole signature cannot fit in the blob; no 16-byte run of it may appear either.
        for run in signature.chunks(16) {
            assert!(!contains(run), "a run of the signature");
        }
    }
}
