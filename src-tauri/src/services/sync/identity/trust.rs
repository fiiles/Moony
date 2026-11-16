//! The pinned trust store (phase 5 spec §5.4, §5.5; revision §2.3, §3): the Local table
//! `sync_trust`, one entry per device ID this device has verified, with the certificate, its kind
//! and issuer, the keys it binds, a state and the signed record behind the state. It decides who
//! may receive a new vault key (`recipients`); the synced `sync_devices` columns are only ever a
//! source of candidates.
//!
//! - The first verified certificate for an ID is pinned for good: a different one later is
//!   ignored and reported (`RefreshReport::conflicts`), whatever its clock.
//! - States only move forward: `trusted` → `left` → `removed`, or `trusted` → `removed`.
//! - `cut_off_epoch` of a left or removed entry is the first key epoch it does not have; NULL
//!   while it still knows the current key.
//!
//! How entries get in: this device's own certificate (`seed_own`), a holder's whole store over the
//! pairing channel (`import_export`), a paired, re-join, Kit-joined (kind 3) or Kit (kind 5)
//! certificate whose issuer vouches for it here (`refresh`), the signer, recipients and Kit a
//! key-change bundle endorses (`import_bundle`), and a Kit joiner's trust on first use
//! (`tofu_from_rows`). A genesis certificate arriving through sync is never pinned. At most one
//! Kit is `trusted`: another Kit certificate is pinned only once the first is removed
//! (`ConflictKind::SecondKit`).
//!
//! An issuer vouches when its entry is pinned with the same signing key and `speaks_at` the
//! certificate's time and its ticket's (`vouch`): while it is `trusted`; after it left or was
//! removed, for what it issued until then; and an older ID that has a pinned re-join successor
//! only until this device pinned that successor (a superseded ID lives on only as a copy; never
//! the successor's own `issued_at`, revision As built R-25). A `removed` record counts when its
//! signer is pinned, speaks at the record's time and is not left out for its issuer
//! (`removal_counts`). An entry left out for its issuer speaks for nothing: its removals do not
//! count and its key changes are not taken (`speaker`, R-22). A Kit (kind 5) is pinned this way
//! only as the vault's first (R-24); a replacement comes in only by the key change naming it.
//! Within one `refresh` pass every record is judged against the store as it was before the pass,
//! so the outcome never depends on row order; records apply before certificates, so a certificate
//! whose issuer is removed in the same pull is pinned only when issued before.
//!
//! Who gets a new key is the rule of revision §3.1 (`recipients`): every pinned `trusted` device,
//! minus the removed, the left and the quiet superseded ones, minus every device whose issuer was
//! removed or is itself left out for that (`IssuerRemoved`, transitive), minus every device a left
//! or superseded issuer vouched for that was not seen in time (`LateVouch`, the R2 mitigation, by
//! first-seen relay positions: `sync_trust_seen`, `sync_tickets`; equal positions are late, R-23),
//! minus every device whose issuer is not pinned here (`IssuerUnknown`, R-23). A `confirmed` entry
//! (named by a verified key change, a Kit join's envelope, or kept by the person, `keep`) is never
//! left out for its issuer and stops the chain.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::cert::{CertKind, Certificate};
use super::record::{Record, RecordType};
use crate::error::{AppError, Result};
use crate::services::sync::hlc::{unpack, DeviceId};
use crate::services::sync::in_savepoint;
use crate::services::sync::keys::VaultId;
use crate::services::sync::state;

/// The most devices a new key is sealed to (spec §5.4).
pub const MAX_RECIPIENTS: usize = 64;
/// The most entries a pairing export may carry.
const MAX_EXPORT_ENTRIES: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TrustState {
    #[serde(rename = "trusted")]
    Trusted,
    #[serde(rename = "left")]
    Left,
    #[serde(rename = "removed")]
    Removed,
}

impl TrustState {
    pub fn as_str(self) -> &'static str {
        match self {
            TrustState::Trusted => "trusted",
            TrustState::Left => "left",
            TrustState::Removed => "removed",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "trusted" => Ok(TrustState::Trusted),
            "left" => Ok(TrustState::Left),
            "removed" => Ok(TrustState::Removed),
            _ => Err(corrupt()),
        }
    }
}

/// The Local `app_config` key of the new-device notices (revision §3.3): a JSON array of
/// `NewDevice`, at most `MAX_NEW_DEVICES`, newest last. Cleared by turning sync off.
pub const NEW_DEVICES_KEY: &str = "sync.new_devices";
/// The most new-device notices kept.
pub const MAX_NEW_DEVICES: usize = 20;

/// One pinned device.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustEntry {
    pub device: DeviceId,
    pub cert: Certificate,
    pub kind: CertKind,
    pub issuer: DeviceId,
    pub sign_pub: [u8; 32],
    pub box_pub: [u8; 32],
    pub ticket_pub: Option<[u8; 32]>,
    pub state: TrustState,
    pub state_record: Option<Record>,
    pub state_by: Option<DeviceId>,
    pub state_at: Option<i64>,
    pub cut_off_epoch: Option<u32>,
    /// When this device pinned it (this device's clock).
    pub pinned_at: i64,
    /// Named as a recipient (or the Kit) by a verified key change, by a Kit join's envelope, or
    /// kept by the person on this device (`keep`, "Je moje"). A confirmed device is never left
    /// out for its issuer (`IssuerRemoved`, `LateVouch`) and stops that chain.
    pub confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// The synced column holds a certificate other than the pinned one.
    OtherCertificate,
    /// A paired certificate uses a join ticket another device was pinned with.
    TicketReused,
    /// A Kit certificate while another Kit is `trusted` here (revision §2.3: at most one).
    SecondKit,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrustConflict {
    pub device: DeviceId,
    pub kind: ConflictKind,
}

/// What a `refresh` (or `tofu_from_rows`) changed and found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RefreshReport {
    /// Newly pinned devices, in the order they were pinned.
    pub pinned: Vec<DeviceId>,
    /// Entries whose state moved, in order.
    pub moved: Vec<(DeviceId, TrustState)>,
    pub conflicts: Vec<TrustConflict>,
    /// Verified records naming this device (a removal, or a `left` record another holder of
    /// this device's key made: this device signs its own only on its way out, after which its
    /// store is gone). The caller ends sync here (spec §7.4).
    pub about_me: Vec<Record>,
}

/// Why a device gets no new key (spec §9.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExclusionReason {
    /// The person is removing it.
    Removing,
    Left,
    Removed,
    /// Its certificate is not pinned here.
    Unverified,
    /// An older ID of a device that re-joined, with no change since its successor appeared (O11).
    SupersededQuiet,
    /// Neither a pinned certificate nor a public key.
    NoKey,
    /// Not confirmed, and its issuer (paired, re-join, Kit-joined, or the Kit's creator) was
    /// removed here or is itself left out for this reason (revision §3.1, transitive).
    IssuerRemoved,
    /// Not confirmed, and its issuer left or was superseded and nothing shows it vouched in time
    /// (revision §3.1, the R2 mitigation); or its issuer is itself left out for this reason. Kept
    /// by the person (`keep`), it is included.
    LateVouch,
    /// Not confirmed, and its issuer is not pinned here (a certificate a Kit joiner trusted on
    /// first use whose issuer has no row), or its issuer is itself left out for this reason. Kept
    /// by the person (`keep`) or named by a key change, it is included.
    IssuerUnknown,
}

/// The recipients of a new key, every other device with its reason, and the Kit apart.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipients {
    /// Sealed to each entry's pinned `box_pub`; ascending device ID.
    pub included: Vec<TrustEntry>,
    /// Ascending device ID.
    pub excluded: Vec<(DeviceId, ExclusionReason)>,
    /// The `trusted` Kit (revision §2.5): sealed to through the Kit envelope, never a bundle
    /// entry, never counted toward the 64.
    pub kit: Option<TrustEntry>,
}

