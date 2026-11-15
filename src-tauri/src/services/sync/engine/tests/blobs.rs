//! Documents and photos in the sync round (attachments design §5.3, §5.6; ADR 0030): a round sends
//! small content before its changes, within a budget, and nothing a blob does fails, delays past
//! that budget or changes the round. The status counts content waiting to move beside the
//! ordinary phase, never in it.

use std::time::Duration;

use super::*;
use crate::models::attachments::AttachmentContent;
use crate::models::sync::{SyncAttachments, SyncAttachmentsProblem, SyncPhase};
use crate::services::attachment_store;
use crate::services::sync::blobs::content::ContentReader;
use crate::services::sync::blobs::policy::BlobPolicy;
use crate::services::sync::blobs::queue;
use crate::services::sync::blobs::transfer::{run_downloads, PRE_PUSH_BUDGET, PRE_PUSH_MAX_PLAIN};
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::status::{self, Runtime};
use crate::services::sync::engine::{run_round, RoundMode, RoundReport};
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::{
    AccountStatus, BlobBody, BlobPage, BlobSink, CheckoutPlan, Delivery, PullPage, PushReceipt,
    RelayError, RelayResult, ServerInfo, SnapshotBlob, VaultInfo,
};

/// A blob ID no store here holds (content that lives on another device).
const ELSEWHERE: &str = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f01";
const ELSEWHERE_THUMB: &str = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f02";

/// Policy `p1`, the owner of the documents.
fn owner(device: &TestConn) {
    exec(
        &device.lock(),
        "INSERT INTO insurance_policies \
           (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
         VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly')",
    );
}

/// A document `id` of policy `p1` with new content `bytes` held here; its blob ID.
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

