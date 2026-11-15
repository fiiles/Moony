//! A replace (join, repair, re-sync) holds the content of every blob this device's document and
//! photo rows named before it (ADR 0030, audit H3): no sweep removes it until the first pull after
//! the replace has caught up to the sync server's head, because a row the replace did not bring
//! back may come back with that pull. Only then do the blobs no row names go.

use super::join::{join, round, snapshot_of_a, XorNative};
use super::*;
use crate::services::attachment_store;
use crate::services::sync::binding::NoBinding;
use crate::services::sync::engine::pull::pull_all;
use crate::services::sync::engine::{PullEnd, RoundMode};
use crate::services::sync::join::resync_by_replace;
use crate::services::sync::relay::fake::Route;
use crate::services::sync::relay::RelayError;

fn policy(conn: &Connection, id: &str) {
    conn.execute(
        "INSERT INTO insurance_policies \
           (id, type, provider, policy_name, policy_number, start_date, payment_frequency) \
         VALUES (?1, 'life', 'Insurer', 'Life', '123', 1767225600, 'monthly')",
        [id],
    )
    .expect("policy");
}

fn with_store(conn: &Connection) {
    conn.execute("ATTACH DATABASE ':memory:' AS files", [])
        .expect("attach");
    crate::db::files_schema::migrate(conn).expect("files schema");
}

/// A document `id` under `policy` with a new blob; returns the blob's ID.
fn document(conn: &Connection, id: &str, policy: &str) -> String {
    let blob = attachment_store::put_bytes(conn, format!("%PDF-1.7 {id}").as_bytes())
        .expect("blob")
        .id;
    conn.execute(
        "INSERT INTO insurance_documents (id, insurance_id, name, blob_id) \
         VALUES (?1, ?2, 'Contract', ?3)",
        [id, policy, &blob],
    )
    .expect("document");
    blob
}

fn readable(conn: &Connection, blob: &str) -> bool {
    attachment_store::size_of(conn, blob)
        .expect("size")
        .is_some()
}

/// What the sweep after the next unlock does once the app restarts.
fn sweep_after_restart(conn: &Connection) {
    attachment_store::delete_unreferenced(conn).expect("sweep");
}

/// The re-sync's push fails, so the document never reaches the vault: its row is gone with the
/// replace, its content stays through an app restart and goes once the next pull caught up.
#[tokio::test]
async fn a_blob_whose_row_the_vault_never_gets_is_held_until_the_pull_catches_up() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    let blob = {
        let b = vault.b.lock();
        with_store(&b);
        policy(&b, "p-own");
        document(&b, "d-own", "p-own")
    };
    let consent = agreed(&vault.b);
    vault.relay.fail_next(Route::Push, RelayError::Server(503));
    let dir = tempfile::tempdir().expect("dir");

    resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &XorNative, NOW)
        .await
        .expect("resync");

    {
        let b = vault.b.lock();
        assert_eq!(count(&b, "SELECT COUNT(*) FROM insurance_documents"), 0);
        assert!(readable(&b, &blob), "held right after the replace");
        sweep_after_restart(&b);
        assert!(readable(&b, &blob), "held across a restart's sweep");
    }

    let report = round(&vault, &vault.b, RoundMode::Normal).await;

    assert!(report.pull.as_ref().is_some_and(|pull| pull.done()));
    let b = vault.b.lock();
    assert!(
        !readable(&b, &blob),
        "no row names it once the pull caught up"
    );
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_held_blobs"), 0);
}

/// The catch-up pull first applies another device's delete of an unrelated policy (whose repair
/// sweeps unreferenced blobs), then this device's own document: the held content survives the
/// first and is there for the second.
#[tokio::test]
async fn an_unrelated_delete_in_the_catch_up_pull_keeps_a_held_blob() {
    let vault = vault().await;
    {
        let a = vault.a.lock();
        policy(&a, "p1");
        policy(&a, "p2");
    }
    snapshot_of_a(&vault, "a1").await;
    round(&vault, &vault.b, RoundMode::Normal).await;
    exec(
        &vault.a.lock(),
        "DELETE FROM insurance_policies WHERE id = 'p2'",
    );
    round(&vault, &vault.a, RoundMode::Normal).await;
    let blob = {
        let b = vault.b.lock();
        with_store(&b);
        document(&b, "d1", "p1")
    };
    let consent = agreed(&vault.b);
    let dir = tempfile::tempdir().expect("dir");

    resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &XorNative, NOW)
        .await
        .expect("resync");
    assert!(readable(&vault.b.lock(), &blob));
    round(&vault, &vault.b, RoundMode::Normal).await;

    let b = vault.b.lock();
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM insurance_policies WHERE id = 'p2'"
        ),
        0,
        "the delete was applied"
    );
    assert_eq!(
        count(
            &b,
            "SELECT COUNT(*) FROM insurance_documents WHERE id = 'd1'"
        ),
        1
    );
    assert_eq!(
        attachment_store::read_all(&b, &blob).expect("blob"),
        b"%PDF-1.7 d1".to_vec()
    );
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_held_blobs"), 0);
}

