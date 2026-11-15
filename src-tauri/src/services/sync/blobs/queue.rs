//! This device's queue of document and photo content to move to or from the sync server
//! (`sync_blobs`, migration 035, ADR 0030). One entry per blob a syncing attachment row names:
//! `up` held here and not confirmed by the server, `down` named and not held here, `done` held
//! here and known to the server.
//!
//! The queue is a function of the rows and the store, never fed by hooks in the services:
//! `reconcile` adds what a row names, drops what no row names, and moves an entry when its
//! content was lost or found. It runs after every merge and restore that touched attachments
//! (`repair::invariants`) and before the lanes take work. Turning sync off empties the queue, and
//! so does a replace, whose repair then rebuilds it from the vault's rows and this store.
//!
//! A blob held here is queued `up` unless the queue knows the server has it: an upload starts by
//! asking the server, so confirming a blob the server already holds costs no body, while
//! skipping one it lacks would lose the only copy.

use rusqlite::{params, Connection, OptionalExtension};

use super::{canonical_id, sync_on};
use crate::db::files_schema;
use crate::error::{AppError, Result};
use crate::models::attachments::AttachmentKind;
use crate::services::attachment_store::REFERENCES;
use crate::services::sync::in_savepoint;
use crate::services::sync::registry::{self, TableClass};

/// The state of an entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlobState {
    /// Held here; the server has not confirmed it.
    Up,
    /// Named by a row, not held here.
    Down,
    /// Held here and known to the server.
    Done,
}

impl BlobState {
    pub fn as_str(self) -> &'static str {
        match self {
            BlobState::Up => "up",
            BlobState::Down => "down",
            BlobState::Done => "done",
        }
    }
}

/// Why a transfer failed; `as_str` is the `last_error` code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferError {
    /// The sync server could not be reached.
    Network,
    /// The sync server answered with an error.
    Server,
    /// The sync server does not have the blob (not uploaded yet from the other device).
    NotOnServer,
    /// The downloaded stream did not open or was incomplete.
    Unreadable,
    /// No room: the server's quota or free space, or this device's.
    NoRoom,
    /// The blob ID is not a canonical UUID: never transferred.
    InvalidId,
    /// A local error (the store, the database).
    Local,
}

impl TransferError {
    pub fn as_str(self) -> &'static str {
        match self {
            TransferError::Network => "network",
            TransferError::Server => "server",
            TransferError::NotOnServer => "notOnServer",
            TransferError::Unreadable => "unreadable",
            TransferError::NoRoom => "noRoom",
            TransferError::InvalidId => "invalidId",
            TransferError::Local => "local",
        }
    }

    /// When to try again after the `failures`-th failure in a row: `base · 2^(failures − 1)`
    /// seconds up to a cap per kind; a full server waits a fixed half hour, an invalid ID forever.
    fn next_try_at(self, failures: u32, now: i64) -> i64 {
        let (base, cap): (i64, i64) = match self {
            TransferError::Network | TransferError::Server | TransferError::Local => (30, 1_800),
            TransferError::NotOnServer => (300, 3_600),
            TransferError::Unreadable => (600, 21_600),
            TransferError::NoRoom => return now.saturating_add(1_800),
            TransferError::InvalidId => return i64::MAX,
        };
        let doublings = failures.saturating_sub(1);
        let delay = if doublings >= 32 {
            cap
        } else {
            (base << doublings).min(cap)
        };
        now.saturating_add(delay)
    }
}

/// One entry a lane takes: what to move and how often it failed before.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transfer {
    pub blob_id: String,
    pub state: BlobState,
    pub thumbnail: bool,
    pub wanted: bool,
    pub attempts: u32,
    /// The plaintext size of a blob held here (uploads); `None` for a download.
    pub size: Option<u64>,
}

/// What a `reconcile` did: entries added as `up` and as `down`, entries dropped because no row
/// names their blob any more, entries whose content was lost or found.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Reconciled {
    pub queued_up: usize,
    pub queued_down: usize,
    pub dropped: usize,
    pub changed: usize,
}

