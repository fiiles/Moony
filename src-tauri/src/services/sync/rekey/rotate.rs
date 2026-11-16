//! Removing devices and changing the key on this device (phase 5 spec §6.4, §6.6, §10).
//!
//! `plan` lists what a removal would do (who gets the new key, who does not and why, which devices
//! the removed ones added) and a consent hash over it; `remove` rebuilds the plan, refuses one
//! that changed (`validation.syncRemovalPlanChanged`) and carries it out under
//! `engine::exclusive`:
//!
//! - **List only:** a `removed` record per device, signed now at the current epoch, written into
//!   its row (captured, so it syncs) and moved into the trust store with no cut-off epoch. No
//!   relay call.
//! - **Key change:** a new key, the bundle (sealed to every recipient's pinned box key, a notice to
//!   every removed device and to every device that left or was removed without a cut-off), and
//!   the pending change (`sync_state.rotation_pending`) stored before anything is sent; then any
//!   pairing in progress is given up and the token rotates with the bundle. `200` commits in one
//!   savepoint (`commit`); a lost race (`403 token_read_only`), `409 too_many_rotations` and any
//!   other refusal wipe the pending change; no answer at all (the network, a timeout, a server
//!   error, a rate limit) keeps it.
//!
//! A pending change is resolved at every engine start and at the start of every round, before
//! anything else and with no person needed (the consent was given): the new token reads the vault
//! → the change landed, commit it with the head of its bundle; the new token is unknown (`401`) →
//! it did not land, send it again; a retired token then (`403`) → its own bundle on the server
//! means it landed after all (commit), else another device changed the key first. A change a
//! resolution ends without it happening leaves a notice (`rekey::RemovalNotDone`). The commit
//! queues the signed head of the change (`rekey::head`). No round pushes while a change is
//! pending. The pending JSON holds the new key: it is
//! written and read only here, in wiped buffers, and never logged.
//!
//! What the remover had not sent is sealed again under the new key by its next round
//! (`reseal`), once its pull has passed the rotation point; a crash after the commit leaves that
//! to the first round.

use std::fmt;
use std::path::Path;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use super::bundle::{self, BundleInput, Notice, OldKey, Reason, Recipient};
use super::pickup::{self, Pickup};
use super::{device_name, head, write_removal_not_done, RemovalNotDone};
use crate::error::{AppError, Result};
use crate::services::sync::capture::guard;
use crate::services::sync::engine::{self, ConnAccess};
use crate::services::sync::envelope::vault_token;
use crate::services::sync::error::{SyncError, SyncResult};
use crate::services::sync::hlc::DeviceId;
use crate::services::sync::identity::trust::{self, TrustEntry, TrustState};
use crate::services::sync::identity::{self, CertKind, Record, RecordType};
use crate::services::sync::in_savepoint;
use crate::services::sync::keys::{self, Credentials, SecretString, VaultAuth};
use crate::services::sync::relay::{Relay, RelayError, Rotated, MAX_REKEY_PAGE};
use crate::services::sync::state::{self, EpochRecord};

pub const PLAN_CHANGED: &str = "validation.syncRemovalPlanChanged";
pub const ROTATION_RACED: &str = "validation.syncRotationRaced";
pub const TOO_MANY_KEY_CHANGES: &str = "validation.syncTooManyKeyChanges";
pub const REMOVAL_UNSUPPORTED: &str = "validation.syncRemovalUnsupported";
pub const REMOVE_THIS_DEVICE: &str = "validation.syncRemoveThisDevice";
const CHANGES_PAUSED: &str = "validation.syncChangesPaused";
const NOT_SET_UP: &str = "validation.syncSetupIncomplete";
const IN_PROGRESS: &str = "validation.syncChangeInProgress";
const KEY_ROTATED: &str = "validation.syncKeyRotated";

/// What a removal does to the key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovalMode {
    /// "Jen odebrat ze seznamu": records only, the key stays.
    ListOnly,
    /// "Odebrat a vyměnit klíč", or "Vyměnit klíč synchronizace" with nobody removed.
    Rotate,
}

/// What a removal would do, as the person confirms it (spec §9.2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    pub mode: RemovalMode,
    /// This device's key epoch.
    pub epoch: u32,
    /// The devices removed, ascending.
    pub removing: Vec<DeviceId>,
    /// Who gets the new key (this device included), each sealed to its pinned box key; empty for
    /// a list-only removal.
    pub recipients: Vec<TrustEntry>,
    /// Every other device with its reason; empty for a list-only removal.
    pub excluded: Vec<(DeviceId, trust::ExclusionReason)>,
    /// Devices a removed device added (it issued their certificate) that are not removed: the
    /// person may tick them too ("Odebrat také"), which makes another plan.
    pub added_by_removing: Vec<DeviceId>,
    /// Ticked recipients the key change confirms (`trust::confirm`) when it commits.
    pub confirming: Vec<DeviceId>,
    /// Hex SHA-256 over the mode, the epoch, the removed devices and each recipient with its
    /// certificate's hash (the ticks show as recipients).
    pub consent: String,
}

