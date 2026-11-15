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

/// Where an attachment's content is, with device sync (ADR 0030). A row without a blob (a plain
/// file from before 0.13) and a profile whose store is unavailable report `Here`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
pub enum AttachmentContent {
    /// Held on this device, and known to the sync server or not syncing.
    #[serde(rename = "here")]
    Here,
    /// Held on this device; the sync server has not confirmed it yet.
    #[serde(rename = "waitingUpload")]
    WaitingUpload,
    /// On another device or the sync server, not fetched here (yet).
    #[serde(rename = "notHere")]
    NotHere,
    /// Being fetched now.
    #[serde(rename = "downloading")]
    Downloading,
    /// The sync server does not have it (not uploaded yet from the other device).
    #[serde(rename = "notOnServer")]
    NotOnServer,
    /// Fetching it failed in a way a plain retry does not fix (it could not be opened, or its ID
    /// is not valid).
    #[serde(rename = "failed")]
    Failed,
    /// Neither here nor on its way: the content is lost on this device.
    #[serde(rename = "missing")]
    Missing,
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
