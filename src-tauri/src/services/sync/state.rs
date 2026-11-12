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
    pub barrier_seq: Option<u64>,
    /// The newest relay sequence at which this device skipped an envelope written below its
    /// barrier (migration 031); `None` when nothing is missed.
    pub missed_below_barrier: Option<u64>,
}

/// The engine's own bookkeeping on the `sync_state` row (migration 030), apart from the chain
/// and cursor fields of [`SyncState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineMarks {
    pub marker_barrier: u32,
    pub capture_broken_at: Option<i64>,
    pub vault_gone_at: Option<i64>,
    pub pulled_bytes: u64,
    pub pulled_bytes_base: u64,
    pub snapshot_uploaded_at: Option<i64>,
    /// When this device last found its snapshot too large to upload (migration 032).
    pub snapshot_too_large_at: Option<i64>,
    pub binding_written: bool,
    pub last_success_at: Option<i64>,
    pub last_error: Option<String>,
    /// The barrier of the newest snapshot this device uploaded or restored, or the barrier it
    /// started syncing at (migration 033). Below `SyncState::barrier`: the device crossed a
    /// barrier and owes the vault a snapshot at it (ruling T8-2, `needs_forced_snapshot`).
    pub snapshot_barrier: u32,
}

type RawState = (
    bool,
    Vec<u8>,
    i64,
    Option<Vec<u8>>,
    i64,
    i64,
    i64,
    i64,
    Option<i64>,
    Option<i64>,
);

/// The row, or `None` for a profile that never turned sync on. One `SELECT` (G5).
pub fn load(conn: &Connection) -> Result<Option<SyncState>> {
    let raw: Option<RawState> = conn
        .query_row(
            "SELECT enabled, device_id, next_seq, last_hash, cursor, snapshot_hlc, barrier, schema_seen, barrier_seq, \
               missed_below_barrier \
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
                    r.get(8)?,
                    r.get(9)?,
                ))
            },
        )
        .optional()?;
    let Some((
        enabled,
        device,
        next_seq,
        last_hash,
        cursor,
        snapshot_hlc,
        barrier,
        schema_seen,
        barrier_seq,
        missed_below_barrier,
    )) = raw
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
        barrier_seq: barrier_seq
            .map(|v| unsigned(v, "barrier_seq"))
            .transpose()?,
        missed_below_barrier: missed_below_barrier
            .map(|v| unsigned(v, "missed_below_barrier"))
            .transpose()?,
    }))
}

fn unsigned<T: TryFrom<i64>>(value: i64, column: &str) -> Result<T> {
    T::try_from(value)
        .map_err(|_| AppError::Internal(format!("sync_state.{column} is out of range: {value}")))
}

/// Creates the row (enabled, this device, today's schema and barrier) or, when it exists, turns it
/// on for `device`. Never touches the cursor, the snapshot columns, `next_seq` or `last_hash` (E3).
/// A disabled row (an account saved before setup) has crossed no barrier while it was off
/// (`open_capture` skips it), so turning it on raises its barrier to this build's; an enabled row
/// keeps its own, which `chain::cross_barrier` raises at the next open.
pub fn ensure_row(conn: &Connection, device: DeviceId) -> Result<()> {
    conn.execute(
        "INSERT INTO sync_state (id, enabled, device_id, schema_seen, barrier) VALUES (1, 1, ?1, ?2, ?3) \
         ON CONFLICT(id) DO UPDATE SET enabled = 1, device_id = excluded.device_id, \
           barrier = CASE WHEN enabled = 0 THEN MAX(barrier, excluded.barrier) ELSE barrier END",
        params![device.0.to_vec(), schema_number(), barrier_number()],
    )?;
    Ok(())
}

/// Turns sync on for the first device of a vault, in the order of ruling R4: the row, then capture
/// on this connection, then the effective rule-pack keys, so those two keys are clocked and queued.
/// A device that starts at its barrier needs no barrier marker, so its barrier counts as marked
/// (`note_starting_barrier`). Binding a vault (`keys::save_credentials`) before or after leaves
/// the same mark. Setup goes through `binding::enable`, which clears a stale restore marker first.
pub fn enable(conn: &Connection, device: DeviceId) -> Result<()> {
    in_savepoint(conn, "sync_enable", || {
        ensure_row(conn, device)?;
        capture::install(conn, device)?;
        packs::persist_effective(conn)?;
        note_starting_barrier(conn)
    })
}

