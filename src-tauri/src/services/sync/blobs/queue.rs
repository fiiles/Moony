//! This device's queue of document and photo content to move to or from the sync server
//! (`sync_blobs`, migration 035, ADR 0030). One entry per blob a syncing attachment row names:
//! `up` held here and not confirmed by the server, `down` named and not held here, `done` held
//! here and known to the server.
//!
//! The queue is a function of the rows and the store, never fed by hooks in the services:
//! `reconcile` adds what a row names, drops what no row names, and moves an entry when its
//! content was lost or found. The full `reconcile` (every entry checked against the store) runs at
//! the end of a pull that merged anything, after every restore and replace, and when a lane
//! starts; each merged changeset, each sync round and each transfer run uses `reconcile_rows`,
//! which looks up only newly named blobs in the store. Turning sync off empties the queue, and
//! so does a replace, whose repair then rebuilds it from the vault's rows and this store.
//!
//! The queue is also where everything that runs often reads where content is (the status, the
//! content of a listing, the upload pick): `up` and `done` are content held here, `down` content
//! not here, as of the last reconcile, and `size` is the plaintext size of content held here. Reading the
//! store instead would walk each blob's pages (ADR 0030).
//!
//! A blob held here is queued `up` unless the queue knows the server has it: an upload starts by
//! asking the server, so confirming a blob the server already holds costs no body, while
//! skipping one it lacks would lose the only copy.

use rusqlite::{params, Connection, OptionalExtension};

use super::{canonical_id, sync_on};
use crate::db::files_schema;
use crate::error::{AppError, Result};
use crate::models::attachments::AttachmentKind;
use crate::services::attachment_store::{complete_sql, REFERENCES};
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

