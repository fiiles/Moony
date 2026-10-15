//! Attachments with their owners: where a photo's or a document's bytes are (ADR 0017).
//!
//! A row references either a blob in the encrypted store (`blob_id`) or, until the conversion of
//! plain files from earlier versions has reached it, a legacy file on disk (`file_path`). Every
//! reader of attachment content goes through [`resolve`], so the `moony-file` scheme, exports and
//! opening in another app treat both the same way.

use crate::error::{AppError, Result};
use crate::models::AttachmentKind;
use crate::services::{attachment_store, document_files, real_estate_photos};
use rusqlite::{Connection, OptionalExtension};
use std::fs::{self, File};
use std::path::{Component, Path, PathBuf};

/// Folder of legacy insurance documents under the app data dir.
pub const INSURANCE_DOCUMENTS_DIR: &str = "insurance_documents";
/// Folder of legacy real estate documents under the app data dir.
pub const REAL_ESTATE_DOCUMENTS_DIR: &str = "real_estate_documents";

/// Which rendition of an attachment: photos also have a thumbnail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    Full,
    Thumbnail,
}

/// Where the bytes of an attachment are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Blob(String),
    Legacy(PathBuf),
}

/// An attachment's content location, MIME type and original file name (when known).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub source: Source,
    pub mime: String,
    pub file_name: Option<String>,
}

fn document_table(kind: AttachmentKind) -> Option<(&'static str, &'static str)> {
    match kind {
        AttachmentKind::InsuranceDocument => Some(("insurance_documents", INSURANCE_DOCUMENTS_DIR)),
        AttachmentKind::RealEstateDocument => {
            Some(("real_estate_documents", REAL_ESTATE_DOCUMENTS_DIR))
        }
        AttachmentKind::Photo => None,
    }
}

/// Locate an attachment's content. `Ok(None)` when no such row exists; a `NotFound` error when the
/// row exists but its legacy file is gone.
pub fn resolve(
    conn: &Connection,
    data_dir: &Path,
    kind: AttachmentKind,
    id: &str,
    variant: Variant,
) -> Result<Option<Resolved>> {
    match document_table(kind) {
        Some((table, subdir)) => resolve_document(conn, data_dir, table, subdir, id),
        None => resolve_photo(conn, data_dir, id, variant),
    }
}

type DocumentRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn resolve_document(
    conn: &Connection,
    data_dir: &Path,
    table: &str,
    subdir: &str,
    id: &str,
) -> Result<Option<Resolved>> {
    let row: Option<DocumentRow> = conn
        .query_row(
            &format!(
                "SELECT file_path, blob_id, mime_type, original_name FROM {table} WHERE id = ?1"
            ),
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((file_path, blob_id, mime_type, original_name)) = row else {
        return Ok(None);
    };
    let (mime, _) = document_meta(
        file_path.as_deref(),
        mime_type.as_deref(),
        original_name.as_deref(),
    );
    if let Some(blob_id) = blob_id {
        return Ok(Some(Resolved {
            source: Source::Blob(blob_id),
            mime,
            file_name: original_name,
        }));
    }
    let stored = file_path.ok_or_else(|| {
        AppError::Internal("Attachment row has neither content nor a file".into())
    })?;
    let path = document_files::resolve_document_path(data_dir, subdir, &stored)?;
    Ok(Some(Resolved {
        source: Source::Legacy(path),
        mime,
        file_name: legacy_original_name(&stored),
    }))
}

type PhotoRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
);

