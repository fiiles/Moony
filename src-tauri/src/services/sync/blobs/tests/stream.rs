//! Blob streams between the store and the wire: an `UploadSource` reads a blob from one store
//! and seals it piece by piece; a `StoreSink` opens the pieces, as a download hands them over, into
//! an incoming blob of another store that becomes visible only once the whole stream checked out.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::db::files_schema;
use crate::error::{AppError, Result};
use crate::services::attachment_store;
use crate::services::sync::blobs::stream::{IncomingBlobs, SinkOutcome, StoreSink, UploadSource};
use crate::services::sync::engine::ConnAccess;
use crate::services::sync::envelope::blob::{sealed_len, BlobSealer, MAX_BLOB_PLAIN};
use crate::services::sync::envelope::stream::{CHUNK, HEADER_LEN};
use crate::services::sync::keys::VaultKeys;
use crate::services::sync::relay::{BlobSink, Delivery, RelayError, RelayResult, SinkFlow};

const EPOCH: u32 = 3;
const KEY: [u8; 32] = [0x11; 32];
const VAULT: [u8; 16] = [0x22; 16];
const SEALED_CHUNK: usize = CHUNK + 16;
const COMPLETE: RelayResult<Delivery> = Ok(Delivery::Complete);

/// A device's attachment store: an in-memory connection with only `files` attached. Its
/// `with_conn` answers `Auth` while `locked` (as a locked `Database` does) and counts its holds:
/// how many are open or waiting now, the most ever at once, and how many there were.
#[derive(Clone)]
struct Store {
    conn: Arc<Mutex<Connection>>,
    locked: Arc<AtomicBool>,
    open: Arc<AtomicUsize>,
    most_open: Arc<AtomicUsize>,
    holds: Arc<AtomicUsize>,
    incoming: Arc<IncomingBlobs>,
}

impl Store {
    fn new() -> Self {
        let conn = Connection::open_in_memory().expect("main");
        conn.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        files_schema::migrate(&conn).expect("files schema");
        Self {
            conn: Arc::new(Mutex::new(conn)),
            locked: Arc::new(AtomicBool::new(false)),
            open: Arc::new(AtomicUsize::new(0)),
            most_open: Arc::new(AtomicUsize::new(0)),
            holds: Arc::new(AtomicUsize::new(0)),
            incoming: Arc::default(),
        }
    }

    /// The connection, for the test's own reads and writes (never while a sink or source runs).
    fn lock(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().expect("test connection")
    }

    fn open(&self) -> usize {
        self.open.load(Ordering::SeqCst)
    }

    fn holds(&self) -> usize {
        self.holds.load(Ordering::SeqCst)
    }

    fn most_open(&self) -> usize {
        self.most_open.load(Ordering::SeqCst)
    }

    fn set_locked(&self, locked: bool) {
        self.locked.store(locked, Ordering::SeqCst);
    }
}

impl ConnAccess for Store {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        if self.locked.load(Ordering::SeqCst) {
            return Err(AppError::Auth("Database is locked".into()));
        }
        let now = self.open.fetch_add(1, Ordering::SeqCst) + 1;
        self.most_open.fetch_max(now, Ordering::SeqCst);
        self.holds.fetch_add(1, Ordering::SeqCst);
        let out = f(&self.lock());
        self.open.fetch_sub(1, Ordering::SeqCst);
        out
    }

    fn try_with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Option<Result<T>> {
        if self.locked.load(Ordering::SeqCst) {
            return Some(Err(AppError::Auth("Database is locked".into())));
        }
        let conn = self.conn.try_lock().ok()?;
        self.holds.fetch_add(1, Ordering::SeqCst);
        Some(f(&conn))
    }

    fn incoming_blobs(&self) -> &IncomingBlobs {
        &self.incoming
    }
}

fn keys() -> VaultKeys {
    VaultKeys::single(EPOCH, Zeroizing::new(KEY))
}

fn blob_id(n: u128) -> Uuid {
    Uuid::from_u128(0x0b9e_6a52_3a4c_4e0f_9d7a_0000_0000_0000 + n)
}

/// `len` bytes of a pattern that never repeats a chunk.
fn pattern(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i * 7 % 251) as u8).collect()
}

