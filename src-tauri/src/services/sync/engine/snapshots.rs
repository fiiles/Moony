//! Snapshots in the engine (spec §8; rulings P4, P6, P14, T8-2, T8-6, T8-7): the sealed dump and
//! its upload policy, opening a downloaded snapshot, the restore by merge after
//! `410 snapshot_required`, and the restore by merge that heals a below-barrier miss.
//!
//! A snapshot travels as the phase 1 record stream (`snapshot::dump`), raw-deflated, then sealed as
//! one STREAM object whose object ID is its relay sequence `S` (`envelope::stream`). Both sides
//! are bounded: the dump stops writing once the record stream passes `MAX_SNAPSHOT_PLAIN` or what
//! goes into the sealer passes the upload limit, and a download over `MAX_SNAPSHOT_BYTES` is
//! refused before it is opened, its inflated stream again capped at `MAX_SNAPSHOT_PLAIN`.
//!
//! The dump holds the session mutex for its whole run (exclusive use, phase 1) inside one read
//! transaction, off the async runtime (`spawn_blocking`); the upload and the download never hold
//! it. `S` is the cursor this device has applied, read in the dump's transaction: the caller dumps
//! only after a pull that ended done (`PullReport::may_snapshot`), and the dump itself refuses a
//! profile with anything in `sync_outbox` or `sync_unsent`.

use std::io::{self, Write};

use flate2::write::DeflateEncoder;
use flate2::{Compression, Decompress, FlushDecompress, Status};
use rusqlite::Connection;
use zeroize::Zeroizing;

use super::ConnAccess;
use crate::error::{AppError, Result};
use crate::services::sync::chain;
use crate::services::sync::envelope::stream::{open_stream, snapshot_object_id, StreamSealer};
use crate::services::sync::error::{SyncError, SyncResult, TooLarge};
use crate::services::sync::keys::Credentials;
use crate::services::sync::relay::{Relay, RelayError, SnapshotBlob, VaultInfo};
use crate::services::sync::snapshot::{self, read_record, DumpStats, Record, RestoreReport};
use crate::services::sync::state::{self, EngineMarks, SyncState};

/// Envelopes since the vault's newest snapshot that make an upload due.
pub const SNAPSHOT_ENVELOPES: u64 = 2_000;
/// Bytes pulled since the vault's newest snapshot that make an upload due.
pub const SNAPSHOT_BYTES: u64 = 8 * 1024 * 1024;
/// A device uploads at most once a day.
pub const SNAPSHOT_MIN_INTERVAL: i64 = 86_400;
/// The longest random delay before a due upload (seconds).
pub const SNAPSHOT_MAX_DELAY: i64 = 3_600;
/// The policy asks for the vault's info at most this often (seconds), unless a schedule falls due.
pub const POLICY_CHECK_INTERVAL: i64 = 3_600;
/// The relay's per-request limit, and the largest download this device opens.
pub const MAX_SNAPSHOT_BYTES: usize = 104_857_600;
/// The largest record stream a dump writes and a download inflates to.
pub const MAX_SNAPSHOT_PLAIN: usize = 512 * 1024 * 1024;

/// The fixed `last_error` of a restore that stopped; the reason is logged only at `debug` (it may
/// name a row).
const RESTORE_STOPPED: &str = "snapshot restore stopped";

/// The key of a snapshot written by a newer schema than this build knows.
const NEWER_SCHEMA: &str = "validation.syncSnapshotNewerSchema";

/// A dump, sealed for relay sequence `seq`.
pub struct SealedSnapshot {
    pub seq: u64,
    pub bytes: Vec<u8>,
    pub stats: DumpStats,
}

impl std::fmt::Debug for SealedSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SealedSnapshot")
            .field("seq", &self.seq)
            .field("bytes", &self.bytes.len())
            .field("stats", &self.stats)
            .finish()
    }
}