/// A new-device notice (revision §3.3): `device` was pinned outside a join, added by `by`
/// (`via_kit`: it joined with the Kit), at `at` on this device's clock.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewDevice {
    #[serde(rename = "device")]
    pub device: String,
    #[serde(rename = "by")]
    pub by: String,
    #[serde(rename = "at")]
    pub at: i64,
    #[serde(rename = "viaKit")]
    pub via_kit: bool,
}

/// What a key-change bundle says about the Kit (revision §2.5): its certificate and, when it
/// replaces another, the removal record of the old one.
#[derive(Clone, Copy, Debug)]
pub struct BundleKit<'a> {
    pub cert: &'a Certificate,
    pub replaces: Option<&'a Record>,
}

/// A holder's whole store, sent to a joiner in pairing slot 2 (spec §5.4, §11.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustExport {
    #[serde(rename = "entries")]
    pub entries: Vec<ExportEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportEntry {
    /// The certificate, base64url.
    #[serde(rename = "cert")]
    pub cert: String,
    #[serde(rename = "state")]
    pub state: TrustState,
    /// The record behind a `left` or `removed` state, base64url.
    #[serde(rename = "record", default, skip_serializing_if = "Option::is_none")]
    pub record: Option<String>,
    #[serde(
        rename = "cutOffEpoch",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub cut_off_epoch: Option<u32>,
    /// `TrustEntry::confirmed`; written only when true.
    #[serde(
        rename = "confirmed",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub confirmed: bool,
    /// Where the holder first saw this certificate (`sync_trust_seen`, R-23); written only when
    /// known. The joiner takes the holder's positions over its own, so it judges late vouches as
    /// the holder does.
    #[serde(rename = "certSeen", default, skip_serializing_if = "Option::is_none")]
    pub cert_seen: Option<u64>,
    /// Where the holder first saw this device's `left` record.
    #[serde(rename = "leftSeen", default, skip_serializing_if = "Option::is_none")]
    pub left_seen: Option<u64>,
    /// Where the holder saw this certificate's issuer announce its join ticket (`sync_tickets`).
    #[serde(
        rename = "ticketSeen",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub ticket_seen: Option<u64>,
}

fn corrupt() -> AppError {
    AppError::Internal("sync: the trust store is not valid".into())
}

fn refused(what: &str) -> AppError {
    AppError::Internal(format!("sync: {what}"))
}

/// The first 8 hex characters of a device ID: as much as a log line names.
fn short(device: DeviceId) -> String {
    device.to_hex()[..8].to_string()
}

fn this_device(conn: &Connection) -> Result<DeviceId> {
    state::load(conn)?
        .map(|s| s.device)
        .ok_or_else(|| refused("sync is not set up on this device"))
}

/// The bound vault's ID.
pub(crate) fn vault_id(conn: &Connection) -> Result<[u8; 16]> {
    bound_vault(conn)?.ok_or_else(|| refused("no vault is bound on this device"))
}

/// The bound vault's ID, if a vault is bound.
fn bound_vault(conn: &Connection) -> Result<Option<[u8; 16]>> {
    let stored: Option<String> = conn
        .query_row("SELECT vault_id FROM sync_state WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    stored.map(|s| Ok(VaultId::from_b64(&s)?.0)).transpose()
}

const ENTRY_COLUMNS: &str = "device, cert, state, state_record, state_by, state_at, \
    cut_off_epoch, pinned_at, confirmed";

type RawEntry = (
    String,
    String,
    String,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    i64,
    bool,
);

fn raw_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<RawEntry> {
    Ok((
        row.get(0)?,
        row.get(1)?,
        row.get(2)?,
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
        row.get(8)?,
    ))
}

/// An entry from its stored columns; the key columns are read back from the certificate, which
/// was verified when it was pinned.
fn parse_entry(raw: RawEntry) -> Result<TrustEntry> {
    let (device, cert, state, record, by, at, cut_off, pinned_at, confirmed) = raw;
    let device = DeviceId::from_hex(&device).map_err(|_| corrupt())?;
    let cert = Certificate::from_b64(&cert).map_err(|_| corrupt())?;
    if cert.body.device != device {
        return Err(corrupt());
    }
    Ok(TrustEntry {
        device,
        kind: cert.body.kind,
        issuer: cert.body.issuer_device,
        sign_pub: cert.body.sign_pub,
        box_pub: cert.body.box_pub,
        ticket_pub: cert.ticket.as_ref().map(|t| t.ticket_pub),
        cert,
        state: TrustState::parse(&state)?,
        state_record: record
            .map(|r| Record::from_b64(&r))
            .transpose()
            .map_err(|_| corrupt())?,
        state_by: by
            .map(|b| DeviceId::from_hex(&b))
            .transpose()
            .map_err(|_| corrupt())?,
        state_at: at,
        cut_off_epoch: cut_off
            .map(u32::try_from)
            .transpose()
            .map_err(|_| corrupt())?,
        pinned_at,
        confirmed,
    })
}

/// The entry of `device`, if pinned.
pub fn entry(conn: &Connection, device: DeviceId) -> Result<Option<TrustEntry>> {
    conn.query_row(
        &format!("SELECT {ENTRY_COLUMNS} FROM sync_trust WHERE device = ?1"),
        [device.to_hex()],
        raw_entry,
    )
    .optional()?
    .map(parse_entry)
    .transpose()
}

/// Every entry, ascending device ID.
pub fn entries(conn: &Connection) -> Result<Vec<TrustEntry>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {ENTRY_COLUMNS} FROM sync_trust ORDER BY device"
    ))?;
    let raws = stmt
        .query_map([], raw_entry)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    raws.into_iter().map(parse_entry).collect()
}

/// When a superseded ID's re-join successors appeared: the earliest time this device pinned one
/// (`pinned_at`, this device's clock: the bound of what the old ID speaks for, revision §3.1 as
/// amended by R-25) and the earliest of that and any successor's own `issued_at` (`since`: what
/// the old ID wrote after it is no quiet copy, O11).
#[derive(Clone, Copy)]
struct Successors {
    pinned_at: i64,
    since: i64,
}

/// The store as the rules read it: every entry, each superseded ID's successors, and every entry
/// left out for its issuer (`excluded_by_issuer`), which speaks for nothing (R-22).
struct Store {
    entries: BTreeMap<DeviceId, TrustEntry>,
    successors: BTreeMap<DeviceId, Successors>,
    excluded: BTreeMap<DeviceId, ExclusionReason>,
}

impl Store {
    fn load(conn: &Connection) -> Result<Self> {
        let entries: BTreeMap<DeviceId, TrustEntry> =
            entries(conn)?.into_iter().map(|e| (e.device, e)).collect();
        let mut successors: BTreeMap<DeviceId, Successors> = BTreeMap::new();
        for entry in entries.values() {
            if entry.kind == CertKind::Rejoin {
                let at = successors.entry(entry.issuer).or_insert(Successors {
                    pinned_at: i64::MAX,
                    since: i64::MAX,
                });
                at.pinned_at = at.pinned_at.min(entry.pinned_at);
                at.since = at.since.min(entry.pinned_at).min(entry.cert.body.issued_at);
            }
        }
        let mut store = Store {
            entries,
            successors,
            excluded: BTreeMap::new(),
        };
        store.excluded = excluded_by_issuer(&store, &Positions::load(conn)?, this_device(conn)?);
        Ok(store)
    }

    fn get(&self, device: &DeviceId) -> Option<&TrustEntry> {
        self.entries.get(device)
    }

    fn contains(&self, device: &DeviceId) -> bool {
        self.entries.contains_key(device)
    }

    fn superseded(&self, device: &DeviceId) -> bool {
        self.successors.contains_key(device)
    }

    /// Whether `device` is left out for its issuer (`IssuerRemoved`, `LateVouch`,
    /// `IssuerUnknown`): it speaks for nothing, whatever it signed and whenever (R-22).
    fn silenced(&self, device: &DeviceId) -> bool {
        self.excluded.contains_key(device)
    }