/// What `remove` (and `resolve_pending`) did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// List-only: the records are written.
    Listed { removed: Vec<DeviceId> },
    /// The key changed to `epoch`.
    Changed { epoch: u32, removed: Vec<DeviceId> },
    /// The sync server did not answer: the change is pending and completes by itself.
    Pending,
    /// Another device changed the key first and had already removed every device asked for
    /// (`by` its name).
    AlreadyRemoved { by: String },
    /// Another device changed the key first (`by` its name, when this device picked its change
    /// up); the person checks the new list and confirms again (O12).
    Raced { by: Option<String> },
    /// Another device changed the key first and removed this one: sync is off here.
    RemovedHere,
}

/// The person's request: what they confirmed, with the plan's consent.
#[derive(Clone, Copy, Debug)]
pub struct Removal<'a> {
    pub mode: RemovalMode,
    pub removing: &'a [DeviceId],
    /// Devices ticked to get the key although a Recovery Kit vouches for them or their issuer is
    /// gone (spec §9.2, As built 33–34).
    pub opted_in: &'a [DeviceId],
    pub consent: &'a str,
}

fn validation(key: &str) -> AppError {
    AppError::Validation(key.to_string())
}

fn not_set_up() -> AppError {
    validation(NOT_SET_UP)
}

fn sorted(devices: &[DeviceId]) -> Vec<DeviceId> {
    let mut out = devices.to_vec();
    out.sort();
    out.dedup();
    out
}

/// Whether a key change of this device is pending (`sync_state.rotation_pending`).
pub fn pending(conn: &Connection) -> Result<bool> {
    let pending: Option<bool> = conn
        .query_row(
            "SELECT rotation_pending IS NOT NULL FROM sync_state WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(pending.unwrap_or(false))
}

/// The devices this device's pending key change removes (none when nothing is pending; a value
/// that does not read removes nobody here, `resolve_pending` reports it).
pub(crate) fn pending_removals(conn: &Connection) -> Result<Vec<DeviceId>> {
    Ok(load_pending(conn)
        .unwrap_or(None)
        .map(|change| change.removed_devices())
        .unwrap_or_default())
}

/// The plan for removing `removing` in `mode`, `opted_in` ticked (spec §6.4 steps 1–2). Refuses:
/// sync off (`syncSetupIncomplete`); capture broken (`syncChangesPaused`); a vault set up before
/// phase 5 or this device no longer trusted (`syncRemovalUnsupported`); a key change pending
/// (`syncChangeInProgress`); removing this device (`syncRemoveThisDevice`: that is turning sync
/// off); a list-only removal of nobody or a device the list does not know
/// (`syncRemovalPlanChanged`); more than 64 recipients (`syncTooManyDevices`).
pub fn plan(
    conn: &Connection,
    removing: &[DeviceId],
    mode: RemovalMode,
    opted_in: &[DeviceId],
) -> Result<Plan> {
    let me = check(conn)?;
    let removing = sorted(removing);
    let opted_in = sorted(opted_in);
    if removing.contains(&me) {
        return Err(validation(REMOVE_THIS_DEVICE));
    }
    if mode == RemovalMode::ListOnly && removing.is_empty() {
        return Err(validation(PLAN_CHANGED));
    }
    let store = trust::entries(conn)?;
    for device in &removing {
        let listed: bool = conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM sync_devices WHERE id = ?1)",
            [device.to_hex()],
            |r| r.get(0),
        )?;
        if !listed && !store.iter().any(|e| e.device == *device) {
            return Err(validation(PLAN_CHANGED));
        }
    }
    let epoch = keys::vault_keys(conn)?
        .ok_or_else(not_set_up)?
        .current_epoch();
    let added_by_removing = store
        .iter()
        .filter(|e| {
            matches!(e.kind, CertKind::Paired | CertKind::Rejoin)
                && e.state == TrustState::Trusted
                && removing.contains(&e.issuer)
                && !removing.contains(&e.device)
        })
        .map(|e| e.device)
        .collect();
    let (recipients, excluded) = match mode {
        RemovalMode::ListOnly => (Vec::new(), Vec::new()),
        RemovalMode::Rotate => {
            let r = trust::recipients(conn, &removing, &opted_in)?;
            (r.included, r.excluded)
        }
    };
    let confirming = recipients
        .iter()
        .map(|e| e.device)
        .filter(|d| *d != me && opted_in.contains(d))
        .collect();
    let mut plan = Plan {
        mode,
        epoch,
        removing,
        recipients,
        excluded,
        added_by_removing,
        confirming,
        consent: String::new(),
    };
    plan.consent = consent(&plan);
    Ok(plan)
}

