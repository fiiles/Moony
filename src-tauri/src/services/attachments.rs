//! Attachments with their owners: where a photo's or a document's bytes are (ADR 0017).
//!
//! A row references either a blob in the encrypted store (`blob_id`) or, until the conversion of
//! plain files from earlier versions has reached it, a legacy file on disk (`file_path`). Every
//! reader of attachment content goes through [`resolve`], so the `moony-file` scheme, exports and
//! opening in another app treat both the same way.
//!
//! New documents go straight into the store ([`add_insurance_document`],
//! [`add_real_estate_document`] from a path the user picked; [`add_insurance_document_bytes`],
//! [`add_real_estate_document_bytes`] from bytes the webview holds, e.g. a photo taken on the
//! phone). Deleting a row deletes its blob in the same transaction and its legacy file after the
//! commit; a legacy file outside the current data folder is never deleted.

use crate::error::{AppError, Result};
use crate::models::{AttachmentContent, AttachmentKind, InsuranceDocument, RealEstateDocument};
use crate::services::sync::blobs::content::{row_syncs, ContentReader};
use crate::services::sync::blobs::{queue, sync_on};
use crate::services::{attachment_store, document_files, real_estate_photos};
use rusqlite::{Connection, OptionalExtension};
use serde::de::DeserializeOwned;
use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Component, Path, PathBuf};
use tauri::http::HeaderMap;
use uuid::Uuid;

/// Folder of legacy insurance documents under the app data dir.
pub const INSURANCE_DOCUMENTS_DIR: &str = "insurance_documents";
/// Folder of legacy real estate documents under the app data dir.
pub const REAL_ESTATE_DOCUMENTS_DIR: &str = "real_estate_documents";

/// The folders earlier versions kept attachments in, as plain files.
pub const LEGACY_DIRS: [&str; 3] = [
    real_estate_photos::PHOTOS_DIR,
    REAL_ESTATE_DOCUMENTS_DIR,
    INSURANCE_DOCUMENTS_DIR,
];

/// True when `path` lies inside one of the legacy attachment folders of `data_dir`, judged by its
/// components (no `..`, no root), not just its text: only such a file may ever be deleted or moved.
pub fn is_legacy_attachment_path(data_dir: &Path, path: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(data_dir) else {
        return false;
    };
    let mut components = rel.components();
    let in_legacy_dir = matches!(
        components.next(),
        Some(Component::Normal(first)) if LEGACY_DIRS.iter().any(|d| first == *d)
    );
    in_legacy_dir
        && components.clone().next().is_some()
        && components.all(|c| matches!(c, Component::Normal(_)))
}

/// Refuse a destination inside the data folder: a copy must never overwrite the databases, the
/// key files or the store.
pub(crate) fn ensure_outside_data_dir(data_dir: &Path, dest: &Path) -> Result<()> {
    let canonical = |p: &Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let data = canonical(data_dir);
    let target = match (dest.parent(), dest.file_name()) {
        (Some(parent), Some(name)) => canonical(parent).join(name),
        _ => canonical(dest),
    };
    if target.starts_with(&data) {
        return Err(AppError::Validation(
            "validation.exportIntoDataFolder".into(),
        ));
    }
    Ok(())
}

/// The error key of content that is not on this device yet (it is on the sync server or another
/// device). A normal state, never logged above `debug`.
pub const NOT_HERE_YET: &str = "validation.attachmentNotHereYet";

/// The error key of content that cannot reach this device as things stand (the sync server
/// refuses it, holds other content under its ID, or it could not be opened). Not retried by
/// opening it; "try again" on its row (`retry`) is the way out.
pub const UNAVAILABLE: &str = "validation.attachmentUnavailable";

/// The full content of the row `id` of `kind` is asked for (the person chose "Download"): only
/// content that is not here and can still arrive is wanted and due now; content here, on its way,
/// or unable to arrive as things stand ([`queue::download_failed`], not on the server) is left
/// exactly as it is, and the answer is its real state (a failed row is reset by [`retry`], not
/// by this). `NotFound` for an unknown row; with sync off the content is here and nothing changes.
pub fn request_download(
    conn: &Connection,
    kind: AttachmentKind,
    id: &str,
) -> Result<AttachmentContent> {
    let content = row_content(conn, kind, id)?;
    if content != AttachmentContent::NotHere {
        return Ok(content);
    }
    queue::want_row(conn, kind, id)?;
    row_content(conn, kind, id)
}

/// "Try again" on a row (`queue::retry_row`): its full content is wanted, and every blob of the
/// row that waits to move, up or down, starts its count again and is due now; that is a failed
/// upload as well as a failed download, never an entry with an invalid ID. Answers the row's
/// content after. With sync off nothing changes and the content is here.
pub fn retry(conn: &Connection, kind: AttachmentKind, id: &str) -> Result<AttachmentContent> {
    let content = row_content(conn, kind, id)?;
    if !sync_on(conn)? {
        return Ok(content);
    }
    queue::retry_row(conn, kind, id)?;
    row_content(conn, kind, id)
}

/// "Download everything" (`queue::want_all`): every full content not here is wanted and due now.
/// How many rows have their full content wanted and not here afterwards, those asked for earlier
/// included (so asking twice answers the same). 0 with sync off.
pub fn download_everything(conn: &Connection) -> Result<u32> {
    if !sync_on(conn)? {
        return Ok(0);
    }
    queue::want_all(conn)
}

/// Whether `id` can be a row ID of an attachment table (the `moony-file` route's rule): 1 to 64
/// letters, digits and hyphens.
pub fn is_row_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// The full content of one row, `NotFound` when there is no such row (or `id` cannot be one).
fn row_content(conn: &Connection, kind: AttachmentKind, id: &str) -> Result<AttachmentContent> {
    if !is_row_id(id) {
        return Err(AppError::NotFound("Attachment not found".into()));
    }
    let sql = match kind {
        AttachmentKind::InsuranceDocument => {
            "SELECT blob_id, NULL FROM insurance_documents WHERE id = ?1"
        }
        AttachmentKind::RealEstateDocument => {
            "SELECT blob_id, NULL FROM real_estate_documents WHERE id = ?1"
        }
        AttachmentKind::Photo => {
            "SELECT blob_id, thumb_blob_id FROM real_estate_photos WHERE id = ?1"
        }
    };
    let (blob, thumb): (Option<String>, Option<String>) = conn
        .query_row(sql, [id], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?
        .ok_or_else(|| AppError::NotFound("Attachment not found".into()))?;
    ContentLookup::new(conn)?.of(kind, blob.as_deref(), thumb.as_deref(), Variant::Full)
}

/// Where the content of the rows of one listing is: one per call, so sync is looked up once and a
/// row costs one queue lookup (the store is only consulted for a blob without a queue entry).
/// A profile without sync has everything here and is never asked anything else.
pub struct ContentLookup<'c> {
    reader: Option<ContentReader<'c>>,
}

impl<'c> ContentLookup<'c> {
    pub fn new(conn: &'c Connection) -> Result<Self> {
        let reader = if sync_on(conn)? {
            Some(ContentReader::new(conn)?)
        } else {
            None
        };
        Ok(Self { reader })
    }

    /// The content of the blob `which` of a row of `kind` with these blob columns (`blob` for a
    /// document, `blob` and `thumb` for a photo).
    pub fn of(
        &self,
        kind: AttachmentKind,
        blob: Option<&str>,
        thumb: Option<&str>,
        which: Variant,
    ) -> Result<AttachmentContent> {
        let Some(reader) = &self.reader else {
            return Ok(AttachmentContent::Here);
        };
        let syncs = row_syncs(kind, blob, thumb)?;
        reader.of(
            match which {
                Variant::Full => blob,
                Variant::Thumbnail => thumb,
            },
            syncs,
        )
    }
}

