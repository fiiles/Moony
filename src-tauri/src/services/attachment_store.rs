//! Encrypted attachment content: the blobs in `attachments.db` (schema `files`, ADR 0017).
//!
//! This module knows nothing about owners. Attachment rows in the main schema reference a blob
//! by `blob_id`; callers write a blob and its row in one transaction (it spans both schemas, and
//! a rollback undoes both), and delete them the same way.
//!
//! Large content is streamed through SQLite's incremental blob I/O, so a document is never held
//! twice in memory while it is stored or exported.

use crate::db::files_schema;
use crate::error::{AppError, Result};
use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use uuid::Uuid;

/// Largest document that can be attached (D7). Photos have no cap: they are re-encoded.
pub const MAX_DOCUMENT_BYTES: u64 = 100 * 1024 * 1024;

/// Chunk size for streaming into and out of a blob.
const CHUNK: usize = 64 * 1024;

/// Every main-schema column that references a blob. A blob no column references is garbage, so a
/// new blob column must be added here (a test in `migrations.rs` checks the schema against it).
pub(crate) const REFERENCES: [(&str, &str); 4] = [
    ("insurance_documents", "blob_id"),
    ("real_estate_documents", "blob_id"),
    ("real_estate_photos", "blob_id"),
    ("real_estate_photos", "thumb_blob_id"),
];

/// A stored blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    pub id: String,
    pub size: u64,
    pub sha256: String,
}

/// The store is attached to this connection, or a `store unavailable` error (D11).
pub fn ensure_available(conn: &Connection) -> Result<()> {
    if files_schema::is_attached(conn)? {
        Ok(())
    } else {
        Err(AppError::Internal("attachments.storeUnavailable".into()))
    }
}

/// Store `bytes` as a new blob.
pub fn put_bytes(conn: &Connection, bytes: &[u8]) -> Result<BlobRef> {
    put_reader(conn, &mut &bytes[..], bytes.len() as u64)
}

/// Store exactly `len` bytes read from `reader` as a new blob, hashing them on the way.
pub fn put_reader<R: Read>(conn: &Connection, reader: &mut R, len: u64) -> Result<BlobRef> {
    ensure_available(conn)?;
    let size = i64::try_from(len)
        .map_err(|_| AppError::Validation("validation.attachmentTooLarge".into()))?;
    let id = Uuid::new_v4().to_string();
    conn.execute(
        "INSERT INTO files.blobs (id, data, size, sha256) VALUES (?1, zeroblob(?2), ?2, '')",
        rusqlite::params![id, size],
    )?;
    let rowid = conn.last_insert_rowid();
    match fill(conn, rowid, reader, len) {
        Ok(sha256) => Ok(BlobRef {
            id,
            size: len,
            sha256,
        }),
        Err(e) => {
            // Outside a transaction a half-written blob would linger until the next sweep.
            let _ = conn.execute("DELETE FROM files.blobs WHERE rowid = ?1", [rowid]);
            Err(e)
        }
    }
}

/// Stream `reader` into the zero-filled blob at `rowid` and record its hash.
fn fill<R: Read>(conn: &Connection, rowid: i64, reader: &mut R, len: u64) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut written: u64 = 0;
    {
        let mut blob = conn.blob_open(files_schema::SCHEMA, "blobs", "data", rowid, false)?;
        let mut buf = vec![0u8; CHUNK];
        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| AppError::Internal(format!("Failed to read attachment: {e}")))?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > len {
                return Err(AppError::Internal(
                    "The attachment grew while it was being stored".into(),
                ));
            }
            hasher.update(&buf[..n]);
            blob.write_all(&buf[..n])
                .map_err(|e| AppError::Database(format!("Failed to store attachment: {e}")))?;
        }
    }
    if written != len {
        return Err(AppError::Internal(
            "The attachment shrank while it was being stored".into(),
        ));
    }

    let sha256 = hex::encode(hasher.finalize());
    conn.execute(
        "UPDATE files.blobs SET sha256 = ?1 WHERE rowid = ?2",
        rusqlite::params![sha256, rowid],
    )?;
    Ok(sha256)
}

fn rowid_of(conn: &Connection, id: &str) -> Result<i64> {
    ensure_available(conn)?;
    conn.query_row("SELECT rowid FROM files.blobs WHERE id = ?1", [id], |r| {
        r.get(0)
    })
    .optional()?
    .ok_or_else(|| AppError::NotFound("Attachment content not found".into()))
}

