//! The `files` schema: `attachments.db`, attached to the main connection (ADR 0017).
//!
//! Attachment content lives in its own SQLCipher file next to `moony.db`. It is attached as the
//! schema `files` without a `KEY` clause, so SQLCipher encrypts it with the main database's key
//! and the master key lifecycle does not change. The schema holds content only (`files.blobs`);
//! owners, names and MIME types live in the main schema.
//!
//! `files` has its own append-only version list, keyed by `PRAGMA files.user_version`: the chain
//! in `migrations.rs` belongs to `moony.db`. A pending version first copies `attachments.db`, the
//! same way the main migrations first copy `moony.db`. Never edit an applied version; append one.

use crate::error::{AppError, Result};
use rusqlite::Connection;
use std::path::{Path, PathBuf};

/// Schema name the store is attached under.
pub const SCHEMA: &str = "files";
/// File name of the store, next to the main database.
pub const FILE_NAME: &str = "attachments.db";

/// Version 1: the content table. `auto_vacuum` is set separately, before the first table exists.
const VERSION_1: &str = r#"
CREATE TABLE files.blobs (
    id         TEXT PRIMARY KEY,
    data       BLOB NOT NULL,
    size       INTEGER NOT NULL,
    sha256     TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
"#;

/// The append-only version list, in application order.
const VERSIONS: &[(i64, &str)] = &[(1, VERSION_1)];

/// `attachments.db` next to the given main database file.
pub fn attachments_path(db_path: &Path) -> PathBuf {
    db_path.with_file_name(FILE_NAME)
}

/// Attach `path` as `files` (creating it, encrypted, when it does not exist yet) and bring its
/// schema up to date. On any failure the schema is detached again, so the connection is left
/// without a store rather than with a half-prepared one.
pub fn attach(conn: &Connection, path: &Path) -> Result<()> {
    let path_str = path
        .to_str()
        .ok_or_else(|| AppError::Internal("Attachments path is not valid UTF-8".into()))?;
    let existed = path.exists();
    conn.execute("ATTACH DATABASE ?1 AS files", [path_str])?;
    let prepared = (|| {
        let pending = pending_versions(conn)?;
        if pending && existed && current_version(conn)? > 0 {
            crate::db::backup_database_file(path)?;
        }
        migrate(conn)
    })();
    if let Err(e) = prepared {
        let _ = conn.execute("DETACH DATABASE files", []);
        return Err(e);
    }
    Ok(())
}

/// Apply every pending version, each in its own transaction, then switch on `secure_delete`.
/// Refuses a store written by a newer build (a version this build does not know).
pub(crate) fn migrate(conn: &Connection) -> Result<()> {
    let current = current_version(conn)?;
    let latest = VERSIONS.last().map(|(v, _)| *v).unwrap_or(0);
    if current > latest {
        return Err(AppError::Database(format!(
            "The attachments store has schema version {current}, newer than this build ({latest})"
        )));
    }
    if current == 0 && !has_tables(conn)? {
        // Takes effect only while the file has no tables.
        conn.execute_batch("PRAGMA files.auto_vacuum = INCREMENTAL;")?;
    }
    for (version, sql) in VERSIONS.iter().filter(|(v, _)| *v > current) {
        let tx = conn.unchecked_transaction()?;
        tx.execute_batch(sql)?;
        tx.execute_batch(&format!("PRAGMA files.user_version = {version};"))?;
        tx.commit()?;
        log::info!("[ATTACHMENTS] Store schema version {version} applied");
    }
    conn.execute_batch("PRAGMA files.secure_delete = ON;")?;
    Ok(())
}

fn current_version(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("PRAGMA files.user_version", [], |r| r.get(0))?)
}

fn pending_versions(conn: &Connection) -> Result<bool> {
    let current = current_version(conn)?;
    Ok(VERSIONS.iter().any(|(v, _)| *v > current))
}

fn has_tables(conn: &Connection) -> Result<bool> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM files.sqlite_master WHERE type = 'table'",
        [],
        |r| r.get(0),
    )?;
    Ok(count > 0)
}