/// A store that holds `plain`, and the local ID it has there.
fn holding(plain: &[u8]) -> (Store, String) {
    let store = Store::new();
    let id = attachment_store::put_bytes(&store.lock(), plain)
        .expect("blob")
        .id;
    (store, id)
}

/// An upload source for `plain` sealed for `blob`, reading from a store that holds it.
fn source(plain: &[u8], blob: Uuid) -> (Store, UploadSource<Store>) {
    let (store, local) = holding(plain);
    let sealer = BlobSealer::new(EPOCH, &KEY, &VAULT, &blob);
    let source = UploadSource::new(store.clone(), local, blob, plain.len() as u64, sealer);
    (store, source)
}

/// The pieces an upload of `plain` as `blob` sends: the header, then one sealed chunk each.
fn wire(plain: &[u8], blob: Uuid) -> Vec<Vec<u8>> {
    let (_, source) = source(plain, blob);
    let announced = source.sealed_len();
    assert_eq!(announced, sealed_len(plain.len() as u64));
    let pieces: Vec<Vec<u8>> = source.into_body().map(|p| p.expect("piece")).collect();
    assert_eq!(pieces.iter().map(Vec::len).sum::<usize>() as u64, announced);
    pieces
}

fn total(pieces: &[Vec<u8>]) -> u64 {
    pieces.iter().map(Vec::len).sum::<usize>() as u64
}

fn sink(to: &Store, blob: Uuid) -> StoreSink<Store> {
    StoreSink::new(to.clone(), blob.to_string(), blob, keys(), VAULT)
}

/// A sink for `blob` on `to`, handed `pieces` as `get_blob` would: `begin` with the announced
/// length, then each piece while the sink says `Continue`; with what `get_blob` would answer.
fn deliver(
    to: &Store,
    blob: Uuid,
    announced: u64,
    pieces: &[Vec<u8>],
) -> (StoreSink<Store>, RelayResult<Delivery>) {
    let mut sink = sink(to, blob);
    if sink.begin(announced) == SinkFlow::Stop {
        return (sink, Ok(Delivery::Stopped));
    }
    for piece in pieces {
        if sink.piece(piece) == SinkFlow::Stop {
            return (sink, Ok(Delivery::Stopped));
        }
    }
    (sink, Ok(Delivery::Complete))
}

/// `deliver`, then `finish` with what `get_blob` answered.
fn download(to: &Store, blob: Uuid, announced: u64, pieces: &[Vec<u8>]) -> SinkOutcome {
    let (sink, delivery) = deliver(to, blob, announced, pieces);
    sink.finish(&delivery)
}

/// The blob is absent for every reader: not complete, no size, nothing to read.
fn assert_unreadable(store: &Store, id: &str) {
    let conn = store.lock();
    assert!(!attachment_store::is_complete(&conn, id).expect("is_complete"));
    assert_eq!(attachment_store::size_of(&conn, id).expect("size_of"), None);
    assert!(matches!(
        attachment_store::read_all(&conn, id),
        Err(AppError::NotFound(_))
    ));
    assert!(matches!(
        attachment_store::read_at(&conn, id, 0, &mut [0u8; 16]),
        Err(AppError::NotFound(_))
    ));
}

/// `assert_unreadable`, and no row of it is left, complete or incoming.
fn assert_absent(store: &Store, id: &str) {
    assert_unreadable(store, id);
    let rows: i64 = store
        .lock()
        .query_row(
            "SELECT COUNT(*) FROM files.blobs WHERE id = ?1",
            [id],
            |r| r.get(0),
        )
        .expect("rows");
    assert_eq!(rows, 0, "a row of {id} is left");
}