/// Counts of attachment rows (a photo once, whatever its two blobs do) by where their content is.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AttachmentSummary {
    /// Rows with content held here that the server has not confirmed.
    pub waiting_upload: u32,
    /// The plaintext bytes of those blobs.
    pub waiting_upload_bytes: i64,
    /// Rows with content not here yet (being fetched, waiting to be, or retried after a transient
    /// failure).
    pub not_here: u32,
    /// Rows with content the server does not have.
    pub not_on_server: u32,
    /// Rows with content that could not be fetched (unreadable, or an invalid ID).
    pub failed: u32,
}

/// The owning row's date per attachment table: the download order among rows (`row_at`).
const ROW_AT: [(&str, &str); 3] = [
    ("insurance_documents", "uploaded_at"),
    ("real_estate_documents", "uploaded_at"),
    ("real_estate_photos", "created_at"),
];

/// The blob column that holds a photo's thumbnail.
const THUMBNAIL_COLUMN: &str = "thumb_blob_id";

/// Clears an entry's retry bookkeeping when its state changes.
const FRESH: &str = "attempts = 0, next_try_at = 0, last_error = NULL, last_error_at = NULL";

/// Every blob a syncing attachment row names, as `(b, thumbnail, row_at, tbl, rid)`: one line per
/// row and blob column of `attachment_store::REFERENCES`, only rows of synced tables that meet
/// their row condition (`registry::row_condition`), since no other row ever reaches the server.
fn references() -> Result<String> {
    let mut parts = Vec::new();
    for (table, column) in REFERENCES {
        let spec = registry::spec(table)
            .ok_or_else(|| AppError::Internal(format!("{table} is not in the sync registry")))?;
        if spec.class != TableClass::Synced {
            continue;
        }
        let row_at = ROW_AT
            .iter()
            .find(|(t, _)| *t == table)
            .map(|(_, c)| *c)
            .ok_or_else(|| AppError::Internal(format!("{table} has no row date for the queue")))?;
        let condition = registry::row_condition(spec, "r").unwrap_or_else(|| "1".into());
        parts.push(format!(
            "SELECT r.\"{column}\" AS b, {thumbnail} AS thumbnail, r.\"{row_at}\" AS row_at, \
               '{table}' AS tbl, r.id AS rid \
             FROM main.\"{table}\" r WHERE r.\"{column}\" IS NOT NULL AND {condition}",
            thumbnail = i32::from(column == THUMBNAIL_COLUMN),
        ));
    }
    Ok(parts.join(" UNION ALL "))
}

/// SQL: the blob `id` is complete in the store (an incoming one is not held yet).
fn held(id: &str) -> String {
    format!("EXISTS (SELECT 1 FROM files.blobs f WHERE f.id = {id} AND f.sha256 <> '')")
}