/// The whole content of a blob.
pub fn read_all(conn: &Connection, id: &str) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    copy_to(conn, id, &mut out)?;
    Ok(out)
}

/// Stream a blob into `writer`; returns the number of bytes written.
pub fn copy_to<W: Write>(conn: &Connection, id: &str, writer: &mut W) -> Result<u64> {
    let rowid = rowid_of(conn, id)?;
    let mut blob = conn.blob_open(files_schema::SCHEMA, "blobs", "data", rowid, true)?;
    std::io::copy(&mut blob, writer)
        .map_err(|e| AppError::Internal(format!("Failed to read attachment: {e}")))
}

/// The recorded size of a blob, or `None` when it does not exist.
pub fn size_of(conn: &Connection, id: &str) -> Result<Option<u64>> {
    ensure_available(conn)?;
    let size: Option<i64> = conn
        .query_row("SELECT size FROM files.blobs WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(size.map(|s| s as u64))
}

/// True when the blob's content still hashes to `sha256`.
pub fn verify(conn: &Connection, id: &str, sha256: &str) -> Result<bool> {
    let mut hasher = HashWriter(Sha256::new());
    copy_to(conn, id, &mut hasher)?;
    Ok(hex::encode(hasher.0.finalize()) == sha256)
}

struct HashWriter(Sha256);

impl Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Delete a blob; returns whether it existed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    ensure_available(conn)?;
    Ok(conn.execute("DELETE FROM files.blobs WHERE id = ?1", [id])? > 0)
}

/// Delete every blob that no attachment row references (D8); returns how many went.
pub fn delete_unreferenced(conn: &Connection) -> Result<usize> {
    ensure_available(conn)?;
    let referenced = REFERENCES
        .iter()
        .map(|(table, column)| {
            format!("SELECT {column} FROM main.{table} WHERE {column} IS NOT NULL")
        })
        .collect::<Vec<_>>()
        .join(" UNION ");
    Ok(conn.execute(
        &format!("DELETE FROM files.blobs WHERE id NOT IN ({referenced})"),
        [],
    )?)
}

/// Pages one [`incremental_vacuum`] call gives back at most, so it never holds the connection for
/// long; the rest goes at the next unlock.
const VACUUM_PAGES_PER_RUN: u32 = 4096;

/// Give the pages of deleted blobs back to the file system (the store uses incremental
/// auto-vacuum), a bounded number per call.
pub fn incremental_vacuum(conn: &Connection) -> Result<()> {
    ensure_available(conn)?;
    conn.execute_batch(&format!(
        "PRAGMA files.incremental_vacuum({VACUUM_PAGES_PER_RUN});"
    ))?;
    Ok(())
}

/// Attachment rows whose content is missing from the store or does not have its recorded size (a
/// photo counts once even when both its blobs are affected). SQLite answers `length()` of a blob
/// from the record header, without reading the content.
pub fn rows_without_content(conn: &Connection) -> Result<u32> {
    ensure_available(conn)?;
    let intact = |column: &str| {
        format!(
            "EXISTS (SELECT 1 FROM files.blobs b WHERE b.id = {column} AND length(b.data) = b.size)"
        )
    };
    let n: i64 = conn.query_row(
        &format!(
            "SELECT
               (SELECT count(*) FROM main.insurance_documents d
                  WHERE d.blob_id IS NOT NULL AND NOT {doc})
             + (SELECT count(*) FROM main.real_estate_documents d
                  WHERE d.blob_id IS NOT NULL AND NOT {doc})
             + (SELECT count(*) FROM main.real_estate_photos p
                  WHERE p.blob_id IS NOT NULL AND NOT ({full} AND {thumb}))",
            doc = intact("d.blob_id"),
            full = intact("p.blob_id"),
            thumb = intact("p.thumb_blob_id"),
        ),
        [],
        |r| r.get(0),
    )?;
    Ok(n as u32)
}

