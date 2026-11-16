//! The key-change bundle (phase 5 spec §6.2, §6.3): opaque to the sync server, at most 256 KiB.
//! All integers are big-endian.
//!
//! ```text
//! header  = "MNYK" ‖ 0x01 ‖ vault_id (16) ‖ from_epoch (u32) ‖ to_epoch (u32) ‖ eph_pub (32)
//! entries = count (u16) ‖ count × ( len (u16) ‖ nonce (24) ‖ ciphertext+tag (len) )
//!   entry key_i = HKDF-SHA256(salt = vault_id, ikm = X25519(eph_secret, box_pub_i),
//!                             info = "moony sync rekey seal v1" ‖ eph_pub ‖ box_pub_i ‖ to_epoch)
//!   entry AAD   = header ‖ index (u16)
//!   entry plain = 0x01 ‖ new_key (32)          a recipient
//!               | 0x02 ‖ removal record         a removal notice (`identity::record`)
//! body    = nonce (24) ‖ XChaCha20-Poly1305 ciphertext+tag
//!   body key   = HKDF-SHA256(salt = vault_id, ikm = new_key, info = "moony sync rekey body v1")
//!   body AAD   = header ‖ SHA-256(entries)
//!   body plain = sig (64) ‖ JSON
//!   sig        = Ed25519(signer, "moony rekey v1" ‖ header ‖ SHA-256(entries) ‖ JSON)
//! ```
//!
//! `len` counts the ciphertext with its tag (the nonce has a fixed length). The JSON is
//! `{"v":1,"by":"<hex>","at":n,"reason":"remove"|"rotate","signer":"<cert b64>",
//! "recipients":[{"device":"<hex>","cert":"<b64>"}],"removed":[{"device":"<hex>",
//! "record":"<b64>"}],"keys":[{"epoch":n,"key":"<b64>","head":h}]}`, `keys` carrying every
//! epoch up to `from_epoch` (a head the signer does not know is left out). Entries are found by
//! trial decryption, so the server learns their count and nothing else; one ephemeral X25519
//! pair per bundle; entries are bound to the header and their index, and the body to every entry.
//!
//! This module builds and opens the bytes and checks the bundle's own signature: an opened body
//! is `Unverified`, and its new key, older keys and contents are reachable only through
//! `Unverified::verify` with the signer's key, which gives `Verified`. Whether that signer may
//! sign a key change is the caller's question (Task 10): its certificate must be the one pinned
//! here and `trusted`, and `to_epoch` must be this device's epoch + 1;
//! what the bundle then teaches is `identity::trust::import_bundle`'s. New keys, the ephemeral
//! secret, the body's older keys and every buffer holding the body JSON live in `Zeroizing`
//! buffers, and `Debug` prints `<redacted>`.

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::XChaCha20Poly1305;
use hkdf::Hkdf;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use crate::error::{AppError, Result};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::{sign_pub, verify_sig, Certificate, Record, SignSeed};
use crate::services::sync::keys::{os_random, SecretString};

const MAGIC: &[u8; 4] = b"MNYK";
const VERSION: u8 = 0x01;
const SEAL_INFO: &[u8] = b"moony sync rekey seal v1";
const BODY_INFO: &[u8] = b"moony sync rekey body v1";
const SIG_DOMAIN: &[u8] = b"moony rekey v1";
const KIND_KEY: u8 = 0x01;
const KIND_NOTICE: u8 = 0x02;

pub const HEADER_LEN: usize = 4 + 1 + 16 + 4 + 4 + 32;
const NONCE: usize = 24;
const TAG: usize = 16;
/// The relay's cap on a stored bundle.
pub const MAX_BUNDLE_BYTES: usize = 256 * 1024;
/// At most this many recipients of a new key (spec §5.4) …
pub const MAX_RECIPIENT_ENTRIES: usize = 64;
/// … and at most this many removal notices in one bundle.
pub const MAX_NOTICE_ENTRIES: usize = 64;
const MAX_ENTRIES: usize = MAX_RECIPIENT_ENTRIES + MAX_NOTICE_ENTRIES;
const JSON_VERSION: u32 = 1;

fn invalid() -> AppError {
    AppError::Internal("sync: a key-change bundle is not valid".into())
}

/// Why the key changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    /// Devices were removed.
    Remove,
    /// The key changed with nobody removed (spec §6.6).
    Rotate,
}

