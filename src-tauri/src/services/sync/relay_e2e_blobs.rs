//! End to end: documents and photos (attachments design §11, ADR 0030) between devices through a
//! real Moony sync server with the blob routes (Moony-relay, `features: ["blobs"]`). Every test is
//! ignored; run them one at a time, against a fresh data folder (the admin login is
//! attempt-limited, the tests share the server, and one of them changes the server-wide blob
//! quota for a moment):
//!
//! ```text
//! # In Moony-relay (built with `npm run build`), on a port nothing else uses:
//! PORT=8797 SETUP_CODE=moony-e2e-setup-code DATA_DIR=$(mktemp -d) npm start
//! cd src-tauri && MOONY_RELAY_E2E=http://127.0.0.1:8797 \
//!   cargo test --lib relay_e2e_blobs -- --ignored --test-threads=1
//! ```
//!
//! The server keeps 1 GiB of its disk free (`507 server_full` below that). Each test has its own
//! person and vault (`relay_e2e_admin`). Devices are fresh migrated profiles behind a `TestConn`
//! with an attachment store attached; each keeps one `BlobPolicy` for its rounds and its lanes, as
//! one running engine shares it, and drives them by hand: a round, then the upload lane ("sync"),
//! and the download lane. Documents and photos are added, read and deleted through the services
//! the app's commands call.

use std::io::Cursor;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
use reqwest::{Method, StatusCode};
use uuid::Uuid;

use super::blobs::pass::{self, Limits, PASS_AT_KEY};
use super::blobs::policy::{BlobPolicy, ServerBlobs};
use super::blobs::queue::{self, TransferError};
use super::blobs::transfer::{run_downloads, run_uploads, upload_one, Outcome, TransferReport};
use super::engine::snapshots::SnapshotPolicy;
use super::engine::status::{self, Runtime};
use super::engine::tests::with_store;
use super::engine::{run_round, RoundMode, RoundReport};
use super::envelope::blob::{sealed_len, BlobSealer};
use super::keys::{Credentials, VaultAuth};
use super::relay::http::HttpRelay;
use super::relay::{
    AccountStatus, BlobBody, BlobPage, BlobSink, CheckoutPlan, Delivery, OutgoingOp, PullPage,
    PushReceipt, Relay, RelayError, RelayResult, ServerInfo, SinkFlow, SnapshotBlob, VaultInfo,
    MIN_BLOB_SEALED,
};
use super::relay_e2e::{first_device, now, relay_at, relay_url, until_allowed, Device};
use super::relay_e2e_admin::admin;
use crate::models::attachments::{AttachmentContent, AttachmentKind};
use crate::models::real_estate::RealEstatePhoto;
use crate::models::sync::{SyncAttachments, SyncAttachmentsProblem};
use crate::services::attachments::{self, NewDocument, Variant};
use crate::services::real_estate_photos;

/// The plaintext chunk of a sealed blob stream (attachments design §5.2).
const CHUNK: usize = 64 * 1024;

/// A policy of this device's platform kind whose answer about the server's blob routes is in:
/// `Yes`, asked again after a pause while the server limits how often one client asks
/// (`GET /v1/server` counts as an account lookup, 60 a minute per address).
async fn ready_policy(relay: &HttpRelay, phone: bool) -> BlobPolicy {
    let policy = BlobPolicy::for_platform(phone);
    for _ in 0..40 {
        match policy.server_has_blobs(relay, now()).await {
            ServerBlobs::Yes => return policy,
            ServerBlobs::No => panic!("the sync server does not list the blobs feature"),
            ServerBlobs::Unknown => tokio::time::sleep(Duration::from_secs(3)).await,
        }
    }
    panic!("the sync server never said whether it has the blob routes");
}

/// One device as its running engine sees it: the profile with its attachment store, and one
/// `SnapshotPolicy` whose `BlobPolicy` its rounds and lanes share.
struct Node {
    device: Device,
    engine: SnapshotPolicy,
}

impl Node {
    /// `device` with an attachment store attached (before a join, so the replace keeps it) and
    /// the transfer policy of a phone or a desktop.
    async fn new(device: Device, relay: &HttpRelay, phone: bool) -> Self {
        with_store(&device.db);
        let mut engine = SnapshotPolicy::default();
        engine.blobs = ready_policy(relay, phone).await;
        Node { device, engine }
    }

    fn policy(&self) -> &BlobPolicy {
        &self.engine.blobs
    }

    fn creds(&self) -> Credentials {
        self.device.credentials()
    }

    /// One round at the real time in `mode`; an error or a problem fails the test.
    async fn round_in(&mut self, relay: &HttpRelay, mode: RoundMode) -> RoundReport {
        let report = run_round(relay, &self.device.db, &mut self.engine, mode, now(), 0)
            .await
            .expect("round");
        assert_eq!(report.problem, None, "{report:?}");
        report
    }

    async fn round(&mut self, relay: &HttpRelay) -> RoundReport {
        self.round_in(relay, RoundMode::Normal).await
    }

