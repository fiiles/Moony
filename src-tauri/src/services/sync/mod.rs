//! Device sync engine (phase 1): the registry, the hybrid logical clock, `sync_state` and the
//! trigger capture installed at unlock, the journal that turns the outbox into changesets, and
//! the merge of other devices' changes and the repairs after it.

pub mod capture;
pub mod changeset;
pub mod hlc;
pub mod journal;
pub mod merge;
pub mod natural_id;
pub mod registry;
pub mod repair;
pub mod state;

#[cfg(test)]
mod probe;
#[cfg(test)]
pub(crate) mod test_support;

use rusqlite::Connection;

use crate::error::Result;

/// Called by `Database::open_with_key` after the migrations and before the self-heals. A profile
/// without sync costs one `SELECT` and gets nothing installed (G5). Never fails: an error is
/// logged and recorded in `sync_state`, and the profile opens without capture.
pub fn on_open(conn: &Connection) {
    if let Err(e) = open_capture(conn) {
        log::warn!("[SYNC] Change capture is off for this session: {e}");
        if let Err(record) = state::record_error(conn, &e.to_string()) {
            log::warn!("[SYNC] Could not record the capture error: {record}");
        }
    }
}

fn open_capture(conn: &Connection) -> Result<()> {
    let Some(sync) = state::load(conn)? else {
        return Ok(());
    };
    if !sync.enabled {
        return Ok(());
    }
    state::note_schema_seen(conn)?;
    let revived = repair::delete_revived_seeds(conn)?;
    if revived > 0 {
        log::info!("[SYNC] Deleted {revived} seeded rows that another device had deleted");
    }
    capture::install(conn, sync.device)
}

/// Runs `work` inside `SAVEPOINT <name>`, rolled back when it fails. Savepoints nest, so this
/// composes with an outer transaction.
pub(crate) fn in_savepoint<T>(
    conn: &Connection,
    name: &str,
    work: impl FnOnce() -> Result<T>,
) -> Result<T> {
    conn.execute_batch(&format!("SAVEPOINT {name}"))?;
    match work() {
        Ok(value) => {
            conn.execute_batch(&format!("RELEASE {name}"))?;
            Ok(value)
        }
        Err(error) => {
            conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}"))?;
            Err(error)
        }
    }
}