    /// Whether `device` may speak now: pinned, `trusted`, and not left out for its issuer.
    fn may_speak(&self, device: &DeviceId) -> bool {
        self.get(device)
            .is_some_and(|e| e.state == TrustState::Trusted)
            && !self.silenced(device)
    }

    /// Whether some Kit other than `except` is `trusted`.
    fn trusted_kit_other_than(&self, except: DeviceId) -> bool {
        self.entries.values().any(|e| {
            e.kind == CertKind::Kit && e.state == TrustState::Trusted && e.device != except
        })
    }

    /// Whether any Kit was ever pinned here, whatever its state.
    fn has_kit(&self) -> bool {
        self.entries.values().any(|e| e.kind == CertKind::Kit)
    }
}

/// Pins `cert` as `trusted` (the caller has verified it).
fn pin(conn: &Connection, cert: &Certificate, now: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_trust (device, cert, kind, issuer, sign_pub, box_pub, ticket_pub, \
           state, pinned_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'trusted', ?8)",
        params![
            cert.body.device.to_hex(),
            cert.to_b64(),
            cert.body.kind.as_u8(),
            cert.body.issuer_device.to_hex(),
            cert.body.sign_pub.to_vec(),
            cert.body.box_pub.to_vec(),
            cert.ticket.as_ref().map(|t| t.ticket_pub.to_vec()),
            now
        ],
    )?;
    Ok(())
}

/// Whether some entry was pinned with this join ticket.
fn ticket_used(conn: &Connection, ticket_pub: &[u8; 32]) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT 1 FROM sync_trust WHERE ticket_pub = ?1",
            [ticket_pub.to_vec()],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// The certificate verifies on its own and belongs to this vault.
fn verified_in(cert: &Certificate, vault: &[u8; 16]) -> Result<()> {
    if &cert.body.vault_id != vault {
        return Err(refused("a device certificate names another vault"));
    }
    cert.verify_signatures()
}

/// Pins this device's own certificate (it made it). The same certificate again is a no-op; a
/// different one for this device is refused.
pub fn seed_own(conn: &Connection, cert: &Certificate, now: i64) -> Result<()> {
    let own = this_device(conn)?;
    if cert.body.device != own {
        return Err(refused("the certificate is not this device's"));
    }
    verified_in(cert, &vault_id(conn)?)?;
    match entry(conn, own)? {
        Some(pinned) if &pinned.cert == cert => Ok(()),
        Some(_) => Err(refused("this device already has another certificate")),
        None => pin(conn, cert, now),
    }
}

/// Whether removal and key changes are possible here: this device's own certificate is pinned.
/// A vault set up before phase 5 has none (spec §5.5, O3).
pub fn is_supported(conn: &Connection) -> Result<bool> {
    let Some(sync) = state::load(conn)? else {
        return Ok(false);
    };
    Ok(entry(conn, sync.device)?.is_some())
}

/// A `sync_devices` row's candidate columns. (`confirm_record` is in the format but unused since
/// revision §1.3: a confirmation is local, `keep`, or a key change's.)
struct DeviceRow {
    id: DeviceId,
    cert: Option<String>,
    left: Option<String>,
    removal: Option<String>,
}

fn device_rows(conn: &Connection) -> Result<Vec<DeviceRow>> {
    let mut stmt =
        conn.prepare("SELECT id, cert, left_record, removal_record FROM sync_devices ORDER BY id")?;
    let raws = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(raws
        .into_iter()
        .filter_map(|(id, cert, left, removal)| {
            // A row whose ID is not a device ID names nobody.
            DeviceId::from_hex(&id).ok().map(|id| DeviceRow {
                id,
                cert,
                left,
                removal,
            })
        })
        .collect())
}

/// `row`'s certificate when it decodes, belongs to this vault and this row, and verifies on its
/// own.
fn row_cert(row: &DeviceRow, vault: &[u8; 16]) -> Option<Certificate> {
    let cert = Certificate::from_b64(row.cert.as_deref()?).ok()?;
    if cert.body.device != row.id || verified_in(&cert, vault).is_err() {
        return None;
    }
    Some(cert)
}

/// `encoded` as a record of `kind` about `subject` in this vault, signature not yet checked.
fn row_record(
    encoded: Option<&str>,
    kind: RecordType,
    subject: DeviceId,
    vault: &[u8; 16],
) -> Option<Record> {
    let record = Record::from_b64(encoded?).ok()?;
    (record.kind == kind && record.subject == subject && &record.vault_id == vault)
        .then_some(record)
}

/// Writes a state move. `left` only from `trusted`; `removed` from anything but `removed`, or
/// fills the cut-off epoch of a `removed` entry that has none; a confirmation record moves nothing.
/// Returns whether it changed.
fn apply_move(conn: &Connection, record: &Record, cut_off_epoch: Option<u32>) -> Result<bool> {
    let subject = record.subject.to_hex();
    let encoded = record.to_b64();
    let changed = match record.kind {
        RecordType::Confirmed => 0,
        RecordType::Left => conn.execute(
            "UPDATE sync_trust SET state = 'left', state_record = ?2, state_by = ?3, \
               state_at = ?4 WHERE device = ?1 AND state = 'trusted'",
            params![subject, encoded, record.signer.to_hex(), record.at],
        )?,
        RecordType::Removed => {
            let moved = conn.execute(
                "UPDATE sync_trust SET state = 'removed', state_record = ?2, state_by = ?3, \
                   state_at = ?4, cut_off_epoch = ?5 WHERE device = ?1 AND state != 'removed'",
                params![
                    subject,
                    encoded,
                    record.signer.to_hex(),
                    record.at,
                    cut_off_epoch
                ],
            )?;
            if moved == 0 {
                match cut_off_epoch {
                    Some(epoch) => conn.execute(
                        "UPDATE sync_trust SET cut_off_epoch = ?2 WHERE device = ?1 \
                           AND state = 'removed' AND cut_off_epoch IS NULL",
                        params![subject, epoch],
                    )?,
                    None => 0,
                }
            } else {
                moved
            }
        }
    };
    Ok(changed > 0)
}

/// Whether the issuer of a paired, re-join, Kit-joined or Kit `cert` vouches for it: pinned with
/// the signing key the certificate names, and `speaks_at` both the certificate's `issued_at` and
/// its ticket's. An issuer left out for its issuer still vouches (R-22): what it vouches for is
/// left out with it (the exclusion is transitive) and is pinned only so that it shows as
/// "Zkontrolujte" and can be kept; a Kit is the exception (`pin_candidates`).
fn vouch(store: &Store, cert: &Certificate) -> bool {
    store.get(&cert.body.issuer_device).is_some_and(|issuer| {
        issuer.sign_pub == cert.body.issuer_sign_pub
            && speaks_at(store, issuer, cert.body.issued_at)
            && cert
                .ticket
                .as_ref()
                .is_none_or(|ticket| speaks_at(store, issuer, ticket.issued_at))
    })
}

/// What a pass does with a certificate that verifies on its own and is not pinned yet.
enum Verdict {
    Pin,
    /// Not now; a later pass may pin it.
    Wait,
}

/// What a pass knows when it judges a certificate.
struct Seen<'a> {
    store: &'a Store,
    /// Rows whose certificate verifies on its own and is not pinned.
    waiting: &'a BTreeSet<DeviceId>,
}

/// Whether `signer` speaks for something it signed at `at`: while it is `trusted`; after it left
/// or was removed, for what it signed until then (`at` no later than the `state_at` of the record
/// that changed its state); and, when a re-join successor of it is pinned, only until this device
/// pinned the first one (revision §3.1: a superseded ID lives on only as a copy; R-25: never the
/// successor's own `issued_at`, which whoever made it chose). So a record counts on every device
/// whether it arrives before or after the news of its signer's leaving.
fn speaks_at(store: &Store, signer: &TrustEntry, at: i64) -> bool {
    let in_state = match signer.state {
        TrustState::Trusted => true,
        TrustState::Left | TrustState::Removed => signer.state_at.is_some_and(|until| at <= until),
    };
    let before_successor = store
        .successors
        .get(&signer.device)
        .is_none_or(|s| at <= s.pinned_at);
    in_state && before_successor
}

