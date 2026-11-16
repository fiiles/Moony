//! The Kit envelope (phase 5 revision §2.4): the Recovery Kit's copy of every vault key, sealed
//! to the Kit's box key by whoever makes a key change and stored opaque on the sync server, at
//! most 256 KiB. All integers are big-endian.
//!
//! ```text
//! header = "MNYE" ‖ 0x01 ‖ vault_id (16) ‖ kit_id (16) ‖ epoch (u32) ‖ eph_pub (32)
//! key    = HKDF-SHA256(salt = vault_id, ikm = X25519(eph_secret, kit_box_pub),
//!                      info = "moony kit envelope v1" ‖ eph_pub ‖ kit_box_pub ‖ epoch)
//! body   = nonce (24) ‖ XChaCha20-Poly1305(key, AAD = header, plain)
//! plain  = sig (64) ‖ JSON
//! sig    = Ed25519(signer, "moony kit envelope v1" ‖ header ‖ JSON)
//! ```
//!
//! The JSON is `{"v":1,"by":"<hex>","at":n,"signer":"<cert b64>","keys":[{"epoch":n,
//! "key":"<b64url>","head":h?}],"confirmed":[{"device":"<hex>","cert":"<hex SHA-256 of the
//! certificate>"}]}`: `keys` every epoch the sealer holds, strictly ascending, the last one the
//! header's `epoch`, a head only where the sealer verified it; `confirmed` the sealer's confirmed
//! set after its key change commits. One ephemeral X25519 pair per envelope.
//!
//! `open` checks the bytes, opens them with the Kit's box secret and checks the signature with
//! the certificate the envelope bundles; whether that signer may speak is the Kit joiner's
//! question (revision §2.6 step 6). Keys and every buffer holding the JSON are `Zeroizing`;
//! `Debug` prints `<redacted>` for keys.

use std::collections::BTreeSet;
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

use super::bundle::OldKey;
use crate::error::{AppError, Result};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::{sign_pub, verify_sig, Certificate, KitKeys, SignSeed};
use crate::services::sync::keys::SecretString;

const MAGIC: &[u8; 4] = b"MNYE";
const VERSION: u8 = 0x01;
/// The seal key's HKDF info prefix and the signature's domain string (one string, revision §2.4).
const DOMAIN: &[u8] = b"moony kit envelope v1";
const JSON_VERSION: u32 = 1;

pub const HEADER_LEN: usize = 4 + 1 + 16 + 16 + 4 + 32;
const NONCE: usize = 24;
const TAG: usize = 16;
/// The server's cap on a stored envelope.
pub const MAX_ENVELOPE_BYTES: usize = 256 * 1024;

fn invalid() -> AppError {
    AppError::Internal("sync: a Kit envelope is not valid".into())
}

fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(bytes);
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub vault_id: [u8; 16],
    pub kit_id: DeviceId,
    /// The newest epoch the envelope holds a key for.
    pub epoch: u32,
    pub eph_pub: [u8; 32],
}

impl Header {
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[..4].copy_from_slice(MAGIC);
        out[4] = VERSION;
        out[5..21].copy_from_slice(&self.vault_id);
        out[21..37].copy_from_slice(&self.kit_id.0);
        out[37..41].copy_from_slice(&self.epoch.to_be_bytes());
        out[41..].copy_from_slice(&self.eph_pub);
        out
    }

    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_LEN || &bytes[..4] != MAGIC || bytes[4] != VERSION {
            return Err(invalid());
        }
        Ok(Header {
            vault_id: array(&bytes[5..21]),
            kit_id: DeviceId(array(&bytes[21..37])),
            epoch: u32::from_be_bytes(array(&bytes[37..41])),
            eph_pub: array(&bytes[41..HEADER_LEN]),
        })
    }
}

/// One item of the confirmed set: a device and the SHA-256 of the certificate it was confirmed
/// with (`Certificate::hash`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Confirmed {
    pub device: DeviceId,
    pub cert_hash: [u8; 32],
}

