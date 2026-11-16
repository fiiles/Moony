//! Device identities (phase 5 spec §5.2): each device's Ed25519 signing key, the certificates
//! that bind a device ID to its signing and box keys inside one vault and say how it got in
//! (`cert`), the signed `left` and `removed` records (`record`), and the pinned store of the
//! devices this device has verified (`trust`). The signing seed lives in
//! `sync_state.device_sign_key`, never crosses IPC and never appears in a log line; like every
//! sync secret it sits in a `Zeroizing` buffer and prints `<redacted>`.
//!
//! This device's own identity (spec §5.2, §5.3, §5.5) is made where its device ID is: at setup
//! (a genesis certificate), at a pairing join (signed with the holder's one-time ticket), at a
//! Recovery Kit join (signed by the Kit's key, `kit`; revision §2.3), and at a re-join or a
//! replace of the same device (signed by the older ID's key). A device whose vault has no certificates (set up before phase 5, or
//! paired by a holder that sent no identity) gets a signing key and no certificate.

pub mod cert;
pub mod kit;
pub mod record;
pub mod trust;

#[cfg(test)]
mod own_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod trust_tests;

use std::fmt;

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use rusqlite::{Connection, OptionalExtension};
use zeroize::Zeroizing;

use super::hlc::DeviceId;
use super::in_savepoint;
use super::keys::{self, os_random};
use super::state;
use crate::error::{AppError, Result};

pub use cert::{CertBody, CertFields, CertKind, Certificate, Ticket, TicketSecret};
pub use kit::{KitKeys, KitSecret};
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

/// The vault bound on this profile, if any (a missing or unreadable one is `None`).
pub fn bound_vault(conn: &Connection) -> Option<[u8; 16]> {
    trust::vault_id(conn).ok()
}

/// A certificate for the `sync_devices` row `row_id` that verifies on its own: it decodes, names
/// that row's device and `vault` (when known), and its signatures hold (`verify_signatures`).
fn valid_row_cert(value: &str, row_id: DeviceId, vault: Option<&[u8; 16]>) -> Option<Certificate> {
    let cert = Certificate::from_b64(value).ok()?;
    let ok = cert.body.device == row_id
        && vault.is_none_or(|v| &cert.body.vault_id == v)
        && cert.verify_signatures().is_ok();
    ok.then_some(cert)
}

/// Whether `value` of the write-once `sync_devices` column `column` (`cert`, `left_record`,
/// `removal_record` or `confirm_record`) in the row of `row_id` (hex) holds up on its own, without looking at any
/// other row: it decodes, has the shape of its column, names that row's device and `vault` (when
/// known), and its self-contained signatures verify. A certificate checks its proof of
/// possession and its issuer's signature (and ticket); a `left` record checks its signature with
/// the signing key of `row_cert` (the same row's certificate) when that is valid, else its shape
/// only; a removal or confirmation record checks its shape (its signer is another device). The merge lets a valid
/// value beat an invalid one whatever their clocks (`merge::columns`), so a holder of the vault
/// key cannot lock a column with an older value that never verifies. Every other column: `true`.
pub fn write_once_value_valid(
    column: &str,
    row_id: &str,
    value: &str,
    vault: Option<&[u8; 16]>,
    row_cert: Option<&str>,
) -> bool {
    let Ok(row_id) = DeviceId::from_hex(row_id) else {
        return false;
    };
    let record_of = |kind: RecordType| {
        Record::from_b64(value).ok().filter(|r| {
            r.kind == kind && r.subject == row_id && vault.is_none_or(|v| &r.vault_id == v)
        })
    };
    match column {
        "cert" => valid_row_cert(value, row_id, vault).is_some(),
        "left_record" => record_of(RecordType::Left).is_some_and(|record| {
            match row_cert.and_then(|c| valid_row_cert(c, row_id, vault)) {
                Some(cert) => record.verify(&cert.body.sign_pub).is_ok(),
                None => true,
            }
        }),
        "removal_record" => record_of(RecordType::Removed).is_some(),
        "confirm_record" => record_of(RecordType::Confirmed).is_some(),
        _ => true,
    }
}

/// Who signs this device's certificate (spec §5.3), and so its kind.
pub enum Issuer<'a> {
    /// It creates the vault (kind 1).
    Genesis,
    /// It joins with the Recovery Kit, whose key signs (kind 3, revision §2.3).
    Kit { kit: &'a KitKeys },
    /// The holder `holder` (signing key `holder_sign_pub`) paired it with a one-time join ticket
    /// (kind 2).
    Ticket {
        secret: &'a TicketSecret,
        ticket: &'a Ticket,
        holder: DeviceId,
        holder_sign_pub: [u8; 32],
    },
    /// It continues an older ID of this device, whose key signs (kind 4).
    Predecessor {
        seed: &'a SignSeed,
        device: DeviceId,
    },
}