/// A device that starts syncing at its barrier (turned on, or joined from a snapshot) has nothing
/// to mark for it: its barrier counts as marked (`marker_barrier`), so the push seals no empty
/// marker, and as snapshotted (`snapshot_barrier`), so no forced upload follows. Only a device
/// that crosses a barrier later needs either.
pub fn note_starting_barrier(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET marker_barrier = MAX(marker_barrier, barrier), \
           snapshot_barrier = MAX(snapshot_barrier, barrier) WHERE id = 1",
        [],
    )?;
    Ok(())
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
        clear_vault_went_back(conn)?;
        Ok(())
    })
}

/// Records that this build opened the profile (its schema number), for the barrier logic (§14)
/// and the unlock sweep. The mark never goes down: a mark left by a newer build stays, so going
/// back to that build sweeps no migration it has already seen and crosses no barrier again
/// (`chain::cross_barrier` only compares the mark with this build's barrier).
pub fn note_schema_seen(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET schema_seen = MAX(schema_seen, ?1) WHERE id = 1",
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

/// The engine's marks, or `None` for a profile that never turned sync on.
pub fn marks(conn: &Connection) -> Result<Option<EngineMarks>> {
    #[allow(clippy::type_complexity)]
    let raw: Option<(
        i64,
        Option<i64>,
        Option<i64>,
        i64,
        i64,
        Option<i64>,
        Option<i64>,
        bool,
        Option<i64>,
        Option<String>,
        i64,
    )> = conn
        .query_row(
            "SELECT marker_barrier, capture_broken_at, vault_gone_at, pulled_bytes, \
                    pulled_bytes_base, snapshot_uploaded_at, snapshot_too_large_at, \
                    binding_written, last_success_at, last_error, snapshot_barrier \
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
                    r.get(8)?,
                    r.get(9)?,
                    r.get(10)?,
                ))
            },
        )
        .optional()?;
    raw.map(
        |(
            marker_barrier,
            capture_broken_at,
            vault_gone_at,
            pulled_bytes,
            pulled_bytes_base,
            snapshot_uploaded_at,
            snapshot_too_large_at,
            binding_written,
            last_success_at,
            last_error,
            snapshot_barrier,
        )| {
            Ok(EngineMarks {
                marker_barrier: unsigned(marker_barrier, "marker_barrier")?,
                capture_broken_at,
                vault_gone_at,
                pulled_bytes: unsigned(pulled_bytes, "pulled_bytes")?,
                pulled_bytes_base: unsigned(pulled_bytes_base, "pulled_bytes_base")?,
                snapshot_uploaded_at,
                snapshot_too_large_at,
                binding_written,
                last_success_at,
                last_error,
                snapshot_barrier: unsigned(snapshot_barrier, "snapshot_barrier")?,
            })
        },
    )
    .transpose()
}

fn signed(value: u64, column: &str) -> Result<i64> {
    i64::try_from(value)
        .map_err(|_| AppError::Internal(format!("sync_state.{column} is out of range: {value}")))
}

/// Records that change capture could not be installed at `at` (the profile opens without it).
pub fn mark_capture_broken(conn: &Connection, at: i64) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET capture_broken_at = ?1 WHERE id = 1",
        [at],
    )?;
    Ok(())
}

pub fn clear_capture_broken(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET capture_broken_at = NULL WHERE id = 1",
        [],
    )?;
    Ok(())
}

/// Sets or clears the pause for a vault the relay no longer knows.
pub fn set_vault_gone(conn: &Connection, at: Option<i64>) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET vault_gone_at = ?1 WHERE id = 1",
        [at],
    )?;
    Ok(())
}

/// Advances the pull cursor. Never lowers it.
pub fn set_cursor(conn: &Connection, cursor: u64) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET cursor = MAX(cursor, ?1) WHERE id = 1",
        [signed(cursor, "cursor")?],
    )?;
    Ok(())
}

pub fn add_pulled_bytes(conn: &Connection, bytes: u64) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET pulled_bytes = pulled_bytes + ?1 WHERE id = 1",
        [signed(bytes, "pulled_bytes")?],
    )?;
    Ok(())
}