/// The blob is stored complete as `plain`, with the SHA-256 of `plain` recorded.
fn assert_stored(store: &Store, id: &str, plain: &[u8]) {
    let conn = store.lock();
    assert_eq!(attachment_store::read_all(&conn, id).expect("read"), plain);
    let sha: String = conn
        .query_row("SELECT sha256 FROM files.blobs WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .expect("sha256");
    assert_eq!(sha, hex::encode(Sha256::digest(plain)));
}

fn round_trip(len: usize, n: u128) {
    let plain = pattern(len);
    let blob = blob_id(n);
    let pieces = wire(&plain, blob);
    assert_eq!(
        pieces.len(),
        1 + len / CHUNK + 1,
        "header, full chunks, last"
    );
    let to = Store::new();
    let outcome = download(&to, blob, total(&pieces), &pieces);
    assert!(matches!(outcome, SinkOutcome::Stored), "{len}: {outcome:?}");
    assert_stored(&to, &blob.to_string(), &plain);
}

#[test]
fn a_blob_goes_through_the_wire_pieces_and_lands_under_its_id() {
    let plain = pattern(3 * CHUNK + 5);
    let blob = blob_id(1);
    let pieces = wire(&plain, blob);
    assert_eq!(pieces.len(), 5, "header, three full chunks, the last");
    assert_eq!(pieces[0].len(), HEADER_LEN);
    assert!(pieces[1..4].iter().all(|p| p.len() == SEALED_CHUNK));
    assert_eq!(pieces[4].len(), 5 + 16);

    let to = Store::new();
    let id = blob.to_string();
    let (sink, delivery) = deliver(&to, blob, total(&pieces), &pieces);
    // Every byte is written, but until `finish` the blob is still arriving: no reader sees it.
    assert_unreadable(&to, &id);
    let outcome = sink.finish(&delivery);
    assert!(matches!(outcome, SinkOutcome::Stored), "{outcome:?}");
    assert_stored(&to, &id, &plain);
}

#[test]
fn an_empty_blob_and_a_whole_chunk_blob_round_trip() {
    round_trip(0, 2);
    round_trip(CHUNK, 3);
    round_trip(2 * CHUNK, 4);
    round_trip(CHUNK - 1, 5);
}

#[test]
fn a_tampered_stream_stores_nothing() {
    let plain = pattern(3 * CHUNK + 5);
    let blob = blob_id(6);
    let pieces = wire(&plain, blob);
    // The epoch in the header (no key for it: indistinguishable from a key not received yet, so
    // it is retried rather than shown as failed), the salt (another key), a middle chunk, and the
    // last chunk, after which every earlier chunk was written already.
    for (piece, at) in [(0, 4), (0, 10), (2, 100), (4, 3)] {
        let mut tampered = pieces.clone();
        tampered[piece][at] ^= 0x01;
        let to = Store::new();
        let outcome = download(&to, blob, total(&tampered), &tampered);
        if (piece, at) == (0, 4) {
            assert!(
                matches!(outcome, SinkOutcome::UnknownEpoch),
                "piece {piece} byte {at}: {outcome:?}"
            );
        } else {
            assert!(
                matches!(outcome, SinkOutcome::Unreadable),
                "piece {piece} byte {at}: {outcome:?}"
            );
        }
        assert_absent(&to, &blob.to_string());
    }
}

#[test]
fn a_stream_sealed_for_another_blob_or_vault_stores_nothing() {
    let plain = pattern(CHUNK + 9);
    let pieces = wire(&plain, blob_id(7));
    let to = Store::new();
    let other = blob_id(8);
    let outcome = download(&to, other, total(&pieces), &pieces);
    assert!(matches!(outcome, SinkOutcome::Unreadable), "{outcome:?}");
    assert_absent(&to, &other.to_string());

    let blob = blob_id(7);
    let mut sink = StoreSink::new(to.clone(), blob.to_string(), blob, keys(), [0x33; 16]);
    assert_eq!(sink.begin(total(&pieces)), SinkFlow::Continue);
    let flows: Vec<SinkFlow> = pieces.iter().map(|p| sink.piece(p)).collect();
    assert_eq!(flows.last(), Some(&SinkFlow::Stop));
    let outcome = sink.finish(&Ok(Delivery::Stopped));
    assert!(matches!(outcome, SinkOutcome::Unreadable), "{outcome:?}");
    assert_absent(&to, &blob.to_string());
}

#[test]
fn a_truncated_stream_stores_nothing() {
    let plain = pattern(3 * CHUNK + 5);
    let blob = blob_id(9);
    let pieces = wire(&plain, blob);
    let announced = total(&pieces);

    // One piece less than announced: three whole chunks were written, the last never came.
    let to = Store::new();
    let outcome = download(&to, blob, announced, &pieces[..pieces.len() - 1]);
    assert!(matches!(outcome, SinkOutcome::Incomplete), "{outcome:?}");
    assert_absent(&to, &blob.to_string());

    // The body ends one byte short.
    let mut short = pieces.clone();
    short.last_mut().expect("last").pop();
    let to = Store::new();
    let outcome = download(&to, blob, announced, &short);
    assert!(matches!(outcome, SinkOutcome::Incomplete), "{outcome:?}");
    assert_absent(&to, &blob.to_string());

    // Nothing came after `begin`.
    let to = Store::new();
    let outcome = download(&to, blob, announced, &[]);
    assert!(matches!(outcome, SinkOutcome::Incomplete), "{outcome:?}");
    assert_absent(&to, &blob.to_string());
}

#[test]
fn a_length_no_stream_has_stores_nothing() {
    let blob = blob_id(10);
    for announced in [0, 10, sealed_len(MAX_BLOB_PLAIN + 1)] {
        let to = Store::new();
        let mut sink = sink(&to, blob);
        assert_eq!(sink.begin(announced), SinkFlow::Stop, "{announced}");
        assert_eq!(sink.piece(&[0u8; 8]), SinkFlow::Stop);
        let outcome = sink.finish(&Ok(Delivery::Stopped));
        assert!(matches!(outcome, SinkOutcome::Unreadable), "{outcome:?}");
        assert_absent(&to, &blob.to_string());
    }
}

#[test]
fn a_download_that_fails_mid_body_leaves_nothing() {
    let plain = pattern(3 * CHUNK + 5);
    let blob = blob_id(11);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let announced = total(&pieces);
    let dropped: RelayResult<Delivery> = Err(RelayError::Network("connection reset".into()));

    // The connection drops after two chunks: `get_blob` answers an error.
    let to = Store::new();
    let (sink, _) = deliver(&to, blob, announced, &pieces[..3]);
    let outcome = sink.finish(&dropped);
    assert!(matches!(outcome, SinkOutcome::Incomplete), "{outcome:?}");
    assert_absent(&to, &id);

    // Every byte came, then the download still failed: what the sink got is discarded all the
    // same, although the stream itself would have opened.
    let to = Store::new();
    let (sink, _) = deliver(&to, blob, announced, &pieces);
    let outcome = sink.finish(&dropped);
    assert!(matches!(outcome, SinkOutcome::Incomplete), "{outcome:?}");
    assert_absent(&to, &id);

    // A transfer cancelled in the middle (its future dropped) leaves nothing either.
    let to = Store::new();
    drop(deliver(&to, blob, announced, &pieces[..2]));
    assert_absent(&to, &id);
}

#[test]
fn a_blob_removed_from_the_store_mid_transfer_is_gone_not_stored() {
    let plain = pattern(3 * CHUNK + 5);
    let blob = blob_id(12);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let to = Store::new();
    let mut sink = sink(&to, blob);
    assert_eq!(sink.begin(total(&pieces)), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[0]), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[1]), SinkFlow::Continue);
    // The row naming it went and the sweep took the incoming blob with it.
    attachment_store::sweep_incomplete(&to.lock()).expect("sweep");
    assert_eq!(sink.piece(&pieces[2]), SinkFlow::Stop);
    assert_eq!(sink.piece(&pieces[3]), SinkFlow::Stop);
    let outcome = sink.finish(&Ok(Delivery::Stopped));
    assert!(
        matches!(outcome, SinkOutcome::Local(AppError::NotFound(_))),
        "{outcome:?}"
    );
    assert_absent(&to, &id);
}

