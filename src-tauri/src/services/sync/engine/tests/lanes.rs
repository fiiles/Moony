//! The transfer lanes beside the rounds (attachments design §5.3, §5.4; ADR 0030): the running
//! engine moves documents and photos by itself, starts its lanes once (the sweep of incomplete
//! blobs and the full reconcile before any transfer), never runs two engines' lanes at once, stops
//! them with itself, retries on "Sync now", takes what the round leaves without ever sending a
//! blob the round sends, follows the platform's download policy, coalesces what it tells the app,
//! and rests in the states where the round does not sync.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Condvar;
use std::time::Duration;

use super::runtime::calls_since;
use super::runtime_loop::{rounds, start_with, until};
use super::*;
use crate::models::attachments::{AttachmentContent, AttachmentKind};
use crate::models::sync::{SyncAttachmentsProblem, SyncProblem};
use crate::services::attachment_store;
use crate::services::sync::blobs::content::ContentReader;
use crate::services::sync::blobs::policy::{BlobPolicy, Lane};
use crate::services::sync::blobs::queue;
use crate::services::sync::blobs::transfer::run_uploads;
use crate::services::sync::engine::runtime::{forward_changes, lane_may_run, Trigger};
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode};
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{
    AccountStatus, BlobBody, BlobPage, BlobSink, CheckoutPlan, Delivery, PullPage, PushReceipt,
    RelayResult, ServerInfo, SinkFlow, SnapshotBlob, VaultInfo,
};

/// A blob ID no store holds: an incoming blob a transfer the app quit left behind.
const LEFT_OVER: &str = "5d1c3a9e-7b2f-4e8a-9c6d-0e1f2a3b4c01";

fn desktop() -> BlobPolicy {
    BlobPolicy::for_platform(false)
}

fn phone() -> BlobPolicy {
    BlobPolicy::for_platform(true)
}

/// A vault whose two devices both have their store, with policy `p1` and property `r1` on `A`.
async fn devices() -> Vault {
    let vault = vault().await;
    with_store(&vault.a);
    with_store(&vault.b);
    exec(
        &vault.a.lock(),
        "INSERT INTO insurance_policies \
           (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
         VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly'); \
         INSERT INTO real_estate (id, name, address, type) VALUES ('r1', 'Flat', 'Street', 'flat'); \
         INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
         VALUES ('batch-1', 'r1', 1767225600);",
    );
    vault
}

/// `len` bytes of a pattern that never repeats a 64 KiB chunk.
fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| ((i * 7 + seed as usize) % 251) as u8)
        .collect()
}

/// A document `id` of policy `p1` on `A` with new content `bytes`; its blob ID.
fn document(device: &TestConn, id: &str, bytes: &[u8]) -> String {
    let conn = device.lock();
    let blob = attachment_store::put_bytes(&conn, bytes).expect("blob").id;
    conn.execute(
        "INSERT INTO insurance_documents (id, insurance_id, name, blob_id, uploaded_at) \
         VALUES (?1, 'p1', 'Contract', ?2, 1767225600)",
        [id, &blob],
    )
    .expect("document");
    blob
}

/// A photo `id` of batch `batch-1` on `A` with new content; its `(full, thumbnail)` blob IDs.
fn photo(device: &TestConn, id: &str) -> (String, String) {
    let conn = device.lock();
    let full = attachment_store::put_bytes(&conn, &pattern(120_000, 3))
        .expect("full")
        .id;
    let thumb = attachment_store::put_bytes(&conn, &pattern(4_000, 4))
        .expect("thumbnail")
        .id;
    conn.execute(
        "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id, created_at) \
         VALUES (?1, 'batch-1', ?2, ?3, 1767225600)",
        [id, &full, &thumb],
    )
    .expect("photo");
    (full, thumb)
}