/// `record`, a removal, verifies by a signer pinned in `store` that `speaks_at` its time and is
/// not left out for its issuer (R-22).
fn removal_counts(record: &Record, store: &Store) -> bool {
    store.get(&record.signer).is_some_and(|signer| {
        !store.silenced(&record.signer)
            && speaks_at(store, signer, record.at)
            && record.verify(&signer.sign_pub).is_ok()
    })
}

/// What the rows' records say, judged against the store before the pass.
struct Judged {
    /// Left and removal records to apply.
    moves: Vec<Record>,
    /// Records naming this device.
    about_me: Vec<Record>,
}

/// The records of every row, judged against `store`.
fn judge_records(rows: &[DeviceRow], store: &Store, own: DeviceId, vault: &[u8; 16]) -> Judged {
    let mut moves = Vec::new();
    let mut about_me = Vec::new();
    for row in rows {
        if let Some(left) = row_record(row.left.as_deref(), RecordType::Left, row.id, vault) {
            let verified = store
                .get(&row.id)
                .is_some_and(|subject| left.verify(&subject.sign_pub).is_ok());
            if verified {
                if row.id == own {
                    about_me.push(left);
                } else if store
                    .get(&row.id)
                    .is_some_and(|s| s.state == TrustState::Trusted)
                {
                    moves.push(left);
                }
            }
        }
        if let Some(removal) =
            row_record(row.removal.as_deref(), RecordType::Removed, row.id, vault)
        {
            if removal_counts(&removal, store) {
                if row.id == own {
                    about_me.push(removal);
                } else if store
                    .get(&row.id)
                    .is_some_and(|s| s.state != TrustState::Removed)
                {
                    moves.push(removal);
                }
            }
        }
    }
    Judged { moves, about_me }
}

fn push_conflict(report: &mut RefreshReport, device: DeviceId, kind: ConflictKind) {
    let conflict = TrustConflict { device, kind };
    if !report.conflicts.contains(&conflict) {
        log::warn!(
            "[SYNC] Ignored a device certificate for {} ({kind:?}); the pinned one stays",
            short(device)
        );
        report.conflicts.push(conflict);
    }
}

/// One pass over the rows' certificates: pins what `accept` allows, by ascending ID, a join
/// ticket at most once. A Kit only as the vault's first (R-24): while no Kit was ever pinned here,
/// the only one `accept` allows in the pass, and its creator not left out for its issuer; any
/// other is `SecondKit` (a replacement Kit comes in only by the key change that names it,
/// `import_bundle`). Returns whether anything was pinned.
fn pin_candidates(
    conn: &Connection,
    rows: &[DeviceRow],
    vault: &[u8; 16],
    now: i64,
    report: &mut RefreshReport,
    accept: impl Fn(&Certificate, &Seen<'_>) -> Verdict,
) -> Result<bool> {
    let store = Store::load(conn)?;
    let candidates: Vec<(DeviceId, Certificate)> = rows
        .iter()
        .filter_map(|row| row_cert(row, vault).map(|cert| (row.id, cert)))
        .collect();
    let waiting: BTreeSet<DeviceId> = candidates
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !store.contains(id))
        .collect();
    let seen = Seen {
        store: &store,
        waiting: &waiting,
    };
    let mut accepted = Vec::new();
    for (id, cert) in candidates {
        if let Some(pinned) = store.get(&id) {
            if pinned.cert != cert {
                push_conflict(report, id, ConflictKind::OtherCertificate);
            }
            continue;
        }
        if let Verdict::Pin = accept(&cert, &seen) {
            accepted.push((id, cert));
        }
    }
    // Never chosen by ID: with another Kit known, or several in one pass, none is pinned.
    let kits = accepted
        .iter()
        .filter(|(_, cert)| cert.body.kind == CertKind::Kit)
        .count();
    let mut tickets: HashSet<[u8; 32]> = HashSet::new();
    let mut pinned_any = false;
    for (id, cert) in accepted {
        if cert.body.kind == CertKind::Kit {
            if store.has_kit() || kits > 1 {
                push_conflict(report, id, ConflictKind::SecondKit);
                continue;
            }
            if store.silenced(&cert.body.issuer_device) {
                continue;
            }
        }
        if let Some(ticket) = &cert.ticket {
            if !tickets.insert(ticket.ticket_pub) || ticket_used(conn, &ticket.ticket_pub)? {
                push_conflict(report, id, ConflictKind::TicketReused);
                continue;
            }
        }
        pin(conn, &cert, now)?;
        report.pinned.push(id);
        pinned_any = true;
    }
    Ok(pinned_any)
}

/// Marks `device` `confirmed`. Returns whether it changed.
fn set_confirmed(conn: &Connection, device: DeviceId) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE sync_trust SET confirmed = 1 WHERE device = ?1 AND confirmed = 0",
        [device.to_hex()],
    )? > 0)
}

/// Logs, once per call, every certificate or record in the rows that does not hold up on its own
/// (`identity::write_once_value_valid`); the passes ignore them.
fn log_invalid(rows: &[DeviceRow], vault: &[u8; 16]) {
    for row in rows {
        let id = row.id.to_hex();
        for (column, value) in [
            ("cert", &row.cert),
            ("left_record", &row.left),
            ("removal_record", &row.removal),
        ] {
            let Some(value) = value.as_deref() else {
                continue;
            };
            if !super::write_once_value_valid(column, &id, value, Some(vault), row.cert.as_deref())
            {
                log::warn!(
                    "[SYNC] Ignored a {column} for {} that does not verify",
                    short(row.id)
                );
            }
        }
    }
}

/// The fixpoint of records then certificates over `sync_devices`, until nothing changes.
fn settle(
    conn: &Connection,
    now: i64,
    report: &mut RefreshReport,
    accept: impl Fn(&Certificate, &Seen<'_>) -> Verdict,
) -> Result<()> {
    let own = this_device(conn)?;
    let vault = vault_id(conn)?;
    let rows = device_rows(conn)?;
    log_invalid(&rows, &vault);
    loop {
        let store = Store::load(conn)?;
        let Judged { moves, about_me } = judge_records(&rows, &store, own, &vault);
        for record in about_me {
            if !report.about_me.contains(&record) {
                log::warn!("[SYNC] A verified record says this device left or was removed");
                report.about_me.push(record);
            }
        }
        let mut changed = false;
        for record in moves {
            if apply_move(conn, &record, None)? {
                // `moves` holds left and removal records only.
                let state = match record.kind {
                    RecordType::Left => TrustState::Left,
                    RecordType::Removed | RecordType::Confirmed => TrustState::Removed,
                };
                report.moved.push((record.subject, state));
                changed = true;
            }
        }
        changed |= pin_candidates(conn, &rows, &vault, now, report, &accept)?;
        if !changed {
            return Ok(());
        }
    }
}

/// Brings the store up to date with `sync_devices` (spec §5.4, revision §2.3): after every merge
/// that touched it, at unlock and after a key pickup. Pins paired, re-join, Kit-joined and Kit
/// certificates whose issuer vouches (a Kit only while no other is `trusted`), applies verified
/// records, and repeats until nothing new verifies. Every paired or Kit-joined device pinned here
/// (not this one) gets a new-device notice (`NEW_DEVICES_KEY`, revision §3.3); a re-join says
/// nothing. One savepoint.
pub fn refresh(conn: &Connection, now: i64) -> Result<RefreshReport> {
    in_savepoint(conn, "sync_trust_refresh", || {
        let mut report = RefreshReport::default();
        settle(conn, now, &mut report, |cert, seen| match cert.body.kind {
            CertKind::Genesis => Verdict::Wait,
            CertKind::Paired | CertKind::Rejoin | CertKind::RecoveryKit | CertKind::Kit => {
                if vouch(seen.store, cert) {
                    Verdict::Pin
                } else {
                    Verdict::Wait
                }
            }
        })?;
        note_new_devices(conn, &report.pinned, now)?;
        Ok(report)
    })
}

/// Appends a notice for each newly pinned paired or Kit-joined device other than this one,
/// keeping the newest `MAX_NEW_DEVICES`.
fn note_new_devices(conn: &Connection, pinned: &[DeviceId], now: i64) -> Result<()> {
    let own = this_device(conn)?;
    let mut fresh = Vec::new();
    for device in pinned {
        let Some(entry) = entry(conn, *device)? else {
            continue;
        };
        if *device == own || !matches!(entry.kind, CertKind::Paired | CertKind::RecoveryKit) {
            continue;
        }
        fresh.push(NewDevice {
            device: device.to_hex(),
            by: entry.issuer.to_hex(),
            at: now,
            via_kit: entry.kind == CertKind::RecoveryKit,
        });
    }
    if fresh.is_empty() {
        return Ok(());
    }
    let mut notices = new_devices(conn)?;
    notices.extend(fresh);
    let drop = notices.len().saturating_sub(MAX_NEW_DEVICES);
    notices.drain(..drop);
    let json = serde_json::to_string(&notices)
        .map_err(|e| AppError::Internal(format!("sync: new-device notices: {e}")))?;
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![NEW_DEVICES_KEY, json],
    )?;
    Ok(())
}