/// `(state, attempts, active)` of a queue entry.
fn entry(device: &TestConn, blob: &str) -> Option<(String, i64, bool)> {
    device
        .lock()
        .query_row(
            "SELECT state, attempts, active FROM sync_blobs WHERE blob_id = ?1",
            [blob],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .ok()
}

fn last_success(device: &TestConn) -> Option<i64> {
    state::marks(&device.lock())
        .expect("marks")
        .expect("sync is on")
        .last_success_at
}

fn shown(device: &TestConn) -> SyncAttachments {
    status::status(&device.lock(), &Runtime::default())
        .expect("status")
        .attachments
}

async fn round_on(relay: &impl Relay, device: &TestConn, mode: RoundMode) -> RoundReport {
    run_round(relay, device, &mut SnapshotPolicy::default(), mode, NOW, 0)
        .await
        .expect("round")
}

fn pushed(report: &RoundReport) -> usize {
    report.push.as_ref().map_or(0, |push| push.pushed)
}

fn blob_calls(relay: &FakeRelay) -> Vec<Route> {
    relay
        .calls()
        .into_iter()
        .filter(|route| matches!(route, Route::PutBlob | Route::HeadBlob | Route::GetBlob))
        .collect()
}

/// A sync server that keeps failing every upload: the round still pushes, pulls, uploads its
/// snapshot and records success, and the other device gets every row; the content waits.
#[tokio::test]
async fn an_upload_that_keeps_failing_holds_neither_data_nor_snapshots() {
    let vault = vault().await;
    with_store(&vault.a);
    with_store(&vault.b);
    vault
        .relay
        .set_failing(Route::PutBlob, Some(RelayError::Server(503)));
    owner(&vault.a);
    let blob = document(&vault.a, "d1", b"%PDF-1.7 contract");
    institution(&vault.a.lock(), "i1");

    let report = round_on(&vault.relay, &vault.a, RoundMode::FirstSnapshot).await;

    assert!(pushed(&report) > 0, "{report:?}");
    assert!(report.snapshot_uploaded, "{report:?}");
    assert_eq!(report.problem, None);
    assert_eq!(last_success(&vault.a), Some(NOW));
    assert!(
        vault.relay.calls().contains(&Route::PutBlob),
        "the upload was tried"
    );

    round_on(&vault.relay, &vault.b, RoundMode::Normal).await;
    run_downloads(
        &vault.relay,
        &vault.b,
        &vault.creds,
        &BlobPolicy::default(),
        NOW,
        None,
    )
    .await
    .expect("downloads");
    {
        let b = vault.b.lock();
        assert!(has_institution(&b, "i1"));
        assert_eq!(
            count(
                &b,
                "SELECT COUNT(*) FROM insurance_documents WHERE id = 'd1'"
            ),
            1
        );
        let content = ContentReader::new(&b)
            .expect("reader")
            .of(Some(&blob), true)
            .expect("content");
        assert_eq!(content, AttachmentContent::NotOnServer);
    }
    let (state, attempts, active) = entry(&vault.a, &blob).expect("queued");
    assert_eq!(state, "up");
    assert!(attempts >= 1, "{attempts}");
    assert!(!active);
    assert_eq!(shown(&vault.a).waiting_upload, 1);
}

/// A sync server that refuses the content for good: a failed item, not content waiting to upload,
/// and the round is a success as without it.
#[tokio::test]
async fn a_refused_upload_is_a_failed_item_and_the_round_succeeds() {
    let vault = vault().await;
    with_store(&vault.a);
    vault.relay.set_failing(
        Route::PutBlob,
        Some(RelayError::BadRequest("blob_too_large".into())),
    );
    owner(&vault.a);
    document(&vault.a, "d1", b"%PDF-1.7 contract");

    let report = round_on(&vault.relay, &vault.a, RoundMode::Normal).await;

    assert!(pushed(&report) > 0, "{report:?}");
    assert_eq!(report.problem, None);
    assert_eq!(last_success(&vault.a), Some(NOW));
    let attachments = shown(&vault.a);
    assert_eq!(attachments.waiting_upload, 0);
    assert_eq!(attachments.failed, 1);
}

/// Content added here since the last merge (no reconcile ran) is queued by the round itself and
/// sent before the round's changes, so the other device finds it when the row arrives.
#[tokio::test]
async fn small_blobs_go_up_before_the_push() {
    let vault = vault().await;
    with_store(&vault.a);
    owner(&vault.a);
    let blob = document(&vault.a, "d1", &[7u8; 200_000]);

    let report = round_on(&vault.relay, &vault.a, RoundMode::Normal).await;

    let calls = vault.relay.calls();
    let put = calls.iter().position(|r| *r == Route::PutBlob);
    let push = calls.iter().position(|r| *r == Route::Push);
    assert!(put.is_some() && push.is_some() && put < push, "{calls:?}");
    assert_eq!(entry(&vault.a, &blob).expect("queued").0, "done");
    assert_eq!(report.problem, None);
    assert_eq!(shown(&vault.a).waiting_upload, 0);
}

/// Content larger than the round's limit is never started by the round: it waits for the lane.
#[tokio::test]
async fn a_large_blob_waits_for_the_lane() {
    let vault = vault().await;
    with_store(&vault.a);
    owner(&vault.a);
    let size = 9 * 1024 * 1024;
    assert!(size > PRE_PUSH_MAX_PLAIN as usize);
    let blob = document(&vault.a, "d1", &vec![7u8; size]);

    let report = round_on(&vault.relay, &vault.a, RoundMode::Normal).await;

    assert!(pushed(&report) > 0, "{report:?}");
    assert_eq!(blob_calls(&vault.relay), Vec::<Route>::new());
    assert_eq!(
        entry(&vault.a, &blob),
        Some(("up".into(), 0, false)),
        "queued, untouched"
    );
    let attachments = shown(&vault.a);
    assert_eq!(attachments.waiting_upload, 1);
    assert_eq!(attachments.waiting_upload_bytes, size as i64);
}

/// The repair's drain sends changes only.
#[tokio::test]
async fn a_drain_round_uploads_nothing() {
    let vault = vault().await;
    with_store(&vault.a);
    owner(&vault.a);
    document(&vault.a, "d1", b"%PDF-1.7 contract");

    let report = round_on(&vault.relay, &vault.a, RoundMode::Drain).await;

    assert!(pushed(&report) > 0, "{report:?}");
    assert_eq!(blob_calls(&vault.relay), Vec::<Route>::new());
    assert!(!vault.relay.calls().contains(&Route::ServerInfo));
}

/// A round with a stalled transfer hangs instead of failing: the budget bounds the step, and this
/// bounds the test.
async fn ends<T>(round: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(3_600), round)
        .await
        .expect("the round ends")
}

