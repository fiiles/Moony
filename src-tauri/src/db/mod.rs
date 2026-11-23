//! Database module for Moony
//!
//! Handles SQLCipher encrypted database connections and migrations

pub mod files_schema;
mod migrations;

pub use migrations::{
    applied_migration_names, known_migration_names, migration_sql, run_migrations,
};

use crate::error::{AppError, Result};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

/// Database state managed by Tauri
///
/// Wraps its internals in Arc so it can be cheaply cloned and shared between
/// Tauri commands and the local HTTP API server.
#[derive(Clone)]
pub struct Database {
    /// Mutex-protected connection (None when locked/closed)
    conn: Arc<Mutex<Option<Connection>>>,
    /// Path to the database file
    db_path: Arc<Mutex<Option<PathBuf>>>,
    /// Advanced under the connection mutex whenever a session opens or closes.
    session_epoch: Arc<AtomicU64>,
    expected_session: Option<u64>,
}

impl Database {
    /// Create a new database instance (not yet connected)
    pub fn new() -> Self {
        Self {
            conn: Arc::new(Mutex::new(None)),
            db_path: Arc::new(Mutex::new(None)),
            session_epoch: Arc::new(AtomicU64::new(0)),
            expected_session: None,
        }
    }

    // Mutex poisoning note: a panic inside any closure holding these locks used
    // to poison them, making every later `.expect()` panic too — the app then
    // appeared frozen on every screen until restart. The SQLite connection
    // itself stays valid across a Rust panic (no explicit transactions are
    // held open), so recovering the guard is safe and one bad command no
    // longer bricks the whole app.

    /// Check if the database is currently open/unlocked
    pub fn is_open(&self) -> bool {
        self.conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_some()
    }

    /// Open and unlock the database with a hex master key
    ///
    /// The key should be in format: 'hexstring' (with quotes, as expected by SQLCipher)
    pub fn open_with_key(&self, path: PathBuf, hex_key: &str) -> Result<()> {
        let mut conn = Connection::open(&path)?;

        // Set SQLCipher encryption key using hex format
        // SQLCipher expects: PRAGMA key = "x'hexstring'"
        // A plaintext copy of the key: wiped when it goes out of scope.
        let pragma_key = Zeroizing::new(format!("x{}", hex_key));
        conn.pragma_update(None, "key", pragma_key.as_str())?;

        // Verify the key works by querying sqlite_master
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .map_err(|e| {
                // The master key was already unlocked from key.enc/recovery.enc, so a
                // failure here means the database and the key files do not belong
                // together (or the database file is damaged) - not a wrong password.
                log::warn!(
                    "[AUTH] Cannot read database {} with the unlocked master key: {}",
                    path.display(),
                    e
                );
                AppError::Auth(crate::services::crypto::AUTH_KEY_FILES_CORRUPTED.into())
            })?;

        configure(&conn)?;

        // Run migrations, copying the database file first when any are pending
        //: a failed upgrade must never leave the only copy half-migrated.
        run_migrations_with_backup(&conn, &path)?;

        attach_attachments_store(&conn, &path);

        // Device sync: capture triggers for a sync-enabled profile, installed before the
        // self-heals below so what they write is captured too. One SELECT otherwise; never fails.
        crate::services::sync::on_open(&mut conn);

        // Load exchange rates from database for offline use
        if let Err(e) = crate::services::currency::load_rates_from_db(&conn) {
            log::warn!("[DB] Warning: Failed to load exchange rates: {}", e);
        }

        self_heal(&conn);

        // Store connection and path
        let mut guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.session_epoch.fetch_add(1, Ordering::SeqCst);
        *guard = Some(conn);
        *self
            .db_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);

