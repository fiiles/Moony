//! Database module for Moony
//!
//! Handles SQLCipher encrypted database connections and migrations

mod migrations;

pub use migrations::{applied_migration_names, known_migration_names};

use crate::error::{AppError, Result};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
}

impl Database {
    /// Create a new database instance (not yet connected)
    pub fn new() -> Self {
        Self {
            conn: Arc::new(Mutex::new(None)),
            db_path: Arc::new(Mutex::new(None)),
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
        let conn = Connection::open(&path)?;

        // Set SQLCipher encryption key using hex format
        // SQLCipher expects: PRAGMA key = "x'hexstring'"
        let pragma_key = format!("x{}", hex_key);
        conn.pragma_update(None, "key", &pragma_key)?;

        // Enable foreign key constraints
        conn.execute("PRAGMA foreign_keys = ON", [])?;

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

        // Run migrations, copying the database file first when any are pending
        //: a failed upgrade must never leave the only copy half-migrated.
        run_migrations_with_backup(&conn, &path)?;

        // Load exchange rates from database for offline use
        if let Err(e) = crate::services::currency::load_rates_from_db(&conn) {
            log::warn!("[DB] Warning: Failed to load exchange rates: {}", e);
        }

        // Idempotent self-heal that runs on every open: corrects any stale investment
        // quantity values left over from older calculations
        // (never blocks DB open on failure)
        if let Err(e) = crate::services::investments::recalculate_all_investment_metrics(&conn) {
            log::warn!(
                "[DB] Warning: Failed to recalculate investment metrics: {}",
                e
            );
        }

        // Store connection and path
        *self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(conn);
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

        // Set SQLCipher encryption key using hex format
        let pragma_key = format!("x{}", hex_key);
        conn.pragma_update(None, "key", &pragma_key)?;

        // Enable foreign key constraints
        conn.execute("PRAGMA foreign_keys = ON", [])?;

        // Run migrations to create schema
        migrations::run_migrations(&conn)?;

        // Note: No exchange rates to load on fresh database

        // Store connection and path
        *self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(conn);
        *self
            .db_path
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(path);

        Ok(())
    }

    /// Close the database connection (lock the app)
    pub fn close(&self) {
        *self
            .conn
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
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
        let conn = guard
            .as_ref()
            .ok_or_else(|| AppError::Auth("Database is locked".into()))?;
        f(conn)
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

/// How many pre-migration copies to keep next to the database.
const MIGRATION_BACKUPS_TO_KEEP: usize = 3;

/// Copy `db_path` to `<name>.bak-<unix-ts>` before the first pending migration
/// runs, keep the newest few copies, then migrate. Returns the migration error
/// (with the backup location) if any step of the chain fails.
fn run_migrations_with_backup(conn: &Connection, db_path: &std::path::Path) -> Result<()> {
    let backup = if migrations::has_pending_migrations(conn)? {
        Some(backup_database_file(db_path)?)
    } else {
        None
    };
    match migrations::run_migrations(conn) {
        Ok(()) => Ok(()),
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
/// `<file>.bak-<unix-ts>` in the same directory and prune older copies.
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

    // Prune: keep the newest MIGRATION_BACKUPS_TO_KEEP copies.
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
        while backups.len() > MIGRATION_BACKUPS_TO_KEEP {
            let old = backups.remove(0);
            let _ = std::fs::remove_file(&old);
            for suffix in ["-wal", "-shm", "-journal"] {
                let _ = std::fs::remove_file(format!("{}{}", old.display(), suffix));
            }
        }
    }

    Ok(backup_path)
}

impl Default for Database {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(remaining.len(), MIGRATION_BACKUPS_TO_KEEP);
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
