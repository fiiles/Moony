//! Where an attachment's content is (`AttachmentContent`), from the transfer queue, and from the
//! store only for a blob the queue has no entry for.

use rusqlite::{Connection, OptionalExtension};

use super::{queue, sync_on};
use crate::db::files_schema;
use crate::error::{AppError, Result};
use crate::models::attachments::{AttachmentContent, AttachmentKind};
use crate::services::attachment_store::complete_sql;
use crate::services::sync::registry::{self, TableClass};

/// Answers `AttachmentContent` for many blobs of one listing; whether sync is on and the store is
/// attached is read once, in `new`.
pub struct ContentReader<'c> {
    conn: &'c Connection,
    store: bool,
    sync_on: bool,
}

impl<'c> ContentReader<'c> {
    pub fn new(conn: &'c Connection) -> Result<Self> {
        Ok(Self {
            conn,
            store: files_schema::is_attached(conn)?,
            sync_on: sync_on(conn)?,
        })
    }

    /// The content of the blob a row names; `syncs` is whether that row meets its row condition
    /// (`row_syncs`). A row without a blob (a legacy plain file) and an unavailable store are
    /// `Here` (the store's unavailable state is reported where it is read).
    ///
    /// A blob with a queue entry is answered from the entry alone, never the store (ADR 0030: the
    /// store's completeness check walks the blob's pages): `up` is `WaitingUpload` for a row that
    /// syncs, or `Failed` when the server refused it or holds a conflict (`queue::STUCK`; "try
    /// again" makes it wait again); `done` is `Here`, and so is `up` for a row that does not sync
    /// and `up` under an invalid ID (held here, never transferred; the summary counts it failed); `down` is `Downloading`, `NotOnServer`, `Failed` (unreadable, an invalid
    /// ID, refused by the server or a conflict there) or `NotHere`. Content lost from the store
    /// outside a merge therefore reads as held until the next full reconcile. Only a blob without
    /// an entry (sync off, a row that does not sync, a row the queue has not seen yet) is looked
    /// up in the store: complete there, `WaitingUpload` for a row that syncs with sync on, else
    /// `Here`; not complete, `Missing`.
    pub fn of(&self, blob_id: Option<&str>, syncs: bool) -> Result<AttachmentContent> {
        let Some(id) = blob_id else {
            return Ok(AttachmentContent::Here);
        };
        if !self.store {
            return Ok(AttachmentContent::Here);
        }
        let entry: Option<(String, bool, Option<String>)> = self
            .conn
            .prepare_cached(
                "SELECT state, active, last_error FROM main.sync_blobs WHERE blob_id = ?1",
            )?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        let Some((state, active, error)) = entry else {
            let held: bool = self
                .conn
                .prepare_cached(&format!("SELECT {}", complete_sql("?1")))?
                .query_row([id], |r| r.get(0))?;
            return Ok(match held {
                true if syncs && self.sync_on => AttachmentContent::WaitingUpload,
                true => AttachmentContent::Here,
                false => AttachmentContent::Missing,
            });
        };
        Ok(match state.as_str() {
            // Content held here under an ID that can never sync: here, nothing to wait for.
            "up" if !syncs || error.as_deref() == Some("invalidId") => AttachmentContent::Here,
            "up" if queue::is_stuck(error.as_deref()) => AttachmentContent::Failed,
            "up" => AttachmentContent::WaitingUpload,
            "down" if active => AttachmentContent::Downloading,
            "down" if queue::download_failed(error.as_deref()) => AttachmentContent::Failed,
            "down" if error.as_deref() == Some("notOnServer") => AttachmentContent::NotOnServer,
            "down" => AttachmentContent::NotHere,
            _ => AttachmentContent::Here,
        })
    }
}

/// Whether an attachment row of `kind` with these blob columns syncs (`registry::row_condition`).
pub fn row_syncs(
    kind: AttachmentKind,
    blob_id: Option<&str>,
    thumb_blob_id: Option<&str>,
) -> Result<bool> {
    let table = queue::table_of(kind);
    let spec = registry::spec(table)
        .ok_or_else(|| AppError::Internal(format!("{table} is not in the sync registry")))?;
    if spec.class != TableClass::Synced {
        return Ok(false);
    }
    for column in spec.requires {
        let value = match *column {
            "blob_id" => blob_id,
            "thumb_blob_id" => thumb_blob_id,
            other => {
                return Err(AppError::Internal(format!(
                    "{table}.{other} is a row condition the content reader does not know"
                )))
            }
        };
        if value.is_none() {
            return Ok(false);
        }
    }
    Ok(true)
}
