//! Streaming a blob between the encrypted store and the wire holds a few chunks in memory, never
//! the blob: a 100 MiB blob is read from the store and sealed piece by piece, and the pieces are
//! opened into the store again, while a counting allocator watches the peak of live bytes.
//!
//! SQLite allocates through its own allocator, not Rust's, so the store's pages are not counted;
//! every buffer the upload source and the store sink hold is.

use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use moony_lib::db::files_schema;
use moony_lib::error::Result;
use moony_lib::services::attachment_store;
use moony_lib::services::sync::blobs::stream::{SinkOutcome, StoreSink, UploadSource};
use moony_lib::services::sync::engine::ConnAccess;
use moony_lib::services::sync::envelope::blob::BlobSealer;
use moony_lib::services::sync::keys::VaultKeys;
use moony_lib::services::sync::relay::{BlobSink, SinkFlow};
use rusqlite::Connection;
use uuid::Uuid;
use zeroize::Zeroizing;

/// Live bytes allocated through Rust's allocator, and the most there were since the last reset.
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

struct Counting;

fn grew(by: usize) {
    let live = LIVE.fetch_add(by, Ordering::SeqCst) + by;
    PEAK.fetch_max(live, Ordering::SeqCst);
}

fn shrank(by: usize) {
    LIVE.fetch_sub(by, Ordering::SeqCst);
}

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            grew(layout.size());
        }
        ptr
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            grew(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        shrank(layout.size());
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(ptr, layout, new_size) };
        if !moved.is_null() {
            if new_size > layout.size() {
                grew(new_size - layout.size());
            } else {
                shrank(layout.size() - new_size);
            }
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// The profile's connection, one hold at a time, as `Database` gives it.
#[derive(Clone)]
struct Db(Arc<Mutex<Connection>>);

impl ConnAccess for Db {
    fn with_conn<T>(&self, f: impl FnOnce(&Connection) -> Result<T>) -> Result<T> {
        f(&self.0.lock().expect("connection"))
    }
}

/// `left` pseudo-random bytes (xorshift64), so no chunk repeats and nothing compresses.
struct Noise {
    state: u64,
    left: u64,
}

impl Read for Noise {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf
            .len()
            .min(usize::try_from(self.left).unwrap_or(usize::MAX));
        for word in buf[..n].chunks_mut(8) {
            self.state ^= self.state << 13;
            self.state ^= self.state >> 7;
            self.state ^= self.state << 17;
            word.copy_from_slice(&self.state.to_le_bytes()[..word.len()]);
        }
        self.left -= n as u64;
        Ok(n)
    }
}

const SIZE: u64 = attachment_store::MAX_DOCUMENT_BYTES;
const KEY: [u8; 32] = [0x11; 32];
const VAULT: [u8; 16] = [0x22; 16];

#[test]
fn a_100_mib_blob_seals_and_opens_within_a_fixed_memory_bound() {
    let conn = Connection::open_in_memory().expect("main");
    files_schema::attach(&conn, Path::new(":memory:")).expect("store");
    let mut noise = Noise {
        state: 0x9e37_79b9_7f4a_7c15,
        left: SIZE,
    };
    let original = attachment_store::put_reader(&conn, &mut noise, SIZE).expect("original");
    let db = Db(Arc::new(Mutex::new(conn)));
    let blob = Uuid::new_v4();
    let copy = blob.to_string();

    let baseline = LIVE.load(Ordering::SeqCst);
    PEAK.store(baseline, Ordering::SeqCst);

    let sealer = BlobSealer::new(0, &KEY, &VAULT, &blob);
    let source = UploadSource::new(db.clone(), original.id.clone(), blob, SIZE, sealer);
    let keys = VaultKeys::single(0, Zeroizing::new(KEY));
    let mut sink = StoreSink::new(db.clone(), copy.clone(), blob, keys, VAULT);
    assert_eq!(sink.begin(source.sealed_len()), SinkFlow::Continue);
    for piece in source.into_body() {
        assert_eq!(sink.piece(&piece.expect("piece")), SinkFlow::Continue);
    }
    let outcome = sink.finish();

    let peak_growth = PEAK.load(Ordering::SeqCst).saturating_sub(baseline);
    assert!(matches!(outcome, SinkOutcome::Stored), "{outcome:?}");
    assert!(
        peak_growth >= 64 * 1024,
        "the allocator saw no chunk: {peak_growth} bytes"
    );
    assert!(
        peak_growth < 2 * 1024 * 1024,
        "peak growth {peak_growth} bytes"
    );

    db.with_conn(|conn| {
        let sha: String = conn.query_row(
            "SELECT sha256 FROM files.blobs WHERE id = ?1",
            [&copy],
            |r| r.get(0),
        )?;
        assert_eq!(sha, original.sha256, "the copy's recorded hash");
        assert!(
            attachment_store::verify(conn, &copy, &original.sha256)?,
            "the copy's content hashes to the original's"
        );
        Ok(())
    })
    .expect("compare");
}
