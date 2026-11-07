//! Device sync engine (phase 1): the registry, the hybrid logical clock, `sync_state` and the
//! trigger capture installed at unlock, the journal that turns the outbox into changesets, and
//! the merge of other devices' changes with the repairs after it, and the chain bookkeeping that
//! judges every pulled envelope, and the snapshots a joining or lagging device restores. Phase 2
//! adds the vault's credentials and this device's key pair and `sync_devices` row.

pub mod capture;
pub mod chain;
pub mod changeset;
#[cfg(test)]
mod convergence;
pub mod devices;
pub mod envelope;
pub mod hlc;
pub mod journal;
pub mod keys;
pub mod merge;
pub mod natural_id;
pub mod recovery;
pub mod registry;
pub mod repair;
pub mod snapshot;
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
        // Only an enabled profile has lost anything; the row is a no-op update otherwise.
        let enabled = state::load(conn).ok().flatten().is_some_and(|s| s.enabled);
        if enabled {
            if let Err(record) = state::mark_capture_broken(conn, now()) {
                log::warn!("[SYNC] Could not mark the capture as broken: {record}");
            }
        }
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn open_capture(conn: &Connection) -> Result<()> {
    let Some(sync) = state::load(conn)? else {
        return Ok(());
    };
    if !sync.enabled {
        return Ok(());
    }
    let revived = repair::delete_revived_seeds(conn)?;
    if revived > 0 {
        log::info!("[SYNC] Deleted {revived} seeded rows that another device had deleted");
    }
    let requeued = chain::cross_barrier(conn, &sync)?;
    if requeued > 0 {
        log::info!("[SYNC] Crossed a barrier: queued {requeued} own rows again");
    }
    state::note_schema_seen(conn)?;
    capture::install(conn, sync.device)
}

/// Runs `work` inside `SAVEPOINT <name>`, rolled back when it fails. Savepoints nest, so this
/// composes with an outer transaction. When the `RELEASE` itself fails (the outermost savepoint
/// commits, and a deferred foreign key check refuses the commit), the savepoint is rolled back
/// too, so the connection never stays inside a transaction it did not open; the error is the
/// `RELEASE`'s.
pub(crate) fn in_savepoint<T>(
    conn: &Connection,
    name: &str,
    work: impl FnOnce() -> Result<T>,
) -> Result<T> {
    conn.execute_batch(&format!("SAVEPOINT {name}"))?;
    let error = match work() {
        Ok(value) => match conn.execute_batch(&format!("RELEASE {name}")) {
            Ok(()) => return Ok(value),
            Err(release) => release.into(),
        },
        Err(error) => error,
    };
    if let Err(rollback) = conn.execute_batch(&format!("ROLLBACK TO {name}; RELEASE {name}")) {
        log::warn!("[SYNC] Could not roll back savepoint {name}: {rollback}");
    }
    Err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppError;

    #[test]
    fn a_failed_release_rolls_the_savepoint_back() {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute_batch(
            "PRAGMA foreign_keys = ON; \
             CREATE TABLE parent (id TEXT PRIMARY KEY); \
             CREATE TABLE child (id TEXT PRIMARY KEY, \
               parent_id TEXT REFERENCES parent(id) DEFERRABLE INITIALLY DEFERRED);",
        )
        .expect("schema");

        let result = in_savepoint(&conn, "sync_test", || {
            conn.execute_batch(
                "INSERT INTO parent (id) VALUES ('kept-out'); \
                 INSERT INTO child (id, parent_id) VALUES ('c', 'missing');",
            )?;
            Ok(())
        });

        let error = result.expect_err("the deferred foreign key refuses the commit");
        assert!(matches!(error, AppError::Database(_)), "{error:?}");
        assert!(conn.is_autocommit(), "no transaction is left open");
        let rows: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM parent) + (SELECT COUNT(*) FROM child)",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(rows, 0, "nothing was written");
    }
}