impl Reason {
    fn as_str(self) -> &'static str {
        match self {
            Reason::Remove => "remove",
            Reason::Rotate => "rotate",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "remove" => Ok(Reason::Remove),
            "rotate" => Ok(Reason::Rotate),
            _ => Err(invalid()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub vault_id: [u8; 16],
    pub from_epoch: u32,
    pub to_epoch: u32,
    pub eph_pub: [u8; 32],
}

impl Header {
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut out = [0u8; HEADER_LEN];
        out[..4].copy_from_slice(MAGIC);
        out[4] = VERSION;
        out[5..21].copy_from_slice(&self.vault_id);
        out[21..25].copy_from_slice(&self.from_epoch.to_be_bytes());
        out[25..29].copy_from_slice(&self.to_epoch.to_be_bytes());
        out[29..].copy_from_slice(&self.eph_pub);
        out
    }

    /// The magic, the version and `to_epoch = from_epoch + 1`.
    fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < HEADER_LEN || &bytes[..4] != MAGIC || bytes[4] != VERSION {
            return Err(invalid());
        }
        let header = Header {
            vault_id: array(&bytes[5..21]),
            from_epoch: u32::from_be_bytes(array(&bytes[21..25])),
            to_epoch: u32::from_be_bytes(array(&bytes[25..29])),
            eph_pub: array(&bytes[29..HEADER_LEN]),
        };
        if header.from_epoch.checked_add(1) != Some(header.to_epoch) {
            return Err(invalid());
        }
        Ok(header)
    }
}

fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(bytes);
    out
}

/// A device that receives the new key, sealed to its pinned box key.
#[derive(Clone, Debug)]
pub struct Recipient {
    pub box_pub: [u8; 32],
    pub cert: Certificate,
}

/// A removed (or left) device's notice, sealed to its pinned box key.
#[derive(Clone, Debug)]
pub struct Notice {
    pub box_pub: [u8; 32],
    pub record: Record,
}

/// An older epoch's key and the relay position it started at, carried in the body.
pub struct OldKey {
    pub epoch: u32,
    pub key: Zeroizing<[u8; 32]>,
    pub head: Option<u64>,
}

impl fmt::Debug for OldKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "OldKey {{ epoch: {}, key: <redacted>, head: {:?} }}",
            self.epoch, self.head
        )
    }
}

/// What a bundle is built from. `recipients` and `notices` become entries in their order
/// (recipients first); `removed` are the body's removal records; `keys` every epoch up to
/// `from_epoch`.
pub struct BundleInput<'a> {
    pub vault_id: [u8; 16],
    pub from_epoch: u32,
    pub to_epoch: u32,
    pub new_key: &'a Zeroizing<[u8; 32]>,
    pub recipients: &'a [Recipient],
    pub notices: &'a [Notice],
    pub signer: &'a SignSeed,
    pub signer_cert: &'a Certificate,
    pub at: i64,
    pub reason: Reason,
    pub removed: &'a [Record],
    pub keys: &'a [OldKey],
}

/// A bundle's opened body: who signed it, when and why, the certificates it endorses, the
/// removal records it carries and the older keys. Reached only through `Verified`.
pub struct BundleBody {
    pub by: DeviceId,
    pub at: i64,
    pub reason: Reason,
    pub signer: Certificate,
    pub recipients: Vec<(DeviceId, Certificate)>,
    pub removed: Vec<(DeviceId, Record)>,
    pub keys: Vec<OldKey>,
    sig: [u8; 64],
    /// `"moony rekey v1" ‖ header ‖ SHA-256(entries) ‖ JSON`: what `sig` signs (the JSON carries
    /// the older keys, so the buffer is wiped).
    signed: Zeroizing<Vec<u8>>,
}

/// A body that opened but whose signature is not checked yet. Only its header and its signer's
/// certificate (whose device is the body's `by`) can be read, to find the key to check it with;
/// the new key and everything else come out of `verify`.
pub struct Unverified {
    new_key: Zeroizing<[u8; 32]>,
    header: Header,
    body: Box<BundleBody>,
}

impl Unverified {
    pub fn header(&self) -> &Header {
        &self.header
    }

    /// The certificate the body names as its signer (`by` is its device). The caller looks the
    /// device up in its own store: the key to verify with is the pinned one, never this
    /// certificate's on its own word.
    pub fn signer(&self) -> &Certificate {
        &self.body.signer
    }