/// The longest wait a failure sets before the next try (`unreadable`'s cap). An entry due further
/// ahead than this (and not `i64::MAX`, never) was dated by a clock that has since been set back:
/// it counts as due.
pub const MAX_BACKOFF: i64 = 21_600;

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
    /// The sync server refuses the blob for good (too large, empty, a length it cannot take, or
    /// a server without the blob route): not retried by itself.
    Refused,
    /// The sync server holds other content under the blob's ID (a stored size that is not this
    /// blob's); it never overwrites a blob, so an upload cannot heal it: not retried by itself.
    Conflict,
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
            TransferError::Refused => "refused",
            TransferError::Conflict => "conflict",
        }
    }

    /// When to try again after the `failures`-th failure in a row: `base · 2^(failures − 1)`
    /// seconds up to a cap per kind (never more than [`MAX_BACKOFF`]); a full server waits a fixed
    /// half hour; an invalid ID, a refused blob and a conflict wait forever (`i64::MAX`).
    fn next_try_at(self, failures: u32, now: i64) -> i64 {
        let (base, cap): (i64, i64) = match self {
            TransferError::Network | TransferError::Server | TransferError::Local => (30, 1_800),
            TransferError::NotOnServer => (300, 3_600),
            TransferError::Unreadable => (600, 21_600),
            TransferError::NoRoom => return now.saturating_add(1_800),
            TransferError::InvalidId | TransferError::Refused | TransferError::Conflict => {
                return i64::MAX
            }
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
    /// Rows with content held here that the server has not confirmed and that can still upload.
    pub waiting_upload: u32,
    /// The plaintext bytes of those blobs.
    pub waiting_upload_bytes: i64,
    /// Rows with content not here yet (being fetched, waiting to be, or retried after a transient
    /// failure).
    pub not_here: u32,
    /// Rows with content the server does not have.
    pub not_on_server: u32,
    /// Rows with content that could not be fetched (unreadable, refused, a conflict, or an invalid
    /// ID), or held here and unable to upload by itself ([`STUCK`]).
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

/// SQL: the `last_error` codes of an entry that never moves by itself (an invalid ID, a refusal of
/// the blob, a conflict on the server): only "try again" on its row (`retry_row`) or a rebuilt
/// queue moves it. Such an entry is a failed item, never content waiting to move.
pub(crate) const STUCK: &str = "('invalidId', 'refused', 'conflict')";

/// Whether `last_error` is a code of [`STUCK`].
pub fn is_stuck(last_error: Option<&str>) -> bool {
    matches!(last_error, Some("invalidId" | "refused" | "conflict"))
}

/// Clears an entry's retry bookkeeping when its state changes.
const FRESH: &str = "attempts = 0, next_try_at = 0, last_error = NULL, last_error_at = NULL";

/// [`FRESH`] for the full reconcile's state changes, except that an entry with an invalid ID keeps
/// its failure: an ID does not become valid when its content appears or disappears.
const FRESH_KEEPING_INVALID: &str = "attempts = 0, \
    next_try_at = CASE WHEN last_error = 'invalidId' THEN next_try_at ELSE 0 END, \
    last_error_at = CASE WHEN last_error = 'invalidId' THEN last_error_at END, \
    last_error = CASE WHEN last_error = 'invalidId' THEN last_error END";

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

/// SQL: the blob `id` is complete on this device (`attachment_store::complete_sql`, the one
/// definition: an incoming or truncated blob is not held). It reads the blob's record past its
/// content (the store's overflow pages), so only the full `reconcile` asks it of every entry;
/// everything else asks it of one blob at a time (ADR 0030).
fn held(id: &str) -> String {
    complete_sql(id)
}

/// SQL: the recorded plaintext size of the blob `id` in the store (one blob: past its content,
/// like [`held`]); what `sync_blobs.size` keeps for an `up` entry.
fn store_size(id: &str) -> String {
    format!("(SELECT sf.size FROM files.blobs sf WHERE sf.id = {id})")
}

/// Brings the queue in line with the rows and the store; the same rows and store give the same
/// queue however often it runs. Drops entries no syncing row names; adds each newly named blob as
/// `up` when held here and `down` otherwise, with its recorded size when held (a blob ID that is
/// not canonical with `invalidId` and a retry time never reached: never offered, never taken as on
/// the server); turns `up` and `done` entries whose content is no longer here into `down`, and
/// `down` entries whose content is here into `up` (losing it forgot whether the server had it; the
/// download lane marks what it fetched `done` itself; an invalid ID keeps its failure either
/// way). Does nothing without the store or with sync off.
///
/// Checking every entry against the store reads each blob's record past its content, so this runs
/// once per restore and replace, once at the end of a pull that merged anything, and when a lane
/// starts; each merged changeset, each sync round and each transfer run uses only
/// `reconcile_rows`.
pub fn reconcile(conn: &Connection, now: i64) -> Result<Reconciled> {
    reconcile_with(conn, now, true)
}

/// The part of `reconcile` that rows can change (after one merged changeset, before a round's
/// uploads, at a transfer run): drops the entries no syncing row names any more and adds the newly
/// named blobs, looking each new one up in the store. A merge never removes or adds content
/// of a blob a row still names (the blob sweep takes only blobs no row names, and their entries go
/// here), so the entries' states against the store are left to the full `reconcile` the pull runs
/// at its end.
pub fn reconcile_rows(conn: &Connection, now: i64) -> Result<Reconciled> {
    reconcile_with(conn, now, false)
}

fn reconcile_with(conn: &Connection, now: i64, scan_store: bool) -> Result<Reconciled> {
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
        let (lost, found) = if scan_store {
            let lost = conn.execute(
                &format!(
                    "UPDATE main.sync_blobs SET state = 'down', size = NULL, \
                       {FRESH_KEEPING_INVALID} \
                     WHERE state IN ('up', 'done') AND NOT {entry_held}"
                ),
                [],
            )?;
            let found = conn.execute(
                &format!(
                    "UPDATE main.sync_blobs SET state = 'up', queued_at = ?1, size = {size}, \
                       {FRESH_KEEPING_INVALID} \
                     WHERE state = 'down' AND {entry_held}",
                    size = store_size("sync_blobs.blob_id"),
                ),
                [now],
            )?;
            // Content held here whose size the queue does not know yet (an entry from before
            // migration 038).
            conn.execute(
                &format!(
                    "UPDATE main.sync_blobs SET size = {} \
                     WHERE state IN ('up', 'done') AND size IS NULL",
                    store_size("sync_blobs.blob_id")
                ),
                [],
            )?;
            (lost, found)
        } else {
            (0, 0)
        };
        // Only newly named blobs are looked up in the store, one at a time.
        let new: Vec<(String, bool, i64, bool, Option<i64>)> = {
            let mut stmt = conn.prepare(&format!(
                "SELECT b, thumbnail, row_at, here, CASE WHEN here THEN {size} END FROM ( \
                   SELECT n.b AS b, MAX(n.thumbnail) AS thumbnail, MIN(n.row_at) AS row_at, \
                     {held_here} AS here FROM ({refs}) n \
                   WHERE NOT EXISTS (SELECT 1 FROM main.sync_blobs q WHERE q.blob_id = n.b) \
                   GROUP BY n.b) ORDER BY b",
                held_here = held("n.b"),
                size = store_size("b"),
            ))?;
            let rows = stmt
                .query_map([], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
                })?
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
               (blob_id, state, thumbnail, row_at, queued_at, next_try_at, last_error, \
                last_error_at, size) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;
        for (blob, thumbnail, row_at, held_here, size) in new {
            if canonical_id(&blob).is_none() {
                log::debug!("[SYNC] A document or photo names an invalid blob ID: {blob}");
                // Never transferred, never taken as on the server: held here it is `up` and
                // never picked (it reads as here and counts as failed), else a failed `down`.
                let state = if held_here {
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
                    i64::MAX,
                    TransferError::InvalidId.as_str(),
                    now,
                    if held_here { size } else { None }
                ])?;
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
                None::<i64>,
                if held_here { size } else { None }
            ])?;
        }
        Ok(report)
    })
}

