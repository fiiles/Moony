//! This device's row in `sync_devices` (spec §11.4): its name, platform, app version and the
//! public half of its X25519 key pair, which the merge keeps write-once. `id` is the device ID as
//! 32 lowercase hex characters, `public_key` base64url.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::collections::{BTreeMap, HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use super::hlc::{unpack, DeviceId};
use super::identity::trust::{self, TrustEntry, TrustState};
use super::identity::{CertKind, Certificate, Record, RecordType};
use super::rekey::{self, rotate};
use super::state;
use crate::error::{AppError, Result};
use crate::models::{
    SyncCheckReason, SyncDevice, SyncDeviceStatus, SyncFormerReason, SyncJoinedVia,
};

const MAX_NAME_CHARS: usize = 60;

/// Each device's newest clock stamp, read along `idx_sync_clock_device_hlc`.
const NEWEST_CLOCKS: &str = "SELECT device, MAX(hlc) FROM sync_clock GROUP BY device";

/// A raw device ID as `sync_devices.id` spells it: lowercase hex.
fn hex_of(device: &[u8]) -> String {
    device.iter().map(|b| format!("{b:02x}")).collect()
}

/// The platform this build runs on, as `sync_devices.platform` records it.
pub fn platform() -> &'static str {
    if cfg!(target_os = "ios") {
        "ios"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "linux"
    }
}

/// A device name has 1 to 60 characters once trimmed.
pub fn validate_device_name(name: &str) -> Result<()> {
    let chars = name.trim().chars().count();
    if (1..=MAX_NAME_CHARS).contains(&chars) {
        Ok(())
    } else {
        Err(AppError::Validation(
            "validation.syncDeviceNameInvalid".into(),
        ))
    }
}

/// Writes this device's row: created with `public_key`, its certificate (phase 5; `None` in a
/// vault without certificates) and `joined_at = now` the first time, afterwards the (trimmed)
/// name, platform and app version change, and the public key and the certificate only fill an
/// empty column: the merge's write-once rule (NULL may take a value once, a value never changes).
pub fn record_this_device(
    conn: &Connection,
    name: &str,
    public_key: &[u8; 32],
    cert: Option<&Certificate>,
    now: i64,
) -> Result<()> {
    validate_device_name(name)?;
    let device = state::load(conn)?
        .ok_or_else(|| AppError::Internal("sync: sync is not set up on this device".into()))?
        .device;
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at, cert) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, platform = excluded.platform, \
           app_version = excluded.app_version, \
           public_key = COALESCE(sync_devices.public_key, excluded.public_key), \
           cert = COALESCE(sync_devices.cert, excluded.cert)",
        params![
            device.to_hex(),
            URL_SAFE_NO_PAD.encode(public_key),
            name.trim(),
            platform(),
            env!("CARGO_PKG_VERSION"),
            now,
            cert.map(Certificate::to_b64)
        ],
    )?;
    Ok(())
}

/// `(name, platform)` of this device's row, or `None` before it is recorded.
pub fn this_device(conn: &Connection) -> Result<Option<(String, String)>> {
    match state::load(conn)? {
        Some(sync) => device_row(conn, sync.device),
        None => Ok(None),
    }
}

/// `joined_at` of this device's own row (unix seconds): when sync was turned on here, written once
/// by `record_this_device`. `None` without a `sync_state` row or before the row is recorded.
pub fn this_device_joined_at(conn: &Connection) -> Result<Option<i64>> {
    let Some(sync) = state::load(conn)? else {
        return Ok(None);
    };
    Ok(conn
        .query_row(
            "SELECT joined_at FROM sync_devices WHERE id = ?1",
            [sync.device.to_hex()],
            |r| r.get(0),
        )
        .optional()?)
}

