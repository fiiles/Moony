//! Backup, restore, full export, integrity check and data location.
//!
//! Archive layout (flat, whitelisted): `manifest.json`, `moony.db` (a
//! `VACUUM INTO` snapshot, still SQLCipher-encrypted), `salt`, `key.enc`,
//! `recovery.enc` and the attachment directories. Restore never deletes the
//! current data: it is moved into `pre-restore-<ts>/` first and moved back
//! if anything goes wrong.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use rusqlite::types::ValueRef;
use rusqlite::Connection;
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::error::{AppError, Result};
use crate::models::{
    BackupFileEntry, BackupInspection, BackupManifest, DataLocation, ExportSummary, IntegrityReport,
};

pub const BACKUP_FORMAT: u32 = 1;
pub const MANIFEST_NAME: &str = "manifest.json";
pub const DB_FILE_NAME: &str = "moony.db";
/// Files at the root of the data directory that make up an account.
pub const KEY_FILES: [&str; 4] = [DB_FILE_NAME, "salt", "key.enc", "recovery.enc"];
/// Directories with plain-file attachments.
pub const ATTACHMENT_DIRS: [&str; 3] = [
    "real_estate_photos",
    "real_estate_documents",
    "insurance_documents",
];

/// Tables that are never part of the JSON export: migration bookkeeping,
/// app-local configuration (API keys) and SQLite's own sequence table.
const EXPORT_DENYLIST: [&str; 3] = ["_migrations", "app_config", "sqlite_sequence"];
/// Columns stripped from every exported row.
const EXPORT_COLUMN_DENYLIST: [&str; 1] = ["mcp_server_token"];

fn io_err(context: &str, e: impl std::fmt::Display) -> AppError {
    AppError::Database(format!("{context}: {e}"))
}

fn zip_err(context: &str, e: impl std::fmt::Display) -> AppError {
    AppError::Database(format!("{context}: {e}"))
}

// ========================== Create ==========================

/// Snapshot the open database with `VACUUM INTO`, then zip it together with
/// the key files and attachment directories into `dest`.
pub fn create_backup(
    conn: &Connection,
    data_dir: &Path,
    dest: &Path,
    app_version: &str,
) -> Result<BackupManifest> {
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(AppError::Validation(
                "validation.backupDestinationMissing".into(),
            ));
        }
    }

    let snapshot = std::env::temp_dir().join(format!("moony-backup-{}.db", uuid::Uuid::new_v4()));
    let snapshot_str = snapshot
        .to_str()
        .ok_or_else(|| AppError::Internal("Temp path is not valid UTF-8".into()))?;
    conn.execute("VACUUM INTO ?1", [snapshot_str])
        .map_err(|e| io_err("Failed to snapshot the database", e))?;

    let result = write_archive(conn, data_dir, &snapshot, dest, app_version);
    let _ = fs::remove_file(&snapshot);
    result
}

fn write_archive(
    conn: &Connection,
    data_dir: &Path,
    snapshot: &Path,
    dest: &Path,
    app_version: &str,
) -> Result<BackupManifest> {
    let file = File::create(dest).map_err(|e| io_err("Failed to create backup file", e))?;
    let mut writer = ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    let mut files: Vec<BackupFileEntry> = Vec::new();

    // Database snapshot (named like the live file so restore is a plain extract).
    files.push(add_file(&mut writer, options, snapshot, DB_FILE_NAME)?);

    // Key files
    for name in KEY_FILES.iter().skip(1) {
        let path = data_dir.join(name);
        if path.is_file() {
            files.push(add_file(&mut writer, options, &path, name)?);
        }
    }

    // Attachment directories (recursive)
    for dir_name in ATTACHMENT_DIRS {
        let dir = data_dir.join(dir_name);
        if dir.is_dir() {
            let mut entries = Vec::new();
            collect_files(&dir, &mut entries)?;
            entries.sort();
            for entry in entries {
                let relative = entry
                    .strip_prefix(data_dir)
                    .map_err(|e| io_err("Attachment outside data dir", e))?;
                let archive_name = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join("/");
                files.push(add_file(&mut writer, options, &entry, &archive_name)?);
            }
        }
    }

    let manifest = BackupManifest {
        format: BACKUP_FORMAT,
        app_version: app_version.to_string(),
        created_at: chrono::Utc::now().timestamp(),
        migrations: crate::db::applied_migration_names(conn)?,
        files,
    };
    let manifest_json = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| AppError::Internal(format!("Failed to serialize manifest: {e}")))?;
    writer
        .start_file(MANIFEST_NAME, options)
        .map_err(|e| zip_err("Failed to write manifest", e))?;
    writer
        .write_all(&manifest_json)
        .map_err(|e| io_err("Failed to write manifest", e))?;
    writer
        .finish()
        .map_err(|e| zip_err("Failed to finish backup archive", e))?;

    Ok(manifest)
}