    /// The upload lane's run.
    async fn uploads(&self, relay: &impl Relay) -> TransferReport {
        run_uploads(
            relay,
            &self.device.db,
            &self.creds(),
            self.policy(),
            now(),
            None,
            None,
        )
        .await
        .expect("uploads")
    }

    /// The download lane's run under this device's policy.
    async fn downloads(&self, relay: &HttpRelay) -> TransferReport {
        self.downloads_with(relay, self.policy()).await
    }

    async fn downloads_with(&self, relay: &HttpRelay, policy: &BlobPolicy) -> TransferReport {
        run_downloads(relay, &self.device.db, &self.creds(), policy, now(), None)
            .await
            .expect("downloads")
    }

    /// What the running engine does after a change: a round (content up to 8 MiB goes up before
    /// its push), then the upload lane for the rest.
    async fn sync(&mut self, relay: &HttpRelay) -> RoundReport {
        let report = self.round(relay).await;
        self.uploads(relay).await;
        report
    }

    /// Insurance policy `p1`, property `r1` and its photo batch `batch-1`.
    fn owners(&self) {
        self.device.exec(
            "INSERT INTO insurance_policies \
               (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
             VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly'); \
             INSERT INTO real_estate (id, name, address, type) \
             VALUES ('r1', 'Flat', 'Street', 'flat'); \
             INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
             VALUES ('batch-1', 'r1', 1767225600);",
        );
    }

    /// A document of policy `p1` added as the app adds one from its bytes; its row and blob IDs.
    fn add_document(&self, name: &str, bytes: &[u8]) -> (String, String) {
        let conn = self.device.db.lock();
        let doc = NewDocument {
            name,
            description: None,
            file_type: "contract",
        };
        let added = attachments::add_insurance_document_bytes(
            &conn,
            "p1",
            &format!("{name}.pdf"),
            bytes,
            &doc,
            now(),
        )
        .expect("add the document");
        let blob: String = conn
            .query_row(
                "SELECT blob_id FROM insurance_documents WHERE id = ?1",
                [&added.id],
                |r| r.get(0),
            )
            .expect("its blob");
        (added.id, blob)
    }

    /// A photo of batch `batch-1` added as the app adds one (decoded, encoded, stored); its row ID.
    fn add_photo(&self, seed: u8) -> String {
        let prepared = real_estate_photos::prepare_photo(&png(seed)).expect("prepared");
        real_estate_photos::record_photo(&self.device.db.lock(), "batch-1", &prepared, now())
            .expect("photo")
            .id
    }

    fn delete_document(&self, id: &str) {
        attachments::delete_document(
            &self.device.db.lock(),
            self.device.dir(),
            AttachmentKind::InsuranceDocument,
            id,
        )
        .expect("delete the document");
    }

    /// The bytes of an attachment, read as the app opens it.
    fn read(&self, kind: AttachmentKind, id: &str, variant: Variant) -> Vec<u8> {
        let conn = self.device.db.lock();
        let resolved = attachments::resolve(&conn, self.device.dir(), kind, id, variant)
            .expect("resolve")
            .expect("a row");
        attachments::read_bytes(&conn, &resolved).expect("read")
    }

    fn document(&self, id: &str) -> Vec<u8> {
        self.read(AttachmentKind::InsuranceDocument, id, Variant::Full)
    }

    /// Where a document's content is, as its listing shows it; `None` without the row.
    fn document_content(&self, id: &str) -> Option<AttachmentContent> {
        attachments::list_insurance_documents(&self.device.db.lock(), "p1")
            .expect("documents")
            .into_iter()
            .find(|doc| doc.id == id)
            .map(|doc| doc.content)
    }

    fn photos(&self) -> Vec<RealEstatePhoto> {
        real_estate_photos::list_photos(&self.device.db.lock(), "batch-1").expect("photos")
    }

