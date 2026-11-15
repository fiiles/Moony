//! Moving blobs between two devices' stores through the fake sync server, one blob at a time:
//! `A` and `B` of the engine tests' vault, each with its attachment store attached and the same
//! policy `p1` and property `r1`, documents and photos written straight into their tables (as
//! a merge would bring them to the other device).

use std::future::Future;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::db::files_schema;
use crate::error::AppError;
use crate::models::attachments::{AttachmentContent, AttachmentKind};
use crate::models::sync::SyncAttachmentsProblem;
use crate::services::attachment_store;
use crate::services::sync::blobs::content::ContentReader;
use crate::services::sync::blobs::policy::BlobPolicy;
use crate::services::sync::blobs::queue::{self, BlobState, Transfer, TransferError};
use crate::services::sync::blobs::transfer::{
    classify, download_one, run_downloads, run_uploads, upload_one, Outcome, TransferReport,
};
use crate::services::sync::engine::tests::{vault, TestConn, Vault, NOW};
use crate::services::sync::keys::VaultAuth;
use crate::services::sync::relay::fake::{FakeRelay, Route};
use crate::services::sync::relay::{
    AccountStatus, BlobBody, BlobPage, BlobSink, CheckoutPlan, Delivery, OutgoingOp, PullPage,
    PushReceipt, Relay, RelayError, RelayResult, ServerInfo, ServerMode, SinkFlow, SnapshotBlob,
    VaultInfo,
};

/// The blob routes of the fake's call log.
const BLOB_ROUTES: [Route; 5] = [
    Route::PutBlob,
    Route::HeadBlob,
    Route::GetBlob,
    Route::ListBlobs,
    Route::DeleteBlob,
];

/// The attachment store, policy `p1` and property `r1` on a device.
fn with_store(device: &TestConn) {
    let conn = device.lock();
    conn.execute("ATTACH DATABASE ':memory:' AS files", [])
        .expect("attach");
    files_schema::migrate(&conn).expect("files schema");
    conn.execute_batch(
        "INSERT INTO insurance_policies \
           (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
         VALUES ('p1', 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly'); \
         INSERT INTO real_estate (id, name, address, type) VALUES ('r1', 'Flat', 'Street', 'flat'); \
         INSERT INTO real_estate_photo_batches (id, real_estate_id, photo_date) \
         VALUES ('batch-1', 'r1', 1767225600);",
    )
    .expect("owners");
}

/// A vault whose two devices both have their store.
async fn devices() -> Vault {
    let vault = vault().await;
    with_store(&vault.a);
    with_store(&vault.b);
    vault
}

/// `len` bytes of a pattern that never repeats a 64 KiB chunk.
fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len)
        .map(|i| ((i * 7 + seed as usize) % 251) as u8)
        .collect()
}

/// A new blob held on `device`; its ID.
fn held(device: &TestConn, bytes: &[u8]) -> String {
    attachment_store::put_bytes(&device.lock(), bytes)
        .expect("blob")
        .id
}

/// A document `id` of policy `p1` naming `blob`.
fn document(device: &TestConn, id: &str, blob: &str) {
    device
        .lock()
        .execute(
            "INSERT INTO insurance_documents (id, insurance_id, name, blob_id, uploaded_at) \
             VALUES (?1, 'p1', 'Contract', ?2, 1767225600)",
            [id, blob],
        )
        .expect("document");
}

/// A photo `id` of property `r1` naming `full` and `thumb`.
fn photo(device: &TestConn, id: &str, full: &str, thumb: &str) {
    device
        .lock()
        .execute(
            "INSERT INTO real_estate_photos (id, batch_id, blob_id, thumb_blob_id, created_at) \
             VALUES (?1, 'batch-1', ?2, ?3, 1767225600)",
            [id, full, thumb],
        )
        .expect("photo");
}

fn reconcile(device: &TestConn) {
    queue::reconcile(&device.lock(), NOW).expect("reconcile");
}

/// A held blob named by a new document on `device`, queued; its ID.
fn queued_document(device: &TestConn, id: &str, bytes: &[u8]) -> String {
    let blob = held(device, bytes);
    document(device, id, &blob);
    reconcile(device);
    blob
}