/// A blob that is not complete here is not content: when it is on its way (an entry that is to
/// come `down` and can arrive) it is wanted now and the answer is [`NOT_HERE_YET`]; when it
/// cannot arrive as things stand the answer is [`UNAVAILABLE`] and nothing is asked for. Nothing is read or
/// created before this answers. A blob that is not here and not on its way (lost, or sync off)
/// is left to the reader, which says it is not found. An incomplete incoming blob is never read.
fn require_here(conn: &Connection, resolved: &Resolved) -> Result<()> {
    let Source::Blob(id) = &resolved.source else {
        return Ok(());
    };
    if attachment_store::is_complete(conn, id)? {
        return Ok(());
    }
    let coming: Option<Option<String>> = conn
        .query_row(
            "SELECT last_error FROM main.sync_blobs WHERE blob_id = ?1 AND state = 'down'",
            [id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(last_error) = coming else {
        return Ok(());
    };
    // Waiting, being fetched, failed for a passing reason or not on the server yet (the other
    // device has not uploaded it) all arrive by themselves; the rest never does.
    if queue::download_failed(last_error.as_deref()) {
        return Err(AppError::Validation(UNAVAILABLE.into()));
    }
    queue::want_blob(conn, id)?;
    Err(AppError::Validation(NOT_HERE_YET.into()))
}

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
    require_here(conn, resolved)?;
    match &resolved.source {
        Source::Blob(id) => attachment_store::read_all(conn, id),
        Source::Legacy(path) => fs::read(path)
            .map_err(|e| AppError::NotFound(format!("Attachment file cannot be read: {e}"))),
    }
}

/// Write a resolved attachment to `dest` (a path the user chose).
pub fn write_to(conn: &Connection, resolved: &Resolved, dest: &Path) -> Result<()> {
    // Before the file is created: a document that is not here leaves nothing behind.
    require_here(conn, resolved)?;
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
    ensure_outside_data_dir(data_dir, dest)?;
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
    ensure_outside_data_dir(data_dir, &dest_dir.join("batch"))?;
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
    // Every photo is checked before the first file is written: a batch with a photo that is not
    // here fails whole, and wants every missing photo so one retry later succeeds.
    let mut photos = Vec::with_capacity(ids.len());
    let mut not_here = false;
    for id in ids {
        let resolved = resolve(conn, data_dir, AttachmentKind::Photo, &id, Variant::Full)?
            .ok_or_else(|| AppError::NotFound("Photo not found".into()))?;
        match require_here(conn, &resolved) {
            Err(AppError::Validation(key)) if key == NOT_HERE_YET => not_here = true,
            other => other?,
        }
        photos.push((id, resolved));
    }
    if not_here {
        return Err(AppError::Validation(NOT_HERE_YET.into()));
    }
    let mut saved = 0;
    for (id, resolved) in photos {
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
            // Bidirectional and invisible format characters can disguise an extension
            // ("cv\u{202E}fdp.exe" displays as "cvexe.pdf").
            '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{2069}'
            | '\u{FEFF}' => '_',
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
    // Before the private folder exists.
    require_here(conn, &resolved)?;
    let dir = cache_dir
        .join(OPEN_COPIES_DIR)
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&dir)
        .map_err(|e| AppError::Internal(format!("Failed to create a folder for the copy: {e}")))?;
    restrict_permissions(&dir, 0o700);
    let written = copy_file_name(conn, kind, id, &resolved).and_then(|name| {
        let path = dir.join(name);
        write_to(conn, &resolved, &path)?;
        restrict_permissions(&path, 0o600);
        Ok(path)
    });
    if written.is_err() {
        // No partial decrypted copy is left behind.
        let _ = fs::remove_dir_all(&dir);
    }
    written
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

/// Where one kind of document lives: its table, its owners' table and the column that joins them.
struct DocumentTables {
    documents: &'static str,
    owners: &'static str,
    owner_column: &'static str,
    /// The `NotFound` message for an owner that does not exist (only the bytes flow looks it up).
    owner_missing: &'static str,
}

const INSURANCE_TABLES: DocumentTables = DocumentTables {
    documents: "insurance_documents",
    owners: "insurance_policies",
    owner_column: "insurance_id",
    owner_missing: "Insurance policy not found",
};

const REAL_ESTATE_TABLES: DocumentTables = DocumentTables {
    documents: "real_estate_documents",
    owners: "real_estate",
    owner_column: "real_estate_id",
    owner_missing: "Real estate not found",
};

/// What a document's row records about its file, known before the bytes are stored.
struct FileFacts {
    original_name: String,
    mime_type: String,
    len: i64,
}

impl FileFacts {
    /// `head` is the file's first bytes ([`SNIFF_BYTES`] of them): they decide the MIME type before
    /// the name does.
    fn new(original_name: String, head: &[u8], len: u64) -> Self {
        let mime_type = attachment_store::sniff_mime(Some(&original_name), head).to_string();
        Self {
            original_name,
            mime_type,
            len: len as i64,
        }
    }
}

struct StoredDocument {
    id: String,
    blob_id: String,
    mime_type: String,
    extension: Option<String>,
    file_size: i64,
}

/// Insert the row of a document whose blob is already in the store, and describe what was stored.
/// It runs inside the caller's transaction, which holds the blob too and commits both.
fn insert_document_row(
    tx: &Connection,
    tables: &DocumentTables,
    owner_id: &str,
    blob_id: &str,
    facts: FileFacts,
    doc: &NewDocument,
    now: i64,
) -> Result<StoredDocument> {
    let id = Uuid::new_v4().to_string();
    tx.execute(
        &format!(
            "INSERT INTO {} (id, {}, name, description, blob_id, mime_type,
                             original_name, file_type, file_size, uploaded_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            tables.documents, tables.owner_column
        ),
        rusqlite::params![
            id,
            owner_id,
            doc.name,
            doc.description,
            blob_id,
            facts.mime_type,
            facts.original_name,
            doc.file_type,
            facts.len,
            now
        ],
    )?;
    Ok(StoredDocument {
        id,
        blob_id: blob_id.to_string(),
        extension: extension_of(&facts.original_name),
        mime_type: facts.mime_type,
        file_size: facts.len,
    })
}

fn insurance_document(
    conn: &Connection,
    insurance_id: &str,
    doc: &NewDocument,
    stored: StoredDocument,
    now: i64,
) -> Result<InsuranceDocument> {
    let content = ContentLookup::new(conn)?.of(
        AttachmentKind::InsuranceDocument,
        Some(&stored.blob_id),
        None,
        Variant::Full,
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
        content,
    })
}

fn real_estate_document(
    conn: &Connection,
    real_estate_id: &str,
    doc: &NewDocument,
    stored: StoredDocument,
    now: i64,
) -> Result<RealEstateDocument> {
    let content = ContentLookup::new(conn)?.of(
        AttachmentKind::RealEstateDocument,
        Some(&stored.blob_id),
        None,
        Variant::Full,
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
        content,
    })
}

/// A document may be at most [`attachment_store::MAX_DOCUMENT_BYTES`]. It takes only the length,
/// so it runs before anything is opened, hashed or written (and is tested with numbers, never by
/// allocating 100 MB).
pub fn check_document_len(len: u64) -> Result<()> {
    if len > attachment_store::MAX_DOCUMENT_BYTES {
        return Err(AppError::Validation("validation.attachmentTooLarge".into()));
    }
    Ok(())
}