    /// The new key and the body, when the signature is `signer_pub`'s. The caller has also to
    /// check that this signer is pinned here with that key and `trusted`,
    /// and that `to_epoch` is its own epoch + 1.
    pub fn verify(self, signer_pub: &[u8; 32]) -> Result<Verified> {
        verify_sig(signer_pub, &self.body.signed, &self.body.sig).map_err(|_| invalid())?;
        Ok(Verified {
            new_key: self.new_key,
            header: self.header,
            body: self.body,
        })
    }
}

impl fmt::Debug for Unverified {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Unverified")
            .field("new_key", &"<redacted>")
            .field("header", &self.header)
            .field("by", &self.body.by)
            .finish_non_exhaustive()
    }
}

/// A body whose signature verified with the key the caller passed.
pub struct Verified {
    pub new_key: Zeroizing<[u8; 32]>,
    pub header: Header,
    pub body: Box<BundleBody>,
}

impl fmt::Debug for Verified {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Verified")
            .field("new_key", &"<redacted>")
            .field("header", &self.header)
            .field("body", &self.body)
            .finish()
    }
}

impl fmt::Debug for BundleBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BundleBody")
            .field("by", &self.by)
            .field("at", &self.at)
            .field("reason", &self.reason)
            .field("recipients", &self.recipients.len())
            .field("removed", &self.removed.len())
            .field("keys", &self.keys)
            .finish_non_exhaustive()
    }
}

/// What this device found in a bundle.
pub enum Opened {
    /// An entry sealed the new key to this device, and the body opened with it; its signature is
    /// not checked yet.
    Recipient(Unverified),
    /// An entry carried a removal notice for this device (its record is not checked here).
    Notice { header: Header, record: Record },
    /// No entry opens for this device.
    Nothing,
}

impl fmt::Debug for Opened {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Opened::Recipient(unverified) => f.debug_tuple("Recipient").field(unverified).finish(),
            Opened::Notice { header, record } => f
                .debug_struct("Notice")
                .field("header", header)
                .field("record", record)
                .finish(),
            Opened::Nothing => f.write_str("Nothing"),
        }
    }
}

/// The OS random source, as `build` takes it.
pub fn os_fill(buf: &mut [u8]) -> Result<()> {
    os_random(buf)
}

/// One entry's key: HKDF over the X25519 secret; `None` for a low-order box key (a
/// non-contributory exchange seals to nobody).
fn entry_key(
    eph_secret: &StaticSecret,
    eph_pub: &[u8; 32],
    box_pub: &[u8; 32],
    vault_id: &[u8; 16],
    to_epoch: u32,
) -> Option<Zeroizing<[u8; 32]>> {
    let shared = eph_secret.diffie_hellman(&PublicKey::from(*box_pub));
    if !shared.was_contributory() {
        return None;
    }
    Some(seal_key(
        shared.as_bytes(),
        eph_pub,
        box_pub,
        vault_id,
        to_epoch,
    ))
}

fn seal_key(
    shared: &[u8; 32],
    eph_pub: &[u8; 32],
    box_pub: &[u8; 32],
    vault_id: &[u8; 16],
    to_epoch: u32,
) -> Zeroizing<[u8; 32]> {
    let mut info = Vec::with_capacity(SEAL_INFO.len() + 32 + 32 + 4);
    info.extend_from_slice(SEAL_INFO);
    info.extend_from_slice(eph_pub);
    info.extend_from_slice(box_pub);
    info.extend_from_slice(&to_epoch.to_be_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(vault_id), shared)
        .expand(&info, &mut key[..])
        .expect("32 bytes is within HKDF-SHA256's output limit");
    key
}

fn body_key(new_key: &[u8; 32], vault_id: &[u8; 16]) -> Zeroizing<[u8; 32]> {
    let mut key = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(vault_id), new_key)
        .expand(BODY_INFO, &mut key[..])
        .expect("32 bytes is within HKDF-SHA256's output limit");
    key
}

fn entry_aad(header: &[u8; HEADER_LEN], index: u16) -> [u8; HEADER_LEN + 2] {
    let mut aad = [0u8; HEADER_LEN + 2];
    aad[..HEADER_LEN].copy_from_slice(header);
    aad[HEADER_LEN..].copy_from_slice(&index.to_be_bytes());
    aad
}