/// `(state, attempts, next_try_at, last_error)` of an entry.
fn entry(device: &TestConn, blob: &str) -> Option<(String, i64, i64, Option<String>)> {
    device
        .lock()
        .query_row(
            "SELECT state, attempts, next_try_at, last_error FROM sync_blobs WHERE blob_id = ?1",
            [blob],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()
}

fn state_of(device: &TestConn, blob: &str) -> Option<String> {
    entry(device, blob).map(|(state, ..)| state)
}

fn content(device: &TestConn, blob: &str) -> AttachmentContent {
    let conn = device.lock();
    ContentReader::new(&conn)
        .expect("reader")
        .of(Some(blob), true)
        .expect("content")
}

/// Whether the store has a row for `blob`, complete or not.
fn in_store(device: &TestConn, blob: &str) -> bool {
    device
        .lock()
        .query_row(
            "SELECT EXISTS (SELECT 1 FROM files.blobs WHERE id = ?1)",
            [blob],
            |r| r.get(0),
        )
        .expect("store")
}

fn sha256_of(device: &TestConn, blob: &str) -> String {
    device
        .lock()
        .query_row(
            "SELECT sha256 FROM files.blobs WHERE id = ?1",
            [blob],
            |r| r.get(0),
        )
        .expect("sha256")
}

fn uuid(blob: &str) -> Uuid {
    Uuid::parse_str(blob).expect("uuid")
}

fn vault_id(vault: &Vault) -> String {
    vault.creds.vault_id.to_b64()
}

fn blob_calls(relay: &FakeRelay) -> Vec<Route> {
    relay
        .calls()
        .into_iter()
        .filter(|route| BLOB_ROUTES.contains(route))
        .collect()
}

/// A complete blob `id` of `from`'s store copied into `to`'s under the same ID (a device that
/// held the content before, e.g. across a replace).
fn copy_blob(from: &TestConn, to: &TestConn, id: &str) {
    let bytes = attachment_store::read_all(&from.lock(), id).expect("read");
    let sha256 = hex::encode(Sha256::digest(&bytes));
    to.lock()
        .execute(
            "INSERT INTO files.blobs (id, data, size, sha256) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, bytes, bytes.len() as i64, sha256],
        )
        .expect("copy");
}

fn next_upload(device: &TestConn) -> Transfer {
    queue::next_upload(&device.lock(), i64::MAX, None)
        .expect("next")
        .expect("an upload is due")
}

fn next_download(device: &TestConn) -> Transfer {
    queue::next_download(&device.lock(), i64::MAX, true)
        .expect("next")
        .expect("a download is due")
}

async fn uploads(vault: &Vault, device: &TestConn, policy: &BlobPolicy) -> TransferReport {
    run_uploads(&vault.relay, device, &vault.creds, policy, None, None)
        .await
        .expect("uploads")
}

async fn downloads(vault: &Vault, device: &TestConn, policy: &BlobPolicy) -> TransferReport {
    run_downloads(&vault.relay, device, &vault.creds, policy, None)
        .await
        .expect("downloads")
}

#[tokio::test]
async fn an_upload_reaches_the_server_and_is_done() {
    let vault = devices().await;
    let blob = queued_document(&vault.a, "d1", &pattern(150_000, 1));
    assert_eq!(content(&vault.a, &blob), AttachmentContent::WaitingUpload);

    let report = uploads(&vault, &vault.a, &BlobPolicy::default()).await;

    assert_eq!(report.uploaded, 1);
    assert!(report.changed);
    assert_eq!(vault.relay.blob_ids(&vault_id(&vault)), vec![uuid(&blob)]);
    assert_eq!(state_of(&vault.a, &blob).as_deref(), Some("done"));
    assert_eq!(content(&vault.a, &blob), AttachmentContent::Here);
}

/// A device that holds a blob the server already has (another device uploaded it) confirms it
/// with a `HEAD` and sends no body.
#[tokio::test]
async fn a_blob_the_server_has_is_confirmed_without_a_put() {
    let vault = devices().await;
    let blob = queued_document(&vault.a, "d1", &pattern(90_000, 2));
    uploads(&vault, &vault.a, &BlobPolicy::default()).await;
    let puts = vault.relay.blob_puts();
    copy_blob(&vault.a, &vault.b, &blob);
    document(&vault.b, "d1", &blob);
    reconcile(&vault.b);
    assert_eq!(state_of(&vault.b, &blob).as_deref(), Some("up"));

    let report = uploads(&vault, &vault.b, &BlobPolicy::default()).await;

    assert_eq!(report.uploaded, 1);
    assert_eq!(vault.relay.blob_puts(), puts, "no body was sent");
    assert_eq!(
        vault.relay.calls().last(),
        Some(&Route::HeadBlob),
        "the server was asked"
    );
    assert_eq!(state_of(&vault.b, &blob).as_deref(), Some("done"));
}

#[tokio::test]
async fn a_download_stores_the_content_under_its_id() {
    let vault = devices().await;
    let bytes = pattern(200_000, 3);
    let blob = queued_document(&vault.a, "d1", &bytes);
    uploads(&vault, &vault.a, &BlobPolicy::default()).await;
    document(&vault.b, "d1", &blob);
    reconcile(&vault.b);
    assert_eq!(content(&vault.b, &blob), AttachmentContent::NotHere);

    let report = downloads(&vault, &vault.b, &BlobPolicy::default()).await;

    assert_eq!(report.stored, 1);
    assert!(report.changed);
    assert_eq!(
        attachment_store::read_all(&vault.b.lock(), &blob).expect("read"),
        bytes
    );
    assert_eq!(sha256_of(&vault.b, &blob), sha256_of(&vault.a, &blob));
    assert_eq!(state_of(&vault.b, &blob).as_deref(), Some("done"));
    assert_eq!(content(&vault.b, &blob), AttachmentContent::Here);
}

#[tokio::test]
async fn content_the_server_lacks_is_not_on_server() {
    let vault = devices().await;
    let blob = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60";
    document(&vault.b, "d1", blob);
    reconcile(&vault.b);
    let transfer = next_download(&vault.b);

    let outcome = download_one(&vault.relay, &vault.b, &vault.creds, &transfer, NOW).await;

    assert_eq!(outcome, Outcome::Failed(TransferError::NotOnServer));
    assert_eq!(content(&vault.b, blob), AttachmentContent::NotOnServer);
    let (state, attempts, next_try_at, _) = entry(&vault.b, blob).expect("entry");
    assert_eq!((state.as_str(), attempts), ("down", 1));
    assert_eq!(next_try_at, NOW + 300);
    assert!(!in_store(&vault.b, blob));
}

#[tokio::test]
async fn a_tampered_or_truncated_blob_is_not_stored_and_shows_failed() {
    let vault = devices().await;
    let tampered = queued_document(&vault.a, "d1", &pattern(150_000, 4));
    let truncated = queued_document(&vault.a, "d2", &pattern(150_000, 5));
    uploads(&vault, &vault.a, &BlobPolicy::default()).await;
    vault.relay.tamper_blob(&vault_id(&vault), uuid(&tampered));
    vault
        .relay
        .truncate_blob(&vault_id(&vault), uuid(&truncated), 70_000);
    document(&vault.b, "d1", &tampered);
    document(&vault.b, "d2", &truncated);
    reconcile(&vault.b);

    let report = downloads(&vault, &vault.b, &BlobPolicy::default()).await;

    assert_eq!((report.stored, report.failed), (0, 2));
    let now = chrono::Utc::now().timestamp();
    for blob in [&tampered, &truncated] {
        assert!(!in_store(&vault.b, blob), "nothing of it is stored");
        assert_eq!(content(&vault.b, blob), AttachmentContent::Failed);
        let (state, attempts, next_try_at, error) = entry(&vault.b, blob).expect("entry");
        assert_eq!(
            (state.as_str(), attempts, error.as_deref()),
            ("down", 1, Some("unreadable"))
        );
        assert!(next_try_at > now, "retried later");
    }
}

#[tokio::test]
async fn no_room_pauses_uploads_and_says_so() {
    let vault = devices().await;
    let first = queued_document(&vault.a, "d1", &pattern(10_000, 6));
    uploads(&vault, &vault.a, &BlobPolicy::default()).await;
    document(&vault.b, "d1", &first);
    reconcile(&vault.b);

    // The server's disk is full (507).
    vault.relay.set_no_room(true);
    let second = queued_document(&vault.a, "d2", &pattern(10_000, 7));
    let policy = BlobPolicy::default();
    let report = uploads(&vault, &vault.a, &policy).await;
    assert_eq!((report.uploaded, report.failed), (0, 1));
    assert_eq!(policy.problem(), Some(SyncAttachmentsProblem::NoRoom));
    assert!(policy.uploads_paused(chrono::Utc::now().timestamp()));
    let (state, _, _, error) = entry(&vault.a, &second).expect("entry");
    assert_eq!((state.as_str(), error.as_deref()), ("up", Some("noRoom")));

    // Paused: nothing is sent, even when the entry is due.
    vault
        .a
        .lock()
        .execute("UPDATE sync_blobs SET next_try_at = 0", [])
        .expect("due");
    let puts = vault.relay.blob_puts();
    uploads(&vault, &vault.a, &policy).await;
    assert_eq!(vault.relay.blob_puts(), puts);

    // Downloads still run.
    let report = downloads(&vault, &vault.b, &policy).await;
    assert_eq!(report.stored, 1);
    assert_eq!(content(&vault.b, &first), AttachmentContent::Here);

    // The vault's quota (413 quota_exceeded) is no room too.
    vault.relay.set_no_room(false);
    vault.relay.set_blob_quota(1);
    let policy = BlobPolicy::default();
    let report = uploads(&vault, &vault.a, &policy).await;
    assert_eq!((report.uploaded, report.failed), (0, 1));
    assert_eq!(policy.problem(), Some(SyncAttachmentsProblem::NoRoom));

    // Room again: the next upload clears the problem.
    vault.relay.set_blob_quota(u64::MAX);
    vault
        .a
        .lock()
        .execute("UPDATE sync_blobs SET next_try_at = 0", [])
        .expect("due");
    let policy = BlobPolicy::default();
    policy.note_no_room(0);
    let report = uploads(&vault, &vault.a, &policy).await;
    assert_eq!(report.uploaded, 1);
    assert_eq!(policy.problem(), None);
}

#[tokio::test]
async fn a_server_without_blobs_transfers_nothing() {
    let vault = devices().await;
    vault.relay.set_server_info(Some(ServerInfo {
        version: "1.0.0".into(),
        protocol: 2,
        mode: ServerMode::SelfHosted,
        blobs: false,
    }));
    let policy = BlobPolicy::default();

    // Nothing waits: no problem to show.
    uploads(&vault, &vault.a, &policy).await;
    assert_eq!(policy.problem(), None);

    let blob = queued_document(&vault.a, "d1", &pattern(10_000, 8));
    document(&vault.b, "d1", &blob);
    let up = uploads(&vault, &vault.a, &policy).await;
    let down = downloads(&vault, &vault.b, &policy).await;

    assert_eq!(
        (up.uploaded, up.failed, down.stored, down.failed),
        (0, 0, 0, 0)
    );
    assert!(
        blob_calls(&vault.relay).is_empty(),
        "{:?}",
        vault.relay.calls()
    );
    assert_eq!(
        policy.problem(),
        Some(SyncAttachmentsProblem::ServerNeedsUpdate)
    );
    let (_, attempts, ..) = entry(&vault.a, &blob).expect("entry");
    assert_eq!(attempts, 0, "nothing was tried");

    // The server was updated; once its answer is asked again, content moves.
    vault.relay.set_server_info(Some(ServerInfo {
        version: "1.1.0".into(),
        protocol: 2,
        mode: ServerMode::SelfHosted,
        blobs: true,
    }));
    policy.forget_server();
    let up = uploads(&vault, &vault.a, &policy).await;
    assert_eq!(up.uploaded, 1);
    assert_eq!(policy.problem(), None);
}

#[tokio::test]
async fn the_servers_answer_is_kept_for_six_hours() {
    let relay = FakeRelay::new();
    let policy = BlobPolicy::default();
    assert!(policy.server_has_blobs(&relay, NOW).await);
    relay.set_server_info(None);
    assert!(
        policy.server_has_blobs(&relay, NOW + 6 * 3_600 - 1).await,
        "cached"
    );
    assert!(
        !policy.server_has_blobs(&relay, NOW + 6 * 3_600).await,
        "asked again"
    );
    assert_eq!(
        policy.problem(),
        None,
        "a server that cannot be asked sets no problem"
    );
    let asked = relay
        .calls()
        .iter()
        .filter(|route| **route == Route::ServerInfo)
        .count();
    assert_eq!(asked, 2);
}

#[tokio::test(start_paused = true)]
async fn a_wake_reaches_each_lane_once_even_while_it_runs() {
    let policy = BlobPolicy::default();
    // Woken while neither lane waits: each lane's next wait returns at once, then waits again.
    policy.wake();
    let started = tokio::time::Instant::now();
    policy.wait(std::time::Duration::from_secs(60)).await;
    policy.wait(std::time::Duration::from_secs(60)).await;
    assert_eq!(started.elapsed(), std::time::Duration::ZERO);
    policy.wait(std::time::Duration::from_secs(60)).await;
    assert_eq!(started.elapsed(), std::time::Duration::from_secs(60));
}

#[tokio::test]
async fn a_row_deleted_during_a_transfer_drops_its_entry() {
    let vault = devices().await;
    // Before the upload starts.
    let blob = queued_document(&vault.a, "d1", &pattern(10_000, 9));
    let transfer = next_upload(&vault.a);
    vault
        .a
        .lock()
        .execute("DELETE FROM insurance_documents WHERE id = 'd1'", [])
        .expect("delete");

    let outcome = upload_one(&vault.relay, &vault.a, &vault.creds, &transfer, NOW).await;

    assert_eq!(outcome, Outcome::Gone);
    assert_eq!(entry(&vault.a, &blob), None);
    assert!(blob_calls(&vault.relay).is_empty());

    // Before the download starts.
    let theirs = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f61";
    document(&vault.b, "d2", theirs);
    reconcile(&vault.b);
    let transfer = next_download(&vault.b);
    vault
        .b
        .lock()
        .execute("DELETE FROM insurance_documents WHERE id = 'd2'", [])
        .expect("delete");
    let outcome = download_one(&vault.relay, &vault.b, &vault.creds, &transfer, NOW).await;
    assert_eq!(outcome, Outcome::Gone);
    assert_eq!(entry(&vault.b, theirs), None);
    assert!(blob_calls(&vault.relay).is_empty());
}

/// The person deletes the document (its row and its content, as the service does) while its
/// content is being read for the upload: the transfer ends quietly as gone, not as network
/// trouble, and nothing reaches the server.
#[tokio::test]
async fn content_deleted_during_its_upload_is_gone_not_offline() {
    let vault = devices().await;
    let blob = queued_document(&vault.a, "d1", &pattern(150_000, 10));
    let transfer = next_upload(&vault.a);
    let device = vault.a.clone();
    let delete = blob.clone();
    let relay = Meddling::new(vault.relay.clone()).during_put(move || {
        let conn = device.lock();
        conn.execute("DELETE FROM insurance_documents WHERE id = 'd1'", [])
            .expect("delete row");
        attachment_store::delete(&conn, &delete).expect("delete content");
    });

    let outcome = upload_one(&relay, &vault.a, &vault.creds, &transfer, NOW).await;

    assert_eq!(outcome, Outcome::Gone);
    assert_eq!(entry(&vault.a, &blob), None);
    assert!(vault.relay.blob_ids(&vault_id(&vault)).is_empty());
}

/// A local read error during an upload (the content no longer has the length the transfer
/// declared) is a local failure with its own backoff, never network trouble.
#[tokio::test]
async fn a_local_read_error_during_an_upload_is_local() {
    let vault = devices().await;
    let blob = queued_document(&vault.a, "d1", &pattern(10_000, 11));
    let mut transfer = next_upload(&vault.a);
    transfer.size = Some(9_999);

    let outcome = upload_one(&vault.relay, &vault.a, &vault.creds, &transfer, NOW).await;

    assert_eq!(outcome, Outcome::Failed(TransferError::Local));
    let (state, attempts, _, error) = entry(&vault.a, &blob).expect("entry");
    assert_eq!(
        (state.as_str(), attempts, error.as_deref()),
        ("up", 1, Some("local"))
    );
    assert!(vault.relay.blob_ids(&vault_id(&vault)).is_empty());
}

/// The row goes while its content downloads: removed with a sweep (its incoming blob goes too),
/// or alone (the stored blob would be named by nothing). Either way the transfer ends as gone,
/// the entry is dropped and no blob is left in the store.
#[tokio::test]
async fn a_row_deleted_during_its_download_leaves_no_blob() {
    let vault = devices().await;
    let swept = queued_document(&vault.a, "d1", &pattern(150_000, 12));
    let alone = queued_document(&vault.a, "d2", &pattern(150_000, 13));
    uploads(&vault, &vault.a, &BlobPolicy::default()).await;
    for (id, blob, sweep) in [("d1", &swept, true), ("d2", &alone, false)] {
        document(&vault.b, id, blob);
        reconcile(&vault.b);
        let transfer = next_download(&vault.b);
        assert_eq!(&transfer.blob_id, blob);
        let device = vault.b.clone();
        let row = id.to_string();
        let relay = Meddling::new(vault.relay.clone()).during_get(move || {
            let conn = device.lock();
            conn.execute("DELETE FROM insurance_documents WHERE id = ?1", [&row])
                .expect("delete row");
            if sweep {
                attachment_store::delete_unreferenced(&conn).expect("sweep");
            }
        });

        let outcome = download_one(&relay, &vault.b, &vault.creds, &transfer, NOW).await;

        assert_eq!(outcome, Outcome::Gone, "{id}");
        assert_eq!(entry(&vault.b, blob), None, "{id}");
        assert!(!in_store(&vault.b, blob), "{id}: no blob resurrected");
    }
}

/// A second download of a blob already being downloaded here touches nothing and notes nothing:
/// it is skipped for now.
#[tokio::test]
async fn a_blob_already_downloading_is_skipped() {
    let vault = devices().await;
    let blob = queued_document(&vault.a, "d1", &pattern(100_000, 14));
    uploads(&vault, &vault.a, &BlobPolicy::default()).await;
    document(&vault.b, "d1", &blob);
    reconcile(&vault.b);
    let transfer = next_download(&vault.b);
    let second: Arc<Mutex<Option<Outcome>>> = Arc::default();
    let (device, relay, creds, inner, seen) = (
        vault.b.clone(),
        vault.relay.clone(),
        vault.creds.clone(),
        transfer.clone(),
        Arc::clone(&second),
    );
    let meddling = Meddling::new(vault.relay.clone()).during_get(move || {
        let outcome = futures_util::FutureExt::now_or_never(download_one(
            &relay, &device, &creds, &inner, NOW,
        ))
        .expect("the fake answers at once");
        *seen.lock().expect("seen") = Some(outcome);
    });

    let outcome = download_one(&meddling, &vault.b, &vault.creds, &transfer, NOW).await;

    assert_eq!(*second.lock().expect("seen"), Some(Outcome::Busy));
    assert_eq!(outcome, Outcome::Done);
    assert_eq!(state_of(&vault.b, &blob).as_deref(), Some("done"));
}

#[tokio::test]
async fn an_invalid_blob_id_is_never_requested() {
    let vault = devices().await;
    let bad = "../../x";
    document(&vault.b, "d1", bad);
    reconcile(&vault.b);

    let report = downloads(&vault, &vault.b, &BlobPolicy::default()).await;
    assert_eq!(report.stored, 0);
    assert!(blob_calls(&vault.relay).is_empty());
    assert_eq!(content(&vault.b, bad), AttachmentContent::Failed);

    // Asked directly, either way: refused before any request or store call.
    let transfer = Transfer {
        blob_id: bad.into(),
        state: BlobState::Down,
        thumbnail: false,
        wanted: true,
        attempts: 0,
        size: Some(10),
    };
    for outcome in [
        download_one(&vault.relay, &vault.b, &vault.creds, &transfer, NOW).await,
        upload_one(&vault.relay, &vault.b, &vault.creds, &transfer, NOW).await,
    ] {
        assert_eq!(outcome, Outcome::Failed(TransferError::InvalidId));
    }
    assert!(blob_calls(&vault.relay).is_empty());
    assert!(!in_store(&vault.b, bad));
    assert_eq!(content(&vault.b, bad), AttachmentContent::Failed);
}

#[tokio::test]
async fn a_lapsed_account_still_downloads_but_does_not_upload() {
    let vault = devices().await;
    let first = queued_document(&vault.a, "d1", &pattern(10_000, 15));
    uploads(&vault, &vault.a, &BlobPolicy::default()).await;
    document(&vault.b, "d1", &first);
    reconcile(&vault.b);
    vault.relay.set_paid(&vault.creds.account, false);
    let second = queued_document(&vault.a, "d2", &pattern(10_000, 16));
    let policy = BlobPolicy::default();

    let up = uploads(&vault, &vault.a, &policy).await;
    let down = downloads(&vault, &vault.b, &policy).await;

    assert_eq!((up.uploaded, up.failed), (0, 0));
    let (state, attempts, next_try_at, error) = entry(&vault.a, &second).expect("entry");
    assert_eq!(
        (state.as_str(), attempts, next_try_at, error),
        ("up", 0, 0, None),
        "the round shows a lapsed account; the entry notes nothing"
    );
    assert_eq!(policy.problem(), None);
    assert_eq!(down.stored, 1);
    assert_eq!(content(&vault.b, &first), AttachmentContent::Here);
}

#[tokio::test]
async fn a_phone_fetches_only_thumbnails_until_asked() {
    let vault = devices().await;
    let full = held(&vault.a, &pattern(120_000, 17));
    let thumb = held(&vault.a, &pattern(5_000, 18));
    photo(&vault.a, "ph1", &full, &thumb);
    reconcile(&vault.a);
    uploads(&vault, &vault.a, &BlobPolicy::default()).await;
    photo(&vault.b, "ph1", &full, &thumb);
    reconcile(&vault.b);
    let phone = BlobPolicy::new(false);

    let report = downloads(&vault, &vault.b, &phone).await;

    assert_eq!(report.stored, 1);
    assert_eq!(content(&vault.b, &thumb), AttachmentContent::Here);
    assert_eq!(content(&vault.b, &full), AttachmentContent::NotHere);

    queue::want_row(&vault.b.lock(), AttachmentKind::Photo, "ph1").expect("want");
    let report = downloads(&vault, &vault.b, &phone).await;
    assert_eq!(report.stored, 1);
    assert_eq!(content(&vault.b, &full), AttachmentContent::Here);
}

#[tokio::test]
async fn a_locked_profile_ends_the_run() {
    let vault = devices().await;
    queued_document(&vault.a, "d1", &pattern(10_000, 19));
    vault
        .a
        .locked
        .store(true, std::sync::atomic::Ordering::SeqCst);

    let result = run_uploads(
        &vault.relay,
        &vault.a,
        &vault.creds,
        &BlobPolicy::default(),
        None,
        None,
    )
    .await;

    assert!(matches!(result, Err(AppError::Auth(_))), "{result:?}");
    assert!(blob_calls(&vault.relay).is_empty());
}

/// The lanes spawn the runs on the engine's runtime: they are `Send` and work on a
/// multi-threaded runtime, where the store work of a transfer leaves the async worker.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_runs_go_on_a_multi_threaded_runtime() {
    let vault = devices().await;
    let bytes = pattern(200_000, 20);
    let blob = queued_document(&vault.a, "d1", &bytes);
    document(&vault.b, "d1", &blob);
    let policy = BlobPolicy::default();
    let (relay, a, creds, lanes) = (
        vault.relay.clone(),
        vault.a.clone(),
        vault.creds.clone(),
        policy.clone(),
    );
    let up = tokio::spawn(async move { run_uploads(&relay, &a, &creds, &lanes, None, None).await })
        .await
        .expect("join")
        .expect("uploads");
    let (relay, b, creds) = (vault.relay.clone(), vault.b.clone(), vault.creds.clone());
    let down = tokio::spawn(async move { run_downloads(&relay, &b, &creds, &policy, None).await })
        .await
        .expect("join")
        .expect("downloads");

    assert_eq!((up.uploaded, down.stored), (1, 1));
    assert_eq!(
        attachment_store::read_all(&vault.b.lock(), &blob).expect("read"),
        bytes
    );
}

