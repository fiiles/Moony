//! Converting the plain attachment files of earlier versions into the encrypted store
//! (ADR 0017, D4 and D5 of the encrypted-attachments spec).
//!
//! After every unlock a background run (`commands::attachments::spawn_conversion`) works through
//! the rows that still point at a plain file, one item at a time:
//!
//! 1. [`read_sources`] reads the file(s) without the connection lock;
//! 2. [`convert`] stores the blob(s), reads them back and compares their hashes, and updates the
//!    row, all in one transaction, then moves the original into [`QUARANTINE_DIR`];
//! 3. at the next unlock [`finish_quarantine`] deletes the quarantine once the store passes its
//!    checks.
//!
//! An original outside the current data folder (a stale path into an old identifier's folder) is
//! converted but never deleted; it is recorded in `attachment_leftovers` and shown in Settings.
//!
//! After the items, [`sweep_unreferenced_legacy_files`] moves every other plain file in the legacy
//! folders into the quarantine too: files whose rows were deleted by earlier versions (which left
//! them on disk), and originals whose move failed or was cut short after their row was converted.

use crate::constants::FORMER_IDENTIFIERS;
use crate::db::files_schema;
use crate::error::{AppError, Result};
use crate::models::{AttachmentKind, AttachmentStatus};
use crate::services::attachments::{self, INSURANCE_DOCUMENTS_DIR, REAL_ESTATE_DOCUMENTS_DIR};
use crate::services::{attachment_store, document_files};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use uuid::Uuid;

/// Converted originals wait here, inside the data folder, until the next unlock.
pub const QUARANTINE_DIR: &str = "attachments-converted";

pub use crate::services::attachments::LEGACY_DIRS;

/// One row that still points at plain files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LegacyItem {
    Document {
        kind: AttachmentKind,
        id: String,
        stored_path: String,
    },
    Photo {
        id: String,
        file: String,
        thumb: String,
    },
}

/// The bytes of an item's file(s), read without the connection lock.
pub struct Sources {
    files: Vec<(PathBuf, Vec<u8>)>,
}

/// What happened to one item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Stored; the original moved into the quarantine.
    Converted,
    /// Stored; the original lies outside the data folder and was left in place.
    KeptOutside,
    /// The row was deleted or converted meanwhile; nothing was written.
    Gone,
}

/// Progress of the current or last run, shared with the status command.
#[derive(Debug, Default)]
pub struct ConversionProgress {
    pub running: AtomicBool,
    /// A run was requested while one was still going (an unlock while the last session's run was
    /// stopping): the running thread starts another one when it ends.
    pub rerun: AtomicBool,
    pub total: AtomicU32,
    pub done: AtomicU32,
    pub failed: AtomicU32,
}

fn document_dir(kind: AttachmentKind) -> &'static str {
    match kind {
        AttachmentKind::InsuranceDocument => INSURANCE_DOCUMENTS_DIR,
        _ => REAL_ESTATE_DOCUMENTS_DIR,
    }
}

fn document_table(kind: AttachmentKind) -> &'static str {
    match kind {
        AttachmentKind::InsuranceDocument => "insurance_documents",
        _ => "real_estate_documents",
    }
}

/// Every row that still points at plain files, documents first.
pub fn pending(conn: &Connection) -> Result<Vec<LegacyItem>> {
    let mut items = Vec::new();
    for kind in [
        AttachmentKind::InsuranceDocument,
        AttachmentKind::RealEstateDocument,
    ] {
        let mut stmt = conn.prepare(&format!(
            "SELECT id, file_path FROM {} WHERE blob_id IS NULL AND file_path IS NOT NULL
             ORDER BY id",
            document_table(kind)
        ))?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        for row in rows {
            let (id, stored_path) = row?;
            items.push(LegacyItem::Document {
                kind,
                id,
                stored_path,
            });
        }
    }
    let mut stmt = conn.prepare(
        "SELECT id, file_path, thumbnail_path FROM real_estate_photos
         WHERE blob_id IS NULL AND file_path IS NOT NULL AND thumbnail_path IS NOT NULL
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok(LegacyItem::Photo {
            id: r.get(0)?,
            file: r.get(1)?,
            thumb: r.get(2)?,
        })
    })?;
    for row in rows {
        items.push(row?);
    }
    Ok(items)
}