fn attempts_of(value: i64) -> u32 {
    u32::try_from(value.max(0)).unwrap_or(u32::MAX)
}

/// The next upload that is due: an `up` entry not being transferred, of at most `max_plain`
/// bytes by the size the queue recorded when given (an entry of unknown size only without a
/// limit); the oldest queued first. Read from the queue alone: the transfer checks the one blob it
/// takes against the store (ADR 0030).
pub fn next_upload(
    conn: &Connection,
    now: i64,
    max_plain: Option<u64>,
) -> Result<Option<Transfer>> {
    next_upload_of(conn, now, max_plain, false)
}

/// `next_upload` among entries that never failed an attempt (`attempts = 0`): the sync round's
/// pick. Content that failed once (a cut, a passing error) is left to the lanes, so its backoff
/// running out never costs a round its budget again.
pub fn next_untried_upload(
    conn: &Connection,
    now: i64,
    max_plain: Option<u64>,
) -> Result<Option<Transfer>> {
    next_upload_of(conn, now, max_plain, true)
}

fn next_upload_of(
    conn: &Connection,
    now: i64,
    max_plain: Option<u64>,
    untried_only: bool,
) -> Result<Option<Transfer>> {
    if !files_schema::is_attached(conn)? {
        return Ok(None);
    }
    let max_plain = max_plain.map(|m| i64::try_from(m).unwrap_or(i64::MAX));
    let found = conn
        .query_row(
            &format!(
                "SELECT blob_id, thumbnail, wanted, attempts, size FROM main.sync_blobs \
                 WHERE state = 'up' AND active = 0 AND {due} \
                   AND (?2 IS NULL OR size <= ?2) AND (NOT ?3 OR attempts = 0) \
                 ORDER BY queued_at, blob_id LIMIT 1",
                due = due("next_try_at"),
            ),
            params![now, max_plain, untried_only],
            |r| {
                Ok(Transfer {
                    blob_id: r.get(0)?,
                    state: BlobState::Up,
                    thumbnail: r.get(1)?,
                    wanted: r.get(2)?,
                    attempts: attempts_of(r.get(3)?),
                    size: r
                        .get::<_, Option<i64>>(4)?
                        .map(|size| u64::try_from(size).unwrap_or(0)),
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
    next_download_skipping(conn, now, eager_full, &[])
}

/// `next_download` passing over the blobs in `skip` (a run leaves a blob another download of
/// this profile holds for its next run).
pub fn next_download_skipping(
    conn: &Connection,
    now: i64,
    eager_full: bool,
    skip: &[String],
) -> Result<Option<Transfer>> {
    let skipped = (0..skip.len())
        .map(|i| format!("?{}", i + 3))
        .collect::<Vec<_>>()
        .join(", ");
    let mut values: Vec<&dyn rusqlite::ToSql> = vec![&now, &eager_full];
    values.extend(skip.iter().map(|id| id as &dyn rusqlite::ToSql));
    let found = conn
        .query_row(
            &format!(
                "SELECT blob_id, thumbnail, wanted, attempts FROM main.sync_blobs \
                 WHERE state = 'down' AND active = 0 AND {due} \
                   AND last_error IS NOT 'invalidId' AND (wanted = 1 OR thumbnail = 1 OR ?2) \
                   AND blob_id NOT IN ({skipped}) \
                 ORDER BY wanted DESC, thumbnail DESC, row_at, blob_id LIMIT 1",
                due = due("next_try_at"),
            ),
            values.as_slice(),
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

/// SQL: the entry whose `next_try_at` is the SQL expression `column` is due at `?1`, the now of
/// the query: its time has come, or it lies further ahead than [`MAX_BACKOFF`] (set by a clock
/// that has since gone back), except `i64::MAX` (never by itself).
fn due(column: &str) -> String {
    format!(
        "({column} <= ?1 OR ({column} > ?1 + {MAX_BACKOFF} AND {column} < {never}))",
        never = i64::MAX
    )
}

/// The content of a `down` entry turned up here (a restore, a copy) before its download: the
/// entry waits for upload again, as `reconcile` would make it, so the server's `HEAD` says
/// whether it has it; nothing claims it is there. The transfer is over either way, so the entry
/// stops being active even when there was nothing to move (the content left again). Whether it
/// moved.
pub fn note_found(conn: &Connection, blob_id: &str, now: i64) -> Result<bool> {
    set_active(conn, blob_id, false)?;
    Ok(conn.execute(
        &format!(
            "UPDATE main.sync_blobs SET state = 'up', active = 0, queued_at = ?2, size = {}, \
               {FRESH} \
             WHERE blob_id = ?1 AND state = 'down' AND {}",
            store_size("sync_blobs.blob_id"),
            held("sync_blobs.blob_id")
        ),
        params![blob_id, now],
    )? > 0)
}

/// Marks a transfer of `blob_id` as running or not.
pub fn set_active(conn: &Connection, blob_id: &str, active: bool) -> Result<()> {
    conn.execute(
        "UPDATE main.sync_blobs SET active = ?2 WHERE blob_id = ?1",
        params![blob_id, active],
    )?;
    Ok(())
}

/// Entries in `state` still marked active while the caller holds that state's lane, so no
/// transfer of them can be running: a transfer of an earlier run was dropped (cut by its budget,
/// or its engine stopped) and could not record it. Each counts a failed attempt of the network's
/// kind, so its backoff applies as after any passing trouble. How many there were.
pub fn fail_abandoned(conn: &Connection, state: BlobState, now: i64) -> Result<usize> {
    let abandoned: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT blob_id FROM main.sync_blobs WHERE state = ?1 AND active = 1")?;
        let rows = stmt
            .query_map([state.as_str()], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    for blob_id in &abandoned {
        note_failed(conn, blob_id, TransferError::Network, now)?;
    }
    Ok(abandoned.len())
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

/// Whether a syncing attachment row names `blob_id` (the rows `reconcile` queues from): a transfer
/// whose row went meanwhile ends as gone.
pub fn is_named(conn: &Connection, blob_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM ({}) n WHERE n.b = ?1)",
            references()?
        ),
        [blob_id],
        |r| r.get(0),
    )?)
}

/// Whether any content waits to move, up or down (an entry that never moves by itself,
/// [`STUCK`], does not wait); with `up_only`, to upload.
pub fn waiting(conn: &Connection, up_only: bool) -> Result<bool> {
    Ok(conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM main.sync_blobs \
               WHERE (state = 'up' OR (state = 'down' AND NOT ?1)) \
                 AND COALESCE(last_error, '') NOT IN {STUCK})"
        ),
        [up_only],
        |r| r.get(0),
    )?)
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
            "UPDATE main.sync_blobs SET state = 'up', queued_at = ?2, \
               size = COALESCE(size, {}), {FRESH} \
             WHERE blob_id = ?1 AND state = 'done' AND {}",
            store_size("sync_blobs.blob_id"),
            held("sync_blobs.blob_id")
        ),
        params![blob_id, now],
    )?;
    Ok(changed > 0)
}

