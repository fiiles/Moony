//! Device certificates (phase 5 spec §5.3, revision §2.3): the bytes, the five issuers and the
//! per-kind signature rules. All integers are big-endian; every signature is Ed25519 over the bytes before
//! it, which start with the domain string:
//!
//! ```text
//! body   = "moony device cert v1" ‖ kind (u8) ‖ vault_id (16) ‖ device_id (16)
//!          ‖ sign_pub (32) ‖ box_pub (32) ‖ issued_at (i64) ‖ issuer_device (16)
//!          ‖ issuer_sign_pub (32)
//! ticket = ticket_pub (32) ‖ ticket_issued_at (i64) ‖ holder_sig (64)
//!          holder_sig by issuer_sign_pub over "moony join ticket v1" ‖ vault_id
//!          ‖ issuer_device ‖ ticket_pub ‖ ticket_issued_at
//! cert   = body ‖ [ticket, kind 2 only] ‖ issuer_sig (64) ‖ subject_sig (64)
//! ```
//!
//! This module checks a certificate on its own (`verify_signatures`); whether its issuer is
//! trusted is the trust store's question. The Recovery Kit is a paper device (revision §2.3): its
//! own certificate (kind 5) is issued by the device that created it, and a device that joins with
//! it gets a kind-3 certificate issued by the Kit. Both bind the Kit's ID to its signing key
//! (`identity::kit::kit_id`), so a Kit certificate names its own ID and a Kit joiner names it as
//! issuer.

use std::fmt;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::kit::{kit_id, KitKeys};
use super::{new_sign_seed, sign_pub, verify_sig, SignSeed};
use crate::error::{AppError, Result};
use crate::services::sync::hlc::DeviceId;

const CERT_DOMAIN: &[u8; 20] = b"moony device cert v1";
const TICKET_DOMAIN: &[u8; 20] = b"moony join ticket v1";

const BODY_LEN: usize = 20 + 1 + 16 + 16 + 32 + 32 + 8 + 16 + 32;
const TICKET_LEN: usize = 32 + 8 + 64;
const SIGS_LEN: usize = 64 + 64;
/// How long a join ticket vouches: a paired certificate is issued no earlier than its ticket and
/// at most this long after it (the joiner signs it within the pairing's lifetime; its own clock is
/// taken no earlier than the ticket's, `identity::create_own`).
pub const TICKET_LIFETIME_SECS: i64 = 3600;
/// The longest encoding (a paired certificate) in base64url without padding.
const MAX_B64_LEN: usize = (BODY_LEN + TICKET_LEN + SIGS_LEN).div_ceil(3) * 4;

/// How a device got into the vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CertKind {
    /// It created the vault; signed by itself.
    Genesis = 1,
    /// A holder paired it; signed by the holder's one-time join ticket.
    Paired = 2,
    /// It joined with the Recovery Kit; issued by the Kit (`issuer_device` = the Kit's ID,
    /// `issuer_sign_pub` = its signing key; revision §2.3).
    RecoveryKit = 3,
    /// The re-join successor of an older ID; signed by the older ID's key.
    Rejoin = 4,
    /// The Recovery Kit itself (revision §2.3): its ID, signing key and box key, issued by the
    /// device that created it; the subject signature is the Kit's own key.
    Kit = 5,
}

impl CertKind {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(CertKind::Genesis),
            2 => Some(CertKind::Paired),
            3 => Some(CertKind::RecoveryKit),
            4 => Some(CertKind::Rejoin),
            5 => Some(CertKind::Kit),
            _ => None,
        }
    }

    fn has_ticket(self) -> bool {
        self == CertKind::Paired
    }

    fn self_issued(self) -> bool {
        self == CertKind::Genesis
    }
}

/// The subject's own facts, which every issuer takes: the signing key comes from the subject's
/// seed and the issuer fields from the kind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertFields {
    pub vault_id: [u8; 16],
    pub device: DeviceId,
    pub box_pub: [u8; 32],
    pub issued_at: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CertBody {
    pub kind: CertKind,
    pub vault_id: [u8; 16],
    pub device: DeviceId,
    pub sign_pub: [u8; 32],
    pub box_pub: [u8; 32],
    pub issued_at: i64,
    pub issuer_device: DeviceId,
    pub issuer_sign_pub: [u8; 32],
}

impl CertBody {
    fn encode_into(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(CERT_DOMAIN);
        out.push(self.kind.as_u8());
        out.extend_from_slice(&self.vault_id);
        out.extend_from_slice(&self.device.0);
        out.extend_from_slice(&self.sign_pub);
        out.extend_from_slice(&self.box_pub);
        out.extend_from_slice(&self.issued_at.to_be_bytes());
        out.extend_from_slice(&self.issuer_device.0);
        out.extend_from_slice(&self.issuer_sign_pub);
    }
}