/// The sink follows its incoming row by ID and rowid: abandoned under it, and the rowid taken by
/// another download, it stops and leaves the other blob alone.
#[test]
fn a_download_whose_row_was_abandoned_never_writes_into_the_blob_that_took_its_place() {
    let plain = pattern(3 * CHUNK + 5);
    let blob = blob_id(32);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let to = Store::new();
    let mut sink = sink(&to, blob);
    assert_eq!(sink.begin(total(&pieces)), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[0]), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[1]), SinkFlow::Continue);
    let other = blob_id(33).to_string();
    {
        let conn = to.lock();
        attachment_store::abandon_incoming(&conn, &id).expect("abandon");
        attachment_store::begin_incoming(&conn, &other, plain.len() as u64).expect("other");
    }
    assert_eq!(sink.piece(&pieces[2]), SinkFlow::Stop);
    let outcome = sink.finish(&Ok(Delivery::Stopped));
    assert!(
        matches!(outcome, SinkOutcome::Local(AppError::NotFound(_))),
        "{outcome:?}"
    );
    assert_absent(&to, &id);
    let data: Vec<u8> = to
        .lock()
        .query_row(
            "SELECT data FROM files.blobs WHERE id = ?1",
            [&other],
            |r| r.get(0),
        )
        .expect("other");
    assert!(data.iter().all(|b| *b == 0), "the other blob is untouched");
}

