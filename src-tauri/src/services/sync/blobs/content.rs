//! Where an attachment's content is (`AttachmentContent`), from the store and the transfer queue.

use rusqlite::{Connection, OptionalExtension};

use super::sync_on;
use crate::db::files_schema;
use crate::error::Result;
use crate::models::attachments::AttachmentContent;

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

    /// The content of the blob a row names. A row without a blob (a legacy plain file) and an
    /// unavailable store are `Here` (the store's unavailable state is reported where it is
    /// read). Held here: `WaitingUpload` while the server has not confirmed it (its entry is `up`,
    /// or, with sync on, the queue has not seen it yet), else `Here`. Not held: `Missing` without
    /// an entry, else from the entry: `Downloading`, `NotOnServer`, `Failed` (unreadable or an
    /// invalid ID) or `NotHere`.
    pub fn of(&self, blob_id: Option<&str>) -> Result<AttachmentContent> {
        let Some(id) = blob_id else {
            return Ok(AttachmentContent::Here);
        };
        if !self.store {
            return Ok(AttachmentContent::Here);
        }
        let held: bool = self
            .conn
            .prepare_cached(
                "SELECT EXISTS (SELECT 1 FROM files.blobs WHERE id = ?1 AND sha256 <> '')",
            )?
            .query_row([id], |r| r.get(0))?;
        let entry: Option<(String, bool, Option<String>)> = self
            .conn
            .prepare_cached(
                "SELECT state, active, last_error FROM main.sync_blobs WHERE blob_id = ?1",
            )?
            .query_row([id], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .optional()?;
        if held {
            return Ok(match entry {
                Some((state, _, _)) if state == "up" => AttachmentContent::WaitingUpload,
                None if self.sync_on => AttachmentContent::WaitingUpload,
                _ => AttachmentContent::Here,
            });
        }
        Ok(match entry {
            None => AttachmentContent::Missing,
            Some((_, true, _)) => AttachmentContent::Downloading,
            Some((_, false, error)) => match error.as_deref() {
                Some("notOnServer") => AttachmentContent::NotOnServer,
                Some("unreadable" | "invalidId") => AttachmentContent::Failed,
                _ => AttachmentContent::NotHere,
            },
        })
    }
}