/// The new-device notices, oldest first (revision §3.3); none when the key is missing or does not
/// read.
pub fn new_devices(conn: &Connection) -> Result<Vec<NewDevice>> {
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [NEW_DEVICES_KEY],
            |r| r.get(0),
        )
        .optional()?;
    Ok(stored
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default())
}

/// A Recovery Kit joiner's first store (spec §5.4, §13): trust on first use of every row whose
/// certificate verifies, genesis and unknown issuers included, then the records as `refresh`
/// applies them. A certificate whose issuer is pinned or has a row still waiting is judged as
/// `refresh` judges it (it waits for its issuer); one whose issuer has no verifiable row is
/// trusted on first use. At most one Kit is pinned, as in `refresh`. Writes no new-device notice.
pub fn tofu_from_rows(conn: &Connection, now: i64) -> Result<RefreshReport> {
    in_savepoint(conn, "sync_trust_tofu", || {
        let mut report = RefreshReport::default();
        settle(conn, now, &mut report, |cert, seen| match cert.body.kind {
            CertKind::Genesis => Verdict::Pin,
            CertKind::Paired | CertKind::Rejoin | CertKind::RecoveryKit | CertKind::Kit => {
                let issuer = cert.body.issuer_device;
                if vouch(seen.store, cert) {
                    Verdict::Pin
                } else if seen.store.contains(&issuer) || seen.waiting.contains(&issuer) {
                    Verdict::Wait
                } else {
                    Verdict::Pin
                }
            }
        })?;
        Ok(report)
    })
}

/// This device's whole store, for a joiner it pairs, with where it first saw each entry's
/// certificate, `left` record and ticket announcement (R-23).
pub fn export(conn: &Connection) -> Result<TrustExport> {
    let positions = Positions::load(conn)?;
    Ok(TrustExport {
        entries: entries(conn)?
            .into_iter()
            .map(|e| ExportEntry {
                cert_seen: positions.seen(e.device, SeenWhat::Cert),
                left_seen: positions.seen(e.device, SeenWhat::Left),
                ticket_seen: positions.announced(&e),
                cert: e.cert.to_b64(),
                state: e.state,
                record: e.state_record.map(|r| r.to_b64()),
                cut_off_epoch: e.cut_off_epoch,
                confirmed: e.confirmed,
            })
            .collect(),
    })
}

/// Seeds an empty store from a holder's export, received over the authenticated pairing channel:
/// it must contain the `holder`'s own certificate in state `trusted`, at most one `trusted` Kit,
/// and every entry must verify (its certificate on its own and in this vault; a `left` record
/// signed by its subject, a `removed` record by another entry of the export; no record for a
/// `trusted` entry; a ticket position only for a certificate with a ticket). Anything wrong
/// refuses the whole export and writes nothing. The holder's positions replace whatever this
/// device noted for the exported devices (a join's snapshot restore notes its own position for
/// everything, R-23), so the joiner leaves out for their issuer exactly what the holder does.
pub fn import_export(
    conn: &Connection,
    export: &TrustExport,
    holder: DeviceId,
    now: i64,
) -> Result<()> {
    let invalid = || refused("the pairing trust export is not valid");
    if export.entries.len() > MAX_EXPORT_ENTRIES {
        return Err(invalid());
    }
    let vault = vault_id(conn)?;
    let mut certs: BTreeMap<DeviceId, (Certificate, &ExportEntry)> = BTreeMap::new();
    let mut tickets = HashSet::new();
    for item in &export.entries {
        let cert = Certificate::from_b64(&item.cert).map_err(|_| invalid())?;
        verified_in(&cert, &vault).map_err(|_| invalid())?;
        if let Some(ticket) = &cert.ticket {
            if !tickets.insert(ticket.ticket_pub) {
                return Err(invalid());
            }
        }
        if certs.insert(cert.body.device, (cert, item)).is_some() {
            return Err(invalid());
        }
    }
    match certs.get(&holder) {
        Some((_, item)) if item.state == TrustState::Trusted => {}
        _ => return Err(invalid()),
    }
    let trusted_kits = certs
        .values()
        .filter(|(cert, item)| cert.body.kind == CertKind::Kit && item.state == TrustState::Trusted)
        .count();
    if trusted_kits > 1 {
        return Err(invalid());
    }
    let mut checked = Vec::with_capacity(certs.len());
    for (device, (cert, item)) in &certs {
        let record = item
            .record
            .as_deref()
            .map(Record::from_b64)
            .transpose()
            .map_err(|_| invalid())?;
        match (item.state, &record) {
            (TrustState::Trusted, None) if item.cut_off_epoch.is_none() => {}
            (TrustState::Left, Some(r)) if r.kind == RecordType::Left => {
                r.verify(&cert.body.sign_pub).map_err(|_| invalid())?;
            }
            (TrustState::Removed, Some(r)) if r.kind == RecordType::Removed => {
                let (signer, _) = certs.get(&r.signer).ok_or_else(invalid)?;
                r.verify(&signer.body.sign_pub).map_err(|_| invalid())?;
            }
            _ => return Err(invalid()),
        }
        if let Some(r) = &record {
            if r.subject != *device || r.vault_id != vault {
                return Err(invalid());
            }
        }
        if item.ticket_seen.is_some() && cert.ticket.is_none() {
            return Err(invalid());
        }
        for pos in [item.cert_seen, item.left_seen, item.ticket_seen]
            .into_iter()
            .flatten()
        {
            position(pos).map_err(|_| invalid())?;
        }
        checked.push((cert, item, record));
    }
    in_savepoint(conn, "sync_trust_import", || {
        let empty: bool =
            conn.query_row("SELECT COUNT(*) = 0 FROM sync_trust", [], |r| r.get(0))?;
        if !empty {
            return Err(refused("the trust store is not empty"));
        }
        for (cert, item, record) in &checked {
            pin(conn, cert, now)?;
            if item.confirmed {
                set_confirmed(conn, cert.body.device)?;
            }
            if let Some(record) = record {
                conn.execute(
                    "UPDATE sync_trust SET state = ?2, state_record = ?3, state_by = ?4, \
                       state_at = ?5, cut_off_epoch = ?6 WHERE device = ?1",
                    params![
                        cert.body.device.to_hex(),
                        item.state.as_str(),
                        record.to_b64(),
                        record.signer.to_hex(),
                        record.at,
                        item.cut_off_epoch
                    ],
                )?;
            }
            let device = cert.body.device;
            conn.execute(
                "DELETE FROM sync_trust_seen WHERE device = ?1",
                [device.to_hex()],
            )?;
            for (what, pos) in [
                (SeenWhat::Cert, item.cert_seen),
                (SeenWhat::Left, item.left_seen),
            ] {
                if let Some(pos) = pos {
                    note_seen(conn, device, what, pos)?;
                }
            }
            if let (Some(ticket), Some(pos)) = (&cert.ticket, item.ticket_seen) {
                conn.execute(
                    "INSERT INTO sync_tickets (ticket_pub, issuer, pos) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(ticket_pub) DO UPDATE SET issuer = excluded.issuer, \
                       pos = excluded.pos",
                    params![
                        ticket.ticket_pub.to_vec(),
                        cert.body.issuer_device.to_hex(),
                        position(pos)?
                    ],
                )?;
            }
        }
        Ok(())
    })
}

