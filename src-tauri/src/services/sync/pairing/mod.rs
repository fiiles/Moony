//! The pairing protocol's bytes (phase 3, spec §20 and ADR 0026): an X25519 exchange between the
//! device that shows a QR code (the shower, S) and the one that scans it (the scanner, C), an
//! HKDF transcript that gives both a session key and a six-digit comparison code, the QR text,
//! slot 1 (the scanner's reply) and slot 2 (the holder's sealed credentials). Everything here is
//! pure: no relay, no database, no clock. The session state machine builds on it.
//!
//! The vectors in `tests.rs` are the contract with every other device. The comparison code only
//! protects the exchange when S's public key reaches C over a channel the attacker cannot modify
//! (a QR on a screen the person is looking at).
//!
//! Secrets hygiene as in phase 2: keys, payloads and decrypted buffers live in `Zeroizing`
//! buffers, and `Debug` of every secret-bearing type prints `<redacted>`.

use std::collections::BTreeMap;
use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::XChaCha20Poly1305;
use hkdf::Hkdf;
use serde::Deserialize;
use sha2::Sha256;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::keys::{os_random, Credentials, SecretString, VaultId, VaultKeys};
use super::relay::http::{is_default_relay, normalize_base_url};
use crate::error::{AppError, Result};

#[cfg(test)]
mod tests;

pub const QR_PREFIX: &str = "moony-pair:1:";
pub const MAX_CODE_BYTES: usize = 512;
pub const KDF_INFO: &[u8] = b"moony pair v1";
pub const SLOT_VERSION: u8 = 0x01;
pub const SLOT1_LEN: usize = 34;
pub const SLOT2_DIRECTION: &[u8] = b"holder->joiner";
pub const CANCEL_MARKER: [u8; 2] = [SLOT_VERSION, 0x00];
pub const MAX_PAYLOAD_JSON: usize = 16 * 1024;
pub const MAX_PAYLOAD_KEYS: usize = 64;

const NONCE: usize = 24;
const TAG: usize = 16;
const AD_LEN: usize = 1 + 14 + 16 + 32 + 32;

fn code_invalid() -> AppError {
    AppError::Validation("validation.syncPairingCodeInvalid".into())
}

fn failed() -> AppError {
    AppError::Validation("validation.syncPairingFailed".into())
}

/// Whether this device already syncs (holds the vault) or wants to join one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Holder,
    Joiner,
}

impl Role {
    pub fn of(sync_on: bool) -> Role {
        if sync_on {
            Role::Holder
        } else {
            Role::Joiner
        }
    }

    pub fn byte(self) -> u8 {
        match self {
            Role::Holder => 0x01,
            Role::Joiner => 0x02,
        }
    }

    pub fn from_byte(b: u8) -> Option<Role> {
        match b {
            0x01 => Some(Role::Holder),
            0x02 => Some(Role::Joiner),
            _ => None,
        }
    }
}

/// Which end of the exchange this device is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Shower,
    Scanner,
}

/// The relay mailbox of one pairing: 16 random bytes, 22 base64url characters.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct MailboxId(pub [u8; 16]);

impl MailboxId {
    pub fn random() -> Result<Self> {
        let mut bytes = [0u8; 16];
        os_random(&mut bytes)?;
        Ok(MailboxId(bytes))
    }

    pub fn to_b64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    /// Only the canonical 22-character spelling.
    pub fn from_b64(s: &str) -> Result<Self> {
        let bytes: [u8; 16] = URL_SAFE_NO_PAD
            .decode(s.as_bytes())
            .map_err(|_| code_invalid())?
            .try_into()
            .map_err(|_| code_invalid())?;
        let id = MailboxId(bytes);
        if id.to_b64() != s {
            return Err(code_invalid());
        }
        Ok(id)
    }
}

impl fmt::Debug for MailboxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("MailboxId(<redacted>)")
    }
}

/// What the QR code carries: the mailbox, the shower's public key and, for a sync server that is
/// not the default, its address.
pub struct PairingCode {
    pub mailbox: MailboxId,
    pub shower_public: [u8; 32],
    pub relay_url: Option<String>,
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingCode(<redacted>)")
    }
}