/// What an envelope is sealed from. `keys` is every epoch the sealer holds, ascending; the last
/// one becomes the header's epoch.
pub struct EnvelopeInput<'a> {
    pub vault_id: [u8; 16],
    pub kit_id: DeviceId,
    pub kit_box_pub: [u8; 32],
    pub signer: &'a SignSeed,
    pub signer_cert: &'a Certificate,
    pub at: i64,
    pub keys: &'a [OldKey],
    pub confirmed: &'a [Confirmed],
}

/// An opened envelope whose signature verified with the certificate it bundles.
pub struct Envelope {
    pub header: Header,
    /// The signer's device (the bundled certificate's).
    pub by: DeviceId,
    pub at: i64,
    pub signer: Certificate,
    /// Strictly ascending; the last is `header.epoch`.
    pub keys: Vec<OldKey>,
    pub confirmed: Vec<Confirmed>,
}

impl fmt::Debug for Envelope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Envelope")
            .field("header", &self.header)
            .field("by", &self.by)
            .field("at", &self.at)
            .field("keys", &self.keys)
            .field("confirmed", &self.confirmed.len())
            .finish_non_exhaustive()
    }
}

fn seal_key(
    shared: &[u8; 32],
    eph_pub: &[u8; 32],
    kit_box_pub: &[u8; 32],
    vault_id: &[u8; 16],
    epoch: u32,
) -> Zeroizing<[u8; 32]> {
    let mut info = Vec::with_capacity(DOMAIN.len() + 32 + 32 + 4);
    info.extend_from_slice(DOMAIN);
    info.extend_from_slice(eph_pub);
    info.extend_from_slice(kit_box_pub);
    info.extend_from_slice(&epoch.to_be_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(vault_id), shared)
        .expand(&info, &mut key[..])
        .expect("32 bytes is within HKDF-SHA256's output limit");
    key
}

/// What the signature signs, in a wiped buffer (the JSON carries the keys).
fn signed_message(header: &[u8; HEADER_LEN], json: &[u8]) -> Zeroizing<Vec<u8>> {
    let mut message = Zeroizing::new(Vec::with_capacity(DOMAIN.len() + HEADER_LEN + json.len()));
    message.extend_from_slice(DOMAIN);
    message.extend_from_slice(header);
    message.extend_from_slice(json);
    message
}

/// The JSON in one wiped buffer (it carries the keys), reserved up front so it never
/// reallocates.
pub(super) fn envelope_json(input: &EnvelopeInput<'_>) -> Zeroizing<String> {
    let signer = input.signer_cert.to_b64();
    let mut json = Zeroizing::new(String::with_capacity(
        256 + signer.len() + 112 * input.keys.len() + 128 * input.confirmed.len(),
    ));
    json.push_str("{\"v\":");
    json.push_str(&JSON_VERSION.to_string());
    json.push_str(",\"by\":\"");
    json.push_str(&input.signer_cert.body.device.to_hex());
    json.push_str("\",\"at\":");
    json.push_str(&input.at.to_string());
    json.push_str(",\"signer\":\"");
    json.push_str(&signer);
    json.push_str("\",\"keys\":[");
    for (i, old) in input.keys.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str("{\"epoch\":");
        json.push_str(&old.epoch.to_string());
        json.push_str(",\"key\":\"");
        URL_SAFE_NO_PAD.encode_string(old.key.as_slice(), &mut json);
        json.push('"');
        if let Some(head) = old.head {
            json.push_str(",\"head\":");
            json.push_str(&head.to_string());
        }
        json.push('}');
    }
    json.push_str("],\"confirmed\":[");
    for (i, item) in input.confirmed.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str("{\"device\":\"");
        json.push_str(&item.device.to_hex());
        json.push_str("\",\"cert\":\"");
        json.push_str(&hex::encode(item.cert_hash));
        json.push_str("\"}");
    }
    json.push_str("]}");
    json
}

