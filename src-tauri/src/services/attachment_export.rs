//! Phones: a document handed to the system's save picker ("Uložit do Souborů" on iOS, the
//! document picker on Android), the one way a document leaves the phone (ADR 0017).
//!
//! The pickers export a file that exists: on iOS the dialog plugin's `save()` exports
//! `<staging dir>/<defaultPath's name>`, and Android's picker gets the staged file copied into the
//! document it created (`write_staged_export`). So a decrypted copy is staged under the document's
//! own name for the length of the picker and removed again whatever the person chose. Its name is
//! the person's, so it can be on no allow-list as the other staged exports are: a marker in the
//! app data folder holds the SHA-256 of the staged name (never the name itself), which is how the
//! discard checks a path and how the startup sweep finds a copy an interrupted export left behind.
//! Names and paths are never logged.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use sha2::{Digest, Sha256};

use crate::error::{AppError, Result};
use crate::models::AttachmentKind;
use crate::services::attachments::{self, Variant};

/// The marker in the app data folder: the hex SHA-256 of the staged copy's file name.
pub const MARKER_FILE_NAME: &str = "attachment-export.staged";

/// The longest staged name, in bytes: file systems allow 255, and the save picker may add " 2".
const MAX_NAME_BYTES: usize = 200;

/// The error key of a path that is not the staged copy (the staged exports' own key: "This file
/// name is not allowed").
const INVALID: &str = "validation.backupExportInvalid";

/// The file name a document is saved under: its display name with the characters no file system
/// takes (path separators, `:*?"<>|`, control characters) replaced, no leading dot (a hidden
/// file), at most `MAX_NAME_BYTES`, and the original extension added when the name lacks it (the
/// desktop's `defaultFileName`). An empty name is "document".
pub fn export_file_name(name: &str, extension: Option<&str>) -> String {
    let cleaned: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_start_matches(['.', ' ']).trim_end();
    let base = if cleaned.is_empty() {
        "document"
    } else {
        cleaned
    };
    let suffix = match extension {
        Some(ext) if !base.to_lowercase().ends_with(&format!(".{ext}")) => format!(".{ext}"),
        _ => String::new(),
    };
    let mut cut = MAX_NAME_BYTES.saturating_sub(suffix.len());
    if cut >= base.len() {
        return format!("{base}{suffix}");
    }
    while !base.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{suffix}", base[..cut].trim_end())
}

/// The extension a saved copy carries: the original file's, else the one its type implies.
fn extension_for(original_name: Option<&str>, mime: &str) -> Option<String> {
    original_name
        .and_then(attachments::extension_of)
        .or_else(|| {
            let ext = match mime {
                "application/pdf" => "pdf",
                "image/jpeg" => "jpg",
                "image/png" => "png",
                "image/gif" => "gif",
                "image/webp" => "webp",
                "image/bmp" => "bmp",
                _ => return None,
            };
            Some(ext.to_string())
        })
}

fn name_hash(name: &str) -> String {
    hex::encode(Sha256::digest(name.as_bytes()))
}

fn marker_path(data_dir: &Path) -> PathBuf {
    data_dir.join(MARKER_FILE_NAME)
}

fn io_err(what: &str, e: io::Error) -> AppError {
    AppError::Internal(format!("{what}: {e}"))
}

fn read_marker(data_dir: &Path) -> Result<Option<String>> {
    match fs::read_to_string(marker_path(data_dir)) {
        Ok(hash) => Ok(Some(hash.trim().to_string())),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(io_err("Failed to read the export marker", e)),
    }
}

fn remove_if_present(path: &Path, what: &str) -> Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(io_err(what, e)),
        _ => Ok(()),
    }
}