/// The preconditions of every removal; this device's ID.
fn check(conn: &Connection) -> Result<DeviceId> {
    let sync = state::load(conn)?
        .filter(|s| s.enabled)
        .ok_or_else(not_set_up)?;
    let marks = state::marks(conn)?.ok_or_else(not_set_up)?;
    if marks.capture_broken_at.is_some() || guard::changes_paused(conn) {
        return Err(validation(CHANGES_PAUSED));
    }
    let own = trust::entry(conn, sync.device)?;
    if !own.is_some_and(|e| e.state == TrustState::Trusted) {
        return Err(validation(REMOVAL_UNSUPPORTED));
    }
    if identity::own_seed(conn)?.is_none() {
        return Err(validation(REMOVAL_UNSUPPORTED));
    }
    if pending(conn)? {
        return Err(validation(IN_PROGRESS));
    }
    Ok(sync.device)
}

/// Hex SHA-256 over a canonical encoding of what the person confirms.
pub fn consent(plan: &Plan) -> String {
    let mut hash = Sha256::new();
    hash.update(b"moony removal consent v1\0");
    hash.update([match plan.mode {
        RemovalMode::ListOnly => 1u8,
        RemovalMode::Rotate => 2u8,
    }]);
    hash.update(plan.epoch.to_be_bytes());
    hash.update((plan.removing.len() as u64).to_be_bytes());
    for device in &plan.removing {
        hash.update(device.0);
    }
    hash.update((plan.recipients.len() as u64).to_be_bytes());
    for entry in &plan.recipients {
        hash.update(entry.device.0);
        hash.update(entry.cert.hash());
    }
    hash.update((plan.confirming.len() as u64).to_be_bytes());
    for device in &plan.confirming {
        hash.update(device.0);
    }
    hash.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// A key change signed and sealed, not yet committed. Never printed (it holds the new key).
struct PendingChange {
    from: u32,
    to: u32,
    key: Zeroizing<[u8; 32]>,
    bundle: Vec<u8>,
    removed: Vec<Record>,
    confirm: Vec<DeviceId>,
}

impl fmt::Debug for PendingChange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingChange")
            .field("from", &self.from)
            .field("to", &self.to)
            .field("key", &"<redacted>")
            .finish_non_exhaustive()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PendingIn {
    v: u32,
    from: u32,
    to: u32,
    key: SecretString,
    bundle: String,
    removed: Vec<String>,
    confirm: Vec<String>,
}

const PENDING_VERSION: u32 = 1;

impl PendingChange {
    /// `{"v":1,"from":n,"to":n,"key":"<b64url>","bundle":"<b64url>","removed":["<record>"],
    /// "confirm":["<hex>"]}`, built in one wiped buffer reserved up front.
    fn to_json(&self) -> Zeroizing<String> {
        let mut json = Zeroizing::new(String::with_capacity(
            160 + self.bundle.len().div_ceil(3) * 4
                + 160 * self.removed.len()
                + 40 * self.confirm.len(),
        ));
        json.push_str(&format!(
            "{{\"v\":{PENDING_VERSION},\"from\":{},\"to\":{},\"key\":\"",
            self.from, self.to
        ));
        URL_SAFE_NO_PAD.encode_string(self.key.as_slice(), &mut json);
        json.push_str("\",\"bundle\":\"");
        URL_SAFE_NO_PAD.encode_string(&self.bundle, &mut json);
        json.push_str("\",\"removed\":[");
        for (i, record) in self.removed.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push('"');
            json.push_str(&record.to_b64());
            json.push('"');
        }
        json.push_str("],\"confirm\":[");
        for (i, device) in self.confirm.iter().enumerate() {
            if i > 0 {
                json.push(',');
            }
            json.push('"');
            json.push_str(&device.to_hex());
            json.push('"');
        }
        json.push_str("]}");
        json
    }

    fn from_json(json: &str) -> Result<Self> {
        let corrupt = || AppError::Internal("sync_state.rotation_pending is not valid".into());
        // The serde error may quote the value, so it is dropped.
        let raw: PendingIn = serde_json::from_str(json).map_err(|_| corrupt())?;
        if raw.v != PENDING_VERSION || raw.from.checked_add(1) != Some(raw.to) {
            return Err(corrupt());
        }
        let decoded = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(raw.key.0.as_bytes())
                .map_err(|_| corrupt())?,
        );
        if decoded.len() != 32 {
            return Err(corrupt());
        }
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(&decoded);
        Ok(PendingChange {
            from: raw.from,
            to: raw.to,
            key,
            bundle: URL_SAFE_NO_PAD
                .decode(raw.bundle.as_bytes())
                .map_err(|_| corrupt())?,
            removed: raw
                .removed
                .iter()
                .map(|r| Record::from_b64(r))
                .collect::<Result<_>>()
                .map_err(|_| corrupt())?,
            confirm: raw
                .confirm
                .iter()
                .map(|d| DeviceId::from_hex(d))
                .collect::<Result<_>>()
                .map_err(|_| corrupt())?,
        })
    }

    fn removed_devices(&self) -> Vec<DeviceId> {
        self.removed.iter().map(|r| r.subject).collect()
    }

    /// The token the new key gives the vault.
    fn new_auth(&self, creds: &Credentials) -> VaultAuth {
        VaultAuth {
            vault_id: creds.vault_id.to_b64(),
            token: Zeroizing::new(vault_token(&self.key, &creds.vault_id.0)),
        }
    }
}

