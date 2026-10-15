//! Attachment kinds shared by the attachment commands (ADR 0017).

use serde::{Deserialize, Serialize};
use specta::Type;

/// Which attachment table a row id belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "kebab-case")]
pub enum AttachmentKind {
    Photo,
    RealEstateDocument,
    InsuranceDocument,
}

/// What the Data settings page shows about attachments: the encrypted store, the conversion of
/// plain files from earlier versions, and plain originals the app will not delete (ADR 0017).
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AttachmentStatus {
    /// `attachments.db` is attached and usable.
    #[serde(rename = "storeAvailable")]
    pub store_available: bool,
    /// Attachments stored encrypted.
    pub stored: u32,
    /// Attachments still in plain files that are waiting for the conversion.
    #[serde(rename = "legacyPending")]
    pub legacy_pending: u32,
    /// Attachments whose plain file is missing on disk.
    pub missing: u32,
    /// Attachments the last conversion run could not convert (retried at the next unlock).
    pub failed: u32,
    /// A conversion run is in progress.
    pub converting: bool,
    /// Items of the current or last run converted so far.
    pub done: u32,
    /// Items of the current or last run.
    pub total: u32,
    /// Plain originals the app found and does not delete: files and old data folders.
    pub leftovers: Vec<String>,
}
