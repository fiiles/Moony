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