/// The plain file(s) of an item, or `None` when one is missing on disk.
fn source_paths(data_dir: &Path, item: &LegacyItem) -> Result<Option<Vec<PathBuf>>> {
    let found = |r: Result<PathBuf>| match r {
        Ok(path) => Ok(Some(path)),
        Err(AppError::NotFound(_)) => Ok(None),
        Err(e) => Err(e),
    };
    match item {
        LegacyItem::Document {
            kind, stored_path, ..
        } => Ok(found(document_files::resolve_document_path(
            data_dir,
            document_dir(*kind),
            stored_path,
        ))?
        .map(|p| vec![p])),
        LegacyItem::Photo { file, thumb, .. } => {
            let file = found(attachments::legacy_photo_path(data_dir, file))?;
            let thumb = found(attachments::legacy_photo_path(data_dir, thumb))?;
            Ok(file.zip(thumb).map(|(f, t)| vec![f, t]))
        }
    }
}

/// Read an item's file(s). `None` when a file is missing (the item is counted as missing).
pub fn read_sources(data_dir: &Path, item: &LegacyItem) -> Result<Option<Sources>> {
    let Some(paths) = source_paths(data_dir, item)? else {
        return Ok(None);
    };
    let mut files = Vec::with_capacity(paths.len());
    for path in paths {
        let bytes = fs::read(&path)
            .map_err(|e| AppError::Internal(format!("Cannot read an attachment file: {e}")))?;
        files.push((path, bytes));
    }
    Ok(Some(Sources { files }))
}

/// Store a blob and read it back: the content must hash to what was read from disk.
fn put_verified(conn: &Connection, bytes: &[u8]) -> Result<String> {
    let blob = attachment_store::put_bytes(conn, bytes)?;
    let expected = hex::encode(Sha256::digest(bytes));
    if blob.sha256 != expected || !attachment_store::verify(conn, &blob.id, &expected)? {
        return Err(AppError::Internal(
            "A converted attachment did not read back identically".into(),
        ));
    }
    Ok(blob.id)
}

/// Convert one item: blob(s), read-back check and row update in one transaction; then move the
/// original(s) into the quarantine (inside the data folder) or record them as leftovers.
pub fn convert(
    conn: &Connection,
    data_dir: &Path,
    item: &LegacyItem,
    sources: &Sources,
) -> Result<Outcome> {
    let inside = sources.files.iter().all(|(p, _)| p.starts_with(data_dir));
    let tx = conn.unchecked_transaction()?;
    let updated = match item {
        LegacyItem::Document {
            kind,
            id,
            stored_path,
        } => {
            let (path, bytes) = &sources.files[0];
            let blob = put_verified(&tx, bytes)?;
            let original = attachments::legacy_original_name(stored_path);
            let name_for_type = original.clone().or_else(|| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(str::to_string)
            });
            let head = &bytes[..bytes.len().min(16)];
            let mime = attachment_store::sniff_mime(name_for_type.as_deref(), head);
            tx.execute(
                &format!(
                    "UPDATE {} SET blob_id = ?1, mime_type = ?2,
                         original_name = COALESCE(original_name, ?3), file_path = NULL
                     WHERE id = ?4 AND blob_id IS NULL",
                    document_table(*kind)
                ),
                rusqlite::params![blob, mime, original, id],
            )?
        }
        LegacyItem::Photo { id, .. } => {
            let full = put_verified(&tx, &sources.files[0].1)?;
            let thumb = put_verified(&tx, &sources.files[1].1)?;
            tx.execute(
                "UPDATE real_estate_photos SET blob_id = ?1, thumb_blob_id = ?2,
                     file_path = NULL, thumbnail_path = NULL
                 WHERE id = ?3 AND blob_id IS NULL",
                rusqlite::params![full, thumb, id],
            )?
        }
    };
    if updated == 0 {
        // Deleted or converted since `pending` ran: the blobs roll back with the transaction.
        return Ok(Outcome::Gone);
    }
    if !inside {
        for (path, _) in &sources.files {
            tx.execute(
                "INSERT OR IGNORE INTO attachment_leftovers (id, path) VALUES (?1, ?2)",
                rusqlite::params![Uuid::new_v4().to_string(), path.to_string_lossy()],
            )?;
        }
    }
    tx.commit()?;

    if !inside {
        return Ok(Outcome::KeptOutside);
    }
    for (path, _) in &sources.files {
        quarantine(data_dir, path);
    }
    Ok(Outcome::Converted)
}