fn store_pending(conn: &Connection, change: &PendingChange) -> Result<()> {
    let json = change.to_json();
    let stored = conn.execute(
        "UPDATE sync_state SET rotation_pending = ?1 WHERE id = 1 AND rotation_pending IS NULL",
        [json.as_str()],
    )?;
    if stored != 1 {
        return Err(validation(IN_PROGRESS));
    }
    Ok(())
}

fn load_pending(conn: &Connection) -> Result<Option<PendingChange>> {
    let stored: Option<Option<String>> = conn
        .query_row(
            "SELECT rotation_pending FROM sync_state WHERE id = 1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    stored
        .flatten()
        .map(|json| PendingChange::from_json(&Zeroizing::new(json)))
        .transpose()
}

fn wipe_pending(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET rotation_pending = NULL WHERE id = 1",
        [],
    )?;
    Ok(())
}

/// Carries out the removal the person confirmed (spec §6.4), under `engine::exclusive` (no round
/// runs). The command checks the password first. For a key change, a vault whose key changed since
/// this device last looked (`rotation` above `rekey_seen`) is picked up first, so the plan is rebuilt on the newest
/// key and list; then a plan whose consent differs is refused (`validation.syncRemovalPlanChanged`,
/// nothing written). `forget_pairing` gives up any pairing in progress (a holder must not hand out
/// the old key mid-change); it is called once the pending change is stored, before the token
/// rotates. `data_dir` is the binding file's folder, for a race that turns out to have removed
/// this device. A lost race answers `validation.syncRotationRaced` with the other device's name,
/// or `Outcome::AlreadyRemoved` when it removed every device asked for (O12).
pub async fn remove<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    request: Removal<'_>,
    data_dir: &Path,
    forget_pairing: &(dyn Fn() + Sync),
    now: i64,
) -> SyncResult<Outcome> {
    let _no_round = engine::exclusive(db).await;
    let (creds, seen) = db.with_conn(|conn| {
        check(conn)?;
        Ok((
            keys::credentials(conn)?.ok_or_else(not_set_up)?,
            state::rekey_seen(conn)?,
        ))
    })?;
    // A key change needs this device's epoch to be the vault's (§6.4 step 1); a list-only
    // removal makes no relay call.
    let behind = match request.mode {
        RemovalMode::ListOnly => false,
        RemovalMode::Rotate => relay.vault_info(&creds.auth()).await?.rotation > seen,
    };
    if behind {
        match pickup::pick_up(relay, db, Some(data_dir), now).await? {
            Pickup::Nothing | Pickup::Adopted { .. } => {}
            Pickup::RemovedHere { .. } => return Ok(Outcome::RemovedHere),
            Pickup::NotGiven | Pickup::Unverified => {
                return Err(SyncError::Local(validation(KEY_ROTATED)))
            }
        }
    }
    let prepared = db.with_conn(|conn| {
        let plan = plan(conn, request.removing, request.mode, request.opted_in)?;
        if plan.consent != request.consent {
            return Err(validation(PLAN_CHANGED));
        }
        match plan.mode {
            RemovalMode::ListOnly => {
                list_only(conn, &plan, now)?;
                Ok(Err(plan.removing))
            }
            RemovalMode::Rotate => {
                let creds = keys::credentials(conn)?.ok_or_else(not_set_up)?;
                Ok(Ok((prepare(conn, &creds, &plan, now)?, creds)))
            }
        }
    })?;
    let (change, creds) = match prepared {
        Err(removed) => return Ok(Outcome::Listed { removed }),
        Ok(prepared) => prepared,
    };
    forget_pairing();
    match send(relay, db, &creds, &change, Some(data_dir), false, now).await? {
        Sent::Done(Outcome::Raced { by: Some(name) }) => Err(SyncError::Local(
            AppError::validation_with(ROTATION_RACED, serde_json::json!({ "name": name })),
        )),
        Sent::Done(Outcome::Raced { by: None }) => Err(SyncError::Local(validation(KEY_ROTATED))),
        Sent::Done(outcome) => Ok(outcome),
        Sent::Pending(error) => {
            log::info!("[SYNC] The key change waits for the sync server: {error}");
            Ok(Outcome::Pending)
        }
    }
}

