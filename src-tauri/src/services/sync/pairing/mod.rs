//! The pairing protocol's bytes (phase 3, spec §20 and ADR 0026): an X25519 exchange between the
//! device that shows a QR code (the shower, S) and the one that scans it (the scanner, C), an
//! HKDF transcript that gives both a session key and a six-digit comparison code, the QR text,
//! slot 1 (the scanner's reply) and slot 2 (the holder's sealed credentials). Everything here is
//! pure: no relay, no database, no clock. The session state machine builds on it.
//!
//! The vectors in `tests.rs` are the contract with every other device. The six digits are 20 bits:
//! from protocol 4 on they protect the exchange whether the code reaches C on a screen or in a
//! message someone can rewrite, because S's key is not in the code (see "Protocol 4").
//!
//! Protocol 2 (audit M1, M2): the shower proves with an acknowledgement (slot 3, an HMAC under a
//! key from the same HKDF output) that it derived its transcript from the scanner's own key, so a
//! relay that also sees the QR and the scanner's digits cannot grind a scanner key whose code
//! matches; and the holder's person types the digits the other device shows, checked here in
//! constant time. The version lives in the QR prefix and in slot 1: a version-1 code is refused
//! with `OTHER_NEEDS_UPDATE`, and a version-1 slot 1 with `syncPairingFailed`. The session key,
//! the code and slot 2 are byte for byte those of version 1.
//!
//! Protocol 3 (device sync phase 5, spec §5.3, §11.2): slot 2 carries payload version 2, which
//! adds each key's rotation head and the holder's identity (its whole trust store and a one-time
//! join ticket with which the joiner signs its own certificate), or `null` from a holder whose
//! vault has no certificates. The QR prefix is `moony-pair:3:`; a version-1 or version-2 code is
//! `OTHER_NEEDS_UPDATE`. The acknowledgement, the HKDF info, slots 1 and 3, the slot-2 version
//! byte and its AD are unchanged.
//!
//! Protocol 4 (sync audit 2, R2-H2, design §3): the code carries `commitment = SHA-256(domain ‖
//! mailbox ‖ S public)` instead of S's key, and S reveals the key in slot 3 (with the
//! acknowledgement tag) only after it read C's key from slot 1. C checks the revealed key against
//! the commitment before it derives anything, so a third party that rewrote the code must fix its
//! own key in the rewritten code before the joiner answers, and must answer the holder before the
//! holder's key is revealed: it never knows both sets of digits while it can still choose a key
//! (the same structure as Bluetooth's numeric comparison). The HKDF info is `"moony pair v2"`,
//! slot 1 has version `0x03`, slot 3 is `REVEAL_VERSION ‖ S public ‖ tag` (65 bytes); the six
//! digits, slot 2, its payload version and its AD are unchanged. A code of version 1 to 3 is
//! `OTHER_NEEDS_UPDATE`, one of version 5 or later `THIS_NEEDS_UPDATE`.
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
use hmac::{Hmac, Mac};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

use super::hlc::DeviceId;
use super::identity::trust::TrustExport;
use super::identity::{sign_pub, Ticket, TicketSecret};
use super::keys::{os_random, Credentials, SecretString, VaultId, VaultKeys};
use super::relay::http::{is_default_relay, normalize_base_url};
use crate::error::{AppError, Result};

pub mod holder;
pub mod session;

#[cfg(test)]
mod tests;