/// Dumps and seals this profile at `S` = its cursor, in one read transaction (`BEGIN DEFERRED …
/// COMMIT`): `snapshot::dump` writes through raw deflate into a `StreamSealer`. Call with
/// exclusive use of the connection. A dump over `MAX_SNAPSHOT_BYTES` sealed is
/// `validation.syncSnapshotTooLarge`; a profile with anything waiting to go out is
/// `validation.syncSnapshotPending` (`snapshot::dump`).
pub fn dump_sealed(conn: &Connection, creds: &Credentials) -> Result<SealedSnapshot> {
    dump_sealed_capped(conn, creds, MAX_SNAPSHOT_BYTES, MAX_SNAPSHOT_PLAIN)?
        .ok_or_else(|| AppError::Validation("validation.syncSnapshotTooLarge".into()))
}

/// `dump_sealed` with its two bounds: `None` once the record stream passes `max_plain` bytes or
/// the sealed stream passes `max_bytes`. Writing stops at the first byte over either bound, so a
/// dump never holds more than about `max_bytes` sealed.
pub(crate) fn dump_sealed_capped(
    conn: &Connection,
    creds: &Credentials,
    max_bytes: usize,
    max_plain: usize,
) -> Result<Option<SealedSnapshot>> {
    let tx = conn.unchecked_transaction()?;
    let seq = loaded(&tx)?.cursor;
    let sealer = StreamSealer::new(
        creds.keys.current_epoch(),
        creds.keys.current(),
        &creds.vault_id.0,
        &snapshot_object_id(seq),
    );
    // What enters the sealer is never more than what leaves it.
    let deflate = DeflateEncoder::new(Capped::new(sealer, max_bytes), Compression::default());
    let mut plain = Capped::new(deflate, max_plain);
    let over = |plain: &Capped<DeflateEncoder<Capped<StreamSealer>>>| {
        plain.over || plain.inner.get_ref().over
    };
    let stats = match snapshot::dump(&tx, seq, &mut plain) {
        Ok(stats) => stats,
        Err(_) if over(&plain) => return Ok(None),
        Err(error) => return Err(error),
    };
    if let Err(error) = plain.inner.try_finish() {
        return if over(&plain) {
            Ok(None)
        } else {
            Err(deflate_error(error))
        };
    }
    let sealer = plain.inner.finish().map_err(deflate_error)?.inner;
    let bytes = sealer.finish()?;
    tx.commit()?;
    if bytes.len() > max_bytes {
        return Ok(None);
    }
    Ok(Some(SealedSnapshot { seq, bytes, stats }))
}

/// Opens a downloaded snapshot to its record stream: refused over `MAX_SNAPSHOT_BYTES` before it
/// is opened, opened as the STREAM object of `blob.seq` (a snapshot served under another sequence
/// does not authenticate), inflated up to `MAX_SNAPSHOT_PLAIN`, and its header must name the same
/// sequence. Too large → `TooLarge(Body)`; anything else wrong → `validation.syncSnapshotInvalid`.
///
/// The decrypted bytes are wiped when dropped, the deflated ones here and the record stream by the
/// caller (`Zeroizing`). What a restore parses out of the stream (`snapshot::restore_merge`) is
/// not: it ends up in the profile anyway.
pub fn open_sealed(creds: &Credentials, blob: &SnapshotBlob) -> SyncResult<Zeroizing<Vec<u8>>> {
    open_sealed_capped(creds, blob, MAX_SNAPSHOT_PLAIN)
}

/// `open_sealed` with the inflated stream capped at `max_plain` bytes.
pub(crate) fn open_sealed_capped(
    creds: &Credentials,
    blob: &SnapshotBlob,
    max_plain: usize,
) -> SyncResult<Zeroizing<Vec<u8>>> {
    let deflated = decrypt(creds, blob)?;
    inflated(&deflated, blob.seq, max_plain)
}

/// `open_sealed` taking the download by value: its sealed bytes are dropped as soon as they are
/// decrypted, before the inflate, so a 100 MiB download is not held next to its record stream.
fn open_owned(creds: &Credentials, blob: SnapshotBlob) -> SyncResult<Zeroizing<Vec<u8>>> {
    let seq = blob.seq;
    let deflated = decrypt(creds, &blob)?;
    drop(blob);
    inflated(&deflated, seq, MAX_SNAPSHOT_PLAIN)
}

