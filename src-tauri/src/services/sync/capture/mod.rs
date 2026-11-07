//! Change capture (spec §6.1–§6.3): per-connection `TEMP` triggers record clocks in `sync_clock`
//! and queue rows in `sync_outbox`. Installed at unlock only for a sync-enabled profile, so a
//! profile without sync carries no triggers at all (G5).
//!
//! `install` registers the clock functions, creates `temp.sync_mode` (the connection's mode:
//! `local` for ordinary writes, `merge` while remote changes are applied, `repair` for the
//! recalculations after a merge) and then the triggers of `triggers::trigger_sql`.

pub mod triggers;

use std::sync::{Arc, Mutex};

use rusqlite::functions::FunctionFlags;
use rusqlite::{params, Connection};

use super::hlc::{now_ms, unpack, Clock, DeviceId, HlcState, Observation};
use super::in_savepoint;
use crate::error::{AppError, Result};

pub use triggers::CLOCK_UPSERT;

/// What the triggers do with a write on this connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// An ordinary write: clocked and queued.
    Local,
    /// The engine applies remote changes: only cascaded tombstones are recorded, nothing is queued.
    /// Every DELETE in this mode RAISEs unless `set_cascade_clock` was called first: the deleted
    /// row and every row its FK cascades reach are tombstoned at that clock (the parent's
    /// tombstone, D13). The merge sets it before each of its deletes (`merge::parents::delete_row`).
    Merge,
    /// Recalculations and repairs after a merge: nothing is recorded.
    Repair,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Local => "local",
            Mode::Merge => "merge",
            Mode::Repair => "repair",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "local" => Ok(Mode::Local),
            "merge" => Ok(Mode::Merge),
            "repair" => Ok(Mode::Repair),
            other => Err(AppError::Internal(format!("unknown sync mode {other}"))),
        }
    }
}

/// Installs capture on this connection: the clock functions, `temp.sync_mode` in mode `local`,
/// and the triggers. The clock starts at the newest stored clock, so this runs after any snapshot
/// restore. Installing again replaces the previous installation.
pub fn install(conn: &Connection, device: DeviceId) -> Result<()> {
    in_savepoint(conn, "sync_capture_install", || {
        drop_installation(conn)?;
        let last: i64 =
            conn.query_row("SELECT COALESCE(MAX(hlc), 0) FROM sync_clock", [], |r| {
                r.get(0)
            })?;
        register_functions(conn, HlcState::new(u64::try_from(last).unwrap_or(0)))?;
        conn.execute_batch(
            "CREATE TEMP TABLE sync_mode (id INTEGER PRIMARY KEY CHECK (id = 1), mode TEXT NOT NULL, \
               cascade_hlc INTEGER, cascade_device BLOB, local_hlc INTEGER, local_owner TEXT); \
             INSERT INTO sync_mode (id, mode) VALUES (1, 'local');",
        )?;
        for sql in triggers::trigger_sql(conn, device)? {
            conn.execute_batch(&sql)?;
        }
        Ok(())
    })
}

/// Removes the triggers and `temp.sync_mode`. The clock functions stay registered until the
/// connection closes; nothing calls them once the triggers are gone.
pub fn uninstall(conn: &Connection) -> Result<()> {
    in_savepoint(conn, "sync_capture_uninstall", || drop_installation(conn))
}

fn drop_installation(conn: &Connection) -> Result<()> {
    let names: Vec<String> = conn
        .prepare(
            "SELECT name FROM sqlite_temp_master WHERE type = 'trigger' \
             AND name LIKE 'sync\\_%' ESCAPE '\\'",
        )?
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    for name in names {
        conn.execute_batch(&format!(
            "DROP TRIGGER IF EXISTS temp.\"{}\"",
            name.replace('"', "\"\"")
        ))?;
    }
    conn.execute_batch("DROP TABLE IF EXISTS temp.sync_mode")?;
    Ok(())
}

pub fn is_installed(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sqlite_temp_master WHERE type = 'table' AND name = 'sync_mode')",
        [],
        |r| r.get(0),
    )?)
}

/// The connection's mode; `Local` when capture is not installed.
pub fn mode(conn: &Connection) -> Result<Mode> {
    if !is_installed(conn)? {
        return Ok(Mode::Local);
    }
    let value: String = conn.query_row("SELECT mode FROM temp.sync_mode", [], |r| r.get(0))?;
    Mode::parse(&value)
}