        Ok(())
    }

    /// Create a new encrypted database with a hex master key
    ///
    /// The key should be in format: 'hexstring' (with quotes, as expected by SQLCipher)
    pub fn create_with_key(&self, path: PathBuf, hex_key: &str) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| AppError::Database(format!("Failed to create directory: {}", e)))?;
        }

        let conn = Connection::open(&path)?;

        // Set SQLCipher encryption key using hex format (a plaintext copy: wiped on drop)
        let pragma_key = Zeroizing::new(format!("x{}", hex_key));
        conn.pragma_update(None, "key", pragma_key.as_str())?;

        configure(&conn)?;

        // Run migrations to create schema
        migrations::run_migrations(&conn)?;

        attach_attachments_store(&conn, &path);

        // Note: No exchange rates to load on fresh database

        // Store connection and path
        let mut guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.session_epoch.fetch_add(1, Ordering::SeqCst);
        *guard = Some(conn);
        *self
            .db_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);

        Ok(())
    }

    /// Which session is open: advances at every open and close (lock), so a value read while one
    /// profile session was open never matches a later session.
    pub(crate) fn session_epoch(&self) -> u64 {
        self.session_epoch.load(Ordering::SeqCst)
    }

    /// Close the database connection (lock the app)
    pub fn close(&self) {
        let mut guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.session_epoch.fetch_add(1, Ordering::SeqCst);
        *guard = None;
    }

    /// Pin async work to the currently open connection session. After logout/restore,
    /// every read or write through this clone fails rather than touching a new database.
    pub(crate) fn current_session(&self) -> Result<Self> {
        let guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let epoch = self.session_epoch.load(Ordering::SeqCst);
        require_session(self.expected_session, epoch)?;
        if guard.is_none() {
            return Err(AppError::Auth("Database is locked".into()));
        }
        let mut scoped = self.clone();
        scoped.expected_session = Some(epoch);
        Ok(scoped)
    }

    /// The same database without a session pin: follows whatever session is open (a worker that
    /// outlives sessions and pins each piece of work itself, `history_recalc::schedule`).
    pub(crate) fn unpinned(&self) -> Self {
        let mut handle = self.clone();
        handle.expected_session = None;
        handle
    }

    /// Execute a function with the database connection
    /// Returns an error if the database is not open
    pub fn with_conn<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        require_session(
            self.expected_session,
            self.session_epoch.load(Ordering::SeqCst),
        )?;
        let conn = guard
            .as_ref()
            .ok_or_else(|| AppError::Auth("Database is locked".into()))?;
        f(conn)
    }

    /// `with_conn` if the connection is free right now, else `None` without waiting (a `Drop`
    /// that must never hang, even when it runs inside a hold of this connection).
    pub fn try_with_conn<F, T>(&self, f: F) -> Option<Result<T>>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let guard = match self.conn.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        let run = || {
            require_session(
                self.expected_session,
                self.session_epoch.load(Ordering::SeqCst),
            )?;
            let conn = guard
                .as_ref()
                .ok_or_else(|| AppError::Auth("Database is locked".into()))?;
            f(conn)
        };
        Some(run())
    }

    /// Execute a mutable function with the database connection
    pub fn with_conn_mut<F, T>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&mut Connection) -> Result<T>,
    {
        let mut guard = self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        require_session(
            self.expected_session,
            self.session_epoch.load(Ordering::SeqCst),
        )?;
        let conn = guard
            .as_mut()
            .ok_or_else(|| AppError::Auth("Database is locked".into()))?;
        f(conn)
    }

    /// Delete the database file (for account deletion)
    pub fn delete_database(&self) -> Result<()> {
        // Close connection first
        self.close();

        // Get and clear the path
        let path = self
            .db_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();

        if let Some(path) = path {
            std::fs::remove_file(&path)
                .map_err(|e| AppError::Database(format!("Failed to delete database: {}", e)))?;
        }

        Ok(())
    }

    /// Get the database file path
    pub fn get_path(&self) -> Option<PathBuf> {
        self.db_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

/// The connection settings every profile session runs with, once the key is set: foreign key
/// constraints, and `secure_delete` on the main database, so a deleted row (the sync secrets that
/// turning sync off deletes, audit M11) is overwritten on its page instead of lingering in free
/// space. The attachments store sets its own (`files_schema`).
pub(crate) fn configure(conn: &Connection) -> Result<()> {
    conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA main.secure_delete = ON;")?;
    Ok(())
}

/// Attach `attachments.db` next to the main file (ADR 0017). A store that cannot be opened (damaged,
/// or from another profile) must not lock the user out of their finances: the profile opens
/// without it, the file is left as it is, and every attachment call reports the store unavailable.
fn attach_attachments_store(conn: &Connection, db_path: &std::path::Path) {
    if let Err(e) = files_schema::attach(conn, &files_schema::attachments_path(db_path)) {
        log::warn!("[ATTACHMENTS] The attachments store is unavailable: {e}");
    }
}

fn require_session(expected: Option<u64>, current: u64) -> Result<()> {
    if expected.is_some_and(|epoch| epoch != current) {
        return Err(AppError::Auth("Database session changed".into()));
    }
    Ok(())
}

/// How many pre-migration copies stay next to a database once its migrations went through.
const MIGRATION_BACKUPS_TO_KEEP: usize = 1;

/// How many stay while a migration fails. Every start that finds a pending migration takes a new
/// copy, so the copy from before the update has to outlive a few retries.
const MIGRATION_BACKUPS_WHILE_FAILING: usize = 3;

/// Copy `db_path` to `<name>.bak-<unix-ts>` before the first pending migration
/// runs, then migrate. Once the chain went through only that copy stays; if a
/// step fails, the earlier copies stay too and the error names the new one.
fn run_migrations_with_backup(conn: &Connection, db_path: &std::path::Path) -> Result<()> {
    let backup = if migrations::has_pending_migrations(conn)? {
        Some(backup_database_file(db_path)?)
    } else {
        None
    };
    match migrations::run_migrations(conn) {
        Ok(()) => {
            if backup.is_some() {
                keep_newest_backups(db_path, MIGRATION_BACKUPS_TO_KEEP);
            }
            Ok(())
        }
        Err(e) => match backup {
            Some(path) => Err(AppError::Database(format!(
                "{e}. A copy of the database taken before migrating is at {}",
                path.display()
            ))),
            None => Err(e),
        },
    }
}

/// Copy the database file (and its WAL/journal side files, if any) to
/// `<file>.bak-<unix-ts>` in the same directory. Older copies are pruned only to
/// `MIGRATION_BACKUPS_WHILE_FAILING` here: the caller prunes to one with
/// `keep_newest_backups` once its migrations went through.
pub fn backup_database_file(db_path: &std::path::Path) -> Result<PathBuf> {
    let file_name = db_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| AppError::Database("Database path has no file name".into()))?;
    let dir = db_path
        .parent()
        .ok_or_else(|| AppError::Database("Database path has no parent directory".into()))?;
    let stamp = chrono::Utc::now().timestamp();
    let backup_path = dir.join(format!("{file_name}.bak-{stamp}"));

    std::fs::copy(db_path, &backup_path).map_err(|e| {
        AppError::Database(format!(
            "Failed to copy database before migration ({}): {e}",
            backup_path.display()
        ))
    })?;
    for suffix in ["-wal", "-shm", "-journal"] {
        let side = dir.join(format!("{file_name}{suffix}"));
        if side.exists() {
            let _ = std::fs::copy(&side, dir.join(format!("{file_name}.bak-{stamp}{suffix}")));
        }
    }
    log::info!(
        "[MIGRATION] Pre-migration backup: {}",
        backup_path.display()
    );

    keep_newest_backups(db_path, MIGRATION_BACKUPS_WHILE_FAILING);
    Ok(backup_path)
}