/// `(name, platform)` of `device`'s row.
pub fn device_row(conn: &Connection, device: DeviceId) -> Result<Option<(String, String)>> {
    Ok(conn
        .query_row(
            "SELECT name, platform FROM sync_devices WHERE id = ?1",
            [device.to_hex()],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?)
}

/// How many devices the list holds (rows not removed): a plain count, cheap enough to ask every few
/// seconds, where `list_devices` reads the clock index too.
pub fn count_devices(conn: &Connection) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sync_devices WHERE removed = 0",
        [],
        |r| r.get(0),
    )?;
    Ok(usize::try_from(n).unwrap_or(0))
}

/// Rows of the list that left or were removed are listed this long after their state time
/// (revision §4.1); older ones stay in the database and the trust store.
pub const FORMER_LISTED_SECS: i64 = 30 * 86_400;

/// An active other device unseen for this many days carries `inactive_days` (the offline limit of
/// a key change: its bundle expires on the server after 30 days).
pub const INACTIVE_AFTER_DAYS: i64 = 30;

/// A raw `sync_devices` row as the list reads it.
struct ListRow {
    id: String,
    name: String,
    platform: String,
    app_version: String,
    joined_at: i64,
    cert: Option<String>,
    removed: bool,
    left_record: Option<String>,
    removal_record: Option<String>,
}

/// `(at, signer)` of a stored record column, read for display only (a list never trusts it:
/// the trust store's entries are the verified word).
fn record_facts(value: Option<&str>) -> Option<(RecordType, i64, DeviceId)> {
    let record = Record::from_b64(value?).ok()?;
    Some((record.kind, record.at, record.signer))
}

/// The devices of this account as Settings lists them (revision §4.1, §6.1, §6.5): every row
/// but the recovery kit's, each with where it stands. This device first, then active before
/// "Zkontrolujte" before former, newest change first (a device without one last), then by name.
///
/// * **Active**: this device, and every pinned `trusted` device that gets the next key. In a
///   vault from before phase 5 (no certificate pinned for this device) every row is active with
///   `joined_via = unknown`.
/// * **Check**: a row with no pinned certificate, or a `trusted` entry the next key change leaves
///   out for its issuer (`IssuerRemoved`, `LateVouch`, `IssuerUnknown`), or an older ID of a
///   device that connected again which still writes changes (a copy). A superseded ID that wrote
///   nothing since its successor appeared is not listed at all (O11).
/// * **Former**: a `left` or `removed` entry (or a row marked removed that was never pinned),
///   listed for `FORMER_LISTED_SECS` after its state time.
///
/// `last_active_at` of this device is its last successful sync; of another, the newest clock
/// stamp it wrote (unix seconds).
///
/// A former device the person took off the list here (`hide_former_devices`) is left out. Only a
/// former one: the same ID listed as anything else shows again.
pub fn list_devices(conn: &Connection, now: i64) -> Result<Vec<SyncDevice>> {
    let hidden = hidden_devices(conn)?;
    let mut devices = all_devices(conn, now)?;
    devices.retain(|d| !(d.status == SyncDeviceStatus::Former && hidden.contains(&d.id)));
    Ok(devices)
}

/// The former devices taken off this device's list (hex IDs). A value that does not read hides
/// nothing.
fn hidden_devices(conn: &Connection) -> Result<HashSet<String>> {
    Ok(state::config_text(conn, state::HIDDEN_DEVICES_KEY)?
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value).ok())
        .unwrap_or_default()
        .into_iter()
        .collect())
}

/// "Odstranit ze seznamu" / "Odstranit vše": takes former devices off this device's list. Local
/// only (`state::HIDDEN_DEVICES_KEY`, never synced) and no password: nothing about trust changes.
/// The `sync_devices` rows and the trust store's `left` / `removed` entries stay as they are, so
/// whatever such a device sends later is still refused (ADR 0032). A device that is not former now
/// (or not listed at all) is passed over; the stored set keeps only devices the list would still
/// show as former, so it never outgrows the 30-day former group. Returns how many came off.
pub fn hide_former_devices(conn: &Connection, devices: &[DeviceId], now: i64) -> Result<usize> {
    let former: HashSet<String> = all_devices(conn, now)?
        .into_iter()
        .filter(|d| d.status == SyncDeviceStatus::Former)
        .map(|d| d.id)
        .collect();
    let mut hidden = hidden_devices(conn)?;
    hidden.retain(|id| former.contains(id));
    let before = hidden.len();
    hidden.extend(
        devices
            .iter()
            .map(DeviceId::to_hex)
            .filter(|id| former.contains(id)),
    );
    let added = hidden.len() - before;
    let mut stored: Vec<String> = hidden.into_iter().collect();
    stored.sort();
    let value = if stored.is_empty() {
        None
    } else {
        Some(
            serde_json::to_string(&stored)
                .map_err(|e| AppError::Internal(format!("sync: the hidden devices: {e}")))?,
        )
    };
    state::set_config_text(conn, state::HIDDEN_DEVICES_KEY, value.as_deref())?;
    Ok(added)
}