/// The upload lane is busy with a long transfer: the round skips its upload step at once and
/// pushes; the attachment added meanwhile is still queued and counted.
#[tokio::test(start_paused = true)]
async fn a_busy_lane_is_skipped_at_once() {
    let vault = vault().await;
    with_store(&vault.a);
    owner(&vault.a);
    let blob = document(&vault.a, "d1", b"%PDF-1.7 contract");
    let mut policy = SnapshotPolicy::default();
    let lanes = policy.blobs.clone();
    let _long_transfer = lanes.up_lane().lock().await;
    let started = tokio::time::Instant::now();

    let report = ends(run_round(
        &vault.relay,
        &vault.a,
        &mut policy,
        RoundMode::Normal,
        NOW,
        0,
    ))
    .await
    .expect("round");

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert!(pushed(&report) > 0, "{report:?}");
    assert_eq!(report.problem, None);
    assert_eq!(blob_calls(&vault.relay), Vec::<Route>::new());
    assert_eq!(entry(&vault.a, &blob), Some(("up".into(), 0, false)));
    assert_eq!(shown(&vault.a).waiting_upload, 1);
}

/// An upload that never ends (a sync server that stalls mid-body) is cut at the budget: the round
/// pushes, and the entry counts a failed attempt, so its backoff applies.
#[tokio::test(start_paused = true)]
async fn an_upload_that_stalls_ends_at_the_budget() {
    let vault = vault().await;
    with_store(&vault.a);
    owner(&vault.a);
    let blob = document(&vault.a, "d1", b"%PDF-1.7 contract");
    let relay = StalledUploads(vault.relay.clone(), None);
    let started = tokio::time::Instant::now();

    let report = ends(round_on(&relay, &vault.a, RoundMode::Normal)).await;

    let spent = started.elapsed();
    assert!(
        spent >= PRE_PUSH_BUDGET && spent < PRE_PUSH_BUDGET + Duration::from_secs(1),
        "{spent:?}"
    );
    assert!(pushed(&report) > 0, "{report:?}");
    assert_eq!(report.problem, None);
    assert_eq!(entry(&vault.a, &blob), Some(("up".into(), 1, false)));
}

/// A blob cut by the budget waits for its backoff: the next round does not spend its budget on it
/// again, and a smaller blob behind it goes up.
#[tokio::test(start_paused = true)]
async fn a_cut_upload_does_not_hold_up_the_next_round() {
    let vault = vault().await;
    with_store(&vault.a);
    owner(&vault.a);
    let slow = document(&vault.a, "d1", &[1u8; 300_000]);
    let relay = StalledUploads(
        vault.relay.clone(),
        Some(uuid::Uuid::parse_str(&slow).expect("id")),
    );
    ends(round_on(&relay, &vault.a, RoundMode::Normal)).await;
    let small = document(&vault.a, "d2", b"%PDF-1.7 small");
    let started = tokio::time::Instant::now();

    let report = ends(round_on(&relay, &vault.a, RoundMode::Normal)).await;

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(report.problem, None);
    assert_eq!(entry(&vault.a, &small).expect("queued").0, "done");
    assert_eq!(entry(&vault.a, &slow), Some(("up".into(), 1, false)));
}

/// A dropped transfer whose drop could not reach the connection leaves its entry marked active:
/// the next run that holds the lane counts it a failed attempt before it picks work, so it waits
/// for its backoff instead of being skipped for good or started again at once.
#[tokio::test]
async fn an_entry_left_active_by_an_earlier_run_counts_a_failed_attempt() {
    let vault = vault().await;
    with_store(&vault.a);
    owner(&vault.a);
    let blob = document(&vault.a, "d1", b"%PDF-1.7 contract");
    {
        let conn = vault.a.lock();
        queue::reconcile(&conn, NOW).expect("reconcile");
        queue::set_active(&conn, &blob, true).expect("left active");
    }

    let report = round_on(&vault.relay, &vault.a, RoundMode::Normal).await;

    assert!(pushed(&report) > 0, "{report:?}");
    assert_eq!(entry(&vault.a, &blob), Some(("up".into(), 1, false)));
    assert_eq!(
        blob_calls(&vault.relay),
        Vec::<Route>::new(),
        "waits for its backoff"
    );
}