    /// `(state, last_error)` of the queue entry of `blob`.
    fn entry(&self, blob: &str) -> Option<(String, Option<String>)> {
        self.device
            .db
            .lock()
            .query_row(
                "SELECT state, last_error FROM sync_blobs WHERE blob_id = ?1",
                [blob],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    fn state(&self, blob: &str) -> Option<String> {
        self.entry(blob).map(|(state, _)| state)
    }

    /// The documents-and-photos part of the status, with the engine's attachments problem.
    fn attachments(&self) -> SyncAttachments {
        let runtime = Runtime {
            attachments_problem: self.policy().problem(),
            ..Runtime::default()
        };
        status::status(&self.device.db.lock(), &runtime)
            .expect("status")
            .attachments
    }

    /// Forgets when this device last completed the daily pass: it is due again.
    fn pass_due(&self) {
        self.device
            .db
            .lock()
            .execute("DELETE FROM app_config WHERE key = ?1", [PASS_AT_KEY])
            .expect("pass mark");
    }
}

/// `len` bytes that read as a PDF, never repeating a 64 KiB chunk.
fn pdf(len: usize, seed: u8) -> Vec<u8> {
    let mut bytes = b"%PDF-1.7\n".to_vec();
    bytes.extend((bytes.len()..len).map(|i| ((i * 7 + seed as usize) % 251) as u8));
    bytes
}

/// A 320 × 240 PNG of its own pattern.
fn png(seed: u8) -> Vec<u8> {
    let image = RgbImage::from_fn(320, 240, |x, y| {
        Rgb([(x as u8) ^ seed, (y as u8).wrapping_add(seed), seed])
    });
    let mut out = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(image)
        .write_to(&mut out, ImageFormat::Png)
        .expect("encode png");
    out.into_inner()
}

fn blob_id(blob: &str) -> Uuid {
    Uuid::parse_str(blob).expect("a blob ID")
}

/// The vault's blobs on the server (one page; the tests' vaults hold a few).
async fn listed(relay: &HttpRelay, creds: &Credentials) -> Vec<Uuid> {
    let page = relay
        .list_blobs(&creds.auth(), None, 1000)
        .await
        .expect("list");
    assert!(!page.more);
    page.blobs.iter().map(|blob| blob.id).collect()
}

/// A first device (desktop) with sync on and its owners synced, and a second that joined it
/// (`phone`: the second one fetches full content only when asked).
async fn two_devices(relay: &HttpRelay, url: &str, phone: bool) -> (Node, Node) {
    let mut a = Node::new(first_device(relay, url, "A").await, relay, false).await;
    a.owners();
    a.round(relay).await;
    let mut b = Node::new(Device::new(), relay, phone).await;
    b.device.join(relay, a.creds(), "B").await;
    b.round(relay).await;
    a.round(relay).await;
    (a, b)
}

/// `plain` sealed for `blob` of the vault of `creds`, as an upload seals it.
fn sealed(creds: &Credentials, blob: &Uuid, plain: &[u8]) -> Vec<u8> {
    let mut sealer = BlobSealer::new(
        creds.keys.current_epoch(),
        creds.keys.current(),
        &creds.vault_id.0,
        blob,
    );
    let mut out = sealer.header().to_vec();
    let (chunks, remainder) = plain.as_chunks::<CHUNK>();
    for chunk in chunks {
        out.extend(sealer.seal_chunk(chunk, false).expect("seal"));
    }
    out.extend(sealer.seal_chunk(remainder, true).expect("seal"));
    assert_eq!(out.len() as u64, sealed_len(plain.len() as u64));
    out
}

fn body(bytes: Vec<u8>) -> BlobBody {
    Box::new(std::iter::once(Ok(bytes)))
}

/// A download kept whole in memory, with the length the server announced.
#[derive(Default)]
struct Collected {
    announced: Option<u64>,
    bytes: Vec<u8>,
}

impl BlobSink for Collected {
    fn begin(&mut self, sealed_len: u64) -> SinkFlow {
        self.announced = Some(sealed_len);
        SinkFlow::Continue
    }

    fn piece(&mut self, bytes: &[u8]) -> SinkFlow {
        self.bytes.extend_from_slice(bytes);
        SinkFlow::Continue
    }
}

/// A raw request to a blob route of `vault`, past `HttpRelay` (which only ever sends canonical
/// IDs), with the vault's token.
async fn raw(
    url: &str,
    vault: &VaultAuth,
    method: Method,
    blob: &str,
    body: Option<Vec<u8>>,
) -> reqwest::Response {
    let mut request = reqwest::Client::new()
        .request(
            method,
            format!("{url}/v1/vaults/{}/blobs/{blob}", vault.vault_id),
        )
        .bearer_auth(vault.token.as_str());
    if let Some(body) = body {
        request = request
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(body);
    }
    request.send().await.expect("raw request")
}

fn header(response: &reqwest::Response, name: &str) -> Option<String> {
    response
        .headers()
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string)
}

/// The real relay, counting the blob uploads (`PUT`) and checks (`HEAD`) made through it.
struct CountingBlobs<'a> {
    relay: &'a HttpRelay,
    puts: AtomicUsize,
    heads: AtomicUsize,
}

impl<'a> CountingBlobs<'a> {
    fn new(relay: &'a HttpRelay) -> Self {
        CountingBlobs {
            relay,
            puts: AtomicUsize::new(0),
            heads: AtomicUsize::new(0),
        }
    }

    fn puts(&self) -> usize {
        self.puts.load(Ordering::SeqCst)
    }

    fn heads(&self) -> usize {
        self.heads.load(Ordering::SeqCst)
    }
}