/// `moony-pair:1:<mailbox>:<S public>[:<relay>]`, every field base64url without padding. The
/// fourth field is written only for a relay that is not the default. Infallible: the caller
/// passes the stored, already-normalized relay address (`normalize_base_url`).
pub fn pairing_code_text(code: &PairingCode) -> String {
    let mut text = format!(
        "{QR_PREFIX}{}:{}",
        code.mailbox.to_b64(),
        URL_SAFE_NO_PAD.encode(code.shower_public)
    );
    if let Some(url) = code.relay_url.as_deref().filter(|u| !is_default_relay(u)) {
        debug_assert!(
            normalize_base_url(url).is_ok_and(|n| n == url),
            "pairing_code_text needs a normalized relay address"
        );
        text.push(':');
        text.push_str(&URL_SAFE_NO_PAD.encode(url));
    }
    text
}

/// Parses a pairing code. Malformed input is `validation.syncPairingCodeInvalid`; a relay address
/// the app does not use is `validation.syncRelayUrlInvalid`. The default relay reads as `None`.
/// No error carries the input.
pub fn parse_pairing_code(text: &str) -> Result<PairingCode> {
    if text.len() > MAX_CODE_BYTES {
        return Err(code_invalid());
    }
    let rest = text.strip_prefix(QR_PREFIX).ok_or_else(code_invalid)?;
    let fields: Vec<&str> = rest.split(':').collect();
    if !(2..=3).contains(&fields.len()) {
        return Err(code_invalid());
    }
    let mailbox = MailboxId::from_b64(fields[0])?;
    let shower_public: [u8; 32] = URL_SAFE_NO_PAD
        .decode(fields[1].as_bytes())
        .map_err(|_| code_invalid())?
        .try_into()
        .map_err(|_| code_invalid())?;
    if URL_SAFE_NO_PAD.encode(shower_public) != fields[1] {
        return Err(code_invalid());
    }
    let relay_url = match fields.get(2) {
        None => None,
        Some(encoded) => {
            let bytes = URL_SAFE_NO_PAD
                .decode(encoded.as_bytes())
                .map_err(|_| code_invalid())?;
            let url = String::from_utf8(bytes).map_err(|_| code_invalid())?;
            if url.is_empty() {
                return Err(code_invalid());
            }
            Some(normalize_base_url(&url)?).filter(|url| !is_default_relay(url))
        }
    };
    Ok(PairingCode {
        mailbox,
        shower_public,
        relay_url,
    })
}

/// Slot 1, the scanner's reply: `0x01 ‖ role ‖ C public`.
pub fn slot1(role: Role, scanner_public: &[u8; 32]) -> [u8; SLOT1_LEN] {
    let mut slot = [0u8; SLOT1_LEN];
    slot[0] = SLOT_VERSION;
    slot[1] = role.byte();
    slot[2..].copy_from_slice(scanner_public);
    slot
}

pub fn parse_slot1(bytes: &[u8]) -> Result<(Role, [u8; 32])> {
    if bytes.len() != SLOT1_LEN || bytes[0] != SLOT_VERSION {
        return Err(failed());
    }
    let role = Role::from_byte(bytes[1]).ok_or_else(failed)?;
    let mut public = [0u8; 32];
    public.copy_from_slice(&bytes[2..]);
    Ok((role, public))
}

/// One device must hold the vault and the other must want to join it.
pub fn check_roles(shower: Role, scanner: Role) -> Result<()> {
    match (shower, scanner) {
        (Role::Holder, Role::Holder) => {
            Err(AppError::Validation("validation.syncPairingBothOn".into()))
        }
        (Role::Joiner, Role::Joiner) => {
            Err(AppError::Validation("validation.syncPairingBothOff".into()))
        }
        _ => Ok(()),
    }
}

/// Exactly `0x01 0x00`, on either slot. The session checks it on every slot before parsing.
pub fn is_cancel_marker(bytes: &[u8]) -> bool {
    bytes == CANCEL_MARKER
}

/// The session key, the comparison code and the slot-2 associated data of one exchange.
pub struct Transcript {
    key: Zeroizing<[u8; 32]>,
    code: u32,
    ad: [u8; AD_LEN],
}

impl Transcript {
    pub fn code(&self) -> u32 {
        self.code
    }