async fn round_on(relay: &impl Relay, device: &TestConn) {
    run_round(
        relay,
        device,
        &mut SnapshotPolicy::default(),
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("round");
}

/// `A` sends what it holds (its round uploads content up to 8 MiB before its push), `B` merges
/// the rows: their content is on the server and not on `B`.
async fn rounds_on_both(vault: &Vault) {
    round_on(&vault.relay, &vault.a).await;
    round_on(&vault.relay, &vault.b).await;
}

/// A document of `A` whose content is on the server and whose row `B` merged; its blob ID.
async fn shared_document(vault: &Vault, id: &str, bytes: &[u8]) -> String {
    let blob = document(&vault.a, id, bytes);
    rounds_on_both(vault).await;
    assert_eq!(state_of(&vault.b, &blob).as_deref(), Some("down"));
    blob
}

fn content(device: &TestConn, blob: &str) -> AttachmentContent {
    let conn = device.lock();
    ContentReader::new(&conn)
        .expect("reader")
        .of(Some(blob), true)
        .expect("content")
}

/// The content of `blob` is complete in `device`'s store.
fn stored(device: &TestConn, blob: &str) -> bool {
    attachment_store::is_complete(&device.lock(), blob).expect("store")
}

/// Incoming (incomplete) blobs in `device`'s store.
fn incoming(device: &TestConn) -> i64 {
    count(
        &device.lock(),
        "SELECT COUNT(*) FROM files.blobs WHERE sha256 = ''",
    )
}

fn state_of(device: &TestConn, blob: &str) -> Option<String> {
    entry(device, blob).map(|(state, ..)| state)
}

/// `(state, attempts, active, last_error)` of a queue entry.
fn entry(device: &TestConn, blob: &str) -> Option<(String, i64, bool, Option<String>)> {
    device
        .lock()
        .query_row(
            "SELECT state, attempts, active, last_error FROM sync_blobs WHERE blob_id = ?1",
            [blob],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()
}

fn active_entries(device: &TestConn) -> i64 {
    count(
        &device.lock(),
        "SELECT COUNT(*) FROM sync_blobs WHERE active = 1",
    )
}

fn blob_calls(routes: &[Route]) -> Vec<Route> {
    routes
        .iter()
        .copied()
        .filter(|r| {
            matches!(
                r,
                Route::PutBlob | Route::HeadBlob | Route::GetBlob | Route::ServerInfo
            )
        })
        .collect()
}

/// Lets the engine's tasks run for a while (real time): long enough for a woken lane to run.
async fn settle_a_moment() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

// ---------------------------------------------------------------------------------------------
// The running engine moves content and tells the app

/// A row whose content is on the server: the running engine of the device that merged it fetches
/// it by itself, tells the app, and its status counts no content elsewhere any more.
#[tokio::test]
async fn the_running_engine_tells_the_app_when_content_arrives() {
    let vault = devices().await;
    let bytes = pattern(150_000, 1);
    let blob = shared_document(&vault, "d1", &bytes).await;
    assert_eq!(content(&vault.b, &blob), AttachmentContent::NotHere);

    let running = start_with(vault.relay.clone(), vault.b.clone(), desktop());

    until("the content arrived", || {
        content(&vault.b, &blob) == AttachmentContent::Here
    })
    .await;
    until("the app was told", || {
        running.seen.attachments_changed() >= 1
    })
    .await;
    until("the status counts it as here", || {
        running.seen.last().attachments.not_here == 0
    })
    .await;
    assert_eq!(
        attachment_store::read_all(&vault.b.lock(), &blob).expect("read"),
        bytes
    );
}

/// Content larger than the round sends before its push goes up in the upload lane.
#[tokio::test]
async fn content_the_round_leaves_goes_up_in_the_lane() {
    let vault = devices().await;
    let blob = document(&vault.a, "d1", &pattern(9 * 1024 * 1024, 2));

    let running = start_with(vault.relay.clone(), vault.a.clone(), desktop());

    until("the lane uploaded it", || {
        state_of(&vault.a, &blob).as_deref() == Some("done")
    })
    .await;
    rounds(&running, 1).await;
    assert_eq!(
        vault.relay.blob_ids(&vault.creds.vault_id.to_b64()),
        vec![uuid::Uuid::parse_str(&blob).expect("id")]
    );
}

// ---------------------------------------------------------------------------------------------
// Starting the lanes

/// An incoming blob a transfer the app quit left behind is swept when the lanes start, before
/// either lane transfers, and only then: one that appears later is left to the transfer that
/// writes it, while the lanes go on running.
#[tokio::test]
async fn a_lane_start_sweeps_incomplete_blobs() {
    let vault = devices().await;
    attachment_store::begin_incoming(&vault.b.lock(), LEFT_OVER, 10).expect("left over");
    let first = shared_document(&vault, "d1", &pattern(20_000, 5)).await;
    assert_eq!(incoming(&vault.b), 1);

    let running = start_with(vault.relay.clone(), vault.b.clone(), desktop());
    until("the first content arrived", || stored(&vault.b, &first)).await;
    assert_eq!(incoming(&vault.b), 0, "swept at the lanes' start");

    // Later: an incoming blob of a live transfer (here: written by the test) is never swept by
    // a lane run.
    let live = "5d1c3a9e-7b2f-4e8a-9c6d-0e1f2a3b4c02";
    attachment_store::begin_incoming(&vault.b.lock(), live, 10).expect("live");
    let second = document(&vault.a, "d2", &pattern(20_000, 6));
    round_on(&vault.relay, &vault.a).await;
    running.handle.wake(Trigger::SyncNow);
    until("the second content arrived", || stored(&vault.b, &second)).await;
    assert_eq!(incoming(&vault.b), 1, "not swept again");
}

/// The lanes' start reconciles the whole queue with the store: content lost outside a merge (an
/// entry `done` without its blob) is found missing and fetched again.
#[tokio::test]
async fn a_lane_start_reconciles_the_queue_with_the_store() {
    let vault = devices().await;
    let bytes = pattern(30_000, 7);
    let blob = shared_document(&vault, "d1", &bytes).await;
    crate::services::sync::blobs::transfer::run_downloads(
        &vault.relay,
        &vault.b,
        &vault.creds,
        &desktop(),
        NOW,
        None,
    )
    .await
    .expect("downloads");
    assert_eq!(state_of(&vault.b, &blob).as_deref(), Some("done"));
    attachment_store::delete(&vault.b.lock(), &blob).expect("the store loses it");

    let _running = start_with(vault.relay.clone(), vault.b.clone(), desktop());

    until("fetched again", || stored(&vault.b, &blob)).await;
    assert_eq!(
        attachment_store::read_all(&vault.b.lock(), &blob).expect("read"),
        bytes
    );
    assert_eq!(state_of(&vault.b, &blob).as_deref(), Some("done"));
}

// ---------------------------------------------------------------------------------------------
// Stopping and restarting

/// The fake sync server, except that a download begins (its incoming blob is reserved) and then
/// never delivers.
struct StalledDownloads {
    inner: FakeRelay,
}

/// A download that stops in its first piece until the test opens the gate, holding its worker
/// thread as a long store write would: its task cannot be cancelled meanwhile.
struct HeldDownloads {
    inner: FakeRelay,
    gate: Arc<Gate>,
}

#[derive(Default)]
struct Gate {
    state: Mutex<(bool, bool)>,
    opened: Condvar,
}

impl Gate {
    /// A transfer reached the gate.
    fn reached(&self) -> bool {
        self.state.lock().expect("gate").0
    }

    fn open(&self) {
        self.state.lock().expect("gate").1 = true;
        self.opened.notify_all();
    }

    fn pass(&self) {
        let mut state = self.state.lock().expect("gate");
        state.0 = true;
        while !state.1 {
            state = self.opened.wait(state).expect("gate");
        }
    }
}

/// Opens the gate when dropped, so a failed assertion never leaves a worker blocked in it (the
/// test's runtime could not shut down).
struct OpenOnDrop(Arc<Gate>);

impl Drop for OpenOnDrop {
    fn drop(&mut self) {
        self.0.open();
    }
}

struct HeldSink<'a> {
    inner: &'a mut (dyn BlobSink + Send),
    gate: Arc<Gate>,
}

impl BlobSink for HeldSink<'_> {
    fn begin(&mut self, sealed_len: u64) -> SinkFlow {
        self.inner.begin(sealed_len)
    }

    fn piece(&mut self, bytes: &[u8]) -> SinkFlow {
        tokio::task::block_in_place(|| self.gate.pass());
        self.inner.piece(bytes)
    }
}

