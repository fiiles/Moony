//! Attachments with their owners: where a photo's or a document's bytes are (ADR 0017).
//!
//! A row references either a blob in the encrypted store (`blob_id`) or, until the conversion of
//! plain files from earlier versions has reached it, a legacy file on disk (`file_path`). Every
//! reader of attachment content goes through [`resolve`], so the `moony-file` scheme, exports and
//! opening in another app treat both the same way.
//!
//! New documents go straight into the store ([`add_insurance_document`],
//! [`add_real_estate_document`]). Deleting a row deletes its blob in the same transaction and its
//! legacy file after the commit; a legacy file outside the current data folder is never deleted.

use crate::error::{AppError, Result};
use crate::models::{AttachmentKind, InsuranceDocument, RealEstateDocument};
use crate::services::{attachment_store, document_files, real_estate_photos};
use rusqlite::{Connection, OptionalExtension};
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

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
pub fn legacy_photo_path(data_dir: &Path, rel: &str) -> Result<PathBuf> {
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

// ============================================================================
// Opening in another app
// ============================================================================

/// Folder under the app cache dir that holds decrypted copies opened in another app.
pub const OPEN_COPIES_DIR: &str = "open";

/// A file name that is safe on every platform: no separators, control or reserved characters, no
/// trailing dots or spaces, not a reserved Windows device name, at most 120 characters with the
/// extension kept.
pub fn sanitize_file_name(name: &str) -> String {
    const RESERVED: [&str; 22] = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim_matches(|c: char| c == ' ' || c == '.');
    let mut out = if trimmed.is_empty() {
        "document".to_string()
    } else {
        trimmed.to_string()
    };
    let stem = out
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    const MAX_CHARS: usize = 120;
    if out.chars().count() > MAX_CHARS {
        let ext = extension_of(&out)
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let keep = MAX_CHARS - ext.chars().count();
        let stem: String = out.chars().take(keep).collect();
        out = format!("{}{ext}", stem.trim_end_matches(['.', ' ']));
    }
    out
}

/// The usual extension of a MIME type, for a copy whose original name is unknown.
fn extension_for_mime(mime: &str) -> Option<&'static str> {
    Some(match mime {
        "application/pdf" => "pdf",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/heic" => "heic",
        "text/plain" => "txt",
        "text/csv" => "csv",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.oasis.opendocument.text" => "odt",
        "application/vnd.oasis.opendocument.spreadsheet" => "ods",
        "application/zip" => "zip",
        _ => return None,
    })
}

/// The name a document's copy gets: its original file name, or its display name with the
/// extension its type implies.
fn copy_file_name(
    conn: &Connection,
    kind: AttachmentKind,
    id: &str,
    resolved: &Resolved,
) -> Result<String> {
    if let Some(original) = &resolved.file_name {
        return Ok(sanitize_file_name(original));
    }
    let display: String = match document_table(kind) {
        Some((table, _)) => conn
            .query_row(
                &format!("SELECT name FROM {table} WHERE id = ?1"),
                [id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or_else(|| "document".into()),
        None => format!("photo_{}", id.chars().take(8).collect::<String>()),
    };
    let ext = match &resolved.source {
        Source::Legacy(path) => path.to_str().and_then(extension_of),
        Source::Blob(_) => None,
    }
    .or_else(|| extension_for_mime(&resolved.mime).map(str::to_string));
    Ok(sanitize_file_name(&match ext {
        Some(ext) => format!("{display}.{ext}"),
        None => display,
    }))
}

/// Write a decrypted copy of a document for another app to open: a fresh private folder under
/// `<cache dir>/open/`, readable by the user only. [`wipe_open_copies`] removes it again on lock,
/// on exit and at the next start (D3).
pub fn open_elsewhere(
    conn: &Connection,
    data_dir: &Path,
    cache_dir: &Path,
    kind: AttachmentKind,
    id: &str,
) -> Result<PathBuf> {
    let resolved = resolve(conn, data_dir, kind, id, Variant::Full)?
        .ok_or_else(|| AppError::NotFound("Attachment not found".into()))?;
    let dir = cache_dir
        .join(OPEN_COPIES_DIR)
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Internal(format!("Failed to create a folder for the copy: {e}")))?;
    restrict_permissions(&dir, 0o700);
    let path = dir.join(copy_file_name(conn, kind, id, &resolved)?);
    write_to(conn, &resolved, &path)?;
    restrict_permissions(&path, 0o600);
    Ok(path)
}

#[cfg(unix)]
fn restrict_permissions(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    if let Err(e) = fs::set_permissions(path, fs::Permissions::from_mode(mode)) {
        log::warn!("[ATTACHMENTS] Could not restrict a temporary copy's permissions: {e}");
    }
}

#[cfg(not(unix))]
fn restrict_permissions(_path: &Path, _mode: u32) {}

/// Delete every decrypted copy opened in another app. A file another app still holds (Windows)
/// stays until the next wipe.
pub fn wipe_open_copies(cache_dir: &Path) {
    let dir = cache_dir.join(OPEN_COPIES_DIR);
    if dir.exists() {
        if let Err(e) = fs::remove_dir_all(&dir) {
            log::warn!("[ATTACHMENTS] Some temporary copies could not be deleted yet: {e}");
        }
    }
}

// ============================================================================
// Adding documents
// ============================================================================

/// What the user entered for a new document.
pub struct NewDocument<'a> {
    pub name: &'a str,
    pub description: Option<&'a str>,
    pub file_type: &'a str,
}

