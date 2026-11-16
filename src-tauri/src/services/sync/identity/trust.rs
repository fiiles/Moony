//! The pinned trust store (phase 5 spec §5.4, §5.5): the Local table `sync_trust`, one entry per
//! device ID this device has verified, with the certificate, its kind and issuer, the keys it
//! binds, a state and the signed record behind the state. It decides who may receive a new vault
//! key (`recipients`); the synced `sync_devices` columns are only ever a source of candidates.
//!
//! - The first verified certificate for an ID is pinned for good: a different one later is
//!   ignored and reported (`RefreshReport::conflicts`), whatever its clock.
//! - States only move forward: `trusted` → `left` → `removed`, or `trusted` → `removed`.
//! - `cut_off_epoch` of a left or removed entry is the first key epoch it does not have; NULL
//!   while it still knows the current key.
//!
//! How entries get in is the table of spec §5.4: this device's own certificate (`seed_own`), a
//! holder's whole store over the pairing channel (`import_export`), a paired or re-join
//! certificate whose issuer is pinned here, a Recovery Kit certificate (labelled), the signer and
//! recipients a key-change bundle endorses (`import_bundle`), and a Kit joiner's trust on first
//! use (`tofu_from_rows`). A genesis certificate arriving through sync is never pinned.
//!
//! Anyone with the vault key can make a Recovery Kit certificate, so one is pinned
//! `kit_unconfirmed`, and so is what it vouches for: such an entry gets a new key only when the
//! person ticks it (`recipients`), does not count toward the 64 otherwise, and signs no removal
//! that counts here, until it is confirmed (`confirm`: a person confirmed it on a trusted device,
//! here or on another whose signed confirmation arrived; or a key change naming it). A Kit
//! certificate for a device ID another certificate names as its issuer with another signing key
//! is refused (`ConflictKind::KitClaimsKnownDevice`).
//!
//! An issuer vouches when its entry is pinned with the same signing key and is `trusted`, or left
//! or was removed after it issued the certificate and its join ticket (`vouches`): a device that
//! left signed what it paired while it was in (its signing key goes with its `sync_state` row when
//! it leaves), so its earlier pairings stay verifiable, and nothing it dates later. A `removed` record
//! counts when its signer is pinned and either `trusted`, or left or removed by a record whose
//! time is not before the removal's own (`speaks_at`): what a device signed while it was in counts
//! whether the news of its leaving arrives before or after. Within one `refresh` pass every record
//! is judged against the store as it was before the pass, so the outcome never depends on row
//! order; records apply before certificates, so a certificate whose issuer is removed in the same
//! pull is not pinned.

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
    pub pinned_at: i64,
    /// Vouched for only by a Recovery Kit certificate (its own, or its issuer's): anyone with
    /// the vault key can make one, so it gets a new key only when the person ticks it
    /// (`recipients`), does not count toward the cap otherwise, and signs no removal that counts
    /// here. Cleared when it is `confirmed`, and for what it vouched for when its issuer is; never set on this device's
    /// own entry.
    pub kit_unconfirmed: bool,
    /// A person confirmed this device on a trusted device: this one (`confirm`), another (its
    /// signed `confirm_record` arrived), or the signer of a key change naming it. A confirmed
    /// device is a recipient even when the device that added it has left or was removed.
    pub confirmed: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConflictKind {
    /// The synced column holds a certificate other than the pinned one.
    OtherCertificate,
    /// A paired certificate uses a join ticket another device was pinned with.
    TicketReused,
    /// A Recovery Kit certificate for a device ID another certificate names as its issuer with
    /// another signing key (it would squat a device not yet pinned here).
    KitClaimsKnownDevice,
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
    /// Vouched for only by a Recovery Kit certificate and not ticked by the person (spec §9.2:
    /// "Poslat nový klíč i tomuto zařízení"); it must join again if left out.
    KitUnconfirmed,
    /// Paired or re-joined by a device that has since left or was removed, not confirmed and not
    /// ticked (spec §9.2: "spárováno zařízením X, které už není v synchronizaci"): a device whose
    /// key is out of the person's hands could date such a pairing back.
    IssuerGone,
}

