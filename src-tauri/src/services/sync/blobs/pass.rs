//! The daily pass (attachments design §5.5, ADR 0030): the only thing that removes blobs from the
//! sync server, and the way a blob the server lost gets back to it.
//!
//! Nothing deletes a server blob at the moment of a delete: the server sees only ciphertext and
//! cannot tell which blobs a row names, and a row can come back (a newer whole row beats a
//! tombstone). So once per 24 h a device that is fully caught up lists the vault's blobs and
//!
//! - deletes those no row on this device names (any attachment row, of the registry's `requires`
//!   columns, and every blob a replace holds) that are older than seven days on the server's clock
//!   (the listing's `now`, never this device's), at most [`PASS_MAX_DELETES`] per pass; and
//! - queues for upload again the content this device holds whose upload the server once confirmed
//!   (a `done` entry) and which the listing no longer has.
//!
//! "Caught up" (`pass_allowed`): the round's pull reached the server's end with nothing stopped or
//! held and the server not past it; nothing waits to push or for its echo; no upload waits that
//! can still happen for other reasons than room (content that never uploads by itself,
//! `queue::STUCK`, does not hold the pass back, nor content waiting for room on the server: the
//! pass may be what frees it); no pause, guard, went-back mark, miss or quarantine. The server must
//! have the blob routes.
//!
//! Every database read or write is one `ConnAccess::with_conn` hold, none open across a network
//! call. The pass ends within [`PASS_BUDGET`] (checked before every request, and every request is
//! cut there). Only a pass that listed every page, put back what was lost and sent every delete it
//! chose is done (`PASS_AT_KEY`); a pass that started and ended otherwise is tried again an hour
//! later ([`PASS_RETRY_AFTER`]), or at once after "Sync now". After a pass that deleted anything,
//! uploads waiting for room are due at once.

use std::collections::HashSet;
use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};
use tokio::time::Instant;
use uuid::Uuid;

use super::canonical_id;
use super::policy::{BlobPolicy, Lane, ServerBlobs};
use super::queue::{self, TransferError, STUCK};
use crate::error::Result;
use crate::services::attachment_store::HOLDS;
use crate::services::sync::capture::guard;
use crate::services::sync::engine::{ConnAccess, PullReport};
use crate::services::sync::keys::Credentials;
use crate::services::sync::registry;
use crate::services::sync::relay::{Relay, RelayError, RemoteBlob, MAX_BLOB_PAGE};
use crate::services::sync::state;

/// How often the pass runs at most (seconds).
pub const PASS_INTERVAL: i64 = 86_400;
/// How old an unreferenced blob must be on the server's clock before the pass deletes it: covers a
/// device that uploaded a blob and has not pushed its row yet.
pub const UNREFERENCED_GRACE: i64 = 7 * 86_400;
/// The most listing pages one pass asks for (each of `MAX_BLOB_PAGE` blobs).
pub const PASS_MAX_PAGES: usize = 100;
/// The most blobs one pass deletes.
pub const PASS_MAX_DELETES: usize = 1_000;
/// The longest one pass runs: with the caps a pass could send about 1,100 requests, and it runs
/// inside a sync round.
pub const PASS_BUDGET: Duration = Duration::from_secs(60);
/// The Local `app_config` key (not in `registry::APP_CONFIG_SYNCED_KEYS`, so never synced and
/// never captured) holding when this device last completed the pass (unix seconds).
pub const PASS_AT_KEY: &str = "sync.blob_pass_at";
/// How long a pass that started and did not complete (the page cap, its budget, a server error)
/// waits before this engine tries again, unless the person asks ("Sync now"): every round would
/// otherwise try, each held for up to [`PASS_BUDGET`]. Kept in memory only (`BlobPolicy`).
pub const PASS_RETRY_AFTER: i64 = 3_600;
/// 2020-01-01: no server built for this app stored a blob before it, so a `createdAt` earlier
/// than this is a server fault and the blob's age unknown: never deleted.
pub const CREATED_AT_FLOOR: i64 = 1_577_836_800;