/// Move a plain original into the quarantine, keeping its path relative to the data folder.
/// Returns whether it moved; one that did not is picked up by the next sweep.
fn quarantine(data_dir: &Path, path: &Path) -> bool {
    if !attachments::is_legacy_attachment_path(data_dir, path) {
        return false;
    }
    let Ok(rel) = path.strip_prefix(data_dir) else {
        return false;
    };
    let target = data_dir.join(QUARANTINE_DIR).join(rel);
    let moved = target
        .parent()
        .map(fs::create_dir_all)
        .unwrap_or(Ok(()))
        .and_then(|_| fs::rename(path, &target));
    match moved {
        Ok(()) => true,
        Err(e) => {
            log::warn!("[ATTACHMENTS] A plain original could not be moved aside: {e}");
            false
        }
    }
}

/// Delete the quarantine once the store is healthy and every converted row has its content at its
/// recorded size. Returns whether it was deleted.
pub fn finish_quarantine(conn: &Connection, data_dir: &Path) -> Result<bool> {
    let dir = data_dir.join(QUARANTINE_DIR);
    if !dir.exists() {
        return Ok(false);
    }
    attachment_store::ensure_available(conn)?;
    let check: String = conn.query_row("PRAGMA files.quick_check", [], |r| r.get(0))?;
    let missing = attachment_store::rows_without_content(conn)?;
    if check != "ok" || missing > 0 {
        log::warn!(
            "[ATTACHMENTS] Keeping the converted originals: store check = {check}, rows without content = {missing}"
        );
        return Ok(false);
    }
    fs::remove_dir_all(&dir)
        .map_err(|e| AppError::Internal(format!("Cannot delete the converted originals: {e}")))?;
    Ok(true)
}

/// The plain files rows still point at (pending items whose file exists), inside the data folder.
pub fn referenced_legacy_files(conn: &Connection, data_dir: &Path) -> Result<HashSet<PathBuf>> {
    let mut referenced = HashSet::new();
    for item in pending(conn)? {
        if let Some(paths) = source_paths(data_dir, &item)? {
            referenced.extend(paths);
        }
    }
    Ok(referenced)
}

/// Every file in the legacy folders that no row points at any more.
pub fn unreferenced_legacy_files(conn: &Connection, data_dir: &Path) -> Result<Vec<PathBuf>> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    out.push(path);
                }
            }
        }
    }
    let referenced = referenced_legacy_files(conn, data_dir)?;
    let mut files = Vec::new();
    for name in LEGACY_DIRS {
        walk(&data_dir.join(name), &mut files);
    }
    files
        .retain(|f| !referenced.contains(f) && attachments::is_legacy_attachment_path(data_dir, f));
    files.sort();
    Ok(files)
}

/// Move the plain files no row points at into the quarantine (deleted at the next unlock, like
/// converted originals). Returns how many were moved.
pub fn sweep_unreferenced_legacy_files(conn: &Connection, data_dir: &Path) -> Result<usize> {
    let files = unreferenced_legacy_files(conn, data_dir)?;
    let mut moved = 0;
    for file in &files {
        if quarantine(data_dir, file) {
            moved += 1;
        }
    }
    Ok(moved)
}

/// Remove the legacy attachment folders' empty sub-folders, and the folders themselves when empty.
pub fn remove_empty_legacy_dirs(data_dir: &Path) {
    fn prune(dir: &Path) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    prune(&path);
                }
            }
        }
        let _ = fs::remove_dir(dir);
    }
    for name in LEGACY_DIRS {
        let dir = data_dir.join(name);
        if dir.is_dir() {
            prune(&dir);
        }
    }
}

