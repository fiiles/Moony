//! Backup, restore, export and data-location models

use serde::{Deserialize, Serialize};
use specta::Type;

/// One file inside a backup archive.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct BackupFileEntry {
    pub path: String,
    pub bytes: u64,
}

/// `manifest.json` written into every backup archive.
#[derive(Debug, Clone, Serialize, Deserialize, Type, PartialEq)]
pub struct BackupManifest {
    pub format: u32,
    #[serde(rename = "appVersion")]
    pub app_version: String,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
    /// Names of the migrations applied to the database inside the archive.
    pub migrations: Vec<String>,
    pub files: Vec<BackupFileEntry>,
}

/// What the UI shows before asking the user to confirm a restore.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct BackupInspection {
    pub manifest: BackupManifest,
    #[serde(rename = "archivePath")]
    pub archive_path: String,
    #[serde(rename = "archiveBytes")]
    pub archive_bytes: u64,
    #[serde(rename = "attachmentCount")]
    pub attachment_count: u32,
    /// The archive's database was migrated by a newer Moony than this build.
    #[serde(rename = "newerThanApp")]
    pub newer_than_app: bool,
}

/// Result of `PRAGMA quick_check` (+ `cipher_integrity_check`).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct IntegrityReport {
    pub ok: bool,
    pub messages: Vec<String>,
}

/// Where the data lives on this machine.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DataLocation {
    #[serde(rename = "dataDir")]
    pub data_dir: String,
    #[serde(rename = "dbPath")]
    pub db_path: String,
    #[serde(rename = "totalBytes")]
    pub total_bytes: u64,
}

/// Summary of a full JSON export.
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ExportSummary {
    pub path: String,
    #[serde(rename = "tableCount")]
    pub table_count: u32,
    #[serde(rename = "rowCount")]
    pub row_count: u64,
}