/// Blob answers take the transfer's own classes, never the round's: no blob error stops or
/// slows ordinary sync. Passing trouble is retried later per blob; no room pauses uploads only;
/// what the round already shows (payment, token, vault) stops the transfers without a note.
#[test]
fn every_blob_answer_has_its_transfer_class() {
    use RelayError as E;
    use TransferError as T;
    let table = [
        (E::Network("timeout".into()), Outcome::Failed(T::Network)),
        (E::Network("connect".into()), Outcome::Failed(T::Network)),
        (E::RateLimited, Outcome::Failed(T::Server)),
        (E::Server(500), Outcome::Failed(T::Server)),
        (E::Server(408), Outcome::Failed(T::Server)),
        (E::Protocol("length".into()), Outcome::Failed(T::Server)),
        (
            E::BadRequest("incomplete_body".into()),
            Outcome::Failed(T::Server),
        ),
        (
            E::BadRequest("length_required".into()),
            Outcome::Failed(T::Server),
        ),
        (E::BlobNotFound, Outcome::Failed(T::NotOnServer)),
        (E::QuotaExceeded, Outcome::Failed(T::NoRoom)),
        (E::InsufficientStorage, Outcome::Failed(T::NoRoom)),
        (
            E::BadRequest("invalid_blob_id".into()),
            Outcome::Failed(T::InvalidId),
        ),
        (E::PaymentRequired, Outcome::Blocked),
        (E::Unauthorized, Outcome::Blocked),
        (E::TokenReadOnly, Outcome::Blocked),
        (E::VaultNotFound, Outcome::Blocked),
    ];
    for (error, expected) in table {
        assert_eq!(classify(&error), expected, "{error:?}");
    }
}