/// List-only (spec §6.4 step 3), in one savepoint: a `removed` record per device at the current
/// epoch, written into its row where it has none (captured) and applied to its pinned entry with
/// no cut-off.
fn list_only(conn: &Connection, plan: &Plan, now: i64) -> Result<()> {
    let me = check(conn)?;
    let seed = identity::own_seed(conn)?.ok_or_else(|| validation(REMOVAL_UNSUPPORTED))?;
    let vault = keys::credentials(conn)?.ok_or_else(not_set_up)?.vault_id.0;
    in_savepoint(conn, "sync_remove_list_only", || {
        for device in &plan.removing {
            let record = Record::sign(
                RecordType::Removed,
                vault,
                *device,
                me,
                now,
                plan.epoch,
                &seed,
            )?;
            write_removal(conn, &record)?;
            trust::mark_removed(conn, &record, None)?;
        }
        Ok(())
    })
}

/// Writes `record` into its subject's row (write-once: an earlier record stays) and marks the row
/// removed; captured, so it syncs.
fn write_removal(conn: &Connection, record: &Record) -> Result<()> {
    conn.execute(
        "UPDATE sync_devices SET removal_record = COALESCE(removal_record, ?1), removed = 1 \
         WHERE id = ?2",
        params![record.to_b64(), record.subject.to_hex()],
    )?;
    Ok(())
}

/// Spec §6.4 step 4 up to the call: the new key, the removal records, the bundle and the pending
/// change, stored in one statement.
fn prepare(conn: &Connection, creds: &Credentials, plan: &Plan, now: i64) -> Result<PendingChange> {
    let me = check(conn)?;
    let seed = identity::own_seed(conn)?.ok_or_else(|| validation(REMOVAL_UNSUPPORTED))?;
    let own = trust::entry(conn, me)?.ok_or_else(|| validation(REMOVAL_UNSUPPORTED))?;
    let from = creds.keys.current_epoch();
    let to = from
        .checked_add(1)
        .ok_or_else(|| AppError::Internal("sync: the key epoch is at its last value".into()))?;
    let vault = creds.vault_id.0;
    let mut key = Zeroizing::new([0u8; 32]);
    keys::os_random(&mut key[..])?;
    let removed = plan
        .removing
        .iter()
        .map(|device| Record::sign(RecordType::Removed, vault, *device, me, now, from, &seed))
        .collect::<Result<Vec<_>>>()?;
    let recipients: Vec<Recipient> = plan
        .recipients
        .iter()
        .map(|e| Recipient {
            box_pub: e.box_pub,
            cert: e.cert.clone(),
        })
        .collect();
    // A notice to each removed device this one has pinned, then to every device that left or
    // was removed from the list and still knows the key (harmless to one that left: its box
    // secret went with its sync state).
    let store = trust::entries(conn)?;
    let mut notices: Vec<Notice> = removed
        .iter()
        .filter_map(|record| {
            store
                .iter()
                .find(|e| e.device == record.subject)
                .map(|e| Notice {
                    box_pub: e.box_pub,
                    record: record.clone(),
                })
        })
        .collect();
    notices.extend(
        store
            .iter()
            .filter(|e| {
                matches!(e.state, TrustState::Left | TrustState::Removed)
                    && e.cut_off_epoch.is_none()
                    && !plan.removing.contains(&e.device)
            })
            .filter_map(|e| {
                e.state_record.as_ref().map(|record| Notice {
                    box_pub: e.box_pub,
                    record: record.clone(),
                })
            }),
    );
    notices.truncate(bundle::MAX_NOTICE_ENTRIES);
    // Only heads this device verified travel (a provisional one is the server's word).
    let heads = head::verified_epochs(conn)?;
    let older: Vec<OldKey> = creds
        .keys
        .epochs()
        .map(|(epoch, key)| OldKey {
            epoch,
            key,
            head: heads.get(&epoch).copied().or((epoch == 0).then_some(0)),
        })
        .collect();
    let bytes = bundle::build(
        &BundleInput {
            vault_id: vault,
            from_epoch: from,
            to_epoch: to,
            new_key: &key,
            recipients: &recipients,
            notices: &notices,
            signer: &seed,
            signer_cert: &own.cert,
            at: now,
            reason: if removed.is_empty() {
                Reason::Rotate
            } else {
                Reason::Remove
            },
            removed: &removed,
            keys: &older,
        },
        &mut bundle::os_fill,
    )?;
    let change = PendingChange {
        from,
        to,
        key,
        bundle: bytes,
        removed,
        confirm: plan.confirming.clone(),
    };
    store_pending(conn, &change)?;
    Ok(change)
}