/// Stream the file the user picked into the store and insert its row, in one transaction.
fn store_document(
    conn: &Connection,
    tables: &DocumentTables,
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
    check_document_len(len)?;
    let mut file =
        File::open(source).map_err(|e| AppError::Internal(format!("Cannot open the file: {e}")))?;
    let head = read_head(&mut file)?;
    let facts = FileFacts::new(original_name, &head, len);

    let tx = conn.unchecked_transaction()?;
    let blob = attachment_store::put_reader(&tx, &mut file, len)?;
    let stored = insert_document_row(&tx, tables, owner_id, &blob.id, facts, doc, now)?;
    tx.commit()?;
    Ok(stored)
}

/// How many of a file's first bytes the MIME sniff looks at.
const SNIFF_BYTES: usize = 16;

/// The first bytes of a file (for MIME sniffing), with the file rewound to its start.
fn read_head(file: &mut File) -> Result<Vec<u8>> {
    let mut head = vec![0u8; SNIFF_BYTES];
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
    let stored = store_document(conn, &INSURANCE_TABLES, insurance_id, source, doc, now)?;
    insurance_document(conn, insurance_id, doc, stored, now)
}

/// Attach a document to a property.
pub fn add_real_estate_document(
    conn: &Connection,
    real_estate_id: &str,
    source: &Path,
    doc: &NewDocument,
    now: i64,
) -> Result<RealEstateDocument> {
    let stored = store_document(conn, &REAL_ESTATE_TABLES, real_estate_id, source, doc, now)?;
    real_estate_document(conn, real_estate_id, doc, stored, now)
}

// ----------------------------------------------------------------------------
// From bytes (the phone: a photographed contract or a file from the Files app exists only as
// bytes in the webview, which has no file-system access, and no plaintext copy may be written)
// ----------------------------------------------------------------------------

/// The base name of a file name the webview sent. A picked file's name can carry a path
/// (`../../x/a.pdf`, `C:\a.pdf`); only its last component is kept, and that is used for the MIME
/// sniff, the extension and `original_name` only, never to reach a file.
pub fn base_name(name: &str) -> String {
    let last = name
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim();
    match last {
        "" | "." | ".." => "document".to_string(),
        last => last.to_string(),
    }
}

/// What the webview sends with a document's bytes, read from the request headers.
pub struct DocumentUpload<T> {
    /// `owner-id`: the policy or property the document belongs to.
    pub owner_id: String,
    /// `file-name`: the picked file's name, not yet reduced to its base name.
    pub file_name: String,
    /// `document`: the user's entries (`InsertInsuranceDocument` / `InsertRealEstateDocument`),
    /// not yet validated.
    pub data: T,
}

/// The text of a header, or an error naming it. A value that is not visible ASCII is as good as
/// missing.
fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::Internal(format!("The request needs a readable {name} header")))
}

/// A percent-encoded header (an HTTP header value is ASCII, file names and entries are not).
fn decoded_header(headers: &HeaderMap, name: &str) -> Result<String> {
    urlencoding::decode(header_text(headers, name)?)
        .map(|text| text.into_owned())
        .map_err(|_| AppError::Internal(format!("The {name} header is not valid UTF-8 text")))
}

/// Read the headers of a document upload: `owner-id` as is, `file-name` and `document` (JSON,
/// camelCase) percent-decoded. A missing or malformed header is an `Internal` error: the webview
/// builds them, the user never types them.
pub fn document_upload<T: DeserializeOwned>(headers: &HeaderMap) -> Result<DocumentUpload<T>> {
    let owner_id = header_text(headers, "owner-id")?.to_string();
    let file_name = decoded_header(headers, "file-name")?;
    let data = serde_json::from_str(&decoded_header(headers, "document")?)
        .map_err(|e| AppError::Internal(format!("The document header is not valid: {e}")))?;
    Ok(DocumentUpload {
        owner_id,
        file_name,
        data,
    })
}

/// Put the bytes of a document into the store and insert its row, in one transaction. The owner
/// is looked up first, so an unknown policy or property stores nothing; empty or oversized bytes
/// are refused before the database is touched.
fn store_document_bytes(
    conn: &Connection,
    tables: &DocumentTables,
    owner_id: &str,
    file_name: &str,
    bytes: &[u8],
    doc: &NewDocument,
    now: i64,
) -> Result<StoredDocument> {
    if bytes.is_empty() {
        return Err(AppError::Validation("validation.fileRequired".into()));
    }
    check_document_len(bytes.len() as u64)?;
    let head = &bytes[..bytes.len().min(SNIFF_BYTES)];
    let facts = FileFacts::new(base_name(file_name), head, bytes.len() as u64);

    let tx = conn.unchecked_transaction()?;
    let owner_exists: bool = tx.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM {} WHERE id = ?1)",
            tables.owners
        ),
        [owner_id],
        |row| row.get(0),
    )?;
    if !owner_exists {
        return Err(AppError::NotFound(tables.owner_missing.into()));
    }
    let blob = attachment_store::put_bytes(&tx, bytes)?;
    let stored = insert_document_row(&tx, tables, owner_id, &blob.id, facts, doc, now)?;
    tx.commit()?;
    Ok(stored)
}

/// Attach a document to an insurance policy from its bytes. `file_name` is whatever the picker
/// reported; only its base name is kept.
pub fn add_insurance_document_bytes(
    conn: &Connection,
    insurance_id: &str,
    file_name: &str,
    bytes: &[u8],
    doc: &NewDocument,
    now: i64,
) -> Result<InsuranceDocument> {
    let stored = store_document_bytes(
        conn,
        &INSURANCE_TABLES,
        insurance_id,
        file_name,
        bytes,
        doc,
        now,
    )?;
    insurance_document(conn, insurance_id, doc, stored, now)
}

/// Attach a document to a property from its bytes. `file_name` is whatever the picker reported;
/// only its base name is kept.
pub fn add_real_estate_document_bytes(
    conn: &Connection,
    real_estate_id: &str,
    file_name: &str,
    bytes: &[u8],
    doc: &NewDocument,
    now: i64,
) -> Result<RealEstateDocument> {
    let stored = store_document_bytes(
        conn,
        &REAL_ESTATE_TABLES,
        real_estate_id,
        file_name,
        bytes,
        doc,
        now,
    )?;
    real_estate_document(conn, real_estate_id, doc, stored, now)
}

// ============================================================================
// Listing documents
// ============================================================================

/// The documents of an insurance policy, newest first, each with where its content is.
pub fn list_insurance_documents(
    conn: &Connection,
    insurance_id: &str,
) -> Result<Vec<InsuranceDocument>> {
    let content = ContentLookup::new(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, insurance_id, name, description, file_path, file_type, file_size, uploaded_at,
                mime_type, original_name, blob_id
         FROM insurance_documents WHERE insurance_id = ?1 ORDER BY uploaded_at DESC",
    )?;
    let rows = stmt.query_map([insurance_id], |row| {
        Ok((
            InsuranceDocument {
                id: row.get(0)?,
                insurance_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                mime_type: String::new(),
                extension: None,
                file_type: row.get(5)?,
                file_size: row.get(6)?,
                uploaded_at: row.get(7)?,
                content: AttachmentContent::Here,
            },
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
        ))
    })?;
    let mut documents = Vec::new();
    for row in rows {
        let (mut doc, file_path, mime_type, original_name, blob_id) = row?;
        (doc.mime_type, doc.extension) = document_meta(
            file_path.as_deref(),
            mime_type.as_deref(),
            original_name.as_deref(),
        );
        doc.content = content.of(
            AttachmentKind::InsuranceDocument,
            blob_id.as_deref(),
            None,
            Variant::Full,
        )?;
        documents.push(doc);
    }
    Ok(documents)
}

