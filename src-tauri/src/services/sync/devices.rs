//! This device's row in `sync_devices` (spec §11.4): its name, platform, app version and the
//! public half of its X25519 key pair, which the merge keeps write-once. `id` is the device ID as
//! 32 lowercase hex characters, `public_key` base64url.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::collections::{HashMap, HashSet};

use rusqlite::{params, Connection, OptionalExtension};

use super::hlc::{unpack, DeviceId};
use super::state;
use crate::error::{AppError, Result};
use crate::models::SyncDevice;

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

/// Writes this device's row: created with `public_key` and `joined_at = now` the first time,
/// afterwards the (trimmed) name, platform and app version change, and the public key only fills
/// an empty column: the merge's write-once rule (NULL may take a value once, a value never
/// changes).
pub fn record_this_device(
    conn: &Connection,
    name: &str,
    public_key: &[u8; 32],
    now: i64,
) -> Result<()> {
    validate_device_name(name)?;
    let device = state::load(conn)?
        .ok_or_else(|| AppError::Internal("sync: sync is not set up on this device".into()))?
        .device;
    conn.execute(
        "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6) \
         ON CONFLICT(id) DO UPDATE SET name = excluded.name, platform = excluded.platform, \
           app_version = excluded.app_version, \
           public_key = COALESCE(sync_devices.public_key, excluded.public_key)",
        params![
            device.to_hex(),
            URL_SAFE_NO_PAD.encode(public_key),
            name.trim(),
            platform(),
            env!("CARGO_PKG_VERSION"),
            now
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

/// The devices of this account as Settings lists them: this device first, then the newest change
/// first (a device without one last), then by name. A device that re-joined or turned sync off
/// keeps its old row until phase 5's removal. `last_active_at` of this device is its last
/// successful sync; of another, the newest clock stamp it wrote (unix seconds).
pub fn list_devices(conn: &Connection) -> Result<Vec<SyncDevice>> {
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
        "SELECT id, name, platform, app_version, joined_at FROM sync_devices WHERE removed = 0",
    )?;
    let mut devices = stmt
        .query_map([], |r| {
            let id: String = r.get(0)?;
            let this_device = id == this;
            let last_active_at = if this_device {
                this_active
            } else {
                newest.get(&id).map(|h| (unpack(*h as u64).0 / 1000) as i64)
            };
            Ok(SyncDevice {
                held: held.contains(&id),
                name: r.get(1)?,
                platform: r.get(2)?,
                app_version: r.get(3)?,
                joined_at: r.get(4)?,
                last_active_at,
                this_device,
                id,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    devices.sort_by(|a, b| {
        b.this_device
            .cmp(&a.this_device)
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

    #[test]
    fn count_devices_counts_the_rows_not_removed() {
        let conn = migrated();
        assert_eq!(count_devices(&conn).unwrap(), 0);
        state::enable(&conn, DEVICE).unwrap();
        record_this_device(&conn, "MacBook", &[0x01; 32], 1_800_000_000).unwrap();
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

        record_this_device(&conn, "MacBook", &[0x01; 32], 1_800_000_000).unwrap();
        assert_eq!(this_device_joined_at(&conn).unwrap(), Some(1_800_000_000));

        // Another device joined later: this device's own row is still the one read.
        conn.execute(
            "INSERT INTO sync_devices (id, public_key, name, platform, app_version, joined_at) \
             VALUES (?1, NULL, 'iPhone', 'ios', '1.0.0', 1_850_000_000)",
            [DeviceId([9; 16]).to_hex()],
        )
        .unwrap();
        // A second record under another name changes neither the row's age nor which row is read.
        record_this_device(&conn, "Work Mac", &[0x01; 32], 1_900_000_000).unwrap();
        assert_eq!(this_device_joined_at(&conn).unwrap(), Some(1_800_000_000));
    }

    #[test]
    fn a_recorded_device_row_is_captured_and_keeps_its_public_key() {
        let conn = migrated();
        state::enable(&conn, DEVICE).unwrap();
        assert_eq!(this_device(&conn).unwrap(), None);

        record_this_device(&conn, "  MacBook  ", &[0x01; 32], 1_800_000_000).unwrap();

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

        record_this_device(&conn, "Work Mac", &[0x02; 32], 1_900_000_000).unwrap();

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
        record_this_device(&conn, "Work Mac", &[0x03; 32], 1_950_000_000).unwrap();
        assert_eq!(
            row(&conn).1.as_deref(),
            Some("AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM"),
            "an empty public key takes a value once"
        );
        record_this_device(&conn, "Work Mac", &[0x02; 32], 1_960_000_000).unwrap();
        assert_eq!(
            row(&conn).1.as_deref(),
            Some("AwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwMDAwM")
        );
    }

    #[test]
    fn recording_needs_the_sync_row() {
        let conn = migrated();
        assert!(record_this_device(&conn, "MacBook", &[0x01; 32], 1).is_err());
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
        assert!(record_this_device(&conn, " ", &[0x01; 32], 1).is_err());
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
        record_this_device(&conn, "MacBook", &[1; 32], 1_700_000_000).unwrap();
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

        let list = list_devices(&conn).unwrap();
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
        record_this_device(&conn, "MacBook", &[1; 32], 1).unwrap();
        let b = DeviceId([8; 16]);
        insert_other(&conn, b, "Phone");
        conn.execute(
            "INSERT INTO sync_heads (device, seq, hash, quarantined) VALUES (?1, 1, x'00', 1)",
            [b.0.to_vec()],
        )
        .unwrap();
        let held: Vec<_> = list_devices(&conn)
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
        record_this_device(&conn, "MacBook", &[1; 32], 1).unwrap();
        insert_other(&conn, DeviceId([8; 16]), "Phone");
        let list = list_devices(&conn).unwrap();
        assert_eq!(list[0].last_active_at, None, "this device never synced");
        assert_eq!(list[1].last_active_at, None);
        assert!(list_devices(&migrated()).unwrap().is_empty());
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
        record_this_device(&conn, "MacBook", &[1; 32], 1).unwrap();
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