struct StoredDocument {
    id: String,
    mime_type: String,
    extension: Option<String>,
    file_size: i64,
}

/// Stream the file the user picked into the store and insert its row, in one transaction.
fn store_document(
    conn: &Connection,
    table: &str,
    owner_column: &str,
    owner_id: &str,
    source: &Path,
    doc: &NewDocument,
    now: i64,
) -> Result<StoredDocument> {
    let original_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("document")
        .to_string();
    let len = fs::metadata(source)
        .map_err(|e| AppError::Internal(format!("Cannot read the file: {e}")))?
        .len();
    if len > attachment_store::MAX_DOCUMENT_BYTES {
        return Err(AppError::Validation("validation.attachmentTooLarge".into()));
    }
    let mut file =
        File::open(source).map_err(|e| AppError::Internal(format!("Cannot open the file: {e}")))?;
    let head = read_head(&mut file)?;
    let mime_type = attachment_store::sniff_mime(Some(&original_name), &head).to_string();

    let id = Uuid::new_v4().to_string();
    let tx = conn.unchecked_transaction()?;
    let blob = attachment_store::put_reader(&tx, &mut file, len)?;
    tx.execute(
        &format!(
            "INSERT INTO {table} (id, {owner_column}, name, description, blob_id, mime_type,
                                  original_name, file_type, file_size, uploaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)"
        ),
        rusqlite::params![
            id,
            owner_id,
            doc.name,
            doc.description,
            blob.id,
            mime_type,
            original_name,
            doc.file_type,
            len as i64,
            now
        ],
    )?;
    tx.commit()?;
    Ok(StoredDocument {
        id,
        mime_type,
        extension: extension_of(&original_name),
        file_size: len as i64,
    })
}

