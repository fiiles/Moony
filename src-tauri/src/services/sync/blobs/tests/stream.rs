//! Blob streams between the store and the wire: an `UploadSource` reads a blob from one store
//! and seals it piece by piece; a `StoreSink` opens the pieces, as a download hands them over, into
//! an incoming blob of another store that becomes visible only once the whole stream checked out.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::db::files_schema;
use crate::error::{AppError, Result};
use crate::services::attachment_store;
use crate::services::sync::blobs::stream::{SinkOutcome, StoreSink, UploadSource};
use crate::services::sync::engine::ConnAccess;
use crate::services::sync::envelope::blob::{sealed_len, BlobSealer, MAX_BLOB_PLAIN};
use crate::services::sync::envelope::stream::{CHUNK, HEADER_LEN};
use crate::services::sync::keys::VaultKeys;
use crate::services::sync::relay::{BlobSink, SinkFlow};

const EPOCH: u32 = 3;
const KEY: [u8; 32] = [0x11; 32];
const VAULT: [u8; 16] = [0x22; 16];
const SEALED_CHUNK: usize = CHUNK + 16;

/// A device's attachment store: an in-memory connection with only `files` attached. Its
/// `with_conn` answers `Auth` while `locked` (as a locked `Database` does) and counts its holds:
/// how many are open now, the most ever open at once, and how many there were.
#[derive(Clone)]
struct Store {
    conn: Arc<Mutex<Connection>>,
    locked: Arc<AtomicBool>,
    open: Arc<AtomicUsize>,
    most_open: Arc<AtomicUsize>,
    holds: Arc<AtomicUsize>,
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

/// A sink for `blob` on `to`, handed `pieces` as `get_blob` would: `begin` with the announced
/// length, then each piece while the sink says `Continue`.
fn deliver(to: &Store, blob: Uuid, announced: u64, pieces: &[Vec<u8>]) -> StoreSink<Store> {
    let mut sink = StoreSink::new(to.clone(), blob.to_string(), blob, keys(), VAULT);
    if sink.begin(announced) == SinkFlow::Continue {
        for piece in pieces {
            if sink.piece(piece) == SinkFlow::Stop {
                break;
            }
        }
    }
    sink
}

/// The blob is absent for every reader, and no row of it is left, complete or incoming.
fn assert_absent(store: &Store, id: &str) {
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
    let rows: i64 = conn
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
    let sink = deliver(&to, blob, total(&pieces), &pieces);
    let outcome = sink.finish();
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
    let sink = deliver(&to, blob, total(&pieces), &pieces);
    // Every byte is written, but until `finish` the blob is still arriving: no reader sees it.
    {
        let conn = to.lock();
        assert!(!attachment_store::is_complete(&conn, &id).expect("is_complete"));
        assert!(matches!(
            attachment_store::read_all(&conn, &id),
            Err(AppError::NotFound(_))
        ));
    }
    let outcome = sink.finish();
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
    // The epoch in the header (no key for it), the salt (another key), a middle chunk, and the
    // last chunk, after which every earlier chunk was written already.
    for (piece, at) in [(0, 4), (0, 10), (2, 100), (4, 3)] {
        let mut tampered = pieces.clone();
        tampered[piece][at] ^= 0x01;
        let to = Store::new();
        let sink = deliver(&to, blob, total(&tampered), &tampered);
        let outcome = sink.finish();
        assert!(
            matches!(outcome, SinkOutcome::Unreadable),
            "piece {piece} byte {at}: {outcome:?}"
        );
        assert_absent(&to, &blob.to_string());
    }
}

#[test]
fn a_stream_sealed_for_another_blob_or_vault_stores_nothing() {
    let plain = pattern(CHUNK + 9);
    let pieces = wire(&plain, blob_id(7));
    let to = Store::new();
    let other = blob_id(8);
    let sink = deliver(&to, other, total(&pieces), &pieces);
    assert!(matches!(sink.finish(), SinkOutcome::Unreadable));
    assert_absent(&to, &other.to_string());

    let blob = blob_id(7);
    let mut sink = StoreSink::new(to.clone(), blob.to_string(), blob, keys(), [0x33; 16]);
    assert_eq!(sink.begin(total(&pieces)), SinkFlow::Continue);
    let flows: Vec<SinkFlow> = pieces.iter().map(|p| sink.piece(p)).collect();
    assert_eq!(flows.last(), Some(&SinkFlow::Stop));
    assert!(matches!(sink.finish(), SinkOutcome::Unreadable));
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
    let sink = deliver(&to, blob, announced, &pieces[..pieces.len() - 1]);
    let outcome = sink.finish();
    assert!(matches!(outcome, SinkOutcome::Incomplete), "{outcome:?}");
    assert_absent(&to, &blob.to_string());

    // The body ends one byte short.
    let mut short = pieces.clone();
    short.last_mut().expect("last").pop();
    let to = Store::new();
    let sink = deliver(&to, blob, announced, &short);
    let outcome = sink.finish();
    assert!(matches!(outcome, SinkOutcome::Incomplete), "{outcome:?}");
    assert_absent(&to, &blob.to_string());

    // Nothing came after `begin`.
    let to = Store::new();
    let sink = deliver(&to, blob, announced, &[]);
    assert!(matches!(sink.finish(), SinkOutcome::Incomplete));
    assert_absent(&to, &blob.to_string());
}

#[test]
fn a_length_no_stream_has_stores_nothing() {
    let blob = blob_id(10);
    for announced in [0, 10, sealed_len(MAX_BLOB_PLAIN + 1)] {
        let to = Store::new();
        let mut sink = StoreSink::new(to.clone(), blob.to_string(), blob, keys(), VAULT);
        assert_eq!(sink.begin(announced), SinkFlow::Stop, "{announced}");
        assert_eq!(sink.piece(&[0u8; 8]), SinkFlow::Stop);
        assert!(matches!(sink.finish(), SinkOutcome::Unreadable));
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

    // The connection drops after two chunks: `get_blob` answers an error and the caller abandons.
    let to = Store::new();
    deliver(&to, blob, announced, &pieces[..3]).abandon();
    assert_absent(&to, &id);

    // Every byte came, then the download still failed: what the sink got is discarded all the same.
    let to = Store::new();
    deliver(&to, blob, announced, &pieces).abandon();
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
    let mut sink = StoreSink::new(to.clone(), id.clone(), blob, keys(), VAULT);
    assert_eq!(sink.begin(total(&pieces)), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[0]), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[1]), SinkFlow::Continue);
    // The row naming it went and the sweep took the incoming blob with it.
    attachment_store::sweep_incomplete(&to.lock()).expect("sweep");
    assert_eq!(sink.piece(&pieces[2]), SinkFlow::Stop);
    assert_eq!(sink.piece(&pieces[3]), SinkFlow::Stop);
    let outcome = sink.finish();
    assert!(
        matches!(outcome, SinkOutcome::Local(AppError::NotFound(_))),
        "{outcome:?}"
    );
    assert_absent(&to, &id);
}