/// The protocol this app speaks: the version in the QR prefix.
pub const PROTOCOL: u32 = 4;
pub const QR_PREFIX: &str = "moony-pair:4:";
pub const MAX_CODE_BYTES: usize = 512;
/// The HKDF info label of protocol 4 (protocols 1 to 3 used `"moony pair v1"`).
pub const KDF_INFO: &[u8] = b"moony pair v2";
/// Slot 2 and the cancel marker; the slot-2 AD keeps it, so slot 2 is laid out as before.
pub const SLOT_VERSION: u8 = 0x01;
/// Slot 1 only: `0x03 ‖ role ‖ C public`. A slot 1 of an earlier protocol is refused.
pub const SLOT1_VERSION: u8 = 0x03;
/// The domain of the commitment in the pairing code.
pub const COMMIT_DOMAIN: &[u8] = b"moony pair commit v1";
/// Slot 3 only: `0x02 ‖ S public ‖ tag`. The 33-byte acknowledgement of protocol 3 (version
/// `0x01`) is refused.
pub const REVEAL_VERSION: u8 = 0x02;
pub const REVEAL_SLOT_LEN: usize = 1 + 32 + 32;
pub const SLOT1_LEN: usize = 34;
pub const SLOT2_DIRECTION: &[u8] = b"holder->joiner";
/// The acknowledgement tag's direction label (slot 3).
pub const ACK_DIRECTION: &[u8] = b"shower->scanner";
/// HKDF output: session key (0..32), code (32..40), ack key (40..72). HKDF-Expand is
/// prefix-consistent, so bytes 0..40 are those of protocol 1's 40-byte expand.
pub const OKM_LEN: usize = 72;
pub const CANCEL_MARKER: [u8; 2] = [SLOT_VERSION, 0x00];
/// A pairing code of an older protocol (`moony-pair:1:` to `moony-pair:3:`): the other device
/// needs an update before it can pair.
pub const OTHER_NEEDS_UPDATE: &str = "validation.syncPairingOtherNeedsUpdate";
/// A pairing code of a version this app does not know (`moony-pair:5:` and later): this device
/// needs an update.
pub const THIS_NEEDS_UPDATE: &str = "validation.syncPairingThisNeedsUpdate";
/// The payload JSON's cap (protocol 3): the holder's trust store travels in it; the relay's slot
/// takes 64 KiB.
pub const MAX_PAYLOAD_JSON: usize = 48 * 1024;
/// The slot-2 payload version of protocol 3.
pub const PAYLOAD_VERSION: u32 = 2;
/// A holder whose store does not fit the slot (spec §9.2's limit of devices).
const TOO_MANY_DEVICES: &str = "validation.syncTooManyDevices";
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

/// What the QR code carries: the mailbox, a commitment to the shower's public key (the key itself
/// is revealed in slot 3) and, for a sync server that is not the default, its address.
pub struct PairingCode {
    pub mailbox: MailboxId,
    pub commitment: [u8; 32],
    pub relay_url: Option<String>,
}

impl fmt::Debug for PairingCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingCode(<redacted>)")
    }
}

/// `SHA-256(COMMIT_DOMAIN ‖ mailbox ‖ S public)`: what the pairing code carries in place of S's key.
pub fn commitment(mailbox: &MailboxId, shower_public: &[u8; 32]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(COMMIT_DOMAIN);
    hash.update(mailbox.0);
    hash.update(shower_public);
    hash.finalize().into()
}

/// `moony-pair:4:<mailbox>:<commitment>[:<relay>]`, every field base64url without padding. The
/// fourth field is written only for a relay that is not the default. Infallible: the caller
/// passes the stored, already-normalized relay address (`normalize_base_url`).
pub fn pairing_code_text(code: &PairingCode) -> String {
    let mut text = format!(
        "{QR_PREFIX}{}:{}",
        code.mailbox.to_b64(),
        URL_SAFE_NO_PAD.encode(code.commitment)
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

/// The version of a `moony-pair:<n>:` text other than this app's, as an error: an older one is
/// `OTHER_NEEDS_UPDATE`, a newer one `THIS_NEEDS_UPDATE`. `None` for this app's version and for
/// text that names no version.
fn other_version(text: &str) -> Option<AppError> {
    let digits = text
        .strip_prefix("moony-pair:")
        .and_then(|rest| rest.split(':').next())
        .filter(|digits| !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit()))?;
    // Longer than any version counter is still "newer", never a parse failure.
    let version = if digits.len() > 9 {
        u32::MAX
    } else {
        digits.parse::<u32>().ok()?
    };
    match version.cmp(&PROTOCOL) {
        std::cmp::Ordering::Less => Some(AppError::Validation(OTHER_NEEDS_UPDATE.into())),
        std::cmp::Ordering::Greater => Some(AppError::Validation(THIS_NEEDS_UPDATE.into())),
        std::cmp::Ordering::Equal => None,
    }
}

/// Parses a pairing code. Malformed input is `validation.syncPairingCodeInvalid`; a relay address
/// the app does not use is `validation.syncRelayUrlInvalid`; a code of an older protocol
/// (`moony-pair:1:` to `moony-pair:3:`, whatever follows) is `OTHER_NEEDS_UPDATE`, a later one
/// (`moony-pair:5:`…) `THIS_NEEDS_UPDATE`. The default relay reads as `None`. No error carries
/// the input. White space and zero-width characters anywhere in it are ignored (a chat app wraps
/// a pasted code, a person types it in groups; owner decision O-3, As built R-114).
pub fn parse_pairing_code(text: &str) -> Result<PairingCode> {
    // A flood of white space is refused before it is walked; what is left must fit the cap.
    if text.len() > 4 * MAX_CODE_BYTES {
        return Err(code_invalid());
    }
    let text: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '\u{200B}'..='\u{200D}' | '\u{FEFF}'))
        .collect();
    let text = text.as_str();
    if text.len() > MAX_CODE_BYTES {
        return Err(code_invalid());
    }
    if let Some(error) = other_version(text) {
        return Err(error);
    }
    let rest = text.strip_prefix(QR_PREFIX).ok_or_else(code_invalid)?;
    let fields: Vec<&str> = rest.split(':').collect();
    if !(2..=3).contains(&fields.len()) {
        return Err(code_invalid());
    }
    let mailbox = MailboxId::from_b64(fields[0])?;
    let commitment: [u8; 32] = URL_SAFE_NO_PAD
        .decode(fields[1].as_bytes())
        .map_err(|_| code_invalid())?
        .try_into()
        .map_err(|_| code_invalid())?;
    if URL_SAFE_NO_PAD.encode(commitment) != fields[1] {
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
        commitment,
        relay_url,
    })
}

