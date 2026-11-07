//! The single `sync_state` row (spec §6.2): whether sync is on, this device's identity and the
//! engine's bookkeeping, and turning sync on and off on this device.

use rusqlite::{params, Connection, OptionalExtension};

use super::capture;
use super::hlc::DeviceId;
use super::in_savepoint;
use super::registry::keys::{barrier_number, schema_number};
use crate::error::{AppError, Result};
use crate::services::categorization::packs;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncState {
    pub enabled: bool,
    pub device: DeviceId,
    pub next_seq: u64,
    pub last_hash: Option<[u8; 32]>,
    pub cursor: u64,
    pub snapshot_hlc: u64,
    pub barrier: u32,
    pub schema_seen: u32,
}

type RawState = (bool, Vec<u8>, i64, Option<Vec<u8>>, i64, i64, i64, i64);

/// The row, or `None` for a profile that never turned sync on. One `SELECT` (G5).
pub fn load(conn: &Connection) -> Result<Option<SyncState>> {
    let raw: Option<RawState> = conn
        .query_row(
            "SELECT enabled, device_id, next_seq, last_hash, cursor, snapshot_hlc, barrier, schema_seen \
             FROM sync_state WHERE id = 1",
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
                    r.get(7)?,
                ))
            },
        )
        .optional()?;
    let Some((enabled, device, next_seq, last_hash, cursor, snapshot_hlc, barrier, schema_seen)) =
        raw
    else {
        return Ok(None);
    };
    let device: [u8; 16] = device
        .try_into()
        .map_err(|_| AppError::Internal("sync_state.device_id is not 16 bytes".into()))?;
    let last_hash = last_hash
        .map(|h| {
            <[u8; 32]>::try_from(h)
                .map_err(|_| AppError::Internal("sync_state.last_hash is not 32 bytes".into()))
        })
        .transpose()?;
    Ok(Some(SyncState {
        enabled,
        device: DeviceId(device),
        next_seq: unsigned(next_seq, "next_seq")?,
        last_hash,
        cursor: unsigned(cursor, "cursor")?,
        snapshot_hlc: unsigned(snapshot_hlc, "snapshot_hlc")?,
        barrier: unsigned(barrier, "barrier")?,
        schema_seen: unsigned(schema_seen, "schema_seen")?,
    }))
}

fn unsigned<T: TryFrom<i64>>(value: i64, column: &str) -> Result<T> {
    T::try_from(value)
        .map_err(|_| AppError::Internal(format!("sync_state.{column} is out of range: {value}")))
}

/// Creates the row (enabled, this device, today's schema and barrier) or, when it exists, turns it
/// on for `device`. Never touches the cursor, the snapshot columns, `next_seq` or `last_hash` (E3).
pub fn ensure_row(conn: &Connection, device: DeviceId) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (id, enabled, device_id, schema_seen, barrier) VALUES (1, 1, ?1, ?2, ?3) \
         ON CONFLICT(id) DO UPDATE SET enabled = 1, device_id = excluded.device_id",
        params![device.0.to_vec(), schema_number(), barrier_number()],
    )?;
    Ok(())
}

/// Turns sync on for the first device of a vault, in the order of ruling R4: the row, then capture
/// on this connection, then the effective rule-pack keys, so those two keys are clocked and queued.
pub fn enable(conn: &Connection, device: DeviceId) -> Result<()> {
    in_savepoint(conn, "sync_enable", || {
        ensure_row(conn, device)?;
        capture::install(conn, device)?;
        packs::persist_effective(conn)
    })
}

/// Turns sync off on this device (§11.6, local part): capture goes, and so does every sync row.
/// Local data stays.
pub fn disable(conn: &Connection) -> Result<()> {
    in_savepoint(conn, "sync_disable", || {
        capture::uninstall(conn)?;
        conn.execute_batch(
            "DELETE FROM sync_state; DELETE FROM sync_clock; DELETE FROM sync_outbox; \
             DELETE FROM sync_heads; DELETE FROM sync_own_log; DELETE FROM sync_unsent; \
             DELETE FROM sync_typed;",
        )?;
        Ok(())
    })
}

/// Records that this build opened the profile (its schema number), for the barrier logic (§14).
pub fn note_schema_seen(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET schema_seen = ?1 WHERE id = 1",
        [schema_number()],
    )?;
    Ok(())
}