/// The size check and `open_stream`: the raw deflate stream.
fn decrypt(creds: &Credentials, blob: &SnapshotBlob) -> SyncResult<Zeroizing<Vec<u8>>> {
    if blob.bytes.len() > MAX_SNAPSHOT_BYTES {
        log::warn!(
            "[SYNC] The vault's snapshot is {} bytes, over the limit",
            blob.bytes.len()
        );
        return Err(SyncError::TooLarge(TooLarge::Body));
    }
    let deflated = open_stream(
        |epoch| creds.keys.get(epoch),
        &creds.vault_id.0,
        &snapshot_object_id(blob.seq),
        &blob.bytes,
    )
    .map_err(|error| {
        log::warn!("[SYNC] The vault's snapshot does not open: {error}");
        invalid_snapshot()
    })?;
    Ok(Zeroizing::new(deflated))
}

/// The inflate (capped at `max_plain`) and the header's sequence check against `seq`.
fn inflated(deflated: &[u8], seq: u64, max_plain: usize) -> SyncResult<Zeroizing<Vec<u8>>> {
    let plain = inflate_capped(deflated, max_plain)?;
    if header_of(&plain)?.seq != seq {
        log::warn!("[SYNC] The vault's snapshot names another sequence than it is stored under");
        return Err(invalid_snapshot());
    }
    Ok(plain)
}

/// Inflates a raw deflate stream into a buffer that is wiped when dropped, `TooLarge(Body)` past
/// `max_plain` bytes. The buffer grows by copying into a new one and wiping the old (a `Vec`'s own
/// reallocation would leave copies behind); a stream that does not inflate, or ends before its
/// final block, is `validation.syncSnapshotInvalid`.
pub(crate) fn inflate_capped(deflated: &[u8], max_plain: usize) -> SyncResult<Zeroizing<Vec<u8>>> {
    // One byte over the cap tells "at the cap" from "over it".
    let limit = max_plain.saturating_add(1);
    let mut inflater = Decompress::new(false);
    let mut plain = Zeroizing::new(Vec::with_capacity(
        deflated.len().saturating_mul(4).clamp(1, limit),
    ));
    loop {
        if plain.len() == plain.capacity() {
            if plain.len() >= limit {
                break;
            }
            let mut grown = Zeroizing::new(Vec::with_capacity(
                plain.capacity().saturating_mul(2).min(limit),
            ));
            grown.extend_from_slice(&plain);
            plain = grown;
        }
        let consumed = usize::try_from(inflater.total_in()).map_err(|_| invalid_snapshot())?;
        let written = inflater.total_out();
        let status = inflater
            .decompress_vec(&deflated[consumed..], &mut plain, FlushDecompress::None)
            .map_err(|_| {
                log::warn!("[SYNC] The vault's snapshot does not inflate");
                invalid_snapshot()
            })?;
        if status == Status::StreamEnd {
            break;
        }
        let stalled = inflater.total_in() as usize == consumed && inflater.total_out() == written;
        if stalled && plain.len() < plain.capacity() {
            // Input exhausted with room to write: the stream ends without its final block.
            log::warn!("[SYNC] The vault's snapshot is cut short");
            return Err(invalid_snapshot());
        }
    }
    if plain.len() > max_plain {
        log::warn!("[SYNC] The vault's snapshot inflates past the limit");
        return Err(SyncError::TooLarge(TooLarge::Body));
    }
    Ok(plain)
}

/// Restores an opened record stream by merge (`snapshot::restore_merge`): rows by their clocks,
/// the cursor `MAX(cursor, S)`, the outbox kept, the below-barrier miss healed or recorded.
pub fn restore_sealed(conn: &Connection, plain: &[u8], now: i64) -> Result<RestoreReport> {
    snapshot::restore_merge(conn, &mut &plain[..], now)
}

/// When an upload is due.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    No,
    Now,
    /// Due, after the random delay: at this time.
    At(i64),
}