fn add_file(
    writer: &mut ZipWriter<File>,
    options: SimpleFileOptions,
    source: &Path,
    archive_name: &str,
) -> Result<BackupFileEntry> {
    let mut input = File::open(source)
        .map_err(|e| io_err(&format!("Failed to read {}", source.display()), e))?;
    writer
        .start_file(archive_name, options)
        .map_err(|e| zip_err("Failed to add file to backup", e))?;
    let bytes = io::copy(&mut input, writer)
        .map_err(|e| io_err(&format!("Failed to copy {}", source.display()), e))?;
    Ok(BackupFileEntry {
        path: archive_name.to_string(),
        bytes,
    })
}

fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir).map_err(|e| io_err("Failed to list attachments", e))? {
        let entry = entry.map_err(|e| io_err("Failed to list attachments", e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, out)?;
        } else if path.is_file() {
            out.push(path);
        }
    }
    Ok(())
}

// ========================== Inspect ==========================

/// Validate the archive layout and read its manifest. Rejects any entry that
/// is not in the whitelist or that escapes the target directory.
pub fn inspect_backup(archive_path: &Path, known_migrations: &[&str]) -> Result<BackupInspection> {
    let file = File::open(archive_path).map_err(|e| io_err("Failed to open backup archive", e))?;
    let archive_bytes = file
        .metadata()
        .map(|m| m.len())
        .map_err(|e| io_err("Failed to read backup archive", e))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|_| AppError::Validation("validation.backupInvalidArchive".into()))?;

    let mut manifest: Option<BackupManifest> = None;
    let mut attachment_count = 0u32;
    let mut has_db = false;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|_| AppError::Validation("validation.backupInvalidArchive".into()))?;
        let Some(safe) = entry.enclosed_name() else {
            return Err(AppError::Validation(
                "validation.backupInvalidArchive".into(),
            ));
        };
        match classify(&safe) {
            EntryKind::Manifest => {
                let mut json = String::new();
                entry
                    .read_to_string(&mut json)
                    .map_err(|_| AppError::Validation("validation.backupInvalidArchive".into()))?;
                manifest =
                    Some(serde_json::from_str(&json).map_err(|_| {
                        AppError::Validation("validation.backupInvalidArchive".into())
                    })?);
            }
            EntryKind::Database => has_db = true,
            EntryKind::KeyFile => {}
            EntryKind::Attachment => {
                if !entry.is_dir() {
                    attachment_count += 1;
                }
            }
            EntryKind::Unknown => {
                return Err(AppError::Validation(
                    "validation.backupInvalidArchive".into(),
                ))
            }
        }
    }

    let manifest = match (manifest, has_db) {
        (Some(m), true) if m.format <= BACKUP_FORMAT => m,
        _ => {
            return Err(AppError::Validation(
                "validation.backupInvalidArchive".into(),
            ))
        }
    };
    let newer_than_app = manifest
        .migrations
        .iter()
        .any(|m| !known_migrations.contains(&m.as_str()));

    Ok(BackupInspection {
        manifest,
        archive_path: archive_path.to_string_lossy().into_owned(),
        archive_bytes,
        attachment_count,
        newer_than_app,
    })
}

enum EntryKind {
    Manifest,
    Database,
    KeyFile,
    Attachment,
    Unknown,
}

fn classify(path: &Path) -> EntryKind {
    let mut components = path.components();
    let Some(Component::Normal(first)) = components.next() else {
        return EntryKind::Unknown;
    };
    let first = first.to_string_lossy();
    let is_root = components.next().is_none();
    if is_root {
        if first == MANIFEST_NAME {
            return EntryKind::Manifest;
        }
        if first == DB_FILE_NAME {
            return EntryKind::Database;
        }
        if KEY_FILES.contains(&first.as_ref()) {
            return EntryKind::KeyFile;
        }
    }
    if ATTACHMENT_DIRS.contains(&first.as_ref()) {
        return EntryKind::Attachment;
    }
    EntryKind::Unknown
}