/// How sending a pending change ended.
enum Sent {
    Done(Outcome),
    /// No answer: the change stays pending (the error, for the caller to show or log).
    Pending(SyncError),
}

/// The token call (spec §6.4 step 4) and what its answer does (step 5). `background`: a
/// resolution no person is waiting for (`resolve_pending`), which leaves a notice when the change
/// the person confirmed does not happen (`rekey::RemovalNotDone`).
///
/// A retired token (`403`) is first checked against the bundles on the server: one with this
/// change's bytes means it landed after all (an answer lost, then a `401` for the new token that
/// was wrong), and it commits as this device's own. Otherwise another device changed the key
/// first: its change is picked up while this one is still stored (so a signer it removes is
/// refused), and only then is this one wiped, so a failure in between keeps it for the next
/// resolution.
async fn send<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    change: &PendingChange,
    data_dir: Option<&Path>,
    background: bool,
    now: i64,
) -> SyncResult<Sent> {
    let new_token = change.new_auth(creds).token;
    match relay
        .rotate_token(&creds.auth(), &new_token, &change.bundle)
        .await
    {
        Ok(rotated) => {
            let epoch = db.with_conn(|conn| commit(conn, change, rotated, now))?;
            log::info!("[SYNC] The sync key changed to epoch {epoch}");
            Ok(Sent::Done(Outcome::Changed {
                epoch,
                removed: change.removed_devices(),
            }))
        }
        Err(RelayError::TokenReadOnly) => {
            let seen = db.with_conn(state::rekey_seen)?;
            if let Some(rotated) = own_rotation(relay, &creds.auth(), seen, &change.bundle).await? {
                let epoch = db.with_conn(|conn| commit(conn, change, rotated, now))?;
                log::info!("[SYNC] A key change of this device had landed: the key is at {epoch}");
                return Ok(Sent::Done(Outcome::Changed {
                    epoch,
                    removed: change.removed_devices(),
                }));
            }
            log::info!("[SYNC] Another device changed the sync key first");
            let outcome = raced(relay, db, change, data_dir, now).await?;
            if outcome != Outcome::RemovedHere {
                let by = match &outcome {
                    Outcome::Raced { by } => by.clone(),
                    Outcome::AlreadyRemoved { by } => Some(by.clone()),
                    _ => None,
                };
                db.with_conn(|conn| wipe_not_done(conn, change, background, by))?;
            }
            Ok(Sent::Done(outcome))
        }
        Err(RelayError::TooManyRotations) => {
            db.with_conn(|conn| wipe_not_done(conn, change, background, None))?;
            Err(SyncError::Local(validation(TOO_MANY_KEY_CHANGES)))
        }
        Err(RelayError::BadRequest(code)) => {
            db.with_conn(|conn| wipe_not_done(conn, change, background, None))?;
            Err(SyncError::Local(AppError::Internal(format!(
                "sync: the sync server refused a key change: {code}"
            ))))
        }
        Err(
            error @ (RelayError::Network(_)
            | RelayError::Server(_)
            | RelayError::Protocol(_)
            | RelayError::RateLimited
            | RelayError::InsufficientStorage),
        ) => Ok(Sent::Pending(error.into())),
        Err(error) => {
            db.with_conn(|conn| wipe_not_done(conn, change, background, None))?;
            Err(error.into())
        }
    }
}