/// The document's display name, `None` for a row that is not there. Photos are not documents.
fn display_name(conn: &Connection, kind: AttachmentKind, id: &str) -> Result<Option<String>> {
    let table = match kind {
        AttachmentKind::InsuranceDocument => "insurance_documents",
        AttachmentKind::RealEstateDocument => "real_estate_documents",
        AttachmentKind::Photo => {
            return Err(AppError::Validation(INVALID.into()));
        }
    };
    Ok(conn
        .query_row(
            &format!("SELECT name FROM {table} WHERE id = ?1"),
            [id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .map(Option::unwrap_or_default))
}

/// Stage a decrypted copy of the document `id` of `kind` in `staging_dir` under its own name and
/// answer its path, for the save picker. A copy an earlier export left is removed first (one at a
/// time). Content that is not on this device fails before anything is written, and a write that
/// fails half way leaves neither the file nor the marker behind.
pub fn stage(
    conn: &Connection,
    data_dir: &Path,
    staging_dir: &Path,
    kind: AttachmentKind,
    id: &str,
) -> Result<PathBuf> {
    let name = display_name(conn, kind, id)?
        .ok_or_else(|| AppError::NotFound("Attachment not found".into()))?;
    let resolved = attachments::resolve(conn, data_dir, kind, id, Variant::Full)?
        .ok_or_else(|| AppError::NotFound("Attachment not found".into()))?;
    let extension = extension_for(resolved.file_name.as_deref(), &resolved.mime);
    let file_name = export_file_name(&name, extension.as_deref());

    sweep(data_dir, staging_dir)?;
    let dest = staging_dir.join(&file_name);
    // The marker first: an export killed while writing still leaves a copy the sweep can find
    fs::write(marker_path(data_dir), name_hash(&file_name))
        .map_err(|e| io_err("Failed to write the export marker", e))?;
    if let Err(e) = attachments::write_to(conn, &resolved, &dest) {
        let _ = fs::remove_file(&dest);
        let _ = fs::remove_file(marker_path(data_dir));
        return Err(e);
    }
    Ok(dest)
}

/// Whether `path` is the copy `stage` put in `staging_dir`: directly inside it (nothing
/// normalised) and named as the marker says.
pub fn is_staged_path(data_dir: &Path, staging_dir: &Path, path: &Path) -> Result<bool> {
    let Some(hash) = read_marker(data_dir)? else {
        return Ok(false);
    };
    Ok(path.parent() == Some(staging_dir)
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name_hash(name) == hash))
}

/// Remove the staged copy at `path` (one already gone is fine) and the marker. Refuses any other
/// path.
pub fn discard(data_dir: &Path, staging_dir: &Path, path: &Path) -> Result<()> {
    if !is_staged_path(data_dir, staging_dir, path)? {
        return Err(AppError::Validation(INVALID.into()));
    }
    remove_if_present(path, "Failed to remove the staged document")?;
    remove_if_present(&marker_path(data_dir), "Failed to remove the export marker")
}

/// Remove what an interrupted export left: the file directly in `staging_dir` the marker names,
/// then the marker. Without a marker there is nothing to do. Returns how many files went.
pub fn sweep(data_dir: &Path, staging_dir: &Path) -> Result<usize> {
    let Some(hash) = read_marker(data_dir)? else {
        return Ok(0);
    };
    let mut removed = 0;
    match fs::read_dir(staging_dir) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|e| io_err("Failed to list the staging folder", e))?;
                let is_file = entry.file_type().is_ok_and(|t| t.is_file());
                let named = entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name_hash(name) == hash);
                if is_file && named {
                    remove_if_present(&entry.path(), "Failed to remove the staged document")?;
                    removed += 1;
                }
            }
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(io_err("Failed to list the staging folder", e)),
    }
    remove_if_present(&marker_path(data_dir), "Failed to remove the export marker")?;
    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::files_schema;
    use crate::services::attachment_store;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().expect("in-memory db");
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        files_schema::migrate(&conn).expect("files schema");
        conn.execute_batch(
            r#"
            CREATE TABLE sync_state (id INTEGER PRIMARY KEY, enabled INTEGER NOT NULL DEFAULT 0);
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
            "#,
        )
        .expect("schema");
        conn
    }

    const ID: &str = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60";
    const PDF: &[u8] = b"%PDF-1.7 recap";

    fn insert_document(conn: &Connection, name: &str, original: Option<&str>) {
        let blob = attachment_store::put_bytes(conn, PDF).expect("put");
        conn.execute(
            "INSERT INTO insurance_documents (id, insurance_id, name, blob_id, mime_type, original_name)
             VALUES (?1, 'pol', ?2, ?3, 'application/pdf', ?4)",
            rusqlite::params![ID, name, blob.id, original],
        )
        .expect("insert");
    }

    struct Dirs {
        _root: tempfile::TempDir,
        data: PathBuf,
        staging: PathBuf,
    }

    fn dirs() -> Dirs {
        let root = tempfile::tempdir().expect("tempdir");
        let data = root.path().join("data");
        let staging = root.path().join("Documents");
        fs::create_dir_all(&data).expect("data");
        fs::create_dir_all(&staging).expect("staging");
        Dirs {
            _root: root,
            data,
            staging,
        }
    }

    #[test]
    fn the_name_is_the_display_name_with_the_extension() {
        assert_eq!(
            export_file_name("KOOP - Rekapitulace", Some("pdf")),
            "KOOP - Rekapitulace.pdf"
        );
        assert_eq!(export_file_name("smlouva.PDF", Some("pdf")), "smlouva.PDF");
        assert_eq!(export_file_name("Účet 2025", None), "Účet 2025");
    }

    #[test]
    fn the_name_cannot_leave_the_folder_or_hide() {
        assert_eq!(
            export_file_name("../../moony.db", Some("pdf")),
            "_.._moony.db.pdf"
        );
        assert_eq!(export_file_name(".hidden", Some("pdf")), "hidden.pdf");
        assert_eq!(
            export_file_name("a/b\\c:d*e?f\"g<h>i|j\n", None),
            "a_b_c_d_e_f_g_h_i_j"
        );
        assert_eq!(export_file_name("   ", Some("pdf")), "document.pdf");
        assert_eq!(export_file_name("..", None), "document");
    }

    #[test]
    fn a_long_name_is_cut_on_a_character_boundary() {
        let long = "ž".repeat(300);
        let name = export_file_name(&long, Some("pdf"));
        assert!(name.len() <= MAX_NAME_BYTES, "{}", name.len());
        assert!(name.ends_with(".pdf"));
        assert!(name.starts_with('ž'));
    }

    #[test]
    fn the_extension_comes_from_the_original_or_the_type() {
        assert_eq!(
            extension_for(Some("scan.JPEG"), "image/jpeg").as_deref(),
            Some("jpeg")
        );
        assert_eq!(
            extension_for(None, "application/pdf").as_deref(),
            Some("pdf")
        );
        assert_eq!(extension_for(None, "application/zip"), None);
    }

    #[test]
    fn staging_writes_the_decrypted_copy_and_a_marker_without_the_name() {
        let conn = setup_test_db();
        let d = dirs();
        insert_document(&conn, "KOOP - Rekapitulace", Some("rekap.pdf"));

        let path = stage(
            &conn,
            &d.data,
            &d.staging,
            AttachmentKind::InsuranceDocument,
            ID,
        )
        .expect("stage");

        assert_eq!(path, d.staging.join("KOOP - Rekapitulace.pdf"));
        assert_eq!(fs::read(&path).expect("read"), PDF);
        let marker = fs::read_to_string(d.data.join(MARKER_FILE_NAME)).expect("marker");
        assert!(!marker.contains("KOOP"));
        assert!(is_staged_path(&d.data, &d.staging, &path).expect("check"));
    }

    #[test]
    fn discard_removes_the_copy_and_the_marker() {
        let conn = setup_test_db();
        let d = dirs();
        insert_document(&conn, "Smlouva", None);
        let path = stage(
            &conn,
            &d.data,
            &d.staging,
            AttachmentKind::InsuranceDocument,
            ID,
        )
        .expect("stage");

        discard(&d.data, &d.staging, &path).expect("discard");

        assert!(!path.exists());
        assert!(!d.data.join(MARKER_FILE_NAME).exists());
        // Twice is refused: nothing is staged any more
        assert!(discard(&d.data, &d.staging, &path).is_err());
    }

    #[test]
    fn discard_refuses_any_other_path() {
        let conn = setup_test_db();
        let d = dirs();
        insert_document(&conn, "Smlouva", None);
        let path = stage(
            &conn,
            &d.data,
            &d.staging,
            AttachmentKind::InsuranceDocument,
            ID,
        )
        .expect("stage");
        let other = d.staging.join("moony-backup-2026-11-22.zip");
        fs::write(&other, b"x").expect("other");

        assert!(discard(&d.data, &d.staging, &other).is_err());
        // The staged name in another folder (the profile's own) is not the staged copy
        assert!(discard(&d.data, &d.staging, &d.data.join("Smlouva.pdf")).is_err());
        assert!(discard(&d.data, &d.staging, &d.staging.join("../data/Smlouva.pdf")).is_err());
        assert!(other.exists());
        assert!(path.exists());
    }

    #[test]
    fn the_sweep_removes_only_what_an_interrupted_export_left() {
        let conn = setup_test_db();
        let d = dirs();
        insert_document(&conn, "Smlouva", None);
        let path = stage(
            &conn,
            &d.data,
            &d.staging,
            AttachmentKind::InsuranceDocument,
            ID,
        )
        .expect("stage");
        let other = d.staging.join("something-else.pdf");
        fs::write(&other, b"x").expect("other");

        assert_eq!(sweep(&d.data, &d.staging).expect("sweep"), 1);

        assert!(!path.exists());
        assert!(other.exists());
        assert!(!d.data.join(MARKER_FILE_NAME).exists());
        assert_eq!(sweep(&d.data, &d.staging).expect("sweep again"), 0);
    }

    #[test]
    fn a_new_stage_removes_the_previous_copy() {
        let conn = setup_test_db();
        let d = dirs();
        insert_document(&conn, "První", None);
        let first = stage(
            &conn,
            &d.data,
            &d.staging,
            AttachmentKind::InsuranceDocument,
            ID,
        )
        .expect("stage");
        conn.execute("UPDATE insurance_documents SET name = 'Druhý'", [])
            .expect("rename");

        let second = stage(
            &conn,
            &d.data,
            &d.staging,
            AttachmentKind::InsuranceDocument,
            ID,
        )
        .expect("stage again");

        assert!(!first.exists());
        assert!(second.exists());
        assert!(!is_staged_path(&d.data, &d.staging, &first).expect("check"));
    }

    #[test]
    fn a_missing_document_or_a_photo_stages_nothing() {
        let conn = setup_test_db();
        let d = dirs();
        assert!(matches!(
            stage(
                &conn,
                &d.data,
                &d.staging,
                AttachmentKind::InsuranceDocument,
                ID
            ),
            Err(AppError::NotFound(_))
        ));
        assert!(stage(&conn, &d.data, &d.staging, AttachmentKind::Photo, ID).is_err());
        assert_eq!(fs::read_dir(&d.staging).expect("list").count(), 0);
        assert!(!d.data.join(MARKER_FILE_NAME).exists());
    }
}