/// The upload policy's memory, per engine and in memory only (a restart starts it over). A
/// snapshot found too large is remembered in `sync_state` instead (`snapshot_too_large_at`), so a
/// restart does not dump it again.
#[derive(Debug, Clone, Default)]
pub struct SnapshotPolicy {
    /// When `decide` last saw the vault's info.
    pub checked_at: Option<i64>,
    /// When a due upload goes (after the random delay).
    pub scheduled_at: Option<i64>,
    /// The newest snapshot a heal fetched that did not heal (or did not restore): not fetched
    /// again until the vault has a newer one.
    pub examined_seq: Option<u64>,
}

impl SnapshotPolicy {
    /// Whether the caller should fetch the vault's info for `decide`: never checked, checked
    /// `POLICY_CHECK_INTERVAL` ago or longer, or a schedule fell due.
    pub fn wants_info(&self, now: i64) -> bool {
        self.checked_at
            .is_none_or(|at| now - at >= POLICY_CHECK_INTERVAL)
            || self.scheduled_at.is_some_and(|at| now >= at)
    }

    /// Whether an upload is due (spec §8). A vault with no snapshot needs one now. Otherwise one is
    /// due once `SNAPSHOT_ENVELOPES` envelopes or `SNAPSHOT_BYTES` pulled bytes have piled up since
    /// the vault's newest snapshot, and this device uploaded none in the last
    /// `SNAPSHOT_MIN_INTERVAL`; it is then scheduled `jitter` seconds later (`0..=
    /// SNAPSHOT_MAX_DELAY`, drawn by the caller) and returned as `At` until that time. A schedule
    /// stays put until it is used or the upload is no longer due.
    pub fn decide(&mut self, info: &VaultInfo, marks: &EngineMarks, now: i64, jitter: i64) -> Due {
        self.checked_at = Some(now);
        let due = match info.snapshot_seq {
            None => {
                self.scheduled_at = None;
                return Due::Now;
            }
            Some(seq) => {
                let piled_up = info.head.saturating_sub(seq) >= SNAPSHOT_ENVELOPES
                    || marks.pulled_bytes >= SNAPSHOT_BYTES;
                let rested = marks
                    .snapshot_uploaded_at
                    .is_none_or(|at| now - at >= SNAPSHOT_MIN_INTERVAL);
                piled_up && rested
            }
        };
        if !due {
            self.scheduled_at = None;
            return Due::No;
        }
        let at = *self
            .scheduled_at
            .get_or_insert(now + jitter.clamp(0, SNAPSHOT_MAX_DELAY));
        if now >= at {
            Due::Now
        } else {
            Due::At(at)
        }
    }
}

/// Restores after `410 snapshot_required` (§8): fetches the vault's newest snapshot, opens it off
/// the runtime without the lock, then restores it by merge under the lock (the cursor becomes
/// `MAX(cursor, S)`, the outbox stays and is pushed afterwards). A restore that stops (a merge
/// stop, a snapshot below this device's barrier or from a newer schema) writes
/// `"snapshot restore stopped"` to `last_error` and returns `validation.syncSnapshotNewerSchema`
/// for a newer schema (the person updates the app), `validation.syncSnapshotInvalid` otherwise. A
/// local failure (`AppError::Database`, `Internal`, …) is returned as it is, noted nowhere, so the
/// round backs off. A snapshot that does not open writes nothing.
pub async fn restore_after_410<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    now: i64,
) -> SyncResult<RestoreReport> {
    let blob = relay.get_snapshot(&creds.auth()).await?;
    let plain = open_off_thread(creds, blob).await?;
    let db = db.clone();
    match off_thread(move || db.with_conn(|conn| restore_or_note(conn, &plain, now))).await? {
        Restore::Done(report) => Ok(report),
        Restore::Stopped(key) => Err(stopped(key)),
    }
}