/// Runs `f` in `mode` and restores the previous mode (and cascade clock) afterwards, whether `f`
/// succeeded or not. Without capture installed it only runs `f`.
pub fn with_mode<T>(conn: &Connection, mode: Mode, f: impl FnOnce() -> Result<T>) -> Result<T> {
    if !is_installed(conn)? {
        return f();
    }
    let (previous, cascade_hlc, cascade_device): (String, Option<i64>, Option<Vec<u8>>) = conn
        .query_row(
            "SELECT mode, cascade_hlc, cascade_device FROM temp.sync_mode",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )?;
    conn.execute("UPDATE temp.sync_mode SET mode = ?1", [mode.as_str()])?;
    let result = f();
    let restored = conn.execute(
        "UPDATE temp.sync_mode SET mode = ?1, cascade_hlc = ?2, cascade_device = ?3",
        params![previous, cascade_hlc, cascade_device],
    );
    match (result, restored) {
        (Ok(value), Ok(_)) => Ok(value),
        (Ok(_), Err(e)) => Err(e.into()),
        (Err(e), Ok(_)) => Err(e),
        (Err(e), Err(restore)) => {
            log::warn!("[SYNC] Could not restore the capture mode after an error: {restore}");
            Err(e)
        }
    }
}

/// The clock merge-mode deletes stamp on their tombstones (the parent's tombstone, D13).
pub fn set_cascade_clock(conn: &Connection, clock: Clock) -> Result<()> {
    conn.execute(
        "UPDATE temp.sync_mode SET cascade_hlc = ?1, cascade_device = ?2",
        params![sql_clock(clock.hlc)?, clock.device.0.to_vec()],
    )?;
    Ok(())
}

/// Moves the connection's clock to at least `hlc`.
pub fn observe(conn: &Connection, hlc: u64) -> Result<Observation> {
    let far: i64 = conn.query_row("SELECT sync_hlc_observe(?1)", [sql_clock(hlc)?], |r| {
        r.get(0)
    })?;
    Ok(if far == 1 {
        Observation::FarFuture {
            ahead_ms: unpack(hlc).0.saturating_sub(now_ms()),
        }
    } else {
        Observation::Ok
    })
}

/// Issues the next clock (the triggers do this themselves).
pub fn tick(conn: &Connection) -> Result<u64> {
    let hlc: i64 = conn.query_row("SELECT sync_hlc_tick()", [], |r| r.get(0))?;
    u64::try_from(hlc).map_err(|_| AppError::Internal(format!("negative clock {hlc}")))
}

fn sql_clock(hlc: u64) -> Result<i64> {
    i64::try_from(hlc).map_err(|_| AppError::Internal(format!("clock {hlc} is out of range")))
}

fn register_functions(conn: &Connection, state: HlcState) -> Result<()> {
    let state = Arc::new(Mutex::new(state));

    let tick_state = Arc::clone(&state);
    conn.create_scalar_function("sync_hlc_tick", 0, FunctionFlags::SQLITE_UTF8, move |_| {
        let issued = lock(&tick_state)?.next(now_ms());
        Ok(issued as i64)
    })?;

    let now_state = Arc::clone(&state);
    conn.create_scalar_function("sync_hlc_now", 0, FunctionFlags::SQLITE_UTF8, move |_| {
        Ok(lock(&now_state)?.last() as i64)
    })?;

    conn.create_scalar_function(
        "sync_hlc_observe",
        1,
        FunctionFlags::SQLITE_UTF8,
        move |ctx| {
            let remote: i64 = ctx.get(0)?;
            let remote = u64::try_from(remote).map_err(|_| {
                rusqlite::Error::UserFunctionError(format!("negative clock {remote}").into())
            })?;
            let observation = lock(&state)?.observe(remote, now_ms());
            Ok(i64::from(matches!(
                observation,
                Observation::FarFuture { .. }
            )))
        },
    )?;
    Ok(())
}

/// The shared clock state; a poisoned lock is an SQL error, never a panic.
fn lock(state: &Mutex<HlcState>) -> rusqlite::Result<std::sync::MutexGuard<'_, HlcState>> {
    state
        .lock()
        .map_err(|_| rusqlite::Error::UserFunctionError("the sync clock state is poisoned".into()))
}

#[cfg(test)]
mod tests;