/// What an accepted key-change bundle teaches (spec §6.3, revision §2.5, §3.1): its signer's and
/// its recipients' certificates, endorsed by the signer, are pinned where this device has none (a
/// pinned one stays) and marked `confirmed`; its removal records, and the Kit's `replaces` record,
/// move their subjects to `removed`, cut off at `to_epoch`; then its Kit is pinned (where none is)
/// and `confirmed`. The signer must `may_speak` here with exactly this certificate (pinned,
/// `trusted`, not left out for its issuer: R-22; the caller checks the bundle's signature with
/// `speaker`). Every certificate must verify (the Kit's must be a kind 5), every removal record
/// must count as `refresh` judges it once the bundle's certificates are in (its signer pinned,
/// speaking at the record's time and not left out for its issuer), and the Kit must not be a
/// second one once the removals are applied, or nothing is written. A removal naming this device
/// is not applied but returned: the caller ends sync here (spec §7.4). One savepoint.
pub fn import_bundle(
    conn: &Connection,
    signer_cert: &Certificate,
    recipients: &[Certificate],
    kit: Option<BundleKit<'_>>,
    removed_records: &[Record],
    to_epoch: u32,
    now: i64,
) -> Result<Vec<Record>> {
    let vault = vault_id(conn)?;
    let own = this_device(conn)?;
    let certs: Vec<&Certificate> = std::iter::once(signer_cert).chain(recipients).collect();
    for cert in &certs {
        verified_in(cert, &vault)?;
    }
    if let Some(kit) = &kit {
        if kit.cert.body.kind != CertKind::Kit {
            return Err(refused("a key change names a Kit that is not one"));
        }
        verified_in(kit.cert, &vault)?;
    }
    let removals: Vec<&Record> = removed_records
        .iter()
        .chain(kit.as_ref().and_then(|k| k.replaces))
        .collect();
    in_savepoint(conn, "sync_trust_bundle", || {
        // Before anything is pinned or confirmed: the bundle's endorsements are worth only what
        // its signer may say.
        if !speaks_now(&Store::load(conn)?, signer_cert) {
            return Err(refused(
                "a key change is signed by a device that may not speak here",
            ));
        }
        for cert in &certs {
            pin_endorsed(conn, cert, now)?;
        }
        // Every record is judged against the store with the bundle's certificates in and before
        // any of its removals, as one `refresh` pass judges records.
        let store = Store::load(conn)?;
        let mut about_me = Vec::new();
        for record in &removals {
            if record.kind != RecordType::Removed || record.vault_id != vault {
                return Err(refused("a key change carries an invalid removal record"));
            }
            if !removal_counts(record, &store) {
                return Err(refused(
                    "a key change carries a removal by a device not trusted here",
                ));
            }
            if record.subject == own {
                log::warn!("[SYNC] A key change says this device was removed");
                about_me.push((*record).clone());
            }
        }
        for record in &removals {
            if record.subject != own {
                apply_move(conn, record, Some(to_epoch))?;
            }
        }
        if let Some(kit) = &kit {
            let store = Store::load(conn)?;
            let new = !store.contains(&kit.cert.body.device);
            if new && store.trusted_kit_other_than(kit.cert.body.device) {
                return Err(refused("a key change names a second Kit"));
            }
            pin_endorsed(conn, kit.cert, now)?;
        }
        Ok(about_me)
    })
}

/// Whether `cert` is pinned in `store` exactly and its device may speak now.
fn speaks_now(store: &Store, cert: &Certificate) -> bool {
    store
        .get(&cert.body.device)
        .is_some_and(|pinned| &pinned.cert == cert)
        && store.may_speak(&cert.body.device)
}

/// The entry of the device `cert` names when it is pinned with exactly `cert` and may speak now
/// (R-22): `trusted` and not left out for its issuer (`IssuerRemoved`, `LateVouch`,
/// `IssuerUnknown`; a `confirmed` entry never is). Whoever signs a key change must be one
/// (`rekey::pickup`).
pub fn speaker(conn: &Connection, cert: &Certificate) -> Result<Option<TrustEntry>> {
    let mut store = Store::load(conn)?;
    if !speaks_now(&store, cert) {
        return Ok(None);
    }
    Ok(store.entries.remove(&cert.body.device))
}

/// A certificate a verified key change endorses: pinned where this device has none (unless its
/// join ticket is already used) and `confirmed`; a different pinned one stays and is not
/// confirmed.
fn pin_endorsed(conn: &Connection, cert: &Certificate, now: i64) -> Result<()> {
    match entry(conn, cert.body.device)? {
        Some(pinned) => {
            if &pinned.cert != cert {
                log::warn!(
                    "[SYNC] A key change names another certificate for {}; the pinned one stays",
                    short(cert.body.device)
                );
            } else {
                set_confirmed(conn, pinned.device)?;
            }
        }
        None => {
            let reused = match &cert.ticket {
                Some(ticket) => ticket_used(conn, &ticket.ticket_pub)?,
                None => false,
            };
            if reused {
                log::warn!(
                    "[SYNC] A key change names {} with a used join ticket; not pinned",
                    short(cert.body.device)
                );
            } else {
                pin(conn, cert, now)?;
                set_confirmed(conn, cert.body.device)?;
            }
        }
    }
    Ok(())
}

/// Moves `record.subject` from `trusted` to `left` (the record must verify with the subject's
/// pinned key). Returns whether it moved.
pub fn mark_left(conn: &Connection, record: &Record) -> Result<bool> {
    if record.kind != RecordType::Left || record.vault_id != vault_id(conn)? {
        return Err(refused("not a left record of this vault"));
    }
    let subject =
        entry(conn, record.subject)?.ok_or_else(|| refused("the device is not pinned here"))?;
    record.verify(&subject.sign_pub)?;
    apply_move(conn, record, None)
}

/// Moves `record.subject` to `removed`, cut off at `cut_off_epoch` (`None`: it still knows the
/// current key), or fills the cut-off of an entry already removed without one. The signer must be
/// pinned and speak for the record's time as `refresh` judges it (`trusted`, or left or removed
/// after the record's `at`). Returns whether anything changed.
pub fn mark_removed(
    conn: &Connection,
    record: &Record,
    cut_off_epoch: Option<u32>,
) -> Result<bool> {
    if record.kind != RecordType::Removed || record.vault_id != vault_id(conn)? {
        return Err(refused("not a removal record of this vault"));
    }
    if !removal_counts(record, &Store::load(conn)?) {
        return Err(refused("the removal's signer is not trusted here"));
    }
    apply_move(conn, record, cut_off_epoch)
}

/// "Je moje" (revision §6.1): the person keeps `device`, a pinned `trusted` device other than
/// this one, on this device. It is marked `confirmed` here only (no record); the next key change
/// this device makes names it, which confirms it everywhere. Returns whether it changed.
pub fn keep(conn: &Connection, device: DeviceId) -> Result<bool> {
    let own = this_device(conn)?;
    let trusted = entry(conn, device)?.is_some_and(|e| e.state == TrustState::Trusted);
    if device == own || !trusted {
        return Err(refused("only another trusted device can be kept"));
    }
    set_confirmed(conn, device)
}