/// Plain originals the app found and will not delete (D5): recorded leftovers that still exist,
/// and the data folders of former app identifiers that still hold attachment folders.
pub fn refresh_leftovers(conn: &Connection, data_dir: &Path) -> Result<Vec<String>> {
    let recorded: Vec<(String, String)> = {
        let mut stmt = conn.prepare("SELECT id, path FROM attachment_leftovers ORDER BY path")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut leftovers = Vec::new();
    for (id, path) in recorded {
        if Path::new(&path).exists() {
            leftovers.push(path);
        } else {
            conn.execute("DELETE FROM attachment_leftovers WHERE id = ?1", [&id])?;
        }
    }
    // Plain files the sweep could not move aside stay visible.
    let mut stuck: Vec<String> = Vec::new();
    for file in unreferenced_legacy_files(conn, data_dir)? {
        if let Some(first) = file
            .strip_prefix(data_dir)
            .ok()
            .and_then(|rel| rel.components().next())
        {
            let folder = data_dir.join(first).to_string_lossy().into_owned();
            if !stuck.contains(&folder) {
                stuck.push(folder);
            }
        }
    }
    leftovers.extend(stuck);
    if let Some(parent) = data_dir.parent() {
        for identifier in FORMER_IDENTIFIERS {
            let folder = parent.join(identifier);
            if folder != data_dir && LEGACY_DIRS.iter().any(|d| folder.join(d).is_dir()) {
                leftovers.push(folder.to_string_lossy().into_owned());
            }
        }
    }
    Ok(leftovers)
}

/// What the Data settings page shows about attachments.
pub fn status(
    conn: &Connection,
    data_dir: &Path,
    progress: &ConversionProgress,
) -> Result<AttachmentStatus> {
    let store_available = files_schema::is_attached(conn)?;
    let stored: i64 = conn.query_row(
        "SELECT (SELECT count(*) FROM insurance_documents WHERE blob_id IS NOT NULL)
              + (SELECT count(*) FROM real_estate_documents WHERE blob_id IS NOT NULL)
              + (SELECT count(*) FROM real_estate_photos WHERE blob_id IS NOT NULL)",
        [],
        |r| r.get(0),
    )?;
    let items = pending(conn)?;
    let mut missing = 0u32;
    for item in &items {
        if source_paths(data_dir, item)?.is_none() {
            missing += 1;
        }
    }
    let legacy_missing = missing;
    // Rows whose encrypted content is gone (an `attachments.db` left behind when the profile was
    // copied, say) are missing, not stored.
    let content_missing = if store_available {
        attachment_store::rows_without_content(conn)?
    } else {
        0
    };
    missing += content_missing;
    Ok(AttachmentStatus {
        store_available,
        stored: (stored as u32).saturating_sub(content_missing),
        legacy_pending: items.len() as u32 - legacy_missing,
        missing,
        failed: progress.failed.load(Ordering::SeqCst),
        converting: progress.running.load(Ordering::SeqCst),
        done: progress.done.load(Ordering::SeqCst),
        total: progress.total.load(Ordering::SeqCst),
        leftovers: refresh_leftovers(conn, data_dir)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        files_schema::migrate(&conn).expect("files schema");
        conn.execute_batch(
            r#"
            CREATE TABLE insurance_documents (
                id TEXT PRIMARY KEY, insurance_id TEXT, name TEXT, file_path TEXT, blob_id TEXT,
                mime_type TEXT, original_name TEXT
            );
            CREATE TABLE real_estate_documents (
                id TEXT PRIMARY KEY, real_estate_id TEXT, name TEXT, file_path TEXT, blob_id TEXT,
                mime_type TEXT, original_name TEXT
            );
            CREATE TABLE real_estate_photos (
                id TEXT PRIMARY KEY, batch_id TEXT, file_path TEXT, thumbnail_path TEXT,
                blob_id TEXT, thumb_blob_id TEXT
            );
            CREATE TABLE attachment_leftovers (
                id TEXT PRIMARY KEY, path TEXT NOT NULL UNIQUE,
                found_at INTEGER NOT NULL DEFAULT (unixepoch())
            );
            "#,
        )
        .expect("schema");
        conn
    }

    const UUID: &str = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60";

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        fs::write(path, bytes).expect("write");
    }

    fn run_all(conn: &Connection, data_dir: &Path) -> Vec<Option<Outcome>> {
        pending(conn)
            .expect("pending")
            .iter()
            .map(|item| {
                read_sources(data_dir, item)
                    .expect("read")
                    .map(|src| convert(conn, data_dir, item, &src).expect("convert"))
            })
            .collect()
    }

    fn blob_of(conn: &Connection, table: &str, id: &str) -> Option<String> {
        conn.query_row(
            &format!("SELECT blob_id FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .expect("row")
    }

    #[test]
    fn inside_files_are_stored_and_moved_into_the_quarantine() {
        let conn = setup_test_db();
        let data = tempfile::tempdir().expect("data");
        let dir = data.path();
        let doc = dir.join(format!("insurance_documents/pol/{UUID}_Smlouva 2020.pdf"));
        write(&doc, b"%PDF-1.4 legacy");
        let photo = dir.join("real_estate_photos/re/b/p.jpg");
        let thumb = dir.join("real_estate_photos/re/b/p_thumb.jpg");
        write(&photo, &[0xFF, 0xD8, 0xFF, 1, 2, 3]);
        write(&thumb, &[0xFF, 0xD8, 0xFF, 4]);
        // Stored under an old identifier's folder; the file was copied into the current one.
        conn.execute(
            "INSERT INTO insurance_documents (id, name, file_path) VALUES ('d', 'Smlouva', ?1)",
            [format!(
                "/old/com.filipkral.moony/insurance_documents/pol/{UUID}_Smlouva 2020.pdf"
            )],
        )
        .expect("doc row");
        conn.execute(
            "INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path)
             VALUES ('p', 'b', 'real_estate_photos/re/b/p.jpg', 'real_estate_photos/re/b/p_thumb.jpg')",
            [],
        )
        .expect("photo row");

        assert_eq!(
            run_all(&conn, dir),
            vec![Some(Outcome::Converted), Some(Outcome::Converted)]
        );

        let (mime, original, path): (String, String, Option<String>) = conn
            .query_row(
                "SELECT mime_type, original_name, file_path FROM insurance_documents WHERE id = 'd'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("doc");
        assert_eq!(
            (mime.as_str(), original.as_str(), path),
            ("application/pdf", "Smlouva 2020.pdf", None)
        );
        let blob = blob_of(&conn, "insurance_documents", "d").expect("blob");
        assert_eq!(
            attachment_store::read_all(&conn, &blob).expect("read"),
            b"%PDF-1.4 legacy".to_vec()
        );
        assert!(blob_of(&conn, "real_estate_photos", "p").is_some());

        assert!(!doc.exists() && !photo.exists() && !thumb.exists());
        let q = dir.join(QUARANTINE_DIR);
        assert!(q
            .join(format!("insurance_documents/pol/{UUID}_Smlouva 2020.pdf"))
            .exists());
        assert!(q.join("real_estate_photos/re/b/p_thumb.jpg").exists());
        assert!(pending(&conn).expect("pending").is_empty());

        remove_empty_legacy_dirs(dir);
        for legacy in LEGACY_DIRS {
            assert!(!dir.join(legacy).exists(), "{legacy} pruned");
        }

        assert!(finish_quarantine(&conn, dir).expect("finish"));
        assert!(!q.exists());
        assert!(!finish_quarantine(&conn, dir).expect("nothing to finish"));
    }

    #[test]
    fn an_original_outside_the_data_folder_is_kept_and_reported() {
        let conn = setup_test_db();
        let data = tempfile::tempdir().expect("data");
        let old = tempfile::tempdir().expect("old folder");
        let outside = old.path().join("pol/doc.pdf");
        write(&outside, b"%PDF outside");
        conn.execute(
            "INSERT INTO insurance_documents (id, name, file_path) VALUES ('d', 'x', ?1)",
            [outside.to_str().expect("utf8")],
        )
        .expect("row");

        assert_eq!(
            run_all(&conn, data.path()),
            vec![Some(Outcome::KeptOutside)]
        );
        assert!(outside.exists(), "never deleted");
        assert!(blob_of(&conn, "insurance_documents", "d").is_some());
        assert_eq!(
            refresh_leftovers(&conn, data.path()).expect("leftovers"),
            vec![outside.to_string_lossy().into_owned()]
        );
        assert!(!data.path().join(QUARANTINE_DIR).exists());

        // Once the user deletes it, it drops off the list.
        fs::remove_file(&outside).expect("rm");
        assert!(refresh_leftovers(&conn, data.path())
            .expect("leftovers")
            .is_empty());
    }

    #[test]
    fn a_missing_file_is_skipped_and_a_rerun_is_a_no_op() {
        let conn = setup_test_db();
        let data = tempfile::tempdir().expect("data");
        conn.execute(
            "INSERT INTO real_estate_documents (id, name, file_path) VALUES ('gone', 'x', '/nowhere/a/b.pdf')",
            [],
        )
        .expect("row");
        conn.execute(
            "INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path)
             VALUES ('p', 'b', 'real_estate_photos/re/b/missing.jpg', 'real_estate_photos/re/b/missing_thumb.jpg')",
            [],
        )
        .expect("photo");
        assert_eq!(run_all(&conn, data.path()), vec![None, None]);
        let progress = ConversionProgress::default();
        let s = status(&conn, data.path(), &progress).expect("status");
        assert_eq!((s.stored, s.legacy_pending, s.missing), (0, 0, 2));
        assert!(s.store_available);
    }

    #[test]
    fn a_row_deleted_meanwhile_writes_nothing() {
        let conn = setup_test_db();
        let data = tempfile::tempdir().expect("data");
        let file = data.path().join("real_estate_documents/re/a.pdf");
        write(&file, b"%PDF");
        conn.execute(
            "INSERT INTO real_estate_documents (id, name, file_path) VALUES ('d', 'x', ?1)",
            [file.to_str().expect("utf8")],
        )
        .expect("row");
        let item = pending(&conn).expect("pending").remove(0);
        let sources = read_sources(data.path(), &item)
            .expect("read")
            .expect("present");
        conn.execute("DELETE FROM real_estate_documents", [])
            .expect("deleted meanwhile");
        assert_eq!(
            convert(&conn, data.path(), &item, &sources).expect("convert"),
            Outcome::Gone
        );
        let blobs: i64 = conn
            .query_row("SELECT count(*) FROM files.blobs", [], |r| r.get(0))
            .expect("count");
        assert_eq!(blobs, 0);
        assert!(file.exists(), "nothing is moved for a row that is gone");
    }

    #[test]
    fn a_failed_conversion_rolls_back_and_is_retried_next_time() {
        let conn = setup_test_db();
        let data = tempfile::tempdir().expect("data");
        let file = data.path().join("real_estate_documents/re/a.pdf");
        write(&file, b"%PDF");
        conn.execute(
            "INSERT INTO real_estate_documents (id, name, file_path) VALUES ('d', 'x', ?1)",
            [file.to_str().expect("utf8")],
        )
        .expect("row");
        let item = pending(&conn).expect("pending").remove(0);
        let sources = read_sources(data.path(), &item)
            .expect("read")
            .expect("present");
        // The row update fails after the blob was written.
        conn.execute_batch(
            "CREATE TRIGGER no_update BEFORE UPDATE ON real_estate_documents
             BEGIN SELECT RAISE(ABORT, 'simulated failure'); END;",
        )
        .expect("trigger");
        assert!(convert(&conn, data.path(), &item, &sources).is_err());
        let blobs: i64 = conn
            .query_row("SELECT count(*) FROM files.blobs", [], |r| r.get(0))
            .expect("count");
        assert_eq!(blobs, 0, "the blob rolled back with the row");
        assert!(file.exists());
        assert_eq!(
            pending(&conn).expect("pending").len(),
            1,
            "retried next time"
        );

        conn.execute_batch("DROP TRIGGER no_update;").expect("drop");
        assert_eq!(
            convert(&conn, data.path(), &item, &sources).expect("convert"),
            Outcome::Converted
        );
    }

    #[test]
    fn the_quarantine_stays_while_a_converted_row_has_no_content() {
        let conn = setup_test_db();
        let data = tempfile::tempdir().expect("data");
        fs::create_dir_all(data.path().join(QUARANTINE_DIR)).expect("quarantine");
        conn.execute(
            "INSERT INTO real_estate_documents (id, name, blob_id) VALUES ('d', 'x', 'lost-blob')",
            [],
        )
        .expect("row");
        assert!(!finish_quarantine(&conn, data.path()).expect("finish"));
        assert!(data.path().join(QUARANTINE_DIR).exists());
    }

    #[test]
    fn a_former_identifier_folder_with_attachments_is_reported() {
        let conn = setup_test_db();
        let root = tempfile::tempdir().expect("support dir");
        let data = root.path().join("com.moonyfinance.desktop");
        fs::create_dir_all(&data).expect("data");
        let old = root.path().join("com.filipkral.moony-tauri");
        fs::create_dir_all(old.join("real_estate_photos/re")).expect("old photos");
        fs::create_dir_all(root.path().join("com.filipkral.moony"))
            .expect("old without attachments");
        assert_eq!(
            refresh_leftovers(&conn, &data).expect("leftovers"),
            vec![old.to_string_lossy().into_owned()]
        );
    }

    #[test]
    fn orphaned_and_stuck_plain_files_are_swept_into_the_quarantine() {
        let conn = setup_test_db();
        let data = tempfile::tempdir().expect("data");
        let dir = data.path();
        // A file of a property an earlier version deleted (no row), and a converted row whose
        // original never moved aside (the app stopped between commit and move).
        let orphan = dir.join("real_estate_photos/deleted-prop/b/x.jpg");
        let stuck = dir.join("insurance_documents/pol/converted.pdf");
        let pending_file = dir.join("insurance_documents/pol/pending.pdf");
        write(&orphan, b"orphan");
        write(&stuck, b"stuck");
        write(&pending_file, b"%PDF pending");
        let blob = attachment_store::put_bytes(&conn, b"stuck")
            .expect("blob")
            .id;
        conn.execute(
            "INSERT INTO insurance_documents (id, name, blob_id) VALUES ('done', 'a', ?1)",
            [&blob],
        )
        .expect("converted row");
        conn.execute(
            "INSERT INTO insurance_documents (id, name, file_path) VALUES ('todo', 'b', ?1)",
            [pending_file.to_str().expect("utf8")],
        )
        .expect("pending row");

        let leftovers = refresh_leftovers(&conn, dir).expect("leftovers");
        assert!(leftovers.contains(
            &dir.join("real_estate_photos")
                .to_string_lossy()
                .into_owned()
        ));
        assert!(leftovers.contains(
            &dir.join("insurance_documents")
                .to_string_lossy()
                .into_owned()
        ));

        assert_eq!(
            sweep_unreferenced_legacy_files(&conn, dir).expect("sweep"),
            2
        );
        assert!(!orphan.exists() && !stuck.exists());
        assert!(
            pending_file.exists(),
            "a file a row still points at stays for its conversion"
        );
        let q = dir.join(QUARANTINE_DIR);
        assert!(q.join("real_estate_photos/deleted-prop/b/x.jpg").exists());
        assert!(q.join("insurance_documents/pol/converted.pdf").exists());
        assert!(refresh_leftovers(&conn, dir).expect("leftovers").is_empty());
    }

    #[test]
    fn rows_without_content_count_as_missing_not_stored() {
        let conn = setup_test_db();
        let data = tempfile::tempdir().expect("data");
        let blob = attachment_store::put_bytes(&conn, b"here")
            .expect("blob")
            .id;
        conn.execute_batch(&format!(
            "INSERT INTO real_estate_documents (id, name, blob_id) VALUES ('a', 'x', '{blob}'), ('b', 'y', 'lost');"
        ))
        .expect("rows");
        let s = status(&conn, data.path(), &ConversionProgress::default()).expect("status");
        assert_eq!((s.stored, s.missing, s.legacy_pending), (1, 1, 0));
    }

    #[test]
    fn the_quarantine_stays_while_a_blob_is_truncated() {
        let conn = setup_test_db();
        let data = tempfile::tempdir().expect("data");
        fs::create_dir_all(data.path().join(QUARANTINE_DIR)).expect("quarantine");
        let blob = attachment_store::put_bytes(&conn, b"full content")
            .expect("blob")
            .id;
        conn.execute("UPDATE files.blobs SET data = x'00' WHERE id = ?1", [&blob])
            .expect("truncate");
        conn.execute(
            "INSERT INTO real_estate_documents (id, name, blob_id) VALUES ('d', 'x', ?1)",
            [&blob],
        )
        .expect("row");
        assert!(!finish_quarantine(&conn, data.path()).expect("finish"));
        assert!(data.path().join(QUARANTINE_DIR).exists());
    }

    #[test]
    fn a_format_1_backup_restores_and_then_converts() {
        let src = tempfile::tempdir().expect("src");
        // A 0.12-style profile: a database with a legacy document row and its plain file.
        let db_path = src.path().join("moony.db");
        {
            let c = Connection::open(&db_path).expect("db");
            c.execute_batch(
                r#"
                CREATE TABLE _migrations (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, applied_at INTEGER NOT NULL DEFAULT (unixepoch()));
                INSERT INTO _migrations (name) VALUES ('001_initial_schema');
                CREATE TABLE insurance_documents (id TEXT PRIMARY KEY, insurance_id TEXT, name TEXT,
                    file_path TEXT, blob_id TEXT, mime_type TEXT, original_name TEXT);
                CREATE TABLE real_estate_documents (id TEXT PRIMARY KEY, real_estate_id TEXT, name TEXT,
                    file_path TEXT, blob_id TEXT, mime_type TEXT, original_name TEXT);
                CREATE TABLE real_estate_photos (id TEXT PRIMARY KEY, batch_id TEXT, file_path TEXT,
                    thumbnail_path TEXT, blob_id TEXT, thumb_blob_id TEXT);
                CREATE TABLE attachment_leftovers (id TEXT PRIMARY KEY, path TEXT NOT NULL UNIQUE,
                    found_at INTEGER NOT NULL DEFAULT (unixepoch()));
                "#,
            )
            .expect("schema");
            c.execute(
                "INSERT INTO insurance_documents (id, name, file_path) VALUES ('d', 'Smlouva', ?1)",
                [format!(
                    "/old/com.filipkral.moony/insurance_documents/pol/{UUID}_Smlouva.pdf"
                )],
            )
            .expect("row");
        }
        let archive = src.path().join("v1.zip");
        {
            use std::io::Write;
            let mut zip = zip::ZipWriter::new(fs::File::create(&archive).expect("zip"));
            let opts = zip::write::SimpleFileOptions::default();
            let manifest = serde_json::json!({
                "format": 1, "appVersion": "0.12.2", "createdAt": 0,
                "migrations": ["001_initial_schema"], "files": []
            });
            zip.start_file("manifest.json", opts).expect("entry");
            zip.write_all(manifest.to_string().as_bytes())
                .expect("manifest");
            zip.start_file("moony.db", opts).expect("entry");
            zip.write_all(&fs::read(&db_path).expect("db bytes"))
                .expect("db");
            zip.start_file(format!("insurance_documents/pol/{UUID}_Smlouva.pdf"), opts)
                .expect("entry");
            zip.write_all(b"%PDF from 0.12").expect("doc");
            zip.finish().expect("finish");
        }

        let dst = tempfile::tempdir().expect("dst");
        crate::services::backup::restore_backup(dst.path(), &archive, &["001_initial_schema"])
            .expect("restore");
        let conn = Connection::open(dst.path().join("moony.db")).expect("restored");
        files_schema::attach(&conn, &dst.path().join(files_schema::FILE_NAME)).expect("attach");

        assert_eq!(run_all(&conn, dst.path()), vec![Some(Outcome::Converted)]);
        let blob = blob_of(&conn, "insurance_documents", "d").expect("converted");
        assert_eq!(
            attachment_store::read_all(&conn, &blob).expect("read"),
            b"%PDF from 0.12".to_vec()
        );
        assert!(!dst
            .path()
            .join(format!("insurance_documents/pol/{UUID}_Smlouva.pdf"))
            .exists());
    }
}