impl Relay for CountingBlobs<'_> {
    async fn server_info(&self) -> RelayResult<ServerInfo> {
        self.relay.server_info().await
    }
    async fn create_account(&self) -> RelayResult<String> {
        self.relay.create_account().await
    }
    async fn account_status(&self, account: &str) -> RelayResult<AccountStatus> {
        self.relay.account_status(account).await
    }
    async fn checkout(&self, account: &str, plan: CheckoutPlan) -> RelayResult<String> {
        self.relay.checkout(account, plan).await
    }
    async fn create_vault(&self, account: &str, vault: &VaultAuth) -> RelayResult<bool> {
        self.relay.create_vault(account, vault).await
    }
    async fn vault_info(&self, vault: &VaultAuth) -> RelayResult<VaultInfo> {
        self.relay.vault_info(vault).await
    }
    async fn push(&self, vault: &VaultAuth, ops: &[OutgoingOp]) -> RelayResult<PushReceipt> {
        self.relay.push(vault, ops).await
    }
    async fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> RelayResult<PullPage> {
        self.relay.pull(vault, after, limit).await
    }
    async fn put_snapshot(&self, vault: &VaultAuth, seq: u64, bytes: Vec<u8>) -> RelayResult<()> {
        self.relay.put_snapshot(vault, seq, bytes).await
    }
    async fn get_snapshot(&self, vault: &VaultAuth) -> RelayResult<SnapshotBlob> {
        self.relay.get_snapshot(vault).await
    }
    async fn delete_vault(&self, vault: &VaultAuth) -> RelayResult<()> {
        self.relay.delete_vault(vault).await
    }
    async fn rotate_token(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        self.relay.rotate_token(vault, new_token, bundle).await
    }
    async fn rotate_token_with_kit(
        &self,
        vault: &VaultAuth,
        new_token: &str,
        bundle: &[u8],
        kit: crate::services::sync::relay::KitRotation<'_>,
    ) -> RelayResult<crate::services::sync::relay::Rotated> {
        self.relay
            .rotate_token_with_kit(vault, new_token, bundle, kit)
            .await
    }
    async fn put_kit(
        &self,
        vault: &VaultAuth,
        kit_token: &str,
        envelope: &[u8],
    ) -> RelayResult<bool> {
        self.relay.put_kit(vault, kit_token, envelope).await
    }
    async fn get_kit(
        &self,
        vault_id: &str,
        kit_token: &str,
    ) -> RelayResult<crate::services::sync::relay::KitRead> {
        self.relay.get_kit(vault_id, kit_token).await
    }
    async fn rekeys(
        &self,
        vault: &VaultAuth,
        after: u32,
        limit: u32,
    ) -> RelayResult<crate::services::sync::relay::RekeyPage> {
        self.relay.rekeys(vault, after, limit).await
    }
    async fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> RelayResult<()> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        self.relay.put_blob(vault, blob, sealed_len, body).await
    }
    async fn head_blob(&self, vault: &VaultAuth, blob: Uuid) -> RelayResult<Option<u64>> {
        self.heads.fetch_add(1, Ordering::SeqCst);
        self.relay.head_blob(vault, blob).await
    }
    async fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> RelayResult<Delivery> {
        self.relay.get_blob(vault, blob, sink).await
    }
    async fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<Uuid>,
        limit: u32,
    ) -> RelayResult<BlobPage> {
        self.relay.list_blobs(vault, after, limit).await
    }
    async fn delete_blob(&self, vault: &VaultAuth, blob: Uuid) -> RelayResult<()> {
        self.relay.delete_blob(vault, blob).await
    }
}

/// Puts the server's blob quota back to `bytes` when dropped, also when the test fails half way
/// (the quota is one setting for every vault of the server).
struct QuotaBack {
    url: String,
    bytes: u64,
}

impl Drop for QuotaBack {
    fn drop(&mut self) {
        let (url, bytes) = (self.url.clone(), self.bytes);
        // Its own runtime on its own thread: a drop cannot await, and may run inside a runtime.
        let restored = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime")
                .block_on(async { admin(&url).await.set_blob_quota(bytes).await })
        })
        .join();
        if restored.is_err() && !std::thread::panicking() {
            panic!("the blob quota could not be put back to {bytes} bytes");
        }
    }
}