/// A key change this device made committed (revision §3.1): every one of `devices` that is pinned
/// and `trusted` here (the recipients and the Kit) is marked `confirmed`; others are passed over.
pub fn mark_confirmed(conn: &Connection, devices: &[DeviceId]) -> Result<()> {
    for device in devices {
        if entry(conn, *device)?.is_some_and(|e| e.state == TrustState::Trusted) {
            set_confirmed(conn, *device)?;
        }
    }
    Ok(())
}

/// A Kit join's envelope (revision §2.6 step 6): every `(device, certificate hash)` whose device is
/// pinned here with a certificate of that hash is marked `confirmed`; others are ignored. Returns
/// how many matched.
pub fn apply_confirmed_set(conn: &Connection, items: &[(DeviceId, [u8; 32])]) -> Result<usize> {
    in_savepoint(conn, "sync_trust_confirmed_set", || {
        let mut matched = 0;
        for (device, hash) in items {
            if entry(conn, *device)?.is_some_and(|e| &e.cert.hash() == hash) {
                set_confirmed(conn, *device)?;
                matched += 1;
            }
        }
        Ok(matched)
    })
}

/// Whether `record` names this vault and counts here as `refresh` judges records: a removal by a
/// signer pinned here that speaks for the record's time (`removal_counts`); a `left` record signed
/// by its subject's pinned key; never a confirmation record (unused, revision §1.3). For a removal
/// notice a key-change bundle carries to this device (phase 5 §7.4).
pub fn record_counts(conn: &Connection, record: &Record) -> Result<bool> {
    if record.vault_id != vault_id(conn)? {
        return Ok(false);
    }
    Ok(match record.kind {
        RecordType::Confirmed => false,
        RecordType::Removed => removal_counts(record, &Store::load(conn)?),
        RecordType::Left => entry(conn, record.subject)?
            .is_some_and(|subject| record.verify(&subject.sign_pub).is_ok()),
    })
}

/// After a key change to `epoch`: every left or removed entry that still knew the key is cut off
/// at it. Returns how many.
pub fn cut_off_all_pending(conn: &Connection, epoch: u32) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE sync_trust SET cut_off_epoch = ?1 \
         WHERE state IN ('left', 'removed') AND cut_off_epoch IS NULL",
        [epoch],
    )?)
}

/// Where something about a device was first seen in the relay's log (revision §3.1): its
/// certificate, or its `left` record.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SeenWhat {
    Cert,
    Left,
}

impl SeenWhat {
    fn as_str(self) -> &'static str {
        match self {
            SeenWhat::Cert => "cert",
            SeenWhat::Left => "left",
        }
    }
}

/// Records that `device`'s `what` was first seen at relay position `pos` (`sync_trust_seen`, Local;
/// the first one wins). The pull calls it for every applied `sync_devices` change that brings a
/// `cert` or `left_record` value, and a snapshot restore with the snapshot's position.
pub fn note_seen(conn: &Connection, device: DeviceId, what: SeenWhat, pos: u64) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sync_trust_seen (device, what, pos) VALUES (?1, ?2, ?3)",
        params![device.to_hex(), what.as_str(), position(pos)?],
    )?;
    Ok(())
}

/// Notes `value`, a `cert` or `left_record` value of `device`'s row an applied change or a
/// restored snapshot brought at relay position `pos`, as a sighting (`note_seen`) only when the
/// row now holds it (the merge kept it) and it verifies: a certificate on its own in this vault
/// and for this row; a `left` record signed by its subject's pinned key, or else by the key of the
/// row's own certificate (R-27). So junk, or a value the merge discarded, never moves a first
/// sighting earlier. Nothing is noted while no vault is bound.
pub fn note_sighting(
    conn: &Connection,
    device: DeviceId,
    what: SeenWhat,
    value: &str,
    pos: u64,
) -> Result<()> {
    let Some(vault) = bound_vault(conn)? else {
        return Ok(());
    };
    let held: Option<(Option<String>, Option<String>)> = conn
        .query_row(
            "SELECT cert, left_record FROM sync_devices WHERE id = ?1",
            [device.to_hex()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((cert, left)) = held else {
        return Ok(());
    };
    let row = DeviceRow {
        id: device,
        cert,
        left,
        removal: None,
    };
    let counts = match what {
        SeenWhat::Cert => row.cert.as_deref() == Some(value) && row_cert(&row, &vault).is_some(),
        SeenWhat::Left => {
            let key = match entry(conn, device)? {
                Some(pinned) => Some(pinned.sign_pub),
                None => row_cert(&row, &vault).map(|c| c.body.sign_pub),
            };
            row.left.as_deref() == Some(value)
                && row_record(Some(value), RecordType::Left, device, &vault)
                    .zip(key)
                    .is_some_and(|(record, key)| record.verify(&key).is_ok())
        }
    };
    if counts {
        note_seen(conn, device, what, pos)?;
    }
    Ok(())
}

/// Records that `issuer` announced the join ticket `ticket_pub` at relay position `pos`
/// (`sync_tickets`, Local; the first one wins).
pub fn note_ticket(
    conn: &Connection,
    ticket_pub: &[u8; 32],
    issuer: DeviceId,
    pos: u64,
) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO sync_tickets (ticket_pub, issuer, pos) VALUES (?1, ?2, ?3)",
        params![ticket_pub.to_vec(), issuer.to_hex(), position(pos)?],
    )?;
    Ok(())
}

fn position(pos: u64) -> Result<i64> {
    i64::try_from(pos).map_err(|_| refused("a relay position is out of range"))
}

/// The first-seen positions and announced tickets, as `recipients` reads them.
struct Positions {
    seen: BTreeMap<(DeviceId, SeenWhat), u64>,
    tickets: BTreeMap<[u8; 32], (DeviceId, u64)>,
}

impl Positions {
    fn load(conn: &Connection) -> Result<Self> {
        let mut seen = BTreeMap::new();
        let mut stmt = conn.prepare("SELECT device, what, pos FROM sync_trust_seen")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (device, what, pos) in rows {
            let what = match what.as_str() {
                "cert" => SeenWhat::Cert,
                "left" => SeenWhat::Left,
                _ => continue,
            };
            if let (Ok(device), Ok(pos)) = (DeviceId::from_hex(&device), u64::try_from(pos)) {
                seen.insert((device, what), pos);
            }
        }
        let mut tickets = BTreeMap::new();
        let mut stmt = conn.prepare("SELECT ticket_pub, issuer, pos FROM sync_tickets")?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, Vec<u8>>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for (ticket, issuer, pos) in rows {
            if let (Ok(ticket), Ok(issuer), Ok(pos)) = (
                <[u8; 32]>::try_from(ticket),
                DeviceId::from_hex(&issuer),
                u64::try_from(pos),
            ) {
                tickets.insert(ticket, (issuer, pos));
            }
        }
        Ok(Positions { seen, tickets })
    }

    fn seen(&self, device: DeviceId, what: SeenWhat) -> Option<u64> {
        self.seen.get(&(device, what)).copied()
    }