/// The documents of a property, newest first, each with where its content is.
pub fn list_real_estate_documents(
    conn: &Connection,
    real_estate_id: &str,
) -> Result<Vec<RealEstateDocument>> {
    let content = ContentLookup::new(conn)?;
    let mut stmt = conn.prepare(
        "SELECT id, real_estate_id, name, description, file_path, file_type, file_size, uploaded_at,
                mime_type, original_name, blob_id
         FROM real_estate_documents WHERE real_estate_id = ?1 ORDER BY uploaded_at DESC",
    )?;
    let rows = stmt.query_map([real_estate_id], |row| {
        Ok((
            RealEstateDocument {
                id: row.get(0)?,
                real_estate_id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                mime_type: String::new(),
                extension: None,
                file_type: row.get(5)?,
                file_size: row.get(6)?,
                uploaded_at: row.get(7)?,
                content: AttachmentContent::Here,
            },
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(8)?,
            row.get::<_, Option<String>>(9)?,
            row.get::<_, Option<String>>(10)?,
        ))
    })?;
    let mut documents = Vec::new();
    for row in rows {
        let (mut doc, file_path, mime_type, original_name, blob_id) = row?;
        (doc.mime_type, doc.extension) = document_meta(
            file_path.as_deref(),
            mime_type.as_deref(),
            original_name.as_deref(),
        );
        doc.content = content.of(
            AttachmentKind::RealEstateDocument,
            blob_id.as_deref(),
            None,
            Variant::Full,
        )?;
        documents.push(doc);
    }
    Ok(documents)
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
        if !crate::db::files_schema::is_attached(conn)? {
            // D11: with the store unavailable the rows still go; the blobs are swept once the
            // store opens again (`attachment_store::delete_unreferenced`).
            if !self.blobs.is_empty() {
                log::warn!(
                    "[ATTACHMENTS] Store unavailable: blobs of deleted rows left for the sweep"
                );
            }
            return Ok(());
        }
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
            if !is_legacy_attachment_path(data_dir, file) {
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
            CREATE TABLE sync_state (id INTEGER PRIMARY KEY, enabled INTEGER NOT NULL DEFAULT 0);
            CREATE TABLE insurance_policies (id TEXT PRIMARY KEY);
            CREATE TABLE real_estate (id TEXT PRIMARY KEY);
            INSERT INTO insurance_policies (id) VALUES ('pol');
            INSERT INTO real_estate (id) VALUES ('re');
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

        let out_dir = tempfile::tempdir().expect("out");
        let out = out_dir.path().to_path_buf();
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

    // ------------------------------------------------------------------------
    // Documents from bytes (the phone camera and Files picker)
    // ------------------------------------------------------------------------

    fn row_count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
            .expect("count")
    }

    fn original_name_of(conn: &Connection, table: &str, id: &str) -> Option<String> {
        conn.query_row(
            &format!("SELECT original_name FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get(0),
        )
        .expect("row")
    }

    #[test]
    fn byte_document_is_stored_with_sniffed_mime() {
        let conn = setup_test_db();
        let data_dir = tempfile::tempdir().expect("data dir");
        let bytes = b"%PDF-1.7 policy".as_slice();

        let doc =
            add_insurance_document_bytes(&conn, "pol", "smlouva.pdf", bytes, &DOC, 1_700_000_000)
                .expect("add");
        assert_eq!(doc.insurance_id, "pol");
        assert_eq!(doc.name, "Smlouva");
        assert_eq!(doc.mime_type, "application/pdf");
        assert_eq!(doc.extension.as_deref(), Some("pdf"));
        assert_eq!(doc.file_size, Some(bytes.len() as i64));
        assert_eq!(doc.file_type, "contract");
        assert_eq!(doc.uploaded_at, 1_700_000_000);

        let row: (Option<String>, Option<String>, Option<i64>) = conn
            .query_row(
                "SELECT file_path, blob_id, file_size FROM insurance_documents WHERE id = ?1",
                [&doc.id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("row");
        assert_eq!(row.0, None);
        assert!(row.1.is_some());
        assert_eq!(row.2, Some(bytes.len() as i64));
        assert_eq!(
            original_name_of(&conn, "insurance_documents", &doc.id).as_deref(),
            Some("smlouva.pdf")
        );
        let resolved = resolve(
            &conn,
            data_dir.path(),
            AttachmentKind::InsuranceDocument,
            &doc.id,
            Variant::Full,
        )
        .expect("resolve")
        .expect("row");
        assert_eq!(read_bytes(&conn, &resolved).expect("bytes"), bytes.to_vec());
        assert!(fs::read_dir(data_dir.path()).expect("dir").next().is_none());

        // The first bytes decide the type, so a photographed page without an extension is a PDF
        // here too.
        let nameless =
            add_insurance_document_bytes(&conn, "pol", "scan", bytes, &DOC, 0).expect("add");
        assert_eq!(nameless.mime_type, "application/pdf");
        assert_eq!(nameless.extension, None);
    }

    #[test]
    fn byte_real_estate_document_is_stored_with_sniffed_mime() {
        let conn = setup_test_db();
        let data_dir = tempfile::tempdir().expect("data dir");
        let bytes = b"%PDF-1.7 deed".as_slice();

        let doc =
            add_real_estate_document_bytes(&conn, "re", "kupni.pdf", bytes, &DOC, 5).expect("add");
        assert_eq!(doc.real_estate_id, "re");
        assert_eq!(doc.mime_type, "application/pdf");
        assert_eq!(doc.extension.as_deref(), Some("pdf"));
        assert_eq!(doc.file_size, Some(bytes.len() as i64));
        assert_eq!(doc.file_type, "contract");
        assert_eq!(doc.uploaded_at, 5);
        assert_eq!(
            original_name_of(&conn, "real_estate_documents", &doc.id).as_deref(),
            Some("kupni.pdf")
        );
        let resolved = resolve(
            &conn,
            data_dir.path(),
            AttachmentKind::RealEstateDocument,
            &doc.id,
            Variant::Full,
        )
        .expect("resolve")
        .expect("row");
        assert_eq!(read_bytes(&conn, &resolved).expect("bytes"), bytes.to_vec());
        assert_eq!(blob_count(&conn), 1);
    }

    #[test]
    fn byte_document_name_with_path_is_reduced_to_base_name() {
        let conn = setup_test_db();
        let doc =
            add_insurance_document_bytes(&conn, "pol", "../../x/smlouva.pdf", b"%PDF-1.7", &DOC, 0)
                .expect("add");
        assert_eq!(
            original_name_of(&conn, "insurance_documents", &doc.id).as_deref(),
            Some("smlouva.pdf")
        );
        assert_eq!(doc.extension.as_deref(), Some("pdf"));

        let re = add_real_estate_document_bytes(
            &conn,
            "re",
            "C:\\Users\\Filip\\kupni.pdf",
            b"%PDF-1.7",
            &DOC,
            0,
        )
        .expect("add");
        assert_eq!(
            original_name_of(&conn, "real_estate_documents", &re.id).as_deref(),
            Some("kupni.pdf")
        );

        for (given, expected) in [
            ("..\\..\\x\\smlouva.pdf", "smlouva.pdf"),
            ("/private/var/tmp/IMG_0001.jpeg", "IMG_0001.jpeg"),
            ("Pojistná smlouva.pdf", "Pojistná smlouva.pdf"),
            ("dir/", "dir"),
            ("..", "document"),
            (".", "document"),
            ("a/..", "document"),
            ("a\\..", "document"),
            ("", "document"),
            ("   ", "document"),
            ("/", "document"),
        ] {
            assert_eq!(base_name(given), expected, "name {given:?}");
        }
    }

    #[test]
    fn byte_document_rejects_empty_bytes() {
        let conn = setup_test_db();
        for err in [
            add_insurance_document_bytes(&conn, "pol", "a.pdf", &[], &DOC, 0).expect_err("empty"),
            add_real_estate_document_bytes(&conn, "re", "a.pdf", &[], &DOC, 0).expect_err("empty"),
        ] {
            assert_eq!(
                err.keyed().map(|(k, _)| k).as_deref(),
                Some("validation.fileRequired")
            );
        }
        assert_eq!(blob_count(&conn), 0);
        assert_eq!(row_count(&conn, "insurance_documents"), 0);
        assert_eq!(row_count(&conn, "real_estate_documents"), 0);
    }

    #[test]
    fn document_len_limit() {
        let max = attachment_store::MAX_DOCUMENT_BYTES;
        assert!(check_document_len(0).is_ok());
        assert!(check_document_len(1).is_ok());
        assert!(check_document_len(max).is_ok());
        for len in [max + 1, u64::MAX] {
            let err = check_document_len(len).expect_err("too large");
            assert_eq!(
                err.keyed().map(|(k, _)| k).as_deref(),
                Some("validation.attachmentTooLarge")
            );
        }
    }

    #[test]
    fn byte_document_for_unknown_owner_is_not_found() {
        let conn = setup_test_db();
        let err = add_insurance_document_bytes(&conn, "ghost", "a.pdf", b"%PDF-1.7", &DOC, 0)
            .expect_err("no policy");
        assert!(matches!(err, AppError::NotFound(_)));
        let err = add_real_estate_document_bytes(&conn, "ghost", "a.pdf", b"%PDF-1.7", &DOC, 0)
            .expect_err("no property");
        assert!(matches!(err, AppError::NotFound(_)));
        // A policy's id does not open a property's documents, and the other way round.
        assert!(matches!(
            add_real_estate_document_bytes(&conn, "pol", "a.pdf", b"%PDF-1.7", &DOC, 0),
            Err(AppError::NotFound(_))
        ));
        assert!(matches!(
            add_insurance_document_bytes(&conn, "re", "a.pdf", b"%PDF-1.7", &DOC, 0),
            Err(AppError::NotFound(_))
        ));

        assert_eq!(blob_count(&conn), 0);
        assert_eq!(row_count(&conn, "insurance_documents"), 0);
        assert_eq!(row_count(&conn, "real_estate_documents"), 0);

        // The refusals leave the connection usable.
        add_insurance_document_bytes(&conn, "pol", "a.pdf", b"%PDF-1.7", &DOC, 0).expect("add");
        assert_eq!(blob_count(&conn), 1);
    }

    #[test]
    fn a_failed_insert_leaves_no_blob_behind() {
        let conn = setup_test_db();
        conn.execute_batch(
            "CREATE TRIGGER refuse BEFORE INSERT ON insurance_documents
             BEGIN SELECT RAISE(ABORT, 'refused'); END;",
        )
        .expect("trigger");
        let err = add_insurance_document_bytes(&conn, "pol", "a.pdf", b"%PDF-1.7", &DOC, 0)
            .expect_err("insert refused");
        assert!(matches!(err, AppError::Database(_)));
        assert_eq!(blob_count(&conn), 0);
        assert_eq!(row_count(&conn, "insurance_documents"), 0);
    }

    fn upload_headers_of(
        owner_id: &str,
        file_name: &str,
        document: &str,
    ) -> tauri::http::HeaderMap {
        let mut headers = tauri::http::HeaderMap::new();
        for (name, value) in [
            ("owner-id", owner_id.to_string()),
            ("file-name", urlencoding::encode(file_name).into_owned()),
            ("document", urlencoding::encode(document).into_owned()),
        ] {
            headers.insert(
                name,
                tauri::http::HeaderValue::from_str(&value).expect("ASCII header"),
            );
        }
        headers
    }

    #[test]
    fn upload_headers_are_read_and_percent_decoded() {
        let json =
            r#"{"name":"Pojistná smlouva č. 7","description":"Čeština","fileType":"contract"}"#;
        let headers = upload_headers_of("pol", "Pojistná smlouva č. 7.pdf", json);

        let upload = document_upload::<crate::models::InsertInsuranceDocument>(&headers)
            .expect("upload headers");
        assert_eq!(upload.owner_id, "pol");
        assert_eq!(upload.file_name, "Pojistná smlouva č. 7.pdf");
        assert_eq!(upload.data.name, "Pojistná smlouva č. 7");
        assert_eq!(upload.data.description.as_deref(), Some("Čeština"));
        assert_eq!(upload.data.file_type.as_deref(), Some("contract"));
        assert!(upload.data.validate().is_ok());

        let upload = document_upload::<crate::models::InsertRealEstateDocument>(&headers)
            .expect("upload headers");
        assert_eq!(upload.data.name, "Pojistná smlouva č. 7");
    }

    #[test]
    fn missing_or_malformed_upload_headers_are_errors_not_panics() {
        type Doc = crate::models::InsertInsuranceDocument;
        let good = upload_headers_of("pol", "a.pdf", r#"{"name":"A"}"#);
        assert!(document_upload::<Doc>(&good).is_ok());

        for missing in ["owner-id", "file-name", "document"] {
            let mut headers = good.clone();
            headers.remove(missing);
            assert!(
                matches!(document_upload::<Doc>(&headers), Err(AppError::Internal(_))),
                "without {missing}"
            );
        }

        let bad_values = [
            ("document", "not json"),
            ("document", "%7B%22name%22%3A1%7D"),
            ("document", "%7B%7D"),
            ("file-name", "%FF%FE"),
            ("document", "%FF"),
        ];
        for (name, value) in bad_values {
            let mut headers = good.clone();
            headers.insert(
                name,
                tauri::http::HeaderValue::from_str(value).expect("ASCII header"),
            );
            assert!(
                matches!(document_upload::<Doc>(&headers), Err(AppError::Internal(_))),
                "{name}: {value}"
            );
        }

        // A header that is not text at all.
        let mut headers = good.clone();
        headers.insert(
            "owner-id",
            tauri::http::HeaderValue::from_bytes(&[0xE9, 0xFF]).expect("opaque header"),
        );
        assert!(matches!(
            document_upload::<Doc>(&headers),
            Err(AppError::Internal(_))
        ));
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

    #[test]
    fn only_paths_inside_a_legacy_folder_count_as_legacy_attachments() {
        let data = Path::new("/data/moony");
        assert!(is_legacy_attachment_path(
            data,
            Path::new("/data/moony/insurance_documents/p/a.pdf")
        ));
        assert!(is_legacy_attachment_path(
            data,
            Path::new("/data/moony/real_estate_photos/r/b/x.jpg")
        ));
        for path in [
            "/data/moony/real_estate_documents/../key.enc",
            "/data/moony/real_estate_documents",
            "/data/moony/moony.db",
            "/data/moony/Backup/insurance_documents/a.pdf",
            "/elsewhere/insurance_documents/p/a.pdf",
        ] {
            assert!(!is_legacy_attachment_path(data, Path::new(path)), "{path}");
        }
    }

    #[test]
    fn deleting_with_the_store_unavailable_removes_the_row_and_leaves_the_blob_for_the_sweep() {
        let conn = setup_test_db();
        let data_dir = tempfile::tempdir().expect("data dir");
        let picked = tempfile::tempdir().expect("picked");
        let source = picked.path().join("a.pdf");
        fs::write(&source, b"%PDF").expect("source");
        let doc = add_insurance_document(&conn, "pol", &source, &DOC, 0).expect("add");
        conn.execute_batch("DETACH DATABASE files;")
            .expect("detach");

        delete_document(
            &conn,
            data_dir.path(),
            AttachmentKind::InsuranceDocument,
            &doc.id,
        )
        .expect("delete without the store");
        let rows: i64 = conn
            .query_row("SELECT count(*) FROM insurance_documents", [], |r| r.get(0))
            .expect("count");
        assert_eq!(rows, 0);
    }

    #[test]
    fn a_copy_is_never_saved_into_the_data_folder() {
        let conn = setup_test_db();
        let data_dir = tempfile::tempdir().expect("data dir");
        let blob = attachment_store::put_bytes(&conn, b"x").expect("put");
        conn.execute(
            "INSERT INTO insurance_documents (id, name, blob_id, mime_type) VALUES ('d', 'a', ?1, 'application/pdf')",
            [&blob.id],
        )
        .expect("row");
        let err = export(
            &conn,
            data_dir.path(),
            AttachmentKind::InsuranceDocument,
            "d",
            &data_dir.path().join("moony.db"),
        )
        .expect_err("refused");
        assert_eq!(
            err.keyed().map(|(k, _)| k).as_deref(),
            Some("validation.exportIntoDataFolder")
        );
        assert!(!data_dir.path().join("moony.db").exists());
        assert!(export_photo_batch(&conn, data_dir.path(), "any", data_dir.path()).is_err());
    }

    #[test]
    fn bidi_controls_cannot_disguise_an_extension() {
        assert_eq!(sanitize_file_name("cv\u{202E}fdp.exe"), "cv_fdp.exe");
        assert_eq!(sanitize_file_name("a\u{200B}b.pdf"), "a_b.pdf");
    }

    // ------------------------------------------------------------------------
    // Content state in the lists, and opening content that is not here (Task 14)
    // ------------------------------------------------------------------------

    use crate::models::{AttachmentContent, RealEstatePhoto};
    use crate::services::sync::blobs::queue;
    use crate::services::sync::hlc::DeviceId;
    use crate::services::sync::{state, test_support::migrated};

    const NOW: i64 = 1_772_323_200;

    /// A real profile with the attachment store, one policy `p1` and one property `r1`; sync on
    /// when `sync` is true.
    fn profile(sync: bool) -> Connection {
        let conn = migrated();
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        files_schema::migrate(&conn).expect("files schema");
        if sync {
            state::enable(&conn, DeviceId([7; 16])).expect("enable");
        }
        conn.execute_batch(
            "INSERT INTO insurance_policies \
               (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
             VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly'); \
             INSERT INTO real_estate (id, name, address, type) VALUES ('r1', 'Flat', 'Street', 'flat'); \
             INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
               VALUES ('b1', 'r1', 1767225600);",
        )
        .expect("owners");
        conn
    }

    /// A blob ID no blob here has (content that lives on another device).
    fn elsewhere(n: u8) -> String {
        format!("0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f{n:02x}")
    }

    fn insurance_doc(conn: &Connection, id: &str, blob: &str, at: i64) {
        conn.execute(
            "INSERT INTO insurance_documents (id, insurance_id, name, blob_id, uploaded_at) \
             VALUES (?1, 'p1', 'Contract', ?2, ?3)",
            rusqlite::params![id, blob, at],
        )
        .expect("document");
    }

    fn estate_doc(conn: &Connection, id: &str, blob: &str, at: i64) {
        conn.execute(
            "INSERT INTO real_estate_documents (id, real_estate_id, name, blob_id, uploaded_at) \
             VALUES (?1, 'r1', 'Deed', ?2, ?3)",
            rusqlite::params![id, blob, at],
        )
        .expect("document");
    }

    fn photo_row(conn: &Connection, id: &str, full: &str, thumb: &str, at: i64) {
        conn.execute(
            "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id, created_at) \
             VALUES (?1, 'b1', ?2, ?3, ?4)",
            rusqlite::params![id, full, thumb, at],
        )
        .expect("photo");
    }

    fn confirm(conn: &Connection, blob: &str) {
        conn.execute(
            "UPDATE sync_blobs SET state = 'done' WHERE blob_id = ?1",
            [blob],
        )
        .expect("done");
    }

    fn wanted(conn: &Connection, blob: &str) -> bool {
        conn.query_row(
            "SELECT wanted FROM sync_blobs WHERE blob_id = ?1",
            [blob],
            |r| r.get(0),
        )
        .expect("entry")
    }

    #[test]
    fn the_lists_report_each_rows_content() {
        let conn = profile(true);
        let confirmed = attachment_store::put_bytes(&conn, b"confirmed")
            .expect("blob")
            .id;
        let waiting = attachment_store::put_bytes(&conn, b"waiting")
            .expect("blob")
            .id;
        let thumb = attachment_store::put_bytes(&conn, b"thumb")
            .expect("blob")
            .id;
        let away = elsewhere(1);
        insurance_doc(&conn, "d-here", &confirmed, 300);
        insurance_doc(&conn, "d-up", &waiting, 200);
        insurance_doc(&conn, "d-down", &away, 100);
        estate_doc(&conn, "e-down", &elsewhere(2), 100);
        estate_doc(&conn, "e-newer", &elsewhere(4), 150);
        photo_row(&conn, "ph", &elsewhere(3), &thumb, 10);
        queue::reconcile(&conn, NOW).expect("reconcile");
        confirm(&conn, &confirmed);
        confirm(&conn, &thumb);
        // `waiting` has an `up` entry; a row the queue has not seen has no entry at all.
        let unseen = attachment_store::put_bytes(&conn, b"unseen")
            .expect("blob")
            .id;
        insurance_doc(&conn, "d-unseen", &unseen, 50);

        let docs = list_insurance_documents(&conn, "p1").expect("list");
        let seen: Vec<_> = docs.iter().map(|d| (d.id.as_str(), d.content)).collect();
        assert_eq!(
            seen,
            [
                ("d-here", AttachmentContent::Here),
                ("d-up", AttachmentContent::WaitingUpload),
                ("d-down", AttachmentContent::NotHere),
                ("d-unseen", AttachmentContent::WaitingUpload),
            ],
            "newest first, as the command returned them"
        );
        let estate = list_real_estate_documents(&conn, "r1").expect("list");
        let estate_ids: Vec<_> = estate.iter().map(|d| d.id.as_str()).collect();
        assert_eq!(estate_ids, ["e-newer", "e-down"], "newest first");
        assert!(estate
            .iter()
            .all(|d| d.content == AttachmentContent::NotHere));

        let batches = real_estate_photos::list_batches(&conn, "r1").expect("batches");
        let photo = &batches[0].photos[0];
        assert_eq!(photo.content, AttachmentContent::NotHere);
        assert_eq!(photo.thumb_content, AttachmentContent::Here);
    }

    #[test]
    fn a_profile_without_sync_lists_everything_as_here() {
        let conn = profile(false);
        let held = attachment_store::put_bytes(&conn, b"held")
            .expect("blob")
            .id;
        insurance_doc(&conn, "d1", &held, 2);
        insurance_doc(&conn, "d2", &elsewhere(1), 1); // lost content: sync off knows nothing else
        estate_doc(&conn, "e1", &held, 1);
        photo_row(&conn, "ph", &held, &elsewhere(2), 1);
        // Sync tables empty or never touched: nothing in them is read.
        conn.execute("DROP TABLE sync_blobs", []).expect("drop");

        let docs = list_insurance_documents(&conn, "p1").expect("list");
        assert!(docs.iter().all(|d| d.content == AttachmentContent::Here));
        assert_eq!(docs.len(), 2);
        assert_eq!(
            list_real_estate_documents(&conn, "r1").expect("list")[0].content,
            AttachmentContent::Here
        );
        let photo = &real_estate_photos::list_batches(&conn, "r1").expect("batches")[0].photos[0];
        assert_eq!(
            (photo.content, photo.thumb_content),
            (AttachmentContent::Here, AttachmentContent::Here)
        );
    }

    #[test]
    fn a_document_added_here_reads_waiting_with_sync_on_and_here_without() {
        for (sync, expected) in [
            (true, AttachmentContent::WaitingUpload),
            (false, AttachmentContent::Here),
        ] {
            let conn = profile(sync);
            let doc = NewDocument {
                name: "Contract",
                description: None,
                file_type: "other",
            };
            let added = add_insurance_document_bytes(&conn, "p1", "a.pdf", b"%PDF-1.7", &doc, NOW)
                .expect("add");
            assert_eq!(added.content, expected, "sync {sync}");
            let added =
                add_real_estate_document_bytes(&conn, "r1", "a.pdf", b"%PDF-1.7", &doc, NOW)
                    .expect("add");
            assert_eq!(added.content, expected, "sync {sync}");
            let prepared =
                real_estate_photos::prepared_for_test(b"full".to_vec(), b"thumb".to_vec());
            let photo: RealEstatePhoto =
                real_estate_photos::record_photo(&conn, "b1", &prepared, NOW).expect("photo");
            assert_eq!(
                (photo.content, photo.thumb_content),
                (expected, expected),
                "sync {sync}"
            );
        }
    }

    #[test]
    fn opening_content_that_is_not_here_asks_for_it() {
        let conn = profile(true);
        let dir = tempfile::tempdir().expect("tempdir");
        let (data, cache, out) = (
            dir.path().join("data"),
            dir.path().join("cache"),
            dir.path().join("out"),
        );
        for d in [&data, &cache, &out] {
            fs::create_dir_all(d).expect("mkdir");
        }
        let away = elsewhere(1);
        insurance_doc(&conn, "d", &away, 1);
        queue::reconcile(&conn, NOW).expect("reconcile");
        assert!(!wanted(&conn, &away));
        let not_here = |e: AppError| matches!(e, AppError::Validation(k) if k == NOT_HERE_YET);
        let kind = AttachmentKind::InsuranceDocument;

        let resolved = resolve(&conn, &data, kind, "d", Variant::Full)
            .expect("resolve")
            .expect("row");
        assert!(not_here(read_bytes(&conn, &resolved).expect_err("read")));
        assert!(wanted(&conn, &away), "reading it asks for it");

        conn.execute("UPDATE sync_blobs SET wanted = 0", [])
            .expect("reset");
        let file = out.join("doc.pdf");
        assert!(not_here(
            export(&conn, &data, kind, "d", &file).expect_err("export")
        ));
        assert!(!file.exists(), "no empty or partial file is left");
        assert!(wanted(&conn, &away));

        conn.execute("UPDATE sync_blobs SET wanted = 0", [])
            .expect("reset");
        assert!(not_here(
            open_elsewhere(&conn, &data, &cache, kind, "d").expect_err("open")
        ));
        assert!(
            !cache.join(OPEN_COPIES_DIR).exists(),
            "no folder for a copy"
        );
        assert!(wanted(&conn, &away));
    }

    #[test]
    fn an_incomplete_incoming_blob_is_not_content() {
        let conn = profile(true);
        let dir = tempfile::tempdir().expect("tempdir");
        let away = elsewhere(1);
        insurance_doc(&conn, "d", &away, 1);
        queue::reconcile(&conn, NOW).expect("reconcile");
        attachment_store::begin_incoming(&conn, &away, 4).expect("begin");
        attachment_store::write_incoming(&conn, &away, 0, b"half").expect("write");
        let resolved = resolve(
            &conn,
            dir.path(),
            AttachmentKind::InsuranceDocument,
            "d",
            Variant::Full,
        )
        .expect("resolve")
        .expect("row");
        let err = read_bytes(&conn, &resolved).expect_err("read");
        assert!(matches!(err, AppError::Validation(k) if k == NOT_HERE_YET));

        // Without a queue entry (sync off) it is still never read as content.
        let conn = profile(false);
        insurance_doc(&conn, "d", &away, 1);
        attachment_store::begin_incoming(&conn, &away, 4).expect("begin");
        attachment_store::write_incoming(&conn, &away, 0, b"half").expect("write");
        let err = read_bytes(&conn, &resolved).expect_err("read");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }

    #[test]
    fn a_photo_batch_with_a_photo_not_here_fails_before_writing_and_wants_every_missing_one() {
        let conn = profile(true);
        let dir = tempfile::tempdir().expect("tempdir");
        let (data, out) = (dir.path().join("data"), dir.path().join("out"));
        for d in [&data, &out] {
            fs::create_dir_all(d).expect("mkdir");
        }
        let here = attachment_store::put_bytes(&conn, b"here")
            .expect("blob")
            .id;
        let thumb = attachment_store::put_bytes(&conn, b"thumb")
            .expect("blob")
            .id;
        let (a, b) = (elsewhere(1), elsewhere(2));
        photo_row(&conn, "p1", &here, &thumb, 1);
        photo_row(&conn, "p2", &a, &thumb, 2);
        photo_row(&conn, "p3", &b, &thumb, 3);
        queue::reconcile(&conn, NOW).expect("reconcile");

        let err = export_photo_batch(&conn, &data, "b1", &out).expect_err("export");
        assert!(matches!(err, AppError::Validation(k) if k == NOT_HERE_YET));
        assert_eq!(fs::read_dir(&out).expect("dir").count(), 0, "no half batch");
        assert!(wanted(&conn, &a) && wanted(&conn, &b));

        // One retry later, with both here, the whole batch is written.
        for (blob, bytes) in [(&a, b"a"), (&b, b"b")] {
            attachment_store::begin_incoming(&conn, blob, 1).expect("begin");
            attachment_store::write_incoming(&conn, blob, 0, bytes).expect("write");
            attachment_store::finish_incoming(&conn, blob, "sha").expect("finish");
        }
        assert_eq!(
            export_photo_batch(&conn, &data, "b1", &out).expect("export"),
            3
        );
    }

    /// Sets the entry of `blob` to a failed `down` with this `last_error`.
    fn fail_down(conn: &Connection, blob: &str, error: &str) {
        conn.execute(
            "UPDATE sync_blobs SET last_error = ?2, attempts = 3, next_try_at = ?3 \
             WHERE blob_id = ?1",
            rusqlite::params![blob, error, i64::MAX],
        )
        .expect("fail");
    }

    fn entry(conn: &Connection, blob: &str) -> (bool, i64, Option<String>) {
        conn.query_row(
            "SELECT wanted, attempts, last_error FROM sync_blobs WHERE blob_id = ?1",
            [blob],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("entry")
    }

    fn read_error(conn: &Connection, id: &str) -> AppError {
        let dir = tempfile::tempdir().expect("tempdir");
        let resolved = resolve(
            conn,
            dir.path(),
            AttachmentKind::InsuranceDocument,
            id,
            Variant::Full,
        )
        .expect("resolve")
        .expect("row");
        read_bytes(conn, &resolved).expect_err("not content")
    }

    #[test]
    fn content_that_cannot_arrive_is_unavailable_not_on_its_way() {
        let conn = profile(true);
        for (i, error) in ["refused", "conflict", "unreadable", "invalidId"]
            .into_iter()
            .enumerate()
        {
            let away = elsewhere(10 + i as u8);
            let id = format!("d-{error}");
            insurance_doc(&conn, &id, &away, 1);
            queue::reconcile(&conn, NOW).expect("reconcile");
            fail_down(&conn, &away, error);
            let err = read_error(&conn, &id);
            assert!(
                matches!(&err, AppError::Validation(k) if k == UNAVAILABLE),
                "{error}: {err:?}"
            );
            assert!(!wanted(&conn, &away), "{error}: nothing is asked for");
            assert_eq!(entry(&conn, &away).1, 3, "{error}: nothing is reset");
        }
    }

    #[test]
    fn content_that_will_arrive_is_on_its_way() {
        let conn = profile(true);
        // Waiting, failed but retryable (network, server), and not on the server yet: the other
        // device has not uploaded it, and it IS on its way.
        for (i, error) in [None, Some("network"), Some("server"), Some("notOnServer")]
            .into_iter()
            .enumerate()
        {
            let away = elsewhere(20 + i as u8);
            let id = format!("d-{i}");
            insurance_doc(&conn, &id, &away, 1);
            queue::reconcile(&conn, NOW).expect("reconcile");
            if let Some(error) = error {
                fail_down(&conn, &away, error);
            }
            let err = read_error(&conn, &id);
            assert!(
                matches!(&err, AppError::Validation(k) if k == NOT_HERE_YET),
                "{error:?}: {err:?}"
            );
            assert!(wanted(&conn, &away), "{error:?}: it is asked for");
        }
    }

    #[test]
    fn download_asks_for_the_full_content_of_a_row_that_is_not_here() {
        let conn = profile(true);
        let (full, thumb) = (elsewhere(1), elsewhere(2));
        photo_row(&conn, "ph", &full, &thumb, 1);
        queue::reconcile(&conn, NOW).expect("reconcile");
        let content = request_download(&conn, AttachmentKind::Photo, "ph").expect("download");
        assert_eq!(content, AttachmentContent::NotHere);
        assert!(wanted(&conn, &full));
        assert!(!wanted(&conn, &thumb), "only the full content is asked for");

        let away = elsewhere(3);
        insurance_doc(&conn, "d", &away, 1);
        queue::reconcile(&conn, NOW).expect("reconcile");
        request_download(&conn, AttachmentKind::InsuranceDocument, "d").expect("download");
        assert!(wanted(&conn, &away));
        estate_doc(&conn, "e", &elsewhere(4), 1);
        queue::reconcile(&conn, NOW).expect("reconcile");
        request_download(&conn, AttachmentKind::RealEstateDocument, "e").expect("download");
        assert!(wanted(&conn, &elsewhere(4)));
    }

    #[test]
    fn download_of_content_already_here_changes_nothing() {
        let conn = profile(true);
        let held = attachment_store::put_bytes(&conn, b"held")
            .expect("blob")
            .id;
        insurance_doc(&conn, "d", &held, 1);
        queue::reconcile(&conn, NOW).expect("reconcile");
        confirm(&conn, &held);
        let kind = AttachmentKind::InsuranceDocument;
        assert_eq!(
            request_download(&conn, kind, "d").expect("download"),
            AttachmentContent::Here
        );
        assert!(!wanted(&conn, &held));
    }

    #[test]
    fn download_of_a_row_that_cannot_arrive_reports_it_and_resets_nothing() {
        let conn = profile(true);
        let kind = AttachmentKind::InsuranceDocument;
        for (i, (error, expected)) in [
            ("refused", AttachmentContent::Failed),
            ("conflict", AttachmentContent::Failed),
            ("unreadable", AttachmentContent::Failed),
            ("invalidId", AttachmentContent::Failed),
            ("notOnServer", AttachmentContent::NotOnServer),
        ]
        .into_iter()
        .enumerate()
        {
            let away = elsewhere(30 + i as u8);
            let id = format!("d{i}");
            insurance_doc(&conn, &id, &away, 1);
            queue::reconcile(&conn, NOW).expect("reconcile");
            fail_down(&conn, &away, error);
            let before = entry(&conn, &away);
            assert_eq!(
                request_download(&conn, kind, &id).expect("download"),
                expected
            );
            assert_eq!(entry(&conn, &away), before, "{error}: nothing changed");
        }
    }

    #[test]
    fn download_of_an_unknown_row_is_not_found() {
        let conn = profile(true);
        for bad in ["", "a/b", "../x", &"x".repeat(65)] {
            let err = request_download(&conn, AttachmentKind::Photo, bad).expect_err("invalid");
            assert!(matches!(err, AppError::NotFound(_)), "{bad:?}: {err:?}");
        }
        let err = request_download(&conn, AttachmentKind::Photo, "nope").expect_err("unknown");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
        let err = retry(&conn, AttachmentKind::RealEstateDocument, "nope").expect_err("unknown");
        assert!(matches!(err, AppError::NotFound(_)), "got {err:?}");
    }

    #[test]
    fn retry_resets_a_failed_download_and_a_failed_upload_of_the_row_only() {
        let conn = profile(true);
        let (full, thumb) = (elsewhere(1), elsewhere(2));
        photo_row(&conn, "ph", &full, &thumb, 1);
        let other = elsewhere(3);
        insurance_doc(&conn, "d", &other, 1);
        let held = attachment_store::put_bytes(&conn, b"held")
            .expect("blob")
            .id;
        insurance_doc(&conn, "up", &held, 2);
        queue::reconcile(&conn, NOW).expect("reconcile");
        fail_down(&conn, &full, "refused");
        fail_down(&conn, &other, "refused");
        fail_down(&conn, &held, "conflict"); // an upload that cannot heal by itself

        let content = retry(&conn, AttachmentKind::Photo, "ph").expect("retry");
        assert_eq!(content, AttachmentContent::NotHere);
        assert_eq!(entry(&conn, &full), (true, 0, None), "wanted, count reset");
        assert_eq!(
            entry(&conn, &other).2.as_deref(),
            Some("refused"),
            "another row"
        );

        let content = retry(&conn, AttachmentKind::InsuranceDocument, "up").expect("retry");
        assert_eq!(content, AttachmentContent::WaitingUpload);
        assert_eq!(entry(&conn, &held), (false, 0, None));
    }

    #[test]
    fn with_sync_off_the_commands_answer_here_and_change_nothing() {
        let conn = profile(false);
        insurance_doc(&conn, "d", &elsewhere(1), 1);
        let kind = AttachmentKind::InsuranceDocument;
        assert_eq!(
            request_download(&conn, kind, "d").expect("download"),
            AttachmentContent::Here
        );
        assert_eq!(
            retry(&conn, kind, "d").expect("retry"),
            AttachmentContent::Here
        );
        assert_eq!(download_everything(&conn).expect("everything"), 0);
        // Even the table of the queue is never read.
        conn.execute("DROP TABLE sync_blobs", []).expect("drop");
        assert_eq!(download_everything(&conn).expect("everything"), 0);
        assert_eq!(
            request_download(&conn, kind, "d").expect("download"),
            AttachmentContent::Here
        );
        let err = request_download(&conn, kind, "unknown").expect_err("unknown");
        assert!(matches!(err, AppError::NotFound(_)));
    }

    #[test]
    fn download_everything_counts_the_full_contents_wanted_and_is_idempotent() {
        let conn = profile(true);
        photo_row(&conn, "ph", &elsewhere(1), &elsewhere(2), 1);
        insurance_doc(&conn, "d", &elsewhere(3), 2);
        let held = attachment_store::put_bytes(&conn, b"held")
            .expect("blob")
            .id;
        estate_doc(&conn, "e", &held, 3);
        queue::reconcile(&conn, NOW).expect("reconcile");
        assert_eq!(download_everything(&conn).expect("everything"), 2);
        assert!(wanted(&conn, &elsewhere(1)) && wanted(&conn, &elsewhere(3)));
        assert!(
            !wanted(&conn, &elsewhere(2)),
            "a thumbnail is not asked for"
        );
        assert_eq!(download_everything(&conn).expect("again"), 2);
    }
}