/// The recipients of a new key and every other device with its reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recipients {
    /// Sealed to each entry's pinned `box_pub`; ascending device ID.
    pub included: Vec<TrustEntry>,
    /// Ascending device ID.
    pub excluded: Vec<(DeviceId, ExclusionReason)>,
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
    /// `TrustEntry::kit_unconfirmed`; written only when true.
    #[serde(
        rename = "kitUnconfirmed",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub kit_unconfirmed: bool,
    /// `TrustEntry::confirmed`; written only when true.
    #[serde(
        rename = "confirmed",
        default,
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub confirmed: bool,
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
pub(super) fn vault_id(conn: &Connection) -> Result<[u8; 16]> {
    let stored: Option<String> = conn
        .query_row("SELECT vault_id FROM sync_state WHERE id = 1", [], |r| {
            r.get(0)
        })
        .optional()?
        .flatten();
    let stored = stored.ok_or_else(|| refused("no vault is bound on this device"))?;
    Ok(VaultId::from_b64(&stored)?.0)
}

const ENTRY_COLUMNS: &str = "device, cert, state, state_record, state_by, state_at, \
    cut_off_epoch, pinned_at, kit_unconfirmed, confirmed";

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
        row.get(9)?,
    ))
}

/// An entry from its stored columns; the key columns are read back from the certificate, which
/// was verified when it was pinned.
fn parse_entry(raw: RawEntry) -> Result<TrustEntry> {
    let (device, cert, state, record, by, at, cut_off, pinned_at, kit_unconfirmed, confirmed) = raw;
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
        kit_unconfirmed,
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

fn entry_map(conn: &Connection) -> Result<BTreeMap<DeviceId, TrustEntry>> {
    Ok(entries(conn)?.into_iter().map(|e| (e.device, e)).collect())
}

/// Pins `cert` as `trusted` (the caller has verified it), unconfirmed when it is vouched for only
/// by a Recovery Kit certificate.
fn pin(conn: &Connection, cert: &Certificate, now: i64, kit_unconfirmed: bool) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_trust (device, cert, kind, issuer, sign_pub, box_pub, ticket_pub, \
           state, pinned_at, kit_unconfirmed) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'trusted', ?8, ?9)",
        params![
            cert.body.device.to_hex(),
            cert.to_b64(),
            cert.body.kind.as_u8(),
            cert.body.issuer_device.to_hex(),
            cert.body.sign_pub.to_vec(),
            cert.body.box_pub.to_vec(),
            cert.ticket.as_ref().map(|t| t.ticket_pub.to_vec()),
            now,
            kit_unconfirmed
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
        None => pin(conn, cert, now, false),
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

/// A `sync_devices` row's candidate columns.
struct DeviceRow {
    id: DeviceId,
    cert: Option<String>,
    left: Option<String>,
    removal: Option<String>,
    confirm: Option<String>,
}

fn device_rows(conn: &Connection) -> Result<Vec<DeviceRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, cert, left_record, removal_record, confirm_record FROM sync_devices \
         ORDER BY id",
    )?;
    let raws = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(raws
        .into_iter()
        .filter_map(|(id, cert, left, removal, confirm)| {
            // A row whose ID is not a device ID names nobody.
            DeviceId::from_hex(&id).ok().map(|id| DeviceRow {
                id,
                cert,
                left,
                removal,
                confirm,
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
/// fills the cut-off epoch of a `removed` entry that has none; a confirmation moves nothing.
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

/// Whether the issuer of a paired or re-join `cert` vouches for it: pinned with the signing key
/// the certificate names, and `speaks_at` both the certificate's `issued_at` and its ticket's (a
/// device that left or was removed vouches only for what it issued until then). `Some` carries
/// the issuer's `kit_unconfirmed`, which the certificate inherits.
fn vouch(store: &BTreeMap<DeviceId, TrustEntry>, cert: &Certificate) -> Option<bool> {
    store
        .get(&cert.body.issuer_device)
        .filter(|issuer| {
            issuer.sign_pub == cert.body.issuer_sign_pub
                && speaks_at(issuer, cert.body.issued_at)
                && cert
                    .ticket
                    .as_ref()
                    .is_none_or(|ticket| speaks_at(issuer, ticket.issued_at))
        })
        .map(|issuer| issuer.kit_unconfirmed)
}

/// Every signing key some paired or re-join certificate (in a row, verifying on its own, or
/// pinned) names for its issuer device.
fn issuer_keys(
    rows: &[DeviceRow],
    store: &BTreeMap<DeviceId, TrustEntry>,
    vault: &[u8; 16],
) -> BTreeMap<DeviceId, BTreeSet<[u8; 32]>> {
    let mut named: BTreeMap<DeviceId, BTreeSet<[u8; 32]>> = BTreeMap::new();
    let row_certs = rows.iter().filter_map(|row| row_cert(row, vault));
    let pinned = store.values().map(|e| e.cert.clone());
    for cert in row_certs.chain(pinned) {
        if matches!(cert.body.kind, CertKind::Paired | CertKind::Rejoin) {
            named
                .entry(cert.body.issuer_device)
                .or_default()
                .insert(cert.body.issuer_sign_pub);
        }
    }
    named
}

/// What a pass does with a certificate that verifies on its own and is not pinned yet.
enum Verdict {
    Pin {
        kit_unconfirmed: bool,
    },
    /// Not now; a later pass may pin it.
    Wait,
    Refuse(ConflictKind),
}

/// What a pass knows when it judges a certificate.
struct Seen<'a> {
    store: &'a BTreeMap<DeviceId, TrustEntry>,
    /// `issuer_keys`.
    issuer_keys: &'a BTreeMap<DeviceId, BTreeSet<[u8; 32]>>,
    /// Rows whose certificate verifies on its own and is not pinned.
    waiting: &'a BTreeSet<DeviceId>,
}

/// A Recovery Kit certificate is pinned unconfirmed, unless another certificate names its device
/// as an issuer with another signing key (spec §5.4 as amended: it would squat that device).
fn judge_kit(cert: &Certificate, seen: &Seen<'_>) -> Verdict {
    let squats = seen
        .issuer_keys
        .get(&cert.body.device)
        .is_some_and(|keys| keys.iter().any(|key| key != &cert.body.sign_pub));
    if squats {
        Verdict::Refuse(ConflictKind::KitClaimsKnownDevice)
    } else {
        Verdict::Pin {
            kit_unconfirmed: true,
        }
    }
}

/// Whether `signer` speaks for a record it signed at `at`: while it is `trusted`, and after it
/// left or was removed for what it signed until then (`at` no later than the `state_at` of the
/// record that changed its state). So a removal counts on every device whether it arrives before
/// or after the news that its signer left or was removed.
fn speaks_at(signer: &TrustEntry, at: i64) -> bool {
    match signer.state {
        TrustState::Trusted => true,
        TrustState::Left | TrustState::Removed => signer.state_at.is_some_and(|until| at <= until),
    }
}

/// `record`, a removal or a confirmation, verifies by a signer pinned in `store`, not
/// `kit_unconfirmed`, that `speaks_at` its time.
fn removal_counts(record: &Record, store: &BTreeMap<DeviceId, TrustEntry>) -> bool {
    store.get(&record.signer).is_some_and(|signer| {
        !signer.kit_unconfirmed
            && speaks_at(signer, record.at)
            && record.verify(&signer.sign_pub).is_ok()
    })
}

/// What the rows' records say, judged against the store before the pass.
struct Judged {
    /// Left and removal records to apply.
    moves: Vec<Record>,
    /// Records naming this device.
    about_me: Vec<Record>,
    /// Devices a counted confirmation names, not confirmed yet.
    confirms: Vec<DeviceId>,
}

/// The records of every row, judged against `store`.
fn judge_records(
    rows: &[DeviceRow],
    store: &BTreeMap<DeviceId, TrustEntry>,
    own: DeviceId,
    vault: &[u8; 16],
) -> Judged {
    let mut moves = Vec::new();
    let mut about_me = Vec::new();
    let mut confirms = Vec::new();
    for row in rows {
        if let Some(confirm) =
            row_record(row.confirm.as_deref(), RecordType::Confirmed, row.id, vault)
        {
            let pending = store.get(&row.id).is_some_and(|subject| !subject.confirmed);
            if pending && removal_counts(&confirm, store) {
                confirms.push(row.id);
            }
        }
        if let Some(left) = row_record(row.left.as_deref(), RecordType::Left, row.id, vault) {
            let verified = store
                .get(&row.id)
                .is_some_and(|subject| left.verify(&subject.sign_pub).is_ok());
            if verified {
                if row.id == own {
                    about_me.push(left);
                } else if store[&row.id].state == TrustState::Trusted {
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
    Judged {
        moves,
        about_me,
        confirms,
    }
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
/// ticket at most once. Returns whether anything was pinned.
fn pin_candidates(
    conn: &Connection,
    rows: &[DeviceRow],
    vault: &[u8; 16],
    now: i64,
    report: &mut RefreshReport,
    accept: impl Fn(&Certificate, &Seen<'_>) -> Verdict,
) -> Result<bool> {
    let store = entry_map(conn)?;
    let candidates: Vec<(DeviceId, Certificate)> = rows
        .iter()
        .filter_map(|row| row_cert(row, vault).map(|cert| (row.id, cert)))
        .collect();
    let waiting: BTreeSet<DeviceId> = candidates
        .iter()
        .map(|(id, _)| *id)
        .filter(|id| !store.contains_key(id))
        .collect();
    let issuer_keys = issuer_keys(rows, &store, vault);
    let seen = Seen {
        store: &store,
        issuer_keys: &issuer_keys,
        waiting: &waiting,
    };
    let mut tickets: HashSet<[u8; 32]> = HashSet::new();
    let mut pinned_any = false;
    for (id, cert) in candidates {
        if let Some(pinned) = store.get(&id) {
            if pinned.cert != cert {
                push_conflict(report, id, ConflictKind::OtherCertificate);
            }
            continue;
        }
        let kit_unconfirmed = match accept(&cert, &seen) {
            Verdict::Pin { kit_unconfirmed } => kit_unconfirmed,
            Verdict::Wait => continue,
            Verdict::Refuse(kind) => {
                push_conflict(report, id, kind);
                continue;
            }
        };
        if let Some(ticket) = &cert.ticket {
            if !tickets.insert(ticket.ticket_pub) || ticket_used(conn, &ticket.ticket_pub)? {
                push_conflict(report, id, ConflictKind::TicketReused);
                continue;
            }
        }
        pin(conn, &cert, now, kit_unconfirmed)?;
        report.pinned.push(id);
        pinned_any = true;
    }
    Ok(pinned_any)
}

/// Confirms every unconfirmed paired or re-join entry whose issuer now vouches for it confirmed
/// (its Kit-joined issuer was confirmed). Returns whether anything changed.
fn promote(conn: &Connection) -> Result<bool> {
    let store = entry_map(conn)?;
    let mut changed = false;
    for entry in store.values() {
        if entry.kit_unconfirmed
            && matches!(entry.kind, CertKind::Paired | CertKind::Rejoin)
            && vouch(&store, &entry.cert) == Some(false)
        {
            changed |= conn.execute(
                "UPDATE sync_trust SET kit_unconfirmed = 0 WHERE device = ?1",
                [entry.device.to_hex()],
            )? > 0;
        }
    }
    Ok(changed)
}

/// A person confirmed `device`: `confirmed` set and `kit_unconfirmed` cleared. Returns whether
/// anything changed.
fn mark_confirmed(conn: &Connection, device: DeviceId) -> Result<bool> {
    Ok(conn.execute(
        "UPDATE sync_trust SET kit_unconfirmed = 0, confirmed = 1 \
         WHERE device = ?1 AND (kit_unconfirmed = 1 OR confirmed = 0)",
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
            ("confirm_record", &row.confirm),
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
        let store = entry_map(conn)?;
        let Judged {
            moves,
            about_me,
            confirms,
        } = judge_records(&rows, &store, own, &vault);
        for record in about_me {
            if !report.about_me.contains(&record) {
                log::warn!("[SYNC] A verified record says this device left or was removed");
                report.about_me.push(record);
            }
        }
        let mut changed = false;
        for device in confirms {
            changed |= mark_confirmed(conn, device)?;
        }
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
        changed |= promote(conn)?;
        if !changed {
            return Ok(());
        }
    }
}

/// Brings the store up to date with `sync_devices` (spec §5.4): after every merge that touched it,
/// at unlock and after a key pickup. Pins paired and re-join certificates whose issuer vouches
/// (unconfirmed when the issuer is) and Recovery Kit certificates (unconfirmed; refused when they
/// would squat a device another certificate names), confirms what a newly confirmed issuer
/// vouches for, applies verified records, and repeats until nothing new verifies. One savepoint.
pub fn refresh(conn: &Connection, now: i64) -> Result<RefreshReport> {
    in_savepoint(conn, "sync_trust_refresh", || {
        let mut report = RefreshReport::default();
        settle(conn, now, &mut report, |cert, seen| match cert.body.kind {
            CertKind::Genesis => Verdict::Wait,
            CertKind::RecoveryKit => judge_kit(cert, seen),
            CertKind::Paired | CertKind::Rejoin => match vouch(seen.store, cert) {
                Some(kit_unconfirmed) => Verdict::Pin { kit_unconfirmed },
                None => Verdict::Wait,
            },
        })?;
        Ok(report)
    })
}

/// A Recovery Kit joiner's first store (spec §5.4, §13): trust on first use of every row whose
/// certificate verifies, genesis and unknown issuers included, then the records as `refresh`
/// applies them. Other Recovery Kit certificates are pinned unconfirmed as in `refresh`, and what
/// they vouch for inherits it, so a paired or re-join certificate whose issuer has a row still
/// waiting waits for it; one whose issuer has no verifiable row is trusted on first use.
pub fn tofu_from_rows(conn: &Connection, now: i64) -> Result<RefreshReport> {
    in_savepoint(conn, "sync_trust_tofu", || {
        let mut report = RefreshReport::default();
        settle(conn, now, &mut report, |cert, seen| match cert.body.kind {
            CertKind::Genesis => Verdict::Pin {
                kit_unconfirmed: false,
            },
            CertKind::RecoveryKit => judge_kit(cert, seen),
            CertKind::Paired | CertKind::Rejoin => {
                let issuer = cert.body.issuer_device;
                match vouch(seen.store, cert) {
                    Some(kit_unconfirmed) => Verdict::Pin { kit_unconfirmed },
                    None if seen.store.contains_key(&issuer) || seen.waiting.contains(&issuer) => {
                        Verdict::Wait
                    }
                    None => Verdict::Pin {
                        kit_unconfirmed: false,
                    },
                }
            }
        })?;
        Ok(report)
    })
}

/// This device's whole store, for a joiner it pairs.
pub fn export(conn: &Connection) -> Result<TrustExport> {
    Ok(TrustExport {
        entries: entries(conn)?
            .into_iter()
            .map(|e| ExportEntry {
                cert: e.cert.to_b64(),
                state: e.state,
                record: e.state_record.map(|r| r.to_b64()),
                cut_off_epoch: e.cut_off_epoch,
                kit_unconfirmed: e.kit_unconfirmed,
                confirmed: e.confirmed,
            })
            .collect(),
    })
}

/// Seeds an empty store from a holder's export, received over the authenticated pairing channel:
/// it must contain the `holder`'s own certificate in state `trusted`, and every entry must verify
/// (its certificate on its own and in this vault; a `left` record signed by its subject, a
/// `removed` record by another entry of the export; no record for a `trusted` entry). Anything
/// wrong refuses the whole export and writes nothing.
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
        Some((_, item)) if item.state == TrustState::Trusted && !item.kit_unconfirmed => {}
        _ => return Err(invalid()),
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
        checked.push((cert, item, record));
    }
    in_savepoint(conn, "sync_trust_import", || {
        let empty: bool =
            conn.query_row("SELECT COUNT(*) = 0 FROM sync_trust", [], |r| r.get(0))?;
        if !empty {
            return Err(refused("the trust store is not empty"));
        }
        for (cert, item, record) in &checked {
            pin(conn, cert, now, item.kit_unconfirmed)?;
            if item.confirmed {
                mark_confirmed(conn, cert.body.device)?;
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
        }
        Ok(())
    })
}

/// What an accepted key-change bundle teaches (spec §6.3): its signer's and its recipients'
/// certificates, endorsed by the signer, are pinned where this device has none (a pinned one
/// stays) and confirmed (`kit_unconfirmed` cleared: the signer's person ticked them), and its
/// removal records move their subjects to `removed`, cut off at `to_epoch`. The caller has
/// checked the bundle and its signer (pinned, trusted, not `kit_unconfirmed`). Every certificate
/// must verify and every removal record must count as `refresh` judges it once the bundle's
/// certificates are in (its signer pinned, not `kit_unconfirmed`, and `trusted` or left or
/// removed no earlier than the record), or nothing is written. A removal naming this device is
/// not applied but returned: the caller ends sync here (spec §7.4). One savepoint.
pub fn import_bundle(
    conn: &Connection,
    signer_cert: &Certificate,
    recipients: &[Certificate],
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
    in_savepoint(conn, "sync_trust_bundle", || {
        for cert in &certs {
            match entry(conn, cert.body.device)? {
                Some(pinned) => {
                    if &pinned.cert != *cert {
                        log::warn!(
                            "[SYNC] A key change names another certificate for {}; the pinned \
                             one stays",
                            short(cert.body.device)
                        );
                    } else {
                        mark_confirmed(conn, pinned.device)?;
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
                        pin(conn, cert, now, false)?;
                        mark_confirmed(conn, cert.body.device)?;
                    }
                }
            }
        }
        // Every record is judged against the store with the bundle's certificates in and before
        // any of its removals, as one `refresh` pass judges records.
        let store = entry_map(conn)?;
        let mut about_me = Vec::new();
        for record in removed_records {
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
                about_me.push(record.clone());
            }
        }
        for record in removed_records {
            if record.subject != own {
                apply_move(conn, record, Some(to_epoch))?;
            }
        }
        Ok(about_me)
    })
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
    let store = entry_map(conn)?;
    if !removal_counts(record, &store) {
        return Err(refused("the removal's signer is not trusted here"));
    }
    apply_move(conn, record, cut_off_epoch)
}

/// The person confirmed `devices` on this device (spec §9.1 "Potvrdit zařízení", and the ticks of
/// §9.2 once the key change committed): for each, this device signs a `confirmed` record, marks
/// the entry `confirmed` (clearing `kit_unconfirmed`) and writes the record into the device's
/// `sync_devices` row (write-once: an earlier confirmation stays), so every other device learns
/// it as `refresh` judges records. This device must be pinned, `trusted`, not `kit_unconfirmed`
/// and hold its signing key; every device must be pinned, `trusted` and not this one. Anything
/// wrong refuses the whole call and writes nothing. Returns the records it signed. The caller
/// checks the password (Task 16). One savepoint.
pub fn confirm(conn: &Connection, devices: &[DeviceId], now: i64) -> Result<Vec<Record>> {
    let own = this_device(conn)?;
    let vault = vault_id(conn)?;
    let store = entry_map(conn)?;
    let speaks = store
        .get(&own)
        .is_some_and(|me| me.state == TrustState::Trusted && !me.kit_unconfirmed);
    if !speaks {
        return Err(refused("this device cannot confirm devices"));
    }
    for device in devices {
        let ok = *device != own
            && store
                .get(device)
                .is_some_and(|e| e.state == TrustState::Trusted);
        if !ok {
            return Err(refused("only another trusted device can be confirmed"));
        }
    }
    let seed = super::own_seed(conn)?.ok_or_else(|| refused("this device has no signing key"))?;
    let epoch: u32 = conn
        .query_row("SELECT key_epoch FROM sync_state WHERE id = 1", [], |r| {
            r.get::<_, Option<i64>>(0)
        })?
        .map(u32::try_from)
        .transpose()
        .map_err(|_| corrupt())?
        .unwrap_or(0);
    in_savepoint(conn, "sync_trust_confirm", || {
        let mut records = Vec::with_capacity(devices.len());
        for device in devices {
            let record = Record::sign(
                RecordType::Confirmed,
                vault,
                *device,
                own,
                now,
                epoch,
                &seed,
            )?;
            mark_confirmed(conn, *device)?;
            conn.execute(
                "UPDATE sync_devices SET confirm_record = ?1 \
                 WHERE id = ?2 AND confirm_record IS NULL",
                params![record.to_b64(), device.to_hex()],
            )?;
            records.push(record);
        }
        Ok(records)
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

/// Who would receive a new key if `removing` were removed now (spec §5.4, §9.2): every `trusted`
/// entry, this device included, minus `removing`, minus superseded IDs that wrote nothing after
/// their successor appeared (O11), and, unless the person ticked them (`opted_in`, the opt-in
/// boxes of §9.2), minus `kit_unconfirmed` entries and minus unconfirmed paired or re-join
/// entries whose issuer has left or was removed (`IssuerGone`); this device is never left out
/// for either. Every other device of the list or the store comes with its reason. Refuses more
/// than 64 recipients (`validation.syncTooManyDevices`); what is left out does not count.
pub fn recipients(
    conn: &Connection,
    removing: &[DeviceId],
    opted_in: &[DeviceId],
) -> Result<Recipients> {
    let own = this_device(conn)?;
    let store = entry_map(conn)?;
    let mut rows: BTreeMap<DeviceId, bool> = BTreeMap::new();
    let mut stmt = conn.prepare("SELECT id, public_key IS NOT NULL FROM sync_devices")?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)))? {
        let (id, has_key) = row?;
        if let Ok(id) = DeviceId::from_hex(&id) {
            rows.insert(id, has_key);
        }
    }
    // The earliest successor of each superseded ID.
    let mut successor_at: BTreeMap<DeviceId, i64> = BTreeMap::new();
    for entry in store.values() {
        if entry.kind == CertKind::Rejoin {
            let at = successor_at.entry(entry.issuer).or_insert(i64::MAX);
            *at = (*at).min(entry.cert.body.issued_at);
        }
    }
    let clocks = newest_clock_secs(conn)?;
    let removing: BTreeSet<DeviceId> = removing.iter().copied().collect();
    let ticked: BTreeSet<DeviceId> = opted_in.iter().copied().collect();
    let issuer_gone = |entry: &TrustEntry| {
        matches!(entry.kind, CertKind::Paired | CertKind::Rejoin)
            && !entry.confirmed
            && store
                .get(&entry.issuer)
                .is_some_and(|issuer| issuer.state != TrustState::Trusted)
    };
    let devices: BTreeSet<DeviceId> = store.keys().chain(rows.keys()).copied().collect();
    let mut included = Vec::new();
    let mut excluded = Vec::new();
    for device in devices {
        let reason = match store.get(&device) {
            _ if removing.contains(&device) => Some(ExclusionReason::Removing),
            Some(entry) => match entry.state {
                TrustState::Left => Some(ExclusionReason::Left),
                TrustState::Removed => Some(ExclusionReason::Removed),
                TrustState::Trusted
                    if entry.kit_unconfirmed && device != own && !ticked.contains(&device) =>
                {
                    Some(ExclusionReason::KitUnconfirmed)
                }
                TrustState::Trusted
                    if issuer_gone(entry) && device != own && !ticked.contains(&device) =>
                {
                    Some(ExclusionReason::IssuerGone)
                }
                TrustState::Trusted => successor_at.get(&device).and_then(|since| {
                    let wrote_since = clocks.get(&device).is_some_and(|at| at > since);
                    (!wrote_since).then_some(ExclusionReason::SupersededQuiet)
                }),
            },
            None if rows.get(&device) == Some(&true) => Some(ExclusionReason::Unverified),
            None => Some(ExclusionReason::NoKey),
        };
        match reason {
            Some(reason) => excluded.push((device, reason)),
            None => included.push(store[&device].clone()),
        }
    }
    if included.len() > MAX_RECIPIENTS {
        return Err(AppError::Validation("validation.syncTooManyDevices".into()));
    }
    Ok(Recipients { included, excluded })
}
