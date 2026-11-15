//! The transfer queue after a pull (ADR 0030): each merged changeset brings the queue in line with
//! the rows it touched, and nothing more: a pull never checks the queue against the store (that
//! walks every blob's pages). Content this store lost outside a merge is noticed by the full
//! reconcile at the next lane start (or replace, or full repair).

use super::join::round;
use super::*;
use crate::models::attachments::AttachmentContent;
use crate::services::attachment_store;
use crate::services::sync::blobs::content::ContentReader;
use crate::services::sync::blobs::policy::BlobPolicy;
use crate::services::sync::blobs::queue;
use crate::services::sync::engine::snapshots::SnapshotPolicy;
use crate::services::sync::engine::{run_round, RoundMode};

const THEIRS: &str = "0b9e6a52-3a4c-4e0f-9d7a-1c2b3d4e5f60";

fn policy(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO insurance_policies \
           (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
         VALUES (?1, 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly')",
        [id],
    )
    .expect("policy");
}

fn state_of(conn: &Connection, blob: &str) -> Option<String> {
    conn.query_row(
        "SELECT state FROM sync_blobs WHERE blob_id = ?1",
        [blob],
        |r| r.get(0),
    )
    .ok()
}

#[tokio::test]
async fn a_pull_reconciles_only_the_rows_it_merged() {
    let vault = vault().await;
    let lost = {
        let b = vault.b.lock();
        b.execute("ATTACH DATABASE ':memory:' AS files", [])
            .expect("attach");
        crate::db::files_schema::migrate(&b).expect("files schema");
        policy(&b, "p-b");
        let blob = attachment_store::put_bytes(&b, b"%PDF-1.7 mine")
            .expect("blob")
            .id;
        b.execute(
            "INSERT INTO insurance_documents (id, insurance_id, name, blob_id) \
             VALUES ('d-b', 'p-b', 'Mine', ?1)",
            [&blob],
        )
        .expect("document");
        queue::reconcile(&b, NOW).expect("reconcile");
        assert_eq!(state_of(&b, &blob).as_deref(), Some("up"));
        attachment_store::delete(&b, &blob).expect("the store loses it");
        blob
    };
    {
        let a = vault.a.lock();
        policy(&a, "p-a");
        a.execute(
            "INSERT INTO insurance_documents (id, insurance_id, name, blob_id) \
             VALUES ('d-a', 'p-a', 'Theirs', ?1)",
            [THEIRS],
        )
        .expect("document");
    }
    round(&vault, &vault.a, RoundMode::Normal).await;

    let report = round(&vault, &vault.b, RoundMode::Normal).await;

    assert!(report.pull.as_ref().is_some_and(|pull| pull.applied > 0));
    let b = vault.b.lock();
    assert_eq!(
        state_of(&b, THEIRS).as_deref(),
        Some("down"),
        "the merged row"
    );
    // The pull did not ask the store, so the entry still said `up`; the round's upload step took
    // it, found the content gone and dropped the entry (ADR 0030, known limits).
    assert_eq!(state_of(&b, &lost), None, "not moved to down by the pull");

    // What the lanes run when they start.
    queue::reconcile(&b, NOW).expect("full reconcile");
    assert_eq!(
        state_of(&b, &lost).as_deref(),
        Some("down"),
        "content lost here is fetched again"
    );
}

/// Whether `policy` has told the app of a change it has not been asked about yet.
fn told(policy: &BlobPolicy) -> bool {
    use futures_util::FutureExt;
    policy.changes().notified().now_or_never().is_some()
}

fn entry(conn: &Connection, blob: &str) -> (i64, i64, Option<String>) {
    conn.query_row(
        "SELECT attempts, next_try_at, last_error FROM sync_blobs WHERE blob_id = ?1",
        [blob],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )
    .expect("entry")
}

/// `A` adds a document with content of `len` bytes; returns the blob's ID.
fn document_on_a(vault: &Vault, id: &str, len: usize) -> String {
    let a = vault.a.lock();
    let blob = attachment_store::put_bytes(&a, &vec![7u8; len])
        .expect("blob")
        .id;
    a.execute(
        "INSERT INTO insurance_documents (id, insurance_id, name, blob_id) \
         VALUES (?1, 'p-a', 'Doc', ?2)",
        [id, &blob],
    )
    .expect("document");
    blob
}

/// Another device's changes arriving are the best sign its uploads are landing: a pull that
/// merged any makes every download waiting for content the server did not have due at once,
/// keeping its count of attempts (a blob still missing goes on with its backoff), and the lane
/// fetches it once the server has it. A pull that merged nothing changes nothing.
#[tokio::test]
async fn a_pull_that_merged_changes_makes_content_not_on_the_server_due() {
    let vault = vault().await;
    with_store(&vault.a);
    with_store(&vault.b);
    policy(&vault.a.lock(), "p-a");
    let blob = document_on_a(&vault, "d-a", 1_000);
    round(&vault, &vault.a, RoundMode::Normal).await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let later = NOW + 3_000;
    {
        let b = vault.b.lock();
        assert_eq!(state_of(&b, &blob).as_deref(), Some("down"));
        b.execute(
            "UPDATE sync_blobs SET attempts = 3, next_try_at = ?2, last_error = 'notOnServer', \
               last_error_at = ?3 WHERE blob_id = ?1",
            rusqlite::params![blob, later, NOW],
        )
        .expect("not on the server, three times");
    }

    // A pull that merged nothing.
    let pulled =
        crate::services::sync::engine::pull::pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
            .await
            .expect("pull");
    assert_eq!(pulled.applied, 0);
    assert_eq!(
        entry(&vault.b.lock(), &blob),
        (3, later, Some("notOnServer".into()))
    );

    // `A` changes something else; `B` merges it.
    policy(&vault.a.lock(), "p-a2");
    round(&vault, &vault.a, RoundMode::Normal).await;
    let pulled =
        crate::services::sync::engine::pull::pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
            .await
            .expect("pull");
    assert!(pulled.applied > 0);
    assert_eq!(
        entry(&vault.b.lock(), &blob),
        (3, 0, Some("notOnServer".into())),
        "due now, attempts kept"
    );

    // The server has it (A's round sent it before its push): the lane fetches it now.
    let fetched = crate::services::sync::blobs::transfer::run_downloads(
        &vault.relay,
        &vault.b,
        &vault.creds,
        &BlobPolicy::for_platform(false),
        NOW,
        None,
    )
    .await
    .expect("downloads");
    assert_eq!(fetched.stored, 1);
    assert_eq!(state_of(&vault.b.lock(), &blob).as_deref(), Some("done"));
}

/// While the sync server is out of reach, a document added here is queued and counted at the
/// start of the round, before any network call: the status and the row agree that it waits.
#[tokio::test]
async fn a_round_that_cannot_reach_the_server_still_counts_content_added_here() {
    let vault = vault().await;
    with_store(&vault.a);
    policy(&vault.a.lock(), "p-a");
    round(&vault, &vault.a, RoundMode::Normal).await;
    let blob = document_on_a(&vault, "d-a", 1_500);
    vault.relay.set_failing(
        crate::services::sync::relay::fake::Route::Pull,
        Some(crate::services::sync::relay::RelayError::Network(
            "offline".into(),
        )),
    );
    let mine = BlobPolicy::for_platform(false);
    let mut snapshots = SnapshotPolicy::default();
    snapshots.blobs = mine.clone();

    let result = run_round(
        &vault.relay,
        &vault.a,
        &mut snapshots,
        RoundMode::Normal,
        NOW,
        0,
    )
    .await;

    assert!(result.is_err(), "the round could not reach the server");
    let a = vault.a.lock();
    let summary = queue::summary(&a).expect("summary");
    assert_eq!(
        (summary.waiting_upload, summary.waiting_upload_bytes),
        (1, 1_500)
    );
    let content = ContentReader::new(&a)
        .expect("reader")
        .of(Some(&blob), true)
        .expect("content");
    assert_eq!(content, AttachmentContent::WaitingUpload);
    assert!(told(&mine), "the app was told the counts changed");
}

/// Content the round uploads before its push is told to the app like a lane's upload, so the
/// "waiting to sync" mark goes as soon as the server has it.
#[tokio::test]
async fn the_rounds_own_upload_is_told_to_the_app() {
    let vault = vault().await;
    with_store(&vault.a);
    policy(&vault.a.lock(), "p-a");
    round(&vault, &vault.a, RoundMode::Normal).await;
    let blob = document_on_a(&vault, "d-a", 2_000);
    let mine = BlobPolicy::for_platform(false);
    let mut snapshots = SnapshotPolicy::default();
    snapshots.blobs = mine.clone();
    // Queued already, so the round's own start finds nothing new to tell.
    queue::reconcile_rows(&vault.a.lock(), NOW).expect("queued");
    assert!(!told(&mine));

    run_round(
        &vault.relay,
        &vault.a,
        &mut snapshots,
        RoundMode::Normal,
        NOW,
        0,
    )
    .await
    .expect("round");

    assert_eq!(state_of(&vault.a.lock(), &blob).as_deref(), Some("done"));
    assert!(told(&mine), "the app was told");
}