pub fn record_error(conn: &Connection, message: &str) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET last_error = ?1, last_error_at = unixepoch() WHERE id = 1",
        [message],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sync::capture;
    use crate::services::sync::hlc::DeviceId;
    use crate::services::sync::registry::keys::{barrier_number, schema_number};
    use crate::services::sync::test_support::migrated;
    use rusqlite::Connection;

    const DEVICE: DeviceId = DeviceId([7; 16]);
    const OTHER: DeviceId = DeviceId([9; 16]);

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).expect(sql)
    }

    fn outbox(conn: &Connection) -> Vec<(String, String)> {
        let mut stmt = conn
            .prepare("SELECT tbl, pk FROM sync_outbox ORDER BY tbl, pk")
            .unwrap();
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect()
    }

    #[test]
    fn enabling_sync_writes_the_effective_rule_packs_and_queues_them() {
        let conn = migrated();
        conn.execute_batch(
            "INSERT INTO user_profile (id, name, surname, email, language) \
               VALUES (1, 'Alex', 'Novak', 'a@b.cz', 'cs'); \
             DELETE FROM app_config WHERE key LIKE 'rule_packs%';",
        )
        .unwrap();

        enable(&conn, DEVICE).unwrap();

        let value = |key: &str| -> String {
            conn.query_row("SELECT value FROM app_config WHERE key = ?1", [key], |r| {
                r.get(0)
            })
            .unwrap()
        };
        assert_eq!(value("rule_packs_enabled"), r#"["global","cz"]"#);
        assert_eq!(value("rule_packs_disabled_rules"), "[]");
        assert_eq!(
            outbox(&conn),
            vec![
                (
                    "app_config".to_string(),
                    "rule_packs_disabled_rules".to_string()
                ),
                ("app_config".to_string(), "rule_packs_enabled".to_string()),
            ]
        );
        let mut stmt = conn
            .prepare("SELECT pk, col, device FROM sync_clock ORDER BY pk, col")
            .unwrap();
        let clocks: Vec<(String, String, Vec<u8>)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            clocks,
            vec![
                (
                    "rule_packs_disabled_rules".into(),
                    "*".into(),
                    DEVICE.0.to_vec()
                ),
                ("rule_packs_enabled".into(), "*".into(), DEVICE.0.to_vec()),
            ]
        );
        assert!(capture::is_installed(&conn).unwrap());
        let state = load(&conn).unwrap().expect("row");
        assert!(state.enabled);
        assert_eq!(state.device, DEVICE);
    }

    #[test]
    fn ensure_row_never_resets_the_cursor_or_snapshot_columns() {
        let conn = migrated();
        assert!(load(&conn).unwrap().is_none());
        ensure_row(&conn, DEVICE).unwrap();
        let fresh = load(&conn).unwrap().expect("row");
        assert!(fresh.enabled);
        assert_eq!(fresh.device, DEVICE);
        assert_eq!(fresh.next_seq, 1);
        assert_eq!(fresh.last_hash, None);
        assert_eq!(fresh.cursor, 0);
        assert_eq!(fresh.snapshot_hlc, 0);
        assert_eq!(fresh.schema_seen, schema_number());
        assert_eq!(fresh.barrier, barrier_number());

        conn.execute(
            "UPDATE sync_state SET enabled = 0, cursor = 7, snapshot_seq = 3, snapshot_hlc = 99, \
               next_seq = 5, last_hash = ?1, schema_seen = 2, barrier = 1",
            [vec![0xab_u8; 32]],
        )
        .unwrap();
        ensure_row(&conn, OTHER).unwrap();

        let kept = load(&conn).unwrap().expect("row");
        assert!(kept.enabled);
        assert_eq!(kept.device, OTHER);
        assert_eq!(kept.cursor, 7);
        assert_eq!(kept.snapshot_hlc, 99);
        assert_eq!(kept.next_seq, 5);
        assert_eq!(kept.last_hash, Some([0xab; 32]));
        assert_eq!(kept.schema_seen, 2);
        assert_eq!(kept.barrier, 1);
        assert_eq!(count(&conn, "SELECT snapshot_seq FROM sync_state"), 3);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_state"), 1);
    }

    #[test]
    fn disable_removes_triggers_and_every_sync_row() {
        let conn = migrated();
        enable(&conn, DEVICE).unwrap();
        conn.execute_batch(
            "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
             INSERT INTO sync_heads (device, seq, hash) VALUES (x'09', 1, x'00'); \
             INSERT INTO sync_own_log (seq, hash, max_hlc) VALUES (1, x'00', 1); \
             INSERT INTO sync_unsent (seq, envelope) VALUES (1, x'00'); \
             INSERT INTO sync_typed (tbl, pk, col, typed, assigned) VALUES ('stock_tags', 't', 'name', 'A', 'A (2)');",
        )
        .unwrap();
        assert!(count(&conn, "SELECT COUNT(*) FROM sync_clock") > 0);

        disable(&conn).unwrap();

        assert!(!capture::is_installed(&conn).unwrap());
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sqlite_temp_master"), 0);
        for table in [
            "sync_state",
            "sync_clock",
            "sync_outbox",
            "sync_heads",
            "sync_own_log",
            "sync_unsent",
            "sync_typed",
        ] {
            assert_eq!(
                count(&conn, &format!("SELECT COUNT(*) FROM {table}")),
                0,
                "{table}"
            );
        }
        conn.execute_batch("INSERT INTO institutions (id, name) VALUES ('i2', 'Bank 2')")
            .unwrap();
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_clock"), 0);
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM institutions WHERE id IN ('i1', 'i2')"
            ),
            2,
            "local data stays"
        );
        crate::services::sync::on_open(&conn);
        assert!(!capture::is_installed(&conn).unwrap());
    }

    #[test]
    fn record_error_stamps_the_message_and_time() {
        let conn = migrated();
        ensure_row(&conn, DEVICE).unwrap();
        record_error(&conn, "relay unreachable").unwrap();
        let (message, at): (String, i64) = conn
            .query_row(
                "SELECT last_error, last_error_at FROM sync_state",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(message, "relay unreachable");
        assert!(at > 1_700_000_000);
    }
}