/// Every route of `Relay` but `get_blob` goes to the fake.
macro_rules! relay_but_get_blob {
    ($ty:ty, $inner:ident, |$self_:ident, $vault:ident, $blob:ident, $sink:ident| $get:block) => {
        impl Relay for $ty {
            async fn server_info(&self) -> RelayResult<ServerInfo> {
                self.$inner.server_info().await
            }
            async fn create_account(&self) -> RelayResult<String> {
                self.$inner.create_account().await
            }
            async fn account_status(&self, account: &str) -> RelayResult<AccountStatus> {
                self.$inner.account_status(account).await
            }
            async fn checkout(&self, account: &str, plan: CheckoutPlan) -> RelayResult<String> {
                self.$inner.checkout(account, plan).await
            }
            async fn create_vault(&self, account: &str, vault: &VaultAuth) -> RelayResult<bool> {
                self.$inner.create_vault(account, vault).await
            }
            async fn vault_info(&self, vault: &VaultAuth) -> RelayResult<VaultInfo> {
                self.$inner.vault_info(vault).await
            }
            async fn push(
                &self,
                vault: &VaultAuth,
                ops: &[OutgoingOp],
            ) -> RelayResult<PushReceipt> {
                self.$inner.push(vault, ops).await
            }
            async fn pull(
                &self,
                vault: &VaultAuth,
                after: u64,
                limit: u32,
            ) -> RelayResult<PullPage> {
                self.$inner.pull(vault, after, limit).await
            }
            async fn put_snapshot(
                &self,
                vault: &VaultAuth,
                seq: u64,
                bytes: Vec<u8>,
            ) -> RelayResult<()> {
                self.$inner.put_snapshot(vault, seq, bytes).await
            }
            async fn get_snapshot(&self, vault: &VaultAuth) -> RelayResult<SnapshotBlob> {
                self.$inner.get_snapshot(vault).await
            }
            async fn delete_vault(&self, vault: &VaultAuth) -> RelayResult<()> {
                self.$inner.delete_vault(vault).await
            }
            async fn put_blob(
                &self,
                vault: &VaultAuth,
                blob: uuid::Uuid,
                sealed_len: u64,
                body: BlobBody,
            ) -> RelayResult<()> {
                self.$inner.put_blob(vault, blob, sealed_len, body).await
            }
            async fn head_blob(
                &self,
                vault: &VaultAuth,
                blob: uuid::Uuid,
            ) -> RelayResult<Option<u64>> {
                self.$inner.head_blob(vault, blob).await
            }
            async fn get_blob(
                &$self_,
                $vault: &VaultAuth,
                $blob: uuid::Uuid,
                $sink: &mut (dyn BlobSink + Send),
            ) -> RelayResult<Delivery> $get
            async fn list_blobs(
                &self,
                vault: &VaultAuth,
                after: Option<uuid::Uuid>,
                limit: u32,
            ) -> RelayResult<BlobPage> {
                self.$inner.list_blobs(vault, after, limit).await
            }
            async fn delete_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<()> {
                self.$inner.delete_blob(vault, blob).await
            }
        }
    };
}