pub(crate) fn table_of(kind: AttachmentKind) -> &'static str {
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

/// "Sync now": every transfer waiting for its backoff is due now, those that waited for room on
/// the server included; never those that do not move by themselves (an invalid ID, a refusal, a
/// conflict: "try again" on their row moves them).
pub fn retry_now(conn: &Connection) -> Result<()> {
    conn.execute(
        &format!(
            "UPDATE main.sync_blobs SET next_try_at = 0 \
             WHERE state IN ('up', 'down') AND COALESCE(last_error, '') NOT IN {STUCK}"
        ),
        [],
    )?;
    Ok(())
}

/// Rows with content held here that the server has not confirmed and that can still upload (a
/// photo once; an entry of [`STUCK`] is a failed item, not waiting).
pub fn uploads_waiting(conn: &Connection) -> Result<u32> {
    let rows: i64 = conn.query_row(
        &format!(
            "SELECT COUNT(*) FROM (SELECT DISTINCT r.tbl, r.rid FROM ({}) r \
               JOIN main.sync_blobs q ON q.blob_id = r.b \
               WHERE q.state = 'up' AND COALESCE(q.last_error, '') NOT IN {STUCK})",
            references()?
        ),
        [],
        |r| r.get(0),
    )?;
    Ok(rows as u32)
}