/// Brings the queue in line with the rows and the store; the same rows and store give the same
/// queue however often it runs. Drops entries no syncing row names; adds each newly named blob as
/// `up` when held here and `down` otherwise (a blob ID that is not canonical as `down` with
/// `invalidId`, never offered); turns `up` and `done` entries whose content is no longer here into
/// `down`, and `down` entries whose content is here into `up` (losing it forgot whether the
/// server had it; the download lane marks what it fetched `done` itself). Does nothing without the
/// store or with sync off.
pub fn reconcile(conn: &Connection, now: i64) -> Result<Reconciled> {
    if !files_schema::is_attached(conn)? || !sync_on(conn)? {
        return Ok(Reconciled::default());
    }
    let refs = references()?;
    let entry_held = held("sync_blobs.blob_id");
    in_savepoint(conn, "sync_blobs_reconcile", || {
        let dropped = conn.execute(
            &format!("DELETE FROM main.sync_blobs WHERE blob_id NOT IN (SELECT b FROM ({refs}))"),
            [],
        )?;
        let lost = conn.execute(
            &format!(
                "UPDATE main.sync_blobs SET state = 'down', {FRESH} \
                 WHERE state IN ('up', 'done') AND NOT {entry_held}"
            ),
            [],
        )?;
        let found = conn.execute(
            &format!(
                "UPDATE main.sync_blobs SET state = 'up', queued_at = ?1, {FRESH} \
                 WHERE state = 'down' AND last_error IS NOT 'invalidId' AND {entry_held}"
            ),
            [now],
        )?;
        let new: Vec<(String, bool, i64, bool)> = {
            let mut stmt = conn.prepare(&format!(
                "SELECT n.b, MAX(n.thumbnail), MIN(n.row_at), {held_here} FROM ({refs}) n \
                 WHERE NOT EXISTS (SELECT 1 FROM main.sync_blobs q WHERE q.blob_id = n.b) \
                 GROUP BY n.b ORDER BY n.b",
                held_here = held("n.b"),
            ))?;
            let rows = stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                .collect::<std::result::Result<_, _>>()?;
            rows
        };
        let mut report = Reconciled {
            dropped,
            changed: lost + found,
            ..Reconciled::default()
        };
        let mut insert = conn.prepare(
            "INSERT INTO main.sync_blobs \
               (blob_id, state, thumbnail, row_at, queued_at, next_try_at, last_error, last_error_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )?;
        for (blob, thumbnail, row_at, held_here) in new {
            if canonical_id(&blob).is_none() {
                log::debug!("[SYNC] A document or photo names an invalid blob ID: {blob}");
                insert.execute(params![
                    blob,
                    BlobState::Down.as_str(),
                    thumbnail,
                    row_at,
                    now,
                    i64::MAX,
                    TransferError::InvalidId.as_str(),
                    now
                ])?;
                report.queued_down += 1;
                continue;
            }
            let state = if held_here {
                report.queued_up += 1;
                BlobState::Up
            } else {
                report.queued_down += 1;
                BlobState::Down
            };
            insert.execute(params![
                blob,
                state.as_str(),
                thumbnail,
                row_at,
                now,
                0,
                None::<String>,
                None::<i64>
            ])?;
        }
        Ok(report)
    })
}

fn attempts_of(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

/// The next upload that is due: an `up` entry held here, not being transferred, of at most
/// `max_plain` bytes when given; the oldest queued first.
pub fn next_upload(
    conn: &Connection,
    now: i64,
    max_plain: Option<u64>,
) -> Result<Option<Transfer>> {
    if !files_schema::is_attached(conn)? {
        return Ok(None);
    }
    let max_plain = max_plain.map(|m| i64::try_from(m).unwrap_or(i64::MAX));
    let found = conn
        .query_row(
            "SELECT q.blob_id, q.thumbnail, q.wanted, q.attempts, f.size \
             FROM main.sync_blobs q JOIN files.blobs f ON f.id = q.blob_id AND f.sha256 <> '' \
             WHERE q.state = 'up' AND q.active = 0 AND q.next_try_at <= ?1 \
               AND (?2 IS NULL OR f.size <= ?2) \
             ORDER BY q.queued_at, q.blob_id LIMIT 1",
            params![now, max_plain],
            |r| {
                Ok(Transfer {
                    blob_id: r.get(0)?,
                    state: BlobState::Up,
                    thumbnail: r.get(1)?,
                    wanted: r.get(2)?,
                    attempts: attempts_of(r.get(3)?),
                    size: Some(u64::try_from(r.get::<_, i64>(4)?).unwrap_or(0)),
                })
            },
        )
        .optional()?;
    Ok(found)
}

/// The next download that is due: a `down` entry not being transferred that is wanted, a
/// thumbnail, or any full content when `eager_full` (a desktop); wanted first, then thumbnails,
/// then the oldest row.
pub fn next_download(conn: &Connection, now: i64, eager_full: bool) -> Result<Option<Transfer>> {
    let found = conn
        .query_row(
            "SELECT blob_id, thumbnail, wanted, attempts FROM main.sync_blobs \
             WHERE state = 'down' AND active = 0 AND next_try_at <= ?1 \
               AND last_error IS NOT 'invalidId' AND (wanted = 1 OR thumbnail = 1 OR ?2) \
             ORDER BY wanted DESC, thumbnail DESC, row_at, blob_id LIMIT 1",
            params![now, eager_full],
            |r| {
                Ok(Transfer {
                    blob_id: r.get(0)?,
                    state: BlobState::Down,
                    thumbnail: r.get(1)?,
                    wanted: r.get(2)?,
                    attempts: attempts_of(r.get(3)?),
                    size: None,
                })
            },
        )
        .optional()?;
    Ok(found)
}

/// Marks a transfer of `blob_id` as running or not.
pub fn set_active(conn: &Connection, blob_id: &str, active: bool) -> Result<()> {
    conn.execute(
        "UPDATE main.sync_blobs SET active = ?2 WHERE blob_id = ?1",
        params![blob_id, active],
    )?;
    Ok(())
}

/// No transfer runs (a lane starting): every entry stops being active.
pub fn clear_active(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE main.sync_blobs SET active = 0 WHERE active = 1", [])?;
    Ok(())
}

/// The server confirmed `blob_id` (an upload), or it was fetched and stored here (a download).
pub fn note_done(conn: &Connection, blob_id: &str) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE main.sync_blobs SET state = 'done', active = 0, {FRESH} WHERE blob_id = ?1"
        ),
        [blob_id],
    )?;
    Ok(())
}