relay_but_get_blob!(StalledDownloads, inner, |self, vault, blob, sink| {
    let sealed = self
        .inner
        .head_blob(vault, blob)
        .await?
        .expect("on the server");
    sink.begin(sealed);
    std::future::pending().await
});

relay_but_get_blob!(HeldDownloads, inner, |self, vault, blob, sink| {
    let mut held = HeldSink {
        inner: sink,
        gate: self.gate.clone(),
    };
    self.inner.get_blob(vault, blob, &mut held).await
});

/// Stopping the engine with a download in flight ends the lanes with it: the transfer is dropped
/// cleanly (its incoming blob abandoned, its entry a failed attempt of the network's kind and no
/// longer active), and nothing of the engine asks the server any more.
#[tokio::test]
async fn stopping_the_engine_stops_the_lanes() {
    let vault = devices().await;
    let blob = shared_document(&vault, "d1", &pattern(50_000, 8)).await;
    let running = start_with(
        StalledDownloads {
            inner: vault.relay.clone(),
        },
        vault.b.clone(),
        desktop(),
    );
    until("the download is in flight", || incoming(&vault.b) == 1).await;
    let seen = running.seen.clone();

    running.handle.stopped().await;

    assert_eq!(incoming(&vault.b), 0, "the incoming blob was abandoned");
    let (state, attempts, active, error) = entry(&vault.b, &blob).expect("entry");
    assert_eq!(
        (state.as_str(), attempts, active, error.as_deref()),
        ("down", 1, false, Some("network"))
    );
    let calls = vault.relay.calls().len();
    settle_a_moment().await;
    assert_eq!(vault.relay.calls().len(), calls, "nothing asks any more");
    drop(seen);
}