/// The round sends content of exactly its size limit; one byte more is left to the lane.
#[tokio::test]
async fn the_size_limit_is_inclusive() {
    let limit = usize::try_from(PRE_PUSH_MAX_PLAIN).expect("limit");
    for (size, sent) in [(limit, true), (limit + 1, false)] {
        let vault = vault().await;
        with_store(&vault.a);
        owner(&vault.a);
        let blob = document(&vault.a, "d1", &vec![3u8; size]);

        round_on(&vault.relay, &vault.a, RoundMode::Normal).await;

        assert_eq!(
            vault.relay.calls().contains(&Route::PutBlob),
            sent,
            "{size} bytes"
        );
        let state = entry(&vault.a, &blob).expect("queued").0;
        assert_eq!(state, if sent { "done" } else { "up" }, "{size} bytes");
    }
}

/// A local failure of the upload step (here the queue cannot be read) leaves the round a success,
/// and the status stays readable with empty attachment figures.
#[tokio::test]
async fn a_broken_queue_fails_neither_the_round_nor_the_status() {
    let vault = vault().await;
    with_store(&vault.a);
    owner(&vault.a);
    document(&vault.a, "d1", b"%PDF-1.7 contract");
    exec(
        &vault.a.lock(),
        "ALTER TABLE sync_blobs RENAME TO sync_blobs_away",
    );

    let report = round_on(&vault.relay, &vault.a, RoundMode::Normal).await;

    assert!(pushed(&report) > 0, "{report:?}");
    assert_eq!(report.problem, None);
    assert_eq!(last_success(&vault.a), Some(NOW));
    let shown = status::status(&vault.a.lock(), &Runtime::default()).expect("status");
    assert_eq!(shown.phase, SyncPhase::Idle);
    assert_eq!(shown.attachments, SyncAttachments::default());
}

/// Content lost from the store outside a merge: the status and the content state read the queue,
/// never the store, so it still shows as waiting to upload (with its size) until a full reconcile.
#[test]
fn the_status_reads_the_queue_not_the_store() {
    let device = TestConn::new(migrated());
    with_store(&device);
    state::enable(&device.lock(), A).expect("enable");
    owner(&device);
    let bytes = b"%PDF-1.7 lost after it was queued";
    let blob = document(&device, "d1", bytes);
    {
        let conn = device.lock();
        queue::reconcile(&conn, NOW).expect("reconcile");
        attachment_store::delete(&conn, &blob).expect("the store loses it");
    }

    let attachments = shown(&device);

    assert_eq!(attachments.waiting_upload, 1);
    assert_eq!(attachments.waiting_upload_bytes, bytes.len() as i64);
    let conn = device.lock();
    let content = ContentReader::new(&conn)
        .expect("reader")
        .of(Some(&blob), true)
        .expect("content");
    assert_eq!(content, AttachmentContent::WaitingUpload);
}

/// A round that merged nothing does not check the store: content lost outside a merge stays
/// `done` until a full reconcile.
#[tokio::test]
async fn a_round_without_a_merge_leaves_the_store_unread() {
    let vault = vault().await;
    with_store(&vault.a);
    owner(&vault.a);
    let blob = document(&vault.a, "d1", b"%PDF-1.7 contract");
    round_on(&vault.relay, &vault.a, RoundMode::Normal).await;
    assert_eq!(entry(&vault.a, &blob).expect("queued").0, "done");
    attachment_store::delete(&vault.a.lock(), &blob).expect("the store loses it");

    round_on(&vault.relay, &vault.a, RoundMode::Normal).await;

    assert_eq!(entry(&vault.a, &blob).expect("queued").0, "done");
}