/// The first bytes of a file (for MIME sniffing), with the file rewound to its start.
fn read_head(file: &mut File) -> Result<Vec<u8>> {
    let mut head = vec![0u8; 16];
    let mut filled = 0;
    while filled < head.len() {
        let n = file
            .read(&mut head[filled..])
            .map_err(|e| AppError::Internal(format!("Cannot read the file: {e}")))?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    head.truncate(filled);
    file.seek(SeekFrom::Start(0))
        .map_err(|e| AppError::Internal(format!("Cannot read the file: {e}")))?;
    Ok(head)
}

/// Attach a document to an insurance policy.
pub fn add_insurance_document(
    conn: &Connection,
    insurance_id: &str,
    source: &Path,
    doc: &NewDocument,
    now: i64,
) -> Result<InsuranceDocument> {
    let stored = store_document(
        conn,
        "insurance_documents",
        "insurance_id",
        insurance_id,
        source,
        doc,
        now,
    )?;
    Ok(InsuranceDocument {
        id: stored.id,
        insurance_id: insurance_id.to_string(),
        name: doc.name.to_string(),
        description: doc.description.map(str::to_string),
        mime_type: stored.mime_type,
        extension: stored.extension,
        file_type: doc.file_type.to_string(),
        file_size: Some(stored.file_size),
        uploaded_at: now,
    })
}

/// Attach a document to a property.
pub fn add_real_estate_document(
    conn: &Connection,
    real_estate_id: &str,
    source: &Path,
    doc: &NewDocument,
    now: i64,
) -> Result<RealEstateDocument> {
    let stored = store_document(
        conn,
        "real_estate_documents",
        "real_estate_id",
        real_estate_id,
        source,
        doc,
        now,
    )?;
    Ok(RealEstateDocument {
        id: stored.id,
        real_estate_id: real_estate_id.to_string(),
        name: doc.name.to_string(),
        description: doc.description.map(str::to_string),
        mime_type: stored.mime_type,
        extension: stored.extension,
        file_type: doc.file_type.to_string(),
        file_size: Some(stored.file_size),
        uploaded_at: now,
    })
}

// ============================================================================
// Deleting
// ============================================================================

/// The content of attachment rows that are being deleted: their blobs, deleted inside the
/// caller's transaction, and their legacy files, removed after the commit.
#[derive(Debug, Default)]
pub struct Released {
    blobs: Vec<String>,
    files: Vec<PathBuf>,
}

impl Released {
    /// Delete the blobs; call inside the transaction that deletes the rows.
    pub fn delete_blobs(&self, conn: &Connection) -> Result<()> {
        for id in &self.blobs {
            attachment_store::delete(conn, id)?;
        }
        Ok(())
    }

    /// Remove the legacy files after the commit, then any folder that became empty. Only files
    /// inside the current data folder are removed (D5); a failure leaves a file for the next
    /// conversion run to report, never an error for the user.
    pub fn remove_files(&self, data_dir: &Path) {
        for file in &self.files {
            if !file.starts_with(data_dir) {
                continue;
            }
            let _ = fs::remove_file(file);
            let mut dir = file.parent();
            while let Some(d) = dir {
                if d == data_dir || !d.starts_with(data_dir) || fs::remove_dir(d).is_err() {
                    break;
                }
                dir = d.parent();
            }
        }
    }

    fn extend(&mut self, other: Released) {
        self.blobs.extend(other.blobs);
        self.files.extend(other.files);
    }
}

/// Blobs and legacy files of the document rows `where_sql` selects.
fn released_documents(
    conn: &Connection,
    data_dir: &Path,
    kind: AttachmentKind,
    where_sql: &str,
    param: &str,
) -> Result<Released> {
    let (table, subdir) =
        document_table(kind).ok_or_else(|| AppError::Internal("Not a document kind".into()))?;
    let mut stmt = conn.prepare(&format!(
        "SELECT blob_id, file_path FROM {table} WHERE {where_sql}"
    ))?;
    let rows = stmt.query_map([param], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
        ))
    })?;
    let mut released = Released::default();
    for row in rows {
        let (blob, file_path) = row?;
        released.blobs.extend(blob);
        if let Some(stored) = file_path {
            if let Ok(path) = document_files::resolve_document_path(data_dir, subdir, &stored) {
                released.files.push(path);
            }
        }
    }
    Ok(released)
}

/// Blobs and legacy files of the photo rows `where_sql` selects.
fn released_photos(
    conn: &Connection,
    data_dir: &Path,
    where_sql: &str,
    param: &str,
) -> Result<Released> {
    let mut stmt = conn.prepare(&format!(
        "SELECT blob_id, thumb_blob_id, file_path, thumbnail_path FROM real_estate_photos
         WHERE {where_sql}"
    ))?;
    let rows = stmt.query_map([param], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;
    let mut released = Released::default();
    for row in rows {
        let (blob, thumb_blob, file, thumb_file) = row?;
        released.blobs.extend(blob);
        released.blobs.extend(thumb_blob);
        for rel in [file, thumb_file].into_iter().flatten() {
            if let Ok(path) = legacy_photo_path(data_dir, &rel) {
                released.files.push(path);
            }
        }
    }
    Ok(released)
}

/// Delete one document row with its content.
pub fn delete_document(
    conn: &Connection,
    data_dir: &Path,
    kind: AttachmentKind,
    id: &str,
) -> Result<()> {
    let (table, _) =
        document_table(kind).ok_or_else(|| AppError::Internal("Not a document kind".into()))?;
    let released = released_documents(conn, data_dir, kind, "id = ?1", id)?;
    let tx = conn.unchecked_transaction()?;
    if tx.execute(&format!("DELETE FROM {table} WHERE id = ?1"), [id])? == 0 {
        return Err(AppError::NotFound("Document not found".into()));
    }
    released.delete_blobs(&tx)?;
    tx.commit()?;
    released.remove_files(data_dir);
    Ok(())
}

/// Delete one photo with its content.
pub fn delete_photo(conn: &Connection, data_dir: &Path, id: &str) -> Result<()> {
    let released = released_photos(conn, data_dir, "id = ?1", id)?;
    let tx = conn.unchecked_transaction()?;
    if tx.execute("DELETE FROM real_estate_photos WHERE id = ?1", [id])? == 0 {
        return Err(AppError::NotFound("Photo not found".into()));
    }
    released.delete_blobs(&tx)?;
    tx.commit()?;
    released.remove_files(data_dir);
    Ok(())
}

/// Delete a photo batch with every photo in it.
pub fn delete_photo_batch(conn: &Connection, data_dir: &Path, batch_id: &str) -> Result<()> {
    let released = released_photos(conn, data_dir, "batch_id = ?1", batch_id)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "DELETE FROM real_estate_photos WHERE batch_id = ?1",
        [batch_id],
    )?;
    if tx.execute(
        "DELETE FROM real_estate_photo_batches WHERE id = ?1",
        [batch_id],
    )? == 0
    {
        return Err(AppError::NotFound("Batch not found".into()));
    }
    released.delete_blobs(&tx)?;
    tx.commit()?;
    released.remove_files(data_dir);
    Ok(())
}