/// The fake sync server with a hook that runs once in the middle of a blob transfer: after the
/// first piece of an upload's body was read, or after a download began.
struct Meddling {
    relay: FakeRelay,
    put: Mutex<Option<Hook>>,
    get: Mutex<Option<Hook>>,
}

type Hook = Box<dyn FnOnce() + Send>;

impl Meddling {
    fn new(relay: FakeRelay) -> Self {
        Self {
            relay,
            put: Mutex::new(None),
            get: Mutex::new(None),
        }
    }

    fn during_put(self, hook: impl FnOnce() + Send + 'static) -> Self {
        *self.put.lock().expect("hook") = Some(Box::new(hook));
        self
    }

    fn during_get(self, hook: impl FnOnce() + Send + 'static) -> Self {
        *self.get.lock().expect("hook") = Some(Box::new(hook));
        self
    }
}

/// An upload body that runs its hook once its first piece was read.
struct HookedBody {
    body: BlobBody,
    hook: Option<Hook>,
}

impl Iterator for HookedBody {
    type Item = std::io::Result<Vec<u8>>;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.body.next();
        if let Some(hook) = self.hook.take() {
            hook();
        }
        item
    }
}

/// A download sink that runs its hook once the download began.
struct HookedSink<'a> {
    sink: &'a mut (dyn BlobSink + Send),
    hook: Option<Hook>,
}