/// Only the download holding the ID finishes its row; a row someone else sealed is not this
/// download's to store.
#[test]
fn a_download_whose_row_someone_else_finished_is_not_stored() {
    let plain = pattern(3 * CHUNK + 5);
    let blob = blob_id(34);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let to = Store::new();
    let mut sink = sink(&to, blob);
    assert_eq!(sink.begin(total(&pieces)), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[0]), SinkFlow::Continue);
    attachment_store::finish_incoming(&to.lock(), &id, "someone else").expect("sealed");
    let mut delivery = COMPLETE;
    for piece in &pieces[1..] {
        if sink.piece(piece) == SinkFlow::Stop {
            delivery = Ok(Delivery::Stopped);
            break;
        }
    }
    let outcome = sink.finish(&delivery);
    assert!(
        matches!(outcome, SinkOutcome::Local(AppError::NotFound(_))),
        "{outcome:?}"
    );
    let sha: String = to
        .lock()
        .query_row("SELECT sha256 FROM files.blobs WHERE id = ?1", [&id], |r| {
            r.get(0)
        })
        .expect("sha256");
    assert_eq!(sha, "someone else", "this download sealed nothing");
}

#[test]
fn a_locked_profile_stops_the_sink_and_nothing_stays_readable() {
    let plain = pattern(2 * CHUNK + 1);
    let blob = blob_id(13);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let to = Store::new();
    let mut sink = sink(&to, blob);
    assert_eq!(sink.begin(total(&pieces)), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[0]), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[1]), SinkFlow::Continue);
    to.set_locked(true);
    assert_eq!(sink.piece(&pieces[2]), SinkFlow::Stop);
    to.set_locked(false);
    let outcome = sink.finish(&Ok(Delivery::Stopped));
    assert!(
        matches!(outcome, SinkOutcome::Local(AppError::Auth(_))),
        "{outcome:?}"
    );
    assert_absent(&to, &id);
}

#[test]
fn a_profile_locked_during_finish_leaves_nothing_readable_and_the_blob_can_come_again() {
    let plain = pattern(2 * CHUNK + 1);
    let blob = blob_id(18);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let to = Store::new();
    let (sink, delivery) = deliver(&to, blob, total(&pieces), &pieces);
    to.set_locked(true);
    let outcome = sink.finish(&delivery);
    assert!(
        matches!(outcome, SinkOutcome::Local(AppError::Auth(_))),
        "{outcome:?}"
    );
    to.set_locked(false);
    // The incoming row could not be removed while the profile was locked; it stays invisible.
    assert_unreadable(&to, &id);
    // The next download of the blob replaces it.
    let outcome = download(&to, blob, total(&pieces), &pieces);
    assert!(matches!(outcome, SinkOutcome::Stored), "{outcome:?}");
    assert_stored(&to, &id, &plain);
}