/// The MIME type of an attachment, from its first bytes and then its file name. Magic bytes win,
/// so a renamed file is still served as what it is.
pub fn sniff_mime(file_name: Option<&str>, head: &[u8]) -> &'static str {
    if head.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return "image/jpeg";
    }
    if head.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if head.starts_with(b"GIF87a") || head.starts_with(b"GIF89a") {
        return "image/gif";
    }
    if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        return "image/webp";
    }
    if head.starts_with(b"%PDF-") {
        return "application/pdf";
    }
    let ext = file_name
        .and_then(|n| n.rsplit_once('.'))
        .map(|(_, e)| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("png") => "image/png",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("heic") => "image/heic",
        Some("pdf") => "application/pdf",
        Some("txt") => "text/plain",
        Some("csv") => "text/csv",
        Some("doc") => "application/msword",
        Some("docx") => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        Some("xls") => "application/vnd.ms-excel",
        Some("xlsx") => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        Some("odt") => "application/vnd.oasis.opendocument.text",
        Some("ods") => "application/vnd.oasis.opendocument.spreadsheet",
        Some("zip") => "application/zip",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A main connection with the store attached in memory and the attachment tables reduced to
    /// their blob columns.
    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        files_schema::migrate(&conn).expect("files schema");
        conn.execute_batch(
            r#"
            CREATE TABLE insurance_documents (id TEXT PRIMARY KEY, blob_id TEXT);
            CREATE TABLE real_estate_documents (id TEXT PRIMARY KEY, blob_id TEXT);
            CREATE TABLE real_estate_photos (id TEXT PRIMARY KEY, blob_id TEXT, thumb_blob_id TEXT);
            CREATE TABLE owners (id TEXT PRIMARY KEY);
            "#,
        )
        .expect("schema");
        conn
    }

    fn sha(bytes: &[u8]) -> String {
        hex::encode(Sha256::digest(bytes))
    }

    #[test]
    fn bytes_round_trip_with_size_and_hash() {
        let conn = setup_test_db();
        let blob = put_bytes(&conn, b"moony").expect("put");
        assert_eq!(blob.size, 5);
        assert_eq!(blob.sha256, sha(b"moony"));
        assert_eq!(read_all(&conn, &blob.id).expect("read"), b"moony".to_vec());
        assert_eq!(size_of(&conn, &blob.id).expect("size"), Some(5));
        assert!(verify(&conn, &blob.id, &blob.sha256).expect("verify"));
        assert!(!verify(&conn, &blob.id, &sha(b"other")).expect("verify"));
    }

    #[test]
    fn a_large_reader_streams_in_and_out() {
        let conn = setup_test_db();
        let payload: Vec<u8> = (0..3 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect();
        let blob = put_reader(&conn, &mut &payload[..], payload.len() as u64).expect("put");
        assert_eq!(blob.sha256, sha(&payload));
        let mut out = Vec::new();
        let n = copy_to(&conn, &blob.id, &mut out).expect("copy");
        assert_eq!(n, payload.len() as u64);
        assert_eq!(out, payload);
    }

    #[test]
    fn an_empty_attachment_is_stored() {
        let conn = setup_test_db();
        let blob = put_bytes(&conn, b"").expect("put");
        assert_eq!(read_all(&conn, &blob.id).expect("read"), Vec::<u8>::new());
    }

    #[test]
    fn a_reader_shorter_or_longer_than_announced_is_refused_and_leaves_nothing() {
        let conn = setup_test_db();
        assert!(put_reader(&conn, &mut &b"abc"[..], 5).is_err());
        assert!(put_reader(&conn, &mut &b"abcdef"[..], 3).is_err());
        let n: i64 = conn
            .query_row("SELECT count(*) FROM files.blobs", [], |r| r.get(0))
            .expect("count");
        assert_eq!(n, 0);
    }

    #[test]
    fn a_missing_blob_is_not_found() {
        let conn = setup_test_db();
        assert!(matches!(
            read_all(&conn, "nope"),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(size_of(&conn, "nope").expect("size"), None);
        assert!(!delete(&conn, "nope").expect("delete"));
    }

    #[test]
    fn without_the_store_every_call_reports_it_unavailable() {
        let conn = Connection::open_in_memory().expect("db");
        let err = put_bytes(&conn, b"x").expect_err("no store");
        assert_eq!(
            err.to_string(),
            "Internal error: attachments.storeUnavailable"
        );
        assert!(read_all(&conn, "x").is_err());
        assert!(delete_unreferenced(&conn).is_err());
    }

    #[test]
    fn a_rollback_undoes_the_blob_and_the_row_together() {
        let conn = setup_test_db();
        let result: Result<()> = (|| {
            let tx = conn.unchecked_transaction()?;
            let blob = put_bytes(&tx, b"content")?;
            tx.execute(
                "INSERT INTO insurance_documents (id, blob_id) VALUES ('d1', ?1)",
                [&blob.id],
            )?;
            // A failing statement after both writes.
            tx.execute("INSERT INTO owners (id) VALUES ('o'), ('o')", [])?;
            tx.commit()?;
            Ok(())
        })();
        assert!(result.is_err());
        let blobs: i64 = conn
            .query_row("SELECT count(*) FROM files.blobs", [], |r| r.get(0))
            .expect("count");
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM insurance_documents", [], |r| r.get(0))
            .expect("count");
        assert_eq!((blobs, rows), (0, 0));
    }

    #[test]
    fn the_sweep_keeps_referenced_blobs_and_deletes_the_rest() {
        let conn = setup_test_db();
        let ids: Vec<String> = (0..5)
            .map(|i| {
                put_bytes(&conn, format!("blob {i}").as_bytes())
                    .expect("put")
                    .id
            })
            .collect();
        conn.execute(
            "INSERT INTO insurance_documents (id, blob_id) VALUES ('a', ?1)",
            [&ids[0]],
        )
        .expect("ins doc");
        conn.execute(
            "INSERT INTO real_estate_documents (id, blob_id) VALUES ('b', ?1)",
            [&ids[1]],
        )
        .expect("re doc");
        conn.execute(
            "INSERT INTO real_estate_photos (id, blob_id, thumb_blob_id) VALUES ('c', ?1, ?2)",
            [&ids[2], &ids[3]],
        )
        .expect("photo");
        conn.execute(
            "INSERT INTO real_estate_photos (id, blob_id, thumb_blob_id) VALUES ('legacy', NULL, NULL)",
            [],
        )
        .expect("legacy photo");

        assert_eq!(delete_unreferenced(&conn).expect("sweep"), 1);
        for id in &ids[..4] {
            assert!(size_of(&conn, id).expect("size").is_some());
        }
        assert!(size_of(&conn, &ids[4]).expect("size").is_none());
        incremental_vacuum(&conn).expect("vacuum");
    }

    #[test]
    fn delete_removes_one_blob() {
        let conn = setup_test_db();
        let a = put_bytes(&conn, b"a").expect("put");
        let b = put_bytes(&conn, b"b").expect("put");
        assert!(delete(&conn, &a.id).expect("delete"));
        assert!(size_of(&conn, &a.id).expect("size").is_none());
        assert!(size_of(&conn, &b.id).expect("size").is_some());
    }

    #[test]
    fn mime_comes_from_magic_bytes_then_the_extension() {
        assert_eq!(
            sniff_mime(Some("scan.pdf"), &[0xFF, 0xD8, 0xFF, 0xE0]),
            "image/jpeg"
        );
        assert_eq!(sniff_mime(None, b"\x89PNG\r\n\x1a\nrest"), "image/png");
        assert_eq!(sniff_mime(None, b"GIF89a..."), "image/gif");
        assert_eq!(
            sniff_mime(None, b"RIFF\x00\x00\x00\x00WEBPVP8 "),
            "image/webp"
        );
        assert_eq!(sniff_mime(Some("x.bin"), b"%PDF-1.4"), "application/pdf");
        assert_eq!(
            sniff_mime(Some("Smlouva.PDF"), b"garbage"),
            "application/pdf"
        );
        assert_eq!(
            sniff_mime(Some("rozpocet.xlsx"), b"PK\x03\x04"),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(sniff_mime(Some("noext"), b"??"), "application/octet-stream");
        assert_eq!(sniff_mime(None, b""), "application/octet-stream");
    }

    #[test]
    fn rows_whose_content_is_missing_or_truncated_are_counted() {
        let conn = setup_test_db();
        let ok = put_bytes(&conn, b"intact").expect("put");
        let truncated = put_bytes(&conn, b"will be cut").expect("put");
        conn.execute(
            "UPDATE files.blobs SET data = x'00' WHERE id = ?1",
            [&truncated.id],
        )
        .expect("truncate");
        conn.execute_batch(&format!(
            "INSERT INTO insurance_documents (id, blob_id) VALUES ('a', '{ok}'), ('b', 'gone');
             INSERT INTO real_estate_documents (id, blob_id) VALUES ('c', '{cut}');
             INSERT INTO real_estate_photos (id, blob_id, thumb_blob_id) VALUES ('p', '{ok}', 'gone-too');
             INSERT INTO real_estate_photos (id, blob_id, thumb_blob_id) VALUES ('legacy', NULL, NULL);",
            ok = ok.id,
            cut = truncated.id
        ))
        .expect("rows");
        assert_eq!(rows_without_content(&conn).expect("count"), 3);
    }
}
