//! The Recovery Kit as a paper device (phase 5 revision §2.2, §2.3). Everything the Kit needs
//! is derived from one 32-byte `kit_seed` with HKDF-SHA256, `salt = vault_id`:
//!
//! | Value            | `info`                 | Use                                              |
//! |------------------|------------------------|--------------------------------------------------|
//! | `kit_box_secret` | `"moony kit box v1"`   | X25519: opens the Kit envelope (`rekey::envelope`) |
//! | `kit_sign_seed`  | `"moony kit sign v1"`  | Ed25519: signs the Kit's own certificate and that of a device joining with it |
//! | `kit_token`      | `"moony kit token v1"` | 32 bytes, base64url (43 characters): reads the envelope from the server |
//! | `kit_id`         | —                      | `SHA-256("moony kit id v1" ‖ vault_id ‖ kit_sign_pub)[0..16]` |
//!
//! The seed is never stored: it lives in a `KitSecret` (a wiped-on-drop buffer that prints
//! `<redacted>`) only while a person holds the Kit in a flow. Only the public keys, the
//! certificate and (inside a pending key change) the token reach the database.

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::{sign_pub, SignSeed};
use crate::error::Result;
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::keys::os_random;

const BOX_INFO: &[u8] = b"moony kit box v1";
const SIGN_INFO: &[u8] = b"moony kit sign v1";
const TOKEN_INFO: &[u8] = b"moony kit token v1";
const ID_DOMAIN: &[u8] = b"moony kit id v1";
/// The Kit token's length in base64url characters (32 bytes, no padding).
pub const KIT_TOKEN_LEN: usize = 43;

/// The Kit's seed (`kit_seed`, spec §2.2). Wiped on drop; `Debug` prints `<redacted>`.
pub struct KitSecret(Zeroizing<[u8; 32]>);

impl KitSecret {
    /// A fresh seed from the operating system's random source.
    pub fn generate() -> Result<Self> {
        let mut seed = Zeroizing::new([0u8; 32]);
        os_random(&mut seed[..])?;
        Ok(KitSecret(seed))
    }

    /// Takes the bytes over; they stay in their wiped-on-drop buffer.
    pub fn from_bytes(bytes: Zeroizing<[u8; 32]>) -> Self {
        KitSecret(bytes)
    }

    /// The seed, for writing it into the Kit's code. Never log it.
    pub fn expose(&self) -> &[u8; 32] {
        &self.0
    }

    /// Everything the Kit derives for `vault_id`.
    pub fn derive(&self, vault_id: &[u8; 16]) -> KitKeys {
        let hkdf = Hkdf::<Sha256>::new(Some(vault_id), self.0.as_slice());
        let expand = |info: &[u8]| {
            let mut out = Zeroizing::new([0u8; 32]);
            hkdf.expand(info, &mut out[..])
                .expect("32 bytes is within HKDF-SHA256's output limit");
            out
        };
        let box_secret = StaticSecret::from(*expand(BOX_INFO));
        let box_pub = PublicKey::from(&box_secret).to_bytes();
        let sign_seed = SignSeed::from_bytes(expand(SIGN_INFO));
        let sign_pub = sign_pub(&sign_seed);
        let token_bytes = expand(TOKEN_INFO);
        let mut token = Zeroizing::new(String::with_capacity(KIT_TOKEN_LEN));
        URL_SAFE_NO_PAD.encode_string(token_bytes.as_slice(), &mut token);
        KitKeys {
            vault_id: *vault_id,
            box_secret,
            box_pub,
            sign_seed,
            sign_pub,
            token,
            id: kit_id(vault_id, &sign_pub),
        }
    }
}

impl fmt::Debug for KitSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("KitSecret(<redacted>)")
    }
}

/// The Kit's ID in `sync_devices` and `sync_trust`, bound to its signing key and the vault.
pub fn kit_id(vault_id: &[u8; 16], kit_sign_pub: &[u8; 32]) -> DeviceId {
    let hash = Sha256::new()
        .chain_update(ID_DOMAIN)
        .chain_update(vault_id)
        .chain_update(kit_sign_pub)
        .finalize();
    let mut id = [0u8; 16];
    id.copy_from_slice(&hash[..16]);
    DeviceId(id)
}

/// What a `KitSecret` derives for one vault. The secrets (box secret, signing seed, token) are
/// wiped on drop; `Debug` prints only the ID and the public keys.
pub struct KitKeys {
    vault_id: [u8; 16],
    box_secret: StaticSecret,
    box_pub: [u8; 32],
    sign_seed: SignSeed,
    sign_pub: [u8; 32],
    token: Zeroizing<String>,
    id: DeviceId,
}

impl KitKeys {
    pub fn vault_id(&self) -> &[u8; 16] {
        &self.vault_id
    }

    /// Opens the Kit envelope. Never log it.
    pub fn box_secret(&self) -> &StaticSecret {
        &self.box_secret
    }

    pub fn box_pub(&self) -> &[u8; 32] {
        &self.box_pub
    }

    /// Signs the Kit's certificate and a Kit joiner's. Never log it.
    pub fn sign_seed(&self) -> &SignSeed {
        &self.sign_seed
    }

    pub fn sign_pub(&self) -> &[u8; 32] {
        &self.sign_pub
    }

    /// The Kit token (`GET /kit`), 43 base64url characters. Never log it.
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn id(&self) -> DeviceId {
        self.id
    }
}

impl fmt::Debug for KitKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("KitKeys")
            .field("id", &self.id)
            .field("box_pub", &hex::encode(self.box_pub))
            .field("sign_pub", &hex::encode(self.sign_pub))
            .finish_non_exhaustive()
    }
}