/// A transfer of `blob_id` failed: one more attempt, and the next try after its kind's backoff.
pub fn note_failed(conn: &Connection, blob_id: &str, error: TransferError, now: i64) -> Result<()> {
    let attempts: Option<i64> = conn
        .query_row(
            "SELECT attempts FROM main.sync_blobs WHERE blob_id = ?1",
            [blob_id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(attempts) = attempts else {
        return Ok(());
    };
    let failures = attempts_of(attempts).saturating_add(1);
    conn.execute(
        "UPDATE main.sync_blobs SET attempts = ?2, next_try_at = ?3, last_error = ?4, \
           last_error_at = ?5, active = 0 \
         WHERE blob_id = ?1",
        params![
            blob_id,
            i64::from(failures),
            error.next_try_at(failures, now),
            error.as_str(),
            now
        ],
    )?;
    Ok(())
}

/// No row names `blob_id` any more (its row went during the transfer): the entry goes.
pub fn note_gone(conn: &Connection, blob_id: &str) -> Result<()> {
    conn.execute("DELETE FROM main.sync_blobs WHERE blob_id = ?1", [blob_id])?;
    Ok(())
}

/// The server lost a blob it had confirmed and this device holds it (the daily pass): the `done`
/// entry waits for upload again. Whether it changed.
pub fn requeue_upload(conn: &Connection, blob_id: &str, now: i64) -> Result<bool> {
    if !files_schema::is_attached(conn)? {
        return Ok(false);
    }
    let changed = conn.execute(
        &format!(
            "UPDATE main.sync_blobs SET state = 'up', queued_at = ?2, {FRESH} \
             WHERE blob_id = ?1 AND state = 'done' AND {}",
            held("sync_blobs.blob_id")
        ),
        params![blob_id, now],
    )?;
    Ok(changed > 0)
}

fn table_of(kind: AttachmentKind) -> &'static str {
    match kind {
        AttachmentKind::Photo => "real_estate_photos",
        AttachmentKind::RealEstateDocument => "real_estate_documents",
        AttachmentKind::InsuranceDocument => "insurance_documents",
    }
}

/// SQL: makes a `down` entry wanted and due now (never one with an invalid ID).
const WANT: &str = "UPDATE main.sync_blobs SET wanted = 1, next_try_at = 0 \
                    WHERE state = 'down' AND last_error IS NOT 'invalidId'";

/// The person opened a row: its full content is wanted and due now.
pub fn want_row(conn: &Connection, kind: AttachmentKind, id: &str) -> Result<()> {
    conn.execute(
        &format!(
            "{WANT} AND blob_id = (SELECT blob_id FROM main.\"{}\" WHERE id = ?1)",
            table_of(kind)
        ),
        [id],
    )?;
    Ok(())
}

/// The person asked to try a row again: its full content is wanted, and every blob of the row
/// that waits to move starts its count again, due now (never one with an invalid ID).
pub fn retry_row(conn: &Connection, kind: AttachmentKind, id: &str) -> Result<()> {
    want_row(conn, kind, id)?;
    let table = table_of(kind);
    let blobs = REFERENCES
        .iter()
        .filter(|(t, _)| *t == table)
        .map(|(_, column)| format!("SELECT \"{column}\" FROM main.\"{table}\" WHERE id = ?1"))
        .collect::<Vec<_>>()
        .join(" UNION ALL ");
    conn.execute(
        &format!(
            "UPDATE main.sync_blobs SET {FRESH} \
             WHERE state IN ('up', 'down') AND last_error IS NOT 'invalidId' AND blob_id IN ({blobs})"
        ),
        [id],
    )?;
    Ok(())
}

/// `blob_id`'s content is wanted and due now. Whether a `down` entry was there to want.
pub fn want_blob(conn: &Connection, blob_id: &str) -> Result<bool> {
    Ok(conn.execute(&format!("{WANT} AND blob_id = ?1"), [blob_id])? > 0)
}

/// "Download everything to this phone": every full content not here is wanted and due now.
/// Returns the rows whose full content is now wanted.
pub fn want_all(conn: &Connection) -> Result<u32> {
    conn.execute(&format!("{WANT} AND thumbnail = 0"), [])?;
    let rows: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM ({}) r JOIN main.sync_blobs q ON q.blob_id = r.b \
             WHERE r.thumbnail = 0 AND q.state = 'down' AND q.wanted = 1 \
               AND q.last_error IS NOT 'invalidId'",
            references()?
        ),
        [],
        |r| r.get(0),
    )?;
    Ok(rows as u32)
}