/// The vault's newest snapshot is at `vault_snapshot_seq`: when that differs from the base the
/// pulled bytes are counted from, count from it anew.
pub fn rebase_pulled_bytes(conn: &Connection, vault_snapshot_seq: u64) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET pulled_bytes_base = ?1, pulled_bytes = 0 \
         WHERE id = 1 AND pulled_bytes_base <> ?1",
        [signed(vault_snapshot_seq, "pulled_bytes_base")?],
    )?;
    Ok(())
}

pub fn set_barrier_seq(conn: &Connection, seq: Option<u64>) -> Result<()> {
    let seq = seq.map(|s| signed(s, "barrier_seq")).transpose()?;
    conn.execute("UPDATE sync_state SET barrier_seq = ?1 WHERE id = 1", [seq])?;
    Ok(())
}

/// This device skipped an envelope written below its barrier at relay sequence `position`
/// (ruling T8-2); the newest such position is kept.
pub fn note_missed_below_barrier(conn: &Connection, position: u64) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET missed_below_barrier = MAX(COALESCE(missed_below_barrier, 0), ?1) \
         WHERE id = 1",
        [signed(position, "missed_below_barrier")?],
    )?;
    Ok(())
}

/// A snapshot that covers every skipped below-barrier envelope was restored.
pub fn clear_missed_below_barrier(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET missed_below_barrier = NULL WHERE id = 1",
        [],
    )?;
    Ok(())
}

/// A marker was sealed at `barrier`; the flag never goes back.
pub fn note_sealed_at_barrier(conn: &Connection, barrier: u32) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET marker_barrier = MAX(marker_barrier, ?1) WHERE id = 1",
        [barrier],
    )?;
    Ok(())
}

/// This device uploaded a snapshot covering relay sequence `seq` and clocks up to `max_hlc`; a
/// snapshot found too large earlier no longer holds uploads back.
pub fn record_snapshot_uploaded(conn: &Connection, seq: u64, max_hlc: u64, at: i64) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET snapshot_seq = MAX(COALESCE(snapshot_seq, 0), ?1), \
           snapshot_hlc = MAX(snapshot_hlc, ?2), snapshot_uploaded_at = ?3, \
           snapshot_too_large_at = NULL, pulled_bytes = 0, pulled_bytes_base = ?1, \
           snapshot_barrier = MAX(snapshot_barrier, barrier) WHERE id = 1",
        params![
            signed(seq, "snapshot_seq")?,
            signed(max_hlc, "snapshot_hlc")?,
            at
        ],
    )?;
    Ok(())
}

/// The vault already holds a newer snapshot than the one this device offered at its barrier
/// (`409 snapshot_outdated`): the forced upload after a crossing is done (`snapshot_barrier`).
pub fn note_snapshot_barrier(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET snapshot_barrier = MAX(snapshot_barrier, barrier) WHERE id = 1",
        [],
    )?;
    Ok(())
}

/// Whether this device crossed a barrier and has not yet uploaded (or restored, or found newer
/// on the relay) a snapshot at it: its next clean round forces an upload (ruling T8-2), which
/// still waits for `chain::may_upload_snapshot`.
pub fn needs_forced_snapshot(sync: &SyncState, marks: &EngineMarks) -> bool {
    sync.enabled && sync.barrier > marks.snapshot_barrier
}

/// This device found its snapshot too large to upload at `at` (its own limit or the relay's).
pub fn record_snapshot_too_large(conn: &Connection, at: i64) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET snapshot_too_large_at = ?1 WHERE id = 1",
        [at],
    )?;
    Ok(())
}

/// A sync round finished: stamp the time and forget the last error.
pub fn record_success(conn: &Connection, at: i64) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET last_success_at = ?1, last_error = NULL WHERE id = 1",
        [at],
    )?;
    Ok(())
}

pub fn set_binding_written(conn: &Connection, written: bool) -> Result<()> {
    conn.execute(
        "UPDATE sync_state SET binding_written = ?1 WHERE id = 1",
        [written],
    )?;
    Ok(())
}