/// A one-time join ticket's public half and the holder's signature over it (spec §5.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ticket {
    pub ticket_pub: [u8; 32],
    pub issued_at: i64,
    pub holder_sig: [u8; 64],
}

impl Ticket {
    pub fn encode(&self) -> [u8; TICKET_LEN] {
        let mut out = [0u8; TICKET_LEN];
        out[..32].copy_from_slice(&self.ticket_pub);
        out[32..40].copy_from_slice(&self.issued_at.to_be_bytes());
        out[40..].copy_from_slice(&self.holder_sig);
        out
    }

    fn decode(bytes: &[u8; TICKET_LEN]) -> Self {
        Ticket {
            ticket_pub: array(&bytes[..32]),
            issued_at: i64::from_be_bytes(array(&bytes[32..40])),
            holder_sig: array(&bytes[40..]),
        }
    }

    fn message(
        vault_id: &[u8; 16],
        holder_device: DeviceId,
        ticket_pub: &[u8; 32],
        issued_at: i64,
    ) -> Vec<u8> {
        let mut message = Vec::with_capacity(TICKET_DOMAIN.len() + 16 + 16 + 32 + 8);
        message.extend_from_slice(TICKET_DOMAIN);
        message.extend_from_slice(vault_id);
        message.extend_from_slice(&holder_device.0);
        message.extend_from_slice(ticket_pub);
        message.extend_from_slice(&issued_at.to_be_bytes());
        message
    }

    /// The holder `holder_device` with key `holder_sign_pub` signed this ticket for `vault_id`.
    pub fn verify(
        &self,
        vault_id: &[u8; 16],
        holder_device: DeviceId,
        holder_sign_pub: &[u8; 32],
    ) -> Result<()> {
        let message = Self::message(vault_id, holder_device, &self.ticket_pub, self.issued_at);
        verify_sig(holder_sign_pub, &message, &self.holder_sig)
    }
}

/// A join ticket's secret seed: handed to the joiner in pairing slot 2 and wiped by both sides
/// right after use.
pub struct TicketSecret(SignSeed);

impl TicketSecret {
    pub fn from_bytes(bytes: Zeroizing<[u8; 32]>) -> Self {
        TicketSecret(SignSeed::from_bytes(bytes))
    }

    pub fn as_seed(&self) -> &SignSeed {
        &self.0
    }

    /// The secret, for sealing it into pairing slot 2. Never log it.
    pub fn expose(&self) -> &[u8; 32] {
        self.0.expose()
    }
}

impl fmt::Debug for TicketSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TicketSecret(<redacted>)")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Certificate {
    pub body: CertBody,
    pub ticket: Option<Ticket>,
    pub issuer_sig: [u8; 64],
    pub subject_sig: [u8; 64],
}

fn invalid() -> AppError {
    AppError::Internal("sync: a device certificate is not valid".into())
}

/// `bytes` as an array; the callers slice exactly `N` bytes.
fn array<const N: usize>(bytes: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    out.copy_from_slice(bytes);
    out
}