#[test]
fn a_second_download_of_a_blob_in_progress_is_refused_and_leaves_the_first_alone() {
    let plain = pattern(3 * CHUNK + 5);
    let blob = blob_id(19);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let announced = total(&pieces);
    let to = Store::new();

    let mut first = sink(&to, blob);
    assert_eq!(first.begin(announced), SinkFlow::Continue);
    assert_eq!(first.piece(&pieces[0]), SinkFlow::Continue);
    assert_eq!(first.piece(&pieces[1]), SinkFlow::Continue);

    // A second sink for the same ID, while the first is mid-body: refused, and settling or
    // dropping it touches nothing of the first's.
    let mut second = sink(&to, blob);
    assert_eq!(second.begin(announced), SinkFlow::Stop);
    assert_eq!(second.piece(&pieces[0]), SinkFlow::Stop);
    let outcome = second.finish(&Ok(Delivery::Stopped));
    assert!(matches!(outcome, SinkOutcome::InProgress), "{outcome:?}");
    let mut third = sink(&to, blob);
    assert_eq!(third.begin(announced), SinkFlow::Stop);
    drop(third);

    for piece in &pieces[2..] {
        assert_eq!(first.piece(piece), SinkFlow::Continue);
    }
    let outcome = first.finish(&COMPLETE);
    assert!(matches!(outcome, SinkOutcome::Stored), "{outcome:?}");
    assert_stored(&to, &id, &plain);

    // Once the first settled, the ID is free again (here: the blob is already stored).
    let outcome = download(&to, blob, announced, &pieces);
    assert!(matches!(outcome, SinkOutcome::AlreadyHere), "{outcome:?}");
    // And after a failed first, a new download of it begins and stores.
    let other = blob_id(20);
    let pieces = wire(&plain, other);
    let (failed, _) = deliver(&to, other, total(&pieces), &pieces[..2]);
    drop(failed);
    let outcome = download(&to, other, total(&pieces), &pieces);
    assert!(matches!(outcome, SinkOutcome::Stored), "{outcome:?}");
}

#[test]
fn a_blob_already_here_is_not_downloaded_again() {
    let plain = pattern(CHUNK + 3);
    let blob = blob_id(21);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let to = Store::new();
    assert!(matches!(
        download(&to, blob, total(&pieces), &pieces),
        SinkOutcome::Stored
    ));

    let mut again = sink(&to, blob);
    assert_eq!(again.begin(total(&pieces)), SinkFlow::Stop);
    let outcome = again.finish(&Ok(Delivery::Stopped));
    assert!(matches!(outcome, SinkOutcome::AlreadyHere), "{outcome:?}");
    // The stored blob is untouched.
    assert_stored(&to, &id, &plain);
}

#[test]
fn a_locked_profile_stops_the_source() {
    let plain = pattern(2 * CHUNK + 1);
    let (store, mut source) = source(&plain, blob_id(14));
    assert_eq!(
        source.next().expect("header").expect("header").len(),
        HEADER_LEN
    );
    assert!(source.next().expect("chunk").is_ok());
    store.set_locked(true);
    let err = source.next().expect("an item").expect_err("locked");
    let cause = err.get_ref().and_then(|e| e.downcast_ref::<AppError>());
    assert!(matches!(cause, Some(AppError::Auth(_))), "{err:?}");
    store.set_locked(false);
    assert!(source.next().is_none(), "the source stops after an error");
}

#[test]
fn a_blob_deleted_during_an_upload_stops_the_source() {
    let plain = pattern(2 * CHUNK + 1);
    let blob = blob_id(15);
    let (store, local) = holding(&plain);
    let sealer = BlobSealer::new(EPOCH, &KEY, &VAULT, &blob);
    let mut source = UploadSource::new(
        store.clone(),
        local.clone(),
        blob,
        plain.len() as u64,
        sealer,
    );
    assert!(source.next().expect("header").is_ok());
    assert!(source.next().expect("chunk").is_ok());
    assert!(attachment_store::delete(&store.lock(), &local).expect("delete"));
    let err = source.next().expect("an item").expect_err("gone");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert!(source.next().is_none());
}

