//! The demo profile (ADR 0037): what the first-run choice screen needs to know.

use serde::{Deserialize, Serialize};
use specta::Type;

/// The `demo_status` answer, readable with no profile open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
pub struct DemoStatus {
    /// A complete demo profile is stored (its completeness marker is there) and no real profile
    /// exists next to it.
    #[serde(rename = "exists")]
    pub exists: bool,
    /// When the stored demo was last opened (unix seconds); `None` when there is none.
    #[serde(rename = "lastOpenedAt")]
    pub last_opened_at: Option<i64>,
    /// The demo is the open profile right now (survives a webview reload).
    #[serde(rename = "active")]
    pub active: bool,
}