/// Wipes the pending change, and in the background leaves the notice that it was not done: `by`
/// changed the key first (when known), and whether every device it meant to remove is removed
/// here anyway. In one hold, so the notice is there whenever the change is gone.
fn wipe_not_done(
    conn: &Connection,
    change: &PendingChange,
    background: bool,
    by: Option<String>,
) -> Result<()> {
    wipe_pending(conn)?;
    if !background {
        return Ok(());
    }
    let devices = change
        .removed
        .iter()
        .map(|record| device_name(conn, record.subject))
        .collect::<Result<Vec<_>>>()?;
    let already_removed = !change.removed.is_empty()
        && change.removed.iter().all(|record| {
            trust::entry(conn, record.subject)
                .ok()
                .flatten()
                .is_some_and(|e| e.state == TrustState::Removed)
        });
    log::warn!("[SYNC] A confirmed removal or key change was not done");
    write_removal_not_done(
        conn,
        &RemovalNotDone {
            by,
            devices,
            already_removed,
        },
    )
}

/// A `200` (spec §6.4 step 5), in one savepoint: the new key current, its epoch's start, the
/// removal records written to the rows and applied to the store (cut off at the new epoch, as is
/// every left or list-removed device that still knew the key), the ticked devices confirmed,
/// the pending change cleared, `rekey_seen` at this rotation and a snapshot under the new key due.
fn commit(conn: &Connection, change: &PendingChange, rotated: Rotated, now: i64) -> Result<u32> {
    let creds = keys::credentials(conn)?.ok_or_else(not_set_up)?;
    let me = state::load(conn)?.ok_or_else(not_set_up)?.device;
    if creds.keys.current_epoch() != change.from {
        return Err(AppError::Internal(
            "sync: a pending key change does not follow this device's epoch".into(),
        ));
    }
    let next = creds.keys.with_next(Zeroizing::new(*change.key))?;
    in_savepoint(conn, "sync_commit_key_change", || {
        keys::store_key_change(conn, &next)?;
        if state::record_epoch(conn, change.to, rotated.head, Some(me), now)?
            == EpochRecord::Differs
        {
            return Err(AppError::Internal(
                "sync: the new epoch is recorded with another start".into(),
            ));
        }
        // The first envelope under the new key: the signed head every other device checks the
        // server's word against (`rekey::head`).
        head::queue_signed_head(conn, me, change.from, change.to, rotated)?;
        for record in &change.removed {
            write_removal(conn, record)?;
            trust::mark_removed(conn, record, Some(change.to))?;
        }
        trust::cut_off_all_pending(conn, change.to)?;
        // Ticked devices still trusted here (one that left meanwhile is not confirmed).
        let confirm: Vec<DeviceId> = change
            .confirm
            .iter()
            .copied()
            .filter(|device| {
                trust::entry(conn, *device)
                    .ok()
                    .flatten()
                    .is_some_and(|e| e.state == TrustState::Trusted)
            })
            .collect();
        if !confirm.is_empty() {
            trust::confirm(conn, &confirm, now)?;
        }
        conn.execute(
            "UPDATE sync_state SET rotation_pending = NULL, \
               rekey_seen = MAX(rekey_seen, ?1), snapshot_epoch_due = ?2 WHERE id = 1",
            params![rotated.rotation, change.to],
        )?;
        Ok(change.to)
    })
}

/// A lost race (`403 token_read_only`, spec §10.1): this device picks the other change up and
/// tells whether it already removed every device this one meant to.
async fn raced<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    change: &PendingChange,
    data_dir: Option<&Path>,
    now: i64,
) -> SyncResult<Outcome> {
    match pickup::pick_up(relay, db, data_dir, now).await? {
        Pickup::RemovedHere { .. } => Ok(Outcome::RemovedHere),
        Pickup::Adopted { by, .. } => Ok(db.with_conn(|conn| {
            let by = device_name(conn, by)?;
            let all_removed = change.removed.iter().all(|record| {
                trust::entry(conn, record.subject)
                    .ok()
                    .flatten()
                    .is_some_and(|e| e.state == TrustState::Removed)
            });
            Ok(if all_removed && !change.removed.is_empty() {
                Outcome::AlreadyRemoved { by }
            } else {
                Outcome::Raced { by: Some(by) }
            })
        })?),
        Pickup::Nothing | Pickup::NotGiven | Pickup::Unverified => Ok(Outcome::Raced { by: None }),
    }
}