fn not_set_up() -> AppError {
    AppError::Internal("sync: sync is not set up on this device".into())
}

/// This device's signing seed, if it has one.
pub fn own_seed(conn: &Connection) -> Result<Option<SignSeed>> {
    let stored: Option<Option<Vec<u8>>> = conn
        .query_row(
            "SELECT device_sign_key FROM sync_state WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    // The BLOB is copied into a wiped buffer; rusqlite's own copy cannot be wiped.
    stored
        .flatten()
        .map(|bytes| SignSeed::from_slice(&Zeroizing::new(bytes)))
        .transpose()
}

fn store_seed(conn: &Connection, seed: &SignSeed) -> Result<()> {
    let changed = conn.execute(
        "UPDATE sync_state SET device_sign_key = ?1 WHERE id = 1",
        [seed.expose().as_slice()],
    )?;
    if changed == 0 {
        return Err(not_set_up());
    }
    Ok(())
}

/// Signs this device's certificate with a new signing key and pins it (`trust::seed_own`); the
/// caller writes it into this device's row (`devices::record_this_device`). The fields are the
/// bound vault, this device's ID and its box key. A device whose certificate is already pinned
/// keeps it and its key (a resumed setup signs nothing new). Nothing is stored when the issuer
/// refuses (a ticket the named holder did not sign). Never logs the seed.
pub fn create_own(conn: &Connection, issuer: Issuer<'_>, now: i64) -> Result<Certificate> {
    let device = state::load(conn)?.ok_or_else(not_set_up)?.device;
    if let Some(pinned) = trust::entry(conn, device)? {
        return Ok(pinned.cert);
    }
    let fields = CertFields {
        vault_id: trust::vault_id(conn)?,
        device,
        box_pub: keys::ensure_device_keypair(conn)?,
        issued_at: now,
    };
    let seed = new_sign_seed()?;
    let cert = match issuer {
        Issuer::Genesis => cert::issue_self(CertKind::Genesis, &fields, &seed)?,
        Issuer::Kit { kit } => cert::issue_kit_join(kit, &fields, &seed)?,
        // Dated no earlier than the ticket: a holder's clock ahead of this one's must not make
        // the certificate look older than its ticket (`cert::TICKET_LIFETIME_SECS`).
        Issuer::Ticket {
            secret,
            ticket,
            holder,
            holder_sign_pub,
        } => {
            let fields = CertFields {
                issued_at: fields.issued_at.max(ticket.issued_at),
                ..fields.clone()
            };
            cert::issue_paired(secret, ticket, holder, &holder_sign_pub, &fields, &seed)?
        }
        Issuer::Predecessor {
            seed: old,
            device: old_device,
        } => cert::issue_rejoin(old, old_device, &fields, &seed)?,
    };
    in_savepoint(conn, "sync_own_identity", || {
        store_seed(conn, &seed)?;
        trust::seed_own(conn, &cert, now)
    })?;
    Ok(cert)
}

/// A new signing key and no certificate: a device in a vault without certificates (§5.5).
pub fn create_uncertified(conn: &Connection) -> Result<()> {
    store_seed(conn, &new_sign_seed()?)
}

/// At unlock: a profile with sync on and no signing key (set up before phase 5) gets one, and no
/// certificate (§5.5). Whether it made one.
pub fn ensure_seed(conn: &Connection) -> Result<bool> {
    if !state::load(conn)?.is_some_and(|sync| sync.enabled) || own_seed(conn)?.is_some() {
        return Ok(false);
    }
    create_uncertified(conn)?;
    Ok(true)
}

/// What a re-join hands to its successor, read before the device ID changes: this device's ID
/// and signing key, when its certificate is pinned here. `None` in a vault without certificates
/// (§5.5): an ID nobody could verify vouches for nobody.
pub fn predecessor(conn: &Connection) -> Result<Option<(SignSeed, DeviceId)>> {
    let Some(sync) = state::load(conn)? else {
        return Ok(None);
    };
    if trust::entry(conn, sync.device)?.is_none() {
        return Ok(None);
    }
    Ok(own_seed(conn)?.map(|seed| (seed, sync.device)))
}

/// After a re-join (the device ID and box key are new): the successor's certificate, signed by
/// the `predecessor`'s key, or only a new signing key when there is none.
pub fn successor(
    conn: &Connection,
    predecessor: Option<(SignSeed, DeviceId)>,
    now: i64,
) -> Result<Option<Certificate>> {
    match predecessor {
        Some((seed, device)) => create_own(
            conn,
            Issuer::Predecessor {
                seed: &seed,
                device,
            },
            now,
        )
        .map(Some),
        None => create_uncertified(conn).map(|()| None),
    }
}