/// The upload follows its blob by ID and rowid after the first item, without reading its hash
/// again: a blob deleted under it whose rowid another blob took is still gone.
#[test]
fn a_blob_deleted_during_an_upload_is_gone_even_when_another_blob_takes_its_place() {
    let plain = pattern(3 * CHUNK + 1);
    let blob = blob_id(30);
    let (store, local) = holding(&plain);
    let sealer = BlobSealer::new(EPOCH, &KEY, &VAULT, &blob);
    let mut source = UploadSource::new(
        store.clone(),
        local.clone(),
        blob,
        plain.len() as u64,
        sealer,
    );
    assert!(source.next().expect("header").is_ok());
    assert!(source.next().expect("chunk").is_ok());
    let rowid = |id: &str| -> i64 {
        store
            .lock()
            .query_row("SELECT rowid FROM files.blobs WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .expect("rowid")
    };
    let before = rowid(&local);
    assert!(attachment_store::delete(&store.lock(), &local).expect("delete"));
    let other = attachment_store::put_bytes(&store.lock(), &plain)
        .expect("other")
        .id;
    assert_eq!(rowid(&other), before, "the rowid is reused");
    let err = source.next().expect("an item").expect_err("gone");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    assert!(source.next().is_none());
}

/// A blob deleted under an upload and begun again as an incoming blob under the same ID can take
/// the same rowid; its zeros are never sealed as content. While a download writes it the upload
/// stops at once; a left-over incoming row (a download dropped without settling) stops it before
/// its last chunk.
#[test]
fn an_upload_never_sends_an_incoming_blob_that_took_its_place() {
    let plain = pattern(3 * CHUNK + 1);
    for left_over in [false, true] {
        let blob = blob_id(31);
        let (store, local) = holding(&plain);
        let sealer = BlobSealer::new(EPOCH, &KEY, &VAULT, &blob);
        let mut source = UploadSource::new(
            store.clone(),
            local.clone(),
            blob,
            plain.len() as u64,
            sealer,
        );
        assert!(source.next().expect("header").is_ok());
        assert!(source.next().expect("chunk").is_ok());
        assert!(attachment_store::delete(&store.lock(), &local).expect("delete"));
        attachment_store::begin_incoming(&store.lock(), &local, plain.len() as u64)
            .expect("incoming");
        if !left_over {
            assert!(store.incoming_blobs().claim(&local));
        }
        let items: Vec<std::io::Result<Vec<u8>>> = source.collect();
        assert!(
            items.last().is_some_and(|item| item.is_err()),
            "left over {left_over}: the last item is an error"
        );
        // A body without its last chunk is stored nowhere: the server refuses an incomplete body.
        assert!(
            items
                .iter()
                .flatten()
                .all(|piece| piece.len() == SEALED_CHUNK),
            "left over {left_over}: the last chunk is never sealed"
        );
        if !left_over {
            assert_eq!(items.len(), 1, "a download in progress stops it at once");
        }
    }
}

#[test]
fn a_declared_length_that_differs_from_the_store_sends_nothing() {
    let plain = pattern(2 * CHUNK + 1);
    let blob = blob_id(22);
    let (store, local) = holding(&plain);
    for declared in [plain.len() as u64 - 1, plain.len() as u64 + 1, 0] {
        let sealer = BlobSealer::new(EPOCH, &KEY, &VAULT, &blob);
        let mut source = UploadSource::new(store.clone(), local.clone(), blob, declared, sealer);
        let err = source
            .next()
            .expect("an item")
            .expect_err("a length that differs");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData, "{declared}");
        assert!(source.next().is_none(), "nothing is sent");
    }
}

#[test]
fn a_blob_not_complete_here_sends_nothing() {
    let store = Store::new();
    let blob = blob_id(23);
    let id = blob.to_string();
    // Still arriving (an incoming blob), then not here at all.
    attachment_store::begin_incoming(&store.lock(), &id, 10).expect("begin");
    for _ in 0..2 {
        let sealer = BlobSealer::new(EPOCH, &KEY, &VAULT, &blob);
        let mut source = UploadSource::new(store.clone(), id.clone(), blob, 10, sealer);
        let err = source.next().expect("an item").expect_err("not complete");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(source.next().is_none(), "nothing is sent");
        attachment_store::abandon_incoming(&store.lock(), &id).expect("abandon");
    }
}

#[test]
fn the_source_never_holds_the_connection_between_pieces() {
    let plain = pattern(3 * CHUNK + 5);
    let (store, source) = source(&plain, blob_id(16));
    let before = store.holds();
    let mut per_item = Vec::new();
    for item in source {
        let piece = item.expect("piece");
        assert!(piece.len() <= SEALED_CHUNK);
        assert_eq!(store.open(), 0, "a hold is open between pieces");
        per_item.push(store.holds() - before - per_item.iter().sum::<usize>());
    }
    // The header follows one check of the stored size; each chunk is one read of at most CHUNK.
    assert_eq!(per_item, vec![1, 1, 1, 1, 1]);
    assert_eq!(store.most_open(), 1);
}