#[tokio::test]
#[ignore = "needs a Moony sync server with blobs: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn the_blob_calls_round_trip_and_refuse_a_bad_id() {
    let url = relay_url();
    let relay = relay_at(&url);
    let info = until_allowed(|| relay.server_info(), |e| *e == RelayError::RateLimited)
        .await
        .expect("server info");
    assert!(info.blobs, "{info:?}");
    assert_eq!(info.protocol, 3);

    let a = first_device(&relay, &url, "Blobs").await;
    let creds = a.credentials();
    let auth = creds.auth();
    let blob = Uuid::new_v4();
    let plain = pdf(150_000, 3);
    let bytes = sealed(&creds, &blob, &plain);
    let len = bytes.len() as u64;

    // PUT, then HEAD: the stored size is the sealed length, in both headers.
    relay
        .put_blob(&auth, blob, len, body(bytes.clone()))
        .await
        .expect("put");
    assert_eq!(relay.head_blob(&auth, blob).await, Ok(Some(len)));
    let id = blob.hyphenated().to_string();
    let head = raw(&url, &auth, Method::HEAD, &id, None).await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(header(&head, "content-length"), Some(len.to_string()));
    assert_eq!(header(&head, "moony-blob-size"), Some(len.to_string()));

    // GET hands the sink the announced length and every sealed byte as stored.
    let mut sink = Collected::default();
    assert_eq!(
        relay.get_blob(&auth, blob, &mut sink).await,
        Ok(Delivery::Complete)
    );
    assert_eq!(sink.announced, Some(len));
    assert_eq!(sink.bytes, bytes);
    let get = raw(&url, &auth, Method::GET, &id, None).await;
    assert_eq!(get.status(), StatusCode::OK);
    assert_eq!(header(&get, "content-length"), Some(len.to_string()));
    assert_eq!(header(&get, "moony-blob-size"), Some(len.to_string()));

    // A second PUT of the same ID: the server answers "already there" without the body, and the
    // client takes that as success.
    relay
        .put_blob(&auth, blob, len, body(bytes.clone()))
        .await
        .expect("a second put is a success");
    let again = raw(&url, &auth, Method::PUT, &id, Some(bytes.clone())).await;
    assert_eq!(again.status(), StatusCode::OK);
    let answer: serde_json::Value = again.json().await.expect("json");
    assert_eq!(answer, serde_json::json!({ "created": false }));

    // The listing has it at its size, dated and answered on the server's clock (about ours).
    let page = relay.list_blobs(&auth, None, 100).await.expect("list");
    assert!(!page.more);
    let local = now();
    let server_now = page.server_now.expect("the listing says the server's time");
    assert!((server_now - local).abs() <= 5, "{server_now} vs {local}");
    let listed = page
        .blobs
        .iter()
        .find(|listed| listed.id == blob)
        .expect("listed");
    assert_eq!(listed.size, len);
    assert!(
        (listed.created_at - local).abs() <= 60,
        "{} vs {local}",
        listed.created_at
    );

    // An ID the vault never had: a typed "not found" on GET, "missing" on HEAD.
    let unknown = Uuid::new_v4();
    let mut nothing = Collected::default();
    assert_eq!(
        relay.get_blob(&auth, unknown, &mut nothing).await,
        Err(RelayError::BlobNotFound)
    );
    assert_eq!(nothing.announced, None);
    assert_eq!(relay.head_blob(&auth, unknown).await, Ok(None));

    // A non-canonical ID (upper case), which `HttpRelay` never sends: the server refuses it on
    // every route, and the client's reading of the answer is the typed invalid-ID error.
    let upper = id.to_uppercase();
    let invalid = RelayError::BadRequest("invalid_blob_id".into());
    for (method, body) in [
        (Method::HEAD, None),
        (Method::GET, None),
        (Method::PUT, Some(bytes.clone())),
        (Method::DELETE, None),
    ] {
        let response = raw(&url, &auth, method.clone(), &upper, body).await;
        let status = response.status().as_u16();
        assert_eq!(status, 400, "{method}");
        let code = header(&response, "moony-error").expect("Moony-Error");
        assert_eq!(RelayError::from_code(status, &code), invalid, "{method}");
        if method != Method::HEAD {
            let text = response.bytes().await.expect("body");
            assert_eq!(
                RelayError::from_response(status, &text),
                invalid,
                "{method}"
            );
        }
    }

    // DELETE: gone from the listing and from HEAD and GET; a second DELETE is a success too.
    relay.delete_blob(&auth, blob).await.expect("delete");
    let page = relay.list_blobs(&auth, None, 100).await.expect("list");
    assert!(page.blobs.iter().all(|listed| listed.id != blob));
    assert_eq!(relay.head_blob(&auth, blob).await, Ok(None));
    assert_eq!(
        relay.get_blob(&auth, blob, &mut Collected::default()).await,
        Err(RelayError::BlobNotFound)
    );
    relay
        .delete_blob(&auth, blob)
        .await
        .expect("deleting a missing blob");
}

#[tokio::test]
#[ignore = "needs a Moony sync server with blobs: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_document_added_on_one_device_opens_on_the_other() {
    let url = relay_url();
    let relay = relay_at(&url);
    // B is a phone first: full content only when the person asks for it.
    let (mut a, mut b) = two_devices(&relay, &url, true).await;
    let (contract, statement) = (pdf(150_000, 1), pdf(90_000, 2));
    let (d1, blob1) = a.add_document("Contract", &contract);
    let (d2, blob2) = a.add_document("Statement", &statement);
    assert_eq!(
        a.state(&blob1),
        None,
        "the row is captured, the queue reads it at sync"
    );

    a.sync(&relay).await;
    for blob in [&blob1, &blob2] {
        assert_eq!(a.state(blob).as_deref(), Some("done"), "{blob}");
    }
    let creds = a.creds();
    assert_eq!(
        relay.head_blob(&creds.auth(), blob_id(&blob1)).await,
        Ok(Some(sealed_len(contract.len() as u64)))
    );

    b.round(&relay).await;
    for id in [&d1, &d2] {
        assert_eq!(b.document_content(id), Some(AttachmentContent::NotHere));
    }
    // The phone's lane fetches no full content by itself.
    let fetched = b.downloads(&relay).await;
    assert_eq!(fetched.stored, 0, "{fetched:?}");
    assert_eq!(b.document_content(&d1), Some(AttachmentContent::NotHere));

    // The person opens the contract: it is wanted, and the lane fetches it.
    let asked =
        attachments::request_download(&b.device.db.lock(), AttachmentKind::InsuranceDocument, &d1)
            .expect("ask for it");
    assert_ne!(asked, AttachmentContent::Here);
    let fetched = b.downloads(&relay).await;
    assert_eq!(fetched.stored, 1, "{fetched:?}");
    assert_eq!(b.document_content(&d1), Some(AttachmentContent::Here));
    assert_eq!(b.document(&d1), contract);
    assert_eq!(b.document_content(&d2), Some(AttachmentContent::NotHere));

    // A desktop's lane fetches the rest by itself.
    let desktop = ready_policy(&relay, false).await;
    let fetched = b.downloads_with(&relay, &desktop).await;
    assert_eq!(fetched.stored, 1, "{fetched:?}");
    assert_eq!(b.document(&d2), statement);
    for id in [&d1, &d2] {
        assert_eq!(
            b.document_content(id),
            Some(AttachmentContent::Here),
            "B {id}"
        );
        assert_eq!(
            a.document_content(id),
            Some(AttachmentContent::Here),
            "A {id}"
        );
    }
    assert_eq!(a.document(&d1), contract);
}

