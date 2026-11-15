//! The transfer queue after a pull (ADR 0030): each merged changeset brings the queue in line with
//! the rows it touched, and a pull that merged anything reconciles the whole queue once at its
//! end, so content this store lost meanwhile is queued to be fetched again.

use super::join::round;
use super::*;
use crate::services::attachment_store;
use crate::services::sync::blobs::queue;
use crate::services::sync::engine::RoundMode;

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
async fn a_pull_that_merged_changes_reconciles_the_whole_queue() {
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
    assert_eq!(
        state_of(&b, &lost).as_deref(),
        Some("down"),
        "content lost here is fetched again"
    );
}
