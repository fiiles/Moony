//! The guard of a profile whose change capture failed (audit H2, ADR 0027): "capture or guard".
//! An enabled profile either captures every change of its synced data or refuses to change it,
//! so a repair or re-sync, which replaces that data with the vault's, never discards an edit
//! nobody recorded.
//!
//! The guard lives on the connection, never in a table, so no transaction can roll it back:
//!
//! - an authorizer (`sqlite3_set_authorizer`) refuses, while a statement is prepared, inserts and
//!   deletes on synced tables of `main` and updates of their synced columns (`decide`); setting
//!   it expires every prepared statement, so cached ones are checked again;
//! - the marker function `moony_sync_guard()` says that the guard is on (`is_imposed`);
//! - `app_config` holds synced and local keys in one table and the authorizer sees no keys, so
//!   three `TEMP` triggers refuse the synced keys (`APP_CONFIG_SYNCED_KEYS`). Their names start
//!   with `sync_`, so a capture install replaces them as it replaces its own triggers. When they
//!   cannot be created, the authorizer refuses every write to `app_config` instead.
//!
//! Local columns, local tables, `sync_devices` (only sync code writes it), the `temp` schema, the
//! attachment store (`files`) and the target of `VACUUM INTO` stay writable, so settings, sync's
//! own bookkeeping, a backup, a repair and turning sync off keep working.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rusqlite::functions::FunctionFlags;
use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::Connection;

use super::super::in_savepoint;
use super::super::registry::{self, ColumnClass, TableClass, APP_CONFIG_SYNCED_KEYS};
use crate::error::{AppError, Result};

/// A write the guard refused: the person repairs sync, or turns it off, to save changes again.
pub const CHANGES_PAUSED: &str = "validation.syncChangesPaused";
/// The repair could not install capture again; turning sync off is the way on.
pub const CAPTURE_STILL_BROKEN: &str = "validation.syncCaptureStillBroken";
/// The text of the guard trigger's RAISE, mapped to CHANGES_PAUSED by `error.rs`.
pub const GUARD_RAISE: &str = "moony sync guard";

const MARKER: &str = "moony_sync_guard";
/// The guard's triggers on `main.app_config`, one per event.
const TRIGGERS: [&str; 3] = [
    "sync_guard_app_config_insert",
    "sync_guard_app_config_update",
    "sync_guard_app_config_delete",
];

/// Outside any transaction only (`conn.is_autocommit()`, else `AppError::Internal`). In this order:
/// the authorizer (a connection setting, not transactional; setting it expires every prepared
/// statement, so cached ones are re-checked); the marker function `moony_sync_guard()`
/// (`create_scalar_function`); then, best effort, the TEMP triggers `sync_guard_app_config_*`
/// (BEFORE INSERT/UPDATE/DELETE ON main.app_config, RAISE(ABORT, GUARD_RAISE) when the old or new
/// key is one of `APP_CONFIG_SYNCED_KEYS`). When the triggers cannot be created the authorizer
/// refuses every write to `app_config` instead. Imposing again replaces the guard.
pub fn impose(conn: &Connection) -> Result<()> {
    outside_transaction(conn, "imposed")?;
    // Closed until the triggers exist: a statement prepared meanwhile never slips through.
    let whole_app_config = Arc::new(AtomicBool::new(true));
    let flag = Arc::clone(&whole_app_config);
    conn.authorizer(Some(move |ctx: AuthContext<'_>| {
        decide(&ctx.action, ctx.database_name, flag.load(Ordering::SeqCst))
    }))?;
    conn.create_scalar_function(
        MARKER,
        0,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |_| Ok(1),
    )?;
    match in_savepoint(conn, "sync_guard_triggers", || {
        conn.execute_batch(&trigger_sql())?;
        Ok(())
    }) {
        Ok(()) => whole_app_config.store(false, Ordering::SeqCst),
        Err(error) => {
            log::warn!("[SYNC] The guard closes all of app_config: its triggers failed: {error}");
        }
    }
    log::warn!("[SYNC] Change capture is off: changes to synced data are refused");
    Ok(())
}