/// Why the pass did not run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skip {
    /// It completed less than [`PASS_INTERVAL`] ago.
    NotDue,
    /// The round's pull did not reach the server's end with nothing stopped or held.
    Behind,
    /// Changes wait to be pushed (`sync_outbox`) or for their echo (`sync_unsent`).
    Pending,
    /// Content waits to upload (an `up` entry that can still upload).
    UploadsWaiting,
    /// Capture is broken, the guard is on, the vault is gone, or sync is off.
    Paused,
    /// The sync server went back below this device's place.
    WentBack,
    /// This device misses changes: below its barrier, or a skipped one.
    Missed,
    /// An author is quarantined.
    Quarantined,
    /// The sync server has no blob routes.
    NoServerFeature,
    /// The sync server could not be asked whether it has them; the next round asks again.
    ServerUnknown,
    /// A pass started less than [`PASS_RETRY_AFTER`] ago and did not complete.
    TriedRecently,
}

/// What one pass did: server blobs deleted, blobs queued to upload again, why it did not run, and
/// whether it completed (every page listed, every chosen delete sent; `PASS_AT_KEY` written).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct PassReport {
    pub deleted: usize,
    pub requeued: usize,
    pub skipped: Option<Skip>,
    pub complete: bool,
}

/// The pass's bounds; tests shrink them (the end-to-end suite also the grace, since a real
/// server dates its blobs by its own clock).
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    pub page: u32,
    pub max_pages: usize,
    pub max_deletes: usize,
    pub budget: Duration,
    /// How old an unreferenced blob must be on the server's clock before it is deleted.
    pub grace: i64,
}

impl Limits {
    pub const DEFAULT: Limits = Limits {
        page: MAX_BLOB_PAGE,
        max_pages: PASS_MAX_PAGES,
        max_deletes: PASS_MAX_DELETES,
        budget: PASS_BUDGET,
        grace: UNREFERENCED_GRACE,
    };
}

/// Why the pass may not run now on this profile after `pull`, or `None` when it may (the server's
/// feature is asked by `run_if_due`). Reads only.
pub fn pass_allowed(conn: &Connection, pull: &PullReport, now: i64) -> Result<Option<Skip>> {
    if passed_at(conn)?.is_some_and(|at| (0..PASS_INTERVAL).contains(&now.saturating_sub(at))) {
        // A mark further ahead than now was written by a clock since set back: due.
        return Ok(Some(Skip::NotDue));
    }
    if !pull.done() || pull.behind {
        return Ok(Some(Skip::Behind));
    }
    let (Some(sync), Some(marks)) = (state::load(conn)?, state::marks(conn)?) else {
        return Ok(Some(Skip::Paused));
    };
    if !sync.enabled
        || marks.capture_broken_at.is_some()
        || marks.vault_gone_at.is_some()
        || guard::changes_paused(conn)
    {
        return Ok(Some(Skip::Paused));
    }
    if state::vault_went_back_at(conn)?.is_some() {
        return Ok(Some(Skip::WentBack));
    }
    if sync.missed_below_barrier.is_some() || state::skipped_miss(conn)?.is_some() {
        return Ok(Some(Skip::Missed));
    }
    let quarantined: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sync_heads WHERE quarantined <> 0)",
        [],
        |r| r.get(0),
    )?;
    if quarantined {
        return Ok(Some(Skip::Quarantined));
    }
    let pending: bool = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM sync_outbox) OR EXISTS (SELECT 1 FROM sync_unsent)",
        [],
        |r| r.get(0),
    )?;
    if pending {
        return Ok(Some(Skip::Pending));
    }
    // Content waiting for room does not hold the pass back: deleting is what may free it, and the
    // pass never deletes a blob a row here names nor moves an `up` entry.
    let uploads_waiting: bool = conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM main.sync_blobs WHERE state = 'up' \
               AND COALESCE(last_error, '') NOT IN {STUCK} AND COALESCE(last_error, '') <> ?1)"
        ),
        [TransferError::NoRoom.as_str()],
        |r| r.get(0),
    )?;
    if uploads_waiting {
        return Ok(Some(Skip::UploadsWaiting));
    }
    Ok(None)
}