impl Certificate {
    /// The bytes `issuer_sig` signs: the body and, for a paired certificate, the ticket.
    pub(super) fn issuer_signed_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(BODY_LEN + TICKET_LEN + SIGS_LEN);
        self.body.encode_into(&mut out);
        if let Some(ticket) = &self.ticket {
            out.extend_from_slice(&ticket.encode());
        }
        out
    }

    /// The bytes `subject_sig` signs: everything before it.
    pub(super) fn subject_signed_bytes(&self) -> Vec<u8> {
        let mut out = self.issuer_signed_bytes();
        out.extend_from_slice(&self.issuer_sig);
        out
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = self.subject_signed_bytes();
        out.extend_from_slice(&self.subject_sig);
        out
    }

    /// Exact lengths: a paired certificate carries a ticket, every other kind none.
    pub fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() <= CERT_DOMAIN.len() || &bytes[..CERT_DOMAIN.len()] != CERT_DOMAIN {
            return Err(invalid());
        }
        let kind = CertKind::from_u8(bytes[CERT_DOMAIN.len()]).ok_or_else(invalid)?;
        let ticket_len = if kind.has_ticket() { TICKET_LEN } else { 0 };
        if bytes.len() != BODY_LEN + ticket_len + SIGS_LEN {
            return Err(invalid());
        }
        let mut at = CERT_DOMAIN.len() + 1;
        let mut take = |n: usize| {
            let slice = &bytes[at..at + n];
            at += n;
            slice
        };
        let body = CertBody {
            kind,
            vault_id: array(take(16)),
            device: DeviceId(array(take(16))),
            sign_pub: array(take(32)),
            box_pub: array(take(32)),
            issued_at: i64::from_be_bytes(array(take(8))),
            issuer_device: DeviceId(array(take(16))),
            issuer_sign_pub: array(take(32)),
        };
        let ticket = kind
            .has_ticket()
            .then(|| Ticket::decode(&array(take(TICKET_LEN))));
        let issuer_sig = array(take(64));
        let subject_sig = array(take(64));
        Ok(Certificate {
            body,
            ticket,
            issuer_sig,
            subject_sig,
        })
    }

    /// base64url without padding, as stored in `sync_devices.cert`.
    pub fn to_b64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.encode())
    }

    /// Strict base64url (no padding, canonical trailing bits), then `decode`.
    pub fn from_b64(s: &str) -> Result<Self> {
        if s.len() > MAX_B64_LEN {
            return Err(invalid());
        }
        let bytes = URL_SAFE_NO_PAD.decode(s).map_err(|_| invalid())?;
        Self::decode(&bytes)
    }

    /// SHA-256 of the encoding: what a removal plan's consent names.
    pub fn hash(&self) -> [u8; 32] {
        Sha256::digest(self.encode()).into()
    }

    /// The per-kind rules of spec §5.3 and revision §2.3, without any trust lookup: a genesis
    /// certificate names itself as issuer and is signed by its own key; a paired one carries a
    /// ticket the named holder signed, is issued within `TICKET_LIFETIME_SECS` after it and is
    /// signed by that ticket; a re-join one names an older ID and is signed by its key; a Kit
    /// joiner's (kind 3) names the Kit as issuer and is signed by the Kit's key; the Kit's own
    /// (kind 5) names its creator as issuer and is signed by the creator's key. A Kit's ID is
    /// bound to its signing key (`kit_id`): a kind 5 names it as its device, a kind 3 as its
    /// issuer. Every kind carries the subject's proof of possession, and no kind but genesis
    /// names itself as issuer.
    pub fn verify_signatures(&self) -> Result<()> {
        let body = &self.body;
        if self.ticket.is_some() != body.kind.has_ticket() {
            return Err(invalid());
        }
        let kit_bound = match body.kind {
            CertKind::Kit => body.device == kit_id(&body.vault_id, &body.sign_pub),
            CertKind::RecoveryKit => {
                body.issuer_device == kit_id(&body.vault_id, &body.issuer_sign_pub)
            }
            _ => true,
        };
        if !kit_bound {
            return Err(invalid());
        }
        let issuer_key = if body.kind.self_issued() {
            if body.issuer_device != body.device || body.issuer_sign_pub != body.sign_pub {
                return Err(invalid());
            }
            body.sign_pub
        } else {
            if body.issuer_device == body.device {
                return Err(invalid());
            }
            match &self.ticket {
                Some(ticket) => {
                    ticket
                        .verify(&body.vault_id, body.issuer_device, &body.issuer_sign_pub)
                        .map_err(|_| invalid())?;
                    let fresh = body.issued_at >= ticket.issued_at
                        && body.issued_at - ticket.issued_at <= TICKET_LIFETIME_SECS;
                    if !fresh {
                        return Err(invalid());
                    }
                    ticket.ticket_pub
                }
                None => body.issuer_sign_pub,
            }
        };
        verify_sig(&issuer_key, &self.issuer_signed_bytes(), &self.issuer_sig)
            .map_err(|_| invalid())?;
        verify_sig(
            &body.sign_pub,
            &self.subject_signed_bytes(),
            &self.subject_sig,
        )
        .map_err(|_| invalid())
    }

    /// Signs `body` (and its ticket) with the issuer's key and then the subject's, and checks
    /// the result: an issuer never hands out a certificate that would not verify.
    fn signed(
        body: CertBody,
        ticket: Option<Ticket>,
        issuer: &SignSeed,
        subject: &SignSeed,
    ) -> Result<Self> {
        let mut cert = Certificate {
            body,
            ticket,
            issuer_sig: [0; 64],
            subject_sig: [0; 64],
        };
        cert.issuer_sig = issuer.sign(&cert.issuer_signed_bytes());
        cert.subject_sig = subject.sign(&cert.subject_signed_bytes());
        cert.verify_signatures()?;
        Ok(cert)
    }
}

fn body(
    kind: CertKind,
    fields: &CertFields,
    subject: &SignSeed,
    issuer_device: DeviceId,
    issuer_sign_pub: [u8; 32],
) -> CertBody {
    CertBody {
        kind,
        vault_id: fields.vault_id,
        device: fields.device,
        sign_pub: sign_pub(subject),
        box_pub: fields.box_pub,
        issued_at: fields.issued_at,
        issuer_device,
        issuer_sign_pub,
    }
}