/// A new device joins: its own documents are not in the vault and go with the replace, but their
/// content is held until its first pull caught up, as on a re-sync.
#[tokio::test]
async fn a_join_holds_the_new_devices_own_content_until_the_pull_catches_up() {
    let vault = vault().await;
    snapshot_of_a(&vault, "a1").await;
    let joiner = TestConn::new(migrated());
    let blob = {
        let c = joiner.lock();
        with_store(&c);
        policy(&c, "own");
        document(&c, "d-own", "own")
    };
    let dir = tempfile::tempdir().expect("dir");

    join(&vault, &joiner, dir.path(), &NoBinding)
        .await
        .expect("join");

    {
        let c = joiner.lock();
        assert_eq!(count(&c, "SELECT COUNT(*) FROM insurance_documents"), 0);
        assert!(readable(&c, &blob), "held right after the join");
        sweep_after_restart(&c);
        assert!(readable(&c, &blob), "held across a restart's sweep");
    }

    round(&vault, &joiner, RoundMode::Normal).await;

    let c = joiner.lock();
    assert!(!readable(&c, &blob));
    assert_eq!(count(&c, "SELECT COUNT(*) FROM sync_held_blobs"), 0);
}

/// `B` re-syncs by replace with a failed push, so its document never reaches the vault and its
/// content is held; returns the blob's ID.
async fn held_after_a_failed_resync(vault: &Vault) -> String {
    snapshot_of_a(vault, "a1").await;
    round(vault, &vault.b, RoundMode::Normal).await;
    let blob = {
        let b = vault.b.lock();
        with_store(&b);
        policy(&b, "p-own");
        document(&b, "d-own", "p-own")
    };
    let consent = agreed(&vault.b);
    vault.relay.fail_next(Route::Push, RelayError::Server(503));
    let dir = tempfile::tempdir().expect("dir");
    resync_by_replace(&vault.relay, &vault.b, consent, dir.path(), &XorNative, NOW)
        .await
        .expect("resync");
    blob
}

/// `A` writes `id` and pushes it; returns the envelope's sequence.
async fn a_pushes(vault: &Vault, id: &str) -> u64 {
    institution(&vault.a.lock(), id);
    round(vault, &vault.a, RoundMode::Normal).await;
    vault.relay.ops().last().expect("an op").seq
}

/// The sync server withholds its newest envelope: the pull ends done but behind the server's head,
/// so a row the replace lacked may still be in what it withholds, and the content stays held.
#[tokio::test]
async fn a_pull_that_is_done_but_behind_keeps_the_holds() {
    let vault = vault().await;
    let blob = held_after_a_failed_resync(&vault).await;
    let seq = a_pushes(&vault, "a2").await;
    vault.relay.drop_op(seq);
    // What the round's own pull meets (a pull alone releases nothing); the round's report carries
    // its echo pull's ending instead.
    let pull = pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect("pull");
    assert!(pull.done() && pull.behind, "{pull:?}");

    round(&vault, &vault.b, RoundMode::Normal).await;

    let b = vault.b.lock();
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_held_blobs"), 1);
    sweep_after_restart(&b);
    assert!(readable(&b, &blob), "held while the pull is behind");
}

/// A pull held by a gap (an earlier envelope of `A` is missing) has not caught up: the content
/// stays held.
#[tokio::test]
async fn a_pull_held_by_a_gap_keeps_the_holds() {
    let vault = vault().await;
    let blob = held_after_a_failed_resync(&vault).await;
    let first = a_pushes(&vault, "a2").await;
    a_pushes(&vault, "a3").await;
    vault.relay.forget_op(first);
    let pull = pull_all(&vault.relay, &vault.b, &vault.creds, NOW)
        .await
        .expect("pull");
    assert_eq!(pull.end(), PullEnd::Waiting, "{pull:?}");

    round(&vault, &vault.b, RoundMode::Normal).await;

    let b = vault.b.lock();
    assert_eq!(count(&b, "SELECT COUNT(*) FROM sync_held_blobs"), 1);
    sweep_after_restart(&b);
    assert!(readable(&b, &blob), "held while the pull waits for a gap");
}