/// Resolves this device's pending key change, if any (spec §6.4 step 6), with no person needed:
/// the new token reads the vault → it landed, committed with the head of its bundle; the new token
/// is unknown (`401`) → sent again, with `send`'s outcomes. `Ok(None)` when nothing was pending.
/// No answer leaves it pending and returns the error, so the caller pushes nothing.
pub async fn resolve_pending<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    data_dir: Option<&Path>,
    now: i64,
) -> SyncResult<Option<Outcome>> {
    let loaded = db.with_conn(|conn| {
        let Some(change) = load_pending(conn)? else {
            return Ok(None);
        };
        let creds = keys::credentials(conn)?.ok_or_else(not_set_up)?;
        if creds.keys.current_epoch() != change.from {
            // The key moved on without it (a commit clears it in the same savepoint): another
            // device's change was picked up first. Nothing to send.
            log::warn!("[SYNC] A pending key change no longer fits this device's key: dropped");
            wipe_not_done(conn, &change, true, None)?;
            return Ok(None);
        }
        Ok(Some((change, creds, state::rekey_seen(conn)?)))
    })?;
    let Some((change, creds, seen)) = loaded else {
        return Ok(None);
    };
    let new_auth = change.new_auth(&creds);
    match relay.vault_info(&new_auth).await {
        Ok(_) => {
            let rotated = own_rotation(relay, &new_auth, seen, &change.bundle)
                .await?
                .ok_or_else(|| {
                    SyncError::from(RelayError::Protocol(
                        "the new token reads the vault but its key change is not listed".into(),
                    ))
                })?;
            let epoch = db.with_conn(|conn| commit(conn, &change, rotated, now))?;
            log::info!("[SYNC] A pending key change had landed: the key is at epoch {epoch}");
            Ok(Some(Outcome::Changed {
                epoch,
                removed: change.removed_devices(),
            }))
        }
        Err(RelayError::Unauthorized) => {
            match send(relay, db, &creds, &change, data_dir, true, now).await? {
                Sent::Done(outcome) => Ok(Some(outcome)),
                Sent::Pending(error) => Err(error),
            }
        }
        Err(error) => Err(error.into()),
    }
}

/// The rotation and head of the bundle this device stored, if the server lists it (read with
/// `auth`, a token that reads the vault). Rotations must go up from page to page, and the walk
/// takes at most `pickup::MAX_BUNDLES_PER_WALK` bundles: a server paging for ever is refused.
async fn own_rotation<R: Relay>(
    relay: &R,
    auth: &VaultAuth,
    seen: u32,
    bundle: &[u8],
) -> SyncResult<Option<Rotated>> {
    let mut after = seen;
    let mut walked = 0usize;
    loop {
        let page = relay.rekeys(auth, after, MAX_REKEY_PAGE).await?;
        for rekey in &page.rekeys {
            walked += 1;
            if rekey.rotation <= after || walked > pickup::MAX_BUNDLES_PER_WALK {
                return Err(SyncError::from(RelayError::Protocol(
                    "the list of key changes does not end".into(),
                )));
            }
            after = rekey.rotation;
            if rekey.bundle == bundle {
                return Ok(Some(Rotated {
                    rotation: rekey.rotation,
                    head: rekey.head,
                }));
            }
        }
        if !page.more || page.rekeys.is_empty() {
            return Ok(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::identity::new_sign_seed;

    fn change() -> PendingChange {
        let seed = new_sign_seed().expect("seed");
        let record = Record::sign(
            RecordType::Removed,
            [0x22; 16],
            DeviceId([3; 16]),
            DeviceId([1; 16]),
            1_700_000_000,
            4,
            &seed,
        )
        .expect("record");
        PendingChange {
            from: 4,
            to: 5,
            key: Zeroizing::new([0x5a; 32]),
            bundle: vec![1, 2, 3, 250],
            removed: vec![record],
            confirm: vec![DeviceId([7; 16])],
        }
    }

    #[test]
    fn the_pending_change_round_trips_and_is_never_printed() {
        let change = change();
        let json = change.to_json();

        let back = PendingChange::from_json(&json).expect("parses");

        assert_eq!((back.from, back.to), (4, 5));
        assert_eq!(*back.key, [0x5a; 32]);
        assert_eq!(back.bundle, change.bundle);
        assert_eq!(back.removed, change.removed);
        assert_eq!(back.confirm, change.confirm);
        let printed = format!("{change:?}");
        assert!(printed.contains("<redacted>"), "{printed}");
        assert!(!printed.contains("5a5a"), "{printed}");
        assert!(!printed.contains("90"), "{printed}");
    }

    #[test]
    fn a_pending_change_that_does_not_read_is_refused() {
        let json = change().to_json();
        for broken in [
            json.replace("\"v\":1", "\"v\":2"),
            json.replace("\"to\":5", "\"to\":6"),
            json.replace("]}", "],\"extra\":1}"),
            json.replace("\"key\":\"", "\"key\":\"AA"),
        ] {
            assert!(PendingChange::from_json(&broken).is_err(), "{broken}");
        }
    }
}