/// When this device last completed the pass, if it did.
pub(crate) fn passed_at(conn: &Connection) -> Result<Option<i64>> {
    let value: Option<String> = conn
        .query_row(
            "SELECT value FROM app_config WHERE key = ?1",
            [PASS_AT_KEY],
            |r| r.get(0),
        )
        .optional()?;
    // A value that does not parse counts as never: the pass runs and writes it again.
    Ok(value.and_then(|v| v.parse().ok()))
}

/// The pass completed at `at`.
pub(crate) fn note_passed(conn: &Connection, at: i64) -> Result<()> {
    conn.execute(
        "INSERT INTO app_config (key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![PASS_AT_KEY, at.to_string()],
    )?;
    Ok(())
}

/// SQL: every blob ID (lower case) a row on this device names, whether or not the row syncs: each
/// column of every table's `TableSpec.requires` (so a new attachment table cannot be forgotten),
/// and every blob a replace holds (`attachment_store::HOLDS`), whose row a pull may still bring.
fn references_sql() -> String {
    registry::tables()
        .iter()
        .flat_map(|spec| spec.requires.iter().map(move |column| (spec.name, *column)))
        .chain(HOLDS.iter().copied())
        .map(|(table, column)| {
            format!(
                "SELECT lower(\"{column}\") AS b FROM main.\"{table}\" WHERE \"{column}\" IS NOT NULL"
            )
        })
        .collect::<Vec<_>>()
        .join(" UNION ")
}

/// Every blob ID a row on this device names or a replace holds, lower case.
fn references(conn: &Connection) -> Result<HashSet<String>> {
    let mut stmt = conn.prepare(&references_sql())?;
    let ids = stmt
        .query_map([], |r| r.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(ids)
}

/// Whether a row on this device names `blob` or a replace holds it, right now: the check just
/// before its delete, since rows can come (an attachment, a restore, a merge) while the pass runs.
pub(crate) fn is_referenced(conn: &Connection, blob: &Uuid) -> Result<bool> {
    Ok(conn.query_row(
        &format!(
            "SELECT EXISTS (SELECT 1 FROM ({}) WHERE b = ?1)",
            references_sql()
        ),
        [blob.hyphenated().to_string()],
        |r| r.get(0),
    )?)
}

/// Queues for upload again every blob this device holds whose upload the server once confirmed
/// (`done`) and which `listed` does not have. A `done` entry never has an invalid ID or a stuck
/// failure (`queue::note_done` clears them), and only `done` entries move: stuck ones wait for
/// "try again". How many moved.
fn requeue_lost(conn: &Connection, listed: &HashSet<Uuid>, now: i64) -> Result<usize> {
    let done: Vec<String> = {
        let mut stmt = conn.prepare("SELECT blob_id FROM main.sync_blobs WHERE state = 'done'")?;
        let rows = stmt
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<_, _>>()?;
        rows
    };
    let mut requeued = 0;
    for blob in done {
        let Some(id) = canonical_id(&blob) else {
            continue;
        };
        if !listed.contains(&id) && queue::requeue_upload(conn, &blob, now)? {
            log::debug!("[SYNC] The sync server lost blob {blob}: it is uploaded again");
            requeued += 1;
        }
    }
    Ok(requeued)
}

/// The age of `blob` on the server's clock, past `grace` ([`UNREFERENCED_GRACE`] outside tests),
/// by `server_now`. A blob dated in the server's future, or before [`CREATED_AT_FLOOR`], is never
/// old enough.
fn old_enough(blob: &RemoteBlob, server_now: i64, grace: i64) -> bool {
    blob.created_at >= CREATED_AT_FLOOR && server_now.saturating_sub(blob.created_at) > grace
}

/// How a request of the pass went.
enum Answer<T> {
    Ok(T),
    /// The pass's time ran out before or during it.
    OutOfTime,
    Failed(RelayError),
}

/// The request `send` makes, unless the pass's time is up (checked before the request is made);
/// cut at the deadline.
async fn within<T, F>(deadline: Instant, send: impl FnOnce() -> F) -> Answer<T>
where
    F: std::future::Future<Output = std::result::Result<T, RelayError>>,
{
    if Instant::now() >= deadline {
        return Answer::OutOfTime;
    }
    match tokio::time::timeout_at(deadline, send()).await {
        Ok(Ok(value)) => Answer::Ok(value),
        Ok(Err(error)) => Answer::Failed(error),
        Err(_) => Answer::OutOfTime,
    }
}

/// Logs why the pass stopped on a sync server answer. A refusal of this vault (payment, token,
/// vault gone) is the round's to show and logs at `debug` only, as a transfer does; other trouble
/// once at `info`.
fn log_stop(error: &RelayError) {
    match error {
        RelayError::PaymentRequired
        | RelayError::Unauthorized
        | RelayError::TokenReadOnly
        | RelayError::VaultNotFound => {
            log::debug!("[SYNC] The daily pass of documents and photos stopped: {error}");
        }
        _ => {
            log::info!(
                "[SYNC] The daily pass of documents and photos stopped; the next round tries again"
            );
            log::debug!("[SYNC] The daily pass of documents and photos stopped: {error}");
        }
    }
}

fn log_out_of_time() {
    log::info!(
        "[SYNC] The daily pass of documents and photos ran out of time; the next round goes on"
    );
}

/// Runs the pass if it is due and allowed (`pass_allowed`, then the server's blob routes through
/// the policy's cached answer); see the module. A sync server error never is an `Err`: the pass
/// stops and the next round tries again. Only a local error is (a locked profile as
/// `AppError::Auth`).
pub async fn run_if_due<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    pull: &PullReport,
    now: i64,
) -> Result<PassReport> {
    run_with(relay, db, creds, policy, pull, now, Limits::DEFAULT).await
}