/// Slot 1, the scanner's reply: `0x03 ‖ role ‖ C public`.
pub fn slot1(role: Role, scanner_public: &[u8; 32]) -> [u8; SLOT1_LEN] {
    let mut slot = [0u8; SLOT1_LEN];
    slot[0] = SLOT1_VERSION;
    slot[1] = role.byte();
    slot[2..].copy_from_slice(scanner_public);
    slot
}

/// Exactly 34 bytes with `SLOT1_VERSION` and a known role; anything else, a version-1 record
/// included, is `validation.syncPairingFailed`.
pub fn parse_slot1(bytes: &[u8]) -> Result<(Role, [u8; 32])> {
    if bytes.len() != SLOT1_LEN || bytes[0] != SLOT1_VERSION {
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

/// Exactly `0x01 0x00`, on any slot. The session checks it on every slot before parsing.
pub fn is_cancel_marker(bytes: &[u8]) -> bool {
    bytes == CANCEL_MARKER
}

/// The session key, the comparison code, the acknowledgement key and the slot-2 associated data
/// of one exchange.
pub struct Transcript {
    key: Zeroizing<[u8; 32]>,
    code: u32,
    ack_key: Zeroizing<[u8; 32]>,
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

    /// `HMAC-SHA256(ack key, ACK_DIRECTION ‖ mailbox ‖ S public ‖ C public)`. Only a shower that
    /// derived its transcript from the scanner's own key can produce it.
    fn tag(&self) -> [u8; 32] {
        // HMAC takes a key of any length, so `new_from_slice` cannot fail; the `hmac` crate's
        // internal pads are not zeroized (a crate limit, as with HKDF's PRK).
        let mut mac = <Hmac<Sha256>>::new_from_slice(&self.ack_key[..])
            .expect("HMAC-SHA256 takes a 32-byte key");
        mac.update(ACK_DIRECTION);
        // The AD's tail is exactly `mailbox ‖ S public ‖ C public`.
        mac.update(&self.ad[1 + SLOT2_DIRECTION.len()..]);
        mac.finalize().into_bytes().into()
    }

    /// Slot 3, the shower's reveal: `REVEAL_VERSION ‖ S public ‖ tag`. The key is bytes 31..63 of
    /// the AD; the tag is the acknowledgement of protocols 2 and 3.
    pub fn reveal(&self) -> [u8; REVEAL_SLOT_LEN] {
        let mut slot = [0u8; REVEAL_SLOT_LEN];
        slot[0] = REVEAL_VERSION;
        slot[1..33].copy_from_slice(&self.ad[31..63]);
        slot[33..].copy_from_slice(&self.tag());
        slot
    }

    /// Whether `tag` is this exchange's acknowledgement tag, compared in constant time.
    pub fn verify_tag(&self, tag: &[u8; 32]) -> bool {
        bool::from(self.tag()[..].ct_eq(&tag[..]))
    }

    /// Whether the person typed this exchange's code (`typed_code_digits`), compared with
    /// `code_text()` in constant time. Input that is not six digits is false.
    pub fn code_matches(&self, typed: &str) -> bool {
        let Some(digits) = typed_code_digits(typed) else {
            return false;
        };
        let expected = Zeroizing::new(self.code_text());
        bool::from(digits[..].ct_eq(expected.as_bytes()))
    }

    #[cfg(test)]
    fn key_bytes(&self) -> &[u8; 32] {
        &self.key
    }

    #[cfg(test)]
    fn ack_key(&self) -> &[u8; 32] {
        &self.ack_key
    }

    #[cfg(test)]
    fn ad(&self) -> &[u8; AD_LEN] {
        &self.ad
    }
}

/// The six ASCII digits of a typed comparison code: six digits, optionally with one space or `-`
/// after the third; full-width digits (U+FF10–U+FF19, as some keyboards type them) count as their
/// ASCII digit. Anything else (other characters, leading or trailing blanks, another length) is
/// `None`. Depends on the typed text only, never on a secret.
pub fn typed_code_digits(typed: &str) -> Option<Zeroizing<[u8; 6]>> {
    const SEPARATOR: u8 = 0;
    let mut chars = Zeroizing::new([0u8; 7]);
    let mut len = 0;
    for c in typed.chars() {
        if len == chars.len() {
            return None;
        }
        chars[len] = match c {
            '0'..='9' => c as u8,
            '\u{FF10}'..='\u{FF19}' => b'0' + (u32::from(c) - 0xFF10) as u8,
            ' ' | '-' => SEPARATOR,
            _ => return None,
        };
        len += 1;
    }
    let mut digits = Zeroizing::new([0u8; 6]);
    match len {
        6 => digits.copy_from_slice(&chars[..6]),
        7 if chars[3] == SEPARATOR => {
            digits[..3].copy_from_slice(&chars[..3]);
            digits[3..].copy_from_slice(&chars[4..]);
        }
        _ => return None,
    }
    digits.iter().all(u8::is_ascii_digit).then_some(digits)
}

impl fmt::Debug for Transcript {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Transcript(<redacted>)")
    }
}

/// X25519 between `own` and `peer_public`, then
/// `HKDF-SHA256(salt = mailbox, ikm = shared, info = "moony pair v2" ‖ S public ‖ C public)` of
/// `OKM_LEN` (72) bytes: 0..32 are the session key, 32..40 (big-endian, mod 10^6) the code and
/// 40..72 the acknowledgement key. `own`'s public key takes the place `side` names and the
/// peer's the other. A low-order peer key (a non-contributory exchange) is refused.
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
    let mut okm = Zeroizing::new([0u8; OKM_LEN]);
    Hkdf::<Sha256>::new(Some(&mailbox.0), shared.as_bytes())
        .expand(&info, &mut okm[..])
        .expect("72 bytes is within HKDF-SHA256's output limit");
    let mut key = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&okm[..32]);
    let mut tail = Zeroizing::new([0u8; 8]);
    tail.copy_from_slice(&okm[32..40]);
    let code = (u64::from_be_bytes(*tail) % 1_000_000) as u32;
    let mut ack_key = Zeroizing::new([0u8; 32]);
    ack_key.copy_from_slice(&okm[40..OKM_LEN]);

    let mut ad = [0u8; AD_LEN];
    ad[0] = SLOT_VERSION;
    ad[1..15].copy_from_slice(SLOT2_DIRECTION);
    ad[15..31].copy_from_slice(&mailbox.0);
    ad[31..63].copy_from_slice(&s_pub);
    ad[63..95].copy_from_slice(&c_pub);
    Ok(Transcript {
        key,
        code,
        ack_key,
        ad,
    })
}