// ========================== Restore ==========================

/// Replace the account in `data_dir` with the archive's contents. The
/// database connection must already be closed. The previous files are kept
/// in `data_dir/pre-restore-<ts>/`; on any failure they are moved back.
pub fn restore_backup(
    data_dir: &Path,
    archive_path: &Path,
    known_migrations: &[&str],
) -> Result<PathBuf> {
    let inspection = inspect_backup(archive_path, known_migrations)?;
    if inspection.newer_than_app {
        return Err(AppError::Validation("validation.backupNewerThanApp".into()));
    }

    fs::create_dir_all(data_dir).map_err(|e| io_err("Failed to create data dir", e))?;
    let pre_restore = data_dir.join(format!("pre-restore-{}", chrono::Utc::now().timestamp()));
    fs::create_dir_all(&pre_restore)
        .map_err(|e| io_err("Failed to create pre-restore folder", e))?;

    // 1. Move the current account aside (never delete).
    for name in managed_names() {
        let current = data_dir.join(name);
        if current.exists() {
            fs::rename(&current, pre_restore.join(name))
                .map_err(|e| io_err(&format!("Failed to move {name} aside"), e))?;
        }
    }

    // 2. Extract; roll back on failure.
    if let Err(e) = extract_archive(data_dir, archive_path) {
        rollback(data_dir, &pre_restore);
        return Err(e);
    }

    Ok(pre_restore)
}

/// Everything restore replaces: the key files, DB side files and attachments.
fn managed_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = KEY_FILES.to_vec();
    names.extend(["moony.db-wal", "moony.db-shm", "moony.db-journal"]);
    names.extend(ATTACHMENT_DIRS);
    names
}

fn extract_archive(data_dir: &Path, archive_path: &Path) -> Result<()> {
    let file = File::open(archive_path).map_err(|e| io_err("Failed to open backup archive", e))?;
    let mut archive = ZipArchive::new(file)
        .map_err(|_| AppError::Validation("validation.backupInvalidArchive".into()))?;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|_| AppError::Validation("validation.backupInvalidArchive".into()))?;
        let Some(safe) = entry.enclosed_name() else {
            return Err(AppError::Validation(
                "validation.backupInvalidArchive".into(),
            ));
        };
        match classify(&safe) {
            EntryKind::Manifest | EntryKind::Unknown => continue,
            EntryKind::Database | EntryKind::KeyFile | EntryKind::Attachment => {}
        }
        let target = data_dir.join(&safe);
        if entry.is_dir() {
            fs::create_dir_all(&target).map_err(|e| io_err("Failed to create folder", e))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|e| io_err("Failed to create folder", e))?;
        }
        let mut out = File::create(&target)
            .map_err(|e| io_err(&format!("Failed to write {}", target.display()), e))?;
        io::copy(&mut entry, &mut out)
            .map_err(|e| io_err(&format!("Failed to write {}", target.display()), e))?;
    }
    // Extracted key files carry the process umask; make them owner-only.
    crate::services::crypto::restrict_key_file_permissions(data_dir);
    Ok(())
}

fn rollback(data_dir: &Path, pre_restore: &Path) {
    for name in managed_names() {
        let target = data_dir.join(name);
        if target.is_dir() {
            let _ = fs::remove_dir_all(&target);
        } else if target.exists() {
            let _ = fs::remove_file(&target);
        }
        let saved = pre_restore.join(name);
        if saved.exists() {
            let _ = fs::rename(&saved, &target);
        }
    }
    let _ = fs::remove_dir(pre_restore);
}

// ========================== Export ==========================