/// `run_if_due` within `limits`.
pub(crate) async fn run_with<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    pull: &PullReport,
    now: i64,
    limits: Limits,
) -> Result<PassReport> {
    if let Some(skip) = db.with_conn(|conn| pass_allowed(conn, pull, now))? {
        return Ok(skipped(skip));
    }
    if policy.pass_tried_within(now, PASS_RETRY_AFTER) {
        return Ok(skipped(Skip::TriedRecently));
    }
    let attempt = attempt(relay, db, creds, policy, now, limits).await;
    let report = match &attempt {
        Ok(report) => *report,
        Err(_) => PassReport::default(),
    };
    // A server without the blob routes, or one that could not be asked, is no attempt: asking
    // again costs one cached answer or one request.
    let feature_skip = matches!(
        report.skipped,
        Some(Skip::NoServerFeature | Skip::ServerUnknown)
    );
    if !report.complete && !feature_skip {
        policy.note_pass_incomplete(now);
    }
    if report.deleted > 0 {
        // Room may be free now: uploads that waited for it go at once, not after their wait.
        db.with_conn(queue::room_freed)?;
        policy.room_may_be_free();
        policy.wake_lane(Lane::Up);
    }
    attempt
}

fn skipped(skip: Skip) -> PassReport {
    PassReport {
        skipped: Some(skip),
        ..PassReport::default()
    }
}