fn body_aad(header: &[u8; HEADER_LEN], entries_hash: &[u8; 32]) -> Vec<u8> {
    let mut aad = Vec::with_capacity(HEADER_LEN + 32);
    aad.extend_from_slice(header);
    aad.extend_from_slice(entries_hash);
    aad
}

/// What the signature signs, in a wiped buffer (the JSON carries the older keys), reserved up
/// front so it never reallocates.
fn signed_message(
    header: &[u8; HEADER_LEN],
    entries_hash: &[u8; 32],
    json: &[u8],
) -> Zeroizing<Vec<u8>> {
    let mut message = Zeroizing::new(Vec::with_capacity(
        SIG_DOMAIN.len() + HEADER_LEN + 32 + json.len(),
    ));
    message.extend_from_slice(SIG_DOMAIN);
    message.extend_from_slice(header);
    message.extend_from_slice(entries_hash);
    message.extend_from_slice(json);
    message
}

fn seal(key: &[u8; 32], nonce: &[u8; NONCE], plain: &[u8], aad: &[u8]) -> Result<Vec<u8>> {
    XChaCha20Poly1305::new(key.into())
        .encrypt(nonce.into(), Payload { msg: plain, aad })
        .map_err(|_| AppError::Internal("sync: a key-change bundle could not be sealed".into()))
}

fn open_sealed(
    key: &[u8; 32],
    nonce: &[u8],
    sealed: &[u8],
    aad: &[u8],
) -> Option<Zeroizing<Vec<u8>>> {
    let nonce: [u8; NONCE] = nonce.try_into().ok()?;
    XChaCha20Poly1305::new(key.into())
        .decrypt((&nonce).into(), Payload { msg: sealed, aad })
        .ok()
        .map(Zeroizing::new)
}

/// The body JSON in one wiped buffer (it carries the older keys).
pub(super) fn body_json(input: &BundleInput<'_>) -> Zeroizing<String> {
    let signer = input.signer_cert.to_b64();
    let recipients: Vec<(String, String)> = input
        .recipients
        .iter()
        .map(|r| (r.cert.body.device.to_hex(), r.cert.to_b64()))
        .collect();
    let removed: Vec<(String, String)> = input
        .removed
        .iter()
        .map(|r| (r.subject.to_hex(), r.to_b64()))
        .collect();
    let plain_len = signer.len()
        + recipients
            .iter()
            .map(|(d, c)| d.len() + c.len() + 32)
            .sum::<usize>()
        + removed
            .iter()
            .map(|(d, r)| d.len() + r.len() + 32)
            .sum::<usize>();
    // Reserved up front so the buffer never reallocates and leaves an unwiped copy behind.
    let mut json = Zeroizing::new(String::with_capacity(
        256 + plain_len + 96 * input.keys.len(),
    ));
    json.push_str("{\"v\":");
    json.push_str(&JSON_VERSION.to_string());
    json.push_str(",\"by\":\"");
    json.push_str(&input.signer_cert.body.device.to_hex());
    json.push_str("\",\"at\":");
    json.push_str(&input.at.to_string());
    json.push_str(",\"reason\":\"");
    json.push_str(input.reason.as_str());
    json.push_str("\",\"signer\":\"");
    json.push_str(&signer);
    json.push_str("\",\"recipients\":[");
    for (i, (device, cert)) in recipients.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str("{\"device\":\"");
        json.push_str(device);
        json.push_str("\",\"cert\":\"");
        json.push_str(cert);
        json.push_str("\"}");
    }
    json.push_str("],\"removed\":[");
    for (i, (device, record)) in removed.iter().enumerate() {
        if i > 0 {
            json.push(',');
        }
        json.push_str("{\"device\":\"");
        json.push_str(device);
        json.push_str("\",\"record\":\"");
        json.push_str(record);
        json.push_str("\"}");
    }
    json.push_str("],\"keys\":[");
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
    json.push_str("]}");
    json
}

/// Builds a bundle. `fill` gives every random byte (the ephemeral secret first, then each
/// entry's nonce in order, then the body's), so a test can fix them; production passes
/// `os_fill`. Refuses `to_epoch` other than `from_epoch + 1`, a signer certificate that is not
/// `signer`'s, more than `MAX_RECIPIENT_ENTRIES` recipients or `MAX_NOTICE_ENTRIES` notices, a
/// low-order box key, an older key at or past `to_epoch`, and a result over `MAX_BUNDLE_BYTES`.
pub fn build(
    input: &BundleInput<'_>,
    fill: &mut dyn FnMut(&mut [u8]) -> Result<()>,
) -> Result<Vec<u8>> {
    build_with_json(input, body_json(input), fill)
}