/// A genesis (setup) certificate: the subject is its own issuer.
pub fn issue_self(kind: CertKind, fields: &CertFields, subject: &SignSeed) -> Result<Certificate> {
    if !kind.self_issued() {
        return Err(AppError::Internal(
            "sync: only a genesis certificate is self-issued".into(),
        ));
    }
    let body = body(kind, fields, subject, fields.device, sign_pub(subject));
    Certificate::signed(body, None, subject, subject)
}

/// A one-time join ticket from `holder` (this device, `holder_device`) for `vault_id`.
pub fn issue_ticket(
    holder: &SignSeed,
    vault_id: [u8; 16],
    holder_device: DeviceId,
    now: i64,
) -> Result<(TicketSecret, Ticket)> {
    let secret = TicketSecret(new_sign_seed()?);
    Ok(ticket_from_secret(
        secret,
        holder,
        vault_id,
        holder_device,
        now,
    ))
}

/// `issue_ticket` with the ticket's secret given: deterministic, for the vectors.
pub(crate) fn ticket_from_secret(
    secret: TicketSecret,
    holder: &SignSeed,
    vault_id: [u8; 16],
    holder_device: DeviceId,
    now: i64,
) -> (TicketSecret, Ticket) {
    let ticket_pub = sign_pub(secret.as_seed());
    let message = Ticket::message(&vault_id, holder_device, &ticket_pub, now);
    let ticket = Ticket {
        ticket_pub,
        issued_at: now,
        holder_sig: holder.sign(&message),
    };
    (secret, ticket)
}

/// The joiner's certificate after a pairing: issued with the ticket's secret, which the
/// `holder_device` (signing key `holder_sign_pub`) vouched for. Refuses a ticket that holder did
/// not sign, or one that does not belong to `secret`.
pub fn issue_paired(
    secret: &TicketSecret,
    ticket: &Ticket,
    holder_device: DeviceId,
    holder_sign_pub: &[u8; 32],
    fields: &CertFields,
    subject: &SignSeed,
) -> Result<Certificate> {
    ticket.verify(&fields.vault_id, holder_device, holder_sign_pub)?;
    if ticket.ticket_pub != sign_pub(secret.as_seed()) || holder_device == fields.device {
        return Err(invalid());
    }
    let body = body(
        CertKind::Paired,
        fields,
        subject,
        holder_device,
        *holder_sign_pub,
    );
    Certificate::signed(body, Some(ticket.clone()), secret.as_seed(), subject)
}

/// A re-join successor's certificate, issued by the older ID's signing key.
pub fn issue_rejoin(
    old: &SignSeed,
    old_device: DeviceId,
    fields: &CertFields,
    subject: &SignSeed,
) -> Result<Certificate> {
    if old_device == fields.device {
        return Err(invalid());
    }
    let body = body(CertKind::Rejoin, fields, subject, old_device, sign_pub(old));
    Certificate::signed(body, None, old, subject)
}

/// The Recovery Kit's own certificate (kind 5, revision §2.3), issued by its creator
/// (`creator_device`, signing key `creator`) at `issued_at`: the Kit's ID, signing key and box
/// key, with the Kit's proof of possession (the creator holds the Kit's seed while it creates it).
pub fn issue_kit(
    creator: &SignSeed,
    creator_device: DeviceId,
    kit: &KitKeys,
    issued_at: i64,
) -> Result<Certificate> {
    if creator_device == kit.id() {
        return Err(invalid());
    }
    let fields = CertFields {
        vault_id: *kit.vault_id(),
        device: kit.id(),
        box_pub: *kit.box_pub(),
        issued_at,
    };
    let body = body(
        CertKind::Kit,
        &fields,
        kit.sign_seed(),
        creator_device,
        sign_pub(creator),
    );
    Certificate::signed(body, None, creator, kit.sign_seed())
}

/// The certificate of a device joining with the Recovery Kit (kind 3, revision §2.3), issued by
/// the Kit's key. Refuses `fields` of another vault than the Kit's, or naming the Kit itself.
pub fn issue_kit_join(
    kit: &KitKeys,
    fields: &CertFields,
    subject: &SignSeed,
) -> Result<Certificate> {
    if &fields.vault_id != kit.vault_id() || fields.device == kit.id() {
        return Err(invalid());
    }
    let body = body(
        CertKind::RecoveryKit,
        fields,
        subject,
        kit.id(),
        *kit.sign_pub(),
    );
    Certificate::signed(body, None, kit.sign_seed(), subject)
}