/// Everything attached to a property (its documents and the photos of all its batches), for a
/// caller that deletes the property.
pub fn release_real_estate(
    conn: &Connection,
    data_dir: &Path,
    real_estate_id: &str,
) -> Result<Released> {
    let mut released = released_documents(
        conn,
        data_dir,
        AttachmentKind::RealEstateDocument,
        "real_estate_id = ?1",
        real_estate_id,
    )?;
    released.extend(released_photos(
        conn,
        data_dir,
        "batch_id IN (SELECT id FROM real_estate_photo_batches WHERE real_estate_id = ?1)",
        real_estate_id,
    )?);
    Ok(released)
}

/// Everything attached to an insurance policy, for a caller that deletes the policy.
pub fn release_insurance(
    conn: &Connection,
    data_dir: &Path,
    insurance_id: &str,
) -> Result<Released> {
    released_documents(
        conn,
        data_dir,
        AttachmentKind::InsuranceDocument,
        "insurance_id = ?1",
        insurance_id,
    )
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
                id TEXT PRIMARY KEY, insurance_id TEXT, name TEXT, description TEXT, file_path TEXT,
                blob_id TEXT, mime_type TEXT, original_name TEXT, file_type TEXT, file_size INTEGER,
                uploaded_at INTEGER
            );
            CREATE TABLE real_estate_documents (
                id TEXT PRIMARY KEY, real_estate_id TEXT, name TEXT, description TEXT, file_path TEXT,
                blob_id TEXT, mime_type TEXT, original_name TEXT, file_type TEXT, file_size INTEGER,
                uploaded_at INTEGER
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

    fn blob_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT count(*) FROM files.blobs", [], |r| r.get(0))
            .expect("count")
    }

    const DOC: NewDocument = NewDocument {
        name: "Smlouva",
        description: Some("2024"),
        file_type: "contract",
    };

    #[test]
    fn an_added_document_goes_into_the_store_with_its_type_and_name() {
        let conn = setup_test_db();
        let data_dir = tempfile::tempdir().expect("data dir");
        let picked = tempfile::tempdir().expect("picked");
        let source = picked.path().join("Pojistná smlouva.PDF");
        fs::write(&source, b"%PDF-1.7 policy").expect("source");

        let doc = add_insurance_document(&conn, "pol", &source, &DOC, 1_700_000_000).expect("add");
        assert_eq!(doc.mime_type, "application/pdf");
        assert_eq!(doc.extension.as_deref(), Some("pdf"));
        assert_eq!(doc.file_size, Some(15));
        assert_eq!(doc.file_type, "contract");

        let row: (Option<String>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT file_path, original_name, blob_id FROM insurance_documents WHERE id = ?1",
                [&doc.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("row");
        assert_eq!(row.0, None);
        assert_eq!(row.1.as_deref(), Some("Pojistná smlouva.PDF"));
        let resolved = resolve(
            &conn,
            data_dir.path(),
            AttachmentKind::InsuranceDocument,
            &doc.id,
            Variant::Full,
        )
        .expect("resolve")
        .expect("row");
        assert_eq!(
            read_bytes(&conn, &resolved).expect("bytes"),
            b"%PDF-1.7 policy".to_vec()
        );
        assert!(fs::read_dir(data_dir.path()).expect("dir").next().is_none());

        let re = add_real_estate_document(&conn, "re", &source, &DOC, 0).expect("add");
        assert_eq!(re.real_estate_id, "re");
        assert_eq!(blob_count(&conn), 2);
    }

    #[test]
    fn a_document_over_the_cap_is_refused_before_anything_is_written() {
        let conn = setup_test_db();
        let picked = tempfile::tempdir().expect("picked");
        let source = picked.path().join("huge.pdf");
        File::create(&source)
            .and_then(|f| f.set_len(attachment_store::MAX_DOCUMENT_BYTES + 1))
            .expect("sparse file");
        let err = add_insurance_document(&conn, "pol", &source, &DOC, 0).expect_err("too large");
        assert_eq!(
            err.keyed().map(|(k, _)| k).as_deref(),
            Some("validation.attachmentTooLarge")
        );
        assert_eq!(blob_count(&conn), 0);
    }

    #[test]
    fn deleting_a_document_deletes_its_blob_or_its_file_inside_the_data_dir_only() {
        let conn = setup_test_db();
        let data_dir = tempfile::tempdir().expect("data dir");
        let picked = tempfile::tempdir().expect("picked");
        let source = picked.path().join("a.pdf");
        fs::write(&source, b"%PDF-1.7").expect("source");
        let enc = add_insurance_document(&conn, "pol", &source, &DOC, 0).expect("add");

        let inside_dir = data_dir.path().join(INSURANCE_DOCUMENTS_DIR).join("pol");
        fs::create_dir_all(&inside_dir).expect("mkdir");
        fs::write(inside_dir.join("in.pdf"), b"inside").expect("inside");
        let outside_dir = tempfile::tempdir().expect("old folder");
        let outside_file = outside_dir.path().join("x").join("out.pdf");
        fs::create_dir_all(outside_file.parent().expect("parent")).expect("mkdir");
        fs::write(&outside_file, b"outside").expect("outside");
        conn.execute_batch(&format!(
            "INSERT INTO insurance_documents (id, insurance_id, name, file_path) VALUES
               ('in', 'pol', 'in', '{}'), ('out', 'pol', 'out', '{}');",
            inside_dir.join("in.pdf").display(),
            outside_file.display()
        ))
        .expect("legacy rows");

        delete_document(
            &conn,
            data_dir.path(),
            AttachmentKind::InsuranceDocument,
            &enc.id,
        )
        .expect("delete enc");
        assert_eq!(blob_count(&conn), 0);
        delete_document(
            &conn,
            data_dir.path(),
            AttachmentKind::InsuranceDocument,
            "in",
        )
        .expect("delete in");
        assert!(!inside_dir.join("in.pdf").exists());
        assert!(!inside_dir.exists(), "the emptied owner folder goes too");
        delete_document(
            &conn,
            data_dir.path(),
            AttachmentKind::InsuranceDocument,
            "out",
        )
        .expect("delete out");
        assert!(
            outside_file.exists(),
            "a file outside the data folder is never deleted"
        );

        let rows: i64 = conn
            .query_row("SELECT count(*) FROM insurance_documents", [], |r| r.get(0))
            .expect("count");
        assert_eq!(rows, 0);
        assert!(matches!(
            delete_document(
                &conn,
                data_dir.path(),
                AttachmentKind::InsuranceDocument,
                "in"
            ),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn deleting_photos_and_batches_releases_blobs_and_legacy_files() {
        let conn = setup_test_db();
        let data_dir = tempfile::tempdir().expect("data dir");
        conn.execute_batch(
            "INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) VALUES ('b1', 're', 0), ('b2', 're', 0);",
        )
        .expect("batches");
        let blob = |bytes: &[u8]| attachment_store::put_bytes(&conn, bytes).expect("put").id;
        let (f1, t1, f2, t2) = (blob(b"f1"), blob(b"t1"), blob(b"f2"), blob(b"t2"));
        conn.execute(
            "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id) VALUES ('p1', 'b1', ?1, ?2)",
            [&f1, &t1],
        )
        .expect("p1");
        conn.execute(
            "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id) VALUES ('p2', 'b2', ?1, ?2)",
            [&f2, &t2],
        )
        .expect("p2");
        let legacy = data_dir.path().join("real_estate_photos/re/b2");
        fs::create_dir_all(&legacy).expect("mkdir");
        fs::write(legacy.join("p.jpg"), b"j").expect("photo");
        fs::write(legacy.join("p_thumb.jpg"), b"t").expect("thumb");
        conn.execute(
            "INSERT INTO real_estate_photos (id, batch_id, file_path, thumbnail_path)
             VALUES ('p3', 'b2', 'real_estate_photos/re/b2/p.jpg', 'real_estate_photos/re/b2/p_thumb.jpg')",
            [],
        )
        .expect("p3");

        delete_photo(&conn, data_dir.path(), "p1").expect("delete photo");
        assert_eq!(blob_count(&conn), 2);
        delete_photo_batch(&conn, data_dir.path(), "b2").expect("delete batch");
        assert_eq!(blob_count(&conn), 0);
        assert!(!legacy.exists());
        let photos: i64 = conn
            .query_row("SELECT count(*) FROM real_estate_photos", [], |r| r.get(0))
            .expect("count");
        assert_eq!(photos, 0);
        assert!(matches!(
            delete_photo_batch(&conn, data_dir.path(), "b2"),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            delete_photo(&conn, data_dir.path(), "p1"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn file_names_are_made_safe_everywhere() {
        assert_eq!(sanitize_file_name("Kupní smlouva.pdf"), "Kupní smlouva.pdf");
        assert_eq!(sanitize_file_name("../../etc/passwd"), "_.._etc_passwd");
        assert_eq!(
            sanitize_file_name("a:b*c?d\"e<f>g|h.pdf"),
            "a_b_c_d_e_f_g_h.pdf"
        );
        assert_eq!(sanitize_file_name("tab\there.txt"), "tab_here.txt");
        assert_eq!(sanitize_file_name("  ..trailing.. "), "trailing");
        assert_eq!(sanitize_file_name(""), "document");
        assert_eq!(sanitize_file_name("..."), "document");
        assert_eq!(sanitize_file_name("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_file_name("nul"), "_nul");
        let long = format!("{}.pdf", "x".repeat(300));
        let cut = sanitize_file_name(&long);
        assert_eq!(cut.chars().count(), 120);
        assert!(cut.ends_with(".pdf"));
    }

    #[test]
    fn a_document_opens_elsewhere_as_a_private_copy_that_the_wipe_removes() {
        let conn = setup_test_db();
        let data_dir = tempfile::tempdir().expect("data dir");
        let cache_dir = tempfile::tempdir().expect("cache dir");
        let picked = tempfile::tempdir().expect("picked");
        let source = picked.path().join("Rozpis nákladů.xlsx");
        fs::write(&source, b"PK\x03\x04 sheet").expect("source");
        let doc = add_real_estate_document(&conn, "re", &source, &DOC, 0).expect("add");
        // A legacy document whose name on disk is just `<uuid>.pdf`: the copy takes its display name.
        let legacy_dir = data_dir.path().join(REAL_ESTATE_DOCUMENTS_DIR).join("re");
        fs::create_dir_all(&legacy_dir).expect("mkdir");
        let legacy_file = legacy_dir.join(format!("{UUID}.pdf"));
        fs::write(&legacy_file, b"%PDF-1.4").expect("legacy");
        conn.execute(
            "INSERT INTO real_estate_documents (id, real_estate_id, name, file_path) VALUES ('old', 're', 'List vlastnictví', ?1)",
            [legacy_file.to_str().expect("utf8")],
        )
        .expect("legacy row");

        let copy = open_elsewhere(
            &conn,
            data_dir.path(),
            cache_dir.path(),
            AttachmentKind::RealEstateDocument,
            &doc.id,
        )
        .expect("open");
        assert_eq!(
            copy.file_name().and_then(|n| n.to_str()),
            Some("Rozpis nákladů.xlsx")
        );
        assert!(copy.starts_with(cache_dir.path().join(OPEN_COPIES_DIR)));
        assert_eq!(fs::read(&copy).expect("copy"), b"PK\x03\x04 sheet".to_vec());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = |p: &Path| fs::metadata(p).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode(&copy), 0o600);
            assert_eq!(mode(copy.parent().expect("dir")), 0o700);
        }

        let old = open_elsewhere(
            &conn,
            data_dir.path(),
            cache_dir.path(),
            AttachmentKind::RealEstateDocument,
            "old",
        )
        .expect("open legacy");
        assert_eq!(
            old.file_name().and_then(|n| n.to_str()),
            Some("List vlastnictví.pdf")
        );

        wipe_open_copies(cache_dir.path());
        assert!(!cache_dir.path().join(OPEN_COPIES_DIR).exists());
        assert!(
            legacy_file.exists(),
            "the wipe never touches the data folder"
        );
        wipe_open_copies(cache_dir.path());
    }
}