/// Every device the list could show, hidden former ones included (`list_devices`).
fn all_devices(conn: &Connection, now: i64) -> Result<Vec<SyncDevice>> {
    let Some(sync) = state::load(conn)? else {
        return Ok(Vec::new());
    };
    let this = sync.device.to_hex();
    let this_active = state::marks(conn)?.and_then(|m| m.last_success_at);
    // One pass over the clock index per device, matched here by the hex of the raw ID (a
    // `hex(device)` per row in SQL would scan the whole table for every device).
    let mut newest = HashMap::new();
    let mut stmt = conn.prepare(NEWEST_CLOCKS)?;
    for row in stmt.query_map([], |r| Ok((r.get::<_, Vec<u8>>(0)?, r.get::<_, i64>(1)?)))? {
        let (device, hlc) = row?;
        newest.insert(hex_of(&device), hlc);
    }
    let mut stmt = conn.prepare("SELECT device FROM sync_heads WHERE quarantined != 0")?;
    let held: HashSet<String> = stmt
        .query_map([], |r| r.get::<_, Vec<u8>>(0))?
        .map(|device| device.map(|d| hex_of(&d)))
        .collect::<std::result::Result<_, _>>()?;
    let mut stmt = conn.prepare(
        "SELECT id, name, platform, app_version, joined_at, cert, removed, left_record, removal_record FROM sync_devices",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(ListRow {
                id: r.get(0)?,
                name: r.get(1)?,
                platform: r.get(2)?,
                app_version: r.get(3)?,
                joined_at: r.get(4)?,
                cert: r.get(5)?,
                removed: r.get::<_, i64>(6)? != 0,
                left_record: r.get(7)?,
                removal_record: r.get(8)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    let supported = trust::is_supported(conn)?;
    let entries: BTreeMap<DeviceId, TrustEntry> = trust::entries(conn)?
        .into_iter()
        .map(|e| (e.device, e))
        .collect();
    // The next key change's view of the store: who it leaves out and why. A vault over the
    // device cap cannot be judged; its rows then show without a verdict.
    let excluded: BTreeMap<DeviceId, trust::ExclusionReason> = if supported {
        match trust::recipients(conn, &[]) {
            Ok(recipients) => recipients.excluded.into_iter().collect(),
            Err(error) => {
                log::debug!("[SYNC] The device list is shown without verdicts: {error}");
                BTreeMap::new()
            }
        }
    } else {
        BTreeMap::new()
    };
    let superseded: HashSet<DeviceId> = entries
        .values()
        .filter(|e| e.kind == CertKind::Rejoin)
        .map(|e| e.issuer)
        .collect();
    let removing: HashSet<DeviceId> = rotate::pending_removals(conn)?.into_iter().collect();
    // A profile with no vault bound has no certificates to recognise a Kit's row by.
    let vault = trust::vault_id(conn).ok();

    let mut devices = Vec::new();
    for row in rows {
        let Ok(device) = DeviceId::from_hex(&row.id) else {
            continue;
        };
        if row.platform == "recoveryKit"
            || vault
                .as_ref()
                .is_some_and(|vault| trust::is_kit_row(device, row.cert.as_deref(), vault))
            || entries
                .get(&device)
                .is_some_and(|entry| entry.kind == CertKind::Kit)
        {
            continue;
        }
        let this_device = row.id == this;
        let entry = entries.get(&device);
        let (status, check_reason, former_reason, state_at, state_by) = if this_device {
            (SyncDeviceStatus::Active, None, None, None, None)
        } else if let Some(entry) = entry {
            match entry.state {
                TrustState::Left => (
                    SyncDeviceStatus::Former,
                    None,
                    Some(SyncFormerReason::Left),
                    entry.state_at,
                    None,
                ),
                TrustState::Removed => (
                    SyncDeviceStatus::Former,
                    None,
                    Some(SyncFormerReason::Removed),
                    entry.state_at,
                    entry.state_by,
                ),
                TrustState::Trusted => {
                    let reason = match excluded.get(&device) {
                        // Wrote nothing since its successor appeared: the successor's details
                        // say "Znovu připojeno"; not listed.
                        Some(trust::ExclusionReason::SupersededQuiet) => continue,
                        Some(trust::ExclusionReason::IssuerRemoved) => {
                            Some(SyncCheckReason::IssuerRemoved)
                        }
                        Some(trust::ExclusionReason::LateVouch) => Some(SyncCheckReason::LateVouch),
                        Some(_) => Some(SyncCheckReason::Unverified),
                        None if superseded.contains(&device) => Some(SyncCheckReason::Copy),
                        None => None,
                    };
                    match reason {
                        Some(reason) => (SyncDeviceStatus::Check, Some(reason), None, None, None),
                        None => (SyncDeviceStatus::Active, None, None, None, None),
                    }
                }
            }
        } else if row.removed {
            // Marked removed, never pinned here: the record's own facts, for display.
            let removal = record_facts(row.removal_record.as_deref());
            let left = record_facts(row.left_record.as_deref());
            match (removal, left) {
                (Some((_, at, by)), _) => (
                    SyncDeviceStatus::Former,
                    None,
                    Some(SyncFormerReason::Removed),
                    Some(at),
                    Some(by),
                ),
                (None, Some((_, at, _))) => (
                    SyncDeviceStatus::Former,
                    None,
                    Some(SyncFormerReason::Left),
                    Some(at),
                    None,
                ),
                // A pre-phase-5 removal: no record and no time, so no way to let it age out of
                // the list. Not listed (review of Task 24, M3).
                (None, None) => continue,
            }
        } else if supported {
            (
                SyncDeviceStatus::Check,
                Some(SyncCheckReason::Unverified),
                None,
                None,
                None,
            )
        } else {
            (SyncDeviceStatus::Active, None, None, None, None)
        };
        if status == SyncDeviceStatus::Former
            && state_at.is_some_and(|at| now.saturating_sub(at) > FORMER_LISTED_SECS)
        {
            continue;
        }
        let last_active_at = if this_device {
            this_active
        } else {
            newest
                .get(&row.id)
                .map(|h| (unpack(*h as u64).0 / 1000) as i64)
        };
        let inactive_days = (status == SyncDeviceStatus::Active && !this_device)
            .then(|| {
                let seen = last_active_at.unwrap_or(row.joined_at);
                now.saturating_sub(seen).max(0) / 86_400
            })
            .filter(|days| *days >= INACTIVE_AFTER_DAYS)
            .and_then(|days| u32::try_from(days).ok());
        let joined_via = match entry.map(|e| e.kind) {
            Some(CertKind::Genesis) => SyncJoinedVia::Created,
            Some(CertKind::Paired) => SyncJoinedVia::Paired,
            Some(CertKind::RecoveryKit) => SyncJoinedVia::RecoveryKit,
            Some(CertKind::Rejoin) => SyncJoinedVia::Rejoined,
            Some(CertKind::Kit) | None => SyncJoinedVia::Unknown,
        };
        let added_by = match entry {
            Some(entry) if entry.kind == CertKind::Paired => {
                Some(rekey::device_name(conn, entry.issuer)?)
            }
            _ => None,
        };
        let state_by = match state_by {
            Some(by) => Some(rekey::device_name(conn, by)?),
            None => None,
        };
        devices.push(SyncDevice {
            held: held.contains(&row.id),
            name: row.name,
            platform: row.platform,
            app_version: row.app_version,
            joined_at: row.joined_at,
            last_active_at,
            this_device,
            removal_pending: removing.contains(&device),
            status,
            check_reason,
            former_reason,
            joined_via,
            added_by,
            state_at,
            state_by,
            inactive_days,
            id: row.id,
        });
    }
    let rank = |status: SyncDeviceStatus| match status {
        SyncDeviceStatus::Active => 0,
        SyncDeviceStatus::Check => 1,
        SyncDeviceStatus::Former => 2,
    };
    devices.sort_by(|a, b| {
        b.this_device
            .cmp(&a.this_device)
            .then_with(|| rank(a.status).cmp(&rank(b.status)))
            .then_with(|| b.last_active_at.cmp(&a.last_active_at))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(devices)
}

/// Renames this device: the trimmed name goes into its `sync_devices` row, a captured write, so
/// it syncs to the other devices. Needs sync on (`validation.syncSetupIncomplete` otherwise).
pub fn rename_this_device(conn: &Connection, name: &str) -> Result<()> {
    validate_device_name(name)?;
    let device = match state::load(conn)? {
        Some(sync) if sync.enabled => sync.device,
        _ => {
            return Err(AppError::Validation(
                "validation.syncSetupIncomplete".into(),
            ))
        }
    };
    let changed = conn.execute(
        "UPDATE sync_devices SET name = ?1 WHERE id = ?2",
        params![name.trim(), device.to_hex()],
    )?;
    if changed == 0 {
        return Err(AppError::Validation(
            "validation.syncSetupIncomplete".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;
    use crate::services::sync::state;
    use crate::services::sync::test_support::migrated;

    const DEVICE: DeviceId = DeviceId([7; 16]);
    const DEVICE_HEX: &str = "07070707070707070707070707070707";
    const NOW: i64 = 1_900_100_000;

    fn row(conn: &Connection) -> (String, Option<String>, String, String, String, i64, i64) {
        conn.query_row(
            "SELECT id, public_key, name, platform, app_version, joined_at, removed \
             FROM sync_devices",
            [],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                ))
            },
        )
        .expect("one device row")
    }

    fn outbox(conn: &Connection) -> Vec<(String, String)> {
        let mut stmt = conn
            .prepare("SELECT tbl, pk FROM sync_outbox WHERE tbl = 'sync_devices'")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    /// Phase 5: the certificate takes a value once, like the public key; a later one never
    /// replaces it, and `None` never clears it.
    #[test]
    fn the_certificate_is_written_once() {
        use crate::services::sync::identity::{cert, CertFields, CertKind, SignSeed};
        use zeroize::Zeroizing;
        let issue = |byte: u8| {
            let fields = CertFields {
                vault_id: [7; 16],
                device: DEVICE,
                box_pub: [1; 32],
                issued_at: 1,
            };
            let seed = SignSeed::from_bytes(Zeroizing::new([byte; 32]));
            cert::issue_self(CertKind::Genesis, &fields, &seed).unwrap()
        };
        let cert_of = |conn: &Connection| -> Option<String> {
            conn.query_row("SELECT cert FROM sync_devices", [], |r| r.get(0))
                .unwrap()
        };
        let conn = migrated();
        state::enable(&conn, DEVICE).unwrap();
        record_this_device(&conn, "MacBook", &[1; 32], None, 1).unwrap();
        assert_eq!(cert_of(&conn), None);

        let first = issue(0x31);
        record_this_device(&conn, "MacBook", &[1; 32], Some(&first), 2).unwrap();
        assert_eq!(
            cert_of(&conn),
            Some(first.to_b64()),
            "an empty column fills"
        );
        record_this_device(&conn, "MacBook", &[1; 32], Some(&issue(0x32)), 3).unwrap();
        record_this_device(&conn, "MacBook", &[1; 32], None, 4).unwrap();
        assert_eq!(cert_of(&conn), Some(first.to_b64()), "never replaced");
    }

    #[test]
    fn count_devices_counts_the_rows_not_removed() {
        let conn = migrated();
        assert_eq!(count_devices(&conn).unwrap(), 0);
        state::enable(&conn, DEVICE).unwrap();
        record_this_device(&conn, "MacBook", &[0x01; 32], None, 1_800_000_000).unwrap();
        conn.execute(
            "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at) \
             VALUES (?1, NULL, 'iPhone', 'ios', '1.0.0', 1_850_000_000)",
            [DeviceId([9; 16]).to_hex()],
        )
        .unwrap();
        assert_eq!(count_devices(&conn).unwrap(), 2);
        conn.execute(
            "UPDATE sync_devices SET removed = 1 WHERE id = ?1",
            [DeviceId([9; 16]).to_hex()],
        )
        .unwrap();
        assert_eq!(count_devices(&conn).unwrap(), 1);
    }

    #[test]
    fn this_device_joined_at_is_its_first_record() {
        let conn = migrated();
        assert_eq!(this_device_joined_at(&conn).unwrap(), None);
        state::enable(&conn, DEVICE).unwrap();
        assert_eq!(this_device_joined_at(&conn).unwrap(), None);

        record_this_device(&conn, "MacBook", &[0x01; 32], None, 1_800_000_000).unwrap();
        assert_eq!(this_device_joined_at(&conn).unwrap(), Some(1_800_000_000));

        // Another device joined later: this device's own row is still the one read.
        conn.execute(
            "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at) \
             VALUES (?1, NULL, 'iPhone', 'ios', '1.0.0', 1_850_000_000)",
            [DeviceId([9; 16]).to_hex()],
        )
        .unwrap();
        // A second record under another name changes neither the row's age nor which row is read.
        record_this_device(&conn, "Work Mac", &[0x01; 32], None, 1_900_000_000).unwrap();
        assert_eq!(this_device_joined_at(&conn).unwrap(), Some(1_800_000_000));
    }

    #[test]
    fn a_recorded_device_row_is_captured_and_keeps_its_public_key() {
        let conn = migrated();
        state::enable(&conn, DEVICE).unwrap();
        assert_eq!(this_device(&conn).unwrap(), None);

        record_this_device(&conn, "  MacBook  ", &[0x01; 32], None, 1_800_000_000).unwrap();

        assert_eq!(
            row(&conn),
            (
                DEVICE_HEX.to_string(),
                Some("AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE".to_string()),
                "MacBook".to_string(),
                platform().to_string(),
                env!("CARGO_PKG_VERSION").to_string(),
                1_800_000_000,
                0,
            )
        );
        assert_eq!(
            outbox(&conn),
            vec![("sync_devices".to_string(), DEVICE_HEX.to_string())]
        );
        conn.execute_batch("DELETE FROM sync_outbox").unwrap();

        record_this_device(&conn, "Work Mac", &[0x02; 32], None, 1_900_000_000).unwrap();

        let (_, public_key, name, _, _, joined_at, _) = row(&conn);
        assert_eq!(
            public_key.as_deref(),
            Some("AQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQEBAQE"),
            "the public key is write-once"
        );
        assert_eq!(name, "Work Mac");
        assert_eq!(joined_at, 1_800_000_000, "joined_at is the first record");
        assert_eq!(
            outbox(&conn),
            vec![("sync_devices".to_string(), DEVICE_HEX.to_string())]
        );
        assert_eq!(
            this_device(&conn).unwrap(),
            Some(("Work Mac".to_string(), platform().to_string()))
        );
        assert_eq!(
            device_row(&conn, DEVICE).unwrap(),
            Some(("Work Mac".to_string(), platform().to_string()))
        );
        assert_eq!(device_row(&conn, DeviceId([9; 16])).unwrap(), None);

        conn.execute_batch("UPDATE sync_devices SET public_key = NULL")
            .unwrap();
        record_this_device(&conn, "Work Mac", &[0x03; 32], None, 1_950_000_000).unwrap();
        assert_eq!(
            row(&conn).1.as_deref(),
            Some("AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM"),
            "an empty public key takes a value once"
        );
        record_this_device(&conn, "Work Mac", &[0x02; 32], None, 1_960_000_000).unwrap();
        assert_eq!(
            row(&conn).1.as_deref(),
            Some("AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM")
        );
    }

    #[test]
    fn recording_needs_the_sync_row() {
        let conn = migrated();
        assert!(record_this_device(&conn, "MacBook", &[0x01; 32], None, 1).is_err());
        assert_eq!(this_device(&conn).unwrap(), None);
    }

    #[test]
    fn device_names_outside_1_to_60_characters_are_refused() {
        let refused = |name: &str| {
            matches!(
                validate_device_name(name),
                Err(AppError::Validation(key)) if key == "validation.syncDeviceNameInvalid"
            )
        };
        assert!(refused(""));
        assert!(refused("   "));
        assert!(refused(&"a".repeat(61)));
        assert!(refused(&"č".repeat(61)));
        assert!(validate_device_name("a").is_ok());
        assert!(validate_device_name(&"č".repeat(60)).is_ok());
        assert!(validate_device_name(&format!("  {}  ", "a".repeat(60))).is_ok());

        let conn = migrated();
        state::enable(&conn, DEVICE).unwrap();
        assert!(record_this_device(&conn, " ", &[0x01; 32], None, 1).is_err());
        assert_eq!(this_device(&conn).unwrap(), None);
    }

    #[test]
    fn the_platform_is_one_of_the_five() {
        assert!(["macos", "windows", "linux", "ios", "android"].contains(&platform()));
        #[cfg(target_os = "macos")]
        assert_eq!(platform(), "macos");
    }

    fn insert_other(conn: &Connection, id: DeviceId, name: &str) {
        conn.execute(
            "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at) \
             VALUES (?1, NULL, ?2, 'ios', '1.0.0', 1_700_000_000)",
            params![id.to_hex(), name],
        )
        .unwrap();
    }

    fn other_clock(conn: &Connection, id: DeviceId, hlc: u64) {
        conn.execute(
            "INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES ('x', ?1, 'c', ?2, ?3)",
            params![id.to_hex(), hlc as i64, id.0.to_vec()],
        )
        .unwrap();
    }

    #[test]
    fn the_list_marks_this_device_and_orders_by_last_change() {
        let conn = migrated();
        state::enable(&conn, DEVICE).unwrap();
        record_this_device(&conn, "MacBook", &[1; 32], None, 1_700_000_000).unwrap();
        conn.execute("UPDATE sync_state SET last_success_at = 1_750_000_000", [])
            .unwrap();
        let (b, c, d) = (DeviceId([8; 16]), DeviceId([9; 16]), DeviceId([6; 16]));
        insert_other(&conn, b, "Zeta");
        insert_other(&conn, c, "Alfa");
        insert_other(&conn, d, "Removed");
        conn.execute(
            "UPDATE sync_devices SET removed = 1 WHERE id = ?1",
            [d.to_hex()],
        )
        .unwrap();
        other_clock(
            &conn,
            b,
            crate::services::sync::hlc::pack(1_800_000_000_000, 0),
        );
        other_clock(
            &conn,
            c,
            crate::services::sync::hlc::pack(1_900_000_000_000, 3),
        );

        let list = list_devices(&conn, NOW).unwrap();
        let summary: Vec<_> = list
            .iter()
            .map(|d| (d.name.as_str(), d.this_device, d.last_active_at, d.held))
            .collect();
        assert_eq!(
            summary,
            vec![
                ("MacBook", true, Some(1_750_000_000), false),
                ("Alfa", false, Some(1_900_000_000), false),
                ("Zeta", false, Some(1_800_000_000), false),
                // "Removed" has no record and no time: not listed (it could never age out).
            ]
        );
        assert_eq!(list[0].id, DEVICE_HEX);
        assert_eq!(list[0].platform, platform());
        assert_eq!(list[0].app_version, env!("CARGO_PKG_VERSION"));
        assert_eq!(list[0].joined_at, 1_700_000_000);
    }

    #[test]
    fn a_held_device_is_flagged() {
        let conn = migrated();
        state::enable(&conn, DEVICE).unwrap();
        record_this_device(&conn, "MacBook", &[1; 32], None, 1).unwrap();
        let b = DeviceId([8; 16]);
        insert_other(&conn, b, "Phone");
        conn.execute(
            "INSERT INTO sync_heads (device, seq, hash, quarantined) VALUES (?1, 1, x'00', 1)",
            [b.0.to_vec()],
        )
        .unwrap();
        let held: Vec<_> = list_devices(&conn, NOW)
            .unwrap()
            .into_iter()
            .map(|d| (d.name, d.held))
            .collect();
        assert_eq!(
            held,
            vec![("MacBook".to_string(), false), ("Phone".to_string(), true)]
        );
    }

    #[test]
    fn a_device_without_clocks_has_no_last_change() {
        let conn = migrated();
        state::enable(&conn, DEVICE).unwrap();
        record_this_device(&conn, "MacBook", &[1; 32], None, 1).unwrap();
        insert_other(&conn, DeviceId([8; 16]), "Phone");
        let list = list_devices(&conn, NOW).unwrap();
        assert_eq!(list[0].last_active_at, None, "this device never synced");
        assert_eq!(list[1].last_active_at, None);
        assert!(list_devices(&migrated(), NOW).unwrap().is_empty());
    }

    #[test]
    fn the_newest_clocks_are_read_along_the_device_index() {
        let conn = migrated();
        let plan: Vec<String> = conn
            .prepare(&format!("EXPLAIN QUERY PLAN {NEWEST_CLOCKS}"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<std::result::Result<_, _>>()
            .unwrap();
        assert!(
            plan.iter()
                .any(|step| step.contains("COVERING INDEX idx_sync_clock_device_hlc")),
            "{plan:?}"
        );
        assert_eq!(hex_of(&DEVICE.0), DEVICE_HEX);
    }

    #[test]
    fn renaming_needs_this_devices_row() {
        let conn = migrated();
        state::enable(&conn, DEVICE).unwrap();
        // Sync is on, but this device's row was never recorded.
        assert!(matches!(
            rename_this_device(&conn, "MacBook"),
            Err(AppError::Validation(k)) if k == "validation.syncSetupIncomplete"
        ));
        let rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM sync_devices", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, 0, "nothing was written");
    }

    #[test]
    fn renaming_this_device_is_captured_and_validated() {
        let conn = migrated();
        state::enable(&conn, DEVICE).unwrap();
        record_this_device(&conn, "MacBook", &[1; 32], None, 1).unwrap();
        let b = DeviceId([8; 16]);
        insert_other(&conn, b, "Phone");
        conn.execute_batch("DELETE FROM sync_outbox").unwrap();

        rename_this_device(&conn, "  Pracovní Mac ").unwrap();
        let name = |id: &str| -> String {
            conn.query_row("SELECT name FROM sync_devices WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(name(DEVICE_HEX), "Pracovní Mac");
        assert_eq!(name(&b.to_hex()), "Phone");
        assert_eq!(
            outbox(&conn),
            vec![("sync_devices".to_string(), DEVICE_HEX.to_string())]
        );

        for bad in ["", &"a".repeat(61)] {
            assert!(matches!(
                rename_this_device(&conn, bad),
                Err(AppError::Validation(k)) if k == "validation.syncDeviceNameInvalid"
            ));
        }
        assert_eq!(name(DEVICE_HEX), "Pracovní Mac");

        conn.execute("UPDATE sync_state SET enabled = 0", [])
            .unwrap();
        assert!(matches!(
            rename_this_device(&conn, "X"),
            Err(AppError::Validation(k)) if k == "validation.syncSetupIncomplete"
        ));
        assert!(matches!(
            rename_this_device(&migrated(), "X"),
            Err(AppError::Validation(k)) if k == "validation.syncSetupIncomplete"
        ));
    }
}