/// Delete all but the newest `keep` copies of `db_path` (`<file>.bak-<unix-ts>`, with their side
/// files). Best effort: a copy that cannot be deleted stays.
fn keep_newest_backups(db_path: &std::path::Path, keep: usize) {
    let (Some(file_name), Some(dir)) = (
        db_path.file_name().and_then(|n| n.to_str()),
        db_path.parent(),
    ) else {
        return;
    };
    let prefix = format!("{file_name}.bak-");
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut backups: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| {
                        n.starts_with(&prefix)
                            && !n.ends_with("-wal")
                            && !n.ends_with("-shm")
                            && !n.ends_with("-journal")
                    })
                    .unwrap_or(false)
            })
            .collect();
        backups.sort();
        while backups.len() > keep {
            let old = backups.remove(0);
            let _ = std::fs::remove_file(&old);
            for suffix in ["-wal", "-shm", "-journal"] {
                let _ = std::fs::remove_file(format!("{}{}", old.display(), suffix));
            }
        }
    }
}

/// `keep_newest_backups` for a database whose migrations went through.
pub fn keep_only_newest_backup(db_path: &std::path::Path) {
    keep_newest_backups(db_path, MIGRATION_BACKUPS_TO_KEEP);
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

/// The idempotent self-heals of every open; returns whether they ran. Neither blocks the open:
/// a failure is logged. A profile whose changes to synced data are paused (the sync guard, or the
/// read-only last resort: `capture::guard::changes_paused`) skips them with one log line, since
/// they write synced tables and every write would be refused; the next unlock runs them.
fn self_heal(conn: &Connection) -> bool {
    if crate::services::sync::capture::guard::changes_paused(conn) {
        log::warn!(
            "[DB] Self-heals skipped: changes to synced data are paused until sync is repaired"
        );
        return false;
    }
    // Corrects any stale investment quantity values left over from older calculations.
    if let Err(e) = crate::services::investments::recalculate_all_investment_metrics(conn) {
        log::warn!(
            "[DB] Warning: Failed to recalculate investment metrics: {}",
            e
        );
    }

    // Follow-up of migration 019: an anchor an extra payment or a rate change re-anchored
    // may hide a balance the user had entered by hand. Idempotent (a processed row is
    // deleted, a later open finds none).
    if let Err(e) =
        crate::services::loan_events::reconcile_legacy_anchors(conn, chrono::Utc::now().timestamp())
    {
        log::warn!(
            "[DB] Warning: Failed to reconcile legacy loan anchors: {}",
            e
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pinned_async_work_cannot_cross_a_close_and_reopen() {
        assert!(require_session(Some(7), 7).is_ok());
        assert!(require_session(Some(7), 8).is_err()); // close
        assert!(require_session(Some(7), 9).is_err()); // reopen, including the same path
        assert!(require_session(Some(9), 9).is_ok());
        assert!(require_session(None, 9).is_ok()); // normal app handles follow the new session
    }

    #[test]
    fn an_unpinned_handle_follows_the_next_session() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let key = crate::services::crypto::master_key_to_hex(&[9; 32]);
        let db = Database::new();
        db.create_with_key(path.clone(), &key).expect("create");
        let pinned = db.current_session().expect("pinned");
        db.close();
        db.open_with_key(path, &key).expect("reopen");
        assert!(pinned.with_conn(|_| Ok(())).is_err(), "the pin holds");
        assert!(pinned.unpinned().with_conn(|_| Ok(())).is_ok());
    }

    #[test]
    fn secure_delete_is_on_for_the_main_database() {
        let dir = tempfile::tempdir().expect("tempdir");
        let conn = Connection::open(dir.path().join("moony.db")).expect("open");
        configure(&conn).expect("configure");
        let secure: i64 = conn
            .query_row("PRAGMA main.secure_delete", [], |r| r.get(0))
            .expect("secure_delete");
        assert_eq!(secure, 1);
        let foreign: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .expect("foreign_keys");
        assert_eq!(foreign, 1);
    }

    #[test]
    fn backup_database_file_copies_and_prunes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db = dir.path().join("moony.db");
        std::fs::write(&db, b"not really sqlite").expect("write db");

        let mut made = Vec::new();
        for i in 0..5 {
            // Distinct names even within one second
            let p = dir.path().join(format!("moony.db.bak-{}", 1_000 + i));
            std::fs::write(&p, b"old").expect("old backup");
            made.push(p);
        }
        let backup = backup_database_file(&db).expect("backup");
        assert!(backup.exists());
        assert_eq!(std::fs::read(&backup).expect("read"), b"not really sqlite");

        let remaining: Vec<_> = std::fs::read_dir(dir.path())
            .expect("dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("moony.db.bak-"))
            .collect();
        assert_eq!(remaining.len(), MIGRATION_BACKUPS_WHILE_FAILING);
        assert!(!made[0].exists(), "oldest copies are pruned");
        assert!(backup.exists(), "the fresh copy is kept");
    }

    fn backup_copies(dir: &std::path::Path) -> usize {
        std::fs::read_dir(dir)
            .expect("dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with("moony.db.bak-"))
            .count()
    }

    const MIGRATIONS_TABLE: &str = "CREATE TABLE _migrations (
        id INTEGER PRIMARY KEY,
        name TEXT NOT NULL UNIQUE,
        applied_at INTEGER NOT NULL DEFAULT (unixepoch())
    );";

    #[test]
    fn pending_migration_takes_a_file_copy_before_it_runs() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let conn = Connection::open(&path).expect("open");
        // A database whose bookkeeping exists but has not recorded the baseline yet.
        conn.execute_batch(MIGRATIONS_TABLE).expect("bookkeeping");

        run_migrations_with_backup(&conn, &path).expect("migrate");

        assert_eq!(backup_copies(dir.path()), 1, "one pre-migration copy");
        assert!(
            migrations::known_migration_names().iter().all(|n| {
                migrations::applied_migration_names(&conn)
                    .expect("applied")
                    .iter()
                    .any(|a| a == n)
            }),
            "the chain ran"
        );
    }

    /// Earlier copies next to the database, as updates before this one left them.
    fn earlier_copies(dir: &std::path::Path, count: usize) -> Vec<PathBuf> {
        (0..count)
            .map(|i| {
                let p = dir.join(format!("moony.db.bak-{}", 1_000 + i));
                std::fs::write(&p, b"old").expect("earlier copy");
                p
            })
            .collect()
    }

    #[test]
    fn a_migration_that_went_through_leaves_only_its_own_copy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch(MIGRATIONS_TABLE).expect("bookkeeping");
        let earlier = earlier_copies(dir.path(), 3);

        run_migrations_with_backup(&conn, &path).expect("migrate");

        assert_eq!(backup_copies(dir.path()), 1, "one copy stays");
        assert!(
            earlier.iter().all(|p| !p.exists()),
            "the copy that stays is this update's"
        );
    }

    #[test]
    fn a_migration_that_failed_keeps_the_earlier_copies() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let conn = Connection::open(&path).expect("open");
        // A table a later migration creates: the chain stops there, after the copy was taken.
        conn.execute_batch(MIGRATIONS_TABLE).expect("bookkeeping");
        conn.execute_batch("CREATE TABLE sync_devices (x)")
            .expect("a table in the way");
        let earlier = earlier_copies(dir.path(), 2);

        run_migrations_with_backup(&conn, &path).expect_err("the chain fails");

        assert_eq!(
            backup_copies(dir.path()),
            3,
            "this run's copy and both before it"
        );
        assert!(earlier.iter().all(|p| p.exists()));
    }

    #[test]
    fn up_to_date_database_is_opened_without_a_file_copy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let conn = Connection::open(&path).expect("open");
        migrations::run_migrations(&conn).expect("first run");

        run_migrations_with_backup(&conn, &path).expect("second run");

        assert_eq!(backup_copies(dir.path()), 0);
    }

    #[test]
    fn opening_the_database_reconciles_the_legacy_loan_anchors_once() {
        const DAY: i64 = 86_400;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let key = crate::services::crypto::master_key_to_hex(&[9; 32]);
        let db = Database::new();
        db.create_with_key(path.clone(), &key).expect("create");
        let start = 1_700_000_000 / DAY * DAY;
        let day = start + 366 * DAY;
        db.with_conn(|conn| {
            // zero interest, 10 000 a month: 780 000 on `day` after the extra payment, and the
            // user's own 700 000 that migration 019 kept
            conn.execute(
                "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date)
                 VALUES ('l', 'Mortgage', '1000000', '0', '10000', ?1)",
                [start],
            )?;
            conn.execute(
                "INSERT INTO loan_events (id, loan_id, kind, event_day, amount, created_at)
                 VALUES ('e', 'l', 'extra_payment', ?1, '100000', 10)",
                [day],
            )?;
            conn.execute(
                "INSERT INTO loan_legacy_anchors (loan_id, amount, day) VALUES ('l', '700000', ?1)",
                [day],
            )?;
            Ok(())
        })
        .expect("seed");
        db.close();

        db.open_with_key(path.clone(), &key).expect("first open");
        let counts = |db: &Database| -> (i64, i64) {
            db.with_conn(|conn| {
                Ok((
                    conn.query_row(
                        "SELECT COUNT(*) FROM loan_events WHERE kind = 'balance_check'",
                        [],
                        |r| r.get(0),
                    )?,
                    conn.query_row("SELECT COUNT(*) FROM loan_legacy_anchors", [], |r| r.get(0))?,
                ))
            })
            .expect("counts")
        };
        assert_eq!(counts(&db), (1, 0), "the override became a record");

        db.close();
        db.open_with_key(path, &key).expect("second open");
        assert_eq!(counts(&db), (1, 0), "a later open changes nothing");
    }

    #[test]
    fn opening_a_sync_enabled_profile_installs_capture_before_the_self_heals() {
        use crate::services::sync::{capture, hlc::DeviceId, state};
        const DAY: i64 = 86_400;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let key = crate::services::crypto::master_key_to_hex(&[9; 32]);
        let db = Database::new();
        db.create_with_key(path.clone(), &key).expect("create");
        let start = 1_700_000_000 / DAY * DAY;
        let day = start + 366 * DAY;
        db.with_conn(|conn| {
            // a position with one transaction and a stale quantity
            conn.execute_batch(
                "INSERT INTO stock_investments (id, ticker, company_name, quantity) \
                   VALUES ('inv', 'AAPL', 'Apple', '99'); \
                 INSERT INTO investment_transactions (id, investment_id, type, ticker, company_name, \
                   quantity, price_per_unit, currency, transaction_date) \
                   VALUES ('tx', 'inv', 'buy', 'AAPL', 'Apple', '5', '100', 'USD', 0);",
            )?;
            // a legacy anchor the second self-heal turns into a balance_check event
            conn.execute(
                "INSERT INTO loans (id, name, principal, interest_rate, monthly_payment, start_date)
                 VALUES ('l', 'Mortgage', '1000000', '0', '10000', ?1)",
                [start],
            )?;
            conn.execute(
                "INSERT INTO loan_events (id, loan_id, kind, event_day, amount, created_at)
                 VALUES ('e', 'l', 'extra_payment', ?1, '100000', 10)",
                [day],
            )?;
            conn.execute(
                "INSERT INTO loan_legacy_anchors (loan_id, amount, day) VALUES ('l', '700000', ?1)",
                [day],
            )?;
            state::enable(conn, DeviceId([7; 16]))
        })
        .expect("seed and enable");
        db.close();

        db.open_with_key(path, &key).expect("open");

        db.with_conn(|conn| {
            assert!(capture::is_installed(conn)?);
            let quantity: String = conn.query_row(
                "SELECT quantity FROM stock_investments WHERE id = 'inv'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(quantity, "5", "the self-heal ran");
            let position_clocks: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sync_clock WHERE tbl = 'stock_investments'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(position_clocks, 0, "a recomputed quantity is not captured");
            let queued_position: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sync_outbox WHERE tbl = 'stock_investments'",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(queued_position, 0);
            // Installed before the reconcile: the event it wrote was captured and queued.
            let check: String = conn.query_row(
                "SELECT id FROM loan_events WHERE kind = 'balance_check'",
                [],
                |r| r.get(0),
            )?;
            let queued_events: Vec<String> = conn
                .prepare("SELECT pk FROM sync_outbox WHERE tbl = 'loan_events'")?
                .query_map([], |r| r.get(0))?
                .collect::<std::result::Result<_, _>>()?;
            assert_eq!(queued_events, vec![check]);
            Ok(())
        })
        .expect("checks");
    }

    /// Audit H2 fix round 1: on a guarded or read-only profile the self-heals, which write
    /// synced tables, are skipped with one log line instead of a refused write per row.
    #[test]
    fn a_profile_with_paused_changes_skips_the_self_heals() {
        use crate::services::sync::capture::guard;
        let mut conn = Connection::open_in_memory().expect("db");
        run_migrations(&conn).expect("migrations");
        assert!(self_heal(&conn), "an ordinary profile heals");

        guard::impose(&mut conn).expect("guard");
        assert!(!self_heal(&conn));
        guard::lift(&mut conn).expect("lift");
        conn.execute_batch("PRAGMA query_only = 1")
            .expect("read-only");
        assert!(!self_heal(&conn));
    }

    #[test]
    fn pre_release_database_is_refused_without_a_file_copy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("moony.db");
        let conn = Connection::open(&path).expect("open");
        conn.execute_batch(MIGRATIONS_TABLE).expect("bookkeeping");
        conn.execute("INSERT INTO _migrations (name) VALUES ('001_baseline')", [])
            .expect("record a name this build does not know");

        let err = run_migrations_with_backup(&conn, &path).expect_err("must refuse");

        assert!(
            err.to_string().contains("pre-release build of Moony"),
            "{err}"
        );
        assert_eq!(backup_copies(dir.path()), 0, "nothing to back up");
    }
}