#[test]
fn a_locked_profile_stops_the_sink_and_nothing_stays_readable() {
    let plain = pattern(2 * CHUNK + 1);
    let blob = blob_id(13);
    let id = blob.to_string();
    let pieces = wire(&plain, blob);
    let to = Store::new();
    let mut sink = StoreSink::new(to.clone(), id.clone(), blob, keys(), VAULT);
    assert_eq!(sink.begin(total(&pieces)), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[0]), SinkFlow::Continue);
    assert_eq!(sink.piece(&pieces[1]), SinkFlow::Continue);
    to.locked.store(true, Ordering::SeqCst);
    assert_eq!(sink.piece(&pieces[2]), SinkFlow::Stop);
    to.locked.store(false, Ordering::SeqCst);
    let outcome = sink.finish();
    assert!(
        matches!(outcome, SinkOutcome::Local(AppError::Auth(_))),
        "{outcome:?}"
    );
    assert_absent(&to, &id);
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
    store.locked.store(true, Ordering::SeqCst);
    let err = source.next().expect("an item").expect_err("locked");
    let cause = err.get_ref().and_then(|e| e.downcast_ref::<AppError>());
    assert!(matches!(cause, Some(AppError::Auth(_))), "{err:?}");
    store.locked.store(false, Ordering::SeqCst);
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
    // The header needs no read; each chunk is one read of at most CHUNK bytes.
    assert_eq!(per_item, vec![0, 1, 1, 1, 1]);
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
        let mut sink = StoreSink::new(to.clone(), blob.to_string(), blob, keys(), VAULT);
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
        assert!(matches!(sink.finish(), SinkOutcome::Stored));
        assert_eq!(to.holds(), 1 + 4 + 1);
        assert_eq!(to.most_open(), 1);
        assert_stored(&to, &blob.to_string(), &plain);
    }
}