/// A locked profile ends the lanes (and the loop) at their next look at the connection.
#[tokio::test]
async fn a_locked_profile_ends_the_lanes() {
    let vault = devices().await;
    let running = start_with(vault.relay.clone(), vault.b.clone(), desktop());
    rounds(&running, 1).await;

    vault.b.locked.store(true, Ordering::SeqCst);
    running.handle.wake_transfers();
    running.handle.wake(Trigger::SyncNow);

    until("the engine ended", || !running.handle.is_running()).await;
}

/// One engine's lanes at a time: an engine started again (a lock and unlock, a sync change)
/// while the old one's download is in flight waits until the old lanes have ended before its own
/// start (the sweep would otherwise remove the incoming blob being written); then it goes on, and
/// nothing is left half-written or marked active.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restart_waits_for_the_old_lanes_to_end() {
    let vault = devices().await;
    let bytes = pattern(300_000, 9);
    let blob = shared_document(&vault, "d1", &bytes).await;
    let gate = Arc::new(Gate::default());
    let _opens = OpenOnDrop(gate.clone());
    let old = start_with(
        HeldDownloads {
            inner: vault.relay.clone(),
            gate: gate.clone(),
        },
        vault.b.clone(),
        desktop(),
    );
    until("the old engine's download is in flight", || gate.reached()).await;
    assert_eq!(incoming(&vault.b), 1);

    old.handle.stop();
    let new = start_with(vault.relay.clone(), vault.b.clone(), desktop());
    rounds(&new, 1).await;
    new.handle.wake_transfers();
    settle_a_moment().await;

    assert!(
        old.handle.is_running(),
        "the old lanes are still in their transfer"
    );
    assert_eq!(
        incoming(&vault.b),
        1,
        "the new lanes did not start beside the old ones"
    );

    gate.open();
    until("the old engine ended", || !old.handle.is_running()).await;
    new.handle.wake_transfers();
    until("the content is here", || stored(&vault.b, &blob)).await;
    until("the entry settled", || {
        state_of(&vault.b, &blob).as_deref() == Some("done")
    })
    .await;
    assert_eq!(
        attachment_store::read_all(&vault.b.lock(), &blob).expect("read"),
        bytes
    );
    assert_eq!(incoming(&vault.b), 0);
    assert_eq!(active_entries(&vault.b), 0);
}

// ---------------------------------------------------------------------------------------------
// "Sync now"

/// A server without room pauses uploads for half an hour and the entry waits for its backoff; once
/// there is room, "Sync now" asks again at once: the pause ends, the entry is due, the lane
/// uploads it, and the status no longer shows the problem.
#[tokio::test]
async fn sync_now_tries_transfers_again_at_once() {
    let vault = devices().await;
    vault.relay.set_no_room(true);
    let blob = document(&vault.a, "d1", &pattern(40_000, 10));
    let running = start_with(vault.relay.clone(), vault.a.clone(), desktop());
    rounds(&running, 1).await;
    until("the problem is shown", || {
        running.seen.last().attachments.problem == Some(SyncAttachmentsProblem::NoRoom)
    })
    .await;
    vault.relay.set_no_room(false);

    running.handle.wake_transfers();
    settle_a_moment().await;
    assert_eq!(
        state_of(&vault.a, &blob).as_deref(),
        Some("up"),
        "paused, and waiting for its backoff"
    );

    running.handle.wake(Trigger::SyncNow);

    until("the lane uploaded it", || {
        state_of(&vault.a, &blob).as_deref() == Some("done")
    })
    .await;
    until("the problem is gone", || {
        running.seen.last().attachments.problem.is_none()
    })
    .await;
}

// ---------------------------------------------------------------------------------------------
// Lanes and rounds

/// The round's upload step and the lanes share the engine's policy (here a phone's): while its
/// upload lane is held, the round sends no content (and still pushes); once free, the lane
/// uploads on the phone as on a desktop.
#[tokio::test]
async fn the_round_and_the_lanes_share_one_policy() {
    let vault = devices().await;
    let blob = document(&vault.a, "d1", &pattern(40_000, 11));
    let policy = phone();
    let held = policy.up_lane().lock().await;

    let running = start_with(vault.relay.clone(), vault.a.clone(), policy.clone());
    rounds(&running, 1).await;

    assert!(!vault.relay.ops().is_empty(), "the round pushed");
    assert!(!vault.relay.calls().contains(&Route::PutBlob));
    assert_eq!(state_of(&vault.a, &blob).as_deref(), Some("up"));

    drop(held);
    running.handle.wake_transfers();
    until("the lane uploaded it", || {
        state_of(&vault.a, &blob).as_deref() == Some("done")
    })
    .await;
}