fn resolve_photo(
    conn: &Connection,
    data_dir: &Path,
    id: &str,
    variant: Variant,
) -> Result<Option<Resolved>> {
    let row: Option<PhotoRow> = conn
        .query_row(
            "SELECT file_path, thumbnail_path, blob_id, thumb_blob_id
             FROM real_estate_photos WHERE id = ?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((file_path, thumbnail_path, blob_id, thumb_blob_id)) = row else {
        return Ok(None);
    };
    let (blob, legacy) = match variant {
        Variant::Full => (blob_id, file_path),
        Variant::Thumbnail => (thumb_blob_id, thumbnail_path),
    };
    let source = match (blob, legacy) {
        (Some(blob), _) => Source::Blob(blob),
        (None, Some(rel)) => Source::Legacy(legacy_photo_path(data_dir, &rel)?),
        (None, None) => {
            return Err(AppError::Internal(
                "Photo row has neither content nor a file".into(),
            ))
        }
    };
    Ok(Some(Resolved {
        source,
        mime: "image/jpeg".into(),
        file_name: None,
    }))
}

/// A legacy photo path is relative to the data dir and stays inside the photos folder.
fn legacy_photo_path(data_dir: &Path, rel: &str) -> Result<PathBuf> {
    let rel_path = Path::new(rel);
    let inside = rel_path
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
        && rel_path.starts_with(real_estate_photos::PHOTOS_DIR);
    let path = data_dir.join(rel_path);
    if inside && path.is_file() {
        Ok(path)
    } else {
        Err(AppError::NotFound("Photo file is missing on disk".into()))
    }
}

/// The original file name a legacy document kept on disk (`<uuid>_<name>`, used by earlier versions),
/// or `None` for `<uuid>.<ext>` names.
pub fn legacy_original_name(stored_path: &str) -> Option<String> {
    let name = Path::new(stored_path).file_name()?.to_str()?;
    let (prefix, rest) = name.split_at_checked(36)?;
    let original = rest.strip_prefix('_')?;
    (uuid::Uuid::parse_str(prefix).is_ok() && !original.is_empty()).then(|| original.to_string())
}

/// Lowercased extension of a file name (1 to 5 letters or digits), if it has one.
pub fn extension_of(name: &str) -> Option<String> {
    let file_name = Path::new(name).file_name()?.to_str()?;
    let (stem, ext) = file_name.rsplit_once('.')?;
    let valid = !stem.is_empty()
        && (1..=5).contains(&ext.len())
        && ext.chars().all(|c| c.is_ascii_alphanumeric());
    valid.then(|| ext.to_ascii_lowercase())
}

/// MIME type and extension of a document row for the wire: the stored MIME type, or the one the
/// original (or legacy) file name implies.
pub fn document_meta(
    file_path: Option<&str>,
    mime_type: Option<&str>,
    original_name: Option<&str>,
) -> (String, Option<String>) {
    let name = original_name.or(file_path);
    let extension = name.and_then(extension_of);
    let mime = match mime_type {
        Some(m) => m.to_string(),
        None => attachment_store::sniff_mime(name, &[]).to_string(),
    };
    (mime, extension)
}

/// The whole content of a resolved attachment.
pub fn read_bytes(conn: &Connection, resolved: &Resolved) -> Result<Vec<u8>> {
    match &resolved.source {
        Source::Blob(id) => attachment_store::read_all(conn, id),
        Source::Legacy(path) => fs::read(path)
            .map_err(|e| AppError::NotFound(format!("Attachment file cannot be read: {e}"))),
    }
}

/// Write a resolved attachment to `dest` (a path the user chose).
pub fn write_to(conn: &Connection, resolved: &Resolved, dest: &Path) -> Result<()> {
    match &resolved.source {
        Source::Blob(id) => {
            let mut file = File::create(dest)
                .map_err(|e| AppError::Internal(format!("Failed to create the file: {e}")))?;
            attachment_store::copy_to(conn, id, &mut file)?;
            file.sync_all()
                .map_err(|e| AppError::Internal(format!("Failed to write the file: {e}")))?;
        }
        Source::Legacy(path) => {
            fs::copy(path, dest)
                .map_err(|e| AppError::Internal(format!("Failed to copy the file: {e}")))?;
        }
    }
    Ok(())
}

/// Save a decrypted copy of an attachment where the user chose.
pub fn export(
    conn: &Connection,
    data_dir: &Path,
    kind: AttachmentKind,
    id: &str,
    dest: &Path,
) -> Result<()> {
    let resolved = resolve(conn, data_dir, kind, id, Variant::Full)?
        .ok_or_else(|| AppError::NotFound("Attachment not found".into()))?;
    write_to(conn, &resolved, dest)
}

/// Save every photo of a batch into `dest_dir` as `batch_<YYYY-MM-DD>_<first 8 of id>.jpg`;
/// returns how many were saved.
pub fn export_photo_batch(
    conn: &Connection,
    data_dir: &Path,
    batch_id: &str,
    dest_dir: &Path,
) -> Result<u32> {
    let photo_date: i64 = conn
        .query_row(
            "SELECT photo_date FROM real_estate_photo_batches WHERE id = ?1",
            [batch_id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::NotFound("Batch not found".into()))?;
    let day = chrono::DateTime::from_timestamp(photo_date, 0)
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "undated".into());
    let ids: Vec<String> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM real_estate_photos WHERE batch_id = ?1 ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([batch_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    let mut saved = 0;
    for id in ids {
        let resolved = resolve(conn, data_dir, AttachmentKind::Photo, &id, Variant::Full)?
            .ok_or_else(|| AppError::NotFound("Photo not found".into()))?;
        let short: String = id.chars().take(8).collect();
        write_to(
            conn,
            &resolved,
            &dest_dir.join(format!("batch_{day}_{short}.jpg")),
        )?;
        saved += 1;
    }
    Ok(saved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::files_schema;

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
            CREATE TABLE real_estate_photo_batches (
                id TEXT PRIMARY KEY, real_estate_id TEXT, photo_date INTEGER NOT NULL
            );
            CREATE TABLE real_estate_photos (
                id TEXT PRIMARY KEY, batch_id TEXT, file_path TEXT, thumbnail_path TEXT,
                blob_id TEXT, thumb_blob_id TEXT, created_at INTEGER NOT NULL DEFAULT 0
            );
            "#,
        )
        .expect("schema");
        conn
    }

    const UUID: &str = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60";

    #[test]
    fn a_document_with_a_blob_resolves_to_the_store() {
        let conn = setup_test_db();
        let dir = tempfile::tempdir().expect("tempdir");
        let blob = attachment_store::put_bytes(&conn, b"%PDF-1.7 x").expect("put");
        conn.execute(
            "INSERT INTO insurance_documents (id, name, blob_id, mime_type, original_name)
             VALUES ('d', 'Smlouva', ?1, 'application/pdf', 'Smlouva.pdf')",
            [&blob.id],
        )
        .expect("row");

        let resolved = resolve(
            &conn,
            dir.path(),
            AttachmentKind::InsuranceDocument,
            "d",
            Variant::Full,
        )
        .expect("resolve")
        .expect("row exists");
        assert_eq!(resolved.source, Source::Blob(blob.id));
        assert_eq!(resolved.mime, "application/pdf");
        assert_eq!(resolved.file_name.as_deref(), Some("Smlouva.pdf"));
        assert_eq!(
            read_bytes(&conn, &resolved).expect("bytes"),
            b"%PDF-1.7 x".to_vec()
        );
    }

    #[test]
    fn a_legacy_document_resolves_to_its_file_under_the_current_data_dir() {
        let conn = setup_test_db();
        let dir = tempfile::tempdir().expect("tempdir");
        let owner = dir.path().join(REAL_ESTATE_DOCUMENTS_DIR).join("re-1");
        fs::create_dir_all(&owner).expect("mkdir");
        let file_name = format!("{UUID}_Kupní smlouva.pdf");
        fs::write(owner.join(&file_name), b"legacy").expect("write");
        let stale =
            format!("/old/com.filipkral.moony/{REAL_ESTATE_DOCUMENTS_DIR}/re-1/{file_name}");
        conn.execute(
            "INSERT INTO real_estate_documents (id, name, file_path) VALUES ('d', 'List', ?1)",
            [&stale],
        )
        .expect("row");

        let resolved = resolve(
            &conn,
            dir.path(),
            AttachmentKind::RealEstateDocument,
            "d",
            Variant::Full,
        )
        .expect("resolve")
        .expect("row exists");
        assert_eq!(resolved.source, Source::Legacy(owner.join(&file_name)));
        assert_eq!(resolved.mime, "application/pdf");
        assert_eq!(resolved.file_name.as_deref(), Some("Kupní smlouva.pdf"));
        assert_eq!(
            read_bytes(&conn, &resolved).expect("bytes"),
            b"legacy".to_vec()
        );
    }

    #[test]
    fn an_unknown_id_is_none_and_a_missing_legacy_file_is_not_found() {
        let conn = setup_test_db();
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(resolve(
            &conn,
            dir.path(),
            AttachmentKind::InsuranceDocument,
            "nope",
            Variant::Full
        )
        .expect("resolve")
        .is_none());
        conn.execute(
            "INSERT INTO insurance_documents (id, name, file_path) VALUES ('gone', 'x', '/old/a/b/c.pdf')",
            [],
        )
        .expect("row");
        assert!(matches!(
            resolve(
                &conn,
                dir.path(),
                AttachmentKind::InsuranceDocument,
                "gone",
                Variant::Full
            ),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn a_photo_resolves_its_full_image_and_thumbnail() {
        let conn = setup_test_db();
        let dir = tempfile::tempdir().expect("tempdir");
        let full = attachment_store::put_bytes(&conn, b"full").expect("full");
        let thumb = attachment_store::put_bytes(&conn, b"thumb").expect("thumb");
        conn.execute(
            "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id) VALUES ('p', 'b', ?1, ?2)",
            [&full.id, &thumb.id],
        )
        .expect("row");
        let get = |v| {
            let r = resolve(&conn, dir.path(), AttachmentKind::Photo, "p", v)
                .expect("resolve")
                .expect("row");
            read_bytes(&conn, &r).expect("bytes")
        };
        assert_eq!(get(Variant::Full), b"full".to_vec());
        assert_eq!(get(Variant::Thumbnail), b"thumb".to_vec());
    }

    #[test]
    fn a_legacy_photo_must_stay_inside_the_photos_folder() {
        let conn = setup_test_db();
        let dir = tempfile::tempdir().expect("tempdir");
        let batch = dir.path().join("real_estate_photos/re/b");
        fs::create_dir_all(&batch).expect("mkdir");
        fs::write(batch.join("p.jpg"), b"jpeg").expect("photo");
        fs::write(batch.join("p_thumb.jpg"), b"thumb").expect("thumb");
        fs::write(dir.path().join("moony.db"), b"secret").expect("db");
        conn.execute_batch(
            "INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path) VALUES
               ('ok', 'b', 'real_estate_photos/re/b/p.jpg', 'real_estate_photos/re/b/p_thumb.jpg'),
               ('escape', 'b', 'real_estate_photos/../moony.db', 'moony.db'),
               ('absolute', 'b', '/etc/hosts', '/etc/hosts');",
        )
        .expect("rows");
        let ok = resolve(
            &conn,
            dir.path(),
            AttachmentKind::Photo,
            "ok",
            Variant::Thumbnail,
        )
        .expect("resolve")
        .expect("row");
        assert_eq!(read_bytes(&conn, &ok).expect("bytes"), b"thumb".to_vec());
        for id in ["escape", "absolute"] {
            assert!(matches!(
                resolve(&conn, dir.path(), AttachmentKind::Photo, id, Variant::Full),
                Err(AppError::NotFound(_))
            ));
        }
    }

    #[test]
    fn export_writes_blob_and_legacy_content_to_the_chosen_path() {
        let conn = setup_test_db();
        let dir = tempfile::tempdir().expect("tempdir");
        let blob = attachment_store::put_bytes(&conn, b"encrypted content").expect("put");
        let legacy_dir = dir.path().join(INSURANCE_DOCUMENTS_DIR).join("pol");
        fs::create_dir_all(&legacy_dir).expect("mkdir");
        fs::write(legacy_dir.join("x.pdf"), b"plain content").expect("write");
        conn.execute(
            "INSERT INTO insurance_documents (id, name, blob_id, mime_type) VALUES ('enc', 'a', ?1, 'application/pdf')",
            [&blob.id],
        )
        .expect("enc");
        conn.execute(
            "INSERT INTO insurance_documents (id, name, file_path) VALUES ('old', 'b', ?1)",
            [legacy_dir.join("x.pdf").to_str().expect("utf8")],
        )
        .expect("old");

        let out = dir.path().join("out");
        fs::create_dir_all(&out).expect("out");
        export(
            &conn,
            dir.path(),
            AttachmentKind::InsuranceDocument,
            "enc",
            &out.join("a.pdf"),
        )
        .expect("export enc");
        export(
            &conn,
            dir.path(),
            AttachmentKind::InsuranceDocument,
            "old",
            &out.join("b.pdf"),
        )
        .expect("export old");
        assert_eq!(
            fs::read(out.join("a.pdf")).expect("a"),
            b"encrypted content".to_vec()
        );
        assert_eq!(
            fs::read(out.join("b.pdf")).expect("b"),
            b"plain content".to_vec()
        );
        assert!(matches!(
            export(
                &conn,
                dir.path(),
                AttachmentKind::InsuranceDocument,
                "nope",
                &out.join("c.pdf")
            ),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn a_batch_exports_every_photo_with_the_dated_name() {
        let conn = setup_test_db();
        let dir = tempfile::tempdir().expect("tempdir");
        // 2026-03-14 12:00 UTC
        conn.execute(
            "INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) VALUES ('b', 're', 1773489600)",
            [],
        )
        .expect("batch");
        for (id, bytes) in [
            ("aaaaaaaa-1111", b"one".as_slice()),
            ("bbbbbbbb-2222", b"two"),
        ] {
            let full = attachment_store::put_bytes(&conn, bytes).expect("full");
            let thumb = attachment_store::put_bytes(&conn, b"t").expect("thumb");
            conn.execute(
                "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id) VALUES (?1, 'b', ?2, ?3)",
                [id, &full.id, &thumb.id],
            )
            .expect("photo");
        }
        let out = tempfile::tempdir().expect("out");
        assert_eq!(
            export_photo_batch(&conn, dir.path(), "b", out.path()).expect("export"),
            2
        );
        assert_eq!(
            fs::read(out.path().join("batch_2026-03-14_aaaaaaaa.jpg")).expect("one"),
            b"one".to_vec()
        );
        assert_eq!(
            fs::read(out.path().join("batch_2026-03-14_bbbbbbbb.jpg")).expect("two"),
            b"two".to_vec()
        );
        assert!(matches!(
            export_photo_batch(&conn, dir.path(), "missing", out.path()),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn names_and_metadata_for_the_wire() {
        assert_eq!(
            legacy_original_name(&format!("/x/insurance_documents/p/{UUID}_Smlouva 2024.pdf"))
                .as_deref(),
            Some("Smlouva 2024.pdf")
        );
        assert_eq!(legacy_original_name(&format!("/x/{UUID}.pdf")), None);
        assert_eq!(legacy_original_name("/x/short_name.pdf"), None);

        assert_eq!(extension_of("Smlouva.PDF").as_deref(), Some("pdf"));
        assert_eq!(extension_of("archive.tar.gz").as_deref(), Some("gz"));
        assert_eq!(extension_of("noext"), None);
        assert_eq!(extension_of(".hidden"), None);
        assert_eq!(extension_of("weird.toolongext"), None);

        assert_eq!(
            document_meta(None, Some("application/pdf"), Some("a.pdf")),
            ("application/pdf".to_string(), Some("pdf".to_string()))
        );
        assert_eq!(
            document_meta(Some(&format!("/d/{UUID}.docx")), None, None),
            (
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
                    .to_string(),
                Some("docx".to_string())
            )
        );
        assert_eq!(
            document_meta(Some("/d/file"), None, None),
            ("application/octet-stream".to_string(), None)
        );
    }
}