/// `build` with the body JSON given: tests make bodies the build would never write.
pub(super) fn build_with_json(
    input: &BundleInput<'_>,
    json: Zeroizing<String>,
    fill: &mut dyn FnMut(&mut [u8]) -> Result<()>,
) -> Result<Vec<u8>> {
    let refuse = |what: &str| AppError::Internal(format!("sync: a key-change bundle {what}"));
    if input.from_epoch.checked_add(1) != Some(input.to_epoch) {
        return Err(refuse("must move one epoch on"));
    }
    if input.signer_cert.body.sign_pub != sign_pub(input.signer) {
        return Err(refuse("names another signer"));
    }
    if input.recipients.len() > MAX_RECIPIENT_ENTRIES || input.notices.len() > MAX_NOTICE_ENTRIES {
        return Err(refuse("has too many entries"));
    }
    if input.keys.iter().any(|old| old.epoch >= input.to_epoch) {
        return Err(refuse("carries a key that is not older"));
    }
    let mut eph = Zeroizing::new([0u8; 32]);
    fill(&mut eph[..])?;
    let eph_secret = StaticSecret::from(*eph);
    let eph_pub = PublicKey::from(&eph_secret).to_bytes();
    let header = Header {
        vault_id: input.vault_id,
        from_epoch: input.from_epoch,
        to_epoch: input.to_epoch,
        eph_pub,
    }
    .encode();

    let count = input.recipients.len() + input.notices.len();
    let mut entries = Vec::with_capacity(2 + count * 220);
    entries.extend_from_slice(&to_u16(count)?.to_be_bytes());
    let mut plain = Zeroizing::new(Vec::with_capacity(1 + 160));
    let sealed_to = input
        .recipients
        .iter()
        .map(|r| (&r.box_pub, None))
        .chain(input.notices.iter().map(|n| (&n.box_pub, Some(&n.record))));
    for (index, (box_pub, record)) in sealed_to.enumerate() {
        plain.clear();
        match record {
            None => {
                plain.push(KIND_KEY);
                plain.extend_from_slice(input.new_key.as_slice());
            }
            Some(record) => {
                plain.push(KIND_NOTICE);
                plain.extend_from_slice(&record.encode());
            }
        }
        let key = entry_key(
            &eph_secret,
            &eph_pub,
            box_pub,
            &input.vault_id,
            input.to_epoch,
        )
        .ok_or_else(|| refuse("names a low-order box key"))?;
        let mut nonce = [0u8; NONCE];
        fill(&mut nonce)?;
        let sealed = seal(&key, &nonce, &plain, &entry_aad(&header, to_u16(index)?))?;
        entries.extend_from_slice(&to_u16(sealed.len())?.to_be_bytes());
        entries.extend_from_slice(&nonce);
        entries.extend_from_slice(&sealed);
    }
    drop(eph_secret);
    let entries_hash: [u8; 32] = Sha256::digest(&entries).into();

    let sig = input
        .signer
        .sign(&signed_message(&header, &entries_hash, json.as_bytes()));
    let mut body_plain = Zeroizing::new(Vec::with_capacity(64 + json.len()));
    body_plain.extend_from_slice(&sig);
    body_plain.extend_from_slice(json.as_bytes());
    let mut body_nonce = [0u8; NONCE];
    fill(&mut body_nonce)?;
    let body = seal(
        &body_key(input.new_key, &input.vault_id),
        &body_nonce,
        &body_plain,
        &body_aad(&header, &entries_hash),
    )?;

    let mut bundle = Vec::with_capacity(HEADER_LEN + entries.len() + NONCE + body.len());
    bundle.extend_from_slice(&header);
    bundle.extend_from_slice(&entries);
    bundle.extend_from_slice(&body_nonce);
    bundle.extend_from_slice(&body);
    if bundle.len() > MAX_BUNDLE_BYTES {
        return Err(refuse("is over 256 KiB"));
    }
    Ok(bundle)
}

/// A length or index as the u16 the bytes carry; refused when it does not fit.
fn to_u16(value: usize) -> Result<u16> {
    u16::try_from(value)
        .map_err(|_| AppError::Internal("sync: a key-change bundle part is too long".into()))
}