/// Starts this device's own hash chain over: the next message is sequence 1 and the log of what
/// it wrote and what is waiting to be sent is emptied.
pub fn reset_chain(conn: &Connection) -> Result<()> {
    in_savepoint(conn, "sync_reset_chain", || {
        conn.execute(
            "UPDATE sync_state SET next_seq = 1, last_hash = NULL WHERE id = 1",
            [],
        )?;
        conn.execute_batch("DELETE FROM sync_own_log; DELETE FROM sync_unsent;")?;
        Ok(())
    })
}

/// The Local `app_config` key (never synced, never captured) holding when this device first saw
/// the sync server's head below its place: a server that went back, e.g. restored from an older
/// backup (`note_vault_went_back`).
const VAULT_WENT_BACK_KEY: &str = "sync.vault_went_back_at";

/// The sync server went back below this device's place (a pull page's head below it): noted at
/// `at` (unix seconds), the first time kept. Sticky: no round, successful or not, clears it; only a
/// completed replace (`reset_engine_marks`) and turning sync off (`disable`) do. While it is set,
/// the status shows `SyncProblem::ServerWentBack` and every re-sync keeps its backup.
pub fn note_vault_went_back(conn: &Connection, at: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2) ON CONFLICT(key) DO NOTHING",
        rusqlite::params![VAULT_WENT_BACK_KEY, at.to_string()],
    )?;
    Ok(())
}

/// When the sync server was seen to go back, if it was (`note_vault_went_back`).
pub fn vault_went_back_at(conn: &Connection) -> Result<Option<i64>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [VAULT_WENT_BACK_KEY],
            |row| row.get(0),
        )
        .optional()?;
    // A value that does not parse still says it happened.
    Ok(value.map(|v| v.parse().unwrap_or(0)))
}

fn clear_vault_went_back(conn: &Connection) -> Result<()> {
    conn.execute(
        "DELETE FROM app_config WHERE key = ?1",
        [VAULT_WENT_BACK_KEY],
    )?;
    Ok(())
}