/// Opens slot 3 of protocol 4: `Some((S public, tag))` only for exactly `REVEAL_SLOT_LEN` bytes
/// with `REVEAL_VERSION` whose key matches `commitment` (the one in the code the scanner read) for
/// this mailbox, compared in constant time. The cancel marker, a 33-byte slot 3 of protocol 3 and
/// anything else is `None`. Nothing is derived here: the caller derives with the returned key and
/// checks the tag with `Transcript::verify_tag`.
pub fn open_reveal(
    slot3: &[u8],
    mailbox: &MailboxId,
    commitment_in_code: &[u8; 32],
) -> Option<([u8; 32], [u8; 32])> {
    if slot3.len() != REVEAL_SLOT_LEN || slot3[0] != REVEAL_VERSION {
        return None;
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&slot3[1..33]);
    let mut tag = [0u8; 32];
    tag.copy_from_slice(&slot3[33..]);
    let matches = commitment(mailbox, &key)[..].ct_eq(&commitment_in_code[..]);
    bool::from(matches).then_some((key, tag))
}

/// The holder vouching for the joiner (protocol 3, spec §5.3): its device ID, its whole trust
/// store, and a one-time join ticket with its secret, which the joiner signs its certificate
/// with. The secret is wiped when this is dropped; `Debug` prints `<redacted>`.
pub struct PairingIdentity {
    pub holder: DeviceId,
    pub export: TrustExport,
    pub ticket_secret: TicketSecret,
    pub ticket: Ticket,
}