/// The fake sync server, except that every upload takes 25 seconds; counts uploads and the most
/// in flight at once.
struct SlowPuts {
    inner: FakeRelay,
    in_flight: Arc<AtomicUsize>,
    most: AtomicUsize,
    puts: AtomicUsize,
}

/// Counts an upload in flight until dropped (it ended, or was cut).
struct InFlight(Arc<AtomicUsize>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Relay for SlowPuts {
    async fn server_info(&self) -> RelayResult<ServerInfo> {
        self.inner.server_info().await
    }
    async fn create_account(&self) -> RelayResult<String> {
        self.inner.create_account().await
    }
    async fn account_status(&self, account: &str) -> RelayResult<AccountStatus> {
        self.inner.account_status(account).await
    }
    async fn checkout(&self, account: &str, plan: CheckoutPlan) -> RelayResult<String> {
        self.inner.checkout(account, plan).await
    }
    async fn create_vault(&self, account: &str, vault: &VaultAuth) -> RelayResult<bool> {
        self.inner.create_vault(account, vault).await
    }
    async fn vault_info(&self, vault: &VaultAuth) -> RelayResult<VaultInfo> {
        self.inner.vault_info(vault).await
    }
    async fn push(&self, vault: &VaultAuth, ops: &[OutgoingOp]) -> RelayResult<PushReceipt> {
        self.inner.push(vault, ops).await
    }
    async fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> RelayResult<PullPage> {
        self.inner.pull(vault, after, limit).await
    }
    async fn put_snapshot(&self, vault: &VaultAuth, seq: u64, bytes: Vec<u8>) -> RelayResult<()> {
        self.inner.put_snapshot(vault, seq, bytes).await
    }
    async fn get_snapshot(&self, vault: &VaultAuth) -> RelayResult<SnapshotBlob> {
        self.inner.get_snapshot(vault).await
    }
    async fn delete_vault(&self, vault: &VaultAuth) -> RelayResult<()> {
        self.inner.delete_vault(vault).await
    }
    async fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> RelayResult<()> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        let now = self.in_flight.fetch_add(1, Ordering::SeqCst) + 1;
        let _counted = InFlight(self.in_flight.clone());
        self.most.fetch_max(now, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(25)).await;
        self.inner.put_blob(vault, blob, sealed_len, body).await
    }
    async fn head_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<Option<u64>> {
        self.inner.head_blob(vault, blob).await
    }
    async fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> RelayResult<Delivery> {
        self.inner.get_blob(vault, blob, sink).await
    }
    async fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<uuid::Uuid>,
        limit: u32,
    ) -> RelayResult<BlobPage> {
        self.inner.list_blobs(vault, after, limit).await
    }
    async fn delete_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<()> {
        self.inner.delete_blob(vault, blob).await
    }
}

/// The upload lane never starts a blob the round is sending: it waits for the lane lock the
/// round's step holds, and takes the blob only once the round's budget cut it (and its backoff
/// ran out). Never two uploads in flight.
#[tokio::test(start_paused = true)]
async fn the_upload_lane_never_sends_a_blob_the_round_is_sending() {
    let vault = devices().await;
    let blob = document(&vault.a, "d1", &pattern(40_000, 12));
    let relay = SlowPuts {
        inner: vault.relay.clone(),
        in_flight: Arc::default(),
        most: AtomicUsize::new(0),
        puts: AtomicUsize::new(0),
    };
    let mut policy = SnapshotPolicy::default();
    let lanes = policy.blobs.clone();

    let round = run_round(&relay, &vault.a, &mut policy, RoundMode::Normal, NOW, 0);
    let lane = async {
        tokio::time::sleep(Duration::from_secs(1)).await;
        run_uploads(
            &relay,
            &vault.a,
            &vault.creds,
            &lanes,
            NOW + 100,
            None,
            None,
        )
        .await
    };
    let (round, lane) = tokio::join!(round, lane);

    round.expect("round");
    assert_eq!(lane.expect("lane").uploaded, 1);
    assert_eq!(
        relay.puts.load(Ordering::SeqCst),
        2,
        "the round's, then the lane's"
    );
    assert_eq!(relay.most.load(Ordering::SeqCst), 1, "never two at once");
    assert_eq!(state_of(&vault.a, &blob).as_deref(), Some("done"));
}

