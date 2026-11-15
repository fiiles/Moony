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
pub mod connection_code;
#[cfg(test)]
mod convergence;
pub mod devices;
pub mod engine;
pub mod envelope;
pub mod error;
pub mod feature;
pub mod held_blobs;
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
#[cfg(test)]
mod relay_e2e;
#[cfg(test)]
mod relay_e2e_admin;
#[cfg(test)]
mod relay_e2e_pairing;
pub mod repair;
pub mod server;
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
    on_open_with(conn, feature::enabled());
}

/// `on_open` with the flag given. `feature_on` decides only the log line: an enabled profile is
/// captured (or guarded) in every build, so a build without sync never writes it uncaptured
/// (audit M12, ADR 0027). Returning early instead would let such a build edit the synced tables
/// with no trigger, and those edits would be lost to the other devices. No round, no network: the
/// engine never starts without the flag.
pub(crate) fn on_open_with(conn: &Connection, feature_on: bool) {
    if !feature_on && state::load(conn).ok().flatten().is_some_and(|s| s.enabled) {
        log::info!(
            "[SYNC] This build does not run sync; the enabled profile's changes are still recorded"
        );
    }
    if let Err(e) = open_capture(conn) {
        log::warn!("[SYNC] Change capture is off for this session: {e}");
        if let Err(record) = state::record_error(conn, CAPTURE_BROKEN_REASON) {
            log::warn!("[SYNC] Could not record the capture error: {record}");
        }
        // Only an enabled profile has lost anything; the row is a no-op update otherwise. A row
        // that exists but does not load may be an enabled profile's, so it is guarded too.
        let enabled = state::on_or_unreadable(conn).unwrap_or(true);
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
    capture::install(conn, sync.device)?;
    // A profile that synced before the documents and photos did enrols them once (ADR 0030). The
    // mark moves only after that, so a failed enrolment is tried again at the next unlock. Capture
    // is installed and records every change, so the failure is no broken capture: only logged.
    let enrol = registry::keys::migration_position(registry::keys::ATTACHMENTS_SYNCED_MIGRATION)
        .is_some_and(|position| sync.schema_seen < position);
    let mut enrolled = true;
    if enrol {
        match in_savepoint(conn, "sync_enrol_attachments", || {
            capture::enrol::enrol_unclocked(conn, sync.device, &capture::enrol::ATTACHMENT_TABLES)
        }) {
            Ok(0) => {}
            Ok(rows) => {
                log::info!("[SYNC] Queued {rows} documents and photos synced before this build");
            }
            Err(error) => {
                log::warn!(
                    "[SYNC] Could not queue the documents and photos of an earlier build \
                     (tried again at the next unlock): {error}"
                );
                enrolled = false;
            }
        }
    }
    if capture_ok && enrolled {
        state::note_schema_seen(conn)?;
    }
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
    use crate::services::sync::test_support;

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

    fn institution(conn: &Connection, id: &str) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO institutions (id, name) VALUES (?1, 'Bank')",
            [id],
        )
    }

    fn outbox_rows(conn: &Connection) -> i64 {
        conn.query_row(
            "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'institutions'",
            [],
            |r| r.get(0),
        )
        .expect("count")
    }

    /// Audit M12: a build without the flag still records an enabled profile's edits.
    #[test]
    fn a_build_without_sync_still_captures_an_enabled_profile() {
        let conn = test_support::migrated();
        state::enable(&conn, hlc::DeviceId([5; 16])).expect("enable");
        capture::uninstall(&conn).expect("uninstall");
        assert!(!capture::is_installed(&conn).unwrap());

        on_open_with(&conn, false);

        assert!(
            capture::is_installed(&conn).unwrap(),
            "capture is installed"
        );
        institution(&conn, "i1").expect("an edit");
        assert_eq!(outbox_rows(&conn), 1, "one outbox row");
    }

    #[test]
    fn a_build_without_sync_installs_nothing_on_a_profile_without_sync() {
        let conn = test_support::migrated();

        on_open_with(&conn, false);

        assert!(!capture::is_installed(&conn).unwrap());
        institution(&conn, "i1").expect("an edit");
        assert_eq!(outbox_rows(&conn), 0);
    }

    #[test]
    fn a_build_without_sync_guards_an_enabled_profile_whose_capture_fails() {
        let conn = test_support::migrated();
        state::enable(&conn, hlc::DeviceId([5; 16])).expect("enable");
        capture::uninstall(&conn).expect("uninstall");
        conn.execute_batch("DROP TABLE sync_clock").expect("break");

        on_open_with(&conn, false);

        assert!(capture::guard::is_imposed(&conn) || capture::guard::is_read_only(&conn));
        assert!(institution(&conn, "i1").is_err(), "the insert is refused");
    }

    // A profile that had sync on before the documents and photos were synced tables (ADR 0030).

    const DEVICE: hlc::DeviceId = hlc::DeviceId([5; 16]);
    const OTHER: hlc::DeviceId = hlc::DeviceId([9; 16]);
    const ATTACHMENTS: &str = "('real_estate_photo_batches', 'real_estate_photos', \
         'real_estate_documents', 'insurance_documents')";

    /// An enabled profile last opened by a build of schema `seen`, without capture (as before
    /// unlock), holding a policy document with a blob, a photo batch and a photo with both blobs,
    /// none of them clocked.
    fn opened_last_by(seen: u32) -> Connection {
        let conn = test_support::migrated();
        state::enable(&conn, DEVICE).expect("enable");
        conn.execute("UPDATE sync_state SET schema_seen = ?1", [seen])
            .expect("schema_seen");
        capture::uninstall(&conn).expect("uninstall");
        conn.execute_batch(
            "INSERT INTO insurance_policies \
               (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
             VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly'); \
             INSERT INTO insurance_documents (id, insurance_id, name, blob_id) \
             VALUES ('d1', 'p1', 'Contract', 'blob-1'); \
             INSERT INTO real_estate (id, name, address, type) \
             VALUES ('r1', 'Flat', 'Street', 'flat'); \
             INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
             VALUES ('pb1', 'r1', 1767225600); \
             INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id) \
             VALUES ('ph1', 'pb1', 'blob-2', 'blob-3');",
        )
        .expect("rows");
        conn.execute_batch("DELETE FROM sync_outbox")
            .expect("outbox");
        conn
    }

    fn before_attachments() -> u32 {
        registry::keys::migration_position(registry::keys::ATTACHMENTS_SYNCED_MIGRATION)
            .expect("known migration")
            - 1
    }

    /// The attachment rows queued, as `table/key`.
    fn queued_attachments(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT tbl || '/' || pk FROM sync_outbox WHERE tbl IN {ATTACHMENTS} \
                 ORDER BY tbl, pk"
            ))
            .expect("prepare");
        stmt.query_map([], |r| r.get(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("rows")
    }

    /// The row clock `'*'` of every attachment row, as `table/key/device`.
    fn generations(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!(
                "SELECT tbl || '/' || pk || '/' || hex(device) FROM sync_clock \
                 WHERE tbl IN {ATTACHMENTS} AND col = '*' ORDER BY tbl, pk"
            ))
            .expect("prepare");
        stmt.query_map([], |r| r.get(0))
            .expect("query")
            .collect::<rusqlite::Result<_>>()
            .expect("rows")
    }

    fn schema_seen(conn: &Connection) -> u32 {
        state::load(conn).expect("load").expect("row").schema_seen
    }

    #[test]
    fn a_profile_that_synced_before_attachments_enrols_them_once() {
        let conn = opened_last_by(before_attachments());

        on_open_with(&conn, true);

        assert_eq!(
            queued_attachments(&conn),
            vec![
                "insurance_documents/d1",
                "real_estate_photo_batches/pb1",
                "real_estate_photos/ph1"
            ]
        );
        let device = DEVICE.to_hex().to_uppercase();
        assert_eq!(
            generations(&conn),
            vec![
                format!("insurance_documents/d1/{device}"),
                format!("real_estate_photo_batches/pb1/{device}"),
                format!("real_estate_photos/ph1/{device}")
            ]
        );
        assert_eq!(schema_seen(&conn), registry::keys::schema_number());
        assert!(capture::is_installed(&conn).expect("installed"));

        conn.execute_batch("DELETE FROM sync_outbox").expect("sent");
        on_open_with(&conn, true);

        assert!(queued_attachments(&conn).is_empty(), "enrolled only once");
    }

    #[test]
    fn a_profile_set_up_after_attachments_enrols_nothing() {
        let conn = opened_last_by(registry::keys::schema_number());

        on_open_with(&conn, true);

        assert!(queued_attachments(&conn).is_empty());
        assert!(generations(&conn).is_empty());
    }

    #[test]
    fn enrolment_skips_legacy_rows_and_rows_with_a_clock() {
        let conn = opened_last_by(before_attachments());
        conn.execute_batch(&format!(
            "INSERT INTO insurance_documents (id, insurance_id, name, file_path) \
             VALUES ('legacy', 'p1', 'Old contract', 'insurance/p1/old.pdf'); \
             INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path, blob_id) \
             VALUES ('half', 'pb1', 'photos/half.jpg', 'photos/half-thumb.jpg', 'blob-4'); \
             INSERT INTO insurance_documents (id, insurance_id, name, blob_id) \
             VALUES ('received', 'p1', 'From the vault', 'blob-5'), \
                    ('left', 'p1', 'Tombstoned', 'blob-6'); \
             INSERT INTO sync_clock (tbl, pk, col, hlc, device) VALUES \
               ('insurance_documents', 'received', '*', 7, x'{other}'), \
               ('insurance_documents', 'left', '-', 8, x'{other}');",
            other = OTHER.to_hex()
        ))
        .expect("rows");

        on_open_with(&conn, true);

        assert_eq!(
            queued_attachments(&conn),
            vec![
                "insurance_documents/d1",
                "real_estate_photo_batches/pb1",
                "real_estate_photos/ph1"
            ]
        );
        let received: (i64, Vec<u8>) = conn
            .query_row(
                "SELECT hlc, device FROM sync_clock \
                 WHERE tbl = 'insurance_documents' AND pk = 'received' AND col = '*'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("clock");
        assert_eq!(
            received,
            (7, OTHER.0.to_vec()),
            "another device's clock stays"
        );
        let clocked: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sync_clock \
                 WHERE pk IN ('legacy', 'half', 'left') AND col = '*'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(clocked, 0);
    }

    /// Capture is installed and records every change, so a failed enrolment never marks it broken:
    /// the profile stays writable, the mark stays put and the next unlock tries again.
    #[test]
    fn a_failed_enrolment_is_tried_again_at_the_next_unlock() {
        let conn = opened_last_by(before_attachments());
        conn.execute_batch(
            "CREATE TRIGGER refuse_enrolment BEFORE INSERT ON sync_outbox \
             WHEN NEW.tbl = 'insurance_documents' BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .expect("trigger");

        on_open_with(&conn, true);

        assert!(queued_attachments(&conn).is_empty(), "rolled back whole");
        assert!(generations(&conn).is_empty());
        assert_eq!(schema_seen(&conn), before_attachments());
        assert!(state::marks(&conn)
            .expect("marks")
            .is_none_or(|m| m.capture_broken_at.is_none()));
        assert!(!capture::guard::is_imposed(&conn));
        institution(&conn, "i1").expect("an edit");

        conn.execute_batch("DROP TRIGGER refuse_enrolment")
            .expect("drop");
        on_open_with(&conn, true);

        assert_eq!(queued_attachments(&conn).len(), 3);
        assert_eq!(schema_seen(&conn), registry::keys::schema_number());
    }

    #[test]
    fn unsent_changes_counts_the_outbox() {
        let conn = test_support::migrated();
        state::enable(&conn, hlc::DeviceId([5; 16])).expect("enable");
        let before = engine::status::unsent_changes(&conn).expect("count");
        institution(&conn, "i1").expect("an edit");
        assert_eq!(
            engine::status::unsent_changes(&conn).expect("count"),
            before + 1
        );
    }
}