impl PairingIdentity {
    /// A copy for one join attempt; the session keeps its own for "Try again" (review PF6). Both
    /// secrets are wiped when dropped.
    pub fn duplicate(&self) -> Self {
        PairingIdentity {
            holder: self.holder,
            export: self.export.clone(),
            ticket_secret: TicketSecret::from_bytes(Zeroizing::new(*self.ticket_secret.expose())),
            ticket: self.ticket.clone(),
        }
    }
}

impl fmt::Debug for PairingIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PairingIdentity(<redacted>)")
    }
}

/// What slot 2 carries (payload version 2): the vault's credentials, the rotation head of each
/// key epoch the holder knows one for, and the holder's identity (`None` from a holder whose
/// vault has no certificates, spec §5.5).
pub struct SlotPayload {
    pub creds: Credentials,
    pub heads: BTreeMap<u32, u64>,
    pub identity: Option<PairingIdentity>,
}

impl fmt::Debug for SlotPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SlotPayload(<redacted>)")
    }
}

/// Slot 2: `0x01 ‖ nonce (24) ‖ XChaCha20-Poly1305(session key, AD, payload JSON)`.
pub fn seal_payload(t: &Transcript, payload: &SlotPayload) -> Result<Vec<u8>> {
    let mut nonce = [0u8; NONCE];
    os_random(&mut nonce)?;
    seal_payload_with_nonce(t, payload, nonce)
}

/// `creds` from a holder without an identity, epoch 0's head 0: the tests' shorthand.
#[cfg(test)]
pub fn seal_credentials(t: &Transcript, creds: &Credentials) -> Result<Vec<u8>> {
    seal_payload(
        t,
        &SlotPayload {
            creds: creds.clone(),
            heads: BTreeMap::from([(0, 0)]),
            identity: None,
        },
    )
}