impl BlobSink for HookedSink<'_> {
    fn begin(&mut self, sealed_len: u64) -> SinkFlow {
        let flow = self.sink.begin(sealed_len);
        if let Some(hook) = self.hook.take() {
            hook();
        }
        flow
    }

    fn piece(&mut self, bytes: &[u8]) -> SinkFlow {
        self.sink.piece(bytes)
    }
}

impl Relay for Meddling {
    fn server_info(&self) -> impl Future<Output = RelayResult<ServerInfo>> + Send {
        self.relay.server_info()
    }
    fn create_account(&self) -> impl Future<Output = RelayResult<String>> + Send {
        self.relay.create_account()
    }
    fn account_status(
        &self,
        account: &str,
    ) -> impl Future<Output = RelayResult<AccountStatus>> + Send {
        self.relay.account_status(account)
    }
    fn checkout(
        &self,
        account: &str,
        plan: CheckoutPlan,
    ) -> impl Future<Output = RelayResult<String>> + Send {
        self.relay.checkout(account, plan)
    }
    fn create_vault(
        &self,
        account: &str,
        vault: &VaultAuth,
    ) -> impl Future<Output = RelayResult<bool>> + Send {
        self.relay.create_vault(account, vault)
    }
    fn vault_info(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<VaultInfo>> + Send {
        self.relay.vault_info(vault)
    }
    fn push(
        &self,
        vault: &VaultAuth,
        ops: &[OutgoingOp],
    ) -> impl Future<Output = RelayResult<PushReceipt>> + Send {
        self.relay.push(vault, ops)
    }
    fn pull(
        &self,
        vault: &VaultAuth,
        after: u64,
        limit: u32,
    ) -> impl Future<Output = RelayResult<PullPage>> + Send {
        self.relay.pull(vault, after, limit)
    }
    fn put_snapshot(
        &self,
        vault: &VaultAuth,
        seq: u64,
        bytes: Vec<u8>,
    ) -> impl Future<Output = RelayResult<()>> + Send {
        self.relay.put_snapshot(vault, seq, bytes)
    }
    fn get_snapshot(
        &self,
        vault: &VaultAuth,
    ) -> impl Future<Output = RelayResult<SnapshotBlob>> + Send {
        self.relay.get_snapshot(vault)
    }
    fn delete_vault(&self, vault: &VaultAuth) -> impl Future<Output = RelayResult<()>> + Send {
        self.relay.delete_vault(vault)
    }
    fn put_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sealed_len: u64,
        body: BlobBody,
    ) -> impl Future<Output = RelayResult<()>> + Send {
        let hook = self.put.lock().expect("hook").take();
        let body: BlobBody = Box::new(HookedBody { body, hook });
        self.relay.put_blob(vault, blob, sealed_len, body)
    }
    fn head_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
    ) -> impl Future<Output = RelayResult<Option<u64>>> + Send {
        self.relay.head_blob(vault, blob)
    }
    fn get_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
        sink: &mut (dyn BlobSink + Send),
    ) -> impl Future<Output = RelayResult<Delivery>> + Send {
        let hook = self.get.lock().expect("hook").take();
        async move {
            let mut hooked = HookedSink { sink, hook };
            self.relay.get_blob(vault, blob, &mut hooked).await
        }
    }
    fn list_blobs(
        &self,
        vault: &VaultAuth,
        after: Option<Uuid>,
        limit: u32,
    ) -> impl Future<Output = RelayResult<BlobPage>> + Send {
        self.relay.list_blobs(vault, after, limit)
    }
    fn delete_blob(
        &self,
        vault: &VaultAuth,
        blob: Uuid,
    ) -> impl Future<Output = RelayResult<()>> + Send {
        self.relay.delete_blob(vault, blob)
    }
}