/// Heals a below-barrier miss (rulings T8-2, T8-6): when this device has `missed_below_barrier`
/// set and the vault's newest snapshot (by `info`) is at or past it, fetches and opens that
/// snapshot and, when it heals the miss (`chain::snapshot_heals_missed` on its header: a clean
/// dumper, at this device's barrier or later), restores it by merge, which clears the mark. A
/// snapshot that does not heal is not restored, and not fetched again
/// (`SnapshotPolicy::examined_seq`) until the vault has a newer one; neither is a snapshot whose
/// restore stopped or that does not open (both are verdicts on its bytes). A transient or local
/// failure leaves `examined_seq` alone, so the next round tries again. A restore that stops is
/// noted and returned as `restore_after_410` does. `None` when nothing was restored.
pub async fn heal_missed<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &mut SnapshotPolicy,
    info: &VaultInfo,
    now: i64,
) -> SyncResult<Option<RestoreReport>> {
    let Some(newest) = info.snapshot_seq else {
        return Ok(None);
    };
    let Some(missed) = db.with_conn(|conn| Ok(loaded(conn)?.missed_below_barrier))? else {
        return Ok(None);
    };
    if newest < missed || policy.examined_seq == Some(newest) {
        return Ok(None);
    }
    let blob = relay.get_snapshot(&creds.auth()).await?;
    let seq = blob.seq;
    let plain = match open_off_thread(creds, blob).await {
        Ok(plain) => plain,
        Err(error) => {
            if is_verdict(&error) {
                policy.examined_seq = Some(seq);
            }
            return Err(error);
        }
    };
    let header = header_of(&plain)?;
    let db = db.clone();
    let outcome = off_thread(move || {
        db.with_conn(|conn| {
            let sync = loaded(conn)?;
            if !chain::snapshot_heals_missed(&sync, header.barrier, header.seq, header.missed) {
                return Ok(Heal::NotHealing);
            }
            Ok(match restore_or_note(conn, &plain, now)? {
                Restore::Done(report) => Heal::Restored(report),
                Restore::Stopped(key) => Heal::Stopped(key),
            })
        })
    })
    .await?;
    match outcome {
        Heal::Restored(report) => {
            log::info!("[SYNC] A snapshot restored what this device skipped below its barrier");
            Ok(Some(report))
        }
        Heal::NotHealing => {
            log::info!("[SYNC] The vault's newest snapshot does not heal this device's miss");
            policy.examined_seq = Some(seq);
            Ok(None)
        }
        Heal::Stopped(key) => {
            policy.examined_seq = Some(seq);
            Err(stopped(key))
        }
    }
}

/// `maybe_upload_with` under the relay's limit, `MAX_SNAPSHOT_BYTES`.
#[allow(clippy::too_many_arguments)]
pub async fn maybe_upload<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &mut SnapshotPolicy,
    info: &VaultInfo,
    now: i64,
    jitter: i64,
    force: bool,
) -> SyncResult<bool> {
    maybe_upload_with(
        relay,
        db,
        creds,
        policy,
        info,
        now,
        jitter,
        force,
        MAX_SNAPSHOT_BYTES,
    )
    .await
}