/// One pass past `pass_allowed`: the server's feature, the listing, the requeue, the deletes.
async fn attempt<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &BlobPolicy,
    now: i64,
    limits: Limits,
) -> Result<PassReport> {
    let mut report = PassReport::default();
    // The deadline comes first: asking the server for its features is part of the budget.
    let deadline = Instant::now() + limits.budget;
    let asked = within(deadline, || async move {
        Ok::<_, RelayError>(policy.server_has_blobs(relay, now).await)
    })
    .await;
    // The upload lane's way of asking, without its problem: nothing waits to upload here.
    match asked {
        Answer::Ok(ServerBlobs::Yes) => {}
        Answer::Ok(ServerBlobs::No) => {
            report.skipped = Some(Skip::NoServerFeature);
            return Ok(report);
        }
        Answer::Ok(ServerBlobs::Unknown) | Answer::Failed(_) => {
            report.skipped = Some(Skip::ServerUnknown);
            return Ok(report);
        }
        Answer::OutOfTime => {
            log_out_of_time();
            return Ok(report);
        }
    }
    let auth = creds.auth();

    // 1. List every page. A blob's age is measured against the smallest `now` of the pages (the
    //    most conservative of the server's own answers); one page without it deletes nothing.
    let mut listed: Vec<RemoteBlob> = Vec::new();
    let mut server_now: Option<i64> = None;
    let mut clock_missing = false;
    let mut after = None;
    let mut every_page = false;
    for _ in 0..limits.max_pages {
        let page = match within(deadline, || relay.list_blobs(&auth, after, limits.page)).await {
            Answer::Ok(page) => page,
            Answer::OutOfTime => {
                log_out_of_time();
                return Ok(report);
            }
            // A part of the listing deletes and puts back nothing: a blob it did not reach is not
            // lost.
            Answer::Failed(error) => {
                log_stop(&error);
                return Ok(report);
            }
        };
        match page.server_now {
            Some(at) => server_now = Some(server_now.map_or(at, |min: i64| min.min(at))),
            None => clock_missing = true,
        }
        after = page.blobs.last().map(|blob| blob.id).or(after);
        listed.extend(page.blobs);
        if !page.more {
            every_page = true;
            break;
        }
    }
    let server_now = if clock_missing { None } else { server_now };
    if server_now.is_none() {
        log::info!("[SYNC] The sync server did not say its time: the daily pass deletes nothing");
    }

    // 2. Put back what the server lost, only after a listing of every page.
    if every_page {
        let ids: HashSet<Uuid> = listed.iter().map(|blob| blob.id).collect();
        report.requeued = db.with_conn(|conn| requeue_lost(conn, &ids, now))?;
        if report.requeued > 0 {
            policy.wake_lane(Lane::Up);
        }
    } else {
        // The page cap: the blobs seen may still be deleted (each delete is safe on its own: no
        // row here names the blob, it is past the grace on the server's clock, and it is checked
        // again just before), but nothing is put back and the pass is not done.
        log::info!(
            "[SYNC] The vault has more blobs than one daily pass lists; the next round goes on"
        );
    }

    // 3. Delete what no row here names and is past the grace on the server's clock.
    if let Some(server_now) = server_now {
        let candidates: Vec<Uuid> = {
            let named = db.with_conn(references)?;
            listed
                .iter()
                .filter(|blob| old_enough(blob, server_now, limits.grace))
                .filter(|blob| !named.contains(&blob.id.hyphenated().to_string()))
                .map(|blob| blob.id)
                .take(limits.max_deletes)
                .collect()
        };
        for blob in candidates {
            if Instant::now() >= deadline {
                log_out_of_time();
                return Ok(report);
            }
            // A row may have come since the listing: checked again, alone in its hold.
            if db.with_conn(|conn| is_referenced(conn, &blob))? {
                log::debug!("[SYNC] Blob {blob} is named again: not deleted from the sync server");
                continue;
            }
            match within(deadline, || relay.delete_blob(&auth, blob)).await {
                Answer::Ok(()) => {}
                Answer::OutOfTime => {
                    log_out_of_time();
                    return Ok(report);
                }
                Answer::Failed(error) => {
                    log_stop(&error);
                    return Ok(report);
                }
            }
            log::debug!("[SYNC] Blob {blob} deleted from the sync server");
            report.deleted += 1;
            // No entry may say the server has it. (An unreferenced blob normally has none: the
            // round reconciles before it pushes. A row deleted since leaves its entry until the
            // next reconcile, which re-adds the blob if a row names it again.)
            db.with_conn(|conn| queue::note_gone(conn, &blob.hyphenated().to_string()))?;
        }
    }
    if !every_page {
        return Ok(report);
    }
    db.with_conn(|conn| note_passed(conn, now))?;
    report.complete = true;
    log::info!(
        "[SYNC] Daily pass of documents and photos: {} deleted from the sync server, {} to upload again",
        report.deleted,
        report.requeued
    );
    Ok(report)
}