/// "Sync now": every transfer waiting for its backoff is due now, except those waiting for room
/// on the server and those that can never move.
pub fn retry_now(conn: &Connection) -> Result<()> {
    conn.execute(
        "UPDATE main.sync_blobs SET next_try_at = 0 \
         WHERE state IN ('up', 'down') AND COALESCE(last_error, '') NOT IN ('noRoom', 'invalidId')",
        [],
    )?;
    Ok(())
}

/// Rows with content held here that the server has not confirmed (a photo once).
pub fn uploads_waiting(conn: &Connection) -> Result<u32> {
    let rows: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM (SELECT DISTINCT r.tbl, r.rid FROM ({}) r \
               JOIN main.sync_blobs q ON q.blob_id = r.b WHERE q.state = 'up')",
            references()?
        ),
        [],
        |r| r.get(0),
    )?;
    Ok(rows as u32)
}

/// The rows by where their content is (a photo once, whatever its two blobs do). Among content
/// not here a row counts in its worst state only (failed, then not on the server, then not here);
/// `waiting_upload` counts the rows with any blob waiting to upload, `waiting_upload_bytes` the
/// plaintext size of those blobs.
pub fn summary(conn: &Connection) -> Result<AttachmentSummary> {
    let (waiting_upload, failed, not_on_server, not_here): (i64, i64, i64, i64) = conn.query_row(
        &format!(
            "WITH per_row AS (
               SELECT MAX(q.state = 'up') AS up,
                 MAX(q.state = 'down' AND q.active = 0
                     AND COALESCE(q.last_error, '') IN ('unreadable', 'invalidId')) AS failed,
                 MAX(q.state = 'down' AND q.active = 0
                     AND COALESCE(q.last_error, '') = 'notOnServer') AS gone,
                 MAX(q.state = 'down') AS down
               FROM ({}) r JOIN main.sync_blobs q ON q.blob_id = r.b
               GROUP BY r.tbl, r.rid)
             SELECT COALESCE(SUM(up), 0), COALESCE(SUM(failed), 0),
               COALESCE(SUM(gone AND NOT failed), 0),
               COALESCE(SUM(down AND NOT failed AND NOT gone), 0)
             FROM per_row",
            references()?
        ),
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let waiting_upload_bytes: i64 = if files_schema::is_attached(conn)? {
        conn.query_row(
            &format!(
                "SELECT COALESCE(SUM(f.size), 0) FROM main.sync_blobs q \
                 JOIN files.blobs f ON f.id = q.blob_id AND f.sha256 <> '' \
                 WHERE q.state = 'up' AND q.blob_id IN (SELECT b FROM ({}))",
                references()?
            ),
            [],
            |r| r.get(0),
        )?
    } else {
        0
    };
    Ok(AttachmentSummary {
        waiting_upload: waiting_upload as u32,
        waiting_upload_bytes,
        not_here: not_here as u32,
        not_on_server: not_on_server as u32,
        failed: failed as u32,
    })
}