fn seal_payload_with_nonce(
    t: &Transcript,
    payload: &SlotPayload,
    nonce: [u8; NONCE],
) -> Result<Vec<u8>> {
    let json = payload_json(payload)?;
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

/// `{"v":2,"relayUrl":"…","accountNumber":"…","vaultId":"…",
/// "keys":[{"epoch":0,"key":"…","head":0}],"identity":null}`, keys in ascending epoch (a key
/// whose head the holder does not know has no `head`); with an identity,
/// `"identity":{"holder":"<hex>","export":{"entries":[…]},"ticket":{"secret":"…",
/// "holderSig":"…","issuedAt":n}}`. Built in one wiped buffer; the export holds no secret.
/// A payload over `MAX_PAYLOAD_JSON` is `validation.syncTooManyDevices`: only a large store
/// makes one.
fn payload_json(payload: &SlotPayload) -> Result<Zeroizing<String>> {
    let creds = &payload.creds;
    let relay = normalize_base_url(&creds.relay_url)?;
    if !valid_account(&creds.account) {
        return Err(AppError::Internal(
            "sync pairing: the account number is not 16 digits".into(),
        ));
    }
    let export = payload
        .identity
        .as_ref()
        .map(|identity| serde_json::to_string(&identity.export))
        .transpose()
        .map_err(|_| AppError::Internal("sync pairing: the trust export".into()))?;
    let count = creds.keys.epochs().count();
    let export_len = export.as_ref().map_or(0, String::len);
    // Reserved up front so the buffer never reallocates and leaves an unwiped copy behind.
    let mut json = Zeroizing::new(String::with_capacity(
        512 + relay.len() + 120 * count + export_len,
    ));
    json.push_str("{\"v\":");
    json.push_str(&PAYLOAD_VERSION.to_string());
    json.push_str(",\"relayUrl\":\"");
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
        json.push('"');
        if let Some(head) = payload.heads.get(&epoch) {
            json.push_str(",\"head\":");
            json.push_str(&head.to_string());
        }
        json.push('}');
    }
    json.push_str("],\"identity\":");
    match (&payload.identity, &export) {
        (Some(identity), Some(export)) => {
            json.push_str("{\"holder\":\"");
            json.push_str(&identity.holder.to_hex());
            json.push_str("\",\"export\":");
            json.push_str(export);
            json.push_str(",\"ticket\":{\"secret\":\"");
            URL_SAFE_NO_PAD.encode_string(identity.ticket_secret.expose(), &mut json);
            json.push_str("\",\"holderSig\":\"");
            URL_SAFE_NO_PAD.encode_string(identity.ticket.holder_sig, &mut json);
            json.push_str("\",\"issuedAt\":");
            json.push_str(&identity.ticket.issued_at.to_string());
            json.push_str("}}");
        }
        _ => json.push_str("null"),
    }
    json.push('}');
    if json.len() > MAX_PAYLOAD_JSON {
        log::warn!("[SYNC] This device's trust store does not fit a pairing");
        return Err(AppError::Validation(TOO_MANY_DEVICES.into()));
    }
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
    #[serde(default)]
    identity: Option<IdentityIn>,
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
struct IdentityIn {
    holder: String,
    export: TrustExport,
    ticket: TicketIn,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TicketIn {
    secret: SecretString,
    #[serde(rename = "holderSig")]
    holder_sig: String,
    #[serde(rename = "issuedAt")]
    issued_at: i64,
}

/// The holder's identity from slot 2: a lowercase 32-hex holder ID, the export as
/// `TrustExport` reads it (unknown fields refused), a 32-byte ticket secret and a 64-byte
/// signature. The ticket's public key is the secret's; whether the holder signed it is checked
/// when the joiner signs its certificate.
fn identity_of(identity: IdentityIn) -> Result<PairingIdentity> {
    let holder = DeviceId::from_hex(&identity.holder).map_err(|_| failed())?;
    if holder.to_hex() != identity.holder {
        return Err(failed());
    }
    let decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(identity.ticket.secret.0.as_bytes())
            .map_err(|_| failed())?,
    );
    let mut secret = Zeroizing::new([0u8; 32]);
    if decoded.len() != secret.len() {
        return Err(failed());
    }
    secret.copy_from_slice(&decoded);
    let ticket_secret = TicketSecret::from_bytes(secret);
    let holder_sig: [u8; 64] = URL_SAFE_NO_PAD
        .decode(identity.ticket.holder_sig.as_bytes())
        .map_err(|_| failed())?
        .try_into()
        .map_err(|_| failed())?;
    let ticket = Ticket {
        ticket_pub: sign_pub(ticket_secret.as_seed()),
        issued_at: identity.ticket.issued_at,
        holder_sig,
    };
    Ok(PairingIdentity {
        holder,
        export: identity.export,
        ticket_secret,
        ticket,
    })
}

/// Decoded keys on their way into `VaultKeys`; wiped on every exit (`from_parts` wipes what it
/// is given, so it takes the map out first).
struct WipedKeys(BTreeMap<u32, [u8; 32]>);

impl Drop for WipedKeys {
    fn drop(&mut self) {
        self.0.values_mut().for_each(zeroize::Zeroize::zeroize);
    }
}

/// Opens slot 2 (payload version 2 only). Any failure to open or to trust the payload is
/// `validation.syncPairingFailed`, except a relay address the app does not use
/// (`validation.syncRelayUrlInvalid`).
pub fn open_payload(t: &Transcript, sealed: &[u8]) -> Result<SlotPayload> {
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
    let mut payload: PayloadIn = serde_json::from_str(json).map_err(|_| failed())?;
    if payload.v != PAYLOAD_VERSION || !valid_account(&payload.account_number.0) {
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
    let identity = payload.identity.take().map(identity_of).transpose()?;
    let heads: BTreeMap<u32, u64> = payload
        .keys
        .iter()
        .filter_map(|entry| entry.head.map(|head| (entry.epoch, head)))
        .collect();
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
        // The temporary and a replaced duplicate are wiped too, not only the map's copy.
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(&decoded);
        if let Some(mut duplicate) = keys.0.insert(entry.epoch, *key) {
            zeroize::Zeroize::zeroize(&mut duplicate);
            return Err(failed());
        }
    }
    let current = *keys.0.keys().next_back().ok_or_else(failed)?;
    let creds = Credentials {
        relay_url,
        account: payload.account_number.0.to_string(),
        vault_id,
        keys: VaultKeys::from_parts(current, std::mem::take(&mut keys.0))?,
    };
    Ok(SlotPayload {
        creds,
        heads,
        identity,
    })
}