/// Seals an envelope. `fill` gives every random byte (the ephemeral secret, then the nonce), so a
/// test can fix them; production passes `bundle::os_fill`. Refuses no keys, keys not strictly
/// ascending, a signer certificate that is not `signer`'s or of another vault, two confirmed
/// items for one device, a low-order Kit box key and a result over `MAX_ENVELOPE_BYTES`.
pub fn build(
    input: &EnvelopeInput<'_>,
    fill: &mut dyn FnMut(&mut [u8]) -> Result<()>,
) -> Result<Vec<u8>> {
    let json = envelope_json(input);
    build_parts(input, json.as_bytes(), json.as_bytes(), fill)
}

/// `build` with the JSON that is signed and the JSON that is sealed given apart: tests make
/// envelopes the build would never write.
pub(super) fn build_parts(
    input: &EnvelopeInput<'_>,
    signed_json: &[u8],
    sealed_json: &[u8],
    fill: &mut dyn FnMut(&mut [u8]) -> Result<()>,
) -> Result<Vec<u8>> {
    let refuse = |what: &str| AppError::Internal(format!("sync: a Kit envelope {what}"));
    let epoch = input
        .keys
        .last()
        .map(|newest| newest.epoch)
        .ok_or_else(|| refuse("holds no key"))?;
    if input
        .keys
        .windows(2)
        .any(|pair| pair[0].epoch >= pair[1].epoch)
    {
        return Err(refuse("holds keys out of order"));
    }
    if input.signer_cert.body.sign_pub != sign_pub(input.signer)
        || input.signer_cert.body.vault_id != input.vault_id
    {
        return Err(refuse("names another signer"));
    }
    let devices: BTreeSet<DeviceId> = input.confirmed.iter().map(|c| c.device).collect();
    if devices.len() != input.confirmed.len() {
        return Err(refuse("names a confirmed device twice"));
    }
    let mut eph = Zeroizing::new([0u8; 32]);
    fill(&mut eph[..])?;
    let eph_secret = StaticSecret::from(*eph);
    let eph_pub = PublicKey::from(&eph_secret).to_bytes();
    let shared = eph_secret.diffie_hellman(&PublicKey::from(input.kit_box_pub));
    if !shared.was_contributory() {
        return Err(refuse("names a low-order Kit key"));
    }
    let header = Header {
        vault_id: input.vault_id,
        kit_id: input.kit_id,
        epoch,
        eph_pub,
    }
    .encode();
    let key = seal_key(
        shared.as_bytes(),
        &eph_pub,
        &input.kit_box_pub,
        &input.vault_id,
        epoch,
    );
    let sig = input.signer.sign(&signed_message(&header, signed_json));
    let mut plain = Zeroizing::new(Vec::with_capacity(64 + sealed_json.len()));
    plain.extend_from_slice(&sig);
    plain.extend_from_slice(sealed_json);
    let mut nonce = [0u8; NONCE];
    fill(&mut nonce)?;
    let sealed = XChaCha20Poly1305::new((&*key).into())
        .encrypt(
            (&nonce).into(),
            Payload {
                msg: &plain,
                aad: &header,
            },
        )
        .map_err(|_| refuse("could not be sealed"))?;
    let mut out = Vec::with_capacity(HEADER_LEN + NONCE + sealed.len());
    out.extend_from_slice(&header);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    if out.len() > MAX_ENVELOPE_BYTES {
        return Err(refuse("is over 256 KiB"));
    }
    Ok(out)
}

