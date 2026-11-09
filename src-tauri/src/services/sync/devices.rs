//! This device's row in `sync_devices` (spec §11.4): its name, platform, app version and the
//! public half of its X25519 key pair, which the merge keeps write-once. `id` is the device ID as
//! 32 lowercase hex characters, `public_key` base64url.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use rusqlite::{params, Connection, OptionalExtension};

use super::hlc::DeviceId;
use super::state;
use crate::error::{AppError, Result};

const MAX_NAME_CHARS: usize = 60;

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
}