/// Dump every user-data table as JSON objects keyed by column name.
pub fn export_all_data(conn: &Connection, dest: &Path, app_version: &str) -> Result<ExportSummary> {
    let tables: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        rows.filter_map(|r| r.ok())
            .filter(|name| {
                !name.starts_with("sqlite_") && !EXPORT_DENYLIST.contains(&name.as_str())
            })
            .collect()
    };

    let mut dump: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    let mut row_count = 0u64;
    for table in &tables {
        let mut stmt = conn.prepare(&format!("SELECT * FROM \"{table}\""))?;
        let columns: Vec<String> = stmt.column_names().iter().map(|c| c.to_string()).collect();
        let rows = stmt.query_map([], |row| {
            let mut obj = serde_json::Map::new();
            for (i, column) in columns.iter().enumerate() {
                if EXPORT_COLUMN_DENYLIST.contains(&column.as_str()) {
                    continue;
                }
                obj.insert(column.clone(), json_value(row.get_ref(i)?));
            }
            Ok(serde_json::Value::Object(obj))
        })?;
        let rows: Vec<serde_json::Value> = rows.filter_map(|r| r.ok()).collect();
        row_count += rows.len() as u64;
        dump.insert(table.clone(), rows);
    }

    let document = serde_json::json!({
        "format": BACKUP_FORMAT,
        "appVersion": app_version,
        "exportedAt": chrono::Utc::now().timestamp(),
        "tables": dump,
    });
    let json = serde_json::to_vec_pretty(&document)
        .map_err(|e| AppError::Internal(format!("Failed to serialize export: {e}")))?;
    fs::write(dest, json).map_err(|e| io_err("Failed to write export file", e))?;

    Ok(ExportSummary {
        path: dest.to_string_lossy().into_owned(),
        table_count: tables.len() as u32,
        row_count,
    })
}

fn json_value(value: ValueRef<'_>) -> serde_json::Value {
    match value {
        ValueRef::Null => serde_json::Value::Null,
        ValueRef::Integer(i) => serde_json::Value::from(i),
        ValueRef::Real(f) => serde_json::Value::from(f),
        ValueRef::Text(t) => serde_json::Value::from(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => serde_json::Value::from(hex::encode(b)),
    }
}

// ========================== Verify ==========================

/// `PRAGMA quick_check`, plus `PRAGMA cipher_integrity_check` when the
/// build supports it. `ok` is true only when every check reports nothing.
pub fn verify_database(conn: &Connection) -> Result<IntegrityReport> {
    let mut messages: Vec<String> = Vec::new();

    let mut stmt = conn.prepare("PRAGMA quick_check")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    for row in rows.filter_map(|r| r.ok()) {
        if row != "ok" {
            messages.push(row);
        }
    }

    // SQLCipher-only pragma: silently skipped on plain SQLite (tests).
    if let Ok(mut stmt) = conn.prepare("PRAGMA cipher_integrity_check") {
        if let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(0)) {
            for row in rows.filter_map(|r| r.ok()) {
                messages.push(row);
            }
        }
    }

    Ok(IntegrityReport {
        ok: messages.is_empty(),
        messages,
    })
}

// ========================== Location ==========================

pub fn data_location(data_dir: &Path, db_path: &Path) -> Result<DataLocation> {
    Ok(DataLocation {
        data_dir: data_dir.to_string_lossy().into_owned(),
        db_path: db_path.to_string_lossy().into_owned(),
        total_bytes: dir_size(data_dir),
    })
}

fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| {
            let path = e.path();
            if path.is_dir() {
                dir_size(&path)
            } else {
                e.metadata().map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNOWN: [&str; 2] = ["001_initial_schema", "002_example"];

    /// A plain-SQLite "account" in a temp data dir: db with two tables and
    /// migration rows, key files, one attachment.
    fn seed_account(dir: &Path) -> Connection {
        let db_path = dir.join(DB_FILE_NAME);
        let conn = Connection::open(&db_path).expect("open db");
        conn.execute_batch(
            r#"
            CREATE TABLE _migrations (id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, applied_at INTEGER NOT NULL DEFAULT (unixepoch()));
            INSERT INTO _migrations (name) VALUES ('001_initial_schema'), ('002_example');
            CREATE TABLE user_profile (id INTEGER PRIMARY KEY, name TEXT NOT NULL, mcp_server_token TEXT);
            INSERT INTO user_profile (id, name, mcp_server_token) VALUES (1, 'Test', 'secret-token');
            CREATE TABLE loans (id TEXT PRIMARY KEY, principal TEXT NOT NULL, note BLOB);
            INSERT INTO loans (id, principal, note) VALUES ('l1', '1000.50', X'0102');
            CREATE TABLE app_config (key TEXT PRIMARY KEY, value TEXT NOT NULL);
            INSERT INTO app_config (key, value) VALUES ('api_key_coingecko', 'cg-secret');
            "#,
        )
        .expect("schema");
        fs::write(dir.join("salt"), b"saltsaltsaltsalt").expect("salt");
        fs::write(dir.join("key.enc"), b"key").expect("key");
        fs::write(dir.join("recovery.enc"), b"recovery").expect("recovery");
        fs::create_dir_all(dir.join("real_estate_photos/batch1")).expect("photos dir");
        fs::write(dir.join("real_estate_photos/batch1/a.jpg"), b"jpeg").expect("photo");
        conn
    }

    #[test]
    fn backup_round_trip_restores_every_file_into_an_empty_data_dir() {
        let src = tempfile::tempdir().expect("src");
        let conn = seed_account(src.path());
        let archive = src.path().join("out.zip");

        let manifest = create_backup(&conn, src.path(), &archive, "9.9.9").expect("backup");
        assert_eq!(manifest.format, BACKUP_FORMAT);
        assert_eq!(manifest.app_version, "9.9.9");
        assert_eq!(
            manifest.migrations,
            vec!["001_initial_schema", "002_example"]
        );
        let names: Vec<&str> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "moony.db",
                "salt",
                "key.enc",
                "recovery.enc",
                "real_estate_photos/batch1/a.jpg"
            ]
        );

        let inspection = inspect_backup(&archive, &KNOWN).expect("inspect");
        assert_eq!(inspection.attachment_count, 1);
        assert!(!inspection.newer_than_app);
        assert!(inspection.archive_bytes > 0);

        let dst = tempfile::tempdir().expect("dst");
        let pre = restore_backup(dst.path(), &archive, &KNOWN).expect("restore");
        assert!(pre.exists());
        assert_eq!(
            fs::read(dst.path().join("salt")).expect("salt"),
            b"saltsaltsaltsalt"
        );
        assert_eq!(
            fs::read(dst.path().join("real_estate_photos/batch1/a.jpg")).expect("photo"),
            b"jpeg"
        );
        assert!(
            !dst.path().join(MANIFEST_NAME).exists(),
            "manifest stays in the archive"
        );
        let restored = Connection::open(dst.path().join(DB_FILE_NAME)).expect("restored db");
        let principal: String = restored
            .query_row("SELECT principal FROM loans WHERE id = 'l1'", [], |r| {
                r.get(0)
            })
            .expect("row survives VACUUM INTO");
        assert_eq!(principal, "1000.50");
    }

    /// key files extracted from an archive are owner-only, whatever the umask.
    #[cfg(unix)]
    #[test]
    fn restored_key_files_are_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let src = tempfile::tempdir().expect("src");
        let conn = seed_account(src.path());
        let archive = src.path().join("out.zip");
        create_backup(&conn, src.path(), &archive, "9.9.9").expect("backup");

        let dst = tempfile::tempdir().expect("dst");
        restore_backup(dst.path(), &archive, &KNOWN).expect("restore");
        for name in crate::services::crypto::SECRET_FILE_NAMES {
            let mode = fs::metadata(dst.path().join(name))
                .expect("restored key file")
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "{name}");
        }
    }

    #[test]
    fn restore_keeps_the_previous_account_in_a_pre_restore_folder() {
        let src = tempfile::tempdir().expect("src");
        let conn = seed_account(src.path());
        let archive = src.path().join("out.zip");
        create_backup(&conn, src.path(), &archive, "1.0.0").expect("backup");

        let dst = tempfile::tempdir().expect("dst");
        fs::write(dst.path().join("salt"), b"OLD-SALT").expect("old salt");
        fs::create_dir_all(dst.path().join("insurance_documents")).expect("old dir");
        fs::write(dst.path().join("insurance_documents/old.pdf"), b"old").expect("old doc");

        let pre = restore_backup(dst.path(), &archive, &KNOWN).expect("restore");

        assert_eq!(
            fs::read(dst.path().join("salt")).expect("new salt"),
            b"saltsaltsaltsalt"
        );
        assert_eq!(fs::read(pre.join("salt")).expect("saved salt"), b"OLD-SALT");
        assert_eq!(
            fs::read(pre.join("insurance_documents/old.pdf")).expect("saved doc"),
            b"old"
        );
        assert!(!dst.path().join("insurance_documents/old.pdf").exists());
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let mut writer = ZipWriter::new(File::create(path).expect("zip"));
        for (name, bytes) in entries {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .expect("entry");
            writer.write_all(bytes).expect("bytes");
        }
        writer.finish().expect("finish");
    }

    fn manifest_json(migrations: &[&str]) -> Vec<u8> {
        serde_json::to_vec(&BackupManifest {
            format: BACKUP_FORMAT,
            app_version: "1.0.0".into(),
            created_at: 0,
            migrations: migrations.iter().map(|m| m.to_string()).collect(),
            files: vec![],
        })
        .expect("json")
    }

    #[test]
    fn archives_with_unknown_or_escaping_entries_are_rejected() {
        let dir = tempfile::tempdir().expect("dir");

        let evil = dir.path().join("evil.zip");
        write_zip(
            &evil,
            &[
                (MANIFEST_NAME, &manifest_json(&KNOWN)),
                (DB_FILE_NAME, b"db"),
                ("../outside.txt", b"x"),
            ],
        );
        assert!(matches!(
            inspect_backup(&evil, &KNOWN),
            Err(AppError::Validation(k)) if k == "validation.backupInvalidArchive"
        ));

        let stray = dir.path().join("stray.zip");
        write_zip(
            &stray,
            &[
                (MANIFEST_NAME, &manifest_json(&KNOWN)),
                (DB_FILE_NAME, b"db"),
                ("notes.txt", b"x"),
            ],
        );
        assert!(inspect_backup(&stray, &KNOWN).is_err());

        let no_manifest = dir.path().join("nomanifest.zip");
        write_zip(&no_manifest, &[(DB_FILE_NAME, b"db")]);
        assert!(inspect_backup(&no_manifest, &KNOWN).is_err());

        let not_zip = dir.path().join("plain.txt");
        fs::write(&not_zip, b"hello").expect("write");
        assert!(inspect_backup(&not_zip, &KNOWN).is_err());
    }

    #[test]
    fn backups_from_a_newer_app_are_flagged_and_not_restored() {
        let dir = tempfile::tempdir().expect("dir");
        let archive = dir.path().join("newer.zip");
        write_zip(
            &archive,
            &[
                (
                    MANIFEST_NAME,
                    &manifest_json(&["001_initial_schema", "099_future"]),
                ),
                (DB_FILE_NAME, b"db"),
            ],
        );
        let inspection = inspect_backup(&archive, &KNOWN).expect("inspect");
        assert!(inspection.newer_than_app);

        let dst = tempfile::tempdir().expect("dst");
        fs::write(dst.path().join("salt"), b"keep").expect("salt");
        let err = restore_backup(dst.path(), &archive, &KNOWN).expect_err("must refuse");
        assert!(matches!(err, AppError::Validation(k) if k == "validation.backupNewerThanApp"));
        assert_eq!(
            fs::read(dst.path().join("salt")).expect("untouched"),
            b"keep"
        );
        assert!(fs::read_dir(dst.path())
            .expect("dir")
            .filter_map(|e| e.ok())
            .all(|e| !e.file_name().to_string_lossy().starts_with("pre-restore-")));
    }

    #[test]
    fn export_dumps_user_tables_and_strips_secrets() {
        let dir = tempfile::tempdir().expect("dir");
        let conn = seed_account(dir.path());
        let dest = dir.path().join("export.json");

        let summary = export_all_data(&conn, &dest, "1.0.0").expect("export");
        assert_eq!(summary.table_count, 2); // user_profile + loans
        assert_eq!(summary.row_count, 2);

        let doc: serde_json::Value =
            serde_json::from_slice(&fs::read(&dest).expect("read")).expect("json");
        let tables = doc["tables"].as_object().expect("tables");
        assert!(tables.contains_key("loans"));
        assert!(tables.contains_key("user_profile"));
        assert!(
            !tables.contains_key("app_config"),
            "API keys are not exported"
        );
        assert!(
            !tables.contains_key("_migrations"),
            "migration bookkeeping is not exported"
        );
        let profile = &tables["user_profile"][0];
        assert_eq!(profile["name"], "Test");
        assert!(
            profile.get("mcp_server_token").is_none(),
            "token is stripped"
        );
        assert_eq!(tables["loans"][0]["principal"], "1000.50");
        assert_eq!(tables["loans"][0]["note"], "0102", "blobs are hex-encoded");
    }

    #[test]
    fn verify_database_reports_ok_for_a_healthy_file() {
        let dir = tempfile::tempdir().expect("dir");
        let conn = seed_account(dir.path());
        let report = verify_database(&conn).expect("verify");
        assert!(report.ok, "{:?}", report.messages);
    }

    #[test]
    fn data_location_sums_the_folder_size() {
        let dir = tempfile::tempdir().expect("dir");
        let _conn = seed_account(dir.path());
        let location = data_location(dir.path(), &dir.path().join(DB_FILE_NAME)).expect("loc");
        assert!(location.total_bytes >= 16 + 3 + 8 + 4);
        assert!(location.db_path.ends_with(DB_FILE_NAME));
    }
}