#[tokio::test]
#[ignore = "needs a Moony sync server with blobs: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_photo_set_travels_with_its_thumbnails() {
    let url = relay_url();
    let relay = relay_at(&url);
    let (mut a, mut b) = two_devices(&relay, &url, true).await;
    let ids: Vec<String> = (0..3).map(|seed| a.add_photo(seed * 40)).collect();
    a.sync(&relay).await;
    assert!(a
        .photos()
        .iter()
        .all(|photo| photo.content == AttachmentContent::Here
            && photo.thumb_content == AttachmentContent::Here));

    b.round(&relay).await;
    let photos = b.photos();
    assert_eq!(photos.len(), ids.len());
    assert!(photos
        .iter()
        .all(|photo| photo.content == AttachmentContent::NotHere
            && photo.thumb_content == AttachmentContent::NotHere));

    // A phone fetches the thumbnails at once, the full photos only when asked.
    let fetched = b.downloads(&relay).await;
    assert_eq!(fetched.stored, ids.len(), "{fetched:?}");
    for photo in b.photos() {
        assert_eq!(photo.thumb_content, AttachmentContent::Here, "{}", photo.id);
        assert_eq!(photo.content, AttachmentContent::NotHere, "{}", photo.id);
    }
    for id in &ids {
        assert_eq!(
            b.read(AttachmentKind::Photo, id, Variant::Thumbnail),
            a.read(AttachmentKind::Photo, id, Variant::Thumbnail),
            "{id}"
        );
    }

    // "Download everything to this phone".
    let wanted = attachments::download_everything(&b.device.db.lock()).expect("download all");
    assert_eq!(wanted as usize, ids.len());
    let fetched = b.downloads(&relay).await;
    assert_eq!(fetched.stored, ids.len(), "{fetched:?}");
    for photo in b.photos() {
        assert_eq!(photo.content, AttachmentContent::Here, "{}", photo.id);
    }
    for id in &ids {
        let full = b.read(AttachmentKind::Photo, id, Variant::Full);
        assert_eq!(
            full,
            a.read(AttachmentKind::Photo, id, Variant::Full),
            "{id}"
        );
        assert!(full.starts_with(&[0xFF, 0xD8]), "a JPEG");
    }
}

#[tokio::test]
#[ignore = "needs a Moony sync server with blobs: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_join_fetches_content_from_the_server() {
    let url = relay_url();
    let relay = relay_at(&url);
    let mut a = Node::new(first_device(&relay, &url, "A").await, &relay, false).await;
    a.owners();
    let contract = pdf(200_000, 7);
    let (doc, _) = a.add_document("Contract", &contract);
    let photos: Vec<String> = (0..2).map(|seed| a.add_photo(100 + seed)).collect();
    a.sync(&relay).await;
    // A snapshot that holds the rows, so the join's replace brings them.
    let report = a.round_in(&relay, RoundMode::FirstSnapshot).await;
    assert!(report.snapshot_uploaded, "{report:?}");

    let mut c = Node::new(Device::new(), &relay, false).await;
    c.device.join(&relay, a.creds(), "C").await;
    c.round(&relay).await;
    assert!(
        c.document_content(&doc).is_some(),
        "the row came with the join"
    );
    let fetched = c.downloads(&relay).await;
    assert_eq!(fetched.stored, 1 + 2 * photos.len(), "{fetched:?}");

    assert_eq!(c.document_content(&doc), Some(AttachmentContent::Here));
    assert_eq!(c.document(&doc), contract);
    for photo in c.photos() {
        assert_eq!(photo.content, AttachmentContent::Here, "{}", photo.id);
        assert_eq!(photo.thumb_content, AttachmentContent::Here, "{}", photo.id);
    }
    for id in &photos {
        assert_eq!(
            c.read(AttachmentKind::Photo, id, Variant::Full),
            a.read(AttachmentKind::Photo, id, Variant::Full)
        );
    }
    assert_eq!(
        c.device
            .count("SELECT COUNT(*) FROM sync_blobs WHERE state <> 'done' OR active <> 0"),
        0,
        "nothing left to move"
    );
    assert_eq!(c.device.count("SELECT COUNT(*) FROM sync_blobs"), 5);
    assert_eq!(c.attachments(), SyncAttachments::default());
}

