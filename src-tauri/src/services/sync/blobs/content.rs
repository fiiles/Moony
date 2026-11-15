//! Where an attachment's content is (`AttachmentContent`), from the store and the transfer queue.

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
    /// `Here` (the store's unavailable state is reported where it is read). Complete here
    /// (`attachment_store::complete_sql`): for a row that syncs, `WaitingUpload` while the server
    /// has not confirmed it (its entry is `up`, or, with sync on, the queue has not seen it yet),
    /// `Failed` for an `up` entry that never moves by itself (`queue::STUCK`: refused, a
    /// conflict, an invalid ID; "try again" makes it wait again), else `Here`; a row that does not sync never uploads, so it is `Here`. Not complete here:
    /// `Missing` without an entry, else from the entry: `Downloading`, `NotOnServer`, `Failed`
    /// (unreadable, an invalid ID, refused by the server or a conflict there) or `NotHere`.
    pub fn of(&self, blob_id: Option<&str>, syncs: bool) -> Result<AttachmentContent> {
        let Some(id) = blob_id else {
            return Ok(AttachmentContent::Here);
        };
        if !self.store {
            return Ok(AttachmentContent::Here);
        }
        let held: bool = self
            .conn
            .prepare_cached(&format!("SELECT {}", complete_sql("?1")))?
            .query_row([id], |r| r.get(0))?;
        let entry: Option<(String, bool, Option<String>)> = self
            .conn
            .prepare_cached(
                "SELECT state, active, last_error FROM main.sync_blobs WHERE blob_id = ?1",
            )?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        if held {
            if !syncs {
                return Ok(AttachmentContent::Here);
            }
            return Ok(match entry {
                Some((state, _, error)) if state == "up" => {
                    if queue::is_stuck(error.as_deref()) {
                        AttachmentContent::Failed
                    } else {
                        AttachmentContent::WaitingUpload
                    }
                }
                None if self.sync_on => AttachmentContent::WaitingUpload,
                _ => AttachmentContent::Here,
            });
        }
        Ok(match entry {
            None => AttachmentContent::Missing,
            Some((_, true, _)) => AttachmentContent::Downloading,
            Some((_, false, error)) => match error.as_deref() {
                Some("notOnServer") => AttachmentContent::NotOnServer,
                Some("unreadable" | "invalidId" | "refused" | "conflict") => {
                    AttachmentContent::Failed
                }
                _ => AttachmentContent::NotHere,
            },
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