/// The rows by where their content is (a photo once, whatever its two blobs do), read from the
/// queue alone, never the store (ADR 0030): an entry `up` or `done` is content held here, `down`
/// content not here, as of the last reconcile; content lost from the store outside a merge reads
/// as held until the next full `reconcile`. Among content not here a row counts in its worst
/// state only (failed, then not on the server, then not here); `waiting_upload` counts the rows
/// with any blob waiting to upload, `waiting_upload_bytes` the plaintext size the queue recorded
/// for those blobs; a held blob that never uploads by itself ([`STUCK`]) counts its row as failed
/// instead. Nothing without the store (every row reads as here then).
pub fn summary(conn: &Connection) -> Result<AttachmentSummary> {
    if !files_schema::is_attached(conn)? {
        return Ok(AttachmentSummary::default());
    }
    let refs = references()?;
    let (waiting_upload, failed, not_on_server, not_here): (i64, i64, i64, i64) = conn.query_row(
        &format!(
            "WITH blobs AS (
               SELECT r.tbl, r.rid, q.state, q.active, COALESCE(q.last_error, '') AS error
               FROM ({refs}) r JOIN main.sync_blobs q ON q.blob_id = r.b),
             per_row AS (
               SELECT MAX(state = 'up' AND error NOT IN {STUCK}) AS up,
                 MAX((state = 'down' AND active = 0
                     AND error IN ('unreadable', 'invalidId', 'refused', 'conflict'))
                   OR (state = 'up' AND error IN {STUCK})) AS failed,
                 MAX(state = 'down' AND active = 0 AND error = 'notOnServer') AS gone,
                 MAX(state = 'down') AS down
               FROM blobs
               GROUP BY tbl, rid)
             SELECT COALESCE(SUM(up), 0), COALESCE(SUM(failed), 0),
               COALESCE(SUM(gone AND NOT failed), 0),
               COALESCE(SUM(down AND NOT failed AND NOT gone), 0)
             FROM per_row"
        ),
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let waiting_upload_bytes: i64 = conn.query_row(
        &format!(
            "SELECT COALESCE(SUM(q.size), 0) FROM main.sync_blobs q \
             WHERE q.state = 'up' AND q.blob_id IN (SELECT b FROM ({refs})) \
               AND COALESCE(q.last_error, '') NOT IN {STUCK}"
        ),
        [],
        |r| r.get(0),
    )?;
    Ok(AttachmentSummary {
        waiting_upload: waiting_upload as u32,
        waiting_upload_bytes,
        not_here: not_here as u32,
        not_on_server: not_on_server as u32,
        failed: failed as u32,
    })
}