#[tokio::test]
#[ignore = "needs a Moony sync server with blobs: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_deleted_document_stays_on_the_server_until_the_pass_and_a_lost_blob_comes_back() {
    let url = relay_url();
    let relay = relay_at(&url);
    let (mut a, mut b) = two_devices(&relay, &url, false).await;
    let creds = a.creds();
    let (d1, blob1) = a.add_document("Old", &pdf(60_000, 11));
    a.sync(&relay).await;
    b.round(&relay).await;
    assert_eq!(b.downloads(&relay).await.stored, 1);

    // Deleted on A, merged on B: gone from both devices, still on the server.
    a.delete_document(&d1);
    a.round(&relay).await;
    b.round(&relay).await;
    let report = a.round(&relay).await;
    assert_eq!(b.document_content(&d1), None);
    assert_eq!(
        b.device.count(&format!(
            "SELECT COUNT(*) FROM files.blobs WHERE id = '{blob1}'"
        )),
        0
    );
    assert!(listed(&relay, &creds).await.contains(&blob_id(&blob1)));

    // The pass at its real grace keeps a blob deleted moments ago.
    let pulled = report.pull.as_ref().expect("a pull");
    a.pass_due();
    let kept = pass::run_if_due(&relay, &a.device.db, &creds, a.policy(), pulled, now())
        .await
        .expect("pass");
    assert_eq!((kept.deleted, kept.complete), (0, true), "{kept:?}");
    assert!(listed(&relay, &creds).await.contains(&blob_id(&blob1)));

    // With the grace at zero (the blob is at least a second old on the server's clock), the pass
    // deletes it from the server.
    tokio::time::sleep(Duration::from_millis(1_100)).await;
    a.pass_due();
    let limits = Limits {
        grace: 0,
        ..Limits::DEFAULT
    };
    let swept = pass::run_with(
        &relay,
        &a.device.db,
        &creds,
        a.policy(),
        pulled,
        now(),
        limits,
    )
    .await
    .expect("pass");
    assert_eq!((swept.deleted, swept.complete), (1, true), "{swept:?}");
    assert!(!listed(&relay, &creds).await.contains(&blob_id(&blob1)));

    // A blob the server loses (removed by hand) is put back by the next pass and the upload
    // lane, and the other device, which never fetched it, then can.
    let content = pdf(70_000, 12);
    let (d2, blob2) = a.add_document("Lost", &content);
    a.sync(&relay).await;
    b.round(&relay).await;
    assert_eq!(b.document_content(&d2), Some(AttachmentContent::NotHere));
    relay
        .delete_blob(&creds.auth(), blob_id(&blob2))
        .await
        .expect("lost on the server");
    let report = a.round(&relay).await;
    a.pass_due();
    let put_back = pass::run_if_due(
        &relay,
        &a.device.db,
        &creds,
        a.policy(),
        report.pull.as_ref().expect("a pull"),
        now(),
    )
    .await
    .expect("pass");
    assert_eq!(
        (put_back.requeued, put_back.deleted, put_back.complete),
        (1, 0, true),
        "{put_back:?}"
    );
    assert_eq!(a.state(&blob2).as_deref(), Some("up"));
    let up = a.uploads(&relay).await;
    assert_eq!(up.uploaded, 1, "{up:?}");
    assert_eq!(a.state(&blob2).as_deref(), Some("done"));
    assert!(listed(&relay, &creds).await.contains(&blob_id(&blob2)));

    assert_eq!(b.downloads(&relay).await.stored, 1);
    assert_eq!(b.document_content(&d2), Some(AttachmentContent::Here));
    assert_eq!(b.document(&d2), content);
}