/// Clears the pauses and the counters that belong to one vault binding, and the mark of a sync
/// server that went back (a completed replace starts over from the server's state).
pub fn reset_engine_marks(conn: &Connection) -> Result<()> {
    clear_vault_went_back(conn)?;
    conn.execute(
        "UPDATE sync_state SET vault_gone_at = NULL, capture_broken_at = NULL, \
           snapshot_uploaded_at = NULL, snapshot_too_large_at = NULL, pulled_bytes = 0, \
           pulled_bytes_base = 0, \
           binding_written = 0 WHERE id = 1",
        [],
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
    fn the_schema_mark_rises_but_never_goes_down() {
        let conn = migrated();
        ensure_row(&conn, DEVICE).unwrap();
        conn.execute("UPDATE sync_state SET schema_seen = 2", [])
            .unwrap();
        note_schema_seen(&conn).unwrap();
        assert_eq!(
            load(&conn).unwrap().expect("row").schema_seen,
            schema_number()
        );

        // A newer build opened the profile, then this one: the newer mark stays, so going back
        // to that build sweeps none of the migrations it has already seen.
        conn.execute(
            "UPDATE sync_state SET schema_seen = ?1",
            [schema_number() + 3],
        )
        .unwrap();
        note_schema_seen(&conn).unwrap();
        assert_eq!(
            load(&conn).unwrap().expect("row").schema_seen,
            schema_number() + 3
        );
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
        assert!(barrier_number() > 1);
        assert_eq!(
            kept.barrier,
            barrier_number(),
            "turning a disabled row on raises its barrier to this build's"
        );
        assert_eq!(count(&conn, "SELECT snapshot_seq FROM sync_state"), 3);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_state"), 1);

        conn.execute("UPDATE sync_state SET barrier = 1", [])
            .unwrap();
        ensure_row(&conn, DEVICE).unwrap();
        assert_eq!(
            load(&conn).unwrap().expect("row").barrier,
            1,
            "an enabled row keeps its barrier until cross_barrier raises it"
        );
    }

    #[test]
    fn disable_removes_triggers_and_every_sync_row() {
        let conn = migrated();
        enable(&conn, DEVICE).unwrap();
        conn.execute_batch(
            "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
             INSERT INTO sync_heads (device, seq, hash) VALUES (x'09', 1, x'00'); \
             INSERT INTO sync_own_log (seq, hash, min_hlc, max_hlc) VALUES (1, x'00', 1, 1); \
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

    fn row_with_hash(conn: &Connection) {
        ensure_row(conn, DEVICE).unwrap();
    }

    #[test]
    fn set_cursor_never_moves_back() {
        let conn = migrated();
        row_with_hash(&conn);
        set_cursor(&conn, 10).unwrap();
        set_cursor(&conn, 4).unwrap();
        assert_eq!(load(&conn).unwrap().unwrap().cursor, 10);
        set_cursor(&conn, 12).unwrap();
        assert_eq!(load(&conn).unwrap().unwrap().cursor, 12);
    }

    #[test]
    fn pulled_bytes_reset_when_the_vault_snapshot_moves() {
        let conn = migrated();
        row_with_hash(&conn);
        let pulled = |c: &Connection| marks(c).unwrap().unwrap().pulled_bytes;
        rebase_pulled_bytes(&conn, 10).unwrap();
        add_pulled_bytes(&conn, 100).unwrap();
        add_pulled_bytes(&conn, 50).unwrap();
        assert_eq!(pulled(&conn), 150);
        rebase_pulled_bytes(&conn, 10).unwrap();
        assert_eq!(pulled(&conn), 150);
        rebase_pulled_bytes(&conn, 20).unwrap();
        add_pulled_bytes(&conn, 7).unwrap();
        assert_eq!(pulled(&conn), 7);
        assert_eq!(marks(&conn).unwrap().unwrap().pulled_bytes_base, 20);
    }

    #[test]
    fn record_snapshot_uploaded_takes_maxima_and_resets_the_counter() {
        let conn = migrated();
        row_with_hash(&conn);
        add_pulled_bytes(&conn, 99).unwrap();
        record_snapshot_uploaded(&conn, 30, 500, 1_800_000_000).unwrap();
        let snap = |c: &Connection| -> (i64, i64) {
            c.query_row(
                "SELECT snapshot_seq, snapshot_hlc FROM sync_state",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap()
        };
        assert_eq!(snap(&conn), (30, 500));
        let m = marks(&conn).unwrap().unwrap();
        assert_eq!(m.snapshot_uploaded_at, Some(1_800_000_000));
        assert_eq!((m.pulled_bytes, m.pulled_bytes_base), (0, 30));

        record_snapshot_uploaded(&conn, 20, 700, 1_800_000_100).unwrap();
        assert_eq!(
            snap(&conn),
            (30, 700),
            "seq keeps the larger, hlc the larger"
        );
        let m = marks(&conn).unwrap().unwrap();
        assert_eq!(m.snapshot_uploaded_at, Some(1_800_000_100));
        assert_eq!(m.pulled_bytes_base, 20);
    }

    #[test]
    fn a_too_large_snapshot_is_remembered_until_an_upload_or_a_reset() {
        let conn = migrated();
        row_with_hash(&conn);
        let too_large = |c: &Connection| marks(c).unwrap().unwrap().snapshot_too_large_at;
        assert_eq!(too_large(&conn), None);
        record_snapshot_too_large(&conn, 1_800_000_000).unwrap();
        assert_eq!(too_large(&conn), Some(1_800_000_000));
        record_snapshot_uploaded(&conn, 3, 3, 1_800_090_000).unwrap();
        assert_eq!(too_large(&conn), None, "an upload clears it");
        record_snapshot_too_large(&conn, 1_800_100_000).unwrap();
        reset_engine_marks(&conn).unwrap();
        assert_eq!(too_large(&conn), None, "another vault binding clears it");
    }

    #[test]
    fn the_marker_flag_never_goes_back() {
        let conn = migrated();
        row_with_hash(&conn);
        note_sealed_at_barrier(&conn, 25).unwrap();
        note_sealed_at_barrier(&conn, 24).unwrap();
        assert_eq!(marks(&conn).unwrap().unwrap().marker_barrier, 25);
    }

    #[test]
    fn reset_chain_empties_the_own_log_and_unsent() {
        let conn = migrated();
        row_with_hash(&conn);
        conn.execute(
            "UPDATE sync_state SET next_seq = 5, last_hash = ?1",
            [vec![1_u8; 32]],
        )
        .unwrap();
        conn.execute_batch(
            "INSERT INTO sync_own_log (seq, hash, min_hlc, max_hlc) VALUES (1, x'00', 1, 1); \
             INSERT INTO sync_unsent (seq, envelope) VALUES (1, x'00');",
        )
        .unwrap();
        reset_chain(&conn).unwrap();
        let state = load(&conn).unwrap().unwrap();
        assert_eq!(state.next_seq, 1);
        assert_eq!(state.last_hash, None);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_own_log"), 0);
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_unsent"), 0);
    }

    #[test]
    fn reset_engine_marks_clears_pauses_and_counters() {
        let conn = migrated();
        row_with_hash(&conn);
        mark_capture_broken(&conn, 11).unwrap();
        set_vault_gone(&conn, Some(12)).unwrap();
        add_pulled_bytes(&conn, 5).unwrap();
        record_snapshot_uploaded(&conn, 9, 9, 13).unwrap();
        set_binding_written(&conn, true).unwrap();
        record_success(&conn, 14).unwrap();
        let m = marks(&conn).unwrap().unwrap();
        assert_eq!(m.capture_broken_at, Some(11));
        assert_eq!(m.vault_gone_at, Some(12));
        assert!(m.binding_written);
        assert_eq!(m.last_success_at, Some(14));

        reset_engine_marks(&conn).unwrap();
        let m = marks(&conn).unwrap().unwrap();
        assert_eq!(m.capture_broken_at, None);
        assert_eq!(m.vault_gone_at, None);
        assert_eq!(m.snapshot_uploaded_at, None);
        assert_eq!((m.pulled_bytes, m.pulled_bytes_base), (0, 0));
        assert!(!m.binding_written);
        assert_eq!(
            m.last_success_at,
            Some(14),
            "success stamp is not a binding mark"
        );
    }

    #[test]
    fn record_success_forgets_the_error_and_clear_capture_broken_clears_it() {
        let conn = migrated();
        row_with_hash(&conn);
        record_error(&conn, "relay unreachable").unwrap();
        mark_capture_broken(&conn, 3).unwrap();
        record_success(&conn, 50).unwrap();
        clear_capture_broken(&conn).unwrap();
        set_barrier_seq(&conn, Some(8)).unwrap();
        let m = marks(&conn).unwrap().unwrap();
        assert_eq!(
            (m.last_success_at, m.last_error, m.capture_broken_at),
            (Some(50), None, None)
        );
        assert_eq!(load(&conn).unwrap().unwrap().barrier_seq, Some(8));
        set_barrier_seq(&conn, None).unwrap();
        assert_eq!(load(&conn).unwrap().unwrap().barrier_seq, None);
    }

    #[test]
    fn the_newest_missed_below_barrier_position_is_kept_until_cleared() {
        let conn = migrated();
        row_with_hash(&conn);
        assert_eq!(load(&conn).unwrap().unwrap().missed_below_barrier, None);
        note_missed_below_barrier(&conn, 7).unwrap();
        note_missed_below_barrier(&conn, 4).unwrap();
        assert_eq!(load(&conn).unwrap().unwrap().missed_below_barrier, Some(7));
        note_missed_below_barrier(&conn, 9).unwrap();
        assert_eq!(load(&conn).unwrap().unwrap().missed_below_barrier, Some(9));
        clear_missed_below_barrier(&conn).unwrap();
        assert_eq!(load(&conn).unwrap().unwrap().missed_below_barrier, None);
    }

    #[test]
    fn a_failed_capture_install_marks_the_profile_capture_broken() {
        let conn = migrated();
        ensure_row(&conn, DEVICE).unwrap();
        // Installing reads sync_clock (a missing sync_outbox would not fail: triggers resolve it lazily).
        conn.execute_batch("DROP TABLE sync_clock").unwrap();
        crate::services::sync::on_open(&conn);
        let m = marks(&conn).unwrap().unwrap();
        assert!(m.capture_broken_at.is_some_and(|at| at > 1_700_000_000));
        assert_eq!(
            m.last_error.as_deref(),
            Some(crate::services::sync::CAPTURE_BROKEN_REASON),
            "a fixed reason, never the error's text (it may name a path)"
        );
        assert!(load(&conn).unwrap().unwrap().enabled);
    }

    #[test]
    fn a_profile_without_sync_has_no_marks() {
        let conn = migrated();
        assert!(marks(&conn).unwrap().is_none());
    }
}