/// The status counts rows by where their content is, beside the ordinary phase: one document
/// waiting to upload (with its size), one photo on another device. The attachments problem is its
/// own field and leaves the phase and the problem alone; sync off shows zeros.
#[test]
fn the_status_counts_waiting_uploads_and_content_elsewhere() {
    let device = TestConn::new(migrated());
    with_store(&device);
    state::enable(&device.lock(), A).expect("enable");
    owner(&device);
    let bytes = b"%PDF-1.7 a contract of some length";
    document(&device, "d1", bytes);
    {
        let conn = device.lock();
        exec(
            &conn,
            "INSERT INTO real_estate (id, name, address, type) VALUES ('r1', 'Flat', 'Street', 'flat'); \
             INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
             VALUES ('batch-1', 'r1', 1767225600);",
        );
        conn.execute(
            "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id, created_at) \
             VALUES ('ph1', 'batch-1', ?1, ?2, 1767225600)",
            [ELSEWHERE, ELSEWHERE_THUMB],
        )
        .expect("photo");
        queue::reconcile(&conn, NOW).expect("reconcile");
    }

    assert_eq!(
        shown(&device),
        SyncAttachments {
            waiting_upload: 1,
            waiting_upload_bytes: bytes.len() as i64,
            not_here: 1,
            not_on_server: 0,
            failed: 0,
            problem: None,
        }
    );

    let runtime = Runtime {
        attachments_problem: Some(SyncAttachmentsProblem::NoRoom),
        ..Runtime::default()
    };
    let with_problem = status::status(&device.lock(), &runtime).expect("status");
    assert_eq!(
        with_problem.attachments.problem,
        Some(SyncAttachmentsProblem::NoRoom)
    );
    assert_eq!(with_problem.phase, SyncPhase::Idle);
    assert_eq!(with_problem.problem, None);

    let off = TestConn::new(migrated());
    with_store(&off);
    let off_status = status::status(&off.lock(), &runtime).expect("status");
    assert_eq!(off_status.phase, SyncPhase::Off);
    assert_eq!(off_status.attachments, SyncAttachments::default());
}

/// The fake sync server, except that the upload of the given blob (of any blob with `None`) never
/// takes its body: the call never ends.
struct StalledUploads(FakeRelay, Option<uuid::Uuid>);

impl Relay for StalledUploads {
    async fn server_info(&self) -> RelayResult<ServerInfo> {
        self.0.server_info().await
    }
    async fn create_account(&self) -> RelayResult<String> {
        self.0.create_account().await
    }
    async fn account_status(&self, account: &str) -> RelayResult<AccountStatus> {
        self.0.account_status(account).await
    }
    async fn checkout(&self, account: &str, plan: CheckoutPlan) -> RelayResult<String> {
        self.0.checkout(account, plan).await
    }
    async fn create_vault(&self, account: &str, vault: &VaultAuth) -> RelayResult<bool> {
        self.0.create_vault(account, vault).await
    }
    async fn vault_info(&self, vault: &VaultAuth) -> RelayResult<VaultInfo> {
        self.0.vault_info(vault).await
    }
    async fn push(&self, vault: &VaultAuth, ops: &[OutgoingOp]) -> RelayResult<PushReceipt> {
        self.0.push(vault, ops).await
    }
    async fn pull(&self, vault: &VaultAuth, after: u64, limit: u32) -> RelayResult<PullPage> {
        self.0.pull(vault, after, limit).await
    }
    async fn put_snapshot(&self, vault: &VaultAuth, seq: u64, bytes: Vec<u8>) -> RelayResult<()> {
        self.0.put_snapshot(vault, seq, bytes).await
    }
    async fn get_snapshot(&self, vault: &VaultAuth) -> RelayResult<SnapshotBlob> {
        self.0.get_snapshot(vault).await
    }
    async fn delete_vault(&self, vault: &VaultAuth) -> RelayResult<()> {
        self.0.delete_vault(vault).await
    }
    async fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> RelayResult<()> {
        if self.1.is_none_or(|stalled| stalled == blob) {
            return std::future::pending().await;
        }
        self.0.put_blob(vault, blob, sealed_len, body).await
    }
    async fn head_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<Option<u64>> {
        self.0.head_blob(vault, blob).await
    }
    async fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: uuid::Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> RelayResult<Delivery> {
        self.0.get_blob(vault, blob, sink).await
    }
    async fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<uuid::Uuid>,
        limit: u32,
    ) -> RelayResult<BlobPage> {
        self.0.list_blobs(vault, after, limit).await
    }
    async fn delete_blob(&self, vault: &VaultAuth, blob: uuid::Uuid) -> RelayResult<()> {
        self.0.delete_blob(vault, blob).await
    }
}