#[test]
fn the_sink_writes_each_chunk_in_its_own_short_hold() {
    let plain = pattern(3 * CHUNK + 5);
    let blob = blob_id(17);
    let pieces = wire(&plain, blob);
    // The whole body in one piece still writes one chunk per hold.
    let whole = vec![pieces.concat()];
    for delivered in [pieces.clone(), whole] {
        let to = Store::new();
        let mut sink = sink(&to, blob);
        assert_eq!(sink.begin(total(&delivered)), SinkFlow::Continue);
        assert_eq!(
            (to.open(), to.holds()),
            (0, 1),
            "begin reserves in one hold"
        );
        for piece in &delivered {
            assert_eq!(sink.piece(piece), SinkFlow::Continue);
            assert_eq!(to.open(), 0, "a hold is open between pieces");
        }
        assert_eq!(to.holds(), 1 + 4, "one hold per chunk");
        assert!(matches!(sink.finish(&COMPLETE), SinkOutcome::Stored));
        assert_eq!(to.holds(), 1 + 4 + 1);
        assert_eq!(to.most_open(), 1);
        assert_stored(&to, &blob.to_string(), &plain);
    }
}

#[test]
fn dropping_a_sink_inside_a_hold_of_its_connection_does_not_hang() {
    let plain = pattern(CHUNK + 3);
    let blob = blob_id(24);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let to = Store::new();
    let (sink, _) = deliver(&to, blob, total(&pieces), &pieces[..2]);
    to.with_conn(|_| {
        drop(sink);
        Ok(())
    })
    .expect("hold");
    // The abandon could not run without waiting: the row stays invisible, the ID is free, and the
    // next download replaces the row.
    assert_unreadable(&to, &id);
    let outcome = download(&to, blob, total(&pieces), &pieces);
    assert!(matches!(outcome, SinkOutcome::Stored), "{outcome:?}");
}

/// Runs `work` on a one-worker multi-threaded runtime while another thread holds `store`'s
/// connection, and checks that another task of that runtime still runs while `work` waits for the
/// connection: the transfer must not take the worker with it.
fn stays_responsive<R: Send + 'static>(store: &Store, work: impl FnOnce() -> R + Send + 'static) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .build()
        .expect("runtime");
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let conn = Arc::clone(&store.conn);
    let holder = std::thread::spawn(move || {
        let _held = conn.lock().expect("hold");
        held_tx.send(()).expect("held");
        release_rx.recv().ok();
    });
    held_rx.recv().expect("held");

    let waiting = store.open() + 1;
    let task = runtime.spawn(async move { work() });
    let deadline = Instant::now() + Duration::from_secs(10);
    while store.open() < waiting {
        assert!(
            Instant::now() < deadline,
            "the work never asked for the connection"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
    let (ran_tx, ran_rx) = mpsc::channel();
    runtime.spawn(async move { ran_tx.send(()).ok() });
    let ran = ran_rx.recv_timeout(Duration::from_secs(5)).is_ok();

    release_tx.send(()).expect("release");
    holder.join().expect("holder");
    runtime.block_on(task).expect("work");
    assert!(
        ran,
        "another task stalled while the transfer waited for the connection"
    );
}

#[test]
fn a_sink_waiting_for_the_connection_does_not_stall_other_tasks() {
    let plain = pattern(2 * CHUNK + 1);
    let blob = blob_id(25);
    let pieces = wire(&plain, blob);
    let to = Store::new();
    let mut sink = sink(&to, blob);
    assert_eq!(sink.begin(total(&pieces)), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[0]), SinkFlow::Continue);
    let chunk = pieces[1].clone();
    stays_responsive(&to, move || {
        assert_eq!(sink.piece(&chunk), SinkFlow::Continue);
        sink
    });
}

#[test]
fn a_source_waiting_for_the_connection_does_not_stall_other_tasks() {
    let plain = pattern(2 * CHUNK + 1);
    let (store, mut source) = source(&plain, blob_id(26));
    assert!(source.next().expect("header").is_ok());
    stays_responsive(&store, move || {
        assert!(source.next().expect("chunk").is_ok());
    });
}
