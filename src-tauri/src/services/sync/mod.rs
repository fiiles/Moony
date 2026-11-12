//! Device sync engine (phase 1): the registry, the hybrid logical clock, `sync_state` and the
//! trigger capture installed at unlock, the journal that turns the outbox into changesets, and
//! the merge of other devices' changes with the repairs after it, and the chain bookkeeping that
//! judges every pulled envelope, and the snapshots a joining or lagging device restores. Phase 2
//! adds the vault's credentials and this device's key pair and `sync_devices` row, and the
//! engine that pulls from the relay, and the restore marker and device binding that make a copied
//! or restored database re-join at unlock, and joining a vault by replace with the repair of a
//! capture-broken profile (`join`), and first-device setup, turning sync off and deleting the
//! synced data (`setup`).

pub mod backups;
pub mod binding;
pub mod capture;
pub mod chain;
pub mod changeset;
#[cfg(test)]
mod convergence;
pub mod devices;
pub mod engine;
pub mod envelope;
pub mod error;
pub mod feature;
pub mod hlc;
pub mod impact;
pub mod join;
pub mod journal;
pub mod keys;
pub mod kit_file;
pub mod merge;
pub mod natural_id;
pub mod pairing;
pub mod recovery;
pub mod registry;
pub mod relay;
// `dev_grant` exists only in debug builds (ruling P18), so the e2e tests do too.
#[cfg(all(test, debug_assertions))]
mod relay_e2e;
#[cfg(all(test, debug_assertions))]
mod relay_e2e_pairing;
pub mod repair;
pub mod setup;
pub mod snapshot;
pub mod state;

#[cfg(test)]
mod probe;
#[cfg(test)]
pub(crate) mod test_support;

use rusqlite::Connection;

use crate::error::Result;

/// The `last_error` of a profile whose change capture could not be installed: a fixed text, never
/// the error's own (it may name a path).
pub const CAPTURE_BROKEN_REASON: &str = "change capture could not be installed";

/// Called by `Database::open_with_key` after the migrations and before the self-heals. A profile
/// without sync costs one `SELECT` and gets nothing installed (G5). Never fails: an error is
/// logged and recorded in `sync_state`, and an enabled profile is marked capture-broken and opens
/// guarded (`capture::guard`: changes to synced data are refused, local ones go through), or
/// read-only when the guard cannot be imposed (`guard_or_close`).
///
/// The invariant (audit H2, ADR 0027): an enabled profile's connection has capture installed, or
/// the guard imposed, or `query_only` on — after `on_open`, after any failed join, repair,
/// re-sync or turn-off, and after a successful one (capture). Turning sync off removes all three.
/// So a repair or re-sync, which replaces the synced data, never discards an edit nobody
/// recorded.
pub fn on_open(conn: &Connection) {
    if let Err(e) = open_capture(conn) {
        log::warn!("[SYNC] Change capture is off for this session: {e}");
        if let Err(record) = state::record_error(conn, CAPTURE_BROKEN_REASON) {
            log::warn!("[SYNC] Could not record the capture error: {record}");
        }
        // Only an enabled profile has lost anything; the row is a no-op update otherwise. A row
        // that exists but does not load may be an enabled profile's, so it is guarded too.
        let enabled = match state::load(conn) {
            Ok(sync) => sync.is_some_and(|s| s.enabled),
            Err(_) => conn
                .query_row("SELECT EXISTS (SELECT 1 FROM sync_state)", [], |r| r.get(0))
                .unwrap_or(true),
        };
        if enabled {
            if let Err(record) = state::mark_capture_broken(conn, now()) {
                log::warn!("[SYNC] Could not mark the capture as broken: {record}");
            }
            guard_or_close(conn, capture::guard::impose);
        }
    }
}

/// The last resort of `on_open` (and of any failure that leaves an enabled profile without
/// capture): `impose`, else `PRAGMA query_only = 1` (logged at warn), so no change goes
/// unrecorded.
fn guard_or_close(conn: &Connection, impose: impl FnOnce(&Connection) -> Result<()>) {
    if let Err(error) = impose(conn) {
        log::warn!("[SYNC] The sync guard could not be imposed; the profile is read-only: {error}");
        if let Err(pragma) = conn.execute_batch("PRAGMA query_only = 1") {
            log::error!("[SYNC] Could not make the profile read-only: {pragma}");
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
    // A capture marked broken may have lost the deletes the sweep reads, and `schema_seen` stays
    // put so the next clean unlock still sweeps what those migrations seeded.
    let capture_ok = state::marks(conn)?.is_none_or(|m| m.capture_broken_at.is_none());
    if capture_ok {
        let seeded = registry::keys::seeded_tables(sync.schema_seen);
        let revived = repair::delete_revived_seeds(conn, &seeded)?;
        if revived > 0 {
            log::info!("[SYNC] Deleted {revived} seeded rows that another device had deleted");
        }
    }
    let requeued = chain::cross_barrier(conn, &sync)?;
    if requeued > 0 {
        log::info!("[SYNC] Crossed a barrier: queued {requeued} own rows again");
    }
    if capture_ok {
        state::note_schema_seen(conn)?;
    }
    capture::install(conn, sync.device)?;
    // Capture records every change now; a guard from an earlier open of this connection goes.
    if capture::guard::is_imposed(conn) {
        capture::guard::lift(conn)?;
    }
    Ok(())
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