/// The header of a well-formed envelope, read without opening anything: not authenticated.
pub fn peek(bytes: &[u8]) -> Result<Header> {
    if bytes.len() > MAX_ENVELOPE_BYTES || bytes.len() < HEADER_LEN + NONCE + TAG + 64 {
        return Err(invalid());
    }
    Header::decode(bytes)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvelopeIn {
    v: u32,
    by: String,
    at: i64,
    signer: String,
    keys: Vec<KeyIn>,
    confirmed: Vec<ConfirmedIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyIn {
    epoch: u32,
    key: SecretString,
    #[serde(default)]
    head: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmedIn {
    device: String,
    cert: String,
}

/// A lowercase 32-hex device ID, exactly.
fn device_of(hex: &str) -> Result<DeviceId> {
    let device = DeviceId::from_hex(hex).map_err(|_| invalid())?;
    (device.to_hex() == hex)
        .then_some(device)
        .ok_or_else(invalid)
}

/// A lowercase 64-hex SHA-256, exactly.
fn hash_of(text: &str) -> Result<[u8; 32]> {
    let bytes = hex::decode(text).map_err(|_| invalid())?;
    let hash: [u8; 32] = bytes.try_into().map_err(|_| invalid())?;
    (hex::encode(hash) == text)
        .then_some(hash)
        .ok_or_else(invalid)
}

/// Opens an envelope with the Kit's keys (revision §2.6 step 4): the header names this vault and
/// this Kit; the body opens with the Kit's box secret; the signature verifies with the bundled
/// signer certificate (which verifies on its own, is of this vault and is `by`'s); the keys are
/// 32 bytes, strictly ascending, the last one the header's epoch; the confirmed items name each
/// device once. Anything else is an error.
pub fn open(bytes: &[u8], kit: &KitKeys) -> Result<Envelope> {
    let header = peek(bytes)?;
    if &header.vault_id != kit.vault_id() || header.kit_id != kit.id() {
        return Err(invalid());
    }
    let header_bytes: [u8; HEADER_LEN] = array(&bytes[..HEADER_LEN]);
    let shared = kit
        .box_secret()
        .diffie_hellman(&PublicKey::from(header.eph_pub));
    if !shared.was_contributory() {
        return Err(invalid());
    }
    let key = seal_key(
        shared.as_bytes(),
        &header.eph_pub,
        kit.box_pub(),
        &header.vault_id,
        header.epoch,
    );
    let nonce: [u8; NONCE] = array(&bytes[HEADER_LEN..HEADER_LEN + NONCE]);
    let plain = XChaCha20Poly1305::new((&*key).into())
        .decrypt(
            (&nonce).into(),
            Payload {
                msg: &bytes[HEADER_LEN + NONCE..],
                aad: &header_bytes,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| invalid())?;
    if plain.len() < 64 {
        return Err(invalid());
    }
    let sig: [u8; 64] = array(&plain[..64]);
    let json = &plain[64..];
    let parsed: EnvelopeIn = serde_json::from_slice(json).map_err(|_| invalid())?;
    if parsed.v != JSON_VERSION {
        return Err(invalid());
    }
    let by = device_of(&parsed.by)?;
    let signer = Certificate::from_b64(&parsed.signer).map_err(|_| invalid())?;
    if signer.body.device != by || signer.body.vault_id != header.vault_id {
        return Err(invalid());
    }
    signer.verify_signatures().map_err(|_| invalid())?;
    verify_sig(
        &signer.body.sign_pub,
        &signed_message(&header_bytes, json),
        &sig,
    )
    .map_err(|_| invalid())?;
    let mut keys: Vec<OldKey> = Vec::with_capacity(parsed.keys.len());
    for entry in &parsed.keys {
        let decoded = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(entry.key.0.as_bytes())
                .map_err(|_| invalid())?,
        );
        let mut key = Zeroizing::new([0u8; 32]);
        let ascending = keys.last().is_none_or(|last| last.epoch < entry.epoch);
        if decoded.len() != key.len() || !ascending {
            return Err(invalid());
        }
        key.copy_from_slice(&decoded);
        keys.push(OldKey {
            epoch: entry.epoch,
            key,
            head: entry.head,
        });
    }
    if keys.last().map(|newest| newest.epoch) != Some(header.epoch) {
        return Err(invalid());
    }
    let mut confirmed = Vec::with_capacity(parsed.confirmed.len());
    let mut seen = BTreeSet::new();
    for item in &parsed.confirmed {
        let device = device_of(&item.device)?;
        if !seen.insert(device) {
            return Err(invalid());
        }
        confirmed.push(Confirmed {
            device,
            cert_hash: hash_of(&item.cert)?,
        });
    }
    Ok(Envelope {
        header,
        by,
        at: parsed.at,
        signer,
        keys,
        confirmed,
    })
}