#[tokio::test]
#[ignore = "needs a Moony sync server with blobs: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn an_upload_refused_for_quota_waits_and_goes_through_after_room_is_made() {
    let url = relay_url();
    let relay = relay_at(&url);
    let (mut a, mut b) = two_devices(&relay, &url, false).await;
    let admin = admin(&url).await;
    let quota = admin.blob_quota().await;
    let _back = QuotaBack {
        url: url.clone(),
        bytes: quota,
    };
    admin.set_blob_quota(1_024).await;
    let content = pdf(40_000, 21);
    let (doc, blob) = a.add_document("Too big", &content);

    // The round sends the row; the upload is refused for room and waits, not stuck.
    let report = a.round(&relay).await;
    assert!(report.push.is_some_and(|push| push.pushed > 0));
    assert_eq!(
        a.entry(&blob),
        Some(("up".into(), Some(TransferError::NoRoom.as_str().into())))
    );
    assert_eq!(a.policy().problem(), Some(SyncAttachmentsProblem::NoRoom));
    let shown = a.attachments();
    assert_eq!(shown.problem, Some(SyncAttachmentsProblem::NoRoom));
    assert_eq!((shown.waiting_upload, shown.failed), (1, 0), "{shown:?}");
    // The lane waits for room: nothing is tried.
    let waited = a.uploads(&relay).await;
    assert_eq!((waited.uploaded, waited.failed), (0, 0), "{waited:?}");
    let creds = a.creds();
    assert_eq!(
        relay.head_blob(&creds.auth(), blob_id(&blob)).await,
        Ok(None)
    );

    // The row still syncs; its content is not on the server yet.
    b.round(&relay).await;
    b.downloads(&relay).await;
    assert_eq!(
        b.document_content(&doc),
        Some(AttachmentContent::NotOnServer)
    );

    // Room is made, and "Sync now" sends it at once.
    admin.set_blob_quota(quota).await;
    a.policy().try_again();
    queue::retry_now(&a.device.db.lock()).expect("sync now");
    let up = a.uploads(&relay).await;
    assert_eq!(up.uploaded, 1, "{up:?}");
    assert_eq!(a.state(&blob).as_deref(), Some("done"));
    assert_eq!(a.policy().problem(), None);
    assert_eq!(a.attachments(), SyncAttachments::default());

    queue::retry_now(&b.device.db.lock()).expect("sync now");
    assert_eq!(b.downloads(&relay).await.stored, 1);
    assert_eq!(b.document(&doc), content);
}

#[tokio::test]
#[ignore = "needs a Moony sync server with blobs: MOONY_RELAY_E2E=http://127.0.0.1:8787"]
async fn a_blob_uploaded_again_is_accepted_as_already_there() {
    let url = relay_url();
    let relay = relay_at(&url);
    let mut a = Node::new(first_device(&relay, &url, "A").await, &relay, false).await;
    a.owners();
    let creds = a.creds();
    let (_, blob) = a.add_document("Contract", &pdf(30_000, 31));
    a.sync(&relay).await;
    assert_eq!(a.state(&blob).as_deref(), Some("done"));

    // A restore or a replace rebuilds the queue from the rows and the store: content held here
    // waits for upload, whether or not the server has it.
    {
        let conn = a.device.db.lock();
        conn.execute("DELETE FROM sync_blobs WHERE blob_id = ?1", [&blob])
            .expect("forget");
        queue::reconcile(&conn, now()).expect("reconcile");
    }
    assert_eq!(a.state(&blob).as_deref(), Some("up"));
    let counting = CountingBlobs::new(&relay);
    let transfer = queue::next_upload(&a.device.db.lock(), now(), None)
        .expect("queue")
        .expect("due");
    assert_eq!(transfer.blob_id, blob);
    let outcome = upload_one(&counting, &a.device.db, &creds, &transfer, now()).await;
    assert_eq!(outcome, Outcome::Confirmed);
    assert_eq!(
        (counting.heads(), counting.puts()),
        (1, 0),
        "no second body"
    );
    assert_eq!(a.state(&blob).as_deref(), Some("done"));
    assert_eq!(a.attachments().waiting_upload, 0);

    // The server holds other bytes under a blob's ID (another size than this content's sealed
    // length): a conflict, never overwritten and never retried by itself.
    let content = pdf(25_000, 32);
    let (_, other) = a.add_document("Other", &content);
    let garbage = usize::try_from(MIN_BLOB_SEALED).expect("size") + 7;
    assert_ne!(garbage as u64, sealed_len(content.len() as u64));
    relay
        .put_blob(
            &creds.auth(),
            blob_id(&other),
            garbage as u64,
            body(vec![0x5a; garbage]),
        )
        .await
        .expect("someone else's bytes");
    let transfer = {
        let conn = a.device.db.lock();
        queue::reconcile_rows(&conn, now()).expect("queue the new row");
        queue::next_upload(&conn, now(), None)
            .expect("queue")
            .expect("due")
    };
    assert_eq!(transfer.blob_id, other);
    let outcome = upload_one(&counting, &a.device.db, &creds, &transfer, now()).await;
    assert_eq!(outcome, Outcome::Failed(TransferError::Conflict));
    assert_eq!((counting.heads(), counting.puts()), (2, 0), "nothing sent");
    assert_eq!(
        a.entry(&other),
        Some(("up".into(), Some(TransferError::Conflict.as_str().into())))
    );
    assert_eq!(
        queue::next_upload(&a.device.db.lock(), now(), None).expect("queue"),
        None,
        "not retried by itself"
    );
    let shown = a.attachments();
    assert_eq!((shown.waiting_upload, shown.failed), (0, 1), "{shown:?}");
    assert_eq!(
        relay.head_blob(&creds.auth(), blob_id(&other)).await,
        Ok(Some(garbage as u64)),
        "the server's copy is untouched"
    );
}