/// Outside any transaction only. Removes the marker function, the authorizer and the triggers; a
/// no-op when none is imposed. A failure part way leaves the guard stricter, never looser: the
/// marker goes first, the triggers last.
pub fn lift(conn: &Connection) -> Result<()> {
    outside_transaction(conn, "lifted")?;
    conn.remove_function(MARKER, 0)?;
    conn.authorizer(None::<fn(AuthContext<'_>) -> Authorization>)?;
    let drops: String = TRIGGERS
        .iter()
        .map(|name| format!("DROP TRIGGER IF EXISTS temp.{name};"))
        .collect();
    conn.execute_batch(&drops)?;
    Ok(())
}

/// Whether the marker function is registered on this connection (never stored in a table).
pub fn is_imposed(conn: &Connection) -> bool {
    conn.prepare(&format!("SELECT {MARKER}()")).is_ok()
}

/// Pure decision, tested directly. Denies only in schema `main`: INSERT and DELETE on a Synced
/// table, UPDATE of a column whose class is not Local; never `sync_devices` (only sync code writes
/// it); `app_config` only when `whole_app_config`. Everything else (reads, Local and Seed tables,
/// `temp`, `files`, the target of `VACUUM INTO`) is allowed.
pub(crate) fn decide(
    action: &AuthAction<'_>,
    database: Option<&str>,
    whole_app_config: bool,
) -> Authorization {
    if database != Some("main") {
        return Authorization::Allow;
    }
    let (table, column) = match *action {
        AuthAction::Insert { table_name } | AuthAction::Delete { table_name } => (table_name, None),
        AuthAction::Update {
            table_name,
            column_name,
        } => (table_name, Some(column_name)),
        _ => return Authorization::Allow,
    };
    let refused = match table {
        "sync_devices" => false,
        "app_config" => whole_app_config,
        _ => {
            registry::spec(table).is_some_and(|spec| spec.class == TableClass::Synced)
                && column.is_none_or(|c| registry::column_class(table, c) != ColumnClass::Local)
        }
    };
    if refused {
        Authorization::Deny
    } else {
        Authorization::Allow
    }
}

fn outside_transaction(conn: &Connection, what: &str) -> Result<()> {
    if conn.is_autocommit() {
        Ok(())
    } else {
        Err(AppError::Internal(format!(
            "the sync guard cannot be {what} inside a transaction"
        )))
    }
}

fn trigger_sql() -> String {
    let keys = APP_CONFIG_SYNCED_KEYS
        .iter()
        .map(|key| format!("'{}'", key.replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(", ");
    let raise = format!("BEGIN SELECT RAISE(ABORT, '{GUARD_RAISE}'); END;");
    let [insert, update, delete] = TRIGGERS;
    format!(
        "DROP TRIGGER IF EXISTS temp.{insert}; DROP TRIGGER IF EXISTS temp.{update}; \
         DROP TRIGGER IF EXISTS temp.{delete}; \
         CREATE TEMP TRIGGER {insert} BEFORE INSERT ON main.app_config \
           WHEN NEW.key IN ({keys}) {raise} \
         CREATE TEMP TRIGGER {update} BEFORE UPDATE ON main.app_config \
           WHEN OLD.key IN ({keys}) OR NEW.key IN ({keys}) {raise} \
         CREATE TEMP TRIGGER {delete} BEFORE DELETE ON main.app_config \
           WHEN OLD.key IN ({keys}) {raise}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::backup;
    use crate::services::sync::capture::{self, install_or_guard};
    use crate::services::sync::engine::tests::TestConn;
    use crate::services::sync::hlc::DeviceId;
    use crate::services::sync::setup::turn_off_between_rounds;
    use crate::services::sync::state;
    use crate::services::sync::test_support::migrated;
    use crate::services::sync::{guard_or_close, on_open};

    const DEVICE: DeviceId = DeviceId([5; 16]);

    fn count(conn: &Connection, sql: &str) -> i64 {
        conn.query_row(sql, [], |r| r.get(0)).expect("count")
    }

    /// `sql` is refused as a paused change, and nothing of it is written.
    fn refused(conn: &Connection, sql: &str) {
        let error: AppError = conn
            .execute_batch(sql)
            .expect_err(&format!("refused: {sql}"))
            .into();
        assert!(
            matches!(&error, AppError::Validation(key) if key == CHANGES_PAUSED),
            "{sql}: {error:?}"
        );
    }

    fn allowed(conn: &Connection, sql: &str) {
        conn.execute_batch(sql)
            .unwrap_or_else(|e| panic!("allowed: {sql}: {e}"));
    }

    fn insert_institution(conn: &Connection, id: &str) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO institutions (id, name) VALUES (?1, 'Bank')",
            [id],
        )
        .map(|_| ())
    }

    fn institution_refused(conn: &Connection, id: &str) {
        let error: AppError = insert_institution(conn, id)
            .expect_err("an insert into institutions is refused")
            .into();
        assert!(
            matches!(&error, AppError::Validation(key) if key == CHANGES_PAUSED),
            "{error:?}"
        );
    }

    /// An enabled profile with its capture installed, as an unlock leaves it.
    fn enabled() -> Connection {
        let conn = migrated();
        state::enable(&conn, DEVICE).expect("enable");
        conn
    }

    #[test]
    fn the_guard_refuses_synced_data_and_allows_local_data() {
        let conn = migrated();
        allowed(
            &conn,
            "INSERT INTO institutions (id, name) VALUES ('i1', 'Bank'); \
             INSERT INTO user_profile (id, name, surname, email) VALUES (1, 'A', 'B', 'a@b.c'); \
             INSERT INTO app_config (key, value) VALUES ('rule_packs_enabled', '[]'); \
             INSERT INTO app_config (key, value) VALUES ('history_rebuild_version', '1');",
        );
        state::ensure_row(&conn, DEVICE).expect("sync row");

        impose(&conn).expect("impose");

        assert!(is_imposed(&conn));
        refused(
            &conn,
            "INSERT INTO institutions (id, name) VALUES ('i2', 'Bank')",
        );
        refused(
            &conn,
            "UPDATE institutions SET name = 'New' WHERE id = 'i1'",
        );
        refused(&conn, "DELETE FROM institutions WHERE id = 'i1'");
        refused(&conn, "UPDATE user_profile SET name = 'C'");
        refused(&conn, "UPDATE user_profile SET currency = 'EUR'");
        refused(
            &conn,
            "INSERT INTO app_config (key, value) VALUES ('rule_packs_disabled_rules', '[]')",
        );
        refused(
            &conn,
            "UPDATE app_config SET value = '[1]' WHERE key = 'rule_packs_enabled'",
        );
        refused(
            &conn,
            "UPDATE app_config SET key = 'milestones.mutedKinds' WHERE key = 'history_rebuild_version'",
        );
        refused(
            &conn,
            "DELETE FROM app_config WHERE key = 'rule_packs_enabled'",
        );
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM institutions WHERE id GLOB 'i[0-9]'"
            ),
            1,
            "nothing of a refused write landed"
        );

        allowed(&conn, "UPDATE user_profile SET theme = 'dark'");
        allowed(&conn, "UPDATE user_profile SET language = 'cs'");
        allowed(
            &conn,
            "UPDATE user_profile SET mcp_server_enabled = 1, mcp_server_token = 't', \
             mcp_server_port = 4242",
        );
        allowed(
            &conn,
            "INSERT INTO app_config (key, value) VALUES ('backup.lastCreatedAt', '1'); \
             UPDATE app_config SET value = '2' WHERE key = 'backup.lastCreatedAt'; \
             INSERT INTO app_config (key, value) VALUES ('history_rebuild_version', '3') \
               ON CONFLICT(key) DO UPDATE SET value = excluded.value; \
             DELETE FROM app_config WHERE key = 'backup.lastCreatedAt';",
        );
        allowed(&conn, "UPDATE sync_state SET cursor = 7 WHERE id = 1");
        allowed(
            &conn,
            "INSERT INTO mcp_pending_refresh (id, domain, generation) VALUES ('m', 'stocks', 'g')",
        );
        allowed(
            &conn,
            "INSERT INTO sync_devices (id, name, platform, app_version, joined_at) \
               VALUES ('d1', 'Phone', 'ios', '1.0.0', 1); \
             DELETE FROM sync_devices WHERE id = 'd1';",
        );
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM institutions WHERE id GLOB 'i[0-9]'"
            ),
            1
        );
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM app_config WHERE key = 'history_rebuild_version'"
            ),
            1
        );
        let theme: String = conn
            .query_row("SELECT theme FROM user_profile", [], |r| r.get(0))
            .expect("theme");
        assert_eq!(theme, "dark");
    }

    #[test]
    fn decide_closes_the_whole_app_config_without_its_trigger() {
        let insert = AuthAction::Insert {
            table_name: "app_config",
        };
        assert_eq!(decide(&insert, Some("main"), true), Authorization::Deny);
        assert_eq!(decide(&insert, Some("main"), false), Authorization::Allow);

        let institutions = AuthAction::Insert {
            table_name: "institutions",
        };
        assert_eq!(
            decide(&institutions, Some("main"), false),
            Authorization::Deny
        );
        for other in ["temp", "files", "vacuum_db"] {
            assert_eq!(
                decide(&institutions, Some(other), true),
                Authorization::Allow,
                "{other}"
            );
        }
        let devices = AuthAction::Delete {
            table_name: "sync_devices",
        };
        assert_eq!(decide(&devices, Some("main"), true), Authorization::Allow);
        let seed = AuthAction::Insert {
            table_name: "exchange_rates",
        };
        assert_eq!(decide(&seed, Some("main"), true), Authorization::Allow);
        let read = AuthAction::Read {
            table_name: "institutions",
            column_name: "name",
        };
        assert_eq!(decide(&read, Some("main"), true), Authorization::Allow);
    }

    #[test]
    fn the_guard_survives_a_rolled_back_savepoint() {
        let conn = migrated();
        impose(&conn).expect("impose");

        allowed(&conn, "SAVEPOINT s; ROLLBACK TO s; RELEASE s");

        assert!(is_imposed(&conn));
        institution_refused(&conn, "i1");
    }

    #[test]
    fn impose_and_lift_refuse_inside_a_transaction() {
        let conn = migrated();
        allowed(&conn, "BEGIN");
        assert!(matches!(impose(&conn), Err(AppError::Internal(_))));
        assert!(!is_imposed(&conn));
        insert_institution(&conn, "i1").expect("still allowed");
        allowed(&conn, "COMMIT");

        impose(&conn).expect("impose");
        allowed(&conn, "BEGIN");
        assert!(matches!(lift(&conn), Err(AppError::Internal(_))));
        assert!(is_imposed(&conn));
        institution_refused(&conn, "i2");
        allowed(&conn, "ROLLBACK");
        institution_refused(&conn, "i3");
    }

    #[test]
    fn a_statement_cached_before_the_guard_is_refused_after_it() {
        let conn = migrated();
        let sql = "INSERT INTO institutions (id, name) VALUES (?1, 'Bank')";
        conn.prepare_cached(sql)
            .expect("prepare")
            .execute(["i1"])
            .expect("before the guard");

        impose(&conn).expect("impose");

        let error: AppError = conn
            .prepare_cached(sql)
            .and_then(|mut stmt| stmt.execute(["i2"]))
            .expect_err("re-checked")
            .into();
        assert!(
            matches!(&error, AppError::Validation(key) if key == CHANGES_PAUSED),
            "{error:?}"
        );
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM institutions WHERE id GLOB 'i[0-9]'"
            ),
            1
        );
    }

    #[test]
    fn lifting_the_guard_lets_writes_through() {
        let conn = migrated();
        lift(&conn).expect("a no-op without a guard");
        impose(&conn).expect("impose");

        lift(&conn).expect("lift");

        assert!(!is_imposed(&conn));
        insert_institution(&conn, "i1").expect("allowed again");
        allowed(
            &conn,
            "INSERT INTO app_config (key, value) VALUES ('rule_packs_enabled', '[]')",
        );
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM sqlite_temp_master WHERE type = 'trigger'"
            ),
            0
        );
    }

    #[test]
    fn a_guarded_profile_can_still_be_backed_up() {
        let conn = migrated();
        let dir = tempfile::tempdir().expect("dir");
        crate::db::files_schema::attach(&conn, &dir.path().join("attachments.db")).expect("store");
        insert_institution(&conn, "i1").expect("row");
        impose(&conn).expect("impose");

        for schema in ["main", "files"] {
            let out = dir.path().join(format!("{schema}-copy.db"));
            conn.execute(
                &format!("VACUUM {schema} INTO ?1"),
                [out.to_str().expect("utf-8")],
            )
            .unwrap_or_else(|e| panic!("VACUUM {schema} INTO: {e}"));
        }
        let zip = dir.path().join("backup.zip");
        backup::create_backup(&conn, dir.path(), &zip, "1.0.0").expect("backup");

        let inspection =
            backup::inspect_backup(&zip, &crate::db::known_migration_names()).expect("readable");
        assert!(!inspection.newer_than_app);
        institution_refused(&conn, "i2");
    }

    #[test]
    fn install_or_guard_imposes_the_guard_when_install_fails() {
        let conn = migrated();
        state::ensure_row(&conn, DEVICE).expect("row");
        allowed(&conn, "ALTER TABLE sync_clock RENAME TO gone");

        assert!(install_or_guard(&conn, DEVICE).is_err());

        institution_refused(&conn, "i1");
        assert!(!capture::is_installed(&conn).expect("installed"));
        assert!(is_imposed(&conn));
    }

    #[test]
    fn when_the_guard_cannot_be_imposed_the_profile_opens_read_only() {
        let conn = migrated();

        guard_or_close(&conn, |_| Err(AppError::Internal("no guard".into())));

        let query_only: i64 = conn
            .query_row("PRAGMA query_only", [], |r| r.get(0))
            .expect("pragma");
        assert_eq!(query_only, 1);
        assert!(insert_institution(&conn, "i1").is_err());
        assert!(conn
            .execute("INSERT INTO _migrations (name) VALUES ('local')", [])
            .is_err());
        assert_eq!(
            count(
                &conn,
                "SELECT COUNT(*) FROM institutions WHERE id GLOB 'i[0-9]'"
            ),
            0
        );
    }

    #[test]
    fn a_failed_capture_install_at_unlock_closes_the_synced_tables() {
        let conn = migrated();
        state::ensure_row(&conn, DEVICE).expect("row");
        // As `state::tests::a_failed_capture_install_marks_the_profile_capture_broken` breaks it.
        allowed(&conn, "DROP TABLE sync_clock");

        on_open(&conn);

        let marks = state::marks(&conn).expect("marks").expect("row");
        assert!(marks.capture_broken_at.is_some());
        assert!(is_imposed(&conn));
        institution_refused(&conn, "i1");
    }

    /// A guarded profile, two device rows; `turn_off_between_rounds` is the command's path.
    #[tokio::test]
    async fn turning_sync_off_works_on_a_guarded_profile_with_device_rows() {
        let conn = enabled();
        allowed(
            &conn,
            "INSERT INTO sync_devices (id, name, platform, app_version, joined_at) VALUES \
               ('d1', 'Mac', 'macos', '1.0.0', 1), ('d2', 'Phone', 'ios', '1.0.0', 2);",
        );
        capture::uninstall(&conn).expect("uninstall");
        impose(&conn).expect("impose");
        let device = TestConn::new(conn);
        let dir = tempfile::tempdir().expect("dir");

        turn_off_between_rounds(&device, dir.path())
            .await
            .expect("turn off");

        let conn = device.lock();
        assert!(!is_imposed(&conn));
        assert!(!capture::is_installed(&conn).expect("installed"));
        assert!(state::load(&conn).expect("state").is_none());
        assert_eq!(count(&conn, "SELECT COUNT(*) FROM sync_devices"), 0);
        insert_institution(&conn, "after").expect("writes go through");
        allowed(
            &conn,
            "INSERT INTO app_config (key, value) VALUES ('rule_packs_enabled', '[]') \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        );
        assert_eq!(
            count(&conn, "SELECT COUNT(*) FROM sync_outbox"),
            0,
            "no capture"
        );
    }

    #[tokio::test]
    async fn turning_sync_off_clears_query_only() {
        let conn = enabled();
        capture::uninstall(&conn).expect("uninstall");
        allowed(&conn, "PRAGMA query_only = 1");
        let device = TestConn::new(conn);
        let dir = tempfile::tempdir().expect("dir");

        turn_off_between_rounds(&device, dir.path())
            .await
            .expect("turn off");

        let conn = device.lock();
        insert_institution(&conn, "after").expect("writes go through");
        assert!(state::load(&conn).expect("state").is_none());
    }

    /// A failed turn-off puts the guard back on a profile that had it.
    #[tokio::test]
    async fn a_failed_turn_off_keeps_the_guard() {
        let conn = enabled();
        capture::uninstall(&conn).expect("uninstall");
        allowed(
            &conn,
            "CREATE TRIGGER fail_turn_off BEFORE DELETE ON sync_state \
             BEGIN SELECT RAISE(ABORT, 'injected'); END;",
        );
        impose(&conn).expect("impose");
        let device = TestConn::new(conn);
        let dir = tempfile::tempdir().expect("dir");

        assert!(turn_off_between_rounds(&device, dir.path()).await.is_err());

        let conn = device.lock();
        assert!(is_imposed(&conn));
        institution_refused(&conn, "i1");
        assert!(state::load(&conn).expect("state").is_some());
    }
}