/// Uploads a snapshot when one is due. The caller (`run_round`) fetches `info` and calls this only
/// after a pull for which `PullReport::may_snapshot` holds (rulings P4, P14). `true` when a
/// snapshot was uploaded.
///
/// The pulled bytes are counted from the vault's newest snapshot (`rebase_pulled_bytes`). Nothing
/// is uploaded while `sync_outbox` or `sync_unsent` holds anything, by a device with a pending
/// below-barrier miss (`chain::may_upload_snapshot`, ruling T8-6: the forced upload included), or
/// within a day of a snapshot found too large (`snapshot_too_large_at`). With `force` (the first clean round after
/// crossing a barrier, ruling T8-2) the upload is due now; otherwise `SnapshotPolicy::decide`
/// says when.
///
/// The dump runs off the runtime holding the session mutex for its whole run, never across the
/// upload, and checks the outbox, `sync_unsent` and the miss again under that lock. A sealed dump
/// over `max_bytes`, or one the relay finds too large, is `TooLarge(Snapshot)` and is not tried
/// again for a day. A snapshot the relay already has a newer one of (`409 snapshot_outdated`)
/// drops the schedule quietly. Success records the upload (`record_snapshot_uploaded`).
#[allow(clippy::too_many_arguments)]
pub async fn maybe_upload_with<R: Relay, D: ConnAccess>(
    relay: &R,
    db: &D,
    creds: &Credentials,
    policy: &mut SnapshotPolicy,
    info: &VaultInfo,
    now: i64,
    jitter: i64,
    force: bool,
    max_bytes: usize,
) -> SyncResult<bool> {
    let marks = db.with_conn(|conn| {
        state::rebase_pulled_bytes(conn, info.snapshot_seq.unwrap_or(0))?;
        if !ready_to_dump(conn)? {
            return Ok(None);
        }
        state::marks(conn)?.map(Some).ok_or_else(not_set_up)
    })?;
    let Some(marks) = marks else {
        return Ok(false);
    };
    if marks
        .snapshot_too_large_at
        .is_some_and(|at| now - at < SNAPSHOT_MIN_INTERVAL)
    {
        return Ok(false);
    }
    if !force && policy.decide(info, &marks, now, jitter) != Due::Now {
        return Ok(false);
    }

    let (dumper, dump_creds) = (db.clone(), creds.clone());
    let dumped = off_thread(move || {
        dumper.with_conn(|conn| {
            if !ready_to_dump(conn)? {
                return Ok(Dumped::NotReady);
            }
            Ok(
                match dump_sealed_capped(conn, &dump_creds, max_bytes, MAX_SNAPSHOT_PLAIN)? {
                    Some(sealed) => Dumped::Sealed(sealed),
                    None => Dumped::TooLarge,
                },
            )
        })
    })
    .await?;
    let sealed = match dumped {
        Dumped::NotReady => return Ok(false),
        Dumped::TooLarge => {
            log::warn!("[SYNC] This profile's snapshot is over the upload limit");
            return Err(too_large(db, policy, now));
        }
        Dumped::Sealed(sealed) => sealed,
    };

    let (seq, stats) = (sealed.seq, sealed.stats);
    match relay.put_snapshot(&creds.auth(), seq, sealed.bytes).await {
        Ok(()) => {}
        Err(RelayError::SnapshotOutdated) => {
            log::info!("[SYNC] The vault already has a newer snapshot");
            policy.scheduled_at = None;
            return Ok(false);
        }
        Err(RelayError::SnapshotTooLarge) => {
            log::warn!("[SYNC] The relay refused this profile's snapshot as too large");
            return Err(too_large(db, policy, now));
        }
        // Any other answer keeps the schedule, so the next round dumps and uploads again. That
        // is bounded by the round's back-off on the transient classes (`SyncError::backs_off`)
        // and by the round stopping on the others; it never loops within a round.
        Err(error) => return Err(error.into()),
    }
    db.with_conn(|conn| state::record_snapshot_uploaded(conn, seq, stats.max_hlc, now))?;
    policy.scheduled_at = None;
    log::info!("[SYNC] Uploaded a snapshot at relay sequence {seq}");
    Ok(true)
}

/// The snapshot is too large: noted in `sync_state` (no upload for a day, across restarts) and the
/// schedule dropped. A failure to note it is returned instead.
fn too_large<D: ConnAccess>(db: &D, policy: &mut SnapshotPolicy, now: i64) -> SyncError {
    policy.scheduled_at = None;
    match db.with_conn(|conn| state::record_snapshot_too_large(conn, now)) {
        Ok(()) => SyncError::TooLarge(TooLarge::Snapshot),
        Err(error) => error.into(),
    }
}

enum Dumped {
    NotReady,
    TooLarge,
    Sealed(SealedSnapshot),
}

enum Heal {
    NotHealing,
    Restored(RestoreReport),
    /// The restore stopped with this validation key (`restore_or_note`).
    Stopped(String),
}

/// What `restore_or_note` did.
enum Restore {
    Done(RestoreReport),
    /// Stopped on the snapshot: the `validation.*` key the restore answered.
    Stopped(String),
}

/// Nothing waits to go out and no below-barrier miss is pending.
fn ready_to_dump(conn: &Connection) -> Result<bool> {
    let pending: i64 = conn.query_row(
        "SELECT (SELECT COUNT(*) FROM sync_outbox) + (SELECT COUNT(*) FROM sync_unsent)",
        [],
        |r| r.get(0),
    )?;
    Ok(pending == 0 && chain::may_upload_snapshot(&loaded(conn)?))
}