/// A bundle split into its parts, its sizes checked.
struct Parts<'a> {
    header: Header,
    header_bytes: [u8; HEADER_LEN],
    /// `(nonce, ciphertext+tag)` per entry, in order.
    entries: Vec<(&'a [u8], &'a [u8])>,
    entries_hash: [u8; 32],
    body_nonce: &'a [u8],
    body: &'a [u8],
}

fn split<'a>(bytes: &'a [u8], vault_id: &[u8; 16]) -> Result<Parts<'a>> {
    if bytes.len() > MAX_BUNDLE_BYTES {
        return Err(invalid());
    }
    let header = Header::decode(bytes)?;
    if &header.vault_id != vault_id {
        return Err(invalid());
    }
    let header_bytes = array(&bytes[..HEADER_LEN]);
    let mut at = HEADER_LEN;
    let take = |at: &mut usize, n: usize| -> Result<&[u8]> {
        let end = at.checked_add(n).filter(|end| *end <= bytes.len());
        let end = end.ok_or_else(invalid)?;
        let slice = &bytes[*at..end];
        *at = end;
        Ok(slice)
    };
    let count = u16::from_be_bytes(array(take(&mut at, 2)?)) as usize;
    if count > MAX_ENTRIES {
        return Err(invalid());
    }
    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let len = u16::from_be_bytes(array(take(&mut at, 2)?)) as usize;
        if len < TAG + 1 {
            return Err(invalid());
        }
        let nonce = take(&mut at, NONCE)?;
        let sealed = take(&mut at, len)?;
        entries.push((nonce, sealed));
    }
    let entries_hash: [u8; 32] = Sha256::digest(&bytes[HEADER_LEN..at]).into();
    let body_nonce = take(&mut at, NONCE)?;
    let body = &bytes[at..];
    if body.len() < TAG + 64 {
        return Err(invalid());
    }
    Ok(Parts {
        header,
        header_bytes,
        entries,
        entries_hash,
        body_nonce,
        body,
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BodyIn {
    v: u32,
    by: String,
    at: i64,
    reason: String,
    signer: String,
    recipients: Vec<DeviceCertIn>,
    removed: Vec<DeviceRecordIn>,
    keys: Vec<KeyIn>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceCertIn {
    device: String,
    cert: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DeviceRecordIn {
    device: String,
    record: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KeyIn {
    epoch: u32,
    key: SecretString,
    #[serde(default)]
    head: Option<u64>,
}

/// A lowercase 32-hex device ID, exactly.
fn device_of(hex: &str) -> Result<DeviceId> {
    let device = DeviceId::from_hex(hex).map_err(|_| invalid())?;
    (device.to_hex() == hex)
        .then_some(device)
        .ok_or_else(invalid)
}

/// Opens and reads the body with `new_key`: every certificate and record decodes and names the
/// device it is listed under in this vault, `by` is the signer certificate's device, the older
/// keys are 32 bytes, below `to_epoch` and each epoch once. The signature is not checked here
/// (`Unverified::verify`).
fn open_body(parts: &Parts<'_>, new_key: &[u8; 32]) -> Result<BundleBody> {
    let vault = &parts.header.vault_id;
    let plain = open_sealed(
        &body_key(new_key, vault),
        parts.body_nonce,
        parts.body,
        &body_aad(&parts.header_bytes, &parts.entries_hash),
    )
    .ok_or_else(invalid)?;
    if plain.len() < 64 {
        return Err(invalid());
    }
    let sig: [u8; 64] = array(&plain[..64]);
    let json = &plain[64..];
    let parsed: BodyIn = serde_json::from_slice(json).map_err(|_| invalid())?;
    if parsed.v != JSON_VERSION {
        return Err(invalid());
    }
    let by = device_of(&parsed.by)?;
    let signer = Certificate::from_b64(&parsed.signer).map_err(|_| invalid())?;
    if signer.body.device != by || &signer.body.vault_id != vault {
        return Err(invalid());
    }
    let recipients = parsed
        .recipients
        .iter()
        .map(|r| {
            let device = device_of(&r.device)?;
            let cert = Certificate::from_b64(&r.cert).map_err(|_| invalid())?;
            if cert.body.device != device || &cert.body.vault_id != vault {
                return Err(invalid());
            }
            Ok((device, cert))
        })
        .collect::<Result<Vec<_>>>()?;
    let removed = parsed
        .removed
        .iter()
        .map(|r| {
            let device = device_of(&r.device)?;
            let record = Record::from_b64(&r.record).map_err(|_| invalid())?;
            if record.subject != device || &record.vault_id != vault {
                return Err(invalid());
            }
            Ok((device, record))
        })
        .collect::<Result<Vec<_>>>()?;
    let mut keys: Vec<OldKey> = Vec::with_capacity(parsed.keys.len());
    for entry in &parsed.keys {
        let decoded = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(entry.key.0.as_bytes())
                .map_err(|_| invalid())?,
        );
        let mut key = Zeroizing::new([0u8; 32]);
        if decoded.len() != key.len()
            || entry.epoch >= parts.header.to_epoch
            || keys.iter().any(|old| old.epoch == entry.epoch)
        {
            return Err(invalid());
        }
        key.copy_from_slice(&decoded);
        keys.push(OldKey {
            epoch: entry.epoch,
            key,
            head: entry.head,
        });
    }
    Ok(BundleBody {
        by,
        at: parsed.at,
        reason: Reason::parse(&parsed.reason)?,
        signer,
        recipients,
        removed,
        keys,
        sig,
        signed: signed_message(&parts.header_bytes, &parts.entries_hash, json),
    })
}

/// The header of a bundle whose parts are well formed (sizes, magic, version, this vault), read
/// without opening anything: not authenticated. A device uses it only to pass over a bundle of a
/// key change it already holds (`to_epoch` at or below its own), and to tell a bundle that does
/// not parse from one that opens and then fails its checks.
pub fn peek(bytes: &[u8], vault_id: &[u8; 16]) -> Result<Header> {
    Ok(split(bytes, vault_id)?.header)
}

/// Opens a bundle for this device (spec §6.3): the entries are tried in order with its box
/// secret; the first that opens decides. A new key opens the body (a body that then does not
/// open, or does not read, is an error: the bundle was tampered with) and comes back
/// `Unverified`; a removal notice is returned with its record unchecked. A bundle of another
/// vault, or one that does not parse, is an error; one with no entry for this device, or whose
/// ephemeral key is of low order, is `Nothing`.
pub fn open(bytes: &[u8], vault_id: &[u8; 16], my_box_secret: &StaticSecret) -> Result<Opened> {
    let parts = split(bytes, vault_id)?;
    let box_pub = PublicKey::from(my_box_secret).to_bytes();
    let shared = my_box_secret.diffie_hellman(&PublicKey::from(parts.header.eph_pub));
    if !shared.was_contributory() {
        return Ok(Opened::Nothing);
    }
    let key = seal_key(
        shared.as_bytes(),
        &parts.header.eph_pub,
        &box_pub,
        vault_id,
        parts.header.to_epoch,
    );
    for (index, (nonce, sealed)) in parts.entries.iter().enumerate() {
        let aad = entry_aad(&parts.header_bytes, to_u16(index)?);
        let Some(plain) = open_sealed(&key, nonce, sealed, &aad) else {
            continue;
        };
        return match plain.split_first() {
            Some((&KIND_KEY, rest)) if rest.len() == 32 => {
                let new_key = Zeroizing::new(array::<32>(rest));
                let body = Box::new(open_body(&parts, &new_key)?);
                Ok(Opened::Recipient(Unverified {
                    new_key,
                    header: parts.header.clone(),
                    body,
                }))
            }
            Some((&KIND_NOTICE, rest)) => {
                let record = Record::decode(rest).map_err(|_| invalid())?;
                if &record.vault_id != vault_id {
                    return Err(invalid());
                }
                Ok(Opened::Notice {
                    header: parts.header.clone(),
                    record,
                })
            }
            _ => Err(invalid()),
        };
    }
    Ok(Opened::Nothing)
}

/// Opens a bundle's body with a key alone (a Recovery Kit holding the new key, spec §8.3); its
/// signature is not checked yet.
pub fn open_body_with_key(bytes: &[u8], vault_id: &[u8; 16], key: &[u8; 32]) -> Result<Unverified> {
    let parts = split(bytes, vault_id)?;
    let body = Box::new(open_body(&parts, key)?);
    Ok(Unverified {
        new_key: Zeroizing::new(*key),
        header: parts.header,
        body,
    })
}