// ---------------------------------------------------------------------------------------------
// Phone and desktop

/// A photo and a document of `A`, on the server, merged by `B`: `(full, thumbnail, document)`.
async fn shared_photo_and_document(vault: &Vault) -> (String, String, String) {
    let (full, thumb) = photo(&vault.a, "ph1");
    let doc = document(&vault.a, "d1", &pattern(30_000, 13));
    rounds_on_both(vault).await;
    (full, thumb, doc)
}

/// A phone fetches thumbnails at once and full content only when asked (opening a photo).
#[tokio::test]
async fn a_phone_fetches_thumbnails_at_once_and_full_content_when_asked() {
    let vault = devices().await;
    let (full, thumb, doc) = shared_photo_and_document(&vault).await;

    let running = start_with(vault.relay.clone(), vault.b.clone(), phone());
    until("the thumbnail arrived", || stored(&vault.b, &thumb)).await;
    running.handle.wake_transfers();
    settle_a_moment().await;
    assert!(
        !stored(&vault.b, &full),
        "the full photo waits to be asked for"
    );
    assert!(!stored(&vault.b, &doc), "so does the document");

    queue::want_row(&vault.b.lock(), AttachmentKind::Photo, "ph1").expect("opened");
    running.handle.wake_transfers();

    until("the opened photo arrived", || stored(&vault.b, &full)).await;
    settle_a_moment().await;
    assert!(!stored(&vault.b, &doc), "nothing else came with it");
}

/// A desktop fetches everything in the background.
#[tokio::test]
async fn a_desktop_fetches_everything_in_the_background() {
    let vault = devices().await;
    let (full, thumb, doc) = shared_photo_and_document(&vault).await;

    let _running = start_with(vault.relay.clone(), vault.b.clone(), desktop());

    until("everything arrived", || {
        [&full, &thumb, &doc]
            .iter()
            .all(|blob| stored(&vault.b, blob))
    })
    .await;
}

/// The platform's policy: a phone fetches full content on demand, a desktop eagerly; this build
/// (the tests run on a desktop) is a desktop.
#[test]
fn the_platform_decides_what_is_fetched_eagerly() {
    assert!(!BlobPolicy::for_platform(true).eager_full());
    assert!(BlobPolicy::for_platform(false).eager_full());
    assert_eq!(
        BlobPolicy::for_this_platform().eager_full(),
        !cfg!(any(target_os = "ios", target_os = "android"))
    );
}

// ---------------------------------------------------------------------------------------------
// Telling the app