/// Restores `plain` by merge. A restore that stops on the snapshot (a validation answer: a merge
/// stop, a stream that does not parse, a snapshot below the barrier or from a newer schema) is
/// logged at `debug`, noted with the fixed text in `last_error` and returned as `Stopped`; any
/// other error (the database, an internal failure) is returned as it is.
fn restore_or_note(conn: &Connection, plain: &[u8], now: i64) -> Result<Restore> {
    match restore_sealed(conn, plain, now) {
        Ok(report) => Ok(Restore::Done(report)),
        Err(AppError::Validation(key)) => {
            log::debug!("[SYNC] A snapshot restore stopped: {key}");
            log::warn!("[SYNC] A snapshot restore stopped");
            state::record_error(conn, RESTORE_STOPPED)?;
            Ok(Restore::Stopped(key))
        }
        Err(error) => Err(error),
    }
}

/// The error a stopped restore returns: a newer schema says so (the person updates the app);
/// every other stop is one key, a damaged snapshot (phase 1 note L142).
fn stopped(key: String) -> SyncError {
    if key == NEWER_SCHEMA {
        SyncError::Local(AppError::Validation(key))
    } else {
        invalid_snapshot()
    }
}

/// Whether an open failure is a verdict on the snapshot's bytes (they do not open, or are too
/// large), not a passing failure.
fn is_verdict(error: &SyncError) -> bool {
    matches!(
        error,
        SyncError::Local(AppError::Validation(_)) | SyncError::TooLarge(TooLarge::Body)
    )
}

/// What a heal reads from a snapshot's header.
struct Header {
    barrier: u32,
    seq: u64,
    missed: Option<u64>,
}

/// The header of a record stream (`devs` comes first, then `header`).
fn header_of(plain: &[u8]) -> SyncResult<Header> {
    let mut input = plain;
    let Some(Record::Devices(devs)) =
        read_record(&mut input, &[]).map_err(|_| invalid_snapshot())?
    else {
        return Err(invalid_snapshot());
    };
    match read_record(&mut input, &devs).map_err(|_| invalid_snapshot())? {
        Some(Record::Header {
            barrier,
            seq,
            missed_below_barrier,
            ..
        }) => Ok(Header {
            barrier,
            seq,
            missed: missed_below_barrier,
        }),
        _ => Err(invalid_snapshot()),
    }
}

/// Opens `blob` on a blocking thread, without the connection lock; its sealed bytes are dropped
/// once decrypted (`open_owned`).
async fn open_off_thread(
    creds: &Credentials,
    blob: SnapshotBlob,
) -> SyncResult<Zeroizing<Vec<u8>>> {
    let creds = creds.clone();
    off_thread(move || open_owned(&creds, blob)).await
}

/// Runs `f` on tokio's blocking pool (a dump, an open or a restore can take seconds).
async fn off_thread<T, E>(
    f: impl FnOnce() -> std::result::Result<T, E> + Send + 'static,
) -> SyncResult<T>
where
    T: Send + 'static,
    E: Into<SyncError> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|_| {
            SyncError::Local(AppError::Internal(
                "sync snapshot: a background task failed".into(),
            ))
        })?
        .map_err(Into::into)
}

/// A writer that refuses the first write that would take it past `limit` bytes.
struct Capped<W> {
    inner: W,
    written: usize,
    limit: usize,
    over: bool,
}

impl<W> Capped<W> {
    fn new(inner: W, limit: usize) -> Self {
        Self {
            inner,
            written: 0,
            limit,
            over: false,
        }
    }
}

impl<W: Write> Write for Capped<W> {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        if data.len() > self.limit - self.written {
            self.over = true;
            return Err(io::Error::other("sync snapshot: over the size limit"));
        }
        let taken = self.inner.write(data)?;
        self.written += taken;
        Ok(taken)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn loaded(conn: &Connection) -> Result<SyncState> {
    state::load(conn)?.ok_or_else(not_set_up)
}

fn not_set_up() -> AppError {
    AppError::Internal("sync snapshot: sync is not set up".into())
}

fn deflate_error(error: io::Error) -> AppError {
    AppError::Internal(format!("sync snapshot: deflate: {error}"))
}

fn invalid_snapshot() -> SyncError {
    SyncError::Local(AppError::Validation(
        "validation.syncSnapshotInvalid".into(),
    ))
}