    /// Six digits, zero-padded.
    pub fn code_text(&self) -> String {
        format!("{:06}", self.code)
    }

    #[cfg(test)]
    fn key_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    #[cfg(test)]
    fn ad(&self) -> &[u8; AD_LEN] {
        &self.ad
    }
}

impl fmt::Debug for Transcript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Transcript(<redacted>)")
    }
}

/// X25519 between `own` and `peer_public`, then
/// `HKDF-SHA256(salt = mailbox, ikm = shared, info = "moony pair v1" ‖ S public ‖ C public)` of 40
/// bytes: the first 32 are the session key, the last 8 (big-endian, mod 10^6) the code. `own`'s
/// public key takes the place `side` names and the peer's the other. A low-order peer key (a
/// non-contributory exchange) is refused.
///
/// The `hkdf` crate's PRK is not zeroized (a crate limit); the OKM and the shared secret are.
pub fn derive(
    own: &StaticSecret,
    side: Side,
    peer_public: &[u8; 32],
    mailbox: &MailboxId,
) -> Result<Transcript> {
    let own_public = PublicKey::from(own).to_bytes();
    let (s_pub, c_pub) = match side {
        Side::Shower => (own_public, *peer_public),
        Side::Scanner => (*peer_public, own_public),
    };
    let shared = own.diffie_hellman(&PublicKey::from(*peer_public));
    if !shared.was_contributory() {
        return Err(failed());
    }
    let mut info = Vec::with_capacity(KDF_INFO.len() + 64);
    info.extend_from_slice(KDF_INFO);
    info.extend_from_slice(&s_pub);
    info.extend_from_slice(&c_pub);
    let mut okm = Zeroizing::new([0u8; 40]);
    Hkdf::<Sha256>::new(Some(&mailbox.0), shared.as_bytes())
        .expand(&info, &mut okm[..])
        .expect("40 bytes is within HKDF-SHA256's output limit");
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&okm[..32]);
    let mut tail = [0u8; 8];
    tail.copy_from_slice(&okm[32..]);
    let code = (u64::from_be_bytes(tail) % 1_000_000) as u32;

    let mut ad = [0u8; AD_LEN];
    ad[0] = SLOT_VERSION;
    ad[1..15].copy_from_slice(SLOT2_DIRECTION);
    ad[15..31].copy_from_slice(&mailbox.0);
    ad[31..63].copy_from_slice(&s_pub);
    ad[63..95].copy_from_slice(&c_pub);
    Ok(Transcript { key, code, ad })
}

/// Slot 2: `0x01 ‖ nonce (24) ‖ XChaCha20-Poly1305(session key, AD, payload JSON)`.
pub fn seal_credentials(t: &Transcript, creds: &Credentials) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE];
    os_random(&mut nonce)?;
    seal_credentials_with_nonce(t, creds, nonce)
}

fn seal_credentials_with_nonce(
    t: &Transcript,
    creds: &Credentials,
    nonce: [u8; NONCE],
) -> Result<Vec<u8>> {
    let json = payload_json(creds)?;
    seal_bytes(t, json.as_bytes(), nonce)
}

fn seal_bytes(t: &Transcript, plain: &[u8], nonce: [u8; NONCE]) -> Result<Vec<u8>> {
    let sealed = XChaCha20Poly1305::new((&*t.key).into())
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: plain,
                aad: &t.ad,
            },
        )
        .map_err(|_| AppError::Internal("sync pairing: encryption failed".into()))?;
    let mut slot = Vec::with_capacity(1 + NONCE + sealed.len());
    slot.push(SLOT_VERSION);
    slot.extend_from_slice(&nonce);
    slot.extend_from_slice(&sealed);
    Ok(slot)
}