    /// Where `issuer` stopped speaking in the log, as `entry` is judged: where its `left` record
    /// was first seen, or where its earliest re-join successor's certificate (other than `entry`'s
    /// own) was, whichever comes first. `None` when neither was seen (it came by a vetted path: an
    /// export without positions, a bundle, this device).
    fn end_of(&self, store: &Store, issuer: &TrustEntry, entry: DeviceId) -> Option<u64> {
        let left = (issuer.state != TrustState::Trusted)
            .then(|| self.seen(issuer.device, SeenWhat::Left))
            .flatten();
        let successor = store
            .entries
            .values()
            .filter(|e| {
                e.kind == CertKind::Rejoin && e.issuer == issuer.device && e.device != entry
            })
            .filter_map(|e| self.seen(e.device, SeenWhat::Cert))
            .min();
        match (left, successor) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Where `entry`'s issuer announced its join ticket, if it did.
    fn announced(&self, entry: &TrustEntry) -> Option<u64> {
        entry
            .ticket_pub
            .and_then(|t| self.tickets.get(&t))
            .filter(|(by, _)| *by == entry.issuer)
            .map(|(_, pos)| *pos)
    }

    /// Where `entry`'s vouch was first seen: its certificate, or the announcement of its join
    /// ticket by its issuer, whichever comes first. `None` when neither was seen.
    fn vouch_of(&self, entry: &TrustEntry) -> Option<u64> {
        let cert = self.seen(entry.device, SeenWhat::Cert);
        let ticket = self.announced(entry);
        match (cert, ticket) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }
}

/// Every entry left out for its issuer (revision §3.1), computed to a fixpoint: not `confirmed`,
/// not this device, not a genesis, and its issuer is `removed` here or is itself in the set for
/// that (`IssuerRemoved`); or its issuer left or was superseded and its vouch was not first seen
/// before the issuer's end (`LateVouch`, `Positions::end_of`; equal positions, which only one
/// restored snapshot gives, cannot be told apart and are late, R-23; an unknown position on either
/// side is in time: the entry or the end came by a vetted path); or its issuer is not pinned here
/// (`IssuerUnknown`, R-23); or its issuer is itself in the set for one of the last two, which it
/// passes on. Entries of every state take part, so a chain through a device that has since left
/// still drops. A `confirmed` entry and this device are never in the set and stop the chain.
/// What is in the set also speaks for nothing (R-22).
fn excluded_by_issuer(
    store: &Store,
    positions: &Positions,
    own: DeviceId,
) -> BTreeMap<DeviceId, ExclusionReason> {
    let mut out: BTreeMap<DeviceId, ExclusionReason> = BTreeMap::new();
    loop {
        let mut changed = false;
        for entry in store.entries.values() {
            if entry.confirmed
                || entry.device == own
                || entry.kind == CertKind::Genesis
                || out.contains_key(&entry.device)
            {
                continue;
            }
            let Some(issuer) = store.get(&entry.issuer) else {
                out.insert(entry.device, ExclusionReason::IssuerUnknown);
                changed = true;
                continue;
            };
            let reason = if issuer.state == TrustState::Removed
                || out.get(&issuer.device) == Some(&ExclusionReason::IssuerRemoved)
            {
                Some(ExclusionReason::IssuerRemoved)
            } else if let Some(passed) = out.get(&issuer.device) {
                Some(*passed)
            } else if issuer.state == TrustState::Left || store.superseded(&issuer.device) {
                let end = positions.end_of(store, issuer, entry.device);
                let late = match (positions.vouch_of(entry), end) {
                    (Some(vouch), Some(end)) => vouch >= end,
                    _ => false,
                };
                late.then_some(ExclusionReason::LateVouch)
            } else {
                None
            };
            if let Some(reason) = reason {
                out.insert(entry.device, reason);
                changed = true;
            }
        }
        if !changed {
            return out;
        }
    }
}

/// The newest clock time (unix seconds) each device wrote.
fn newest_clock_secs(conn: &Connection) -> Result<BTreeMap<DeviceId, i64>> {
    let mut stmt = conn.prepare("SELECT device, MAX(hlc) FROM sync_clock GROUP BY device")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(device, hlc)| {
            let id: [u8; 16] = device.try_into().ok()?;
            let (ms, _) = unpack(u64::try_from(hlc).ok()?);
            Some((DeviceId(id), (ms / 1000) as i64))
        })
        .collect())
}

/// Whether a `sync_devices` row's `cert` is a Kit's own certificate (kind 5) that verifies on
/// its own in `vault` and names this row: the only row that is a Kit's, whatever its `platform`
/// says (a row that only calls itself `recoveryKit` is an ordinary unverified row, R-26).
pub(crate) fn is_kit_row(row_id: DeviceId, cert: Option<&str>, vault: &[u8; 16]) -> bool {
    cert.and_then(|c| Certificate::from_b64(c).ok())
        .is_some_and(|c| {
            c.body.kind == CertKind::Kit
                && c.body.device == row_id
                && verified_in(&c, vault).is_ok()
        })
}

/// The Kit a new key is sealed to (revision §2.5): among the `trusted` Kits not left out for
/// their creator (R-24), the `confirmed` one, else the only one; none when that is not one Kit
/// (never chosen by ID; the store holds at most one `trusted` Kit unless something is wrong).
fn sealing_kit(store: &Store) -> Option<TrustEntry> {
    let usable: Vec<&TrustEntry> = store
        .entries
        .values()
        .filter(|e| {
            e.kind == CertKind::Kit && e.state == TrustState::Trusted && !store.silenced(&e.device)
        })
        .collect();
    let confirmed: Vec<&TrustEntry> = usable.iter().copied().filter(|e| e.confirmed).collect();
    let pick = if confirmed.is_empty() {
        usable
    } else {
        confirmed
    };
    match pick.as_slice() {
        [one] => Some((*one).clone()),
        [] => None,
        _ => {
            log::warn!("[SYNC] More than one Recovery Kit could be sealed to; none is");
            None
        }
    }
}

/// Who would receive a new key if `removing` were removed now (revision §3.1): every pinned
/// `trusted` device entry, this device included, minus `removing`, minus superseded IDs that
/// wrote nothing after their successor appeared (O11), minus the entries `excluded_by_issuer`
/// leaves out (`IssuerRemoved`, `LateVouch`, `IssuerUnknown`; never this device). Every other
/// device of the list or the store comes with its reason. The Kit is returned apart
/// (`Recipients::kit`, `sealing_kit`) and never listed otherwise, nor is the row of a Kit that is
/// not pinned (`is_kit_row`). Refuses more than 64 recipients (`validation.syncTooManyDevices`);
/// what is left out does not count.
pub fn recipients(conn: &Connection, removing: &[DeviceId]) -> Result<Recipients> {
    let store = Store::load(conn)?;
    let vault = vault_id(conn)?;
    // Each row: whether it has a public key, and whether it is a Kit's.
    let mut rows: BTreeMap<DeviceId, (bool, bool)> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT id, public_key IS NOT NULL, cert FROM sync_devices")?;
    for row in stmt.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, bool>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })? {
        let (id, has_key, cert) = row?;
        if let Ok(id) = DeviceId::from_hex(&id) {
            rows.insert(id, (has_key, is_kit_row(id, cert.as_deref(), &vault)));
        }
    }
    let clocks = newest_clock_secs(conn)?;
    let removing: BTreeSet<DeviceId> = removing.iter().copied().collect();
    let devices: BTreeSet<DeviceId> = store.entries.keys().chain(rows.keys()).copied().collect();
    let mut included = Vec::new();
    let mut excluded = Vec::new();
    let kit = sealing_kit(&store);
    for device in devices {
        let entry = store.get(&device);
        if entry.is_some_and(|e| e.kind == CertKind::Kit) {
            continue;
        }
        if entry.is_none() && rows.get(&device).is_some_and(|(_, kit_row)| *kit_row) {
            continue;
        }
        let reason = match entry {
            _ if removing.contains(&device) => Some(ExclusionReason::Removing),
            Some(entry) => match entry.state {
                TrustState::Left => Some(ExclusionReason::Left),
                TrustState::Removed => Some(ExclusionReason::Removed),
                TrustState::Trusted => store.excluded.get(&device).copied().or_else(|| {
                    store.successors.get(&device).and_then(|successors| {
                        let wrote_since =
                            clocks.get(&device).is_some_and(|at| *at > successors.since);
                        (!wrote_since).then_some(ExclusionReason::SupersededQuiet)
                    })
                }),
            },
            None if rows.get(&device).is_some_and(|(has_key, _)| *has_key) => {
                Some(ExclusionReason::Unverified)
            }
            None => Some(ExclusionReason::NoKey),
        };
        match (reason, entry) {
            (Some(reason), _) => excluded.push((device, reason)),
            (None, Some(entry)) => included.push(entry.clone()),
            (None, None) => {}
        }
    }
    if included.len() > MAX_RECIPIENTS {
        return Err(AppError::Validation("validation.syncTooManyDevices".into()));
    }
    Ok(Recipients {
        included,
        excluded,
        kit,
    })
}
