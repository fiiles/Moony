//! Document and photo content on the sync server (ADR 0030). The rows travel in changesets like
//! any synced row; their content (blobs in `attachments.db`) moves separately, one sealed stream
//! per blob. `queue` is this device's transfer queue (`sync_blobs`), reconciled from the rows and
//! the store; `content` says per blob where the content is; `stream` moves one blob between the
//! store and the wire, sealed and opened a chunk at a time; `transfer` moves one queued blob to or
//! from the server and runs the queue one blob at a time, under the shared `policy`.

pub mod content;
pub mod policy;
pub mod queue;
pub mod stream;
pub mod transfer;

#[cfg(test)]
mod tests;

use rusqlite::Connection;
use uuid::Uuid;

use crate::error::Result;

/// The blob ID as a UUID when `text` is its canonical form (lowercase, hyphenated), the only form
/// the sync server accepts and the only one this app writes. Anything else (upper case, braces,
/// the 32-hex form, a path) is `None`.
pub fn canonical_id(text: &str) -> Option<Uuid> {
    let id = Uuid::parse_str(text).ok()?;
    (id.hyphenated().to_string() == text).then_some(id)
}

/// Whether sync is on for this profile (one `SELECT`; no row is off).
pub(crate) fn sync_on(conn: &Connection) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT COALESCE((SELECT enabled FROM main.sync_state WHERE id = 1), 0)",
        [],
        |r| r.get(0),
    )?)
}