/// True when the store is attached to this connection.
pub fn is_attached(conn: &Connection) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT count(*) FROM pragma_database_list WHERE name = ?1",
        [SCHEMA],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::attachment_store;

    const KEY: &str = "x'00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff'";
    const OTHER_KEY: &str = "x'ffeeddccbbaa99887766554433221100ffeeddccbbaa99887766554433221100'";

    fn keyed(path: &Path, key: &str) -> Connection {
        let conn = Connection::open(path).expect("open");
        conn.pragma_update(None, "key", key).expect("key");
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |_| Ok(()))
            .expect("key works");
        conn
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    fn plaintext_header(path: &Path) -> bool {
        let bytes = std::fs::read(path).expect("read");
        bytes.starts_with(b"SQLite format 3\0")
    }

    #[test]
    fn a_fresh_in_memory_store_gets_version_1_incremental_vacuum_and_secure_delete() {
        let conn = Connection::open_in_memory().expect("db");
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        migrate(&conn).expect("migrate");
        assert_eq!(current_version(&conn).expect("version"), 1);
        let secure: i64 = conn
            .query_row("PRAGMA files.secure_delete", [], |r| r.get(0))
            .expect("secure_delete");
        assert_eq!(secure, 1);
        assert!(is_attached(&conn).expect("attached"));
        // Running again is a no-op.
        migrate(&conn).expect("migrate again");
        assert_eq!(current_version(&conn).expect("version"), 1);
    }

    #[test]
    fn a_store_from_a_newer_build_is_refused() {
        let conn = Connection::open_in_memory().expect("db");
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        conn.execute_batch("PRAGMA files.user_version = 99;")
            .expect("version");
        assert!(migrate(&conn).is_err());
    }

    #[test]
    fn the_store_is_encrypted_with_the_main_key_and_holds_no_plaintext() {
        let dir = tempfile::tempdir().expect("tempdir");
        let main_path = dir.path().join("moony.db");
        let store_path = attachments_path(&main_path);
        let jpeg: Vec<u8> = [0xFF, 0xD8, 0xFF, 0xE0]
            .iter()
            .copied()
            .chain((0..200_000u32).map(|i| (i % 251) as u8))
            .collect();
        let pdf: Vec<u8> = b"%PDF-1.7\n"
            .iter()
            .copied()
            .chain(vec![b'A'; 50_000])
            .collect();
        {
            let conn = keyed(&main_path, KEY);
            attach(&conn, &store_path).expect("attach");
            attachment_store::put_bytes(&conn, &jpeg).expect("jpeg");
            attachment_store::put_bytes(&conn, &pdf).expect("pdf");

            let snapshot = dir.path().join("snapshot.db");
            conn.execute("VACUUM files INTO ?1", [snapshot.to_str().expect("utf8")])
                .expect("vacuum into");
            assert!(!plaintext_header(&snapshot));
        }

        assert!(!plaintext_header(&store_path));
        let bytes = std::fs::read(&store_path).expect("read store");
        assert!(!contains(&bytes, &[0xFF, 0xD8, 0xFF, 0xE0]));
        assert!(!contains(&bytes, b"%PDF-1.7"));

        // Opening the store on its own needs the main key.
        assert!(Connection::open(&store_path)
            .and_then(|c| c.query_row("SELECT count(*) FROM blobs", [], |r| r.get::<_, i64>(0)))
            .is_err());
        let direct = keyed(&store_path, KEY);
        let n: i64 = direct
            .query_row("SELECT count(*) FROM blobs", [], |r| r.get(0))
            .expect("count");
        assert_eq!(n, 2);
    }

    #[test]
    fn reattaching_keeps_the_version_and_the_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let main_path = dir.path().join("moony.db");
        let store_path = attachments_path(&main_path);
        let id = {
            let conn = keyed(&main_path, KEY);
            attach(&conn, &store_path).expect("attach");
            attachment_store::put_bytes(&conn, b"hello")
                .expect("put")
                .id
        };
        let conn = keyed(&main_path, KEY);
        attach(&conn, &store_path).expect("re-attach");
        assert_eq!(current_version(&conn).expect("version"), 1);
        assert_eq!(
            attachment_store::read_all(&conn, &id).expect("read"),
            b"hello".to_vec()
        );
        // No pre-migration copy: nothing was pending.
        let copies = std::fs::read_dir(dir.path())
            .expect("dir")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak-"))
            .count();
        assert_eq!(copies, 0);
    }

    #[test]
    fn a_store_under_another_key_is_not_attached_and_not_touched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let store_path = dir.path().join(FILE_NAME);
        {
            let foreign = keyed(&dir.path().join("other.db"), OTHER_KEY);
            attach(&foreign, &store_path).expect("attach under the other key");
            attachment_store::put_bytes(&foreign, b"foreign").expect("put");
        }
        let before = std::fs::read(&store_path).expect("read");

        let conn = keyed(&dir.path().join("moony.db"), KEY);
        assert!(attach(&conn, &store_path).is_err());
        assert!(!is_attached(&conn).expect("attached?"));
        assert_eq!(std::fs::read(&store_path).expect("read"), before);
    }
}