/// A burst of changes (one every 10 ms for a second) is told a few times, never more than once
/// per gap, and the last change is always told.
#[tokio::test(start_paused = true)]
async fn a_burst_of_changes_is_told_a_few_times_and_the_last_one_always() {
    let changes = Arc::new(tokio::sync::Notify::new());
    let told = Arc::new(Mutex::new(Vec::new()));
    let gap = Duration::from_millis(250);
    let forward = {
        let (changes, told) = (changes.clone(), told.clone());
        tokio::spawn(async move {
            forward_changes(&changes, gap, || {
                told.lock().expect("told").push(tokio::time::Instant::now());
            })
            .await;
        })
    };
    let start = tokio::time::Instant::now();
    let mut last_change = start;
    for _ in 0..100 {
        changes.notify_one();
        last_change = tokio::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    tokio::time::sleep(Duration::from_secs(2)).await;
    forward.abort();

    let told = told.lock().expect("told").clone();
    assert!(
        (2..=6).contains(&told.len()),
        "{} events for 100 changes",
        told.len()
    );
    assert!(told.windows(2).all(|w| w[1] - w[0] >= gap), "{told:?}");
    assert!(
        told.last().is_some_and(|at| *at >= last_change),
        "the last change was told"
    );
}

// ---------------------------------------------------------------------------------------------
// Where the round does not sync

/// Capture broken or the vault gone: the round does not sync, and neither lane asks the server.
#[tokio::test]
async fn the_lanes_rest_while_capture_is_broken_or_the_vault_is_gone() {
    for vault_gone in [false, true] {
        let vault = devices().await;
        let blob = shared_document(&vault, "d1", &pattern(20_000, 14)).await;
        {
            let b = vault.b.lock();
            if vault_gone {
                state::set_vault_gone(&b, Some(NOW)).expect("vault gone");
            } else {
                state::mark_capture_broken(&b, NOW).expect("capture broken");
            }
        }
        let from = vault.relay.calls().len();

        let running = start_with(vault.relay.clone(), vault.b.clone(), desktop());
        rounds(&running, 1).await;
        running.handle.wake_transfers();
        settle_a_moment().await;

        assert_eq!(
            blob_calls(&calls_since(&vault.relay, from)),
            Vec::<Route>::new(),
            "vault gone: {vault_gone}"
        );
        assert!(!stored(&vault.b, &blob));
    }
}

/// While the round backs off after a failure (here the server out of reach), the lanes rest too;
/// the next round that runs ("Sync now" here) wakes them and they fetch.
#[tokio::test]
async fn the_lanes_rest_while_the_round_backs_off() {
    let vault = devices().await;
    let blob = shared_document(&vault, "d1", &pattern(20_000, 17)).await;
    vault.relay.fail_next(
        Route::Pull,
        crate::services::sync::relay::RelayError::Network("connect".into()),
    );
    let from = vault.relay.calls().len();

    let running = start_with(vault.relay.clone(), vault.b.clone(), desktop());
    rounds(&running, 1).await;
    assert_eq!(running.handle.runtime().problem, Some(SyncProblem::Offline));
    running.handle.wake_transfers();
    settle_a_moment().await;
    assert_eq!(
        blob_calls(&calls_since(&vault.relay, from)),
        Vec::<Route>::new()
    );

    running.handle.wake(Trigger::SyncNow);
    until("fetched after the round", || stored(&vault.b, &blob)).await;
}

/// A lapsed account: like the round's pulls, downloads go on while the server allows reads; like
/// its pushes, uploads wait (content over the round's limit, so only a lane would send it).
#[tokio::test]
async fn a_lapsed_account_downloads_but_does_not_upload() {
    let vault = devices().await;
    let shared = shared_document(&vault, "d1", &pattern(20_000, 15)).await;
    vault.relay.set_paid(&vault.creds.account, false);
    let waiting = document(&vault.a, "d2", &pattern(9 * 1024 * 1024, 16));
    let from = vault.relay.calls().len();

    let on_a = start_with(vault.relay.clone(), vault.a.clone(), desktop());
    let on_b = start_with(vault.relay.clone(), vault.b.clone(), desktop());
    rounds(&on_a, 1).await;
    assert_eq!(on_a.handle.runtime().problem, Some(SyncProblem::Lapsed));

    until("B fetched its content", || stored(&vault.b, &shared)).await;
    on_a.handle.wake_transfers();
    settle_a_moment().await;
    rounds(&on_b, 1).await;

    let calls = calls_since(&vault.relay, from);
    assert!(!calls.contains(&Route::PutBlob), "{calls:?}");
    assert!(!calls.contains(&Route::HeadBlob), "{calls:?}");
    let (state, attempts, ..) = entry(&vault.a, &waiting).expect("entry");
    assert_eq!((state.as_str(), attempts), ("up", 0));
}

/// Which lane runs after a round with which problem.
#[test]
fn each_lane_runs_where_the_round_would() {
    use SyncProblem as P;
    for problem in [P::CaptureBroken, P::VaultGone, P::Unauthorized] {
        assert!(!lane_may_run(Lane::Up, Some(problem)), "{problem:?}");
        assert!(!lane_may_run(Lane::Down, Some(problem)), "{problem:?}");
    }
    for problem in [P::Lapsed, P::KeyRotated] {
        assert!(!lane_may_run(Lane::Up, Some(problem)), "{problem:?}");
        assert!(lane_may_run(Lane::Down, Some(problem)), "{problem:?}");
    }
    for problem in [
        None,
        Some(P::Offline),
        Some(P::RateLimited),
        Some(P::RelayError),
        Some(P::QuotaExceeded),
        Some(P::Gap),
        Some(P::ServerWentBack),
    ] {
        assert!(lane_may_run(Lane::Up, problem), "{problem:?}");
        assert!(lane_may_run(Lane::Down, problem), "{problem:?}");
    }
}