/// `{"v":1,"relayUrl":"…","accountNumber":"…","vaultId":"…","keys":[{"epoch":0,"key":"…"}]}`,
/// keys in ascending epoch, built in one wiped buffer.
fn payload_json(creds: &Credentials) -> Result<Zeroizing<String>> {
    let relay = normalize_base_url(&creds.relay_url)?;
    if !valid_account(&creds.account) {
        return Err(AppError::Internal(
            "sync pairing: the account number is not 16 digits".into(),
        ));
    }
    let count = creds.keys.epochs().count();
    // Reserved up front so the buffer never reallocates and leaves an unwiped copy behind.
    let mut json = Zeroizing::new(String::with_capacity(128 + relay.len() + 80 * count));
    json.push_str("{\"v\":1,\"relayUrl\":\"");
    json.push_str(&relay);
    json.push_str("\",\"accountNumber\":\"");
    json.push_str(&creds.account);
    json.push_str("\",\"vaultId\":\"");
    json.push_str(&creds.vault_id.to_b64());
    json.push_str("\",\"keys\":[");
    for (i, (epoch, key)) in creds.keys.epochs().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str("{\"epoch\":");
        json.push_str(&epoch.to_string());
        json.push_str(",\"key\":\"");
        URL_SAFE_NO_PAD.encode_string(key.as_slice(), &mut json);
        json.push_str("\"}");
    }
    json.push_str("]}");
    Ok(json)
}

fn valid_account(account: &str) -> bool {
    account.len() == 16 && account.bytes().all(|b| b.is_ascii_digit())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PayloadIn {
    v: u32,
    #[serde(rename = "relayUrl")]
    relay_url: String,
    #[serde(rename = "accountNumber")]
    account_number: SecretString,
    #[serde(rename = "vaultId")]
    vault_id: SecretString,
    keys: Vec<KeyIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyIn {
    epoch: u32,
    key: SecretString,
}

/// Decoded keys on their way into `VaultKeys`; wiped on every exit (`from_parts` wipes what it
/// is given, so it takes the map out first).
struct WipedKeys(BTreeMap<u32, [u8; 32]>);

impl Drop for WipedKeys {
    fn drop(&mut self) {
        self.0.values_mut().for_each(zeroize::Zeroize::zeroize);
    }
}

/// Opens slot 2. Any failure to open or to trust the payload is `validation.syncPairingFailed`,
/// except a relay address the app does not use (`validation.syncRelayUrlInvalid`).
pub fn open_credentials(t: &Transcript, sealed: &[u8]) -> Result<Credentials> {
    // Too short, or longer than any legal slot: refused before the AEAD runs.
    if sealed.len() < 1 + NONCE + TAG
        || sealed.len() > 1 + NONCE + MAX_PAYLOAD_JSON + TAG
        || sealed[0] != SLOT_VERSION
    {
        return Err(failed());
    }
    let mut nonce = [0u8; NONCE];
    nonce.copy_from_slice(&sealed[1..1 + NONCE]);
    let plain = Zeroizing::new(
        XChaCha20Poly1305::new((&*t.key).into())
            .decrypt(
                (&nonce).into(),
                Payload {
                    msg: &sealed[1 + NONCE..],
                    aad: &t.ad,
                },
            )
            .map_err(|_| failed())?,
    );
    if plain.len() > MAX_PAYLOAD_JSON {
        return Err(failed());
    }
    let json = std::str::from_utf8(&plain).map_err(|_| failed())?;
    // The serde error may quote the value, so it is dropped.
    let payload: PayloadIn = serde_json::from_str(json).map_err(|_| failed())?;
    if payload.v != 1 || !valid_account(&payload.account_number.0) {
        return Err(failed());
    }
    let vault_id = VaultId::from_b64(&payload.vault_id.0).map_err(|_| failed())?;
    if vault_id.to_b64() != *payload.vault_id.0 {
        return Err(failed());
    }
    if payload.keys.is_empty() || payload.keys.len() > MAX_PAYLOAD_KEYS {
        return Err(failed());
    }
    let relay_url = normalize_base_url(&payload.relay_url)?;
    let mut keys = WipedKeys(BTreeMap::new());
    for entry in &payload.keys {
        let decoded = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(entry.key.0.as_bytes())
                .map_err(|_| failed())?,
        );
        if decoded.len() != 32 {
            return Err(failed());
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&decoded);
        if keys.0.insert(entry.epoch, key).is_some() {
            return Err(failed());
        }
    }
    let current = *keys.0.keys().next_back().ok_or_else(failed)?;
    Ok(Credentials {
        relay_url,
        account: payload.account_number.0.to_string(),
        vault_id,
        keys: VaultKeys::from_parts(current, std::mem::take(&mut keys.0))?,
    })
}
